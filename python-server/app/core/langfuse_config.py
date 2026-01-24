"""Langfuse integration configuration.

This module provides configuration for the Langfuse observability integration.
Configuration is loaded from environment variables.
"""

import os
from typing import Optional

from pydantic import BaseModel, Field, model_validator


class LangfuseConfig(BaseModel):
    """Langfuse integration configuration.

    Attributes:
        enabled: Enable Langfuse tracing (default: False)
        public_key: Langfuse public key (required when enabled)
        secret_key: Langfuse secret key (required when enabled)
        host: Langfuse server URL (default: https://cloud.langfuse.com)
        sample_rate: Sampling rate 0.0-1.0 (default: 1.0)
        flush_interval: Flush interval in seconds (default: 5.0)
        debug: Enable debug mode (default: False)
    """

    enabled: bool = Field(default=False, description="Enable Langfuse tracing")
    public_key: Optional[str] = Field(default=None, description="Langfuse public key")
    secret_key: Optional[str] = Field(default=None, description="Langfuse secret key")
    host: str = Field(
        default="https://cloud.langfuse.com", description="Langfuse server URL"
    )
    sample_rate: float = Field(
        default=1.0, ge=0.0, le=1.0, description="Sampling rate (0.0-1.0)"
    )
    flush_interval: float = Field(
        default=5.0, gt=0, description="Flush interval in seconds"
    )
    debug: bool = Field(default=False, description="Enable debug mode")

    @model_validator(mode="after")
    def validate_keys_when_enabled(self) -> "LangfuseConfig":
        """Validate that keys are provided when Langfuse is enabled."""
        if self.enabled:
            if not self.public_key:
                raise ValueError(
                    "LANGFUSE_PUBLIC_KEY is required when LANGFUSE_ENABLED=true"
                )
            if not self.secret_key:
                raise ValueError(
                    "LANGFUSE_SECRET_KEY is required when LANGFUSE_ENABLED=true"
                )
        return self

    @classmethod
    def from_env(cls) -> "LangfuseConfig":
        """Load configuration from environment variables.

        Environment variables:
            LANGFUSE_ENABLED: Enable Langfuse tracing (default: false)
            LANGFUSE_PUBLIC_KEY: Langfuse public key
            LANGFUSE_SECRET_KEY: Langfuse secret key
            LANGFUSE_HOST: Langfuse server URL (default: https://cloud.langfuse.com)
            LANGFUSE_SAMPLE_RATE: Sampling rate 0.0-1.0 (default: 1.0)
            LANGFUSE_FLUSH_INTERVAL: Flush interval in seconds (default: 5.0)
            LANGFUSE_DEBUG: Enable debug mode (default: false)

        Returns:
            LangfuseConfig instance
        """
        return cls(
            enabled=os.getenv("LANGFUSE_ENABLED", "false").lower() == "true",
            public_key=os.getenv("LANGFUSE_PUBLIC_KEY"),
            secret_key=os.getenv("LANGFUSE_SECRET_KEY"),
            host=os.getenv("LANGFUSE_HOST", "https://cloud.langfuse.com"),
            sample_rate=float(os.getenv("LANGFUSE_SAMPLE_RATE", "1.0")),
            flush_interval=float(os.getenv("LANGFUSE_FLUSH_INTERVAL", "5.0")),
            debug=os.getenv("LANGFUSE_DEBUG", "false").lower() == "true",
        )


# Global configuration instance
_langfuse_config: Optional[LangfuseConfig] = None


def get_langfuse_config() -> LangfuseConfig:
    """Get the global Langfuse configuration.

    Returns:
        LangfuseConfig instance loaded from environment variables
    """
    global _langfuse_config
    if _langfuse_config is None:
        _langfuse_config = LangfuseConfig.from_env()
    return _langfuse_config


def reset_langfuse_config() -> None:
    """Reset the global Langfuse configuration (for testing)."""
    global _langfuse_config
    _langfuse_config = None
