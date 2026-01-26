# Protocol Detector
#
# This module provides protocol auto-detection based on request structure,
# endpoint path, and explicit headers.

from typing import Any, Optional

from .unified import Protocol


class ProtocolDetector:
    """Protocol detector for auto-detecting request format."""

    @staticmethod
    def detect(request: dict[str, Any]) -> Protocol:
        """
        Detect protocol based on request structure.

        Uses heuristics to identify the format:
        - Anthropic: has `max_tokens` (required), may have `system` as top-level field
        - Response API: has `input` field or specific Response API fields
        - OpenAI: default fallback (most common format)

        Args:
            request: Raw request payload

        Returns:
            Detected Protocol enum value
        """
        # Check for Anthropic format first (more specific indicators)
        if ProtocolDetector._is_anthropic_format(request):
            return Protocol.ANTHROPIC

        # Check for Response API format
        if ProtocolDetector._is_response_api_format(request):
            return Protocol.RESPONSE_API

        # Default to OpenAI (most common)
        return Protocol.OPENAI

    @staticmethod
    def detect_from_explicit_header(headers: dict[str, str]) -> Optional[Protocol]:
        """
        Detect protocol from explicit `x-protocol` header.

        Supported values: "openai", "anthropic", "claude", "response", "response-api"

        Args:
            headers: Request headers (case-insensitive keys)

        Returns:
            Protocol if detected from header, None otherwise
        """
        # Handle case-insensitive header lookup
        x_protocol = None
        for key, value in headers.items():
            if key.lower() == "x-protocol":
                x_protocol = value.lower()
                break

        if x_protocol is None:
            return None

        if x_protocol == "openai":
            return Protocol.OPENAI
        elif x_protocol in ("anthropic", "claude"):
            return Protocol.ANTHROPIC
        elif x_protocol in ("response", "response-api"):
            return Protocol.RESPONSE_API

        return None

    @staticmethod
    def detect_with_headers(
        request: dict[str, Any],
        headers: dict[str, str],
        path: str,
    ) -> Protocol:
        """
        Comprehensive protocol detection with all available signals.

        Priority order (highest to lowest):
        1. Explicit `x-protocol` header (most reliable, user-specified)
        2. Path-based detection
        3. Request structure analysis (fallback)

        Args:
            request: Raw request payload
            headers: Request headers
            path: Request path

        Returns:
            Detected Protocol enum value
        """
        # 1. Highest priority: explicit x-protocol header
        protocol = ProtocolDetector.detect_from_explicit_header(headers)
        if protocol is not None:
            return protocol

        # 2. Path-based detection
        protocol = ProtocolDetector.detect_from_path(path)
        if protocol is not None:
            return protocol

        # 3. Fallback to request structure analysis
        return ProtocolDetector.detect(request)

    @staticmethod
    def _is_anthropic_format(request: dict[str, Any]) -> bool:
        """
        Check if request matches Anthropic format.

        Requires multiple conditions to reduce false positives:
        - system + max_tokens together, OR
        - max_tokens + Anthropic-style content blocks

        Args:
            request: Raw request payload

        Returns:
            True if request matches Anthropic format
        """
        has_system_field = "system" in request
        has_max_tokens = "max_tokens" in request

        # Check for Anthropic-style content blocks in messages
        messages = request.get("messages", [])
        has_anthropic_content = any(
            isinstance(msg.get("content"), list)
            and any(
                isinstance(block, dict)
                and block.get("type") in ("text", "image", "tool_use", "tool_result")
                for block in msg.get("content", [])
            )
            for msg in messages
            if isinstance(msg, dict)
        )

        # Require multiple conditions to reduce false positives:
        # - system field + max_tokens (strong Anthropic indicator)
        # - OR max_tokens + Anthropic-style content blocks
        return (has_system_field and has_max_tokens) or (
            has_max_tokens and has_anthropic_content
        )

    @staticmethod
    def _is_response_api_format(request: dict[str, Any]) -> bool:
        """
        Check if request matches Response API format.

        Response API format indicators:
        - Has `input` field (primary indicator)
        - Has `instructions` without `messages`
        - Has `max_output_tokens` instead of `max_tokens`

        Args:
            request: Raw request payload

        Returns:
            True if request matches Response API format
        """
        # Primary indicator: has "input" field
        if "input" in request:
            return True

        # Secondary indicator: has "instructions" without "messages"
        if "instructions" in request and "messages" not in request:
            return True

        # Tertiary indicator: has "max_output_tokens"
        if "max_output_tokens" in request and "max_tokens" not in request:
            return True

        return False

    @staticmethod
    def detect_from_path(path: str) -> Optional[Protocol]:
        """
        Detect protocol from endpoint path.

        Returns Protocol if path clearly indicates a protocol, None otherwise.

        Args:
            path: Request path (e.g., "/v1/chat/completions")

        Returns:
            Protocol if detected from path, None otherwise
        """
        path_lower = path.lower()

        if "/chat/completions" in path_lower:
            return Protocol.OPENAI
        elif "/messages" in path_lower and "/responses" not in path_lower:
            return Protocol.ANTHROPIC
        elif "/responses" in path_lower:
            return Protocol.RESPONSE_API
        elif "/completions" in path_lower and "/chat/" not in path_lower:
            return Protocol.OPENAI

        return None

    @staticmethod
    def detect_with_path_hint(request: dict[str, Any], path: str) -> Protocol:
        """
        Detect protocol with path hint.

        Path-based detection takes priority if available.

        Args:
            request: Raw request payload
            path: Request path

        Returns:
            Detected Protocol enum value
        """
        protocol = ProtocolDetector.detect_from_path(path)
        if protocol is not None:
            return protocol

        return ProtocolDetector.detect(request)

    @staticmethod
    def detect_from_content_type(content_type: str) -> Optional[Protocol]:
        """
        Detect protocol from Content-Type header.

        Some providers use specific content types.

        Args:
            content_type: Content-Type header value

        Returns:
            Protocol if detected from content type, None otherwise
        """
        # Currently all protocols use application/json
        # This method is here for future extensibility
        return None

    @staticmethod
    def detect_from_headers(headers: dict[str, str]) -> Optional[Protocol]:
        """
        Detect protocol from request headers.

        Some providers use specific headers that can help identify the protocol.

        Args:
            headers: Request headers

        Returns:
            Protocol if detected from headers, None otherwise
        """
        # Check for Anthropic-specific headers
        if "anthropic-version" in headers or "x-api-key" in headers:
            return Protocol.ANTHROPIC

        # Check for OpenAI-specific headers
        if "openai-organization" in headers:
            return Protocol.OPENAI

        return None

    @staticmethod
    def detect_comprehensive(
        request: dict[str, Any],
        path: Optional[str] = None,
        headers: Optional[dict[str, str]] = None,
    ) -> Protocol:
        """
        Comprehensive protocol detection using all available signals.

        Priority order:
        1. Path-based detection (most reliable)
        2. Header-based detection
        3. Content-based detection (fallback)

        Args:
            request: Raw request payload
            path: Optional request path
            headers: Optional request headers

        Returns:
            Detected Protocol enum value
        """
        # Try path-based detection first
        if path:
            protocol = ProtocolDetector.detect_from_path(path)
            if protocol is not None:
                return protocol

        # Try header-based detection
        if headers:
            protocol = ProtocolDetector.detect_from_headers(headers)
            if protocol is not None:
                return protocol

        # Fall back to content-based detection
        return ProtocolDetector.detect(request)
