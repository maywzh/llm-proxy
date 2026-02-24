# Transform Pipeline
#
# This module provides the TransformPipeline class that orchestrates the
# complete transformation flow between client and provider protocols.

from abc import ABC, abstractmethod
from typing import Any, Optional, TYPE_CHECKING

from .base import TransformContext
from .rectifier import sanitize_provider_payload
from .registry import TransformerRegistry
from .unified import (
    Protocol,
    UnifiedRequest,
    UnifiedResponse,
    UnifiedStreamChunk,
    ThinkingContent,
)

if TYPE_CHECKING:
    from app.scripting.engine import LuaEngine


# =============================================================================
# Feature Transformer Interface
# =============================================================================


class FeatureTransformer(ABC):
    """
    Feature transformer trait for cross-cutting concerns.

    Feature transformers are applied to the Unified Internal Format (UIF)
    to handle concerns that span multiple protocols.
    """

    @abstractmethod
    def transform_request(self, request: UnifiedRequest) -> None:
        """Transform request before sending to provider."""
        pass

    @abstractmethod
    def transform_response(self, response: UnifiedResponse) -> None:
        """Transform response before returning to client."""
        pass

    @abstractmethod
    def transform_stream_chunk(self, chunk: UnifiedStreamChunk) -> None:
        """Transform streaming chunk."""
        pass

    @property
    @abstractmethod
    def name(self) -> str:
        """Feature name for logging/debugging."""
        pass


# =============================================================================
# Built-in Feature Transformers
# =============================================================================


class ReasoningTransformer(FeatureTransformer):
    """Transformer for handling thinking/reasoning blocks."""

    def __init__(self, include_thinking: bool = True) -> None:
        self._include_thinking = include_thinking

    @staticmethod
    def _is_thinking_content(content: Any) -> bool:
        return isinstance(content, ThinkingContent)

    def transform_request(self, request: UnifiedRequest) -> None:
        pass  # No-op for now

    def transform_response(self, response: UnifiedResponse) -> None:
        if not self._include_thinking:
            response.content = [
                c for c in response.content if not self._is_thinking_content(c)
            ]

    def transform_stream_chunk(self, chunk: UnifiedStreamChunk) -> None:
        if not self._include_thinking:
            if chunk.delta and self._is_thinking_content(chunk.delta):
                chunk.delta = None
            if chunk.content_block and self._is_thinking_content(chunk.content_block):
                chunk.content_block = None

    @property
    def name(self) -> str:
        return "reasoning"


class TokenLimitTransformer(FeatureTransformer):
    """Transformer for enforcing token limits."""

    def __init__(
        self, max_tokens: Optional[int] = None, cap_instead_of_reject: bool = True
    ) -> None:
        self._max_tokens = max_tokens
        self._cap_instead_of_reject = cap_instead_of_reject

    def transform_request(self, request: UnifiedRequest) -> None:
        if self._max_tokens is None:
            return

        if request.parameters.max_tokens is not None:
            if request.parameters.max_tokens > self._max_tokens:
                if self._cap_instead_of_reject:
                    request.parameters.max_tokens = self._max_tokens
                else:
                    raise ValueError(
                        f"max_tokens {request.parameters.max_tokens} "
                        f"exceeds limit {self._max_tokens}"
                    )

    def transform_response(self, response: UnifiedResponse) -> None:
        pass  # No-op

    def transform_stream_chunk(self, chunk: UnifiedStreamChunk) -> None:
        pass  # No-op

    @property
    def name(self) -> str:
        return "token_limit"


class FeatureTransformerChain(FeatureTransformer):
    """A chain of feature transformers applied in sequence."""

    def __init__(self) -> None:
        self._transformers: list[FeatureTransformer] = []

    def add(self, transformer: FeatureTransformer) -> "FeatureTransformerChain":
        """Add a transformer to the chain."""
        self._transformers.append(transformer)
        return self

    def transform_request(self, request: UnifiedRequest) -> None:
        for transformer in self._transformers:
            transformer.transform_request(request)

    def transform_response(self, response: UnifiedResponse) -> None:
        for transformer in self._transformers:
            transformer.transform_response(response)

    def transform_stream_chunk(self, chunk: UnifiedStreamChunk) -> None:
        for transformer in self._transformers:
            transformer.transform_stream_chunk(chunk)

    @property
    def name(self) -> str:
        return "chain"

    def is_empty(self) -> bool:
        return len(self._transformers) == 0

    def names(self) -> list[str]:
        return [t.name for t in self._transformers]


# =============================================================================
# Transform Pipeline
# =============================================================================


class TransformPipeline:
    """
    Execute a transformation pipeline.

    The pipeline supports optional feature transformers that are applied
    to the Unified Internal Format (UIF) between protocol transformations.

    Pipeline Flow:
    ```
    Client Request
        ↓
    [Protocol: transform_request_out]  ← Client format → UIF
        ↓
    [Feature Transformers: transform_request]  ← Applied to UIF
        ↓
    [Protocol: transform_request_in]   ← UIF → Provider format
        ↓
    Provider Backend
        ↓
    [Protocol: transform_response_in]  ← Provider format → UIF
        ↓
    [Feature Transformers: transform_response]  ← Applied to UIF
        ↓
    [Protocol: transform_response_out] ← UIF → Client format
        ↓
    Client Response
    ```
    """

    def __init__(
        self,
        registry: TransformerRegistry,
        feature_transformers: Optional[FeatureTransformer] = None,
        lua_engine: Optional["LuaEngine"] = None,
    ) -> None:
        self._registry = registry
        self._feature_transformers = feature_transformers
        self._lua_engine = lua_engine

    @classmethod
    def default(cls) -> "TransformPipeline":
        """Create a pipeline with default registry."""
        return cls(TransformerRegistry.default())

    @classmethod
    def with_default_transformers(cls) -> "TransformPipeline":
        """Create a pipeline with default transformers registered."""
        from .protocols.openai import OpenAITransformer
        from .protocols.anthropic import AnthropicTransformer
        from .protocols.response_api import ResponseApiTransformer

        registry = TransformerRegistry()
        registry.register(OpenAITransformer())
        registry.register(AnthropicTransformer())
        registry.register(ResponseApiTransformer())
        return cls(registry)

    def with_features(self, features: FeatureTransformer) -> "TransformPipeline":
        """Create a new pipeline with feature transformers."""
        return TransformPipeline(self._registry, features)

    def set_features(self, features: FeatureTransformer) -> None:
        """Set feature transformers (mutates this pipeline)."""
        self._feature_transformers = features

    def has_features(self) -> bool:
        """Check if feature transformers are configured."""
        return self._feature_transformers is not None

    @property
    def registry(self) -> TransformerRegistry:
        """Get the transformer registry."""
        return self._registry

    @property
    def features(self) -> Optional[FeatureTransformer]:
        """Get the feature transformers (if any)."""
        return self._feature_transformers

    def set_lua_engine(self, engine: "LuaEngine") -> None:
        """Set the Lua scripting engine."""
        self._lua_engine = engine

    @property
    def lua_engine(self) -> Optional["LuaEngine"]:
        return self._lua_engine

    def has_lua_script(self, provider_name: str) -> bool:
        if self._lua_engine is None:
            return False
        return self._lua_engine.has_script(provider_name)

    def has_lua_transform_hooks(self, provider_name: str) -> bool:
        if self._lua_engine is None:
            return False
        return self._lua_engine.has_transform_hooks(provider_name)

    # =========================================================================
    # Core Transformation Methods
    # =========================================================================

    def transform_request(
        self, raw: dict[str, Any], ctx: TransformContext
    ) -> dict[str, Any]:
        """Transform a client request to provider format."""
        client_transformer = self._registry.get_or_error(ctx.client_protocol)
        provider_transformer = self._registry.get_or_error(ctx.provider_protocol)

        client_proto = (
            str(ctx.client_protocol.value)
            if hasattr(ctx.client_protocol, "value")
            else str(ctx.client_protocol)
        )
        provider_proto = (
            str(ctx.provider_protocol.value)
            if hasattr(ctx.provider_protocol, "value")
            else str(ctx.provider_protocol)
        )

        # Step 1: Client format → Unified
        # Try Lua on_transform_request_out first; fallback to hardcoded.
        unified: Optional[UnifiedRequest] = None
        if self._lua_engine is not None:
            uif_dict = self._lua_engine.call_on_transform_request_out(
                ctx.provider_name,
                raw,
                ctx.original_model,
                client_proto,
                provider_proto,
            )
            if uif_dict is not None:
                unified = UnifiedRequest.model_validate(uif_dict)

        if unified is None:
            unified = client_transformer.transform_request_out(raw)

        # Update model name if mapped
        if ctx.mapped_model and ctx.mapped_model != ctx.original_model:
            unified.model = ctx.mapped_model

        # Step 2: Apply feature transformers to UIF
        if self._feature_transformers:
            self._feature_transformers.transform_request(unified)

        # Step 3: Unified → Provider format
        # Try Lua on_transform_request_in first; fallback to hardcoded.
        if self._lua_engine is not None:
            provider_json = self._lua_engine.call_on_transform_request_in(
                ctx.provider_name,
                unified.model_dump(exclude_none=True),
                ctx.original_model,
                client_proto,
                provider_proto,
            )
            if provider_json is not None:
                return provider_json

        return provider_transformer.transform_request_in(unified)

    def transform_response(
        self, raw: dict[str, Any], ctx: TransformContext
    ) -> dict[str, Any]:
        """Transform a provider response to client format."""
        client_transformer = self._registry.get_or_error(ctx.client_protocol)
        provider_transformer = self._registry.get_or_error(ctx.provider_protocol)

        client_proto = (
            str(ctx.client_protocol.value)
            if hasattr(ctx.client_protocol, "value")
            else str(ctx.client_protocol)
        )
        provider_proto = (
            str(ctx.provider_protocol.value)
            if hasattr(ctx.provider_protocol, "value")
            else str(ctx.provider_protocol)
        )

        # Step 1: Provider format → Unified
        # Try Lua on_transform_response_in first; fallback to hardcoded.
        unified: Optional[UnifiedResponse] = None
        if self._lua_engine is not None:
            uif_dict = self._lua_engine.call_on_transform_response_in(
                ctx.provider_name,
                raw,
                ctx.original_model,
                client_proto,
                provider_proto,
            )
            if uif_dict is not None:
                unified = UnifiedResponse.model_validate(uif_dict)

        if unified is None:
            unified = provider_transformer.transform_response_in(
                raw, ctx.original_model
            )

        # Restore original model name for client
        unified.model = ctx.original_model

        # Step 2: Apply feature transformers to UIF
        if self._feature_transformers:
            self._feature_transformers.transform_response(unified)

        # Step 3: Unified → Client format
        # Try Lua on_transform_response_out first; fallback to hardcoded.
        if self._lua_engine is not None:
            client_json = self._lua_engine.call_on_transform_response_out(
                ctx.provider_name,
                unified.model_dump(exclude_none=True),
                ctx.original_model,
                client_proto,
                provider_proto,
            )
            if client_json is not None:
                return client_json

        return client_transformer.transform_response_out(unified, ctx.client_protocol)

    def transform_stream_chunk(self, chunk: UnifiedStreamChunk) -> None:
        """
        Transform a streaming chunk (applies feature transformers).

        Args:
            chunk: Unified stream chunk to transform (mutated in place)
        """
        if self._feature_transformers:
            self._feature_transformers.transform_stream_chunk(chunk)

    # =========================================================================
    # Bypass Mode Methods
    # =========================================================================

    def should_bypass(self, ctx: TransformContext) -> bool:
        """
        Check if bypass mode should be used.

        Bypass mode is used when:
        1. Client and provider use the same protocol
        2. No feature transformers are configured
        3. No Lua script or transform hooks for the provider
        """
        return (
            ctx.is_same_protocol()
            and not self.has_features()
            and not self.has_lua_script(ctx.provider_name)
            and not self.has_lua_transform_hooks(ctx.provider_name)
        )

    def transform_request_with_bypass(
        self, raw: dict[str, Any], ctx: TransformContext
    ) -> tuple[dict[str, Any], bool]:
        """
        Transform request with bypass optimization.

        If bypass is possible, only applies model name mapping and returns
        the payload with a flag indicating bypass was used.

        Args:
            raw: Raw request payload from client
            ctx: Transform context

        Returns:
            Tuple of (transformed_payload, bypassed)
        """
        if self.should_bypass(ctx):
            # Bypass mode: only apply model name mapping
            payload = raw.copy()
            if ctx.mapped_model and ctx.mapped_model != ctx.original_model:
                payload["model"] = ctx.mapped_model

            # Sanitize empty assistant messages for Anthropic-compatible protocols.
            # Anthropic API rejects non-final assistant messages with empty content.
            if ctx.provider_protocol in (Protocol.ANTHROPIC, Protocol.GCP_VERTEX):
                _sanitize_empty_assistant_messages(payload)

            # Sanitize payload before sending to provider
            sanitize_provider_payload(payload)
            ensure_tool_use_result_pairing(payload)

            return payload, True
        else:
            # Full transformation
            payload = self.transform_request(raw, ctx)
            # Sanitize payload before sending to provider
            sanitize_provider_payload(payload)
            ensure_tool_use_result_pairing(payload)
            return payload, False

    def transform_response_with_bypass(
        self, raw: dict[str, Any], ctx: TransformContext
    ) -> tuple[dict[str, Any], bool]:
        """
        Transform response with bypass optimization.

        If bypass is possible, only applies model name restoration and returns
        the payload with a flag indicating bypass was used.

        Args:
            raw: Raw response payload from provider
            ctx: Transform context

        Returns:
            Tuple of (transformed_payload, bypassed)
        """
        if self.should_bypass(ctx):
            # Bypass mode: only restore original model name
            payload = raw.copy()
            payload["model"] = ctx.original_model
            return payload, True
        else:
            # Full transformation
            return self.transform_response(raw, ctx), False

    # =========================================================================
    # Streaming Transformation Methods
    # =========================================================================

    def transform_stream_chunk_in(
        self, chunk: bytes, ctx: TransformContext
    ) -> list[UnifiedStreamChunk]:
        """
        Transform streaming chunk from provider format to unified chunks.

        Args:
            chunk: Raw bytes from provider stream
            ctx: Transform context

        Returns:
            List of unified stream chunks
        """
        provider_transformer = self._registry.get_or_error(ctx.provider_protocol)
        chunks = provider_transformer.transform_stream_chunk_in(chunk)

        # Apply feature transformers to each chunk
        if self._feature_transformers:
            for c in chunks:
                self._feature_transformers.transform_stream_chunk(c)

        return chunks

    def transform_stream_chunk_out(
        self, chunk: UnifiedStreamChunk, ctx: TransformContext
    ) -> str:
        """
        Transform unified streaming chunk to client format.

        Args:
            chunk: Unified stream chunk
            ctx: Transform context

        Returns:
            SSE-formatted string for the chunk
        """
        client_transformer = self._registry.get_or_error(ctx.client_protocol)
        return client_transformer.transform_stream_chunk_out(chunk, ctx.client_protocol)


def _sanitize_empty_assistant_messages(payload: dict[str, Any]) -> None:
    """
    Sanitize empty assistant messages in Anthropic-format payloads.

    Anthropic API requires all messages to have non-empty content,
    except for the optional final assistant message (used for prefill).
    Replaces empty content in non-final assistant messages with a placeholder.
    """
    messages = payload.get("messages")
    if not isinstance(messages, list) or len(messages) <= 1:
        return
    for i in range(len(messages) - 1):
        if messages[i].get("role") != "assistant":
            continue
        c = messages[i].get("content")
        is_empty = (isinstance(c, str) and not c) or (
            isinstance(c, list) and len(c) == 0
        )
        if is_empty:
            messages[i]["content"] = "null"


def ensure_tool_use_result_pairing(payload: dict[str, Any]) -> None:
    """
    Ensure every tool_use / tool_call has a matching tool_result response.

    Providers like AWS Bedrock Converse strictly validate that every tool_use
    block is immediately followed by a corresponding tool_result. When a client
    sends an incomplete conversation (e.g. tool call was cancelled), the missing
    result causes a 400 error.

    Handles both payload formats:
      - OpenAI: assistant messages carry ``tool_calls``; results appear as
        subsequent ``{"role":"tool","tool_call_id":"..."}`` messages.
      - Anthropic: ``tool_use`` and ``tool_result`` are content blocks inside
        the ``content`` array.

    For any unpaired tool_use, a placeholder tool_result is injected so that
    downstream providers never see an orphan.
    """
    messages = payload.get("messages")
    if not isinstance(messages, list):
        return

    # Collect all tool_result / tool-role IDs present in the conversation.
    result_ids: set[str] = set()
    for msg in messages:
        # OpenAI format: role:"tool" with tool_call_id
        if msg.get("role") == "tool":
            tid = msg.get("tool_call_id")
            if isinstance(tid, str):
                result_ids.add(tid)
        # Anthropic format: content blocks with type:"tool_result"
        content = msg.get("content")
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict) and block.get("type") == "tool_result":
                    tid = block.get("tool_use_id")
                    if isinstance(tid, str):
                        result_ids.add(tid)

    # Walk messages, collect orphaned tool_use IDs per assistant message index.
    inserts: list[tuple[int, list[str]]] = []
    for i, msg in enumerate(messages):
        if msg.get("role") != "assistant":
            continue
        orphans: list[str] = []

        # OpenAI format: tool_calls array on assistant message
        tool_calls = msg.get("tool_calls")
        if isinstance(tool_calls, list):
            for call in tool_calls:
                tid = call.get("id") if isinstance(call, dict) else None
                if isinstance(tid, str) and tid not in result_ids:
                    orphans.append(tid)

        # Anthropic format: tool_use content blocks
        content = msg.get("content")
        if isinstance(content, list):
            for block in content:
                if isinstance(block, dict) and block.get("type") == "tool_use":
                    tid = block.get("id")
                    if isinstance(tid, str) and tid not in result_ids:
                        orphans.append(tid)

        if orphans:
            inserts.append((i, orphans))

    # Insert placeholders in reverse order to preserve indices.
    for assistant_idx, orphan_ids in reversed(inserts):
        insert_pos = assistant_idx + 1
        use_openai_format = isinstance(messages[assistant_idx].get("tool_calls"), list)

        if use_openai_format:
            # OpenAI: one role:"tool" message per orphan (insert in reverse)
            for tid in reversed(orphan_ids):
                messages.insert(
                    insert_pos,
                    {
                        "role": "tool",
                        "tool_call_id": tid,
                        "content": "[Tool call interrupted - no result available]",
                    },
                )
        else:
            # Anthropic: one user message with tool_result blocks
            blocks = [
                {
                    "type": "tool_result",
                    "tool_use_id": tid,
                    "content": "[Tool call interrupted - no result available]",
                    "is_error": True,
                }
                for tid in orphan_ids
            ]
            messages.insert(insert_pos, {"role": "user", "content": blocks})
