"""Provider selection and management service"""
import random
from typing import Dict

from app.models.provider import Provider
from app.core.config import get_config


class ProviderService:
    """Manages provider selection with weighted load balancing"""
    
    def __init__(self):
        self._providers: list[Provider] = []
        self._weights: list[int] = []
        self._initialized = False
    
    def initialize(self) -> None:
        """Initialize providers from configuration"""
        if self._initialized:
            return
        
        config = get_config()
        self._providers = [
            Provider(
                name=p.name,
                api_base=p.api_base,
                api_key=p.api_key,
                weight=p.weight,
                model_mapping=p.model_mapping
            )
            for p in config.providers
        ]
        self._weights = [p.weight for p in self._providers]
        self._initialized = True
    
    def get_next_provider(self) -> Provider:
        """Get next provider based on weighted random selection"""
        if not self._initialized:
            self.initialize()
        return random.choices(self._providers, weights=self._weights, k=1)[0]
    
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
        """Get all unique model names from all providers"""
        if not self._initialized:
            self.initialize()
        models = set()
        for provider in self._providers:
            models.update(provider.model_mapping.keys())
        return models


_provider_service: ProviderService | None = None


def get_provider_service() -> ProviderService:
    """Get singleton provider service instance"""
    global _provider_service
    if _provider_service is None:
        _provider_service = ProviderService()
    return _provider_service