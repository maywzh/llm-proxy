"""Tests for rate limiting functionality"""
import time
from typing import List

import pytest
from fastapi import HTTPException

from app.models.config import MasterKeyConfig, RateLimitConfig, ServerConfig
from app.core.rate_limiter import RateLimiter
from app.core.security import verify_master_key
from app.core.config import get_config


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
        # Unregistered key should return False
        assert limiter.check_rate_limit("unknown-key") is False


class TestVerifyMasterKey:
    """Test verify_master_key function"""
    
    def test_no_master_keys_configured(self, monkeypatch):
        """Test when no master keys are configured"""
        # Mock config with no master keys
        class MockConfig:
            master_keys = []
        
        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())
        
        # Should allow access without authentication
        is_valid, key_id = verify_master_key(None)
        assert is_valid is True
        assert key_id is None
    
    def test_master_keys_valid(self, monkeypatch):
        """Test new master_keys with valid key"""
        test_master_keys = [
            MasterKeyConfig(
                key="sk-key-1",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
            ),
            MasterKeyConfig(
                key="sk-key-2",
                rate_limit=RateLimitConfig(requests_per_second=5, burst_size=10)
            )
        ]
        
        class MockConfig:
            master_keys = test_master_keys
        
        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())
        
        # Initialize rate limiter
        from app.core.security import init_rate_limiter
        init_rate_limiter()
        
        is_valid, key_id = verify_master_key("Bearer sk-key-1")
        assert is_valid is True
        assert key_id == "sk-key-1"
    
    def test_master_keys_invalid(self, monkeypatch):
        """Test new master_keys with invalid key"""
        test_master_keys = [
            MasterKeyConfig(
                key="sk-key-1",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
            )
        ]
        
        class MockConfig:
            master_keys = test_master_keys
        
        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())
        
        with pytest.raises(HTTPException) as exc_info:
            verify_master_key("Bearer wrong-key")
        
        assert exc_info.value.status_code == 401
    
    def test_master_keys_missing_header(self, monkeypatch):
        """Test new master_keys with missing authorization header"""
        test_master_keys = [
            MasterKeyConfig(
                key="sk-key-1",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
            )
        ]
        
        class MockConfig:
            master_keys = test_master_keys
        
        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())
        
        with pytest.raises(HTTPException) as exc_info:
            verify_master_key(None)
        
        assert exc_info.value.status_code == 401
    
    def test_rate_limit_exceeded(self, monkeypatch):
        """Test rate limit enforcement"""
        test_master_keys = [
            MasterKeyConfig(
                key="sk-key-1",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2)
            )
        ]
        
        class MockConfig:
            master_keys = test_master_keys
        
        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())
        
        # Initialize rate limiter
        from app.core.security import init_rate_limiter
        init_rate_limiter()
        
        # First 2 requests should succeed
        is_valid, key_id = verify_master_key("Bearer sk-key-1")
        assert is_valid is True
        
        is_valid, key_id = verify_master_key("Bearer sk-key-1")
        assert is_valid is True
        
        # Third request should fail with 429
        with pytest.raises(HTTPException) as exc_info:
            verify_master_key("Bearer sk-key-1")
        
        assert exc_info.value.status_code == 429
        assert "Rate limit exceeded" in exc_info.value.detail


class TestRateLimitingIntegration:
    """Integration tests for rate limiting"""
    
    def test_multiple_keys_independent_limits(self, monkeypatch):
        """Test that multiple keys have independent rate limits"""
        test_master_keys = [
            MasterKeyConfig(
                key="sk-fast-key",
                rate_limit=RateLimitConfig(requests_per_second=100, burst_size=100)
            ),
            MasterKeyConfig(
                key="sk-slow-key",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2)
            )
        ]
        
        class MockConfig:
            master_keys = test_master_keys
        
        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())
        
        # Initialize rate limiter
        from app.core.security import init_rate_limiter
        init_rate_limiter()
        
        # Exhaust slow key
        verify_master_key("Bearer sk-slow-key")
        verify_master_key("Bearer sk-slow-key")
        
        with pytest.raises(HTTPException) as exc_info:
            verify_master_key("Bearer sk-slow-key")
        assert exc_info.value.status_code == 429
        
        # Fast key should still work
        is_valid, key_id = verify_master_key("Bearer sk-fast-key")
        assert is_valid is True
        assert key_id == "sk-fast-key"
    
    def test_unlimited_key_no_rate_limit(self, monkeypatch):
        """Test that keys without rate_limit field have unlimited requests"""
        test_master_keys = [
            MasterKeyConfig(
                key="sk-unlimited-key",
                rate_limit=None  # No rate limiting
            ),
            MasterKeyConfig(
                key="sk-limited-key",
                rate_limit=RateLimitConfig(requests_per_second=2, burst_size=2)
            )
        ]
        
        class MockConfig:
            master_keys = test_master_keys
        
        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())
        
        # Initialize rate limiter
        from app.core.security import init_rate_limiter
        init_rate_limiter()
        
        # Unlimited key should allow many requests
        for _ in range(100):
            is_valid, key_id = verify_master_key("Bearer sk-unlimited-key")
            assert is_valid is True
            assert key_id == "sk-unlimited-key"
        
        # Limited key should be rate limited
        verify_master_key("Bearer sk-limited-key")
        verify_master_key("Bearer sk-limited-key")
        
        with pytest.raises(HTTPException) as exc_info:
            verify_master_key("Bearer sk-limited-key")
        assert exc_info.value.status_code == 429
    
    def test_mixed_rate_limits(self, monkeypatch):
        """Test configuration with mixed rate limits (some None, some set)"""
        test_master_keys = [
            MasterKeyConfig(
                key="sk-key-1",
                rate_limit=RateLimitConfig(requests_per_second=10, burst_size=10)
            ),
            MasterKeyConfig(
                key="sk-key-2",
                rate_limit=None  # Unlimited
            ),
            MasterKeyConfig(
                key="sk-key-3",
                rate_limit=RateLimitConfig(requests_per_second=5, burst_size=5)
            ),
            MasterKeyConfig(
                key="sk-key-4",
                rate_limit=None  # Unlimited
            )
        ]
        
        class MockConfig:
            master_keys = test_master_keys
        
        monkeypatch.setattr("app.core.security.get_config", lambda: MockConfig())
        
        # Initialize rate limiter
        from app.core.security import init_rate_limiter
        init_rate_limiter()
        
        # Unlimited keys should work without limits
        for _ in range(50):
            verify_master_key("Bearer sk-key-2")
            verify_master_key("Bearer sk-key-4")
        
        # Limited keys should be rate limited
        for _ in range(10):
            verify_master_key("Bearer sk-key-1")
        
        with pytest.raises(HTTPException) as exc_info:
            verify_master_key("Bearer sk-key-1")
        assert exc_info.value.status_code == 429