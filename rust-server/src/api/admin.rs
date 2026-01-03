//! Admin API handlers for dynamic configuration management.
//!
//! Provides RESTful endpoints for managing providers and master keys.
//! All endpoints require ADMIN_KEY authentication.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{OpenApi, ToSchema};

use crate::core::database::{
    create_key_preview, CreateMasterKey, CreateProvider, DynamicConfig,
    MasterKeyEntity, ProviderEntity, UpdateMasterKey, UpdateProvider,
};

/// OpenAPI documentation for Admin API
#[derive(OpenApi)]
#[openapi(
    paths(
        validate_admin_key,
        list_providers,
        create_provider,
        get_provider,
        update_provider,
        delete_provider,
        list_master_keys,
        create_master_key,
        get_master_key,
        update_master_key,
        delete_master_key,
        get_config_version,
        reload_config,
    ),
    components(
        schemas(
            AuthValidateResponse,
            ProviderListResponse,
            ProviderResponse,
            CreateProviderRequest,
            UpdateProviderRequest,
            MasterKeyListResponse,
            MasterKeyResponse,
            CreateMasterKeyRequest,
            UpdateMasterKeyRequest,
            ConfigVersionResponse,
            AdminErrorResponse,
        )
    ),
    tags(
        (name = "auth", description = "Authentication endpoints"),
        (name = "providers", description = "Provider management endpoints"),
        (name = "master-keys", description = "Master key management endpoints"),
        (name = "config", description = "Configuration management endpoints")
    ),
    info(
        title = "LLM Proxy Admin API",
        version = "1.0.0",
        description = "Admin API for managing LLM Proxy configuration including providers and master keys.",
        license(name = "MIT")
    ),
    servers(
        (url = "http://127.0.0.1:17999", description = "Local development server"),
        (url = "http://localhost:17999", description = "Local development server (localhost)")
    ),
    security(
        ("bearer_auth" = [])
    ),
    modifiers(&SecurityAddon)
)]
pub struct AdminApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearer_auth",
                utoipa::openapi::security::SecurityScheme::Http(
                    utoipa::openapi::security::Http::new(
                        utoipa::openapi::security::HttpAuthScheme::Bearer,
                    ),
                ),
            );
        }
    }
}

/// Admin API state
pub struct AdminState {
    pub dynamic_config: Arc<DynamicConfig>,
    pub admin_key: String,
}

/// Verify admin authentication
fn verify_admin_auth(headers: &HeaderMap, admin_key: &str) -> Result<(), AdminError> {
    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or(AdminError::Unauthorized)?;

    if !auth_header.starts_with("Bearer ") {
        return Err(AdminError::Unauthorized);
    }

    let provided_key = &auth_header[7..];
    if provided_key != admin_key {
        return Err(AdminError::Unauthorized);
    }

    Ok(())
}

/// Admin API error types
#[derive(Debug)]
pub enum AdminError {
    Unauthorized,
    NotFound(String),
    BadRequest(String),
    Internal(String),
    Database(sqlx::Error),
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AdminError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            AdminError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AdminError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AdminError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
            AdminError::Database(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Database error: {}", e),
            ),
        };

        let body = serde_json::json!({
            "error": {
                "message": message,
                "code": status.as_u16()
            }
        });

        (status, Json(body)).into_response()
    }
}

impl From<sqlx::Error> for AdminError {
    fn from(e: sqlx::Error) -> Self {
        AdminError::Database(e)
    }
}

// ============================================================================
// Provider API Types
// ============================================================================

/// Response containing list of providers
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "version": 1,
    "providers": [{
        "id": "openai-1",
        "provider_type": "openai",
        "api_base": "https://api.openai.com/v1",
        "model_mapping": {"gpt-4": "gpt-4-turbo"},
        "is_enabled": true,
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z"
    }]
}))]
pub struct ProviderListResponse {
    /// Current configuration version
    pub version: i64,
    /// List of providers
    pub providers: Vec<ProviderResponse>,
}

/// Provider response
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "id": "openai-1",
    "provider_type": "openai",
    "api_base": "https://api.openai.com/v1",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "is_enabled": true,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z"
}))]
pub struct ProviderResponse {
    /// Unique provider identifier
    pub id: String,
    /// Provider type (e.g., "openai", "azure", "anthropic")
    pub provider_type: String,
    /// Base URL for the provider API
    pub api_base: String,
    /// Model name mapping (request model -> provider model)
    pub model_mapping: std::collections::HashMap<String, String>,
    /// Whether this provider is enabled
    pub is_enabled: bool,
    /// Creation timestamp (RFC 3339 format)
    pub created_at: String,
    /// Last update timestamp (RFC 3339 format)
    pub updated_at: String,
}

impl From<ProviderEntity> for ProviderResponse {
    fn from(e: ProviderEntity) -> Self {
        Self {
            id: e.id,
            provider_type: e.provider_type,
            api_base: e.api_base,
            model_mapping: e.model_mapping.0,
            is_enabled: e.is_enabled,
            created_at: e.created_at.to_rfc3339(),
            updated_at: e.updated_at.to_rfc3339(),
        }
    }
}

/// Request to create a new provider
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "id": "openai-1",
    "provider_type": "openai",
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-your-api-key",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "is_enabled": true
}))]
pub struct CreateProviderRequest {
    /// Unique provider identifier
    pub id: String,
    /// Provider type (e.g., "openai", "azure", "anthropic")
    pub provider_type: String,
    /// Base URL for the provider API
    pub api_base: String,
    /// API key for authentication
    pub api_key: String,
    /// Model name mapping (request model -> provider model)
    #[serde(default)]
    pub model_mapping: std::collections::HashMap<String, String>,
    /// Whether this provider is enabled (default: true)
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}

/// Request to update an existing provider
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "api_base": "https://api.openai.com/v1",
    "is_enabled": false
}))]
pub struct UpdateProviderRequest {
    /// Provider type (e.g., "openai", "azure", "anthropic")
    pub provider_type: Option<String>,
    /// Base URL for the provider API
    pub api_base: Option<String>,
    /// API key for authentication
    pub api_key: Option<String>,
    /// Model name mapping (request model -> provider model)
    pub model_mapping: Option<std::collections::HashMap<String, String>>,
    /// Whether this provider is enabled
    pub is_enabled: Option<bool>,
}

// ============================================================================
// Master Key API Types
// ============================================================================

/// Response containing list of master keys
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "version": 1,
    "keys": [{
        "id": "key-1",
        "name": "Production Key",
        "key_preview": "sk-***abc",
        "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
        "rate_limit": 100,
        "is_enabled": true,
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z"
    }]
}))]
pub struct MasterKeyListResponse {
    /// Current configuration version
    pub version: i64,
    /// List of master keys
    pub keys: Vec<MasterKeyResponse>,
}

/// Master key response
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "id": "key-1",
    "name": "Production Key",
    "key_preview": "sk-***abc",
    "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
    "rate_limit": 100,
    "is_enabled": true,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z"
}))]
pub struct MasterKeyResponse {
    /// Unique key identifier
    pub id: String,
    /// Human-readable name for the key
    pub name: String,
    /// Masked preview of the key (e.g., "sk-***abc")
    pub key_preview: String,
    /// List of models this key can access (empty = all models)
    pub allowed_models: Vec<String>,
    /// Rate limit in requests per second (null = unlimited)
    pub rate_limit: Option<i32>,
    /// Whether this key is enabled
    pub is_enabled: bool,
    /// Creation timestamp (RFC 3339 format)
    pub created_at: String,
    /// Last update timestamp (RFC 3339 format)
    pub updated_at: String,
}

impl MasterKeyResponse {
    fn from_entity(e: MasterKeyEntity, preview: String) -> Self {
        Self {
            id: e.id,
            name: e.name,
            key_preview: preview,
            allowed_models: e.allowed_models,
            rate_limit: e.rate_limit,
            is_enabled: e.is_enabled,
            created_at: e.created_at.to_rfc3339(),
            updated_at: e.updated_at.to_rfc3339(),
        }
    }
}

/// Request to create a new master key
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "id": "key-1",
    "key": "sk-your-master-key",
    "name": "Production Key",
    "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
    "rate_limit": 100,
    "is_enabled": true
}))]
pub struct CreateMasterKeyRequest {
    /// Unique key identifier
    pub id: String,
    /// The actual key value (will be hashed for storage)
    pub key: String,
    /// Human-readable name for the key
    pub name: String,
    /// List of models this key can access (empty = all models)
    #[serde(default)]
    pub allowed_models: Vec<String>,
    /// Rate limit in requests per second (null = unlimited)
    pub rate_limit: Option<i32>,
    /// Whether this key is enabled (default: true)
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}

/// Request to update an existing master key
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "name": "Updated Key Name",
    "rate_limit": 200,
    "is_enabled": false
}))]
pub struct UpdateMasterKeyRequest {
    /// New key value (will be hashed for storage)
    pub key: Option<String>,
    /// Human-readable name for the key
    pub name: Option<String>,
    /// List of models this key can access (empty = all models)
    pub allowed_models: Option<Vec<String>>,
    /// Rate limit in requests per second (null = unlimited)
    pub rate_limit: Option<i32>,
    /// Whether this key is enabled
    pub is_enabled: Option<bool>,
}

// ============================================================================
// Config API Types
// ============================================================================

/// Response containing configuration version
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "version": 1,
    "timestamp": "2024-01-01T00:00:00Z"
}))]
pub struct ConfigVersionResponse {
    /// Current configuration version
    pub version: i64,
    /// Timestamp when configuration was loaded (RFC 3339 format)
    pub timestamp: String,
}

/// Admin API error response
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "error": {
        "message": "Unauthorized",
        "code": 401
    }
}))]
pub struct AdminErrorResponse {
    /// Error details
    pub error: AdminErrorDetail,
}

/// Admin API error detail
#[derive(Debug, Serialize, ToSchema)]
pub struct AdminErrorDetail {
    /// Error message
    pub message: String,
    /// HTTP status code
    pub code: u16,
}

/// Auth validation response
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "valid": true,
    "message": "Admin key is valid"
}))]
pub struct AuthValidateResponse {
    /// Whether the admin key is valid
    pub valid: bool,
    /// Validation message
    pub message: String,
}

fn default_true() -> bool {
    true
}

// ============================================================================
// Provider Handlers
// ============================================================================

/// List all providers
///
/// Returns a list of all configured providers with their current configuration version.
#[utoipa::path(
    get,
    path = "/admin/v1/providers",
    tag = "providers",
    responses(
        (status = 200, description = "List of providers", body = ProviderListResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse)
    )
)]
pub async fn list_providers(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
) -> Result<Json<ProviderListResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let providers = db.load_all_providers().await?;
    let version = db.get_config_version().await?;

    Ok(Json(ProviderListResponse {
        version,
        providers: providers.into_iter().map(|p| p.into()).collect(),
    }))
}

/// Get a single provider
///
/// Returns the configuration for a specific provider by ID.
#[utoipa::path(
    get,
    path = "/admin/v1/providers/{id}",
    tag = "providers",
    params(
        ("id" = String, Path, description = "Provider ID")
    ),
    responses(
        (status = 200, description = "Provider details", body = ProviderResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 404, description = "Provider not found", body = AdminErrorResponse)
    )
)]
pub async fn get_provider(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<ProviderResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let provider = db
        .get_provider(&id)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Provider '{}' not found", id)))?;

    Ok(Json(provider.into()))
}

/// Create a new provider
///
/// Creates a new provider with the specified configuration.
#[utoipa::path(
    post,
    path = "/admin/v1/providers",
    tag = "providers",
    request_body = CreateProviderRequest,
    responses(
        (status = 201, description = "Provider created", body = ProviderResponse),
        (status = 400, description = "Bad request", body = AdminErrorResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse)
    )
)]
pub async fn create_provider(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Json(req): Json<CreateProviderRequest>,
) -> Result<(StatusCode, Json<ProviderResponse>), AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    if req.id.is_empty() {
        return Err(AdminError::BadRequest("Provider ID is required".to_string()));
    }
    if req.api_base.is_empty() {
        return Err(AdminError::BadRequest("API base is required".to_string()));
    }
    if req.api_key.is_empty() {
        return Err(AdminError::BadRequest("API key is required".to_string()));
    }

    let db = state.dynamic_config.database();

    if db.get_provider(&req.id).await?.is_some() {
        return Err(AdminError::BadRequest(format!(
            "Provider '{}' already exists",
            req.id
        )));
    }

    let create = CreateProvider {
        id: req.id,
        provider_type: req.provider_type,
        api_base: req.api_base,
        api_key: req.api_key,
        model_mapping: req.model_mapping,
        is_enabled: req.is_enabled,
    };

    let provider = db.create_provider(&create).await?;
    tracing::info!(provider_id = %provider.id, "Provider created");

    Ok((StatusCode::CREATED, Json(provider.into())))
}

/// Update an existing provider
///
/// Updates the configuration for an existing provider. Only provided fields will be updated.
#[utoipa::path(
    put,
    path = "/admin/v1/providers/{id}",
    tag = "providers",
    params(
        ("id" = String, Path, description = "Provider ID")
    ),
    request_body = UpdateProviderRequest,
    responses(
        (status = 200, description = "Provider updated", body = ProviderResponse),
        (status = 400, description = "Bad request", body = AdminErrorResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 404, description = "Provider not found", body = AdminErrorResponse)
    )
)]
pub async fn update_provider(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateProviderRequest>,
) -> Result<Json<ProviderResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();

    let update = UpdateProvider {
        provider_type: req.provider_type,
        api_base: req.api_base,
        api_key: req.api_key,
        model_mapping: req.model_mapping,
        is_enabled: req.is_enabled,
    };

    let provider = db
        .update_provider(&id, &update)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Provider '{}' not found", id)))?;

    tracing::info!(provider_id = %id, "Provider updated");

    Ok(Json(provider.into()))
}

/// Delete a provider
///
/// Permanently deletes a provider from the configuration.
#[utoipa::path(
    delete,
    path = "/admin/v1/providers/{id}",
    tag = "providers",
    params(
        ("id" = String, Path, description = "Provider ID")
    ),
    responses(
        (status = 204, description = "Provider deleted"),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 404, description = "Provider not found", body = AdminErrorResponse)
    )
)]
pub async fn delete_provider(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let deleted = db.delete_provider(&id).await?;

    if !deleted {
        return Err(AdminError::NotFound(format!(
            "Provider '{}' not found",
            id
        )));
    }

    tracing::info!(provider_id = %id, "Provider deleted");

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Master Key Handlers
// ============================================================================

/// List all master keys
///
/// Returns a list of all configured master keys with their current configuration version.
/// Key values are masked for security.
#[utoipa::path(
    get,
    path = "/admin/v1/master-keys",
    tag = "master-keys",
    responses(
        (status = 200, description = "List of master keys", body = MasterKeyListResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse)
    )
)]
pub async fn list_master_keys(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
) -> Result<Json<MasterKeyListResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let keys = db.load_all_master_keys().await?;
    let version = db.get_config_version().await?;

    let responses: Vec<MasterKeyResponse> = keys
        .into_iter()
        .map(|k| {
            let preview = format!("***{}", &k.key_hash[..6]);
            MasterKeyResponse::from_entity(k, preview)
        })
        .collect();

    Ok(Json(MasterKeyListResponse {
        version,
        keys: responses,
    }))
}

/// Get a single master key
///
/// Returns the configuration for a specific master key by ID.
/// The key value is masked for security.
#[utoipa::path(
    get,
    path = "/admin/v1/master-keys/{id}",
    tag = "master-keys",
    params(
        ("id" = String, Path, description = "Master key ID")
    ),
    responses(
        (status = 200, description = "Master key details", body = MasterKeyResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 404, description = "Master key not found", body = AdminErrorResponse)
    )
)]
pub async fn get_master_key(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<MasterKeyResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let key = db
        .get_master_key(&id)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Master key '{}' not found", id)))?;

    let preview = format!("***{}", &key.key_hash[..6]);
    Ok(Json(MasterKeyResponse::from_entity(key, preview)))
}

/// Create a new master key
///
/// Creates a new master key with the specified configuration.
/// The key value will be hashed for secure storage.
#[utoipa::path(
    post,
    path = "/admin/v1/master-keys",
    tag = "master-keys",
    request_body = CreateMasterKeyRequest,
    responses(
        (status = 201, description = "Master key created", body = MasterKeyResponse),
        (status = 400, description = "Bad request", body = AdminErrorResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse)
    )
)]
pub async fn create_master_key(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Json(req): Json<CreateMasterKeyRequest>,
) -> Result<(StatusCode, Json<MasterKeyResponse>), AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    if req.id.is_empty() {
        return Err(AdminError::BadRequest("Key ID is required".to_string()));
    }
    if req.key.is_empty() {
        return Err(AdminError::BadRequest("Key value is required".to_string()));
    }
    if req.name.is_empty() {
        return Err(AdminError::BadRequest("Key name is required".to_string()));
    }

    let db = state.dynamic_config.database();

    if db.get_master_key(&req.id).await?.is_some() {
        return Err(AdminError::BadRequest(format!(
            "Master key '{}' already exists",
            req.id
        )));
    }

    let key_preview = create_key_preview(&req.key);

    let create = CreateMasterKey {
        id: req.id,
        key: req.key,
        name: req.name,
        allowed_models: req.allowed_models,
        rate_limit: req.rate_limit,
        is_enabled: req.is_enabled,
    };

    let key = db.create_master_key(&create).await?;
    tracing::info!(key_id = %key.id, key_name = %key.name, "Master key created");

    Ok((
        StatusCode::CREATED,
        Json(MasterKeyResponse::from_entity(key, key_preview)),
    ))
}

/// Update an existing master key
///
/// Updates the configuration for an existing master key. Only provided fields will be updated.
/// If a new key value is provided, it will be hashed for secure storage.
#[utoipa::path(
    put,
    path = "/admin/v1/master-keys/{id}",
    tag = "master-keys",
    params(
        ("id" = String, Path, description = "Master key ID")
    ),
    request_body = UpdateMasterKeyRequest,
    responses(
        (status = 200, description = "Master key updated", body = MasterKeyResponse),
        (status = 400, description = "Bad request", body = AdminErrorResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 404, description = "Master key not found", body = AdminErrorResponse)
    )
)]
pub async fn update_master_key(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateMasterKeyRequest>,
) -> Result<Json<MasterKeyResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();

    let update = UpdateMasterKey {
        key: req.key.clone(),
        name: req.name,
        allowed_models: req.allowed_models,
        rate_limit: req.rate_limit,
        is_enabled: req.is_enabled,
    };

    let key = db
        .update_master_key(&id, &update)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Master key '{}' not found", id)))?;

    let preview = if let Some(new_key) = req.key {
        create_key_preview(&new_key)
    } else {
        format!("***{}", &key.key_hash[..6])
    };

    tracing::info!(key_id = %id, "Master key updated");

    Ok(Json(MasterKeyResponse::from_entity(key, preview)))
}

/// Delete a master key
///
/// Permanently deletes a master key from the configuration.
#[utoipa::path(
    delete,
    path = "/admin/v1/master-keys/{id}",
    tag = "master-keys",
    params(
        ("id" = String, Path, description = "Master key ID")
    ),
    responses(
        (status = 204, description = "Master key deleted"),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 404, description = "Master key not found", body = AdminErrorResponse)
    )
)]
pub async fn delete_master_key(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let deleted = db.delete_master_key(&id).await?;

    if !deleted {
        return Err(AdminError::NotFound(format!(
            "Master key '{}' not found",
            id
        )));
    }

    tracing::info!(key_id = %id, "Master key deleted");

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Auth Handlers
// ============================================================================

/// Validate admin key
///
/// Validates the admin API key for UI login. Returns success if the key is valid.
#[utoipa::path(
    post,
    path = "/admin/v1/auth/validate",
    tag = "auth",
    responses(
        (status = 200, description = "Admin key is valid", body = AuthValidateResponse),
        (status = 401, description = "Invalid admin key", body = AuthValidateResponse),
        (status = 503, description = "Admin key not configured", body = AdminErrorResponse)
    )
)]
pub async fn validate_admin_key(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
) -> Result<Json<AuthValidateResponse>, Response> {
    if state.admin_key.is_empty() {
        let body = serde_json::json!({
            "error": {
                "message": "Admin API not configured. Set ADMIN_KEY environment variable.",
                "code": 503
            }
        });
        return Err((StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response());
    }

    let auth_header = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok());

    let provided_key = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => {
            return Ok(Json(AuthValidateResponse {
                valid: false,
                message: "Invalid admin key".to_string(),
            }));
        }
    };

    if provided_key != state.admin_key {
        let body = AuthValidateResponse {
            valid: false,
            message: "Invalid admin key".to_string(),
        };
        return Err((StatusCode::UNAUTHORIZED, Json(body)).into_response());
    }

    Ok(Json(AuthValidateResponse {
        valid: true,
        message: "Admin key is valid".to_string(),
    }))
}

// ============================================================================
// Config Handlers
// ============================================================================

/// Get current config version
///
/// Returns the current configuration version and when it was loaded.
#[utoipa::path(
    get,
    path = "/admin/v1/config/version",
    tag = "config",
    responses(
        (status = 200, description = "Configuration version", body = ConfigVersionResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse)
    )
)]
pub async fn get_config_version(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
) -> Result<Json<ConfigVersionResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let config = state.dynamic_config.get();

    Ok(Json(ConfigVersionResponse {
        version: config.version,
        timestamp: config.loaded_at.to_rfc3339(),
    }))
}

/// Reload configuration from database
///
/// Triggers a reload of the configuration from the database.
/// This is useful after making changes via the Admin API.
#[utoipa::path(
    post,
    path = "/admin/v1/config/reload",
    tag = "config",
    responses(
        (status = 200, description = "Configuration reloaded", body = ConfigVersionResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 500, description = "Internal server error", body = AdminErrorResponse)
    )
)]
pub async fn reload_config(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
) -> Result<Json<ConfigVersionResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let version = state.dynamic_config.reload().await?;
    let config = state.dynamic_config.get();

    tracing::info!(version = version, "Configuration reloaded via Admin API");

    Ok(Json(ConfigVersionResponse {
        version: config.version,
        timestamp: config.loaded_at.to_rfc3339(),
    }))
}

// ============================================================================
// Router
// ============================================================================

/// Create Admin API router
pub fn admin_router(state: Arc<AdminState>) -> Router {
    Router::new()
        // Auth routes
        .route("/auth/validate", post(validate_admin_key))
        // Provider routes
        .route("/providers", get(list_providers).post(create_provider))
        .route(
            "/providers/:id",
            get(get_provider)
                .put(update_provider)
                .delete(delete_provider),
        )
        // Master key routes
        .route("/master-keys", get(list_master_keys).post(create_master_key))
        .route(
            "/master-keys/:id",
            get(get_master_key)
                .put(update_master_key)
                .delete(delete_master_key),
        )
        // Config routes
        .route("/config/version", get(get_config_version))
        .route("/config/reload", post(reload_config))
        .with_state(state)
}