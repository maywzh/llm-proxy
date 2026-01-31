//! Unified stream metrics recording.
//!
//! Single responsibility: convert StreamStats to Prometheus metrics.
//!
//! Records:
//! - TPS (tokens per second): output_tokens / first_token_time.elapsed()
//! - TTFT (time to first token): first_token_time - start_time
//! - Token Usage: input/output/total tokens

use std::time::Instant;

use crate::core::metrics::get_metrics;

/// Stream statistics - pure data structure.
#[derive(Debug, Clone)]
pub struct StreamStats {
    pub model: String,
    pub provider: String,
    pub api_key_name: String,
    pub client: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub start_time: Instant,
    pub first_token_time: Option<Instant>,
}

impl StreamStats {
    /// Create new StreamStats with required fields.
    pub fn new(
        model: impl Into<String>,
        provider: impl Into<String>,
        api_key_name: impl Into<String>,
        client: impl Into<String>,
        start_time: Instant,
    ) -> Self {
        Self {
            model: model.into(),
            provider: provider.into(),
            api_key_name: api_key_name.into(),
            client: client.into(),
            input_tokens: 0,
            output_tokens: 0,
            start_time,
            first_token_time: None,
        }
    }
}

/// Record all stream metrics in one place.
///
/// Single responsibility: convert StreamStats to Prometheus metrics.
pub fn record_stream_metrics(stats: &StreamStats) {
    let metrics = get_metrics();

    // TPS (only if we have first token and output tokens)
    if let Some(first_token) = stats.first_token_time {
        if stats.output_tokens > 0 {
            let duration = first_token.elapsed().as_secs_f64();
            if duration > 0.0 {
                let tps = stats.output_tokens as f64 / duration;
                metrics
                    .tokens_per_second
                    .with_label_values(&["provider", &stats.model, &stats.provider])
                    .observe(tps);
                tracing::debug!(
                    model = %stats.model,
                    provider = %stats.provider,
                    tokens = stats.output_tokens,
                    duration_secs = format!("{:.3}", duration),
                    tps = format!("{:.2}", tps),
                    "Stream TPS"
                );
            }
        }

        // TTFT
        if let Some(ttft_duration) = first_token.checked_duration_since(stats.start_time) {
            let ttft = ttft_duration.as_secs_f64();
            metrics
                .ttft
                .with_label_values(&["provider", &stats.model, &stats.provider])
                .observe(ttft);
            tracing::debug!(
                model = %stats.model,
                provider = %stats.provider,
                ttft_secs = format!("{:.3}", ttft),
                "Stream TTFT"
            );
        }
    }

    // Token Usage
    if stats.input_tokens > 0 || stats.output_tokens > 0 {
        metrics
            .token_usage
            .with_label_values(&[
                &stats.model,
                &stats.provider,
                "prompt",
                &stats.api_key_name,
                &stats.client,
            ])
            .inc_by(stats.input_tokens as u64);
        metrics
            .token_usage
            .with_label_values(&[
                &stats.model,
                &stats.provider,
                "completion",
                &stats.api_key_name,
                &stats.client,
            ])
            .inc_by(stats.output_tokens as u64);
        metrics
            .token_usage
            .with_label_values(&[
                &stats.model,
                &stats.provider,
                "total",
                &stats.api_key_name,
                &stats.client,
            ])
            .inc_by((stats.input_tokens + stats.output_tokens) as u64);

        tracing::debug!(
            model = %stats.model,
            provider = %stats.provider,
            input_tokens = stats.input_tokens,
            output_tokens = stats.output_tokens,
            "Stream tokens"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_stream_stats_new() {
        let start = Instant::now();
        let stats = StreamStats::new("gpt-4", "OpenAI", "test-key", "claude-code", start);

        assert_eq!(stats.model, "gpt-4");
        assert_eq!(stats.provider, "OpenAI");
        assert_eq!(stats.api_key_name, "test-key");
        assert_eq!(stats.client, "claude-code");
        assert_eq!(stats.input_tokens, 0);
        assert_eq!(stats.output_tokens, 0);
        assert!(stats.first_token_time.is_none());
    }

    #[test]
    fn test_stream_stats_with_all_fields() {
        let start = Instant::now();
        let first_token = start + Duration::from_millis(500);

        let stats = StreamStats {
            model: "gpt-4".to_string(),
            provider: "OpenAI".to_string(),
            api_key_name: "test-key".to_string(),
            client: "claude-code".to_string(),
            input_tokens: 100,
            output_tokens: 200,
            start_time: start,
            first_token_time: Some(first_token),
        };

        assert_eq!(stats.input_tokens, 100);
        assert_eq!(stats.output_tokens, 200);
        assert!(stats.first_token_time.is_some());
    }

    #[test]
    fn test_record_stream_metrics_with_valid_stats() {
        // Initialize metrics if not already initialized
        let _ = crate::core::metrics::init_metrics();

        let start = Instant::now() - Duration::from_secs(2);
        let first_token = Instant::now() - Duration::from_secs(1);

        let stats = StreamStats {
            model: "test-model-rust".to_string(),
            provider: "test-provider-rust".to_string(),
            api_key_name: "test-key-rust".to_string(),
            client: "claude-code".to_string(),
            input_tokens: 50,
            output_tokens: 100,
            start_time: start,
            first_token_time: Some(first_token),
        };

        // Should not panic
        record_stream_metrics(&stats);
    }

    #[test]
    fn test_record_stream_metrics_with_no_first_token() {
        let _ = crate::core::metrics::init_metrics();

        let stats = StreamStats {
            model: "test-model-no-token".to_string(),
            provider: "test-provider".to_string(),
            api_key_name: "test-key".to_string(),
            client: "unknown".to_string(),
            input_tokens: 50,
            output_tokens: 100,
            start_time: Instant::now(),
            first_token_time: None,
        };

        // Should not panic, TPS and TTFT should be skipped
        record_stream_metrics(&stats);
    }

    #[test]
    fn test_record_stream_metrics_with_zero_tokens() {
        let _ = crate::core::metrics::init_metrics();

        let stats = StreamStats {
            model: "test-model-zero".to_string(),
            provider: "test-provider".to_string(),
            api_key_name: "test-key".to_string(),
            client: "unknown".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            start_time: Instant::now(),
            first_token_time: None,
        };

        // Should not panic, token usage should be skipped
        record_stream_metrics(&stats);
    }

    #[test]
    fn test_record_stream_metrics_with_zero_output_tokens() {
        let _ = crate::core::metrics::init_metrics();

        let start = Instant::now() - Duration::from_secs(1);
        let first_token = Instant::now() - Duration::from_millis(500);

        let stats = StreamStats {
            model: "test-model-no-output".to_string(),
            provider: "test-provider".to_string(),
            api_key_name: "test-key".to_string(),
            client: "unknown".to_string(),
            input_tokens: 50,
            output_tokens: 0,
            start_time: start,
            first_token_time: Some(first_token),
        };

        // Should not panic, TPS should be skipped (no output tokens)
        record_stream_metrics(&stats);
    }
}
