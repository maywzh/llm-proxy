//! HTTP request handlers for the LLM proxy API.
//!
//! This module contains all endpoint handlers including chat completions,
//! model listings, and metrics.

use crate::api::models::*;
use crate::api::streaming::{
    calculate_message_tokens_with_tools, create_sse_stream, rewrite_model_in_response,
};
use crate::core::config::CredentialConfig;
use crate::core::database::hash_key;
use crate::core::jsonl_logger::{
    log_provider_request, log_provider_response, log_request, log_response,
};
use crate::core::langfuse::{
    build_langfuse_tags, extract_client_metadata, get_langfuse_service, GenerationData,
};
use crate::core::logging::{generate_request_id, get_api_key_name, PROVIDER_CONTEXT, REQUEST_ID};
use crate::core::metrics::get_metrics;
use crate::core::middleware::{extract_client, ApiKeyName, HasCredentials, ModelName, ProviderName};
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
fn convert_credential(
    c: &crate::core::database::CredentialEntity,
) -> crate::core::config::CredentialConfig {
    crate::core::config::CredentialConfig {
        credential_key: c.credential_key.clone(),
        name: c.name.clone(),
        description: None,
        rate_limit: c
            .rate_limit
            .map(|rps| crate::core::config::RateLimitConfig {
                requests_per_second: rps as u32,
                burst_size: (rps as u32).saturating_mul(2),
            }),
        enabled: c.is_enabled,
        allowed_models: c.allowed_models.clone(),
    }
}

/// Convert database provider to config provider
fn convert_provider(
    p: &crate::core::database::ProviderEntity,
) -> crate::core::config::ProviderConfig {
    crate::core::config::ProviderConfig {
        name: p.provider_key.clone(),
        api_base: p.api_base.clone(),
        api_key: p.api_key.clone(),
        weight: p.weight as u32,
        model_mapping: p.model_mapping.0.clone(),
        provider_type: p.provider_type.clone(),
    }
}

// ============================================================================
// Gemini 3 Thought Signature Support
// ============================================================================

/// Check if provider name indicates Gemini 3 (for thought_signature handling)
fn is_gemini3_provider_name(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    name_lower.contains("gemini-3")
        || name_lower.contains("gemini3")
        || name_lower.contains("gemini_3")
}

/// Log thought_signature presence in response for debugging (pass-through, no modification)
fn log_gemini_response_signatures(response: &serde_json::Value, provider_name: &str) {
    if !is_gemini3_provider_name(provider_name) {
        return;
    }

    if let Some(choices) = response.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(message) = choice.get("message") {
                check_and_log_signatures(message, "message");
            }
            if let Some(delta) = choice.get("delta") {
                check_and_log_signatures(delta, "delta");
            }
        }
    }
}

fn check_and_log_signatures(content: &serde_json::Value, location: &str) {
    if let Some(sig) = content
        .get("extra_content")
        .and_then(|e| e.get("google"))
        .and_then(|g| g.get("thought_signature"))
    {
        tracing::debug!(
            location,
            sig_len = sig.as_str().map(|s| s.len()).unwrap_or(0),
            "Found thought_signature in {}.extra_content",
            location
        );
    }

    if let Some(tool_calls) = content.get("tool_calls").and_then(|t| t.as_array()) {
        let sig_count = tool_calls
            .iter()
            .filter(|tc| {
                tc.get("extra_content")
                    .and_then(|e| e.get("google"))
                    .and_then(|g| g.get("thought_signature"))
                    .is_some()
            })
            .count();

        if sig_count > 0 {
            tracing::debug!(
                location,
                sig_count,
                "Found {} thought_signatures in {}.tool_calls",
                sig_count,
                location
            );
        }
    }
}

/// Log thought_signature presence in request for debugging (pass-through, no modification)
fn log_gemini_request_signatures(payload: &serde_json::Value, provider_name: &str) {
    if !is_gemini3_provider_name(provider_name) {
        return;
    }

    if let Some(messages) = payload.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                let signatures_count = tool_calls
                    .iter()
                    .filter(|tc| {
                        tc.get("extra_content")
                            .and_then(|e| e.get("google"))
                            .and_then(|g| g.get("thought_signature"))
                            .is_some()
                    })
                    .count();

                if signatures_count > 0 {
                    tracing::debug!(
                        signatures_count,
                        "Gemini 3 request contains thought_signatures in extra_content (pass-through)"
                    );
                }
            }
        }
    }
}

// ============================================================================
// Bedrock Claude Compatibility
// ============================================================================

/// Check if the model is a Bedrock Claude model.
/// Bedrock Claude models have prefix "claude-" and suffix "-bedrock".
pub fn is_bedrock_claude_model(model: &str) -> bool {
    model.starts_with("claude-") && model.ends_with("-bedrock")
}

/// Check if messages contain tool_calls (OpenAI format) but no tools definition.
/// This is used to detect when we need to inject a placeholder tool for Bedrock compatibility.
fn messages_contain_tool_calls(payload: &serde_json::Value) -> bool {
    payload
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|messages| {
            messages.iter().any(|msg| {
                msg.get("tool_calls")
                    .and_then(|tc| tc.as_array())
                    .map(|arr| !arr.is_empty())
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Check if the payload already has tools defined.
fn has_tools_defined(payload: &serde_json::Value) -> bool {
    payload
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|arr| !arr.is_empty())
        .unwrap_or(false)
}

/// Inject a placeholder tool for Bedrock Claude compatibility.
/// This is needed when messages contain tool_calls but no tools definition.
fn inject_placeholder_tool(payload: &mut serde_json::Value) {
    let placeholder_tool = json!({
        "type": "function",
        "function": {
            "name": "_placeholder_tool",
            "description": "Placeholder tool for Bedrock compatibility",
            "parameters": {
                "type": "object",
                "properties": {}
            }
        }
    });

    if let Some(obj) = payload.as_object_mut() {
        obj.insert("tools".to_string(), json!([placeholder_tool]));
    }

    tracing::debug!("Injected placeholder tool for Bedrock Claude compatibility");
}

/// Apply Bedrock Claude compatibility fix if needed.
/// Injects a placeholder tool when:
/// 1. The model is a Bedrock Claude model
/// 2. Messages contain tool_calls
/// 3. No tools are defined in the request
pub fn apply_bedrock_compatibility(payload: &mut serde_json::Value, model: &str) {
    if is_bedrock_claude_model(model)
        && messages_contain_tool_calls(payload)
        && !has_tools_defined(payload)
    {
        inject_placeholder_tool(payload);
    }
}

// ============================================================================
// AppState
// ============================================================================

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
                (
                    rc.version,
                    rc.credentials.iter().map(convert_credential).collect(),
                )
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
    fn rebuild_cache(
        &self,
        runtime_config: &crate::core::database::RuntimeConfig,
    ) -> CachedProviderService {
        let providers: Vec<_> = runtime_config
            .providers
            .iter()
            .map(convert_provider)
            .collect();
        let credentials: Vec<_> = runtime_config
            .credentials
            .iter()
            .map(convert_credential)
            .collect();

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

impl HasCredentials for AppState {
    fn get_credentials(&self) -> Vec<crate::core::config::CredentialConfig> {
        AppState::get_credentials(self)
    }
}

/// Verify API key authentication and check rate limits.
///
/// Checks both x-api-key and Authorization headers against configured credentials.
/// x-api-key takes precedence over Authorization: Bearer (consistent with Python).
/// If a credential is found, also enforces rate limiting if configured.
///
/// Returns the full CredentialConfig for the authenticated credential, or None if no auth required.
fn verify_auth(headers: &HeaderMap, state: &AppState) -> Result<Option<CredentialConfig>> {
    // Extract the provided key from headers
    // x-api-key takes precedence over Authorization: Bearer (consistent with Python/Claude API)
    let provided_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            headers.get("authorization").and_then(|auth_header| {
                auth_header
                    .to_str()
                    .ok()
                    .and_then(|auth_str| auth_str.strip_prefix("Bearer "))
            })
        });

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
            state
                .rate_limiter
                .check_rate_limit(&credential_config.credential_key)?;

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
    let Some(suffix) = provider_suffix.filter(|s| !s.is_empty()) else {
        return model.to_string();
    };

    // Check if model starts with "suffix/" without allocating
    model
        .strip_prefix(suffix)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(model)
        .to_string()
}

// ============================================================================
// Helper Types and Structs for Chat Completions
// ============================================================================

/// Context for Langfuse tracing initialization.
struct LangfuseContext {
    /// The trace ID if Langfuse is enabled and sampled
    trace_id: Option<String>,
    /// Generation data for tracking the request
    generation_data: GenerationData,
}

/// Parsed request data from the chat completion payload.
struct ParsedRequest {
    /// Original model name from the request
    original_model: Option<String>,
    /// Effective model name after stripping provider suffix
    effective_model: Option<String>,
    /// Model label for logging and metrics
    model_label: String,
    /// Whether this is a streaming request
    is_stream: bool,
    /// Estimated prompt tokens for fallback calculation
    prompt_tokens_for_fallback: Option<usize>,
}

/// Selected provider with request URL.
struct SelectedProvider {
    /// The provider configuration
    provider: Provider,
    /// The URL to send the request to
    url: String,
}

// ============================================================================
// Helper Functions for Chat Completions
// ============================================================================

/// Initialize Langfuse tracing for a chat completion request.
///
/// Creates a trace and initializes generation data if Langfuse is enabled.
/// Returns the trace context containing trace_id and generation_data.
///
/// # Arguments
/// * `request_id` - Unique identifier for this request
/// * `api_key_name` - Name of the API key used for authentication
/// * `headers` - HTTP headers from the request
fn init_langfuse_trace(
    request_id: &str,
    api_key_name: &str,
    headers: &HeaderMap,
) -> LangfuseContext {
    // Extract client metadata from headers for Langfuse tracing
    let client_metadata = extract_client_metadata(headers);
    let user_agent = client_metadata.get("user_agent").cloned();

    // Build tags for Langfuse
    let tags = build_langfuse_tags("/v1/chat/completions", api_key_name, user_agent.as_deref());

    // Initialize Langfuse tracing
    let langfuse = get_langfuse_service();
    let trace_id = if let Ok(service) = langfuse.read() {
        service.create_trace(
            request_id,
            api_key_name,
            "/v1/chat/completions",
            tags,
            client_metadata,
        )
    } else {
        None
    };

    // Initialize generation data for Langfuse
    let generation_data = GenerationData {
        trace_id: trace_id.clone().unwrap_or_default(),
        request_id: request_id.to_string(),
        credential_name: api_key_name.to_string(),
        endpoint: "/v1/chat/completions".to_string(),
        start_time: Utc::now(),
        ..Default::default()
    };

    LangfuseContext {
        trace_id,
        generation_data,
    }
}

/// Parse and validate the chat completion request body.
///
/// Extracts model information, streaming flag, and captures data for Langfuse.
/// Returns an error if the request body is invalid.
///
/// # Arguments
/// * `payload` - The JSON request payload
/// * `state` - Application state containing configuration
/// * `generation_data` - Mutable reference to generation data for Langfuse
fn parse_request_body(
    payload: &serde_json::Value,
    state: &AppState,
    generation_data: &mut GenerationData,
) -> Result<ParsedRequest> {
    let original_model = payload
        .get("model")
        .and_then(|m| m.as_str())
        .map(|m| m.to_string());

    // Strip provider suffix if configured (e.g., "Proxy/gpt-4" -> "gpt-4")
    let effective_model = original_model
        .as_ref()
        .map(|m| strip_provider_suffix(m, state.config.provider_suffix.as_deref()));
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

    let is_stream = payload
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);
    generation_data.is_streaming = is_stream;

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

    // Calculate prompt tokens for fallback
    let prompt_tokens_for_fallback = payload
        .get("messages")
        .and_then(|m| m.as_array())
        .and_then(|messages| {
            let count_model = effective_model.as_deref().unwrap_or("gpt-3.5-turbo");
            let tools = payload.get("tools").and_then(|t| t.as_array());
            let tool_choice = payload.get("tool_choice");
            calculate_message_tokens_with_tools(
                messages,
                count_model,
                tools.map(|tool_list| tool_list.as_slice()),
                tool_choice,
            )
            .ok()
        });

    Ok(ParsedRequest {
        original_model,
        effective_model,
        model_label,
        is_stream,
        prompt_tokens_for_fallback,
    })
}

/// Select a provider for the chat completion request.
///
/// Uses weighted selection algorithm to choose a provider that supports the model.
/// Updates generation data with provider information.
///
/// # Arguments
/// * `state` - Application state containing provider service
/// * `effective_model` - The model name to use for provider selection
/// * `model_label` - Model label for logging
/// * `generation_data` - Mutable reference to generation data for Langfuse
/// * `trace_id` - Optional trace ID for Langfuse
fn select_provider(
    state: &AppState,
    effective_model: Option<&str>,
    model_label: &str,
    generation_data: &mut GenerationData,
    trace_id: &Option<String>,
) -> Result<SelectedProvider> {
    let provider_service = state.get_provider_service();

    let provider = match provider_service.get_next_provider(effective_model) {
        Ok(p) => p,
        Err(err) => {
            tracing::error!(
                error = %err,
                model = %model_label,
                "Provider selection failed - no available provider for model"
            );
            return Err(AppError::BadRequest(err));
        }
    };

    // Capture provider info for Langfuse
    generation_data.provider_key = provider.name.clone();
    generation_data.provider_type = "openai".to_string();
    generation_data.provider_api_base = provider.api_base.clone();
    let mapped_model = provider
        .model_mapping
        .get(model_label)
        .cloned()
        .unwrap_or_else(|| model_label.to_string());
    generation_data.mapped_model = mapped_model;

    // Update trace with provider info
    if let Some(ref tid) = trace_id {
        let langfuse = get_langfuse_service();
        if let Ok(service) = langfuse.read() {
            service.update_trace_provider(tid, &provider.name, &provider.api_base, model_label);
        };
    }

    let url = format!("{}/chat/completions", provider.api_base);

    Ok(SelectedProvider { provider, url })
}

/// Apply model mapping to the request payload.
///
/// Updates the model field in the payload based on provider's model mapping.
/// Also applies Bedrock Claude compatibility fix if needed.
///
/// # Arguments
/// * `payload` - Mutable reference to the JSON payload
/// * `provider` - The selected provider
/// * `original_model` - Original model name from request
/// * `effective_model` - Effective model name after suffix stripping
fn apply_model_mapping(
    payload: &mut serde_json::Value,
    provider: &Provider,
    original_model: &Option<String>,
    effective_model: &Option<String>,
) {
    // Apply Bedrock Claude compatibility fix BEFORE model mapping
    // This uses the effective model name (before mapping to ARN or other provider-specific names)
    if let Some(eff_model) = effective_model.as_ref() {
        apply_bedrock_compatibility(payload, eff_model);
    }

    if let Some(eff_model) = effective_model.as_ref() {
        let mapped = provider.get_mapped_model(eff_model);
        if mapped != *eff_model {
            // Pattern or exact match found, use mapped model
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
}

/// Send HTTP request to the provider.
///
/// Sends the chat completion request to the upstream provider.
///
/// # Arguments
/// * `http_client` - HTTP client for making requests
/// * `url` - Provider endpoint URL
/// * `api_key` - Provider API key
/// * `payload` - Request payload
async fn send_provider_request(
    http_client: &reqwest::Client,
    url: &str,
    api_key: &str,
    payload: &serde_json::Value,
) -> std::result::Result<reqwest::Response, reqwest::Error> {
    http_client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(payload)
        .send()
        .await
}

/// Handle backend error response.
///
/// Parses error response from the provider and creates appropriate error response.
///
/// # Arguments
/// * `response` - The HTTP response from provider
/// * `status` - HTTP status code
/// * `model_label` - Model label for response extensions
/// * `provider_name` - Provider name for response extensions
/// * `api_key_name` - API key name for response extensions
/// * `generation_data` - Mutable reference to generation data for Langfuse
/// * `trace_id` - Optional trace ID for Langfuse
async fn handle_backend_error(
    response: reqwest::Response,
    status: reqwest::StatusCode,
    model_label: &str,
    provider_name: &str,
    api_key_name: &str,
    generation_data: &mut GenerationData,
    trace_id: &Option<String>,
) -> Response {
    let error_body = match response.bytes().await {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(body) => body,
            Err(_) => {
                let text = String::from_utf8_lossy(&bytes).to_string();
                json!({
                    "error": {
                        "message": text,
                        "type": "error",
                        "code": status.as_u16()
                    }
                })
            }
        },
        Err(_) => json!({
            "error": {
                "message": format!("HTTP {}", status),
                "type": "error",
                "code": status.as_u16()
            }
        }),
    };

    tracing::error!(
        provider = %provider_name,
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
        let langfuse = get_langfuse_service();
        if let Ok(service) = langfuse.read() {
            service.trace_generation(generation_data.clone());
        };
    }

    // Build response with proper extensions
    let mut resp = Json(error_body).into_response();
    *resp.status_mut() =
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    resp.extensions_mut()
        .insert(ModelName(model_label.to_string()));
    resp.extensions_mut()
        .insert(ProviderName(provider_name.to_string()));
    resp.extensions_mut()
        .insert(ApiKeyName(api_key_name.to_string()));
    resp
}

/// Handle streaming chat completion response.
///
/// Creates an SSE stream for streaming responses.
///
/// # Arguments
/// * `response` - HTTP response from provider
/// * `model_label` - Model label for metrics
/// * `provider_name` - Provider name for metrics
/// * `prompt_tokens_for_fallback` - Estimated prompt tokens
/// * `ttft_timeout_secs` - Time to first token timeout
/// * `generation_data` - Generation data for Langfuse (consumed)
/// * `trace_id` - Optional trace ID for Langfuse
/// * `api_key_name` - API key name for error responses
/// * `request_id` - Request ID for JSONL logging
/// * `request_payload` - Request payload for JSONL logging
/// * `client` - Client name from User-Agent header
#[allow(clippy::too_many_arguments)]
async fn handle_streaming_response(
    response: reqwest::Response,
    model_label: String,
    provider_name: String,
    prompt_tokens_for_fallback: Option<usize>,
    ttft_timeout_secs: Option<u64>,
    generation_data: GenerationData,
    trace_id: &Option<String>,
    api_key_name: &str,
    request_id: String,
    request_payload: serde_json::Value,
    client: String,
) -> Result<Response> {
    // Only pass generation_data if trace_id is Some (Langfuse enabled and sampled)
    let langfuse_data = if trace_id.is_some() {
        Some(generation_data)
    } else {
        None
    };

    match create_sse_stream(
        response,
        model_label.clone(),
        provider_name.clone(),
        prompt_tokens_for_fallback,
        ttft_timeout_secs,
        langfuse_data,
        Some(request_id),
        Some("/v1/chat/completions".to_string()),
        Some(request_payload),
        Some(client),
    )
    .await
    {
        Ok(sse_stream) => Ok(sse_stream.into_response()),
        Err(e) => {
            tracing::error!(
                provider = %provider_name,
                error = %e,
                "Streaming error"
            );
            Ok(build_error_response(
                StatusCode::GATEWAY_TIMEOUT,
                e.to_string(),
                &model_label,
                &provider_name,
                api_key_name,
            ))
        }
    }
}

/// Handle non-streaming chat completion response.
///
/// Parses JSON response and records metrics.
///
/// # Arguments
/// * `response` - HTTP response from provider
/// * `model_label` - Model label for metrics
/// * `provider_name` - Provider name for metrics
/// * `generation_data` - Mutable reference to generation data for Langfuse
/// * `trace_id` - Optional trace ID for Langfuse
/// * `api_key_name` - API key name for error responses
/// * `request_id` - Request ID for JSONL logging
/// * `request_payload` - Request payload for JSONL logging
/// * `client` - Client name from User-Agent header
#[allow(clippy::too_many_arguments)]
async fn handle_non_streaming_response(
    response: reqwest::Response,
    model_label: &str,
    provider_name: &str,
    generation_data: &mut GenerationData,
    trace_id: &Option<String>,
    api_key_name: &str,
    request_id: &str,
    _request_payload: &serde_json::Value,
    client: &str,
) -> Result<Response> {
    let status = response.status();

    let response_data: serde_json::Value = match response.json().await {
        Ok(data) => {
            // Log provider response to JSONL (the raw response from provider)
            log_provider_response(request_id, provider_name, status.as_u16(), None, &data);
            data
        }
        Err(e) => {
            tracing::error!(
                provider = %provider_name,
                error = %e,
                "Failed to parse provider response JSON"
            );
            // Record error in Langfuse
            generation_data.is_error = true;
            generation_data.error_message = Some(format!("Invalid JSON from provider: {}", e));
            generation_data.end_time = Some(Utc::now());
            if trace_id.is_some() {
                let langfuse = get_langfuse_service();
                if let Ok(service) = langfuse.read() {
                    service.trace_generation(generation_data.clone());
                };
            }
            return Ok(build_error_response(
                StatusCode::BAD_GATEWAY,
                format!("Invalid JSON from provider: {}", e),
                model_label,
                provider_name,
                api_key_name,
            ));
        }
    };

    // Capture output for Langfuse
    if let Some(choices) = response_data.get("choices").and_then(|c| c.as_array()) {
        if let Some(first_choice) = choices.first() {
            // Extract content
            let content = first_choice
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .map(|s| s.to_string());
            // Extract reasoning_content (for OpenAI-compatible APIs like Grok)
            let reasoning = first_choice
                .get("message")
                .and_then(|m| m.get("reasoning_content"))
                .and_then(|r| r.as_str())
                .map(|s| s.to_string());
            // Combine content and reasoning for Langfuse output
            generation_data.output_content = match (content, reasoning) {
                (Some(c), Some(r)) => Some(format!("{}\n\n[Reasoning]\n{}", c, r)),
                (Some(c), None) => Some(c),
                (None, Some(r)) => Some(format!("[Reasoning]\n{}", r)),
                (None, None) => None,
            };
            generation_data.finish_reason = first_choice
                .get("finish_reason")
                .and_then(|r| r.as_str())
                .map(|s| s.to_string());
        }
    }

    // Record token usage
    if let Some(usage_obj) = response_data.get("usage") {
        if let Ok(usage) = serde_json::from_value::<Usage>(usage_obj.clone()) {
            record_token_usage(&usage, model_label, provider_name, client);

            // Capture usage for Langfuse
            generation_data.prompt_tokens = usage.prompt_tokens;
            generation_data.completion_tokens = usage.completion_tokens;
            generation_data.total_tokens = usage.total_tokens;
        }
    }

    // Record successful generation in Langfuse
    generation_data.end_time = Some(Utc::now());
    if trace_id.is_some() {
        let langfuse = get_langfuse_service();
        if let Ok(service) = langfuse.read() {
            service.trace_generation(generation_data.clone());
        };
    }

    // Log response info: DEBUG shows summary, TRACE shows full response with size
    if tracing::enabled!(tracing::Level::DEBUG) {
        let finish_reason = response_data
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("finish_reason"))
            .and_then(|r| r.as_str())
            .unwrap_or("unknown");

        tracing::debug!(
            request_id = %request_id,
            provider = %provider_name,
            model = %model_label,
            finish_reason = %finish_reason,
            "Chat completion response received"
        );

        if tracing::enabled!(tracing::Level::TRACE) {
            let response_json = serde_json::to_string_pretty(&response_data).unwrap_or_default();
            tracing::trace!(
                request_id = %request_id,
                response_bytes = response_json.len(),
                response_body = %response_json,
                "Full response payload"
            );
        }
    }

    // Log Gemini 3 thought_signature presence in response (pass-through debugging)
    log_gemini_response_signatures(&response_data, provider_name);

    // Log response to JSONL file (non-streaming)
    log_response(request_id, status.as_u16(), None, &response_data);

    let rewritten = rewrite_model_in_response(response_data, model_label);
    Ok(Json(rewritten).into_response())
}

/// Record error in Langfuse and return error response.
///
/// Helper function to record errors in Langfuse tracing.
///
/// # Arguments
/// * `generation_data` - Mutable reference to generation data
/// * `trace_id` - Optional trace ID for Langfuse
/// * `error_message` - Error message to record
fn record_langfuse_error(
    generation_data: &mut GenerationData,
    trace_id: &Option<String>,
    error_message: &str,
) {
    generation_data.is_error = true;
    generation_data.error_message = Some(error_message.to_string());
    generation_data.end_time = Some(Utc::now());

    if trace_id.is_some() {
        let langfuse = get_langfuse_service();
        if let Ok(service) = langfuse.read() {
            service.trace_generation(generation_data.clone());
        };
    }
}

/// Add response extensions for middleware logging.
///
/// Adds model, provider, and API key name to response extensions.
///
/// # Arguments
/// * `response` - Mutable reference to the response
/// * `model_label` - Model label
/// * `provider_name` - Provider name
/// * `api_key_name` - API key name
fn add_response_extensions(
    response: &mut Response,
    model_label: String,
    provider_name: String,
    api_key_name: String,
) {
    response.extensions_mut().insert(ModelName(model_label));
    response
        .extensions_mut()
        .insert(ProviderName(provider_name));
    response.extensions_mut().insert(ApiKeyName(api_key_name));
}

// ============================================================================
// Main Chat Completions Handler
// ============================================================================

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
        // Extract client from User-Agent header for metrics
        let client = extract_client(&headers);

        // Initialize Langfuse tracing
        let mut langfuse_ctx = init_langfuse_trace(&request_id, &api_key_name, &headers);

        // Validate request body
        if !payload.is_object() {
            record_langfuse_error(
                &mut langfuse_ctx.generation_data,
                &langfuse_ctx.trace_id,
                "Request body must be a JSON object",
            );
            return Err(AppError::BadRequest(
                "Request body must be a JSON object".to_string(),
            ));
        }

        // Parse request body
        let parsed = parse_request_body(&payload, &state, &mut langfuse_ctx.generation_data)?;

        // Model permission is now checked by model_permission_middleware

        // Select provider
        let selected = match select_provider(
            &state,
            parsed.effective_model.as_deref(),
            &parsed.model_label,
            &mut langfuse_ctx.generation_data,
            &langfuse_ctx.trace_id,
        ) {
            Ok(s) => s,
            Err(e) => {
                record_langfuse_error(
                    &mut langfuse_ctx.generation_data,
                    &langfuse_ctx.trace_id,
                    &e.to_string(),
                );
                return Err(e);
            }
        };

        // Apply model mapping to payload
        apply_model_mapping(
            &mut payload,
            &selected.provider,
            &parsed.original_model,
            &parsed.effective_model,
        );

        // Execute request within provider context scope
        let provider = selected.provider;
        let url = selected.url;
        let model_label = parsed.model_label;
        let is_stream = parsed.is_stream;
        let prompt_tokens_for_fallback = parsed.prompt_tokens_for_fallback;
        let trace_id = langfuse_ctx.trace_id;
        let mut generation_data = langfuse_ctx.generation_data;

        PROVIDER_CONTEXT
            .scope(provider.name.clone(), async move {
                // Log request info: DEBUG shows summary, TRACE shows full payload with size
                if tracing::enabled!(tracing::Level::DEBUG) {
                    let messages_count = payload
                        .get("messages")
                        .and_then(|m| m.as_array())
                        .map(|arr| arr.len())
                        .unwrap_or(0);

                    tracing::debug!(
                        request_id = %request_id,
                        provider = %provider.name,
                        model = %model_label,
                        stream = is_stream,
                        messages_count = messages_count,
                        "Processing chat completion request"
                    );

                    if tracing::enabled!(tracing::Level::TRACE) {
                        let payload_json =
                            serde_json::to_string_pretty(&payload).unwrap_or_default();
                        tracing::trace!(
                            request_id = %request_id,
                            payload_bytes = payload_json.len(),
                            request_body = %payload_json,
                            "Full request payload"
                        );
                    }
                }

                // Log Gemini 3 thought_signature presence in request (pass-through debugging)
                log_gemini_request_signatures(&payload, &provider.name);

                // Log request immediately to JSONL
                log_request(
                    &request_id,
                    "/v1/chat/completions",
                    &provider.name,
                    &payload,
                );

                let api_key_name = get_api_key_name();

                // Log provider request to JSONL (the request sent to provider)
                log_provider_request(
                    &request_id,
                    &provider.name,
                    &provider.api_base,
                    "/chat/completions",
                    &payload,
                );

                // Send request to provider
                let response = match send_provider_request(
                    &state.http_client,
                    &url,
                    &provider.api_key,
                    &payload,
                )
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
                        record_langfuse_error(
                            &mut generation_data,
                            &trace_id,
                            &format!("Upstream request failed: {}", e),
                        );
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

                // Handle backend error responses
                if status.is_client_error() || status.is_server_error() {
                    return Ok(handle_backend_error(
                        response,
                        status,
                        &model_label,
                        &provider.name,
                        &api_key_name,
                        &mut generation_data,
                        &trace_id,
                    )
                    .await);
                }

                // Handle successful response
                let mut final_response = if is_stream {
                    handle_streaming_response(
                        response,
                        model_label.clone(),
                        provider.name.clone(),
                        prompt_tokens_for_fallback,
                        state.config.ttft_timeout_secs,
                        generation_data,
                        &trace_id,
                        &api_key_name,
                        request_id.clone(),
                        payload.clone(),
                        client.clone(),
                    )
                    .await?
                } else {
                    handle_non_streaming_response(
                        response,
                        &model_label,
                        &provider.name,
                        &mut generation_data,
                        &trace_id,
                        &api_key_name,
                        &request_id,
                        &payload,
                        &client,
                    )
                    .await?
                };

                // Add response extensions for middleware logging
                add_response_extensions(
                    &mut final_response,
                    model_label,
                    provider.name,
                    api_key_name,
                );

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
        let effective_model = original_model
            .map(|m| strip_provider_suffix(m, state.config.provider_suffix.as_deref()));
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

                let response = match state
                    .http_client
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
                    *response.status_mut() = StatusCode::from_u16(status.as_u16())
                        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                    response
                        .extensions_mut()
                        .insert(ModelName(model_label.clone()));
                    response
                        .extensions_mut()
                        .insert(ProviderName(provider.name.clone()));
                    response
                        .extensions_mut()
                        .insert(ApiKeyName(api_key_name.clone()));
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
                    // Use wildcard/regex matching for filtering
                    all_models
                        .into_iter()
                        .filter(|m| {
                            crate::api::auth::model_matches_allowed_list(m, &config.allowed_models)
                        })
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
                    id: model.clone(),
                    object: "model".to_string(),
                    created: 1677610602,
                    owned_by: "system".to_string(),
                    // OpenAI compatibility fields
                    permission: vec![],
                    root: model,
                    parent: None,
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
fn record_token_usage(usage: &Usage, model: &str, provider: &str, client: &str) {
    let api_key_name = get_api_key_name();
    let metrics = get_metrics();

    metrics
        .token_usage
        .with_label_values(&[model, provider, "prompt", &api_key_name, client])
        .inc_by(usage.prompt_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "completion", &api_key_name, client])
        .inc_by(usage.completion_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "total", &api_key_name, client])
        .inc_by(usage.total_tokens as u64);

    let request_id = crate::core::logging::get_request_id();
    tracing::debug!(
        request_id = %request_id,
        model = %model,
        provider = %provider,
        api_key_name = %api_key_name,
        client = %client,
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
        assert_eq!(strip_provider_suffix("Proxy/gpt-4", Some("Proxy")), "gpt-4");
    }

    #[test]
    fn test_strip_provider_suffix_without_prefix() {
        // When model doesn't have the prefix, it should remain unchanged
        assert_eq!(strip_provider_suffix("gpt-4", Some("Proxy")), "gpt-4");
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
        assert_eq!(strip_provider_suffix("Proxy/gpt-4", None), "Proxy/gpt-4");
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

    // ========================================================================
    // Bedrock Claude Compatibility Tests
    // ========================================================================

    #[test]
    fn test_is_bedrock_claude_model() {
        // Valid Bedrock Claude models
        assert!(is_bedrock_claude_model("claude-3-opus-bedrock"));
        assert!(is_bedrock_claude_model("claude-3-sonnet-bedrock"));
        assert!(is_bedrock_claude_model("claude-4.5-opus-bedrock"));
        assert!(is_bedrock_claude_model("claude-instant-bedrock"));

        // Non-Bedrock models
        assert!(!is_bedrock_claude_model("claude-3-opus"));
        assert!(!is_bedrock_claude_model("gpt-4"));
        assert!(!is_bedrock_claude_model("bedrock-claude-3-opus")); // Wrong prefix
        assert!(!is_bedrock_claude_model("claude-3-opus-azure")); // Wrong suffix
    }

    #[test]
    fn test_messages_contain_tool_calls_with_tool_calls() {
        let payload = json!({
            "model": "claude-3-opus-bedrock",
            "messages": [
                {"role": "user", "content": "Hello"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"location\": \"NYC\"}"
                            }
                        }
                    ]
                }
            ]
        });
        assert!(messages_contain_tool_calls(&payload));
    }

    #[test]
    fn test_messages_contain_tool_calls_without_tool_calls() {
        let payload = json!({
            "model": "claude-3-opus-bedrock",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there!"}
            ]
        });
        assert!(!messages_contain_tool_calls(&payload));
    }

    #[test]
    fn test_messages_contain_tool_calls_empty_tool_calls() {
        let payload = json!({
            "model": "claude-3-opus-bedrock",
            "messages": [
                {"role": "user", "content": "Hello"},
                {
                    "role": "assistant",
                    "content": "Hi",
                    "tool_calls": []
                }
            ]
        });
        assert!(!messages_contain_tool_calls(&payload));
    }

    #[test]
    fn test_has_tools_defined_with_tools() {
        let payload = json!({
            "model": "claude-3-opus-bedrock",
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather",
                        "parameters": {"type": "object", "properties": {}}
                    }
                }
            ]
        });
        assert!(has_tools_defined(&payload));
    }

    #[test]
    fn test_has_tools_defined_without_tools() {
        let payload = json!({
            "model": "claude-3-opus-bedrock",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        assert!(!has_tools_defined(&payload));
    }

    #[test]
    fn test_has_tools_defined_empty_tools() {
        let payload = json!({
            "model": "claude-3-opus-bedrock",
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": []
        });
        assert!(!has_tools_defined(&payload));
    }

    #[test]
    fn test_apply_bedrock_compatibility_injects_placeholder() {
        let mut payload = json!({
            "model": "claude-3-opus-bedrock",
            "messages": [
                {"role": "user", "content": "Hello"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{}"
                            }
                        }
                    ]
                }
            ]
        });

        apply_bedrock_compatibility(&mut payload, "claude-3-opus-bedrock");

        // Should have tools injected
        assert!(payload.get("tools").is_some());
        let tools = payload.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0]["function"]["name"].as_str().unwrap(),
            "_placeholder_tool"
        );
    }

    #[test]
    fn test_apply_bedrock_compatibility_no_injection_for_non_bedrock() {
        let mut payload = json!({
            "model": "claude-3-opus",
            "messages": [
                {"role": "user", "content": "Hello"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{}"
                            }
                        }
                    ]
                }
            ]
        });

        apply_bedrock_compatibility(&mut payload, "claude-3-opus");

        // Should NOT have tools injected (not a Bedrock model)
        assert!(payload.get("tools").is_none());
    }

    #[test]
    fn test_apply_bedrock_compatibility_no_injection_with_existing_tools() {
        let mut payload = json!({
            "model": "claude-3-opus-bedrock",
            "messages": [
                {"role": "user", "content": "Hello"},
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "get_weather",
                                "arguments": "{}"
                            }
                        }
                    ]
                }
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather",
                        "parameters": {"type": "object", "properties": {}}
                    }
                }
            ]
        });

        apply_bedrock_compatibility(&mut payload, "claude-3-opus-bedrock");

        // Should still have original tools (not replaced)
        let tools = payload.get("tools").unwrap().as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(
            tools[0]["function"]["name"].as_str().unwrap(),
            "get_weather"
        );
    }

    #[test]
    fn test_apply_bedrock_compatibility_no_injection_without_tool_calls() {
        let mut payload = json!({
            "model": "claude-3-opus-bedrock",
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Hi there!"}
            ]
        });

        apply_bedrock_compatibility(&mut payload, "claude-3-opus-bedrock");

        // Should NOT have tools injected (no tool_calls in messages)
        assert!(payload.get("tools").is_none());
    }
}
