"""Admin API for dynamic configuration management"""

import json
import os
from datetime import datetime
from math import ceil
from typing import Any, Optional

from fastapi import APIRouter, Depends, HTTPException, Header, Query, status
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
                "weight": 1,
                "provider_params": {},
            }
        }
    )

    provider_key: str = Field(
        ..., description="Unique provider key identifier", examples=["openai-primary"]
    )
    provider_type: str = Field(
        default="openai",
        description="Provider type (openai, azure, gcp-vertex, etc.)",
        examples=["openai", "azure", "gcp-vertex"],
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
    weight: int = Field(
        default=1,
        ge=1,
        description="Load balancing weight",
        examples=[1],
    )
    provider_params: dict[str, Any] = Field(
        default_factory=dict,
        description="Provider-specific parameters (e.g., GCP Vertex settings)",
        examples=[{"gcp_project": "my-project", "gcp_location": "us-central1"}],
    )
    lua_script: Optional[str] = Field(
        default=None,
        description="Optional Lua script for request/response transformation",
    )


_UNSET = object()


class ProviderUpdate(BaseModel):
    """Provider update request"""

    model_config = ConfigDict(
        json_schema_extra={
            "example": {"api_base": "https://api.openai.com/v1", "is_enabled": True}
        }
    )

    provider_type: Optional[str] = Field(
        None, description="Provider type (openai, azure, gcp-vertex, etc.)"
    )
    api_base: Optional[str] = Field(None, description="API base URL")
    api_key: Optional[str] = Field(None, description="API key for the provider")
    model_mapping: Optional[dict[str, str]] = Field(
        None, description="Model name mapping"
    )
    weight: Optional[int] = Field(None, ge=1, description="Load balancing weight")
    provider_params: Optional[dict[str, Any]] = Field(
        None, description="Provider-specific parameters"
    )
    is_enabled: Optional[bool] = Field(
        None, description="Whether the provider is enabled"
    )
    lua_script: Any = Field(
        default=_UNSET,
        description="Optional Lua script (omit=don't change, null=clear, string=set)",
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
                "weight": 1,
                "provider_params": {},
                "is_enabled": True,
            }
        },
    )

    id: int = Field(..., description="Auto-increment provider ID")
    provider_key: str = Field(..., description="Unique provider key identifier")
    provider_type: str = Field(
        ..., description="Provider type (openai, azure, gcp-vertex, etc.)"
    )
    api_base: str = Field(..., description="API base URL")
    model_mapping: dict[str, str] = Field(..., description="Model name mapping")
    weight: int = Field(default=1, description="Load balancing weight")
    provider_params: dict[str, Any] = Field(
        default_factory=dict, description="Provider-specific parameters"
    )
    is_enabled: bool = Field(..., description="Whether the provider is enabled")
    lua_script: Optional[str] = Field(
        default=None,
        description="Optional Lua script for request/response transformation",
    )


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
                        "weight": 1,
                        "provider_params": {},
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
                weight=p.weight,
                provider_params=p.get_provider_params(),
                is_enabled=p.is_enabled,
                lua_script=p.lua_script,
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
        weight=provider.weight,
        provider_params=provider.provider_params,
        lua_script=provider.lua_script,
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
            weight=new_provider.weight,
            provider_params=new_provider.get_provider_params(),
            is_enabled=new_provider.is_enabled,
            lua_script=new_provider.lua_script,
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
        weight=provider.weight,
        provider_params=provider.get_provider_params(),
        is_enabled=provider.is_enabled,
        lua_script=provider.lua_script,
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

    # Handle lua_script with _UNSET sentinel: omitted=don't change, null=clear, string=set
    if update_data.lua_script is not _UNSET:
        update_dict["lua_script"] = (
            update_data.lua_script
        )  # None (clear) or string (set)
    else:
        update_dict.pop("lua_script", None)

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


class ValidateScriptRequest(BaseModel):
    lua_script: str


class ValidateScriptResponse(BaseModel):
    valid: bool
    error: Optional[str] = None


@router.post(
    "/providers/{provider_id}/validate-script",
    response_model=ValidateScriptResponse,
    summary="Validate a Lua script",
    description="Check whether the given Lua script compiles and defines at least one hook",
    tags=["providers"],
    responses={
        401: {"model": ErrorResponse, "description": "Unauthorized"},
        404: {"model": ErrorResponse, "description": "Provider not found"},
    },
)
async def api_validate_script(
    provider_id: int,
    body: ValidateScriptRequest,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> ValidateScriptResponse:
    """Validate a Lua script for syntax and hook presence"""
    provider = await get_provider_by_id(db, provider_id)
    if not provider:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Provider with ID {provider_id} not found",
        )

    from app.scripting.sandbox import sanitize_lua_error, validate_script

    error = validate_script(body.lua_script)
    if error is None:
        return ValidateScriptResponse(valid=True)
    return ValidateScriptResponse(valid=False, error=sanitize_lua_error(error))


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


# =============================================================================
# Request Logs API
# =============================================================================


def _extract_client_from_headers(headers_json: Optional[str]) -> Optional[str]:
    """Extract normalized client name from request_headers JSON."""
    if not headers_json:
        return None
    try:
        headers = json.loads(headers_json)
    except (json.JSONDecodeError, TypeError):
        return None
    ua = headers.get("user-agent", "") or headers.get("User-Agent", "")
    if not ua:
        return "unknown"
    from app.utils.client import CLIENT_PATTERNS

    for pattern, client_name in CLIENT_PATTERNS:
        if pattern in ua:
            return client_name
    first_token = ua.split(" ")[0].split("/")[0]
    cleaned = "".join(c for c in first_token if c.isalnum() or c in "-_.")[:30]
    return cleaned if cleaned else "other"


class RequestLogItem(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: int
    timestamp: str
    request_id: str
    endpoint: Optional[str] = None
    credential_name: Optional[str] = None
    model_requested: Optional[str] = None
    model_mapped: Optional[str] = None
    provider_name: Optional[str] = None
    provider_type: Optional[str] = None
    client_protocol: Optional[str] = None
    provider_protocol: Optional[str] = None
    is_streaming: Optional[bool] = None
    status_code: Optional[int] = None
    input_tokens: int = 0
    output_tokens: int = 0
    total_tokens: int = 0
    total_duration_ms: Optional[int] = None
    ttft_ms: Optional[int] = None
    error_category: Optional[str] = None
    error_message: Optional[str] = None
    client: Optional[str] = None


class RequestLogDetail(RequestLogItem):
    request_headers: Optional[str] = None
    request_body: Optional[str] = None
    response_body: Optional[str] = None


class RequestLogListResponse(BaseModel):
    items: list[RequestLogItem]
    total: int
    page: int
    page_size: int
    total_pages: int


class RequestLogStatsResponse(BaseModel):
    total_requests: int
    total_errors: int
    error_rate: float
    total_input_tokens: int
    total_output_tokens: int
    avg_duration_ms: Optional[float] = None
    avg_ttft_ms: Optional[float] = None
    requests_by_provider: dict[str, int]
    requests_by_model: dict[str, int]
    requests_by_status: dict[str, int]


def _parse_iso_time(value: Optional[str]) -> Optional[datetime]:
    if not value:
        return None
    try:
        dt = datetime.fromisoformat(value)
        if dt.tzinfo is None:
            from datetime import timezone

            dt = dt.replace(tzinfo=timezone.utc)
        return dt
    except ValueError:
        return None


@router.get("/logs", response_model=RequestLogListResponse, tags=["logs"])
async def api_list_logs(
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    page: int = Query(1, ge=1),
    page_size: int = Query(50, ge=1, le=200),
    request_id: Optional[str] = Query(None),
    provider_name: Optional[str] = Query(None),
    model: Optional[str] = Query(None),
    credential_name: Optional[str] = Query(None),
    status_code: Optional[int] = Query(None),
    is_streaming: Optional[bool] = Query(None),
    error_only: bool = Query(False),
    start_time: Optional[str] = Query(None),
    end_time: Optional[str] = Query(None),
    sort_by: str = Query("timestamp"),
    sort_order: str = Query("desc"),
) -> RequestLogListResponse:
    """List request logs with pagination and filtering."""
    from sqlalchemy import func, or_, select

    from app.core.database import RequestLogModel

    async with db.session() as session:
        # Build base query
        base = select(RequestLogModel)
        count_q = select(func.count(RequestLogModel.id))

        # Apply filters
        conditions = []
        if request_id:
            conditions.append(RequestLogModel.request_id == request_id)
        if provider_name:
            conditions.append(RequestLogModel.provider_name == provider_name)
        if model:
            conditions.append(RequestLogModel.model_requested == model)
        if credential_name:
            conditions.append(RequestLogModel.credential_name == credential_name)
        if status_code is not None:
            conditions.append(RequestLogModel.status_code == status_code)
        if is_streaming is not None:
            conditions.append(RequestLogModel.is_streaming == is_streaming)
        if error_only:
            conditions.append(
                or_(
                    RequestLogModel.status_code >= 400,
                    RequestLogModel.error_category.isnot(None),
                )
            )
        st = _parse_iso_time(start_time)
        if st:
            conditions.append(RequestLogModel.timestamp >= st)
        et = _parse_iso_time(end_time)
        if et:
            conditions.append(RequestLogModel.timestamp <= et)

        for cond in conditions:
            base = base.where(cond)
            count_q = count_q.where(cond)

        # Count total
        total = (await session.execute(count_q)).scalar() or 0
        total_pages = ceil(total / page_size) if total > 0 else 1

        # Sort
        allowed_sort = {
            "timestamp": RequestLogModel.timestamp,
            "status_code": RequestLogModel.status_code,
            "total_duration_ms": RequestLogModel.total_duration_ms,
            "total_tokens": RequestLogModel.total_tokens,
            "input_tokens": RequestLogModel.input_tokens,
            "output_tokens": RequestLogModel.output_tokens,
        }
        sort_col = allowed_sort.get(sort_by, RequestLogModel.timestamp)
        if sort_order == "asc":
            base = base.order_by(sort_col.asc())
        else:
            base = base.order_by(sort_col.desc())

        # Paginate
        base = base.offset((page - 1) * page_size).limit(page_size)
        result = await session.execute(base)
        rows = result.scalars().all()

        items = [
            RequestLogItem(
                id=r.id,
                timestamp=r.timestamp.isoformat(),
                request_id=r.request_id,
                endpoint=r.endpoint,
                credential_name=r.credential_name,
                model_requested=r.model_requested,
                model_mapped=r.model_mapped,
                provider_name=r.provider_name,
                provider_type=r.provider_type,
                client_protocol=r.client_protocol,
                provider_protocol=r.provider_protocol,
                is_streaming=r.is_streaming,
                status_code=r.status_code,
                input_tokens=r.input_tokens,
                output_tokens=r.output_tokens,
                total_tokens=r.total_tokens,
                total_duration_ms=r.total_duration_ms,
                ttft_ms=r.ttft_ms,
                error_category=r.error_category,
                error_message=r.error_message,
                client=_extract_client_from_headers(r.request_headers),
            )
            for r in rows
        ]

    return RequestLogListResponse(
        items=items,
        total=total,
        page=page,
        page_size=page_size,
        total_pages=total_pages,
    )


@router.get("/logs/stats", response_model=RequestLogStatsResponse, tags=["logs"])
async def api_log_stats(
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    start_time: Optional[str] = Query(None),
    end_time: Optional[str] = Query(None),
    provider_name: Optional[str] = Query(None),
    model: Optional[str] = Query(None),
) -> RequestLogStatsResponse:
    """Get aggregated request log statistics."""
    from sqlalchemy import case, func, or_, select

    from app.core.database import RequestLogModel

    async with db.session() as session:
        conditions = []
        st = _parse_iso_time(start_time)
        if st:
            conditions.append(RequestLogModel.timestamp >= st)
        et = _parse_iso_time(end_time)
        if et:
            conditions.append(RequestLogModel.timestamp <= et)
        if provider_name:
            conditions.append(RequestLogModel.provider_name == provider_name)
        if model:
            conditions.append(RequestLogModel.model_requested == model)

        # Aggregated stats
        agg = select(
            func.count(RequestLogModel.id).label("total"),
            func.sum(
                case(
                    (
                        or_(
                            RequestLogModel.status_code >= 400,
                            RequestLogModel.error_category.isnot(None),
                        ),
                        1,
                    ),
                    else_=0,
                )
            ).label("errors"),
            func.coalesce(func.sum(RequestLogModel.input_tokens), 0).label(
                "input_tokens"
            ),
            func.coalesce(func.sum(RequestLogModel.output_tokens), 0).label(
                "output_tokens"
            ),
            func.avg(RequestLogModel.total_duration_ms).label("avg_duration"),
            func.avg(RequestLogModel.ttft_ms).label("avg_ttft"),
        )
        for cond in conditions:
            agg = agg.where(cond)
        row = (await session.execute(agg)).one()
        total_requests = row.total or 0
        total_errors = row.errors or 0
        error_rate = (total_errors / total_requests) if total_requests > 0 else 0.0

        # Group by provider
        by_provider_q = (
            select(
                RequestLogModel.provider_name,
                func.count(RequestLogModel.id),
            )
            .where(RequestLogModel.provider_name.isnot(None))
            .group_by(RequestLogModel.provider_name)
        )
        for cond in conditions:
            by_provider_q = by_provider_q.where(cond)
        by_provider_rows = (await session.execute(by_provider_q)).all()
        requests_by_provider = {r[0]: r[1] for r in by_provider_rows}

        # Group by model
        by_model_q = (
            select(
                RequestLogModel.model_requested,
                func.count(RequestLogModel.id),
            )
            .where(RequestLogModel.model_requested.isnot(None))
            .group_by(RequestLogModel.model_requested)
        )
        for cond in conditions:
            by_model_q = by_model_q.where(cond)
        by_model_rows = (await session.execute(by_model_q)).all()
        requests_by_model = {r[0]: r[1] for r in by_model_rows}

        # Group by status code
        by_status_q = (
            select(
                RequestLogModel.status_code,
                func.count(RequestLogModel.id),
            )
            .where(RequestLogModel.status_code.isnot(None))
            .group_by(RequestLogModel.status_code)
        )
        for cond in conditions:
            by_status_q = by_status_q.where(cond)
        by_status_rows = (await session.execute(by_status_q)).all()
        requests_by_status = {str(r[0]): r[1] for r in by_status_rows}

    return RequestLogStatsResponse(
        total_requests=total_requests,
        total_errors=total_errors,
        error_rate=round(error_rate, 4),
        total_input_tokens=row.input_tokens,
        total_output_tokens=row.output_tokens,
        avg_duration_ms=(
            round(float(row.avg_duration), 1) if row.avg_duration else None
        ),
        avg_ttft_ms=(round(float(row.avg_ttft), 1) if row.avg_ttft else None),
        requests_by_provider=requests_by_provider,
        requests_by_model=requests_by_model,
        requests_by_status=requests_by_status,
    )


@router.get("/logs/{log_id}", response_model=RequestLogDetail, tags=["logs"])
async def api_get_log(
    log_id: int,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> RequestLogDetail:
    """Get a single request log entry with full details."""
    from sqlalchemy import select

    from app.core.database import RequestLogModel

    async with db.session() as session:
        result = await session.execute(
            select(RequestLogModel).where(RequestLogModel.id == log_id)
        )
        r = result.scalar_one_or_none()
        if r is None:
            raise HTTPException(status_code=404, detail="Log entry not found")

    return RequestLogDetail(
        id=r.id,
        timestamp=r.timestamp.isoformat(),
        request_id=r.request_id,
        endpoint=r.endpoint,
        credential_name=r.credential_name,
        model_requested=r.model_requested,
        model_mapped=r.model_mapped,
        provider_name=r.provider_name,
        provider_type=r.provider_type,
        client_protocol=r.client_protocol,
        provider_protocol=r.provider_protocol,
        is_streaming=r.is_streaming,
        status_code=r.status_code,
        input_tokens=r.input_tokens,
        output_tokens=r.output_tokens,
        total_tokens=r.total_tokens,
        total_duration_ms=r.total_duration_ms,
        ttft_ms=r.ttft_ms,
        error_category=r.error_category,
        error_message=r.error_message,
        client=_extract_client_from_headers(r.request_headers),
        request_headers=r.request_headers,
        request_body=r.request_body,
        response_body=r.response_body,
    )


# =============================================================================
# Error Logs API
# =============================================================================


class ErrorLogItem(BaseModel):
    model_config = ConfigDict(from_attributes=True)

    id: int
    timestamp: str
    request_id: Optional[str] = None
    error_category: str
    error_code: Optional[int] = None
    error_message: Optional[str] = None
    provider_name: Optional[str] = None
    credential_name: Optional[str] = None
    model_requested: Optional[str] = None
    model_mapped: Optional[str] = None
    endpoint: Optional[str] = None
    client_protocol: Optional[str] = None
    provider_protocol: Optional[str] = None
    is_streaming: Optional[bool] = None
    total_duration_ms: Optional[int] = None


class ErrorLogDetail(ErrorLogItem):
    request_body: Optional[str] = None
    response_body: Optional[str] = None
    provider_request_body: Optional[str] = None
    provider_request_headers: Optional[str] = None


class ErrorLogListResponse(BaseModel):
    items: list[ErrorLogItem]
    total: int
    page: int
    page_size: int
    total_pages: int


@router.get("/error-logs", response_model=ErrorLogListResponse, tags=["logs"])
async def api_list_error_logs(
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
    page: int = Query(1, ge=1),
    page_size: int = Query(50, ge=1, le=200),
    request_id: Optional[str] = Query(None),
    provider_name: Optional[str] = Query(None),
    error_category: Optional[str] = Query(None),
    start_time: Optional[str] = Query(None),
    end_time: Optional[str] = Query(None),
    sort_by: str = Query("timestamp"),
    sort_order: str = Query("desc"),
) -> ErrorLogListResponse:
    """List error logs with pagination and filtering."""
    from sqlalchemy import func, select

    from app.core.database import ErrorLogModel

    async with db.session() as session:
        base = select(ErrorLogModel)
        count_q = select(func.count(ErrorLogModel.id))

        conditions = []
        if request_id:
            conditions.append(ErrorLogModel.request_id == request_id)
        if provider_name:
            conditions.append(ErrorLogModel.provider_name == provider_name)
        if error_category:
            conditions.append(ErrorLogModel.error_category == error_category)
        st = _parse_iso_time(start_time)
        if st:
            conditions.append(ErrorLogModel.timestamp >= st)
        et = _parse_iso_time(end_time)
        if et:
            conditions.append(ErrorLogModel.timestamp <= et)

        for cond in conditions:
            base = base.where(cond)
            count_q = count_q.where(cond)

        total = (await session.execute(count_q)).scalar() or 0
        total_pages = ceil(total / page_size) if total > 0 else 1

        allowed_sort = {
            "timestamp": ErrorLogModel.timestamp,
            "error_category": ErrorLogModel.error_category,
            "total_duration_ms": ErrorLogModel.total_duration_ms,
        }
        sort_col = allowed_sort.get(sort_by, ErrorLogModel.timestamp)
        if sort_order == "asc":
            base = base.order_by(sort_col.asc())
        else:
            base = base.order_by(sort_col.desc())

        base = base.offset((page - 1) * page_size).limit(page_size)
        result = await session.execute(base)
        rows = result.scalars().all()

        items = [
            ErrorLogItem(
                id=r.id,
                timestamp=r.timestamp.isoformat(),
                request_id=r.request_id,
                error_category=r.error_category,
                error_code=r.error_code,
                error_message=r.error_message,
                provider_name=r.provider_name,
                credential_name=r.credential_name,
                model_requested=None,
                model_mapped=r.mapped_model,
                endpoint=r.endpoint,
                client_protocol=r.client_protocol,
                provider_protocol=r.provider_protocol,
                is_streaming=r.is_streaming,
                total_duration_ms=r.total_duration_ms,
            )
            for r in rows
        ]

    return ErrorLogListResponse(
        items=items,
        total=total,
        page=page,
        page_size=page_size,
        total_pages=total_pages,
    )


@router.get("/error-logs/{log_id}", response_model=ErrorLogDetail, tags=["logs"])
async def api_get_error_log(
    log_id: int,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> ErrorLogDetail:
    """Get a single error log entry with full details."""
    from sqlalchemy import select

    from app.core.database import ErrorLogModel

    async with db.session() as session:
        result = await session.execute(
            select(ErrorLogModel).where(ErrorLogModel.id == log_id)
        )
        r = result.scalar_one_or_none()
        if r is None:
            raise HTTPException(status_code=404, detail="Error log entry not found")

    return ErrorLogDetail(
        id=r.id,
        timestamp=r.timestamp.isoformat(),
        request_id=r.request_id,
        error_category=r.error_category,
        error_code=r.error_code,
        error_message=r.error_message,
        provider_name=r.provider_name,
        credential_name=r.credential_name,
        model_requested=None,
        model_mapped=r.mapped_model,
        endpoint=r.endpoint,
        client_protocol=r.client_protocol,
        provider_protocol=r.provider_protocol,
        is_streaming=r.is_streaming,
        total_duration_ms=r.total_duration_ms,
        request_body=r.request_body,
        response_body=r.response_body,
        provider_request_body=r.provider_request_body,
        provider_request_headers=r.provider_request_headers,
    )


# =============================================================================
# Log Deletion API
# =============================================================================


class BatchDeleteRequest(BaseModel):
    ids: list[int]


class BatchDeleteResponse(BaseModel):
    deleted: int


@router.delete(
    "/logs/{log_id}",
    status_code=status.HTTP_204_NO_CONTENT,
    tags=["logs"],
)
async def api_delete_log(
    log_id: int,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> None:
    """Delete a single request log entry."""
    from sqlalchemy import delete

    from app.core.database import RequestLogModel

    async with db.session() as session:
        result = await session.execute(
            delete(RequestLogModel).where(RequestLogModel.id == log_id)
        )
        if result.rowcount == 0:  # type: ignore[union-attr]
            raise HTTPException(status_code=404, detail="Log entry not found")
    logger.info(f"Request log deleted: {log_id}")


@router.post(
    "/logs/batch-delete",
    response_model=BatchDeleteResponse,
    tags=["logs"],
)
async def api_batch_delete_logs(
    body: BatchDeleteRequest,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> BatchDeleteResponse:
    """Batch delete request log entries."""
    if not body.ids:
        return BatchDeleteResponse(deleted=0)
    if len(body.ids) > 1000:
        raise HTTPException(
            status_code=400, detail="Cannot delete more than 1000 records at once"
        )

    from sqlalchemy import delete

    from app.core.database import RequestLogModel

    async with db.session() as session:
        result = await session.execute(
            delete(RequestLogModel).where(RequestLogModel.id.in_(body.ids))
        )
        deleted = result.rowcount or 0  # type: ignore[union-attr]
    logger.info(f"Request logs batch deleted: {deleted} records")
    return BatchDeleteResponse(deleted=deleted)


@router.delete(
    "/error-logs/{log_id}",
    status_code=status.HTTP_204_NO_CONTENT,
    tags=["logs"],
)
async def api_delete_error_log(
    log_id: int,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> None:
    """Delete a single error log entry."""
    from sqlalchemy import delete

    from app.core.database import ErrorLogModel

    async with db.session() as session:
        result = await session.execute(
            delete(ErrorLogModel).where(ErrorLogModel.id == log_id)
        )
        if result.rowcount == 0:  # type: ignore[union-attr]
            raise HTTPException(status_code=404, detail="Error log entry not found")
    logger.info(f"Error log deleted: {log_id}")


@router.post(
    "/error-logs/batch-delete",
    response_model=BatchDeleteResponse,
    tags=["logs"],
)
async def api_batch_delete_error_logs(
    body: BatchDeleteRequest,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> BatchDeleteResponse:
    """Batch delete error log entries."""
    if not body.ids:
        return BatchDeleteResponse(deleted=0)
    if len(body.ids) > 1000:
        raise HTTPException(
            status_code=400, detail="Cannot delete more than 1000 records at once"
        )

    from sqlalchemy import delete

    from app.core.database import ErrorLogModel

    async with db.session() as session:
        result = await session.execute(
            delete(ErrorLogModel).where(ErrorLogModel.id.in_(body.ids))
        )
        deleted = result.rowcount or 0  # type: ignore[union-attr]
    logger.info(f"Error logs batch deleted: {deleted} records")
    return BatchDeleteResponse(deleted=deleted)
