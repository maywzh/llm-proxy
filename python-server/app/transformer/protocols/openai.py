# OpenAI Protocol Transformer
#
# Handles conversion between OpenAI Chat Completions API format and
# the Unified Internal Format (UIF).

import json
from datetime import datetime, timezone
from typing import Any, Optional

from app.utils.gemini3 import THOUGHT_SIGNATURE_SEPARATOR

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
)


class OpenAITransformer(Transformer):
    """
    OpenAI protocol transformer.

    Handles bidirectional conversion between OpenAI Chat Completions API format
    and the Unified Internal Format (UIF).
    """

    @property
    def protocol(self) -> Protocol:
        """Get the protocol this transformer handles."""
        return Protocol.OPENAI

    @property
    def endpoint(self) -> str:
        """Get the endpoint path for this protocol."""
        return "/v1/chat/completions"

    # =========================================================================
    # Hook 1: transform_request_out (Client → Unified)
    # =========================================================================

    def transform_request_out(self, raw: dict[str, Any]) -> UnifiedRequest:
        """
        Transform OpenAI request format to Unified Internal Format.

        Args:
            raw: Raw OpenAI request payload

        Returns:
            UnifiedRequest in UIF format
        """
        messages: list[UnifiedMessage] = []
        system: Optional[str] = None

        for msg in raw.get("messages", []):
            role = Role.from_string(msg.get("role", "user"))

            # Extract system message
            if role == Role.SYSTEM:
                system = self._extract_text_content(msg.get("content"))
                continue

            content = self._parse_content(msg.get("content"))
            tool_calls = self._parse_tool_calls(msg.get("tool_calls"))

            # Strip __thought__ from tool_call_id if present
            tool_call_id = msg.get("tool_call_id")
            if tool_call_id and THOUGHT_SIGNATURE_SEPARATOR in tool_call_id:
                tool_call_id = tool_call_id.split(THOUGHT_SIGNATURE_SEPARATOR, 1)[0]

            messages.append(
                UnifiedMessage(
                    role=role,
                    content=content,
                    name=msg.get("name"),
                    tool_calls=tool_calls,
                    tool_call_id=tool_call_id,
                )
            )

        # Parse tools
        tools = [
            UnifiedTool(
                name=t["function"]["name"],
                description=t["function"].get("description"),
                input_schema=t["function"].get("parameters", {}),
                tool_type=t.get("type", "function"),
            )
            for t in raw.get("tools", [])
        ]

        # Build parameters - exclude known fields from extra
        known_fields = {
            "model",
            "messages",
            "temperature",
            "max_tokens",
            "max_completion_tokens",
            "top_p",
            "stop",
            "stream",
            "tools",
            "tool_choice",
        }
        extra = {k: v for k, v in raw.items() if k not in known_fields}

        parameters = UnifiedParameters(
            temperature=raw.get("temperature"),
            max_tokens=raw.get("max_tokens") or raw.get("max_completion_tokens"),
            top_p=raw.get("top_p"),
            stop_sequences=raw.get("stop"),
            stream=raw.get("stream", False),
            extra=extra,
        )

        # Normalize tool_choice to dict format for UIF
        tool_choice = self._normalize_tool_choice_to_uif(raw.get("tool_choice"))

        return UnifiedRequest(
            model=raw.get("model", ""),
            messages=messages,
            system=system,
            parameters=parameters,
            tools=tools,
            tool_choice=tool_choice,
            client_protocol=Protocol.OPENAI,
        )

    # =========================================================================
    # Hook 2: transform_request_in (Unified → Provider)
    # =========================================================================

    def transform_request_in(self, unified: UnifiedRequest) -> dict[str, Any]:
        """
        Transform Unified Internal Format to OpenAI request format.

        Args:
            unified: Request in UIF format

        Returns:
            Request payload in OpenAI format
        """
        messages: list[dict[str, Any]] = []

        # Add system message if present
        if unified.system:
            messages.append({"role": "system", "content": unified.system})

        # Add other messages
        for msg in unified.messages:
            # Check if message contains ToolResult content blocks (from Anthropic format).
            # Anthropic puts multiple tool_results in a single user message,
            # but OpenAI requires each as a separate {role: "tool"} message.
            has_tool_results = any(
                isinstance(c, ToolResultContent) for c in msg.content
            )

            if has_tool_results:
                # Emit each tool_result FIRST as an independent role: "tool" message.
                # This must come before any non-tool-result user content to maintain
                # the assistant(tool_calls) → tool(result) adjacency required by
                # downstream providers (e.g. Bedrock Converse).
                for c in msg.content:
                    if isinstance(c, ToolResultContent):
                        content_str = self._tool_result_content_to_string(
                            c.content, c.is_error
                        )
                        messages.append(
                            {
                                "role": "tool",
                                "content": content_str,
                                "tool_call_id": c.tool_use_id,
                            }
                        )

                # Emit non-tool-result content as a user message after tool results
                non_tool_parts = [
                    self._unified_content_part_to_openai(c)
                    for c in msg.content
                    if not isinstance(c, ToolResultContent)
                    and self._unified_content_part_to_openai(c) is not None
                ]
                if non_tool_parts:
                    messages.append(
                        {
                            "role": "user",
                            "content": non_tool_parts,
                        }
                    )
            else:
                messages.append(self._unified_to_message(msg))

        request: dict[str, Any] = {
            "model": unified.model,
            "messages": messages,
        }

        # Add optional parameters
        if unified.parameters.temperature is not None:
            request["temperature"] = unified.parameters.temperature
        if unified.parameters.max_tokens is not None:
            request["max_tokens"] = unified.parameters.max_tokens
        if unified.parameters.top_p is not None:
            request["top_p"] = unified.parameters.top_p
        if unified.parameters.stop_sequences:
            request["stop"] = unified.parameters.stop_sequences
        if unified.parameters.stream:
            request["stream"] = True

        # Add tools
        if unified.tools:
            request["tools"] = [
                {
                    "type": t.tool_type or "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema,
                    },
                }
                for t in unified.tools
            ]

        # Convert tool_choice (handle Anthropic format conversion)
        if unified.tool_choice:
            request["tool_choice"] = self._convert_tool_choice(unified.tool_choice)

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
        Transform OpenAI response to Unified Internal Format.

        Args:
            raw: Raw response payload from OpenAI
            original_model: Original model name from client request

        Returns:
            UnifiedResponse in UIF format
        """
        choices = raw.get("choices", [])
        if not choices:
            return UnifiedResponse(
                id=raw.get("id", ""),
                model=original_model,
                content=[],
                stop_reason=None,
                usage=UnifiedUsage(),
                tool_calls=[],
            )

        choice = choices[0]
        message = choice.get("message", {})

        content = self._parse_content(message.get("content"))
        tool_calls = self._parse_tool_calls(message.get("tool_calls"))

        # Parse reasoning_content (thinking) from OpenAI-compatible response
        reasoning = message.get("reasoning_content")
        if reasoning and isinstance(reasoning, str) and reasoning:
            content.insert(0, ThinkingContent(text=reasoning))

        # Parse thinking_blocks for structured thinking content with signatures
        thinking_blocks = message.get("thinking_blocks")
        if thinking_blocks and isinstance(thinking_blocks, list):
            for block in thinking_blocks:
                if isinstance(block, dict) and block.get("type") == "thinking":
                    block_text = block.get("thinking", "")
                    block_sig = block.get("signature")
                    if block_sig and block_text:
                        # thinking_blocks with both text+sig: add thinking then signature
                        content.insert(0, ThinkingContent(text=block_text))
                        content.append(ThinkingContent(text="", signature=block_sig))
                    elif block_sig:
                        # Signature-only block
                        content.append(ThinkingContent(text="", signature=block_sig))

        # Parse provider_specific_fields.thought_signatures as fallback
        has_signature = any(
            isinstance(c, ThinkingContent) and not c.text and c.signature
            for c in content
        )
        if not has_signature:
            psf = message.get("provider_specific_fields")
            if psf and isinstance(psf, dict):
                sigs = psf.get("thought_signatures")
                if sigs and isinstance(sigs, list):
                    for sig in sigs:
                        if isinstance(sig, str):
                            content.append(ThinkingContent(text="", signature=sig))

        # Extract signatures from tool_call provider_specific_fields.thought_signature
        # and tool_call_ids (__thought__ encoding) as final fallback
        has_signature_now = any(
            isinstance(c, ThinkingContent) and not c.text and c.signature
            for c in content
        )
        if not has_signature_now:
            raw_tool_calls = message.get("tool_calls") or []
            for tc in raw_tool_calls:
                # Check provider_specific_fields.thought_signature on tool call
                tc_psf = tc.get("provider_specific_fields")
                if tc_psf and isinstance(tc_psf, dict):
                    tc_sig = tc_psf.get("thought_signature")
                    if isinstance(tc_sig, str) and tc_sig:
                        content.append(ThinkingContent(text="", signature=tc_sig))
                        continue
                # Fallback: extract from tool_call_id encoding
                tc_id = tc.get("id", "")
                if THOUGHT_SIGNATURE_SEPARATOR in tc_id:
                    sig = tc_id.split(THOUGHT_SIGNATURE_SEPARATOR, 1)[1]
                    if sig:
                        content.append(ThinkingContent(text="", signature=sig))

        stop_reason = self._parse_finish_reason(choice.get("finish_reason"))

        usage_data = raw.get("usage", {})
        usage = UnifiedUsage(
            input_tokens=usage_data.get("prompt_tokens", 0),
            output_tokens=usage_data.get("completion_tokens", 0),
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
        Transform Unified Internal Format to OpenAI response format.

        Args:
            unified: Response in UIF format
            client_protocol: Client's expected protocol

        Returns:
            Response payload in OpenAI format
        """
        content = self._unified_content_to_openai(unified.content)
        reasoning_content = self._unified_reasoning_to_openai(unified.content)
        thinking_blocks = self._unified_thinking_blocks(unified.content)
        thought_signatures = self._unified_thought_signatures(unified.content)
        tool_calls = self._unified_tool_calls_to_openai(unified.tool_calls)

        # Encode last thought_signature into tool_call_ids and
        # set provider_specific_fields.thought_signature on each tool call (litellm compat)
        if tool_calls and thought_signatures:
            last_sig = thought_signatures[-1]
            for tc in tool_calls:
                if THOUGHT_SIGNATURE_SEPARATOR not in tc["id"]:
                    tc["id"] = f"{tc['id']}{THOUGHT_SIGNATURE_SEPARATOR}{last_sig}"
                tc["provider_specific_fields"] = {"thought_signature": last_sig}

        finish_reason = self._stop_reason_to_finish_reason(unified.stop_reason)

        message: dict[str, Any] = {
            "role": "assistant",
            "content": content,
        }
        if reasoning_content:
            message["reasoning_content"] = reasoning_content
        if thinking_blocks:
            message["thinking_blocks"] = thinking_blocks
        if thought_signatures:
            message["provider_specific_fields"] = {
                "thought_signatures": thought_signatures
            }
        if tool_calls:
            message["tool_calls"] = tool_calls

        return {
            "id": unified.id,
            "object": "chat.completion",
            "created": int(datetime.now(timezone.utc).timestamp()),
            "model": unified.model,
            "choices": [
                {
                    "index": 0,
                    "message": message,
                    "finish_reason": finish_reason,
                }
            ],
            "usage": {
                "prompt_tokens": unified.usage.input_tokens,
                "completion_tokens": unified.usage.output_tokens,
                "total_tokens": unified.usage.total_tokens(),
            },
        }

    # =========================================================================
    # Streaming: transform_stream_chunk_in
    # =========================================================================

    def transform_stream_chunk_in(self, chunk: bytes) -> list[UnifiedStreamChunk]:
        """
        Transform OpenAI streaming chunk to unified chunks.

        Args:
            chunk: Raw bytes from OpenAI stream

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
                    chunks.extend(self._parse_stream_chunk(parsed))
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
        Transform unified streaming chunk to OpenAI SSE format.

        Args:
            chunk: Unified stream chunk
            client_protocol: Client's expected protocol

        Returns:
            SSE-formatted string
        """
        if chunk.chunk_type == ChunkType.CONTENT_BLOCK_START:
            # Handle tool_use content block start
            if chunk.content_block and isinstance(chunk.content_block, ToolUseContent):
                openai_chunk = {
                    "id": "chatcmpl-stream",
                    "object": "chat.completion.chunk",
                    "created": int(datetime.now(timezone.utc).timestamp()),
                    "model": "model",
                    "choices": [
                        {
                            "index": 0,
                            "delta": {
                                "tool_calls": [
                                    {
                                        "index": chunk.index,
                                        "id": chunk.content_block.id,
                                        "type": "function",
                                        "function": {
                                            "name": chunk.content_block.name,
                                            "arguments": "",
                                        },
                                    }
                                ]
                            },
                            "finish_reason": None,
                        }
                    ],
                }
                return f"data: {json.dumps(openai_chunk)}\n\n"
            return ""

        elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_DELTA:
            if chunk.delta:
                if isinstance(chunk.delta, TextContent):
                    openai_chunk = {
                        "id": "chatcmpl-stream",
                        "object": "chat.completion.chunk",
                        "created": int(datetime.now(timezone.utc).timestamp()),
                        "model": "model",
                        "choices": [
                            {
                                "index": chunk.index,
                                "delta": {"content": chunk.delta.text},
                                "finish_reason": None,
                            }
                        ],
                    }
                    return f"data: {json.dumps(openai_chunk)}\n\n"

                elif isinstance(chunk.delta, ThinkingContent):
                    if not chunk.delta.text and chunk.delta.signature:
                        # Signature-only: output as provider_specific_fields.thought_signatures
                        openai_chunk = {
                            "id": "chatcmpl-stream",
                            "object": "chat.completion.chunk",
                            "created": int(datetime.now(timezone.utc).timestamp()),
                            "model": "model",
                            "choices": [
                                {
                                    "index": chunk.index,
                                    "delta": {
                                        "provider_specific_fields": {
                                            "thought_signatures": [
                                                chunk.delta.signature
                                            ]
                                        }
                                    },
                                    "finish_reason": None,
                                }
                            ],
                        }
                        return f"data: {json.dumps(openai_chunk)}\n\n"
                    # Regular thinking content as reasoning_content
                    openai_chunk = {
                        "id": "chatcmpl-stream",
                        "object": "chat.completion.chunk",
                        "created": int(datetime.now(timezone.utc).timestamp()),
                        "model": "model",
                        "choices": [
                            {
                                "index": chunk.index,
                                "delta": {"reasoning_content": chunk.delta.text},
                                "finish_reason": None,
                            }
                        ],
                    }
                    return f"data: {json.dumps(openai_chunk)}\n\n"

                elif isinstance(chunk.delta, ToolInputDeltaContent):
                    openai_chunk = {
                        "id": "chatcmpl-stream",
                        "object": "chat.completion.chunk",
                        "created": int(datetime.now(timezone.utc).timestamp()),
                        "model": "model",
                        "choices": [
                            {
                                "index": 0,
                                "delta": {
                                    "tool_calls": [
                                        {
                                            "index": chunk.delta.index,
                                            "function": {
                                                "arguments": chunk.delta.partial_json
                                            },
                                        }
                                    ]
                                },
                                "finish_reason": None,
                            }
                        ],
                    }
                    return f"data: {json.dumps(openai_chunk)}\n\n"
            return ""

        elif chunk.chunk_type == ChunkType.MESSAGE_DELTA:
            finish_reason = self._stop_reason_to_finish_reason(chunk.stop_reason)
            openai_chunk: dict[str, Any] = {
                "id": "chatcmpl-stream",
                "object": "chat.completion.chunk",
                "created": int(datetime.now(timezone.utc).timestamp()),
                "model": "model",
                "choices": [
                    {
                        "index": 0,
                        "delta": {},
                        "finish_reason": finish_reason,
                    }
                ],
            }
            if chunk.usage:
                openai_chunk["usage"] = {
                    "prompt_tokens": chunk.usage.input_tokens,
                    "completion_tokens": chunk.usage.output_tokens,
                    "total_tokens": chunk.usage.total_tokens(),
                }
            return f"data: {json.dumps(openai_chunk)}\n\n"

        elif chunk.chunk_type == ChunkType.MESSAGE_STOP:
            return "data: [DONE]\n\n"

        return ""

    # =========================================================================
    # Protocol Detection
    # =========================================================================

    def can_handle(self, raw: dict[str, Any]) -> bool:
        """
        Check if this transformer can handle the given request.

        OpenAI format: has "messages" array and no Anthropic-specific fields.

        Args:
            raw: Raw request payload

        Returns:
            True if this is an OpenAI format request
        """
        # Must have messages
        if "messages" not in raw:
            return False

        # Anthropic has system as top-level field
        if "system" in raw:
            return False

        # Check for Anthropic-style content blocks in messages
        for msg in raw.get("messages", []):
            content = msg.get("content")
            if isinstance(content, list):
                for block in content:
                    if isinstance(block, dict):
                        block_type = block.get("type")
                        if block_type in ("tool_use", "tool_result"):
                            return False

        return True

    # =========================================================================
    # Helper Methods: Content Parsing
    # =========================================================================

    def _extract_text_content(self, content: Any) -> Optional[str]:
        """Extract text from content (string or array format)."""
        if isinstance(content, str):
            return content
        if isinstance(content, list):
            texts = [
                p.get("text", "")
                for p in content
                if isinstance(p, dict) and p.get("type") == "text"
            ]
            return "".join(texts) if texts else None
        return None

    def _parse_content(self, content: Any) -> list[UnifiedContent]:
        """Parse OpenAI content to unified content blocks."""
        if content is None:
            return []
        if isinstance(content, str):
            return [create_text_content(content)]
        if isinstance(content, list):
            result: list[UnifiedContent] = []
            for part in content:
                if isinstance(part, dict):
                    if part.get("type") == "text":
                        result.append(create_text_content(part.get("text", "")))
                    elif part.get("type") == "image_url":
                        url = part.get("image_url", {}).get("url", "")
                        if url.startswith("data:"):
                            # Parse data URL: data:image/jpeg;base64,/9j/4AAQ...
                            parts = url.split(",", 1)
                            if len(parts) == 2:
                                media_type = parts[0].replace("data:", "").split(";")[0]
                                result.append(create_image_base64(media_type, parts[1]))
                            else:
                                result.append(create_image_url(url))
                        else:
                            result.append(create_image_url(url))
            return result
        return []

    def _parse_tool_calls(self, tool_calls: Any) -> list[UnifiedToolCall]:
        """Parse OpenAI tool calls to unified format.
        Strips __thought__ signature encoding from tool_call_id if present.
        """
        if not tool_calls:
            return []
        result: list[UnifiedToolCall] = []
        for tc in tool_calls:
            try:
                arguments = json.loads(tc.get("function", {}).get("arguments", "{}"))
            except json.JSONDecodeError:
                arguments = {}
            tc_id = tc.get("id", "")
            # Strip __thought__ signature from tool_call_id
            if THOUGHT_SIGNATURE_SEPARATOR in tc_id:
                tc_id = tc_id.split(THOUGHT_SIGNATURE_SEPARATOR, 1)[0]
            result.append(
                UnifiedToolCall(
                    id=tc_id,
                    name=tc.get("function", {}).get("name", ""),
                    arguments=arguments,
                )
            )
        return result

    # =========================================================================
    # Helper Methods: Message Conversion
    # =========================================================================

    def _unified_to_message(self, msg: UnifiedMessage) -> dict[str, Any]:
        """Convert unified message to OpenAI message format."""
        result: dict[str, Any] = {"role": msg.role.value}

        # Build content
        if len(msg.content) == 1 and isinstance(msg.content[0], TextContent):
            result["content"] = msg.content[0].text
        elif msg.content:
            content_parts = [
                self._unified_content_part_to_openai(c)
                for c in msg.content
                if self._unified_content_part_to_openai(c) is not None
            ]
            if content_parts:
                result["content"] = content_parts
        else:
            # Handle tool result messages
            if msg.role == Role.TOOL:
                for c in msg.content:
                    if isinstance(c, ToolResultContent):
                        if isinstance(c.content, str):
                            result["content"] = c.content
                        else:
                            result["content"] = json.dumps(c.content)
                        break

        # Add tool calls
        if msg.tool_calls:
            result["tool_calls"] = [
                {
                    "id": tc.id,
                    "type": "function",
                    "function": {
                        "name": tc.name,
                        "arguments": json.dumps(tc.arguments),
                    },
                }
                for tc in msg.tool_calls
            ]

        # Add tool_call_id for tool result messages
        if msg.tool_call_id:
            result["tool_call_id"] = msg.tool_call_id

        if msg.name:
            result["name"] = msg.name

        return result

    def _unified_content_part_to_openai(
        self, content: UnifiedContent
    ) -> Optional[dict[str, Any]]:
        """Convert unified content to OpenAI content part."""
        if isinstance(content, TextContent):
            return {"type": "text", "text": content.text}
        elif isinstance(content, ImageContent):
            if content.source_type == "base64":
                url = f"data:{content.media_type};base64,{content.data}"
            else:
                url = content.data
            return {"type": "image_url", "image_url": {"url": url}}
        return None

    @staticmethod
    def _tool_result_content_to_string(content: Any, is_error: bool) -> str:
        """Convert Anthropic tool_result content to OpenAI-compatible string.

        Anthropic tool_result content can be: string, None, or structured list
        of content blocks like [{"type":"text","text":"..."}].
        OpenAI tool messages only support string content.
        """
        if content is None:
            text = ""
        elif isinstance(content, str):
            text = content
        elif isinstance(content, list):
            text = "\n".join(
                block.get("text", "")
                for block in content
                if isinstance(block, dict) and block.get("text")
            )
        else:
            text = json.dumps(content)

        if is_error and text:
            return f"[Error] {text}"
        return text

    def _unified_content_to_openai(
        self, content: list[UnifiedContent]
    ) -> Optional[str]:
        """Convert unified content list to OpenAI response content."""
        if not content:
            return None
        if len(content) == 1 and isinstance(content[0], TextContent):
            return content[0].text
        texts = [c.text for c in content if isinstance(c, TextContent)]
        return "".join(texts) if texts else None

    def _unified_reasoning_to_openai(
        self, content: list[UnifiedContent]
    ) -> Optional[str]:
        """Extract thinking content as reasoning_content for OpenAI-compatible APIs.
        Excludes signature-only blocks (text="" with signature set).
        """
        thinking_texts = [
            c.text
            for c in content
            if isinstance(c, ThinkingContent) and (c.text or not c.signature)
        ]
        return "".join(thinking_texts) if thinking_texts else None

    def _unified_thinking_blocks(
        self, content: list[UnifiedContent]
    ) -> Optional[list[dict[str, Any]]]:
        """Build thinking_blocks list from UIF content (litellm-compatible)."""
        blocks: list[dict[str, Any]] = []
        for c in content:
            if isinstance(c, ThinkingContent):
                if c.text and not (not c.text and c.signature):
                    block: dict[str, Any] = {"type": "thinking", "thinking": c.text}
                    blocks.append(block)
                # Signature-only blocks will be attached below
        # Attach signatures to the last thinking block
        for c in content:
            if isinstance(c, ThinkingContent) and not c.text and c.signature:
                if blocks and "signature" not in blocks[-1]:
                    blocks[-1]["signature"] = c.signature
        return blocks if blocks else None

    def _unified_thought_signatures(
        self, content: list[UnifiedContent]
    ) -> Optional[list[str]]:
        """Extract thought_signatures list from signature-only Thinking blocks."""
        sigs = [
            c.signature
            for c in content
            if isinstance(c, ThinkingContent) and not c.text and c.signature
        ]
        return sigs if sigs else None

    def _unified_tool_calls_to_openai(
        self, tool_calls: list[UnifiedToolCall]
    ) -> Optional[list[dict[str, Any]]]:
        """Convert unified tool calls to OpenAI format."""
        if not tool_calls:
            return None
        return [
            {
                "id": tc.id,
                "type": "function",
                "function": {
                    "name": tc.name,
                    "arguments": json.dumps(tc.arguments),
                },
            }
            for tc in tool_calls
        ]

    # =========================================================================
    # Helper Methods: Stop Reason Conversion
    # =========================================================================

    def _parse_finish_reason(self, reason: Optional[str]) -> Optional[StopReason]:
        """Convert OpenAI finish_reason to unified StopReason."""
        if not reason:
            return None
        mapping = {
            "stop": StopReason.END_TURN,
            "length": StopReason.MAX_TOKENS,
            "tool_calls": StopReason.TOOL_USE,
            "content_filter": StopReason.CONTENT_FILTER,
        }
        return mapping.get(reason, StopReason.END_TURN)

    def _stop_reason_to_finish_reason(
        self, reason: Optional[StopReason]
    ) -> Optional[str]:
        """Convert unified StopReason to OpenAI finish_reason."""
        if not reason:
            return None
        mapping = {
            StopReason.END_TURN: "stop",
            StopReason.MAX_TOKENS: "length",
            StopReason.LENGTH: "length",
            StopReason.STOP_SEQUENCE: "stop",
            StopReason.TOOL_USE: "tool_calls",
            StopReason.CONTENT_FILTER: "content_filter",
        }
        return mapping.get(reason, "stop")

    # =========================================================================
    # Helper Methods: Tool Choice Conversion
    # =========================================================================

    def _normalize_tool_choice_to_uif(
        self, tool_choice: Any
    ) -> Optional[dict[str, Any]]:
        """
        Normalize OpenAI tool_choice to UIF dict format.

        OpenAI formats:
        - "auto" -> {"type": "auto"}
        - "none" -> {"type": "none"}
        - "required" -> {"type": "any"}
        - {"type": "function", "function": {"name": "xxx"}} -> {"type": "tool", "name": "xxx"}
        """
        if tool_choice is None:
            return None
        if isinstance(tool_choice, str):
            if tool_choice == "auto":
                return {"type": "auto"}
            elif tool_choice == "none":
                return {"type": "none"}
            elif tool_choice == "required":
                return {"type": "any"}
            else:
                return {"type": tool_choice}
        if isinstance(tool_choice, dict):
            # Already in dict format, normalize if needed
            tc_type = tool_choice.get("type")
            if tc_type == "function":
                # OpenAI specific function format
                func = tool_choice.get("function", {})
                name = func.get("name")
                if name:
                    return {"type": "tool", "name": name}
            return tool_choice
        return None

    def _convert_tool_choice(self, tool_choice: Any) -> Any:
        """
        Convert tool_choice to OpenAI format.

        Handles conversion from Anthropic format:
        - {"type": "auto"} -> "auto"
        - {"type": "any"} -> "required"
        - {"type": "none"} -> "none"
        - {"type": "tool", "name": "xxx"} -> {"type": "function", "function": {"name": "xxx"}}
        """
        if isinstance(tool_choice, dict):
            tc_type = tool_choice.get("type")
            if tc_type == "auto":
                return "auto"
            elif tc_type == "any":
                return "required"
            elif tc_type == "none":
                return "none"
            elif tc_type == "tool":
                name = tool_choice.get("name")
                if name:
                    return {"type": "function", "function": {"name": name}}
        return tool_choice

    # =========================================================================
    # Helper Methods: Stream Chunk Parsing
    # =========================================================================

    def _parse_stream_chunk(self, parsed: dict[str, Any]) -> list[UnifiedStreamChunk]:
        """Parse a single OpenAI stream chunk to unified chunks."""
        chunks: list[UnifiedStreamChunk] = []
        emitted_message_delta = False

        for choice in parsed.get("choices", []):
            delta = choice.get("delta", {})

            # Handle content delta (text content is always at index 0)
            if "content" in delta and delta["content"]:
                chunks.append(
                    UnifiedStreamChunk.content_block_delta(
                        0, create_text_content(delta["content"])
                    )
                )

            # Handle reasoning_content delta (thinking content)
            if "reasoning_content" in delta and delta["reasoning_content"]:
                chunks.append(
                    UnifiedStreamChunk.content_block_delta(
                        0,
                        ThinkingContent(text=delta["reasoning_content"]),
                    )
                )

            # Handle thinking_blocks delta (structured thinking with signatures)
            if "thinking_blocks" in delta and delta["thinking_blocks"]:
                for block in delta["thinking_blocks"]:
                    if isinstance(block, dict) and block.get("type") == "thinking":
                        block_text = block.get("thinking", "")
                        block_sig = block.get("signature")
                        if block_text:
                            chunks.append(
                                UnifiedStreamChunk.content_block_delta(
                                    0,
                                    ThinkingContent(text=block_text),
                                )
                            )
                        if block_sig:
                            chunks.append(
                                UnifiedStreamChunk.content_block_delta(
                                    0,
                                    ThinkingContent(text="", signature=block_sig),
                                )
                            )

            # Handle provider_specific_fields.thought_signatures delta
            psf = delta.get("provider_specific_fields")
            if psf and isinstance(psf, dict):
                sigs = psf.get("thought_signatures")
                if sigs and isinstance(sigs, list):
                    for sig in sigs:
                        if isinstance(sig, str):
                            chunks.append(
                                UnifiedStreamChunk.content_block_delta(
                                    0,
                                    ThinkingContent(text="", signature=sig),
                                )
                            )

            # Handle tool_calls delta (streaming tool use)
            # Tool calls start at index 1 (after text content at index 0)
            if "tool_calls" in delta:
                for tc in delta["tool_calls"]:
                    # Calculate the actual content block index:
                    # - Index 0 is reserved for text content
                    # - Tool calls start at index 1
                    # - Handle negative indices safely
                    tc_index = tc.get("index", 0)
                    if tc_index < 0:
                        content_block_index = 1
                    else:
                        content_block_index = tc_index + 1

                    # If this is the first chunk for this tool call (has id),
                    # emit a content_block_start
                    if tc.get("id"):
                        tool_id = tc.get("id", "")
                        tool_name = tc.get("function", {}).get("name", "")
                        chunks.append(
                            UnifiedStreamChunk.content_block_start(
                                content_block_index,
                                ToolUseContent(id=tool_id, name=tool_name, input={}),
                            )
                        )

                    # If there are arguments, emit a tool_input_delta
                    func = tc.get("function", {})
                    if func.get("arguments"):
                        chunks.append(
                            UnifiedStreamChunk.content_block_delta(
                                content_block_index,
                                ToolInputDeltaContent(
                                    index=content_block_index,
                                    partial_json=func["arguments"],
                                ),
                            )
                        )

            # Handle finish reason
            if choice.get("finish_reason"):
                stop_reason = self._parse_finish_reason(choice["finish_reason"])
                usage_data = parsed.get("usage", {})
                usage = UnifiedUsage(
                    input_tokens=usage_data.get("prompt_tokens", 0),
                    output_tokens=usage_data.get("completion_tokens", 0),
                )
                chunks.append(
                    UnifiedStreamChunk.message_delta(
                        stop_reason or StopReason.END_TURN, usage
                    )
                )
                emitted_message_delta = True

        # Handle usage in chunks without finish_reason
        # OpenAI may send usage in a separate final chunk or in a chunk with empty choices
        if not emitted_message_delta and "usage" in parsed:
            usage_data = parsed["usage"]
            usage = UnifiedUsage(
                input_tokens=usage_data.get("prompt_tokens", 0),
                output_tokens=usage_data.get("completion_tokens", 0),
            )
            chunks.append(UnifiedStreamChunk.message_delta(StopReason.END_TURN, usage))

        return chunks
