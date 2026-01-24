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
use crate::core::langfuse::{get_langfuse_service, GenerationData};
use crate::core::logging::{generate_request_id, get_api_key_name, PROVIDER_CONTEXT, REQUEST_ID};
use crate::core::metrics::get_metrics;
use crate::core::middleware::{ApiKeyName, ModelName, ProviderName};
use crate::core::{AppError, RateLimiter, Result};
use crate::services::ProviderService;
use crate::with_request_context;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use prometheus::{Encoder, TextEncoder};
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;

/// Cached ProviderService with version tracking for efficient hot reload.
/// Only rebuilds when the config version changes.
struct CachedProviderService {
    version: i64,
    service: ProviderService,
    credentials: Vec<crate::core::config::CredentialConfig>,
}

/// Convert database credential to config credential
fn convert_credential(c: &crate::core::database::CredentialEntity) -> crate::core::config::CredentialConfig {
    crate::core::config::CredentialConfig {
        credential_key: c.credential_key.clone(),
        name: c.name.clone(),
        description: None,
        rate_limit: c.rate_limit.map(|rps| crate::core::config::RateLimitConfig {
            requests_per_second: rps as u32,
            burst_size: (rps as u32).saturating_mul(2),
        }),
        enabled: c.is_enabled,
        allowed_models: c.allowed_models.clone(),
    }
}

/// Convert database provider to config provider
fn convert_provider(p: &crate::core::database::ProviderEntity) -> crate::core::config::ProviderConfig {
    crate::core::config::ProviderConfig {
        name: p.provider_key.clone(),
        api_base: p.api_base.clone(),
        api_key: p.api_key.clone(),
        weight: p.weight as u32,
        model_mapping: p.model_mapping.0.clone(),
    }
}

/// Shared application state.
#[derive(Clone)]
pub struct AppState {
    pub config: crate::core::config::AppConfig,
    pub rate_limiter: Arc<RateLimiter>,
    pub http_client: reqwest::Client,
    pub dynamic_config: Option<Arc<crate::core::DynamicConfig>>,
    /// Cached ProviderService with version tracking for O(1) access
    cached_service: Arc<arc_swap::ArcSwap<CachedProviderService>>,
}

impl AppState {
    /// Create a new AppState with cached ProviderService
    pub fn new(
        config: crate::core::config::AppConfig,
        provider_service: ProviderService,
        rate_limiter: Arc<RateLimiter>,
        http_client: reqwest::Client,
        dynamic_config: Option<Arc<crate::core::DynamicConfig>>,
    ) -> Self {
        let (initial_version, initial_credentials) = match &dynamic_config {
            Some(dc) => {
                let rc = dc.get_full();
                (rc.version, rc.credentials.iter().map(convert_credential).collect())
            }
            None => (0, config.credentials.clone()),
        };

        let cached = CachedProviderService {
            version: initial_version,
            service: provider_service,
            credentials: initial_credentials,
        };

        Self {
            config,
            rate_limiter,
            http_client,
            dynamic_config,
            cached_service: Arc::new(arc_swap::ArcSwap::from_pointee(cached)),
        }
    }

    /// Rebuild cache from current DynamicConfig state
    fn rebuild_cache(&self, runtime_config: &crate::core::database::RuntimeConfig) -> CachedProviderService {
        let providers: Vec<_> = runtime_config.providers.iter().map(convert_provider).collect();
        let credentials: Vec<_> = runtime_config.credentials.iter().map(convert_credential).collect();

        let app_config = crate::core::config::AppConfig {
            providers,
            server: self.config.server.clone(),
            verify_ssl: self.config.verify_ssl,
            request_timeout_secs: self.config.request_timeout_secs,
            ttft_timeout_secs: self.config.ttft_timeout_secs,
            credentials: credentials.clone(),
            provider_suffix: self.config.provider_suffix.clone(),
            min_tokens_limit: self.config.min_tokens_limit,
            max_tokens_limit: self.config.max_tokens_limit,
        };

        CachedProviderService {
            version: runtime_config.version,
            service: ProviderService::new(app_config),
            credentials,
        }
    }

    /// Get cached service, rebuilding if version changed.
    /// Returns Arc to avoid cloning on fast path.
    fn get_cached(&self) -> arc_swap::Guard<Arc<CachedProviderService>> {
        let cached = self.cached_service.load();
        
        if let Some(ref dc) = self.dynamic_config {
            let runtime_config = dc.get_full();
            if cached.version != runtime_config.version {
                // Version changed, rebuild cache
                let new_cached = Arc::new(self.rebuild_cache(&runtime_config));
                self.cached_service.store(new_cached);
                
                tracing::debug!(
                    old_version = cached.version,
                    new_version = runtime_config.version,
                    "ProviderService cache updated"
                );
                
                return self.cached_service.load();
            }
        }
        
        cached
    }

    /// Get ProviderService - O(1) for most requests
    pub fn get_provider_service(&self) -> ProviderService {
        self.get_cached().service.clone()
    }

    /// Get credentials - O(1) for most requests
    pub fn get_credentials(&self) -> Vec<crate::core::config::CredentialConfig> {
        self.get_cached().credentials.clone()
    }
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

    // Get credentials from DynamicConfig if available
    let credentials = state.get_credentials();
    
    // Check if any authentication is required
    if credentials.is_empty() {
        return Ok(None);
    }

    let provided_key = provided_key.ok_or(AppError::Unauthorized)?;

    // Hash the provided key for comparison with stored hashes
    let provided_key_hash = hash_key(provided_key);

    // Check against credentials configuration
    for credential_config in credentials {
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

    with_request_context!(request_id.clone(), api_key_name.clone(), async move {
        // Extract client metadata from headers for Langfuse tracing
        let mut client_metadata = std::collections::HashMap::new();
        let user_agent = headers.get("user-agent").and_then(|v| v.to_str().ok()).map(|s| s.to_string());
        if let Some(ref ua) = user_agent {
            client_metadata.insert("user_agent".to_string(), ua.clone());
        }
        if let Some(forwarded_for) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            client_metadata.insert("x_forwarded_for".to_string(), forwarded_for.to_string());
        }
        if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            client_metadata.insert("x_real_ip".to_string(), real_ip.to_string());
        }
        if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
            client_metadata.insert("origin".to_string(), origin.to_string());
        }
        if let Some(referer) = headers.get("referer").and_then(|v| v.to_str().ok()) {
            client_metadata.insert("referer".to_string(), referer.to_string());
        }

        // Build tags for Langfuse (credential, user-agent will be added after provider selection)
        let mut tags = vec![
            "endpoint:/v1/chat/completions".to_string(),
            format!("credential:{}", api_key_name),
        ];
        if let Some(ref ua) = user_agent {
            // Truncate user-agent for tag (tags should be short)
            let ua_short = if ua.len() > 50 { &ua[..50] } else { ua.as_str() };
            tags.push(format!("user_agent:{}", ua_short));
        }

        // Initialize Langfuse tracing (provider tag will be added via update_trace_provider)
        let langfuse = get_langfuse_service();
        let trace_id = if let Ok(service) = langfuse.read() {
            service.create_trace(
                &request_id,
                &api_key_name,
                "/v1/chat/completions",
                tags,
                client_metadata,
            )
        } else {
            None
        };

        // Initialize generation data for Langfuse
        let mut generation_data = GenerationData {
            trace_id: trace_id.clone().unwrap_or_default(),
            request_id: request_id.clone(),
            credential_name: api_key_name.clone(),
            endpoint: "/v1/chat/completions".to_string(),
            start_time: Utc::now(),
            ..Default::default()
        };

        if !payload.is_object() {
            // Record error in Langfuse
            generation_data.is_error = true;
            generation_data.error_message = Some("Request body must be a JSON object".to_string());
            generation_data.end_time = Some(Utc::now());
            if trace_id.is_some() {
                if let Ok(service) = langfuse.read() {
                    service.trace_generation(generation_data);
                }
            }
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

        // Capture input data for Langfuse
        generation_data.original_model = model_label.clone();
        generation_data.input_messages = payload
            .get("messages")
            .and_then(|m| m.as_array())
            .map(|arr| arr.to_vec())
            .unwrap_or_default();
        generation_data.is_streaming = payload
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false);

        // Capture model parameters for Langfuse
        let param_keys = [
            "temperature",
            "max_tokens",
            "top_p",
            "frequency_penalty",
            "presence_penalty",
            "stop",
            "n",
            "logprobs",
        ];
        for key in param_keys {
            if let Some(val) = payload.get(key) {
                generation_data
                    .model_parameters
                    .insert(key.to_string(), val.clone());
            }
        }

        // Get fresh provider service from DynamicConfig
        let provider_service = state.get_provider_service();
        
        // Select provider based on the effective model (with suffix stripped)
        // Return 400 (Bad Request) if no provider available, not 500
        let provider = match provider_service
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
                // Record error in Langfuse
                generation_data.is_error = true;
                generation_data.error_message = Some(err.clone());
                generation_data.end_time = Some(Utc::now());
                if trace_id.is_some() {
                    if let Ok(service) = langfuse.read() {
                        service.trace_generation(generation_data);
                    }
                }
                return Err(AppError::BadRequest(err));
            }
        };

        // Capture provider info for Langfuse
        generation_data.provider_key = provider.name.clone();
        generation_data.provider_type = "openai".to_string(); // Default provider type
        generation_data.provider_api_base = provider.api_base.clone();
        let mapped_model = provider
            .model_mapping
            .get(&model_label)
            .cloned()
            .unwrap_or_else(|| model_label.clone());
        generation_data.mapped_model = mapped_model.clone();

        // Update trace with provider info (so trace metadata includes provider)
        if let Some(ref tid) = trace_id {
            if let Ok(service) = langfuse.read() {
                service.update_trace_provider(
                    tid,
                    &provider.name,
                    &provider.api_base,
                    &model_label,
                );
            }
        }

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
                            let create_error_response = |status: StatusCode, message: String, gen_data: &mut GenerationData| -> Response {
                                // Record error in Langfuse
                                gen_data.is_error = true;
                                gen_data.error_message = Some(message.clone());
                                gen_data.end_time = Some(Utc::now());
                                if trace_id.is_some() {
                                    if let Ok(service) = langfuse.read() {
                                        service.trace_generation(gen_data.clone());
                                    }
                                }
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
                                        &mut generation_data,
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

                                // Record error in Langfuse
                                let error_message = error_body
                                    .get("error")
                                    .and_then(|e| e.get("message"))
                                    .and_then(|m| m.as_str())
                                    .unwrap_or(&format!("HTTP {}", status))
                                    .to_string();
                                generation_data.is_error = true;
                                generation_data.error_message = Some(error_message);
                                generation_data.end_time = Some(Utc::now());
                                if trace_id.is_some() {
                                    if let Ok(service) = langfuse.read() {
                                        service.trace_generation(generation_data.clone());
                                    }
                                }

                                // Faithfully return the backend's status code and error body
                                let mut response = Json(error_body).into_response();
                                *response.status_mut() = StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                                response.extensions_mut().insert(ModelName(model_label.clone()));
                                response.extensions_mut().insert(ProviderName(provider.name.clone()));
                                response.extensions_mut().insert(ApiKeyName(api_key_name.clone()));
                                return Ok(response);
                            }

                            let mut final_response = if is_stream {
                                // For streaming, pass generation_data to create_sse_stream
                                // Only pass if trace_id is Some (Langfuse enabled and sampled)
                                let langfuse_data = if trace_id.is_some() {
                                    Some(generation_data)
                                } else {
                                    None
                                };
                                
                                match create_sse_stream(
                                    response,
                                    model_label.clone(),
                                    provider.name.clone(),
                                    prompt_tokens_for_fallback,
                                    state.config.ttft_timeout_secs,
                                    langfuse_data,
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
                                        // Note: generation_data was moved into create_sse_stream
                                        // so we can't record error here - it's handled inside the stream
                                        return Ok(build_error_response(
                                            StatusCode::GATEWAY_TIMEOUT,
                                            e.to_string(),
                                            &model_label,
                                            &provider.name,
                                            &api_key_name,
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
                                            &mut generation_data,
                                        ));
                                    }
                                };

                                // Capture output for Langfuse
                                if let Some(choices) = response_data.get("choices").and_then(|c| c.as_array()) {
                                    if let Some(first_choice) = choices.first() {
                                        generation_data.output_content = first_choice
                                            .get("message")
                                            .and_then(|m| m.get("content"))
                                            .and_then(|c| c.as_str())
                                            .map(|s| s.to_string());
                                        generation_data.finish_reason = first_choice
                                            .get("finish_reason")
                                            .and_then(|r| r.as_str())
                                            .map(|s| s.to_string());
                                    }
                                }

                                // Record token usage (api_key_name read from context inside record_token_usage)
                                if let Some(usage_obj) = response_data.get("usage") {
                                    if let Ok(usage) = serde_json::from_value::<Usage>(usage_obj.clone()) {
                                        record_token_usage(&usage, &model_label, &provider.name);
                                        
                                        // Capture usage for Langfuse
                                        generation_data.prompt_tokens = usage.prompt_tokens;
                                        generation_data.completion_tokens = usage.completion_tokens;
                                        generation_data.total_tokens = usage.total_tokens;
                                    }
                                }

                                // Record successful generation in Langfuse
                                generation_data.end_time = Some(Utc::now());
                                if trace_id.is_some() {
                                    if let Ok(service) = langfuse.read() {
                                        service.trace_generation(generation_data);
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

    with_request_context!(request_id.clone(), api_key_name.clone(), async move {
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

        // Get fresh provider service from DynamicConfig
        let provider_service = state.get_provider_service();
        
        let provider = match provider_service.get_next_provider(effective_model.as_deref()) {
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

            // Get fresh provider service from DynamicConfig
            let provider_service = state.get_provider_service();
            let all_models = provider_service.get_all_models();
            
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

