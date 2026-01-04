"""Configuration models"""

from typing import Dict, Optional
from pydantic import BaseModel, Field


class ProviderConfig(BaseModel):
    """Provider configuration"""

    name: str
    api_base: str
    api_key: str
    weight: int = Field(default=1, ge=1)
    model_mapping: Dict[str, str] = Field(default_factory=dict)


class RateLimitConfig(BaseModel):
    """Rate limiting configuration for a master key"""

    requests_per_second: int = Field(gt=0, description="Maximum requests per second")
    burst_size: int = Field(default=10, gt=0, description="Maximum burst size")


class MasterKeyConfig(BaseModel):
    """Master API key configuration with optional rate limiting"""

    key: str = Field(description="The actual API key")
    name: Optional[str] = Field(
        default=None, description="Human-readable name for the key"
    )
    description: Optional[str] = Field(default=None, description="Optional description")
    rate_limit: Optional[RateLimitConfig] = Field(
        default=None,
        description="Optional rate limiting configuration. If None, no rate limiting is applied.",
    )
    enabled: bool = Field(default=True, description="Whether this key is enabled")
    allowed_models: list[str] = Field(
        default_factory=list,
        description="List of allowed model names. Empty list means all models are allowed.",
    )


class ServerConfig(BaseModel):
    """Server configuration"""

    host: str = "0.0.0.0"
    port: int = 18000


class AppConfig(BaseModel):
    """Application configuration"""

    providers: list[ProviderConfig]
    server: ServerConfig = Field(default_factory=ServerConfig)
    verify_ssl: bool = True
    request_timeout_secs: int = Field(
        default=300,
        gt=0,
        description="Request timeout in seconds for upstream providers",
    )
    master_keys: list[MasterKeyConfig] = Field(
        default_factory=list,
        description="List of master keys with optional rate limiting",
    )
