//! Protocol-aware proxy handler using transformer pipeline.
//!
//! This module provides a unified proxy handler that uses the transformer
//! system for protocol conversion between different LLM API formats.

use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use futures::StreamExt;
use serde_json::{json, Value};

use crate::api::handlers::AppState;
use crate::api::streaming::create_sse_stream;
use crate::core::config::CredentialConfig;
use crate::core::database::hash_key;
use crate::core::jsonl_logger::{
    log_provider_request, log_provider_response, log_request, log_response,
};
use crate::core::langfuse::{
    build_langfuse_tags, extract_client_metadata, get_langfuse_service, GenerationData,
};
use crate::core::logging::{generate_request_id, PROVIDER_CONTEXT};
use crate::core::metrics::get_metrics;
use crate::core::middleware::{ApiKeyName, HasCredentials, ModelName, ProviderName};
use crate::core::stream_metrics::{record_stream_metrics, StreamStats};
use crate::core::{AppError, Result};
use crate::transformer::{
    provider_type_to_protocol, CrossProtocolStreamState, Protocol, ProtocolDetector,
    TransformContext, TransformPipeline, TransformerRegistry,
};
use crate::with_request_context;

// ============================================================================
// Proxy State Extension
// ============================================================================

/// Extended AppState with transformer registry
#[derive(Clone)]
pub struct ProxyState {
    pub app_state: Arc<AppState>,
    pub transformer_registry: Arc<TransformerRegistry>,
    pub transform_pipeline: Arc<TransformPipeline>,
}

impl ProxyState {
    /// Create a new proxy state
    pub fn new(app_state: Arc<AppState>) -> Self {
        let registry = Arc::new(TransformerRegistry::new());
        let pipeline = Arc::new(TransformPipeline::new(registry.clone()));

        ProxyState {
            app_state,
            transformer_registry: registry,
            transform_pipeline: pipeline,
        }
    }
}

impl HasCredentials for ProxyState {
    fn get_credentials(&self) -> Vec<crate::core::config::CredentialConfig> {
        self.app_state.get_credentials()
    }
}

// ============================================================================
// Authentication Helpers
// ============================================================================

/// Verify API key authentication (supports both OpenAI and Claude formats)
pub fn verify_auth_multi_format(
    headers: &HeaderMap,
    state: &AppState,
) -> Result<Option<CredentialConfig>> {
    // Extract the provided key from Authorization header or x-api-key (Claude style)
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

    let credentials = state.get_credentials();

    if credentials.is_empty() {
        return Ok(None);
    }

    let provided_key = provided_key.ok_or(AppError::Unauthorized)?;
    let provided_key_hash = hash_key(provided_key);

    for credential_config in credentials {
        if credential_config.enabled && credential_config.credential_key == provided_key_hash {
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
fn strip_provider_suffix(model: &str, provider_suffix: Option<&str>) -> String {
    let Some(suffix) = provider_suffix.filter(|s| !s.is_empty()) else {
        return model.to_string();
    };

    model
        .strip_prefix(suffix)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(model)
        .to_string()
}

// ============================================================================
// Error Response Builders
// ============================================================================

/// Build an error response in the appropriate protocol format
pub fn build_protocol_error_response(
    protocol: Protocol,
    status: StatusCode,
    error_type: &str,
    message: &str,
    model: Option<&str>,
    provider: Option<&str>,
    api_key_name: Option<&str>,
) -> Response {
    let body = match protocol {
        Protocol::Anthropic => {
            json!({
                "type": "error",
                "error": {
                    "type": error_type,
                    "message": message
                }
            })
        }
        Protocol::OpenAI | Protocol::ResponseApi => {
            json!({
                "error": {
                    "message": message,
                    "type": error_type,
                    "code": status.as_u16()
                }
            })
        }
    };

    let mut response = Json(body).into_response();
    *response.status_mut() = status;

    if let Some(m) = model {
        response.extensions_mut().insert(ModelName(m.to_string()));
    }
    if let Some(p) = provider {
        response
            .extensions_mut()
            .insert(ProviderName(p.to_string()));
    }
    if let Some(k) = api_key_name {
        response.extensions_mut().insert(ApiKeyName(k.to_string()));
    }

    response
}

// ============================================================================
// Protocol-Aware Proxy Handler
// ============================================================================

/// Generic proxy handler context
pub struct ProxyContext {
    pub request_id: String,
    pub api_key_name: String,
    pub client_protocol: Protocol,
    pub provider_protocol: Protocol,
    pub original_model: String,
    pub mapped_model: String,
    pub provider_name: String,
    pub is_streaming: bool,
    pub generation_data: GenerationData,
    pub trace_id: Option<String>,
}

/// Handle a proxy request with protocol conversion
///
/// This is the main entry point for protocol-aware proxying.
/// It handles:
/// 1. Protocol detection (from path or request structure)
/// 2. Request transformation (client format → provider format)
/// 3. Upstream request execution
/// 4. Response transformation (provider format → client format)
pub async fn handle_proxy_request(
    state: Arc<ProxyState>,
    headers: HeaderMap,
    path: &str,
    payload: Value,
) -> Result<Response> {
    let request_start = Instant::now();
    let request_id = generate_request_id();
    let key_config = verify_auth_multi_format(&headers, &state.app_state)?;
    let api_key_name = get_key_name(&key_config);

    // Detect client protocol
    let client_protocol = ProtocolDetector::detect_with_path_hint(&payload, path);

    with_request_context!(request_id.clone(), api_key_name.clone(), async move {
        // Initialize Langfuse tracing
        let (trace_id, mut generation_data) =
            init_langfuse_trace(&request_id, &api_key_name, &headers, path);

        // Parse model from request
        let original_model = extract_model_from_request(&payload, client_protocol);
        let effective_model = strip_provider_suffix(
            &original_model,
            state.app_state.config.provider_suffix.as_deref(),
        );

        generation_data.original_model = effective_model.clone();
        generation_data.is_streaming = extract_stream_flag(&payload, client_protocol);

        // Select provider
        let provider_service = state.app_state.get_provider_service();
        let provider = match provider_service.get_next_provider(Some(&effective_model)) {
            Ok(p) => p,
            Err(err) => {
                tracing::error!(
                    request_id = %request_id,
                    error = %err,
                    model = %effective_model,
                    "Provider selection failed"
                );
                return Ok(build_protocol_error_response(
                    client_protocol,
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    &err,
                    Some(&effective_model),
                    None,
                    Some(&api_key_name),
                ));
            }
        };

        // Determine provider protocol from provider_type field
        let provider_protocol = provider_type_to_protocol(&provider.provider_type);

        // Build transform context
        let transform_ctx = TransformContext {
            request_id: request_id.clone(),
            client_protocol,
            provider_protocol,
            original_model: original_model.clone(),
            mapped_model: provider.get_mapped_model(&effective_model),
            provider_name: provider.name.clone(),
            stream: generation_data.is_streaming,
            ..Default::default()
        };

        // Update generation data
        generation_data.provider_key = provider.name.clone();
        generation_data.provider_type = provider_protocol.to_string();
        generation_data.provider_api_base = provider.api_base.clone();
        generation_data.mapped_model = transform_ctx.mapped_model.clone();

        // Transform request with bypass optimization
        let (provider_payload, bypassed) = state
            .transform_pipeline
            .transform_request_with_bypass(payload.clone(), &transform_ctx)?;

        // Record bypass or cross-protocol metrics
        let metrics = get_metrics();
        if bypassed {
            metrics
                .bypass_requests
                .with_label_values(&[&effective_model, &provider.name, path])
                .inc();
            tracing::debug!(
                request_id = %request_id,
                model = %effective_model,
                provider = %provider.name,
                "Using bypass mode (same-protocol optimization)"
            );
        } else {
            // Record cross-protocol transformation metric
            metrics
                .cross_protocol_requests
                .with_label_values(&[
                    &client_protocol.to_string(),
                    &provider_protocol.to_string(),
                    &provider.name,
                ])
                .inc();
        }

        let url = format!(
            "{}{}",
            provider.api_base,
            get_provider_endpoint(provider_protocol)
        );

        // Log request immediately to JSONL
        log_request(&request_id, path, &provider.name, &payload);

        // Execute request within provider context
        PROVIDER_CONTEXT
            .scope(provider.name.clone(), async move {
                // Log request info: DEBUG shows summary, TRACE shows full payload
                let messages_count = payload
                    .get("messages")
                    .and_then(|m| m.as_array())
                    .map(|arr| arr.len())
                    .unwrap_or(0);

                tracing::debug!(
                    request_id = %request_id,
                    provider = %provider.name,
                    model = %effective_model,
                    client_protocol = %client_protocol,
                    provider_protocol = %provider_protocol,
                    stream = generation_data.is_streaming,
                    messages_count = messages_count,
                    "Processing proxy request"
                );

                // TRACE level: log full original payload (client format)
                if tracing::enabled!(tracing::Level::TRACE) {
                    let payload_json = serde_json::to_string_pretty(&payload).unwrap_or_default();
                    tracing::trace!(
                        request_id = %request_id,
                        payload_bytes = payload_json.len(),
                        client_request = %payload_json,
                        "Original client request payload"
                    );

                    let provider_json =
                        serde_json::to_string_pretty(&provider_payload).unwrap_or_default();
                    tracing::trace!(
                        request_id = %request_id,
                        payload_bytes = provider_json.len(),
                        provider_request = %provider_json,
                        "Transformed provider request payload"
                    );
                }

                // Log provider request to JSONL
                let provider_endpoint = get_provider_endpoint(provider_protocol);
                log_provider_request(
                    &request_id,
                    &provider.name,
                    &provider.api_base,
                    provider_endpoint,
                    &provider_payload,
                );

                // Send request to provider
                // P0 fix: Handle Anthropic protocol with x-api-key header instead of Authorization Bearer
                let response = match if provider_protocol == Protocol::Anthropic {
                    // Anthropic API requires x-api-key header and anthropic-version header
                    let anthropic_version = headers
                        .get("anthropic-version")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("2023-06-01");

                    let mut req = state
                        .app_state
                        .http_client
                        .post(&url)
                        .header("x-api-key", &provider.api_key)
                        .header("anthropic-version", anthropic_version)
                        .header("Content-Type", "application/json");

                    // Forward anthropic-beta header if provided by client
                    if let Some(beta) = headers.get("anthropic-beta") {
                        if let Ok(beta_str) = beta.to_str() {
                            req = req.header("anthropic-beta", beta_str);
                        }
                    }

                    req.json(&provider_payload).send().await
                } else {
                    // OpenAI and other protocols use Authorization Bearer header
                    state
                        .app_state
                        .http_client
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", provider.api_key))
                        .header("Content-Type", "application/json")
                        .json(&provider_payload)
                        .send()
                        .await
                } {
                    Ok(resp) => resp,
                    Err(e) => {
                        tracing::error!(
                            request_id = %request_id,
                            provider = %provider.name,
                            error = %e,
                            "HTTP request failed"
                        );

                        let (status, error_type) = if e.is_timeout() {
                            (StatusCode::GATEWAY_TIMEOUT, "timeout_error")
                        } else {
                            (StatusCode::BAD_GATEWAY, "api_error")
                        };

                        return Ok(build_protocol_error_response(
                            client_protocol,
                            status,
                            error_type,
                            &format!("Upstream request failed: {}", e),
                            Some(&effective_model),
                            Some(&provider.name),
                            Some(&api_key_name),
                        ));
                    }
                };

                let status = response.status();

                // Handle error responses
                if status.is_client_error() || status.is_server_error() {
                    return handle_error_response(
                        response,
                        status,
                        client_protocol,
                        &effective_model,
                        &provider.name,
                        &api_key_name,
                    )
                    .await;
                }

                // Handle successful response
                if generation_data.is_streaming {
                    // Pre-calculate input tokens for usage fallback
                    let input_tokens = if let Some(messages) = payload.get("messages") {
                        if let Some(arr) = messages.as_array() {
                            let model_label = &transform_ctx.mapped_model;
                            let tools = payload.get("tools").and_then(|t| t.as_array());
                            let tool_choice = payload.get("tool_choice");
                            let mut combined_messages: Vec<Value> = Vec::new();

                            if let Some(system) = payload.get("system") {
                                let system_message = match system {
                                    Value::String(text) => json!({"role": "system", "content": text}),
                                    Value::Array(blocks) => json!({"role": "system", "content": blocks}),
                                    _ => json!({"role": "system", "content": ""}),
                                };
                                combined_messages.push(system_message);
                            }

                            combined_messages.extend(arr.iter().cloned());

                            let total_tokens = crate::api::streaming::calculate_message_tokens_with_tools(
                                &combined_messages,
                                model_label,
                                tools.map(|tool_list| tool_list.as_slice()),
                                tool_choice,
                            )
                            .ok();

                            tracing::debug!(
                                request_id = %request_id,
                                input_tokens = ?total_tokens,
                                "Pre-calculated input tokens for V2 streaming request"
                            );

                            total_tokens
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    handle_streaming_proxy_response(
                        response,
                        &state,
                        transform_ctx,
                        generation_data,
                        trace_id,
                        &api_key_name,
                        payload.clone(),
                        request_start,
                        path,
                        input_tokens,
                    )
                    .await
                } else {
                    handle_non_streaming_proxy_response(
                        response,
                        &state,
                        transform_ctx,
                        generation_data,
                        trace_id,
                        &api_key_name,
                        payload.clone(),
                        request_start,
                        path,
                    )
                    .await
                }
            })
            .await
    })
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Initialize Langfuse tracing
fn init_langfuse_trace(
    request_id: &str,
    api_key_name: &str,
    headers: &HeaderMap,
    endpoint: &str,
) -> (Option<String>, GenerationData) {
    let client_metadata = extract_client_metadata(headers);
    let user_agent = client_metadata.get("user_agent").cloned();

    let tags = build_langfuse_tags(endpoint, api_key_name, user_agent.as_deref());

    let langfuse = get_langfuse_service();
    let trace_id = if let Ok(service) = langfuse.read() {
        service.create_trace(request_id, api_key_name, endpoint, tags, client_metadata)
    } else {
        None
    };

    let generation_data = GenerationData {
        trace_id: trace_id.clone().unwrap_or_default(),
        request_id: request_id.to_string(),
        credential_name: api_key_name.to_string(),
        endpoint: endpoint.to_string(),
        start_time: Utc::now(),
        ..Default::default()
    };

    (trace_id, generation_data)
}

/// Extract model from request based on protocol
fn extract_model_from_request(payload: &Value, protocol: Protocol) -> String {
    match protocol {
        Protocol::OpenAI | Protocol::Anthropic => payload
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string(),
        Protocol::ResponseApi => payload
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string(),
    }
}

/// Extract stream flag from request based on protocol
fn extract_stream_flag(payload: &Value, protocol: Protocol) -> bool {
    match protocol {
        Protocol::OpenAI | Protocol::ResponseApi => payload
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false),
        Protocol::Anthropic => payload
            .get("stream")
            .and_then(|s| s.as_bool())
            .unwrap_or(false),
    }
}

/// Get the endpoint path for a provider protocol
fn get_provider_endpoint(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::OpenAI => "/chat/completions",
        Protocol::Anthropic => "/v1/messages",
        Protocol::ResponseApi => "/v1/responses",
    }
}

/// Handle error response from provider
async fn handle_error_response(
    response: reqwest::Response,
    status: reqwest::StatusCode,
    client_protocol: Protocol,
    model: &str,
    provider_name: &str,
    api_key_name: &str,
) -> Result<Response> {
    let default_message = format!("HTTP {} from {}", status, provider_name);

    let (error_body, raw_text) = match response.bytes().await {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).to_string();
            let json = serde_json::from_slice::<Value>(&bytes).ok();
            (json, text)
        }
        Err(e) => (None, format!("Failed to read response: {}", e)),
    };

    // Try to extract error message from various formats
    let error_message = if let Some(ref body) = error_body {
        // Try OpenAI format: {"error": {"message": "..."}}
        body.get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            // Try Anthropic format: {"type": "error", "error": {"message": "..."}}
            .or_else(|| {
                body.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
            // Try simple format: {"error": "..."}
            .or_else(|| {
                body.get("error")
                    .and_then(|e| e.as_str())
                    .map(|s| s.to_string())
            })
            // Try message field directly: {"message": "..."}
            .or_else(|| {
                body.get("message")
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
            // Use raw text if no structured message found
            .unwrap_or_else(|| {
                if raw_text.is_empty() {
                    default_message.clone()
                } else {
                    // Truncate long error bodies
                    if raw_text.len() > 500 {
                        format!("{}...", &raw_text[..500])
                    } else {
                        raw_text.clone()
                    }
                }
            })
    } else if raw_text.is_empty() {
        default_message.clone()
    } else {
        raw_text.clone()
    };

    // TRACE level: log full error response body
    if tracing::enabled!(tracing::Level::TRACE) {
        tracing::trace!(
            provider = %provider_name,
            status = %status,
            raw_body = %raw_text,
            "Full error response body from provider"
        );
    }

    tracing::error!(
        provider = %provider_name,
        status = %status,
        error_message = %error_message,
        "Backend API returned error"
    );

    Ok(build_protocol_error_response(
        client_protocol,
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        "api_error",
        &error_message,
        Some(model),
        Some(provider_name),
        Some(api_key_name),
    ))
}

/// Handle streaming response with protocol conversion
#[allow(clippy::too_many_arguments)]
async fn handle_streaming_proxy_response(
    response: reqwest::Response,
    state: &Arc<ProxyState>,
    ctx: TransformContext,
    generation_data: GenerationData,
    trace_id: Option<String>,
    api_key_name: &str,
    request_payload: Value,
    request_start: Instant,
    endpoint: &str,
    input_tokens: Option<usize>,
) -> Result<Response> {
    let client_protocol = ctx.client_protocol;
    let provider_protocol = ctx.provider_protocol;
    let model_label = ctx.original_model.clone();
    let provider_name = ctx.provider_name.clone();
    let request_id = ctx.request_id.clone();

    // For same-protocol streaming, we can use bypass optimization
    if client_protocol == provider_protocol {
        // Direct passthrough with model rewriting
        let langfuse_data = if trace_id.is_some() {
            Some(generation_data)
        } else {
            None
        };

        match create_sse_stream(
            response,
            model_label.clone(),
            provider_name.clone(),
            input_tokens,
            state.app_state.config.ttft_timeout_secs,
            langfuse_data,
            Some(request_id.clone()),
            Some(endpoint.to_string()),
            Some(request_payload.clone()),
        )
        .await
        {
            Ok(sse_stream) => {
                let mut resp = sse_stream.into_response();
                resp.extensions_mut().insert(ModelName(model_label));
                resp.extensions_mut().insert(ProviderName(provider_name));
                resp.extensions_mut()
                    .insert(ApiKeyName(api_key_name.to_string()));
                Ok(resp)
            }
            Err(e) => {
                tracing::error!(provider = %provider_name, error = %e, "Streaming error");
                Ok(build_protocol_error_response(
                    client_protocol,
                    StatusCode::GATEWAY_TIMEOUT,
                    "timeout_error",
                    &e.to_string(),
                    Some(&model_label),
                    Some(&provider_name),
                    Some(api_key_name),
                ))
            }
        }
    } else {
        // Cross-protocol streaming requires chunk-by-chunk transformation
        // with state tracking to emit proper event sequences
        // Use unfold pattern to enable metrics recording at stream end
        use futures::stream::Stream;
        use std::pin::Pin;

        struct CrossProtocolStreamingState {
            stream: Pin<Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>>,
            stream_state: CrossProtocolStreamState,
            registry: Arc<TransformerRegistry>,
            client_protocol: Protocol,
            provider_protocol: Protocol,
            // Metrics tracking
            model: String,
            provider: String,
            api_key_name: String,
            start_time: Instant,
            first_token_time: Option<Instant>,
        }

        let streaming_state = CrossProtocolStreamingState {
            stream: Box::pin(response.bytes_stream()),
            stream_state: CrossProtocolStreamState::with_input_tokens(&model_label, input_tokens),
            registry: state.transformer_registry.clone(),
            client_protocol,
            provider_protocol,
            model: model_label.clone(),
            provider: provider_name.clone(),
            api_key_name: api_key_name.to_string(),
            start_time: request_start,
            first_token_time: None,
        };

        let transform_stream =
            futures::stream::unfold(streaming_state, |mut state| async move {
                match state.stream.next().await {
                    Some(Ok(bytes)) => {
                        // Track first token time
                        if state.first_token_time.is_none() {
                            state.first_token_time = Some(Instant::now());
                        }

                        // Get transformers
                        let provider_transformer = state.registry.get(state.provider_protocol);
                        let client_transformer = state.registry.get(state.client_protocol);

                        let output = if let (Some(provider_t), Some(client_t)) =
                            (provider_transformer, client_transformer)
                        {
                            // Transform chunks from provider format to unified format
                            match provider_t.transform_stream_chunk_in(&bytes) {
                                Ok(unified_chunks) => {
                                    // Process chunks through state tracker
                                    let processed_chunks =
                                        state.stream_state.process_chunks(unified_chunks);

                                    // Transform unified chunks to client format
                                    let mut output = String::new();
                                    for chunk in processed_chunks {
                                        if let Ok(formatted) = client_t
                                            .transform_stream_chunk_out(&chunk, state.client_protocol)
                                        {
                                            output.push_str(&formatted);
                                        }
                                    }
                                    axum::body::Bytes::from(output)
                                }
                                Err(_) => bytes,
                            }
                        } else {
                            bytes
                        };

                        Some((Ok::<_, std::io::Error>(output), state))
                    }
                    Some(Err(e)) => {
                        Some((Err(std::io::Error::other(e.to_string())), state))
                    }
                    None => {
                        // Stream ended - record metrics
                        let final_usage = state.stream_state.get_final_usage(None);
                        let stats = StreamStats {
                            model: state.model.clone(),
                            provider: state.provider.clone(),
                            api_key_name: state.api_key_name.clone(),
                            input_tokens: final_usage
                                .as_ref()
                                .map(|u| u.input_tokens as usize)
                                .unwrap_or(0),
                            output_tokens: final_usage
                                .as_ref()
                                .map(|u| u.output_tokens as usize)
                                .unwrap_or(0),
                            start_time: state.start_time,
                            first_token_time: state.first_token_time,
                        };
                        record_stream_metrics(&stats);
                        None
                    }
                }
            });

        let body = Body::from_stream(transform_stream);

        let content_type = match client_protocol {
            Protocol::Anthropic => "text/event-stream",
            _ => "text/event-stream",
        };

        let mut resp = Response::builder()
            .status(200)
            .header("Content-Type", content_type)
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(body)
            .unwrap();

        resp.extensions_mut().insert(ModelName(model_label));
        resp.extensions_mut().insert(ProviderName(provider_name));
        resp.extensions_mut()
            .insert(ApiKeyName(api_key_name.to_string()));

        Ok(resp)
    }
}

/// Handle non-streaming response with protocol conversion
#[allow(clippy::too_many_arguments)]
async fn handle_non_streaming_proxy_response(
    response: reqwest::Response,
    state: &Arc<ProxyState>,
    ctx: TransformContext,
    mut generation_data: GenerationData,
    trace_id: Option<String>,
    api_key_name: &str,
    _request_payload: Value,
    _request_start: Instant,
    _endpoint: &str,
) -> Result<Response> {
    let client_protocol = ctx.client_protocol;
    let model_label = ctx.original_model.clone();
    let provider_name = ctx.provider_name.clone();
    let request_id = ctx.request_id.clone();

    let status = response.status();

    // Parse response JSON
    let response_data: Value = match response.json().await {
        Ok(data) => data,
        Err(e) => {
            tracing::error!(provider = %provider_name, error = %e, "Failed to parse response");
            return Ok(build_protocol_error_response(
                client_protocol,
                StatusCode::BAD_GATEWAY,
                "api_error",
                &format!("Invalid JSON from provider: {}", e),
                Some(&model_label),
                Some(&provider_name),
                Some(api_key_name),
            ));
        }
    };

    // Log provider response to JSONL (raw response before transformation)
    log_provider_response(
        &request_id,
        &provider_name,
        status.as_u16(),
        None,
        &response_data,
    );

    // DEBUG level: log response summary
    tracing::debug!(
        request_id = %request_id,
        provider = %provider_name,
        model = %model_label,
        status = %status,
        "[Response] Non-streaming response received"
    );

    // DEBUG level: log response body (compact JSON for readability)
    if tracing::enabled!(tracing::Level::DEBUG) {
        let response_json = serde_json::to_string(&response_data).unwrap_or_default();
        // Truncate very long responses for log readability
        let truncated = if response_json.len() > 2000 {
            format!(
                "{}... (truncated, {} bytes total)",
                &response_json[..2000],
                response_json.len()
            )
        } else {
            response_json
        };
        tracing::debug!(
            request_id = %request_id,
            "[Response Body] {}",
            truncated
        );
    }

    // TRACE level: log full provider response before transformation (pretty-printed)
    if tracing::enabled!(tracing::Level::TRACE) {
        let response_json = serde_json::to_string_pretty(&response_data).unwrap_or_default();
        tracing::trace!(
            request_id = %request_id,
            response_bytes = response_json.len(),
            provider_response = %response_json,
            "Provider response payload (before transformation)"
        );
    }

    // Transform response using pipeline with bypass optimization
    let (client_response, bypassed) = state
        .transform_pipeline
        .transform_response_with_bypass(response_data, &ctx)?;

    // Record bypass metrics for response
    if bypassed {
        tracing::debug!(
            request_id = %request_id,
            model = %model_label,
            provider = %provider_name,
            "Response bypass mode (same-protocol optimization)"
        );
    }

    // TRACE level: log transformed client response
    if tracing::enabled!(tracing::Level::TRACE) {
        let client_json = serde_json::to_string_pretty(&client_response).unwrap_or_default();
        tracing::trace!(
            request_id = %request_id,
            response_bytes = client_json.len(),
            client_response = %client_json,
            "Transformed client response payload"
        );
    }

    // Record Langfuse
    generation_data.end_time = Some(Utc::now());
    if trace_id.is_some() {
        if let Ok(service) = get_langfuse_service().read() {
            service.trace_generation(generation_data);
        }
    }

    // Log response to JSONL file (non-streaming)
    log_response(&request_id, status.as_u16(), None, &client_response);

    let mut resp = Json(client_response).into_response();
    resp.extensions_mut().insert(ModelName(model_label));
    resp.extensions_mut().insert(ProviderName(provider_name));
    resp.extensions_mut()
        .insert(ApiKeyName(api_key_name.to_string()));

    Ok(resp)
}

// ============================================================================
// Endpoint Handlers
// ============================================================================

/// OpenAI-compatible chat completions endpoint using transformer pipeline
pub async fn chat_completions_v2(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response> {
    handle_proxy_request(state, headers, "/v1/chat/completions", payload).await
}

/// Anthropic-compatible messages endpoint using transformer pipeline
pub async fn messages_v2(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response> {
    handle_proxy_request(state, headers, "/v1/messages", payload).await
}

/// Response API endpoint using transformer pipeline
pub async fn responses_v2(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response> {
    handle_proxy_request(state, headers, "/v1/responses", payload).await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_provider_suffix() {
        assert_eq!(strip_provider_suffix("Proxy/gpt-4", Some("Proxy")), "gpt-4");
        assert_eq!(strip_provider_suffix("gpt-4", Some("Proxy")), "gpt-4");
        assert_eq!(strip_provider_suffix("Proxy/gpt-4", None), "Proxy/gpt-4");
    }

    #[test]
    fn test_extract_model_from_request() {
        let payload = json!({ "model": "gpt-4", "messages": [] });
        assert_eq!(
            extract_model_from_request(&payload, Protocol::OpenAI),
            "gpt-4"
        );
    }

    #[test]
    fn test_extract_stream_flag() {
        let payload = json!({ "model": "gpt-4", "stream": true });
        assert!(extract_stream_flag(&payload, Protocol::OpenAI));

        let payload = json!({ "model": "gpt-4" });
        assert!(!extract_stream_flag(&payload, Protocol::OpenAI));
    }

    #[test]
    fn test_get_provider_endpoint() {
        assert_eq!(get_provider_endpoint(Protocol::OpenAI), "/chat/completions");
        assert_eq!(get_provider_endpoint(Protocol::Anthropic), "/v1/messages");
        assert_eq!(
            get_provider_endpoint(Protocol::ResponseApi),
            "/v1/responses"
        );
    }
}
