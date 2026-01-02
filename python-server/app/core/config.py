"""Configuration management for database-only mode"""
import os
from dataclasses import dataclass
from functools import lru_cache
from typing import Optional

from dotenv import load_dotenv

from app.models.config import AppConfig, ServerConfig

load_dotenv()


@dataclass
class AppEnvConfig:
    """Application configuration from environment variables"""
    host: str = "0.0.0.0"
    port: int = 18000
    verify_ssl: bool = True
    request_timeout_secs: int = 300
    db_url: Optional[str] = None
    admin_key: Optional[str] = None

    @classmethod
    def from_env(cls) -> "AppEnvConfig":
        """Load configuration from environment variables"""
        return cls(
            host=os.environ.get("HOST", "0.0.0.0"),
            port=int(os.environ.get("PORT", "18000")),
            verify_ssl=os.environ.get("VERIFY_SSL", "true").lower() in ("true", "1", "yes"),
            request_timeout_secs=int(os.environ.get("REQUEST_TIMEOUT_SECS", "300")),
            db_url=os.environ.get("DB_URL"),
            admin_key=os.environ.get("ADMIN_KEY"),
        )


_cached_config: Optional[AppConfig] = None


def set_config(config: AppConfig) -> None:
    """Set the cached configuration (used by database mode)"""
    global _cached_config
    _cached_config = config


def clear_config_cache() -> None:
    """Clear the configuration cache"""
    global _cached_config
    _cached_config = None
    get_config.cache_clear()


@lru_cache
def get_config() -> AppConfig:
    """Get cached configuration instance. Returns empty config if not set."""
    global _cached_config
    if _cached_config is not None:
        return _cached_config
    return AppConfig(
        providers=[],
        master_keys=[],
        server=ServerConfig(),
        verify_ssl=True,
        request_timeout_secs=300,
    )


def get_env_config() -> AppEnvConfig:
    """Get environment configuration"""
    return AppEnvConfig.from_env()