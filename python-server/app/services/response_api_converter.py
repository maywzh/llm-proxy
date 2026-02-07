"""Response API to OpenAI format conversion utilities.

This module provides bidirectional conversion between OpenAI Response API format
and OpenAI Chat Completions API format for request/response handling.

Response API is OpenAI's newer API format (/v1/responses) that supports:
- Stateful conversations with session management
- Computer use and other advanced tools
- Structured output with JSON schemas
"""

import json
import time
import uuid
from dataclasses import dataclass, field
from io import StringIO
from typing import Any, AsyncIterator, Dict, List, Optional, Union

from app.core.logging import get_logger

logger = get_logger()


# ============================================================================
# Response API Types (as dataclasses for type safety)
# ============================================================================


@dataclass
class ResponseUsage:
    """Response API usage stats."""

    input_tokens: int = 0
    output_tokens: int = 0
    total_tokens: int = 0

    def to_dict(self) -> Dict[str, int]:
        return {
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "total_tokens": self.total_tokens,
        }


@dataclass
class ResponseApiRequest:
    """Response API request structure."""

    model: str
    input: Optional[Union[str, List[Dict[str, Any]]]] = None
    instructions: Optional[str] = None
    max_output_tokens: Optional[int] = None
    temperature: Optional[float] = None
    top_p: Optional[float] = None
    tools: Optional[List[Dict[str, Any]]] = None
    tool_choice: Optional[Any] = None
    response_format: Optional[Dict[str, Any]] = None
    stream: bool = False
    extra: Dict[str, Any] = field(default_factory=dict)


@dataclass
class ResponseApiResponse:
    """Response API response structure."""

    id: str
    object: str
    created_at: int
    model: str
    output: List[Dict[str, Any]]
    status: str
    status_details: Optional[Dict[str, Any]]
    usage: ResponseUsage

    def to_dict(self) -> Dict[str, Any]:
        result = {
            "id": self.id,
            "object": self.object,
            "created_at": self.created_at,
            "model": self.model,
            "output": self.output,
            "status": self.status,
            "usage": self.usage.to_dict(),
        }
        if self.status_details is not None:
            result["status_details"] = self.status_details
        return result


# ============================================================================
# Request Conversion: Response API -> OpenAI Chat Completions
# ============================================================================


def response_api_to_openai_request(
    request: ResponseApiRequest,
) -> Dict[str, Any]:
    """Convert Response API request to OpenAI Chat Completions format.

    Args:
        request: The Response API request

    Returns:
        OpenAI-compatible request dict
    """
    messages: List[Dict[str, Any]] = []

    # Add system message from instructions
    if request.instructions:
        messages.append({"role": "system", "content": request.instructions})

    # Convert input to messages
    if request.input is not None:
        converted = _convert_input_to_messages(request.input)
        messages.extend(converted)

    # Build OpenAI request
    openai_request: Dict[str, Any] = {
        "model": request.model,
        "messages": messages,
    }

    # Add optional parameters
    if request.max_output_tokens is not None:
        openai_request["max_tokens"] = request.max_output_tokens
    if request.temperature is not None:
        openai_request["temperature"] = request.temperature
    if request.top_p is not None:
        openai_request["top_p"] = request.top_p
    if request.stream:
        openai_request["stream"] = True

    # Convert tools
    if request.tools:
        openai_tools = _convert_response_tools_to_openai(request.tools)
        if openai_tools:
            openai_request["tools"] = openai_tools

    # Pass through tool_choice
    if request.tool_choice is not None:
        openai_request["tool_choice"] = request.tool_choice

    # Pass through response_format
    if request.response_format is not None:
        openai_request["response_format"] = request.response_format

    logger.debug(
        f"Converted Response API request to OpenAI format: model={request.model}, "
        f"messages_count={len(messages)}"
    )

    return openai_request


def _convert_input_to_messages(
    input_data: Union[str, List[Dict[str, Any]]],
) -> List[Dict[str, Any]]:
    """Convert Response API input to OpenAI messages."""
    if isinstance(input_data, str):
        return [{"role": "user", "content": input_data}]

    if isinstance(input_data, list):
        messages = []
        for item in input_data:
            item_type = item.get("type")
            if item_type == "message":
                role = item.get("role", "user")
                content = item.get("content")
                openai_content = _convert_response_content_to_openai(content)
                messages.append({"role": role, "content": openai_content})
            # item_reference type is not directly supported, skip
        return messages

    return []


def _convert_response_content_to_openai(content: Any) -> Any:
    """Convert Response API content to OpenAI format."""
    if content is None:
        return ""

    if isinstance(content, str):
        return content

    if isinstance(content, list):
        openai_parts = []
        for part in content:
            part_type = part.get("type")
            if part_type in ("input_text", "output_text"):
                openai_parts.append({"type": "text", "text": part.get("text", "")})
            elif part_type == "input_image":
                openai_parts.append(
                    {
                        "type": "image_url",
                        "image_url": {"url": part.get("image_url", "")},
                    }
                )
            # tool_use and tool_result are handled separately

        if len(openai_parts) == 1 and openai_parts[0].get("type") == "text":
            return openai_parts[0].get("text", "")

        return openai_parts

    return content


def _convert_response_tools_to_openai(
    tools: List[Dict[str, Any]],
) -> List[Dict[str, Any]]:
    """Convert Response API tools to OpenAI format."""
    openai_tools = []
    for tool in tools:
        tool_type = tool.get("type")
        if tool_type == "function":
            openai_tools.append(
                {
                    "type": "function",
                    "function": {
                        "name": tool.get("name", ""),
                        "description": tool.get("description"),
                        "parameters": tool.get("parameters", {}),
                    },
                }
            )
        # Other tool types (computer_use_preview, web_search_preview, file_search)
        # are not directly supported in OpenAI Chat Completions

    return openai_tools


# ============================================================================
# Response Conversion: OpenAI Chat Completions -> Response API
# ============================================================================


def openai_to_response_api_response(
    openai_response: Dict[str, Any],
    original_model: str,
) -> ResponseApiResponse:
    """Convert OpenAI Chat Completions response to Response API format.

    Args:
        openai_response: The OpenAI API response
        original_model: The original model name from request

    Returns:
        Response API response object
    """
    response_id = openai_response.get("id") or f"resp_{uuid.uuid4().hex[:24]}"
    created_at = openai_response.get("created") or int(time.time())

    output: List[Dict[str, Any]] = []
    status = "completed"

    # Process choices
    choices = openai_response.get("choices", [])
    if choices:
        first_choice = choices[0]

        # Check finish_reason
        finish_reason = first_choice.get("finish_reason", "stop")
        status = _map_finish_reason_to_status(finish_reason)

        message = first_choice.get("message", {})
        if message:
            # Extract text content
            text_content = message.get("content")
            if text_content:
                output.append(
                    {
                        "type": "message",
                        "id": f"msg_{uuid.uuid4().hex[:24]}",
                        "role": "assistant",
                        "content": [{"type": "output_text", "text": text_content}],
                        "status": "completed",
                    }
                )

            # Extract tool_calls
            tool_calls = message.get("tool_calls", [])
            for tc in tool_calls:
                tc_id = tc.get("id", "")
                function = tc.get("function", {})
                output.append(
                    {
                        "type": "function_call",
                        "id": f"fc_{uuid.uuid4().hex[:24]}",
                        "call_id": tc_id,
                        "name": function.get("name", ""),
                        "arguments": function.get("arguments", "{}"),
                        "status": "completed",
                    }
                )

    # Extract usage
    usage_obj = openai_response.get("usage", {})
    usage = ResponseUsage(
        input_tokens=usage_obj.get("prompt_tokens", 0),
        output_tokens=usage_obj.get("completion_tokens", 0),
        total_tokens=usage_obj.get("total_tokens", 0),
    )
    if usage.total_tokens == 0:
        usage.total_tokens = usage.input_tokens + usage.output_tokens

    return ResponseApiResponse(
        id=response_id,
        object="response",
        created_at=created_at,
        model=original_model,
        output=output,
        status=status,
        status_details=None,
        usage=usage,
    )


def _map_finish_reason_to_status(reason: str) -> str:
    """Map OpenAI finish_reason to Response API status."""
    return {
        "stop": "completed",
        "length": "incomplete",
        "content_filter": "failed",
        "tool_calls": "completed",
    }.get(reason, "completed")


# ============================================================================
# Streaming Conversion: OpenAI -> Response API
# ============================================================================


@dataclass
class StreamingState:
    """State for streaming conversion using on-demand synthesis pattern."""

    response_id: str
    original_model: str
    created_at: int
    text_buffer: str = ""
    current_tool_calls: Dict[int, Dict[str, Any]] = field(default_factory=dict)
    final_status: str = "completed"
    usage: ResponseUsage = field(default_factory=ResponseUsage)
    # On-demand synthesis flags
    response_created: bool = False
    output_item_started: bool = False


async def convert_openai_streaming_to_response_api(
    openai_stream: AsyncIterator[bytes],
    original_model: str,
) -> AsyncIterator[str]:
    """Convert OpenAI streaming response to Response API streaming format.

    Uses on-demand synthesis pattern (V2 style): events are synthesized
    only when needed, rather than pre-generated unconditionally.

    Args:
        openai_stream: Async iterator of OpenAI SSE chunks (bytes)
        original_model: The original model name from request

    Yields:
        Response API formatted SSE event strings
    """
    response_id = f"resp_{uuid.uuid4().hex[:24]}"
    created_at = int(time.time())

    state = StreamingState(
        response_id=response_id,
        original_model=original_model,
        created_at=created_at,
    )

    # On-demand synthesis: no pre-generated events
    buffer = StringIO()
    stream_done = False

    try:
        async for chunk in openai_stream:
            if not chunk:
                break

            buffer.write(chunk.decode("utf-8", errors="ignore"))
            buffer_content = buffer.getvalue()

            # Process complete SSE events (delimited by \n\n)
            while "\n\n" in buffer_content:
                event_block, remaining = buffer_content.split("\n\n", 1)
                buffer = StringIO()
                buffer.write(remaining)
                buffer_content = remaining

                for line in event_block.split("\n"):
                    line = line.strip()

                    if not line or line.startswith(":"):
                        continue

                    if line.startswith("data: "):
                        chunk_data = line[6:]
                        if chunk_data.strip() == "[DONE]":
                            stream_done = True
                            break

                        try:
                            chunk_json = json.loads(chunk_data)

                            # Extract usage
                            usage = chunk_json.get("usage")
                            if usage:
                                state.usage = ResponseUsage(
                                    input_tokens=usage.get("prompt_tokens", 0),
                                    output_tokens=usage.get("completion_tokens", 0),
                                    total_tokens=usage.get("total_tokens", 0),
                                )

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
                            state.final_status = _map_finish_reason_to_status(
                                finish_reason
                            )

                        # Check for content
                        has_content = (
                            delta.get("content") is not None
                            or delta.get("tool_calls") is not None
                        )

                        # On-demand synthesis: emit response.created on first content
                        if has_content and not state.response_created:
                            yield _format_response_api_event(
                                "response.created",
                                {
                                    "type": "response.created",
                                    "response": {
                                        "id": state.response_id,
                                        "object": "response",
                                        "created_at": state.created_at,
                                        "model": state.original_model,
                                        "status": "in_progress",
                                        "output": [],
                                    },
                                },
                            )
                            state.response_created = True

                        # Handle text delta
                        content = delta.get("content")
                        if content is not None and isinstance(content, str):
                            # Emit output_item.added on first text
                            if not state.output_item_started and content:
                                yield _format_response_api_event(
                                    "response.output_item.added",
                                    {
                                        "type": "response.output_item.added",
                                        "output_index": 0,
                                        "item": {
                                            "type": "message",
                                            "id": f"msg_{uuid.uuid4().hex[:24]}",
                                            "role": "assistant",
                                            "content": [],
                                            "status": "in_progress",
                                        },
                                    },
                                )
                                yield _format_response_api_event(
                                    "response.content_part.added",
                                    {
                                        "type": "response.content_part.added",
                                        "output_index": 0,
                                        "content_index": 0,
                                        "part": {"type": "output_text", "text": ""},
                                    },
                                )
                                state.output_item_started = True

                            state.text_buffer += content

                            yield _format_response_api_event(
                                "response.output_text.delta",
                                {
                                    "type": "response.output_text.delta",
                                    "output_index": 0,
                                    "content_index": 0,
                                    "delta": content,
                                },
                            )

                        # Handle tool_calls
                        tool_calls = delta.get("tool_calls", [])
                        if tool_calls:
                            for tc_delta in tool_calls:
                                for event in _process_tool_call_for_response_api(
                                    tc_delta, state
                                ):
                                    yield event

                if stream_done:
                    break

            if stream_done:
                break

    except Exception as e:
        logger.error(f"Streaming error: {e}")
        import traceback

        logger.error(traceback.format_exc())
        yield _format_response_api_event(
            "error",
            {"type": "error", "error": {"type": "api_error", "message": str(e)}},
        )
        return

    # Generate final events (only if response was created)
    for event in _generate_response_api_final_events(state):
        yield event


def _process_tool_call_for_response_api(
    tc_delta: Dict[str, Any],
    state: StreamingState,
) -> List[str]:
    """Process tool call delta for Response API streaming."""
    events = []
    tc_index = tc_delta.get("index", 0)

    if tc_index not in state.current_tool_calls:
        state.current_tool_calls[tc_index] = {"id": "", "name": "", "arguments": ""}

    tool_call = state.current_tool_calls[tc_index]

    # Extract tool call data
    if tc_delta.get("id"):
        tool_call["id"] = tc_delta["id"]

    function = tc_delta.get("function", {})
    if function.get("name"):
        tool_call["name"] = function["name"]
        events.append(
            _format_response_api_event(
                "response.function_call_arguments.delta",
                {
                    "type": "response.function_call_arguments.delta",
                    "output_index": tc_index,
                    "call_id": tool_call["id"],
                    "delta": "",
                },
            )
        )

    if function.get("arguments"):
        tool_call["arguments"] += function["arguments"]
        events.append(
            _format_response_api_event(
                "response.function_call_arguments.delta",
                {
                    "type": "response.function_call_arguments.delta",
                    "output_index": tc_index,
                    "call_id": tool_call["id"],
                    "delta": function["arguments"],
                },
            )
        )

    return events


def _generate_response_api_final_events(state: StreamingState) -> List[str]:
    """Generate final events for Response API stream."""
    events = []

    # Only emit events if response was created
    if state.response_created:
        # Emit content_part.done if text was sent
        if state.output_item_started and state.text_buffer:
            events.append(
                _format_response_api_event(
                    "response.content_part.done",
                    {
                        "type": "response.content_part.done",
                        "output_index": 0,
                        "content_index": 0,
                        "part": {"type": "output_text", "text": state.text_buffer},
                    },
                )
            )

            events.append(
                _format_response_api_event(
                    "response.output_item.done",
                    {
                        "type": "response.output_item.done",
                        "output_index": 0,
                        "item": {
                            "type": "message",
                            "id": f"msg_{uuid.uuid4().hex[:24]}",
                            "role": "assistant",
                            "content": [
                                {"type": "output_text", "text": state.text_buffer}
                            ],
                            "status": "completed",
                        },
                    },
                )
            )

        # Emit response.done
        events.append(
            _format_response_api_event(
                "response.done",
                {
                    "type": "response.done",
                    "response": {
                        "id": state.response_id,
                        "object": "response",
                        "created_at": state.created_at,
                        "model": state.original_model,
                        "status": state.final_status,
                        "output": _build_output_for_response(state),
                        "usage": state.usage.to_dict(),
                    },
                },
            )
        )

    return events


def _build_output_for_response(state: StreamingState) -> List[Dict[str, Any]]:
    """Build output array for final response."""
    output = []

    # Add text message
    if state.text_buffer:
        output.append(
            {
                "type": "message",
                "id": f"msg_{uuid.uuid4().hex[:24]}",
                "role": "assistant",
                "content": [{"type": "output_text", "text": state.text_buffer}],
                "status": "completed",
            }
        )

    # Add tool calls
    for tc in state.current_tool_calls.values():
        output.append(
            {
                "type": "function_call",
                "id": f"fc_{uuid.uuid4().hex[:24]}",
                "call_id": tc["id"],
                "name": tc["name"],
                "arguments": tc["arguments"],
                "status": "completed",
            }
        )

    return output


def _format_response_api_event(event_type: str, data: Dict[str, Any]) -> str:
    """Format a Response API SSE event string."""
    return f"event: {event_type}\ndata: {json.dumps(data, ensure_ascii=False)}\n\n"
