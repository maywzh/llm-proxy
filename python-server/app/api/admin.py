"""Admin API for dynamic configuration management"""

import os
from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, Header, status
from fastapi.security import HTTPBearer
from loguru import logger
from pydantic import BaseModel, Field, ConfigDict

from app.core.database import (
    Database,
    DynamicConfig,
    get_database,
    get_dynamic_config,
    list_providers,
    get_provider_by_id,
    get_provider_by_key,
    create_provider,
    update_provider,
    delete_provider,
    list_credentials,
    get_credential_by_id,
    create_credential,
    update_credential,
    delete_credential,
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

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "provider_key": "openai-primary",
                "provider_type": "openai",
                "api_base": "https://api.openai.com/v1",
                "api_key": "sk-xxx",
                "model_mapping": {"gpt-4": "gpt-4-turbo"},
            }
        }
    )

    provider_key: str = Field(
        ..., description="Unique provider key identifier", examples=["openai-primary"]
    )
    provider_type: str = Field(
        default="openai",
        description="Provider type (openai, azure, etc.)",
        examples=["openai", "azure"],
    )
    api_base: str = Field(
        ..., description="API base URL", examples=["https://api.openai.com/v1"]
    )
    api_key: str = Field(
        ..., description="API key for the provider", examples=["sk-xxx"]
    )
    model_mapping: dict[str, str] = Field(
        default_factory=dict,
        description="Model name mapping (source -> target)",
        examples=[{"gpt-4": "gpt-4-turbo"}],
    )


class ProviderUpdate(BaseModel):
    """Provider update request"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {"api_base": "https://api.openai.com/v1", "is_enabled": True}
        }
    )

    provider_type: Optional[str] = Field(
        None, description="Provider type (openai, azure, etc.)"
    )
    api_base: Optional[str] = Field(None, description="API base URL")
    api_key: Optional[str] = Field(None, description="API key for the provider")
    model_mapping: Optional[dict[str, str]] = Field(
        None, description="Model name mapping"
    )
    is_enabled: Optional[bool] = Field(
        None, description="Whether the provider is enabled"
    )


class ProviderResponse(BaseModel):
    """Provider response model"""

    model_config = ConfigDict(
        from_attributes=True,
        json_schema_extra={
            "example": {
                "id": 1,
                "provider_key": "openai-primary",
                "provider_type": "openai",
                "api_base": "https://api.openai.com/v1",
                "model_mapping": {"gpt-4": "gpt-4-turbo"},
                "is_enabled": True,
            }
        },
    )

    id: int = Field(..., description="Auto-increment provider ID")
    provider_key: str = Field(..., description="Unique provider key identifier")
    provider_type: str = Field(..., description="Provider type (openai, azure, etc.)")
    api_base: str = Field(..., description="API base URL")
    model_mapping: dict[str, str] = Field(..., description="Model name mapping")
    is_enabled: bool = Field(..., description="Whether the provider is enabled")


class ProviderListResponse(BaseModel):
    """Provider list response"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "version": 1,
                "providers": [
                    {
                        "id": 1,
                        "provider_key": "openai-primary",
                        "provider_type": "openai",
                        "api_base": "https://api.openai.com/v1",
                        "model_mapping": {},
                        "is_enabled": True,
                    }
                ],
            }
        }
    )

    version: int = Field(..., description="Current configuration version")
    providers: list[ProviderResponse] = Field(..., description="List of providers")


class ProviderCreateResponse(BaseModel):
    """Provider creation response"""

    version: int = Field(..., description="Current configuration version")
    provider: ProviderResponse = Field(..., description="Created provider")


class CredentialCreate(BaseModel):
    """Credential creation request"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "key": "sk-my-secret-key",
                "name": "Production Key",
                "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
                "rate_limit": 100,
            }
        }
    )

    key: str = Field(
        ..., description="The actual API key", examples=["sk-my-secret-key"]
    )
    name: str = Field(
        ..., description="Human-readable name", examples=["Production Key"]
    )
    allowed_models: list[str] = Field(
        default_factory=list,
        description="Allowed models (empty = all)",
        examples=[["gpt-4", "gpt-3.5-turbo"]],
    )
    rate_limit: Optional[int] = Field(
        default=None, description="Rate limit (requests per second)", examples=[100]
    )


class CredentialUpdate(BaseModel):
    """Credential update request"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "name": "Updated Key Name",
                "rate_limit": 200,
                "is_enabled": True,
            }
        }
    )

    name: Optional[str] = Field(None, description="Human-readable name")
    allowed_models: Optional[list[str]] = Field(
        None, description="Allowed models (empty = all)"
    )
    rate_limit: Optional[int] = Field(
        None, description="Rate limit (requests per second)"
    )
    is_enabled: Optional[bool] = Field(
        None, description="Whether the credential is enabled"
    )


class CredentialResponse(BaseModel):
    """Credential response model"""

    model_config = ConfigDict(
        from_attributes=True,
        json_schema_extra={
            "example": {
                "id": 1,
                "name": "Production Key",
                "key_preview": "sk-***key",
                "allowed_models": ["gpt-4"],
                "rate_limit": 100,
                "is_enabled": True,
            }
        },
    )

    id: int = Field(..., description="Auto-increment credential ID")
    name: str = Field(..., description="Human-readable name")
    key_preview: str = Field(..., description="Masked preview of the key")
    allowed_models: list[str] = Field(..., description="Allowed models")
    rate_limit: Optional[int] = Field(
        None, description="Rate limit (requests per second)"
    )
    is_enabled: bool = Field(..., description="Whether the credential is enabled")


class CredentialListResponse(BaseModel):
    """Credential list response"""

    version: int = Field(..., description="Current configuration version")
    credentials: list[CredentialResponse] = Field(
        ..., description="List of credentials"
    )


class CredentialCreateResponse(BaseModel):
    """Credential creation response"""

    version: int = Field(..., description="Current configuration version")
    credential: CredentialResponse = Field(..., description="Created credential")


class ConfigVersionResponse(BaseModel):
    """Config version response"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {"version": 1, "timestamp": "2024-01-01T00:00:00Z"}
        }
    )

    version: int = Field(..., description="Current configuration version")
    timestamp: str = Field(..., description="Last update timestamp in ISO format")


class ConfigReloadResponse(BaseModel):
    """Config reload response"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {
                "version": 2,
                "timestamp": "2024-01-01T00:00:00Z",
                "providers_count": 3,
                "credentials_count": 2,
            }
        }
    )

    version: int = Field(..., description="New configuration version")
    timestamp: str = Field(..., description="Reload timestamp in ISO format")
    providers_count: int = Field(..., description="Number of active providers")
    credentials_count: int = Field(..., description="Number of active credentials")


class UpdateResponse(BaseModel):
    """Generic update response"""

    version: int = Field(..., description="Current configuration version")
    status: str = Field(..., description="Update status", examples=["updated"])


class ErrorResponse(BaseModel):
    """Error response model"""

    model_config = ConfigDict(
        json_schema_extra={"example": {"detail": "Provider 'openai-primary' not found"}}
    )

    detail: str = Field(..., description="Error message")


class AuthValidateResponse(BaseModel):
    """Auth validation response"""

    model_config = ConfigDict(
        json_schema_extra={"example": {"valid": True, "message": "Admin key is valid"}}
    )

    valid: bool = Field(..., description="Whether the admin key is valid")
    message: str = Field(..., description="Validation message")


@router.post(
    "/auth/validate",
    response_model=AuthValidateResponse,
    summary="Validate admin key",
    description="Validate the admin API key for UI login. Returns success if the key is valid.",
    tags=["auth"],
    responses={
        200: {"model": AuthValidateResponse, "description": "Admin key is valid"},
        401: {"model": AuthValidateResponse, "description": "Invalid admin key"},
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Admin key not configured",
        },
    },
)
async def api_validate_admin_key(
    authorization: Optional[str] = Header(None),
) -> AuthValidateResponse:
    """Validate admin API key for UI login"""
    if not ADMIN_KEY:
        raise HTTPException(
            status_code=status.HTTP_503_SERVICE_UNAVAILABLE,
            detail="Admin API not configured. Set ADMIN_KEY environment variable.",
        )

    if not authorization or not authorization.startswith("Bearer "):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail=AuthValidateResponse(
                valid=False, message="Invalid admin key"
            ).model_dump(),
        )

    provided_key = authorization[7:]
    if provided_key != ADMIN_KEY:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail=AuthValidateResponse(
                valid=False, message="Invalid admin key"
            ).model_dump(),
        )

    return AuthValidateResponse(valid=True, message="Admin key is valid")


@router.get(
    "/providers",
    response_model=ProviderListResponse,
    summary="List all providers",
    description="Get a list of all configured providers with their settings",
    tags=["providers"],
    responses={
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
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
                provider_key=p.provider_key,
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
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        409: {
            "model": ErrorResponse,
            "description": "Conflict - Provider already exists",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
    },
)
async def api_create_provider(
    provider: ProviderCreate,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> ProviderCreateResponse:
    """Create a new provider"""
    existing = await get_provider_by_key(db, provider.provider_key)
    if existing:
        raise HTTPException(
            status_code=status.HTTP_409_CONFLICT,
            detail=f"Provider with key '{provider.provider_key}' already exists",
        )

    new_provider = await create_provider(
        db,
        provider_key=provider.provider_key,
        provider_type=provider.provider_type,
        api_base=provider.api_base,
        api_key=provider.api_key,
        model_mapping=provider.model_mapping,
    )

    await config.reload()
    logger.info(f"Provider created: {new_provider.id} ({provider.provider_key})")

    return ProviderCreateResponse(
        version=config.version,
        provider=ProviderResponse(
            id=new_provider.id,
            provider_key=new_provider.provider_key,
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
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        404: {
            "model": ErrorResponse,
            "description": "Not found - Provider does not exist",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
    },
)
async def api_get_provider(
    provider_id: int,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> ProviderResponse:
    """Get a provider by ID"""
    provider = await get_provider_by_id(db, provider_id)
    if not provider:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Provider with ID {provider_id} not found",
        )

    return ProviderResponse(
        id=provider.id,
        provider_key=provider.provider_key,
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
        400: {
            "model": ErrorResponse,
            "description": "Bad request - No fields to update",
        },
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        404: {
            "model": ErrorResponse,
            "description": "Not found - Provider does not exist",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
    },
)
async def api_update_provider(
    provider_id: int,
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
            detail=f"Provider with ID {provider_id} not found",
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
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        404: {
            "model": ErrorResponse,
            "description": "Not found - Provider does not exist",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
    },
)
async def api_delete_provider(
    provider_id: int,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> None:
    """Delete a provider"""
    deleted = await delete_provider(db, provider_id)
    if not deleted:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Provider with ID {provider_id} not found",
        )

    await config.reload()
    logger.info(f"Provider deleted: {provider_id}")


@router.get(
    "/credentials",
    response_model=CredentialListResponse,
    summary="List all credentials",
    description="Get a list of all configured credentials (actual keys are hidden)",
    tags=["credentials"],
    responses={
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
    },
)
async def api_list_credentials(
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> CredentialListResponse:
    """List all credentials (keys are hidden)"""
    creds = await list_credentials(db)
    return CredentialListResponse(
        version=config.version,
        credentials=[
            CredentialResponse(
                id=c.id,
                name=c.name,
                key_preview=create_key_preview(c.credential_key[:10]),
                allowed_models=c.allowed_models or [],
                rate_limit=c.rate_limit,
                is_enabled=c.is_enabled,
            )
            for c in creds
        ],
    )


@router.post(
    "/credentials",
    response_model=CredentialCreateResponse,
    status_code=status.HTTP_201_CREATED,
    summary="Create a new credential",
    description="Create a new credential for API authentication",
    tags=["credentials"],
    responses={
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
    },
)
async def api_create_credential(
    cred_data: CredentialCreate,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> CredentialCreateResponse:
    """Create a new credential"""
    new_cred = await create_credential(
        db,
        key=cred_data.key,
        name=cred_data.name,
        allowed_models=cred_data.allowed_models,
        rate_limit=cred_data.rate_limit,
    )

    await config.reload()
    logger.info(f"Credential created: {new_cred.id}")

    return CredentialCreateResponse(
        version=config.version,
        credential=CredentialResponse(
            id=new_cred.id,
            name=new_cred.name,
            key_preview=create_key_preview(cred_data.key),
            allowed_models=new_cred.allowed_models or [],
            rate_limit=new_cred.rate_limit,
            is_enabled=new_cred.is_enabled,
        ),
    )


@router.get(
    "/credentials/{credential_id}",
    response_model=CredentialResponse,
    summary="Get a credential by ID",
    description="Retrieve a specific credential configuration by its unique identifier",
    tags=["credentials"],
    responses={
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        404: {
            "model": ErrorResponse,
            "description": "Not found - Credential does not exist",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
    },
)
async def api_get_credential(
    credential_id: int,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> CredentialResponse:
    """Get a credential by ID"""
    cred = await get_credential_by_id(db, credential_id)
    if not cred:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Credential with ID {credential_id} not found",
        )

    return CredentialResponse(
        id=cred.id,
        name=cred.name,
        key_preview=create_key_preview(cred.credential_key[:10]),
        allowed_models=cred.allowed_models or [],
        rate_limit=cred.rate_limit,
        is_enabled=cred.is_enabled,
    )


@router.put(
    "/credentials/{credential_id}",
    response_model=UpdateResponse,
    summary="Update a credential",
    description="Update an existing credential configuration",
    tags=["credentials"],
    responses={
        400: {
            "model": ErrorResponse,
            "description": "Bad request - No fields to update",
        },
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        404: {
            "model": ErrorResponse,
            "description": "Not found - Credential does not exist",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
    },
)
async def api_update_credential(
    credential_id: int,
    update_data: CredentialUpdate,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> UpdateResponse:
    """Update a credential"""
    update_dict = {k: v for k, v in update_data.model_dump().items() if v is not None}
    if not update_dict:
        raise HTTPException(
            status_code=status.HTTP_400_BAD_REQUEST,
            detail="No fields to update",
        )

    updated = await update_credential(db, credential_id, **update_dict)
    if not updated:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Credential with ID {credential_id} not found",
        )

    await config.reload()
    logger.info(f"Credential updated: {credential_id}")

    return UpdateResponse(version=config.version, status="updated")


@router.delete(
    "/credentials/{credential_id}",
    status_code=status.HTTP_204_NO_CONTENT,
    summary="Delete a credential",
    description="Delete an existing credential",
    tags=["credentials"],
    responses={
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        404: {
            "model": ErrorResponse,
            "description": "Not found - Credential does not exist",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database or admin key not configured",
        },
    },
)
async def api_delete_credential(
    credential_id: int,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    config: DynamicConfig = Depends(get_config),
) -> None:
    """Delete a credential"""
    deleted = await delete_credential(db, credential_id)
    if not deleted:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Credential with ID {credential_id} not found",
        )

    await config.reload()
    logger.info(f"Credential deleted: {credential_id}")


@router.get(
    "/config/version",
    response_model=ConfigVersionResponse,
    summary="Get configuration version",
    description="Get the current configuration version and timestamp",
    tags=["config"],
    responses={
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Dynamic config not initialized",
        },
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
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Dynamic config not initialized",
        },
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
        credentials_count=len(versioned.credentials),
    )
