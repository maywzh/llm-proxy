# Gemini Protocol Transformer
#
# Handles conversion between Google Gemini (Vertex AI) API format
# and the Unified Internal Format (UIF).

import json
import uuid
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
    create_text_content,
    create_thinking_content,
    create_tool_result_content,
    create_tool_use_content,
)


class GeminiTransformer(Transformer):
    """
    Gemini protocol transformer.

    Handles bidirectional conversion between Google Gemini API format
    and the Unified Internal Format (UIF).
    """

    def __init__(self) -> None:
        self._first_chunk_seen = False
        self._content_block_index = 0
        self._active_text_block = False

    def _reset_stream_state(self) -> None:
        self._first_chunk_seen = False
        self._content_block_index = 0
        self._active_text_block = False

    @property
    def protocol(self) -> Protocol:
        return Protocol.GEMINI

    @property
    def endpoint(self) -> str:
        return "/v1/projects"

    # =========================================================================
    # Hook 1: transform_request_out (Client → Unified)
    # =========================================================================

    def transform_request_out(self, raw: dict[str, Any]) -> UnifiedRequest:
        contents = raw.get("contents", [])
        messages: list[UnifiedMessage] = []
        for content in contents:
            messages.append(self._gemini_content_to_unified(content))

        # System instruction
        system: Optional[str] = None
        si = raw.get("systemInstruction")
        if si:
            parts = si.get("parts", [])
            texts = [p.get("text", "") for p in parts if "text" in p]
            if texts:
                system = "\n".join(texts)

        # Generation config
        gen_config = raw.get("generationConfig", {})
        parameters = UnifiedParameters(
            temperature=gen_config.get("temperature"),
            max_tokens=gen_config.get("maxOutputTokens"),
            top_p=gen_config.get("topP"),
            top_k=gen_config.get("topK"),
            stop_sequences=gen_config.get("stopSequences"),
            stream=False,
        )

        # Tools
        tools: list[UnifiedTool] = []
        for tool_group in raw.get("tools", []):
            for decl in tool_group.get("functionDeclarations", []):
                tools.append(self._gemini_tool_to_unified(decl))

        # Tool choice
        tool_choice: Optional[dict[str, Any]] = None
        tool_config = raw.get("toolConfig")
        if tool_config:
            fcc = tool_config.get("functionCallingConfig", {})
            mode = fcc.get("mode", "")
            if mode == "AUTO":
                tool_choice = {"type": "auto"}
            elif mode == "ANY":
                names = fcc.get("allowedFunctionNames", [])
                if len(names) == 1:
                    tool_choice = {"type": "tool", "name": names[0]}
                else:
                    tool_choice = {"type": "any"}
            elif mode == "NONE":
                tool_choice = {"type": "none"}

        model = raw.get("model", "")

        return UnifiedRequest(
            model=model,
            messages=messages,
            system=system,
            parameters=parameters,
            tools=tools,
            tool_choice=tool_choice,
            client_protocol=Protocol.GEMINI,
        )

    # =========================================================================
    # Hook 2: transform_request_in (Unified → Provider)
    # =========================================================================

    def transform_request_in(self, unified: UnifiedRequest) -> dict[str, Any]:
        # Build contents with consecutive same-role merging
        contents: list[dict[str, Any]] = []
        pending_role: Optional[str] = None
        pending_parts: list[dict[str, Any]] = []

        for msg in unified.messages:
            gemini_msg = self._unified_to_gemini_content(msg, unified.messages)
            role = gemini_msg.get("role", "user")
            parts = gemini_msg.get("parts", [])

            if pending_role == role:
                pending_parts.extend(parts)
            else:
                if pending_role is not None and pending_parts:
                    contents.append({"role": pending_role, "parts": pending_parts})
                pending_role = role
                pending_parts = list(parts)

        if pending_role is not None and pending_parts:
            contents.append({"role": pending_role, "parts": pending_parts})

        request: dict[str, Any] = {"contents": contents}

        # System instruction
        if unified.system:
            request["systemInstruction"] = {"parts": [{"text": unified.system}]}

        # Generation config
        gen_config: dict[str, Any] = {}
        if unified.parameters.temperature is not None:
            gen_config["temperature"] = unified.parameters.temperature
        if unified.parameters.max_tokens is not None:
            gen_config["maxOutputTokens"] = unified.parameters.max_tokens
        if unified.parameters.top_p is not None:
            gen_config["topP"] = unified.parameters.top_p
        if unified.parameters.top_k is not None:
            gen_config["topK"] = unified.parameters.top_k
        if unified.parameters.stop_sequences:
            gen_config["stopSequences"] = unified.parameters.stop_sequences
        if gen_config:
            request["generationConfig"] = gen_config

        # Tools
        if unified.tools:
            decls = [self._unified_tool_to_gemini(t) for t in unified.tools]
            request["tools"] = [{"functionDeclarations": decls}]

        # Tool choice
        if unified.tool_choice:
            tc_type = unified.tool_choice.get("type", "auto")
            mode_map = {
                "auto": "AUTO",
                "any": "ANY",
                "required": "ANY",
                "none": "NONE",
                "tool": "ANY",
            }
            gemini_mode = mode_map.get(tc_type, "AUTO")
            fcc: dict[str, Any] = {"mode": gemini_mode}
            if tc_type == "tool":
                name = unified.tool_choice.get("name")
                if name:
                    fcc["allowedFunctionNames"] = [name]
            request["toolConfig"] = {"functionCallingConfig": fcc}

        return request

    # =========================================================================
    # Hook 3: transform_response_in (Provider → Unified)
    # =========================================================================

    def transform_response_in(
        self, raw: dict[str, Any], original_model: str
    ) -> UnifiedResponse:
        candidates = raw.get("candidates", [])
        candidate = candidates[0] if candidates else {}

        parts = candidate.get("content", {}).get("parts", [])

        content: list[UnifiedContent] = []
        tool_calls: list[UnifiedToolCall] = []

        for part in parts:
            for uc in self._part_to_unified_list(part):
                if isinstance(uc, ToolUseContent):
                    tool_calls.append(
                        UnifiedToolCall(
                            id=uc.id,
                            name=uc.name,
                            arguments=uc.input,
                        )
                    )
                content.append(uc)

        stop_reason = self._finish_reason_to_unified(candidate.get("finishReason"))

        usage = self._parse_usage(raw.get("usageMetadata", {}))

        resp_id = raw.get("responseId", "")
        if not resp_id:
            resp_id = str(uuid.uuid4())

        return UnifiedResponse(
            id=resp_id,
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
        parts = self._unified_contents_to_parts(unified.content)

        for tc in unified.tool_calls:
            parts.append({"functionCall": {"name": tc.name, "args": tc.arguments}})

        finish_reason = self._unified_to_finish_reason(unified.stop_reason)

        return {
            "candidates": [
                {
                    "content": {"role": "model", "parts": parts},
                    "finishReason": finish_reason,
                }
            ],
            "usageMetadata": {
                "promptTokenCount": unified.usage.input_tokens,
                "candidatesTokenCount": unified.usage.output_tokens,
                "totalTokenCount": unified.usage.input_tokens
                + unified.usage.output_tokens,
            },
            "modelVersion": unified.model,
        }

    # =========================================================================
    # Streaming: transform_stream_chunk_in
    # =========================================================================

    def transform_stream_chunk_in(self, chunk: bytes) -> list[UnifiedStreamChunk]:
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
                data_str = line[6:]
                try:
                    data = json.loads(data_str)
                except json.JSONDecodeError:
                    continue

                # First chunk: emit MessageStart + ContentBlockStart
                if not self._first_chunk_seen:
                    self._first_chunk_seen = True

                    usage = self._parse_usage(data.get("usageMetadata", {}))
                    model = data.get("modelVersion", "")
                    resp_id = data.get("responseId", "")

                    msg = UnifiedResponse(
                        id=resp_id,
                        model=model,
                        content=[],
                        stop_reason=None,
                        usage=usage,
                        tool_calls=[],
                    )
                    chunks.append(UnifiedStreamChunk.message_start(msg))
                    chunks.append(
                        UnifiedStreamChunk.content_block_start(
                            0, create_text_content("")
                        )
                    )
                    self._content_block_index = 1
                    self._active_text_block = True

                # Extract parts from candidate
                parts = None
                candidates = data.get("candidates", [])
                if candidates:
                    candidate = candidates[0]
                    parts = candidate.get("content", {}).get("parts", [])

                if parts:
                    for part in parts:
                        if "text" in part:
                            text = part["text"]
                            is_thought = part.get("thought", False)
                            if is_thought:
                                chunks.append(
                                    UnifiedStreamChunk.content_block_delta(
                                        0,
                                        create_thinking_content(text),
                                    )
                                )
                            else:
                                chunks.append(
                                    UnifiedStreamChunk.content_block_delta(
                                        0,
                                        create_text_content(text),
                                    )
                                )
                                # Extract thoughtSignature from non-thought text parts
                                sig = part.get("thoughtSignature")
                                if sig:
                                    chunks.append(
                                        UnifiedStreamChunk.content_block_delta(
                                            0,
                                            create_thinking_content("", sig),
                                        )
                                    )
                        elif "functionCall" in part:
                            fc = part["functionCall"]
                            if self._active_text_block:
                                chunks.append(UnifiedStreamChunk.content_block_stop(0))
                                self._active_text_block = False

                            name = fc.get("name", "")
                            args = fc.get("args", {})
                            call_id = f"call_{uuid.uuid4().hex[:24]}"
                            idx = self._content_block_index
                            self._content_block_index += 1

                            chunks.append(
                                UnifiedStreamChunk.content_block_start(
                                    idx,
                                    create_tool_use_content(call_id, name, args),
                                )
                            )
                            args_str = json.dumps(args)
                            chunks.append(
                                UnifiedStreamChunk.content_block_delta(
                                    idx,
                                    ToolInputDeltaContent(
                                        index=idx, partial_json=args_str
                                    ),
                                )
                            )
                            chunks.append(UnifiedStreamChunk.content_block_stop(idx))

                # Check for finish
                finish_reason = None
                if candidates:
                    finish_reason = candidates[0].get("finishReason")

                if finish_reason:
                    if self._active_text_block:
                        chunks.append(UnifiedStreamChunk.content_block_stop(0))
                        self._active_text_block = False

                    usage = self._parse_usage(data.get("usageMetadata", {}))
                    stop = self._finish_reason_to_unified(finish_reason)
                    chunks.append(
                        UnifiedStreamChunk.message_delta(
                            stop or StopReason.END_TURN, usage
                        )
                    )
                    chunks.append(UnifiedStreamChunk.message_stop())

        return chunks

    # =========================================================================
    # Streaming: transform_stream_chunk_out
    # =========================================================================

    def transform_stream_chunk_out(
        self, chunk: UnifiedStreamChunk, client_protocol: Protocol
    ) -> str:
        if chunk.chunk_type == ChunkType.MESSAGE_START:
            return ""

        if chunk.chunk_type == ChunkType.CONTENT_BLOCK_START:
            return ""

        if chunk.chunk_type == ChunkType.CONTENT_BLOCK_DELTA:
            if chunk.delta:
                if isinstance(chunk.delta, TextContent):
                    part = {"text": chunk.delta.text}
                elif isinstance(chunk.delta, ThinkingContent):
                    if not chunk.delta.text and chunk.delta.signature:
                        # Signature-only block
                        part = {"text": "", "thoughtSignature": chunk.delta.signature}
                    else:
                        part = {"thought": True, "text": chunk.delta.text}
                elif isinstance(chunk.delta, ToolInputDeltaContent):
                    try:
                        args = json.loads(chunk.delta.partial_json)
                    except json.JSONDecodeError:
                        return ""
                    part = {"functionCall": {"name": "", "args": args}}
                else:
                    return ""
                event = {
                    "candidates": [{"content": {"role": "model", "parts": [part]}}]
                }
                return f"data: {json.dumps(event)}\n\n"
            return ""

        if chunk.chunk_type == ChunkType.CONTENT_BLOCK_STOP:
            return ""

        if chunk.chunk_type == ChunkType.MESSAGE_DELTA:
            finish_reason = self._unified_to_finish_reason(chunk.stop_reason)
            usage_json = None
            if chunk.usage:
                usage_json = {
                    "promptTokenCount": chunk.usage.input_tokens,
                    "candidatesTokenCount": chunk.usage.output_tokens,
                    "totalTokenCount": chunk.usage.input_tokens
                    + chunk.usage.output_tokens,
                }
            event = {
                "candidates": [
                    {
                        "content": {"role": "model", "parts": []},
                        "finishReason": finish_reason,
                    }
                ],
                "usageMetadata": usage_json,
            }
            return f"data: {json.dumps(event)}\n\n"

        if chunk.chunk_type == ChunkType.MESSAGE_STOP:
            return ""

        if chunk.chunk_type == ChunkType.PING:
            return ""

        return ""

    # =========================================================================
    # Protocol Detection
    # =========================================================================

    def can_handle(self, raw: dict[str, Any]) -> bool:
        if "contents" in raw:
            return True
        if "generationConfig" in raw and "messages" not in raw:
            return True
        return False

    # =========================================================================
    # Helper Methods: Part ↔ UnifiedContent
    # =========================================================================

    def _part_to_unified_list(self, part: dict[str, Any]) -> list[UnifiedContent]:
        """Convert a Gemini part to one or more UnifiedContent blocks.

        Returns a list because a part with `thoughtSignature` produces
        an extra Thinking block with signature only.
        """
        result: list[UnifiedContent] = []

        if "text" in part:
            text = part["text"]
            if part.get("thought", False):
                result.append(create_thinking_content(text))
                return result
            result.append(create_text_content(text))
            sig = part.get("thoughtSignature")
            if sig:
                result.append(create_thinking_content("", sig))
            return result

        if "functionCall" in part:
            fc = part["functionCall"]
            name = fc.get("name", "")
            args = fc.get("args", {})
            call_id = f"call_{uuid.uuid4().hex[:24]}"
            result.append(create_tool_use_content(call_id, name, args))
            sig = part.get("thoughtSignature")
            if sig:
                result.append(create_thinking_content("", sig))
            return result

        if "functionResponse" in part:
            fr = part["functionResponse"]
            name = fr.get("name", "")
            response = fr.get("response")
            result.append(create_tool_result_content(name, response, False))
            return result

        if "inlineData" in part:
            inline = part["inlineData"]
            mime_type = inline.get("mimeType", "")
            data = inline.get("data", "")
            result.append(create_image_base64(mime_type, data))
            sig = part.get("thoughtSignature")
            if sig:
                result.append(create_thinking_content("", sig))
            return result

        return result

    def _unified_to_part(self, content: UnifiedContent) -> Optional[dict[str, Any]]:
        if isinstance(content, TextContent):
            return {"text": content.text}
        if isinstance(content, ThinkingContent):
            if not content.text and content.signature:
                # Signature-only block — handled by _unified_contents_to_parts
                return None
            return {"thought": True, "text": content.text}
        if isinstance(content, ToolUseContent):
            return {"functionCall": {"name": content.name, "args": content.input}}
        if isinstance(content, ToolResultContent):
            return {
                "functionResponse": {
                    "name": content.tool_use_id,
                    "response": content.content,
                }
            }
        if isinstance(content, ImageContent):
            return {
                "inlineData": {
                    "mimeType": content.media_type,
                    "data": content.data,
                }
            }
        return None

    def _unified_contents_to_parts(
        self, contents: list[UnifiedContent]
    ) -> list[dict[str, Any]]:
        """Convert a sequence of UnifiedContent to Gemini parts,
        re-attaching thoughtSignature from Thinking(text="", sig=Some) to the preceding part.
        """
        parts: list[dict[str, Any]] = []
        for content in contents:
            if (
                isinstance(content, ThinkingContent)
                and not content.text
                and content.signature
            ):
                # Signature block — attach to the previous part
                if parts:
                    parts[-1]["thoughtSignature"] = content.signature
            else:
                part = self._unified_to_part(content)
                if part:
                    parts.append(part)
        return parts

    # =========================================================================
    # Helper Methods: Message Conversion
    # =========================================================================

    def _gemini_content_to_unified(self, content: dict[str, Any]) -> UnifiedMessage:
        role_str = content.get("role", "user")
        role = Role.ASSISTANT if role_str == "model" else Role.USER

        parts = content.get("parts", [])
        unified_content: list[UnifiedContent] = []
        tool_calls: list[UnifiedToolCall] = []
        tool_call_id: Optional[str] = None

        for part in parts:
            for uc in self._part_to_unified_list(part):
                if isinstance(uc, ToolUseContent):
                    tool_calls.append(
                        UnifiedToolCall(id=uc.id, name=uc.name, arguments=uc.input)
                    )
                if isinstance(uc, ToolResultContent):
                    tool_call_id = uc.tool_use_id
                unified_content.append(uc)

        # If user message with only functionResponse parts, use Tool role
        effective_role = role
        if (
            role == Role.USER
            and tool_call_id is not None
            and all(isinstance(c, ToolResultContent) for c in unified_content)
        ):
            effective_role = Role.TOOL

        return UnifiedMessage(
            role=effective_role,
            content=unified_content,
            tool_calls=tool_calls,
            tool_call_id=tool_call_id,
        )

    def _unified_to_gemini_content(
        self,
        msg: UnifiedMessage,
        all_messages: list[UnifiedMessage],
    ) -> dict[str, Any]:
        role = "model" if msg.role == Role.ASSISTANT else "user"

        # Separate ToolResultContent (needs name lookup) from other content
        tool_result_contents: list[ToolResultContent] = []
        other_contents: list[UnifiedContent] = []
        for content in msg.content:
            if isinstance(content, ToolResultContent):
                tool_result_contents.append(content)
            else:
                other_contents.append(content)

        # Convert other contents (handles signature re-attachment)
        parts = self._unified_contents_to_parts(other_contents)

        # Convert tool results with name lookup
        for tr in tool_result_contents:
            fn_name = self._find_function_name(tr.tool_use_id, all_messages)
            if fn_name is None:
                fn_name = tr.tool_use_id
            parts.append(
                {
                    "functionResponse": {
                        "name": fn_name,
                        "response": tr.content,
                    }
                }
            )

        # Append tool_calls not already in content
        existing_tc_ids = {c.id for c in msg.content if isinstance(c, ToolUseContent)}
        for tc in msg.tool_calls:
            if tc.id not in existing_tc_ids:
                parts.append({"functionCall": {"name": tc.name, "args": tc.arguments}})

        return {"role": role, "parts": parts}

    @staticmethod
    def _find_function_name(
        tool_use_id: str, messages: list[UnifiedMessage]
    ) -> Optional[str]:
        for msg in messages:
            for content in msg.content:
                if isinstance(content, ToolUseContent) and content.id == tool_use_id:
                    return content.name
            for tc in msg.tool_calls:
                if tc.id == tool_use_id:
                    return tc.name
        return None

    # =========================================================================
    # Helper Methods: Stop Reason
    # =========================================================================

    @staticmethod
    def _finish_reason_to_unified(
        reason: Optional[str],
    ) -> Optional[StopReason]:
        if not reason:
            return None
        mapping = {
            "STOP": StopReason.END_TURN,
            "MAX_TOKENS": StopReason.MAX_TOKENS,
            "SAFETY": StopReason.CONTENT_FILTER,
            "RECITATION": StopReason.CONTENT_FILTER,
            "BLOCKLIST": StopReason.CONTENT_FILTER,
            "PROHIBITED_CONTENT": StopReason.CONTENT_FILTER,
            "SPII": StopReason.CONTENT_FILTER,
        }
        return mapping.get(reason, StopReason.END_TURN)

    @staticmethod
    def _unified_to_finish_reason(reason: Optional[StopReason]) -> str:
        if not reason:
            return "STOP"
        mapping = {
            StopReason.END_TURN: "STOP",
            StopReason.MAX_TOKENS: "MAX_TOKENS",
            StopReason.LENGTH: "MAX_TOKENS",
            StopReason.TOOL_USE: "STOP",
            StopReason.STOP_SEQUENCE: "STOP",
            StopReason.CONTENT_FILTER: "SAFETY",
        }
        return mapping.get(reason, "STOP")

    # =========================================================================
    # Helper Methods: Tool Definitions
    # =========================================================================

    @staticmethod
    def _unified_tool_to_gemini(tool: UnifiedTool) -> dict[str, Any]:
        decl: dict[str, Any] = {
            "name": tool.name,
            "parameters": tool.input_schema,
        }
        if tool.description:
            decl["description"] = tool.description
        return decl

    @staticmethod
    def _gemini_tool_to_unified(decl: dict[str, Any]) -> UnifiedTool:
        return UnifiedTool(
            name=decl.get("name", ""),
            description=decl.get("description"),
            input_schema=decl.get("parameters", {}),
            tool_type="function",
        )

    # =========================================================================
    # Helper Methods: Usage
    # =========================================================================

    @staticmethod
    def _parse_usage(usage_meta: dict[str, Any]) -> UnifiedUsage:
        return UnifiedUsage(
            input_tokens=usage_meta.get("promptTokenCount", 0),
            output_tokens=usage_meta.get("candidatesTokenCount", 0),
            cache_read_tokens=usage_meta.get("cachedContentTokenCount"),
        )
