"""Tests for configuration management"""
import os
import tempfile
from unittest.mock import patch

import pytest
import yaml

from app.core.config import (
    expand_env_vars,
    expand_config_env_vars,
    str_to_bool,
    load_config,
    get_config
)
from app.models.config import AppConfig, ProviderConfig, ServerConfig


@pytest.mark.unit
class TestExpandEnvVars:
    """Test environment variable expansion"""
    
    def test_expand_simple_var(self, monkeypatch):
        """Test expanding simple environment variable"""
        monkeypatch.setenv('TEST_VAR', 'test_value')
        result = expand_env_vars('${TEST_VAR}')
        assert result == 'test_value'
    
    def test_expand_var_with_default(self, monkeypatch):
        """Test expanding variable with default value"""
        monkeypatch.delenv('MISSING_VAR', raising=False)
        result = expand_env_vars('${MISSING_VAR:-default_value}')
        assert result == 'default_value'
    
    def test_expand_var_with_colon_default(self, monkeypatch):
        """Test expanding variable with colon-prefixed default"""
        monkeypatch.delenv('MISSING_VAR', raising=False)
        result = expand_env_vars('${MISSING_VAR:default_value}')
        assert result == 'default_value'
    
    def test_expand_existing_var_ignores_default(self, monkeypatch):
        """Test that existing variable ignores default value"""
        monkeypatch.setenv('EXISTING_VAR', 'actual_value')
        result = expand_env_vars('${EXISTING_VAR:-default_value}')
        assert result == 'actual_value'
    
    def test_expand_multiple_vars(self, monkeypatch):
        """Test expanding multiple variables in one string"""
        monkeypatch.setenv('VAR1', 'value1')
        monkeypatch.setenv('VAR2', 'value2')
        result = expand_env_vars('${VAR1}/path/${VAR2}')
        assert result == 'value1/path/value2'
    
    def test_expand_non_string_returns_unchanged(self):
        """Test that non-string values are returned unchanged"""
        assert expand_env_vars(123) == 123
        assert expand_env_vars(None) is None
        assert expand_env_vars([1, 2, 3]) == [1, 2, 3]
    
    def test_expand_missing_var_without_default(self, monkeypatch):
        """Test expanding missing variable without default returns empty string"""
        monkeypatch.delenv('MISSING_VAR', raising=False)
        result = expand_env_vars('${MISSING_VAR}')
        assert result == ''


@pytest.mark.unit
class TestExpandConfigEnvVars:
    """Test recursive environment variable expansion in config"""
    
    def test_expand_dict_values(self, monkeypatch):
        """Test expanding environment variables in dictionary"""
        monkeypatch.setenv('API_KEY', 'secret-key')
        config = {'api_key': '${API_KEY}', 'name': 'test'}
        result = expand_config_env_vars(config)
        assert result['api_key'] == 'secret-key'
        assert result['name'] == 'test'
    
    def test_expand_nested_dict(self, monkeypatch):
        """Test expanding variables in nested dictionary"""
        monkeypatch.setenv('HOST', 'localhost')
        monkeypatch.setenv('PORT', '8080')
        config = {
            'server': {
                'host': '${HOST}',
                'port': '${PORT}'
            }
        }
        result = expand_config_env_vars(config)
        assert result['server']['host'] == 'localhost'
        assert result['server']['port'] == '8080'
    
    def test_expand_list_values(self, monkeypatch):
        """Test expanding variables in list"""
        monkeypatch.setenv('URL1', 'http://url1.com')
        monkeypatch.setenv('URL2', 'http://url2.com')
        config = ['${URL1}', '${URL2}', 'static']
        result = expand_config_env_vars(config)
        assert result == ['http://url1.com', 'http://url2.com', 'static']
    
    def test_expand_complex_structure(self, monkeypatch):
        """Test expanding variables in complex nested structure"""
        monkeypatch.setenv('API_BASE', 'https://api.test.com')
        monkeypatch.setenv('API_KEY', 'test-key')
        config = {
            'providers': [
                {
                    'name': 'provider1',
                    'api_base': '${API_BASE}',
                    'api_key': '${API_KEY}'
                }
            ]
        }
        result = expand_config_env_vars(config)
        assert result['providers'][0]['api_base'] == 'https://api.test.com'
        assert result['providers'][0]['api_key'] == 'test-key'


@pytest.mark.unit
class TestStrToBool:
    """Test string to boolean conversion"""
    
    def test_true_strings(self):
        """Test various true string representations"""
        assert str_to_bool('true') is True
        assert str_to_bool('True') is True
        assert str_to_bool('TRUE') is True
        assert str_to_bool('1') is True
        assert str_to_bool('yes') is True
        assert str_to_bool('Yes') is True
        assert str_to_bool('on') is True
        assert str_to_bool('ON') is True
    
    def test_false_strings(self):
        """Test various false string representations"""
        assert str_to_bool('false') is False
        assert str_to_bool('False') is False
        assert str_to_bool('0') is False
        assert str_to_bool('no') is False
        assert str_to_bool('off') is False
        assert str_to_bool('') is False
    
    def test_boolean_values(self):
        """Test that boolean values are returned unchanged"""
        assert str_to_bool(True) is True
        assert str_to_bool(False) is False
    
    def test_numeric_values(self):
        """Test numeric value conversion"""
        assert str_to_bool(1) is True
        assert str_to_bool(0) is False
        assert str_to_bool(42) is True


@pytest.mark.unit
class TestLoadConfig:
    """Test configuration loading from file"""
    
    def test_load_valid_config(self, test_config_file):
        """Test loading valid configuration file"""
        config = load_config(test_config_file)
        assert isinstance(config, AppConfig)
        assert len(config.providers) == 2
        assert config.providers[0].name == 'provider1'
        assert config.providers[0].weight == 2
        assert config.server.port == 18000
        assert config.verify_ssl is True
    
    def test_load_config_with_env_vars(self, monkeypatch):
        """Test loading config with environment variable expansion"""
        monkeypatch.setenv('API_BASE', 'https://test.api.com')
        monkeypatch.setenv('API_KEY', 'env-key')
        
        config_dict = {
            'providers': [{
                'name': 'test',
                'api_base': '${API_BASE}',
                'api_key': '${API_KEY}',
                'weight': 1,
                'model_mapping': {}
            }],
            'server': {'host': '0.0.0.0', 'port': 18000},
            'verify_ssl': True
        }
        
        with tempfile.NamedTemporaryFile(mode='w', suffix='.yaml', delete=False) as f:
            yaml.dump(config_dict, f)
            config_path = f.name
        
        try:
            config = load_config(config_path)
            assert config.providers[0].api_base == 'https://test.api.com'
            assert config.providers[0].api_key == 'env-key'
        finally:
            os.unlink(config_path)
    
    def test_load_config_with_defaults(self):
        """Test loading config with default values"""
        config_dict = {
            'providers': [{
                'name': 'test',
                'api_base': 'https://api.test.com',
                'api_key': 'test-key',
                'model_mapping': {}
            }],
            'verify_ssl': True
        }
        
        with tempfile.NamedTemporaryFile(mode='w', suffix='.yaml', delete=False) as f:
            yaml.dump(config_dict, f)
            config_path = f.name
        
        try:
            config = load_config(config_path)
            assert config.providers[0].weight == 1  # Default weight
            assert config.server.host == '0.0.0.0'  # Default host
            assert config.server.port == 18000  # Default port
        finally:
            os.unlink(config_path)
    
    def test_load_config_file_not_found(self):
        """Test loading non-existent config file raises error"""
        with pytest.raises(FileNotFoundError):
            load_config('nonexistent.yaml')
    
    def test_load_config_invalid_yaml(self):
        """Test loading invalid YAML raises error"""
        with tempfile.NamedTemporaryFile(mode='w', suffix='.yaml', delete=False) as f:
            f.write('invalid: yaml: content: [')
            config_path = f.name
        
        try:
            with pytest.raises(yaml.YAMLError):
                load_config(config_path)
        finally:
            os.unlink(config_path)
    
    def test_load_config_verify_ssl_string(self):
        """Test verify_ssl string conversion"""
        config_dict = {
            'providers': [{
                'name': 'test',
                'api_base': 'https://api.test.com',
                'api_key': 'test-key',
                'model_mapping': {}
            }],
            'verify_ssl': 'false'
        }
        
        with tempfile.NamedTemporaryFile(mode='w', suffix='.yaml', delete=False) as f:
            yaml.dump(config_dict, f)
            config_path = f.name
        
        try:
            config = load_config(config_path)
            assert config.verify_ssl is False
        finally:
            os.unlink(config_path)


@pytest.mark.unit
class TestGetConfig:
    """Test cached configuration retrieval"""
    
    def test_get_config_returns_cached_instance(self, test_config_file, monkeypatch, clear_config_cache):
        """Test that get_config returns cached instance"""
        monkeypatch.setenv('CONFIG_PATH', test_config_file)
        
        config1 = get_config()
        config2 = get_config()
        
        assert config1 is config2  # Same instance
    
    def test_get_config_uses_env_var(self, test_config_file, monkeypatch, clear_config_cache):
        """Test that get_config uses CONFIG_PATH environment variable"""
        monkeypatch.setenv('CONFIG_PATH', test_config_file)
        
        config = get_config()
        assert isinstance(config, AppConfig)
        assert len(config.providers) == 2
    
    def test_get_config_default_path(self, monkeypatch, clear_config_cache):
        """Test that get_config uses default path when env var not set"""
        monkeypatch.delenv('CONFIG_PATH', raising=False)
        
        # Create config.yaml in current directory
        config_dict = {
            'providers': [{
                'name': 'test',
                'api_base': 'https://api.test.com',
                'api_key': 'test-key',
                'model_mapping': {}
            }],
            'verify_ssl': True
        }
        
        with open('config.yaml', 'w') as f:
            yaml.dump(config_dict, f)
        
        try:
            config = get_config()
            assert isinstance(config, AppConfig)
        finally:
            if os.path.exists('config.yaml'):
                os.unlink('config.yaml')
            get_config.cache_clear()


@pytest.mark.unit
class TestConfigModels:
    """Test configuration model validation"""
    
    def test_provider_config_validation(self):
        """Test ProviderConfig validation"""
        provider = ProviderConfig(
            name='test',
            api_base='https://api.test.com',
            api_key='test-key',
            weight=5,
            model_mapping={'gpt-4': 'gpt-4-0613'}
        )
        assert provider.name == 'test'
        assert provider.weight == 5
    
    def test_provider_config_default_weight(self):
        """Test ProviderConfig default weight"""
        provider = ProviderConfig(
            name='test',
            api_base='https://api.test.com',
            api_key='test-key'
        )
        assert provider.weight == 1
    
    def test_provider_config_invalid_weight(self):
        """Test ProviderConfig rejects invalid weight"""
        with pytest.raises(ValueError):
            ProviderConfig(
                name='test',
                api_base='https://api.test.com',
                api_key='test-key',
                weight=0  # Must be >= 1
            )
    
    def test_server_config_defaults(self):
        """Test ServerConfig default values"""
        server = ServerConfig()
        assert server.host == '0.0.0.0'
        assert server.port == 18000
    
    def test_app_config_validation(self):
        """Test AppConfig validation"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='test-key'
                )
            ],
            server=ServerConfig(port=8080),
            verify_ssl=False
        )
        assert len(config.providers) == 1
        assert config.server.port == 8080
        assert config.verify_ssl is False
    
    def test_app_config_requires_providers(self):
        """Test AppConfig requires at least one provider"""
        with pytest.raises(ValueError):
            AppConfig(providers=[], verify_ssl=True)