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
    """Rate limiting configuration for a credential"""

    requests_per_second: int = Field(gt=0, description="Maximum requests per second")
    burst_size: int = Field(default=10, gt=0, description="Maximum burst size")


class CredentialConfig(BaseModel):
    """Credential configuration with optional rate limiting"""

    credential_key: str = Field(description="The actual API credential key")
    name: Optional[str] = Field(
        default=None, description="Human-readable name for the credential"
    )
    description: Optional[str] = Field(default=None, description="Optional description")
    rate_limit: Optional[RateLimitConfig] = Field(
        default=None,
        description="Optional rate limiting configuration. If None, no rate limiting is applied.",
    )
    enabled: bool = Field(
        default=True, description="Whether this credential is enabled"
    )
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
    ttft_timeout_secs: Optional[int] = Field(
        default=None,
        ge=0,
        description="Time To First Token timeout in seconds. If None or 0, TTFT timeout is disabled.",
    )
    credentials: list[CredentialConfig] = Field(
        default_factory=list,
        description="List of credentials with optional rate limiting",
    )
