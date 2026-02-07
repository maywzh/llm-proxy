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
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use futures::StreamExt;
use serde_json::{json, Value};

use crate::api::disconnect::DisconnectStream;
use crate::api::proxy::{build_gcp_vertex_url, verify_auth_multi_format, ProxyState};
use crate::core::jsonl_logger::{
    log_provider_request, log_provider_response, log_provider_streaming_response, log_request,
    log_response, log_streaming_response,
};
use crate::core::langfuse::{get_langfuse_service, init_langfuse_trace, GenerationData};
use crate::core::logging::{generate_request_id, PROVIDER_CONTEXT};
use crate::core::metrics::get_metrics;
use crate::core::middleware::{extract_client, ApiKeyName, ModelName, ProviderName};
use crate::core::stream_metrics::{record_stream_metrics, StreamStats};
use crate::core::utils::get_key_name;
use crate::core::Result;
use crate::core::StreamCancelHandle;
use crate::with_request_context;

// ============================================================================
// Error Response Builder
// ============================================================================

fn build_vertex_error_response(
    status: StatusCode,
    error_type: &str,
    message: &str,
    model: Option<&str>,
    provider: Option<&str>,
    api_key_name: Option<&str>,
) -> Response {
    let body = json!({
        "type": "error",
        "error": {
            "type": error_type,
            "message": message
        }
    });

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
// Path Parsing
// ============================================================================

/// Parse the model and action from the path parameter.
/// The format is `{model}:{action}` where action is rawPredict or streamRawPredict.
fn parse_model_and_action(model_and_action: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = model_and_action.rsplitn(2, ':').collect();
    if parts.len() != 2 {
        return None;
    }
    // rsplitn reverses the order, so action is first, model is second
    let action = parts[0].to_string();
    let model = parts[1].to_string();

    // Validate action
    if action != "rawPredict" && action != "streamRawPredict" {
        return None;
    }

    Some((model, action))
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
    let key_config = verify_auth_multi_format(&headers, &state.app_state)?;
    let api_key_name = get_key_name(&key_config);

    // Parse model and action from path
    let (model, action) = match parse_model_and_action(&model_and_action) {
        Some((m, a)) => (m, a),
        None => {
            return Ok(build_vertex_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                &format!(
                    "Invalid model:action format. Expected 'model:rawPredict' or 'model:streamRawPredict', got '{}'",
                    model_and_action
                ),
                None,
                None,
                Some(&api_key_name),
            ));
        }
    };

    let is_streaming = action == "streamRawPredict";

    // Set stream flag in payload based on action
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("stream".to_string(), Value::Bool(is_streaming));
    }

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
                return Ok(build_vertex_error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
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

        // Build the upstream URL using the shared helper function
        let upstream_url = build_gcp_vertex_url(
            &provider.api_base,
            &project,
            &location,
            &publisher,
            &mapped_model,
            is_streaming,
        );

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

                let mut req = state
                    .app_state
                    .http_client
                    .post(&upstream_url)
                    .header("Authorization", format!("Bearer {}", provider.api_key))
                    .header("Content-Type", "application/json")
                    .header("anthropic-version", anthropic_version);

                // Forward anthropic-beta header if provided by client
                if let Some(beta) = headers.get("anthropic-beta") {
                    if let Ok(beta_str) = beta.to_str() {
                        req = req.header("anthropic-beta", beta_str);
                    }
                }

                let response = match req.json(&payload).send().await {
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
                        return Ok(build_vertex_error_response(
                            status,
                            error_type,
                            &format!("Upstream request failed: {}", e),
                            Some(&model),
                            Some(&provider.name),
                            Some(&api_key_name),
                        ));
                    }
                };

                let status = response.status();

                // Handle error responses
                if status.is_client_error() || status.is_server_error() {
                    let error_body = match response.bytes().await {
                        Ok(bytes) => {
                            let text = String::from_utf8_lossy(&bytes).to_string();
                            serde_json::from_slice::<Value>(&bytes).unwrap_or_else(|_| {
                                json!({
                                    "type": "error",
                                    "error": {
                                        "type": "api_error",
                                        "message": text
                                    }
                                })
                            })
                        }
                        Err(_) => json!({
                            "type": "error",
                            "error": {
                                "type": "api_error",
                                "message": format!("HTTP {}", status)
                            }
                        }),
                    };

                    let error_message = error_body
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or(&format!("HTTP {}", status))
                        .to_string();

                    tracing::error!(
                        request_id = %request_id,
                        provider = %provider.name,
                        status = %status,
                        error = %error_message,
                        "GCP Vertex AI returned error"
                    );

                    return Ok(build_vertex_error_response(
                        StatusCode::from_u16(status.as_u16())
                            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        "api_error",
                        &error_message,
                        Some(&model),
                        Some(&provider.name),
                        Some(&api_key_name),
                    ));
                }

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
                    state.generation_data.end_time = Some(Utc::now());
                    if let Ok(service) = get_langfuse_service().read() {
                        service.trace_generation(state.generation_data.clone());
                    }
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

                state.finalized = true;
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

    resp.extensions_mut().insert(ModelName(model));
    resp.extensions_mut().insert(ProviderName(provider_name));
    resp.extensions_mut().insert(ApiKeyName(api_key_name));

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
    let status = response.status();
    let mut generation_data = generation_data;

    let response_data: Value = match response.json().await {
        Ok(data) => data,
        Err(e) => {
            tracing::error!(
                request_id = %request_id,
                provider = %provider_name,
                error = %e,
                "Failed to parse response JSON"
            );
            return Ok(build_vertex_error_response(
                StatusCode::BAD_GATEWAY,
                "api_error",
                &format!("Invalid JSON from provider: {}", e),
                Some(&model),
                Some(&provider_name),
                Some(&api_key_name),
            ));
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

            // Record token metrics
            let metrics = get_metrics();
            metrics
                .token_usage
                .with_label_values(&[&model, &provider_name, "prompt", &api_key_name, "unknown"])
                .inc_by(input);
            metrics
                .token_usage
                .with_label_values(&[
                    &model,
                    &provider_name,
                    "completion",
                    &api_key_name,
                    "unknown",
                ])
                .inc_by(output);
            metrics
                .token_usage
                .with_label_values(&[&model, &provider_name, "total", &api_key_name, "unknown"])
                .inc_by(input + output);
        }
    }

    // Record Langfuse
    generation_data.end_time = Some(Utc::now());
    if trace_id.is_some() {
        if let Ok(service) = get_langfuse_service().read() {
            service.trace_generation(generation_data);
        }
    }

    // Log response
    log_response(&request_id, status.as_u16(), None, &response_data);

    tracing::debug!(
        request_id = %request_id,
        model = %model,
        provider = %provider_name,
        "GCP Vertex AI non-streaming response completed"
    );

    let mut resp = Json(response_data).into_response();
    resp.extensions_mut().insert(ModelName(model));
    resp.extensions_mut().insert(ProviderName(provider_name));
    resp.extensions_mut().insert(ApiKeyName(api_key_name));

    Ok(resp)
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
}
