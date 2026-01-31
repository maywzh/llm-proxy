//! Claude API compatible endpoints.
//!
//! This module provides Claude Messages API compatibility by converting
//! Claude format requests to OpenAI format, proxying to providers,
//! and converting responses back to Claude format.

use crate::api::claude_models::{
    ClaudeContentBlock, ClaudeErrorResponse, ClaudeMessage, ClaudeMessageContent,
    ClaudeMessagesRequest, ClaudeSystemPrompt, ClaudeTokenCountRequest, ClaudeTokenCountResponse,
    ClaudeTool,
};
use crate::api::handlers::AppState;
use crate::api::streaming::calculate_message_tokens_with_tools;
use crate::core::config::CredentialConfig;
use crate::core::database::hash_key;
use crate::core::jsonl_logger::{
    log_provider_request, log_provider_response, log_provider_streaming_response, log_request,
    log_response, log_streaming_response,
};
use crate::core::langfuse::{
    build_langfuse_tags, extract_client_metadata, get_langfuse_service, GenerationData,
};
use crate::core::logging::{generate_request_id, get_api_key_name, PROVIDER_CONTEXT, REQUEST_ID};
use crate::core::metrics::get_metrics;
use crate::core::middleware::{ApiKeyName, ModelName, ProviderName};
use crate::core::stream_metrics::{record_stream_metrics, StreamStats};
use crate::core::{AppError, Result};
use crate::services::claude_converter::{
    claude_to_openai_request, convert_openai_streaming_to_claude, openai_to_claude_response,
};
use crate::with_request_context;
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
use std::sync::Arc;
use std::time::Instant;

// ============================================================================
// Authentication
// ============================================================================

/// Verify API key authentication and check rate limits for Claude API.
fn verify_auth(headers: &HeaderMap, state: &AppState) -> Result<Option<CredentialConfig>> {
    // Extract the provided key from Authorization header
    // Claude API uses "x-api-key" header, but we also support "Authorization: Bearer"
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
// Error Response Builder
// ============================================================================

/// Build a Claude-formatted error response with optional extensions.
fn build_claude_error_response(
    status: StatusCode,
    error_type: &str,
    message: &str,
    model: Option<&str>,
    provider: Option<&str>,
    api_key_name: Option<&str>,
) -> Response {
    let error = ClaudeErrorResponse::new(error_type, message);
    let mut response = Json(error).into_response();
    *response.status_mut() = status;

    // Add extensions for middleware metrics tracking
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
// Handlers
// ============================================================================

/// Claude Messages API endpoint.
///
/// Converts Claude API requests to OpenAI format, proxies to provider,
/// and converts response back to Claude format.
///
/// Supports both streaming and non-streaming modes.
#[utoipa::path(
    post,
    path = "/v1/messages",
    tag = "claude",
    request_body = ClaudeMessagesRequest,
    responses(
        (status = 200, description = "Claude message response"),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized"),
        (status = 429, description = "Rate limit exceeded"),
        (status = 502, description = "Bad gateway - upstream error"),
        (status = 504, description = "Gateway timeout")
    ),
    security(
        ("api_key" = []),
        ("bearer_auth" = [])
    )
)]
#[tracing::instrument(skip(state, headers, claude_request))]
pub async fn create_message(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(claude_request): Json<ClaudeMessagesRequest>,
) -> Result<Response> {
    let request_id = generate_request_id();
    let key_config = verify_auth(&headers, &state)?;
    let api_key_name = get_key_name(&key_config);

    with_request_context!(request_id.clone(), api_key_name.clone(), async move {
        // Extract client metadata from headers for Langfuse tracing using shared helper
        let client_metadata = extract_client_metadata(&headers);
        let user_agent = client_metadata.get("user_agent").cloned();

        // Build tags for Langfuse using shared helper
        let tags = build_langfuse_tags("/v1/messages", &api_key_name, user_agent.as_deref());

        // Initialize Langfuse tracing
        let langfuse = get_langfuse_service();
        let trace_id = if let Ok(service) = langfuse.read() {
            service.create_trace(
                &request_id,
                &api_key_name,
                "/v1/messages",
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
            endpoint: "/v1/messages".to_string(),
            start_time: Utc::now(),
            ..Default::default()
        };

        tracing::debug!(
            request_id = %request_id,
            model = %claude_request.model,
            stream = claude_request.stream,
            "Processing Claude request"
        );

        // Strip provider suffix if configured
        let effective_model = strip_provider_suffix(
            &claude_request.model,
            state.config.provider_suffix.as_deref(),
        );
        let model_label = effective_model.clone();

        // Capture input data for Langfuse
        generation_data.original_model = model_label.clone();
        generation_data.is_streaming = claude_request.stream;

        // Get fresh provider service from DynamicConfig
        let provider_service = state.get_provider_service();

        // Select provider based on the effective model
        let provider = match provider_service.get_next_provider(Some(&effective_model)) {
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
                return Ok(build_claude_error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    &err,
                    Some(&model_label),
                    None, // No provider available
                    Some(&api_key_name),
                ));
            }
        };

        // Capture provider info for Langfuse
        generation_data.provider_key = provider.name.clone();
        generation_data.provider_type = provider.provider_type.clone();
        generation_data.provider_api_base = provider.api_base.clone();
        // Use pattern-aware model mapping
        let mapped_model = provider.get_mapped_model(&model_label);
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

        // Convert Claude request to OpenAI format
        let openai_request = claude_to_openai_request(
            &claude_request,
            Some(&provider.model_mapping),
            state.config.min_tokens_limit,
            state.config.max_tokens_limit,
        );

        // Capture input messages for Langfuse tracing
        generation_data.input_messages = openai_request
            .get("messages")
            .and_then(|m| m.as_array())
            .map(|arr| arr.to_vec())
            .unwrap_or_default();

        // Capture model parameters for Langfuse
        let param_keys = ["temperature", "max_tokens", "top_p", "stop"];
        for key in param_keys {
            if let Some(val) = openai_request.get(key) {
                generation_data
                    .model_parameters
                    .insert(key.to_string(), val.clone());
            }
        }

        // Determine URL based on provider_type
        let is_anthropic = provider.provider_type == "anthropic";
        let url = if is_anthropic {
            format!("{}/v1/messages", provider.api_base)
        } else {
            format!("{}/chat/completions", provider.api_base)
        };

        // Execute request within provider context scope
        PROVIDER_CONTEXT
            .scope(provider.name.clone(), async move {
                tracing::debug!(
                    request_id = %request_id,
                    provider = %provider.name,
                    provider_type = ?provider.provider_type,
                    model = %model_label,
                    stream = claude_request.stream,
                    "Processing Claude completion request"
                );

                // Log request immediately to JSONL
                let claude_request_value =
                    serde_json::to_value(&claude_request).unwrap_or_default();
                log_request(
                    &request_id,
                    "/v1/messages",
                    &provider.name,
                    &claude_request_value,
                );

                // Get api_key_name from context for use in closures
                let api_key_name = get_api_key_name();

                // Log provider request to JSONL
                let provider_endpoint = if is_anthropic { "/v1/messages" } else { "/chat/completions" };
                log_provider_request(
                    &request_id,
                    &provider.name,
                    &provider.api_base,
                    provider_endpoint,
                    if is_anthropic { &claude_request_value } else { &openai_request },
                );

                // Build and send request based on provider_type
                let response = if is_anthropic {
                    // Anthropic protocol: use x-api-key and forward anthropic-specific headers
                    let mut req = state
                        .http_client
                        .post(&url)
                        .header("x-api-key", &provider.api_key)
                        .header("Content-Type", "application/json");

                    // Forward anthropic-version header (use client value or default)
                    let anthropic_version = headers
                        .get("anthropic-version")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("2023-06-01");
                    req = req.header("anthropic-version", anthropic_version);

                    // Forward anthropic-beta header if provided by client
                    if let Some(beta) = headers.get("anthropic-beta").and_then(|v| v.to_str().ok()) {
                        req = req.header("anthropic-beta", beta);
                    }

                    req.json(&claude_request).send().await
                } else {
                    // OpenAI protocol: use Authorization Bearer
                    state
                        .http_client
                        .post(&url)
                        .header("Authorization", format!("Bearer {}", provider.api_key))
                        .header("Content-Type", "application/json")
                        .json(&openai_request)
                        .send()
                        .await
                };

                let response = match response
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

                        // Record error in Langfuse
                        generation_data.is_error = true;
                        generation_data.error_message =
                            Some(format!("Upstream request failed: {}", e));
                        generation_data.end_time = Some(Utc::now());
                        if trace_id.is_some() {
                            if let Ok(service) = langfuse.read() {
                                service.trace_generation(generation_data);
                            }
                        }

                        let (status, error_type) = if e.is_timeout() {
                            (StatusCode::GATEWAY_TIMEOUT, "timeout_error")
                        } else {
                            (StatusCode::BAD_GATEWAY, "api_error")
                        };
                        return Ok(build_claude_error_response(
                            status,
                            error_type,
                            &format!("Upstream request failed: {}", e),
                            Some(&model_label),
                            Some(&provider.name),
                            Some(&api_key_name),
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
                if status.is_client_error() || status.is_server_error() {
                    let error_body = match response.bytes().await {
                        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                            Ok(body) => body,
                            Err(_) => {
                                let text = String::from_utf8_lossy(&bytes).to_string();
                                json!({"error": {"message": text}})
                            }
                        },
                        Err(_) => json!({"error": {"message": format!("HTTP {}", status)}}),
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
                    generation_data.error_message = Some(error_message.clone());
                    generation_data.end_time = Some(Utc::now());
                    if trace_id.is_some() {
                        if let Ok(service) = langfuse.read() {
                            service.trace_generation(generation_data);
                        }
                    }

                    return Ok(build_claude_error_response(
                        StatusCode::from_u16(status.as_u16())
                            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        "api_error",
                        &error_message,
                        Some(&model_label),
                        Some(&provider.name),
                        Some(&api_key_name),
                    ));
                }

                if claude_request.stream {
                    // Calculate input tokens for fallback (only used if provider doesn't return usage)
                    let fallback_input_tokens = calculate_claude_input_tokens(&claude_request);
                    tracing::debug!(
                        "[USAGE_DEBUG] Calculated fallback_input_tokens: {:?} for model: {}",
                        fallback_input_tokens,
                        claude_request.model
                    );

                    // Handle streaming response
                    handle_streaming_response(
                        response,
                        claude_request.model.clone(),
                        model_label,
                        provider.name.clone(),
                        api_key_name,
                        generation_data,
                        trace_id,
                        request_id.clone(),
                        serde_json::to_value(&claude_request).unwrap_or_default(),
                        fallback_input_tokens,
                    )
                    .await
                } else {
                    // Handle non-streaming response
                    handle_non_streaming_response(
                        response,
                        claude_request.model.clone(),
                        model_label,
                        provider.name.clone(),
                        api_key_name,
                        generation_data,
                        trace_id,
                        request_id.clone(),
                        serde_json::to_value(&claude_request).unwrap_or_default(),
                    )
                    .await
                }
            })
            .await
    })
}

/// Handle streaming Claude response.
#[allow(clippy::too_many_arguments)]
async fn handle_streaming_response(
    response: reqwest::Response,
    original_model: String,
    model_label: String,
    provider_name: String,
    api_key_name: String,
    generation_data: GenerationData,
    trace_id: Option<String>,
    request_id: String,
    request_payload: serde_json::Value,
    fallback_input_tokens: Option<usize>,
) -> Result<Response> {
    let stream = response.bytes_stream();
    let start_time = std::time::Instant::now();

    // Convert OpenAI streaming to Claude streaming format
    let claude_stream = convert_openai_streaming_to_claude(
        Box::pin(stream),
        original_model,
        fallback_input_tokens,
    );

    // Wrap the stream to capture output for Langfuse and JSONL logging
    let model_label_clone = model_label.clone();
    let provider_name_clone = provider_name.clone();
    let api_key_name_clone = api_key_name.clone();
    let trace_id_clone = trace_id.clone();
    let request_id_clone = request_id.clone();
    let _request_payload_clone = request_payload.clone();

    let langfuse_stream = {
        let accumulated_output = String::new();
        let finish_reason: Option<String> = None;
        let usage_input_tokens: u32 = 0;
        let usage_output_tokens: u32 = 0;
        let first_token_received = false;
        let first_token_time: Option<Instant> = None;
        let accumulated_sse_data: Vec<String> = Vec::new();

        futures::stream::unfold(
            (
                claude_stream,
                accumulated_output,
                finish_reason,
                usage_input_tokens,
                usage_output_tokens,
                first_token_received,
                first_token_time,
                generation_data,
                trace_id_clone,
                accumulated_sse_data,
                start_time,
                model_label.clone(),
                provider_name.clone(),
                request_id.clone(),
                api_key_name.clone(),
            ),
            move |(
                mut stream,
                mut accumulated_output,
                mut finish_reason,
                mut usage_input_tokens,
                mut usage_output_tokens,
                mut first_token_received,
                mut first_token_time,
                mut gen_data,
                trace_id,
                mut accumulated_sse_data,
                start_time,
                model_label,
                provider_name,
                request_id,
                api_key_name,
            )| {
                async move {
                    match stream.next().await {
                        Some(event) => {
                            // Track TTFT
                            if !first_token_received {
                                first_token_received = true;
                                first_token_time = Some(Instant::now());
                                if trace_id.is_some() {
                                    gen_data.ttft_time = Some(Utc::now());
                                }
                            }

                            // Accumulate raw SSE data for JSONL logging
                            if !request_id.is_empty() {
                                // Store raw SSE lines for JSONL logging
                                for line in event.lines() {
                                    if line.starts_with("data: ") {
                                        accumulated_sse_data.push(line.to_string());
                                    }
                                }
                            }

                            // Parse event to extract output content and usage
                            if event.starts_with("event: content_block_delta") {
                                // Extract text from content_block_delta events
                                for line in event.lines() {
                                    if let Some(data_str) = line.strip_prefix("data: ") {
                                        if let Ok(data) =
                                            serde_json::from_str::<serde_json::Value>(data_str)
                                        {
                                            if let Some(delta) = data.get("delta") {
                                                if delta.get("type").and_then(|t| t.as_str())
                                                    == Some("text_delta")
                                                {
                                                    if let Some(text) =
                                                        delta.get("text").and_then(|t| t.as_str())
                                                    {
                                                        accumulated_output.push_str(text);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if event.starts_with("event: message_delta") {
                                // Extract finish_reason and usage from message_delta events
                                for line in event.lines() {
                                    if let Some(data_str) = line.strip_prefix("data: ") {
                                        if let Ok(data) =
                                            serde_json::from_str::<serde_json::Value>(data_str)
                                        {
                                            if let Some(delta) = data.get("delta") {
                                                if let Some(reason) = delta
                                                    .get("stop_reason")
                                                    .and_then(|r| r.as_str())
                                                {
                                                    finish_reason = Some(reason.to_string());
                                                }
                                            }
                                            if let Some(usage) = data.get("usage") {
                                                if let Some(input) = usage
                                                    .get("input_tokens")
                                                    .and_then(|t| t.as_u64())
                                                {
                                                    usage_input_tokens = input as u32;
                                                }
                                                if let Some(output) = usage
                                                    .get("output_tokens")
                                                    .and_then(|t| t.as_u64())
                                                {
                                                    usage_output_tokens = output as u32;
                                                }
                                            }
                                        }
                                    }
                                }
                            } else if event.starts_with("event: message_stop") {
                                // Stream is ending, record Langfuse generation
                                if trace_id.is_some() {
                                    gen_data.output_content = Some(accumulated_output.clone());
                                    gen_data.finish_reason = finish_reason.clone();
                                    gen_data.prompt_tokens = usage_input_tokens;
                                    gen_data.completion_tokens = usage_output_tokens;
                                    gen_data.total_tokens =
                                        usage_input_tokens + usage_output_tokens;
                                    gen_data.end_time = Some(Utc::now());
                                    if let Ok(service) = get_langfuse_service().read() {
                                        service.trace_generation(gen_data.clone());
                                    }
                                }

                                // Record stream metrics using unified function
                                let stats = StreamStats {
                                    model: model_label.clone(),
                                    provider: provider_name.clone(),
                                    api_key_name: api_key_name.clone(),
                                    input_tokens: usage_input_tokens as usize,
                                    output_tokens: usage_output_tokens as usize,
                                    start_time,
                                    first_token_time,
                                };
                                record_stream_metrics(&stats);

                                // Log streaming response to JSONL file
                                if !request_id.is_empty() {
                                    log_streaming_response(
                                        &request_id,
                                        200,
                                        None,
                                        accumulated_sse_data.clone(),
                                    );
                                    // Log provider streaming response (the raw SSE data from provider)
                                    log_provider_streaming_response(
                                        &request_id,
                                        &provider_name,
                                        200,
                                        None,
                                        accumulated_sse_data.clone(),
                                    );
                                }
                            }

                            Some((
                                Ok::<_, std::io::Error>(axum::body::Bytes::from(event)),
                                (
                                    stream,
                                    accumulated_output,
                                    finish_reason,
                                    usage_input_tokens,
                                    usage_output_tokens,
                                    first_token_received,
                                    first_token_time,
                                    gen_data,
                                    trace_id,
                                    accumulated_sse_data,
                                    start_time,
                                    model_label,
                                    provider_name,
                                    request_id,
                                    api_key_name,
                                ),
                            ))
                        }
                        None => None,
                    }
                }
            },
        )
    };

    let body = Body::from_stream(langfuse_stream);

    // Note: Metrics are recorded by middleware via response extensions

    tracing::debug!(
        model = %model_label_clone,
        provider = %provider_name_clone,
        api_key_name = %api_key_name_clone,
        request_id = %request_id_clone,
        "Claude streaming response started"
    );

    let mut response = Response::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap();

    // Add extensions for middleware metrics tracking
    response
        .extensions_mut()
        .insert(ModelName(model_label_clone));
    response
        .extensions_mut()
        .insert(ProviderName(provider_name_clone));
    response
        .extensions_mut()
        .insert(ApiKeyName(api_key_name_clone));

    Ok(response)
}

/// Handle non-streaming Claude response.
#[allow(clippy::too_many_arguments)]
async fn handle_non_streaming_response(
    response: reqwest::Response,
    original_model: String,
    model_label: String,
    provider_name: String,
    api_key_name: String,
    mut generation_data: GenerationData,
    trace_id: Option<String>,
    request_id: String,
    _request_payload: serde_json::Value,
) -> Result<Response> {
    let status = response.status();

    let response_data: serde_json::Value = match response.json().await {
        Ok(data) => {
            // Log provider response to JSONL (the raw OpenAI-format response from provider)
            log_provider_response(&request_id, &provider_name, status.as_u16(), None, &data);
            data
        }
        Err(e) => {
            tracing::error!(
                provider = %provider_name,
                error = %e,
                "Failed to parse provider response JSON"
            );
            return Ok(build_claude_error_response(
                StatusCode::BAD_GATEWAY,
                "api_error",
                &format!("Invalid JSON from provider: {}", e),
                Some(&model_label),
                Some(&provider_name),
                Some(&api_key_name),
            ));
        }
    };

    // Convert OpenAI response to Claude format
    let claude_response = match openai_to_claude_response(&response_data, &original_model) {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!(
                provider = %provider_name,
                error = %e,
                "Failed to convert OpenAI response to Claude format"
            );
            return Ok(build_claude_error_response(
                StatusCode::BAD_GATEWAY,
                "api_error",
                &format!("Failed to convert response: {}", e),
                Some(&model_label),
                Some(&provider_name),
                Some(&api_key_name),
            ));
        }
    };

    // Capture usage for Langfuse and metrics
    if let Some(usage_obj) = response_data.get("usage") {
        if let (Some(prompt_tokens), Some(completion_tokens)) = (
            usage_obj.get("prompt_tokens").and_then(|t| t.as_u64()),
            usage_obj.get("completion_tokens").and_then(|t| t.as_u64()),
        ) {
            generation_data.prompt_tokens = prompt_tokens as u32;
            generation_data.completion_tokens = completion_tokens as u32;
            generation_data.total_tokens = (prompt_tokens + completion_tokens) as u32;

            // Record token metrics
            let metrics = get_metrics();
            metrics
                .token_usage
                .with_label_values(&[&model_label, &provider_name, "prompt", &api_key_name])
                .inc_by(prompt_tokens);
            metrics
                .token_usage
                .with_label_values(&[&model_label, &provider_name, "completion", &api_key_name])
                .inc_by(completion_tokens);
            metrics
                .token_usage
                .with_label_values(&[&model_label, &provider_name, "total", &api_key_name])
                .inc_by(prompt_tokens + completion_tokens);
        }
    }

    // Record successful generation in Langfuse
    generation_data.end_time = Some(Utc::now());
    if trace_id.is_some() {
        if let Ok(service) = get_langfuse_service().read() {
            service.trace_generation(generation_data);
        }
    }

    // Log response to JSONL file (non-streaming)
    log_response(
        &request_id,
        status.as_u16(),
        None,
        &serde_json::to_value(&claude_response).unwrap_or_default(),
    );

    // Note: Request count metrics are recorded by middleware via response extensions

    tracing::debug!(
        model = %model_label,
        provider = %provider_name,
        api_key_name = %api_key_name,
        request_id = %request_id,
        "Claude non-streaming response completed"
    );

    let mut response = Json(claude_response).into_response();
    response.extensions_mut().insert(ModelName(model_label));
    response
        .extensions_mut()
        .insert(ProviderName(provider_name));
    response.extensions_mut().insert(ApiKeyName(api_key_name));

    Ok(response)
}

/// Claude token counting endpoint.
///
/// Provides accurate token count for the given messages using tiktoken.
#[utoipa::path(
    post,
    path = "/v1/messages/count_tokens",
    tag = "claude",
    request_body = ClaudeTokenCountRequest,
    responses(
        (status = 200, description = "Token count response", body = ClaudeTokenCountResponse),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized")
    ),
    security(
        ("api_key" = []),
        ("bearer_auth" = [])
    )
)]
#[tracing::instrument(skip(state, headers, claude_request))]
pub async fn count_tokens(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(claude_request): Json<ClaudeTokenCountRequest>,
) -> Result<Json<ClaudeTokenCountResponse>> {
    let request_id = generate_request_id();
    let _key_config = verify_auth(&headers, &state)?;

    REQUEST_ID
        .scope(request_id.clone(), async move {
            let model = &claude_request.model;
            let messages_value = build_claude_messages_for_token_count(
                &claude_request.system,
                &claude_request.messages,
            );
            let tools_value = build_openai_tools_for_token_count(&claude_request.tools);
            let tool_choice = claude_request.tool_choice.as_ref();
            let total_tokens = calculate_message_tokens_with_tools(
                &messages_value,
                model,
                tools_value.as_deref(),
                tool_choice,
            )
            .map_err(AppError::BadRequest)?;

            let estimated_tokens = std::cmp::max(1, total_tokens) as i32;

            Ok(Json(ClaudeTokenCountResponse {
                input_tokens: estimated_tokens,
            }))
        })
        .await
}

// ============================================================================
// Token Counting Helpers
// ============================================================================

fn build_claude_messages_for_token_count(
    system: &Option<ClaudeSystemPrompt>,
    messages: &[ClaudeMessage],
) -> Vec<Value> {
    let mut combined = Vec::new();

    if let Some(system_prompt) = system {
        let system_content = match system_prompt {
            ClaudeSystemPrompt::Text(text) => Value::String(text.clone()),
            ClaudeSystemPrompt::Blocks(blocks) => {
                let items = blocks
                    .iter()
                    .map(|block| json!({"type": "text", "text": block.text}))
                    .collect::<Vec<_>>();
                Value::Array(items)
            }
        };
        combined.push(json!({"role": "system", "content": system_content}));
    }

    for message in messages {
        let content_value = match &message.content {
            ClaudeMessageContent::Text(text) => Value::String(text.clone()),
            ClaudeMessageContent::Blocks(blocks) => {
                let items = blocks
                    .iter()
                    .map(convert_claude_block_for_token_count)
                    .collect::<Vec<_>>();
                Value::Array(items)
            }
        };
        combined.push(json!({"role": message.role, "content": content_value}));
    }

    combined
}

fn convert_claude_block_for_token_count(block: &ClaudeContentBlock) -> Value {
    match block {
        ClaudeContentBlock::Text(text_block) => {
            json!({"type": "text", "text": text_block.text})
        }
        ClaudeContentBlock::Image(image_block) => {
            let data_uri = format!(
                "data:{};base64,{}",
                image_block.source.media_type, image_block.source.data
            );
            json!({
                "type": "image_url",
                "image_url": {
                    "url": data_uri,
                    "detail": "auto"
                }
            })
        }
        ClaudeContentBlock::ToolUse(tool_use) => json!({
            "type": "tool_use",
            "id": tool_use.id,
            "name": tool_use.name,
            "input": tool_use.input
        }),
        ClaudeContentBlock::ToolResult(tool_result) => json!({
            "type": "tool_result",
            "tool_use_id": tool_result.tool_use_id,
            "content": tool_result.content,
            "is_error": tool_result.is_error
        }),
        ClaudeContentBlock::Thinking(thinking) => json!({
            "type": "thinking",
            "thinking": thinking.thinking,
            "signature": thinking.signature
        }),
    }
}

fn build_openai_tools_for_token_count(tools: &Option<Vec<ClaudeTool>>) -> Option<Vec<Value>> {
    tools.as_ref().map(|tool_list| {
        tool_list
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.input_schema
                    }
                })
            })
            .collect::<Vec<_>>()
    })
}

/// Calculate input tokens for Claude request using tiktoken (for streaming fallback).
///
/// This function is used to provide a fallback token count when the upstream
/// provider doesn't return usage information in streaming responses.
fn calculate_claude_input_tokens(request: &ClaudeMessagesRequest) -> Option<usize> {
    let model = &request.model;
    let messages_value = build_claude_messages_for_token_count(&request.system, &request.messages);
    let tools_value = build_openai_tools_for_token_count(&request.tools);
    let tool_choice = request.tool_choice.as_ref();
    calculate_message_tokens_with_tools(
        &messages_value,
        model,
        tools_value.as_deref(),
        tool_choice,
    )
    .ok()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_provider_suffix_with_matching_prefix() {
        assert_eq!(
            strip_provider_suffix("Proxy/claude-3-opus", Some("Proxy")),
            "claude-3-opus"
        );
    }

    #[test]
    fn test_strip_provider_suffix_without_prefix() {
        assert_eq!(
            strip_provider_suffix("claude-3-opus", Some("Proxy")),
            "claude-3-opus"
        );
    }

    #[test]
    fn test_strip_provider_suffix_no_suffix_configured() {
        assert_eq!(
            strip_provider_suffix("Proxy/claude-3-opus", None),
            "Proxy/claude-3-opus"
        );
    }

    #[test]
    fn test_build_claude_error_response() {
        let response = build_claude_error_response(
            StatusCode::BAD_REQUEST,
            "invalid_request_error",
            "Test error",
            Some("test-model"),
            Some("test-provider"),
            Some("test-key"),
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response.extensions().get::<ModelName>().is_some());
        assert!(response.extensions().get::<ProviderName>().is_some());
        assert!(response.extensions().get::<ApiKeyName>().is_some());
    }
}
