"""Claude to OpenAI format conversion utilities.

This module provides bidirectional conversion between Claude API format
and OpenAI API format for request/response handling.
"""

import json
import re
import uuid
from io import StringIO
from typing import Any, AsyncIterator, Dict, List, Optional, Union

from app.core.claude_constants import ClaudeConstants
from app.core.config import get_env_config
from app.core.logging import get_logger
from app.models.claude import ClaudeMessage, ClaudeMessagesRequest
from app.models.provider import get_mapped_model

logger = get_logger()

# Regex pattern to strip x-anthropic-billing-header prefix from system text
BILLING_HEADER_REGEX = re.compile(r"^x-anthropic-billing-header:\s*")


def _strip_billing_header(text: str) -> str:
    """Strip x-anthropic-billing-header prefix from text if present."""
    return BILLING_HEADER_REGEX.sub("", text)


def claude_to_openai_request(
    claude_request: ClaudeMessagesRequest,
    model_mapping: Optional[Dict[str, str]] = None,
) -> Dict[str, Any]:
    """Convert Claude API request format to OpenAI format.

    Args:
        claude_request: The Claude Messages API request
        model_mapping: Optional model name mapping dict

    Returns:
        OpenAI-compatible request dict
    """
    # Map model using provider's model_mapping if available (supports wildcard patterns)
    openai_model = claude_request.model
    if model_mapping:
        openai_model = get_mapped_model(claude_request.model, model_mapping)

    # Convert messages
    openai_messages: List[Dict[str, Any]] = []

    # Add system message if present
    if claude_request.system:
        system_text = _extract_system_text(claude_request.system)
        if system_text.strip():
            openai_messages.append(
                {"role": ClaudeConstants.ROLE_SYSTEM, "content": system_text.strip()}
            )

    # Process Claude messages
    i = 0
    while i < len(claude_request.messages):
        msg = claude_request.messages[i]

        if msg.role == ClaudeConstants.ROLE_USER:
            openai_message = _convert_claude_user_message(msg)
            openai_messages.append(openai_message)
        elif msg.role == ClaudeConstants.ROLE_ASSISTANT:
            openai_message = _convert_claude_assistant_message(msg)
            openai_messages.append(openai_message)

            # Check if next message contains tool results
            if i + 1 < len(claude_request.messages):
                next_msg = claude_request.messages[i + 1]
                if _is_tool_result_message(next_msg):
                    # Process tool results
                    i += 1  # Skip to tool result message
                    tool_results = _convert_claude_tool_results(next_msg)
                    openai_messages.extend(tool_results)

        i += 1

    # Clamp max_tokens to configured limits
    env_config = get_env_config()
    clamped_max_tokens = min(
        max(claude_request.max_tokens, env_config.min_tokens_limit),
        env_config.max_tokens_limit,
    )

    # Build OpenAI request
    openai_request: Dict[str, Any] = {
        "model": openai_model,
        "messages": openai_messages,
        "max_tokens": clamped_max_tokens,
        "temperature": claude_request.temperature,
        "stream": claude_request.stream,
    }

    # Add optional parameters
    if claude_request.stop_sequences:
        openai_request["stop"] = claude_request.stop_sequences
    if claude_request.top_p is not None:
        openai_request["top_p"] = claude_request.top_p

    # Convert tools
    if claude_request.tools:
        openai_tools = _convert_claude_tools(claude_request.tools)
        if openai_tools:
            openai_request["tools"] = openai_tools

    # Convert tool choice
    if claude_request.tool_choice:
        openai_request["tool_choice"] = _convert_tool_choice(claude_request.tool_choice)

    logger.debug(
        f"Converted Claude request to OpenAI format: model={openai_model}, "
        f"messages_count={len(openai_messages)}"
    )

    return openai_request


def openai_to_claude_response(
    openai_response: Dict[str, Any],
    original_model: str,
) -> Dict[str, Any]:
    """Convert OpenAI response to Claude format.

    Args:
        openai_response: The OpenAI API response
        original_model: The original Claude model name from request

    Returns:
        Claude-compatible response dict

    Raises:
        ValueError: If no choices in OpenAI response
    """
    # Extract response data
    choices = openai_response.get("choices", [])
    if not choices:
        raise ValueError("No choices in OpenAI response")

    choice = choices[0]
    message = choice.get("message", {})

    # Build Claude content blocks
    content_blocks = _build_content_blocks(message)

    # Map finish reason
    finish_reason = choice.get("finish_reason", "stop")
    stop_reason = _map_finish_reason(finish_reason)

    # Build Claude response
    claude_response = {
        "id": openai_response.get("id", f"msg_{uuid.uuid4()}"),
        "type": "message",
        "role": ClaudeConstants.ROLE_ASSISTANT,
        "model": original_model,
        "content": content_blocks,
        "stop_reason": stop_reason,
        "stop_sequence": None,
        "usage": {
            "input_tokens": openai_response.get("usage", {}).get("prompt_tokens", 0),
            "output_tokens": openai_response.get("usage", {}).get(
                "completion_tokens", 0
            ),
        },
    }

    return claude_response


def convert_claude_message_to_openai(message: Dict[str, Any]) -> Dict[str, Any]:
    """Convert a single Claude message dict to OpenAI format.

    Args:
        message: Claude message dict with role and content

    Returns:
        OpenAI-compatible message dict
    """
    role = message.get("role", "user")
    content = message.get("content")

    if role == ClaudeConstants.ROLE_USER:
        return _convert_user_message_dict(content)
    elif role == ClaudeConstants.ROLE_ASSISTANT:
        return _convert_assistant_message_dict(content)
    else:
        # Default handling for unknown roles
        return {"role": role, "content": content if isinstance(content, str) else ""}


def convert_openai_tool_calls_to_claude(
    tool_calls: List[Dict[str, Any]],
) -> List[Dict[str, Any]]:
    """Convert OpenAI tool_calls to Claude tool_use content blocks.

    Args:
        tool_calls: List of OpenAI tool call objects

    Returns:
        List of Claude tool_use content blocks
    """
    claude_blocks = []

    for tool_call in tool_calls:
        if tool_call.get("type") == ClaudeConstants.TOOL_FUNCTION:
            function_data = tool_call.get(ClaudeConstants.TOOL_FUNCTION, {})
            try:
                arguments = json.loads(function_data.get("arguments", "{}"))
            except json.JSONDecodeError:
                arguments = {"raw_arguments": function_data.get("arguments", "")}

            claude_blocks.append(
                {
                    "type": ClaudeConstants.CONTENT_TOOL_USE,
                    "id": tool_call.get("id", f"tool_{uuid.uuid4()}"),
                    "name": function_data.get("name", ""),
                    "input": arguments,
                }
            )

    return claude_blocks


async def convert_openai_streaming_to_claude(
    openai_stream: AsyncIterator[bytes],
    original_model: str,
    fallback_input_tokens: Optional[int] = None,
) -> AsyncIterator[str]:
    """Convert OpenAI streaming response to Claude streaming format.

    Uses on-demand synthesis pattern (V2 style): events are synthesized
    only when needed, rather than pre-generated unconditionally.

    Args:
        openai_stream: Async iterator of OpenAI SSE chunks (bytes)
        original_model: The original Claude model name from request
        fallback_input_tokens: Optional pre-calculated input tokens for fallback

    Yields:
        Claude-formatted SSE event strings
    """
    message_id = f"msg_{uuid.uuid4().hex[:24]}"

    # On-demand synthesis state flags
    message_started = False
    ping_emitted = False
    thinking_block_started = False
    text_block_started = False

    # Block indices
    thinking_block_index = -1  # Will be set to 0 when thinking starts
    text_block_index = 0  # Shifts to 1 if thinking block is present

    # Process streaming chunks
    tool_block_counter = 0
    current_tool_calls: Dict[int, Dict[str, Any]] = {}
    final_stop_reason = ClaudeConstants.STOP_END_TURN
    usage_data = {"input_tokens": fallback_input_tokens or 0, "output_tokens": 0}
    usage_found = False
    stream_done = False

    buffer = StringIO()
    try:
        async for chunk in openai_stream:
            # Check for empty chunk which signals end of stream
            if not chunk:
                break

            # Append chunk to buffer using StringIO for better performance
            buffer.write(chunk.decode("utf-8", errors="ignore"))

            # Process complete SSE events (delimited by \n\n)
            buffer_content = buffer.getvalue()
            while "\n\n" in buffer_content:
                event_block, remaining = buffer_content.split("\n\n", 1)
                # Reset buffer with remaining content
                buffer = StringIO()
                buffer.write(remaining)
                buffer_content = remaining

                # Process each line in the event block
                for line in event_block.split("\n"):
                    line = line.strip()

                    if not line:
                        continue

                    if line.startswith("data: "):
                        chunk_data = line[6:]
                        if chunk_data.strip() == "[DONE]":
                            stream_done = True
                            break

                        try:
                            chunk_json = json.loads(chunk_data)

                            # Extract usage if present AND valid (input_tokens > 0)
                            # Only bypass fallback if provider returns meaningful usage
                            usage = chunk_json.get("usage")
                            if usage and not usage_found:
                                extracted_usage = _extract_usage_data(usage)
                                if extracted_usage.get("input_tokens", 0) > 0:
                                    usage_data = extracted_usage
                                    usage_found = True

                            choices = chunk_json.get("choices", [])
                            if not choices:
                                continue

                        except json.JSONDecodeError as e:
                            logger.warning(
                                f"Failed to parse chunk: {chunk_data[:200]}, error: {e}"
                            )
                            continue

                        choice = choices[0]
                        delta = choice.get("delta", {})
                        finish_reason = choice.get("finish_reason")

                        # Handle finish reason
                        if finish_reason:
                            final_stop_reason = _map_finish_reason(finish_reason)

                        if not delta:
                            continue

                        # On-demand synthesis: check if we have content
                        has_content = (
                            delta.get("content") is not None
                            or delta.get("reasoning_content") is not None
                            or delta.get("tool_calls") is not None
                        )

                        # Emit message_start + ping on first content
                        if has_content and not message_started:
                            yield _format_sse_event(
                                ClaudeConstants.EVENT_MESSAGE_START,
                                {
                                    "type": ClaudeConstants.EVENT_MESSAGE_START,
                                    "message": {
                                        "id": message_id,
                                        "type": "message",
                                        "role": ClaudeConstants.ROLE_ASSISTANT,
                                        "model": original_model,
                                        "content": [],
                                        "stop_reason": None,
                                        "stop_sequence": None,
                                        "usage": {
                                            "input_tokens": usage_data["input_tokens"],
                                            "output_tokens": 0,
                                        },
                                    },
                                },
                            )
                            message_started = True

                            if not ping_emitted:
                                yield _format_sse_event(
                                    ClaudeConstants.EVENT_PING,
                                    {"type": ClaudeConstants.EVENT_PING},
                                )
                                ping_emitted = True

                        # Handle reasoning_content delta (for extended thinking)
                        reasoning_content = delta.get("reasoning_content")
                        if reasoning_content is not None:
                            # Start thinking block if not yet started
                            if not thinking_block_started:
                                thinking_block_started = True
                                thinking_block_index = 0
                                # Shift text block index to 1 since thinking is at 0
                                text_block_index = 1

                                yield _format_sse_event(
                                    ClaudeConstants.EVENT_CONTENT_BLOCK_START,
                                    {
                                        "type": ClaudeConstants.EVENT_CONTENT_BLOCK_START,
                                        "index": 0,
                                        "content_block": {
                                            "type": ClaudeConstants.CONTENT_THINKING,
                                            "thinking": "",
                                        },
                                    },
                                )

                            # Emit thinking delta
                            yield _format_sse_event(
                                ClaudeConstants.EVENT_CONTENT_BLOCK_DELTA,
                                {
                                    "type": ClaudeConstants.EVENT_CONTENT_BLOCK_DELTA,
                                    "index": thinking_block_index,
                                    "delta": {
                                        "type": ClaudeConstants.DELTA_THINKING,
                                        "thinking": reasoning_content,
                                    },
                                },
                            )

                        # Handle text delta
                        text_content = delta.get("content")
                        if text_content is not None:
                            # Accumulate output tokens if provider didn't provide usage
                            if not usage_found and text_content:
                                from app.utils.streaming import count_tokens

                                usage_data["output_tokens"] += count_tokens(
                                    text_content, original_model
                                )

                            # On-demand synthesis: emit content_block_start for text if not yet emitted
                            if not text_block_started:
                                yield _format_sse_event(
                                    ClaudeConstants.EVENT_CONTENT_BLOCK_START,
                                    {
                                        "type": ClaudeConstants.EVENT_CONTENT_BLOCK_START,
                                        "index": text_block_index,
                                        "content_block": {
                                            "type": ClaudeConstants.CONTENT_TEXT,
                                            "text": "",
                                        },
                                    },
                                )
                                text_block_started = True

                            yield _format_sse_event(
                                ClaudeConstants.EVENT_CONTENT_BLOCK_DELTA,
                                {
                                    "type": ClaudeConstants.EVENT_CONTENT_BLOCK_DELTA,
                                    "index": text_block_index,
                                    "delta": {
                                        "type": ClaudeConstants.DELTA_TEXT,
                                        "text": text_content,
                                    },
                                },
                            )

                        # Handle tool call deltas
                        if "tool_calls" in delta and delta["tool_calls"]:
                            for event in _process_tool_call_delta(
                                delta["tool_calls"],
                                current_tool_calls,
                                text_block_index,
                                tool_block_counter,
                            ):
                                if event.get("_update_counter"):
                                    tool_block_counter = event["_update_counter"]
                                else:
                                    yield _format_sse_event(
                                        event["event"], event["data"]
                                    )

                # Check if stream is done (set inside the for loop)
                if stream_done:
                    break

            # Check if stream is done (set inside the while loop)
            if stream_done:
                break

    except Exception as e:
        logger.error(f"Streaming error: {e}")
        import traceback

        logger.error(traceback.format_exc())
        error_event = {
            "type": "error",
            "error": {"type": "api_error", "message": f"Streaming error: {str(e)}"},
        }
        yield f"event: error\ndata: {json.dumps(error_event, ensure_ascii=False)}\n\n"
        return

    # Send final SSE events (only if message was started - on-demand synthesis)
    if message_started:
        # content_block_stop for thinking block (if started)
        if thinking_block_started:
            yield _format_sse_event(
                ClaudeConstants.EVENT_CONTENT_BLOCK_STOP,
                {
                    "type": ClaudeConstants.EVENT_CONTENT_BLOCK_STOP,
                    "index": thinking_block_index,
                },
            )

        # content_block_stop for text block (only if started)
        if text_block_started:
            yield _format_sse_event(
                ClaudeConstants.EVENT_CONTENT_BLOCK_STOP,
                {
                    "type": ClaudeConstants.EVENT_CONTENT_BLOCK_STOP,
                    "index": text_block_index,
                },
            )

        for tool_data in current_tool_calls.values():
            if tool_data.get("started") and tool_data.get("claude_index") is not None:
                yield _format_sse_event(
                    ClaudeConstants.EVENT_CONTENT_BLOCK_STOP,
                    {
                        "type": ClaudeConstants.EVENT_CONTENT_BLOCK_STOP,
                        "index": tool_data["claude_index"],
                    },
                )

        yield _format_sse_event(
            ClaudeConstants.EVENT_MESSAGE_DELTA,
            {
                "type": ClaudeConstants.EVENT_MESSAGE_DELTA,
                "delta": {"stop_reason": final_stop_reason, "stop_sequence": None},
                "usage": usage_data,
            },
        )

        yield _format_sse_event(
            ClaudeConstants.EVENT_MESSAGE_STOP,
            {"type": ClaudeConstants.EVENT_MESSAGE_STOP},
        )


# ============================================================================
# Private helper functions
# ============================================================================


def _extract_system_text(
    system: Union[str, List[Any]],
) -> str:
    """Extract system text from Claude system field.

    Also strips x-anthropic-billing-header prefix from text blocks.
    """
    if isinstance(system, str):
        return _strip_billing_header(system)

    if isinstance(system, list):
        text_parts = []
        for block in system:
            if hasattr(block, "type") and block.type == ClaudeConstants.CONTENT_TEXT:
                text_parts.append(_strip_billing_header(block.text))
            elif (
                isinstance(block, dict)
                and block.get("type") == ClaudeConstants.CONTENT_TEXT
            ):
                text_parts.append(_strip_billing_header(block.get("text", "")))
        return "\n\n".join(text_parts)

    return ""


def _is_tool_result_message(msg: ClaudeMessage) -> bool:
    """Check if a message contains tool results."""
    if msg.role != ClaudeConstants.ROLE_USER:
        return False
    if not isinstance(msg.content, list):
        return False
    return any(
        block.type == ClaudeConstants.CONTENT_TOOL_RESULT
        for block in msg.content
        if hasattr(block, "type")
    )


def _convert_claude_user_message(msg: ClaudeMessage) -> Dict[str, Any]:
    """Convert Claude user message to OpenAI format."""
    if msg.content is None:
        return {"role": ClaudeConstants.ROLE_USER, "content": ""}

    if isinstance(msg.content, str):
        return {"role": ClaudeConstants.ROLE_USER, "content": msg.content}

    # Handle multimodal content
    openai_content: List[Dict[str, Any]] = []
    for block in msg.content:
        if block.type == ClaudeConstants.CONTENT_TEXT:
            openai_content.append({"type": "text", "text": block.text})
        elif block.type == ClaudeConstants.CONTENT_IMAGE:
            # Convert Claude image format to OpenAI format
            image_content = _convert_image_block(block)
            if image_content:
                openai_content.append(image_content)

    if len(openai_content) == 1 and openai_content[0]["type"] == "text":
        return {"role": ClaudeConstants.ROLE_USER, "content": openai_content[0]["text"]}
    else:
        return {"role": ClaudeConstants.ROLE_USER, "content": openai_content}


def _convert_image_block(block: Any) -> Optional[Dict[str, Any]]:
    """Convert Claude image block to OpenAI format."""
    source = getattr(block, "source", None)
    if source is None:
        return None

    # Handle both Pydantic model and dict
    if hasattr(source, "type"):
        source_type = source.type
        media_type = getattr(source, "media_type", None)
        data = getattr(source, "data", None)
    elif isinstance(source, dict):
        source_type = source.get("type")
        media_type = source.get("media_type")
        data = source.get("data")
    else:
        return None

    if source_type == "base64" and media_type and data:
        return {
            "type": "image_url",
            "image_url": {"url": f"data:{media_type};base64,{data}"},
        }

    return None


def _convert_claude_assistant_message(msg: ClaudeMessage) -> Dict[str, Any]:
    """Convert Claude assistant message to OpenAI format."""
    text_parts: List[str] = []
    tool_calls: List[Dict[str, Any]] = []

    if msg.content is None:
        return {"role": ClaudeConstants.ROLE_ASSISTANT, "content": None}

    if isinstance(msg.content, str):
        return {"role": ClaudeConstants.ROLE_ASSISTANT, "content": msg.content}

    for block in msg.content:
        if block.type == ClaudeConstants.CONTENT_TEXT:
            text_parts.append(block.text)
        elif block.type == ClaudeConstants.CONTENT_TOOL_USE:
            tool_calls.append(
                {
                    "id": block.id,
                    "type": ClaudeConstants.TOOL_FUNCTION,
                    ClaudeConstants.TOOL_FUNCTION: {
                        "name": block.name,
                        "arguments": json.dumps(block.input, ensure_ascii=False),
                    },
                }
            )

    openai_message: Dict[str, Any] = {"role": ClaudeConstants.ROLE_ASSISTANT}

    # Set content
    if text_parts:
        openai_message["content"] = "".join(text_parts)
    else:
        openai_message["content"] = None

    # Set tool calls
    if tool_calls:
        openai_message["tool_calls"] = tool_calls

    return openai_message


def _convert_claude_tool_results(msg: ClaudeMessage) -> List[Dict[str, Any]]:
    """Convert Claude tool results to OpenAI format."""
    tool_messages: List[Dict[str, Any]] = []

    if isinstance(msg.content, list):
        for block in msg.content:
            if block.type == ClaudeConstants.CONTENT_TOOL_RESULT:
                content = _parse_tool_result_content(block.content)
                tool_messages.append(
                    {
                        "role": ClaudeConstants.ROLE_TOOL,
                        "tool_call_id": block.tool_use_id,
                        "content": content,
                    }
                )

    return tool_messages


def _parse_tool_result_content(content: Any) -> str:
    """Parse and normalize tool result content into a string format."""
    if content is None:
        return "No content provided"

    if isinstance(content, str):
        return content

    if isinstance(content, list):
        result_parts = []
        for item in content:
            if (
                isinstance(item, dict)
                and item.get("type") == ClaudeConstants.CONTENT_TEXT
            ):
                result_parts.append(item.get("text", ""))
            elif isinstance(item, str):
                result_parts.append(item)
            elif isinstance(item, dict):
                if "text" in item:
                    result_parts.append(item.get("text", ""))
                else:
                    try:
                        result_parts.append(json.dumps(item, ensure_ascii=False))
                    except Exception:
                        result_parts.append(str(item))
        return "\n".join(result_parts).strip()

    if isinstance(content, dict):
        if content.get("type") == ClaudeConstants.CONTENT_TEXT:
            return content.get("text", "")
        try:
            return json.dumps(content, ensure_ascii=False)
        except Exception:
            return str(content)

    try:
        return str(content)
    except Exception:
        return "Unparseable content"


def _convert_claude_tools(tools: List[Any]) -> List[Dict[str, Any]]:
    """Convert Claude tools to OpenAI format."""
    openai_tools = []
    for tool in tools:
        if tool.name and tool.name.strip():
            openai_tools.append(
                {
                    "type": ClaudeConstants.TOOL_FUNCTION,
                    ClaudeConstants.TOOL_FUNCTION: {
                        "name": tool.name,
                        "description": tool.description or "",
                        "parameters": tool.input_schema,
                    },
                }
            )
    return openai_tools


def _convert_tool_choice(tool_choice: Dict[str, Any]) -> Any:
    """Convert Claude tool_choice to OpenAI format."""
    choice_type = tool_choice.get("type")
    if choice_type == "auto":
        return "auto"
    elif choice_type == "any":
        return "auto"
    elif choice_type == "tool" and "name" in tool_choice:
        return {
            "type": ClaudeConstants.TOOL_FUNCTION,
            ClaudeConstants.TOOL_FUNCTION: {"name": tool_choice["name"]},
        }
    else:
        return "auto"


def _build_content_blocks(message: Dict[str, Any]) -> List[Dict[str, Any]]:
    """Build Claude content blocks from OpenAI message."""
    content_blocks: List[Dict[str, Any]] = []

    # Add reasoning/thinking content first (if present)
    # OpenAI-compatible APIs like Grok use reasoning_content for extended thinking
    reasoning_content = message.get("reasoning_content")
    if reasoning_content:
        content_blocks.append(
            {"type": ClaudeConstants.CONTENT_THINKING, "thinking": reasoning_content}
        )

    # Add text content
    text_content = message.get("content")
    if text_content is not None:
        content_blocks.append(
            {"type": ClaudeConstants.CONTENT_TEXT, "text": text_content}
        )

    # Add tool calls
    tool_calls = message.get("tool_calls", []) or []
    for tool_call in tool_calls:
        if tool_call.get("type") == ClaudeConstants.TOOL_FUNCTION:
            function_data = tool_call.get(ClaudeConstants.TOOL_FUNCTION, {})
            try:
                arguments = json.loads(function_data.get("arguments", "{}"))
            except json.JSONDecodeError:
                arguments = {"raw_arguments": function_data.get("arguments", "")}

            content_blocks.append(
                {
                    "type": ClaudeConstants.CONTENT_TOOL_USE,
                    "id": tool_call.get("id", f"tool_{uuid.uuid4()}"),
                    "name": function_data.get("name", ""),
                    "input": arguments,
                }
            )

    # Ensure at least one content block
    if not content_blocks:
        content_blocks.append({"type": ClaudeConstants.CONTENT_TEXT, "text": ""})

    return content_blocks


def _map_finish_reason(finish_reason: str) -> str:
    """Map OpenAI finish reason to Claude stop reason."""
    return {
        "stop": ClaudeConstants.STOP_END_TURN,
        "length": ClaudeConstants.STOP_MAX_TOKENS,
        "tool_calls": ClaudeConstants.STOP_TOOL_USE,
        "function_call": ClaudeConstants.STOP_TOOL_USE,
    }.get(finish_reason, ClaudeConstants.STOP_END_TURN)


def _convert_user_message_dict(content: Any) -> Dict[str, Any]:
    """Convert user message content dict to OpenAI format."""
    if content is None:
        return {"role": ClaudeConstants.ROLE_USER, "content": ""}

    if isinstance(content, str):
        return {"role": ClaudeConstants.ROLE_USER, "content": content}

    if isinstance(content, list):
        openai_content = []
        for block in content:
            if isinstance(block, dict):
                block_type = block.get("type")
                if block_type == ClaudeConstants.CONTENT_TEXT:
                    openai_content.append(
                        {"type": "text", "text": block.get("text", "")}
                    )
                elif block_type == ClaudeConstants.CONTENT_IMAGE:
                    source = block.get("source", {})
                    if (
                        source.get("type") == "base64"
                        and "media_type" in source
                        and "data" in source
                    ):
                        openai_content.append(
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": f"data:{source['media_type']};base64,{source['data']}"
                                },
                            }
                        )

        if len(openai_content) == 1 and openai_content[0]["type"] == "text":
            return {
                "role": ClaudeConstants.ROLE_USER,
                "content": openai_content[0]["text"],
            }
        return {"role": ClaudeConstants.ROLE_USER, "content": openai_content}

    return {"role": ClaudeConstants.ROLE_USER, "content": ""}


def _convert_assistant_message_dict(content: Any) -> Dict[str, Any]:
    """Convert assistant message content dict to OpenAI format."""
    if content is None:
        return {"role": ClaudeConstants.ROLE_ASSISTANT, "content": None}

    if isinstance(content, str):
        return {"role": ClaudeConstants.ROLE_ASSISTANT, "content": content}

    if isinstance(content, list):
        text_parts = []
        tool_calls = []

        for block in content:
            if isinstance(block, dict):
                block_type = block.get("type")
                if block_type == ClaudeConstants.CONTENT_TEXT:
                    text_parts.append(block.get("text", ""))
                elif block_type == ClaudeConstants.CONTENT_TOOL_USE:
                    tool_calls.append(
                        {
                            "id": block.get("id", f"tool_{uuid.uuid4()}"),
                            "type": ClaudeConstants.TOOL_FUNCTION,
                            ClaudeConstants.TOOL_FUNCTION: {
                                "name": block.get("name", ""),
                                "arguments": json.dumps(
                                    block.get("input", {}), ensure_ascii=False
                                ),
                            },
                        }
                    )

        openai_message: Dict[str, Any] = {"role": ClaudeConstants.ROLE_ASSISTANT}
        openai_message["content"] = "".join(text_parts) if text_parts else None
        if tool_calls:
            openai_message["tool_calls"] = tool_calls

        return openai_message

    return {"role": ClaudeConstants.ROLE_ASSISTANT, "content": None}


def _format_sse_event(event_type: str, data: Dict[str, Any]) -> str:
    """Format an SSE event string."""
    return f"event: {event_type}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n"


def _extract_usage_data(usage: Dict[str, Any]) -> Dict[str, Any]:
    """Extract usage data from OpenAI response."""
    cache_read_input_tokens = 0
    prompt_tokens_details = usage.get("prompt_tokens_details", {})
    if prompt_tokens_details:
        cache_read_input_tokens = prompt_tokens_details.get("cached_tokens", 0)

    return {
        "input_tokens": usage.get("prompt_tokens", 0),
        "output_tokens": usage.get("completion_tokens", 0),
        "cache_read_input_tokens": cache_read_input_tokens,
    }


def _process_tool_call_delta(
    tool_call_deltas: List[Dict[str, Any]],
    current_tool_calls: Dict[int, Dict[str, Any]],
    text_block_index: int,
    tool_block_counter: int,
) -> List[Dict[str, Any]]:
    """Process tool call deltas and yield SSE events.

    Returns a list of events to yield, including counter updates.
    """
    events = []

    for tc_delta in tool_call_deltas:
        tc_index = tc_delta.get("index", 0)

        # Initialize tool call tracking by index if not exists
        if tc_index not in current_tool_calls:
            current_tool_calls[tc_index] = {
                "id": None,
                "name": None,
                "args_buffer": "",
                "json_sent": False,
                "claude_index": None,
                "started": False,
            }

        tool_call = current_tool_calls[tc_index]

        # Update tool call ID if provided
        if tc_delta.get("id"):
            tool_call["id"] = tc_delta["id"]

        # Update function name
        function_data = tc_delta.get(ClaudeConstants.TOOL_FUNCTION, {})
        if function_data.get("name"):
            tool_call["name"] = function_data["name"]

        # Start content block when we have complete initial data
        if tool_call["id"] and tool_call["name"] and not tool_call["started"]:
            tool_block_counter += 1
            claude_index = text_block_index + tool_block_counter
            tool_call["claude_index"] = claude_index
            tool_call["started"] = True

            events.append({"_update_counter": tool_block_counter})
            events.append(
                {
                    "event": ClaudeConstants.EVENT_CONTENT_BLOCK_START,
                    "data": {
                        "type": ClaudeConstants.EVENT_CONTENT_BLOCK_START,
                        "index": claude_index,
                        "content_block": {
                            "type": ClaudeConstants.CONTENT_TOOL_USE,
                            "id": tool_call["id"],
                            "name": tool_call["name"],
                            "input": {},
                        },
                    },
                }
            )

        # Handle function arguments
        if (
            "arguments" in function_data
            and tool_call["started"]
            and function_data["arguments"] is not None
        ):
            tool_call["args_buffer"] += function_data["arguments"]

            # Try to parse complete JSON and send delta
            try:
                json.loads(tool_call["args_buffer"])
                if not tool_call["json_sent"]:
                    events.append(
                        {
                            "event": ClaudeConstants.EVENT_CONTENT_BLOCK_DELTA,
                            "data": {
                                "type": ClaudeConstants.EVENT_CONTENT_BLOCK_DELTA,
                                "index": tool_call["claude_index"],
                                "delta": {
                                    "type": ClaudeConstants.DELTA_INPUT_JSON,
                                    "partial_json": tool_call["args_buffer"],
                                },
                            },
                        }
                    )
                    tool_call["json_sent"] = True
            except json.JSONDecodeError:
                pass

    return events
