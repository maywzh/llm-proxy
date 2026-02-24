"""Configuration models"""

from typing import Any, Dict, Literal, Optional, Union
from pydantic import BaseModel, Field, field_validator


class ModelMappingEntry(BaseModel):
    """Extended model mapping entry with metadata.

    Supports rich model information including token limits, costs, and capabilities.
    """

    mapped_model: str = Field(
        description="The actual model name to use for the provider"
    )

    # Token limits
    max_tokens: Optional[int] = Field(
        default=None, description="Maximum context window (input + output)"
    )
    max_input_tokens: Optional[int] = Field(
        default=None, description="Maximum input tokens"
    )
    max_output_tokens: Optional[int] = Field(
        default=None, description="Maximum output tokens"
    )

    # Cost information (per 1K tokens for readability)
    input_cost_per_1k_tokens: Optional[float] = Field(
        default=None, description="Cost per 1K input tokens in USD"
    )
    output_cost_per_1k_tokens: Optional[float] = Field(
        default=None, description="Cost per 1K output tokens in USD"
    )

    # Model capabilities
    supports_vision: Optional[bool] = Field(
        default=None, description="Whether model supports image input"
    )
    supports_function_calling: Optional[bool] = Field(
        default=None, description="Whether model supports function/tool calling"
    )
    supports_streaming: Optional[bool] = Field(
        default=None, description="Whether model supports streaming responses"
    )
    supports_response_schema: Optional[bool] = Field(
        default=None, description="Whether model supports JSON schema responses"
    )
    supports_reasoning: Optional[bool] = Field(
        default=None, description="Whether model supports extended thinking/reasoning"
    )
    supports_computer_use: Optional[bool] = Field(
        default=None, description="Whether model supports computer use"
    )
    supports_pdf_input: Optional[bool] = Field(
        default=None, description="Whether model supports PDF input"
    )

    # Model mode
    mode: Optional[Literal["chat", "completion", "embedding", "image_generation"]] = (
        Field(default=None, description="Model operation mode")
    )


# Union type for backward compatibility: string or extended entry
ModelMappingValue = Union[str, ModelMappingEntry]


def normalize_model_mapping(
    raw_mapping: Dict[str, Any],
) -> Dict[str, ModelMappingValue]:
    """Normalize model_mapping to support both simple and extended formats.

    Args:
        raw_mapping: Raw mapping dict, can contain strings or dicts

    Returns:
        Normalized mapping with Union[str, ModelMappingEntry] values
    """
    result: Dict[str, ModelMappingValue] = {}
    for key, value in raw_mapping.items():
        if isinstance(value, str):
            result[key] = value
        elif isinstance(value, dict):
            result[key] = ModelMappingEntry(**value)
        elif isinstance(value, ModelMappingEntry):
            result[key] = value
        else:
            raise ValueError(
                f"Invalid model_mapping value type for key '{key}': {type(value)}"
            )
    return result


def get_mapped_model_name(value: ModelMappingValue) -> str:
    """Extract the mapped model name from either format."""
    if isinstance(value, str):
        return value
    return value.mapped_model


class ProviderConfig(BaseModel):
    """Provider configuration"""

    name: str
    api_base: str
    api_key: str
    weight: int = Field(default=1, ge=1)
    model_mapping: Dict[str, ModelMappingValue] = Field(default_factory=dict)
    provider_type: str = Field(
        default="openai",
        description="Provider type (openai, anthropic, gcp-vertex, etc.)",
    )
    provider_params: Dict[str, Any] = Field(
        default_factory=dict,
        description="Provider-specific parameters (e.g., GCP Vertex settings)",
    )
    lua_script: Optional[str] = Field(
        default=None,
        description="Optional Lua script for request/response transformation",
    )

    @property
    def gcp_project(self) -> Optional[str]:
        """Get GCP project ID from provider_params."""
        return self.provider_params.get("gcp_project")

    @property
    def gcp_location(self) -> str:
        """Get GCP location from provider_params, defaults to us-central1."""
        return self.provider_params.get("gcp_location", "us-central1")

    @property
    def gcp_publisher(self) -> str:
        """Get GCP publisher from provider_params, defaults to anthropic."""
        return self.provider_params.get("gcp_publisher", "anthropic")

    @field_validator("model_mapping", mode="before")
    @classmethod
    def validate_model_mapping(cls, v: Any) -> Dict[str, ModelMappingValue]:
        """Validate and normalize model_mapping to support both formats."""
        if not isinstance(v, dict):
            return {}
        return normalize_model_mapping(v)


class RateLimitConfig(BaseModel):
    """Rate limiting configuration for a credential"""

    requests_per_second: int = Field(gt=0, description="Maximum requests per second")
    burst_size: int = Field(default=1, gt=0, description="Maximum burst size")


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
