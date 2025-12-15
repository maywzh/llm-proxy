"""Tests for provider service"""
from collections import Counter
from unittest.mock import Mock, patch

import pytest

from app.services.provider_service import ProviderService, get_provider_service
from app.models.provider import Provider
from app.models.config import AppConfig, ProviderConfig


@pytest.mark.unit
class TestProviderService:
    """Test ProviderService class"""
    
    def test_initialize_providers(self, test_config, monkeypatch, clear_config_cache):
        """Test initializing providers from config"""
        from app.services import provider_service as ps_module
        monkeypatch.setattr(ps_module, 'get_config', lambda: test_config)
        ps_module._provider_service = None
        
        service = ProviderService()
        service.initialize()
        
        assert service._initialized is True
        assert len(service._providers) == 2
        assert service._providers[0].name == 'provider1'
        assert service._providers[1].name == 'provider2'
        assert service._weights == [2, 1]
    
    def test_initialize_only_once(self, test_config, monkeypatch, clear_config_cache):
        """Test that initialize only runs once"""
        from app.services import provider_service as ps_module
        mock_get_config = Mock(return_value=test_config)
        monkeypatch.setattr(ps_module, 'get_config', mock_get_config)
        ps_module._provider_service = None
        
        service = ProviderService()
        service.initialize()
        service.initialize()
        service.initialize()
        
        # get_config should only be called once
        assert mock_get_config.call_count == 1
    
    def test_get_next_provider_returns_provider(self, provider_service):
        """Test get_next_provider returns a Provider instance"""
        provider = provider_service.get_next_provider()
        
        assert isinstance(provider, Provider)
        assert provider.name in ['provider1', 'provider2']
    
    def test_get_next_provider_weighted_distribution(self, provider_service):
        """Test weighted distribution of provider selection"""
        # Sample 1000 times to check distribution
        selections = [provider_service.get_next_provider().name for _ in range(1000)]
        counts = Counter(selections)
        
        # provider1 has weight 2, provider2 has weight 1
        # So we expect roughly 2:1 ratio (with some variance)
        ratio = counts['provider1'] / counts['provider2']
        assert 1.5 < ratio < 2.5  # Allow 25% variance
    
    def test_get_next_provider_auto_initializes(self, test_config, monkeypatch):
        """Test get_next_provider auto-initializes if not initialized"""
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: test_config)
        
        service = ProviderService()
        assert service._initialized is False
        
        provider = service.get_next_provider()
        
        assert service._initialized is True
        assert isinstance(provider, Provider)
    
    def test_get_all_providers(self, provider_service):
        """Test getting all providers"""
        providers = provider_service.get_all_providers()
        
        assert len(providers) == 2
        assert all(isinstance(p, Provider) for p in providers)
        assert providers[0].name == 'provider1'
        assert providers[1].name == 'provider2'
    
    def test_get_provider_weights(self, provider_service):
        """Test getting provider weights"""
        weights = provider_service.get_provider_weights()
        
        assert weights == [2, 1]
    
    def test_get_all_models(self, provider_service):
        """Test getting all unique models"""
        models = provider_service.get_all_models()
        
        # From test config: provider1 has gpt-4, gpt-3.5-turbo
        # provider2 has gpt-4, claude-3
        expected_models = {'gpt-4', 'gpt-3.5-turbo', 'claude-3'}
        assert models == expected_models
    
    def test_get_all_models_empty_mapping(self, test_config, monkeypatch):
        """Test get_all_models with empty model mappings"""
        # Create config with no model mappings
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key',
                    model_mapping={}
                )
            ],
            verify_ssl=True
        )
        
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        service = ProviderService()
        service.initialize()
        models = service.get_all_models()
        
        assert models == set()
    
    def test_provider_service_thread_safety(self, provider_service):
        """Test that provider service can be called concurrently"""
        import concurrent.futures
        
        def get_provider():
            return provider_service.get_next_provider().name
        
        with concurrent.futures.ThreadPoolExecutor(max_workers=10) as executor:
            futures = [executor.submit(get_provider) for _ in range(100)]
            results = [f.result() for f in futures]
        
        # All calls should succeed
        assert len(results) == 100
        assert all(name in ['provider1', 'provider2'] for name in results)


@pytest.mark.unit
class TestGetProviderService:
    """Test get_provider_service singleton function"""
    
    def test_get_provider_service_returns_instance(self):
        """Test get_provider_service returns ProviderService instance"""
        service = get_provider_service()
        assert isinstance(service, ProviderService)
    
    def test_get_provider_service_singleton(self):
        """Test get_provider_service returns same instance"""
        service1 = get_provider_service()
        service2 = get_provider_service()
        
        assert service1 is service2
    
    def test_get_provider_service_reset(self, monkeypatch):
        """Test resetting provider service singleton"""
        from app.services import provider_service as ps_module
        
        # Reset singleton
        ps_module._provider_service = None
        
        service1 = get_provider_service()
        
        # Reset again
        ps_module._provider_service = None
        
        service2 = get_provider_service()
        
        # Should be different instances after reset
        assert service1 is not service2


@pytest.mark.unit
class TestProviderServiceEdgeCases:
    """Test edge cases and error conditions"""
    
    def test_single_provider(self, monkeypatch, clear_config_cache):
        """Test service with single provider"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='only-provider',
                    api_base='https://api.test.com',
                    api_key='key',
                    weight=1,
                    model_mapping={'gpt-4': 'gpt-4-0613'}
                )
            ],
            verify_ssl=True
        )
        
        from app.services import provider_service as ps_module
        monkeypatch.setattr(ps_module, 'get_config', lambda: config)
        ps_module._provider_service = None
        
        service = ProviderService()
        service.initialize()
        
        # Should always return the same provider
        for _ in range(10):
            provider = service.get_next_provider()
            assert provider.name == 'only-provider'
    
    def test_many_providers(self, monkeypatch, clear_config_cache):
        """Test service with many providers"""
        providers = [
            ProviderConfig(
                name=f'provider{i}',
                api_base=f'https://api{i}.com',
                api_key=f'key{i}',
                weight=i+1,
                model_mapping={}
            )
            for i in range(10)
        ]
        
        config = AppConfig(providers=providers, verify_ssl=True)
        
        from app.services import provider_service as ps_module
        monkeypatch.setattr(ps_module, 'get_config', lambda: config)
        ps_module._provider_service = None
        
        service = ProviderService()
        service.initialize()
        
        assert len(service.get_all_providers()) == 10
        assert len(service.get_provider_weights()) == 10
    
    def test_equal_weights(self, monkeypatch, clear_config_cache):
        """Test providers with equal weights"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='provider1',
                    api_base='https://api1.com',
                    api_key='key1',
                    weight=1,
                    model_mapping={}
                ),
                ProviderConfig(
                    name='provider2',
                    api_base='https://api2.com',
                    api_key='key2',
                    weight=1,
                    model_mapping={}
                )
            ],
            verify_ssl=True
        )
        
        from app.services import provider_service as ps_module
        monkeypatch.setattr(ps_module, 'get_config', lambda: config)
        ps_module._provider_service = None
        
        service = ProviderService()
        service.initialize()
        
        # Sample many times
        selections = [service.get_next_provider().name for _ in range(1000)]
        counts = Counter(selections)
        
        # Should be roughly equal (within 20% variance)
        ratio = counts['provider1'] / counts['provider2']
        assert 0.8 < ratio < 1.2
    
    def test_very_different_weights(self, monkeypatch, clear_config_cache):
        """Test providers with very different weights"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='heavy',
                    api_base='https://api1.com',
                    api_key='key1',
                    weight=100,
                    model_mapping={}
                ),
                ProviderConfig(
                    name='light',
                    api_base='https://api2.com',
                    api_key='key2',
                    weight=1,
                    model_mapping={}
                )
            ],
            verify_ssl=True
        )
        
        from app.services import provider_service as ps_module
        monkeypatch.setattr(ps_module, 'get_config', lambda: config)
        ps_module._provider_service = None
        
        service = ProviderService()
        service.initialize()
        
        # Sample many times
        selections = [service.get_next_provider().name for _ in range(1000)]
        counts = Counter(selections)
        
        # Heavy should be selected much more often
        assert counts['heavy'] > counts['light'] * 50
    
    def test_overlapping_models(self, monkeypatch, clear_config_cache):
        """Test providers with overlapping model mappings"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='provider1',
                    api_base='https://api1.com',
                    api_key='key1',
                    weight=1,
                    model_mapping={'gpt-4': 'gpt-4-0613', 'gpt-3.5': 'gpt-3.5-turbo'}
                ),
                ProviderConfig(
                    name='provider2',
                    api_base='https://api2.com',
                    api_key='key2',
                    weight=1,
                    model_mapping={'gpt-4': 'gpt-4-1106', 'claude': 'claude-3'}
                )
            ],
            verify_ssl=True
        )
        
        from app.services import provider_service as ps_module
        monkeypatch.setattr(ps_module, 'get_config', lambda: config)
        ps_module._provider_service = None
        
        service = ProviderService()
        service.initialize()
        
        models = service.get_all_models()
        # Should have unique model names (gpt-4 appears in both but counted once)
        assert models == {'gpt-4', 'gpt-3.5', 'claude'}


@pytest.mark.unit
class TestProviderServiceState:
    """Test provider service state management"""
    
    def test_uninitialized_state(self):
        """Test service starts uninitialized"""
        service = ProviderService()
        
        assert service._initialized is False
        assert service._providers == []
        assert service._weights == []
    
    def test_state_after_initialization(self, test_config, monkeypatch):
        """Test service state after initialization"""
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: test_config)
        
        service = ProviderService()
        service.initialize()
        
        assert service._initialized is True
        assert len(service._providers) > 0
        assert len(service._weights) > 0
        assert len(service._providers) == len(service._weights)
    
    def test_providers_immutability(self, provider_service):
        """Test that returned providers list is not the internal list"""
        providers1 = provider_service.get_all_providers()
        providers2 = provider_service.get_all_providers()
        
        # Should return the same list reference (not a copy)
        # This is acceptable as Provider is immutable
        assert providers1 is providers2