"""Tests for data models"""

import pytest
from pydantic import ValidationError

from app.models.config import (
    ProviderConfig,
    ServerConfig,
    AppConfig,
    ModelMappingEntry,
    get_mapped_model_name,
    normalize_model_mapping,
)
from app.models.provider import Provider


@pytest.mark.unit
class TestProviderConfig:
    """Test ProviderConfig model"""

    def test_create_provider_config(self):
        """Test creating valid ProviderConfig"""
        provider = ProviderConfig(
            name="test-provider",
            api_base="https://api.test.com/v1",
            api_key="test-key-123",
            weight=5,
            model_mapping={"gpt-4": "gpt-4-0613"},
        )

        assert provider.name == "test-provider"
        assert provider.api_base == "https://api.test.com/v1"
        assert provider.api_key == "test-key-123"
        assert provider.weight == 5
        assert provider.model_mapping == {"gpt-4": "gpt-4-0613"}

    def test_provider_config_default_weight(self):
        """Test ProviderConfig uses default weight of 1"""
        provider = ProviderConfig(
            name="test", api_base="https://api.test.com", api_key="key"
        )
        assert provider.weight == 1

    def test_provider_config_default_model_mapping(self):
        """Test ProviderConfig uses empty dict for model_mapping by default"""
        provider = ProviderConfig(
            name="test", api_base="https://api.test.com", api_key="key"
        )
        assert provider.model_mapping == {}

    def test_provider_config_weight_validation(self):
        """Test ProviderConfig validates weight >= 1"""
        with pytest.raises(ValidationError) as exc_info:
            ProviderConfig(
                name="test", api_base="https://api.test.com", api_key="key", weight=0
            )
        assert "weight" in str(exc_info.value)

    def test_provider_config_negative_weight(self):
        """Test ProviderConfig rejects negative weight"""
        with pytest.raises(ValidationError):
            ProviderConfig(
                name="test", api_base="https://api.test.com", api_key="key", weight=-1
            )

    def test_provider_config_missing_required_fields(self):
        """Test ProviderConfig requires name, api_base, api_key"""
        with pytest.raises(ValidationError):
            ProviderConfig(name="test")

    def test_provider_config_serialization(self):
        """Test ProviderConfig can be serialized to dict"""
        provider = ProviderConfig(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=3,
            model_mapping={"model1": "actual-model1"},
        )

        data = provider.model_dump()
        assert data["name"] == "test"
        assert data["weight"] == 3
        assert data["model_mapping"] == {"model1": "actual-model1"}

    def test_provider_config_from_dict(self):
        """Test creating ProviderConfig from dictionary"""
        data = {
            "name": "test",
            "api_base": "https://api.test.com",
            "api_key": "key",
            "weight": 2,
            "model_mapping": {"gpt-4": "gpt-4-0613"},
        }

        provider = ProviderConfig(**data)
        assert provider.name == "test"
        assert provider.weight == 2


@pytest.mark.unit
class TestServerConfig:
    """Test ServerConfig model"""

    def test_create_server_config(self):
        """Test creating ServerConfig with custom values"""
        server = ServerConfig(host="127.0.0.1", port=8080)

        assert server.host == "127.0.0.1"
        assert server.port == 8080

    def test_server_config_defaults(self):
        """Test ServerConfig default values"""
        server = ServerConfig()

        assert server.host == "0.0.0.0"
        assert server.port == 18000

    def test_server_config_partial_defaults(self):
        """Test ServerConfig with some defaults"""
        server = ServerConfig(port=9000)

        assert server.host == "0.0.0.0"
        assert server.port == 9000

    def test_server_config_serialization(self):
        """Test ServerConfig serialization"""
        server = ServerConfig(host="localhost", port=3000)
        data = server.model_dump()

        assert data["host"] == "localhost"
        assert data["port"] == 3000


@pytest.mark.unit
class TestAppConfig:
    """Test AppConfig model"""

    def test_create_app_config(self):
        """Test creating valid AppConfig"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="provider1", api_base="https://api1.com", api_key="key1"
                )
            ],
            server=ServerConfig(port=8080),
            verify_ssl=False,
        )

        assert len(config.providers) == 1
        assert config.providers[0].name == "provider1"
        assert config.server.port == 8080
        assert config.verify_ssl is False

    def test_app_config_default_server(self):
        """Test AppConfig uses default ServerConfig"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test", api_base="https://api.test.com", api_key="key"
                )
            ],
            verify_ssl=True,
        )

        assert config.server.host == "0.0.0.0"
        assert config.server.port == 18000

    def test_app_config_default_verify_ssl(self):
        """Test AppConfig defaults verify_ssl to True"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test", api_base="https://api.test.com", api_key="key"
                )
            ]
        )

        assert config.verify_ssl is True

    def test_app_config_multiple_providers(self):
        """Test AppConfig with multiple providers"""
        config = AppConfig(
            providers=[
                ProviderConfig(name="p1", api_base="https://api1.com", api_key="key1"),
                ProviderConfig(name="p2", api_base="https://api2.com", api_key="key2"),
                ProviderConfig(name="p3", api_base="https://api3.com", api_key="key3"),
            ],
            verify_ssl=True,
        )

        assert len(config.providers) == 3
        assert config.providers[0].name == "p1"
        assert config.providers[1].name == "p2"
        assert config.providers[2].name == "p3"

    def test_app_config_empty_providers(self):
        """Test AppConfig allows empty providers (database mode starts empty)"""
        config = AppConfig(providers=[], verify_ssl=True)
        assert len(config.providers) == 0

    def test_app_config_serialization(self):
        """Test AppConfig serialization"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test",
                    api_base="https://api.test.com",
                    api_key="key",
                    weight=2,
                )
            ],
            server=ServerConfig(port=9000),
            verify_ssl=False,
        )

        data = config.model_dump()
        assert len(data["providers"]) == 1
        assert data["providers"][0]["name"] == "test"
        assert data["server"]["port"] == 9000
        assert data["verify_ssl"] is False


@pytest.mark.unit
class TestProvider:
    """Test Provider dataclass"""

    def test_create_provider(self):
        """Test creating Provider instance"""
        provider = Provider(
            name="test-provider",
            api_base="https://api.test.com/v1",
            api_key="test-key",
            weight=3,
            model_mapping={"gpt-4": "gpt-4-0613"},
        )

        assert provider.name == "test-provider"
        assert provider.api_base == "https://api.test.com/v1"
        assert provider.api_key == "test-key"
        assert provider.weight == 3
        assert provider.model_mapping == {"gpt-4": "gpt-4-0613"}

    def test_provider_equality(self):
        """Test Provider equality comparison"""
        provider1 = Provider(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping={},
        )

        provider2 = Provider(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping={},
        )

        assert provider1 == provider2

    def test_provider_inequality(self):
        """Test Provider inequality"""
        provider1 = Provider(
            name="test1",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping={},
        )

        provider2 = Provider(
            name="test2",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping={},
        )

        assert provider1 != provider2

    def test_provider_empty_model_mapping(self):
        """Test Provider with empty model mapping"""
        provider = Provider(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping={},
        )

        assert provider.model_mapping == {}

    def test_provider_complex_model_mapping(self):
        """Test Provider with complex model mapping"""
        mapping = {
            "gpt-4": "gpt-4-0613",
            "gpt-3.5-turbo": "gpt-3.5-turbo-0613",
            "claude-3": "claude-3-opus-20240229",
        }

        provider = Provider(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping=mapping,
        )

        assert len(provider.model_mapping) == 3
        assert provider.model_mapping["gpt-4"] == "gpt-4-0613"
        assert provider.model_mapping["claude-3"] == "claude-3-opus-20240229"

    def test_provider_wildcard_model_mapping(self):
        """Test Provider with wildcard/regex model mapping patterns"""
        mapping = {
            "gpt-4": "gpt-4-exact",  # Exact match
            "claude-opus-4-5-.*": "claude-opus-mapped",  # Regex pattern
            "gemini-*": "gemini-mapped",  # Simple wildcard
        }

        provider = Provider(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping=mapping,
        )

        # Test exact match (highest priority)
        assert provider.supports_model("gpt-4") is True
        assert provider.get_mapped_model("gpt-4") == "gpt-4-exact"

        # Test regex pattern matching
        assert provider.supports_model("claude-opus-4-5-20240620") is True
        assert (
            provider.get_mapped_model("claude-opus-4-5-20240620")
            == "claude-opus-mapped"
        )
        assert provider.supports_model("claude-opus-4-5-latest") is True
        assert (
            provider.get_mapped_model("claude-opus-4-5-latest") == "claude-opus-mapped"
        )

        # Test simple wildcard matching
        assert provider.supports_model("gemini-pro") is True
        assert provider.get_mapped_model("gemini-pro") == "gemini-mapped"
        assert provider.supports_model("gemini-ultra") is True
        assert provider.get_mapped_model("gemini-ultra") == "gemini-mapped"

        # Test non-matching models
        assert provider.supports_model("gpt-3.5-turbo") is False
        assert (
            provider.get_mapped_model("gpt-3.5-turbo") == "gpt-3.5-turbo"
        )  # Returns original
        assert provider.supports_model("claude-sonnet") is False

    def test_provider_exact_match_priority(self):
        """Test that exact matches take priority over pattern matches"""
        mapping = {
            "claude-.*": "claude-pattern",  # Pattern
            "claude-opus": "claude-opus-exact",  # Exact match
        }

        provider = Provider(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping=mapping,
        )

        # Exact match should take priority
        assert provider.get_mapped_model("claude-opus") == "claude-opus-exact"

        # Pattern should match other claude models
        assert provider.get_mapped_model("claude-sonnet") == "claude-pattern"


@pytest.mark.unit
class TestModelIntegration:
    """Test integration between different models"""

    def test_provider_config_to_provider(self):
        """Test converting ProviderConfig to Provider"""
        config = ProviderConfig(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=2,
            model_mapping={"gpt-4": "gpt-4-0613"},
        )

        provider = Provider(
            name=config.name,
            api_base=config.api_base,
            api_key=config.api_key,
            weight=config.weight,
            model_mapping=config.model_mapping,
        )

        assert provider.name == config.name
        assert provider.weight == config.weight
        assert provider.model_mapping == config.model_mapping

    def test_app_config_with_multiple_provider_configs(self):
        """Test AppConfig with multiple ProviderConfigs"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="provider1",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=2,
                ),
                ProviderConfig(
                    name="provider2",
                    api_base="https://api2.com",
                    api_key="key2",
                    weight=1,
                ),
            ],
            verify_ssl=True,
        )

        # Convert to Provider instances
        providers = [
            Provider(
                name=p.name,
                api_base=p.api_base,
                api_key=p.api_key,
                weight=p.weight,
                model_mapping=p.model_mapping,
            )
            for p in config.providers
        ]

        assert len(providers) == 2
        assert providers[0].weight == 2
        assert providers[1].weight == 1


@pytest.mark.unit
class TestModelMappingExtended:
    """Test extended model mapping with metadata"""

    def test_model_mapping_entry_creation(self):
        """Test creating ModelMappingEntry with all fields"""
        entry = ModelMappingEntry(
            mapped_model="claude-3-sonnet",
            max_tokens=200000,
            max_input_tokens=200000,
            max_output_tokens=4096,
            input_cost_per_1k_tokens=0.003,
            output_cost_per_1k_tokens=0.015,
            supports_vision=True,
            supports_function_calling=True,
            supports_streaming=True,
            supports_response_schema=True,
            supports_reasoning=False,
            mode="chat",
        )

        assert entry.mapped_model == "claude-3-sonnet"
        assert entry.max_tokens == 200000
        assert entry.max_output_tokens == 4096
        assert entry.input_cost_per_1k_tokens == 0.003
        assert entry.supports_vision is True
        assert entry.mode == "chat"

    def test_model_mapping_entry_minimal(self):
        """Test ModelMappingEntry with only required field"""
        entry = ModelMappingEntry(mapped_model="gpt-4-turbo")

        assert entry.mapped_model == "gpt-4-turbo"
        assert entry.max_tokens is None
        assert entry.supports_vision is None

    def test_simple_string_format_backward_compat(self):
        """Test backward compatible string format in ProviderConfig"""
        provider = ProviderConfig(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            model_mapping={"gpt-4": "gpt-4-turbo"},
        )

        # Simple string format should still work
        assert "gpt-4" in provider.model_mapping
        value = provider.model_mapping["gpt-4"]
        assert isinstance(value, str)
        assert value == "gpt-4-turbo"

    def test_extended_object_format(self):
        """Test extended object format with metadata"""
        provider = ProviderConfig(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            model_mapping={
                "claude-3": {
                    "mapped_model": "claude-3-sonnet",
                    "max_tokens": 200000,
                    "supports_vision": True,
                }
            },
        )

        entry = provider.model_mapping["claude-3"]
        assert isinstance(entry, ModelMappingEntry)
        assert entry.mapped_model == "claude-3-sonnet"
        assert entry.max_tokens == 200000
        assert entry.supports_vision is True

    def test_mixed_formats(self):
        """Test mixed simple and extended formats in same provider"""
        provider = ProviderConfig(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            model_mapping={
                "gpt-4": "gpt-4-turbo",  # Simple format
                "claude-3": {  # Extended format
                    "mapped_model": "claude-3-sonnet",
                    "max_tokens": 200000,
                },
            },
        )

        # Check simple format
        assert isinstance(provider.model_mapping["gpt-4"], str)
        assert provider.model_mapping["gpt-4"] == "gpt-4-turbo"

        # Check extended format
        assert isinstance(provider.model_mapping["claude-3"], ModelMappingEntry)
        assert provider.model_mapping["claude-3"].mapped_model == "claude-3-sonnet"

    def test_get_mapped_model_name_from_string(self):
        """Test extracting mapped_model from simple string"""
        assert get_mapped_model_name("gpt-4-turbo") == "gpt-4-turbo"

    def test_get_mapped_model_name_from_entry(self):
        """Test extracting mapped_model from ModelMappingEntry"""
        entry = ModelMappingEntry(mapped_model="claude-3-sonnet", max_tokens=200000)
        assert get_mapped_model_name(entry) == "claude-3-sonnet"

    def test_normalize_model_mapping(self):
        """Test normalize_model_mapping function"""
        raw = {
            "gpt-4": "gpt-4-turbo",
            "claude-3": {"mapped_model": "claude-3-sonnet", "max_tokens": 200000},
        }

        normalized = normalize_model_mapping(raw)

        assert isinstance(normalized["gpt-4"], str)
        assert isinstance(normalized["claude-3"], ModelMappingEntry)

    def test_provider_with_extended_mapping(self):
        """Test Provider dataclass with extended model mapping"""
        mapping = {
            "gpt-4": "gpt-4-turbo",
            "claude-3": ModelMappingEntry(
                mapped_model="claude-3-sonnet",
                max_tokens=200000,
                supports_vision=True,
            ),
        }

        provider = Provider(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping=mapping,
        )

        # Test supports_model with both formats
        assert provider.supports_model("gpt-4") is True
        assert provider.supports_model("claude-3") is True

        # Test get_mapped_model with both formats
        assert provider.get_mapped_model("gpt-4") == "gpt-4-turbo"
        assert provider.get_mapped_model("claude-3") == "claude-3-sonnet"

    def test_provider_get_model_metadata(self):
        """Test Provider.get_model_metadata method"""
        mapping = {
            "gpt-4": "gpt-4-turbo",  # Simple format - no metadata
            "claude-3": ModelMappingEntry(
                mapped_model="claude-3-sonnet",
                max_tokens=200000,
                supports_vision=True,
            ),
        }

        provider = Provider(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping=mapping,
        )

        # Simple format returns None
        assert provider.get_model_metadata("gpt-4") is None

        # Extended format returns metadata
        metadata = provider.get_model_metadata("claude-3")
        assert metadata is not None
        assert metadata.max_tokens == 200000
        assert metadata.supports_vision is True

    def test_pattern_matching_with_extended_format(self):
        """Test pattern matching works with extended format"""
        mapping = {
            "claude-opus-4-5-.*": ModelMappingEntry(
                mapped_model="claude-opus-mapped",
                max_tokens=128000,
                supports_reasoning=True,
            ),
        }

        provider = Provider(
            name="test",
            api_base="https://api.test.com",
            api_key="key",
            weight=1,
            model_mapping=mapping,
        )

        # Pattern should match
        assert provider.supports_model("claude-opus-4-5-20240620") is True
        assert (
            provider.get_mapped_model("claude-opus-4-5-20240620")
            == "claude-opus-mapped"
        )

        # Metadata should be available through pattern match
        metadata = provider.get_model_metadata("claude-opus-4-5-20240620")
        assert metadata is not None
        assert metadata.max_tokens == 128000
        assert metadata.supports_reasoning is True

    def test_model_mapping_entry_serialization(self):
        """Test ModelMappingEntry serialization excludes None values"""
        entry = ModelMappingEntry(
            mapped_model="test-model",
            max_tokens=100000,
            # All other fields are None
        )

        data = entry.model_dump(exclude_none=True)
        assert data == {"mapped_model": "test-model", "max_tokens": 100000}
        assert "supports_vision" not in data

    def test_provider_config_from_db_format(self):
        """Test ProviderConfig can parse database JSONB format"""
        # Simulating data from PostgreSQL JSONB
        db_data = {
            "name": "db-provider",
            "api_base": "https://api.test.com",
            "api_key": "key",
            "model_mapping": {
                "gpt-4": "gpt-4-turbo",
                "claude-3": {
                    "mapped_model": "claude-3-sonnet",
                    "max_tokens": 200000,
                    "max_output_tokens": 4096,
                    "input_cost_per_1k_tokens": 0.003,
                    "output_cost_per_1k_tokens": 0.015,
                    "supports_vision": True,
                    "supports_function_calling": True,
                    "mode": "chat",
                },
            },
        }

        provider = ProviderConfig(**db_data)

        # Verify simple format
        assert provider.model_mapping["gpt-4"] == "gpt-4-turbo"

        # Verify extended format
        claude = provider.model_mapping["claude-3"]
        assert isinstance(claude, ModelMappingEntry)
        assert claude.mapped_model == "claude-3-sonnet"
        assert claude.max_tokens == 200000
        assert claude.input_cost_per_1k_tokens == 0.003
        assert claude.supports_vision is True
        assert claude.mode == "chat"
