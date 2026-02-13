"""Tests for V2 API endpoints authentication and model permissions.

These tests verify that V2 endpoints properly require authentication
by testing the verify_auth dependency directly.
"""

import pytest
from unittest.mock import MagicMock
from fastapi import HTTPException

from app.api.dependencies import verify_auth, check_model_permission
from app.core.database import hash_key


def _mock_request():
    """Create a mock Request with url.path for verify_auth."""
    req = MagicMock()
    req.url.path = "/v2/test"
    return req


@pytest.mark.unit
class TestV2EndpointsAuthDependency:
    """Tests for V2 endpoints authentication dependency."""

    @pytest.mark.asyncio
    async def test_verify_auth_requires_credentials_when_configured(
        self, monkeypatch, clear_config_cache
    ):
        """Test that verify_auth raises 401 when credentials are configured but not provided."""
        from app.models.config import (
            AppConfig,
            ProviderConfig,
            CredentialConfig,
            RateLimitConfig,
        )
        from app.core import security as security_module
        from app.core.security import init_rate_limiter

        raw_key = "secret-key"
        hashed_key = hash_key(raw_key)

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test", api_base="https://api.test.com", api_key="key"
                )
            ],
            credentials=[
                CredentialConfig(
                    credential_key=hashed_key,
                    name="test-key",
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20),
                )
            ],
            verify_ssl=True,
        )

        monkeypatch.setattr(security_module, "get_config", lambda: config)
        init_rate_limiter()

        # No authorization should raise 401
        with pytest.raises(HTTPException) as exc_info:
            await verify_auth(_mock_request(), authorization=None, x_api_key=None)
        assert exc_info.value.status_code == 401

    @pytest.mark.asyncio
    async def test_verify_auth_accepts_x_api_key(self, monkeypatch, clear_config_cache):
        """Test that verify_auth accepts x-api-key header."""
        from app.models.config import (
            AppConfig,
            ProviderConfig,
            CredentialConfig,
            RateLimitConfig,
        )
        from app.core import security as security_module
        from app.core.security import init_rate_limiter

        raw_key = "secret-key"
        hashed_key = hash_key(raw_key)

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test", api_base="https://api.test.com", api_key="key"
                )
            ],
            credentials=[
                CredentialConfig(
                    credential_key=hashed_key,
                    name="test-key",
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20),
                )
            ],
            verify_ssl=True,
        )

        monkeypatch.setattr(security_module, "get_config", lambda: config)
        init_rate_limiter()

        # x-api-key should work
        result = await verify_auth(
            _mock_request(), authorization=None, x_api_key=raw_key
        )
        assert result is not None
        assert result.name == "test-key"

    @pytest.mark.asyncio
    async def test_verify_auth_x_api_key_precedence(
        self, monkeypatch, clear_config_cache
    ):
        """Test that x-api-key takes precedence over Authorization header."""
        from app.models.config import (
            AppConfig,
            ProviderConfig,
            CredentialConfig,
            RateLimitConfig,
        )
        from app.core import security as security_module
        from app.core.security import init_rate_limiter

        valid_key = "valid-key"
        invalid_key = "invalid-key"
        hashed_valid_key = hash_key(valid_key)

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="test", api_base="https://api.test.com", api_key="key"
                )
            ],
            credentials=[
                CredentialConfig(
                    credential_key=hashed_valid_key,
                    name="valid-key-name",
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20),
                )
            ],
            verify_ssl=True,
        )

        monkeypatch.setattr(security_module, "get_config", lambda: config)
        init_rate_limiter()

        # Valid x-api-key with invalid Bearer should succeed
        result = await verify_auth(
            _mock_request(),
            authorization=f"Bearer {invalid_key}",
            x_api_key=valid_key,
        )
        assert result is not None
        assert result.name == "valid-key-name"

        # Invalid x-api-key with valid Bearer should fail (x-api-key takes precedence)
        with pytest.raises(HTTPException) as exc_info:
            await verify_auth(
                _mock_request(),
                authorization=f"Bearer {valid_key}",
                x_api_key=invalid_key,
            )
        assert exc_info.value.status_code == 401


@pytest.mark.unit
class TestV2ModelPermissionCheck:
    """Tests for model permission checking in V2 endpoints."""

    def test_check_model_permission_allows_when_no_credential(self):
        """Test that model permission check passes when no credential is provided."""
        # Should not raise
        check_model_permission("gpt-4", None)

    def test_check_model_permission_allows_when_no_restrictions(self):
        """Test that model permission check passes when credential has no restrictions."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = []
        # Should not raise
        check_model_permission("gpt-4", mock_credential)

    def test_check_model_permission_allows_matching_model(self):
        """Test that model permission check passes for allowed models."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["gpt-4", "gpt-3.5-turbo"]
        # Should not raise
        check_model_permission("gpt-4", mock_credential)

    def test_check_model_permission_denies_unauthorized_model(self):
        """Test that model permission check raises 403 for unauthorized models."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["gpt-4"]

        with pytest.raises(HTTPException) as exc_info:
            check_model_permission("claude-3", mock_credential)
        assert exc_info.value.status_code == 403
        assert "not allowed" in exc_info.value.detail.lower()

    def test_check_model_permission_supports_wildcard(self):
        """Test that model permission check supports wildcard patterns."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["gpt-*"]

        # Should not raise for matching pattern
        check_model_permission("gpt-4", mock_credential)
        check_model_permission("gpt-3.5-turbo", mock_credential)

        # Should raise for non-matching
        with pytest.raises(HTTPException) as exc_info:
            check_model_permission("claude-3", mock_credential)
        assert exc_info.value.status_code == 403

    def test_check_model_permission_supports_regex(self):
        """Test that model permission check supports regex patterns."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = [
            "claude-haiku-4-5-.+"
        ]  # Requires at least one char after dash

        # Should not raise for matching pattern
        check_model_permission("claude-haiku-4-5-20241022", mock_credential)

        # Should raise for non-matching (no suffix after dash)
        with pytest.raises(HTTPException) as exc_info:
            check_model_permission("claude-haiku-4-5-", mock_credential)
        assert exc_info.value.status_code == 403

    def test_check_model_permission_exact_match_no_partial(self):
        """Test that exact model names don't allow partial matches."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["grok-4"]

        # Exact match should work
        check_model_permission("grok-4", mock_credential)

        # Partial match should fail
        with pytest.raises(HTTPException) as exc_info:
            check_model_permission("grok-4-vision", mock_credential)
        assert exc_info.value.status_code == 403
