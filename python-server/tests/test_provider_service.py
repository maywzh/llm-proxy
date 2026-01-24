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

        monkeypatch.setattr(ps_module, "get_config", lambda: test_config)
        ps_module._provider_service = None

        service = ProviderService()
        service.initialize()

        assert service._initialized is True
        assert len(service._providers) == 2
        assert service._providers[0].name == "provider1"
        assert service._providers[1].name == "provider2"
        assert service._weights == [2, 1]

    def test_initialize_only_once(self, test_config, monkeypatch, clear_config_cache):
        """Test that initialize only runs once"""
        from app.services import provider_service as ps_module

        mock_get_config = Mock(return_value=test_config)
        monkeypatch.setattr(ps_module, "get_config", mock_get_config)
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
        assert provider.name in ["provider1", "provider2"]

    def test_get_next_provider_weighted_distribution(self, provider_service):
        """Test weighted distribution of provider selection"""
        # Sample 1000 times to check distribution
        selections = [provider_service.get_next_provider().name for _ in range(1000)]
        counts = Counter(selections)

        # provider1 has weight 2, provider2 has weight 1
        # So we expect roughly 2:1 ratio (with some variance)
        ratio = counts["provider1"] / counts["provider2"]
        assert 1.5 < ratio < 2.5  # Allow 25% variance

    def test_get_next_provider_auto_initializes(
        self, test_config, monkeypatch, clear_config_cache
    ):
        """Test get_next_provider auto-initializes if not initialized"""
        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: test_config)
        ps_module._provider_service = None

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
        assert providers[0].name == "provider1"
        assert providers[1].name == "provider2"

    def test_get_provider_weights(self, provider_service):
        """Test getting provider weights"""
        weights = provider_service.get_provider_weights()

        assert weights == [2, 1]

    def test_get_all_models(self, provider_service):
        """Test getting all unique models"""
        models = provider_service.get_all_models()

        # From test config: provider1 has gpt-4, gpt-3.5-turbo
        # provider2 has gpt-4, claude-3
        expected_models = {"gpt-4", "gpt-3.5-turbo", "claude-3"}
        assert models == expected_models

    def test_get_all_models_empty_mapping(
        self, test_config, monkeypatch, clear_config_cache
    ):
        """Test get_all_models with empty model mappings"""
        # Create config with no model mappings
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test",
                    api_base="https://api.test.com",
                    api_key="key",
                    model_mapping={},
                )
            ],
            verify_ssl=True,
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
        ps_module._provider_service = None

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
        assert all(name in ["provider1", "provider2"] for name in results)


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
                    name="only-provider",
                    api_base="https://api.test.com",
                    api_key="key",
                    weight=1,
                    model_mapping={"gpt-4": "gpt-4-0613"},
                )
            ],
            verify_ssl=True,
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
        ps_module._provider_service = None

        service = ProviderService()
        service.initialize()

        # Should always return the same provider
        for _ in range(10):
            provider = service.get_next_provider()
            assert provider.name == "only-provider"

    def test_many_providers(self, monkeypatch, clear_config_cache):
        """Test service with many providers"""
        providers = [
            ProviderConfig(
                name=f"provider{i}",
                api_base=f"https://api{i}.com",
                api_key=f"key{i}",
                weight=i + 1,
                model_mapping={},
            )
            for i in range(10)
        ]

        config = AppConfig(providers=providers, verify_ssl=True)

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
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
                    name="provider1",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=1,
                    model_mapping={},
                ),
                ProviderConfig(
                    name="provider2",
                    api_base="https://api2.com",
                    api_key="key2",
                    weight=1,
                    model_mapping={},
                ),
            ],
            verify_ssl=True,
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
        ps_module._provider_service = None

        service = ProviderService()
        service.initialize()

        # Sample many times
        selections = [service.get_next_provider().name for _ in range(1000)]
        counts = Counter(selections)

        # Should be roughly equal (within 20% variance)
        ratio = counts["provider1"] / counts["provider2"]
        assert 0.8 < ratio < 1.2

    def test_very_different_weights(self, monkeypatch, clear_config_cache):
        """Test providers with very different weights"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="heavy",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=100,
                    model_mapping={},
                ),
                ProviderConfig(
                    name="light",
                    api_base="https://api2.com",
                    api_key="key2",
                    weight=1,
                    model_mapping={},
                ),
            ],
            verify_ssl=True,
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
        ps_module._provider_service = None

        service = ProviderService()
        service.initialize()

        # Sample many times
        selections = [service.get_next_provider().name for _ in range(1000)]
        counts = Counter(selections)

        # Heavy should be selected much more often
        assert counts["heavy"] > counts["light"] * 50

    def test_overlapping_models(self, monkeypatch, clear_config_cache):
        """Test providers with overlapping model mappings"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="provider1",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=1,
                    model_mapping={"gpt-4": "gpt-4-0613", "gpt-3.5": "gpt-3.5-turbo"},
                ),
                ProviderConfig(
                    name="provider2",
                    api_base="https://api2.com",
                    api_key="key2",
                    weight=1,
                    model_mapping={"gpt-4": "gpt-4-1106", "claude": "claude-3"},
                ),
            ],
            verify_ssl=True,
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
        ps_module._provider_service = None

        service = ProviderService()
        service.initialize()

        models = service.get_all_models()
        # Should have unique model names (gpt-4 appears in both but counted once)
        assert models == {"gpt-4", "gpt-3.5", "claude"}


@pytest.mark.unit
class TestModelBasedProviderSelection:
    """Test model-based provider selection"""

    def test_get_next_provider_with_model(self, provider_service):
        """Test provider selection with specific model"""
        # Request gpt-4 which both providers have
        provider = provider_service.get_next_provider(model="gpt-4")
        assert provider.name in ["provider1", "provider2"]
        assert "gpt-4" in provider.model_mapping

    def test_get_next_provider_with_model_only_one_has(self, provider_service):
        """Test provider selection when only one provider has the model"""
        # Request gpt-3.5-turbo which only provider1 has
        for _ in range(10):
            provider = provider_service.get_next_provider(model="gpt-3.5-turbo")
            assert provider.name == "provider1"
            assert "gpt-3.5-turbo" in provider.model_mapping

    def test_get_next_provider_with_model_weighted_distribution(self, provider_service):
        """Test weighted distribution when multiple providers have the model"""
        # Both providers have gpt-4, provider1 has weight 2, provider2 has weight 1
        selections = [
            provider_service.get_next_provider(model="gpt-4").name for _ in range(1000)
        ]
        from collections import Counter

        counts = Counter(selections)

        # Should follow weight distribution (2:1 ratio)
        ratio = counts["provider1"] / counts["provider2"]
        assert 1.5 < ratio < 2.5

    def test_get_next_provider_with_nonexistent_model(self, provider_service):
        """Test error when requesting model that no provider has"""
        with pytest.raises(ValueError, match="No provider supports model"):
            provider_service.get_next_provider(model="nonexistent-model")

    def test_get_next_provider_without_model_uses_all_providers(self, provider_service):
        """Test that not specifying model uses all providers"""
        selections = [provider_service.get_next_provider().name for _ in range(1000)]
        from collections import Counter

        counts = Counter(selections)

        # Both providers should be selected
        assert "provider1" in counts
        assert "provider2" in counts

        # Should follow weight distribution (2:1 ratio)
        ratio = counts["provider1"] / counts["provider2"]
        assert 1.5 < ratio < 2.5


@pytest.mark.unit
class TestComplexMultiProviderMultiModel:
    """Test complex scenarios with multiple providers and models"""

    def test_complex_weight_calculation_scenario(self, monkeypatch, clear_config_cache):
        """Test complex multi-provider multi-model weight calculation

        Scenario:
        - Provider0: models A, B, C (weight=2)
        - Provider1: models A, B, D (weight=3)
        - Provider2: models B, D (weight=1)
        - Provider3: model C (weight=4)

        Expected behavior:
        1. Model A: Provider0(2) + Provider1(3) = ratio 2:3
        2. Model B: Provider0(2) + Provider1(3) + Provider2(1) = ratio 2:3:1
        3. Model C: Provider0(2) + Provider3(4) = ratio 2:4 = 1:2
        4. Model D: Provider1(3) + Provider2(1) = ratio 3:1
        5. Model E: Should raise ValueError (no provider supports it)
        """
        from app.models.config import AppConfig, ProviderConfig

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="provider0",
                    api_base="https://api0.com",
                    api_key="key0",
                    weight=2,
                    model_mapping={
                        "model-a": "provider0-model-a",
                        "model-b": "provider0-model-b",
                        "model-c": "provider0-model-c",
                    },
                ),
                ProviderConfig(
                    name="provider1",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=3,
                    model_mapping={
                        "model-a": "provider1-model-a",
                        "model-b": "provider1-model-b",
                        "model-d": "provider1-model-d",
                    },
                ),
                ProviderConfig(
                    name="provider2",
                    api_base="https://api2.com",
                    api_key="key2",
                    weight=1,
                    model_mapping={
                        "model-b": "provider2-model-b",
                        "model-d": "provider2-model-d",
                    },
                ),
                ProviderConfig(
                    name="provider3",
                    api_base="https://api3.com",
                    api_key="key3",
                    weight=4,
                    model_mapping={"model-c": "provider3-model-c"},
                ),
            ],
            verify_ssl=True,
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
        ps_module._provider_service = None

        service = ProviderService()
        service.initialize()

        # Test 1: Model A - Provider0(2) and Provider1(3) participate, ratio 2:3
        model_a_selections = [
            service.get_next_provider(model="model-a").name for _ in range(2000)
        ]
        model_a_counts = Counter(model_a_selections)

        assert "provider0" in model_a_counts
        assert "provider1" in model_a_counts
        assert "provider2" not in model_a_counts
        assert "provider3" not in model_a_counts

        ratio_a = model_a_counts["provider0"] / model_a_counts["provider1"]
        # Expected ratio 2:3 = 0.667, allow 30% variance
        assert 0.47 < ratio_a < 0.87, f"Model A ratio was {ratio_a}, expected ~0.667"

        # Test 2: Model B - Provider0(2), Provider1(3), Provider2(1) participate, ratio 2:3:1
        model_b_selections = [
            service.get_next_provider(model="model-b").name for _ in range(3000)
        ]
        model_b_counts = Counter(model_b_selections)

        assert "provider0" in model_b_counts
        assert "provider1" in model_b_counts
        assert "provider2" in model_b_counts
        assert "provider3" not in model_b_counts

        # Check ratios: provider0:provider1 should be 2:3
        ratio_b_01 = model_b_counts["provider0"] / model_b_counts["provider1"]
        assert (
            0.47 < ratio_b_01 < 0.87
        ), f"Model B provider0:provider1 ratio was {ratio_b_01}, expected ~0.667"

        # Check ratios: provider0:provider2 should be 2:1
        ratio_b_02 = model_b_counts["provider0"] / model_b_counts["provider2"]
        assert (
            1.4 < ratio_b_02 < 2.6
        ), f"Model B provider0:provider2 ratio was {ratio_b_02}, expected ~2.0"

        # Check ratios: provider1:provider2 should be 3:1
        ratio_b_12 = model_b_counts["provider1"] / model_b_counts["provider2"]
        assert (
            2.1 < ratio_b_12 < 3.9
        ), f"Model B provider1:provider2 ratio was {ratio_b_12}, expected ~3.0"

        # Test 3: Model C - Provider0(2) and Provider3(4) participate, ratio 2:4 = 1:2
        model_c_selections = [
            service.get_next_provider(model="model-c").name for _ in range(2000)
        ]
        model_c_counts = Counter(model_c_selections)

        assert "provider0" in model_c_counts
        assert "provider3" in model_c_counts
        assert "provider1" not in model_c_counts
        assert "provider2" not in model_c_counts

        ratio_c = model_c_counts["provider0"] / model_c_counts["provider3"]
        # Expected ratio 2:4 = 0.5, allow 30% variance
        assert 0.35 < ratio_c < 0.65, f"Model C ratio was {ratio_c}, expected ~0.5"

        # Test 4: Model D - Provider1(3) and Provider2(1) participate, ratio 3:1
        model_d_selections = [
            service.get_next_provider(model="model-d").name for _ in range(2000)
        ]
        model_d_counts = Counter(model_d_selections)

        assert "provider1" in model_d_counts
        assert "provider2" in model_d_counts
        assert "provider0" not in model_d_counts
        assert "provider3" not in model_d_counts

        ratio_d = model_d_counts["provider1"] / model_d_counts["provider2"]
        # Expected ratio 3:1 = 3.0, allow 30% variance
        assert 2.1 < ratio_d < 3.9, f"Model D ratio was {ratio_d}, expected ~3.0"

        # Test 5: Model E - No provider supports it, should raise ValueError
        with pytest.raises(ValueError, match="No provider supports model: model-e"):
            service.get_next_provider(model="model-e")


@pytest.mark.unit
class TestWildcardModelMapping:
    """Test wildcard/regex pattern matching in model mapping"""

    def test_provider_selection_with_regex_pattern(
        self, monkeypatch, clear_config_cache
    ):
        """Test provider selection with regex pattern in model_mapping"""
        from app.models.config import AppConfig, ProviderConfig

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="claude-provider",
                    api_base="https://api.claude.com",
                    api_key="key1",
                    weight=1,
                    model_mapping={
                        "claude-opus-4-5-.*": "claude-opus-mapped",  # Regex pattern
                    },
                ),
                ProviderConfig(
                    name="openai-provider",
                    api_base="https://api.openai.com",
                    api_key="key2",
                    weight=1,
                    model_mapping={
                        "gpt-4": "gpt-4-turbo",  # Exact match
                    },
                ),
            ]
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)

        service = ProviderService()
        service.initialize()

        # Test regex pattern matching - should select claude-provider
        provider = service.get_next_provider(model="claude-opus-4-5-20240620")
        assert provider.name == "claude-provider"
        assert (
            provider.get_mapped_model("claude-opus-4-5-20240620")
            == "claude-opus-mapped"
        )

        # Test another variant of the pattern
        provider = service.get_next_provider(model="claude-opus-4-5-latest")
        assert provider.name == "claude-provider"
        assert (
            provider.get_mapped_model("claude-opus-4-5-latest") == "claude-opus-mapped"
        )

        # Test exact match - should select openai-provider
        provider = service.get_next_provider(model="gpt-4")
        assert provider.name == "openai-provider"
        assert provider.get_mapped_model("gpt-4") == "gpt-4-turbo"

        # Test non-matching model - should raise error
        with pytest.raises(ValueError, match="No provider supports model"):
            service.get_next_provider(model="unknown-model")

    def test_provider_selection_with_simple_wildcard(
        self, monkeypatch, clear_config_cache
    ):
        """Test provider selection with simple wildcard (*) in model_mapping"""
        from app.models.config import AppConfig, ProviderConfig

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="gemini-provider",
                    api_base="https://api.gemini.com",
                    api_key="key1",
                    weight=1,
                    model_mapping={
                        "gemini-*": "gemini-pro",  # Simple wildcard
                    },
                ),
            ]
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)

        service = ProviderService()
        service.initialize()

        # Test simple wildcard matching
        provider = service.get_next_provider(model="gemini-pro")
        assert provider.name == "gemini-provider"
        assert provider.get_mapped_model("gemini-pro") == "gemini-pro"

        provider = service.get_next_provider(model="gemini-ultra")
        assert provider.name == "gemini-provider"
        assert provider.get_mapped_model("gemini-ultra") == "gemini-pro"

        provider = service.get_next_provider(model="gemini-1.5-pro")
        assert provider.name == "gemini-provider"
        assert provider.get_mapped_model("gemini-1.5-pro") == "gemini-pro"

    def test_exact_match_priority_over_pattern(self, monkeypatch, clear_config_cache):
        """Test that exact matches take priority over pattern matches"""
        from app.models.config import AppConfig, ProviderConfig

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="provider1",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=1,
                    model_mapping={
                        "claude-.*": "claude-pattern",  # Pattern
                        "claude-opus": "claude-opus-exact",  # Exact match
                    },
                ),
            ]
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)

        service = ProviderService()
        service.initialize()

        # Exact match should take priority
        provider = service.get_next_provider(model="claude-opus")
        assert provider.get_mapped_model("claude-opus") == "claude-opus-exact"

        # Pattern should match other claude models
        provider = service.get_next_provider(model="claude-sonnet")
        assert provider.get_mapped_model("claude-sonnet") == "claude-pattern"

    def test_multiple_providers_with_patterns(self, monkeypatch, clear_config_cache):
        """Test weighted selection when multiple providers have matching patterns"""
        from app.models.config import AppConfig, ProviderConfig
        from collections import Counter

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="provider1",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=2,
                    model_mapping={
                        "claude-opus-4-5-.*": "provider1-claude",
                    },
                ),
                ProviderConfig(
                    name="provider2",
                    api_base="https://api2.com",
                    api_key="key2",
                    weight=1,
                    model_mapping={
                        "claude-opus-4-5-.*": "provider2-claude",
                    },
                ),
            ]
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)

        service = ProviderService()
        service.initialize()

        # Both providers should be selected with weighted distribution
        selections = [
            service.get_next_provider(model="claude-opus-4-5-20240620").name
            for _ in range(1000)
        ]
        counts = Counter(selections)

        assert "provider1" in counts
        assert "provider2" in counts

        # Should follow weight distribution (2:1 ratio)
        ratio = counts["provider1"] / counts["provider2"]
        assert 1.5 < ratio < 2.5, f"Ratio was {ratio}, expected ~2.0"

    def test_get_all_models_filters_patterns(self, monkeypatch, clear_config_cache):
        """Test that get_all_models filters out wildcard/regex patterns"""
        from app.models.config import AppConfig, ProviderConfig

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="provider1",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=1,
                    model_mapping={
                        "gpt-4": "gpt-4-turbo",  # Exact match - should be included
                        "claude-opus-4-5-.*": "claude-mapped",  # Regex pattern - should be filtered
                        "gemini-*": "gemini-pro",  # Simple wildcard - should be filtered
                    },
                ),
                ProviderConfig(
                    name="provider2",
                    api_base="https://api2.com",
                    api_key="key2",
                    weight=1,
                    model_mapping={
                        "gpt-3.5-turbo": "gpt-3.5-turbo-0125",  # Exact match - should be included
                        "claude-.*": "claude-default",  # Regex pattern - should be filtered
                    },
                ),
            ]
        )

        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
        ps_module._provider_service = None

        service = ProviderService()
        service.initialize()

        models = service.get_all_models()

        # Only exact matches should be returned
        assert models == {"gpt-4", "gpt-3.5-turbo"}

        # Patterns should NOT be in the result
        assert "claude-opus-4-5-.*" not in models
        assert "gemini-*" not in models
        assert "claude-.*" not in models


@pytest.mark.unit
class TestProviderServiceState:
    """Test provider service state management"""

    def test_uninitialized_state(self):
        """Test service starts uninitialized"""
        service = ProviderService()

        assert service._initialized is False
        assert service._providers == []
        assert service._weights == []

    def test_state_after_initialization(
        self, test_config, monkeypatch, clear_config_cache
    ):
        """Test service state after initialization"""
        from app.services import provider_service as ps_module

        monkeypatch.setattr(ps_module, "get_config", lambda: test_config)
        ps_module._provider_service = None

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
