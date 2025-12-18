"""Tests for data models"""
import pytest
from pydantic import ValidationError

from app.models.config import ProviderConfig, ServerConfig, AppConfig
from app.models.provider import Provider


@pytest.mark.unit
class TestProviderConfig:
    """Test ProviderConfig model"""
    
    def test_create_provider_config(self):
        """Test creating valid ProviderConfig"""
        provider = ProviderConfig(
            name='test-provider',
            api_base='https://api.test.com/v1',
            api_key='test-key-123',
            weight=5,
            model_mapping={'gpt-4': 'gpt-4-0613'}
        )
        
        assert provider.name == 'test-provider'
        assert provider.api_base == 'https://api.test.com/v1'
        assert provider.api_key == 'test-key-123'
        assert provider.weight == 5
        assert provider.model_mapping == {'gpt-4': 'gpt-4-0613'}
    
    def test_provider_config_default_weight(self):
        """Test ProviderConfig uses default weight of 1"""
        provider = ProviderConfig(
            name='test',
            api_base='https://api.test.com',
            api_key='key'
        )
        assert provider.weight == 1
    
    def test_provider_config_default_model_mapping(self):
        """Test ProviderConfig uses empty dict for model_mapping by default"""
        provider = ProviderConfig(
            name='test',
            api_base='https://api.test.com',
            api_key='key'
        )
        assert provider.model_mapping == {}
    
    def test_provider_config_weight_validation(self):
        """Test ProviderConfig validates weight >= 1"""
        with pytest.raises(ValidationError) as exc_info:
            ProviderConfig(
                name='test',
                api_base='https://api.test.com',
                api_key='key',
                weight=0
            )
        assert 'weight' in str(exc_info.value)
    
    def test_provider_config_negative_weight(self):
        """Test ProviderConfig rejects negative weight"""
        with pytest.raises(ValidationError):
            ProviderConfig(
                name='test',
                api_base='https://api.test.com',
                api_key='key',
                weight=-1
            )
    
    def test_provider_config_missing_required_fields(self):
        """Test ProviderConfig requires name, api_base, api_key"""
        with pytest.raises(ValidationError):
            ProviderConfig(name='test')
    
    def test_provider_config_serialization(self):
        """Test ProviderConfig can be serialized to dict"""
        provider = ProviderConfig(
            name='test',
            api_base='https://api.test.com',
            api_key='key',
            weight=3,
            model_mapping={'model1': 'actual-model1'}
        )
        
        data = provider.model_dump()
        assert data['name'] == 'test'
        assert data['weight'] == 3
        assert data['model_mapping'] == {'model1': 'actual-model1'}
    
    def test_provider_config_from_dict(self):
        """Test creating ProviderConfig from dictionary"""
        data = {
            'name': 'test',
            'api_base': 'https://api.test.com',
            'api_key': 'key',
            'weight': 2,
            'model_mapping': {'gpt-4': 'gpt-4-0613'}
        }
        
        provider = ProviderConfig(**data)
        assert provider.name == 'test'
        assert provider.weight == 2


@pytest.mark.unit
class TestServerConfig:
    """Test ServerConfig model"""
    
    def test_create_server_config(self):
        """Test creating ServerConfig with custom values"""
        server = ServerConfig(
            host='127.0.0.1',
            port=8080,
            master_api_key='secret-key'
        )
        
        assert server.host == '127.0.0.1'
        assert server.port == 8080
        assert server.master_api_key == 'secret-key'
    
    def test_server_config_defaults(self):
        """Test ServerConfig default values"""
        server = ServerConfig()
        
        assert server.host == '0.0.0.0'
        assert server.port == 18000
        assert server.master_api_key is None
    
    def test_server_config_partial_defaults(self):
        """Test ServerConfig with some defaults"""
        server = ServerConfig(port=9000)
        
        assert server.host == '0.0.0.0'
        assert server.port == 9000
        assert server.master_api_key is None
    
    def test_server_config_serialization(self):
        """Test ServerConfig serialization"""
        server = ServerConfig(host='localhost', port=3000)
        data = server.model_dump()
        
        assert data['host'] == 'localhost'
        assert data['port'] == 3000
        assert data['master_api_key'] is None


@pytest.mark.unit
class TestAppConfig:
    """Test AppConfig model"""
    
    def test_create_app_config(self):
        """Test creating valid AppConfig"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='provider1',
                    api_base='https://api1.com',
                    api_key='key1'
                )
            ],
            server=ServerConfig(port=8080),
            verify_ssl=False
        )
        
        assert len(config.providers) == 1
        assert config.providers[0].name == 'provider1'
        assert config.server.port == 8080
        assert config.verify_ssl is False
    
    def test_app_config_default_server(self):
        """Test AppConfig uses default ServerConfig"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            verify_ssl=True
        )
        
        assert config.server.host == '0.0.0.0'
        assert config.server.port == 18000
    
    def test_app_config_default_verify_ssl(self):
        """Test AppConfig defaults verify_ssl to True"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ]
        )
        
        assert config.verify_ssl is True
    
    def test_app_config_multiple_providers(self):
        """Test AppConfig with multiple providers"""
        config = AppConfig(
            providers=[
                ProviderConfig(name='p1', api_base='https://api1.com', api_key='key1'),
                ProviderConfig(name='p2', api_base='https://api2.com', api_key='key2'),
                ProviderConfig(name='p3', api_base='https://api3.com', api_key='key3')
            ],
            verify_ssl=True
        )
        
        assert len(config.providers) == 3
        assert config.providers[0].name == 'p1'
        assert config.providers[1].name == 'p2'
        assert config.providers[2].name == 'p3'
    
    def test_app_config_empty_providers(self):
        """Test AppConfig requires at least one provider"""
        with pytest.raises(ValidationError) as exc_info:
            AppConfig(providers=[], verify_ssl=True)
        assert 'providers' in str(exc_info.value)
    
    def test_app_config_serialization(self):
        """Test AppConfig serialization"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key',
                    weight=2
                )
            ],
            server=ServerConfig(port=9000),
            verify_ssl=False
        )
        
        data = config.model_dump()
        assert len(data['providers']) == 1
        assert data['providers'][0]['name'] == 'test'
        assert data['server']['port'] == 9000
        assert data['verify_ssl'] is False


@pytest.mark.unit
class TestProvider:
    """Test Provider dataclass"""
    
    def test_create_provider(self):
        """Test creating Provider instance"""
        provider = Provider(
            name='test-provider',
            api_base='https://api.test.com/v1',
            api_key='test-key',
            weight=3,
            model_mapping={'gpt-4': 'gpt-4-0613'}
        )
        
        assert provider.name == 'test-provider'
        assert provider.api_base == 'https://api.test.com/v1'
        assert provider.api_key == 'test-key'
        assert provider.weight == 3
        assert provider.model_mapping == {'gpt-4': 'gpt-4-0613'}
    
    def test_provider_equality(self):
        """Test Provider equality comparison"""
        provider1 = Provider(
            name='test',
            api_base='https://api.test.com',
            api_key='key',
            weight=1,
            model_mapping={}
        )
        
        provider2 = Provider(
            name='test',
            api_base='https://api.test.com',
            api_key='key',
            weight=1,
            model_mapping={}
        )
        
        assert provider1 == provider2
    
    def test_provider_inequality(self):
        """Test Provider inequality"""
        provider1 = Provider(
            name='test1',
            api_base='https://api.test.com',
            api_key='key',
            weight=1,
            model_mapping={}
        )
        
        provider2 = Provider(
            name='test2',
            api_base='https://api.test.com',
            api_key='key',
            weight=1,
            model_mapping={}
        )
        
        assert provider1 != provider2
    
    def test_provider_empty_model_mapping(self):
        """Test Provider with empty model mapping"""
        provider = Provider(
            name='test',
            api_base='https://api.test.com',
            api_key='key',
            weight=1,
            model_mapping={}
        )
        
        assert provider.model_mapping == {}
    
    def test_provider_complex_model_mapping(self):
        """Test Provider with complex model mapping"""
        mapping = {
            'gpt-4': 'gpt-4-0613',
            'gpt-3.5-turbo': 'gpt-3.5-turbo-0613',
            'claude-3': 'claude-3-opus-20240229'
        }
        
        provider = Provider(
            name='test',
            api_base='https://api.test.com',
            api_key='key',
            weight=1,
            model_mapping=mapping
        )
        
        assert len(provider.model_mapping) == 3
        assert provider.model_mapping['gpt-4'] == 'gpt-4-0613'
        assert provider.model_mapping['claude-3'] == 'claude-3-opus-20240229'


@pytest.mark.unit
class TestModelIntegration:
    """Test integration between different models"""
    
    def test_provider_config_to_provider(self):
        """Test converting ProviderConfig to Provider"""
        config = ProviderConfig(
            name='test',
            api_base='https://api.test.com',
            api_key='key',
            weight=2,
            model_mapping={'gpt-4': 'gpt-4-0613'}
        )
        
        provider = Provider(
            name=config.name,
            api_base=config.api_base,
            api_key=config.api_key,
            weight=config.weight,
            model_mapping=config.model_mapping
        )
        
        assert provider.name == config.name
        assert provider.weight == config.weight
        assert provider.model_mapping == config.model_mapping
    
    def test_app_config_with_multiple_provider_configs(self):
        """Test AppConfig with multiple ProviderConfigs"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='provider1',
                    api_base='https://api1.com',
                    api_key='key1',
                    weight=2
                ),
                ProviderConfig(
                    name='provider2',
                    api_base='https://api2.com',
                    api_key='key2',
                    weight=1
                )
            ],
            verify_ssl=True
        )
        
        # Convert to Provider instances
        providers = [
            Provider(
                name=p.name,
                api_base=p.api_base,
                api_key=p.api_key,
                weight=p.weight,
                model_mapping=p.model_mapping
            )
            for p in config.providers
        ]
        
        assert len(providers) == 2
        assert providers[0].weight == 2
        assert providers[1].weight == 1