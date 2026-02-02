"""Provider selection and management service"""

import random
from typing import Optional

from app.models.provider import Provider, _is_pattern
from app.core.config import get_config
from app.services.cooldown_service import get_cooldown_service


class ProviderService:
    """Manages provider selection with weighted load balancing.

    This service supports dynamic configuration updates. When providers are
    updated via Admin API, call reinitialize() to reload the configuration.
    """

    def __init__(self):
        self._providers: list[Provider] = []
        self._weights: list[int] = []
        self._initialized = False

    def initialize(self) -> None:
        """Initialize providers from configuration"""
        if self._initialized:
            return
        self._load_providers()
        self._initialized = True

    def reinitialize(self) -> None:
        """Reinitialize providers from configuration (for hot reload)"""
        self._load_providers()
        self._initialized = True

    def _load_providers(self) -> None:
        """Load providers from configuration"""
        config = get_config()
        self._providers = [
            Provider(
                name=p.name,
                api_base=p.api_base,
                api_key=p.api_key,
                weight=p.weight,
                model_mapping=p.model_mapping,
                provider_type=p.provider_type,
            )
            for p in config.providers
        ]
        self._weights = [p.weight for p in self._providers]

    def get_next_provider(self, model: Optional[str] = None) -> Provider:
        """Get next provider based on weighted random selection

        Args:
            model: Optional model name to filter providers that support it

        Returns:
            Selected provider

        Raises:
            ValueError: If no provider supports the requested model or all are in cooldown
        """
        if not self._initialized:
            self.initialize()

        # If no model specified, use all providers with original weights
        if model is None:
            available_providers = self._providers
            available_weights = self._weights
        else:
            # Filter providers that have the requested model (supports wildcard patterns)
            available_providers = []
            available_weights = []

            for provider, weight in zip(self._providers, self._weights):
                if provider.supports_model(model):
                    available_providers.append(provider)
                    available_weights.append(weight)

        # If no provider has the model, raise error
        if not available_providers:
            raise ValueError(f"No provider supports model: {model}")

        # Filter out providers in cooldown
        cooldown_svc = get_cooldown_service()
        available_providers, available_weights = cooldown_svc.filter_available_providers(
            available_providers, available_weights
        )

        # Check if all providers are in cooldown
        if not available_providers:
            all_cooldowns = cooldown_svc.get_all_cooldowns()
            cooldown_info = ", ".join(
                f"{k}({v.remaining_seconds}s)" for k, v in all_cooldowns.items()
            )
            raise ValueError(
                f"All providers for model '{model}' are in cooldown: {cooldown_info}"
            )

        return random.choices(available_providers, weights=available_weights, k=1)[0]

    def get_all_providers(self) -> list[Provider]:
        """Get all configured providers"""
        if not self._initialized:
            self.initialize()
        return self._providers

    def get_provider_weights(self) -> list[int]:
        """Get provider weights"""
        if not self._initialized:
            self.initialize()
        return self._weights

    def get_all_models(self) -> set[str]:
        """Get all unique model names from all providers.

        Note: Wildcard/regex patterns are filtered out from the result.
        Only exact model names are returned for /v1/models compatibility.
        """
        if not self._initialized:
            self.initialize()
        models = set()
        for provider in self._providers:
            for key in provider.model_mapping.keys():
                # Filter out wildcard/regex patterns
                if not _is_pattern(key):
                    models.add(key)
        return models


_provider_service: ProviderService | None = None


def get_provider_service() -> ProviderService:
    """Get singleton provider service instance"""
    global _provider_service
    if _provider_service is None:
        _provider_service = ProviderService()
    return _provider_service
