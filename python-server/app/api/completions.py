"""Completions API endpoints"""

from dataclasses import dataclass
from datetime import datetime, timezone
from typing import Optional, Tuple
import json
import uuid

import httpx
from fastapi import APIRouter, Request, Depends, HTTPException
from fastapi.responses import JSONResponse, Response
from starlette.background import BackgroundTask
from starlette.requests import ClientDisconnect

from app.api.dependencies import verify_auth, get_provider_svc
from app.core.jsonl_logger import (
    log_request,
    log_response,
    log_provider_request,
    log_provider_response,
)
from app.services.provider_service import ProviderService
from app.services.langfuse_service import (
    get_langfuse_service,
    GenerationData,
    extract_client_metadata,
    build_langfuse_tags,
    LangfuseService,
)
from app.utils.streaming import create_streaming_response, rewrite_model_in_response
from app.utils.gemini3 import (
    log_gemini_request_signatures,
    log_gemini_response_signatures,
    normalize_gemini3_request,
    normalize_gemini3_response,
    strip_gemini3_provider_fields,
)
from app.core.config import get_config, get_env_config
from app.core.exceptions import TTFTTimeoutError
from app.core.http_client import get_http_client
from app.core.metrics import TOKEN_USAGE
from app.core.logging import (
    get_logger,
    set_provider_context,
    clear_provider_context,
    get_api_key_name,
)
from app.core.utils import strip_provider_suffix
from app.utils.client import extract_client
from app.models.provider import Provider

router = APIRouter()
logger = get_logger()


@dataclass
class RequestContext:
    """Context data for a completion request.

    Holds all the parsed and computed data needed throughout the request lifecycle.
    """

    request_id: str
    credential_name: str
    endpoint: str
    trace_id: Optional[str]
    generation_data: GenerationData
    original_model: Optional[str]
    effective_model: Optional[str]
    request_data: dict
    is_streaming: bool


def _attach_response_metadata(response, model_name: str, provider_name: str):
    """Store model/provider info on response for downstream middleware."""
    extensions = getattr(response, "extensions", None)
    if not isinstance(extensions, dict):
        extensions = {}
        setattr(response, "extensions", extensions)
    extensions["model"] = model_name
    extensions["provider"] = provider_name
    return response


async def _close_stream_resources(stream_ctx):
    """Close streaming context (client is shared, don't close it)"""
    await stream_ctx.__aexit__(None, None, None)


def _parse_non_stream_error(response: httpx.Response) -> dict:
    """Parse error response body from non-streaming request."""
    try:
        return response.json()
    except Exception:
        message = response.text or f"HTTP {response.status_code}"
        return {"error": {"message": message}}


async def _parse_stream_error(response: httpx.Response) -> dict:
    """Parse error response body from streaming request."""
    try:
        body = await response.aread()
    except Exception:
        body = b""
    if body:
        try:
            return json.loads(body.decode("utf-8"))
        except Exception:
            text = body.decode("utf-8", errors="replace")
            return {"error": {"message": text or f"HTTP {response.status_code}"}}
    return {"error": {"message": f"HTTP {response.status_code}"}}


def _init_langfuse_trace(
    request: Request,
    endpoint: str,
    langfuse_service: LangfuseService,
) -> Tuple[str, str, Optional[str], GenerationData]:
    """Initialize Langfuse tracing for a completion request.

    Creates a trace and initializes generation data for tracking the request
    through Langfuse observability.

    Args:
        request: The FastAPI request object
        endpoint: The API endpoint being called (e.g., "chat/completions")
        langfuse_service: The Langfuse service instance

    Returns:
        Tuple containing:
        - request_id: Unique identifier for this request
        - credential_name: Name of the credential used for authentication
        - trace_id: Langfuse trace ID (None if tracing disabled/not sampled)
        - generation_data: Initialized GenerationData for tracking
    """
    request_id = getattr(request.state, "request_id", str(uuid.uuid4()))
    credential_name = getattr(request.state, "credential_name", "anonymous")

    # Extract client metadata from headers for Langfuse tracing
    client_metadata = extract_client_metadata(request)
    user_agent = client_metadata.get("user_agent")

    # Build tags for Langfuse (credential, user-agent)
    tags = build_langfuse_tags(endpoint, credential_name, user_agent)

    # Create trace (returns None if disabled or not sampled)
    trace_id = langfuse_service.create_trace(
        request_id=request_id,
        credential_name=credential_name,
        endpoint=f"/v1/{endpoint}",
        tags=tags,
        client_metadata=client_metadata,
    )

    # Initialize generation data for Langfuse
    generation_data = GenerationData(
        trace_id=trace_id or "",
        name="chat-completion" if "chat" in endpoint else "completion",
        request_id=request_id,
        credential_name=credential_name,
        endpoint=f"/v1/{endpoint}",
        start_time=datetime.now(timezone.utc),
    )

    return request_id, credential_name, trace_id, generation_data


async def _parse_request_body(
    request: Request,
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
) -> Tuple[dict, Optional[str], Optional[str]]:
    """Parse and validate the request body.

    Extracts the JSON body, handles client disconnection and JSON errors,
    and captures input data for Langfuse tracing.

    Args:
        request: The FastAPI request object
        generation_data: GenerationData to update with parsed info
        trace_id: Langfuse trace ID for error recording
        langfuse_service: The Langfuse service instance

    Returns:
        Tuple containing:
        - data: Parsed request body as dict
        - original_model: The model name from the request
        - effective_model: The model name with provider suffix stripped

    Raises:
        HTTPException: If client disconnects or JSON is invalid
    """
    try:
        data = await request.json()
    except ClientDisconnect:
        logger.info("Client disconnected before request body was read")
        generation_data.is_error = True
        generation_data.error_message = "Client closed request"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=408, detail="Client closed request")
    except json.JSONDecodeError as e:
        logger.warning(f"Invalid JSON in request body: {str(e)}")
        generation_data.is_error = True
        generation_data.error_message = f"Invalid JSON: {str(e)}"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=422, detail=f"Invalid JSON: {str(e)}")

    original_model = data.get("model")

    # Strip provider suffix if present (e.g., "Proxy/gpt-4" -> "gpt-4")
    effective_model = (
        strip_provider_suffix(original_model) if original_model else original_model
    )

    # Capture input data for Langfuse
    generation_data.original_model = effective_model or "unknown"
    generation_data.input_messages = data.get("messages", [])
    generation_data.model_parameters = {
        k: v
        for k, v in data.items()
        if k
        in [
            "temperature",
            "max_tokens",
            "top_p",
            "frequency_penalty",
            "presence_penalty",
            "stop",
            "n",
            "logprobs",
        ]
    }
    generation_data.is_streaming = data.get("stream", False)

    return data, original_model, effective_model


def _select_provider(
    provider_svc: ProviderService,
    effective_model: Optional[str],
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
) -> Provider:
    """Select a provider using weighted algorithm.

    Selects the appropriate provider based on the model and updates
    generation data with provider information.

    Args:
        provider_svc: The provider service for selection
        effective_model: The model name (with provider suffix stripped)
        generation_data: GenerationData to update with provider info
        trace_id: Langfuse trace ID for error recording and updates
        langfuse_service: The Langfuse service instance

    Returns:
        The selected Provider

    Raises:
        HTTPException: If no provider is available for the model
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
        raise HTTPException(status_code=400, detail=str(e))

    # Capture provider info for Langfuse
    generation_data.provider_key = provider.name
    generation_data.provider_api_base = provider.api_base
    # Use pattern-aware model mapping
    mapped_model = (
        provider.get_mapped_model(effective_model)
        if provider.model_mapping
        else effective_model
    )
    generation_data.mapped_model = mapped_model or effective_model or "unknown"

    # Update trace with provider info (so trace metadata includes provider)
    if trace_id:
        langfuse_service.update_trace_provider(
            trace_id=trace_id,
            provider_key=provider.name,
            provider_api_base=provider.api_base,
            model=effective_model or "unknown",
        )

    return provider


async def _handle_streaming_request(
    request: Request,
    provider: Provider,
    data: dict,
    effective_model: Optional[str],
    provider_model: Optional[str],
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    endpoint: str,
    request_id: str,
    client: str,
) -> Response:
    """Handle a streaming (SSE) completion request.

    Establishes a streaming connection to the provider and returns
    a streaming response to the client.

    Args:
        request: The FastAPI request object
        provider: The selected provider configuration
        data: The request body data
        effective_model: The model name (with provider suffix stripped)
        provider_model: The model name sent to provider (after mapping)
        generation_data: GenerationData for Langfuse tracing
        trace_id: Langfuse trace ID
        langfuse_service: The Langfuse service instance
        endpoint: The API endpoint being called
        request_id: Request ID for JSONL logging

    Returns:
        A streaming Response object
    """
    logger.debug(f"Streaming request to {provider.name} for model {effective_model}")

    headers = {
        "Authorization": f"Bearer {provider.api_key}",
        "Content-Type": "application/json",
    }
    url = f"{provider.api_base}/{endpoint}"

    strip_gemini3_provider_fields(data, provider_model)

    # Log provider request to JSONL (consistent with Rust V1)
    log_provider_request(
        request_id=request_id,
        provider=provider.name,
        api_base=provider.api_base,
        endpoint=f"/{endpoint}",
        payload=data,
    )

    # Use shared HTTP client for streaming
    http_client = get_http_client()
    stream_ctx = http_client.stream("POST", url, json=data, headers=headers)
    try:
        response = await stream_ctx.__aenter__()
    except Exception:
        # Don't close shared client, just exit stream context
        await stream_ctx.__aexit__(None, None, None)
        raise

    try:
        if response.status_code >= 400:
            error_body = await _parse_stream_error(response)
            logger.error(
                f"Backend API returned error status {response.status_code} "
                f"from provider {provider.name} during streaming"
            )
            await _close_stream_resources(stream_ctx)

            # Record error in Langfuse
            generation_data.is_error = True
            generation_data.error_message = (
                error_body.get("error", {}).get("message", "")
                or f"HTTP {response.status_code}"
            )
            generation_data.end_time = datetime.now(timezone.utc)
            if trace_id:
                langfuse_service.trace_generation(generation_data)

            error_response = JSONResponse(
                content=error_body, status_code=response.status_code
            )
            return _attach_response_metadata(
                error_response, request.state.model, provider.name
            )

        config = get_config()
        # Pass generation_data to streaming response for TTFT and output capture
        streaming_response = create_streaming_response(
            response,
            effective_model,
            provider.name,
            provider_model,
            data,
            config.ttft_timeout_secs,
            generation_data=generation_data if trace_id else None,
            client=client,
        )
        streaming_response.background = BackgroundTask(
            _close_stream_resources, stream_ctx
        )
        return _attach_response_metadata(
            streaming_response, request.state.model, provider.name
        )
    except Exception:
        await _close_stream_resources(stream_ctx)
        raise


def _record_token_metrics(
    usage: dict,
    model_name: str,
    provider_name: str,
    generation_data: GenerationData,
    client: str,
) -> None:
    """Record token usage metrics to Prometheus.

    Updates Prometheus counters for prompt, completion, and total tokens.

    Args:
        usage: The usage dict from the API response
        model_name: The model name for metric labels
        provider_name: The provider name for metric labels
        generation_data: GenerationData to update with token counts
        client: The client name from User-Agent header
    """
    api_key_name = get_api_key_name()

    # Capture usage for Langfuse
    generation_data.prompt_tokens = usage.get("prompt_tokens", 0)
    generation_data.completion_tokens = usage.get("completion_tokens", 0)
    generation_data.total_tokens = usage.get("total_tokens", 0)

    if "prompt_tokens" in usage:
        TOKEN_USAGE.labels(
            model=model_name,
            provider=provider_name,
            token_type="prompt",
            api_key_name=api_key_name,
            client=client,
        ).inc(usage["prompt_tokens"])

    if "completion_tokens" in usage:
        TOKEN_USAGE.labels(
            model=model_name,
            provider=provider_name,
            token_type="completion",
            api_key_name=api_key_name,
            client=client,
        ).inc(usage["completion_tokens"])

    if "total_tokens" in usage:
        TOKEN_USAGE.labels(
            model=model_name,
            provider=provider_name,
            token_type="total",
            api_key_name=api_key_name,
            client=client,
        ).inc(usage["total_tokens"])

        logger.debug(
            f"Token usage - model={model_name} provider={provider_name} key={api_key_name} client={client} "
            f"prompt={usage.get('prompt_tokens', 0)} "
            f"completion={usage.get('completion_tokens', 0)} "
            f"total={usage.get('total_tokens', 0)}"
        )


async def _handle_non_streaming_request(
    request: Request,
    provider: Provider,
    data: dict,
    effective_model: Optional[str],
    provider_model: Optional[str],
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
    endpoint: str,
    request_id: str,
    client: str,
) -> Response:
    """Handle a non-streaming completion request.

    Makes a synchronous request to the provider and returns the JSON response.

    Args:
        request: The FastAPI request object
        provider: The selected provider configuration
        data: The request body data
        effective_model: The model name (with provider suffix stripped)
        generation_data: GenerationData for Langfuse tracing
        trace_id: Langfuse trace ID
        langfuse_service: The Langfuse service instance
        endpoint: The API endpoint being called
        request_id: Request ID for JSONL logging

    Returns:
        A JSON Response object
    """
    logger.debug(
        f"Non-streaming request to {provider.name} for model {effective_model}"
    )

    # Pre-calculate input tokens for fallback (when provider doesn't return usage)
    from app.utils.streaming import (
        build_messages_for_token_count,
        calculate_message_tokens,
    )

    messages = data.get("messages", [])
    tools = data.get("tools")
    system = data.get("system")

    combined_messages = build_messages_for_token_count(messages, system)
    fallback_input_tokens = calculate_message_tokens(
        combined_messages,
        effective_model or "gpt-3.5-turbo",
        tools=tools,
        tool_choice=data.get("tool_choice"),
    )

    logger.debug(f"Pre-calculated fallback input tokens: {fallback_input_tokens}")

    headers = {
        "Authorization": f"Bearer {provider.api_key}",
        "Content-Type": "application/json",
    }
    url = f"{provider.api_base}/{endpoint}"

    strip_gemini3_provider_fields(data, provider_model)

    # Log provider request to JSONL (consistent with Rust V1)
    log_provider_request(
        request_id=request_id,
        provider=provider.name,
        api_base=provider.api_base,
        endpoint=f"/{endpoint}",
        payload=data,
    )

    # Use shared HTTP client for non-streaming requests
    http_client = get_http_client()
    response = await http_client.post(url, json=data, headers=headers)

    # Check if backend API returned an error status code
    # Faithfully pass through the backend error
    if response.status_code >= 400:
        error_body = _parse_non_stream_error(response)

        # Log provider error response to JSONL
        log_provider_response(
            request_id=request_id,
            provider=provider.name,
            status_code=response.status_code,
            error_msg=(
                error_body.get("error", {}).get("message")
                if isinstance(error_body.get("error"), dict)
                else error_body.get("error")
            ),
            body=error_body,
        )

        logger.error(
            f"Backend API returned error status {response.status_code} "
            f"from provider {provider.name}"
        )

        # Record error in Langfuse
        generation_data.is_error = True
        # Handle both {"error": "string"} and {"error": {"message": "string"}} formats
        error_field = error_body.get("error", {})
        if isinstance(error_field, str):
            generation_data.error_message = error_field
        elif isinstance(error_field, dict):
            generation_data.error_message = error_field.get("message", "")
        else:
            generation_data.error_message = f"HTTP {response.status_code}"

        if not generation_data.error_message:
            generation_data.error_message = f"HTTP {response.status_code}"

        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)

        # Faithfully return the backend's status code and error body
        # Use Response instead of JSONResponse to avoid double serialization
        error_response = Response(
            content=json.dumps(error_body),
            status_code=response.status_code,
            media_type="application/json",
        )
        return _attach_response_metadata(
            error_response, request.state.model, provider.name
        )

    response_data = response.json()

    # Log provider response to JSONL (consistent with Rust V1)
    log_provider_response(
        request_id=request_id,
        provider=provider.name,
        status_code=response.status_code,
        error_msg=None,
        body=response_data,
    )

    # Normalize Gemini 3 response (align with LiteLLM handling)
    normalize_gemini3_response(response_data, provider_model)

    # Log Gemini 3 response signatures for debugging (pass-through, no modification)
    log_gemini_response_signatures(response_data, provider_model)

    # Capture output for Langfuse
    if "choices" in response_data and response_data["choices"]:
        choice = response_data["choices"][0]
        generation_data.output_content = choice.get("message", {}).get("content", "")
        generation_data.finish_reason = choice.get("finish_reason")

    # Extract and record token usage
    if "usage" in response_data:
        # Use effective_model (without provider suffix) for metrics
        model_name = effective_model or "unknown"
        _record_token_metrics(
            response_data["usage"],
            model_name,
            provider.name,
            generation_data,
            client,
        )
    else:
        # Use fallback token calculation when provider doesn't return usage
        logger.warning(
            f"Provider {provider.name} didn't return usage, using fallback calculation"
        )
        fallback_usage = {
            "prompt_tokens": fallback_input_tokens,
            "completion_tokens": 0,  # Cannot calculate without response text
            "total_tokens": fallback_input_tokens,
        }
        model_name = effective_model or "unknown"
        _record_token_metrics(
            fallback_usage,
            model_name,
            provider.name,
            generation_data,
            client,
        )

    # Record successful generation in Langfuse
    generation_data.end_time = datetime.now(timezone.utc)
    if trace_id:
        langfuse_service.trace_generation(generation_data)

    # Use effective_model in response so client sees model without provider suffix
    response_data = rewrite_model_in_response(response_data, effective_model)
    success_response = JSONResponse(
        content=response_data, status_code=response.status_code
    )
    return _attach_response_metadata(
        success_response, request.state.model, provider.name
    )


def _handle_request_error(
    error: Exception,
    provider: Provider,
    generation_data: GenerationData,
    trace_id: Optional[str],
    langfuse_service: LangfuseService,
) -> HTTPException:
    """Handle errors that occur during request processing.

    Logs the error, records it in Langfuse, and returns an appropriate HTTPException.

    Args:
        error: The exception that occurred
        provider: The provider configuration
        generation_data: GenerationData for Langfuse tracing
        trace_id: Langfuse trace ID
        langfuse_service: The Langfuse service instance

    Returns:
        An HTTPException to raise
    """
    generation_data.is_error = True
    generation_data.end_time = datetime.now(timezone.utc)

    if isinstance(error, TTFTTimeoutError):
        logger.error(f"TTFT timeout for provider {provider.name}: {str(error)}")
        generation_data.error_message = f"TTFT timeout: first token not received within {error.timeout_secs} seconds"
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        return HTTPException(
            status_code=504,
            detail={
                "error": {
                    "message": f"TTFT timeout: first token not received within {error.timeout_secs} seconds",
                    "type": "timeout_error",
                    "code": "ttft_timeout",
                }
            },
        )

    if isinstance(error, httpx.RemoteProtocolError):
        logger.error(
            f"Remote protocol error for provider {provider.name}: {str(error)} - "
            f"Provider closed connection unexpectedly during request"
        )
        generation_data.error_message = (
            f"Provider {provider.name} connection closed unexpectedly"
        )
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        return HTTPException(
            status_code=502,
            detail=f"Provider {provider.name} connection closed unexpectedly",
        )

    if isinstance(error, httpx.TimeoutException):
        logger.error(f"Timeout error for provider {provider.name}: {str(error)}")
        generation_data.error_message = "Gateway timeout"
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        return HTTPException(status_code=504, detail="Gateway timeout")

    if isinstance(error, httpx.HTTPStatusError):
        logger.error(
            f"HTTP error for provider {provider.name}: {error.response.status_code} - {str(error)}"
        )
        generation_data.error_message = str(error)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        return HTTPException(status_code=error.response.status_code, detail=str(error))

    if isinstance(error, httpx.RequestError):
        logger.error(
            f"Network request error for provider {provider.name}: {str(error)}"
        )
        generation_data.error_message = f"Provider {provider.name} network error"
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        return HTTPException(
            status_code=502, detail=f"Provider {provider.name} network error"
        )

    # Unexpected error
    logger.exception(f"Unexpected error for provider {provider.name}")
    generation_data.error_message = "Internal server error"
    if trace_id:
        langfuse_service.trace_generation(generation_data)
    return HTTPException(status_code=500, detail="Internal server error")


async def proxy_completion_request(
    request: Request, endpoint: str, provider_svc: ProviderService
) -> Response:
    """Proxy completion requests to LLM providers with Langfuse tracing.

    This is the main coordinator function that orchestrates the completion request
    flow by delegating to specialized helper functions.

    Args:
        request: The FastAPI request object
        endpoint: The API endpoint (e.g., "chat/completions" or "completions")
        provider_svc: The provider service for selecting providers

    Returns:
        A Response object (streaming or JSON)

    Raises:
        HTTPException: For various error conditions
    """
    # Initialize Langfuse tracing
    langfuse_service = get_langfuse_service()
    request_id, credential_name, trace_id, generation_data = _init_langfuse_trace(
        request, endpoint, langfuse_service
    )

    # Parse and validate request body
    data, original_model, effective_model = await _parse_request_body(
        request, generation_data, trace_id, langfuse_service
    )

    # Select provider based on the effective model (without provider suffix)
    provider = _select_provider(
        provider_svc, effective_model, generation_data, trace_id, langfuse_service
    )

    # Store effective model (without provider suffix) and provider in request state for metrics
    request.state.model = effective_model or "unknown"
    request.state.provider = provider.name

    # Apply model mapping if configured
    if "model" in data and provider.model_mapping:
        # Use effective_model for model_mapping lookup (supports wildcard patterns)
        data["model"] = provider.get_mapped_model(effective_model)

    provider_model = data.get("model") if isinstance(data.get("model"), str) else None

    # Normalize Gemini 3 request payload (align with LiteLLM handling)
    normalize_gemini3_request(data, provider_model)

    # Log client request to JSONL (consistent with Rust V1 and Python V2)
    log_request(
        request_id=request_id,
        endpoint=f"/v1/{endpoint}",
        provider=provider.name,
        payload=data,
    )

    # Extract client from User-Agent header for metrics
    client = extract_client(request)

    try:
        # Set provider context for logging
        set_provider_context(provider.name)

        # Log Gemini 3 request signatures for debugging (pass-through, no modification)
        log_gemini_request_signatures(data, provider_model)

        if data.get("stream", False):
            return await _handle_streaming_request(
                request,
                provider,
                data,
                effective_model,
                provider_model,
                generation_data,
                trace_id,
                langfuse_service,
                endpoint,
                request_id,
                client,
            )
        else:
            return await _handle_non_streaming_request(
                request,
                provider,
                data,
                effective_model,
                provider_model,
                generation_data,
                trace_id,
                langfuse_service,
                endpoint,
                request_id,
                client,
            )

    except HTTPException:
        # Re-raise HTTP exceptions as-is (already handled)
        raise
    except Exception as e:
        raise _handle_request_error(
            e, provider, generation_data, trace_id, langfuse_service
        )
    finally:
        # Clear provider context after request
        clear_provider_context()


@router.post("/chat/completions")
async def chat_completions(
    request: Request,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """Proxy chat completions requests to providers"""
    return await proxy_completion_request(request, "chat/completions", provider_svc)


@router.post("/completions")
async def completions(
    request: Request,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """Proxy completions requests to providers"""
    return await proxy_completion_request(request, "completions", provider_svc)
