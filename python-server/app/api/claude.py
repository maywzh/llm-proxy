"""Claude API compatible endpoints.

This module provides Claude Messages API compatibility by converting
Claude format requests to OpenAI format, proxying to providers,
and converting responses back to Claude format.
"""

from datetime import datetime, timezone
import json
import uuid

import httpx
from fastapi import APIRouter, Request, Depends, HTTPException
from fastapi.responses import JSONResponse, StreamingResponse
from starlette.background import BackgroundTask

from app.api.dependencies import verify_auth, get_provider_svc
from app.api.proxy import _strip_provider_suffix
from app.services.provider_service import ProviderService
from app.services.langfuse_service import (
    get_langfuse_service,
    GenerationData,
    extract_client_metadata,
    build_langfuse_tags,
)
from app.services.claude_converter import (
    claude_to_openai_request,
    openai_to_claude_response,
    convert_openai_streaming_to_claude,
)
from app.utils.streaming import count_tokens as tiktoken_count
from app.models.claude import ClaudeMessagesRequest, ClaudeTokenCountRequest
from app.core.http_client import get_http_client
from app.core.metrics import TOKEN_USAGE
from app.core.logging import (
    get_logger,
    set_provider_context,
    clear_provider_context,
    get_api_key_name,
)

router = APIRouter(prefix="/v1", tags=["claude"])
logger = get_logger()


def _calculate_claude_input_tokens(request: ClaudeMessagesRequest) -> int:
    """Calculate input tokens for Claude request using tiktoken

    Args:
        request: Claude Messages API request

    Returns:
        Estimated input tokens count
    """
    model = request.model
    total = 0

    # Count system prompt tokens
    if request.system:
        if isinstance(request.system, str):
            total += tiktoken_count(request.system, model)
        elif isinstance(request.system, list):
            for block in request.system:
                if hasattr(block, 'text'):
                    total += tiktoken_count(block.text, model)
                elif isinstance(block, dict) and 'text' in block:
                    total += tiktoken_count(block['text'], model)

    # Count message tokens (role + content + overhead)
    for msg in request.messages:
        # Role tokens
        total += tiktoken_count(msg.role, model)

        # Content tokens
        if isinstance(msg.content, str):
            total += tiktoken_count(msg.content, model)
        elif isinstance(msg.content, list):
            for block in msg.content:
                block_dict = block.dict() if hasattr(block, 'dict') else block
                block_type = block_dict.get('type') if isinstance(block_dict, dict) else None

                # Text content
                if block_type == 'text':
                    text = block_dict.get('text', '')
                    total += tiktoken_count(text, model)

                # Image content (Claude format)
                elif block_type == 'image':
                    # Claude images use 'source' field with base64 data
                    # Conservative estimate: 765 tokens per image
                    total += 765

        # Message format overhead
        total += 4

    # Conversation overhead
    total += 2

    # Count tools tokens
    if request.tools:
        from app.utils.streaming import calculate_tools_tokens
        tools_list = [t.dict() if hasattr(t, 'dict') else t for t in request.tools]
        total += calculate_tools_tokens(tools_list, model)

    return total


async def _close_stream_resources(stream_ctx):
    """Close streaming context."""
    await stream_ctx.__aexit__(None, None, None)


def _build_claude_error_response(message: str, error_type: str = "api_error") -> dict:
    """Build a Claude-formatted error response."""
    return {
        "type": "error",
        "error": {
            "type": error_type,
            "message": message,
        },
    }


@router.post("/messages")
async def create_message(
    request: Request,
    claude_request: ClaudeMessagesRequest,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """Claude Messages API endpoint.

    Converts Claude API requests to OpenAI format, proxies to provider,
    and converts response back to Claude format.

    Supports both streaming and non-streaming modes.
    """
    # Initialize Langfuse tracing
    langfuse_service = get_langfuse_service()
    request_id = getattr(request.state, "request_id", str(uuid.uuid4()))
    credential_name = getattr(request.state, "credential_name", "anonymous")

    # Extract client metadata from headers for Langfuse tracing
    client_metadata = extract_client_metadata(request)
    user_agent = client_metadata.get("user_agent")

    # Build tags for Langfuse (credential, user-agent)
    tags = build_langfuse_tags("messages", credential_name, user_agent)

    # Create trace (returns None if disabled or not sampled)
    trace_id = langfuse_service.create_trace(
        request_id=request_id,
        credential_name=credential_name,
        endpoint="/v1/messages",
        tags=tags,
        client_metadata=client_metadata,
    )

    # Initialize generation data
    generation_data = GenerationData(
        trace_id=trace_id or "",
        name="claude-messages",
        request_id=request_id,
        credential_name=credential_name,
        endpoint="/v1/messages",
        start_time=datetime.now(timezone.utc),
    )

    try:
        logger.debug(
            f"Processing Claude request: model={claude_request.model}, "
            f"stream={claude_request.stream}"
        )

        # Strip provider suffix before selecting provider (P0 fix: ensure consistency with Rust v1)
        effective_model = _strip_provider_suffix(claude_request.model)
        logger.debug(
            f"Effective model after stripping provider suffix: {effective_model}"
        )

        # Select provider based on effective model
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

        # Capture provider info
        generation_data.provider_key = provider.name
        generation_data.provider_api_base = provider.api_base
        generation_data.original_model = claude_request.model

        # Update trace with provider info (so trace metadata includes provider)
        if trace_id:
            langfuse_service.update_trace_provider(
                trace_id=trace_id,
                provider_key=provider.name,
                provider_api_base=provider.api_base,
                model=claude_request.model,
            )

        # Convert Claude request to OpenAI format
        openai_request = claude_to_openai_request(
            claude_request,
            model_mapping=provider.model_mapping,
        )

        generation_data.mapped_model = openai_request.get("model", claude_request.model)
        generation_data.input_messages = openai_request.get("messages", [])
        generation_data.is_streaming = claude_request.stream or False

        # Store in request state for metrics
        request.state.model = claude_request.model
        request.state.provider = provider.name

        # P0 fix: Use x-api-key header for Anthropic protocol
        headers = {
            "Authorization": f"Bearer {provider.api_key}",
            "Content-Type": "application/json",
        }

        # Check if provider is Anthropic type and adjust headers
        if hasattr(provider, "provider_type") and provider.provider_type == "anthropic":
            headers["anthropic-version"] = request.headers.get(
                "anthropic-version", "2023-06-01"
            )
            headers["x-api-key"] = provider.api_key

            # Forward anthropic-beta header if provided
            anthropic_beta = request.headers.get("anthropic-beta")
            if anthropic_beta:
                headers["anthropic-beta"] = anthropic_beta

            del headers["Authorization"]
            url = f"{provider.api_base}/v1/messages"
        else:
            url = f"{provider.api_base}/chat/completions"

        try:
            set_provider_context(provider.name)

            if claude_request.stream:
                return await _handle_streaming_request(
                    url=url,
                    openai_request=openai_request,
                    headers=headers,
                    claude_request=claude_request,
                    provider=provider,
                    generation_data=generation_data,
                    trace_id=trace_id,
                    langfuse_service=langfuse_service,
                )
            else:
                return await _handle_non_streaming_request(
                    url=url,
                    openai_request=openai_request,
                    headers=headers,
                    claude_request=claude_request,
                    provider=provider,
                    generation_data=generation_data,
                    trace_id=trace_id,
                    langfuse_service=langfuse_service,
                )

        except httpx.TimeoutException:
            logger.error(f"Timeout error for provider {provider.name}")
            generation_data.is_error = True
            generation_data.error_message = "Gateway timeout"
            generation_data.end_time = datetime.now(timezone.utc)
            if trace_id:
                langfuse_service.trace_generation(generation_data)
            return JSONResponse(
                content=_build_claude_error_response(
                    "Gateway timeout", "timeout_error"
                ),
                status_code=504,
            )
        except httpx.RequestError as e:
            logger.error(f"Network error for provider {provider.name}: {str(e)}")
            generation_data.is_error = True
            generation_data.error_message = f"Provider {provider.name} network error"
            generation_data.end_time = datetime.now(timezone.utc)
            if trace_id:
                langfuse_service.trace_generation(generation_data)
            return JSONResponse(
                content=_build_claude_error_response(
                    f"Provider {provider.name} network error", "api_error"
                ),
                status_code=502,
            )
        finally:
            clear_provider_context()

    except HTTPException:
        raise
    except Exception as e:
        logger.exception("Unexpected error processing Claude request")
        generation_data.is_error = True
        generation_data.error_message = "Internal server error"
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)
        return JSONResponse(
            content=_build_claude_error_response("Internal server error", "api_error"),
            status_code=500,
        )


async def _handle_streaming_request(
    url: str,
    openai_request: dict,
    headers: dict,
    claude_request: ClaudeMessagesRequest,
    provider,
    generation_data: GenerationData,
    trace_id: str,
    langfuse_service,
) -> StreamingResponse:
    """Handle streaming Claude request."""
    client = get_http_client()
    stream_ctx = client.stream("POST", url, json=openai_request, headers=headers)

    try:
        response = await stream_ctx.__aenter__()
    except Exception:
        await stream_ctx.__aexit__(None, None, None)
        raise

    if response.status_code >= 400:
        error_body = await response.aread()
        await _close_stream_resources(stream_ctx)

        try:
            error_json = json.loads(error_body.decode("utf-8"))
        except Exception:
            error_json = {
                "error": {"message": error_body.decode("utf-8", errors="replace")}
            }

        generation_data.is_error = True
        generation_data.error_message = error_json.get("error", {}).get(
            "message", f"HTTP {response.status_code}"
        )
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)

        return JSONResponse(
            content=_build_claude_error_response(
                error_json.get("error", {}).get(
                    "message", f"HTTP {response.status_code}"
                )
            ),
            status_code=response.status_code,
        )

    # Track accumulated output for Langfuse
    accumulated_output: list[str] = []
    finish_reason: str | None = None
    usage_data: dict = {}
    first_token_received = False

    # Pre-calculate input tokens for fallback
    fallback_input_tokens = _calculate_claude_input_tokens(claude_request)

    async def stream_generator():
        nonlocal accumulated_output, finish_reason, usage_data, first_token_received

        try:
            async for event in convert_openai_streaming_to_claude(
                response.aiter_bytes(),
                claude_request.model,
                fallback_input_tokens=fallback_input_tokens,
            ):
                # Track TTFT
                if not first_token_received and trace_id:
                    first_token_received = True
                    generation_data.ttft_time = datetime.now(timezone.utc)

                # Parse event to extract output content and usage
                if event.startswith("event: content_block_delta"):
                    try:
                        # Find the data line
                        lines = event.split("\n")
                        for line in lines:
                            if line.startswith("data: "):
                                data = json.loads(line[6:])
                                delta = data.get("delta", {})
                                if delta.get("type") == "text_delta":
                                    accumulated_output.append(delta.get("text", ""))
                    except (json.JSONDecodeError, KeyError):
                        pass
                elif event.startswith("event: message_delta"):
                    try:
                        lines = event.split("\n")
                        for line in lines:
                            if line.startswith("data: "):
                                data = json.loads(line[6:])
                                delta = data.get("delta", {})
                                if delta.get("stop_reason"):
                                    finish_reason = delta.get("stop_reason")
                                if data.get("usage"):
                                    usage_data = data.get("usage", {})
                    except (json.JSONDecodeError, KeyError):
                        pass

                yield event.encode("utf-8")

            # Record successful generation in Langfuse after streaming completes
            if trace_id:
                generation_data.output_content = "".join(accumulated_output)
                generation_data.finish_reason = finish_reason
                if usage_data:
                    generation_data.prompt_tokens = usage_data.get("input_tokens", 0)
                    generation_data.completion_tokens = usage_data.get(
                        "output_tokens", 0
                    )
                    generation_data.total_tokens = (
                        generation_data.prompt_tokens
                        + generation_data.completion_tokens
                    )
                generation_data.end_time = datetime.now(timezone.utc)
                langfuse_service.trace_generation(generation_data)

        except Exception as e:
            # Record error in Langfuse
            if trace_id:
                generation_data.is_error = True
                generation_data.error_message = str(e)
                generation_data.output_content = "".join(accumulated_output)
                generation_data.end_time = datetime.now(timezone.utc)
                langfuse_service.trace_generation(generation_data)
            raise

    streaming_response = StreamingResponse(
        stream_generator(),
        media_type="text/event-stream",
        headers={
            "Cache-Control": "no-cache",
            "Connection": "keep-alive",
            "Access-Control-Allow-Origin": "*",
        },
    )
    streaming_response.background = BackgroundTask(_close_stream_resources, stream_ctx)
    return streaming_response


async def _handle_non_streaming_request(
    url: str,
    openai_request: dict,
    headers: dict,
    claude_request: ClaudeMessagesRequest,
    provider,
    generation_data: GenerationData,
    trace_id: str,
    langfuse_service,
) -> JSONResponse:
    """Handle non-streaming Claude request."""
    # Pre-calculate input tokens for fallback
    fallback_input_tokens = _calculate_claude_input_tokens(claude_request)
    logger.debug(f"Pre-calculated fallback input tokens: {fallback_input_tokens}")

    client = get_http_client()
    response = await client.post(url, json=openai_request, headers=headers)

    if response.status_code >= 400:
        try:
            error_json = response.json()
        except Exception:
            error_json = {"error": {"message": response.text}}

        # Handle both error formats: {"error": {"message": "..."}} and {"error": "..."}
        error_obj = error_json.get("error", {})
        if isinstance(error_obj, str):
            error_message = error_obj
        else:
            error_message = error_obj.get("message", f"HTTP {response.status_code}")

        generation_data.is_error = True
        generation_data.error_message = error_message
        generation_data.end_time = datetime.now(timezone.utc)
        if trace_id:
            langfuse_service.trace_generation(generation_data)

        return JSONResponse(
            content=_build_claude_error_response(error_message),
            status_code=response.status_code,
        )

    openai_response = response.json()
    claude_response = openai_to_claude_response(
        openai_response,
        claude_request.model,
    )

    # Capture usage for Langfuse and metrics
    if "usage" in openai_response:
        usage = openai_response["usage"]
        generation_data.prompt_tokens = usage.get("prompt_tokens", 0)
        generation_data.completion_tokens = usage.get("completion_tokens", 0)
        generation_data.total_tokens = usage.get("total_tokens", 0)

        # Record token metrics
        model_name = claude_request.model or "unknown"
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
    else:
        # Use fallback token calculation when provider doesn't return usage
        logger.warning(
            f"Provider {provider.name} didn't return usage for Claude API, using fallback calculation"
        )
        generation_data.prompt_tokens = fallback_input_tokens
        generation_data.completion_tokens = 0  # Cannot calculate without response text
        generation_data.total_tokens = fallback_input_tokens

        # Record fallback token metrics
        model_name = claude_request.model or "unknown"
        api_key_name = get_api_key_name()

        TOKEN_USAGE.labels(
            model=model_name,
            provider=provider.name,
            token_type="prompt",
            api_key_name=api_key_name,
        ).inc(fallback_input_tokens)

        TOKEN_USAGE.labels(
            model=model_name,
            provider=provider.name,
            token_type="total",
            api_key_name=api_key_name,
        ).inc(fallback_input_tokens)

    generation_data.end_time = datetime.now(timezone.utc)
    if trace_id:
        langfuse_service.trace_generation(generation_data)

    return JSONResponse(content=claude_response)


@router.post("/messages/count_tokens")
async def count_tokens(
    claude_request: ClaudeTokenCountRequest,
    _: None = Depends(verify_auth),
):
    """Claude token counting endpoint.

    Provides accurate token count for the given messages using tiktoken.
    """
    try:
        model = claude_request.model
        total_tokens = 0

        # Count system message tokens
        if claude_request.system:
            if isinstance(claude_request.system, str):
                total_tokens += tiktoken_count(claude_request.system, model)
            elif isinstance(claude_request.system, list):
                for block in claude_request.system:
                    if hasattr(block, "text"):
                        total_tokens += tiktoken_count(block.text, model)

        # Count message tokens
        for msg in claude_request.messages:
            # Count role tokens
            total_tokens += tiktoken_count(msg.role, model)

            if msg.content is None:
                continue
            elif isinstance(msg.content, str):
                total_tokens += tiktoken_count(msg.content, model)
            elif isinstance(msg.content, list):
                for block in msg.content:
                    if hasattr(block, "text") and block.text is not None:
                        total_tokens += tiktoken_count(block.text, model)

            # Format overhead per message (same as streaming.py)
            total_tokens += 4

        # Conversation format overhead
        total_tokens += 2

        # Ensure at least 1 token
        estimated_tokens = max(1, total_tokens)

        return {"input_tokens": estimated_tokens}

    except Exception as e:
        logger.error(f"Error counting tokens: {e}")
        raise HTTPException(status_code=500, detail=str(e))
