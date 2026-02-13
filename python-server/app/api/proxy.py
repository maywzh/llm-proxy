"""V2 Proxy API endpoints with cross-protocol transformation support.

This module provides V2 API endpoints that support cross-protocol transformation
between different LLM API formats (OpenAI, Anthropic, Response API).
"""

from datetime import datetime, timezone
from typing import Any, Awaitable, Callable, Optional
import json
import time
import uuid

import httpx
from fastapi import APIRouter, Request, Depends, HTTPException
from fastapi.responses import JSONResponse, Response
from starlette.background import BackgroundTask
from starlette.requests import ClientDisconnect
from starlette.responses import StreamingResponse

from app.api.dependencies import verify_auth, get_provider_svc
from app.services.provider_service import ProviderService, get_provider_service
from app.services.langfuse_service import (
    get_langfuse_service,
    GenerationData,
    extract_client_metadata,
    build_langfuse_tags,
    LangfuseService,
)
from app.core.http_client import get_http_client
from app.core.header_policy import sanitize_anthropic_beta_header
from app.core.utils import strip_provider_suffix
from app.utils.client import extract_client
from app.utils.gemini3 import normalize_gemini3_request, strip_gemini3_provider_fields
from app.core.jsonl_logger import (
    log_request,
    log_provider_request,
    log_provider_response,
    log_provider_streaming_response,
)
from app.core.error_logger import log_error, ErrorCategory
from app.core.metrics import (
    BYPASS_REQUESTS,
    BYPASS_STREAMING_BYTES,
    CROSS_PROTOCOL_REQUESTS,
    CLIENT_DISCONNECTS,
    TOKEN_USAGE,
)
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
from app.models.config import CredentialConfig
from app.models.provider import Provider
from app.transformer import (
    Protocol,
    ProtocolDetector,
    TransformContext,
    TransformPipeline,
    TransformerRegistry,
    CrossProtocolStreamState,
    SseParser,
    format_sse_done,
    OpenAITransformer,
    AnthropicTransformer,
    GcpVertexTransformer,
    ResponseApiTransformer,
)

router = APIRouter()
logger = get_logger()


# =============================================================================
# Global Pipeline Instance
# =============================================================================

_transform_pipeline: Optional[TransformPipeline] = None


def get_transform_pipeline() -> TransformPipeline:
    """Get or create the global transform pipeline."""
    global _transform_pipeline
    if _transform_pipeline is None:
        registry = TransformerRegistry()
        registry.register(OpenAITransformer())
        registry.register(AnthropicTransformer())
        registry.register(GcpVertexTransformer())
        registry.register(ResponseApiTransformer())
        _transform_pipeline = TransformPipeline(registry)
    return _transform_pipeline


# =============================================================================
# Helper Functions
# =============================================================================


def _get_provider_endpoint(protocol: Protocol) -> str:
    """Get the endpoint path for a provider protocol."""
    if protocol == Protocol.OPENAI:
        return "/chat/completions"
    elif protocol == Protocol.ANTHROPIC:
        return "/v1/messages"
    elif protocol == Protocol.RESPONSE_API:
        return "/responses"
    elif protocol == Protocol.GCP_VERTEX:
        # GCP Vertex endpoint is built dynamically in _build_gcp_vertex_url
        return ""
    return "/chat/completions"


def _extract_model_from_request(payload: dict[str, Any], protocol: Protocol) -> str:
    """Extract model from request based on protocol."""
    return payload.get("model", "unknown")


def _extract_stream_flag(payload: dict[str, Any], protocol: Protocol) -> bool:
    """Extract stream flag from request based on protocol."""
    return payload.get("stream", False)


def _provider_type_to_protocol(provider_type: str) -> Protocol:
    """Convert provider_type string to Protocol enum."""
    return Protocol.from_provider_type(provider_type)


def _build_gcp_vertex_url(provider: "Provider", model: str, is_streaming: bool) -> str:
    """Build the GCP Vertex AI endpoint URL.

    The api_base should include any custom prefix (e.g., https://xxx.com/models/gcp-vertex).
    This function only appends the dynamic GCP Vertex path.

    Args:
        provider: Provider instance with GCP configuration
        model: Model name (e.g., claude-sonnet-4-20250514)
        is_streaming: Whether this is a streaming request

    Returns:
        Full URL for the GCP Vertex AI endpoint
    """
    action = "streamRawPredict" if is_streaming else "rawPredict"
    return (
        f"{provider.api_base}/v1/projects/{provider.gcp_project}"
        f"/locations/{provider.gcp_location}"
        f"/publishers/{provider.gcp_publisher}"
        f"/models/{model}:{action}"
    )


def _build_provider_url(
    provider: "Provider", ctx: "TransformContext", model: str
) -> str:
    """Build the provider endpoint URL based on protocol.

    Args:
        provider: Provider instance
        ctx: Transform context with protocol info
        model: Model name for GCP Vertex

    Returns:
        Full URL for the provider endpoint
    """
    if ctx.provider_protocol == Protocol.GCP_VERTEX:
        return _build_gcp_vertex_url(provider, model, ctx.stream)
    return f"{provider.api_base}{_get_provider_endpoint(ctx.provider_protocol)}"


def _normalize_gemini3_provider_payload(
    provider_payload: dict[str, Any], ctx: TransformContext
) -> None:
    if ctx.provider_protocol != Protocol.OPENAI:
        return
    model = (
        provider_payload.get("model")
        if isinstance(provider_payload.get("model"), str)
        else None
    )
    normalize_gemini3_request(provider_payload, model)
    strip_gemini3_provider_fields(provider_payload, model)


def _attach_response_metadata(
    response: Response, model_name: str, provider_name: str
) -> Response:
    """Store model/provider info on response for downstream middleware."""
    extensions = getattr(response, "extensions", None)
    if not isinstance(extensions, dict):
        extensions = {}
        setattr(response, "extensions", extensions)
    extensions["model"] = model_name
    extensions["provider"] = provider_name
    return response


def _create_protocol_error_event(
    protocol: Protocol, error_type: str, message: str
) -> bytes:
    """Create a protocol-aware error event for streaming responses.

    Args:
        protocol: The protocol format (Anthropic, OpenAI, etc.)
        error_type: The error type string
        message: The error message

    Returns:
        Formatted SSE event as bytes
    """
    if protocol in (Protocol.ANTHROPIC, Protocol.GCP_VERTEX):
        event = {"type": "error", "error": {"type": error_type, "message": message}}
    else:  # OpenAI and Response API
        event = {"error": {"message": message, "type": error_type}}

    return f"data: {json.dumps(event)}\n\n".encode()


def _build_protocol_error_response(
    protocol: Protocol,
    status_code: int,
    error_type: str,
    message: str,
    model: Optional[str] = None,
    provider: Optional[str] = None,
) -> Response:
    """Build an error response in the appropriate protocol format."""
    if protocol in (Protocol.ANTHROPIC, Protocol.GCP_VERTEX):
        body = {
            "type": "error",
            "error": {
                "type": error_type,
                "message": message,
            },
        }
    else:  # OpenAI and Response API
        body = {
            "error": {
                "message": message,
                "type": error_type,
                "code": status_code,
            },
        }

    response = JSONResponse(content=body, status_code=status_code)
    if model:
        _attach_response_metadata(response, model, provider or "unknown")
    return response


def _build_provider_debug_headers(
    url: str,
    ctx: TransformContext,
    headers: dict[str, str],
) -> dict[str, str]:
    """Build provider request headers for error logging (with secrets masked)."""
    h: dict[str, str] = {"url": url, "content-type": "application/json"}
    if ctx.provider_protocol == Protocol.ANTHROPIC:
        h["x-api-key"] = "***"
        h["anthropic-version"] = headers.get("anthropic-version", "2023-06-01")
        if "anthropic-beta" in headers:
            h["anthropic-beta"] = headers["anthropic-beta"]
    elif ctx.provider_protocol == Protocol.GCP_VERTEX:
        h["authorization"] = "Bearer ***"
        h["anthropic-version"] = headers.get("anthropic-version", "vertex-2023-10-16")
        if "anthropic-beta" in headers:
            h["anthropic-beta"] = headers["anthropic-beta"]
    else:
        h["authorization"] = "Bearer ***"
    return h


def _record_protocol_metrics(ctx: TransformContext, bypassed: bool) -> None:
    """Record bypass or cross-protocol metrics based on transformation result.

    Args:
        ctx: Transform context containing protocol and provider info
        bypassed: Whether the request/response was bypassed (same-protocol)
    """
    client_protocol_str = ctx.client_protocol.value
    provider_protocol_str = ctx.provider_protocol.value
    provider_name = ctx.provider_name

    if bypassed:
        # Same-protocol optimization was used
        BYPASS_REQUESTS.labels(
            client_protocol=client_protocol_str,
            provider_protocol=provider_protocol_str,
            provider=provider_name,
        ).inc()
    else:
        # Cross-protocol transformation was required
        CROSS_PROTOCOL_REQUESTS.labels(
            client_protocol=client_protocol_str,
            provider_protocol=provider_protocol_str,
            provider=provider_name,
        ).inc()


def _record_bypass_streaming_bytes(provider: str, byte_count: int) -> None:
    """Record bytes passed through without transformation in streaming mode.

    Args:
        provider: Provider name
        byte_count: Number of bytes passed through
    """
    if byte_count > 0:
        BYPASS_STREAMING_BYTES.labels(provider=provider).inc(byte_count)


async def _cleanup_stream_context(
    stream_ctx,
    ctx: TransformContext,
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    chunk_collector: list[str],
    usage_tracker: Optional[StreamingUsageTracker] = None,
):
    """Cleanup streaming context and finalize logging.

    Args:
        stream_ctx: The streaming context to close
        ctx: Transform context with metadata
        generation_data: Generation data to finalize
        trace_id: Langfuse trace ID
        langfuse_service: Langfuse service instance
        chunk_collector: Collected chunks for logging
        usage_tracker: Optional usage tracker for metrics recording
    """
    await stream_ctx.__aexit__(None, None, None)
    generation_data.end_time = datetime.now(timezone.utc)
    if trace_id:
        langfuse_service.trace_generation(generation_data)

    # Log streaming provider response to JSONL
    log_provider_streaming_response(
        request_id=ctx.request_id,
        provider=ctx.provider_name,
        status_code=200,
        error_msg=None,
        chunk_sequence=chunk_collector,
    )

    # Record token usage metrics at exit point
    if usage_tracker is not None:
        client = ctx.metadata.get("client", "unknown")
        stats = StreamStats(
            model=ctx.original_model or "unknown",
            provider=ctx.provider_name,
            api_key_name=get_api_key_name(),
            client=client,
            input_tokens=usage_tracker.input_tokens,
            output_tokens=usage_tracker.output_tokens,
            start_time=usage_tracker.start_time,
            first_token_time=usage_tracker.first_token_time,
        )
        record_stream_metrics(stats)


# =============================================================================
# Langfuse Integration
# =============================================================================


def _init_langfuse_trace(
    request: Request,
    endpoint: str,
    langfuse_service: LangfuseService,
) -> tuple[str, str, Optional[str], GenerationData]:
    """Initialize Langfuse tracing for a proxy request."""
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
        name="v2-proxy",
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
    effective_model: Optional[str],
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    client_protocol: Protocol,
) -> tuple[Provider, Optional[Response]]:
    """Select a provider using weighted algorithm.

    Returns (provider, None) on success, or (None, error_response) on failure.
    Error response is formatted according to client_protocol for consistency.
    """
    try:
        provider = provider_svc.get_next_provider(model=effective_model)
    except ValueError as e:
        logger.error(f"Provider selection failed: {str(e)}")
        generation_data.is_error = True
        generation_data.error_message = str(e)
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        # Return protocol-aware error response instead of HTTPException
        error_response = _build_protocol_error_response(
            client_protocol,
            400,
            "invalid_request_error",
            str(e),
            effective_model,
            None,
        )
        return None, error_response

    generation_data.provider_key = provider.name
    generation_data.provider_api_base = provider.api_base
    mapped_model = (
        provider.get_mapped_model(effective_model)
        if provider.model_mapping
        else effective_model
    )
    generation_data.mapped_model = mapped_model or effective_model or "unknown"

    if trace_id:
        langfuse_service.update_trace_provider(
            trace_id=trace_id,
            provider_key=provider.name,
            provider_api_base=provider.api_base,
            model=effective_model or "unknown",
        )

    return provider, None


# =============================================================================
# Streaming Response Handler
# =============================================================================


async def _create_cross_protocol_stream(
    response: httpx.Response,
    pipeline: TransformPipeline,
    ctx: TransformContext,
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    input_tokens: int = 0,
    chunk_collector: Optional[list[str]] = None,
    usage_tracker: Optional[StreamingUsageTracker] = None,
    disconnect_check: Optional[Callable[[], Awaitable[bool]]] = None,
):
    """Create a cross-protocol streaming response generator.

    Args:
        response: HTTP response from provider
        pipeline: Transform pipeline for protocol conversion
        ctx: Transform context with metadata
        generation_data: Generation data for Langfuse tracing
        trace_id: Langfuse trace ID
        langfuse_service: Langfuse service instance
        input_tokens: Pre-calculated input tokens for usage fallback
        chunk_collector: Optional list to collect chunks for logging
        usage_tracker: Optional usage tracker for metrics recording
        disconnect_check: Optional async callable that returns True if client disconnected
    """
    client_protocol = ctx.client_protocol
    provider_protocol = ctx.provider_protocol
    model_label = ctx.original_model
    provider_name = ctx.provider_name

    # Initialize usage tracker with input tokens
    if usage_tracker is not None:
        usage_tracker.input_tokens = input_tokens

    # For same-protocol streaming, use bypass optimization
    if client_protocol == provider_protocol:
        # Direct passthrough with model rewriting
        # Track total bytes for bypass streaming metrics
        # Use aiter_lines() to ensure complete SSE lines (prevents JSON truncation)
        total_bypass_bytes = 0
        sse_buffer = ""
        is_response_api = client_protocol == Protocol.RESPONSE_API
        bypass_usage_found = False
        async for line in response.aiter_lines():
            # Accumulate lines into SSE events (events are separated by blank lines)
            sse_buffer += line + "\n"

            # Check if we have a complete SSE event (blank line marks end of event)
            if not line.strip():
                # We have a complete event, process the buffer
                chunk_str = sse_buffer
                sse_buffer = ""
            else:
                # Continue accumulating lines
                continue

            # Only check disconnect on non-empty lines (skip SSE event boundaries)
            # Check after getting the line but before processing it
            if disconnect_check is not None:
                try:
                    if await disconnect_check():
                        logger.debug(
                            f"Client disconnected, stopping bypass stream from provider {provider_name} "
                            f"model={model_label or 'unknown'}"
                        )
                        # Record client disconnect metric
                        CLIENT_DISCONNECTS.labels(
                            model=model_label or "unknown",
                            provider=provider_name,
                        ).inc()
                        break
                except Exception as e:
                    logger.debug(f"Error checking client disconnect: {e}")

            chunk = chunk_str.encode("utf-8")
            # Track first token time
            if usage_tracker is not None and usage_tracker.first_token_time is None:
                usage_tracker.first_token_time = time.time()
            # Collect raw SSE data for JSONL logging
            if chunk_collector is not None:
                chunk_collector.append(chunk_str)

            # Extract usage/content from Response API events in bypass mode
            if is_response_api and usage_tracker is not None:
                for buf_line in chunk_str.splitlines():
                    if not buf_line.startswith("data: "):
                        continue
                    data_str = buf_line[6:].strip()
                    if not data_str or data_str == "[DONE]":
                        continue
                    try:
                        event_obj = json.loads(data_str)
                        event_type = event_obj.get("type", "")

                        if not bypass_usage_found and event_type in (
                            "response.completed",
                            "response.done",
                        ):
                            resp_usage = (event_obj.get("response") or {}).get("usage")
                            if resp_usage:
                                inp = resp_usage.get("input_tokens", 0)
                                out = resp_usage.get("output_tokens", 0)
                                if inp > 0 or out > 0:
                                    usage_tracker.input_tokens = inp
                                    usage_tracker.output_tokens = out
                                    usage_tracker.usage_from_provider = True
                                    bypass_usage_found = True
                                    generation_data.prompt_tokens = inp
                                    generation_data.completion_tokens = out
                                    generation_data.total_tokens = inp + out

                        if event_type == "response.output_text.delta":
                            delta = event_obj.get("delta", "")
                            if delta:
                                generation_data.output_content += delta
                    except (json.JSONDecodeError, TypeError):
                        pass

            # Track bytes for bypass streaming metrics
            total_bypass_bytes += len(chunk)
            yield chunk

        # Process any remaining content in the buffer
        if sse_buffer.strip():
            chunk = sse_buffer.encode("utf-8")
            if chunk_collector is not None:
                chunk_collector.append(sse_buffer)
            total_bypass_bytes += len(chunk)
            yield chunk

        # Record bypass streaming bytes metric
        _record_bypass_streaming_bytes(provider_name, total_bypass_bytes)
        return

    # Cross-protocol streaming requires chunk-by-chunk transformation
    stream_state = CrossProtocolStreamState(
        model=model_label, input_tokens=input_tokens
    )
    sse_parser = SseParser()

    # Use aiter_lines() to ensure complete lines (prevents JSON truncation)
    async for line in response.aiter_lines():
        # Only check disconnect on non-empty lines (skip SSE event boundaries)
        # Check after getting the line but before processing it
        if line.strip() and disconnect_check is not None:
            try:
                if await disconnect_check():
                    logger.debug(
                        f"Client disconnected, stopping cross-protocol stream from provider {provider_name} "
                        f"model={model_label or 'unknown'}"
                    )
                    # Record client disconnect metric
                    CLIENT_DISCONNECTS.labels(
                        model=model_label or "unknown",
                        provider=provider_name,
                    ).inc()
                    break
            except Exception as e:
                logger.debug(f"Error checking client disconnect: {e}")

        try:
            # Track first token time
            if usage_tracker is not None and usage_tracker.first_token_time is None:
                usage_tracker.first_token_time = time.time()

            # Collect raw SSE data for JSONL logging
            if chunk_collector is not None:
                chunk_collector.append(line + "\n")

            # Feed line to SSE parser (adding newline back for parser)
            # The parser expects bytes and handles event boundaries
            chunk = (line + "\n").encode("utf-8")
            events = sse_parser.parse(chunk)

            for event in events:
                if event.data == "[DONE]":
                    # Finalize stream and emit closing events
                    final_chunks = stream_state.finalize()
                    for final_chunk in final_chunks:
                        formatted = pipeline.transform_stream_chunk_out(
                            final_chunk, ctx
                        )
                        yield formatted.encode("utf-8")
                    # Add [DONE] marker only if finalize didn't emit MessageStop
                    # (finalize emits MessageStop which OpenAI transformer converts to [DONE])
                    if not stream_state.message_stopped:
                        yield format_sse_done().encode("utf-8")

                    # Capture final usage for metrics recording
                    if usage_tracker is not None:
                        final_usage = stream_state.get_final_usage()
                        if final_usage:
                            usage_tracker.input_tokens = final_usage.input_tokens
                            usage_tracker.output_tokens = final_usage.output_tokens
                            usage_tracker.usage_from_provider = (
                                stream_state.usage is not None
                                and stream_state.usage != final_usage
                            )
                    continue

                if event.data:
                    # Transform chunk from provider format to unified format
                    # Note: event.data is already parsed JSON from SSE parser,
                    # but transform_stream_chunk_in expects SSE format with "data: " prefix
                    sse_formatted = f"data: {event.data}\n"
                    unified_chunks = pipeline.transform_stream_chunk_in(
                        sse_formatted.encode("utf-8"), ctx
                    )

                    # Process chunks through state tracker
                    processed_chunks = stream_state.process_chunks(unified_chunks)

                    # Transform unified chunks to client format
                    for unified_chunk in processed_chunks:
                        formatted = pipeline.transform_stream_chunk_out(
                            unified_chunk, ctx
                        )
                        yield formatted.encode("utf-8")

        except Exception as e:
            logger.error(f"Error processing stream chunk: {e}", exc_info=True)
            # Send protocol-aware error event
            error_event = _create_protocol_error_event(
                client_protocol, "stream_error", str(e)
            )
            yield error_event
            # Terminate stream after error
            break


async def _handle_streaming_request(
    request: Request,
    provider: Provider,
    data: dict[str, Any],
    effective_model: Optional[str],
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    pipeline: TransformPipeline,
    ctx: TransformContext,
) -> Response:
    """Handle a streaming (SSE) proxy request."""
    logger.debug(f"Streaming V2 request to {provider.name} for model {effective_model}")

    # Pre-calculate input tokens for usage fallback
    from app.utils.streaming import (
        build_messages_for_token_count,
        calculate_message_tokens,
    )

    messages = data.get("messages", [])
    tools = data.get("tools")
    system = data.get("system")

    combined_messages = build_messages_for_token_count(messages, system)
    input_tokens = calculate_message_tokens(
        combined_messages,
        effective_model or "gpt-4",
        tools=tools,
        tool_choice=data.get("tool_choice"),
    )

    logger.debug(f"Pre-calculated input tokens: {input_tokens}")

    # Build provider request headers
    headers = {
        "Authorization": f"Bearer {provider.api_key}",
        "Content-Type": "application/json",
    }

    # Add Anthropic-specific headers if needed
    if ctx.provider_protocol == Protocol.ANTHROPIC:
        headers["anthropic-version"] = "2023-06-01"
        headers["x-api-key"] = provider.api_key
        del headers["Authorization"]
    # GCP Vertex uses Bearer token authentication (already set above)
    elif ctx.provider_protocol == Protocol.GCP_VERTEX:
        headers["anthropic-version"] = "vertex-2023-10-16"

    # Forward anthropic-beta header (filtered by policy)
    if ctx.provider_protocol in (Protocol.ANTHROPIC, Protocol.GCP_VERTEX):
        anthropic_beta = sanitize_anthropic_beta_header(
            provider.provider_type,
            provider.provider_params,
            request.headers.get("anthropic-beta"),
        )
        if anthropic_beta:
            headers["anthropic-beta"] = anthropic_beta

    # Transform request
    provider_payload, bypassed = pipeline.transform_request_with_bypass(data, ctx)
    _normalize_gemini3_provider_payload(provider_payload, ctx)

    # Record bypass/cross-protocol metrics
    _record_protocol_metrics(ctx, bypassed)

    if bypassed:
        logger.debug(f"Using bypass mode for streaming request to {provider.name}")

    # Build URL based on provider protocol
    mapped_model = (
        provider.get_mapped_model(effective_model)
        if effective_model
        else effective_model
    )
    url = _build_provider_url(provider, ctx, mapped_model or "")
    provider_endpoint = _get_provider_endpoint(ctx.provider_protocol)

    # Log provider request to JSONL
    log_provider_request(
        request_id=ctx.request_id,
        provider=provider.name,
        api_base=provider.api_base,
        endpoint=provider_endpoint,
        payload=provider_payload,
    )

    # Use shared HTTP client for streaming
    client = get_http_client()
    stream_ctx = client.stream("POST", url, json=provider_payload, headers=headers)

    try:
        response = await stream_ctx.__aenter__()
    except Exception as e:
        await stream_ctx.__aexit__(None, None, None)
        logger.error(f"Failed to connect to provider {provider.name}: {e}")
        raise HTTPException(status_code=502, detail=f"Provider connection failed: {e}")

    try:
        if response.status_code >= 400:
            # Report error status to adaptive routing
            get_provider_service().report_http_status(
                provider.name,
                response.status_code,
                response.headers.get("retry-after"),
            )
            # Read error body
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
            # Handle both error formats: {"error": {"message": "..."}} and {"error": "..."}
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

            # Log provider error response to JSONL
            log_provider_response(
                request_id=ctx.request_id,
                provider=provider.name,
                status_code=response.status_code,
                error_msg=generation_data.error_message,
                body=error_json,
            )

            if response.status_code != 429:
                error_category = (
                    ErrorCategory.PROVIDER_5XX
                    if response.status_code >= 500
                    else ErrorCategory.PROVIDER_4XX
                )
                provider_debug_headers = _build_provider_debug_headers(
                    url,
                    ctx,
                    headers,
                )
                log_error(
                    error_category=error_category,
                    error_code=response.status_code,
                    error_message=generation_data.error_message,
                    request_id=ctx.request_id,
                    provider_name=ctx.provider_name,
                    credential_name=generation_data.credential_name,
                    model_requested=ctx.original_model,
                    model_mapped=ctx.mapped_model,
                    endpoint=generation_data.endpoint,
                    client_protocol=ctx.client_protocol.value
                    if ctx.client_protocol
                    else None,
                    provider_protocol=ctx.provider_protocol.value
                    if ctx.provider_protocol
                    else None,
                    is_streaming=True,
                    response_body=json.dumps(error_json) if error_json else None,
                    provider_request_body=json.dumps(provider_payload),
                    provider_request_headers=json.dumps(provider_debug_headers),
                )

            return _build_protocol_error_response(
                ctx.client_protocol,
                response.status_code,
                "api_error",
                generation_data.error_message,
                effective_model,
                provider.name,
            )

        # Report streaming success to adaptive routing
        get_provider_service().report_http_status(
            provider.name,
            response.status_code,
            response.headers.get("retry-after"),
        )

        # Collect chunks for JSONL logging
        chunk_collector: list[str] = []

        # Create usage tracker for metrics recording at exit
        usage_tracker = StreamingUsageTracker(input_tokens=input_tokens)

        # Create streaming response
        async def stream_generator():
            async for chunk in _create_cross_protocol_stream(
                response,
                pipeline,
                ctx,
                generation_data,
                trace_id,
                langfuse_service,
                input_tokens=input_tokens,
                chunk_collector=chunk_collector,
                usage_tracker=usage_tracker,
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
        # Use BackgroundTask for cleanup instead of finally block
        streaming_response.background = BackgroundTask(
            _cleanup_stream_context,
            stream_ctx,
            ctx,
            generation_data,
            trace_id,
            langfuse_service,
            chunk_collector,
            usage_tracker,
        )
        return _attach_response_metadata(
            streaming_response, request.state.model, provider.name
        )

    except Exception:
        await stream_ctx.__aexit__(None, None, None)
        raise


# =============================================================================
# Non-Streaming Response Handler
# =============================================================================


async def _handle_non_streaming_request(
    request: Request,
    provider: Provider,
    data: dict[str, Any],
    effective_model: Optional[str],
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    pipeline: TransformPipeline,
    ctx: TransformContext,
) -> Response:
    """Handle a non-streaming proxy request."""
    logger.debug(
        f"Non-streaming V2 request to {provider.name} for model {effective_model}"
    )

    # Build provider request headers
    headers = {
        "Authorization": f"Bearer {provider.api_key}",
        "Content-Type": "application/json",
    }

    # Add Anthropic-specific headers if needed
    if ctx.provider_protocol == Protocol.ANTHROPIC:
        headers["anthropic-version"] = "2023-06-01"
        headers["x-api-key"] = provider.api_key
        del headers["Authorization"]
    # GCP Vertex uses Bearer token authentication (already set above)
    elif ctx.provider_protocol == Protocol.GCP_VERTEX:
        headers["anthropic-version"] = "vertex-2023-10-16"

    # Forward anthropic-beta header (filtered by policy)
    if ctx.provider_protocol in (Protocol.ANTHROPIC, Protocol.GCP_VERTEX):
        anthropic_beta = sanitize_anthropic_beta_header(
            provider.provider_type,
            provider.provider_params,
            request.headers.get("anthropic-beta"),
        )
        if anthropic_beta:
            headers["anthropic-beta"] = anthropic_beta

    # Transform request
    provider_payload, bypassed = pipeline.transform_request_with_bypass(data, ctx)
    _normalize_gemini3_provider_payload(provider_payload, ctx)

    # Record bypass/cross-protocol metrics
    _record_protocol_metrics(ctx, bypassed)

    if bypassed:
        logger.debug(f"Using bypass mode for non-streaming request to {provider.name}")

    # Build URL based on provider protocol
    mapped_model = (
        provider.get_mapped_model(effective_model)
        if effective_model
        else effective_model
    )
    url = _build_provider_url(provider, ctx, mapped_model or "")
    provider_endpoint = _get_provider_endpoint(ctx.provider_protocol)

    # Log provider request to JSONL
    log_provider_request(
        request_id=ctx.request_id,
        provider=provider.name,
        api_base=provider.api_base,
        endpoint=provider_endpoint,
        payload=provider_payload,
    )

    # Use shared HTTP client
    client = get_http_client()
    response = await client.post(url, json=provider_payload, headers=headers)

    # Report HTTP status to adaptive routing
    get_provider_service().report_http_status(
        provider.name,
        response.status_code,
        response.headers.get("retry-after"),
    )

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

        # Log provider error response to JSONL
        log_provider_response(
            request_id=ctx.request_id,
            provider=provider.name,
            status_code=response.status_code,
            error_msg=generation_data.error_message,
            body=error_body,
        )

        if response.status_code != 429:
            error_category = (
                ErrorCategory.PROVIDER_5XX
                if response.status_code >= 500
                else ErrorCategory.PROVIDER_4XX
            )
            provider_debug_headers = _build_provider_debug_headers(
                url,
                ctx,
                headers,
            )
            log_error(
                error_category=error_category,
                error_code=response.status_code,
                error_message=generation_data.error_message,
                request_id=ctx.request_id,
                provider_name=ctx.provider_name,
                credential_name=generation_data.credential_name,
                model_requested=ctx.original_model,
                model_mapped=ctx.mapped_model,
                endpoint=generation_data.endpoint,
                client_protocol=ctx.client_protocol.value
                if ctx.client_protocol
                else None,
                provider_protocol=ctx.provider_protocol.value
                if ctx.provider_protocol
                else None,
                is_streaming=False,
                response_body=json.dumps(error_body) if error_body else None,
                provider_request_body=json.dumps(provider_payload),
                provider_request_headers=json.dumps(provider_debug_headers),
            )

        return _build_protocol_error_response(
            ctx.client_protocol,
            response.status_code,
            "api_error",
            generation_data.error_message,
            effective_model,
            provider.name,
        )

    response_data = response.json()

    # Record token usage metrics from provider response
    _record_token_usage(
        response_data,
        ctx,
        effective_model or "unknown",
        provider.name,
        generation_data,
        extract_client(request),
    )

    # Log provider response to JSONL
    log_provider_response(
        request_id=ctx.request_id,
        provider=provider.name,
        status_code=response.status_code,
        error_msg=None,
        body=response_data,
    )

    # Transform response
    client_response, bypassed = pipeline.transform_response_with_bypass(
        response_data, ctx
    )

    if bypassed:
        logger.debug(f"Using bypass mode for response from {provider.name}")

    # Record successful generation
    generation_data.end_time = datetime.now(timezone.utc)
    if trace_id:
        langfuse_service.trace_generation(generation_data)

    success_response = JSONResponse(
        content=client_response, status_code=response.status_code
    )
    return _attach_response_metadata(
        success_response, request.state.model, provider.name
    )


def _record_token_usage(
    response_data: dict[str, Any],
    ctx: TransformContext,
    model_name: str,
    provider_name: str,
    generation_data: GenerationData,
    client: str,
) -> None:
    """Extract and record token usage metrics from provider response.

    Handles both OpenAI (prompt_tokens/completion_tokens/total_tokens) and
    Anthropic (input_tokens/output_tokens) usage formats.
    """
    usage = response_data.get("usage", {})
    if not usage:
        return

    api_key_name = get_api_key_name()

    # Normalize field names across protocols
    input_t = usage.get("prompt_tokens") or usage.get("input_tokens") or 0
    output_t = usage.get("completion_tokens") or usage.get("output_tokens") or 0
    total_t = usage.get("total_tokens") or (input_t + output_t)

    # Update generation_data for Langfuse
    generation_data.prompt_tokens = input_t
    generation_data.completion_tokens = output_t
    generation_data.total_tokens = total_t

    if input_t:
        TOKEN_USAGE.labels(
            model=model_name,
            provider=provider_name,
            token_type="prompt",
            api_key_name=api_key_name,
            client=client,
        ).inc(input_t)

    if output_t:
        TOKEN_USAGE.labels(
            model=model_name,
            provider=provider_name,
            token_type="completion",
            api_key_name=api_key_name,
            client=client,
        ).inc(output_t)

    if total_t:
        TOKEN_USAGE.labels(
            model=model_name,
            provider=provider_name,
            token_type="total",
            api_key_name=api_key_name,
            client=client,
        ).inc(total_t)


# =============================================================================
# Main Proxy Handler
# =============================================================================


async def handle_proxy_request(
    request: Request,
    path: str,
    provider_svc: ProviderService,
) -> Response:
    """Handle a V2 proxy request with protocol transformation.

    This is the main entry point for protocol-aware proxying.
    It handles:
    1. Protocol detection (from path or request structure)
    2. Request transformation (client format → provider format)
    3. Upstream request execution
    4. Response transformation (provider format → client format)
    """
    # Initialize Langfuse tracing
    langfuse_service = get_langfuse_service()
    request_id, credential_name, trace_id, generation_data = _init_langfuse_trace(
        request, path, langfuse_service
    )
    client = extract_client(request)

    # Detect client protocol from path (before parsing body)
    client_protocol = ProtocolDetector.detect_with_path_hint({}, path)

    # Parse request body
    try:
        data = await _parse_request_body(
            request, generation_data, trace_id, langfuse_service
        )
    except ClientDisconnect:
        # Return protocol-aware error response for client disconnect
        return _build_protocol_error_response(
            client_protocol,
            408,
            "client_closed_request",
            "Client closed request",
        )

    # Detect client protocol from path (now with body data)
    client_protocol = ProtocolDetector.detect_with_path_hint(data, path)

    # Extract model and stream flag
    original_model = _extract_model_from_request(data, client_protocol)
    effective_model = strip_provider_suffix(original_model)
    is_streaming = _extract_stream_flag(data, client_protocol)

    # Update generation data
    generation_data.original_model = effective_model
    generation_data.is_streaming = is_streaming

    # Select provider
    provider, error_response = _select_provider(
        provider_svc,
        effective_model,
        generation_data,
        trace_id,
        langfuse_service,
        client_protocol,
    )

    # Return protocol-aware error if provider selection failed
    if error_response is not None:
        return error_response

    # Store model and provider in request state for metrics
    request.state.model = effective_model or "unknown"
    request.state.provider = provider.name

    # Determine provider protocol from provider_type
    provider_protocol = _provider_type_to_protocol(
        getattr(provider, "provider_type", "openai")
    )

    # Get transform pipeline
    pipeline = get_transform_pipeline()

    # Build transform context
    ctx = TransformContext(
        request_id=request_id,
        client_protocol=client_protocol,
        provider_protocol=provider_protocol,
        original_model=original_model,
        mapped_model=provider.get_mapped_model(effective_model),
        provider_name=provider.name,
        stream=is_streaming,
        metadata={"client": client},
    )

    # Log client request to JSONL
    log_request(
        request_id=request_id,
        endpoint=path,
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
                generation_data,
                trace_id,
                langfuse_service,
                pipeline,
                ctx,
            )
        else:
            return await _handle_non_streaming_request(
                request,
                provider,
                data,
                effective_model,
                generation_data,
                trace_id,
                langfuse_service,
                pipeline,
                ctx,
            )

    except HTTPException:
        raise
    except httpx.TimeoutException:
        provider_svc.report_transport_error(provider.name)
        logger.error(f"Timeout error for provider {provider.name}")
        generation_data.is_error = True
        generation_data.error_message = "Gateway timeout"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=504, detail="Gateway timeout")
    except httpx.RequestError as e:
        provider_svc.report_transport_error(provider.name)
        logger.error(f"Network request error for provider {provider.name}: {e}")
        generation_data.is_error = True
        generation_data.error_message = f"Provider {provider.name} network error"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(
            status_code=502, detail=f"Provider {provider.name} network error"
        )
    except Exception:
        logger.exception(f"Unexpected error for provider {provider.name}")
        generation_data.is_error = True
        generation_data.error_message = "Internal server error"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=500, detail="Internal server error")
    finally:
        clear_provider_context()


# =============================================================================
# V2 API Endpoints
# =============================================================================


@router.post("/chat/completions")
async def chat_completions_v2(
    request: Request,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """OpenAI-compatible chat completions endpoint with cross-protocol support.

    This endpoint accepts OpenAI-format requests and can route them to any
    provider (OpenAI, Anthropic, etc.) with automatic protocol transformation.
    """
    return await handle_proxy_request(request, "/v1/chat/completions", provider_svc)


@router.post("/messages")
async def messages_v2(
    request: Request,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """Anthropic-compatible messages endpoint with cross-protocol support.

    This endpoint accepts Anthropic-format requests and can route them to any
    provider (OpenAI, Anthropic, etc.) with automatic protocol transformation.
    """
    return await handle_proxy_request(request, "/v1/messages", provider_svc)


@router.post("/responses")
async def responses_v2(
    request: Request,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """Response API endpoint with cross-protocol support.

    This endpoint accepts Response API format requests and can route them to any
    provider (OpenAI, Anthropic, etc.) with automatic protocol transformation.
    """
    return await handle_proxy_request(request, "/v1/responses", provider_svc)


@router.get("/models")
async def list_models_v2(
    credential_config: Optional["CredentialConfig"] = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """List all available models (V2 - OpenAI compatible).

    Returns a list of all available models that can be used with the API.
    This endpoint is compatible with the OpenAI Models API.
    """
    from app.api.dependencies import model_matches_allowed_list

    models_set = provider_svc.get_all_models()

    if credential_config and credential_config.allowed_models:
        # Filter models using wildcard/regex matching
        filtered_models = set()
        for model in models_set:
            if model_matches_allowed_list(model, credential_config.allowed_models):
                filtered_models.add(model)
        models_set = filtered_models

    models_list = [
        {
            "id": model,
            "object": "model",
            "created": 1677610602,
            "owned_by": "system",
            "permission": [],
            "root": model,
            "parent": None,
        }
        for model in sorted(models_set)
    ]

    return {"object": "list", "data": models_list}


@router.post("/messages/count_tokens")
async def count_tokens_v2(
    request: Request,
    _: None = Depends(verify_auth),
):
    """Claude token counting endpoint (V2).

    Provides accurate token count for the given messages using tiktoken.
    """
    from app.api.claude import (
        _build_claude_messages_for_token_count,
        _convert_claude_tools_for_token_count,
    )
    from app.models.claude import ClaudeTokenCountRequest
    from app.utils.streaming import calculate_message_tokens

    try:
        data = await request.json()
        claude_request = ClaudeTokenCountRequest(**data)

        model = claude_request.model
        combined_messages = _build_claude_messages_for_token_count(
            claude_request.system, claude_request.messages
        )
        tools = _convert_claude_tools_for_token_count(claude_request.tools)
        total_tokens = calculate_message_tokens(
            combined_messages,
            model,
            tools=tools,
            tool_choice=claude_request.tool_choice,
        )

        # Ensure at least 1 token
        estimated_tokens = max(1, total_tokens)

        return {"input_tokens": estimated_tokens}

    except Exception as e:
        logger.error(f"Error counting tokens (V2): {e}")
        raise HTTPException(status_code=500, detail=str(e))


@router.post("/completions")
async def completions_v2(
    request: Request,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """Legacy completions endpoint (V2 - OpenAI compatible).

    This endpoint is compatible with the OpenAI Completions API (legacy).
    """
    from app.api.completions import proxy_completion_request

    return await proxy_completion_request(request, "completions", provider_svc)
