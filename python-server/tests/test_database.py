"""Tests for database module"""
import pytest
from unittest.mock import patch

from app.core.database import (
    DatabaseConfig,
    Database,
    hash_key,
    create_key_preview,
    InitResult,
)


class TestDatabaseConfig:
    """Tests for DatabaseConfig"""

    def test_convert_postgresql_url(self):
        """Test converting postgresql:// URL"""
        config = DatabaseConfig(url="postgresql://user:pass@localhost/db")
        assert config.url == "postgresql+asyncpg://user:pass@localhost/db"

    def test_convert_postgres_url(self):
        """Test converting postgres:// URL"""
        config = DatabaseConfig(url="postgres://user:pass@localhost/db")
        assert config.url == "postgresql+asyncpg://user:pass@localhost/db"

    def test_already_async_url(self):
        """Test URL already in async format"""
        config = DatabaseConfig(url="postgresql+asyncpg://user:pass@localhost/db")
        assert config.url == "postgresql+asyncpg://user:pass@localhost/db"

    def test_unsupported_url(self):
        """Test unsupported database URL"""
        with pytest.raises(ValueError, match="Unsupported database URL"):
            DatabaseConfig(url="mysql://user:pass@localhost/db")

    def test_no_url_configured(self):
        """Test when no URL is configured"""
        with patch.dict("os.environ", {}, clear=True):
            config = DatabaseConfig(url=None)
            assert config.url is None
            assert not config.is_configured

    def test_url_from_env(self, monkeypatch):
        """Test loading URL from environment variable"""
        monkeypatch.setenv("DB_URL", "postgresql://user:pass@localhost/db")
        config = DatabaseConfig()
        assert config.url == "postgresql+asyncpg://user:pass@localhost/db"
        assert config.is_configured


class TestHashKey:
    """Tests for hash_key function"""

    def test_hash_key_returns_sha256(self):
        """Test that hash_key returns SHA-256 hash"""
        result = hash_key("test-key")
        assert len(result) == 64
        assert all(c in "0123456789abcdef" for c in result)

    def test_hash_key_consistent(self):
        """Test that hash_key is consistent"""
        assert hash_key("test") == hash_key("test")

    def test_hash_key_different_inputs(self):
        """Test that different inputs produce different hashes"""
        assert hash_key("key1") != hash_key("key2")


class TestCreateKeyPreview:
    """Tests for create_key_preview function"""

    def test_normal_key(self):
        """Test preview for normal length key"""
        result = create_key_preview("sk-1234567890abcdef")
        assert result == "sk-***def"

    def test_short_key(self):
        """Test preview for short key"""
        result = create_key_preview("short")
        assert result == "***"

    def test_very_short_key(self):
        """Test preview for very short key"""
        result = create_key_preview("abc")
        assert result == "***"


class TestInitResult:
    """Tests for InitResult dataclass"""

    def test_default_values(self):
        """Test default values"""
        result = InitResult()
        assert result.providers == []
        assert result.master_keys == []
        assert result.version == 0

    def test_with_values(self):
        """Test with provided values"""
        providers = [{"id": "test"}]
        master_keys = [{"id": "key1"}]
        result = InitResult(
            providers=providers,
            master_keys=master_keys,
            version=5,
        )
        assert result.providers == providers
        assert result.master_keys == master_keys
        assert result.version == 5


class TestDatabase:
    """Tests for Database class"""

    def test_init_without_config(self):
        """Test initialization without config"""
        db = Database()
        assert db.config is not None

    def test_init_with_config(self):
        """Test initialization with config"""
        config = DatabaseConfig(url="postgresql://localhost/test")
        db = Database(config)
        assert db.config == config

    @pytest.mark.asyncio
    async def test_connect_without_url(self):
        """Test connect raises error without URL"""
        with patch.dict("os.environ", {}, clear=True):
            config = DatabaseConfig(url=None)
            db = Database(config)
            with pytest.raises(RuntimeError, match="Database URL not configured"):
                await db.connect()

    @pytest.mark.asyncio
    async def test_session_without_connect(self):
        """Test session raises error without connection"""
        config = DatabaseConfig(url="postgresql://localhost/test")
        db = Database(config)
        with pytest.raises(RuntimeError, match="Database not connected"):
            async with db.session():
                pass