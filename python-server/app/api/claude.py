"""Claude API compatible endpoints.

This module provides Claude Messages API compatibility by converting
Claude format requests to OpenAI format, proxying to providers,
and converting responses back to Claude format.
"""

from datetime import datetime, timezone
from typing import Any, Optional
import json
import time
import uuid

import httpx
from fastapi import APIRouter, Request, Depends, HTTPException
from fastapi.responses import JSONResponse, StreamingResponse
from starlette.background import BackgroundTask

from app.api.dependencies import verify_auth, get_provider_svc
from app.core.utils import strip_provider_suffix
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
from app.utils.gemini3 import normalize_gemini3_request, strip_gemini3_provider_fields
from app.utils.streaming import calculate_message_tokens
from app.models.claude import ClaudeMessagesRequest, ClaudeTokenCountRequest
from app.core.http_client import get_http_client
from app.core.metrics import TOKEN_USAGE
from app.core.stream_metrics import StreamStats, record_stream_metrics
from app.core.logging import (
    get_logger,
    set_provider_context,
    clear_provider_context,
    get_api_key_name,
)
from app.utils.client import extract_client

router = APIRouter(prefix="/v1", tags=["claude"])
logger = get_logger()


def _convert_claude_block_for_token_count(block: Any) -> dict[str, Any]:
    block_dict = block.dict() if hasattr(block, "dict") else block
    if not isinstance(block_dict, dict):
        return {"type": "text", "text": str(block)}

    block_type = block_dict.get("type")
    if block_type == "text":
        return {"type": "text", "text": block_dict.get("text", "")}
    if block_type == "image":
        source = block_dict.get("source", {})
        media_type = source.get("media_type", "image/png")
        data = source.get("data", "")
        data_uri = f"data:{media_type};base64,{data}"
        return {
            "type": "image_url",
            "image_url": {"url": data_uri, "detail": "auto"},
        }
    if block_type == "tool_use":
        return {
            "type": "tool_use",
            "id": block_dict.get("id"),
            "name": block_dict.get("name"),
            "input": block_dict.get("input"),
        }
    if block_type == "tool_result":
        return {
            "type": "tool_result",
            "tool_use_id": block_dict.get("tool_use_id"),
            "content": block_dict.get("content"),
            "is_error": block_dict.get("is_error"),
        }
    if block_type == "thinking":
        return {
            "type": "thinking",
            "thinking": block_dict.get("thinking", ""),
            "signature": block_dict.get("signature"),
        }
    return block_dict


def _build_claude_messages_for_token_count(
    system: Optional[Any], messages: list
) -> list[dict[str, Any]]:
    combined: list[dict[str, Any]] = []
    if system is not None:
        if isinstance(system, str):
            system_content: Any = system
        elif isinstance(system, list):
            system_content = [
                (
                    {"type": "text", "text": block.text}
                    if hasattr(block, "text")
                    else {"type": "text", "text": block.get("text", "")}
                )
                for block in system
            ]
        else:
            system_content = ""
        combined.append({"role": "system", "content": system_content})

    for msg in messages:
        if isinstance(msg.content, str):
            content_value: Any = msg.content
        else:
            content_value = [
                _convert_claude_block_for_token_count(block) for block in msg.content
            ]
        combined.append({"role": msg.role, "content": content_value})

    return combined


def _convert_claude_tools_for_token_count(
    tools: Optional[list],
) -> Optional[list[dict[str, Any]]]:
    if not tools:
        return None
    converted: list[dict[str, Any]] = []
    for tool in tools:
        if hasattr(tool, "model_dump"):
            tool_dict = tool.model_dump()
        elif hasattr(tool, "dict"):
            tool_dict = tool.dict()
        else:
            tool_dict = tool
        converted.append(
            {
                "type": "function",
                "function": {
                    "name": tool_dict.get("name"),
                    "description": tool_dict.get("description"),
                    "parameters": tool_dict.get("input_schema", {}),
                },
            }
        )
    return converted


def _calculate_claude_input_tokens(request: ClaudeMessagesRequest) -> int:
    model = request.model
    combined_messages = _build_claude_messages_for_token_count(
        request.system, request.messages
    )
    tools = _convert_claude_tools_for_token_count(request.tools)
    return calculate_message_tokens(
        combined_messages,
        model,
        tools=tools,
        tool_choice=request.tool_choice,
    )


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

    # Extract client from User-Agent header for metrics
    client = extract_client(request)

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
        effective_model = strip_provider_suffix(claude_request.model)
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
        provider_model = (
            openai_request.get("model")
            if isinstance(openai_request.get("model"), str)
            else None
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
            normalize_gemini3_request(openai_request, provider_model)
            strip_gemini3_provider_fields(openai_request, provider_model)

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

    # Timing for metrics
    start_time = time.time()
    first_token_time: float | None = None

    # Pre-calculate input tokens for fallback
    fallback_input_tokens = _calculate_claude_input_tokens(claude_request)

    async def stream_generator():
        nonlocal accumulated_output, finish_reason, usage_data, first_token_received, first_token_time

        try:
            async for event in convert_openai_streaming_to_claude(
                response.aiter_bytes(),
                claude_request.model,
                fallback_input_tokens=fallback_input_tokens,
            ):
                # Track TTFT
                if not first_token_received:
                    first_token_received = True
                    first_token_time = time.time()
                    if trace_id:
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

            # Record token usage metrics using unified function
            model_name = claude_request.model or "unknown"
            api_key_name = get_api_key_name()
            if usage_data:
                input_tokens = usage_data.get("input_tokens", 0)
                output_tokens = usage_data.get("output_tokens", 0)
            else:
                # Fallback when provider doesn't return usage
                input_tokens = fallback_input_tokens
                output_tokens = 0

            stats = StreamStats(
                model=model_name,
                provider=provider.name,
                api_key_name=api_key_name,
                client=client,
                input_tokens=input_tokens,
                output_tokens=output_tokens,
                start_time=start_time,
                first_token_time=first_token_time,
            )
            record_stream_metrics(stats)

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
                client=client,
            ).inc(usage["prompt_tokens"])

        if "completion_tokens" in usage:
            TOKEN_USAGE.labels(
                model=model_name,
                provider=provider.name,
                token_type="completion",
                api_key_name=api_key_name,
                client=client,
            ).inc(usage["completion_tokens"])

        if "total_tokens" in usage:
            TOKEN_USAGE.labels(
                model=model_name,
                provider=provider.name,
                token_type="total",
                api_key_name=api_key_name,
                client=client,
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
            client=client,
        ).inc(fallback_input_tokens)

        TOKEN_USAGE.labels(
            model=model_name,
            provider=provider.name,
            token_type="total",
            api_key_name=api_key_name,
            client=client,
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
        logger.error(f"Error counting tokens: {e}")
        raise HTTPException(status_code=500, detail=str(e))
