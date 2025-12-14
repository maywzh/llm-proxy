//! HTTP middleware for request tracking and metrics.
//!
//! This module provides middleware for tracking request metrics including
//! duration, active requests, and status codes.

use crate::core::metrics::get_metrics;
use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use std::time::Instant;

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
    /// * `state` - Application state (unused but required by Axum)
    /// * `request` - Incoming HTTP request
    /// * `next` - Next middleware/handler in the chain
    pub async fn track_metrics<S>(
        State(_state): State<Arc<S>>,
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
        metrics.active_requests.with_label_values(&[&endpoint]).inc();

        let start = Instant::now();

        // Process request
        let response = next.run(request).await;

        let duration = start.elapsed().as_secs_f64();
        let status_code = response.status().as_u16().to_string();

        // Get model and provider from response extensions (set by handlers)
        let model = response
            .extensions()
            .get::<String>()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());
        let provider = response
            .extensions()
            .get::<String>()
            .cloned()
            .unwrap_or_else(|| "unknown".to_string());

        // Record metrics
        metrics
            .request_count
            .with_label_values(&[&method, &endpoint, &model, &provider, &status_code])
            .inc();

        metrics
            .request_duration
            .with_label_values(&[&method, &endpoint, &model, &provider])
            .observe(duration);

        // Log request
        tracing::info!(
            "{} {} - model={} provider={} status={} duration={:.3}s",
            method,
            endpoint,
            model,
            provider,
            status_code,
            duration
        );

        // Decrement active requests
        metrics.active_requests.with_label_values(&[&endpoint]).dec();

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
        routing::get,
        Router,
    };
    use tower::ServiceExt;

    #[test]
    fn test_metrics_middleware_initialization() {
        // Initialize metrics for testing
        init_metrics();
        
        // Verify metrics are accessible
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
            .layer(middleware::from_fn_with_state(
                Arc::new(()),
                MetricsMiddleware::track_metrics::<()>,
            ));

        let request = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();

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
            .layer(middleware::from_fn_with_state(
                Arc::new(()),
                MetricsMiddleware::track_metrics::<()>,
            ));

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
        
        // Get initial count
        let initial = metrics.active_requests.with_label_values(&["/test"]).get();
        
        // Use Arc<Mutex> to share state between handler and test
        let in_handler = Arc::new(tokio::sync::Mutex::new(false));
        let in_handler_clone = in_handler.clone();
        
        async fn slow_handler(flag: Arc<tokio::sync::Mutex<bool>>) -> &'static str {
            // Signal that we're in the handler
            *flag.lock().await = true;
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            "ok"
        }

        let app = Router::new()
            .route("/test", get(move || slow_handler(in_handler_clone)))
            .layer(middleware::from_fn_with_state(
                Arc::new(()),
                MetricsMiddleware::track_metrics::<()>,
            ));

        let request = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();

        // Spawn the request in a separate task
        let handle = tokio::spawn(async move {
            app.oneshot(request).await.unwrap()
        });
        
        // Wait for handler to start
        while !*in_handler.lock().await {
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
        
        // Check that active requests was incremented
        let during = metrics.active_requests.with_label_values(&["/test"]).get();
        assert!(during > initial, "Active requests should be incremented during execution");
        
        // Wait for request to complete
        let _response = handle.await.unwrap();
        
        // After request completes, active requests should be back to initial
        let final_count = metrics.active_requests.with_label_values(&["/test"]).get();
        assert_eq!(final_count, initial);
    }

    #[tokio::test]
    async fn test_middleware_records_duration() {
        init_metrics();
        let metrics = get_metrics();
        
        async fn handler() -> &'static str {
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            "ok"
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn_with_state(
                Arc::new(()),
                MetricsMiddleware::track_metrics::<()>,
            ));

        let request = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();

        let _response = app.oneshot(request).await.unwrap();
        
        // Verify duration was recorded
        let metric = metrics
            .request_duration
            .with_label_values(&["GET", "/test", "unknown", "unknown"]);
        
        assert!(metric.get_sample_count() > 0);
    }

    #[tokio::test]
    async fn test_middleware_records_status_code() {
        init_metrics();
        let metrics = get_metrics();
        
        async fn handler() -> StatusCode {
            StatusCode::NOT_FOUND
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn_with_state(
                Arc::new(()),
                MetricsMiddleware::track_metrics::<()>,
            ));

        let request = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        
        // Verify request count was incremented
        let count = metrics
            .request_count
            .with_label_values(&["GET", "/test", "unknown", "unknown", "404"])
            .get();
        
        assert!(count > 0);
    }
}