"""Property-based tests using Hypothesis"""
from collections import Counter

import pytest
from hypothesis import given, strategies as st, settings

from app.services.provider_service import ProviderService
from app.models.provider import Provider
from app.models.config import AppConfig, ProviderConfig


@pytest.mark.property
class TestProviderSelectionProperties:
    """Property-based tests for provider selection"""
    
    @given(
        weight1=st.integers(min_value=1, max_value=100),
        weight2=st.integers(min_value=1, max_value=100)
    )
    @settings(max_examples=50)
    def test_provider_selection_respects_weights(self, weight1, weight2, monkeypatch):
        """Test that provider selection respects weight ratios"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='provider1',
                    api_base='https://api1.com',
                    api_key='key1',
                    weight=weight1,
                    model_mapping={}
                ),
                ProviderConfig(
                    name='provider2',
                    api_base='https://api2.com',
                    api_key='key2',
                    weight=weight2,
                    model_mapping={}
                )
            ],
            verify_ssl=True
        )
        
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        service = ProviderService()
        service.initialize()
        
        # Sample many times
        selections = [service.get_next_provider().name for _ in range(1000)]
        counts = Counter(selections)
        
        # Calculate expected ratio
        total_weight = weight1 + weight2
        expected_ratio = weight1 / weight2
        actual_ratio = counts['provider1'] / counts['provider2']
        
        # Allow 30% variance due to randomness
        assert abs(actual_ratio - expected_ratio) < expected_ratio * 0.3
    
    @given(num_providers=st.integers(min_value=1, max_value=10))
    @settings(max_examples=20)
    def test_all_providers_eventually_selected(self, num_providers, monkeypatch):
        """Test that all providers are eventually selected"""
        providers = [
            ProviderConfig(
                name=f'provider{i}',
                api_base=f'https://api{i}.com',
                api_key=f'key{i}',
                weight=1,
                model_mapping={}
            )
            for i in range(num_providers)
        ]
        
        config = AppConfig(providers=providers, verify_ssl=True)
        
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        service = ProviderService()
        service.initialize()
        
        # Sample enough times to hit all providers
        selections = {service.get_next_provider().name for _ in range(num_providers * 100)}
        
        # All providers should be selected at least once
        expected_names = {f'provider{i}' for i in range(num_providers)}
        assert selections == expected_names


@pytest.mark.property
class TestConfigValidationProperties:
    """Property-based tests for configuration validation"""
    
    @given(
        weight=st.integers(min_value=1, max_value=1000),
        api_key=st.text(min_size=1, max_size=100)
    )
    @settings(max_examples=50)
    def test_provider_config_accepts_valid_values(self, weight, api_key):
        """Test that ProviderConfig accepts valid values"""
        provider = ProviderConfig(
            name='test',
            api_base='https://api.test.com',
            api_key=api_key,
            weight=weight,
            model_mapping={}
        )
        
        assert provider.weight == weight
        assert provider.api_key == api_key
    
    @given(weight=st.integers(max_value=0))
    @settings(max_examples=20)
    def test_provider_config_rejects_invalid_weight(self, weight):
        """Test that ProviderConfig rejects invalid weights"""
        from pydantic import ValidationError
        
        with pytest.raises(ValidationError):
            ProviderConfig(
                name='test',
                api_base='https://api.test.com',
                api_key='key',
                weight=weight,
                model_mapping={}
            )
    
    @given(
        model_name=st.text(min_size=1, max_size=50),
        actual_model=st.text(min_size=1, max_size=50)
    )
    @settings(max_examples=30)
    def test_model_mapping_preserves_values(self, model_name, actual_model):
        """Test that model mapping preserves arbitrary string values"""
        provider = ProviderConfig(
            name='test',
            api_base='https://api.test.com',
            api_key='key',
            weight=1,
            model_mapping={model_name: actual_model}
        )
        
        assert provider.model_mapping[model_name] == actual_model


@pytest.mark.property
class TestModelMappingProperties:
    """Property-based tests for model mapping"""
    
    @given(
        models=st.lists(
            st.tuples(
                st.text(min_size=1, max_size=20, alphabet=st.characters(blacklist_categories=('Cs',))),
                st.text(min_size=1, max_size=20, alphabet=st.characters(blacklist_categories=('Cs',)))
            ),
            min_size=1,
            max_size=10,
            unique_by=lambda x: x[0]
        )
    )
    @settings(max_examples=30)
    def test_get_all_models_returns_unique_names(self, models, monkeypatch):
        """Test that get_all_models returns unique model names"""
        model_mapping = dict(models)
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key',
                    weight=1,
                    model_mapping=model_mapping
                )
            ],
            verify_ssl=True
        )
        
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        service = ProviderService()
        service.initialize()
        
        all_models = service.get_all_models()
        
        # Should return unique model names
        assert len(all_models) == len(model_mapping)
        assert all_models == set(model_mapping.keys())


@pytest.mark.property
class TestStringProcessingProperties:
    """Property-based tests for string processing"""
    
    @given(text=st.text(min_size=0, max_size=1000))
    @settings(max_examples=50)
    def test_env_var_expansion_handles_arbitrary_text(self, text):
        """Test that env var expansion handles arbitrary text"""
        from app.core.config import expand_env_vars
        
        # Should not crash on any input
        result = expand_env_vars(text)
        assert isinstance(result, str)
    
    @given(
        bool_str=st.sampled_from(['true', 'True', 'TRUE', '1', 'yes', 'Yes', 'on', 'ON'])
    )
    @settings(max_examples=20)
    def test_str_to_bool_recognizes_true_values(self, bool_str):
        """Test that str_to_bool recognizes various true values"""
        from app.core.config import str_to_bool
        
        assert str_to_bool(bool_str) is True
    
    @given(
        bool_str=st.sampled_from(['false', 'False', 'FALSE', '0', 'no', 'No', 'off', 'OFF', ''])
    )
    @settings(max_examples=20)
    def test_str_to_bool_recognizes_false_values(self, bool_str):
        """Test that str_to_bool recognizes various false values"""
        from app.core.config import str_to_bool
        
        assert str_to_bool(bool_str) is False


@pytest.mark.property
class TestProviderProperties:
    """Property-based tests for Provider dataclass"""
    
    @given(
        name=st.text(min_size=1, max_size=50),
        api_base=st.text(min_size=1, max_size=100),
        api_key=st.text(min_size=1, max_size=100),
        weight=st.integers(min_value=1, max_value=1000)
    )
    @settings(max_examples=50)
    def test_provider_equality_is_reflexive(self, name, api_base, api_key, weight):
        """Test that provider equality is reflexive (p == p)"""
        provider = Provider(
            name=name,
            api_base=api_base,
            api_key=api_key,
            weight=weight,
            model_mapping={}
        )
        
        assert provider == provider
    
    @given(
        name=st.text(min_size=1, max_size=50),
        api_base=st.text(min_size=1, max_size=100),
        api_key=st.text(min_size=1, max_size=100),
        weight=st.integers(min_value=1, max_value=1000)
    )
    @settings(max_examples=50)
    def test_provider_equality_is_symmetric(self, name, api_base, api_key, weight):
        """Test that provider equality is symmetric (p1 == p2 implies p2 == p1)"""
        provider1 = Provider(
            name=name,
            api_base=api_base,
            api_key=api_key,
            weight=weight,
            model_mapping={}
        )
        
        provider2 = Provider(
            name=name,
            api_base=api_base,
            api_key=api_key,
            weight=weight,
            model_mapping={}
        )
        
        assert (provider1 == provider2) == (provider2 == provider1)


@pytest.mark.property
class TestWeightDistributionProperties:
    """Property-based tests for weight distribution"""
    
    @given(
        weights=st.lists(
            st.integers(min_value=1, max_value=100),
            min_size=2,
            max_size=5
        )
    )
    @settings(max_examples=30)
    def test_total_selections_equals_sample_size(self, weights, monkeypatch):
        """Test that total selections equals sample size"""
        providers = [
            ProviderConfig(
                name=f'provider{i}',
                api_base=f'https://api{i}.com',
                api_key=f'key{i}',
                weight=w,
                model_mapping={}
            )
            for i, w in enumerate(weights)
        ]
        
        config = AppConfig(providers=providers, verify_ssl=True)
        
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        service = ProviderService()
        service.initialize()
        
        sample_size = 1000
        selections = [service.get_next_provider().name for _ in range(sample_size)]
        
        assert len(selections) == sample_size
    
    @given(
        weights=st.lists(
            st.integers(min_value=1, max_value=100),
            min_size=1,
            max_size=10
        )
    )
    @settings(max_examples=30)
    def test_weights_sum_correctly(self, weights, monkeypatch):
        """Test that provider weights sum correctly"""
        providers = [
            ProviderConfig(
                name=f'provider{i}',
                api_base=f'https://api{i}.com',
                api_key=f'key{i}',
                weight=w,
                model_mapping={}
            )
            for i, w in enumerate(weights)
        ]
        
        config = AppConfig(providers=providers, verify_ssl=True)
        
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        service = ProviderService()
        service.initialize()
        
        provider_weights = service.get_provider_weights()
        
        assert sum(provider_weights) == sum(weights)
        assert provider_weights == weights


@pytest.mark.property
class TestSecurityProperties:
    """Property-based tests for security"""
    
    @given(api_key=st.text(min_size=1, max_size=100))
    @settings(max_examples=50)
    def test_correct_key_always_validates(self, api_key, monkeypatch):
        """Test that correct API key always validates"""
        from app.core.security import verify_master_key
        from app.models.config import AppConfig, ProviderConfig, ServerConfig
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key=api_key),
            verify_ssl=True
        )
        
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        result = verify_master_key(f'Bearer {api_key}')
        assert result is True
    
    @given(
        correct_key=st.text(min_size=1, max_size=50),
        wrong_key=st.text(min_size=1, max_size=50)
    )
    @settings(max_examples=50)
    def test_wrong_key_never_validates(self, correct_key, wrong_key, monkeypatch):
        """Test that wrong API key never validates"""
        # Skip if keys happen to be the same
        if correct_key == wrong_key:
            return
        
        from app.core.security import verify_master_key
        from app.models.config import AppConfig, ProviderConfig, ServerConfig
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key=correct_key),
            verify_ssl=True
        )
        
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        result = verify_master_key(f'Bearer {wrong_key}')
        assert result is False