//! Admin API handlers for dynamic configuration management.
//!
//! Provides RESTful endpoints for managing providers and credentials.
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
    create_key_preview, CreateCredential, CreateProvider, DynamicConfig,
    CredentialEntity, ProviderEntity, UpdateCredential, UpdateProvider,
};

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
    /// Model name mapping (request model -> provider model)
    pub model_mapping: std::collections::HashMap<String, String>,
    /// Weight for load balancing (higher = more traffic)
    pub weight: i32,
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
            provider_key: e.provider_key,
            provider_type: e.provider_type,
            api_base: e.api_base,
            model_mapping: e.model_mapping.0,
            weight: e.weight,
            is_enabled: e.is_enabled,
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
    "is_enabled": true
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
    /// Model name mapping (request model -> provider model)
    #[serde(default)]
    pub model_mapping: std::collections::HashMap<String, String>,
    /// Weight for load balancing (default: 1, higher = more traffic)
    #[serde(default = "default_weight")]
    pub weight: i32,
    /// Whether this provider is enabled (default: true)
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}

/// Request to update an existing provider
#[derive(Debug, Deserialize, ToSchema)]
#[schema(example = json!({
    "api_base": "https://api.openai.com/v1",
    "weight": 2,
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
    /// Weight for load balancing (higher = more traffic)
    pub weight: Option<i32>,
    /// Whether this provider is enabled
    pub is_enabled: Option<bool>,
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
        return Err(AdminError::BadRequest("Provider key is required".to_string()));
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
        return Err(AdminError::BadRequest("Credential name is required".to_string()));
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
        // Credential routes
        .route("/credentials", get(list_credentials).post(create_credential))
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
        .with_state(state)
}