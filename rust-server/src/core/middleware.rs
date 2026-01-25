//! HTTP middleware for request tracking and metrics.
//!
//! This module provides middleware for tracking request metrics including
//! duration, active requests, and status codes.

use crate::core::metrics::get_metrics;
use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::time::Instant;

/// Extension type for storing model name in response
#[derive(Clone, Debug)]
pub struct ModelName(pub String);

/// Extension type for storing provider name in response
#[derive(Clone, Debug)]
pub struct ProviderName(pub String);

/// Extension type for storing API key name in response
#[derive(Clone, Debug)]
pub struct ApiKeyName(pub String);

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
    pub async fn track_metrics(
        request: Request,
        next: Next,
    ) -> Response {
        let endpoint = request.uri().path().to_string();
        let method = request.method().to_string();

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
                .with_label_values(&[&method, &endpoint, model, provider, &status_code, api_key_name])
                .inc();

            metrics
                .request_duration
                .with_label_values(&[&method, &endpoint, model, provider, api_key_name])
                .observe(duration);
        }

        // Log request - show model, provider, and key for LLM endpoints
        if endpoint == "/v1/chat/completions" || endpoint == "/v1/messages" {
            tracing::info!(
                "{} {} - model={} provider={} key={} status={} duration={:.3}s",
                method,
                endpoint,
                model,
                provider,
                api_key_name,
                status_code,
                duration
            );
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

        let initial = metrics.active_requests.with_label_values(&["/test"]).get();

        let in_handler = Arc::new(tokio::sync::Mutex::new(false));
        let in_handler_clone = in_handler.clone();

        async fn slow_handler(flag: Arc<tokio::sync::Mutex<bool>>) -> &'static str {
            *flag.lock().await = true;
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            "ok"
        }

        let app = Router::new()
            .route("/test", get(move || slow_handler(in_handler_clone)))
            .layer(middleware::from_fn(MetricsMiddleware::track_metrics));

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let handle = tokio::spawn(async move { app.oneshot(request).await.unwrap() });

        while !*in_handler.lock().await {
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }

        let during = metrics.active_requests.with_label_values(&["/test"]).get();
        assert!(
            during > initial,
            "Active requests should be incremented during execution"
        );

        let _response = handle.await.unwrap();

        let final_count = metrics.active_requests.with_label_values(&["/test"]).get();
        assert_eq!(final_count, initial);
    }

    #[tokio::test]
    async fn test_middleware_records_duration_for_llm_requests() {
        init_metrics();
        let metrics = get_metrics();

        async fn handler() -> Response<Body> {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            let mut response = Response::new(Body::from("ok"));
            response.extensions_mut().insert(ModelName("gpt-4".to_string()));
            response.extensions_mut().insert(ProviderName("openai".to_string()));
            response.extensions_mut().insert(ApiKeyName("test-key".to_string()));
            response
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(MetricsMiddleware::track_metrics));

        let request = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let _response = app.oneshot(request).await.unwrap();

        let metric = metrics
            .request_duration
            .with_label_values(&["GET", "/test", "gpt-4", "openai", "test-key"]);

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

        let request = Request::builder().uri("/test-non-llm").body(Body::empty()).unwrap();

        let _response = app.oneshot(request).await.unwrap();

        // Metrics with "unknown" provider should not be recorded
        let metric = metrics
            .request_duration
            .with_label_values(&["GET", "/test-non-llm", "unknown", "unknown", "anonymous"]);

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
}
