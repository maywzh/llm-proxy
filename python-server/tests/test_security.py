"""Tests for security utilities"""
import pytest

from app.core.security import verify_master_key, init_rate_limiter
from app.models.config import AppConfig, ProviderConfig, MasterKeyConfig, RateLimitConfig


@pytest.mark.unit
class TestVerifyMasterKey:
    """Test master API key verification with new master_keys system"""
    
    def test_verify_when_no_master_keys_configured(self, monkeypatch, clear_config_cache):
        """Test verification succeeds when no master keys are configured"""
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
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        # Should allow any request when no master keys are set
        is_valid, key_id = verify_master_key(None)
        assert is_valid is True
        assert key_id is None
        
        is_valid, key_id = verify_master_key('Bearer any-key')
        assert is_valid is True
        assert key_id is None
        
        is_valid, key_id = verify_master_key('invalid')
        assert is_valid is True
        assert key_id is None
    
    def test_verify_with_valid_master_key(self, monkeypatch, clear_config_cache):
        """Test verification succeeds with valid master key"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            master_keys=[
                MasterKeyConfig(
                    key='secret-key',
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
                )
            ],
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        init_rate_limiter()
        
        is_valid, key_id = verify_master_key('Bearer secret-key')
        assert is_valid is True
        assert key_id == 'secret-key'
    
    def test_verify_with_invalid_master_key(self, monkeypatch, clear_config_cache):
        """Test verification fails with invalid master key"""
        from fastapi import HTTPException
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            master_keys=[
                MasterKeyConfig(
                    key='secret-key',
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
                )
            ],
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        init_rate_limiter()
        
        with pytest.raises(HTTPException) as exc_info:
            verify_master_key('Bearer wrong-key')
        assert exc_info.value.status_code == 401
    
    def test_verify_without_bearer_prefix(self, monkeypatch, clear_config_cache):
        """Test verification fails without Bearer prefix"""
        from fastapi import HTTPException
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            master_keys=[
                MasterKeyConfig(
                    key='secret-key',
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
                )
            ],
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        init_rate_limiter()
        
        with pytest.raises(HTTPException) as exc_info:
            verify_master_key('secret-key')
        assert exc_info.value.status_code == 401
    
    def test_verify_with_none_authorization(self, monkeypatch, clear_config_cache):
        """Test verification fails with None authorization when keys are configured"""
        from fastapi import HTTPException
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            master_keys=[
                MasterKeyConfig(
                    key='secret-key',
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
                )
            ],
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        init_rate_limiter()
        
        with pytest.raises(HTTPException) as exc_info:
            verify_master_key(None)
        assert exc_info.value.status_code == 401
    
    def test_verify_with_multiple_master_keys(self, monkeypatch, clear_config_cache):
        """Test verification with multiple master keys"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            master_keys=[
                MasterKeyConfig(
                    key='key-1',
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
                ),
                MasterKeyConfig(
                    key='key-2',
                    rate_limit=RateLimitConfig(requests_per_second=5, burst_size=10)
                )
            ],
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        init_rate_limiter()
        
        # Both keys should work
        is_valid, key_id = verify_master_key('Bearer key-1')
        assert is_valid is True
        assert key_id == 'key-1'
        
        is_valid, key_id = verify_master_key('Bearer key-2')
        assert is_valid is True
        assert key_id == 'key-2'


@pytest.mark.unit
class TestSecurityIntegration:
    """Test security integration with API"""
    
    @pytest.mark.asyncio
    async def test_security_with_dependencies(self, monkeypatch, clear_config_cache):
        """Test security works with FastAPI dependencies"""
        from app.api.dependencies import verify_auth
        from fastapi import HTTPException
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            master_keys=[
                MasterKeyConfig(
                    key='secret-key',
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
                )
            ],
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        init_rate_limiter()
        
        # Test with correct key
        result = await verify_auth('Bearer secret-key')
        assert result == 'secret-key'  # Should return the key_id
        
        # Test with incorrect key
        with pytest.raises(HTTPException) as exc_info:
            await verify_auth('Bearer wrong-key')
        assert exc_info.value.status_code == 401
    
    @pytest.mark.asyncio
    async def test_security_disabled_allows_all(self, monkeypatch, clear_config_cache):
        """Test that when master keys are not set, all requests are allowed"""
        from app.api.dependencies import verify_auth
        
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
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        # All should succeed and return None when no keys configured
        assert await verify_auth(None) is None
        assert await verify_auth('') is None
        assert await verify_auth('Bearer any-key') is None
        assert await verify_auth('invalid-format') is None