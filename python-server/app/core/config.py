"""Configuration management for the LLM proxy server.

This module handles configuration loading from environment variables.
Dynamic configuration (providers, credentials) is loaded from the database.
"""

import os
from typing import Optional

from dotenv import load_dotenv

from app.models.config import AppConfig, ServerConfig

load_dotenv()


def _str_to_bool(value: str) -> bool:
    """Convert string to boolean. Accepts: 'true', '1', 'yes', 'on' (case-insensitive)"""
    return value.lower() in ("true", "1", "yes", "on")


class EnvConfig:
    """Server configuration from environment variables.

    Providers and credentials are loaded from the database, not from config files.
    """

    def __init__(self):
        self.host: str = os.environ.get("HOST", "0.0.0.0")
        self.port: int = int(os.environ.get("PORT", "18000"))
        self.verify_ssl: bool = _str_to_bool(os.environ.get("VERIFY_SSL", "true"))
        self.request_timeout_secs: int = int(
            os.environ.get("REQUEST_TIMEOUT_SECS", "300")
        )
        self.db_url: Optional[str] = os.environ.get("DB_URL")
        self.admin_key: Optional[str] = os.environ.get("ADMIN_KEY")
        # Global provider suffix that can be optionally prefixed to model names
        # e.g., if PROVIDER_SUFFIX="Proxy", then "Proxy/gpt-4" and "gpt-4" are equivalent
        self.provider_suffix: Optional[str] = os.environ.get("PROVIDER_SUFFIX")

    @classmethod
    def from_env(cls) -> "EnvConfig":
        """Load configuration from environment variables"""
        return cls()


_cached_config: Optional[AppConfig] = None


def set_config(config: AppConfig) -> None:
    """Set the runtime configuration (called after loading from database)"""
    global _cached_config
    _cached_config = config


def clear_config_cache() -> None:
    """Clear the configuration cache"""
    global _cached_config
    _cached_config = None


def get_config() -> AppConfig:
    """Get current runtime configuration.

    Returns empty config if not yet loaded from database.
    """
    global _cached_config
    if _cached_config is not None:
        return _cached_config
    env = EnvConfig()
    return AppConfig(
        providers=[],
        credentials=[],
        server=ServerConfig(host=env.host, port=env.port),
        verify_ssl=env.verify_ssl,
        request_timeout_secs=env.request_timeout_secs,
    )


def get_env_config() -> EnvConfig:
    """Get environment configuration"""
    return EnvConfig.from_env()
