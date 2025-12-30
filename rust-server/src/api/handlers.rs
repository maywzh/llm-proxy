//! HTTP request handlers for the LLM proxy API.
//!
//! This module contains all endpoint handlers including chat completions,
//! health checks, model listings, and metrics.

use crate::api::models::*;
use crate::api::streaming::{
    calculate_message_tokens, create_sse_stream, rewrite_model_in_response,
};
use crate::core::logging::{generate_request_id, PROVIDER_CONTEXT, REQUEST_ID};
use crate::core::metrics::get_metrics;
use crate::core::middleware::{ModelName, ProviderName};
use crate::core::{AppError, RateLimiter, Result};
use crate::services::ProviderService;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use prometheus::{Encoder, TextEncoder};
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub config: crate::core::config::AppConfig,
    pub provider_service: ProviderService,
    pub rate_limiter: Arc<RateLimiter>,
    pub http_client: reqwest::Client,
}

/// Verify API key authentication and check rate limits.
///
/// Checks the Authorization header against configured master keys.
/// If a master key is found, also enforces rate limiting if configured.
fn verify_auth(headers: &HeaderMap, state: &AppState) -> Result<()> {
    // Extract the provided key from Authorization header
    let provided_key = if let Some(auth_header) = headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if auth_str.starts_with("Bearer ") {
                Some(&auth_str[7..])
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Check if any authentication is required
    if state.config.master_keys.is_empty() {
        return Ok(());
    }

    let provided_key = provided_key.ok_or(AppError::Unauthorized)?;

    // Check against master_keys configuration
    for key_config in &state.config.master_keys {
        if key_config.enabled && key_config.key == provided_key {
            // Check rate limit for this key
            state.rate_limiter.check_rate_limit(&key_config.key)?;

            tracing::debug!(
                key_name = %key_config.name,
                "Request authenticated with master key"
            );
            return Ok(());
        }
    }

    Err(AppError::Unauthorized)
}

/// Handle chat completion requests.
///
/// Supports both streaming and non-streaming responses.
#[tracing::instrument(skip(state, headers, payload))]
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
) -> Result<Response> {
    let request_id = generate_request_id();

    REQUEST_ID
        .scope(request_id.clone(), async move {
            verify_auth(&headers, &state)?;

            if !payload.is_object() {
                return Err(AppError::BadRequest(
                    "Request body must be a JSON object".to_string(),
                ));
            }

            let original_model = payload
                .get("model")
                .and_then(|m| m.as_str())
                .map(|m| m.to_string());
            let model_label = original_model
                .clone()
                .unwrap_or_else(|| "unknown".to_string());

            // Select provider based on the requested model
            // Return 400 (Bad Request) if no provider available, not 500
            let provider = match state
                .provider_service
                .get_next_provider(original_model.as_deref())
            {
                Ok(p) => p,
                Err(err) => {
                    tracing::error!(
                        request_id = %request_id,
                        error = %err,
                        model = %model_label,
                        "Provider selection failed - no available provider for model"
                    );
                    return Err(AppError::BadRequest(err));
                }
            };

            // Map model if needed
            if let Some(orig) = original_model.as_ref() {
                if let Some(mapped) = provider.model_mapping.get(orig) {
                    if let Some(obj) = payload.as_object_mut() {
                        obj.insert(
                            "model".to_string(),
                            serde_json::Value::String(mapped.clone()),
                        );
                    }
                }
            }

            let url = format!("{}/chat/completions", provider.api_base);
            let is_stream = payload
                .get("stream")
                .and_then(|s| s.as_bool())
                .unwrap_or(false);

            let prompt_tokens_for_fallback = payload
                .get("messages")
                .and_then(|m| m.as_array())
                .map(|messages| {
                    let count_model = original_model.as_deref().unwrap_or("gpt-3.5-turbo");
                    calculate_message_tokens(messages, count_model)
                });

            // Execute request within provider context scope
            PROVIDER_CONTEXT
                .scope(provider.name.clone(), async move {
                    tracing::debug!(
                        request_id = %request_id,
                        provider = %provider.name,
                        model = %model_label,
                        stream = is_stream,
                        "Processing chat completion request"
                    );

                    // Create a response builder that will be used for all error cases
                    // This ensures model and provider are always set for metrics
                    let create_error_response = |status: StatusCode, message: String| -> Response {
                        build_error_response(status, message, &model_label, &provider.name)
                    };

                    let response = match state.http_client
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", provider.api_key))
                        .header("Content-Type", "application/json")
                        .json(&payload)
                        .send()
                        .await
                    {
                        Ok(resp) => resp,
                        Err(e) => {
                            tracing::error!(
                                request_id = %request_id,
                                provider = %provider.name,
                                url = %url,
                                model = %model_label,
                                error = %e,
                                is_timeout = e.is_timeout(),
                                is_connect = e.is_connect(),
                                "HTTP request failed to provider"
                            );
                            let status = if e.is_timeout() {
                                StatusCode::GATEWAY_TIMEOUT
                            } else {
                                StatusCode::BAD_GATEWAY
                            };
                            return Ok(create_error_response(
                                status,
                                format!("Upstream request failed: {}", e),
                            ));
                        }
                    };

                    let status = response.status();
                    tracing::debug!(
                        request_id = %request_id,
                        provider = %provider.name,
                        url = %url,
                        status = %status,
                        method = "POST",
                        "HTTP request completed"
                    );

                    // Check if backend API returned an error status code
                    // Faithfully pass through the backend error
                    if status.is_client_error() || status.is_server_error() {
                        let error_body = match response.bytes().await {
                            Ok(bytes) => {
                                // Try to parse as JSON first
                                match serde_json::from_slice::<serde_json::Value>(&bytes) {
                                    Ok(body) => body,
                                    Err(_) => {
                                        // If can't parse as JSON, create error object with text
                                        let text = String::from_utf8_lossy(&bytes).to_string();
                                        json!({
                                            "error": {
                                                "message": text,
                                                "type": "error",
                                                "code": status.as_u16()
                                            }
                                        })
                                    }
                                }
                            }
                            Err(_) => json!({
                                "error": {
                                    "message": format!("HTTP {}", status),
                                    "type": "error",
                                    "code": status.as_u16()
                                }
                            }),
                        };

                        tracing::error!(
                            request_id = %request_id,
                            provider = %provider.name,
                            status = %status,
                            "Backend API returned error status"
                        );

                        // Faithfully return the backend's status code and error body
                        let mut response = Json(error_body).into_response();
                        *response.status_mut() = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                        response.extensions_mut().insert(ModelName(model_label.clone()));
                        response.extensions_mut().insert(ProviderName(provider.name.clone()));
                        return Ok(response);
                    }

                    let mut final_response = if is_stream {
                        // For streaming, response is already checked for errors above
                        // Pass input tokens for fallback calculation
                        let sse_stream = create_sse_stream(
                            response,
                            model_label.clone(),
                            provider.name.clone(),
                            prompt_tokens_for_fallback,
                        )
                        .await;

                        sse_stream.into_response()
                    } else {
                        let response_data: serde_json::Value = match response.json().await {
                            Ok(data) => data,
                            Err(e) => {
                                tracing::error!(
                                    request_id = %request_id,
                                    provider = %provider.name,
                                    error = %e,
                                    "Failed to parse provider response JSON"
                                );
                                return Ok(create_error_response(
                                    StatusCode::BAD_GATEWAY,
                                    format!("Invalid JSON from provider: {}", e),
                                ));
                            }
                        };

                        // Record token usage
                        if let Some(usage_obj) = response_data.get("usage") {
                            if let Ok(usage) = serde_json::from_value::<Usage>(usage_obj.clone()) {
                                record_token_usage(&usage, &model_label, &provider.name);
                            }
                        }

                        let rewritten = rewrite_model_in_response(response_data, &model_label);
                        Json(rewritten).into_response()
                    };

                    // Add model and provider info to response extensions for middleware logging
                    final_response
                        .extensions_mut()
                        .insert(ModelName(model_label));
                    final_response
                        .extensions_mut()
                        .insert(ProviderName(provider.name));

                    Ok(final_response)
                })
                .await
        })
        .await
}

/// Handle legacy completions endpoint.
#[tracing::instrument(skip(state, headers, payload))]
pub async fn completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Result<Response> {
    let request_id = generate_request_id();

    REQUEST_ID
        .scope(request_id.clone(), async move {
            verify_auth(&headers, &state)?;

            if !payload.is_object() {
                return Err(AppError::BadRequest(
                    "Request body must be a JSON object".to_string(),
                ));
            }

            // Extract model from payload if available
            let model = payload.get("model").and_then(|m| m.as_str());
            let model_label = model.unwrap_or("unknown").to_string();

            let provider = match state.provider_service.get_next_provider(model) {
                Ok(p) => p,
                Err(err) => {
                    tracing::error!(
                        request_id = %request_id,
                        error = %err,
                        "Provider selection failed"
                    );
                    return Err(AppError::BadRequest(err));
                }
            };

            let url = format!("{}/completions", provider.api_base);

            // Execute request within provider context scope
            PROVIDER_CONTEXT
                .scope(provider.name.clone(), async move {
                    tracing::debug!(
                        request_id = %request_id,
                        provider = %provider.name,
                        "Processing completions request"
                    );

                    let response = match state.http_client
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", provider.api_key))
                        .header("Content-Type", "application/json")
                        .json(&payload)
                        .send()
                        .await
                    {
                        Ok(resp) => resp,
                        Err(e) => {
                            tracing::error!(
                                request_id = %request_id,
                                provider = %provider.name,
                                url = %url,
                                model = %model_label,
                                error = %e,
                                is_timeout = e.is_timeout(),
                                is_connect = e.is_connect(),
                                "HTTP request failed to provider"
                            );
                            let status = if e.is_timeout() {
                                StatusCode::GATEWAY_TIMEOUT
                            } else {
                                StatusCode::BAD_GATEWAY
                            };
                            return Ok(build_error_response(
                                status,
                                format!("Upstream request failed: {}", e),
                                &model_label,
                                &provider.name,
                            ));
                        }
                    };

                    let status = response.status();
                    tracing::debug!(
                        request_id = %request_id,
                        provider = %provider.name,
                        url = %url,
                        status = %status,
                        method = "POST",
                        "HTTP request completed"
                    );

                    // Check if backend API returned an error status code
                    // Faithfully pass through the backend error
                    if status.is_client_error() || status.is_server_error() {
                        let error_body = match response.bytes().await {
                            Ok(bytes) => {
                                // Try to parse as JSON first
                                match serde_json::from_slice::<serde_json::Value>(&bytes) {
                                    Ok(body) => body,
                                    Err(_) => {
                                        // If can't parse as JSON, create error object with text
                                        let text = String::from_utf8_lossy(&bytes).to_string();
                                        json!({
                                            "error": {
                                                "message": text,
                                                "type": "error",
                                                "code": status.as_u16()
                                            }
                                        })
                                    }
                                }
                            }
                            Err(_) => json!({
                                "error": {
                                    "message": format!("HTTP {}", status),
                                    "type": "error",
                                    "code": status.as_u16()
                                }
                            }),
                        };

                        tracing::error!(
                            request_id = %request_id,
                            provider = %provider.name,
                            status = %status,
                            "Backend API returned error status"
                        );

                        // Faithfully return the backend's status code and error body
                        let mut response = Json(error_body).into_response();
                        *response.status_mut() = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                        response.extensions_mut().insert(ModelName(model_label.clone()));
                        response.extensions_mut().insert(ProviderName(provider.name.clone()));
                        return Ok(response);
                    }

                    let response_data: serde_json::Value = match response.json().await {
                        Ok(data) => data,
                        Err(e) => {
                            tracing::error!(
                                request_id = %request_id,
                                provider = %provider.name,
                                error = %e,
                                "Failed to parse provider response JSON"
                            );
                            return Ok(build_error_response(
                                StatusCode::BAD_GATEWAY,
                                format!("Invalid JSON from provider: {}", e),
                                &model_label,
                                &provider.name,
                            ));
                        }
                    };
                    let mut final_response = Json(response_data).into_response();

                    // Add model and provider info to response extensions for middleware logging
                    final_response
                        .extensions_mut()
                        .insert(ModelName(model_label));
                    final_response
                        .extensions_mut()
                        .insert(ProviderName(provider.name));

                    Ok(final_response)
                })
                .await
        })
        .await
}

/// List available models.
#[tracing::instrument(skip(state, headers))]
pub async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ModelList>> {
    let request_id = generate_request_id();

    REQUEST_ID
        .scope(request_id.clone(), async move {
            verify_auth(&headers, &state)?;

            tracing::debug!(
                request_id = %request_id,
                "Listing available models"
            );

            let models = state.provider_service.get_all_models();
            let mut model_list: Vec<ModelInfo> = models
                .into_iter()
                .map(|model| ModelInfo {
                    id: model,
                    object: "model".to_string(),
                    created: 1677610602,
                    owned_by: "system".to_string(),
                })
                .collect();

            model_list.sort_by(|a, b| a.id.cmp(&b.id));

            Ok(Json(ModelList {
                object: "list".to_string(),
                data: model_list,
            }))
        })
        .await
}

/// Basic health check endpoint.
#[tracing::instrument(skip(state))]
pub async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let request_id = generate_request_id();

    REQUEST_ID
        .scope(request_id.clone(), async move {
            tracing::debug!(
                request_id = %request_id,
                "Health check requested"
            );

            let providers = state.provider_service.get_all_providers();
            let weights = state.provider_service.get_provider_weights();
            let total_weight: u32 = weights.iter().sum();

            let provider_info = providers
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let weight = weights[i];
                    let probability = (weight as f64 / total_weight as f64) * 100.0;
                    ProviderInfo {
                        name: p.name.clone(),
                        weight,
                        probability: format!("{:.1}%", probability),
                    }
                })
                .collect();

            Json(HealthResponse {
                status: "ok".to_string(),
                providers: providers.len(),
                provider_info,
            })
        })
        .await
}

/// Detailed health check with provider testing.
/// Tests all providers concurrently, but models serially within each provider.
#[tracing::instrument(skip(state))]
pub async fn health_detailed(State(state): State<Arc<AppState>>) -> Json<DetailedHealthResponse> {
    let request_id = generate_request_id();

    REQUEST_ID
        .scope(request_id.clone(), async move {
            tracing::debug!(
                request_id = %request_id,
                "Detailed health check requested"
            );

            let providers = state.provider_service.get_all_providers();

            // Test all providers concurrently
            let tasks: Vec<_> = providers
                .into_iter()
                .map(|provider| {
                    let config = state.config.clone();
                    async move { test_provider(provider, config).await }
                })
                .collect();

            let results = futures::future::join_all(tasks).await;

            let mut providers_map = std::collections::HashMap::new();
            for (name, health) in results {
                providers_map.insert(name, health);
            }

            Json(DetailedHealthResponse {
                providers: providers_map,
            })
        })
        .await
}

/// Test a single provider by checking all its models serially.
#[tracing::instrument(
    skip(provider, config),
    fields(
        provider_name = %provider.name,
        models_count = provider.model_mapping.len(),
    )
)]
async fn test_provider(
    provider: crate::api::models::Provider,
    config: crate::core::config::AppConfig,
) -> (String, crate::api::models::ProviderHealth) {
    use crate::api::models::{ModelHealth, ProviderHealth};

    if provider.model_mapping.is_empty() {
        return (
            provider.name.clone(),
            ProviderHealth {
                status: "error".to_string(),
                error: Some("no models configured".to_string()),
                models: vec![],
            },
        );
    }

    // Create a dedicated client for health checks with shorter timeout
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(5)
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();

    let mut model_results = Vec::new();

    // Test models serially within this provider
    for (model_name, actual_model) in &provider.model_mapping {
        let start = Instant::now();

        let test_payload = serde_json::json!({
            "model": actual_model,
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 10
        });

        match client
            .post(format!("{}/chat/completions", provider.api_base))
            .header("Authorization", format!("Bearer {}", provider.api_key))
            .header("Content-Type", "application/json")
            .json(&test_payload)
            .send()
            .await
        {
            Ok(response) => {
                let latency_ms = start.elapsed().as_millis();
                if response.status().is_success() {
                    model_results.push(ModelHealth {
                        model: model_name.clone(),
                        status: "ok".to_string(),
                        latency: format!("{}ms", latency_ms),
                        error: None,
                    });
                } else {
                    model_results.push(ModelHealth {
                        model: model_name.clone(),
                        status: "error".to_string(),
                        latency: format!("{}ms", latency_ms),
                        error: Some(format!("HTTP {}", response.status())),
                    });
                }
            }
            Err(e) => {
                let latency_ms = start.elapsed().as_millis();
                model_results.push(ModelHealth {
                    model: model_name.clone(),
                    status: "error".to_string(),
                    latency: format!("{}ms", latency_ms),
                    error: Some(e.to_string().chars().take(100).collect()),
                });
            }
        }
    }

    // Determine overall provider status
    let all_ok = model_results.iter().all(|m| m.status == "ok");
    let any_ok = model_results.iter().any(|m| m.status == "ok");

    let provider_status = if all_ok {
        "ok"
    } else if any_ok {
        "partial"
    } else {
        "error"
    };

    (
        provider.name.clone(),
        ProviderHealth {
            status: provider_status.to_string(),
            error: None,
            models: model_results,
        },
    )
}

/// Prometheus metrics endpoint.
#[tracing::instrument]
pub async fn metrics_handler() -> Result<Response> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();

    encoder
        .encode(&metric_families, &mut buffer)
        .map_err(|e| AppError::Internal(e.to_string()))?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", encoder.format_type())
        .body(buffer.into())
        .unwrap())
}

/// Record token usage metrics.
#[tracing::instrument(
    skip(usage),
    fields(
        model = %model,
        provider = %provider,
        prompt_tokens = usage.prompt_tokens,
        completion_tokens = usage.completion_tokens,
        total_tokens = usage.total_tokens,
    )
)]
fn record_token_usage(usage: &Usage, model: &str, provider: &str) {
    let metrics = get_metrics();

    metrics
        .token_usage
        .with_label_values(&[model, provider, "prompt"])
        .inc_by(usage.prompt_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "completion"])
        .inc_by(usage.completion_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "total"])
        .inc_by(usage.total_tokens as u64);

    let request_id = crate::core::logging::get_request_id();
    tracing::debug!(
        request_id = %request_id,
        model = %model,
        provider = %provider,
        prompt_tokens = usage.prompt_tokens,
        completion_tokens = usage.completion_tokens,
        total_tokens = usage.total_tokens,
        "Token usage recorded"
    );
}

fn build_error_response(
    status: StatusCode,
    message: String,
    model: &str,
    provider: &str,
) -> Response {
    let body = json!({
        "error": {
            "message": message,
            "type": "error",
            "code": status.as_u16()
        }
    });

    let mut response = Json(body).into_response();
    *response.status_mut() = status;
    response
        .extensions_mut()
        .insert(ModelName(model.to_string()));
    response
        .extensions_mut()
        .insert(ProviderName(provider.to_string()));
    response
}

