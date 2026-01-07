//! HTTP request handlers for the LLM proxy API.
//!
//! This module contains all endpoint handlers including chat completions,
//! model listings, and metrics.

use crate::api::models::*;
use crate::api::streaming::{
    calculate_message_tokens, create_sse_stream, rewrite_model_in_response,
};
use crate::core::config::CredentialConfig;
use crate::core::database::hash_key;
use crate::core::logging::{generate_request_id, get_api_key_name, PROVIDER_CONTEXT, REQUEST_ID, API_KEY_NAME};
use crate::core::metrics::get_metrics;
use crate::core::middleware::{ApiKeyName, ModelName, ProviderName};
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
use std::collections::HashSet;
use std::sync::Arc;

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
/// Checks the Authorization header against configured credentials.
/// If a credential is found, also enforces rate limiting if configured.
///
/// Returns the full CredentialConfig for the authenticated credential, or None if no auth required.
fn verify_auth(headers: &HeaderMap, state: &AppState) -> Result<Option<CredentialConfig>> {
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
    if state.config.credentials.is_empty() {
        return Ok(None);
    }

    let provided_key = provided_key.ok_or(AppError::Unauthorized)?;

    // Hash the provided key for comparison with stored hashes
    let provided_key_hash = hash_key(provided_key);

    // Check against credentials configuration
    for credential_config in &state.config.credentials {
        if credential_config.enabled && credential_config.credential_key == provided_key_hash {
            // Check rate limit for this credential using the hash
            state.rate_limiter.check_rate_limit(&credential_config.credential_key)?;

            tracing::debug!(
                credential_name = %credential_config.name,
                "Request authenticated with credential"
            );
            return Ok(Some(credential_config.clone()));
        }
    }

    Err(AppError::Unauthorized)
}

/// Get the key name from an optional CredentialConfig
fn get_key_name(key_config: &Option<CredentialConfig>) -> String {
    key_config
        .as_ref()
        .map(|k| k.name.clone())
        .unwrap_or_else(|| "anonymous".to_string())
}

/// Strip the provider suffix from model name if configured.
///
/// If provider_suffix is set (e.g., "Proxy"), then:
/// - "Proxy/gpt-4" -> "gpt-4"
/// - "gpt-4" -> "gpt-4" (unchanged)
/// - "Other/gpt-4" -> "Other/gpt-4" (unchanged, different prefix)
fn strip_provider_suffix(model: &str, provider_suffix: Option<&str>) -> String {
    match provider_suffix {
        Some(suffix) if !suffix.is_empty() => {
            let prefix = format!("{}/", suffix);
            if model.starts_with(&prefix) {
                model[prefix.len()..].to_string()
            } else {
                model.to_string()
            }
        }
        _ => model.to_string(),
    }
}

/// Handle chat completion requests.
///
/// Supports both streaming and non-streaming responses.
/// This endpoint is compatible with the OpenAI Chat Completions API.
#[utoipa::path(
    post,
    path = "/v1/chat/completions",
    tag = "completions",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Chat completion response", body = ChatCompletionResponse),
        (status = 400, description = "Bad request", body = ApiErrorResponse),
        (status = 401, description = "Unauthorized", body = ApiErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = ApiErrorResponse),
        (status = 502, description = "Bad gateway - upstream error", body = ApiErrorResponse),
        (status = 504, description = "Gateway timeout", body = ApiErrorResponse)
    ),
    security(
        ("bearer_auth" = [])
    )
)]
#[tracing::instrument(skip(state, headers, payload))]
pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
) -> Result<Response> {
    let request_id = generate_request_id();
    let key_config = verify_auth(&headers, &state)?;
    let api_key_name = get_key_name(&key_config);

    REQUEST_ID
        .scope(request_id.clone(), async move {
            API_KEY_NAME
                .scope(api_key_name.clone(), async move {
                    if !payload.is_object() {
                        return Err(AppError::BadRequest(
                            "Request body must be a JSON object".to_string(),
                        ));
                    }

                    let original_model = payload
                        .get("model")
                        .and_then(|m| m.as_str())
                        .map(|m| m.to_string());
                    
                    // Strip provider suffix if configured (e.g., "Proxy/gpt-4" -> "gpt-4")
                    let effective_model = original_model.as_ref().map(|m| {
                        strip_provider_suffix(m, state.config.provider_suffix.as_deref())
                    });
                    let model_label = effective_model
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string());

                    // Select provider based on the effective model (with suffix stripped)
                    // Return 400 (Bad Request) if no provider available, not 500
                    let provider = match state
                        .provider_service
                        .get_next_provider(effective_model.as_deref())
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

                    // Map model if needed (use effective_model for mapping lookup)
                    if let Some(eff_model) = effective_model.as_ref() {
                        if let Some(mapped) = provider.model_mapping.get(eff_model) {
                            if let Some(obj) = payload.as_object_mut() {
                                obj.insert(
                                    "model".to_string(),
                                    serde_json::Value::String(mapped.clone()),
                                );
                            }
                        } else if original_model.as_ref() != effective_model.as_ref() {
                            // If no mapping found but we stripped a prefix, update the model in payload
                            if let Some(obj) = payload.as_object_mut() {
                                obj.insert(
                                    "model".to_string(),
                                    serde_json::Value::String(eff_model.clone()),
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
                            let count_model = effective_model.as_deref().unwrap_or("gpt-3.5-turbo");
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

                            // Get api_key_name from context for use in closures
                            let api_key_name = get_api_key_name();
                            
                            // Create a response builder that will be used for all error cases
                            // This ensures model, provider, and api_key_name are always set for metrics
                            let create_error_response = |status: StatusCode, message: String| -> Response {
                                build_error_response(status, message, &model_label, &provider.name, &api_key_name)
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
                                response.extensions_mut().insert(ApiKeyName(api_key_name.clone()));
                                return Ok(response);
                            }

                            let mut final_response = if is_stream {
                                // For streaming, response is already checked for errors above
                                // Pass input tokens for fallback calculation and TTFT timeout
                                match create_sse_stream(
                                    response,
                                    model_label.clone(),
                                    provider.name.clone(),
                                    prompt_tokens_for_fallback,
                                    state.config.ttft_timeout_secs,
                                )
                                .await
                                {
                                    Ok(sse_stream) => sse_stream.into_response(),
                                    Err(e) => {
                                        tracing::error!(
                                            request_id = %request_id,
                                            provider = %provider.name,
                                            error = %e,
                                            "Streaming error"
                                        );
                                        return Ok(create_error_response(
                                            StatusCode::GATEWAY_TIMEOUT,
                                            e.to_string(),
                                        ));
                                    }
                                }
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

                                // Record token usage (api_key_name read from context inside record_token_usage)
                                if let Some(usage_obj) = response_data.get("usage") {
                                    if let Ok(usage) = serde_json::from_value::<Usage>(usage_obj.clone()) {
                                        record_token_usage(&usage, &model_label, &provider.name);
                                    }
                                }

                                let rewritten = rewrite_model_in_response(response_data, &model_label);
                                Json(rewritten).into_response()
                            };

                            // Add model, provider, and api_key_name info to response extensions for middleware logging
                            final_response
                                .extensions_mut()
                                .insert(ModelName(model_label));
                            final_response
                                .extensions_mut()
                                .insert(ProviderName(provider.name));
                            final_response
                                .extensions_mut()
                                .insert(ApiKeyName(api_key_name));

                            Ok(final_response)
                        })
                        .await
                })
                .await
        })
        .await
}

/// Handle legacy completions endpoint.
///
/// This endpoint is compatible with the OpenAI Completions API (legacy).
#[utoipa::path(
    post,
    path = "/v1/completions",
    tag = "completions",
    request_body = serde_json::Value,
    responses(
        (status = 200, description = "Completion response"),
        (status = 400, description = "Bad request", body = ApiErrorResponse),
        (status = 401, description = "Unauthorized", body = ApiErrorResponse),
        (status = 429, description = "Rate limit exceeded", body = ApiErrorResponse),
        (status = 502, description = "Bad gateway - upstream error", body = ApiErrorResponse),
        (status = 504, description = "Gateway timeout", body = ApiErrorResponse)
    ),
    security(
        ("bearer_auth" = [])
    )
)]
#[tracing::instrument(skip(state, headers, payload))]
pub async fn completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Result<Response> {
    let request_id = generate_request_id();
    let key_config = verify_auth(&headers, &state)?;
    let api_key_name = get_key_name(&key_config);

    REQUEST_ID
        .scope(request_id.clone(), async move {
            API_KEY_NAME
                .scope(api_key_name.clone(), async move {
                    if !payload.is_object() {
                        return Err(AppError::BadRequest(
                            "Request body must be a JSON object".to_string(),
                        ));
                    }

                    // Extract model from payload if available
                    let original_model = payload.get("model").and_then(|m| m.as_str());
                    
                    // Strip provider suffix if configured (e.g., "Proxy/gpt-4" -> "gpt-4")
                    let effective_model = original_model.map(|m| {
                        strip_provider_suffix(m, state.config.provider_suffix.as_deref())
                    });
                    let model_label = effective_model.as_deref().unwrap_or("unknown").to_string();

                    let provider = match state.provider_service.get_next_provider(effective_model.as_deref()) {
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

                            // Get api_key_name from context
                            let api_key_name = get_api_key_name();

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
                                        &api_key_name,
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
                                response.extensions_mut().insert(ApiKeyName(api_key_name.clone()));
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
                                        &api_key_name,
                                    ));
                                }
                            };
                            let mut final_response = Json(response_data).into_response();

                            // Add model, provider, and api_key_name info to response extensions for middleware logging
                            final_response
                                .extensions_mut()
                                .insert(ModelName(model_label));
                            final_response
                                .extensions_mut()
                                .insert(ProviderName(provider.name));
                            final_response
                                .extensions_mut()
                                .insert(ApiKeyName(api_key_name));

                            Ok(final_response)
                        })
                        .await
                })
                .await
        })
        .await
}

/// List available models.
///
/// Returns a list of all available models that can be used with the API.
/// This endpoint is compatible with the OpenAI Models API.
#[utoipa::path(
    get,
    path = "/v1/models",
    tag = "models",
    responses(
        (status = 200, description = "List of available models", body = ModelList),
        (status = 401, description = "Unauthorized", body = ApiErrorResponse)
    ),
    security(
        ("bearer_auth" = [])
    )
)]
#[tracing::instrument(skip(state, headers))]
pub async fn list_models(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<ModelList>> {
    let request_id = generate_request_id();

    REQUEST_ID
        .scope(request_id.clone(), async move {
            let key_config = verify_auth(&headers, &state)?;

            tracing::debug!(
                request_id = %request_id,
                "Listing available models"
            );

            let all_models = state.provider_service.get_all_models();
            
            // Filter models based on allowed_models if configured
            let filtered_models: HashSet<String> = if let Some(ref config) = key_config {
                if !config.allowed_models.is_empty() {
                    let allowed_set: HashSet<&String> = config.allowed_models.iter().collect();
                    all_models
                        .into_iter()
                        .filter(|m| allowed_set.contains(m))
                        .collect()
                } else {
                    all_models
                }
            } else {
                all_models
            };

            let mut model_list: Vec<ModelInfo> = filtered_models
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
/// Reads api_key_name from context.
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
    let api_key_name = get_api_key_name();
    let metrics = get_metrics();

    metrics
        .token_usage
        .with_label_values(&[model, provider, "prompt", &api_key_name])
        .inc_by(usage.prompt_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "completion", &api_key_name])
        .inc_by(usage.completion_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "total", &api_key_name])
        .inc_by(usage.total_tokens as u64);

    let request_id = crate::core::logging::get_request_id();
    tracing::debug!(
        request_id = %request_id,
        model = %model,
        provider = %provider,
        api_key_name = %api_key_name,
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
    api_key_name: &str,
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
        .extensions_mut()
        .insert(ApiKeyName(api_key_name.to_string()));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_provider_suffix_with_matching_prefix() {
        // When provider_suffix is "Proxy", "Proxy/gpt-4" should become "gpt-4"
        assert_eq!(
            strip_provider_suffix("Proxy/gpt-4", Some("Proxy")),
            "gpt-4"
        );
    }

    #[test]
    fn test_strip_provider_suffix_without_prefix() {
        // When model doesn't have the prefix, it should remain unchanged
        assert_eq!(
            strip_provider_suffix("gpt-4", Some("Proxy")),
            "gpt-4"
        );
    }

    #[test]
    fn test_strip_provider_suffix_different_prefix() {
        // When model has a different prefix, it should remain unchanged
        assert_eq!(
            strip_provider_suffix("Other/gpt-4", Some("Proxy")),
            "Other/gpt-4"
        );
    }

    #[test]
    fn test_strip_provider_suffix_no_suffix_configured() {
        // When no provider_suffix is configured, model should remain unchanged
        assert_eq!(
            strip_provider_suffix("Proxy/gpt-4", None),
            "Proxy/gpt-4"
        );
    }

    #[test]
    fn test_strip_provider_suffix_empty_suffix() {
        // When provider_suffix is empty string, model should remain unchanged
        assert_eq!(
            strip_provider_suffix("Proxy/gpt-4", Some("")),
            "Proxy/gpt-4"
        );
    }

    #[test]
    fn test_strip_provider_suffix_complex_model_name() {
        // Test with more complex model names
        assert_eq!(
            strip_provider_suffix("Proxy/gpt-4-turbo-preview", Some("Proxy")),
            "gpt-4-turbo-preview"
        );
        assert_eq!(
            strip_provider_suffix("Proxy/claude-3-opus-20240229", Some("Proxy")),
            "claude-3-opus-20240229"
        );
    }

    #[test]
    fn test_strip_provider_suffix_nested_slashes() {
        // Test with model names that have slashes in them
        assert_eq!(
            strip_provider_suffix("Proxy/org/model-name", Some("Proxy")),
            "org/model-name"
        );
    }

    #[test]
    fn test_strip_provider_suffix_case_sensitive() {
        // Prefix matching should be case-sensitive
        assert_eq!(
            strip_provider_suffix("proxy/gpt-4", Some("Proxy")),
            "proxy/gpt-4"
        );
        assert_eq!(
            strip_provider_suffix("PROXY/gpt-4", Some("Proxy")),
            "PROXY/gpt-4"
        );
    }

    #[test]
    fn test_strip_provider_suffix_partial_match() {
        // Should not strip if it's only a partial match (no slash)
        assert_eq!(
            strip_provider_suffix("Proxygpt-4", Some("Proxy")),
            "Proxygpt-4"
        );
    }
}

