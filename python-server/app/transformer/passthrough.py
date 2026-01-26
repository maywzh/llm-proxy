# Passthrough Transformer
#
# This module provides a passthrough transformer for same-protocol scenarios.
# It enables zero-copy streaming for optimal performance when client and provider
# use the same protocol.

import json
from typing import Any, Optional

from .base import Transformer, TransformContext
from .unified import (
    ChunkType,
    Protocol,
    Role,
    StopReason,
    UnifiedContent,
    UnifiedMessage,
    UnifiedParameters,
    UnifiedRequest,
    UnifiedResponse,
    UnifiedStreamChunk,
    UnifiedUsage,
    create_text_content,
)


# =============================================================================
# Passthrough Transformer
# =============================================================================


class PassthroughTransformer(Transformer):
    """
    Passthrough transformer for same-protocol scenarios.

    This transformer performs minimal transformation when client and provider
    use the same protocol. It only applies model name mapping if needed.

    Performance Benefits:
    - Avoids JSON parsing/serialization overhead
    - Enables zero-copy streaming (bytes pass through unchanged)
    - Reduces memory allocations
    - Lower latency for same-protocol scenarios
    """

    def __init__(self, protocol: Protocol) -> None:
        self._protocol = protocol

    @property
    def protocol(self) -> Protocol:
        return self._protocol

    @property
    def endpoint(self) -> str:
        if self._protocol == Protocol.OPENAI:
            return "/v1/chat/completions"
        elif self._protocol == Protocol.ANTHROPIC:
            return "/v1/messages"
        elif self._protocol == Protocol.RESPONSE_API:
            return "/v1/responses"
        return "/v1/chat/completions"

    def can_handle(self, raw: dict[str, Any]) -> bool:
        """Passthrough transformer can handle any request of its protocol."""
        return True

    # =========================================================================
    # Request Transformation
    # =========================================================================

    def transform_request_out(self, raw: dict[str, Any]) -> UnifiedRequest:
        """
        Transform external request format to Unified Internal Format.

        For passthrough, we create a minimal UnifiedRequest.
        This is only used when we need to go through the full pipeline.
        """
        model = raw.get("model", "unknown")

        messages = []
        for msg in raw.get("messages", []):
            role_str = msg.get("role", "user")
            try:
                role = Role.from_string(role_str)
            except ValueError:
                role = Role.USER

            content_raw = msg.get("content", "")
            if isinstance(content_raw, str):
                content = [create_text_content(content_raw)]
            else:
                content = [create_text_content(str(content_raw))]

            messages.append(UnifiedMessage(role=role, content=content))

        parameters = UnifiedParameters(
            temperature=raw.get("temperature"),
            max_tokens=raw.get("max_tokens"),
            top_p=raw.get("top_p"),
            stream=raw.get("stream", False),
        )

        return UnifiedRequest(
            model=model,
            messages=messages,
            parameters=parameters,
            client_protocol=self._protocol,
        )

    def transform_request_in(self, unified: UnifiedRequest) -> dict[str, Any]:
        """
        Transform Unified Internal Format to provider request format.

        For passthrough, we reconstruct a minimal request.
        """
        messages = []
        for msg in unified.messages:
            content = msg.text_content()
            messages.append({"role": msg.role.value, "content": content})

        result: dict[str, Any] = {"model": unified.model, "messages": messages}

        # Add optional parameters
        if unified.parameters.max_tokens is not None:
            result["max_tokens"] = unified.parameters.max_tokens
        if unified.parameters.temperature is not None:
            result["temperature"] = unified.parameters.temperature
        if unified.parameters.top_p is not None:
            result["top_p"] = unified.parameters.top_p
        if unified.parameters.stream:
            result["stream"] = True

        return result

    # =========================================================================
    # Response Transformation
    # =========================================================================

    def transform_response_in(
        self, raw: dict[str, Any], original_model: str
    ) -> UnifiedResponse:
        """
        Transform provider response to Unified Internal Format.

        For passthrough, we create a minimal UnifiedResponse.
        """
        response_id = raw.get("id", "unknown")

        # Extract content from choices
        content_text = ""
        choices = raw.get("choices", [])
        if choices:
            message = choices[0].get("message", {})
            content_text = message.get("content", "")

        # Extract usage
        usage_data = raw.get("usage", {})
        usage = UnifiedUsage(
            input_tokens=usage_data.get("prompt_tokens", 0),
            output_tokens=usage_data.get("completion_tokens", 0),
        )

        return UnifiedResponse.text(response_id, original_model, content_text, usage)

    def transform_response_out(
        self, unified: UnifiedResponse, client_protocol: Protocol
    ) -> dict[str, Any]:
        """
        Transform Unified Internal Format to client response format.

        For passthrough, we reconstruct a minimal response.
        """
        content = unified.text_content()
        finish_reason = unified.stop_reason.value if unified.stop_reason else "stop"

        return {
            "id": unified.id,
            "model": unified.model,
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": content},
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
    # Streaming Transformation
    # =========================================================================

    def transform_stream_chunk_in(self, chunk: bytes) -> list[UnifiedStreamChunk]:
        """
        Transform streaming chunk from provider format to unified chunks.

        For passthrough, we create minimal unified chunks.
        """
        try:
            chunk_str = chunk.decode("utf-8")
        except UnicodeDecodeError:
            return []

        chunks = []

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

    def _parse_stream_chunk(self, parsed: dict) -> list[UnifiedStreamChunk]:
        """Parse a single stream chunk."""
        chunks = []

        for choice in parsed.get("choices", []):
            delta = choice.get("delta", {})

            # Handle content delta
            if "content" in delta and delta["content"]:
                chunks.append(
                    UnifiedStreamChunk.content_block_delta(
                        0, create_text_content(delta["content"])
                    )
                )

            # Handle finish reason
            finish_reason = choice.get("finish_reason")
            if finish_reason:
                stop_reason = self._parse_finish_reason(finish_reason)
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

        return chunks

    def _parse_finish_reason(self, reason: str) -> Optional[StopReason]:
        """Parse finish reason string to StopReason enum."""
        mapping = {
            "stop": StopReason.END_TURN,
            "end_turn": StopReason.END_TURN,
            "length": StopReason.MAX_TOKENS,
            "max_tokens": StopReason.MAX_TOKENS,
            "tool_calls": StopReason.TOOL_USE,
            "tool_use": StopReason.TOOL_USE,
            "content_filter": StopReason.CONTENT_FILTER,
        }
        return mapping.get(reason, StopReason.END_TURN)

    def transform_stream_chunk_out(
        self, chunk: UnifiedStreamChunk, client_protocol: Protocol
    ) -> str:
        """
        Transform unified streaming chunk to client format.

        For passthrough, we reconstruct SSE format.
        """
        if chunk.chunk_type == ChunkType.CONTENT_BLOCK_DELTA:
            if chunk.delta and hasattr(chunk.delta, "text"):
                data = {
                    "choices": [
                        {"index": chunk.index, "delta": {"content": chunk.delta.text}}
                    ]
                }
                return f"data: {json.dumps(data)}\n\n"

        elif chunk.chunk_type == ChunkType.MESSAGE_DELTA:
            if chunk.stop_reason:
                data: dict[str, Any] = {
                    "choices": [
                        {
                            "index": chunk.index,
                            "delta": {},
                            "finish_reason": chunk.stop_reason.value,
                        }
                    ]
                }
                if chunk.usage:
                    data["usage"] = {
                        "prompt_tokens": chunk.usage.input_tokens,
                        "completion_tokens": chunk.usage.output_tokens,
                        "total_tokens": chunk.usage.total_tokens(),
                    }
                return f"data: {json.dumps(data)}\n\n"

        elif chunk.chunk_type == ChunkType.MESSAGE_STOP:
            return "data: [DONE]\n\n"

        return ""

    # =========================================================================
    # Static Utility Methods
    # =========================================================================

    @staticmethod
    def is_safe_model_name(name: str) -> bool:
        """
        Check if a model name contains only safe characters.

        Safe characters are: alphanumeric, hyphen, underscore, period, forward slash, colon.
        This prevents JSON injection attacks when using string replacement.

        Args:
            name: Model name to check

        Returns:
            True if the model name is safe for string replacement
        """
        if not name:
            return False
        return all(
            c.isalnum() or c in "-_./:" for c in name
        )

    @staticmethod
    def apply_model_mapping(payload: dict[str, Any], mapped_model: str) -> None:
        """
        Apply model name mapping to a JSON payload.

        This modifies the "model" field in-place if present.
        """
        if "model" in payload:
            payload["model"] = mapped_model

    @staticmethod
    def apply_model_mapping_json_safe(payload: bytes, mapped_model: str) -> Optional[bytes]:
        """
        Apply model name mapping using safe JSON parsing.

        This is the safe fallback that always works but is slower.

        Args:
            payload: Raw request bytes
            mapped_model: Model name to set

        Returns:
            Modified bytes, or None if parsing failed
        """
        try:
            json_data = json.loads(payload)
            PassthroughTransformer.apply_model_mapping(json_data, mapped_model)
            return json.dumps(json_data).encode("utf-8")
        except json.JSONDecodeError:
            return None

    @staticmethod
    def apply_model_mapping_bytes(
        payload: bytes, original_model: str, mapped_model: str
    ) -> Optional[bytes]:
        """
        Apply model name mapping to raw bytes.

        This is a fast path that avoids full JSON parsing when possible.
        Falls back to safe JSON parsing if model names contain unsafe characters.
        Returns the modified bytes if mapping was applied, or None if the
        original bytes should be used.
        """
        # If models are the same, no mapping needed
        if original_model == mapped_model:
            return None

        # Security check: if model names contain unsafe characters, use safe JSON parsing
        if not PassthroughTransformer.is_safe_model_name(original_model) or \
           not PassthroughTransformer.is_safe_model_name(mapped_model):
            return PassthroughTransformer.apply_model_mapping_json_safe(payload, mapped_model)

        try:
            payload_str = payload.decode("utf-8")
        except UnicodeDecodeError:
            return None

        # Look for "model": "original_model" pattern
        search_pattern = f'"model":"{original_model}"'
        replace_pattern = f'"model":"{mapped_model}"'

        if search_pattern in payload_str:
            return payload_str.replace(search_pattern, replace_pattern).encode("utf-8")

        # Try with space after colon
        search_pattern = f'"model": "{original_model}"'
        replace_pattern = f'"model": "{mapped_model}"'

        if search_pattern in payload_str:
            return payload_str.replace(search_pattern, replace_pattern).encode("utf-8")

        # Fall back to full JSON parsing
        return PassthroughTransformer.apply_model_mapping_json_safe(payload, mapped_model)

    @staticmethod
    def rewrite_model_in_chunk(
        chunk: bytes, original_model: str, mapped_model: str
    ) -> Optional[bytes]:
        """
        Rewrite model name in streaming chunk bytes.

        This is optimized for SSE streaming where we need to rewrite
        the model field in each chunk without full parsing.
        Returns the modified bytes if rewriting was applied, or None if
        the original bytes should be used.
        """
        # If models are the same, no rewriting needed
        if original_model == mapped_model:
            return None

        # Security check: if model names contain unsafe characters, skip rewriting
        if not PassthroughTransformer.is_safe_model_name(original_model) or \
           not PassthroughTransformer.is_safe_model_name(mapped_model):
            return None

        try:
            chunk_str = chunk.decode("utf-8")
        except UnicodeDecodeError:
            return None

        # Fast check: does this chunk contain the model we need to replace?
        if mapped_model not in chunk_str:
            return None

        # Replace model name in the chunk
        # Provider returns mapped_model, we need to restore original_model
        search_pattern = f'"model":"{mapped_model}"'
        replace_pattern = f'"model":"{original_model}"'

        if search_pattern in chunk_str:
            return chunk_str.replace(search_pattern, replace_pattern).encode("utf-8")

        # Try with space after colon
        search_pattern = f'"model": "{mapped_model}"'
        replace_pattern = f'"model": "{original_model}"'

        if search_pattern in chunk_str:
            return chunk_str.replace(search_pattern, replace_pattern).encode("utf-8")

        return None


# =============================================================================
# Bypass Utility Functions
# =============================================================================


def should_bypass(
    client_protocol: Protocol, provider_protocol: Protocol, has_features: bool
) -> bool:
    """
    Check if bypass mode should be used for a transformation.

    Bypass mode is used when:
    1. Client and provider use the same protocol
    2. No feature transformers are configured

    Args:
        client_protocol: The protocol used by the client
        provider_protocol: The protocol used by the provider
        has_features: Whether feature transformers are configured

    Returns:
        True if bypass mode should be used
    """
    return client_protocol == provider_protocol and not has_features


def transform_request_bypass(
    payload: bytes,
    original_model: str,
    mapped_model: str,
    client_protocol: Protocol,
    provider_protocol: Protocol,
    has_features: bool,
) -> Optional[bytes]:
    """
    Transform request bytes with bypass optimization.

    If bypass is possible, only applies model name mapping.
    Otherwise, returns None to indicate full transformation is needed.

    Args:
        payload: Raw request bytes
        original_model: Original model name from client
        mapped_model: Mapped model name for provider
        client_protocol: Client protocol
        provider_protocol: Provider protocol
        has_features: Whether feature transformers are configured

    Returns:
        Transformed bytes if bypass was used, None otherwise
    """
    if not should_bypass(client_protocol, provider_protocol, has_features):
        return None

    # Apply model mapping if needed
    if original_model != mapped_model:
        result = PassthroughTransformer.apply_model_mapping_bytes(
            payload, original_model, mapped_model
        )
        return result if result is not None else payload
    else:
        return payload


def transform_response_bypass(
    payload: bytes,
    original_model: str,
    mapped_model: str,
    client_protocol: Protocol,
    provider_protocol: Protocol,
    has_features: bool,
) -> Optional[bytes]:
    """
    Transform response bytes with bypass optimization.

    If bypass is possible, only restores original model name.
    Otherwise, returns None to indicate full transformation is needed.

    Args:
        payload: Raw response bytes from provider
        original_model: Original model name from client
        mapped_model: Mapped model name used by provider
        client_protocol: Client protocol
        provider_protocol: Provider protocol
        has_features: Whether feature transformers are configured

    Returns:
        Transformed bytes if bypass was used, None otherwise
    """
    if not should_bypass(client_protocol, provider_protocol, has_features):
        return None

    # Restore original model name if needed
    if original_model != mapped_model:
        result = PassthroughTransformer.apply_model_mapping_bytes(
            payload, mapped_model, original_model
        )
        return result if result is not None else payload
    else:
        return payload


def transform_stream_chunk_bypass(
    chunk: bytes,
    original_model: str,
    mapped_model: str,
    client_protocol: Protocol,
    provider_protocol: Protocol,
    has_features: bool,
) -> Optional[bytes]:
    """
    Transform streaming chunk bytes with bypass optimization.

    If bypass is possible, only rewrites model name in the chunk.
    Otherwise, returns None to indicate full transformation is needed.

    Args:
        chunk: Raw streaming chunk bytes from provider
        original_model: Original model name from client
        mapped_model: Mapped model name used by provider
        client_protocol: Client protocol
        provider_protocol: Provider protocol
        has_features: Whether feature transformers are configured

    Returns:
        Transformed bytes if bypass was used, None otherwise
    """
    if not should_bypass(client_protocol, provider_protocol, has_features):
        return None

    # Rewrite model name if needed
    if original_model != mapped_model:
        result = PassthroughTransformer.rewrite_model_in_chunk(
            chunk, original_model, mapped_model
        )
        return result if result is not None else chunk
    else:
        return chunk
