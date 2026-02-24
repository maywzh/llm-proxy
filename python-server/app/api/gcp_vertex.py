"""GCP Vertex AI API endpoints.

This module provides GCP Vertex AI Anthropic API compatibility by proxying
requests to the configured provider in Anthropic native format.

URL format:
    /models/gcp-vertex/v1/projects/{project}/locations/{location}/publishers/{publisher}/models/{model}:{action}

Actions:
    - rawPredict: Non-streaming request
    - streamRawPredict: Streaming request

Request/Response format: Anthropic native format
"""

from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Any, Awaitable, Callable, Optional
import json
import re
import time
import uuid

import httpx
from fastapi import APIRouter, Request, Depends, HTTPException
from fastapi.responses import JSONResponse
from starlette.background import BackgroundTask
from starlette.requests import ClientDisconnect
from starlette.responses import StreamingResponse

from app.api.dependencies import verify_auth, get_provider_svc
from app.services.provider_service import ProviderService
from app.models.provider import GcpVertexConfig
from app.services.langfuse_service import (
    get_langfuse_service,
    GenerationData,
    extract_client_metadata,
    build_langfuse_tags,
    LangfuseService,
)
from app.core.http_client import get_http_client
from app.core.header_policy import sanitize_anthropic_beta_header
from app.utils.client import extract_client
from app.core.jsonl_logger import (
    log_request,
    log_provider_request,
    log_provider_response,
    log_provider_streaming_response,
)
from app.core.metrics import CLIENT_DISCONNECTS
from app.core.stream_metrics import (
    StreamStats,
    StreamingUsageTracker,
    record_stream_metrics,
)
from app.core.logging import (
    get_logger,
    set_provider_context,
    clear_provider_context,
    get_api_key_name,
)
from app.utils.streaming import build_messages_for_token_count, calculate_message_tokens
from app.transformer.rectifier import sanitize_provider_payload

router = APIRouter(prefix="/models/gcp-vertex/v1", tags=["gcp-vertex"])
logger = get_logger()


# =============================================================================
# URL Parsing
# =============================================================================


@dataclass
class VertexPathInfo:
    """Parsed Vertex AI path information."""

    project: str
    location: str
    publisher: str
    model: str
    action: str  # rawPredict or streamRawPredict


def parse_vertex_path(path: str) -> VertexPathInfo:
    """Parse Vertex AI path to extract resource information.

    Expected format: projects/{project}/locations/{location}/publishers/{publisher}/models/{model}:{action}

    Args:
        path: The path portion after /models/gcp-vertex/v1/

    Returns:
        VertexPathInfo with parsed components

    Raises:
        HTTPException: If path format is invalid
    """
    pattern = (
        r"^projects/([^/]+)/locations/([^/]+)/publishers/([^/]+)/models/([^:]+):(\w+)$"
    )
    match = re.match(pattern, path)

    if not match:
        raise HTTPException(
            status_code=400,
            detail="Invalid Vertex AI path format. Expected: projects/{project}/locations/{location}/publishers/{publisher}/models/{model}:{action}",
        )

    project, location, publisher, model, action = match.groups()

    if action not in ("rawPredict", "streamRawPredict"):
        raise HTTPException(
            status_code=400,
            detail=f"Invalid action '{action}'. Expected 'rawPredict' or 'streamRawPredict'",
        )

    return VertexPathInfo(
        project=project,
        location=location,
        publisher=publisher,
        model=model,
        action=action,
    )


# =============================================================================
# Helper Functions
# =============================================================================


def _build_vertex_error_response(message: str, error_type: str = "api_error") -> dict:
    """Build a Vertex AI (Anthropic) formatted error response."""
    return {
        "type": "error",
        "error": {
            "type": error_type,
            "message": message,
        },
    }


def _extract_model_from_request(data: dict[str, Any], path_info: VertexPathInfo) -> str:
    """Extract the effective model name.

    Uses model from request body if present, otherwise uses path model.
    """
    return data.get("model", path_info.model)


def _build_provider_url(
    provider_api_base: str,
    project: str,
    location: str,
    publisher: str,
    model: str,
    is_streaming: bool,
) -> str:
    """Build the GCP Vertex AI URL.

    Format: {api_base}/v1/projects/{project}/locations/{location}/publishers/{publisher}/models/{model}:{action}
    """
    action = "streamRawPredict" if is_streaming else "rawPredict"
    return f"{provider_api_base}/v1/projects/{project}/locations/{location}/publishers/{publisher}/models/{model}:{action}"


def _build_anthropic_headers(
    provider_api_key: str,
    request_headers: dict[str, str],
    provider_type: str,
    provider_params: dict[str, Any],
) -> dict[str, str]:
    """Build headers for Anthropic provider.

    GCP Vertex AI uses Bearer token authentication.
    """
    headers = {
        "Content-Type": "application/json",
        "anthropic-version": request_headers.get(
            "anthropic-version", "vertex-2023-10-16"
        ),
        "Authorization": f"Bearer {provider_api_key}",
    }

    anthropic_beta = sanitize_anthropic_beta_header(
        provider_type,
        provider_params,
        request_headers.get("anthropic-beta"),
    )
    if anthropic_beta:
        headers["anthropic-beta"] = anthropic_beta

    # Apply custom headers from provider_params
    custom_headers = (provider_params or {}).get("custom_headers")
    if isinstance(custom_headers, dict):
        headers.update(custom_headers)

    return headers


def _calculate_input_tokens(data: dict[str, Any], model: str) -> int:
    """Pre-calculate input tokens for usage fallback."""
    messages = data.get("messages", [])
    system = data.get("system")
    tools = data.get("tools")

    combined_messages = build_messages_for_token_count(messages, system)
    return calculate_message_tokens(
        combined_messages,
        model,
        tools=tools,
        tool_choice=data.get("tool_choice"),
    )


# =============================================================================
# Langfuse Integration
# =============================================================================


def _init_langfuse_trace(
    request: Request,
    endpoint: str,
    langfuse_service: LangfuseService,
) -> tuple[str, str, Optional[str], GenerationData]:
    """Initialize Langfuse tracing for a Vertex AI request."""
    request_id = getattr(request.state, "request_id", str(uuid.uuid4()))
    credential_name = getattr(request.state, "credential_name", "anonymous")

    client_metadata = extract_client_metadata(request)
    user_agent = client_metadata.get("user_agent")
    tags = build_langfuse_tags(endpoint, credential_name, user_agent)

    trace_id = langfuse_service.create_trace(
        request_id=request_id,
        credential_name=credential_name,
        endpoint=endpoint,
        tags=tags,
        client_metadata=client_metadata,
    )

    generation_data = GenerationData(
        trace_id=trace_id or "",
        name="gcp-vertex-anthropic",
        request_id=request_id,
        credential_name=credential_name,
        endpoint=endpoint,
        start_time=datetime.now(timezone.utc),
    )

    return request_id, credential_name, trace_id, generation_data


# =============================================================================
# Request Parsing
# =============================================================================


async def _parse_request_body(
    request: Request,
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
) -> dict[str, Any]:
    """Parse and validate the request body."""
    try:
        data = await request.json()
    except ClientDisconnect:
        logger.info("Client disconnected before request body was read")
        generation_data.is_error = True
        generation_data.error_message = "Client closed request"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise ClientDisconnect()
    except json.JSONDecodeError as e:
        logger.warning(f"Invalid JSON in request body: {str(e)}")
        generation_data.is_error = True
        generation_data.error_message = f"Invalid JSON: {str(e)}"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=422, detail=f"Invalid JSON: {str(e)}")

    return data


# =============================================================================
# Provider Selection
# =============================================================================


def _select_provider(
    provider_svc: ProviderService,
    effective_model: str,
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
):
    """Select a provider for the model."""
    try:
        provider = provider_svc.get_next_provider(model=effective_model)
    except ValueError as e:
        logger.error(f"Provider selection failed: {str(e)}")
        generation_data.is_error = True
        generation_data.error_message = str(e)
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=400, detail=str(e))

    generation_data.provider_key = provider.name
    generation_data.provider_api_base = provider.api_base

    if trace_id:
        langfuse_service.update_trace_provider(
            trace_id=trace_id,
            provider_key=provider.name,
            provider_api_base=provider.api_base,
            model=effective_model,
        )

    return provider


# =============================================================================
# Streaming Response Handler
# =============================================================================


async def _cleanup_stream_context(
    stream_ctx,
    request_id: str,
    provider_name: str,
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    chunk_collector: list[str],
    usage_tracker: Optional[StreamingUsageTracker] = None,
    model: str = "unknown",
    client: str = "unknown",
):
    """Cleanup streaming context and finalize logging."""
    await stream_ctx.__aexit__(None, None, None)
    generation_data.end_time = datetime.now(timezone.utc)
    if trace_id:
        langfuse_service.trace_generation(generation_data)

    # Log streaming provider response to JSONL
    log_provider_streaming_response(
        request_id=request_id,
        provider=provider_name,
        status_code=200,
        error_msg=None,
        chunk_sequence=chunk_collector,
    )

    # Record token usage metrics at exit point
    if usage_tracker is not None:
        stats = StreamStats(
            model=model,
            provider=provider_name,
            api_key_name=get_api_key_name(),
            client=client,
            input_tokens=usage_tracker.input_tokens,
            output_tokens=usage_tracker.output_tokens,
            start_time=usage_tracker.start_time,
            first_token_time=usage_tracker.first_token_time,
        )
        record_stream_metrics(stats)


async def _create_anthropic_stream(
    response: httpx.Response,
    generation_data: GenerationData,
    provider_name: str,
    model: str,
    input_tokens: int,
    chunk_collector: list[str],
    usage_tracker: StreamingUsageTracker,
    disconnect_check: Optional[Callable[[], Awaitable[bool]]] = None,
):
    """Create an Anthropic streaming response generator (passthrough mode)."""
    usage_tracker.input_tokens = input_tokens

    async for line in response.aiter_lines():
        # Check for client disconnect
        if line.strip() and disconnect_check is not None:
            try:
                if await disconnect_check():
                    logger.debug(
                        f"Client disconnected, stopping stream from provider {provider_name} "
                        f"model={model}"
                    )
                    CLIENT_DISCONNECTS.labels(
                        model=model,
                        provider=provider_name,
                    ).inc()
                    break
            except Exception as e:
                logger.debug(f"Error checking client disconnect: {e}")

        # Track first token time
        if usage_tracker.first_token_time is None:
            usage_tracker.first_token_time = time.time()

        # Collect for logging
        chunk_str = line + "\n"
        chunk_collector.append(chunk_str)

        # Parse usage from message_delta events
        if line.startswith("data: "):
            try:
                data = json.loads(line[6:])
                if data.get("type") == "message_delta":
                    usage = data.get("usage", {})
                    if usage.get("output_tokens"):
                        usage_tracker.output_tokens = usage["output_tokens"]
                elif data.get("type") == "message_start":
                    message = data.get("message", {})
                    usage = message.get("usage", {})
                    if usage.get("input_tokens"):
                        usage_tracker.input_tokens = usage["input_tokens"]
            except json.JSONDecodeError:
                pass

        yield chunk_str.encode("utf-8")


async def _handle_streaming_request(
    request: Request,
    provider,
    data: dict[str, Any],
    effective_model: str,
    mapped_model: str,
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    request_id: str,
    client: str,
    is_streaming: bool = True,
):
    """Handle a streaming Vertex AI request."""
    logger.debug(
        f"Streaming Vertex AI request to {provider.name} for model {effective_model}"
    )

    # Pre-calculate input tokens
    input_tokens = _calculate_input_tokens(data, effective_model)
    logger.debug(f"Pre-calculated input tokens: {input_tokens}")

    # Build headers for Anthropic provider
    headers = _build_anthropic_headers(
        provider.api_key,
        dict(request.headers),
        provider.provider_type,
        provider.provider_params,
    )

    # Prepare request payload - use mapped model
    provider_payload = {**data, "model": mapped_model}

    # Sanitize payload before forwarding to provider.
    sanitize_provider_payload(provider_payload)

    # Ensure stream is true
    provider_payload["stream"] = True

    # Build GCP Vertex AI URL using provider's GCP parameters
    gcp_config = GcpVertexConfig.from_provider(provider)
    if gcp_config is None:
        gcp_config = GcpVertexConfig.from_provider_with_defaults(provider)

    url = _build_provider_url(
        provider.api_base,
        gcp_config.project,
        gcp_config.location,
        gcp_config.publisher,
        mapped_model,
        is_streaming,
    )

    # Log provider request
    log_provider_request(
        request_id=request_id,
        provider=provider.name,
        api_base=provider.api_base,
        endpoint="/v1/messages",
        payload=provider_payload,
    )

    # Use shared HTTP client for streaming
    http_client = get_http_client()
    stream_ctx = http_client.stream("POST", url, json=provider_payload, headers=headers)

    try:
        response = await stream_ctx.__aenter__()
    except Exception as e:
        await stream_ctx.__aexit__(None, None, None)
        logger.error(f"Failed to connect to provider {provider.name}: {e}")
        raise HTTPException(status_code=502, detail=f"Provider connection failed: {e}")

    try:
        if response.status_code >= 400:
            error_body = b""
            async for chunk in response.aiter_bytes():
                error_body += chunk
            await stream_ctx.__aexit__(None, None, None)

            try:
                error_json = json.loads(error_body.decode("utf-8"))
            except Exception:
                error_json = {
                    "error": {"message": error_body.decode("utf-8", errors="replace")}
                }

            logger.error(
                f"Backend API returned error status {response.status_code} "
                f"from provider {provider.name}"
            )

            generation_data.is_error = True
            error_field = error_json.get("error", {})
            if isinstance(error_field, str):
                generation_data.error_message = error_field
            elif isinstance(error_field, dict):
                generation_data.error_message = (
                    error_field.get("message", "") or f"HTTP {response.status_code}"
                )
            else:
                generation_data.error_message = f"HTTP {response.status_code}"
            generation_data.end_time = datetime.now(timezone.utc)
            if trace_id:
                langfuse_service.trace_generation(generation_data)

            log_provider_response(
                request_id=request_id,
                provider=provider.name,
                status_code=response.status_code,
                error_msg=generation_data.error_message,
                body=error_json,
            )

            return JSONResponse(
                content=_build_vertex_error_response(
                    generation_data.error_message, "api_error"
                ),
                status_code=response.status_code,
            )

        # Create usage tracker and chunk collector
        chunk_collector: list[str] = []
        usage_tracker = StreamingUsageTracker(input_tokens=input_tokens)

        async def stream_generator():
            async for chunk in _create_anthropic_stream(
                response,
                generation_data,
                provider.name,
                effective_model,
                input_tokens,
                chunk_collector,
                usage_tracker,
                disconnect_check=request.is_disconnected,
            ):
                yield chunk

        streaming_response = StreamingResponse(
            stream_generator(),
            media_type="text/event-stream",
            headers={
                "Cache-Control": "no-cache",
                "Connection": "keep-alive",
            },
        )

        streaming_response.background = BackgroundTask(
            _cleanup_stream_context,
            stream_ctx,
            request_id,
            provider.name,
            generation_data,
            trace_id,
            langfuse_service,
            chunk_collector,
            usage_tracker,
            effective_model,
            client,
        )

        return streaming_response

    except Exception:
        await stream_ctx.__aexit__(None, None, None)
        raise


# =============================================================================
# Non-Streaming Response Handler
# =============================================================================


async def _handle_non_streaming_request(
    request: Request,
    provider,
    data: dict[str, Any],
    effective_model: str,
    mapped_model: str,
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    request_id: str,
    client: str,
    is_streaming: bool = False,
):
    """Handle a non-streaming Vertex AI request."""
    logger.debug(
        f"Non-streaming Vertex AI request to {provider.name} for model {effective_model}"
    )

    # Pre-calculate input tokens
    input_tokens = _calculate_input_tokens(data, effective_model)
    logger.debug(f"Pre-calculated input tokens: {input_tokens}")

    # Build headers for Anthropic provider
    headers = _build_anthropic_headers(
        provider.api_key,
        dict(request.headers),
        provider.provider_type,
        provider.provider_params,
    )

    # Prepare request payload - use mapped model
    provider_payload = {**data, "model": mapped_model}

    # Sanitize payload before forwarding to provider.
    sanitize_provider_payload(provider_payload)

    # Ensure stream is false
    provider_payload["stream"] = False

    # Build GCP Vertex AI URL using provider's GCP parameters
    gcp_config = GcpVertexConfig.from_provider(provider)
    if gcp_config is None:
        gcp_config = GcpVertexConfig.from_provider_with_defaults(provider)

    url = _build_provider_url(
        provider.api_base,
        gcp_config.project,
        gcp_config.location,
        gcp_config.publisher,
        mapped_model,
        is_streaming,
    )

    # Log provider request
    log_provider_request(
        request_id=request_id,
        provider=provider.name,
        api_base=provider.api_base,
        endpoint="/v1/messages",
        payload=provider_payload,
    )

    # Use shared HTTP client
    http_client = get_http_client()
    response = await http_client.post(url, json=provider_payload, headers=headers)

    if response.status_code >= 400:
        try:
            error_body = response.json()
        except Exception:
            error_body = {
                "error": {"message": response.text or f"HTTP {response.status_code}"}
            }

        logger.error(
            f"Backend API returned error status {response.status_code} "
            f"from provider {provider.name}"
        )

        generation_data.is_error = True
        error_field = error_body.get("error", {})
        if isinstance(error_field, str):
            generation_data.error_message = error_field
        elif isinstance(error_field, dict):
            generation_data.error_message = error_field.get("message", "")
        else:
            generation_data.error_message = f"HTTP {response.status_code}"

        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)

        log_provider_response(
            request_id=request_id,
            provider=provider.name,
            status_code=response.status_code,
            error_msg=generation_data.error_message,
            body=error_body,
        )

        return JSONResponse(
            content=_build_vertex_error_response(
                generation_data.error_message, "api_error"
            ),
            status_code=response.status_code,
        )

    response_data = response.json()

    # Log provider response
    log_provider_response(
        request_id=request_id,
        provider=provider.name,
        status_code=response.status_code,
        error_msg=None,
        body=response_data,
    )

    # Record successful generation
    generation_data.end_time = datetime.now(timezone.utc)

    # Capture usage for metrics
    usage = response_data.get("usage", {})
    if usage:
        generation_data.prompt_tokens = usage.get("input_tokens", 0)
        generation_data.completion_tokens = usage.get("output_tokens", 0)
        generation_data.total_tokens = (
            generation_data.prompt_tokens + generation_data.completion_tokens
        )
    else:
        generation_data.prompt_tokens = input_tokens
        generation_data.completion_tokens = 0
        generation_data.total_tokens = input_tokens

    if trace_id:
        langfuse_service.trace_generation(generation_data)

    return JSONResponse(content=response_data)


# =============================================================================
# Main Endpoint
# =============================================================================


@router.post(
    "/projects/{project}/locations/{location}/publishers/{publisher}/models/{model_and_action:path}"
)
async def vertex_ai_proxy(
    request: Request,
    project: str,
    location: str,
    publisher: str,
    model_and_action: str,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """GCP Vertex AI Anthropic API endpoint.

    Proxies requests in Anthropic native format to the configured provider.

    URL format:
        /models/gcp-vertex/v1/projects/{project}/locations/{location}/publishers/{publisher}/models/{model}:{action}

    Actions:
        - rawPredict: Non-streaming request
        - streamRawPredict: Streaming request
    """
    # Parse the model and action from the path
    full_path = f"projects/{project}/locations/{location}/publishers/{publisher}/models/{model_and_action}"
    path_info = parse_vertex_path(full_path)

    # Initialize Langfuse tracing
    langfuse_service = get_langfuse_service()
    endpoint = f"/models/gcp-vertex/v1/{full_path}"
    request_id, credential_name, trace_id, generation_data = _init_langfuse_trace(
        request, endpoint, langfuse_service
    )

    # Extract client info
    client = extract_client(request)

    # Parse request body
    try:
        data = await _parse_request_body(
            request, generation_data, trace_id, langfuse_service
        )
    except ClientDisconnect:
        return JSONResponse(
            content=_build_vertex_error_response(
                "Client closed request", "client_closed_request"
            ),
            status_code=408,
        )

    # Determine streaming from action
    is_streaming = path_info.action == "streamRawPredict"

    # Also check request body for stream flag
    if "stream" in data:
        is_streaming = data.get("stream", False) or is_streaming

    # Extract effective model
    effective_model = _extract_model_from_request(data, path_info)

    # Update generation data
    generation_data.original_model = effective_model
    generation_data.is_streaming = is_streaming

    # Select provider
    provider = _select_provider(
        provider_svc,
        effective_model,
        generation_data,
        trace_id,
        langfuse_service,
    )

    # Get mapped model from provider
    mapped_model = provider.get_mapped_model(effective_model)

    generation_data.mapped_model = mapped_model

    # Store in request state for metrics
    request.state.model = effective_model
    request.state.provider = provider.name

    # Log client request
    log_request(
        request_id=request_id,
        endpoint=endpoint,
        provider=provider.name,
        payload=data,
    )

    try:
        set_provider_context(provider.name)

        if is_streaming:
            return await _handle_streaming_request(
                request,
                provider,
                data,
                effective_model,
                mapped_model,
                generation_data,
                trace_id,
                langfuse_service,
                request_id,
                client,
            )
        else:
            return await _handle_non_streaming_request(
                request,
                provider,
                data,
                effective_model,
                mapped_model,
                generation_data,
                trace_id,
                langfuse_service,
                request_id,
                client,
            )

    except HTTPException:
        raise
    except httpx.TimeoutException:
        logger.error(f"Timeout error for provider {provider.name}")
        generation_data.is_error = True
        generation_data.error_message = "Gateway timeout"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        return JSONResponse(
            content=_build_vertex_error_response("Gateway timeout", "timeout_error"),
            status_code=504,
        )
    except httpx.RequestError as e:
        logger.error(f"Network request error for provider {provider.name}: {e}")
        generation_data.is_error = True
        generation_data.error_message = f"Provider {provider.name} network error"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        return JSONResponse(
            content=_build_vertex_error_response(
                f"Provider {provider.name} network error", "api_error"
            ),
            status_code=502,
        )
    except Exception:
        logger.exception(f"Unexpected error for provider {provider.name}")
        generation_data.is_error = True
        generation_data.error_message = "Internal server error"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        return JSONResponse(
            content=_build_vertex_error_response("Internal server error", "api_error"),
            status_code=500,
        )
    finally:
        clear_provider_context()
