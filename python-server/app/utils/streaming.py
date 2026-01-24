"""Streaming response utilities"""

import asyncio
import json
import time
from datetime import datetime, timezone
from typing import TYPE_CHECKING, AsyncIterator, Optional, Dict, Any

import httpx
import tiktoken
from fastapi.responses import StreamingResponse

from app.core.config import get_config
from app.core.exceptions import TTFTTimeoutError
from app.core.metrics import TOKEN_USAGE, TTFT, TOKENS_PER_SECOND
from app.core.logging import (
    set_provider_context,
    clear_provider_context,
    get_logger,
    get_api_key_name,
)

if TYPE_CHECKING:
    from app.services.langfuse_service import GenerationData


def count_tokens(text: str, model: str) -> int:
    """Count tokens in text using tiktoken"""
    try:
        encoding = tiktoken.encoding_for_model(model)
    except KeyError:
        # Fallback to cl100k_base for unknown models
        encoding = tiktoken.get_encoding("cl100k_base")
    return len(encoding.encode(text))


def calculate_message_tokens(messages: list, model: str) -> int:
    """Calculate tokens for a list of messages including format overhead"""
    try:
        encoding = tiktoken.encoding_for_model(model)
    except KeyError:
        encoding = tiktoken.get_encoding("cl100k_base")

    total_tokens = 0
    for message in messages:
        # Count tokens in content
        content = message.get("content", "")
        if isinstance(content, str):
            total_tokens += len(encoding.encode(content))
        elif isinstance(content, list):
            # Handle multi-modal content
            for item in content:
                if isinstance(item, dict) and item.get("type") == "text":
                    total_tokens += len(encoding.encode(item.get("text", "")))

        # Add tokens for role and other fields
        total_tokens += len(encoding.encode(message.get("role", "")))
        if "name" in message:
            total_tokens += len(encoding.encode(message["name"]))

        # Format overhead per message
        total_tokens += 4

    # Conversation format overhead
    total_tokens += 2

    return total_tokens


async def rewrite_sse_chunk(chunk: bytes, original_model: Optional[str]) -> bytes:
    """Rewrite model field in SSE chunk"""
    if not original_model:
        return chunk

    chunk_str = chunk.decode("utf-8", errors="ignore")
    if '"model":' not in chunk_str:
        return chunk

    lines = chunk_str.split("\n")
    rewritten_lines = []

    for line in lines:
        if line.startswith("data: ") and line != "data: [DONE]":
            try:
                json_str = line[6:]
                json_obj = json.loads(json_str)
                if "model" in json_obj:
                    json_obj["model"] = original_model
                rewritten_lines.append(
                    "data: " + json.dumps(json_obj, separators=(", ", ": "))
                )
            except:
                rewritten_lines.append(line)
        else:
            rewritten_lines.append(line)

    return "\n".join(rewritten_lines).encode("utf-8")


async def stream_response(
    response: httpx.Response,
    original_model: Optional[str],
    provider_name: str,
    request_data: Optional[Dict[str, Any]] = None,
    ttft_timeout_secs: Optional[int] = None,
    generation_data: Optional["GenerationData"] = None,
) -> AsyncIterator[bytes]:
    """Stream response from provider with model rewriting and token tracking

    Args:
        response: HTTP response from provider
        original_model: Original model name from request
        provider_name: Provider name for metrics
        request_data: Original request data for fallback token counting
        ttft_timeout_secs: Time To First Token timeout in seconds. If None or 0, disabled.
        generation_data: Optional Langfuse generation data for tracing
    """
    logger = get_logger()
    api_key_name = get_api_key_name()

    # Import langfuse service here to avoid circular imports
    from app.services.langfuse_service import get_langfuse_service

    langfuse_service = get_langfuse_service()

    # Calculate input tokens for fallback
    input_tokens = 0
    if request_data:
        model_for_counting = original_model or "gpt-3.5-turbo"
        messages = request_data.get("messages", [])
        if messages:
            input_tokens = calculate_message_tokens(messages, model_for_counting)
            logger.debug(f"Calculated input tokens: {input_tokens}")

    # Track output tokens and usage status
    output_tokens = 0
    usage_found = False
    model_for_counting = original_model or "gpt-3.5-turbo"

    # Performance tracking
    start_time = time.time()
    provider_first_token_time: Optional[float] = None
    token_count = 0

    # Track accumulated output for Langfuse
    accumulated_output: list[str] = []
    finish_reason: Optional[str] = None

    try:
        # Set provider context for logging
        set_provider_context(provider_name)

        # Create async iterator for response bytes
        response_iter = response.aiter_bytes()
        first_chunk_received = False

        # Apply TTFT timeout for the first chunk if configured
        ttft_enabled = ttft_timeout_secs is not None and ttft_timeout_secs > 0

        while True:
            try:
                if not first_chunk_received and ttft_enabled:
                    try:
                        chunk = await asyncio.wait_for(
                            response_iter.__anext__(), timeout=float(ttft_timeout_secs)
                        )
                    except asyncio.TimeoutError:
                        raise TTFTTimeoutError(ttft_timeout_secs, provider_name)
                else:
                    chunk = await response_iter.__anext__()
            except StopAsyncIteration:
                break

            first_chunk_received = True
            now = time.time()

            # Track token usage from streaming chunks
            chunk_str = chunk.decode("utf-8", errors="ignore")

            # Record provider TTFT on first token from provider
            if provider_first_token_time is None:
                provider_first_token_time = now
                provider_ttft = now - start_time
                TTFT.labels(
                    source="provider",
                    model=original_model or "unknown",
                    provider=provider_name,
                ).observe(provider_ttft)
                logger.debug(f"Provider TTFT: {provider_ttft:.3f}s")

                # Capture TTFT for Langfuse
                if generation_data:
                    generation_data.ttft_time = datetime.now(timezone.utc)

            # Try to extract usage from provider (preferred method)
            # Only process complete lines to avoid parsing incomplete JSON
            if '"usage":' in chunk_str and "\n" in chunk_str:
                try:
                    lines = chunk_str.split("\n")
                    for line in lines:
                        # Only process complete SSE data lines
                        if line.startswith("data: ") and line != "data: [DONE]":
                            json_str = line[6:].strip()
                            # Skip if line appears incomplete (no closing brace)
                            if not json_str or not json_str.endswith("}"):
                                continue

                            try:
                                json_obj = json.loads(json_str)
                                if "usage" in json_obj and json_obj["usage"]:
                                    usage = json_obj["usage"]
                                    model_name = original_model or "unknown"

                                    if "prompt_tokens" in usage:
                                        TOKEN_USAGE.labels(
                                            model=model_name,
                                            provider=provider_name,
                                            token_type="prompt",
                                            api_key_name=api_key_name,
                                        ).inc(usage["prompt_tokens"])

                                    if "completion_tokens" in usage:
                                        TOKEN_USAGE.labels(
                                            model=model_name,
                                            provider=provider_name,
                                            token_type="completion",
                                            api_key_name=api_key_name,
                                        ).inc(usage["completion_tokens"])

                                    if "total_tokens" in usage:
                                        TOKEN_USAGE.labels(
                                            model=model_name,
                                            provider=provider_name,
                                            token_type="total",
                                            api_key_name=api_key_name,
                                        ).inc(usage["total_tokens"])

                                    usage_found = True

                                    # Capture usage for Langfuse
                                    if generation_data:
                                        generation_data.prompt_tokens = usage.get(
                                            "prompt_tokens", 0
                                        )
                                        generation_data.completion_tokens = usage.get(
                                            "completion_tokens", 0
                                        )
                                        generation_data.total_tokens = usage.get(
                                            "total_tokens", 0
                                        )

                                    logger.debug(
                                        f"Token usage from provider - "
                                        f"model={model_name} provider={provider_name} key={api_key_name} "
                                        f"prompt={usage.get('prompt_tokens', 0)} "
                                        f"completion={usage.get('completion_tokens', 0)} "
                                        f"total={usage.get('total_tokens', 0)}"
                                    )
                            except json.JSONDecodeError:
                                # Skip incomplete JSON lines silently
                                pass
                except Exception as e:
                    # Log unexpected errors only
                    logger.debug(f"Error processing usage chunk: {e}")

            # Accumulate output tokens for fallback (only if usage not found yet)
            if not usage_found and request_data:
                try:
                    lines = chunk_str.split("\n")
                    for line in lines:
                        if line.startswith("data: ") and line != "data: [DONE]":
                            json_str = line[6:]
                            json_obj = json.loads(json_str)
                            if "choices" in json_obj:
                                for choice in json_obj["choices"]:
                                    delta = choice.get("delta", {})
                                    content = delta.get("content", "")
                                    if content:
                                        output_tokens += count_tokens(
                                            content, model_for_counting
                                        )
                except Exception as e:
                    logger.debug(f"Failed to count tokens from content: {e}")

            # Count tokens in this chunk for TPS calculation and accumulate output for Langfuse
            try:
                lines = chunk_str.split("\n")
                for line in lines:
                    if line.startswith("data: ") and line != "data: [DONE]":
                        json_str = line[6:]
                        json_obj = json.loads(json_str)
                        if "choices" in json_obj:
                            for choice in json_obj["choices"]:
                                delta = choice.get("delta", {})
                                content = delta.get("content", "")
                                if content:
                                    token_count += count_tokens(
                                        content, model_for_counting
                                    )
                                    # Accumulate output for Langfuse
                                    if generation_data:
                                        accumulated_output.append(content)
                                # Capture finish_reason for Langfuse
                                if choice.get("finish_reason"):
                                    finish_reason = choice.get("finish_reason")
            except Exception as e:
                logger.debug(f"Failed to count tokens for TPS: {e}")

            yield await rewrite_sse_chunk(chunk, original_model)

        # If no usage was provided by provider, use calculated values (fallback)
        if not usage_found and request_data:
            model_name = original_model or "unknown"
            total_tokens = input_tokens + output_tokens

            TOKEN_USAGE.labels(
                model=model_name,
                provider=provider_name,
                token_type="prompt",
                api_key_name=api_key_name,
            ).inc(input_tokens)

            TOKEN_USAGE.labels(
                model=model_name,
                provider=provider_name,
                token_type="completion",
                api_key_name=api_key_name,
            ).inc(output_tokens)

            TOKEN_USAGE.labels(
                model=model_name,
                provider=provider_name,
                token_type="total",
                api_key_name=api_key_name,
            ).inc(total_tokens)

            # Capture fallback usage for Langfuse
            if generation_data:
                generation_data.prompt_tokens = input_tokens
                generation_data.completion_tokens = output_tokens
                generation_data.total_tokens = total_tokens

            logger.info(
                f"Token usage calculated (fallback) - "
                f"model={model_name} provider={provider_name} key={api_key_name} "
                f"prompt={input_tokens} completion={output_tokens} total={total_tokens}"
            )

        # Calculate and record TPS metrics
        end_time = time.time()
        if token_count > 0:
            # Provider TPS: from first token to last token
            if provider_first_token_time is not None:
                provider_duration = end_time - provider_first_token_time
                if provider_duration > 0:
                    provider_tps = token_count / provider_duration
                    TOKENS_PER_SECOND.labels(
                        source="provider",
                        model=original_model or "unknown",
                        provider=provider_name,
                    ).observe(provider_tps)
                    logger.debug(f"Provider TPS: {provider_tps:.2f} tokens/s")

        # Finalize Langfuse generation data
        if generation_data:
            generation_data.output_content = "".join(accumulated_output)
            generation_data.finish_reason = finish_reason
            generation_data.end_time = datetime.now(timezone.utc)
            langfuse_service.trace_generation(generation_data)

    except TTFTTimeoutError:
        # Record error in Langfuse
        if generation_data:
            generation_data.is_error = True
            generation_data.error_message = f"TTFT timeout: first token not received within {ttft_timeout_secs} seconds"
            generation_data.end_time = datetime.now(timezone.utc)
            langfuse_service.trace_generation(generation_data)
        raise
    except httpx.RemoteProtocolError as e:
        # Handle connection closed by remote server during streaming
        logger.error(
            f"Remote protocol error during streaming from provider {provider_name}: {str(e)} - "
            f"Provider closed connection unexpectedly"
        )

        # Record error in Langfuse
        if generation_data:
            generation_data.is_error = True
            generation_data.error_message = (
                f"Provider {provider_name} closed connection unexpectedly"
            )
            generation_data.output_content = "".join(accumulated_output)
            generation_data.end_time = datetime.now(timezone.utc)
            langfuse_service.trace_generation(generation_data)

        # Send error event to client using SSE format
        error_message = f"Provider {provider_name} closed connection unexpectedly"
        error_event = json.dumps(
            {
                "error": {
                    "message": error_message,
                    "type": "provider_disconnected",
                    "code": "remote_protocol_error",
                }
            }
        )
        yield f"event: error\ndata: {error_event}\n\n".encode("utf-8")
    except Exception as e:
        # Handle any unexpected errors during streaming
        error_detail = str(e)
        logger.exception(
            f"Unexpected error during streaming from provider {provider_name}"
        )

        # Record error in Langfuse
        if generation_data:
            generation_data.is_error = True
            generation_data.error_message = error_detail
            generation_data.output_content = "".join(accumulated_output)
            generation_data.end_time = datetime.now(timezone.utc)
            langfuse_service.trace_generation(generation_data)

        # Send error event to client using SSE format
        error_event = json.dumps(
            {
                "error": {
                    "message": error_detail,
                    "type": "stream_error",
                    "code": "internal_error",
                }
            }
        )
        yield f"event: error\ndata: {error_event}\n\n".encode("utf-8")
    finally:
        # Clear provider context after streaming completes
        clear_provider_context()


def create_streaming_response(
    response: httpx.Response,
    original_model: Optional[str],
    provider_name: str,
    request_data: Optional[Dict[str, Any]] = None,
    ttft_timeout_secs: Optional[int] = None,
    generation_data: Optional["GenerationData"] = None,
) -> StreamingResponse:
    """Create streaming response with proper cleanup

    Args:
        response: HTTP response from provider
        original_model: Original model name from request
        provider_name: Provider name for metrics
        request_data: Original request data for fallback token counting
        ttft_timeout_secs: Time To First Token timeout in seconds. If None or 0, disabled.
        generation_data: Optional Langfuse generation data for tracing
    """
    return StreamingResponse(
        stream_response(
            response,
            original_model,
            provider_name,
            request_data,
            ttft_timeout_secs,
            generation_data,
        ),
        media_type="text/event-stream",
    )


def rewrite_model_in_response(
    response_data: dict, original_model: Optional[str]
) -> dict:
    """Rewrite model field in non-streaming response"""
    if original_model and "model" in response_data:
        response_data["model"] = original_model
    return response_data
