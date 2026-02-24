//! Claude API compatible endpoints.
//!
//! This module provides Claude Messages API compatibility by converting
//! Claude format requests to OpenAI format, proxying to providers,
//! and converting responses back to Claude format.

use crate::api::auth::{verify_auth, AuthFormat};
use crate::api::claude_models::{
    ClaudeContentBlock, ClaudeMessage, ClaudeMessageContent, ClaudeMessagesRequest,
    ClaudeSystemPrompt, ClaudeTokenCountRequest, ClaudeTokenCountResponse, ClaudeTool,
};
use crate::api::gemini3::{normalize_request_payload, strip_gemini3_provider_fields};
use crate::api::handlers::AppState;
use crate::api::proxy::ensure_tool_use_result_pairing;
use crate::api::streaming::calculate_message_tokens_with_tools;
use crate::api::upstream::{
    attach_response_extensions, build_protocol_error_response, build_upstream_request,
    execute_upstream_request_or_transport_error, finalize_non_streaming_response,
    parse_upstream_json_or_error_with_log, record_token_metrics,
    split_upstream_status_error_with_log, StatusErrorResponseMode, UpstreamAuth, UpstreamContext,
};
use crate::core::error_types::{ERROR_TYPE_API, ERROR_TYPE_INVALID_REQUEST};
use crate::core::header_policy::sanitize_anthropic_beta_header;
use crate::core::jsonl_logger::{
    log_provider_request, log_provider_response, log_provider_streaming_response, log_request,
    log_streaming_response,
};
use crate::core::langfuse::{
    fail_generation_if_sampled, finish_generation_if_sampled, init_langfuse_trace,
    update_trace_provider_if_sampled, GenerationData,
};
use crate::core::logging::{generate_request_id, get_api_key_name, PROVIDER_CONTEXT, REQUEST_ID};
use crate::core::middleware::extract_client;
use crate::core::stream_metrics::{record_stream_metrics, StreamStats};
use crate::core::utils::{get_key_name, strip_provider_suffix};
use crate::core::{AppError, Result};
use crate::services::claude_converter::{
    claude_to_openai_request, convert_openai_streaming_to_claude, openai_to_claude_response,
};
use crate::transformer::Protocol;
use crate::with_request_context;
use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use chrono::Utc;
use futures::StreamExt;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

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
    let key_config = verify_auth(
        &headers,
        &state,
        AuthFormat::MultiFormat,
        Some("/v1/messages"),
    )?;
    let api_key_name = get_key_name(&key_config);

    with_request_context!(request_id.clone(), api_key_name.clone(), async move {
        // Extract client from User-Agent header for metrics
        let client = extract_client(&headers);

        let (trace_id, mut generation_data) =
            init_langfuse_trace(&request_id, &api_key_name, &headers, "/v1/messages");

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
                fail_generation_if_sampled(&trace_id, &mut generation_data, err.clone());
                return Ok(build_protocol_error_response(
                    Protocol::Anthropic,
                    StatusCode::BAD_REQUEST,
                    ERROR_TYPE_INVALID_REQUEST,
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
        update_trace_provider_if_sampled(
            &trace_id,
            &provider.name,
            &provider.api_base,
            &model_label,
        );

        // Convert Claude request to OpenAI format
        let mut openai_request = claude_to_openai_request(
            &claude_request,
            Some(&provider.model_mapping),
            state.config.min_tokens_limit,
            state.config.max_tokens_limit,
        );
        let gemini_model = openai_request
            .get("model")
            .and_then(|model| model.as_str())
            .map(|model| model.to_string());

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
        if !is_anthropic {
            normalize_request_payload(&mut openai_request, gemini_model.as_deref());
            strip_gemini3_provider_fields(&mut openai_request, gemini_model.as_deref());
        }
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
                let mut provider_request_value = claude_request_value.clone();
                ensure_tool_use_result_pairing(&mut provider_request_value);
                ensure_tool_use_result_pairing(&mut openai_request);
                log_request(
                    &request_id,
                    "/v1/messages",
                    &provider.name,
                    &claude_request_value,
                );

                // Get api_key_name from context for use in closures
                let api_key_name = get_api_key_name();

                // Log provider request to JSONL
                let provider_endpoint = if is_anthropic {
                    "/v1/messages"
                } else {
                    "/chat/completions"
                };
                log_provider_request(
                    &request_id,
                    &provider.name,
                    &provider.api_base,
                    provider_endpoint,
                    if is_anthropic {
                        &provider_request_value
                    } else {
                        &openai_request
                    },
                );

                let anthropic_version = headers
                    .get("anthropic-version")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("2023-06-01");
                let anthropic_beta_header = sanitize_anthropic_beta_header(
                    &provider.provider_type,
                    &provider.provider_params,
                    headers.get("anthropic-beta").and_then(|v| v.to_str().ok()),
                );

                let custom_headers: Option<std::collections::HashMap<String, String>> = provider
                    .provider_params
                    .get("custom_headers")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());

                // Build and send request based on provider_type
                let response = if is_anthropic {
                    let request = build_upstream_request(
                        &state.http_client,
                        &url,
                        &provider_request_value,
                        UpstreamAuth::XApiKey(&provider.api_key),
                        Some(anthropic_version),
                        anthropic_beta_header.as_deref(),
                        custom_headers.as_ref(),
                    );
                    let ctx = UpstreamContext {
                        protocol: Protocol::Anthropic,
                        model: Some(&model_label),
                        provider: &provider.name,
                        api_key_name: Some(&api_key_name),
                        request_id: Some(&request_id),
                    };
                    execute_upstream_request_or_transport_error(
                        request,
                        &provider_service,
                        &ctx,
                        Some(&url),
                        Some(&model_label),
                        "HTTP request failed to provider",
                    )
                    .await
                } else {
                    let request = build_upstream_request(
                        &state.http_client,
                        &url,
                        &openai_request,
                        UpstreamAuth::Bearer(&provider.api_key),
                        None,
                        None,
                        custom_headers.as_ref(),
                    );
                    let ctx = UpstreamContext {
                        protocol: Protocol::Anthropic,
                        model: Some(&model_label),
                        provider: &provider.name,
                        api_key_name: Some(&api_key_name),
                        request_id: Some(&request_id),
                    };
                    execute_upstream_request_or_transport_error(
                        request,
                        &provider_service,
                        &ctx,
                        Some(&url),
                        Some(&model_label),
                        "HTTP request failed to provider",
                    )
                    .await
                };

                let response = match response {
                    Ok(resp) => resp,
                    Err((error_message, error_response)) => {
                        fail_generation_if_sampled(&trace_id, &mut generation_data, error_message);

                        return Ok(error_response);
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
                let ctx = UpstreamContext {
                    protocol: Protocol::Anthropic,
                    model: Some(&model_label),
                    provider: &provider.name,
                    api_key_name: Some(&api_key_name),
                    request_id: Some(&request_id),
                };
                let response = match split_upstream_status_error_with_log(
                    response,
                    StatusErrorResponseMode::Protocol,
                    &ctx,
                    ERROR_TYPE_API,
                    "Backend API returned error status",
                    false,
                    false,
                )
                .await
                {
                    Ok(resp) => resp,
                    Err((parsed, error_response)) => {
                        fail_generation_if_sampled(&trace_id, &mut generation_data, parsed.message);

                        return Ok(error_response);
                    }
                };

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
                        client.clone(),
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
                        client.clone(),
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
    client: String,
    generation_data: GenerationData,
    trace_id: Option<String>,
    request_id: String,
    request_payload: serde_json::Value,
    fallback_input_tokens: Option<usize>,
) -> Result<Response> {
    let stream = response.bytes_stream();
    let start_time = std::time::Instant::now();

    // Convert OpenAI streaming to Claude streaming format
    let claude_stream =
        convert_openai_streaming_to_claude(Box::pin(stream), original_model, fallback_input_tokens);

    // Wrap the stream to capture output for Langfuse and JSONL logging
    let model_label_clone = model_label.clone();
    let provider_name_clone = provider_name.clone();
    let api_key_name_clone = api_key_name.clone();
    let client_clone = client.clone();
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
                client_clone.clone(),
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
                client,
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
                                gen_data.output_content = Some(accumulated_output.clone());
                                gen_data.finish_reason = finish_reason.clone();
                                gen_data.prompt_tokens = usage_input_tokens;
                                gen_data.completion_tokens = usage_output_tokens;
                                gen_data.total_tokens = usage_input_tokens + usage_output_tokens;
                                finish_generation_if_sampled(&trace_id, &mut gen_data);

                                // Record stream metrics using unified function
                                let stats = StreamStats {
                                    model: model_label.clone(),
                                    provider: provider_name.clone(),
                                    api_key_name: api_key_name.clone(),
                                    client: client.clone(),
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
                                    client,
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
    attach_response_extensions(
        &mut response,
        Some(&model_label_clone),
        Some(&provider_name_clone),
        Some(&api_key_name_clone),
    );

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
    client: String,
    mut generation_data: GenerationData,
    trace_id: Option<String>,
    request_id: String,
    _request_payload: serde_json::Value,
) -> Result<Response> {
    let (status, response_data): (reqwest::StatusCode, serde_json::Value) =
        match parse_upstream_json_or_error_with_log(
            response,
            &UpstreamContext {
                protocol: Protocol::Anthropic,
                model: Some(&model_label),
                provider: &provider_name,
                api_key_name: Some(&api_key_name),
                request_id: Some(&request_id),
            },
            "Failed to parse provider response JSON",
        )
        .await
        {
            Ok((status, data)) => {
                // Log provider response to JSONL (the raw OpenAI-format response from provider)
                log_provider_response(&request_id, &provider_name, status.as_u16(), None, &data);
                (status, data)
            }
            Err((error_message, error_response)) => {
                fail_generation_if_sampled(&trace_id, &mut generation_data, error_message);
                return Ok(error_response);
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
            return Ok(build_protocol_error_response(
                Protocol::Anthropic,
                StatusCode::BAD_GATEWAY,
                ERROR_TYPE_API,
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

            record_token_metrics(
                prompt_tokens,
                completion_tokens,
                &model_label,
                &provider_name,
                &api_key_name,
                &client,
            );
        }
    }

    let claude_response_value = serde_json::to_value(&claude_response).unwrap_or_default();
    let response = finalize_non_streaming_response(
        trace_id.as_deref(),
        &mut generation_data,
        &request_id,
        status.as_u16(),
        claude_response_value,
        &model_label,
        &provider_name,
        &api_key_name,
    );

    tracing::debug!(
        model = %model_label,
        provider = %provider_name,
        api_key_name = %api_key_name,
        request_id = %request_id,
        "Claude non-streaming response completed"
    );

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
    let _key_config = verify_auth(
        &headers,
        &state,
        AuthFormat::MultiFormat,
        Some("/v1/messages/count_tokens"),
    )?;

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
    calculate_message_tokens_with_tools(&messages_value, model, tools_value.as_deref(), tool_choice)
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
    fn test_build_protocol_error_response() {
        let response = build_protocol_error_response(
            Protocol::Anthropic,
            StatusCode::BAD_REQUEST,
            ERROR_TYPE_INVALID_REQUEST,
            "Test error",
            Some("test-model"),
            Some("test-provider"),
            Some("test-key"),
        );
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response
            .extensions()
            .get::<crate::core::middleware::ModelName>()
            .is_some());
        assert!(response
            .extensions()
            .get::<crate::core::middleware::ProviderName>()
            .is_some());
        assert!(response
            .extensions()
            .get::<crate::core::middleware::ApiKeyName>()
            .is_some());
    }
}
