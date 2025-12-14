//! HTTP request handlers for the LLM proxy API.
//!
//! This module contains all endpoint handlers including chat completions,
//! health checks, model listings, and metrics.

use crate::api::models::*;
use crate::api::streaming::{create_sse_stream, rewrite_model_in_response};
use crate::core::{AppError, Result};
use crate::core::metrics::get_metrics;
use crate::services::ProviderService;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use prometheus::{Encoder, TextEncoder};
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
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut payload): Json<ChatCompletionRequest>,
) -> Result<Response> {
    verify_auth(&headers, &state.config)?;

    let provider = state.provider_service.get_next_provider();
    let original_model = payload.model.clone();

    // Map model if needed
    if let Some(mapped_model) = provider.model_mapping.get(&payload.model) {
        payload.model = mapped_model.clone();
    }

    let url = format!("{}/chat/completions", provider.api_base);
    let is_stream = payload.stream.unwrap_or(false);

    tracing::debug!(
        "Request to {} for model {} (stream: {})",
        provider.name,
        original_model,
        is_stream
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
        .await?;

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
}

/// Handle legacy completions endpoint.
pub async fn completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Result<Response> {
    verify_auth(&headers, &state.config)?;

    let provider = state.provider_service.get_next_provider();
    let url = format!("{}/completions", provider.api_base);

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
        .await?;

    let response_data: serde_json::Value = response.json().await?;
    Ok(Json(response_data).into_response())
}

/// List available models.
pub async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ModelList>> {
    verify_auth(&headers, &state.config)?;

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
}

/// Basic health check endpoint.
pub async fn health(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
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
}

/// Detailed health check with provider testing.
pub async fn health_detailed(
    State(state): State<Arc<AppState>>,
) -> Json<DetailedHealthResponse> {
    let providers = state.provider_service.get_all_providers();
    let mut results = std::collections::HashMap::new();

    for provider in providers {
        let start = Instant::now();

        if provider.model_mapping.is_empty() {
            results.insert(
                provider.name.clone(),
                ProviderHealth {
                    status: "error".to_string(),
                    latency: Some("0ms".to_string()),
                    tested_model: None,
                    error: Some("no models configured".to_string()),
                },
            );
            continue;
        }

        let model_name = provider.model_mapping.keys().next().unwrap().clone();
        let actual_model = provider.model_mapping.get(&model_name).unwrap().clone();

        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(!state.config.verify_ssl)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

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
                    results.insert(
                        provider.name.clone(),
                        ProviderHealth {
                            status: "ok".to_string(),
                            latency: Some(format!("{}ms", latency_ms)),
                            tested_model: Some(model_name),
                            error: None,
                        },
                    );
                } else {
                    results.insert(
                        provider.name.clone(),
                        ProviderHealth {
                            status: "error".to_string(),
                            latency: Some(format!("{}ms", latency_ms)),
                            tested_model: None,
                            error: Some(format!("HTTP {}", response.status())),
                        },
                    );
                }
            }
            Err(e) => {
                let latency_ms = start.elapsed().as_millis();
                results.insert(
                    provider.name.clone(),
                    ProviderHealth {
                        status: "error".to_string(),
                        latency: Some(format!("{}ms", latency_ms)),
                        tested_model: None,
                        error: Some(e.to_string().chars().take(100).collect()),
                    },
                );
            }
        }
    }

    Json(DetailedHealthResponse { providers: results })
}

/// Prometheus metrics endpoint.
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

    tracing::debug!(
        "Token usage - model={} provider={} prompt={} completion={} total={}",
        model,
        provider,
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.total_tokens
    );
}