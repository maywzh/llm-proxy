"""Tests for rate limiting functionality"""

import pytest
from fastapi import HTTPException

from app.models.config import CredentialConfig, RateLimitConfig
from app.core.rate_limiter import RateLimiter
from app.core.security import verify_credential_key
from app.core.database import hash_key


class TestRateLimiter:
    """Test RateLimiter class"""

    def test_register_key(self):
        """Test registering a new key"""
        limiter = RateLimiter()
        limiter.register_key("test-key", requests_per_second=10, burst_size=20)

        # Should allow first request
        assert limiter.check_rate_limit("test-key") is True

    def test_rate_limit_enforcement(self):
        """Test that rate limits are enforced"""
        limiter = RateLimiter()
        # Very low rate limit for testing
        limiter.register_key("test-key", requests_per_second=2, burst_size=2)

        # First 2 requests should succeed (burst)
        assert limiter.check_rate_limit("test-key") is True
        assert limiter.check_rate_limit("test-key") is True

        # Third request should fail
        assert limiter.check_rate_limit("test-key") is False

    def test_independent_key_limits(self):
        """Test that different keys have independent rate limits"""
        limiter = RateLimiter()
        limiter.register_key("key1", requests_per_second=2, burst_size=2)
        limiter.register_key("key2", requests_per_second=10, burst_size=10)

        # Exhaust key1's limit
        assert limiter.check_rate_limit("key1") is True
        assert limiter.check_rate_limit("key1") is True
        assert limiter.check_rate_limit("key1") is False

        # key2 should still work
        assert limiter.check_rate_limit("key2") is True
        assert limiter.check_rate_limit("key2") is True

    def test_unregistered_key(self):
        """Test behavior with unregistered key"""
        limiter = RateLimiter()
        # Unregistered key should return True (no rate limit configured = allow access)
        assert limiter.check_rate_limit("unknown-key") is True


class TestVerifyCredentialKey:
    """Test verify_credential_key function"""

    def test_no_credentials_configured(self, monkeypatch):
        """Test when no credentials are configured"""

        # Mock config with no credentials
        class MockConfig:
            credentials = []

        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())

        # Should allow access without authentication
        is_valid, credential_config = verify_credential_key(None)
        assert is_valid is True
        assert credential_config is None

    def test_credentials_valid(self, monkeypatch):
        """Test credentials with valid key (hashed)"""
        raw_key_1 = "sk-key-1"
        raw_key_2 = "sk-key-2"
        test_credentials = [
            CredentialConfig(
                credential_key=hash_key(raw_key_1),
                name="key-1",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20),
            ),
            CredentialConfig(
                credential_key=hash_key(raw_key_2),
                name="key-2",
                rate_limit=RateLimitConfig(requests_per_second=5, burst_size=10),
            ),
        ]

        class MockConfig:
            credentials = test_credentials

        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())

        # Initialize rate limiter
        from app.core.security import init_rate_limiter

        init_rate_limiter()

        is_valid, credential_config = verify_credential_key(f"Bearer {raw_key_1}")
        assert is_valid is True
        assert credential_config is not None
        assert credential_config.name == "key-1"

    def test_credentials_invalid(self, monkeypatch):
        """Test credentials with invalid key"""
        raw_key = "sk-key-1"
        test_credentials = [
            CredentialConfig(
                credential_key=hash_key(raw_key),
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20),
            )
        ]

        class MockConfig:
            credentials = test_credentials

        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())

        with pytest.raises(HTTPException) as exc_info:
            verify_credential_key("Bearer wrong-key")

        assert exc_info.value.status_code == 401

    def test_credentials_missing_header(self, monkeypatch):
        """Test credentials with missing authorization header"""
        raw_key = "sk-key-1"
        test_credentials = [
            CredentialConfig(
                credential_key=hash_key(raw_key),
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20),
            )
        ]

        class MockConfig:
            credentials = test_credentials

        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())

        with pytest.raises(HTTPException) as exc_info:
            verify_credential_key(None)

        assert exc_info.value.status_code == 401

    def test_rate_limit_exceeded(self, monkeypatch):
        """Test rate limit enforcement"""
        raw_key = "sk-key-1"
        hashed_key = hash_key(raw_key)
        test_credentials = [
            CredentialConfig(
                credential_key=hashed_key,
                name="key-1",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2),
            )
        ]

        class MockConfig:
            credentials = test_credentials

        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())

        # Initialize rate limiter
        from app.core.security import init_rate_limiter

        init_rate_limiter()

        # First 2 requests should succeed
        is_valid, credential_config = verify_credential_key(f"Bearer {raw_key}")
        assert is_valid is True

        is_valid, credential_config = verify_credential_key(f"Bearer {raw_key}")
        assert is_valid is True

        # Third request should fail with 429
        with pytest.raises(HTTPException) as exc_info:
            verify_credential_key(f"Bearer {raw_key}")

        assert exc_info.value.status_code == 429
        assert "Rate limit exceeded" in exc_info.value.detail


class TestRateLimitingIntegration:
    """Integration tests for rate limiting"""

    def test_multiple_keys_independent_limits(self, monkeypatch):
        """Test that multiple keys have independent rate limits"""
        raw_fast_key = "sk-fast-key"
        raw_slow_key = "sk-slow-key"
        test_credentials = [
            CredentialConfig(
                credential_key=hash_key(raw_fast_key),
                name="fast-key",
                rate_limit=RateLimitConfig(requests_per_second=100, burst_size=100),
            ),
            CredentialConfig(
                credential_key=hash_key(raw_slow_key),
                name="slow-key",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2),
            ),
        ]

        class MockConfig:
            credentials = test_credentials

        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())

        # Initialize rate limiter
        from app.core.security import init_rate_limiter

        init_rate_limiter()

        # Exhaust slow key
        verify_credential_key(f"Bearer {raw_slow_key}")
        verify_credential_key(f"Bearer {raw_slow_key}")

        with pytest.raises(HTTPException) as exc_info:
            verify_credential_key(f"Bearer {raw_slow_key}")
        assert exc_info.value.status_code == 429

        # Fast key should still work
        is_valid, credential_config = verify_credential_key(f"Bearer {raw_fast_key}")
        assert is_valid is True
        assert credential_config is not None
        assert credential_config.name == "fast-key"

    def test_unlimited_key_no_rate_limit(self, monkeypatch):
        """Test that keys without rate_limit field have unlimited requests"""
        raw_unlimited_key = "sk-unlimited-key"
        raw_limited_key = "sk-limited-key"
        test_credentials = [
            CredentialConfig(
                credential_key=hash_key(raw_unlimited_key),
                name="unlimited-key",
                rate_limit=None,  # No rate limiting
            ),
            CredentialConfig(
                credential_key=hash_key(raw_limited_key),
                name="limited-key",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2),
            ),
        ]

        class MockConfig:
            credentials = test_credentials

        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())

        # Initialize rate limiter
        from app.core.security import init_rate_limiter

        init_rate_limiter()

        # Unlimited key should allow many requests
        for _ in range(100):
            is_valid, credential_config = verify_credential_key(
                f"Bearer {raw_unlimited_key}"
            )
            assert is_valid is True
            assert credential_config is not None
            assert credential_config.name == "unlimited-key"

        # Limited key should be rate limited
        verify_credential_key(f"Bearer {raw_limited_key}")
        verify_credential_key(f"Bearer {raw_limited_key}")

        with pytest.raises(HTTPException) as exc_info:
            verify_credential_key(f"Bearer {raw_limited_key}")
        assert exc_info.value.status_code == 429

    def test_mixed_rate_limits(self, monkeypatch):
        """Test configuration with mixed rate limits (some None, some set)"""
        raw_key_1 = "sk-key-1"
        raw_key_2 = "sk-key-2"
        raw_key_3 = "sk-key-3"
        raw_key_4 = "sk-key-4"
        test_credentials = [
            CredentialConfig(
                credential_key=hash_key(raw_key_1),
                name="key-1",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=10),
            ),
            CredentialConfig(
                credential_key=hash_key(raw_key_2),
                name="key-2",
                rate_limit=None,  # Unlimited
            ),
            CredentialConfig(
                credential_key=hash_key(raw_key_3),
                name="key-3",
                rate_limit=RateLimitConfig(requests_per_second=5, burst_size=5),
            ),
            CredentialConfig(
                credential_key=hash_key(raw_key_4),
                name="key-4",
                rate_limit=None,  # Unlimited
            ),
        ]

        class MockConfig:
            credentials = test_credentials

        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())

        # Initialize rate limiter
        from app.core.security import init_rate_limiter

        init_rate_limiter()

        # Unlimited keys should work without limits
        for _ in range(50):
            verify_credential_key(f"Bearer {raw_key_2}")
            verify_credential_key(f"Bearer {raw_key_4}")

        # Limited keys should be rate limited
        for _ in range(10):
            verify_credential_key(f"Bearer {raw_key_1}")

        with pytest.raises(HTTPException) as exc_info:
            verify_credential_key(f"Bearer {raw_key_1}")
        assert exc_info.value.status_code == 429


class TestRateLimitHotReload:
    """Tests for rate limit hot reload via init_rate_limiter"""

    def test_hot_reload_adds_new_credential(self, monkeypatch):
        """After reload, newly added credential's rate limit is enforced"""
        from app.core import security as security_module

        raw_key_a = "sk-key-a"
        raw_key_b = "sk-key-b"

        # Initial: only key-a
        creds_v1 = [
            CredentialConfig(
                credential_key=hash_key(raw_key_a),
                name="key-a",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=10),
            ),
        ]

        class ConfigV1:
            credentials = creds_v1

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV1())
        security_module.init_rate_limiter()

        # key-a works
        is_valid, _ = verify_credential_key(f"Bearer {raw_key_a}")
        assert is_valid is True

        # Hot reload: add key-b with strict limit
        creds_v2 = creds_v1 + [
            CredentialConfig(
                credential_key=hash_key(raw_key_b),
                name="key-b",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2),
            ),
        ]

        class ConfigV2:
            credentials = creds_v2

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV2())
        security_module.init_rate_limiter()

        # key-b should now be enforced
        verify_credential_key(f"Bearer {raw_key_b}")
        verify_credential_key(f"Bearer {raw_key_b}")
        with pytest.raises(HTTPException) as exc_info:
            verify_credential_key(f"Bearer {raw_key_b}")
        assert exc_info.value.status_code == 429

    def test_hot_reload_removes_rate_limit(self, monkeypatch):
        """After reload, credential whose rate_limit is removed becomes unlimited"""
        from app.core import security as security_module

        raw_key = "sk-key-remove"

        # Initial: key with rate limit
        creds_v1 = [
            CredentialConfig(
                credential_key=hash_key(raw_key),
                name="key-remove",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2),
            ),
        ]

        class ConfigV1:
            credentials = creds_v1

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV1())
        security_module.init_rate_limiter()

        # Exhaust the limit
        verify_credential_key(f"Bearer {raw_key}")
        verify_credential_key(f"Bearer {raw_key}")
        with pytest.raises(HTTPException):
            verify_credential_key(f"Bearer {raw_key}")

        # Hot reload: remove rate limit
        creds_v2 = [
            CredentialConfig(
                credential_key=hash_key(raw_key),
                name="key-remove",
                rate_limit=None,
            ),
        ]

        class ConfigV2:
            credentials = creds_v2

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV2())
        security_module.init_rate_limiter()

        # Should now be unlimited
        for _ in range(50):
            is_valid, _ = verify_credential_key(f"Bearer {raw_key}")
            assert is_valid is True

    def test_hot_reload_updates_rate_limit(self, monkeypatch):
        """After reload, changed rate limit takes effect"""
        from app.core import security as security_module

        raw_key = "sk-key-update"

        # Initial: strict limit (2 rps)
        creds_v1 = [
            CredentialConfig(
                credential_key=hash_key(raw_key),
                name="key-update",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2),
            ),
        ]

        class ConfigV1:
            credentials = creds_v1

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV1())
        security_module.init_rate_limiter()

        # Exhaust the limit
        verify_credential_key(f"Bearer {raw_key}")
        verify_credential_key(f"Bearer {raw_key}")
        with pytest.raises(HTTPException):
            verify_credential_key(f"Bearer {raw_key}")

        # Hot reload: increase to 100 rps
        creds_v2 = [
            CredentialConfig(
                credential_key=hash_key(raw_key),
                name="key-update",
                rate_limit=RateLimitConfig(requests_per_second=100, burst_size=100),
            ),
        ]

        class ConfigV2:
            credentials = creds_v2

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV2())
        security_module.init_rate_limiter()

        # Should now allow many more requests
        for _ in range(50):
            is_valid, _ = verify_credential_key(f"Bearer {raw_key}")
            assert is_valid is True

    def test_hot_reload_removes_credential(self, monkeypatch):
        """After reload, removed credential is rejected with 401"""
        from app.core import security as security_module

        raw_key_a = "sk-key-keep"
        raw_key_b = "sk-key-drop"

        creds_v1 = [
            CredentialConfig(
                credential_key=hash_key(raw_key_a),
                name="key-keep",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=10),
            ),
            CredentialConfig(
                credential_key=hash_key(raw_key_b),
                name="key-drop",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=10),
            ),
        ]

        class ConfigV1:
            credentials = creds_v1

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV1())
        security_module.init_rate_limiter()

        # Both work initially
        verify_credential_key(f"Bearer {raw_key_a}")
        verify_credential_key(f"Bearer {raw_key_b}")

        # Hot reload: remove key-b
        creds_v2 = [
            CredentialConfig(
                credential_key=hash_key(raw_key_a),
                name="key-keep",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=10),
            ),
        ]

        class ConfigV2:
            credentials = creds_v2

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV2())
        security_module.init_rate_limiter()

        # key-a still works
        is_valid, _ = verify_credential_key(f"Bearer {raw_key_a}")
        assert is_valid is True

        # key-b should now be rejected (401, not in credentials)
        with pytest.raises(HTTPException) as exc_info:
            verify_credential_key(f"Bearer {raw_key_b}")
        assert exc_info.value.status_code == 401

    def test_hot_reload_adds_rate_limit_to_existing(self, monkeypatch):
        """After reload, credential that gains a rate_limit is enforced"""
        from app.core import security as security_module

        raw_key = "sk-key-gain-limit"

        # Initial: no rate limit
        creds_v1 = [
            CredentialConfig(
                credential_key=hash_key(raw_key),
                name="key-gain",
                rate_limit=None,
            ),
        ]

        class ConfigV1:
            credentials = creds_v1

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV1())
        security_module.init_rate_limiter()

        # Unlimited
        for _ in range(50):
            is_valid, _ = verify_credential_key(f"Bearer {raw_key}")
            assert is_valid is True

        # Hot reload: add strict rate limit
        creds_v2 = [
            CredentialConfig(
                credential_key=hash_key(raw_key),
                name="key-gain",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2),
            ),
        ]

        class ConfigV2:
            credentials = creds_v2

        monkeypatch.setattr(security_module, "get_config", lambda: ConfigV2())
        security_module.init_rate_limiter()

        # Should now be limited
        verify_credential_key(f"Bearer {raw_key}")
        verify_credential_key(f"Bearer {raw_key}")
        with pytest.raises(HTTPException) as exc_info:
            verify_credential_key(f"Bearer {raw_key}")
        assert exc_info.value.status_code == 429
