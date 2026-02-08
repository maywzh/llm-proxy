//! Unified upstream request execution helpers.
//!
//! This module centralizes transport/status feedback reporting for provider
//! adaptive routing to avoid scattered cross-cutting logic across handlers.

use crate::core::middleware::{ApiKeyName, ModelName, ProviderName};
use crate::services::ProviderService;
use crate::transformer::Protocol;
use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::{json, Value};

const MAX_ERROR_MESSAGE_LEN: usize = 500;

use crate::core::error_types::{ERROR_TYPE_API, ERROR_TYPE_TIMEOUT};

/// Common context for upstream operations, reducing parameter passing.
#[derive(Clone, Copy)]
pub struct UpstreamContext<'a> {
    pub protocol: Protocol,
    pub model: Option<&'a str>,
    pub provider: &'a str,
    pub api_key_name: Option<&'a str>,
    pub request_id: Option<&'a str>,
}

/// Authentication mode for upstream provider request.
#[derive(Clone, Copy)]
pub enum UpstreamAuth<'a> {
    Bearer(&'a str),
    XApiKey(&'a str),
}

/// Parsed upstream error payload and derived message.
#[derive(Debug, Clone)]
pub struct UpstreamErrorPayload {
    pub body: Value,
    pub message: String,
    pub raw_text: String,
}

/// Build a provider request with unified auth and optional Anthropic headers.
pub fn build_upstream_request(
    http_client: &reqwest::Client,
    url: &str,
    payload: &serde_json::Value,
    auth: UpstreamAuth<'_>,
    anthropic_version: Option<&str>,
    anthropic_beta: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut request = http_client.post(url);

    request = match auth {
        UpstreamAuth::Bearer(api_key) => {
            request.header("Authorization", format!("Bearer {}", api_key))
        }
        UpstreamAuth::XApiKey(api_key) => request.header("x-api-key", api_key),
    };

    if let Some(version) = anthropic_version {
        request = request.header("anthropic-version", version);
    }
    if let Some(beta) = anthropic_beta {
        request = request.header("anthropic-beta", beta);
    }

    request.json(payload)
}

/// Build full GCP Vertex rawPredict/streamRawPredict URL.
///
/// All path-segment parameters are validated to reject `/` characters,
/// preventing path-traversal attacks. Returns `Err` if any parameter
/// contains path separators or traversal sequences.
pub fn build_gcp_vertex_url(
    api_base: &str,
    gcp_project: &str,
    gcp_location: &str,
    gcp_publisher: &str,
    model: &str,
    is_streaming: bool,
) -> Result<String, String> {
    fn is_safe_path_segment(s: &str) -> bool {
        !s.is_empty() && !s.contains('/') && !s.contains('\\') && s != ".." && s != "."
    }
    if !(is_safe_path_segment(gcp_project)
        && is_safe_path_segment(gcp_location)
        && is_safe_path_segment(gcp_publisher)
        && is_safe_path_segment(model))
    {
        return Err(
            "GCP Vertex URL parameters must not contain path separators or traversal sequences"
                .to_string(),
        );
    }
    let action = if is_streaming {
        "streamRawPredict"
    } else {
        "rawPredict"
    };
    Ok(format!(
        "{}/v1/projects/{}/locations/{}/publishers/{}/models/{}:{}",
        api_base, gcp_project, gcp_location, gcp_publisher, model, action
    ))
}

fn get_anthropic_version<'a>(headers: &'a HeaderMap, default: &'a str) -> &'a str {
    headers
        .get("anthropic-version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or(default)
}

pub fn build_protocol_upstream_request(
    http_client: &reqwest::Client,
    url: &str,
    provider_protocol: Protocol,
    provider_api_key: &str,
    headers: &HeaderMap,
    anthropic_beta_header: Option<&str>,
    payload: &serde_json::Value,
) -> reqwest::RequestBuilder {
    match provider_protocol {
        Protocol::Anthropic => {
            let anthropic_version = get_anthropic_version(headers, "2023-06-01");
            build_upstream_request(
                http_client,
                url,
                payload,
                UpstreamAuth::XApiKey(provider_api_key),
                Some(anthropic_version),
                anthropic_beta_header,
            )
        }
        Protocol::GcpVertex => {
            let anthropic_version = get_anthropic_version(headers, "vertex-2023-10-16");
            build_upstream_request(
                http_client,
                url,
                payload,
                UpstreamAuth::Bearer(provider_api_key),
                Some(anthropic_version),
                anthropic_beta_header,
            )
        }
        _ => build_upstream_request(
            http_client,
            url,
            payload,
            UpstreamAuth::Bearer(provider_api_key),
            None,
            None,
        ),
    }
}

/// Build masked provider request headers for debug logging.
pub fn build_provider_debug_headers(
    provider_protocol: Protocol,
    url: &str,
    headers: &HeaderMap,
    anthropic_beta_header: Option<&str>,
) -> Value {
    let mut header_map = serde_json::Map::new();
    header_map.insert("url".to_string(), json!(url));
    header_map.insert("content-type".to_string(), json!("application/json"));

    match provider_protocol {
        Protocol::Anthropic => {
            header_map.insert("x-api-key".to_string(), json!("***"));
            let anthropic_version = get_anthropic_version(headers, "2023-06-01");
            header_map.insert("anthropic-version".to_string(), json!(anthropic_version));
            if let Some(beta) = anthropic_beta_header {
                header_map.insert("anthropic-beta".to_string(), json!(beta));
            }
        }
        Protocol::GcpVertex => {
            header_map.insert("authorization".to_string(), json!("Bearer ***"));
            let anthropic_version = get_anthropic_version(headers, "vertex-2023-10-16");
            header_map.insert("anthropic-version".to_string(), json!(anthropic_version));
            if let Some(beta) = anthropic_beta_header {
                header_map.insert("anthropic-beta".to_string(), json!(beta));
            }
        }
        _ => {
            header_map.insert("authorization".to_string(), json!("Bearer ***"));
        }
    }

    Value::Object(header_map)
}

fn build_protocol_error_body(
    protocol: Protocol,
    status: StatusCode,
    error_type: &str,
    message: &str,
) -> Value {
    match protocol {
        Protocol::Anthropic | Protocol::GcpVertex => json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message
            }
        }),
        Protocol::OpenAI | Protocol::ResponseApi => json!({
            "error": {
                "message": message,
                "type": error_type,
                "code": status.as_u16()
            }
        }),
    }
}

fn truncate_message(message: &str) -> String {
    let mut chars = message.chars();
    let truncated: String = chars.by_ref().take(MAX_ERROR_MESSAGE_LEN).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

/// Extract canonical error message from provider error payload.
pub fn extract_error_message(body: &Value) -> Option<String> {
    body.get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            body.get("error")
                .and_then(|e| e.as_str())
                .map(|s| s.to_string())
        })
        .or_else(|| {
            body.get("message")
                .and_then(|m| m.as_str())
                .map(|s| s.to_string())
        })
}

/// Attach optional middleware extensions to a response.
pub fn attach_response_extensions(
    response: &mut Response,
    model: Option<&str>,
    provider: Option<&str>,
    api_key_name: Option<&str>,
) {
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
}

/// Build a JSON response with optional middleware extensions.
pub fn build_json_response<T: Serialize>(
    status: StatusCode,
    body: T,
    model: Option<&str>,
    provider: Option<&str>,
    api_key_name: Option<&str>,
) -> Response {
    let mut response = Json(body).into_response();
    *response.status_mut() = status;
    attach_response_extensions(&mut response, model, provider, api_key_name);
    response
}

/// Parse upstream error response and build protocol-specific fallback body.
async fn read_upstream_error(
    response: reqwest::Response,
    status: StatusCode,
    fallback_protocol: Protocol,
    fallback_error_type: &str,
) -> UpstreamErrorPayload {
    let default_message = format!("HTTP {}", status);

    let (parsed_body, raw_text) = match response.bytes().await {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes).to_string();
            let body = serde_json::from_slice::<Value>(&bytes).ok();
            (body, text)
        }
        Err(error) => (None, format!("Failed to read response: {}", error)),
    };

    let message = parsed_body
        .as_ref()
        .and_then(extract_error_message)
        .or_else(|| {
            if raw_text.is_empty() {
                None
            } else {
                Some(truncate_message(&raw_text))
            }
        })
        .unwrap_or(default_message.clone());

    let body = parsed_body.unwrap_or_else(|| {
        // Use truncated message for fallback body to avoid leaking raw upstream details
        let fallback_message = if raw_text.is_empty() {
            default_message.as_str()
        } else {
            message.as_str()
        };
        build_protocol_error_body(
            fallback_protocol,
            status,
            fallback_error_type,
            fallback_message,
        )
    });

    UpstreamErrorPayload {
        body,
        message,
        raw_text,
    }
}

/// Build an error response in protocol-specific format with optional extensions.
pub fn build_protocol_error_response(
    protocol: Protocol,
    status: StatusCode,
    error_type: &str,
    message: &str,
    model: Option<&str>,
    provider: Option<&str>,
    api_key_name: Option<&str>,
) -> Response {
    let body = build_protocol_error_body(protocol, status, error_type, message);
    build_json_response(status, body, model, provider, api_key_name)
}

/// Classify upstream transport errors into HTTP status/type/message.
fn classify_upstream_error(error: &reqwest::Error) -> (StatusCode, &'static str, String) {
    let status = if error.is_timeout() {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    };
    let error_type = if error.is_timeout() {
        ERROR_TYPE_TIMEOUT
    } else {
        ERROR_TYPE_API
    };
    // Sanitize message for client: avoid leaking internal URLs/IPs from reqwest::Error
    let message = if error.is_timeout() {
        "Upstream request timed out".to_string()
    } else if error.is_connect() {
        "Failed to connect to upstream provider".to_string()
    } else {
        "Upstream request failed".to_string()
    };
    (status, error_type, message)
}

/// Build transport error response and emit a unified error log.
fn build_transport_error_response_with_log(
    ctx: &UpstreamContext<'_>,
    error: &reqwest::Error,
    url: Option<&str>,
    log_model: Option<&str>,
    log_message: &str,
) -> (String, Response) {
    let (status, error_type, message) = classify_upstream_error(error);
    let response = build_protocol_error_response(
        ctx.protocol,
        status,
        error_type,
        &message,
        ctx.model,
        Some(ctx.provider),
        ctx.api_key_name,
    );

    tracing::error!(
        request_id = ?ctx.request_id,
        provider = %ctx.provider,
        url = ?url,
        model = ?log_model,
        error = %error,
        is_timeout = error.is_timeout(),
        is_connect = error.is_connect(),
        "{}",
        log_message
    );

    (message, response)
}

/// Build protocol error response for invalid upstream JSON and return canonical message.
pub fn build_invalid_json_error_response(
    protocol: Protocol,
    error: &impl std::fmt::Display,
    model: Option<&str>,
    provider: Option<&str>,
    api_key_name: Option<&str>,
) -> (String, Response) {
    // Log full error internally but only expose generic message to client
    let internal_message = format!("Invalid JSON from provider: {}", error);
    let message = "Invalid JSON response from upstream provider".to_string();
    tracing::debug!(internal_error = %internal_message, "JSON decode failure detail");
    let response = build_protocol_error_response(
        protocol,
        StatusCode::BAD_GATEWAY,
        ERROR_TYPE_API,
        &message,
        model,
        provider,
        api_key_name,
    );
    (message, response)
}

/// Parse upstream JSON body and emit unified structured logs on decode failure.
pub async fn parse_upstream_json_or_error_with_log(
    response: reqwest::Response,
    ctx: &UpstreamContext<'_>,
    log_message: &str,
) -> std::result::Result<(reqwest::StatusCode, Value), (String, Response)> {
    let status = response.status();
    match response.json::<Value>().await {
        Ok(body) => Ok((status, body)),
        Err(error) => {
            let (error_message, error_response) = build_invalid_json_error_response(
                ctx.protocol,
                &error,
                ctx.model,
                Some(ctx.provider),
                ctx.api_key_name,
            );
            tracing::error!(
                request_id = ?ctx.request_id,
                provider = %ctx.provider,
                error = %error_message,
                "{}",
                log_message
            );
            Err((error_message, error_response))
        }
    }
}

/// Normalize reqwest status code into axum status code.
fn normalize_upstream_status(status: reqwest::StatusCode) -> StatusCode {
    StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

/// Parse upstream error response and return normalized status code with payload.
async fn read_upstream_error_with_status(
    response: reqwest::Response,
    status: reqwest::StatusCode,
    fallback_protocol: Protocol,
    fallback_error_type: &str,
) -> (StatusCode, UpstreamErrorPayload) {
    let status_code = normalize_upstream_status(status);
    let parsed = read_upstream_error(
        response,
        status_code,
        fallback_protocol,
        fallback_error_type,
    )
    .await;
    (status_code, parsed)
}

/// Build protocol error response from upstream status error with parsed payload.
async fn build_protocol_status_error_response(
    response: reqwest::Response,
    status: reqwest::StatusCode,
    protocol: Protocol,
    fallback_error_type: &str,
    model: Option<&str>,
    provider: Option<&str>,
    api_key_name: Option<&str>,
) -> (StatusCode, UpstreamErrorPayload, Response) {
    let (status_code, parsed) =
        read_upstream_error_with_status(response, status, protocol, fallback_error_type).await;
    let error_response = build_protocol_error_response(
        protocol,
        status_code,
        fallback_error_type,
        &parsed.message,
        model,
        provider,
        api_key_name,
    );
    (status_code, parsed, error_response)
}

/// Build passthrough-body response from upstream status error with parsed payload.
async fn build_passthrough_status_error_response(
    response: reqwest::Response,
    status: reqwest::StatusCode,
    fallback_protocol: Protocol,
    fallback_error_type: &str,
    model: Option<&str>,
    provider: Option<&str>,
    api_key_name: Option<&str>,
) -> (StatusCode, UpstreamErrorPayload, Response) {
    let (status_code, parsed) =
        read_upstream_error_with_status(response, status, fallback_protocol, fallback_error_type)
            .await;
    let error_response = build_json_response(
        status_code,
        parsed.body.clone(),
        model,
        provider,
        api_key_name,
    );
    (status_code, parsed, error_response)
}

/// Response body mode for upstream HTTP status errors.
#[derive(Clone, Copy, Debug)]
pub enum StatusErrorResponseMode {
    Protocol,
    Passthrough,
}

/// Split status errors and emit unified structured logs.
pub async fn split_upstream_status_error_with_log(
    response: reqwest::Response,
    mode: StatusErrorResponseMode,
    ctx: &UpstreamContext<'_>,
    fallback_error_type: &str,
    log_message: &str,
    include_error_message: bool,
    trace_raw_body: bool,
) -> std::result::Result<reqwest::Response, (UpstreamErrorPayload, Response)> {
    let status = response.status();
    if !(status.is_client_error() || status.is_server_error()) {
        return Ok(response);
    }

    let (status_code, parsed, error_response) = match mode {
        StatusErrorResponseMode::Protocol => {
            build_protocol_status_error_response(
                response,
                status,
                ctx.protocol,
                fallback_error_type,
                ctx.model,
                Some(ctx.provider),
                ctx.api_key_name,
            )
            .await
        }
        StatusErrorResponseMode::Passthrough => {
            build_passthrough_status_error_response(
                response,
                status,
                ctx.protocol,
                fallback_error_type,
                ctx.model,
                Some(ctx.provider),
                ctx.api_key_name,
            )
            .await
        }
    };

    if trace_raw_body && tracing::enabled!(tracing::Level::TRACE) {
        tracing::trace!(
            request_id = ?ctx.request_id,
            provider = %ctx.provider,
            status = %status_code,
            raw_body = %parsed.raw_text,
            "Full error response body from provider"
        );
    }

    if include_error_message {
        tracing::error!(
            request_id = ?ctx.request_id,
            provider = %ctx.provider,
            status = %status_code,
            error_message = %parsed.message,
            "{}",
            log_message
        );
    } else {
        tracing::error!(
            request_id = ?ctx.request_id,
            provider = %ctx.provider,
            status = %status_code,
            "{}",
            log_message
        );
    }

    Err((parsed, error_response))
}

/// Build fallback response when status-error split unexpectedly returns success.
pub fn build_unexpected_status_split_response(
    ctx: &UpstreamContext<'_>,
    context: &str,
) -> Response {
    let message = format!("Unexpected upstream success in {}", context);
    tracing::warn!(provider = %ctx.provider, "{}", message);
    build_protocol_error_response(
        ctx.protocol,
        StatusCode::INTERNAL_SERVER_ERROR,
        ERROR_TYPE_API,
        &message,
        ctx.model,
        Some(ctx.provider),
        ctx.api_key_name,
    )
}

/// Execute an upstream request and report runtime feedback to `ProviderService`.
///
/// - Transport errors trigger `report_transport_error`
/// - HTTP responses trigger `report_http_status` with `Retry-After` support
pub async fn execute_upstream_request(
    request: reqwest::RequestBuilder,
    provider_service: &ProviderService,
    provider_name: &str,
) -> std::result::Result<reqwest::Response, reqwest::Error> {
    match request.send().await {
        Ok(response) => {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok());
            provider_service.report_http_status(
                provider_name,
                response.status().as_u16(),
                retry_after,
            );
            Ok(response)
        }
        Err(error) => {
            provider_service.report_transport_error(provider_name);
            Err(error)
        }
    }
}

/// Execute upstream request and map transport failures into protocol error responses.
pub async fn execute_upstream_request_or_transport_error(
    request: reqwest::RequestBuilder,
    provider_service: &ProviderService,
    ctx: &UpstreamContext<'_>,
    url: Option<&str>,
    log_model: Option<&str>,
    log_message: &str,
) -> std::result::Result<reqwest::Response, (String, Response)> {
    match execute_upstream_request(request, provider_service, ctx.provider).await {
        Ok(response) => Ok(response),
        Err(error) => Err(build_transport_error_response_with_log(
            ctx,
            &error,
            url,
            log_model,
            log_message,
        )),
    }
}

/// Record token usage metrics for prompt, completion, and total.
pub fn record_token_metrics(
    prompt_tokens: u64,
    completion_tokens: u64,
    model: &str,
    provider: &str,
    api_key_name: &str,
    client: &str,
) {
    let metrics = crate::core::metrics::get_metrics();
    let total = prompt_tokens + completion_tokens;

    metrics
        .token_usage
        .with_label_values(&[model, provider, "prompt", api_key_name, client])
        .inc_by(prompt_tokens);
    metrics
        .token_usage
        .with_label_values(&[model, provider, "completion", api_key_name, client])
        .inc_by(completion_tokens);
    metrics
        .token_usage
        .with_label_values(&[model, provider, "total", api_key_name, client])
        .inc_by(total);
}

/// Finalize non-streaming response: Langfuse generation + JSONL log + JSON response.
#[allow(clippy::too_many_arguments)]
pub fn finalize_non_streaming_response(
    trace_id: Option<&str>,
    generation_data: &mut crate::core::langfuse::GenerationData,
    request_id: &str,
    status: u16,
    response_body: serde_json::Value,
    model: &str,
    provider: &str,
    api_key_name: &str,
) -> Response {
    let trace_id_owned = trace_id.map(|s| s.to_string());
    crate::core::langfuse::finish_generation_if_sampled(&trace_id_owned, generation_data);
    crate::core::jsonl_logger::log_response(request_id, status, None, &response_body);
    build_json_response(
        StatusCode::OK,
        response_body,
        Some(model),
        Some(provider),
        Some(api_key_name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Helper to read an axum Response body as JSON Value.
    async fn body_json(response: Response) -> Value {
        let body = response.into_body();
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // -- build_upstream_request ------------------------------------------------

    #[test]
    fn test_build_upstream_request_bearer_auth() {
        let client = reqwest::Client::new();
        let payload = json!({"model": "gpt-4", "messages": []});
        let req = build_upstream_request(
            &client,
            "https://api.example.com/v1/chat/completions",
            &payload,
            UpstreamAuth::Bearer("sk-test-key"),
            None,
            None,
        )
        .build()
        .unwrap();

        assert_eq!(
            req.headers()
                .get("Authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer sk-test-key"
        );
        assert!(req.headers().get("x-api-key").is_none());
        assert!(req.headers().get("anthropic-version").is_none());
    }

    #[test]
    fn test_build_upstream_request_xapikey_auth() {
        let client = reqwest::Client::new();
        let payload = json!({"model": "claude-3-opus"});
        let req = build_upstream_request(
            &client,
            "https://api.anthropic.com/v1/messages",
            &payload,
            UpstreamAuth::XApiKey("sk-ant-key"),
            Some("2023-06-01"),
            None,
        )
        .build()
        .unwrap();

        assert_eq!(
            req.headers().get("x-api-key").unwrap().to_str().unwrap(),
            "sk-ant-key"
        );
        assert!(req.headers().get("Authorization").is_none());
        assert_eq!(
            req.headers()
                .get("anthropic-version")
                .unwrap()
                .to_str()
                .unwrap(),
            "2023-06-01"
        );
    }

    #[test]
    fn test_build_upstream_request_with_anthropic_beta() {
        let client = reqwest::Client::new();
        let payload = json!({"model": "claude-3-opus"});
        let req = build_upstream_request(
            &client,
            "https://api.anthropic.com/v1/messages",
            &payload,
            UpstreamAuth::XApiKey("key"),
            Some("2023-06-01"),
            Some("max-tokens-3-5-sonnet-2024-07-15"),
        )
        .build()
        .unwrap();

        assert_eq!(
            req.headers()
                .get("anthropic-beta")
                .unwrap()
                .to_str()
                .unwrap(),
            "max-tokens-3-5-sonnet-2024-07-15"
        );
    }

    // -- build_protocol_upstream_request ----------------------------------------

    #[test]
    fn test_build_protocol_upstream_request_openai() {
        let client = reqwest::Client::new();
        let payload = json!({"model": "gpt-4"});
        let headers = HeaderMap::new();
        let req = build_protocol_upstream_request(
            &client,
            "https://api.openai.com/v1/chat/completions",
            Protocol::OpenAI,
            "sk-openai-key",
            &headers,
            None,
            &payload,
        )
        .build()
        .unwrap();

        assert_eq!(
            req.headers()
                .get("Authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer sk-openai-key"
        );
        assert!(req.headers().get("anthropic-version").is_none());
    }

    #[test]
    fn test_build_protocol_upstream_request_anthropic() {
        let client = reqwest::Client::new();
        let payload = json!({"model": "claude-3-opus"});
        let headers = HeaderMap::new();
        let req = build_protocol_upstream_request(
            &client,
            "https://api.anthropic.com/v1/messages",
            Protocol::Anthropic,
            "sk-ant-key",
            &headers,
            None,
            &payload,
        )
        .build()
        .unwrap();

        assert_eq!(
            req.headers().get("x-api-key").unwrap().to_str().unwrap(),
            "sk-ant-key"
        );
        assert_eq!(
            req.headers()
                .get("anthropic-version")
                .unwrap()
                .to_str()
                .unwrap(),
            "2023-06-01"
        );
    }

    #[test]
    fn test_build_protocol_upstream_request_gcp_vertex() {
        let client = reqwest::Client::new();
        let payload = json!({"model": "claude-3-opus"});
        let headers = HeaderMap::new();
        let req = build_protocol_upstream_request(
            &client,
            "https://vertex.googleapis.com/v1/...",
            Protocol::GcpVertex,
            "ya29.access-token",
            &headers,
            Some("output-128k-2025-02-19"),
            &payload,
        )
        .build()
        .unwrap();

        assert_eq!(
            req.headers()
                .get("Authorization")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bearer ya29.access-token"
        );
        assert_eq!(
            req.headers()
                .get("anthropic-version")
                .unwrap()
                .to_str()
                .unwrap(),
            "vertex-2023-10-16"
        );
        assert_eq!(
            req.headers()
                .get("anthropic-beta")
                .unwrap()
                .to_str()
                .unwrap(),
            "output-128k-2025-02-19"
        );
    }

    // -- build_provider_debug_headers ------------------------------------------

    #[test]
    fn test_build_provider_debug_headers_openai() {
        let headers = HeaderMap::new();
        let result = build_provider_debug_headers(
            Protocol::OpenAI,
            "https://api.openai.com",
            &headers,
            None,
        );

        assert_eq!(result["authorization"], "Bearer ***");
        assert_eq!(result["url"], "https://api.openai.com");
        assert!(result.get("x-api-key").is_none());
    }

    #[test]
    fn test_build_provider_debug_headers_anthropic() {
        let headers = HeaderMap::new();
        let result = build_provider_debug_headers(
            Protocol::Anthropic,
            "https://api.anthropic.com",
            &headers,
            Some("beta-feature"),
        );

        assert_eq!(result["x-api-key"], "***");
        assert_eq!(result["anthropic-version"], "2023-06-01");
        assert_eq!(result["anthropic-beta"], "beta-feature");
        assert!(result.get("authorization").is_none());
    }

    #[test]
    fn test_build_provider_debug_headers_gcp_vertex() {
        let headers = HeaderMap::new();
        let result = build_provider_debug_headers(
            Protocol::GcpVertex,
            "https://vertex.api.com",
            &headers,
            None,
        );

        assert_eq!(result["authorization"], "Bearer ***");
        assert_eq!(result["anthropic-version"], "vertex-2023-10-16");
        assert!(result.get("anthropic-beta").is_none());
    }

    // -- build_protocol_error_body (tested via build_protocol_error_response) --

    #[tokio::test]
    async fn test_build_protocol_error_response_anthropic() {
        let response = build_protocol_error_response(
            Protocol::Anthropic,
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_error",
            "Too many requests",
            Some("claude-3-opus"),
            Some("anthropic-provider"),
            Some("key-1"),
        );

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);

        let body = body_json(response).await;
        assert_eq!(body["type"], "error");
        assert_eq!(body["error"]["type"], "rate_limit_error");
        assert_eq!(body["error"]["message"], "Too many requests");
    }

    #[tokio::test]
    async fn test_build_protocol_error_response_openai() {
        let response = build_protocol_error_response(
            Protocol::OpenAI,
            StatusCode::INTERNAL_SERVER_ERROR,
            "api_error",
            "Something went wrong",
            Some("gpt-4"),
            Some("openai"),
            None,
        );

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = body_json(response).await;
        assert_eq!(body["error"]["message"], "Something went wrong");
        assert_eq!(body["error"]["type"], "api_error");
        assert_eq!(body["error"]["code"], 500);
    }

    // -- build_json_response ---------------------------------------------------

    #[tokio::test]
    async fn test_build_json_response_with_extensions() {
        let body = json!({"result": "ok"});
        let response = build_json_response(
            StatusCode::OK,
            body,
            Some("gpt-4"),
            Some("openai"),
            Some("key-1"),
        );

        assert_eq!(response.status(), StatusCode::OK);

        // Verify extensions
        let ext = response.extensions();
        assert_eq!(ext.get::<ModelName>().unwrap().0, "gpt-4");
        assert_eq!(ext.get::<ProviderName>().unwrap().0, "openai");
        assert_eq!(ext.get::<ApiKeyName>().unwrap().0, "key-1");
    }

    #[tokio::test]
    async fn test_build_json_response_without_extensions() {
        let body = json!({"result": "ok"});
        let response = build_json_response(StatusCode::CREATED, body, None, None, None);

        assert_eq!(response.status(), StatusCode::CREATED);

        let ext = response.extensions();
        assert!(ext.get::<ModelName>().is_none());
        assert!(ext.get::<ProviderName>().is_none());
        assert!(ext.get::<ApiKeyName>().is_none());
    }

    // -- attach_response_extensions --------------------------------------------

    #[test]
    fn test_attach_response_extensions_all() {
        let mut response = Json(json!({})).into_response();
        attach_response_extensions(&mut response, Some("gpt-4"), Some("openai"), Some("key-1"));

        assert_eq!(response.extensions().get::<ModelName>().unwrap().0, "gpt-4");
        assert_eq!(
            response.extensions().get::<ProviderName>().unwrap().0,
            "openai"
        );
        assert_eq!(
            response.extensions().get::<ApiKeyName>().unwrap().0,
            "key-1"
        );
    }

    #[test]
    fn test_attach_response_extensions_partial() {
        let mut response = Json(json!({})).into_response();
        attach_response_extensions(&mut response, Some("gpt-4"), None, None);

        assert!(response.extensions().get::<ModelName>().is_some());
        assert!(response.extensions().get::<ProviderName>().is_none());
        assert!(response.extensions().get::<ApiKeyName>().is_none());
    }

    // -- build_invalid_json_error_response -------------------------------------

    #[tokio::test]
    async fn test_build_invalid_json_error_response() {
        let err = serde_json::from_str::<Value>("not json").unwrap_err();
        let (message, response) = build_invalid_json_error_response(
            Protocol::OpenAI,
            &err,
            Some("gpt-4"),
            Some("openai"),
            None,
        );

        assert_eq!(message, "Invalid JSON response from upstream provider");
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    // -- build_unexpected_status_split_response --------------------------------

    #[tokio::test]
    async fn test_build_unexpected_status_split_response() {
        let ctx = UpstreamContext {
            protocol: Protocol::OpenAI,
            model: Some("gpt-4"),
            provider: "openai",
            api_key_name: Some("key-1"),
            request_id: Some("req-123"),
        };

        let response = build_unexpected_status_split_response(&ctx, "handle_error_response");

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = body_json(response).await;
        assert!(body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unexpected upstream success"));
    }

    // -- record_token_metrics --------------------------------------------------

    #[test]
    fn test_record_token_metrics() {
        crate::core::metrics::init_metrics();
        let metrics = crate::core::metrics::get_metrics();

        let before_prompt = metrics
            .token_usage
            .with_label_values(&[
                "test-model-rtm",
                "test-provider-rtm",
                "prompt",
                "test-key-rtm",
                "test-client-rtm",
            ])
            .get();
        let before_completion = metrics
            .token_usage
            .with_label_values(&[
                "test-model-rtm",
                "test-provider-rtm",
                "completion",
                "test-key-rtm",
                "test-client-rtm",
            ])
            .get();
        let before_total = metrics
            .token_usage
            .with_label_values(&[
                "test-model-rtm",
                "test-provider-rtm",
                "total",
                "test-key-rtm",
                "test-client-rtm",
            ])
            .get();

        record_token_metrics(
            100,
            50,
            "test-model-rtm",
            "test-provider-rtm",
            "test-key-rtm",
            "test-client-rtm",
        );

        let after_prompt = metrics
            .token_usage
            .with_label_values(&[
                "test-model-rtm",
                "test-provider-rtm",
                "prompt",
                "test-key-rtm",
                "test-client-rtm",
            ])
            .get();
        let after_completion = metrics
            .token_usage
            .with_label_values(&[
                "test-model-rtm",
                "test-provider-rtm",
                "completion",
                "test-key-rtm",
                "test-client-rtm",
            ])
            .get();
        let after_total = metrics
            .token_usage
            .with_label_values(&[
                "test-model-rtm",
                "test-provider-rtm",
                "total",
                "test-key-rtm",
                "test-client-rtm",
            ])
            .get();

        assert_eq!(after_prompt - before_prompt, 100);
        assert_eq!(after_completion - before_completion, 50);
        assert_eq!(after_total - before_total, 150);
    }

    // -- classify_upstream_error -----------------------------------------------

    #[test]
    fn test_classify_upstream_error_connect_branch() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        // Use a known unreachable address to trigger a connect error
        let err = rt
            .block_on(async { client.get("http://192.0.2.1:1").send().await })
            .unwrap_err();

        let (status, error_type, message) = classify_upstream_error(&err);
        // Connect errors may also appear as timeouts depending on OS
        assert!(
            status == StatusCode::BAD_GATEWAY || status == StatusCode::GATEWAY_TIMEOUT,
            "unexpected status: {}",
            status
        );
        assert!(
            error_type == ERROR_TYPE_API || error_type == ERROR_TYPE_TIMEOUT,
            "unexpected error_type: {}",
            error_type
        );
        // Must NOT leak IP addresses
        assert!(
            !message.contains("192.0.2.1"),
            "message leaked internal IP: {}",
            message
        );
    }

    // -- normalize_upstream_status ---------------------------------------------

    #[test]
    fn test_normalize_upstream_status_valid() {
        let status = normalize_upstream_status(reqwest::StatusCode::OK);
        assert_eq!(status, StatusCode::OK);

        let status = normalize_upstream_status(reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

        let status = normalize_upstream_status(reqwest::StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    // -- UpstreamContext -------------------------------------------------------

    #[test]
    fn test_upstream_context_clone() {
        let ctx = UpstreamContext {
            protocol: Protocol::OpenAI,
            model: Some("gpt-4"),
            provider: "openai",
            api_key_name: Some("key-1"),
            request_id: Some("req-123"),
        };
        let ctx2 = ctx;
        assert_eq!(ctx2.provider, "openai");
        assert_eq!(ctx2.request_id, Some("req-123"));
    }

    // -- StatusErrorResponseMode -----------------------------------------------

    #[test]
    fn test_status_error_response_mode_debug() {
        let mode = StatusErrorResponseMode::Protocol;
        assert_eq!(format!("{:?}", mode), "Protocol");

        let mode = StatusErrorResponseMode::Passthrough;
        assert_eq!(format!("{:?}", mode), "Passthrough");
    }

    // -- extract_error_message (additional edge cases) -------------------------

    #[test]
    fn test_extract_error_message_empty_object() {
        let body = json!({});
        assert_eq!(extract_error_message(&body), None);
    }

    #[test]
    fn test_extract_error_message_error_with_non_string_message() {
        let body = json!({"error": {"message": 42}});
        assert_eq!(extract_error_message(&body), None);
    }

    #[test]
    fn test_extract_error_message_null_error() {
        let body = json!({"error": null});
        assert_eq!(extract_error_message(&body), None);
    }

    // -- truncate_message (additional edge cases) ------------------------------

    #[test]
    fn test_truncate_message_exact_boundary() {
        let exact: String = "x".repeat(MAX_ERROR_MESSAGE_LEN);
        assert_eq!(truncate_message(&exact), exact);
    }

    #[test]
    fn test_truncate_message_one_over() {
        let one_over: String = "x".repeat(MAX_ERROR_MESSAGE_LEN + 1);
        let result = truncate_message(&one_over);
        assert_eq!(result.len(), MAX_ERROR_MESSAGE_LEN + 3);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_message_empty() {
        assert_eq!(truncate_message(""), "");
    }

    #[test]
    fn test_truncate_message_unicode() {
        // Ensure truncation handles multi-byte chars correctly (by char count, not byte count)
        let unicode: String = "中".repeat(MAX_ERROR_MESSAGE_LEN + 10);
        let result = truncate_message(&unicode);
        let char_count: usize = result.chars().count();
        assert_eq!(char_count, MAX_ERROR_MESSAGE_LEN + 3); // 500 chars + "..."
    }

    // -- GCP Vertex URL tests -------------------------------------------------

    #[test]
    fn test_build_gcp_vertex_url_streaming() {
        let url = build_gcp_vertex_url(
            "https://us-east5-aiplatform.googleapis.com",
            "my-proj",
            "us-east5",
            "anthropic",
            "claude-3-5-sonnet@20241022",
            true,
        )
        .unwrap();
        assert_eq!(
            url,
            "https://us-east5-aiplatform.googleapis.com/v1/projects/my-proj/locations/us-east5/publishers/anthropic/models/claude-3-5-sonnet@20241022:streamRawPredict"
        );
    }

    #[test]
    fn test_build_gcp_vertex_url_non_streaming() {
        let url =
            build_gcp_vertex_url("https://base", "proj", "loc", "pub", "model", false).unwrap();
        assert!(url.ends_with(":rawPredict"));
    }

    #[test]
    fn test_build_gcp_vertex_url_rejects_path_traversal() {
        assert!(build_gcp_vertex_url("https://b", "..", "loc", "pub", "m", true).is_err());
        assert!(build_gcp_vertex_url("https://b", "proj", "a/b", "pub", "m", true).is_err());
        assert!(build_gcp_vertex_url("https://b", "proj", "loc", "a\\b", "m", true).is_err());
    }

    #[test]
    fn test_build_gcp_vertex_url_rejects_empty_segment() {
        assert!(build_gcp_vertex_url("https://b", "", "loc", "pub", "m", true).is_err());
        assert!(build_gcp_vertex_url("https://b", "proj", "", "pub", "m", true).is_err());
        assert!(build_gcp_vertex_url("https://b", "proj", "loc", "", "m", true).is_err());
        assert!(build_gcp_vertex_url("https://b", "proj", "loc", "pub", "", true).is_err());
    }

    #[test]
    fn test_extract_error_message_nested() {
        let body = json!({"error": {"message": "foo"}});
        assert_eq!(extract_error_message(&body), Some("foo".to_string()));
    }

    #[test]
    fn test_extract_error_message_string_error() {
        let body = json!({"error": "bar"});
        assert_eq!(extract_error_message(&body), Some("bar".to_string()));
    }

    #[test]
    fn test_extract_error_message_top_level() {
        let body = json!({"message": "baz"});
        assert_eq!(extract_error_message(&body), Some("baz".to_string()));
    }

    #[test]
    fn test_extract_error_message_missing() {
        let body = json!({"other": "value"});
        assert_eq!(extract_error_message(&body), None);
    }

    #[test]
    fn test_classify_upstream_error_sanitizes_message() {
        // Verify function signature and that timeout branch produces a sanitized message.
        // We build a real reqwest timeout error via a client with 1ms timeout.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(1))
            .build()
            .unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let err = rt
            .block_on(async { client.get("http://192.0.2.1:1").send().await })
            .unwrap_err();

        let (status, _error_type, message) = classify_upstream_error(&err);
        // Must NOT leak the raw reqwest error text
        assert!(
            message == "Upstream request timed out"
                || message == "Failed to connect to upstream provider",
            "unexpected sanitized message: {}",
            message,
        );
        assert!(status == StatusCode::GATEWAY_TIMEOUT || status == StatusCode::BAD_GATEWAY,);
    }

    #[test]
    fn test_truncate_message_short() {
        let short = "hello world";
        assert_eq!(truncate_message(short), short);
    }

    #[test]
    fn test_truncate_message_long() {
        let long: String = "x".repeat(600);
        let result = truncate_message(&long);
        assert_eq!(result.len(), MAX_ERROR_MESSAGE_LEN + 3); // 500 chars + "..."
        assert!(result.ends_with("..."));
    }
}
