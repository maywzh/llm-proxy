//! GCP Vertex AI proxy handler for Anthropic Claude models.
//!
//! This module provides a proxy for GCP Vertex AI endpoints that use Anthropic's
//! Claude models with rawPredict/streamRawPredict actions.
//!
//! URL format:
//! `/models/gcp-vertex/v1/projects/{project}/locations/{location}/publishers/{publisher}/models/{model}:{action}`
//!
//! Actions:
//! - `rawPredict`: Non-streaming request
//! - `streamRawPredict`: Streaming request

use std::sync::Arc;
use std::time::Instant;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::Response,
    Json,
};
use futures::StreamExt;
use serde_json::Value;

use crate::api::auth::{verify_auth, AuthFormat};
use crate::api::disconnect::DisconnectStream;
use crate::api::proxy::ProxyState;
use crate::api::rectifier::sanitize_provider_payload;
use crate::api::upstream::{
    attach_response_extensions, build_gcp_vertex_url_with_actions, build_protocol_error_response,
    build_upstream_request, execute_upstream_request_or_transport_error,
    finalize_non_streaming_response, parse_upstream_json_or_error_with_log, record_token_metrics,
    split_upstream_status_error_with_log, StatusErrorResponseMode, UpstreamAuth, UpstreamContext,
};
use crate::core::error_types::{ERROR_TYPE_API, ERROR_TYPE_INVALID_REQUEST};
use crate::core::header_policy::sanitize_anthropic_beta_header;
use crate::core::jsonl_logger::{
    log_provider_request, log_provider_response, log_provider_streaming_response, log_request,
    log_streaming_response,
};
use crate::core::langfuse::{
    fail_generation_if_sampled, finish_generation_if_sampled, init_langfuse_trace, GenerationData,
};
use crate::core::logging::{generate_request_id, PROVIDER_CONTEXT};
use crate::core::metrics::get_metrics;
use crate::core::middleware::extract_client;
use crate::core::stream_metrics::{record_stream_metrics, StreamStats};
use crate::core::utils::get_key_name;
use crate::core::Result;
use crate::core::StreamCancelHandle;
use crate::transformer::Protocol;
use crate::with_request_context;

// ============================================================================
// Path Parsing
// ============================================================================

/// Known streaming action verbs for GCP Vertex endpoints.
const STREAMING_ACTIONS: &[&str] = &["streamRawPredict", "streamGenerateContent"];

/// Known blocking action verbs for GCP Vertex endpoints.
const BLOCKING_ACTIONS: &[&str] = &["rawPredict", "generateContent"];

/// Parse the model and action from the path parameter.
/// The format is `{model}:{action}` where action is a known GCP Vertex verb.
fn parse_model_and_action(model_and_action: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = model_and_action.rsplitn(2, ':').collect();
    if parts.len() != 2 {
        return None;
    }
    // rsplitn reverses the order, so action is first, model is second
    let action = parts[0].to_string();
    let model = parts[1].to_string();

    // Validate action
    if !STREAMING_ACTIONS.contains(&action.as_str()) && !BLOCKING_ACTIONS.contains(&action.as_str())
    {
        return None;
    }

    Some((model, action))
}

/// Check if an action verb represents a streaming request.
fn is_streaming_action(action: &str) -> bool {
    STREAMING_ACTIONS.contains(&action)
}

// ============================================================================
// Handler
// ============================================================================

/// GCP Vertex AI proxy handler.
///
/// Proxies requests to GCP Vertex AI Anthropic Claude models.
///
/// URL format:
/// `/models/gcp-vertex/v1/projects/{project}/locations/{location}/publishers/{publisher}/models/{model}:{action}`
pub async fn gcp_vertex_proxy(
    State(state): State<Arc<ProxyState>>,
    Path((project, location, publisher, model_and_action)): Path<(String, String, String, String)>,
    headers: HeaderMap,
    Json(mut payload): Json<Value>,
) -> Result<Response> {
    let request_id = generate_request_id();
    // GCP Vertex proxy forwards to upstream LLM, so it should be rate limited
    let key_config = verify_auth(
        &headers,
        &state.app_state,
        AuthFormat::MultiFormat,
        Some("/models/gcp-vertex"),
    )?;
    let api_key_name = get_key_name(&key_config);

    // Parse model and action from path
    let (model, action) = match parse_model_and_action(&model_and_action) {
        Some((m, a)) => (m, a),
        None => {
            return Ok(build_protocol_error_response(
                Protocol::GcpVertex,
                StatusCode::BAD_REQUEST,
                ERROR_TYPE_INVALID_REQUEST,
                &format!(
                    "Invalid model:action format. Expected 'model:<action>' where action is one of {:?} or {:?}, got '{}'",
                    BLOCKING_ACTIONS, STREAMING_ACTIONS, model_and_action
                ),
                None,
                None,
                Some(&api_key_name),
            ));
        }
    };

    let is_streaming = is_streaming_action(&action);

    // Set stream flag in payload based on action
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("stream".to_string(), Value::Bool(is_streaming));
    }

    // Sanitize payload before forwarding to provider.
    sanitize_provider_payload(&mut payload);

    with_request_context!(request_id.clone(), api_key_name.clone(), async move {
        let client = extract_client(&headers);
        let request_start = Instant::now();

        // Initialize Langfuse tracing
        let endpoint = format!(
            "/models/gcp-vertex/v1/projects/{}/locations/{}/publishers/{}/models/{}:{}",
            project, location, publisher, model, action
        );
        let (trace_id, mut generation_data) =
            init_langfuse_trace(&request_id, &api_key_name, &headers, &endpoint);

        // Set generation data
        generation_data.original_model = model.clone();
        generation_data.is_streaming = is_streaming;

        // Select provider for the model
        let provider_service = state.app_state.get_provider_service();
        let provider = match provider_service.get_next_provider(Some(&model)) {
            Ok(p) => p,
            Err(err) => {
                tracing::error!(
                    request_id = %request_id,
                    error = %err,
                    model = %model,
                    "Provider selection failed"
                );
                return Ok(build_protocol_error_response(
                    Protocol::GcpVertex,
                    StatusCode::BAD_REQUEST,
                    ERROR_TYPE_INVALID_REQUEST,
                    &err,
                    Some(&model),
                    None,
                    Some(&api_key_name),
                ));
            }
        };

        // Update generation data with provider info
        generation_data.provider_key = provider.name.clone();
        generation_data.provider_type = provider.provider_type.clone();
        generation_data.provider_api_base = provider.api_base.clone();
        let mapped_model = provider.get_mapped_model(&model);
        generation_data.mapped_model = mapped_model.clone();

        // Build the upstream URL using the action from the client URL directly
        let upstream_url = match build_gcp_vertex_url_with_actions(
            &provider.api_base,
            &project,
            &location,
            &publisher,
            &mapped_model,
            is_streaming,
            &action,
            &action,
        ) {
            Ok(url) => url,
            Err(err) => {
                tracing::error!(
                    request_id = %request_id,
                    provider = %provider.name,
                    error = %err,
                    "GCP Vertex URL validation failed"
                );
                return Ok(build_protocol_error_response(
                    Protocol::GcpVertex,
                    StatusCode::BAD_REQUEST,
                    ERROR_TYPE_INVALID_REQUEST,
                    &err,
                    Some(&model),
                    Some(&provider.name),
                    Some(&api_key_name),
                ));
            }
        };

        // Log request
        log_request(&request_id, &endpoint, &provider.name, &payload);

        PROVIDER_CONTEXT
            .scope(provider.name.clone(), async move {
                tracing::debug!(
                    request_id = %request_id,
                    provider = %provider.name,
                    model = %model,
                    action = %action,
                    upstream_url = %upstream_url,
                    "Processing GCP Vertex AI request"
                );

                // Log provider request
                log_provider_request(
                    &request_id,
                    &provider.name,
                    &provider.api_base,
                    &endpoint,
                    &payload,
                );

                // Build and send request
                // GCP Vertex AI uses Bearer token authentication with anthropic-version header
                let anthropic_version = headers
                    .get("anthropic-version")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("vertex-2023-10-16");

                let anthropic_beta_header = sanitize_anthropic_beta_header(
                    &provider.provider_type,
                    &provider.provider_params,
                    headers.get("anthropic-beta").and_then(|v| v.to_str().ok()),
                );

                let request = build_upstream_request(
                    &state.app_state.http_client,
                    &upstream_url,
                    &payload,
                    UpstreamAuth::Bearer(&provider.api_key),
                    Some(anthropic_version),
                    anthropic_beta_header.as_deref(),
                );

                let upstream_ctx = UpstreamContext {
                    protocol: Protocol::GcpVertex,
                    model: Some(&model),
                    provider: &provider.name,
                    api_key_name: Some(&api_key_name),
                    request_id: Some(&request_id),
                };

                let response = match execute_upstream_request_or_transport_error(
                    request,
                    &provider_service,
                    &upstream_ctx,
                    None,
                    Some(&model),
                    "HTTP request failed",
                )
                .await
                {
                    Ok(resp) => resp,
                    Err((error_message, error_response)) => {
                        fail_generation_if_sampled(&trace_id, &mut generation_data, error_message);
                        return Ok(error_response);
                    }
                };

                // Handle error responses
                let response = match split_upstream_status_error_with_log(
                    response,
                    StatusErrorResponseMode::Protocol,
                    &upstream_ctx,
                    ERROR_TYPE_API,
                    "GCP Vertex AI returned error",
                    true,
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

                if is_streaming {
                    handle_streaming_response(
                        response,
                        model.clone(),
                        provider.name.clone(),
                        api_key_name.clone(),
                        client.clone(),
                        generation_data,
                        trace_id,
                        request_id.clone(),
                        request_start,
                        payload.clone(),
                    )
                    .await
                } else {
                    handle_non_streaming_response(
                        response,
                        model.clone(),
                        provider.name.clone(),
                        api_key_name.clone(),
                        generation_data,
                        trace_id,
                        request_id.clone(),
                    )
                    .await
                }
            })
            .await
    })
}

/// Handle streaming response for GCP Vertex AI.
#[allow(clippy::too_many_arguments)]
async fn handle_streaming_response(
    response: reqwest::Response,
    model: String,
    provider_name: String,
    api_key_name: String,
    client: String,
    generation_data: GenerationData,
    trace_id: Option<String>,
    request_id: String,
    request_start: Instant,
    _request_payload: Value,
) -> Result<Response> {
    let stream = response.bytes_stream();
    let cancel_handle = StreamCancelHandle::new();
    let cancel_rx = cancel_handle.subscribe();
    let cancel_handle_for_completion = cancel_handle.clone();

    struct StreamingState {
        stream: std::pin::Pin<
            Box<
                dyn futures::Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>>
                    + Send,
            >,
        >,
        model: String,
        provider_name: String,
        api_key_name: String,
        client: String,
        request_id: String,
        start_time: Instant,
        first_token_time: Option<Instant>,
        accumulated_output: String,
        input_tokens: u32,
        output_tokens: u32,
        finish_reason: Option<String>,
        accumulated_sse_data: Vec<String>,
        sse_line_buffer: String,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
        cancel_handle: StreamCancelHandle,
        generation_data: GenerationData,
        trace_id: Option<String>,
        finalized: bool,
    }

    let streaming_state = StreamingState {
        stream: Box::pin(stream),
        model: model.clone(),
        provider_name: provider_name.clone(),
        api_key_name: api_key_name.clone(),
        client: client.clone(),
        request_id: request_id.clone(),
        start_time: request_start,
        first_token_time: None,
        accumulated_output: String::new(),
        input_tokens: 0,
        output_tokens: 0,
        finish_reason: None,
        accumulated_sse_data: Vec::new(),
        sse_line_buffer: String::new(),
        cancel_rx,
        cancel_handle: cancel_handle_for_completion,
        generation_data,
        trace_id,
        finalized: false,
    };

    let transform_stream = futures::stream::unfold(streaming_state, |mut state| async move {
        use tokio::select;

        if state.finalized {
            return None;
        }

        // Race between stream data and cancellation
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
                        provider = %state.provider_name,
                        model = %state.model,
                        "Client disconnected during GCP Vertex AI streaming"
                    );
                    get_metrics().client_disconnects_total.inc();

                    let stats = StreamStats {
                        model: state.model.clone(),
                        provider: state.provider_name.clone(),
                        api_key_name: state.api_key_name.clone(),
                        client: state.client.clone(),
                        input_tokens: state.input_tokens as usize,
                        output_tokens: state.output_tokens as usize,
                        start_time: state.start_time,
                        first_token_time: state.first_token_time,
                    };
                    record_stream_metrics(&stats);

                    return None;
                }
            }
        };

        match chunk_result {
            Some(Ok(bytes)) => {
                if state.first_token_time.is_none() {
                    state.first_token_time = Some(Instant::now());
                }

                // Append to SSE line buffer
                let chunk_str = String::from_utf8_lossy(&bytes);
                state.sse_line_buffer.push_str(&chunk_str);

                // Extract complete SSE events
                let mut complete_events = Vec::new();
                while let Some(pos) = state.sse_line_buffer.find("\n\n") {
                    let event = state.sse_line_buffer[..pos].to_string();
                    state.sse_line_buffer = state.sse_line_buffer[pos + 2..].to_string();
                    if !event.trim().is_empty() {
                        complete_events.push(event);
                    }
                }

                if complete_events.is_empty() {
                    return Some((Ok::<_, std::io::Error>(axum::body::Bytes::new()), state));
                }

                let mut output = String::new();
                for event in complete_events {
                    // Accumulate raw SSE data
                    for line in event.lines() {
                        if line.starts_with("data: ") {
                            state.accumulated_sse_data.push(line.to_string());
                        }
                    }

                    // Parse and extract content/usage from Anthropic SSE events
                    for line in event.lines() {
                        if let Some(data_str) = line.strip_prefix("data: ") {
                            if let Ok(data) = serde_json::from_str::<Value>(data_str) {
                                // Extract text from content_block_delta
                                if let Some(delta) = data.get("delta") {
                                    if delta.get("type").and_then(|t| t.as_str())
                                        == Some("text_delta")
                                    {
                                        if let Some(text) =
                                            delta.get("text").and_then(|t| t.as_str())
                                        {
                                            state.accumulated_output.push_str(text);
                                        }
                                    }
                                    // Extract stop_reason from message_delta
                                    if let Some(reason) =
                                        delta.get("stop_reason").and_then(|r| r.as_str())
                                    {
                                        state.finish_reason = Some(reason.to_string());
                                    }
                                }
                                // Extract usage
                                if let Some(usage) = data.get("usage") {
                                    if let Some(input) =
                                        usage.get("input_tokens").and_then(|t| t.as_u64())
                                    {
                                        state.input_tokens = input as u32;
                                    }
                                    if let Some(output_t) =
                                        usage.get("output_tokens").and_then(|t| t.as_u64())
                                    {
                                        state.output_tokens = output_t as u32;
                                    }
                                }
                            }
                        }
                    }

                    // Pass through the event
                    output.push_str(&event);
                    output.push_str("\n\n");
                }

                Some((Ok(axum::body::Bytes::from(output)), state))
            }
            Some(Err(e)) => Some((Err(std::io::Error::other(e.to_string())), state)),
            None => {
                // Process remaining buffer
                if !state.sse_line_buffer.trim().is_empty() {
                    let remaining = std::mem::take(&mut state.sse_line_buffer);
                    for line in remaining.lines() {
                        if line.starts_with("data: ") {
                            state.accumulated_sse_data.push(line.to_string());
                        }
                    }
                }

                // Mark completion
                state.cancel_handle.mark_completed();

                // Record metrics
                let stats = StreamStats {
                    model: state.model.clone(),
                    provider: state.provider_name.clone(),
                    api_key_name: state.api_key_name.clone(),
                    client: state.client.clone(),
                    input_tokens: state.input_tokens as usize,
                    output_tokens: state.output_tokens as usize,
                    start_time: state.start_time,
                    first_token_time: state.first_token_time,
                };
                record_stream_metrics(&stats);

                // Record Langfuse
                if state.trace_id.is_some() {
                    state.generation_data.output_content = Some(state.accumulated_output.clone());
                    state.generation_data.finish_reason = state.finish_reason.clone();
                    state.generation_data.prompt_tokens = state.input_tokens;
                    state.generation_data.completion_tokens = state.output_tokens;
                    state.generation_data.total_tokens = state.input_tokens + state.output_tokens;
                    finish_generation_if_sampled(&state.trace_id, &mut state.generation_data);
                }

                // Log streaming response
                if !state.request_id.is_empty() {
                    log_streaming_response(
                        &state.request_id,
                        200,
                        None,
                        state.accumulated_sse_data.clone(),
                    );
                    log_provider_streaming_response(
                        &state.request_id,
                        &state.provider_name,
                        200,
                        None,
                        state.accumulated_sse_data.clone(),
                    );
                }

                None
            }
        }
    });

    let body = Body::from_stream(transform_stream);
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
        Some(&model),
        Some(&provider_name),
        Some(&api_key_name),
    );

    Ok(resp)
}

/// Handle non-streaming response for GCP Vertex AI.
#[allow(clippy::too_many_arguments)]
async fn handle_non_streaming_response(
    response: reqwest::Response,
    model: String,
    provider_name: String,
    api_key_name: String,
    generation_data: GenerationData,
    trace_id: Option<String>,
    request_id: String,
) -> Result<Response> {
    let mut generation_data = generation_data;

    let (status, response_data): (reqwest::StatusCode, Value) =
        match parse_upstream_json_or_error_with_log(
            response,
            &UpstreamContext {
                protocol: Protocol::GcpVertex,
                model: Some(&model),
                provider: &provider_name,
                api_key_name: Some(&api_key_name),
                request_id: Some(&request_id),
            },
            "Failed to parse response JSON",
        )
        .await
        {
            Ok((status, data)) => (status, data),
            Err((error_message, error_response)) => {
                fail_generation_if_sampled(&trace_id, &mut generation_data, error_message);
                return Ok(error_response);
            }
        };

    // Log provider response
    log_provider_response(
        &request_id,
        &provider_name,
        status.as_u16(),
        None,
        &response_data,
    );

    // Extract usage for metrics and Langfuse
    if let Some(usage) = response_data.get("usage") {
        if let (Some(input), Some(output)) = (
            usage.get("input_tokens").and_then(|t| t.as_u64()),
            usage.get("output_tokens").and_then(|t| t.as_u64()),
        ) {
            generation_data.prompt_tokens = input as u32;
            generation_data.completion_tokens = output as u32;
            generation_data.total_tokens = (input + output) as u32;

            record_token_metrics(
                input,
                output,
                &model,
                &provider_name,
                &api_key_name,
                "unknown",
            );
        }
    }

    tracing::debug!(
        request_id = %request_id,
        model = %model,
        provider = %provider_name,
        "GCP Vertex AI non-streaming response completed"
    );

    Ok(finalize_non_streaming_response(
        trace_id.as_deref(),
        &mut generation_data,
        &request_id,
        status.as_u16(),
        response_data,
        &model,
        &provider_name,
        &api_key_name,
    ))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_and_action_raw_predict() {
        let result = parse_model_and_action("claude-sonnet-4-5:rawPredict");
        assert!(result.is_some());
        let (model, action) = result.unwrap();
        assert_eq!(model, "claude-sonnet-4-5");
        assert_eq!(action, "rawPredict");
    }

    #[test]
    fn test_parse_model_and_action_stream_raw_predict() {
        let result = parse_model_and_action("claude-3-opus:streamRawPredict");
        assert!(result.is_some());
        let (model, action) = result.unwrap();
        assert_eq!(model, "claude-3-opus");
        assert_eq!(action, "streamRawPredict");
    }

    #[test]
    fn test_parse_model_and_action_invalid_action() {
        let result = parse_model_and_action("claude-sonnet-4-5:predict");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_model_and_action_no_colon() {
        let result = parse_model_and_action("claude-sonnet-4-5");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_model_and_action_empty() {
        let result = parse_model_and_action("");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_model_and_action_with_version() {
        let result = parse_model_and_action("claude-3-5-sonnet@20240620:rawPredict");
        assert!(result.is_some());
        let (model, action) = result.unwrap();
        assert_eq!(model, "claude-3-5-sonnet@20240620");
        assert_eq!(action, "rawPredict");
    }

    #[test]
    fn test_parse_model_and_action_generate_content() {
        let result = parse_model_and_action("gemini-3-pro-preview:generateContent");
        assert!(result.is_some());
        let (model, action) = result.unwrap();
        assert_eq!(model, "gemini-3-pro-preview");
        assert_eq!(action, "generateContent");
    }

    #[test]
    fn test_parse_model_and_action_stream_generate_content() {
        let result = parse_model_and_action("gemini-3-pro-preview:streamGenerateContent");
        assert!(result.is_some());
        let (model, action) = result.unwrap();
        assert_eq!(model, "gemini-3-pro-preview");
        assert_eq!(action, "streamGenerateContent");
    }

    #[test]
    fn test_is_streaming_action() {
        assert!(is_streaming_action("streamRawPredict"));
        assert!(is_streaming_action("streamGenerateContent"));
        assert!(!is_streaming_action("rawPredict"));
        assert!(!is_streaming_action("generateContent"));
    }
}
