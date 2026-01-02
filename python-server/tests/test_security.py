"""Tests for security utilities"""
import pytest

from app.core.security import verify_master_key, init_rate_limiter
from app.core.database import hash_key
from app.models.config import AppConfig, ProviderConfig, MasterKeyConfig, RateLimitConfig


@pytest.mark.unit
class TestVerifyMasterKey:
    """Test master API key verification (database mode with hashed keys)"""
    
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
        """Test verification succeeds with valid master key (hashed)"""
        raw_key = 'secret-key'
        hashed_key = hash_key(raw_key)
        
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
                    key=hashed_key,
                    name='test-key',
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
                )
            ],
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        init_rate_limiter()
        
        is_valid, key_id = verify_master_key(f'Bearer {raw_key}')
        assert is_valid is True
        assert key_id == 'test-key'
    
    def test_verify_with_invalid_master_key(self, monkeypatch, clear_config_cache):
        """Test verification fails with invalid master key"""
        from fastapi import HTTPException
        
        raw_key = 'secret-key'
        hashed_key = hash_key(raw_key)
        
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
                    key=hashed_key,
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
        
        raw_key = 'secret-key'
        hashed_key = hash_key(raw_key)
        
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
                    key=hashed_key,
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
        
        raw_key = 'secret-key'
        hashed_key = hash_key(raw_key)
        
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
                    key=hashed_key,
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
        raw_key_1 = 'key-1'
        raw_key_2 = 'key-2'
        hashed_key_1 = hash_key(raw_key_1)
        hashed_key_2 = hash_key(raw_key_2)
        
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
                    key=hashed_key_1,
                    name='first-key',
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
                ),
                MasterKeyConfig(
                    key=hashed_key_2,
                    name='second-key',
                    rate_limit=RateLimitConfig(requests_per_second=5, burst_size=10)
                )
            ],
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        init_rate_limiter()
        
        is_valid, key_id = verify_master_key(f'Bearer {raw_key_1}')
        assert is_valid is True
        assert key_id == 'first-key'
        
        is_valid, key_id = verify_master_key(f'Bearer {raw_key_2}')
        assert is_valid is True
        assert key_id == 'second-key'


@pytest.mark.unit
class TestSecurityIntegration:
    """Test security integration with API"""
    
    @pytest.mark.asyncio
    async def test_security_with_dependencies(self, monkeypatch, clear_config_cache):
        """Test security works with FastAPI dependencies"""
        from app.api.dependencies import verify_auth
        from fastapi import HTTPException
        
        raw_key = 'secret-key'
        hashed_key = hash_key(raw_key)
        
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
                    key=hashed_key,
                    name='test-key',
                    rate_limit=RateLimitConfig(requests_per_second=10, burst_size=20)
                )
            ],
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        init_rate_limiter()
        
        result = await verify_auth(f'Bearer {raw_key}')
        assert result == 'test-key'
        
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
        
        assert await verify_auth(None) is None
        assert await verify_auth('') is None
        assert await verify_auth('Bearer any-key') is None
        assert await verify_auth('invalid-format') is None