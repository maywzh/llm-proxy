"""Tests for shared utility functions."""

from unittest.mock import patch

from app.core.utils import strip_provider_suffix


class TestStripProviderSuffix:
    """Tests for strip_provider_suffix function."""

    def test_no_suffix_configured(self):
        """When no provider suffix is configured, return model as-is."""
        with patch("app.core.utils.get_env_config") as mock_get_env_config:
            mock_config = type("MockConfig", (), {"provider_suffix": None})()
            mock_get_env_config.return_value = mock_config
            assert strip_provider_suffix("gpt-4") == "gpt-4"
            assert strip_provider_suffix("openrouter/gpt-4") == "openrouter/gpt-4"

    def test_empty_suffix_configured(self):
        """When empty provider suffix is configured, return model as-is."""
        with patch("app.core.utils.get_env_config") as mock_get_env_config:
            mock_config = type("MockConfig", (), {"provider_suffix": ""})()
            mock_get_env_config.return_value = mock_config
            assert strip_provider_suffix("gpt-4") == "gpt-4"

    def test_suffix_matches(self):
        """When model starts with provider suffix, strip it."""
        with patch("app.core.utils.get_env_config") as mock_get_env_config:
            mock_config = type("MockConfig", (), {"provider_suffix": "openrouter"})()
            mock_get_env_config.return_value = mock_config
            assert strip_provider_suffix("openrouter/gpt-4") == "gpt-4"
            assert strip_provider_suffix("openrouter/claude-3") == "claude-3"

    def test_suffix_no_match(self):
        """When model doesn't start with provider suffix, return as-is."""
        with patch("app.core.utils.get_env_config") as mock_get_env_config:
            mock_config = type("MockConfig", (), {"provider_suffix": "openrouter"})()
            mock_get_env_config.return_value = mock_config
            assert strip_provider_suffix("gpt-4") == "gpt-4"
            assert strip_provider_suffix("other/gpt-4") == "other/gpt-4"

    def test_partial_match_not_stripped(self):
        """Partial matches should not be stripped."""
        with patch("app.core.utils.get_env_config") as mock_get_env_config:
            mock_config = type("MockConfig", (), {"provider_suffix": "open"})()
            mock_get_env_config.return_value = mock_config
            # "openrouter/gpt-4" should NOT match "open/" prefix
            assert strip_provider_suffix("openrouter/gpt-4") == "openrouter/gpt-4"
