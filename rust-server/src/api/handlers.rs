//! HTTP request handlers for the LLM proxy API.
//!
//! This module contains all endpoint handlers including chat completions,
//! health checks, model listings, and metrics.

use crate::api::models::*;
use crate::api::streaming::{create_sse_stream, rewrite_model_in_response};
use crate::core::{AppError, Result};
use crate::core::logging::{PROVIDER_CONTEXT, REQUEST_ID, generate_request_id};
use crate::core::metrics::get_metrics;
use crate::services::ProviderService;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use prometheus::{Encoder, TextEncoder};
use std::error::Error;
use std::sync::Arc;
use std::time::Instant;

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub config: crate::core::config::AppConfig,
    pub provider_service: ProviderService,
}

/// Verify API key authentication.
///
/// Checks the Authorization header against the configured master API key.
fn verify_auth(headers: &HeaderMap, config: &crate::core::config::AppConfig) -> Result<()> {
    if let Some(master_key) = &config.server.master_api_key {
        if let Some(auth_header) = headers.get("authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                if auth_str.starts_with("Bearer ") {
                    let provided_key = &auth_str[7..];
                    if provided_key == master_key {
                        return Ok(());
                    }
                }
            }
        }
        return Err(AppError::Unauthorized);
    }
    Ok(())
}

/// Handle chat completion requests.
///
/// Supports both streaming and non-streaming responses.
#[tracing::instrument(
    skip(state, headers, payload),
    fields(
        model = %payload.model,
        stream = payload.stream.unwrap_or(false),
    )
)]
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut payload): Json<ChatCompletionRequest>,
) -> Result<Response> {
    let request_id = generate_request_id();
    
    REQUEST_ID.scope(request_id.clone(), async move {
        verify_auth(&headers, &state.config)?;

        let original_model = payload.model.clone();
        
        // Select provider based on the requested model
        let provider = state
            .provider_service
            .get_next_provider(Some(&original_model))
            .map_err(AppError::Internal)?;

        // Map model if needed
        if let Some(mapped_model) = provider.model_mapping.get(&payload.model) {
            payload.model = mapped_model.clone();
        }

        let url = format!("{}/chat/completions", provider.api_base);
        let is_stream = payload.stream.unwrap_or(false);

        // Execute request within provider context scope
        PROVIDER_CONTEXT
            .scope(provider.name.clone(), async move {
                tracing::debug!(
                    request_id = %request_id,
                    provider = %provider.name,
                    model = %original_model,
                    stream = is_stream,
                    "Processing chat completion request"
                );

            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(!state.config.verify_ssl)
                .timeout(std::time::Duration::from_secs(300))
                .build()?;

            let response = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", provider.api_key))
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await
                .map_err(|e| {
                    tracing::error!(
                        request_id = %request_id,
                        provider = %provider.name,
                        url = %url,
                        model = %original_model,
                        error = %e,
                        error_source = ?e.source(),
                        is_timeout = e.is_timeout(),
                        is_connect = e.is_connect(),
                        "HTTP request failed to provider"
                    );
                    AppError::from(e)
                })?;

            tracing::debug!(
                request_id = %request_id,
                provider = %provider.name,
                url = %url,
                status = %response.status(),
                method = "POST",
                "HTTP request completed"
            );

            if is_stream {
                let sse_stream = create_sse_stream(response, original_model, provider.name).await;
                Ok(sse_stream.into_response())
            } else {
                let response_data: serde_json::Value = response.json().await?;

                // Record token usage
                if let Some(usage_obj) = response_data.get("usage") {
                    if let Ok(usage) = serde_json::from_value::<Usage>(usage_obj.clone()) {
                        record_token_usage(&usage, &original_model, &provider.name);
                    }
                }

                let rewritten = rewrite_model_in_response(response_data, &original_model);
                Ok(Json(rewritten).into_response())
            }
        })
        .await
    }).await
}

/// Handle legacy completions endpoint.
#[tracing::instrument(
    skip(state, headers, payload),
    fields(
        model = payload.get("model").and_then(|m| m.as_str()).unwrap_or("unknown"),
    )
)]
pub async fn completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Result<Response> {
    let request_id = generate_request_id();
    
    REQUEST_ID.scope(request_id.clone(), async move {
        verify_auth(&headers, &state.config)?;

        // Extract model from payload if available
        let model = payload.get("model").and_then(|m| m.as_str());
        let model_str = model.map(|m| m.to_string());
        
        let provider = state
            .provider_service
            .get_next_provider(model)
            .map_err(AppError::Internal)?;
        
        let url = format!("{}/completions", provider.api_base);

        // Execute request within provider context scope
        PROVIDER_CONTEXT
            .scope(provider.name.clone(), async move {
                tracing::debug!(
                    request_id = %request_id,
                    provider = %provider.name,
                    "Processing completions request"
                );

            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(!state.config.verify_ssl)
                .timeout(std::time::Duration::from_secs(300))
                .build()?;

            let response = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", provider.api_key))
                .header("Content-Type", "application/json")
                .json(&payload)
                .send()
                .await
                .map_err(|e| {
                    tracing::error!(
                        request_id = %request_id,
                        provider = %provider.name,
                        url = %url,
                        model = ?model_str,
                        error = %e,
                        error_source = ?e.source(),
                        is_timeout = e.is_timeout(),
                        is_connect = e.is_connect(),
                        "HTTP request failed to provider"
                    );
                    AppError::from(e)
                })?;

            tracing::debug!(
                request_id = %request_id,
                provider = %provider.name,
                url = %url,
                status = %response.status(),
                method = "POST",
                "HTTP request completed"
            );

            let response_data: serde_json::Value = response.json().await?;
            Ok(Json(response_data).into_response())
        })
        .await
    }).await
}

/// List available models.
#[tracing::instrument(skip(state, headers))]
pub async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ModelList>> {
    let request_id = generate_request_id();
    
    REQUEST_ID.scope(request_id.clone(), async move {
        verify_auth(&headers, &state.config)?;

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
    }).await
}

/// Basic health check endpoint.
#[tracing::instrument(skip(state))]
pub async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let request_id = generate_request_id();
    
    REQUEST_ID.scope(request_id.clone(), async move {
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
    }).await
}

/// Detailed health check with provider testing.
/// Tests all providers concurrently, but models serially within each provider.
#[tracing::instrument(skip(state))]
pub async fn health_detailed(
    State(state): State<Arc<AppState>>,
) -> Json<DetailedHealthResponse> {
    let request_id = generate_request_id();
    
    REQUEST_ID.scope(request_id.clone(), async move {
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
            async move {
                test_provider(provider, config).await
            }
        })
        .collect();
    
    let results = futures::future::join_all(tasks).await;
    
    let mut providers_map = std::collections::HashMap::new();
    for (name, health) in results {
        providers_map.insert(name, health);
    }
    
        Json(DetailedHealthResponse { providers: providers_map })
    }).await
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
    
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(30))
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