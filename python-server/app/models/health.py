"""Health check models"""

from enum import Enum
from typing import Optional
from pydantic import BaseModel, Field, ConfigDict


class HealthStatus(str, Enum):
    """Health status enum"""

    HEALTHY = "healthy"
    UNHEALTHY = "unhealthy"
    DISABLED = "disabled"
    UNKNOWN = "unknown"


class ModelHealthStatus(BaseModel):
    """Single model health status"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "model": "gpt-4",
                "status": "healthy",
                "response_time_ms": 1234,
                "error": None,
            }
        }
    )

    model: str = Field(..., description="Model name")
    status: HealthStatus = Field(..., description="Health status")
    response_time_ms: Optional[int] = Field(
        None, description="Response time in milliseconds"
    )
    error: Optional[str] = Field(None, description="Error message if unhealthy")


class ProviderHealthStatus(BaseModel):
    """Provider health status"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "provider_id": 1,
                "provider_key": "openai-primary",
                "status": "healthy",
                "models": [
                    {
                        "model": "gpt-4",
                        "status": "healthy",
                        "response_time_ms": 1234,
                        "error": None,
                    }
                ],
                "avg_response_time_ms": 1234,
                "checked_at": "2024-01-01T00:00:00Z",
            }
        }
    )

    provider_id: int = Field(..., description="Provider ID")
    provider_key: str = Field(..., description="Provider key")
    status: HealthStatus = Field(..., description="Overall provider health status")
    models: list[ModelHealthStatus] = Field(
        ..., description="Health status of each model"
    )
    avg_response_time_ms: Optional[int] = Field(
        None, description="Average response time across all models"
    )
    checked_at: str = Field(
        ..., description="Timestamp when health check was performed"
    )


class HealthCheckRequest(BaseModel):
    """Health check request"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "provider_ids": [1, 2],
                "models": ["gpt-4", "gpt-3.5-turbo"],
                "timeout_secs": 10,
                "max_concurrent": 2,
            }
        }
    )

    provider_ids: Optional[list[int]] = Field(
        None, description="Specific provider IDs to check (empty = all providers)"
    )
    models: Optional[list[str]] = Field(
        None, description="Specific models to test (empty = default test models)"
    )
    timeout_secs: int = Field(
        default=30, ge=1, le=120, description="Timeout for each model test in seconds"
    )
    max_concurrent: int = Field(
        default=2,
        ge=1,
        le=10,
        description="Maximum number of providers to check concurrently (default: 2)",
    )


class HealthCheckResponse(BaseModel):
    """Health check response"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "providers": [
                    {
                        "provider_id": 1,
                        "provider_key": "openai-primary",
                        "status": "healthy",
                        "models": [
                            {
                                "model": "gpt-4",
                                "status": "healthy",
                                "response_time_ms": 1234,
                                "error": None,
                            }
                        ],
                        "avg_response_time_ms": 1234,
                        "checked_at": "2024-01-01T00:00:00Z",
                    }
                ],
                "total_providers": 1,
                "healthy_providers": 1,
                "unhealthy_providers": 0,
            }
        }
    )

    providers: list[ProviderHealthStatus] = Field(
        ..., description="Health status of each provider"
    )
    total_providers: int = Field(..., description="Total number of providers checked")
    healthy_providers: int = Field(..., description="Number of healthy providers")
    unhealthy_providers: int = Field(..., description="Number of unhealthy providers")


class CheckProviderHealthRequest(BaseModel):
    """Request body for checking a single provider's health with concurrent model testing"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "models": ["gpt-4", "gpt-3.5-turbo"],
                "max_concurrent": 2,
                "timeout_secs": 30,
            }
        }
    )

    models: Optional[list[str]] = Field(
        None,
        description="Specific models to test. If empty/null, tests ALL models from provider's model_mapping",
    )
    max_concurrent: int = Field(
        default=2,
        ge=1,
        le=10,
        description="Maximum number of models to test concurrently (default: 2)",
    )
    timeout_secs: int = Field(
        default=30,
        ge=1,
        le=120,
        description="Timeout for each model test in seconds (default: 30)",
    )


class ProviderHealthSummary(BaseModel):
    """Summary statistics for provider health check"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "total_models": 3,
                "healthy_models": 2,
                "unhealthy_models": 1,
            }
        }
    )

    total_models: int = Field(..., description="Total number of models tested")
    healthy_models: int = Field(..., description="Number of healthy models")
    unhealthy_models: int = Field(..., description="Number of unhealthy models")


class CheckProviderHealthResponse(BaseModel):
    """Response for checking a single provider's health with concurrent model testing"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "provider_id": 1,
                "provider_key": "openai-primary",
                "status": "healthy",
                "models": [
                    {
                        "model": "gpt-4",
                        "status": "healthy",
                        "response_time_ms": 1234,
                        "error": None,
                    },
                    {
                        "model": "gpt-3.5-turbo",
                        "status": "unhealthy",
                        "response_time_ms": 5000,
                        "error": "Timeout after 30s",
                    },
                ],
                "summary": {
                    "total_models": 2,
                    "healthy_models": 1,
                    "unhealthy_models": 1,
                },
                "avg_response_time_ms": 3117,
                "checked_at": "2024-01-15T10:30:00Z",
            }
        }
    )

    provider_id: int = Field(..., description="Provider ID")
    provider_key: str = Field(..., description="Provider key identifier")
    status: HealthStatus = Field(
        ..., description="Overall provider status: healthy, unhealthy, disabled, unknown"
    )
    models: list[ModelHealthStatus] = Field(
        ..., description="Health status for each tested model"
    )
    summary: ProviderHealthSummary = Field(..., description="Summary statistics")
    avg_response_time_ms: Optional[int] = Field(
        None, description="Average response time across all models in milliseconds"
    )
    checked_at: str = Field(
        ..., description="ISO 8601 timestamp of when health check was performed"
    )
