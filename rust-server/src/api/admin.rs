//! Admin API handlers for dynamic configuration management.
//!
//! Provides RESTful endpoints for managing providers and credentials.
//! All endpoints require ADMIN_KEY authentication.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use utoipa::{OpenApi, ToSchema};

use crate::core::config::ModelMappingValue;
use crate::core::database::{
    create_key_preview, CreateCredential, CreateProvider, CredentialEntity, DynamicConfig,
    ProviderEntity, UpdateCredential, UpdateProvider,
};
use crate::core::middleware::CLIENT_PATTERNS;

/// OpenAPI documentation for Admin API (admin endpoints only)
#[derive(OpenApi)]
#[openapi(
    paths(
        validate_admin_key,
        list_providers,
        create_provider,
        get_provider,
        update_provider,
        delete_provider,
        validate_script,
        list_credentials,
        create_credential,
        get_credential,
        update_credential,
        delete_credential,
        get_config_version,
        reload_config,
        crate::api::health::check_health,
        crate::api::health::get_provider_health,
        crate::api::health::check_provider_health_concurrent,
    ),
    components(
        schemas(
            AuthValidateResponse,
            ProviderListResponse,
            ProviderResponse,
            CreateProviderRequest,
            UpdateProviderRequest,
            CredentialListResponse,
            CredentialResponse,
            CreateCredentialRequest,
            UpdateCredentialRequest,
            ConfigVersionResponse,
            AdminErrorResponse,
            ValidateScriptRequest,
            ValidateScriptResponse,
            crate::api::health::HealthStatus,
            crate::api::health::ModelHealthStatus,
            crate::api::health::ProviderHealthStatus,
            crate::api::health::HealthCheckRequest,
            crate::api::health::HealthCheckResponse,
            crate::api::models::CheckProviderHealthRequest,
            crate::api::models::CheckProviderHealthResponse,
            crate::api::models::ProviderHealthSummary,
            crate::api::models::ModelHealthResult,
        )
    ),
    tags(
        (name = "auth", description = "Authentication endpoints"),
        (name = "providers", description = "Provider management endpoints"),
        (name = "credentials", description = "Credential management endpoints"),
        (name = "config", description = "Configuration management endpoints"),
        (name = "health", description = "Health check endpoints")
    ),
    info(
        title = "LLM Proxy Admin API",
        version = "1.0.0",
        description = "Admin API for managing LLM Proxy configuration including providers and credentials.",
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

/// OpenAPI documentation for V1 API (OpenAI-compatible endpoints)
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::handlers::chat_completions,
        crate::api::handlers::completions,
        crate::api::handlers::list_models,
    ),
    components(
        schemas(
            crate::api::models::ChatCompletionRequest,
            crate::api::models::ChatCompletionResponse,
            crate::api::models::Message,
            crate::api::models::Choice,
            crate::api::models::Usage,
            crate::api::models::ModelList,
            crate::api::models::ModelInfo,
            crate::api::models::ApiErrorResponse,
            crate::api::models::ApiErrorDetail,
        )
    ),
    tags(
        (name = "completions", description = "OpenAI-compatible completion endpoints"),
        (name = "models", description = "OpenAI-compatible model listing endpoints")
    ),
    info(
        title = "LLM Proxy V1 API",
        version = "1.0.0",
        description = "OpenAI-compatible API endpoints for chat completions and model listing.",
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
pub struct V1ApiDoc;

/// Create combined OpenAPI documentation by merging Admin and V1 APIs at runtime.
/// This avoids duplication in the source code while providing a unified API doc.
pub fn combined_openapi() -> utoipa::openapi::OpenApi {
    let mut combined = AdminApiDoc::openapi();
    let v1_doc = V1ApiDoc::openapi();

    // Merge paths
    for (path, item) in v1_doc.paths.paths {
        combined.paths.paths.insert(path, item);
    }

    // Merge components (schemas)
    if let Some(v1_components) = v1_doc.components {
        if let Some(ref mut combined_components) = combined.components {
            for (name, schema) in v1_components.schemas {
                combined_components.schemas.insert(name, schema);
            }
        }
    }

    // Merge tags
    if let Some(v1_tags) = v1_doc.tags {
        if let Some(ref mut combined_tags) = combined.tags {
            combined_tags.extend(v1_tags);
        } else {
            combined.tags = Some(v1_tags);
        }
    }

    // Update info for combined doc
    combined.info.title = "LLM Proxy API".to_string();
    combined.info.description = Some(
        "LLM Proxy API with OpenAI-compatible endpoints and Admin API for configuration management.".to_string()
    );

    combined
}

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
    pub http_client: reqwest::Client,
}

/// Verify admin authentication
pub fn verify_admin_auth(headers: &HeaderMap, admin_key: &str) -> Result<(), AdminError> {
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
        "id": 1,
        "provider_key": "openai-1",
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
    "id": 1,
    "provider_key": "openai-1",
    "provider_type": "openai",
    "api_base": "https://api.openai.com/v1",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "weight": 1,
    "is_enabled": true,
    "provider_params": {},
    "lua_script": null,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z"
}))]
pub struct ProviderResponse {
    /// Auto-increment provider ID
    pub id: i32,
    /// Unique provider key identifier
    pub provider_key: String,
    /// Provider type (e.g., "openai", "azure", "anthropic")
    pub provider_type: String,
    /// Base URL for the provider API
    pub api_base: String,
    /// Model name mapping (request model -> provider model or extended entry)
    pub model_mapping: HashMap<String, ModelMappingValue>,
    /// Weight for load balancing (higher = more traffic)
    pub weight: i32,
    /// Whether this provider is enabled
    pub is_enabled: bool,
    /// Provider-specific parameters (e.g., GCP project, location, publisher)
    pub provider_params: HashMap<String, serde_json::Value>,
    /// Optional Lua script for request/response transformation
    pub lua_script: Option<String>,
    /// Creation timestamp (RFC 3339 format)
    pub created_at: String,
    /// Last update timestamp (RFC 3339 format)
    pub updated_at: String,
}

impl From<ProviderEntity> for ProviderResponse {
    fn from(e: ProviderEntity) -> Self {
        Self {
            id: e.id,
            provider_key: e.provider_key,
            provider_type: e.provider_type,
            api_base: e.api_base,
            model_mapping: e.model_mapping.0,
            weight: e.weight,
            is_enabled: e.is_enabled,
            provider_params: e.provider_params.0,
            lua_script: e.lua_script,
            created_at: e.created_at.to_rfc3339(),
            updated_at: e.updated_at.to_rfc3339(),
        }
    }
}

/// Request to create a new provider
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "provider_key": "openai-1",
    "provider_type": "openai",
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-your-api-key",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "weight": 1,
    "is_enabled": true,
    "provider_params": {}
}))]
pub struct CreateProviderRequest {
    /// Unique provider key identifier
    pub provider_key: String,
    /// Provider type (e.g., "openai", "azure", "anthropic")
    pub provider_type: String,
    /// Base URL for the provider API
    pub api_base: String,
    /// API key for authentication
    pub api_key: String,
    /// Model name mapping (request model -> provider model or extended entry)
    #[serde(default)]
    pub model_mapping: HashMap<String, ModelMappingValue>,
    /// Weight for load balancing (default: 1, higher = more traffic)
    #[serde(default = "default_weight")]
    pub weight: i32,
    /// Whether this provider is enabled (default: true)
    #[serde(default = "default_true")]
    pub is_enabled: bool,
    /// Provider-specific parameters (e.g., GCP project, location, publisher)
    #[serde(default)]
    pub provider_params: HashMap<String, serde_json::Value>,
    /// Optional Lua script for request/response transformation
    #[serde(default)]
    pub lua_script: Option<String>,
}

/// Request to update an existing provider
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "api_base": "https://api.openai.com/v1",
    "weight": 2,
    "is_enabled": false,
    "provider_params": {}
}))]
pub struct UpdateProviderRequest {
    /// Provider type (e.g., "openai", "azure", "anthropic")
    pub provider_type: Option<String>,
    /// Base URL for the provider API
    pub api_base: Option<String>,
    /// API key for authentication
    pub api_key: Option<String>,
    /// Model name mapping (request model -> provider model or extended entry)
    pub model_mapping: Option<HashMap<String, ModelMappingValue>>,
    /// Weight for load balancing (higher = more traffic)
    pub weight: Option<i32>,
    /// Whether this provider is enabled
    pub is_enabled: Option<bool>,
    /// Provider-specific parameters (e.g., GCP project, location, publisher)
    pub provider_params: Option<HashMap<String, serde_json::Value>>,
    /// Optional Lua script (absent=don't change, null=clear, value=set)
    #[serde(
        default,
        deserialize_with = "crate::core::database::deserialize_optional_nullable"
    )]
    pub lua_script: Option<Option<String>>,
}

// ============================================================================
// Credential API Types
// ============================================================================

/// Response containing list of credentials
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "version": 1,
    "credentials": [{
        "id": 1,
        "name": "Production Credential",
        "key_preview": "sk-***abc",
        "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
        "rate_limit": 100,
        "is_enabled": true,
        "created_at": "2024-01-01T00:00:00Z",
        "updated_at": "2024-01-01T00:00:00Z"
    }]
}))]
pub struct CredentialListResponse {
    /// Current configuration version
    pub version: i64,
    /// List of credentials
    pub credentials: Vec<CredentialResponse>,
}

/// Credential response
#[derive(Debug, Serialize, ToSchema)]
#[schema(example = json!({
    "id": 1,
    "name": "Production Credential",
    "key_preview": "sk-***abc",
    "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
    "rate_limit": 100,
    "is_enabled": true,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z"
}))]
pub struct CredentialResponse {
    /// Auto-increment credential ID
    pub id: i32,
    /// Human-readable name for the credential
    pub name: String,
    /// Masked preview of the key (e.g., "sk-***abc")
    pub key_preview: String,
    /// List of models this credential can access (empty = all models)
    pub allowed_models: Vec<String>,
    /// Rate limit in requests per second (null = unlimited)
    pub rate_limit: Option<i32>,
    /// Whether this credential is enabled
    pub is_enabled: bool,
    /// Creation timestamp (RFC 3339 format)
    pub created_at: String,
    /// Last update timestamp (RFC 3339 format)
    pub updated_at: String,
}

impl CredentialResponse {
    fn from_entity(e: CredentialEntity, preview: String) -> Self {
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

/// Request to create a new credential
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "key": "sk-your-credential-key",
    "name": "Production Credential",
    "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
    "rate_limit": 100,
    "is_enabled": true
}))]
pub struct CreateCredentialRequest {
    /// The actual key value (will be hashed for storage)
    pub key: String,
    /// Human-readable name for the credential
    pub name: String,
    /// List of models this credential can access (empty = all models)
    #[serde(default)]
    pub allowed_models: Vec<String>,
    /// Rate limit in requests per second (null = unlimited)
    pub rate_limit: Option<i32>,
    /// Whether this credential is enabled (default: true)
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}

/// Request to update an existing credential
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "name": "Updated Credential Name",
    "rate_limit": 200,
    "is_enabled": false
}))]
pub struct UpdateCredentialRequest {
    /// New key value (will be hashed for storage)
    pub key: Option<String>,
    /// Human-readable name for the credential
    pub name: Option<String>,
    /// List of models this credential can access (empty = all models)
    pub allowed_models: Option<Vec<String>>,
    /// Rate limit in requests per second (null = unlimited)
    pub rate_limit: Option<i32>,
    /// Whether this credential is enabled
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

fn default_weight() -> i32 {
    1
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
        ("id" = i32, Path, description = "Provider ID")
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
    Path(id): Path<i32>,
) -> Result<Json<ProviderResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let provider = db
        .get_provider(id)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Provider with ID {} not found", id)))?;

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

    if req.provider_key.is_empty() {
        return Err(AdminError::BadRequest(
            "Provider key is required".to_string(),
        ));
    }
    if req.api_base.is_empty() {
        return Err(AdminError::BadRequest("API base is required".to_string()));
    }
    if req.api_key.is_empty() {
        return Err(AdminError::BadRequest("API key is required".to_string()));
    }

    let db = state.dynamic_config.database();

    if db.get_provider_by_key(&req.provider_key).await?.is_some() {
        return Err(AdminError::BadRequest(format!(
            "Provider with key '{}' already exists",
            req.provider_key
        )));
    }

    let create = CreateProvider {
        provider_key: req.provider_key,
        provider_type: req.provider_type,
        api_base: req.api_base,
        api_key: req.api_key,
        model_mapping: req.model_mapping,
        weight: req.weight,
        is_enabled: req.is_enabled,
        provider_params: req.provider_params,
        lua_script: req.lua_script,
    };

    let provider = db.create_provider(&create).await?;
    tracing::info!(provider_id = %provider.id, provider_key = %provider.provider_key, "Provider created");

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
        ("id" = i32, Path, description = "Provider ID")
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
    Path(id): Path<i32>,
    Json(req): Json<UpdateProviderRequest>,
) -> Result<Json<ProviderResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();

    let update = UpdateProvider {
        provider_type: req.provider_type,
        api_base: req.api_base,
        api_key: req.api_key,
        model_mapping: req.model_mapping,
        weight: req.weight,
        is_enabled: req.is_enabled,
        provider_params: req.provider_params,
        lua_script: req.lua_script,
    };

    let provider = db
        .update_provider(id, &update)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Provider with ID {} not found", id)))?;

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
        ("id" = i32, Path, description = "Provider ID")
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
    Path(id): Path<i32>,
) -> Result<StatusCode, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let deleted = db.delete_provider(id).await?;

    if !deleted {
        return Err(AdminError::NotFound(format!(
            "Provider with ID {} not found",
            id
        )));
    }

    tracing::info!(provider_id = %id, "Provider deleted");

    Ok(StatusCode::NO_CONTENT)
}

// ============================================================================
// Credential Handlers
// ============================================================================

/// List all credentials
///
/// Returns a list of all configured credentials with their current configuration version.
/// Key values are masked for security.
#[utoipa::path(
    get,
    path = "/admin/v1/credentials",
    tag = "credentials",
    responses(
        (status = 200, description = "List of credentials", body = CredentialListResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse)
    )
)]
pub async fn list_credentials(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
) -> Result<Json<CredentialListResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let credentials = db.load_all_credentials().await?;
    let version = db.get_config_version().await?;

    let responses: Vec<CredentialResponse> = credentials
        .into_iter()
        .map(|c| {
            let preview = format!("***{}", &c.credential_key[..6]);
            CredentialResponse::from_entity(c, preview)
        })
        .collect();

    Ok(Json(CredentialListResponse {
        version,
        credentials: responses,
    }))
}

/// Get a single credential
///
/// Returns the configuration for a specific credential by ID.
/// The key value is masked for security.
#[utoipa::path(
    get,
    path = "/admin/v1/credentials/{id}",
    tag = "credentials",
    params(
        ("id" = i32, Path, description = "Credential ID")
    ),
    responses(
        (status = 200, description = "Credential details", body = CredentialResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 404, description = "Credential not found", body = AdminErrorResponse)
    )
)]
pub async fn get_credential(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<CredentialResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let credential = db
        .get_credential(id)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Credential with ID {} not found", id)))?;

    let preview = format!("***{}", &credential.credential_key[..6]);
    Ok(Json(CredentialResponse::from_entity(credential, preview)))
}

/// Create a new credential
///
/// Creates a new credential with the specified configuration.
/// The key value will be hashed for secure storage.
#[utoipa::path(
    post,
    path = "/admin/v1/credentials",
    tag = "credentials",
    request_body = CreateCredentialRequest,
    responses(
        (status = 201, description = "Credential created", body = CredentialResponse),
        (status = 400, description = "Bad request", body = AdminErrorResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse)
    )
)]
pub async fn create_credential(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Json(req): Json<CreateCredentialRequest>,
) -> Result<(StatusCode, Json<CredentialResponse>), AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    if req.key.is_empty() {
        return Err(AdminError::BadRequest("Key value is required".to_string()));
    }
    if req.name.is_empty() {
        return Err(AdminError::BadRequest(
            "Credential name is required".to_string(),
        ));
    }

    let db = state.dynamic_config.database();

    let key_preview = create_key_preview(&req.key);

    let create = CreateCredential {
        key: req.key,
        name: req.name,
        allowed_models: req.allowed_models,
        rate_limit: req.rate_limit,
        is_enabled: req.is_enabled,
    };

    let credential = db.create_credential(&create).await?;
    tracing::info!(credential_id = %credential.id, credential_name = %credential.name, "Credential created");

    Ok((
        StatusCode::CREATED,
        Json(CredentialResponse::from_entity(credential, key_preview)),
    ))
}

/// Update an existing credential
///
/// Updates the configuration for an existing credential. Only provided fields will be updated.
/// If a new key value is provided, it will be hashed for secure storage.
#[utoipa::path(
    put,
    path = "/admin/v1/credentials/{id}",
    tag = "credentials",
    params(
        ("id" = i32, Path, description = "Credential ID")
    ),
    request_body = UpdateCredentialRequest,
    responses(
        (status = 200, description = "Credential updated", body = CredentialResponse),
        (status = 400, description = "Bad request", body = AdminErrorResponse),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 404, description = "Credential not found", body = AdminErrorResponse)
    )
)]
pub async fn update_credential(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<UpdateCredentialRequest>,
) -> Result<Json<CredentialResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();

    let update = UpdateCredential {
        key: req.key.clone(),
        name: req.name,
        allowed_models: req.allowed_models,
        rate_limit: req.rate_limit,
        is_enabled: req.is_enabled,
    };

    let credential = db
        .update_credential(id, &update)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Credential with ID {} not found", id)))?;

    let preview = if let Some(new_key) = req.key {
        create_key_preview(&new_key)
    } else {
        format!("***{}", &credential.credential_key[..6])
    };

    tracing::info!(credential_id = %id, "Credential updated");

    Ok(Json(CredentialResponse::from_entity(credential, preview)))
}

/// Delete a credential
///
/// Permanently deletes a credential from the configuration.
#[utoipa::path(
    delete,
    path = "/admin/v1/credentials/{id}",
    tag = "credentials",
    params(
        ("id" = i32, Path, description = "Credential ID")
    ),
    responses(
        (status = 204, description = "Credential deleted"),
        (status = 401, description = "Unauthorized", body = AdminErrorResponse),
        (status = 404, description = "Credential not found", body = AdminErrorResponse)
    )
)]
pub async fn delete_credential(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<StatusCode, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let db = state.dynamic_config.database();
    let deleted = db.delete_credential(id).await?;

    if !deleted {
        return Err(AdminError::NotFound(format!(
            "Credential with ID {} not found",
            id
        )));
    }

    tracing::info!(credential_id = %id, "Credential deleted");

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

    let auth_header = headers.get("authorization").and_then(|h| h.to_str().ok());

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
// Request Logs API
// ============================================================================

fn extract_client_from_headers(headers_json: Option<&str>) -> Option<String> {
    let json_str = headers_json?;
    let parsed: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let ua = parsed
        .get("user-agent")
        .or_else(|| parsed.get("User-Agent"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if ua.is_empty() {
        return Some("unknown".to_string());
    }
    for (pattern, name) in CLIENT_PATTERNS {
        if ua.contains(pattern) {
            return Some(name.to_string());
        }
    }
    let first_token: &str = ua.split(' ').next().unwrap_or("");
    let first_token = first_token.split('/').next().unwrap_or("");
    let cleaned: String = first_token
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .take(30)
        .collect();
    Some(if cleaned.is_empty() {
        "other".to_string()
    } else {
        cleaned
    })
}

#[derive(Debug, Serialize)]
pub struct RequestLogItem {
    pub id: i64,
    pub timestamp: String,
    pub request_id: String,
    pub endpoint: Option<String>,
    pub credential_name: Option<String>,
    pub model_requested: Option<String>,
    pub model_mapped: Option<String>,
    pub provider_name: Option<String>,
    pub provider_type: Option<String>,
    pub client_protocol: Option<String>,
    pub provider_protocol: Option<String>,
    pub is_streaming: Option<bool>,
    pub status_code: Option<i32>,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub total_tokens: i32,
    pub total_duration_ms: Option<i32>,
    pub ttft_ms: Option<i32>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub client: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RequestLogDetail {
    #[serde(flatten)]
    pub item: RequestLogItem,
    pub request_headers: Option<String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RequestLogListResponse {
    pub items: Vec<RequestLogItem>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}

#[derive(Debug, Serialize)]
pub struct RequestLogStatsResponse {
    pub total_requests: i64,
    pub total_errors: i64,
    pub error_rate: f64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub avg_duration_ms: Option<f64>,
    pub avg_ttft_ms: Option<f64>,
    pub requests_by_provider: HashMap<String, i64>,
    pub requests_by_model: HashMap<String, i64>,
    pub requests_by_status: HashMap<String, i64>,
}

#[derive(Debug, Deserialize)]
pub struct LogQueryParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub request_id: Option<String>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    pub credential_name: Option<String>,
    pub status_code: Option<i32>,
    pub is_streaming: Option<bool>,
    pub error_only: Option<bool>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LogStatsParams {
    pub provider_name: Option<String>,
    pub model: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

fn parse_iso_time(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

pub async fn list_logs(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Query(params): Query<LogQueryParams>,
) -> Result<Json<RequestLogListResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * page_size;

    let pool = state.dynamic_config.database().pool();

    // Build WHERE clauses dynamically
    let mut conditions: Vec<String> = Vec::new();
    let mut param_idx = 0usize;

    struct BindValues {
        strings: Vec<String>,
        ints: Vec<i32>,
        bools: Vec<bool>,
        timestamps: Vec<chrono::DateTime<chrono::Utc>>,
    }
    let mut binds = BindValues {
        strings: Vec::new(),
        ints: Vec::new(),
        bools: Vec::new(),
        timestamps: Vec::new(),
    };

    if let Some(ref req_id) = params.request_id {
        param_idx += 1;
        conditions.push(format!("request_id = ${}", param_idx));
        binds.strings.push(req_id.clone());
    }
    if let Some(ref prov) = params.provider_name {
        param_idx += 1;
        conditions.push(format!("provider_name = ${}", param_idx));
        binds.strings.push(prov.clone());
    }
    if let Some(ref model) = params.model {
        param_idx += 1;
        conditions.push(format!("model_requested = ${}", param_idx));
        binds.strings.push(model.clone());
    }
    if let Some(ref cred) = params.credential_name {
        param_idx += 1;
        conditions.push(format!("credential_name = ${}", param_idx));
        binds.strings.push(cred.clone());
    }
    if let Some(sc) = params.status_code {
        param_idx += 1;
        conditions.push(format!("status_code = ${}", param_idx));
        binds.ints.push(sc);
    }
    if let Some(is_str) = params.is_streaming {
        param_idx += 1;
        conditions.push(format!("is_streaming = ${}", param_idx));
        binds.bools.push(is_str);
    }
    if params.error_only.unwrap_or(false) {
        conditions.push("(status_code >= 400 OR error_category IS NOT NULL)".to_string());
    }
    if let Some(ref st) = params.start_time {
        if let Some(ts) = parse_iso_time(st) {
            param_idx += 1;
            conditions.push(format!("timestamp >= ${}", param_idx));
            binds.timestamps.push(ts);
        }
    }
    if let Some(ref et) = params.end_time {
        if let Some(ts) = parse_iso_time(et) {
            param_idx += 1;
            conditions.push(format!("timestamp <= ${}", param_idx));
            binds.timestamps.push(ts);
        }
    }

    debug_assert_eq!(
        param_idx,
        binds.strings.len() + binds.ints.len() + binds.bools.len() + binds.timestamps.len(),
        "bind_params: parameter count mismatch"
    );

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sort_col = match params.sort_by.as_deref() {
        Some("status_code") => "status_code",
        Some("total_duration_ms") => "total_duration_ms",
        Some("input_tokens") => "input_tokens",
        Some("output_tokens") => "output_tokens",
        Some("total_tokens") => "total_tokens",
        _ => "timestamp",
    };
    let sort_dir = match params.sort_order.as_deref() {
        Some("asc") => "ASC",
        _ => "DESC",
    };

    let count_sql = format!("SELECT COUNT(*) FROM request_logs {}", where_clause);
    let data_sql = format!(
        "SELECT id, timestamp, request_id, endpoint, credential_name, \
         model_requested, model_mapped, provider_name, provider_type, \
         client_protocol, provider_protocol, is_streaming, status_code, \
         input_tokens, output_tokens, total_tokens, \
         total_duration_ms, ttft_ms, error_category, error_message, \
         request_headers \
         FROM request_logs {} ORDER BY {} {} LIMIT {} OFFSET {}",
        where_clause, sort_col, sort_dir, page_size, offset
    );

    // Bind all parameters in order
    macro_rules! bind_params {
        ($query:expr) => {{
            let mut q = $query;
            let mut str_idx = 0usize;
            let mut int_idx = 0usize;
            let mut bool_idx = 0usize;
            let mut ts_idx = 0usize;

            if params.request_id.is_some() {
                q = q.bind(&binds.strings[str_idx]);
                str_idx += 1;
            }
            if params.provider_name.is_some() {
                q = q.bind(&binds.strings[str_idx]);
                str_idx += 1;
            }
            if params.model.is_some() {
                q = q.bind(&binds.strings[str_idx]);
                str_idx += 1;
            }
            if params.credential_name.is_some() {
                q = q.bind(&binds.strings[str_idx]);
                #[allow(unused_assignments)]
                {
                    str_idx += 1;
                }
            }
            if params.status_code.is_some() {
                q = q.bind(binds.ints[int_idx]);
                #[allow(unused_assignments)]
                {
                    int_idx += 1;
                }
            }
            if params.is_streaming.is_some() {
                q = q.bind(binds.bools[bool_idx]);
                #[allow(unused_assignments)]
                {
                    bool_idx += 1;
                }
            }
            if params
                .start_time
                .as_ref()
                .and_then(|s| parse_iso_time(s))
                .is_some()
            {
                q = q.bind(binds.timestamps[ts_idx]);
                ts_idx += 1;
            }
            if params
                .end_time
                .as_ref()
                .and_then(|s| parse_iso_time(s))
                .is_some()
            {
                q = q.bind(binds.timestamps[ts_idx]);
                #[allow(unused_assignments)]
                {
                    ts_idx += 1;
                }
            }
            q
        }};
    }

    let total: i64 = bind_params!(sqlx::query_scalar::<_, i64>(&count_sql))
        .fetch_one(pool)
        .await?;

    let total_pages = if total == 0 {
        1
    } else {
        (total + page_size - 1) / page_size
    };

    #[derive(sqlx::FromRow)]
    struct LogRow {
        id: i64,
        timestamp: chrono::DateTime<chrono::Utc>,
        request_id: String,
        endpoint: Option<String>,
        credential_name: Option<String>,
        model_requested: Option<String>,
        model_mapped: Option<String>,
        provider_name: Option<String>,
        provider_type: Option<String>,
        client_protocol: Option<String>,
        provider_protocol: Option<String>,
        is_streaming: Option<bool>,
        status_code: Option<i32>,
        input_tokens: i32,
        output_tokens: i32,
        total_tokens: i32,
        total_duration_ms: Option<i32>,
        ttft_ms: Option<i32>,
        error_category: Option<String>,
        error_message: Option<String>,
        request_headers: Option<String>,
    }

    let rows: Vec<LogRow> = bind_params!(sqlx::query_as::<_, LogRow>(&data_sql))
        .fetch_all(pool)
        .await?;

    let items: Vec<RequestLogItem> = rows
        .into_iter()
        .map(|r| RequestLogItem {
            id: r.id,
            timestamp: r.timestamp.to_rfc3339(),
            request_id: r.request_id,
            endpoint: r.endpoint,
            credential_name: r.credential_name,
            model_requested: r.model_requested,
            model_mapped: r.model_mapped,
            provider_name: r.provider_name,
            provider_type: r.provider_type,
            client_protocol: r.client_protocol,
            provider_protocol: r.provider_protocol,
            is_streaming: r.is_streaming,
            status_code: r.status_code,
            input_tokens: r.input_tokens,
            output_tokens: r.output_tokens,
            total_tokens: r.total_tokens,
            total_duration_ms: r.total_duration_ms,
            ttft_ms: r.ttft_ms,
            error_category: r.error_category,
            error_message: r.error_message,
            client: extract_client_from_headers(r.request_headers.as_deref()),
        })
        .collect();

    Ok(Json(RequestLogListResponse {
        items,
        total,
        page,
        page_size,
        total_pages,
    }))
}

pub async fn get_log_stats(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Query(params): Query<LogStatsParams>,
) -> Result<Json<RequestLogStatsResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let pool = state.dynamic_config.database().pool();

    let mut conditions: Vec<String> = Vec::new();
    let mut str_binds: Vec<String> = Vec::new();
    let mut ts_binds: Vec<chrono::DateTime<chrono::Utc>> = Vec::new();
    let mut param_idx = 0;

    if let Some(ref prov) = params.provider_name {
        param_idx += 1;
        conditions.push(format!("provider_name = ${}", param_idx));
        str_binds.push(prov.clone());
    }
    if let Some(ref model) = params.model {
        param_idx += 1;
        conditions.push(format!("model_requested = ${}", param_idx));
        str_binds.push(model.clone());
    }
    if let Some(ref st) = params.start_time {
        if let Some(ts) = parse_iso_time(st) {
            param_idx += 1;
            conditions.push(format!("timestamp >= ${}", param_idx));
            ts_binds.push(ts);
        }
    }
    if let Some(ref et) = params.end_time {
        if let Some(ts) = parse_iso_time(et) {
            param_idx += 1;
            conditions.push(format!("timestamp <= ${}", param_idx));
            ts_binds.push(ts);
        }
    }
    debug_assert_eq!(
        param_idx as usize,
        str_binds.len() + ts_binds.len(),
        "bind_params: parameter count mismatch"
    );

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let stats_sql = format!(
        "SELECT \
         COUNT(*) as total_requests, \
         COUNT(*) FILTER (WHERE status_code >= 400 OR error_category IS NOT NULL) as total_errors, \
         COALESCE(SUM(input_tokens), 0) as total_input_tokens, \
         COALESCE(SUM(output_tokens), 0) as total_output_tokens, \
         AVG(total_duration_ms)::float8 as avg_duration_ms, \
         AVG(ttft_ms)::float8 as avg_ttft_ms \
         FROM request_logs {}",
        where_clause
    );

    #[derive(sqlx::FromRow)]
    struct StatsRow {
        total_requests: Option<i64>,
        total_errors: Option<i64>,
        total_input_tokens: Option<i64>,
        total_output_tokens: Option<i64>,
        avg_duration_ms: Option<f64>,
        avg_ttft_ms: Option<f64>,
    }

    let mut query = sqlx::query_as::<_, StatsRow>(&stats_sql);
    let mut si = 0usize;
    let mut ti = 0usize;
    if params.provider_name.is_some() {
        query = query.bind(&str_binds[si]);
        si += 1;
    }
    if params.model.is_some() {
        query = query.bind(&str_binds[si]);
        #[allow(unused_assignments)]
        {
            si += 1;
        }
    }
    if params
        .start_time
        .as_ref()
        .and_then(|s| parse_iso_time(s))
        .is_some()
    {
        query = query.bind(ts_binds[ti]);
        ti += 1;
    }
    if params
        .end_time
        .as_ref()
        .and_then(|s| parse_iso_time(s))
        .is_some()
    {
        query = query.bind(ts_binds[ti]);
        #[allow(unused_assignments)]
        {
            ti += 1;
        }
    }

    let row = query.fetch_one(pool).await?;
    let total_requests = row.total_requests.unwrap_or(0);
    let total_errors = row.total_errors.unwrap_or(0);
    let error_rate = if total_requests > 0 {
        total_errors as f64 / total_requests as f64
    } else {
        0.0
    };

    // Helper closure to bind params to a group-by query
    macro_rules! bind_stats_params {
        ($q:expr) => {{
            let mut q = $q;
            let mut si = 0usize;
            let mut ti = 0usize;
            if params.provider_name.is_some() {
                q = q.bind(&str_binds[si]);
                si += 1;
            }
            if params.model.is_some() {
                q = q.bind(&str_binds[si]);
                #[allow(unused_assignments)]
                {
                    si += 1;
                }
            }
            if params
                .start_time
                .as_ref()
                .and_then(|s| parse_iso_time(s))
                .is_some()
            {
                q = q.bind(ts_binds[ti]);
                ti += 1;
            }
            if params
                .end_time
                .as_ref()
                .and_then(|s| parse_iso_time(s))
                .is_some()
            {
                q = q.bind(ts_binds[ti]);
                #[allow(unused_assignments)]
                {
                    ti += 1;
                }
            }
            q
        }};
    }

    let by_provider_sql = format!(
        "SELECT COALESCE(provider_name, 'unknown'), COUNT(*) FROM request_logs {} GROUP BY provider_name ORDER BY COUNT(*) DESC LIMIT 50",
        where_clause
    );
    let by_provider: Vec<(String, i64)> = bind_stats_params!(sqlx::query_as(&by_provider_sql))
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    let by_model_sql = format!(
        "SELECT COALESCE(model_requested, 'unknown'), COUNT(*) FROM request_logs {} GROUP BY model_requested ORDER BY COUNT(*) DESC LIMIT 50",
        where_clause
    );
    let by_model: Vec<(String, i64)> = bind_stats_params!(sqlx::query_as(&by_model_sql))
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    let by_status_sql = format!(
        "SELECT COALESCE(status_code::text, 'unknown'), COUNT(*) FROM request_logs {} GROUP BY status_code ORDER BY COUNT(*) DESC",
        where_clause
    );
    let by_status: Vec<(String, i64)> = bind_stats_params!(sqlx::query_as(&by_status_sql))
        .fetch_all(pool)
        .await
        .unwrap_or_default();

    Ok(Json(RequestLogStatsResponse {
        total_requests,
        total_errors,
        error_rate,
        total_input_tokens: row.total_input_tokens.unwrap_or(0),
        total_output_tokens: row.total_output_tokens.unwrap_or(0),
        avg_duration_ms: row.avg_duration_ms,
        avg_ttft_ms: row.avg_ttft_ms,
        requests_by_provider: by_provider.into_iter().collect(),
        requests_by_model: by_model.into_iter().collect(),
        requests_by_status: by_status.into_iter().collect(),
    }))
}

pub async fn get_log_detail(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(log_id): Path<i64>,
) -> Result<Json<RequestLogDetail>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let pool = state.dynamic_config.database().pool();

    #[derive(sqlx::FromRow)]
    struct DetailRow {
        id: i64,
        timestamp: chrono::DateTime<chrono::Utc>,
        request_id: String,
        endpoint: Option<String>,
        credential_name: Option<String>,
        model_requested: Option<String>,
        model_mapped: Option<String>,
        provider_name: Option<String>,
        provider_type: Option<String>,
        client_protocol: Option<String>,
        provider_protocol: Option<String>,
        is_streaming: Option<bool>,
        status_code: Option<i32>,
        input_tokens: i32,
        output_tokens: i32,
        total_tokens: i32,
        total_duration_ms: Option<i32>,
        ttft_ms: Option<i32>,
        error_category: Option<String>,
        error_message: Option<String>,
        request_headers: Option<String>,
        request_body: Option<String>,
        response_body: Option<String>,
    }

    let row: DetailRow = sqlx::query_as(
        "SELECT id, timestamp, request_id, endpoint, credential_name, \
         model_requested, model_mapped, provider_name, provider_type, \
         client_protocol, provider_protocol, is_streaming, status_code, \
         input_tokens, output_tokens, total_tokens, \
         total_duration_ms, ttft_ms, error_category, error_message, \
         request_headers, request_body, response_body \
         FROM request_logs WHERE id = $1",
    )
    .bind(log_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AdminError::NotFound(format!("Log {} not found", log_id)))?;

    Ok(Json(RequestLogDetail {
        item: RequestLogItem {
            id: row.id,
            timestamp: row.timestamp.to_rfc3339(),
            request_id: row.request_id,
            endpoint: row.endpoint,
            credential_name: row.credential_name,
            model_requested: row.model_requested,
            model_mapped: row.model_mapped,
            provider_name: row.provider_name,
            provider_type: row.provider_type,
            client_protocol: row.client_protocol,
            provider_protocol: row.provider_protocol,
            is_streaming: row.is_streaming,
            status_code: row.status_code,
            input_tokens: row.input_tokens,
            output_tokens: row.output_tokens,
            total_tokens: row.total_tokens,
            total_duration_ms: row.total_duration_ms,
            ttft_ms: row.ttft_ms,
            error_category: row.error_category,
            error_message: row.error_message,
            client: extract_client_from_headers(row.request_headers.as_deref()),
        },
        request_headers: row.request_headers,
        request_body: row.request_body,
        response_body: row.response_body,
    }))
}

// ============================================================================
// Error Logs API
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ErrorLogItem {
    pub id: i64,
    pub timestamp: String,
    pub request_id: Option<String>,
    pub error_category: String,
    pub error_code: Option<i32>,
    pub error_message: Option<String>,
    pub provider_name: Option<String>,
    pub credential_name: Option<String>,
    pub model_requested: Option<String>,
    pub model_mapped: Option<String>,
    pub endpoint: Option<String>,
    pub client_protocol: Option<String>,
    pub provider_protocol: Option<String>,
    pub is_streaming: Option<bool>,
    pub total_duration_ms: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct ErrorLogDetail {
    #[serde(flatten)]
    pub item: ErrorLogItem,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub provider_request_body: Option<String>,
    pub provider_request_headers: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ErrorLogListResponse {
    pub items: Vec<ErrorLogItem>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub total_pages: i64,
}

#[derive(Debug, Deserialize)]
pub struct ErrorLogQueryParams {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub request_id: Option<String>,
    pub provider_name: Option<String>,
    pub error_category: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

pub async fn list_error_logs(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Query(params): Query<ErrorLogQueryParams>,
) -> Result<Json<ErrorLogListResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let page = params.page.unwrap_or(1).max(1);
    let page_size = params.page_size.unwrap_or(50).clamp(1, 200);
    let offset = (page - 1) * page_size;
    let pool = state.dynamic_config.database().pool();

    let mut conditions: Vec<String> = Vec::new();
    let mut str_binds: Vec<String> = Vec::new();
    let mut ts_binds: Vec<chrono::DateTime<chrono::Utc>> = Vec::new();
    let mut param_idx = 0;

    if let Some(ref req_id) = params.request_id {
        param_idx += 1;
        conditions.push(format!("request_id = ${}", param_idx));
        str_binds.push(req_id.clone());
    }
    if let Some(ref prov) = params.provider_name {
        param_idx += 1;
        conditions.push(format!("provider_name = ${}", param_idx));
        str_binds.push(prov.clone());
    }
    if let Some(ref cat) = params.error_category {
        param_idx += 1;
        conditions.push(format!("error_category = ${}", param_idx));
        str_binds.push(cat.clone());
    }
    if let Some(ref st) = params.start_time {
        if let Some(ts) = parse_iso_time(st) {
            param_idx += 1;
            conditions.push(format!("timestamp >= ${}", param_idx));
            ts_binds.push(ts);
        }
    }
    if let Some(ref et) = params.end_time {
        if let Some(ts) = parse_iso_time(et) {
            param_idx += 1;
            conditions.push(format!("timestamp <= ${}", param_idx));
            ts_binds.push(ts);
        }
    }
    debug_assert_eq!(
        param_idx as usize,
        str_binds.len() + ts_binds.len(),
        "bind_error_params: parameter count mismatch"
    );

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sort_col = match params.sort_by.as_deref() {
        Some("error_category") => "error_category",
        Some("total_duration_ms") => "total_duration_ms",
        _ => "timestamp",
    };
    let sort_dir = match params.sort_order.as_deref() {
        Some("asc") => "ASC",
        _ => "DESC",
    };

    let count_sql = format!("SELECT COUNT(*) FROM error_logs {}", where_clause);
    let data_sql = format!(
        "SELECT id, timestamp, request_id, error_category, error_code, error_message, \
         provider_name, credential_name, \
         NULL as model_requested, mapped_model as model_mapped, \
         endpoint, client_protocol, provider_protocol, is_streaming, total_duration_ms \
         FROM error_logs {} ORDER BY {} {} LIMIT {} OFFSET {}",
        where_clause, sort_col, sort_dir, page_size, offset
    );

    macro_rules! bind_error_params {
        ($query:expr) => {{
            let mut q = $query;
            let mut si = 0usize;
            let mut ti = 0usize;
            if params.request_id.is_some() {
                q = q.bind(&str_binds[si]);
                si += 1;
            }
            if params.provider_name.is_some() {
                q = q.bind(&str_binds[si]);
                si += 1;
            }
            if params.error_category.is_some() {
                q = q.bind(&str_binds[si]);
                #[allow(unused_assignments)]
                {
                    si += 1;
                }
            }
            if params
                .start_time
                .as_ref()
                .and_then(|s| parse_iso_time(s))
                .is_some()
            {
                q = q.bind(ts_binds[ti]);
                ti += 1;
            }
            if params
                .end_time
                .as_ref()
                .and_then(|s| parse_iso_time(s))
                .is_some()
            {
                q = q.bind(ts_binds[ti]);
                #[allow(unused_assignments)]
                {
                    ti += 1;
                }
            }
            q
        }};
    }

    let total: i64 = bind_error_params!(sqlx::query_scalar::<_, i64>(&count_sql))
        .fetch_one(pool)
        .await?;

    let total_pages = if total == 0 {
        1
    } else {
        (total + page_size - 1) / page_size
    };

    #[derive(sqlx::FromRow)]
    struct ErrRow {
        id: i64,
        timestamp: chrono::DateTime<chrono::Utc>,
        request_id: Option<String>,
        error_category: String,
        error_code: Option<i32>,
        error_message: Option<String>,
        provider_name: Option<String>,
        credential_name: Option<String>,
        model_requested: Option<String>,
        model_mapped: Option<String>,
        endpoint: Option<String>,
        client_protocol: Option<String>,
        provider_protocol: Option<String>,
        is_streaming: Option<bool>,
        total_duration_ms: Option<i32>,
    }

    let rows: Vec<ErrRow> = bind_error_params!(sqlx::query_as::<_, ErrRow>(&data_sql))
        .fetch_all(pool)
        .await?;

    let items: Vec<ErrorLogItem> = rows
        .into_iter()
        .map(|r| ErrorLogItem {
            id: r.id,
            timestamp: r.timestamp.to_rfc3339(),
            request_id: r.request_id,
            error_category: r.error_category,
            error_code: r.error_code,
            error_message: r.error_message,
            provider_name: r.provider_name,
            credential_name: r.credential_name,
            model_requested: r.model_requested,
            model_mapped: r.model_mapped,
            endpoint: r.endpoint,
            client_protocol: r.client_protocol,
            provider_protocol: r.provider_protocol,
            is_streaming: r.is_streaming,
            total_duration_ms: r.total_duration_ms,
        })
        .collect();

    Ok(Json(ErrorLogListResponse {
        items,
        total,
        page,
        page_size,
        total_pages,
    }))
}

pub async fn get_error_log_detail(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(log_id): Path<i64>,
) -> Result<Json<ErrorLogDetail>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    let pool = state.dynamic_config.database().pool();

    #[derive(sqlx::FromRow)]
    struct ErrDetailRow {
        id: i64,
        timestamp: chrono::DateTime<chrono::Utc>,
        request_id: Option<String>,
        error_category: String,
        error_code: Option<i32>,
        error_message: Option<String>,
        provider_name: Option<String>,
        credential_name: Option<String>,
        model_requested: Option<String>,
        model_mapped: Option<String>,
        endpoint: Option<String>,
        client_protocol: Option<String>,
        provider_protocol: Option<String>,
        is_streaming: Option<bool>,
        total_duration_ms: Option<i32>,
        request_body: Option<String>,
        response_body: Option<String>,
        provider_request_body: Option<String>,
        provider_request_headers: Option<String>,
    }

    let row: ErrDetailRow = sqlx::query_as(
        "SELECT id, timestamp, request_id, error_category, error_code, error_message, \
         provider_name, credential_name, \
         NULL as model_requested, mapped_model as model_mapped, \
         endpoint, client_protocol, provider_protocol, is_streaming, total_duration_ms, \
         request_body::text, response_body::text, \
         provider_request_body::text, provider_request_headers::text \
         FROM error_logs WHERE id = $1",
    )
    .bind(log_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AdminError::NotFound(format!("Error log {} not found", log_id)))?;

    Ok(Json(ErrorLogDetail {
        item: ErrorLogItem {
            id: row.id,
            timestamp: row.timestamp.to_rfc3339(),
            request_id: row.request_id,
            error_category: row.error_category,
            error_code: row.error_code,
            error_message: row.error_message,
            provider_name: row.provider_name,
            credential_name: row.credential_name,
            model_requested: row.model_requested,
            model_mapped: row.model_mapped,
            endpoint: row.endpoint,
            client_protocol: row.client_protocol,
            provider_protocol: row.provider_protocol,
            is_streaming: row.is_streaming,
            total_duration_ms: row.total_duration_ms,
        },
        request_body: row.request_body,
        response_body: row.response_body,
        provider_request_body: row.provider_request_body,
        provider_request_headers: row.provider_request_headers,
    }))
}

// ============================================================================
// Log Deletion API
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct BatchDeleteRequest {
    pub ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
pub struct BatchDeleteResponse {
    pub deleted: i64,
}

pub async fn delete_log(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(log_id): Path<i64>,
) -> Result<StatusCode, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;
    let pool = state.dynamic_config.database().pool();

    let result = sqlx::query("DELETE FROM request_logs WHERE id = $1")
        .bind(log_id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AdminError::NotFound(format!("Log {} not found", log_id)));
    }

    tracing::info!(log_id = %log_id, "Request log deleted");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn batch_delete_logs(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Json(body): Json<BatchDeleteRequest>,
) -> Result<Json<BatchDeleteResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    if body.ids.is_empty() {
        return Ok(Json(BatchDeleteResponse { deleted: 0 }));
    }
    if body.ids.len() > 1000 {
        return Err(AdminError::BadRequest(
            "Cannot delete more than 1000 records at once".to_string(),
        ));
    }

    let pool = state.dynamic_config.database().pool();

    let mut sql = String::from("DELETE FROM request_logs WHERE id IN (");
    for (i, _) in body.ids.iter().enumerate() {
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push('$');
        sql.push_str(&(i + 1).to_string());
    }
    sql.push(')');

    let mut query = sqlx::query(&sql);
    for id in &body.ids {
        query = query.bind(*id);
    }

    let result = query.execute(pool).await?;
    let deleted = result.rows_affected() as i64;

    tracing::info!(deleted = %deleted, "Request logs batch deleted");
    Ok(Json(BatchDeleteResponse { deleted }))
}

pub async fn delete_error_log(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(log_id): Path<i64>,
) -> Result<StatusCode, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;
    let pool = state.dynamic_config.database().pool();

    let result = sqlx::query("DELETE FROM error_logs WHERE id = $1")
        .bind(log_id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AdminError::NotFound(format!(
            "Error log {} not found",
            log_id
        )));
    }

    tracing::info!(log_id = %log_id, "Error log deleted");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn batch_delete_error_logs(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Json(body): Json<BatchDeleteRequest>,
) -> Result<Json<BatchDeleteResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    if body.ids.is_empty() {
        return Ok(Json(BatchDeleteResponse { deleted: 0 }));
    }
    if body.ids.len() > 1000 {
        return Err(AdminError::BadRequest(
            "Cannot delete more than 1000 records at once".to_string(),
        ));
    }

    let pool = state.dynamic_config.database().pool();

    let mut sql = String::from("DELETE FROM error_logs WHERE id IN (");
    for (i, _) in body.ids.iter().enumerate() {
        if i > 0 {
            sql.push_str(", ");
        }
        sql.push('$');
        sql.push_str(&(i + 1).to_string());
    }
    sql.push(')');

    let mut query = sqlx::query(&sql);
    for id in &body.ids {
        query = query.bind(*id);
    }

    let result = query.execute(pool).await?;
    let deleted = result.rows_affected() as i64;

    tracing::info!(deleted = %deleted, "Error logs batch deleted");
    Ok(Json(BatchDeleteResponse { deleted }))
}

// ============================================================================
// Lua Script Validation
// ============================================================================

#[derive(Debug, Deserialize, ToSchema)]
pub struct ValidateScriptRequest {
    pub lua_script: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ValidateScriptResponse {
    pub valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Strip internal paths and details from Lua error messages.
fn sanitize_lua_error(err: &str) -> String {
    if err.contains("exceeds maximum") {
        return err.to_string();
    }
    if err.contains("must define at least one hook") {
        return err.to_string();
    }
    if let Some(msg) = err.strip_prefix("Lua compilation error: ") {
        return format!("Syntax error: {msg}");
    }
    if err.contains("instruction limit") {
        return "Script exceeded execution limit (possible infinite loop)".to_string();
    }
    "Script validation failed".to_string()
}

#[utoipa::path(
    post,
    path = "/admin/v1/providers/{id}/validate-script",
    tag = "providers",
    request_body = ValidateScriptRequest,
    responses(
        (status = 200, description = "Validation result", body = ValidateScriptResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Provider not found"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn validate_script(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(body): Json<ValidateScriptRequest>,
) -> Result<Json<ValidateScriptResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    // Verify provider exists
    let db = state.dynamic_config.database();
    let _provider = db
        .get_provider(id)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Provider with ID {} not found", id)))?;

    match crate::scripting::sandbox::validate_script(&body.lua_script) {
        Ok(()) => Ok(Json(ValidateScriptResponse {
            valid: true,
            error: None,
        })),
        Err(e) => {
            tracing::debug!(provider_id = id, error = %e, "Script validation failed");
            Ok(Json(ValidateScriptResponse {
                valid: false,
                error: Some(sanitize_lua_error(&e)),
            }))
        }
    }
}

// ============================================================================
// Router
// ============================================================================

/// Create Admin API router
pub fn admin_router(state: Arc<AdminState>) -> Router {
    use crate::api::health::{health_router, provider_health_router};

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
        // Provider health check route (POST /admin/v1/providers/{id}/health)
        .nest("/providers", provider_health_router())
        // Provider script validation route
        .route("/providers/:id/validate-script", post(validate_script))
        // Credential routes
        .route(
            "/credentials",
            get(list_credentials).post(create_credential),
        )
        .route(
            "/credentials/:id",
            get(get_credential)
                .put(update_credential)
                .delete(delete_credential),
        )
        // Config routes
        .route("/config/version", get(get_config_version))
        .route("/config/reload", post(reload_config))
        // Health check routes
        .nest("/health", health_router())
        // Request logs routes (stats and batch-delete before :id to avoid path conflict)
        .route("/logs", get(list_logs))
        .route("/logs/stats", get(get_log_stats))
        .route("/logs/batch-delete", post(batch_delete_logs))
        .route("/logs/:id", get(get_log_detail).delete(delete_log))
        // Error logs routes
        .route("/error-logs", get(list_error_logs))
        .route("/error-logs/batch-delete", post(batch_delete_error_logs))
        .route(
            "/error-logs/:id",
            get(get_error_log_detail).delete(delete_error_log),
        )
        .with_state(state)
}
