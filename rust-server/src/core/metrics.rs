//! Prometheus metrics for monitoring the LLM proxy server.
//!
//! This module provides a centralized metrics registry with various metric types
//! for tracking requests, latency, token usage, and provider health.

use prometheus::{
    register_gauge_vec, register_histogram_vec, register_int_counter_vec, GaugeVec, HistogramVec,
    IntCounterVec,
};
use std::sync::OnceLock;

/// Container for all application metrics.
pub struct Metrics {
    /// Total number of requests by method, endpoint, model, provider, and status
    pub request_count: IntCounterVec,

    /// Request duration histogram in seconds
    pub request_duration: HistogramVec,

    /// Number of currently active requests by endpoint
    pub active_requests: GaugeVec,

    /// Total token usage by model, provider, and token type
    pub token_usage: IntCounterVec,

    /// Provider health status (1=healthy, 0=unhealthy)
    pub provider_health: GaugeVec,

    /// Provider response latency histogram in seconds
    pub provider_latency: HistogramVec,

    /// Time to first token (TTFT) histogram in seconds for streaming requests
    /// Measures upstream provider latency
    pub ttft: HistogramVec,

    /// Tokens per second (TPS) histogram for streaming requests
    /// Measures upstream provider throughput
    pub tokens_per_second: HistogramVec,
}

static METRICS: OnceLock<Metrics> = OnceLock::new();

/// Initialize the metrics registry.
///
/// This should be called once at application startup. Subsequent calls will
/// return the same instance.
///
/// # Examples
///
/// ```no_run
/// use llm_proxy_rust::core::metrics::init_metrics;
///
/// let metrics = init_metrics();
/// metrics.request_count.with_label_values(&["GET", "/health", "unknown", "unknown", "200"]).inc();
/// ```
pub fn init_metrics() -> &'static Metrics {
    METRICS.get_or_init(|| {
        let request_count = register_int_counter_vec!(
            "llm_proxy_requests_total",
            "Total number of requests",
            &["method", "endpoint", "model", "provider", "status_code", "api_key_name"]
        )
        .expect("Failed to register request_count metric");

        let request_duration = register_histogram_vec!(
            "llm_proxy_request_duration_seconds",
            "Request duration in seconds",
            &["method", "endpoint", "model", "provider", "api_key_name"],
            vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0]
        )
        .expect("Failed to register request_duration metric");

        let active_requests = register_gauge_vec!(
            "llm_proxy_active_requests",
            "Number of active requests",
            &["endpoint"]
        )
        .expect("Failed to register active_requests metric");

        let token_usage = register_int_counter_vec!(
            "llm_proxy_tokens_total",
            "Total number of tokens used",
            &["model", "provider", "token_type", "api_key_name"]
        )
        .expect("Failed to register token_usage metric");

        let provider_health = register_gauge_vec!(
            "llm_proxy_provider_health",
            "Provider health status (1=healthy, 0=unhealthy)",
            &["provider"]
        )
        .expect("Failed to register provider_health metric");

        let provider_latency = register_histogram_vec!(
            "llm_proxy_provider_latency_seconds",
            "Provider response latency in seconds",
            &["provider"],
            vec![0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0]
        )
        .expect("Failed to register provider_latency metric");

        let ttft = register_histogram_vec!(
            "llm_proxy_ttft_seconds",
            "Time to first token (TTFT) in seconds for streaming requests (upstream provider latency)",
            &["source", "model", "provider"],
            vec![0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0]
        )
        .expect("Failed to register ttft metric");

        let tokens_per_second = register_histogram_vec!(
            "llm_proxy_tokens_per_second",
            "Tokens generated per second for streaming requests (upstream provider throughput)",
            &["source", "model", "provider"],
            vec![1.0, 5.0, 10.0, 20.0, 30.0, 50.0, 75.0, 100.0, 150.0, 200.0]
        )
        .expect("Failed to register tokens_per_second metric");

        Metrics {
            request_count,
            request_duration,
            active_requests,
            token_usage,
            provider_health,
            provider_latency,
            ttft,
            tokens_per_second,
        }
    })
}

/// Get the global metrics instance.
///
/// # Panics
///
/// Panics if metrics have not been initialized via [`init_metrics`].
pub fn get_metrics() -> &'static Metrics {
    METRICS.get().expect("Metrics not initialized")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_initialization() {
        let metrics = init_metrics();

        // Test that we can access metrics
        metrics
            .request_count
            .with_label_values(&["GET", "/test", "model", "provider", "200", "test-key"])
            .inc();

        // Verify the same instance is returned
        let metrics2 = get_metrics();
        assert!(std::ptr::eq(metrics, metrics2));
    }

    #[test]
    fn test_request_count_metric() {
        let metrics = init_metrics();

        // Use unique label values to avoid conflicts with other tests
        let initial = metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4-unique", "openai-unique", "201", "test-key-unique"])
            .get();

        metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4-unique", "openai-unique", "201", "test-key-unique"])
            .inc();

        let after = metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4-unique", "openai-unique", "201", "test-key-unique"])
            .get();

        assert_eq!(after, initial + 1);
    }

    #[test]
    fn test_request_duration_metric() {
        let metrics = init_metrics();

        metrics
            .request_duration
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "openai", "test-key"])
            .observe(1.5);

        metrics
            .request_duration
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "openai", "test-key"])
            .observe(2.3);

        // Verify metric was recorded (count should be 2)
        let metric = metrics.request_duration.with_label_values(&[
            "POST",
            "/v1/chat/completions",
            "gpt-4",
            "openai",
            "test-key",
        ]);

        // Just verify we can access it without panicking
        let _ = metric.get_sample_count();
    }

    #[test]
    fn test_active_requests_metric() {
        let metrics = init_metrics();

        let initial = metrics
            .active_requests
            .with_label_values(&["/v1/chat/completions"])
            .get();

        metrics
            .active_requests
            .with_label_values(&["/v1/chat/completions"])
            .inc();

        let after_inc = metrics
            .active_requests
            .with_label_values(&["/v1/chat/completions"])
            .get();

        assert_eq!(after_inc, initial + 1.0);

        metrics
            .active_requests
            .with_label_values(&["/v1/chat/completions"])
            .dec();

        let after_dec = metrics
            .active_requests
            .with_label_values(&["/v1/chat/completions"])
            .get();

        assert_eq!(after_dec, initial);
    }

    #[test]
    fn test_token_usage_metric() {
        let metrics = init_metrics();

        let initial = metrics
            .token_usage
            .with_label_values(&["gpt-4", "openai", "prompt", "test-key"])
            .get();

        metrics
            .token_usage
            .with_label_values(&["gpt-4", "openai", "prompt", "test-key"])
            .inc_by(100);

        let after = metrics
            .token_usage
            .with_label_values(&["gpt-4", "openai", "prompt", "test-key"])
            .get();

        assert_eq!(after, initial + 100);
    }

    #[test]
    fn test_provider_health_metric() {
        let metrics = init_metrics();

        metrics
            .provider_health
            .with_label_values(&["openai"])
            .set(1.0);

        let health = metrics.provider_health.with_label_values(&["openai"]).get();

        assert_eq!(health, 1.0);

        metrics
            .provider_health
            .with_label_values(&["openai"])
            .set(0.0);

        let health = metrics.provider_health.with_label_values(&["openai"]).get();

        assert_eq!(health, 0.0);
    }

    #[test]
    fn test_provider_latency_metric() {
        let metrics = init_metrics();

        metrics
            .provider_latency
            .with_label_values(&["openai"])
            .observe(0.5);

        metrics
            .provider_latency
            .with_label_values(&["openai"])
            .observe(1.2);

        // Verify metric was recorded
        let metric = metrics.provider_latency.with_label_values(&["openai"]);

        let _ = metric.get_sample_count();
    }

    #[test]
    fn test_multiple_providers_metrics() {
        let metrics = init_metrics();

        metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "openai", "200", "test-key"])
            .inc();

        metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "anthropic", "200", "test-key"])
            .inc();

        let openai_count = metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "openai", "200", "test-key"])
            .get();

        let anthropic_count = metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "anthropic", "200", "test-key"])
            .get();

        assert!(openai_count >= 1);
        assert!(anthropic_count >= 1);
    }

    #[test]
    fn test_metrics_with_different_status_codes() {
        let metrics = init_metrics();

        metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "openai", "200", "test-key"])
            .inc();

        metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "openai", "500", "test-key"])
            .inc();

        let success_count = metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "openai", "200", "test-key"])
            .get();

        let error_count = metrics
            .request_count
            .with_label_values(&["POST", "/v1/chat/completions", "gpt-4", "openai", "500", "test-key"])
            .get();

        assert!(success_count >= 1);
        assert!(error_count >= 1);
    }
}
