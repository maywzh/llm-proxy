"""Tests for security utilities"""
import pytest

from app.core.security import verify_master_key
from app.models.config import AppConfig, ProviderConfig, ServerConfig


@pytest.mark.unit
class TestVerifyMasterKey:
    """Test master API key verification"""
    
    def test_verify_with_correct_key(self, monkeypatch, clear_config_cache):
        """Test verification succeeds with correct key"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key='secret-master-key'),
            verify_ssl=True
        )
        
        # Patch get_config in the security module where it's imported
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        result = verify_master_key('Bearer secret-master-key')
        assert result is True
    
    def test_verify_with_incorrect_key(self, monkeypatch, clear_config_cache):
        """Test verification fails with incorrect key"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key='secret-master-key'),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        result = verify_master_key('Bearer wrong-key')
        assert result is False
    
    def test_verify_without_bearer_prefix(self, monkeypatch, clear_config_cache):
        """Test verification fails without Bearer prefix"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key='secret-master-key'),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        result = verify_master_key('secret-master-key')
        assert result is False
    
    def test_verify_with_none_authorization(self, monkeypatch, clear_config_cache):
        """Test verification fails with None authorization"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key='secret-master-key'),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        result = verify_master_key(None)
        assert result is False
    
    def test_verify_with_empty_string(self, monkeypatch, clear_config_cache):
        """Test verification fails with empty string"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key='secret-master-key'),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        result = verify_master_key('')
        assert result is False
    
    def test_verify_when_no_master_key_configured(self, monkeypatch, clear_config_cache):
        """Test verification succeeds when no master key is configured"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key=None),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        # Should allow any request when no master key is set
        result = verify_master_key(None)
        assert result is True
        
        result = verify_master_key('Bearer any-key')
        assert result is True
        
        result = verify_master_key('invalid')
        assert result is True
    
    def test_verify_with_bearer_case_sensitive(self, monkeypatch, clear_config_cache):
        """Test that Bearer prefix is case-sensitive"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key='secret-master-key'),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        # 'bearer' (lowercase) should fail
        result = verify_master_key('bearer secret-master-key')
        assert result is False
        
        # 'BEARER' (uppercase) should fail
        result = verify_master_key('BEARER secret-master-key')
        assert result is False
    
    def test_verify_with_extra_spaces(self, monkeypatch, clear_config_cache):
        """Test verification with extra spaces"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key='secret-master-key'),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        # Extra space after Bearer should fail (key won't match)
        result = verify_master_key('Bearer  secret-master-key')
        assert result is False
    
    def test_verify_key_exact_match(self, monkeypatch, clear_config_cache):
        """Test that key must match exactly"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key='secret-master-key'),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        # Partial match should fail
        result = verify_master_key('Bearer secret-master')
        assert result is False
        
        # Extra characters should fail
        result = verify_master_key('Bearer secret-master-key-extra')
        assert result is False


@pytest.mark.unit
class TestSecurityEdgeCases:
    """Test security edge cases"""
    
    def test_verify_with_very_long_key(self, monkeypatch, clear_config_cache):
        """Test verification with very long key"""
        long_key = 'x' * 10000
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key=long_key),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        result = verify_master_key(f'Bearer {long_key}')
        assert result is True
    
    def test_verify_with_special_characters_in_key(self, monkeypatch, clear_config_cache):
        """Test verification with special characters in key"""
        special_key = 'key!@#$%^&*()_+-=[]{}|;:,.<>?'
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key=special_key),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        result = verify_master_key(f'Bearer {special_key}')
        assert result is True
    
    def test_verify_with_unicode_in_key(self, monkeypatch, clear_config_cache):
        """Test verification with unicode characters in key"""
        unicode_key = 'key-中文-🔑'
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key=unicode_key),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        result = verify_master_key(f'Bearer {unicode_key}')
        assert result is True
    
    def test_verify_with_newlines_in_authorization(self, monkeypatch, clear_config_cache):
        """Test verification with newlines in authorization header"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key='secret-key'),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        result = verify_master_key('Bearer secret-key\n')
        assert result is False
    
    def test_verify_empty_master_key(self, monkeypatch, clear_config_cache):
        """Test verification with empty master key configured"""
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key=''),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        # Empty string master key should match empty string after Bearer prefix
        result = verify_master_key('Bearer ')
        assert result is True
        
        # But not match other keys
        result = verify_master_key('Bearer anything')
        assert result is False


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
            server=ServerConfig(master_api_key='secret-key'),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        # Test with correct key
        result = await verify_auth('Bearer secret-key')
        assert result is None  # Should not raise
        
        # Test with incorrect key
        with pytest.raises(HTTPException) as exc_info:
            await verify_auth('Bearer wrong-key')
        assert exc_info.value.status_code == 401
    
    @pytest.mark.asyncio
    async def test_security_disabled_allows_all(self, monkeypatch, clear_config_cache):
        """Test that when master key is not set, all requests are allowed"""
        from app.api.dependencies import verify_auth
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='test',
                    api_base='https://api.test.com',
                    api_key='key'
                )
            ],
            server=ServerConfig(master_api_key=None),
            verify_ssl=True
        )
        
        from app.core import security as security_module
        monkeypatch.setattr(security_module, 'get_config', lambda: config)
        
        # All should succeed
        assert await verify_auth(None) is None
        assert await verify_auth('') is None
        assert await verify_auth('Bearer any-key') is None
        assert await verify_auth('invalid-format') is None