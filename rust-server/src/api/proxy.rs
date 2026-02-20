//! Protocol-aware proxy handler using transformer pipeline.
//!
//! This module provides a unified proxy handler that uses the transformer
//! system for protocol conversion between different LLM API formats.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use serde_json::{json, Value};

use crate::api::auth::{verify_auth, AuthFormat};
use crate::api::claude_models::{ClaudeTokenCountRequest, ClaudeTokenCountResponse};
use crate::api::disconnect::DisconnectStream;
use crate::api::gemini3::{normalize_request_payload, strip_gemini3_provider_fields};
use crate::api::handlers::AppState;
use crate::api::models::{
    GcpVertexConfig, LiteLlmParams, ModelInfo, ModelInfoDetails, ModelInfoEntry, ModelInfoListV1,
    ModelInfoQueryParams, ModelInfoQueryParamsV1, ModelList, PaginatedModelInfoList,
};
use crate::api::rectifier::sanitize_provider_payload;
use crate::api::streaming::{
    calculate_message_tokens_with_tools, create_sse_stream, StreamRequestLogContext,
};
use crate::api::upstream::{
    attach_response_extensions, build_json_response, build_protocol_error_response,
    build_protocol_upstream_request, build_provider_debug_headers,
    build_unexpected_status_split_response, build_upstream_request,
    execute_upstream_request_or_transport_error, finalize_non_streaming_response,
    parse_upstream_json_or_error_with_log, split_upstream_status_error_with_log,
    StatusErrorResponseMode, UpstreamAuth, UpstreamContext, UpstreamErrorPayload,
};
use crate::core::error_logger::{log_error, mask_headers, ErrorCategory, ErrorLogRecord};
use crate::core::error_types::{ERROR_TYPE_API, ERROR_TYPE_INVALID_REQUEST, ERROR_TYPE_TIMEOUT};
use crate::core::header_policy::sanitize_anthropic_beta_header;
use crate::core::jsonl_logger::{log_provider_request, log_provider_response, log_request};
use crate::core::langfuse::{fail_generation_if_sampled, init_langfuse_trace, GenerationData};
use crate::core::logging::{generate_request_id, PROVIDER_CONTEXT, REQUEST_ID};
use crate::core::metrics::get_metrics;
use crate::core::middleware::{extract_client, HasCredentials};
use crate::core::request_logger::{log_request_record, RequestLogRecord};
use crate::core::stream_metrics::{record_stream_metrics, StreamStats};
use crate::core::utils::{get_key_name, strip_provider_suffix};
use crate::core::StreamCancelHandle;
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
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .unwrap_or_else(generate_request_id);
    let key_config = verify_auth(
        &headers,
        &state.app_state,
        AuthFormat::MultiFormat,
        Some(path),
    )?;
    let api_key_name = get_key_name(&key_config);

    // Detect client protocol
    let client_protocol = ProtocolDetector::detect_with_path_hint(&payload, path);

    with_request_context!(request_id.clone(), api_key_name.clone(), async move {
        // Extract client from User-Agent header for metrics
        let client = extract_client(&headers);
        let masked_headers_str = serde_json::to_string(&mask_headers(&headers)).ok();

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
        generation_data.is_streaming = extract_stream_flag(&payload);

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
                    ERROR_TYPE_INVALID_REQUEST,
                    &err,
                    Some(&effective_model),
                    None,
                    Some(&api_key_name),
                ));
            }
        };

        // Determine provider protocol from provider_type field
        let provider_protocol = provider_type_to_protocol(&provider.provider_type);

        // DEBUG: Log protocol information for debugging usage issues
        tracing::debug!(
            request_id = %request_id,
            client_protocol = %client_protocol,
            provider_protocol = %provider_protocol,
            provider_type = %provider.provider_type,
            is_same_protocol = (client_protocol == provider_protocol),
            "Protocol detection for streaming request"
        );

        // Build transform context
        let transform_ctx = TransformContext {
            request_id: request_id.clone(),
            client_protocol,
            provider_protocol,
            original_model: original_model.clone(),
            mapped_model: provider.get_mapped_model(&effective_model),
            provider_name: provider.name.clone(),
            provider_type: provider.provider_type.clone(),
            stream: generation_data.is_streaming,
            ..Default::default()
        };

        // Update generation data
        generation_data.provider_key = provider.name.clone();
        generation_data.provider_type = provider_protocol.to_string();
        generation_data.provider_api_base = provider.api_base.clone();
        generation_data.mapped_model = transform_ctx.mapped_model.clone();

        // Transform request with bypass optimization
        let (mut provider_payload, bypassed) = state
            .transform_pipeline
            .transform_request_with_bypass(payload.clone(), &transform_ctx)?;

        // Sanitize payload before sending to provider
        sanitize_provider_payload(&mut provider_payload);

        // Ensure every tool_use/tool_call has a matching tool_result
        ensure_tool_use_result_pairing(&mut provider_payload);

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

        normalize_gemini3_provider_payload(&mut provider_payload, provider_protocol);

        // Build URL based on provider protocol
        let url =
            if provider_protocol == Protocol::GcpVertex || provider_protocol == Protocol::Gemini {
                // GCP Vertex AI requires special URL construction
                let gcp_config = GcpVertexConfig::from_provider(&provider).unwrap_or_else(|| {
                    tracing::error!(
                        request_id = %request_id,
                        provider = %provider.name,
                        "gcp_project is required for gcp-vertex/gemini provider, using defaults"
                    );
                    GcpVertexConfig::from_provider_with_defaults(&provider)
                });

                // For Gemini protocol, override action verbs and append ?alt=sse
                let (blocking_act, streaming_act) = if provider_protocol == Protocol::Gemini {
                    (
                        "generateContent".to_string(),
                        "streamGenerateContent".to_string(),
                    )
                } else {
                    (
                        gcp_config.blocking_action.clone(),
                        gcp_config.streaming_action.clone(),
                    )
                };

                match crate::api::upstream::build_gcp_vertex_url_with_actions(
                    &provider.api_base,
                    &gcp_config.project,
                    &gcp_config.location,
                    &gcp_config.publisher,
                    &transform_ctx.mapped_model,
                    generation_data.is_streaming,
                    &blocking_act,
                    &streaming_act,
                ) {
                    Ok(mut url) => {
                        if provider_protocol == Protocol::Gemini && generation_data.is_streaming {
                            url.push_str("?alt=sse");
                        }
                        url
                    }
                    Err(err) => {
                        tracing::error!(
                            request_id = %request_id,
                            provider = %provider.name,
                            error = %err,
                            "GCP Vertex URL validation failed"
                        );
                        return Ok(build_protocol_error_response(
                            client_protocol,
                            StatusCode::BAD_REQUEST,
                            ERROR_TYPE_INVALID_REQUEST,
                            &err,
                            Some(&effective_model),
                            Some(&provider.name),
                            Some(&api_key_name),
                        ));
                    }
                }
            } else {
                format!(
                    "{}{}",
                    provider.api_base,
                    get_provider_endpoint(provider_protocol)
                )
            };

        tracing::debug!(
            request_id = %request_id,
            provider = %provider.name,
            url = %url,
            provider_protocol = %provider_protocol,
            "Built provider URL"
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

                let anthropic_beta_header = sanitize_anthropic_beta_header(
                    &provider.provider_type,
                    &provider.provider_params,
                    headers.get("anthropic-beta").and_then(|v| v.to_str().ok()),
                );

                let request = build_protocol_upstream_request(
                    &state.app_state.http_client,
                    &url,
                    provider_protocol,
                    &provider.api_key,
                    &headers,
                    anthropic_beta_header.as_deref(),
                    &provider_payload,
                );

                let upstream_ctx = UpstreamContext {
                    protocol: client_protocol,
                    model: Some(&effective_model),
                    provider: &provider.name,
                    api_key_name: Some(&api_key_name),
                    request_id: Some(&request_id),
                };

                let response = match execute_upstream_request_or_transport_error(
                    request,
                    &provider_service,
                    &upstream_ctx,
                    None,
                    Some(&effective_model),
                    "HTTP request failed",
                )
                .await
                {
                    Ok(resp) => resp,
                    Err((error_message, error_response)) => {
                        log_request_record(RequestLogRecord {
                            request_id: request_id.clone(),
                            endpoint: Some(path.to_string()),
                            credential_name: Some(api_key_name.clone()),
                            model_requested: Some(effective_model.clone()),
                            model_mapped: Some(transform_ctx.mapped_model.clone()),
                            provider_name: Some(provider.name.clone()),
                            provider_type: Some(provider.provider_type.clone()),
                            client_protocol: Some(client_protocol.to_string()),
                            provider_protocol: Some(provider_protocol.to_string()),
                            is_streaming: generation_data.is_streaming,
                            total_duration_ms: Some(
                                request_start.elapsed().as_millis().min(i32::MAX as u128) as i32,
                            ),
                            error_category: Some("transport".to_string()),
                            error_message: Some(error_message),
                            request_headers: masked_headers_str.clone(),
                            ..Default::default()
                        });
                        return Ok(error_response);
                    }
                };

                let status = response.status();

                // Handle error responses
                if status.is_client_error() || status.is_server_error() {
                    // Consume the response body first via handle_error_response
                    let (error_response, upstream_payload) = handle_error_response(
                        response,
                        client_protocol,
                        &effective_model,
                        &provider.name,
                        &api_key_name,
                        &request_id,
                    )
                    .await?;

                    let response_body = upstream_payload.as_ref().map(|p| p.body.clone());

                    // Log provider 4xx errors (excluding 429) as they indicate
                    // potential issues with our request transformation
                    if status.is_client_error() && status.as_u16() != 429 {
                        let provider_headers = build_provider_debug_headers(
                            provider_protocol,
                            &url,
                            &headers,
                            anthropic_beta_header.as_deref(),
                        );

                        log_error(ErrorLogRecord {
                            request_id: request_id.clone(),
                            error_category: ErrorCategory::Provider4xx,
                            error_message: format!("HTTP {} from {}", status, provider.name),
                            error_code: Some(status.as_u16() as i32),
                            endpoint: path.to_string(),
                            client_protocol: client_protocol.to_string(),
                            request_headers: Some(mask_headers(&headers)),
                            request_body: Some(payload.clone()),
                            provider_name: provider.name.clone(),
                            provider_api_base: provider.api_base.clone(),
                            provider_protocol: provider_protocol.to_string(),
                            mapped_model: transform_ctx.mapped_model.clone(),
                            response_status_code: Some(status.as_u16() as i32),
                            response_body: response_body.clone(),
                            credential_name: api_key_name.clone(),
                            client: client.clone(),
                            is_streaming: generation_data.is_streaming,
                            total_duration_ms: Some(
                                request_start.elapsed().as_millis().min(i32::MAX as u128) as i32,
                            ),
                            provider_request_body: Some(provider_payload.clone()),
                            provider_request_headers: Some(provider_headers),
                        });
                    }

                    // Log provider 5xx errors
                    if status.is_server_error() {
                        let provider_headers = build_provider_debug_headers(
                            provider_protocol,
                            &url,
                            &headers,
                            anthropic_beta_header.as_deref(),
                        );

                        log_error(ErrorLogRecord {
                            request_id: request_id.clone(),
                            error_category: ErrorCategory::Provider5xx,
                            error_message: format!("HTTP {} from {}", status, provider.name),
                            error_code: Some(status.as_u16() as i32),
                            endpoint: path.to_string(),
                            client_protocol: client_protocol.to_string(),
                            request_headers: Some(mask_headers(&headers)),
                            request_body: Some(payload.clone()),
                            provider_name: provider.name.clone(),
                            provider_api_base: provider.api_base.clone(),
                            provider_protocol: provider_protocol.to_string(),
                            mapped_model: transform_ctx.mapped_model.clone(),
                            response_status_code: Some(status.as_u16() as i32),
                            response_body,
                            credential_name: api_key_name.clone(),
                            client: client.clone(),
                            is_streaming: generation_data.is_streaming,
                            total_duration_ms: Some(
                                request_start.elapsed().as_millis().min(i32::MAX as u128) as i32,
                            ),
                            provider_request_body: Some(provider_payload.clone()),
                            provider_request_headers: Some(provider_headers),
                        });
                    }

                    // Log request record for error responses
                    log_request_record(RequestLogRecord {
                        request_id: request_id.clone(),
                        endpoint: Some(path.to_string()),
                        credential_name: Some(api_key_name.clone()),
                        model_requested: Some(effective_model.clone()),
                        model_mapped: Some(transform_ctx.mapped_model.clone()),
                        provider_name: Some(provider.name.clone()),
                        provider_type: Some(provider.provider_type.clone()),
                        client_protocol: Some(client_protocol.to_string()),
                        provider_protocol: Some(provider_protocol.to_string()),
                        is_streaming: generation_data.is_streaming,
                        status_code: Some(status.as_u16() as i32),
                        total_duration_ms: Some(
                            request_start.elapsed().as_millis().min(i32::MAX as u128) as i32,
                        ),
                        error_category: Some(
                            if status.is_server_error() {
                                "provider_5xx"
                            } else {
                                "provider_4xx"
                            }
                            .to_string(),
                        ),
                        error_message: Some(format!("HTTP {} from {}", status, provider.name)),
                        request_headers: masked_headers_str.clone(),
                        ..Default::default()
                    });

                    return Ok(error_response);
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
                                    Value::String(text) => {
                                        json!({"role": "system", "content": text})
                                    }
                                    Value::Array(blocks) => {
                                        json!({"role": "system", "content": blocks})
                                    }
                                    _ => json!({"role": "system", "content": ""}),
                                };
                                combined_messages.push(system_message);
                            }

                            combined_messages.extend(arr.iter().cloned());

                            let total_tokens =
                                crate::api::streaming::calculate_message_tokens_with_tools(
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
                        client.clone(),
                        masked_headers_str.clone(),
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
                        masked_headers_str.clone(),
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

/// Ensure every tool_use / tool_call has a matching tool_result response.
///
/// Providers like AWS Bedrock Converse strictly validate that every `tool_use`
/// block is immediately followed by a corresponding `tool_result`. When a
/// client sends an incomplete conversation (e.g. tool call was cancelled), the
/// missing result causes a 400 error.
///
/// This function handles both payload formats:
///   - **OpenAI format**: assistant messages carry `tool_calls`; results appear
///     as subsequent `{"role":"tool","tool_call_id":"..."}` messages.
///   - **Anthropic format**: `tool_use` and `tool_result` are content blocks
///     inside the `content` array.
///
/// For any unpaired tool_use, a placeholder tool_result is injected so that
/// downstream providers never see an orphan.
pub(crate) fn ensure_tool_use_result_pairing(payload: &mut Value) {
    let messages = match payload.get_mut("messages").and_then(|m| m.as_array_mut()) {
        Some(m) => m,
        None => return,
    };

    // Collect all tool_result / tool role IDs present in the conversation.
    let mut result_ids: HashSet<String> = HashSet::new();
    for msg in messages.iter() {
        // OpenAI format: role:"tool" with tool_call_id
        if msg.get("role").and_then(|r| r.as_str()) == Some("tool") {
            if let Some(id) = msg.get("tool_call_id").and_then(|v| v.as_str()) {
                result_ids.insert(id.to_string());
            }
        }
        // Anthropic format: content blocks with type:"tool_result"
        if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    if let Some(id) = block.get("tool_use_id").and_then(|v| v.as_str()) {
                        result_ids.insert(id.to_string());
                    }
                }
            }
        }
    }

    // Walk messages, collect orphaned tool_use IDs per assistant message index.
    // We insert placeholder results right after the assistant message that
    // introduced the tool call, so we iterate in reverse to keep indices stable.
    let mut inserts: Vec<(usize, Vec<String>)> = Vec::new();
    for (i, msg) in messages.iter().enumerate() {
        let is_assistant = msg.get("role").and_then(|r| r.as_str()) == Some("assistant");
        if !is_assistant {
            continue;
        }
        let mut orphans: Vec<String> = Vec::new();

        // OpenAI format: tool_calls array on assistant message
        if let Some(calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
            for call in calls {
                if let Some(id) = call.get("id").and_then(|v| v.as_str()) {
                    if !result_ids.contains(id) {
                        orphans.push(id.to_string());
                    }
                }
            }
        }

        // Anthropic format: tool_use content blocks
        if let Some(content) = msg.get("content").and_then(|c| c.as_array()) {
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                    if let Some(id) = block.get("id").and_then(|v| v.as_str()) {
                        if !result_ids.contains(id) {
                            orphans.push(id.to_string());
                        }
                    }
                }
            }
        }

        if !orphans.is_empty() {
            inserts.push((i, orphans));
        }
    }

    // Insert placeholders in reverse order to preserve indices.
    for (assistant_idx, orphan_ids) in inserts.into_iter().rev() {
        let insert_pos = assistant_idx + 1;
        // Detect format: if the assistant message has `tool_calls` field, use
        // OpenAI format; otherwise use Anthropic content block format.
        let use_openai_format = messages[assistant_idx].get("tool_calls").is_some();

        if use_openai_format {
            // OpenAI: one role:"tool" message per orphan
            for id in orphan_ids.into_iter().rev() {
                let placeholder = json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": "[Tool call interrupted - no result available]"
                });
                messages.insert(insert_pos, placeholder);
            }
        } else {
            // Anthropic: one user message with tool_result blocks
            let blocks: Vec<Value> = orphan_ids
                .into_iter()
                .map(|id| {
                    json!({
                        "type": "tool_result",
                        "tool_use_id": id,
                        "content": "[Tool call interrupted - no result available]",
                        "is_error": true
                    })
                })
                .collect();
            let placeholder = json!({
                "role": "user",
                "content": blocks
            });
            messages.insert(insert_pos, placeholder);
        }
    }
}

/// Extract model from request based on protocol
fn extract_model_from_request(payload: &Value, protocol: Protocol) -> String {
    match protocol {
        Protocol::OpenAI | Protocol::Anthropic | Protocol::GcpVertex | Protocol::Gemini => payload
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

/// Extract stream flag from request payload
fn extract_stream_flag(payload: &Value) -> bool {
    payload
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false)
}

fn normalize_gemini3_provider_payload(provider_payload: &mut Value, protocol: Protocol) {
    if protocol != Protocol::OpenAI {
        return;
    }
    let gemini_model = provider_payload
        .get("model")
        .and_then(|model| model.as_str())
        .map(|model| model.to_string());
    normalize_request_payload(provider_payload, gemini_model.as_deref());
    strip_gemini3_provider_fields(provider_payload, gemini_model.as_deref());
}

/// Get the endpoint path for a provider protocol
fn get_provider_endpoint(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::OpenAI => "/chat/completions",
        Protocol::Anthropic => "/v1/messages",
        Protocol::ResponseApi => "/responses",
        Protocol::GcpVertex | Protocol::Gemini => "", // GCP Vertex/Gemini uses dynamic endpoints constructed elsewhere
    }
}

/// Handle error response from provider.
/// Returns the HTTP response to send to the client and the parsed upstream error payload (if available).
async fn handle_error_response(
    response: reqwest::Response,
    client_protocol: Protocol,
    model: &str,
    provider_name: &str,
    api_key_name: &str,
    request_id: &str,
) -> Result<(Response, Option<UpstreamErrorPayload>)> {
    let ctx = UpstreamContext {
        protocol: client_protocol,
        model: Some(model),
        provider: provider_name,
        api_key_name: Some(api_key_name),
        request_id: Some(request_id),
    };
    match split_upstream_status_error_with_log(
        response,
        StatusErrorResponseMode::Protocol,
        &ctx,
        ERROR_TYPE_API,
        "Backend API returned error",
        true,
        true,
    )
    .await
    {
        Ok(_success_response) => Ok((
            build_unexpected_status_split_response(&ctx, "error response handler"),
            None,
        )),
        Err((payload, error_response)) => Ok((error_response, Some(payload))),
    }
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
    client: String,
    masked_headers: Option<String>,
) -> Result<Response> {
    let client_protocol = ctx.client_protocol;
    let provider_protocol = ctx.provider_protocol;
    let model_label = ctx.original_model.clone();
    let provider_name = ctx.provider_name.clone();
    let provider_type_str = ctx.provider_type.clone();
    let request_id = ctx.request_id.clone();

    // For same-protocol streaming, we can use bypass optimization
    if client_protocol == provider_protocol {
        // Direct passthrough with model rewriting
        let langfuse_data = if trace_id.is_some() {
            Some(generation_data)
        } else {
            None
        };

        // Create cancellation handle for streaming requests
        let cancel_handle = Some(StreamCancelHandle::new());
        let cancel_handle_clone = cancel_handle.clone();

        match create_sse_stream(
            response,
            model_label.clone(),
            provider_name.clone(),
            Some(ctx.mapped_model.clone()),
            input_tokens,
            state.app_state.config.ttft_timeout_secs,
            langfuse_data,
            Some(request_id.clone()),
            Some(endpoint.to_string()),
            Some(request_payload.clone()),
            Some(client.clone()),
            cancel_handle_clone,
            Some(StreamRequestLogContext {
                mapped_model: ctx.mapped_model.clone(),
                provider_type: provider_type_str.clone(),
                client_protocol: client_protocol.to_string(),
                provider_protocol: provider_protocol.to_string(),
                request_headers: masked_headers.clone(),
            }),
        )
        .await
        {
            Ok(sse_stream) => {
                let mut resp = sse_stream.into_response();
                attach_response_extensions(
                    &mut resp,
                    Some(&model_label),
                    Some(&provider_name),
                    Some(api_key_name),
                );

                // Wrap body with DisconnectStream to detect client disconnects
                if let Some(handle) = cancel_handle {
                    let (parts, body) = resp.into_parts();
                    let new_body = Body::from_stream(DisconnectStream {
                        stream: body.into_data_stream(),
                        cancel_handle: handle,
                    });
                    resp = Response::from_parts(parts, new_body);
                }

                Ok(resp)
            }
            Err(e) => {
                tracing::error!(provider = %provider_name, error = %e, "Streaming error");
                Ok(build_protocol_error_response(
                    client_protocol,
                    StatusCode::GATEWAY_TIMEOUT,
                    ERROR_TYPE_TIMEOUT,
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
        use tokio::select;
        use tokio::sync::watch;

        // DEBUG: Log that we're entering cross-protocol streaming path
        tracing::debug!(
            request_id = %request_id,
            client_protocol = %client_protocol,
            provider_protocol = %provider_protocol,
            "Entering cross-protocol streaming path"
        );

        // Create cancellation handle for cross-protocol streaming
        let cancel_handle = StreamCancelHandle::new();
        let cancel_rx = cancel_handle.subscribe();
        // Clone the handle so we can mark it completed from inside the unfold closure
        let cancel_handle_for_completion = cancel_handle.clone();

        struct CrossProtocolStreamingState {
            stream: Pin<
                Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
            >,
            stream_state: CrossProtocolStreamState,
            registry: Arc<TransformerRegistry>,
            client_protocol: Protocol,
            provider_protocol: Protocol,
            // Metrics tracking
            model: String,
            provider: String,
            api_key_name: String,
            client: String,
            start_time: Instant,
            first_token_time: Option<Instant>,
            // Flag to track if we've sent the final output
            finalized: bool,
            // SSE line buffer for handling TCP fragmentation
            sse_line_buffer: String,
            // Cancellation receiver for client disconnect detection
            cancel_rx: watch::Receiver<bool>,
            // Handle to mark completion when stream ends normally
            cancel_handle: StreamCancelHandle,
            // Request logging context
            request_id: String,
            endpoint: String,
            credential_name: String,
            model_requested: String,
            mapped_model: String,
            provider_type: String,
            request_headers: Option<String>,
        }

        let masked = masked_headers;
        let streaming_state = CrossProtocolStreamingState {
            stream: Box::pin(response.bytes_stream()),
            stream_state: CrossProtocolStreamState::with_input_tokens(&model_label, input_tokens),
            registry: state.transformer_registry.clone(),
            client_protocol,
            provider_protocol,
            model: model_label.clone(),
            provider: provider_name.clone(),
            api_key_name: api_key_name.to_string(),
            client: client.clone(),
            start_time: request_start,
            first_token_time: None,
            finalized: false,
            sse_line_buffer: String::new(),
            cancel_rx,
            cancel_handle: cancel_handle_for_completion,
            request_id: request_id.clone(),
            endpoint: endpoint.to_string(),
            credential_name: api_key_name.to_string(),
            model_requested: model_label.clone(),
            mapped_model: ctx.mapped_model.clone(),
            provider_type: provider_type_str.clone(),
            request_headers: masked,
        };

        let transform_stream = futures::stream::unfold(streaming_state, |mut state| async move {
            // If we've already finalized, end the stream
            if state.finalized {
                return None;
            }

            // Use select! to race between stream data and cancellation
            let chunk_result = {
                let stream_future = state.stream.next();
                let cancel_future = async {
                    let mut rx = state.cancel_rx.clone();
                    let _ = rx.changed().await;
                    true
                };

                select! {
                    chunk = stream_future => chunk,
                    _ = cancel_future => {
                        tracing::info!(
                            provider = %state.provider,
                            model = %state.model,
                            "Client disconnected during cross-protocol streaming, cancelling stream"
                        );
                        get_metrics().client_disconnects_total.inc();

                        // Record partial metrics
                        let final_usage = state.stream_state.get_final_usage(None);
                        let stats = StreamStats {
                            model: state.model.clone(),
                            provider: state.provider.clone(),
                            api_key_name: state.api_key_name.clone(),
                            client: state.client.clone(),
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

                        // Log request record for client disconnect
                        let ttft = state.first_token_time.map(|ft| ft.duration_since(state.start_time).as_millis().min(i32::MAX as u128) as i32);
                        log_request_record(RequestLogRecord {
                            request_id: state.request_id.clone(),
                            endpoint: Some(state.endpoint.clone()),
                            credential_name: Some(state.credential_name.clone()),
                            model_requested: Some(state.model_requested.clone()),
                            model_mapped: Some(state.mapped_model.clone()),
                            provider_name: Some(state.provider.clone()),
                            provider_type: Some(state.provider_type.clone()),
                            client_protocol: Some(state.client_protocol.to_string()),
                            provider_protocol: Some(state.provider_protocol.to_string()),
                            is_streaming: true,
                            status_code: Some(499),
                            input_tokens: stats.input_tokens as i32,
                            output_tokens: stats.output_tokens as i32,
                            total_tokens: (stats.input_tokens + stats.output_tokens) as i32,
                            total_duration_ms: Some(state.start_time.elapsed().as_millis().min(i32::MAX as u128) as i32),
                            ttft_ms: ttft,
                            error_category: Some("client_disconnect".to_string()),
                            request_headers: state.request_headers.clone(),
                            ..Default::default()
                        });

                        return None;
                    }
                }
            };

            match chunk_result {
                Some(Ok(bytes)) => {
                    // Track first token time
                    if state.first_token_time.is_none() {
                        state.first_token_time = Some(Instant::now());
                    }

                    // Append incoming bytes to SSE line buffer
                    let chunk_str = String::from_utf8_lossy(&bytes);
                    state.sse_line_buffer.push_str(&chunk_str);

                    // Extract complete SSE events from buffer (events end with \n\n)
                    let mut complete_events = Vec::new();
                    while let Some(pos) = state.sse_line_buffer.find("\n\n") {
                        let event = state.sse_line_buffer[..pos].to_string();
                        state.sse_line_buffer = state.sse_line_buffer[pos + 2..].to_string();
                        if !event.trim().is_empty() {
                            complete_events.push(event);
                        }
                    }

                    // If no complete events yet, return empty and wait for more data
                    if complete_events.is_empty() {
                        return Some((Ok::<_, std::io::Error>(axum::body::Bytes::new()), state));
                    }

                    // Get transformers
                    let provider_transformer = state.registry.get(state.provider_protocol);
                    let client_transformer = state.registry.get(state.client_protocol);

                    let output = if let (Some(provider_t), Some(client_t)) =
                        (provider_transformer, client_transformer)
                    {
                        let mut output = String::new();

                        // Process each complete SSE event
                        for event in complete_events {
                            // Reconstruct bytes with \n\n for the transformer
                            let event_bytes = bytes::Bytes::from(format!("{}\n\n", event));

                            // Transform chunks from provider format to unified format
                            match provider_t.transform_stream_chunk_in(&event_bytes) {
                                Ok(unified_chunks) => {
                                    // Process chunks through state tracker
                                    let processed_chunks =
                                        state.stream_state.process_chunks(unified_chunks);

                                    // Transform unified chunks to client format
                                    for chunk in processed_chunks {
                                        if let Ok(formatted) = client_t.transform_stream_chunk_out(
                                            &chunk,
                                            state.client_protocol,
                                        ) {
                                            output.push_str(&formatted);
                                        }
                                    }
                                }
                                Err(_) => {
                                    // Passthrough on error
                                    output.push_str(&event);
                                    output.push_str("\n\n");
                                }
                            }
                        }
                        axum::body::Bytes::from(output)
                    } else {
                        // No transformers, reconstruct the events
                        let output: String = complete_events
                            .into_iter()
                            .map(|e| format!("{}\n\n", e))
                            .collect();
                        axum::body::Bytes::from(output)
                    };

                    Some((Ok::<_, std::io::Error>(output), state))
                }
                Some(Err(e)) => Some((Err(std::io::Error::other(e.to_string())), state)),
                None => {
                    // Process any remaining data in the SSE line buffer
                    let provider_transformer = state.registry.get(state.provider_protocol);
                    let client_transformer = state.registry.get(state.client_protocol);

                    let mut output = String::new();

                    if !state.sse_line_buffer.trim().is_empty() {
                        let remaining = std::mem::take(&mut state.sse_line_buffer);
                        if let (Some(provider_t), Some(client_t)) =
                            (provider_transformer.as_ref(), client_transformer.as_ref())
                        {
                            // Process remaining buffer as an event
                            let event_bytes = bytes::Bytes::from(format!("{}\n\n", remaining));
                            if let Ok(unified_chunks) =
                                provider_t.transform_stream_chunk_in(&event_bytes)
                            {
                                let processed_chunks =
                                    state.stream_state.process_chunks(unified_chunks);
                                for chunk in processed_chunks {
                                    if let Ok(formatted) = client_t
                                        .transform_stream_chunk_out(&chunk, state.client_protocol)
                                    {
                                        output.push_str(&formatted);
                                    }
                                }
                            }
                        }
                    }

                    // Stream ended - finalize and emit closing events

                    // DEBUG: Log state before finalize
                    tracing::debug!(
                        model = %state.model,
                        provider = %state.provider,
                        accumulated_usage = ?state.stream_state.usage(),
                        message_delta_emitted = state.stream_state.message_delta_emitted,
                        "Stream ended, calling finalize()"
                    );

                    // Generate final events (message_delta with usage, message_stop)
                    let final_chunks = state.stream_state.finalize();

                    // DEBUG: Log final chunks
                    tracing::debug!(
                        final_chunks_count = final_chunks.len(),
                        final_chunks = ?final_chunks.iter().map(|c| format!("{:?} usage={:?}", c.chunk_type, c.usage)).collect::<Vec<_>>(),
                        "finalize() returned chunks"
                    );

                    if let Some(client_t) = client_transformer {
                        for chunk in &final_chunks {
                            if let Ok(formatted) =
                                client_t.transform_stream_chunk_out(chunk, state.client_protocol)
                            {
                                output.push_str(&formatted);
                            }
                        }
                    }

                    // DEBUG: Log final output
                    tracing::debug!(
                        output_len = output.len(),
                        output_preview = %if output.len() > 500 { &output[..500] } else { &output },
                        "Final output before [DONE]"
                    );

                    // Add [DONE] marker only if finalize didn't emit MessageStop
                    // (finalize emits MessageStop which OpenAI transformer converts to [DONE])
                    if !state.stream_state.message_stopped {
                        output.push_str("data: [DONE]\n\n");
                    }

                    // Mark the cancel handle as completed since stream finished normally
                    // This prevents false positive disconnect metrics when DisconnectStream is dropped
                    state.cancel_handle.mark_completed();

                    // Record metrics
                    let final_usage = state.stream_state.get_final_usage(None);
                    let stats = StreamStats {
                        model: state.model.clone(),
                        provider: state.provider.clone(),
                        api_key_name: state.api_key_name.clone(),
                        client: state.client.clone(),
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

                    // Log request record for streaming success
                    let ttft = state.first_token_time.map(|ft| {
                        ft.duration_since(state.start_time)
                            .as_millis()
                            .min(i32::MAX as u128) as i32
                    });
                    log_request_record(RequestLogRecord {
                        request_id: state.request_id.clone(),
                        endpoint: Some(state.endpoint.clone()),
                        credential_name: Some(state.credential_name.clone()),
                        model_requested: Some(state.model_requested.clone()),
                        model_mapped: Some(state.mapped_model.clone()),
                        provider_name: Some(state.provider.clone()),
                        provider_type: Some(state.provider_type.clone()),
                        client_protocol: Some(state.client_protocol.to_string()),
                        provider_protocol: Some(state.provider_protocol.to_string()),
                        is_streaming: true,
                        status_code: Some(200),
                        input_tokens: stats.input_tokens as i32,
                        output_tokens: stats.output_tokens as i32,
                        total_tokens: (stats.input_tokens + stats.output_tokens) as i32,
                        total_duration_ms: Some(
                            state.start_time.elapsed().as_millis().min(i32::MAX as u128) as i32,
                        ),
                        ttft_ms: ttft,
                        request_headers: state.request_headers.clone(),
                        ..Default::default()
                    });

                    // Mark as finalized so next iteration returns None
                    state.finalized = true;

                    // Return final output with closing events
                    Some((Ok(axum::body::Bytes::from(output)), state))
                }
            }
        });

        let body = Body::from_stream(transform_stream);

        // Wrap with DisconnectStream to detect client disconnects
        let body = Body::from_stream(DisconnectStream {
            stream: body.into_data_stream(),
            cancel_handle,
        });

        let mut resp = Response::builder()
            .status(200)
            .header("Content-Type", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .body(body)
            .unwrap();

        attach_response_extensions(
            &mut resp,
            Some(&model_label),
            Some(&provider_name),
            Some(api_key_name),
        );

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
    request_start: Instant,
    endpoint: &str,
    masked_headers: Option<String>,
) -> Result<Response> {
    let client_protocol = ctx.client_protocol;
    let model_label = ctx.original_model.clone();
    let provider_name = ctx.provider_name.clone();
    let request_id = ctx.request_id.clone();

    // Parse response JSON
    let (status, response_data): (reqwest::StatusCode, Value) =
        match parse_upstream_json_or_error_with_log(
            response,
            &UpstreamContext {
                protocol: client_protocol,
                model: Some(&model_label),
                provider: &provider_name,
                api_key_name: Some(api_key_name),
                request_id: Some(&request_id),
            },
            "Failed to parse response",
        )
        .await
        {
            Ok((status, data)) => (status, data),
            Err((error_message, error_response)) => {
                fail_generation_if_sampled(&trace_id, &mut generation_data, error_message);
                return Ok(error_response);
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

    // Log request record for non-streaming success
    let usage = client_response.get("usage");
    let input_tokens = usage
        .and_then(|u| u.get("prompt_tokens").or_else(|| u.get("input_tokens")))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let output_tokens = usage
        .and_then(|u| {
            u.get("completion_tokens")
                .or_else(|| u.get("output_tokens"))
        })
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    log_request_record(RequestLogRecord {
        request_id: request_id.clone(),
        endpoint: Some(endpoint.to_string()),
        credential_name: Some(api_key_name.to_string()),
        model_requested: Some(ctx.original_model.clone()),
        model_mapped: Some(ctx.mapped_model.clone()),
        provider_name: Some(ctx.provider_name.clone()),
        provider_type: Some(ctx.provider_type.clone()),
        client_protocol: Some(ctx.client_protocol.to_string()),
        provider_protocol: Some(ctx.provider_protocol.to_string()),
        is_streaming: false,
        status_code: Some(status.as_u16() as i32),
        input_tokens,
        output_tokens,
        total_tokens: input_tokens + output_tokens,
        total_duration_ms: Some(request_start.elapsed().as_millis().min(i32::MAX as u128) as i32),
        request_headers: masked_headers,
        ..Default::default()
    });

    Ok(finalize_non_streaming_response(
        trace_id.as_deref(),
        &mut generation_data,
        &request_id,
        status.as_u16(),
        client_response,
        &model_label,
        &provider_name,
        api_key_name,
    ))
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

/// List available models (V2).
///
/// Returns a list of all available models that can be used with the API.
/// This endpoint is compatible with the OpenAI Models API.
pub async fn list_models_v2(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
) -> Result<Json<ModelList>> {
    let request_id = generate_request_id();

    REQUEST_ID
        .scope(request_id.clone(), async move {
            let key_config = verify_auth(
                &headers,
                &state.app_state,
                AuthFormat::MultiFormat,
                Some("/v2/models"),
            )?;

            tracing::debug!(
                request_id = %request_id,
                "Listing available models (V2)"
            );

            // Get fresh provider service from DynamicConfig
            let provider_service = state.app_state.get_provider_service();
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

/// List model deployments in LiteLLM v1 format (no pagination).
///
/// Returns all model entries without pagination.
/// Supports filtering by model name and litellm_model_id only.
pub async fn list_model_info_v1(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    Query(query): Query<ModelInfoQueryParamsV1>,
) -> Result<Json<ModelInfoListV1>> {
    let request_id = generate_request_id();

    REQUEST_ID
        .scope(request_id.clone(), async move {
            let key_config = verify_auth(
                &headers,
                &state.app_state,
                AuthFormat::MultiFormat,
                Some("/v1/model/info"),
            )?;

            tracing::debug!(
                request_id = %request_id,
                "Listing model info (V1)"
            );

            let allowed_models = key_config
                .as_ref()
                .map(|config| config.allowed_models.clone())
                .unwrap_or_default();

            let provider_service = state.app_state.get_provider_service();
            let providers = provider_service.get_all_providers();

            let mut data = Vec::new();
            for provider in providers {
                let mut model_keys: Vec<String> = provider.model_mapping.keys().cloned().collect();
                model_keys.sort();
                for model_name in model_keys {
                    if !crate::api::models::model_allowed_for_info(&model_name, &allowed_models) {
                        continue;
                    }

                    let value = provider.model_mapping.get(&model_name);
                    let mapped_model = value
                        .map(|v| v.mapped_model().to_string())
                        .unwrap_or_default();

                    let mut model_info = ModelInfoDetails::new(
                        provider.name.clone(),
                        provider.provider_type.clone(),
                        provider.weight,
                        crate::api::models::is_pattern(&model_name),
                    );

                    if let Some(metadata) = value.and_then(|v| v.metadata()) {
                        model_info = model_info.with_metadata(metadata);
                    }

                    data.push(ModelInfoEntry {
                        model_name: model_name.clone(),
                        litellm_params: LiteLlmParams {
                            model: mapped_model,
                            api_base: provider.api_base.clone(),
                            custom_llm_provider: provider.provider_type.clone(),
                        },
                        model_info,
                    });
                }
            }

            // Apply v1 filters (only model and litellm_model_id)

            // 1. Exact match by model name
            if let Some(ref model) = query.model {
                data.retain(|entry| entry.model_name == *model);
            }

            // 2. Filter by litellm_model_id (mapped model) - v1 naming convention
            if let Some(ref model_id) = query.litellm_model_id {
                data.retain(|entry| entry.litellm_params.model == *model_id);
            }

            Ok(Json(ModelInfoListV1 { data }))
        })
        .await
}

/// List model deployments in LiteLLM-compatible format (V2).
pub async fn list_model_info_v2(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    Query(query): Query<ModelInfoQueryParams>,
) -> Result<Json<PaginatedModelInfoList>> {
    let request_id = generate_request_id();

    REQUEST_ID
        .scope(request_id.clone(), async move {
            let key_config = verify_auth(
                &headers,
                &state.app_state,
                AuthFormat::MultiFormat,
                Some("/v2/model/info"),
            )?;

            tracing::debug!(
                request_id = %request_id,
                "Listing model info (V2)"
            );

            let allowed_models = key_config
                .as_ref()
                .map(|config| config.allowed_models.clone())
                .unwrap_or_default();

            let provider_service = state.app_state.get_provider_service();
            let providers = provider_service.get_all_providers();

            let mut data = Vec::new();
            for provider in providers {
                let mut model_keys: Vec<String> = provider.model_mapping.keys().cloned().collect();
                model_keys.sort();
                for model_name in model_keys {
                    if !crate::api::models::model_allowed_for_info(&model_name, &allowed_models) {
                        continue;
                    }

                    // Get value from model_mapping
                    let value = provider.model_mapping.get(&model_name);
                    let mapped_model = value
                        .map(|v| v.mapped_model().to_string())
                        .unwrap_or_default();

                    // Build base model_info
                    let mut model_info = ModelInfoDetails::new(
                        provider.name.clone(),
                        provider.provider_type.clone(),
                        provider.weight,
                        crate::api::models::is_pattern(&model_name),
                    );

                    // Apply extended metadata if available
                    if let Some(metadata) = value.and_then(|v| v.metadata()) {
                        model_info = model_info.with_metadata(metadata);
                    }

                    data.push(ModelInfoEntry {
                        model_name: model_name.clone(),
                        litellm_params: LiteLlmParams {
                            model: mapped_model,
                            api_base: provider.api_base.clone(),
                            custom_llm_provider: provider.provider_type.clone(),
                        },
                        model_info,
                    });
                }
            }

            // Apply filters

            // 1. Exact match by model name
            if let Some(ref model) = query.model {
                data.retain(|entry| entry.model_name == *model);
            }

            // 2. Filter by modelId (mapped model)
            if let Some(ref model_id) = query.model_id {
                data.retain(|entry| entry.litellm_params.model == *model_id);
            }

            // 3. Fuzzy search in model_name
            if let Some(ref search) = query.search {
                let search_lower = search.to_lowercase();
                data.retain(|entry| entry.model_name.to_lowercase().contains(&search_lower));
            }

            // Apply sorting (only if sortBy is specified)
            if let Some(ref sort_by) = query.sort_by {
                let sort_asc = query.sort_order.to_lowercase() != "desc";
                match sort_by.as_str() {
                    "provider_name" => {
                        if sort_asc {
                            data.sort_by(|a, b| {
                                a.model_info.provider_name.cmp(&b.model_info.provider_name)
                            });
                        } else {
                            data.sort_by(|a, b| {
                                b.model_info.provider_name.cmp(&a.model_info.provider_name)
                            });
                        }
                    }
                    "weight" => {
                        if sort_asc {
                            data.sort_by(|a, b| a.model_info.weight.cmp(&b.model_info.weight));
                        } else {
                            data.sort_by(|a, b| b.model_info.weight.cmp(&a.model_info.weight));
                        }
                    }
                    _ => {
                        // Default: sort by model_name
                        if sort_asc {
                            data.sort_by(|a, b| a.model_name.cmp(&b.model_name));
                        } else {
                            data.sort_by(|a, b| b.model_name.cmp(&a.model_name));
                        }
                    }
                }
            }

            // Apply pagination
            let total = data.len();
            let page = query.page.max(1);
            let size = query.size.clamp(1, 100);
            let total_pages = if total == 0 { 0 } else { total.div_ceil(size) };
            let start = (page - 1) * size;
            let paginated_data: Vec<ModelInfoEntry> = if start < total {
                data.into_iter().skip(start).take(size).collect()
            } else {
                Vec::new()
            };

            Ok(Json(PaginatedModelInfoList {
                data: paginated_data,
                total_count: total,
                current_page: page,
                size,
                total_pages,
            }))
        })
        .await
}

/// Handle legacy completions endpoint (V2).
///
/// This endpoint is compatible with the OpenAI Completions API (legacy).
pub async fn completions_v2(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Response> {
    let request_id = generate_request_id();
    let key_config = verify_auth(
        &headers,
        &state.app_state,
        AuthFormat::MultiFormat,
        Some("/v2/completions"),
    )?;
    let api_key_name = key_config
        .as_ref()
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "anonymous".to_string());

    REQUEST_ID
        .scope(request_id.clone(), async move {
            if !payload.is_object() {
                return Err(AppError::BadRequest(
                    "Request body must be a JSON object".to_string(),
                ));
            }

            // Extract model from payload if available
            let original_model = payload.get("model").and_then(|m| m.as_str());

            // Strip provider suffix if configured
            let effective_model = original_model.map(|m| {
                strip_provider_suffix(m, state.app_state.config.provider_suffix.as_deref())
            });
            let model_label = effective_model.as_deref().unwrap_or("unknown").to_string();

            // Get fresh provider service from DynamicConfig
            let provider_service = state.app_state.get_provider_service();

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
                        "Processing completions request (V2)"
                    );

                    let request = build_upstream_request(
                        &state.app_state.http_client,
                        &url,
                        &payload,
                        UpstreamAuth::Bearer(&provider.api_key),
                        None,
                        None,
                    );

                    let upstream_ctx = UpstreamContext {
                        protocol: Protocol::OpenAI,
                        model: Some(&model_label),
                        provider: &provider.name,
                        api_key_name: Some(&api_key_name),
                        request_id: Some(&request_id),
                    };

                    let response = match execute_upstream_request_or_transport_error(
                        request,
                        &provider_service,
                        &upstream_ctx,
                        Some(&url),
                        Some(&model_label),
                        "HTTP request failed to provider",
                    )
                    .await
                    .map_err(|(_, resp)| resp)
                    {
                        Ok(resp) => resp,
                        Err(error_response) => {
                            return Ok(error_response);
                        }
                    };

                    // Check if backend API returned an error status code
                    let response = match split_upstream_status_error_with_log(
                        response,
                        StatusErrorResponseMode::Passthrough,
                        &upstream_ctx,
                        ERROR_TYPE_API,
                        "Backend API returned error status",
                        false,
                        false,
                    )
                    .await
                    .map_err(|(_, resp)| resp)
                    {
                        Ok(resp) => resp,
                        Err(error_response) => {
                            return Ok(error_response);
                        }
                    };

                    let response_data: Value = match parse_upstream_json_or_error_with_log(
                        response,
                        &upstream_ctx,
                        "Failed to parse response JSON",
                    )
                    .await
                    .map_err(|(_, resp)| resp)
                    {
                        Ok((_status, data)) => data,
                        Err(error_response) => {
                            return Ok(error_response);
                        }
                    };

                    Ok(build_json_response(
                        StatusCode::OK,
                        response_data,
                        Some(&model_label),
                        Some(&provider.name),
                        Some(&api_key_name),
                    ))
                })
                .await
        })
        .await
}

/// Claude token counting endpoint (V2).
///
/// Provides accurate token count for the given messages using tiktoken.
pub async fn count_tokens_v2(
    State(state): State<Arc<ProxyState>>,
    headers: HeaderMap,
    Json(claude_request): Json<ClaudeTokenCountRequest>,
) -> Result<Json<ClaudeTokenCountResponse>> {
    let request_id = generate_request_id();
    let _key_config = verify_auth(
        &headers,
        &state.app_state,
        AuthFormat::MultiFormat,
        Some("/v2/messages/count_tokens"),
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
// Token Counting Helpers (for V2)
// ============================================================================

fn build_claude_messages_for_token_count(
    system: &Option<crate::api::claude_models::ClaudeSystemPrompt>,
    messages: &[crate::api::claude_models::ClaudeMessage],
) -> Vec<Value> {
    use crate::api::claude_models::{ClaudeMessageContent, ClaudeSystemPrompt};

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

fn convert_claude_block_for_token_count(
    block: &crate::api::claude_models::ClaudeContentBlock,
) -> Value {
    use crate::api::claude_models::ClaudeContentBlock;

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

fn build_openai_tools_for_token_count(
    tools: &Option<Vec<crate::api::claude_models::ClaudeTool>>,
) -> Option<Vec<Value>> {
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::upstream::build_gcp_vertex_url;

    #[test]
    fn test_extract_model_from_request() {
        let payload = json!({ "model": "gpt-4", "messages": [] });
        assert_eq!(
            extract_model_from_request(&payload, Protocol::OpenAI),
            "gpt-4"
        );
        assert_eq!(
            extract_model_from_request(&payload, Protocol::GcpVertex),
            "gpt-4"
        );
    }

    #[test]
    fn test_extract_stream_flag() {
        let payload = json!({ "model": "gpt-4", "stream": true });
        assert!(extract_stream_flag(&payload));

        let payload = json!({ "model": "gpt-4" });
        assert!(!extract_stream_flag(&payload));
    }

    #[test]
    fn test_get_provider_endpoint() {
        assert_eq!(get_provider_endpoint(Protocol::OpenAI), "/chat/completions");
        assert_eq!(get_provider_endpoint(Protocol::Anthropic), "/v1/messages");
        assert_eq!(get_provider_endpoint(Protocol::ResponseApi), "/responses");
        assert_eq!(get_provider_endpoint(Protocol::GcpVertex), "");
    }

    #[test]
    fn test_build_gcp_vertex_url() {
        let url = build_gcp_vertex_url(
            "https://us-central1-aiplatform.googleapis.com",
            "my-project",
            "us-central1",
            "anthropic",
            "claude-3-5-sonnet@20241022",
            false,
        )
        .expect("valid GCP Vertex URL");
        assert_eq!(
            url,
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/publishers/anthropic/models/claude-3-5-sonnet@20241022:rawPredict"
        );

        let url_streaming = build_gcp_vertex_url(
            "https://us-central1-aiplatform.googleapis.com",
            "my-project",
            "us-central1",
            "anthropic",
            "claude-3-5-sonnet@20241022",
            true,
        )
        .expect("valid GCP Vertex streaming URL");
        assert_eq!(
            url_streaming,
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/publishers/anthropic/models/claude-3-5-sonnet@20241022:streamRawPredict"
        );
    }

    #[test]
    fn test_normalize_gemini3_provider_payload_openai() {
        let mut payload = json!({
            "model": "gemini-3-pro",
            "thinking_level": "low",
            "thinkingConfig": {"thinkingLevel": "low"},
            "messages": [{
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "do", "arguments": "{}"}
                }]
            }]
        });

        normalize_gemini3_provider_payload(&mut payload, Protocol::OpenAI);

        assert!(payload.get("thinking_level").is_none());
        assert!(payload.get("thinkingConfig").is_none());

        let tool_call = &payload["messages"][0]["tool_calls"][0];
        assert!(tool_call["provider_specific_fields"]["thought_signature"].is_string());
        assert!(tool_call["function"]["provider_specific_fields"]["thought_signature"].is_string());
        assert!(tool_call["extra_content"]["google"]["thought_signature"].is_string());
    }

    #[test]
    fn test_normalize_gemini3_provider_payload_non_gemini_noop() {
        let mut payload = json!({
            "model": "gpt-4",
            "thinking_level": "low",
            "thinkingConfig": {"thinkingLevel": "low"},
            "messages": [{
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "do", "arguments": "{}"}
                }]
            }]
        });

        normalize_gemini3_provider_payload(&mut payload, Protocol::OpenAI);

        assert!(payload.get("thinking_level").is_some());
        assert!(payload.get("thinkingConfig").is_some());

        let tool_call = &payload["messages"][0]["tool_calls"][0];
        assert!(tool_call.get("provider_specific_fields").is_none());
        assert!(tool_call["function"]
            .get("provider_specific_fields")
            .is_none());
        assert!(tool_call.get("extra_content").is_none());
    }

    // ========================================================================
    // ensure_tool_use_result_pairing tests
    // ========================================================================

    #[test]
    fn test_tool_pairing_openai_no_orphans() {
        let mut payload = json!({
            "messages": [
                {"role": "user", "content": "Read file"},
                {"role": "assistant", "content": "OK", "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "read", "arguments": "{}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_1", "content": "file content"}
            ]
        });
        let original = payload.clone();
        ensure_tool_use_result_pairing(&mut payload);
        assert_eq!(payload, original);
    }

    #[test]
    fn test_tool_pairing_openai_orphan_injected() {
        let mut payload = json!({
            "messages": [
                {"role": "user", "content": "Read file"},
                {"role": "assistant", "content": "OK", "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "read", "arguments": "{}"}}
                ]},
                {"role": "user", "content": "Never mind"}
            ]
        });
        ensure_tool_use_result_pairing(&mut payload);
        let msgs = payload["messages"].as_array().unwrap();
        // Placeholder inserted after assistant (index 1), before user (now index 3)
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[2]["role"], "tool");
        assert_eq!(msgs[2]["tool_call_id"], "call_1");
    }

    #[test]
    fn test_tool_pairing_openai_multiple_orphans() {
        let mut payload = json!({
            "messages": [
                {"role": "assistant", "content": "OK", "tool_calls": [
                    {"id": "call_a", "type": "function", "function": {"name": "f1", "arguments": "{}"}},
                    {"id": "call_b", "type": "function", "function": {"name": "f2", "arguments": "{}"}}
                ]},
                {"role": "tool", "tool_call_id": "call_a", "content": "result_a"},
                {"role": "user", "content": "next"}
            ]
        });
        ensure_tool_use_result_pairing(&mut payload);
        let msgs = payload["messages"].as_array().unwrap();
        // call_b is orphaned, placeholder inserted after assistant at index 0
        assert_eq!(msgs.len(), 4);
        assert_eq!(msgs[1]["role"], "tool");
        assert_eq!(msgs[1]["tool_call_id"], "call_b");
    }

    #[test]
    fn test_tool_pairing_anthropic_no_orphans() {
        let mut payload = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Let me read"},
                    {"type": "tool_use", "id": "tu_1", "name": "read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_1", "content": "file data"}
                ]}
            ]
        });
        let original = payload.clone();
        ensure_tool_use_result_pairing(&mut payload);
        assert_eq!(payload, original);
    }

    #[test]
    fn test_tool_pairing_anthropic_orphan_injected() {
        let mut payload = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tu_1", "name": "read", "input": {}}
                ]},
                {"role": "user", "content": "Forget it"}
            ]
        });
        ensure_tool_use_result_pairing(&mut payload);
        let msgs = payload["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1]["role"], "user");
        let blocks = msgs[1]["content"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "tool_result");
        assert_eq!(blocks[0]["tool_use_id"], "tu_1");
        assert_eq!(blocks[0]["is_error"], true);
    }
}
