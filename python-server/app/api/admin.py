"""Admin API for dynamic configuration management"""

import os
import secrets
from datetime import datetime
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, Header, status
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials
from loguru import logger
from pydantic import BaseModel, Field, ConfigDict

from app.core.database import (
    Database,
    DynamicConfig,
    get_database,
    get_dynamic_config,
    list_providers,
    get_provider_by_id,
    create_provider,
    update_provider,
    delete_provider,
    list_master_keys,
    get_master_key_by_id,
    create_master_key,
    update_master_key,
    delete_master_key,
    hash_key,
    create_key_preview,
)


router = APIRouter(prefix="/admin/v1", tags=["admin"])
security = HTTPBearer(description="Admin API key for authentication")


ADMIN_KEY = os.environ.get("ADMIN_KEY")


async def verify_admin_key(authorization: Optional[str] = Header(None)) -> None:
    """Verify admin API key"""
    if not ADMIN_KEY:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Admin API not configured. Set ADMIN_KEY environment variable.",
        )

    if not authorization or not authorization.startswith("Bearer "):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Missing or invalid authorization header",
        )

    provided_key = authorization[7:]
    if provided_key != ADMIN_KEY:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid admin key",
        )


def get_db() -> Database:
    """Get database dependency"""
    db = get_database()
    if db is None:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Database not configured. Set DB_URL environment variable.",
        )
    return db


def get_config() -> DynamicConfig:
    """Get dynamic config dependency"""
    config = get_dynamic_config()
    if config is None:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Dynamic config not initialized.",
        )
    return config


class ProviderCreate(BaseModel):
    """Provider creation request"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "id": "openai-primary",
            "provider_type": "openai",
            "api_base": "https://api.openai.com/v1",
            "api_key": "sk-xxx",
            "model_mapping": {"gpt-4": "gpt-4-turbo"}
        }
    })
    
    id: str = Field(..., description="Unique provider ID", examples=["openai-primary"])
    provider_type: str = Field(default="openai", description="Provider type (openai, azure, etc.)", examples=["openai", "azure"])
    api_base: str = Field(..., description="API base URL", examples=["https://api.openai.com/v1"])
    api_key: str = Field(..., description="API key for the provider", examples=["sk-xxx"])
    model_mapping: dict[str, str] = Field(default_factory=dict, description="Model name mapping (source -> target)", examples=[{"gpt-4": "gpt-4-turbo"}])


class ProviderUpdate(BaseModel):
    """Provider update request"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "api_base": "https://api.openai.com/v1",
            "is_enabled": True
        }
    })
    
    provider_type: Optional[str] = Field(None, description="Provider type (openai, azure, etc.)")
    api_base: Optional[str] = Field(None, description="API base URL")
    api_key: Optional[str] = Field(None, description="API key for the provider")
    model_mapping: Optional[dict[str, str]] = Field(None, description="Model name mapping")
    is_enabled: Optional[bool] = Field(None, description="Whether the provider is enabled")


class ProviderResponse(BaseModel):
    """Provider response model"""
    model_config = ConfigDict(from_attributes=True, json_schema_extra={
        "example": {
            "id": "openai-primary",
            "provider_type": "openai",
            "api_base": "https://api.openai.com/v1",
            "model_mapping": {"gpt-4": "gpt-4-turbo"},
            "is_enabled": True
        }
    })
    
    id: str = Field(..., description="Unique provider identifier")
    provider_type: str = Field(..., description="Provider type (openai, azure, etc.)")
    api_base: str = Field(..., description="API base URL")
    model_mapping: dict[str, str] = Field(..., description="Model name mapping")
    is_enabled: bool = Field(..., description="Whether the provider is enabled")


class ProviderListResponse(BaseModel):
    """Provider list response"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "version": 1,
            "providers": [
                {
                    "id": "openai-primary",
                    "provider_type": "openai",
                    "api_base": "https://api.openai.com/v1",
                    "model_mapping": {},
                    "is_enabled": True
                }
            ]
        }
    })
    
    version: int = Field(..., description="Current configuration version")
    providers: list[ProviderResponse] = Field(..., description="List of providers")


class ProviderCreateResponse(BaseModel):
    """Provider creation response"""
    version: int = Field(..., description="Current configuration version")
    provider: ProviderResponse = Field(..., description="Created provider")


class MasterKeyCreate(BaseModel):
    """Master key creation request"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "id": "key-1",
            "key": "sk-my-secret-key",
            "name": "Production Key",
            "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
            "rate_limit": 100
        }
    })
    
    id: str = Field(..., description="Unique key ID", examples=["key-1"])
    key: str = Field(..., description="The actual API key", examples=["sk-my-secret-key"])
    name: str = Field(..., description="Human-readable name", examples=["Production Key"])
    allowed_models: list[str] = Field(default_factory=list, description="Allowed models (empty = all)", examples=[["gpt-4", "gpt-3.5-turbo"]])
    rate_limit: Optional[int] = Field(default=None, description="Rate limit (requests per second)", examples=[100])


class MasterKeyUpdate(BaseModel):
    """Master key update request"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "name": "Updated Key Name",
            "rate_limit": 200,
            "is_enabled": True
        }
    })
    
    name: Optional[str] = Field(None, description="Human-readable name")
    allowed_models: Optional[list[str]] = Field(None, description="Allowed models (empty = all)")
    rate_limit: Optional[int] = Field(None, description="Rate limit (requests per second)")
    is_enabled: Optional[bool] = Field(None, description="Whether the key is enabled")


class MasterKeyResponse(BaseModel):
    """Master key response model"""
    model_config = ConfigDict(from_attributes=True, json_schema_extra={
        "example": {
            "id": "key-1",
            "name": "Production Key",
            "key_preview": "sk-***key",
            "allowed_models": ["gpt-4"],
            "rate_limit": 100,
            "is_enabled": True
        }
    })
    
    id: str = Field(..., description="Unique key identifier")
    name: str = Field(..., description="Human-readable name")
    key_preview: str = Field(..., description="Masked preview of the key")
    allowed_models: list[str] = Field(..., description="Allowed models")
    rate_limit: Optional[int] = Field(None, description="Rate limit (requests per second)")
    is_enabled: bool = Field(..., description="Whether the key is enabled")


class MasterKeyListResponse(BaseModel):
    """Master key list response"""
    version: int = Field(..., description="Current configuration version")
    keys: list[MasterKeyResponse] = Field(..., description="List of master keys")


class MasterKeyCreateResponse(BaseModel):
    """Master key creation response"""
    version: int = Field(..., description="Current configuration version")
    key: MasterKeyResponse = Field(..., description="Created master key")


class MasterKeyRotateResponse(BaseModel):
    """Master key rotation response"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "version": 2,
            "new_key": "sk-new-generated-key",
            "message": "Save this key securely. It will not be shown again."
        }
    })
    
    version: int = Field(..., description="Current configuration version")
    new_key: str = Field(..., description="The new generated key")
    message: str = Field(..., description="Important message about the key")


class ConfigVersionResponse(BaseModel):
    """Config version response"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "version": 1,
            "timestamp": "2024-01-01T00:00:00Z"
        }
    })
    
    version: int = Field(..., description="Current configuration version")
    timestamp: str = Field(..., description="Last update timestamp in ISO format")


class ConfigReloadResponse(BaseModel):
    """Config reload response"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "version": 2,
            "timestamp": "2024-01-01T00:00:00Z",
            "providers_count": 3,
            "master_keys_count": 2
        }
    })
    
    version: int = Field(..., description="New configuration version")
    timestamp: str = Field(..., description="Reload timestamp in ISO format")
    providers_count: int = Field(..., description="Number of active providers")
    master_keys_count: int = Field(..., description="Number of active master keys")


class StatusUpdateResponse(BaseModel):
    """Status update response"""
    version: int = Field(..., description="Current configuration version")
    is_enabled: bool = Field(..., description="New enabled status")


class UpdateResponse(BaseModel):
    """Generic update response"""
    version: int = Field(..., description="Current configuration version")
    status: str = Field(..., description="Update status", examples=["updated"])


class HealthResponse(BaseModel):
    """Admin health check response"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "status": "ok",
            "database_configured": True,
            "admin_key_configured": True,
            "config_loaded": True,
            "config_version": 1
        }
    })
    
    status: str = Field(..., description="Health status")
    database_configured: bool = Field(..., description="Whether database is configured")
    admin_key_configured: bool = Field(..., description="Whether admin key is configured")
    config_loaded: bool = Field(..., description="Whether configuration is loaded")
    config_version: int = Field(..., description="Current configuration version")


class ErrorResponse(BaseModel):
    """Error response model"""
    model_config = ConfigDict(json_schema_extra={
        "example": {
            "detail": "Provider 'openai-primary' not found"
        }
    })
    
    detail: str = Field(..., description="Error message")


@router.get(
    "/providers",
    response_model=ProviderListResponse,
    summary="List all providers",
    description="Get a list of all configured providers with their settings",
    tags=["providers"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_list_providers(
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> ProviderListResponse:
    """List all providers"""
    providers = await list_providers(db)
    return ProviderListResponse(
        version=config.version,
        providers=[
            ProviderResponse(
                id=p.id,
                provider_type=p.provider_type,
                api_base=p.api_base,
                model_mapping=p.get_model_mapping(),
                is_enabled=p.is_enabled,
            )
            for p in providers
        ],
    )


@router.post(
    "/providers",
    response_model=ProviderCreateResponse,
    status_code=status.HTTP_201_CREATED,
    summary="Create a new provider",
    description="Create a new LLM provider configuration",
    tags=["providers"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        409: {"model": ErrorResponse, "description": "Conflict - Provider already exists"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_create_provider(
    provider: ProviderCreate,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> ProviderCreateResponse:
    """Create a new provider"""
    existing = await get_provider_by_id(db, provider.id)
    if existing:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=f"Provider '{provider.id}' already exists",
        )

    new_provider = await create_provider(
        db,
        provider_id=provider.id,
        provider_type=provider.provider_type,
        api_base=provider.api_base,
        api_key=provider.api_key,
        model_mapping=provider.model_mapping,
    )

    await config.reload()
    logger.info(f"Provider created: {provider.id}")

    return ProviderCreateResponse(
        version=config.version,
        provider=ProviderResponse(
            id=new_provider.id,
            provider_type=new_provider.provider_type,
            api_base=new_provider.api_base,
            model_mapping=new_provider.get_model_mapping(),
            is_enabled=new_provider.is_enabled,
        ),
    )


@router.get(
    "/providers/{provider_id}",
    response_model=ProviderResponse,
    summary="Get a provider by ID",
    description="Retrieve a specific provider configuration by its unique identifier",
    tags=["providers"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        404: {"model": ErrorResponse, "description": "Not found - Provider does not exist"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_get_provider(
    provider_id: str,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> ProviderResponse:
    """Get a provider by ID"""
    provider = await get_provider_by_id(db, provider_id)
    if not provider:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Provider '{provider_id}' not found",
        )

    return ProviderResponse(
        id=provider.id,
        provider_type=provider.provider_type,
        api_base=provider.api_base,
        model_mapping=provider.get_model_mapping(),
        is_enabled=provider.is_enabled,
    )


@router.put(
    "/providers/{provider_id}",
    response_model=UpdateResponse,
    summary="Update a provider",
    description="Update an existing provider configuration",
    tags=["providers"],
    responses={
        400: {"model": ErrorResponse, "description": "Bad request - No fields to update"},
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        404: {"model": ErrorResponse, "description": "Not found - Provider does not exist"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_update_provider(
    provider_id: str,
    update_data: ProviderUpdate,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> UpdateResponse:
    """Update a provider"""
    update_dict = {k: v for k, v in update_data.model_dump().items() if v is not None}
    if not update_dict:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="No fields to update",
        )

    updated = await update_provider(db, provider_id, **update_dict)
    if not updated:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Provider '{provider_id}' not found",
        )

    await config.reload()
    logger.info(f"Provider updated: {provider_id}")

    return UpdateResponse(version=config.version, status="updated")


@router.delete(
    "/providers/{provider_id}",
    status_code=status.HTTP_204_NO_CONTENT,
    summary="Delete a provider",
    description="Delete an existing provider configuration",
    tags=["providers"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        404: {"model": ErrorResponse, "description": "Not found - Provider does not exist"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_delete_provider(
    provider_id: str,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> None:
    """Delete a provider"""
    deleted = await delete_provider(db, provider_id)
    if not deleted:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Provider '{provider_id}' not found",
        )

    await config.reload()
    logger.info(f"Provider deleted: {provider_id}")


@router.patch(
    "/providers/{provider_id}/status",
    response_model=StatusUpdateResponse,
    summary="Enable or disable a provider",
    description="Toggle the enabled status of a provider",
    tags=["providers"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        404: {"model": ErrorResponse, "description": "Not found - Provider does not exist"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_set_provider_status(
    provider_id: str,
    enabled: bool,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> StatusUpdateResponse:
    """Enable or disable a provider"""
    updated = await update_provider(db, provider_id, is_enabled=enabled)
    if not updated:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Provider '{provider_id}' not found",
        )

    await config.reload()
    logger.info(f"Provider {provider_id} {'enabled' if enabled else 'disabled'}")

    return StatusUpdateResponse(version=config.version, is_enabled=enabled)


@router.get(
    "/master-keys",
    response_model=MasterKeyListResponse,
    summary="List all master keys",
    description="Get a list of all configured master keys (actual keys are hidden)",
    tags=["master-keys"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_list_master_keys(
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> MasterKeyListResponse:
    """List all master keys (keys are hidden)"""
    keys = await list_master_keys(db)
    return MasterKeyListResponse(
        version=config.version,
        keys=[
            MasterKeyResponse(
                id=k.id,
                name=k.name,
                key_preview=create_key_preview(k.key_hash[:10]),
                allowed_models=k.allowed_models or [],
                rate_limit=k.rate_limit,
                is_enabled=k.is_enabled,
            )
            for k in keys
        ],
    )


@router.post(
    "/master-keys",
    response_model=MasterKeyCreateResponse,
    status_code=status.HTTP_201_CREATED,
    summary="Create a new master key",
    description="Create a new master API key for authentication",
    tags=["master-keys"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        409: {"model": ErrorResponse, "description": "Conflict - Master key already exists"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_create_master_key(
    key_data: MasterKeyCreate,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> MasterKeyCreateResponse:
    """Create a new master key"""
    existing = await get_master_key_by_id(db, key_data.id)
    if existing:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=f"Master key '{key_data.id}' already exists",
        )

    new_key = await create_master_key(
        db,
        key_id=key_data.id,
        key=key_data.key,
        name=key_data.name,
        allowed_models=key_data.allowed_models,
        rate_limit=key_data.rate_limit,
    )

    await config.reload()
    logger.info(f"Master key created: {key_data.id}")

    return MasterKeyCreateResponse(
        version=config.version,
        key=MasterKeyResponse(
            id=new_key.id,
            name=new_key.name,
            key_preview=create_key_preview(key_data.key),
            allowed_models=new_key.allowed_models or [],
            rate_limit=new_key.rate_limit,
            is_enabled=new_key.is_enabled,
        ),
    )


@router.get(
    "/master-keys/{key_id}",
    response_model=MasterKeyResponse,
    summary="Get a master key by ID",
    description="Retrieve a specific master key configuration by its unique identifier",
    tags=["master-keys"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        404: {"model": ErrorResponse, "description": "Not found - Master key does not exist"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_get_master_key(
    key_id: str,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> MasterKeyResponse:
    """Get a master key by ID"""
    key = await get_master_key_by_id(db, key_id)
    if not key:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Master key '{key_id}' not found",
        )

    return MasterKeyResponse(
        id=key.id,
        name=key.name,
        key_preview=create_key_preview(key.key_hash[:10]),
        allowed_models=key.allowed_models or [],
        rate_limit=key.rate_limit,
        is_enabled=key.is_enabled,
    )


@router.put(
    "/master-keys/{key_id}",
    response_model=UpdateResponse,
    summary="Update a master key",
    description="Update an existing master key configuration",
    tags=["master-keys"],
    responses={
        400: {"model": ErrorResponse, "description": "Bad request - No fields to update"},
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        404: {"model": ErrorResponse, "description": "Not found - Master key does not exist"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_update_master_key(
    key_id: str,
    update_data: MasterKeyUpdate,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> UpdateResponse:
    """Update a master key"""
    update_dict = {k: v for k, v in update_data.model_dump().items() if v is not None}
    if not update_dict:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="No fields to update",
        )

    updated = await update_master_key(db, key_id, **update_dict)
    if not updated:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Master key '{key_id}' not found",
        )

    await config.reload()
    logger.info(f"Master key updated: {key_id}")

    return UpdateResponse(version=config.version, status="updated")


@router.delete(
    "/master-keys/{key_id}",
    status_code=status.HTTP_204_NO_CONTENT,
    summary="Delete a master key",
    description="Delete an existing master key",
    tags=["master-keys"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        404: {"model": ErrorResponse, "description": "Not found - Master key does not exist"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_delete_master_key(
    key_id: str,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> None:
    """Delete a master key"""
    deleted = await delete_master_key(db, key_id)
    if not deleted:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Master key '{key_id}' not found",
        )

    await config.reload()
    logger.info(f"Master key deleted: {key_id}")


@router.patch(
    "/master-keys/{key_id}/status",
    response_model=StatusUpdateResponse,
    summary="Enable or disable a master key",
    description="Toggle the enabled status of a master key",
    tags=["master-keys"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        404: {"model": ErrorResponse, "description": "Not found - Master key does not exist"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_set_master_key_status(
    key_id: str,
    enabled: bool,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> StatusUpdateResponse:
    """Enable or disable a master key"""
    updated = await update_master_key(db, key_id, is_enabled=enabled)
    if not updated:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Master key '{key_id}' not found",
        )

    await config.reload()
    logger.info(f"Master key {key_id} {'enabled' if enabled else 'disabled'}")

    return StatusUpdateResponse(version=config.version, is_enabled=enabled)


@router.post(
    "/master-keys/{key_id}/rotate",
    response_model=MasterKeyRotateResponse,
    summary="Rotate a master key",
    description="Generate a new key for an existing master key. The old key will be invalidated.",
    tags=["master-keys"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        404: {"model": ErrorResponse, "description": "Not found - Master key does not exist"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Database or admin key not configured"},
    },
)
async def api_rotate_master_key(
    key_id: str,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> MasterKeyRotateResponse:
    """Rotate a master key (generate new key)"""
    existing = await get_master_key_by_id(db, key_id)
    if not existing:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Master key '{key_id}' not found",
        )

    new_key = f"sk-{secrets.token_urlsafe(32)}"
    await update_master_key(db, key_id, key=new_key)

    await config.reload()
    logger.info(f"Master key rotated: {key_id}")

    return MasterKeyRotateResponse(
        version=config.version,
        new_key=new_key,
        message="Save this key securely. It will not be shown again.",
    )


@router.get(
    "/config/version",
    response_model=ConfigVersionResponse,
    summary="Get configuration version",
    description="Get the current configuration version and timestamp",
    tags=["config"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Dynamic config not initialized"},
    },
)
async def api_get_config_version(
    _: None = Depends(verify_admin_key),
    config: DynamicConfig = Depends(get_config),
) -> ConfigVersionResponse:
    """Get current configuration version"""
    return ConfigVersionResponse(
        version=config.version,
        timestamp=config.config.timestamp.isoformat() if config.config else "",
    )


@router.post(
    "/config/reload",
    response_model=ConfigReloadResponse,
    summary="Reload configuration",
    description="Reload configuration from database and apply changes",
    tags=["config"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized - Invalid or missing admin key"},
        503: {"model": ErrorResponse, "description": "Service unavailable - Dynamic config not initialized"},
    },
)
async def api_reload_config(
    _: None = Depends(verify_admin_key),
    config: DynamicConfig = Depends(get_config),
) -> ConfigReloadResponse:
    """Reload configuration from database"""
    versioned = await config.reload()
    logger.info(f"Configuration reloaded via API, version={versioned.version}")

    return ConfigReloadResponse(
        version=versioned.version,
        timestamp=versioned.timestamp.isoformat(),
        providers_count=len(versioned.providers),
        master_keys_count=len(versioned.master_keys),
    )


@router.get(
    "/health",
    response_model=HealthResponse,
    summary="Admin API health check",
    description="Check the health status of the Admin API (no authentication required)",
    tags=["health"],
)
async def admin_health() -> HealthResponse:
    """Admin API health check (no auth required)"""
    db = get_database()
    config = get_dynamic_config()

    return HealthResponse(
        status="ok",
        database_configured=db is not None,
        admin_key_configured=ADMIN_KEY is not None,
        config_loaded=config is not None and config.config is not None,
        config_version=config.version if config else 0,
    )