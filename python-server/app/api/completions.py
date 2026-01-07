"""Completions API endpoints"""

from typing import Optional
import json

import httpx
from fastapi import APIRouter, Request, Depends, HTTPException
from fastapi.responses import JSONResponse
from starlette.background import BackgroundTask
from starlette.requests import ClientDisconnect

from app.api.dependencies import verify_auth, get_provider_svc
from app.services.provider_service import ProviderService
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
    """Common logic for proxying completion requests"""
    try:
        data = await request.json()
    except ClientDisconnect:
        logger.info("Client disconnected before request body was read")
        raise HTTPException(status_code=499, detail="Client closed request")
    except json.JSONDecodeError as e:
        logger.warning(f"Invalid JSON in request body: {str(e)}")
        raise HTTPException(status_code=422, detail=f"Invalid JSON: {str(e)}")

    original_model = data.get("model")

    # Strip provider suffix if present (e.g., "Proxy/gpt-4" -> "gpt-4")
    effective_model = (
        _strip_provider_suffix(original_model) if original_model else original_model
    )

    # Select provider based on the effective model (without provider suffix)
    try:
        provider = provider_svc.get_next_provider(model=effective_model)
    except ValueError as e:
        logger.error(f"Provider selection failed: {str(e)}")
        raise HTTPException(status_code=400, detail=str(e))

    # Store effective model (without provider suffix) and provider in request state for metrics
    request.state.model = effective_model or "unknown"
    request.state.provider = provider.name

    if "model" in data and provider.model_mapping:
        # Use effective_model for model_mapping lookup
        data["model"] = provider.model_mapping.get(effective_model, effective_model)

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
                    error_response = JSONResponse(
                        content=error_body, status_code=response.status_code
                    )
                    return _attach_response_metadata(
                        error_response, request.state.model, provider.name
                    )

                config = get_config()
                streaming_response = create_streaming_response(
                    response,
                    effective_model,
                    provider.name,
                    data,
                    config.ttft_timeout_secs,
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
                # Faithfully return the backend's status code and error body
                error_response = JSONResponse(
                    content=error_body, status_code=response.status_code
                )
                return _attach_response_metadata(
                    error_response, request.state.model, provider.name
                )

            response_data = response.json()

            # Extract and record token usage
            if "usage" in response_data:
                usage = response_data["usage"]
                # Use effective_model (without provider suffix) for metrics
                model_name = effective_model or "unknown"
                api_key_name = get_api_key_name()

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
        raise HTTPException(
            status_code=502,
            detail=f"Provider {provider.name} connection closed unexpectedly",
        )
    except httpx.TimeoutException as e:
        logger.error(f"Timeout error for provider {provider.name}: {str(e)}")
        raise HTTPException(status_code=504, detail="Gateway timeout")
    except httpx.HTTPStatusError as e:
        logger.error(
            f"HTTP error for provider {provider.name}: {e.response.status_code} - {str(e)}"
        )
        raise HTTPException(status_code=e.response.status_code, detail=str(e))
    except httpx.RequestError as e:
        logger.error(f"Network request error for provider {provider.name}: {str(e)}")
        raise HTTPException(
            status_code=502, detail=f"Provider {provider.name} network error"
        )
    except Exception as e:
        logger.exception(f"Unexpected error for provider {provider.name}")
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
