"""Completions API endpoints"""

from datetime import datetime, timezone
from typing import Optional
import json
import uuid

import httpx
from fastapi import APIRouter, Request, Depends, HTTPException
from fastapi.responses import JSONResponse, Response
from starlette.background import BackgroundTask
from starlette.requests import ClientDisconnect

from app.api.dependencies import verify_auth, get_provider_svc
from app.services.provider_service import ProviderService
from app.services.langfuse_service import (
    get_langfuse_service,
    GenerationData,
    extract_client_metadata,
    build_langfuse_tags,
)
from app.utils.streaming import create_streaming_response, rewrite_model_in_response
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

router = APIRouter()
logger = get_logger()


def _strip_provider_suffix(model: str) -> str:
    """Strip the global provider suffix from model name if present.

    If PROVIDER_SUFFIX is set (e.g., "Proxy"), then model names like
    "Proxy/gpt-4" will be converted to "gpt-4". Model names without
    the prefix (e.g., "gpt-4") are returned unchanged.

    Args:
        model: The model name, possibly with provider suffix prefix

    Returns:
        The model name with provider suffix stripped if it was present
    """
    env_config = get_env_config()
    provider_suffix = env_config.provider_suffix

    if provider_suffix and "/" in model:
        prefix, base_model = model.split("/", 1)
        if prefix == provider_suffix:
            return base_model

    return model


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
    try:
        return response.json()
    except Exception:
        message = response.text or f"HTTP {response.status_code}"
        return {"error": {"message": message}}


async def _parse_stream_error(response: httpx.Response) -> dict:
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


async def proxy_completion_request(
    request: Request, endpoint: str, provider_svc: ProviderService
):
    """Common logic for proxying completion requests with Langfuse tracing"""
    # Initialize Langfuse tracing
    langfuse_service = get_langfuse_service()
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

    try:
        data = await request.json()
    except ClientDisconnect:
        logger.info("Client disconnected before request body was read")
        generation_data.is_error = True
        generation_data.error_message = "Client closed request"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=499, detail="Client closed request")
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
        _strip_provider_suffix(original_model) if original_model else original_model
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

    # Select provider based on the effective model (without provider suffix)
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

    # Store effective model (without provider suffix) and provider in request state for metrics
    request.state.model = effective_model or "unknown"
    request.state.provider = provider.name

    if "model" in data and provider.model_mapping:
        # Use effective_model for model_mapping lookup (supports wildcard patterns)
        data["model"] = provider.get_mapped_model(effective_model)

    headers = {
        "Authorization": f"Bearer {provider.api_key}",
        "Content-Type": "application/json",
    }

    url = f"{provider.api_base}/{endpoint}"

    try:
        # Set provider context for logging
        set_provider_context(provider.name)

        if data.get("stream", False):
            logger.debug(
                f"Streaming request to {provider.name} for model {effective_model}"
            )
            # Use shared HTTP client for streaming
            client = get_http_client()
            stream_ctx = client.stream("POST", url, json=data, headers=headers)
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
                    data,
                    config.ttft_timeout_secs,
                    generation_data=generation_data if trace_id else None,
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
        else:
            logger.debug(
                f"Non-streaming request to {provider.name} for model {effective_model}"
            )
            # Use shared HTTP client for non-streaming requests
            client = get_http_client()
            response = await client.post(url, json=data, headers=headers)

            # Check if backend API returned an error status code
            # Faithfully pass through the backend error
            if response.status_code >= 400:
                error_body = _parse_non_stream_error(response)

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
                    media_type="application/json"
                )
                return _attach_response_metadata(
                    error_response, request.state.model, provider.name
                )

            response_data = response.json()

            # Capture output for Langfuse
            if "choices" in response_data and response_data["choices"]:
                choice = response_data["choices"][0]
                generation_data.output_content = choice.get("message", {}).get(
                    "content", ""
                )
                generation_data.finish_reason = choice.get("finish_reason")

            # Extract and record token usage
            if "usage" in response_data:
                usage = response_data["usage"]
                # Use effective_model (without provider suffix) for metrics
                model_name = effective_model or "unknown"
                api_key_name = get_api_key_name()

                # Capture usage for Langfuse
                generation_data.prompt_tokens = usage.get("prompt_tokens", 0)
                generation_data.completion_tokens = usage.get("completion_tokens", 0)
                generation_data.total_tokens = usage.get("total_tokens", 0)

                if "prompt_tokens" in usage:
                    TOKEN_USAGE.labels(
                        model=model_name,
                        provider=provider.name,
                        token_type="prompt",
                        api_key_name=api_key_name,
                    ).inc(usage["prompt_tokens"])

                if "completion_tokens" in usage:
                    TOKEN_USAGE.labels(
                        model=model_name,
                        provider=provider.name,
                        token_type="completion",
                        api_key_name=api_key_name,
                    ).inc(usage["completion_tokens"])

                if "total_tokens" in usage:
                    TOKEN_USAGE.labels(
                        model=model_name,
                        provider=provider.name,
                        token_type="total",
                        api_key_name=api_key_name,
                    ).inc(usage["total_tokens"])

                    logger.debug(
                        f"Token usage - model={model_name} provider={provider.name} key={api_key_name} "
                        f"prompt={usage.get('prompt_tokens', 0)} "
                        f"completion={usage.get('completion_tokens', 0)} "
                        f"total={usage.get('total_tokens', 0)}"
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

    except TTFTTimeoutError as e:
        logger.error(f"TTFT timeout for provider {provider.name}: {str(e)}")
        generation_data.is_error = True
        generation_data.error_message = (
            f"TTFT timeout: first token not received within {e.timeout_secs} seconds"
        )
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(
            status_code=504,
            detail={
                "error": {
                    "message": f"TTFT timeout: first token not received within {e.timeout_secs} seconds",
                    "type": "timeout_error",
                    "code": "ttft_timeout",
                }
            },
        )
    except httpx.RemoteProtocolError as e:
        logger.error(
            f"Remote protocol error for provider {provider.name}: {str(e)} - "
            f"Provider closed connection unexpectedly during request"
        )
        generation_data.is_error = True
        generation_data.error_message = (
            f"Provider {provider.name} connection closed unexpectedly"
        )
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(
            status_code=502,
            detail=f"Provider {provider.name} connection closed unexpectedly",
        )
    except httpx.TimeoutException as e:
        logger.error(f"Timeout error for provider {provider.name}: {str(e)}")
        generation_data.is_error = True
        generation_data.error_message = "Gateway timeout"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=504, detail="Gateway timeout")
    except httpx.HTTPStatusError as e:
        logger.error(
            f"HTTP error for provider {provider.name}: {e.response.status_code} - {str(e)}"
        )
        generation_data.is_error = True
        generation_data.error_message = str(e)
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=e.response.status_code, detail=str(e))
    except httpx.RequestError as e:
        logger.error(f"Network request error for provider {provider.name}: {str(e)}")
        generation_data.is_error = True
        generation_data.error_message = f"Provider {provider.name} network error"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(
            status_code=502, detail=f"Provider {provider.name} network error"
        )
    except Exception as e:
        logger.exception(f"Unexpected error for provider {provider.name}")
        generation_data.is_error = True
        generation_data.error_message = "Internal server error"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        raise HTTPException(status_code=500, detail="Internal server error")
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
