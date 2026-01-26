# Transformer Base Classes
#
# This module defines the abstract base class for protocol transformers
# and the TransformContext dataclass for passing context through the pipeline.

from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Any

from .unified import (
    Protocol,
    UnifiedRequest,
    UnifiedResponse,
    UnifiedStreamChunk,
)


# =============================================================================
# Transform Context
# =============================================================================


@dataclass
class TransformContext:
    """Context passed through the transformation pipeline."""

    request_id: str = ""
    client_protocol: Protocol = Protocol.OPENAI
    provider_protocol: Protocol = Protocol.OPENAI
    original_model: str = ""
    mapped_model: str = ""
    provider_name: str = ""
    stream: bool = False
    metadata: dict[str, Any] = field(default_factory=dict)

    def is_same_protocol(self) -> bool:
        """Check if this is a same-protocol transformation."""
        return self.client_protocol == self.provider_protocol


# =============================================================================
# Transformer Abstract Base Class
# =============================================================================


class Transformer(ABC):
    """
    Abstract base class for protocol transformers.

    Each protocol implements this class to provide bidirectional conversion
    between its native format and the Unified Internal Format.

    The 4-Hook Transformation Model:
    1. transform_request_out: Client Format → UIF
    2. transform_request_in: UIF → Provider Format
    3. transform_response_in: Provider Format → UIF
    4. transform_response_out: UIF → Client Format
    """

    @property
    @abstractmethod
    def protocol(self) -> Protocol:
        """Get the protocol this transformer handles."""
        pass

    @abstractmethod
    def transform_request_out(self, raw: dict[str, Any]) -> UnifiedRequest:
        """
        Transform external request format to Unified Internal Format.

        Hook: transform_request_out (Client → Unified)

        Args:
            raw: Raw request payload as dict

        Returns:
            UnifiedRequest in UIF format

        Raises:
            ValueError: If request format is invalid
        """
        pass

    @abstractmethod
    def transform_request_in(self, unified: UnifiedRequest) -> dict[str, Any]:
        """
        Transform Unified Internal Format to provider request format.

        Hook: transform_request_in (Unified → Provider)

        Args:
            unified: Request in UIF format

        Returns:
            Request payload in provider's format
        """
        pass

    @abstractmethod
    def transform_response_in(
        self, raw: dict[str, Any], original_model: str
    ) -> UnifiedResponse:
        """
        Transform provider response to Unified Internal Format.

        Hook: transform_response_in (Provider → Unified)

        Args:
            raw: Raw response payload from provider
            original_model: Original model name from client request

        Returns:
            UnifiedResponse in UIF format
        """
        pass

    @abstractmethod
    def transform_response_out(
        self, unified: UnifiedResponse, client_protocol: Protocol
    ) -> dict[str, Any]:
        """
        Transform Unified Internal Format to client response format.

        Hook: transform_response_out (Unified → Client)

        Args:
            unified: Response in UIF format
            client_protocol: Client's expected protocol

        Returns:
            Response payload in client's expected format
        """
        pass

    @abstractmethod
    def transform_stream_chunk_in(self, chunk: bytes) -> list[UnifiedStreamChunk]:
        """
        Transform streaming chunk from provider format to unified chunks.

        Returns a list because one provider chunk might map to multiple unified chunks.

        Args:
            chunk: Raw bytes from provider stream

        Returns:
            List of UnifiedStreamChunk
        """
        pass

    @abstractmethod
    def transform_stream_chunk_out(
        self, chunk: UnifiedStreamChunk, client_protocol: Protocol
    ) -> str:
        """
        Transform unified streaming chunk to client format.

        Returns the SSE-formatted string for the chunk.

        Args:
            chunk: Unified stream chunk
            client_protocol: Client's expected protocol

        Returns:
            SSE-formatted string
        """
        pass

    @property
    @abstractmethod
    def endpoint(self) -> str:
        """Get the endpoint path for this protocol."""
        pass

    @property
    def content_type(self) -> str:
        """Get the content type for requests."""
        return "application/json"

    @abstractmethod
    def can_handle(self, raw: dict[str, Any]) -> bool:
        """
        Check if this transformer can handle the given request.

        Used by the protocol detector to auto-detect the format.

        Args:
            raw: Raw request payload

        Returns:
            True if this transformer can handle the request
        """
        pass
