# Anthropic Protocol Transformer
#
# Handles conversion between Anthropic Messages API format and
# the Unified Internal Format (UIF).

import json
import re
from typing import Any, Optional

from ..base import Transformer
from ..unified import (
    ChunkType,
    ImageContent,
    Protocol,
    Role,
    StopReason,
    TextContent,
    ThinkingContent,
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
    create_image_base64,
    create_image_url,
    create_text_content,
    create_thinking_content,
    create_tool_result_content,
    create_tool_use_content,
)

# Regex pattern to strip x-anthropic-billing-header prefix from system text
BILLING_HEADER_REGEX = re.compile(r"^x-anthropic-billing-header:\s*")


def strip_billing_header(text: str) -> str:
    """Strip x-anthropic-billing-header prefix from text if present."""
    return BILLING_HEADER_REGEX.sub("", text)


def is_bedrock_claude_model(model: str) -> bool:
    """
    Check if the model is a Bedrock Claude model.
    Bedrock Claude models have prefix "claude-" and suffix "-bedrock".
    """
    return model.startswith("claude-") and model.endswith("-bedrock")


def messages_contain_tool_content(messages: list[UnifiedMessage]) -> bool:
    """Check if messages contain tool_use or tool_result content blocks."""
    for msg in messages:
        for content in msg.content:
            if isinstance(content, (ToolUseContent, ToolResultContent)):
                return True
    return False


def create_placeholder_tool() -> UnifiedTool:
    """Create a placeholder tool for Bedrock compatibility."""
    return UnifiedTool(
        name="_placeholder_tool",
        description="Placeholder tool for Bedrock compatibility",
        input_schema={"type": "object", "properties": {}},
        tool_type="function",
    )


class AnthropicTransformer(Transformer):
    """
    Anthropic protocol transformer.

    Handles bidirectional conversion between Anthropic Messages API format
    and the Unified Internal Format (UIF).
    """

    @property
    def protocol(self) -> Protocol:
        """Get the protocol this transformer handles."""
        return Protocol.ANTHROPIC

    @property
    def endpoint(self) -> str:
        """Get the endpoint path for this protocol."""
        return "/v1/messages"

    # =========================================================================
    # Hook 1: transform_request_out (Client → Unified)
    # =========================================================================

    def transform_request_out(self, raw: dict[str, Any]) -> UnifiedRequest:
        """
        Transform Anthropic request format to Unified Internal Format.

        Args:
            raw: Raw Anthropic request payload

        Returns:
            UnifiedRequest in UIF format
        """
        messages: list[UnifiedMessage] = []

        for msg in raw.get("messages", []):
            messages.append(self._message_to_unified(msg))

        # Extract system prompt
        system = self._extract_system(raw.get("system"))

        # Parse tools
        tools = [
            UnifiedTool(
                name=t["name"],
                description=t.get("description"),
                input_schema=t.get("input_schema", {}),
                tool_type="function",
            )
            for t in raw.get("tools", [])
        ]

        # Build parameters - extract thinking config to extra
        extra: dict[str, Any] = {}
        if "thinking" in raw:
            extra["thinking"] = raw["thinking"]
        if "metadata" in raw:
            extra["metadata"] = raw["metadata"]

        parameters = UnifiedParameters(
            temperature=raw.get("temperature"),
            max_tokens=raw.get("max_tokens"),
            top_p=raw.get("top_p"),
            top_k=raw.get("top_k"),
            stop_sequences=raw.get("stop_sequences"),
            stream=raw.get("stream", False),
            extra=extra,
        )

        return UnifiedRequest(
            model=raw.get("model", ""),
            messages=messages,
            system=system,
            parameters=parameters,
            tools=tools,
            tool_choice=raw.get("tool_choice"),
            client_protocol=Protocol.ANTHROPIC,
            metadata=raw.get("metadata", {}),
        )

    # =========================================================================
    # Hook 2: transform_request_in (Unified → Provider)
    # =========================================================================

    def transform_request_in(self, unified: UnifiedRequest) -> dict[str, Any]:
        """
        Transform Unified Internal Format to Anthropic request format.

        Args:
            unified: Request in UIF format

        Returns:
            Request payload in Anthropic format
        """
        messages: list[dict[str, Any]] = []

        for msg in unified.messages:
            messages.append(self._unified_to_message(msg))

        # Build tools with Bedrock compatibility
        tools: Optional[list[dict[str, Any]]] = None
        if unified.tools:
            tools = [
                {
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema,
                }
                for t in unified.tools
            ]
        elif is_bedrock_claude_model(unified.model) and messages_contain_tool_content(
            unified.messages
        ):
            # Inject placeholder tool for Bedrock compatibility
            placeholder = create_placeholder_tool()
            tools = [
                {
                    "name": placeholder.name,
                    "description": placeholder.description,
                    "input_schema": placeholder.input_schema,
                }
            ]

        request: dict[str, Any] = {
            "model": unified.model,
            "max_tokens": unified.parameters.max_tokens or 4096,
            "messages": messages,
        }

        # Add system prompt
        if unified.system:
            request["system"] = unified.system

        # Add optional parameters
        if unified.parameters.temperature is not None:
            request["temperature"] = unified.parameters.temperature
        if unified.parameters.top_p is not None:
            request["top_p"] = unified.parameters.top_p
        if unified.parameters.top_k is not None:
            request["top_k"] = unified.parameters.top_k
        if unified.parameters.stop_sequences:
            request["stop_sequences"] = unified.parameters.stop_sequences
        if unified.parameters.stream:
            request["stream"] = True

        # Add tools
        if tools:
            request["tools"] = tools

        # Add tool_choice
        if unified.tool_choice:
            request["tool_choice"] = unified.tool_choice

        # Add thinking config from extra
        if "thinking" in unified.parameters.extra:
            request["thinking"] = unified.parameters.extra["thinking"]

        # Add metadata
        if unified.metadata:
            request["metadata"] = unified.metadata

        return request

    # =========================================================================
    # Hook 3: transform_response_in (Provider → Unified)
    # =========================================================================

    def transform_response_in(
        self, raw: dict[str, Any], original_model: str
    ) -> UnifiedResponse:
        """
        Transform Anthropic response to Unified Internal Format.

        Args:
            raw: Raw response payload from Anthropic
            original_model: Original model name from client request

        Returns:
            UnifiedResponse in UIF format
        """
        content: list[UnifiedContent] = []
        tool_calls: list[UnifiedToolCall] = []

        for block in raw.get("content", []):
            unified_content = self._content_block_to_unified(block)
            content.append(unified_content)

            # Extract tool calls from tool_use content
            if isinstance(unified_content, ToolUseContent):
                tool_calls.append(
                    UnifiedToolCall(
                        id=unified_content.id,
                        name=unified_content.name,
                        arguments=unified_content.input,
                    )
                )

        stop_reason = self._stop_reason_to_unified(raw.get("stop_reason"))

        usage_data = raw.get("usage", {})
        usage = UnifiedUsage(
            input_tokens=usage_data.get("input_tokens", 0),
            output_tokens=usage_data.get("output_tokens", 0),
            cache_read_tokens=usage_data.get("cache_read_input_tokens"),
            cache_write_tokens=usage_data.get("cache_creation_input_tokens"),
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
        Transform Unified Internal Format to Anthropic response format.

        Args:
            unified: Response in UIF format
            client_protocol: Client's expected protocol

        Returns:
            Response payload in Anthropic format
        """
        content: list[dict[str, Any]] = []

        # Convert content blocks
        for c in unified.content:
            block = self._unified_to_content_block(c)
            if block:
                content.append(block)

        # Convert tool_calls to Anthropic tool_use content blocks
        for tc in unified.tool_calls:
            content.append(
                {
                    "type": "tool_use",
                    "id": tc.id,
                    "name": tc.name,
                    "input": tc.arguments,
                }
            )

        stop_reason = self._unified_to_stop_reason(unified.stop_reason)

        return {
            "id": unified.id,
            "type": "message",
            "role": "assistant",
            "content": content,
            "model": unified.model,
            "stop_reason": stop_reason,
            "stop_sequence": None,
            "usage": {
                "input_tokens": unified.usage.input_tokens,
                "output_tokens": unified.usage.output_tokens,
                "cache_creation_input_tokens": unified.usage.cache_write_tokens,
                "cache_read_input_tokens": unified.usage.cache_read_tokens,
            },
        }

    # =========================================================================
    # Streaming: transform_stream_chunk_in
    # =========================================================================

    def transform_stream_chunk_in(self, chunk: bytes) -> list[UnifiedStreamChunk]:
        """
        Transform Anthropic streaming chunk to unified chunks.

        Args:
            chunk: Raw bytes from Anthropic stream

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
        Transform unified streaming chunk to Anthropic SSE format.

        Args:
            chunk: Unified stream chunk
            client_protocol: Client's expected protocol

        Returns:
            SSE-formatted string
        """
        event: dict[str, Any] = {}
        event_name = ""

        if chunk.chunk_type == ChunkType.MESSAGE_START:
            event_name = "message_start"
            if chunk.message:
                content: list[dict[str, Any]] = []
                for c in chunk.message.content:
                    block = self._unified_to_content_block(c)
                    if block:
                        content.append(block)

                event = {
                    "type": "message_start",
                    "message": {
                        "id": chunk.message.id,
                        "type": "message",
                        "role": "assistant",
                        "model": chunk.message.model,
                        "content": content,
                        "stop_reason": None,
                        "stop_sequence": None,
                        "usage": {
                            "input_tokens": chunk.message.usage.input_tokens,
                            "output_tokens": chunk.message.usage.output_tokens,
                        },
                    },
                }
            else:
                return ""

        elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_START:
            event_name = "content_block_start"
            if chunk.content_block:
                block = self._unified_to_content_block(chunk.content_block)
                if block:
                    event = {
                        "type": "content_block_start",
                        "index": chunk.index,
                        "content_block": block,
                    }
                else:
                    return ""
            else:
                return ""

        elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_DELTA:
            event_name = "content_block_delta"
            if chunk.delta:
                delta_json = self._unified_to_delta(chunk.delta)
                if delta_json:
                    event = {
                        "type": "content_block_delta",
                        "index": chunk.index,
                        "delta": delta_json,
                    }
                else:
                    return ""
            else:
                return ""

        elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_STOP:
            event_name = "content_block_stop"
            event = {
                "type": "content_block_stop",
                "index": chunk.index,
            }

        elif chunk.chunk_type == ChunkType.MESSAGE_DELTA:
            event_name = "message_delta"
            stop_reason = self._unified_to_stop_reason(chunk.stop_reason)
            usage_json: Optional[dict[str, Any]] = None
            if chunk.usage:
                usage_json = {
                    "input_tokens": chunk.usage.input_tokens,
                    "output_tokens": chunk.usage.output_tokens,
                }
            event = {
                "type": "message_delta",
                "delta": {
                    "stop_reason": stop_reason,
                    "stop_sequence": None,
                },
                "usage": usage_json,
            }

        elif chunk.chunk_type == ChunkType.MESSAGE_STOP:
            event_name = "message_stop"
            event = {"type": "message_stop"}

        elif chunk.chunk_type == ChunkType.PING:
            event_name = "ping"
            event = {"type": "ping"}

        if not event:
            return ""

        return f"event: {event_name}\ndata: {json.dumps(event)}\n\n"

    # =========================================================================
    # Protocol Detection
    # =========================================================================

    def can_handle(self, raw: dict[str, Any]) -> bool:
        """
        Check if this transformer can handle the given request.

        Anthropic format indicators:
        - Has top-level "system" field
        - Has "max_tokens" and messages with typed content blocks

        Args:
            raw: Raw request payload

        Returns:
            True if this is an Anthropic format request
        """
        # Anthropic has system as top-level field
        if "system" in raw:
            return True

        # Check for Anthropic-style content blocks in messages
        if "max_tokens" in raw:
            for msg in raw.get("messages", []):
                content = msg.get("content")
                if isinstance(content, list):
                    for block in content:
                        if isinstance(block, dict):
                            block_type = block.get("type")
                            if block_type in (
                                "text",
                                "image",
                                "tool_use",
                                "tool_result",
                            ):
                                return True

        return False

    # =========================================================================
    # Helper Methods: Content Conversion
    # =========================================================================

    def _content_block_to_unified(self, block: dict[str, Any]) -> UnifiedContent:
        """Convert Anthropic content block to unified content."""
        block_type = block.get("type")

        if block_type == "text":
            return create_text_content(block.get("text", ""))

        elif block_type == "image":
            source = block.get("source", {})
            source_type = source.get("type", "base64")
            if source_type == "base64":
                return create_image_base64(
                    source.get("media_type", ""), source.get("data", "")
                )
            else:
                return create_image_url(source.get("data", ""))

        elif block_type == "tool_use":
            return create_tool_use_content(
                block.get("id", ""),
                block.get("name", ""),
                block.get("input", {}),
            )

        elif block_type == "tool_result":
            return create_tool_result_content(
                block.get("tool_use_id", ""),
                block.get("content"),
                block.get("is_error", False),
            )

        elif block_type == "thinking":
            return create_thinking_content(
                block.get("thinking", ""), block.get("signature")
            )

        # Default to text
        return create_text_content(str(block))

    def _unified_to_content_block(
        self, content: UnifiedContent
    ) -> Optional[dict[str, Any]]:
        """Convert unified content to Anthropic content block."""
        if isinstance(content, TextContent):
            return {"type": "text", "text": content.text}

        elif isinstance(content, ImageContent):
            return {
                "type": "image",
                "source": {
                    "type": content.source_type,
                    "media_type": content.media_type,
                    "data": content.data,
                },
            }

        elif isinstance(content, ToolUseContent):
            return {
                "type": "tool_use",
                "id": content.id,
                "name": content.name,
                "input": content.input,
            }

        elif isinstance(content, ToolResultContent):
            result: dict[str, Any] = {
                "type": "tool_result",
                "tool_use_id": content.tool_use_id,
                "content": content.content,
            }
            if content.is_error:
                result["is_error"] = True
            return result

        elif isinstance(content, ThinkingContent):
            result = {"type": "thinking", "thinking": content.text}
            if content.signature:
                result["signature"] = content.signature
            return result

        return None

    def _unified_to_delta(self, content: UnifiedContent) -> Optional[dict[str, Any]]:
        """Convert unified content to Anthropic delta format."""
        if isinstance(content, TextContent):
            return {"type": "text_delta", "text": content.text}

        elif isinstance(content, ThinkingContent):
            if content.signature:
                return {"type": "signature_delta", "signature": content.signature}
            return {"type": "thinking_delta", "thinking": content.text}

        elif isinstance(content, ToolInputDeltaContent):
            return {"type": "input_json_delta", "partial_json": content.partial_json}

        return None

    # =========================================================================
    # Helper Methods: Message Conversion
    # =========================================================================

    def _message_to_unified(self, msg: dict[str, Any]) -> UnifiedMessage:
        """Convert Anthropic message to unified message."""
        role = Role.from_string(msg.get("role", "user"))
        content: list[UnifiedContent] = []
        tool_calls: list[UnifiedToolCall] = []
        tool_call_id: Optional[str] = None

        raw_content = msg.get("content")
        if isinstance(raw_content, str):
            content = [create_text_content(raw_content)]
        elif isinstance(raw_content, list):
            for block in raw_content:
                unified_content = self._content_block_to_unified(block)
                content.append(unified_content)

                # Extract tool calls from tool_use content
                if isinstance(unified_content, ToolUseContent):
                    tool_calls.append(
                        UnifiedToolCall(
                            id=unified_content.id,
                            name=unified_content.name,
                            arguments=unified_content.input,
                        )
                    )

                # Extract tool_call_id from tool_result
                if isinstance(unified_content, ToolResultContent):
                    tool_call_id = unified_content.tool_use_id

        return UnifiedMessage(
            role=role,
            content=content,
            name=msg.get("name"),
            tool_calls=tool_calls,
            tool_call_id=tool_call_id,
        )

    def _unified_to_message(self, msg: UnifiedMessage) -> dict[str, Any]:
        """Convert unified message to Anthropic message format."""
        role = msg.role.value

        # Build content
        if len(msg.content) == 1 and isinstance(msg.content[0], TextContent):
            content: Any = msg.content[0].text
        else:
            content = [
                self._unified_to_content_block(c)
                for c in msg.content
                if self._unified_to_content_block(c) is not None
            ]

        return {"role": role, "content": content}

    # =========================================================================
    # Helper Methods: System Prompt
    # =========================================================================

    def _extract_system(self, system: Any) -> Optional[str]:
        """Extract system prompt from Anthropic system field."""
        if system is None:
            return None

        if isinstance(system, str):
            return strip_billing_header(system)

        if isinstance(system, list):
            texts = []
            for block in system:
                if isinstance(block, dict) and block.get("type") == "text":
                    texts.append(strip_billing_header(block.get("text", "")))
            return "\n".join(texts) if texts else None

        return None

    # =========================================================================
    # Helper Methods: Stop Reason Conversion
    # =========================================================================

    def _stop_reason_to_unified(self, reason: Optional[str]) -> Optional[StopReason]:
        """Convert Anthropic stop_reason to unified StopReason."""
        if not reason:
            return None
        mapping = {
            "end_turn": StopReason.END_TURN,
            "max_tokens": StopReason.MAX_TOKENS,
            "stop_sequence": StopReason.STOP_SEQUENCE,
            "tool_use": StopReason.TOOL_USE,
        }
        return mapping.get(reason, StopReason.END_TURN)

    def _unified_to_stop_reason(self, reason: Optional[StopReason]) -> Optional[str]:
        """Convert unified StopReason to Anthropic stop_reason."""
        if not reason:
            return None
        mapping = {
            StopReason.END_TURN: "end_turn",
            StopReason.MAX_TOKENS: "max_tokens",
            StopReason.LENGTH: "max_tokens",
            StopReason.STOP_SEQUENCE: "stop_sequence",
            StopReason.TOOL_USE: "tool_use",
            StopReason.CONTENT_FILTER: "end_turn",
        }
        return mapping.get(reason, "end_turn")

    # =========================================================================
    # Helper Methods: Stream Event Parsing
    # =========================================================================

    def _parse_stream_event(self, event: dict[str, Any]) -> list[UnifiedStreamChunk]:
        """Parse Anthropic stream event to unified chunks."""
        chunks: list[UnifiedStreamChunk] = []
        event_type = event.get("type")

        if event_type == "message_start":
            message = event.get("message", {})
            content: list[UnifiedContent] = []
            for block in message.get("content", []):
                content.append(self._content_block_to_unified(block))

            usage_data = message.get("usage", {})
            usage = UnifiedUsage(
                input_tokens=usage_data.get("input_tokens", 0),
                output_tokens=usage_data.get("output_tokens", 0),
                cache_read_tokens=usage_data.get("cache_read_input_tokens"),
                cache_write_tokens=usage_data.get("cache_creation_input_tokens"),
            )

            unified_response = UnifiedResponse(
                id=message.get("id", ""),
                model=message.get("model", ""),
                content=content,
                stop_reason=self._stop_reason_to_unified(message.get("stop_reason")),
                usage=usage,
                tool_calls=[],
            )
            chunks.append(UnifiedStreamChunk.message_start(unified_response))

        elif event_type == "content_block_start":
            index = event.get("index", 0)
            content_block = event.get("content_block", {})
            unified_content = self._content_block_to_unified(content_block)
            chunks.append(
                UnifiedStreamChunk.content_block_start(index, unified_content)
            )

        elif event_type == "content_block_delta":
            index = event.get("index", 0)
            delta = event.get("delta", {})
            delta_type = delta.get("type")

            if delta_type == "text_delta":
                chunks.append(
                    UnifiedStreamChunk.content_block_delta(
                        index, create_text_content(delta.get("text", ""))
                    )
                )
            elif delta_type == "input_json_delta":
                chunks.append(
                    UnifiedStreamChunk.content_block_delta(
                        index,
                        ToolInputDeltaContent(
                            index=index, partial_json=delta.get("partial_json", "")
                        ),
                    )
                )
            elif delta_type == "thinking_delta":
                chunks.append(
                    UnifiedStreamChunk.content_block_delta(
                        index, create_thinking_content(delta.get("thinking", ""))
                    )
                )
            elif delta_type == "signature_delta":
                chunks.append(
                    UnifiedStreamChunk.content_block_delta(
                        index, create_thinking_content("", delta.get("signature"))
                    )
                )

        elif event_type == "content_block_stop":
            index = event.get("index", 0)
            chunks.append(UnifiedStreamChunk.content_block_stop(index))

        elif event_type == "message_delta":
            delta = event.get("delta", {})
            usage_data = event.get("usage", {})

            stop_reason = self._stop_reason_to_unified(delta.get("stop_reason"))
            usage = UnifiedUsage(
                input_tokens=usage_data.get("input_tokens", 0),
                output_tokens=usage_data.get("output_tokens", 0),
                cache_read_tokens=usage_data.get("cache_read_input_tokens"),
                cache_write_tokens=usage_data.get("cache_creation_input_tokens"),
            )
            chunks.append(
                UnifiedStreamChunk.message_delta(
                    stop_reason or StopReason.END_TURN, usage
                )
            )

        elif event_type == "message_stop":
            chunks.append(UnifiedStreamChunk.message_stop())

        elif event_type == "ping":
            chunks.append(UnifiedStreamChunk.ping())

        return chunks
