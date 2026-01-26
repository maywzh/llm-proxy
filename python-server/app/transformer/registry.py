# Transformer Registry
#
# This module provides a registry for managing protocol transformers.
# It supports registering, retrieving, and auto-detecting transformers.

from typing import Optional

from .base import Transformer
from .unified import Protocol


class TransformerRegistry:
    """Registry for managing protocol transformers."""

    _instance: Optional["TransformerRegistry"] = None

    def __init__(self) -> None:
        self._transformers: dict[Protocol, Transformer] = {}

    @classmethod
    def get_instance(cls) -> "TransformerRegistry":
        """Get the singleton instance of the registry."""
        if cls._instance is None:
            cls._instance = cls()
        return cls._instance

    @classmethod
    def reset_instance(cls) -> None:
        """Reset the singleton instance (useful for testing)."""
        cls._instance = None

    @classmethod
    def default(cls) -> "TransformerRegistry":
        """
        Create a registry with default transformers.

        Note: This creates a new instance, not the singleton.
        For the singleton with default transformers, use get_default_instance().
        """
        # Import here to avoid circular imports
        # These will be implemented in the protocols/ directory
        registry = cls()
        # Transformers will be registered when protocols are implemented
        # from .protocols.openai import OpenAITransformer
        # from .protocols.anthropic import AnthropicTransformer
        # from .protocols.response_api import ResponseApiTransformer
        # registry.register(OpenAITransformer())
        # registry.register(AnthropicTransformer())
        # registry.register(ResponseApiTransformer())
        return registry

    @classmethod
    def get_default_instance(cls) -> "TransformerRegistry":
        """Get the singleton instance with default transformers registered."""
        instance = cls.get_instance()
        # Register default transformers if not already registered
        # This will be implemented when protocols are available
        return instance

    def register(self, transformer: Transformer) -> None:
        """
        Register a transformer.

        Args:
            transformer: The transformer to register
        """
        self._transformers[transformer.protocol] = transformer

    def unregister(self, protocol: Protocol) -> Optional[Transformer]:
        """
        Unregister a transformer by protocol.

        Args:
            protocol: The protocol to unregister

        Returns:
            The unregistered transformer, or None if not found
        """
        return self._transformers.pop(protocol, None)

    def get(self, protocol: Protocol) -> Optional[Transformer]:
        """
        Get a transformer by protocol.

        Args:
            protocol: The protocol to get the transformer for

        Returns:
            The transformer, or None if not found
        """
        return self._transformers.get(protocol)

    def get_or_error(self, protocol: Protocol) -> Transformer:
        """
        Get a transformer by protocol, raising error if not found.

        Args:
            protocol: The protocol to get the transformer for

        Returns:
            The transformer

        Raises:
            ValueError: If no transformer is registered for the protocol
        """
        transformer = self.get(protocol)
        if transformer is None:
            raise ValueError(f"Unsupported protocol: {protocol}")
        return transformer

    def detect_and_get(self, raw: dict) -> Optional[Transformer]:
        """
        Detect the protocol and get the appropriate transformer.

        Args:
            raw: Raw request payload

        Returns:
            The transformer that can handle the request, or None if not found
        """
        for transformer in self._transformers.values():
            if transformer.can_handle(raw):
                return transformer
        return None

    def protocols(self) -> list[Protocol]:
        """
        List all registered protocols.

        Returns:
            List of registered protocols
        """
        return list(self._transformers.keys())

    def is_registered(self, protocol: Protocol) -> bool:
        """
        Check if a protocol is registered.

        Args:
            protocol: The protocol to check

        Returns:
            True if the protocol is registered
        """
        return protocol in self._transformers

    def __len__(self) -> int:
        """Return the number of registered transformers."""
        return len(self._transformers)

    def __contains__(self, protocol: Protocol) -> bool:
        """Check if a protocol is registered."""
        return protocol in self._transformers
