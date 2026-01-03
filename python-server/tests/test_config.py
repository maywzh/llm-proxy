"""Tests for configuration management"""

import pytest

from app.core.config import (
    get_config,
    set_config,
    clear_config_cache,
    get_env_config,
    EnvConfig,
)
from app.models.config import AppConfig, ProviderConfig, ServerConfig


@pytest.mark.unit
class TestGetConfig:
    """Test cached configuration retrieval"""

    def test_get_config_returns_empty_config_when_not_set(self, clear_config_cache):
        """Test that get_config returns empty config when not set"""
        config = get_config()
        assert isinstance(config, AppConfig)
        assert len(config.providers) == 0
        assert len(config.master_keys) == 0

    def test_set_config_updates_cached_config(self, clear_config_cache):
        """Test that set_config updates the cached configuration"""
        test_config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test", api_base="https://api.test.com", api_key="test-key"
                )
            ],
            verify_ssl=True,
        )
        set_config(test_config)

        config = get_config()
        assert len(config.providers) == 1
        assert config.providers[0].name == "test"

    def test_clear_config_cache_resets_cache(self, clear_config_cache):
        """Test that clear_config_cache resets the cache"""
        from app.core.config import clear_config_cache as do_clear_cache
        from app.core import config as config_module

        test_config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test", api_base="https://api.test.com", api_key="test-key"
                )
            ],
            verify_ssl=True,
        )
        set_config(test_config)

        config1 = get_config()
        assert len(config1.providers) == 1

        config_module._cached_config = None
        do_clear_cache()

        config2 = get_config()
        assert len(config2.providers) == 0


@pytest.mark.unit
class TestEnvConfig:
    """Test EnvConfig"""

    def test_from_env_defaults(self, monkeypatch):
        """Test default values from environment"""
        monkeypatch.delenv("HOST", raising=False)
        monkeypatch.delenv("PORT", raising=False)
        monkeypatch.delenv("VERIFY_SSL", raising=False)
        monkeypatch.delenv("REQUEST_TIMEOUT_SECS", raising=False)
        monkeypatch.delenv("DB_URL", raising=False)
        monkeypatch.delenv("ADMIN_KEY", raising=False)

        config = EnvConfig.from_env()
        assert config.host == "0.0.0.0"
        assert config.port == 18000
        assert config.verify_ssl is True
        assert config.request_timeout_secs == 300
        assert config.db_url is None
        assert config.admin_key is None

    def test_from_env_with_values(self, monkeypatch):
        """Test loading values from environment"""
        monkeypatch.setenv("HOST", "127.0.0.1")
        monkeypatch.setenv("PORT", "8080")
        monkeypatch.setenv("VERIFY_SSL", "false")
        monkeypatch.setenv("REQUEST_TIMEOUT_SECS", "600")
        monkeypatch.setenv("DB_URL", "postgresql://localhost/test")
        monkeypatch.setenv("ADMIN_KEY", "admin-secret")

        config = EnvConfig.from_env()
        assert config.host == "127.0.0.1"
        assert config.port == 8080
        assert config.verify_ssl is False
        assert config.request_timeout_secs == 600
        assert config.db_url == "postgresql://localhost/test"
        assert config.admin_key == "admin-secret"

    def test_get_env_config(self, monkeypatch):
        """Test get_env_config function"""
        monkeypatch.setenv("DB_URL", "postgresql://localhost/test")
        config = get_env_config()
        assert isinstance(config, EnvConfig)
        assert config.db_url == "postgresql://localhost/test"


@pytest.mark.unit
class TestConfigModels:
    """Test configuration model validation"""

    def test_provider_config_validation(self):
        """Test ProviderConfig validation"""
        provider = ProviderConfig(
            name="test",
            api_base="https://api.test.com",
            api_key="test-key",
            weight=5,
            model_mapping={"gpt-4": "gpt-4-0613"},
        )
        assert provider.name == "test"
        assert provider.weight == 5

    def test_provider_config_default_weight(self):
        """Test ProviderConfig default weight"""
        provider = ProviderConfig(
            name="test", api_base="https://api.test.com", api_key="test-key"
        )
        assert provider.weight == 1

    def test_provider_config_invalid_weight(self):
        """Test ProviderConfig rejects invalid weight"""
        with pytest.raises(ValueError):
            ProviderConfig(
                name="test",
                api_base="https://api.test.com",
                api_key="test-key",
                weight=0,
            )

    def test_server_config_defaults(self):
        """Test ServerConfig default values"""
        server = ServerConfig()
        assert server.host == "0.0.0.0"
        assert server.port == 18000

    def test_app_config_validation(self):
        """Test AppConfig validation"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test", api_base="https://api.test.com", api_key="test-key"
                )
            ],
            server=ServerConfig(port=8080),
            verify_ssl=False,
        )
        assert len(config.providers) == 1
        assert config.server.port == 8080
        assert config.verify_ssl is False

    def test_app_config_allows_empty_providers(self):
        """Test AppConfig allows empty providers (database mode starts empty)"""
        config = AppConfig(providers=[], verify_ssl=True)
        assert len(config.providers) == 0
