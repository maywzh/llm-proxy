//! HTTP middleware for request tracking, metrics, and model permission checking.
//!
//! This module provides middleware for tracking request metrics including
//! duration, active requests, and status codes. It also provides model
//! permission checking middleware.

use crate::api::auth::{check_model_permission, hash_key};
use crate::core::config::CredentialConfig;
use crate::core::metrics::get_metrics;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

/// Paths that require model permission checking
const MODEL_CHECK_PATHS: &[&str] = &[
    "/v1/chat/completions",
    "/v1/completions",
    "/v1/messages",
    "/v1/messages/count_tokens",
    "/v2/chat/completions",
    "/v2/messages",
    "/v2/responses",
];

/// Extension type for storing model name in response
#[derive(Clone, Debug)]
pub struct ModelName(pub String);

/// Extension type for storing provider name in response
#[derive(Clone, Debug)]
pub struct ProviderName(pub String);

/// Extension type for storing API key name in response
#[derive(Clone, Debug)]
pub struct ApiKeyName(pub String);

/// Known client patterns for User-Agent mapping
/// Each tuple: (pattern to match in UA, normalized client name)
/// Order matters - more specific patterns should come first
const CLIENT_PATTERNS: &[(&str, &str)] = &[
    // Claude CLI / Claude Code (claude-cli/2.1.25, claude-vscode, etc.)
    ("claude-cli", "claude-code"),
    ("claude-code", "claude-code"),
    ("claude-proxy", "claude-proxy"),
    // Kilo-Code (VSCode extension)
    ("Kilo-Code", "kilo-code"),
    // Codex CLI (codex_cli_rs, codex_vscode)
    ("codex_cli_rs", "codex-cli"),
    ("codex_vscode", "codex-vscode"),
    ("codex", "codex"),
    ("Codex", "codex"),
    // AI SDK (ai-sdk/openai-compatible, ai-sdk/anthropic, etc.)
    ("ai-sdk/openai-compatible", "ai-sdk-openai"),
    ("ai-sdk/anthropic", "ai-sdk-anthropic"),
    ("ai-sdk", "ai-sdk"),
    ("ai/", "ai-sdk"),
    // OpenAI SDK (OpenAI/JS, openai-python, etc.)
    ("OpenAI/JS", "openai-js"),
    ("openai-python", "openai-python"),
    ("OpenAI-Python", "openai-python"),
    ("openai-node", "openai-node"),
    ("OpenAI-Node", "openai-node"),
    ("openai/", "openai-sdk"),
    ("OpenAI/", "openai-sdk"),
    // Anthropic SDK
    ("anthropic-sdk", "anthropic-sdk"),
    ("Anthropic", "anthropic-sdk"),
    // Other AI coding assistants
    ("opencode", "opencode"),
    ("OpenCode", "opencode"),
    ("cursor", "cursor"),
    ("Cursor", "cursor"),
    ("copilot", "copilot"),
    ("Copilot", "copilot"),
    ("continue", "continue"),
    ("Continue", "continue"),
    ("aider", "aider"),
    ("Aider", "aider"),
    ("cline", "cline"),
    ("Cline", "cline"),
    // API testing tools
    ("Apifox", "apifox"),
    ("PostmanRuntime", "postman"),
    ("insomnia", "insomnia"),
    // Common HTTP clients
    ("python-httpx", "python-httpx"),
    ("python-requests", "python-requests"),
    ("httpx", "httpx"),
    ("axios", "axios"),
    ("node-fetch", "node-fetch"),
    ("curl", "curl"),
    ("wget", "wget"),
    // Terminal apps
    ("iTerm2", "iterm2"),
    // Browsers (low priority - usually not direct API calls)
    ("Mozilla", "browser"),
    // LangChain / LlamaIndex
    ("langchain", "langchain"),
    ("LangChain", "langchain"),
    ("llama-index", "llama-index"),
    ("LlamaIndex", "llama-index"),
];

/// Extract normalized client name from User-Agent header
pub fn extract_client(headers: &HeaderMap) -> String {
    let raw = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if raw.is_empty() {
        return "unknown".to_string();
    }

    // Try to match known client patterns
    for (pattern, client_name) in CLIENT_PATTERNS {
        if raw.contains(pattern) {
            return client_name.to_string();
        }
    }

    // Fallback: extract first token (before space or slash) and truncate to 30 chars
    let first_token = raw
        .split(|c: char| c == ' ' || c == '/')
        .next()
        .unwrap_or(raw);

    let cleaned: String = first_token
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .take(30)
        .collect();

    if cleaned.is_empty() {
        "other".to_string()
    } else {
        cleaned
    }
}

/// Middleware for logging admin API requests.
///
/// This middleware logs all requests to /admin/v1/* endpoints with:
/// - HTTP method
/// - Request path
/// - Response status code
/// - Request duration
pub async fn admin_logging_middleware(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let start = Instant::now();

    let response = next.run(request).await;

    let duration = start.elapsed().as_secs_f64();
    let status = response.status().as_u16();

    tracing::info!(
        "[Admin API] {} {} - status={} duration={:.3}s",
        method,
        path,
        status,
        duration
    );

    response
}

// ============================================================================
// Model Permission Middleware
// ============================================================================

/// Trait for states that can provide credentials for model permission checking
pub trait HasCredentials: Clone + Send + Sync + 'static {
    fn get_credentials(&self) -> Vec<CredentialConfig>;
}

/// Check if a path requires model permission checking
fn requires_model_check(path: &str) -> bool {
    MODEL_CHECK_PATHS.contains(&path)
}

/// Extract API key from headers (supports both Bearer and x-api-key)
fn extract_api_key_from_headers(headers: &HeaderMap) -> Option<String> {
    // x-api-key takes priority
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| {
            headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "))
                .map(|s| s.to_string())
        })
}

/// Find credential config by API key
fn find_credential_by_key(
    api_key: &str,
    credentials: &[CredentialConfig],
) -> Option<CredentialConfig> {
    let key_hash = hash_key(api_key);
    credentials
        .iter()
        .find(|c| c.enabled && c.credential_key == key_hash)
        .cloned()
}

/// Model permission middleware for AppState
pub async fn model_permission_middleware<S: HasCredentials>(
    State(state): State<Arc<S>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    // Skip if not in whitelist or not POST
    if !requires_model_check(&path) || method != axum::http::Method::POST {
        return next.run(request).await;
    }

    // Extract headers before consuming request
    let headers = request.headers().clone();

    // Read body using axum's Bytes extractor approach
    let (parts, body) = request.into_parts();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(_) => {
            // Can't read body, let handler deal with it
            let request = Request::from_parts(parts, Body::empty());
            return next.run(request).await;
        }
    };

    // Parse JSON to extract model
    let model = match serde_json::from_slice::<Value>(&bytes) {
        Ok(json) => json
            .get("model")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string()),
        Err(_) => {
            // Invalid JSON, let handler deal with it
            let request = Request::from_parts(parts, Body::from(bytes));
            return next.run(request).await;
        }
    };

    // If no model in request, proceed
    let Some(model) = model else {
        let request = Request::from_parts(parts, Body::from(bytes));
        return next.run(request).await;
    };

    // Get credential from headers
    let credentials = state.get_credentials();

    // If no credentials configured, proceed
    if credentials.is_empty() {
        let request = Request::from_parts(parts, Body::from(bytes));
        return next.run(request).await;
    }

    // Get API key from headers
    let api_key = match extract_api_key_from_headers(&headers) {
        Some(key) => key,
        None => {
            // No API key, auth middleware will handle it
            let request = Request::from_parts(parts, Body::from(bytes));
            return next.run(request).await;
        }
    };

    // Find credential config
    let credential_config = find_credential_by_key(&api_key, &credentials);

    // Check model permission
    if let Err(e) = check_model_permission(Some(&model), &credential_config) {
        tracing::warn!(
            model = %model,
            credential_name = credential_config.as_ref().map(|c| c.name.as_str()).unwrap_or("unknown"),
            "Model permission denied in middleware"
        );

        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": {
                    "message": e.to_string(),
                    "type": "permission_error",
                    "code": "model_not_allowed"
                }
            })),
        )
            .into_response();
    }

    // Rebuild request with body and proceed
    let request = Request::from_parts(parts, Body::from(bytes));
    next.run(request).await
}

/// Middleware for tracking request metrics.
pub struct MetricsMiddleware;

impl MetricsMiddleware {
    /// Track metrics for incoming requests.
    ///
    /// This middleware:
    /// - Increments active request counter
    /// - Measures request duration
    /// - Records request count by status code
    /// - Logs request details
    ///
    /// # Arguments
    ///
    /// * `request` - Incoming HTTP request
    /// * `next` - Next middleware/handler in the chain
    pub async fn track_metrics(request: Request, next: Next) -> Response {
        let endpoint = request.uri().path().to_string();
        let method = request.method().to_string();
        let client = extract_client(request.headers());

        // Skip metrics endpoint itself to avoid recursion
        if endpoint == "/metrics" {
            return next.run(request).await;
        }

        let metrics = get_metrics();

        // Increment active requests
        metrics
            .active_requests
            .with_label_values(&[&endpoint])
            .inc();

        let start = Instant::now();

        // Process request
        let response = next.run(request).await;

        let duration = start.elapsed().as_secs_f64();
        let status_code = response.status().as_u16().to_string();

        // Get model, provider, and api_key_name from response extensions (set by handlers)
        let model = response
            .extensions()
            .get::<ModelName>()
            .map(|m| m.0.as_str())
            .unwrap_or("unknown");
        let provider = response
            .extensions()
            .get::<ProviderName>()
            .map(|p| p.0.as_str())
            .unwrap_or("unknown");
        let api_key_name = response
            .extensions()
            .get::<ApiKeyName>()
            .map(|k| k.0.as_str())
            .unwrap_or("anonymous");

        // Record metrics only for LLM requests (where provider is set)
        // Skip non-LLM endpoints like /api/event_logging, /debug/pprof, /v1/models, etc.
        if provider != "unknown" {
            metrics
                .request_count
                .with_label_values(&[
                    &method,
                    &endpoint,
                    model,
                    provider,
                    &status_code,
                    api_key_name,
                    client.as_str(),
                ])
                .inc();

            metrics
                .request_duration
                .with_label_values(&[
                    &method,
                    &endpoint,
                    model,
                    provider,
                    api_key_name,
                    client.as_str(),
                ])
                .observe(duration);
        }

        // Check if this is a streaming response (SSE)
        let is_streaming = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| ct.contains("text/event-stream"))
            .unwrap_or(false);

        // Log request - show model, provider, and key for LLM endpoints
        let is_llm_endpoint = endpoint == "/v1/chat/completions"
            || endpoint == "/v1/messages"
            || endpoint == "/v1/completions"
            || endpoint == "/v2/chat/completions"
            || endpoint == "/v2/messages"
            || endpoint == "/v2/completions"
            || endpoint == "/v2/responses";
        if is_llm_endpoint {
            // For streaming responses, duration is actually TTFB (time to first byte)
            // since next.run() returns when headers are ready, not when body is complete
            if is_streaming {
                tracing::info!(
                    "{} {} - status={} client={} key={} model={} provider={} ttfb={:.3}s",
                    method,
                    endpoint,
                    status_code,
                    client,
                    api_key_name,
                    model,
                    provider,
                    duration
                );
            } else {
                tracing::info!(
                    "{} {} - status={} client={} key={} model={} provider={} duration={:.3}s",
                    method,
                    endpoint,
                    status_code,
                    client,
                    api_key_name,
                    model,
                    provider,
                    duration
                );
            }
        } else {
            tracing::info!(
                "{} {} - status={} duration={:.3}s",
                method,
                endpoint,
                status_code,
                duration
            );
        }

        // Decrement active requests
        metrics
            .active_requests
            .with_label_values(&[&endpoint])
            .dec();

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::metrics::init_metrics;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware,
        response::Response,
        routing::get,
        Router,
    };
    use std::sync::Arc;
    use tower::ServiceExt;

    #[test]
    fn test_metrics_middleware_initialization() {
        init_metrics();

        let metrics = get_metrics();
        assert!(metrics.active_requests.with_label_values(&["/test"]).get() >= 0.0);
    }

    #[tokio::test]
    async fn test_middleware_tracks_request() {
        init_metrics();

        async fn handler() -> &'static str {
            "ok"
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(MetricsMiddleware::track_metrics));

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_skips_metrics_endpoint() {
        init_metrics();

        async fn handler() -> &'static str {
            "metrics"
        }

        let app = Router::new()
            .route("/metrics", get(handler))
            .layer(middleware::from_fn(MetricsMiddleware::track_metrics));

        let request = Request::builder()
            .uri("/metrics")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_middleware_increments_active_requests() {
        init_metrics();
        let metrics = get_metrics();

        let endpoint = "/test-active-requests";
        let initial = metrics.active_requests.with_label_values(&[endpoint]).get();

        let in_handler = Arc::new(tokio::sync::Mutex::new(false));
        let in_handler_clone = in_handler.clone();

        async fn slow_handler(flag: Arc<tokio::sync::Mutex<bool>>) -> &'static str {
            *flag.lock().await = true;
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            "ok"
        }

        let app = Router::new()
            .route(endpoint, get(move || slow_handler(in_handler_clone)))
            .layer(middleware::from_fn(MetricsMiddleware::track_metrics));

        let request = Request::builder()
            .uri(endpoint)
            .body(Body::empty())
            .unwrap();

        let handle = tokio::spawn(async move { app.oneshot(request).await.unwrap() });

        while !*in_handler.lock().await {
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }

        let during = metrics.active_requests.with_label_values(&[endpoint]).get();
        assert!(
            during > initial,
            "Active requests should be incremented during execution"
        );

        let _response = handle.await.unwrap();

        let final_count = metrics.active_requests.with_label_values(&[endpoint]).get();
        assert_eq!(final_count, initial);
    }

    #[tokio::test]
    async fn test_middleware_records_duration_for_llm_requests() {
        init_metrics();
        let metrics = get_metrics();

        async fn handler() -> Response<Body> {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            let mut response = Response::new(Body::from("ok"));
            response
                .extensions_mut()
                .insert(ModelName("gpt-4".to_string()));
            response
                .extensions_mut()
                .insert(ProviderName("openai".to_string()));
            response
                .extensions_mut()
                .insert(ApiKeyName("test-key".to_string()));
            response
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(MetricsMiddleware::track_metrics));

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let _response = app.oneshot(request).await.unwrap();

        let metric = metrics
            .request_duration
            .with_label_values(&["GET", "/test", "gpt-4", "openai", "test-key", "unknown"]);

        assert!(metric.get_sample_count() > 0);
    }

    #[tokio::test]
    async fn test_middleware_skips_metrics_for_non_llm_requests() {
        init_metrics();
        let metrics = get_metrics();

        async fn handler() -> &'static str {
            "ok"
        }

        let app = Router::new()
            .route("/test-non-llm", get(handler))
            .layer(middleware::from_fn(MetricsMiddleware::track_metrics));

        let request = Request::builder()
            .uri("/test-non-llm")
            .body(Body::empty())
            .unwrap();

        let _response = app.oneshot(request).await.unwrap();

        // Metrics with "unknown" provider should not be recorded
        let metric = metrics.request_duration.with_label_values(&[
            "GET",
            "/test-non-llm",
            "unknown",
            "unknown",
            "anonymous",
            "unknown",
        ]);

        assert_eq!(metric.get_sample_count(), 0);
    }

    #[tokio::test]
    async fn test_admin_logging_middleware() {
        async fn handler() -> &'static str {
            "admin response"
        }

        let app = Router::new()
            .route("/admin/v1/providers", get(handler))
            .layer(middleware::from_fn(admin_logging_middleware));

        let request = Request::builder()
            .uri("/admin/v1/providers")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // Tests for extract_client function
    #[test]
    fn test_extract_client_empty_ua() {
        let headers = HeaderMap::new();
        assert_eq!(extract_client(&headers), "unknown");
    }

    #[test]
    fn test_extract_client_claude_code() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "user-agent",
            "claude-cli/2.1.25 (external, claude-vscode)"
                .parse()
                .unwrap(),
        );
        assert_eq!(extract_client(&headers), "claude-code");
    }

    #[test]
    fn test_extract_client_kilo_code() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", "Kilo-Code/5.2.2".parse().unwrap());
        assert_eq!(extract_client(&headers), "kilo-code");
    }

    #[test]
    fn test_extract_client_codex_cli() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "user-agent",
            "codex_cli_rs/0.89.0 (Mac OS 26.2.0; arm64)"
                .parse()
                .unwrap(),
        );
        assert_eq!(extract_client(&headers), "codex-cli");
    }

    #[test]
    fn test_extract_client_ai_sdk_openai() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "user-agent",
            "ai-sdk/openai-compatible/1.0.31 ai-sdk/provider-ut"
                .parse()
                .unwrap(),
        );
        assert_eq!(extract_client(&headers), "ai-sdk-openai");
    }

    #[test]
    fn test_extract_client_openai_js() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", "OpenAI/JS 6.16.0".parse().unwrap());
        assert_eq!(extract_client(&headers), "openai-js");
    }

    #[test]
    fn test_extract_client_python_httpx() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", "python-httpx/0.28.1".parse().unwrap());
        assert_eq!(extract_client(&headers), "python-httpx");
    }

    #[test]
    fn test_extract_client_curl() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", "curl/8.7.1".parse().unwrap());
        assert_eq!(extract_client(&headers), "curl");
    }

    #[test]
    fn test_extract_client_browser() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "user-agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"
                .parse()
                .unwrap(),
        );
        assert_eq!(extract_client(&headers), "browser");
    }

    #[test]
    fn test_extract_client_unknown_returns_first_token() {
        let mut headers = HeaderMap::new();
        headers.insert("user-agent", "MyCustomClient/1.0.0".parse().unwrap());
        assert_eq!(extract_client(&headers), "MyCustomClient");
    }

    #[test]
    fn test_extract_client_apifox() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "user-agent",
            "Apifox/1.0.0 (https://apifox.com)".parse().unwrap(),
        );
        assert_eq!(extract_client(&headers), "apifox");
    }
}
