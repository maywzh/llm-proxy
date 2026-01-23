//! Health check API models and handlers.
//!
//! Provides endpoints for checking provider health status by making
//! actual API calls with minimal token usage.

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::ToSchema;

use crate::api::admin::{verify_admin_auth, AdminError, AdminState};
use crate::api::models::{CheckProviderHealthRequest, CheckProviderHealthResponse};
use crate::services::health_check_service::{check_providers_health, HealthCheckService};

/// Health status enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// Provider is healthy and responding
    Healthy,
    /// Provider is not responding or returning errors
    Unhealthy,
    /// Provider is disabled
    Disabled,
    /// Health status is unknown
    Unknown,
}

/// Single model health status
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "model": "gpt-4",
    "status": "healthy",
    "response_time_ms": 1234,
    "error": null
}))]
pub struct ModelHealthStatus {
    /// Model name
    pub model: String,
    /// Health status
    pub status: HealthStatus,
    /// Response time in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<i32>,
    /// Error message if unhealthy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Provider health status
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "provider_id": 1,
    "provider_key": "openai-primary",
    "status": "healthy",
    "models": [{
        "model": "gpt-4",
        "status": "healthy",
        "response_time_ms": 1234,
        "error": null
    }],
    "avg_response_time_ms": 1234,
    "checked_at": "2024-01-01T00:00:00Z"
}))]
pub struct ProviderHealthStatus {
    /// Provider ID
    pub provider_id: i32,
    /// Provider key
    pub provider_key: String,
    /// Overall provider health status
    pub status: HealthStatus,
    /// Health status of each model
    pub models: Vec<ModelHealthStatus>,
    /// Average response time across all models
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_response_time_ms: Option<i32>,
    /// Timestamp when health check was performed
    pub checked_at: String,
}

/// Health check request
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[schema(example = json!({
    "provider_ids": [1, 2],
    "models": ["gpt-4", "gpt-3.5-turbo"],
    "timeout_secs": 10,
    "max_concurrent": 2
}))]
pub struct HealthCheckRequest {
    /// Specific provider IDs to check (empty = all providers)
    #[serde(default)]
    pub provider_ids: Option<Vec<i32>>,
    /// Specific models to test (empty = default test models)
    #[serde(default)]
    pub models: Option<Vec<String>>,
    /// Timeout for each model test in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Maximum number of providers to check concurrently (default: 2)
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,
}

fn default_timeout() -> u64 {
    30
}

fn default_max_concurrent() -> usize {
    2
}

impl Default for HealthCheckRequest {
    fn default() -> Self {
        Self {
            provider_ids: None,
            models: None,
            timeout_secs: 30,
            max_concurrent: 2,
        }
    }
}

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "providers": [{
        "provider_id": 1,
        "provider_key": "openai-primary",
        "status": "healthy",
        "models": [{
            "model": "gpt-4",
            "status": "healthy",
            "response_time_ms": 1234,
            "error": null
        }],
        "avg_response_time_ms": 1234,
        "checked_at": "2024-01-01T00:00:00Z"
    }],
    "total_providers": 1,
    "healthy_providers": 1,
    "unhealthy_providers": 0
}))]
pub struct HealthCheckResponse {
    /// Health status of each provider
    pub providers: Vec<ProviderHealthStatus>,
    /// Total number of providers checked
    pub total_providers: usize,
    /// Number of healthy providers
    pub healthy_providers: usize,
    /// Number of unhealthy providers
    pub unhealthy_providers: usize,
}

/// Query parameters for get provider health endpoint
#[derive(Debug, Deserialize)]
pub struct GetProviderHealthQuery {
    /// Comma-separated list of models to test (optional)
    pub models: Option<String>,
    /// Timeout for each model test in seconds (default: 10)
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

/// Check provider health
///
/// Check health status of all or specific providers by testing their models.
/// This endpoint tests provider availability by making actual API calls
/// with minimal token usage (max_tokens=5) to reduce costs.
#[utoipa::path(
    post,
    path = "/admin/v1/health/check",
    tag = "health",
    request_body = HealthCheckRequest,
    responses(
        (status = 200, description = "Health check completed", body = HealthCheckResponse),
        (status = 401, description = "Unauthorized", body = crate::api::admin::AdminErrorResponse)
    )
)]
pub async fn check_health(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Json(request): Json<HealthCheckRequest>,
) -> Result<Json<HealthCheckResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    tracing::info!(
        provider_ids = ?request.provider_ids,
        models = ?request.models,
        timeout_secs = request.timeout_secs,
        max_concurrent = request.max_concurrent,
        "Health check requested"
    );

    // Check providers health
    let health_statuses = check_providers_health(
        state.dynamic_config.database(),
        &state.http_client,
        request.provider_ids,
        request.models,
        request.timeout_secs,
        request.max_concurrent,
    )
    .await;

    // Calculate summary statistics
    let total_providers = health_statuses.len();
    let healthy_providers = health_statuses
        .iter()
        .filter(|p| p.status == HealthStatus::Healthy)
        .count();
    let unhealthy_providers = health_statuses
        .iter()
        .filter(|p| p.status == HealthStatus::Unhealthy)
        .count();

    tracing::info!(
        total = total_providers,
        healthy = healthy_providers,
        unhealthy = unhealthy_providers,
        "Health check completed"
    );

    Ok(Json(HealthCheckResponse {
        providers: health_statuses,
        total_providers,
        healthy_providers,
        unhealthy_providers,
    }))
}

/// Get provider health status
///
/// Get health status of a specific provider by testing its models.
#[utoipa::path(
    get,
    path = "/admin/v1/health/providers/{provider_id}",
    tag = "health",
    params(
        ("provider_id" = i32, Path, description = "Provider ID"),
        ("models" = Option<String>, Query, description = "Comma-separated list of models to test"),
        ("timeout_secs" = u64, Query, description = "Timeout for each model test in seconds (default: 10)")
    ),
    responses(
        (status = 200, description = "Provider health status", body = ProviderHealthStatus),
        (status = 401, description = "Unauthorized", body = crate::api::admin::AdminErrorResponse),
        (status = 404, description = "Provider not found", body = crate::api::admin::AdminErrorResponse)
    )
)]
pub async fn get_provider_health(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(provider_id): Path<i32>,
    Query(query): Query<GetProviderHealthQuery>,
) -> Result<Json<ProviderHealthStatus>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    // Get provider to verify it exists
    let db = state.dynamic_config.database();
    let _provider = db
        .get_provider(provider_id)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Provider with ID {} not found", provider_id)))?;

    // Parse models parameter
    let model_list = query.models.as_ref().map(|m| {
        m.split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
    });

    tracing::info!(
        provider_id = provider_id,
        models = ?model_list,
        timeout_secs = query.timeout_secs,
        "Health check requested for provider"
    );

    // Check provider health (use default max_concurrent=2 for single provider)
    let health_statuses = check_providers_health(
        db,
        &state.http_client,
        Some(vec![provider_id]),
        model_list,
        query.timeout_secs,
        2, // max_concurrent (doesn't matter for single provider)
    )
    .await;

    if health_statuses.is_empty() {
        return Err(AdminError::Internal(
            "Failed to check provider health".to_string(),
        ));
    }

    Ok(Json(health_statuses[0].clone()))
}

/// Check provider health with concurrent model testing
///
/// Check health status of a specific provider by testing all its mapped models
/// with configurable concurrency control. This endpoint makes actual API calls
/// with minimal token usage (max_tokens=5) to verify model availability.
#[utoipa::path(
    post,
    path = "/admin/v1/providers/{provider_id}/health",
    tag = "health",
    params(
        ("provider_id" = i32, Path, description = "Provider ID")
    ),
    request_body = CheckProviderHealthRequest,
    responses(
        (status = 200, description = "Health check completed", body = CheckProviderHealthResponse),
        (status = 400, description = "Invalid request parameters", body = crate::api::admin::AdminErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::api::admin::AdminErrorResponse),
        (status = 404, description = "Provider not found", body = crate::api::admin::AdminErrorResponse)
    )
)]
pub async fn check_provider_health_concurrent(
    State(state): State<Arc<AdminState>>,
    headers: HeaderMap,
    Path(provider_id): Path<i32>,
    Json(request): Json<CheckProviderHealthRequest>,
) -> Result<Json<CheckProviderHealthResponse>, AdminError> {
    verify_admin_auth(&headers, &state.admin_key)?;

    // Get provider to verify it exists
    let db = state.dynamic_config.database();
    let provider = db
        .get_provider(provider_id)
        .await?
        .ok_or_else(|| AdminError::NotFound(format!("Provider with ID {} not found", provider_id)))?;

    // Validate request parameters
    if request.max_concurrent < 1 || request.max_concurrent > 10 {
        return Err(AdminError::BadRequest(
            "max_concurrent must be between 1 and 10".to_string(),
        ));
    }
    if request.timeout_secs < 1 || request.timeout_secs > 120 {
        return Err(AdminError::BadRequest(
            "timeout_secs must be between 1 and 120".to_string(),
        ));
    }

    tracing::info!(
        provider_id = provider_id,
        models = ?request.models,
        max_concurrent = request.max_concurrent,
        timeout_secs = request.timeout_secs,
        "Concurrent health check requested for provider"
    );

    // Create service and check provider health
    let service = HealthCheckService::new(state.http_client.clone(), request.timeout_secs);
    let result = service
        .check_provider_health_concurrent(&provider, request.models, request.max_concurrent)
        .await;

    tracing::info!(
        provider_id = provider_id,
        healthy_models = result.summary.healthy_models,
        total_models = result.summary.total_models,
        "Concurrent health check completed for provider"
    );

    Ok(Json(result))
}

/// Create health check router
pub fn health_router() -> Router<Arc<AdminState>> {
    Router::new()
        .route("/check", post(check_health))
        .route("/providers/:provider_id", get(get_provider_health))
}

/// Create provider health router (for /admin/v1/providers/{id}/health endpoint)
pub fn provider_health_router() -> Router<Arc<AdminState>> {
    Router::new().route("/:provider_id/health", post(check_provider_health_concurrent))
}