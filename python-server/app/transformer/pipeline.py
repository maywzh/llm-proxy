# Transform Pipeline
#
# This module provides the TransformPipeline class that orchestrates the
# complete transformation flow between client and provider protocols.

from abc import ABC, abstractmethod
from typing import Any, Optional

from .base import TransformContext
from .registry import TransformerRegistry
from .unified import (
    Protocol,
    UnifiedRequest,
    UnifiedResponse,
    UnifiedStreamChunk,
    ThinkingContent,
)


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
    ) -> None:
        self._registry = registry
        self._feature_transformers = feature_transformers

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

    # =========================================================================
    # Core Transformation Methods
    # =========================================================================

    def transform_request(
        self, raw: dict[str, Any], ctx: TransformContext
    ) -> dict[str, Any]:
        """
        Transform a client request to provider format.

        Args:
            raw: Raw request payload from client
            ctx: Transform context with protocol info

        Returns:
            Transformed request payload for provider
        """
        client_transformer = self._registry.get_or_error(ctx.client_protocol)
        provider_transformer = self._registry.get_or_error(ctx.provider_protocol)

        # Step 1: Client format → Unified
        unified = client_transformer.transform_request_out(raw)

        # Update model name if mapped
        if ctx.mapped_model and ctx.mapped_model != ctx.original_model:
            unified.model = ctx.mapped_model

        # Step 2: Apply feature transformers to UIF
        if self._feature_transformers:
            self._feature_transformers.transform_request(unified)

        # Step 3: Unified → Provider format
        return provider_transformer.transform_request_in(unified)

    def transform_response(
        self, raw: dict[str, Any], ctx: TransformContext
    ) -> dict[str, Any]:
        """
        Transform a provider response to client format.

        Args:
            raw: Raw response payload from provider
            ctx: Transform context with protocol info

        Returns:
            Transformed response payload for client
        """
        client_transformer = self._registry.get_or_error(ctx.client_protocol)
        provider_transformer = self._registry.get_or_error(ctx.provider_protocol)

        # Step 1: Provider format → Unified
        unified = provider_transformer.transform_response_in(raw, ctx.original_model)

        # Restore original model name for client
        unified.model = ctx.original_model

        # Step 2: Apply feature transformers to UIF
        if self._feature_transformers:
            self._feature_transformers.transform_response(unified)

        # Step 3: Unified → Client format
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

        In bypass mode, requests/responses pass through with minimal transformation
        (only model name mapping is applied).

        Args:
            ctx: Transform context

        Returns:
            True if bypass mode should be used
        """
        return ctx.is_same_protocol() and not self.has_features()

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
            _sanitize_provider_payload(payload)

            return payload, True
        else:
            # Full transformation
            payload = self.transform_request(raw, ctx)
            # Sanitize payload before sending to provider
            _sanitize_provider_payload(payload)
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


# =============================================================================
# Sanitization Helpers
# =============================================================================


def _sanitize_provider_payload(payload: dict[str, Any]) -> None:
    """
    Sanitize the provider-bound payload to prevent validation errors.

    This is the single entry point for all message-level sanitization before
    the payload is sent to any provider. Consolidating the logic here avoids
    scattered, case-by-case fixes across the codebase.

    Steps applied to each message's content array:
      1. Strip thinking blocks — their provider-specific signatures are not
         reusable across providers and a thinking block without a signature
         is also invalid.
      2. Replace blank text fields — providers like Bedrock Converse reject
         content blocks whose text field is blank. Clients (e.g. Claude Code)
         may legitimately send empty text blocks.
      3. Backfill empty assistant content — after the above steps the content
         array may be empty; an empty assistant message is invalid for most
         providers, so we insert a placeholder.
    """
    messages = payload.get("messages")
    if not isinstance(messages, list):
        return
    for msg in messages:
        content = msg.get("content")
        if not isinstance(content, list):
            continue

        # 1. Strip thinking blocks
        content = [
            block
            for block in content
            if not (isinstance(block, dict) and block.get("type") == "thinking")
        ]

        # 2. Replace blank text fields
        for block in content:
            if (
                isinstance(block, dict)
                and block.get("type") == "text"
                and isinstance(block.get("text"), str)
                and not block["text"].strip()
            ):
                block["text"] = "."

        # 3. Backfill empty assistant content
        if not content and msg.get("role") == "assistant":
            content = [{"type": "text", "text": "."}]

        msg["content"] = content


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
