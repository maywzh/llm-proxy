# Response API Protocol Transformer
#
# Handles conversion between OpenAI Response API format (/v1/responses) and
# the Unified Internal Format (UIF).
#
# Response API is OpenAI's newer API format that supports:
# - Stateful conversations with session management
# - Computer use and other advanced tools
# - Structured output with JSON schemas
# - Multiple modalities

import json
import uuid
from datetime import datetime, timezone
from typing import Any, Optional

from ..base import Transformer
from ..unified import (
    ChunkType,
    FileContent,
    ImageContent,
    Protocol,
    RefusalContent,
    Role,
    StopReason,
    TextContent,
    ToolInputDeltaContent,
    ToolResultContent,
    ToolUseContent,
    UnifiedContent,
    UnifiedMessage,
    UnifiedParameters,
    UnifiedRequest,
    UnifiedResponse,
    UnifiedStreamChunk,
    UnifiedTool,
    UnifiedToolCall,
    UnifiedUsage,
    create_image_url,
    create_refusal_content,
    create_text_content,
    create_tool_result_content,
    create_tool_use_content,
)


class ResponseApiTransformer(Transformer):
    """
    Response API protocol transformer.

    Handles bidirectional conversion between OpenAI Response API format
    and the Unified Internal Format (UIF).
    """

    @property
    def protocol(self) -> Protocol:
        """Get the protocol this transformer handles."""
        return Protocol.RESPONSE_API

    @property
    def endpoint(self) -> str:
        """Get the endpoint path for this protocol."""
        return "/v1/responses"

    # =========================================================================
    # Hook 1: transform_request_out (Client → Unified)
    # =========================================================================

    def transform_request_out(self, raw: dict[str, Any]) -> UnifiedRequest:
        """
        Transform Response API request format to Unified Internal Format.

        Args:
            raw: Raw Response API request payload

        Returns:
            UnifiedRequest in UIF format
        """
        # Convert input to messages
        messages = self._input_to_messages(raw.get("input"))

        # Convert tools
        tools = self._tools_to_unified(raw.get("tools"))

        # Build parameters - exclude known fields from extra
        known_fields = {
            "model",
            "input",
            "instructions",
            "max_output_tokens",
            "temperature",
            "top_p",
            "tools",
            "tool_choice",
            "response_format",
            "modalities",
            "stream",
        }
        extra = {k: v for k, v in raw.items() if k not in known_fields}

        parameters = UnifiedParameters(
            temperature=raw.get("temperature"),
            max_tokens=raw.get("max_output_tokens"),
            top_p=raw.get("top_p"),
            stop_sequences=None,
            stream=raw.get("stream", False),
            extra=extra,
        )

        return UnifiedRequest(
            model=raw.get("model", ""),
            messages=messages,
            system=raw.get("instructions"),
            parameters=parameters,
            tools=tools,
            tool_choice=raw.get("tool_choice"),
            client_protocol=Protocol.RESPONSE_API,
        )

    # =========================================================================
    # Hook 2: transform_request_in (Unified → Provider)
    # =========================================================================

    def transform_request_in(self, unified: UnifiedRequest) -> dict[str, Any]:
        """
        Transform Unified Internal Format to Response API request format.

        Args:
            unified: Request in UIF format

        Returns:
            Request payload in Response API format
        """
        request: dict[str, Any] = {
            "model": unified.model,
        }

        # Convert messages to input
        input_data = self._messages_to_input(unified.messages)
        if input_data is not None:
            request["input"] = input_data

        # Add instructions (system prompt)
        if unified.system:
            request["instructions"] = unified.system

        # Add optional parameters
        if unified.parameters.max_tokens is not None:
            request["max_output_tokens"] = unified.parameters.max_tokens
        if unified.parameters.temperature is not None:
            request["temperature"] = unified.parameters.temperature
        if unified.parameters.top_p is not None:
            request["top_p"] = unified.parameters.top_p
        if unified.parameters.stream:
            request["stream"] = True

        # Convert tools
        if unified.tools:
            request["tools"] = self._unified_to_tools(unified.tools)

        # Add tool_choice
        if unified.tool_choice:
            request["tool_choice"] = unified.tool_choice

        # Add extra parameters
        request.update(unified.parameters.extra)

        return request

    # =========================================================================
    # Hook 3: transform_response_in (Provider → Unified)
    # =========================================================================

    def transform_response_in(
        self, raw: dict[str, Any], original_model: str
    ) -> UnifiedResponse:
        """
        Transform Response API response to Unified Internal Format.

        Args:
            raw: Raw response payload from Response API
            original_model: Original model name from client request

        Returns:
            UnifiedResponse in UIF format
        """
        content, tool_calls = self._output_to_unified(raw.get("output", []))
        stop_reason = self._status_to_stop_reason(raw.get("status", "completed"))

        usage_data = raw.get("usage", {})
        usage = UnifiedUsage(
            input_tokens=usage_data.get("input_tokens", 0),
            output_tokens=usage_data.get("output_tokens", 0),
        )

        return UnifiedResponse(
            id=raw.get("id", ""),
            model=original_model,
            content=content,
            stop_reason=stop_reason,
            usage=usage,
            tool_calls=tool_calls,
        )

    # =========================================================================
    # Hook 4: transform_response_out (Unified → Client)
    # =========================================================================

    def transform_response_out(
        self, unified: UnifiedResponse, client_protocol: Protocol
    ) -> dict[str, Any]:
        """
        Transform Unified Internal Format to Response API response format.

        Args:
            unified: Response in UIF format
            client_protocol: Client's expected protocol

        Returns:
            Response payload in Response API format
        """
        output: list[dict[str, Any]] = []

        # Collect text content into a message
        text_content: list[dict[str, Any]] = []
        for c in unified.content:
            if isinstance(c, TextContent):
                text_content.append({"type": "output_text", "text": c.text})
            elif isinstance(c, RefusalContent):
                text_content.append({"type": "refusal", "refusal": c.reason})

        if text_content:
            output.append(
                {
                    "type": "message",
                    "id": f"msg_{uuid.uuid4().hex[:12]}",
                    "role": "assistant",
                    "content": text_content,
                    "status": "completed",
                }
            )

        # Add function calls
        for tool_call in unified.tool_calls:
            output.append(
                {
                    "type": "function_call",
                    "id": f"fc_{uuid.uuid4().hex[:12]}",
                    "call_id": tool_call.id,
                    "name": tool_call.name,
                    "arguments": json.dumps(tool_call.arguments),
                    "status": "completed",
                }
            )

        status = self._stop_reason_to_status(unified.stop_reason)

        return {
            "id": unified.id,
            "object": "response",
            "created_at": int(datetime.now(timezone.utc).timestamp()),
            "model": unified.model,
            "output": output,
            "status": status,
            "status_details": None,
            "usage": {
                "input_tokens": unified.usage.input_tokens,
                "output_tokens": unified.usage.output_tokens,
                "total_tokens": unified.usage.total_tokens(),
            },
        }

    # =========================================================================
    # Streaming: transform_stream_chunk_in
    # =========================================================================

    def transform_stream_chunk_in(self, chunk: bytes) -> list[UnifiedStreamChunk]:
        """
        Transform Response API streaming chunk to unified chunks.

        Args:
            chunk: Raw bytes from Response API stream

        Returns:
            List of UnifiedStreamChunk
        """
        chunks: list[UnifiedStreamChunk] = []

        try:
            chunk_str = chunk.decode("utf-8")
        except UnicodeDecodeError:
            return chunks

        for line in chunk_str.split("\n"):
            line = line.strip()
            if not line or line.startswith(":"):
                continue

            if line.startswith("data: "):
                data = line[6:]
                if data == "[DONE]":
                    chunks.append(UnifiedStreamChunk.message_stop())
                    continue

                try:
                    parsed = json.loads(data)
                    chunks.extend(self._parse_stream_event(parsed))
                except json.JSONDecodeError:
                    continue

        return chunks

    # =========================================================================
    # Streaming: transform_stream_chunk_out
    # =========================================================================

    def transform_stream_chunk_out(
        self, chunk: UnifiedStreamChunk, client_protocol: Protocol
    ) -> str:
        """
        Transform unified streaming chunk to Response API SSE format.

        Args:
            chunk: Unified stream chunk
            client_protocol: Client's expected protocol

        Returns:
            SSE-formatted string
        """
        if chunk.chunk_type == ChunkType.MESSAGE_START:
            if chunk.message:
                event = {
                    "type": "response.created",
                    "response": {
                        "id": chunk.message.id,
                        "object": "response",
                        "model": chunk.message.model,
                        "status": "in_progress",
                    },
                }
                return f"data: {json.dumps(event)}\n\n"
            return ""

        elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_START:
            if chunk.content_block:
                if isinstance(chunk.content_block, TextContent):
                    event = {
                        "type": "response.output_item.added",
                        "output_index": chunk.index,
                        "item": {
                            "type": "message",
                            "id": f"item_{chunk.index}",
                            "role": "assistant",
                            "content": [],
                            "status": "in_progress",
                        },
                    }
                    return f"data: {json.dumps(event)}\n\n"
                elif isinstance(chunk.content_block, ToolUseContent):
                    event = {
                        "type": "response.output_item.added",
                        "output_index": chunk.index,
                        "item": {
                            "type": "function_call",
                            "id": f"fc_{chunk.index}",
                            "call_id": chunk.content_block.id,
                            "name": chunk.content_block.name,
                            "arguments": "",
                            "status": "in_progress",
                        },
                    }
                    return f"data: {json.dumps(event)}\n\n"
            return ""

        elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_DELTA:
            if chunk.delta:
                if isinstance(chunk.delta, TextContent):
                    event = {
                        "type": "response.output_text.delta",
                        "item_id": f"item_{chunk.index}",
                        "output_index": chunk.index,
                        "delta": chunk.delta.text,
                    }
                    return f"data: {json.dumps(event)}\n\n"
                elif isinstance(chunk.delta, ToolInputDeltaContent):
                    event = {
                        "type": "response.function_call_arguments.delta",
                        "item_id": f"fc_{chunk.index}",
                        "output_index": chunk.index,
                        "delta": chunk.delta.partial_json,
                    }
                    return f"data: {json.dumps(event)}\n\n"
            return ""

        elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_STOP:
            event = {
                "type": "response.output_item.done",
                "output_index": chunk.index,
            }
            return f"data: {json.dumps(event)}\n\n"

        elif chunk.chunk_type == ChunkType.MESSAGE_DELTA:
            usage_json: Optional[dict[str, Any]] = None
            if chunk.usage:
                usage_json = {
                    "input_tokens": chunk.usage.input_tokens,
                    "output_tokens": chunk.usage.output_tokens,
                    "total_tokens": chunk.usage.total_tokens(),
                }
            event = {
                "type": "response.completed",
                "response": {
                    "status": "completed",
                    "usage": usage_json,
                },
            }
            return f"data: {json.dumps(event)}\n\n"

        elif chunk.chunk_type == ChunkType.MESSAGE_STOP:
            return "data: [DONE]\n\n"

        return ""

    # =========================================================================
    # Protocol Detection
    # =========================================================================

    def can_handle(self, raw: dict[str, Any]) -> bool:
        """
        Check if this transformer can handle the given request.

        Response API format indicators:
        - Has "input" field (string or array)
        - Has "instructions" field without "messages"
        - Has "max_output_tokens" field

        Args:
            raw: Raw request payload

        Returns:
            True if this is a Response API format request
        """
        # Response API has input field
        if "input" in raw:
            return True

        # Response API has instructions without messages
        if "instructions" in raw and "messages" not in raw:
            return True

        # Response API uses max_output_tokens instead of max_tokens
        if "max_output_tokens" in raw and "messages" not in raw:
            return True

        return False

    # =========================================================================
    # Helper Methods: Input Conversion
    # =========================================================================

    def _input_to_messages(self, input_data: Any) -> list[UnifiedMessage]:
        """Convert Response API input to unified messages."""
        if input_data is None:
            return []

        # Simple text input
        if isinstance(input_data, str):
            return [UnifiedMessage.user(input_data)]

        # Array of input items
        if isinstance(input_data, list):
            messages: list[UnifiedMessage] = []
            for item in input_data:
                if isinstance(item, dict):
                    item_type = item.get("type")
                    if item_type == "message":
                        role = Role.from_string(item.get("role", "user"))
                        content = self._content_to_unified(item.get("content"))
                        messages.append(
                            UnifiedMessage(role=role, content=content, tool_calls=[])
                        )
                    # Skip item_reference for now
            return messages

        return []

    def _content_to_unified(self, content: Any) -> list[UnifiedContent]:
        """Convert Response API content to unified content."""
        if content is None:
            return []

        # Simple text content
        if isinstance(content, str):
            return [create_text_content(content)]

        # Array of content parts
        if isinstance(content, list):
            result: list[UnifiedContent] = []
            for part in content:
                if isinstance(part, dict):
                    part_type = part.get("type")
                    if part_type in ("input_text", "output_text"):
                        result.append(create_text_content(part.get("text", "")))
                    elif part_type == "input_image":
                        result.append(create_image_url(part.get("image_url", "")))
                    elif part_type == "input_file":
                        result.append(
                            FileContent(
                                file_id=part.get("file_id", ""),
                                filename=part.get("filename"),
                            )
                        )
                    elif part_type == "tool_use":
                        try:
                            args = json.loads(part.get("arguments", "{}"))
                        except json.JSONDecodeError:
                            args = {}
                        result.append(
                            create_tool_use_content(
                                part.get("id", ""),
                                part.get("name", ""),
                                args,
                            )
                        )
                    elif part_type == "tool_result":
                        result.append(
                            create_tool_result_content(
                                part.get("tool_use_id", ""),
                                part.get("output", ""),
                                False,
                            )
                        )
            return result

        return []

    def _messages_to_input(
        self, messages: list[UnifiedMessage]
    ) -> Optional[list[dict[str, Any]]]:
        """Convert unified messages to Response API input."""
        if not messages:
            return None

        items: list[dict[str, Any]] = []
        for msg in messages:
            content = self._unified_to_content(msg.content)
            items.append(
                {
                    "type": "message",
                    "role": msg.role.value,
                    "content": content,
                }
            )

        return items

    def _unified_to_content(self, content: list[UnifiedContent]) -> Any:
        """Convert unified content to Response API content."""
        if not content:
            return ""

        # Single text content - return as string
        if len(content) == 1 and isinstance(content[0], TextContent):
            return content[0].text

        # Multiple content parts
        parts: list[dict[str, Any]] = []
        for c in content:
            if isinstance(c, TextContent):
                parts.append({"type": "input_text", "text": c.text})
            elif isinstance(c, ImageContent):
                if c.source_type == "url":
                    parts.append({"type": "input_image", "image_url": c.data})
                else:
                    # Convert base64 to data URL
                    url = f"data:{c.media_type};base64,{c.data}"
                    parts.append({"type": "input_image", "image_url": url})
            elif isinstance(c, FileContent):
                parts.append({"type": "input_file", "file_id": c.file_id})
            elif isinstance(c, ToolUseContent):
                parts.append(
                    {
                        "type": "tool_use",
                        "id": c.id,
                        "name": c.name,
                        "arguments": json.dumps(c.input),
                    }
                )
            elif isinstance(c, ToolResultContent):
                output = (
                    c.content if isinstance(c.content, str) else json.dumps(c.content)
                )
                parts.append(
                    {
                        "type": "tool_result",
                        "tool_use_id": c.tool_use_id,
                        "output": output,
                    }
                )

        return parts

    # =========================================================================
    # Helper Methods: Tool Conversion
    # =========================================================================

    def _tools_to_unified(
        self, tools: Optional[list[dict[str, Any]]]
    ) -> list[UnifiedTool]:
        """Convert Response API tools to unified tools."""
        if not tools:
            return []

        result: list[UnifiedTool] = []
        for tool in tools:
            tool_type = tool.get("type", "function")
            if tool_type == "function":
                result.append(
                    UnifiedTool(
                        name=tool.get("name", ""),
                        description=tool.get("description"),
                        input_schema=tool.get("parameters", {}),
                        tool_type="function",
                    )
                )
            elif tool_type == "computer_use_preview":
                result.append(
                    UnifiedTool(
                        name="computer_use",
                        description="Computer use capability",
                        input_schema={},
                        tool_type="computer_use_preview",
                    )
                )
            elif tool_type == "web_search_preview":
                result.append(
                    UnifiedTool(
                        name="web_search",
                        description="Web search capability",
                        input_schema={},
                        tool_type="web_search_preview",
                    )
                )
            elif tool_type == "file_search":
                result.append(
                    UnifiedTool(
                        name="file_search",
                        description="File search capability",
                        input_schema={},
                        tool_type="file_search",
                    )
                )

        return result

    def _unified_to_tools(self, tools: list[UnifiedTool]) -> list[dict[str, Any]]:
        """Convert unified tools to Response API tools."""
        result: list[dict[str, Any]] = []
        for tool in tools:
            tool_type = tool.tool_type or "function"
            if tool_type == "computer_use_preview":
                result.append({"type": "computer_use_preview"})
            elif tool_type == "web_search_preview":
                result.append({"type": "web_search_preview"})
            elif tool_type == "file_search":
                result.append({"type": "file_search"})
            else:
                result.append(
                    {
                        "type": "function",
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema,
                    }
                )

        return result

    # =========================================================================
    # Helper Methods: Output Conversion
    # =========================================================================

    def _output_to_unified(
        self, output: list[dict[str, Any]]
    ) -> tuple[list[UnifiedContent], list[UnifiedToolCall]]:
        """Convert Response API output to unified content and tool calls."""
        content: list[UnifiedContent] = []
        tool_calls: list[UnifiedToolCall] = []

        for item in output:
            item_type = item.get("type")
            if item_type == "message":
                for c in item.get("content", []):
                    content_type = c.get("type")
                    if content_type == "output_text":
                        content.append(create_text_content(c.get("text", "")))
                    elif content_type == "refusal":
                        content.append(create_refusal_content(c.get("refusal", "")))
            elif item_type == "function_call":
                try:
                    args = json.loads(item.get("arguments", "{}"))
                except json.JSONDecodeError:
                    args = {}
                content.append(
                    create_tool_use_content(
                        item.get("call_id", ""),
                        item.get("name", ""),
                        args,
                    )
                )
                tool_calls.append(
                    UnifiedToolCall(
                        id=item.get("id", ""),
                        name=item.get("name", ""),
                        arguments=args,
                    )
                )

        return content, tool_calls

    # =========================================================================
    # Helper Methods: Status/Stop Reason Conversion
    # =========================================================================

    def _status_to_stop_reason(self, status: str) -> Optional[StopReason]:
        """Convert Response API status to unified StopReason."""
        mapping = {
            "completed": StopReason.END_TURN,
            "incomplete": StopReason.MAX_TOKENS,
            "cancelled": StopReason.END_TURN,
            "failed": StopReason.CONTENT_FILTER,
        }
        return mapping.get(status, StopReason.END_TURN)

    def _stop_reason_to_status(self, reason: Optional[StopReason]) -> str:
        """Convert unified StopReason to Response API status."""
        if not reason:
            return "completed"
        mapping = {
            StopReason.END_TURN: "completed",
            StopReason.MAX_TOKENS: "incomplete",
            StopReason.LENGTH: "incomplete",
            StopReason.STOP_SEQUENCE: "completed",
            StopReason.TOOL_USE: "completed",
            StopReason.CONTENT_FILTER: "failed",
        }
        return mapping.get(reason, "completed")

    # =========================================================================
    # Helper Methods: Stream Event Parsing
    # =========================================================================

    def _parse_stream_event(self, event: dict[str, Any]) -> list[UnifiedStreamChunk]:
        """Parse Response API stream event to unified chunks."""
        chunks: list[UnifiedStreamChunk] = []
        event_type = event.get("type")

        if event_type in ("response.created", "response.in_progress"):
            # Message start event
            response = event.get("response", {})
            unified_response = UnifiedResponse(
                id=response.get("id", "resp_stream"),
                model=response.get("model", "model"),
                content=[],
                stop_reason=None,
                usage=UnifiedUsage(),
                tool_calls=[],
            )
            chunks.append(UnifiedStreamChunk.message_start(unified_response))

        elif event_type == "response.output_item.added":
            # Content block start
            index = event.get("output_index", 0)
            item = event.get("item", {})
            item_type = item.get("type")
            if item_type == "message":
                chunks.append(
                    UnifiedStreamChunk.content_block_start(
                        index, create_text_content("")
                    )
                )
            elif item_type == "function_call":
                chunks.append(
                    UnifiedStreamChunk.content_block_start(
                        index,
                        create_tool_use_content(
                            item.get("call_id", ""),
                            item.get("name", ""),
                            {},
                        ),
                    )
                )

        elif event_type == "response.content_part.added":
            # Content part added
            index = event.get("output_index", 0)
            part = event.get("part", {})
            part_type = part.get("type")
            if part_type == "output_text":
                chunks.append(
                    UnifiedStreamChunk.content_block_start(
                        index, create_text_content("")
                    )
                )

        elif event_type == "response.output_text.delta":
            # Text delta
            index = event.get("output_index", 0)
            delta = event.get("delta", "")
            chunks.append(
                UnifiedStreamChunk.content_block_delta(
                    index, create_text_content(delta)
                )
            )

        elif event_type == "response.content_part.delta":
            # Content part delta
            index = event.get("output_index", 0)
            delta = event.get("delta", {})
            delta_type = delta.get("type")
            if delta_type == "text_delta":
                chunks.append(
                    UnifiedStreamChunk.content_block_delta(
                        index, create_text_content(delta.get("text", ""))
                    )
                )

        elif event_type == "response.function_call_arguments.delta":
            # Function call arguments delta
            index = event.get("output_index", 0)
            delta = event.get("delta", "")
            chunks.append(
                UnifiedStreamChunk.content_block_delta(
                    index, ToolInputDeltaContent(index=index, partial_json=delta)
                )
            )

        elif event_type == "response.output_item.done":
            # Content block stop
            index = event.get("output_index", 0)
            chunks.append(UnifiedStreamChunk.content_block_stop(index))

        elif event_type == "response.completed":
            # Message delta with usage, then stop
            response = event.get("response", {})
            usage_data = response.get("usage", {})
            usage = UnifiedUsage(
                input_tokens=usage_data.get("input_tokens", 0),
                output_tokens=usage_data.get("output_tokens", 0),
            )
            chunks.append(UnifiedStreamChunk.message_delta(StopReason.END_TURN, usage))
            chunks.append(UnifiedStreamChunk.message_stop())

        elif event_type == "response.done":
            # Message stop
            chunks.append(UnifiedStreamChunk.message_stop())

        return chunks
