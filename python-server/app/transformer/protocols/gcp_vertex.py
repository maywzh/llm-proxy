# GCP Vertex AI Protocol Transformer
#
# Handles conversion between GCP Vertex AI format and the Unified Internal Format (UIF).
# GCP Vertex AI uses the same request/response format as Anthropic Messages API.

from typing import Any

from ..unified import Protocol
from .anthropic import AnthropicTransformer


class GcpVertexTransformer(AnthropicTransformer):
    """
    GCP Vertex AI protocol transformer.

    GCP Vertex AI (for Claude models) uses the same request/response format as
    the Anthropic Messages API. This transformer inherits from AnthropicTransformer
    and overrides only the protocol-specific properties.

    Key differences from direct Anthropic API:
    - Different endpoint URL structure (handled in proxy.py)
    - Uses Bearer token authentication (handled in proxy.py)
    - Same message format and content structure
    """

    @property
    def protocol(self) -> Protocol:
        """Get the protocol this transformer handles."""
        return Protocol.GCP_VERTEX

    @property
    def endpoint(self) -> str:
        """Get the endpoint path for this protocol.

        Note: This is not used directly as GCP Vertex has a dynamic URL structure.
        The actual URL is built in proxy.py based on project/location/model.
        """
        return "/v1/projects/{project}/locations/{location}/publishers/{publisher}/models/{model}"

    def can_handle(self, raw: dict[str, Any]) -> bool:
        """
        Check if this transformer can handle the given request.

        GCP Vertex uses the same format as Anthropic, so we delegate to the
        parent class. Protocol detection is primarily done via provider_type.

        Args:
            raw: Raw request payload

        Returns:
            True if this is an Anthropic/GCP Vertex format request
        """
        return super().can_handle(raw)
