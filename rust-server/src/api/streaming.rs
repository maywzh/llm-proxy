//! Server-Sent Events (SSE) streaming support for chat completions.
//!
//! This module handles streaming responses from LLM providers, including
//! model name rewriting and token usage tracking.

use crate::api::models::{StreamChunk, Usage};
use crate::core::metrics::get_metrics;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{Stream, StreamExt};
use reqwest::Response;
use std::convert::Infallible;

/// Create an SSE stream from a provider response.
///
/// This function:
/// - Converts the provider's byte stream to SSE events
/// - Rewrites model names to match the original request
/// - Tracks token usage from streaming responses
///
/// # Arguments
///
/// * `response` - HTTP response from the provider
/// * `original_model` - Model name from the original request
/// * `provider_name` - Name of the provider for metrics
pub async fn create_sse_stream(
    response: Response,
    original_model: String,
    provider_name: String,
) -> Sse<impl Stream<Item = std::result::Result<Event, Infallible>>> {
    let stream = response.bytes_stream();

    let event_stream = stream.filter_map(move |chunk_result| {
        let original_model = original_model.clone();
        let provider_name = provider_name.clone();

        async move {
            match chunk_result {
                Ok(bytes) => {
                    let chunk_str = String::from_utf8_lossy(&bytes);

                    // Track token usage if present
                    if chunk_str.contains("\"usage\":") {
                        if let Some(usage) = extract_usage_from_chunk(&chunk_str) {
                            record_token_usage(&usage, &original_model, &provider_name);
                        }
                    }

                    // Rewrite model in chunk
                    let rewritten = rewrite_model_in_chunk(&chunk_str, &original_model);

                    Some(Ok(Event::default().data(rewritten)))
                }
                Err(e) => {
                    tracing::error!("Stream error: {}", e);
                    None
                }
            }
        }
    });

    Sse::new(event_stream).keep_alive(KeepAlive::default())
}

/// Rewrite model name in a streaming chunk.
///
/// Parses SSE data lines and replaces the model field with the original model name.
fn rewrite_model_in_chunk(chunk: &str, original_model: &str) -> String {
    if !chunk.contains("\"model\":") {
        return chunk.to_string();
    }

    let lines: Vec<&str> = chunk.split('\n').collect();
    let mut rewritten_lines = Vec::new();

    for line in lines {
        if line.starts_with("data: ") && line != "data: [DONE]" {
            let json_str = &line[6..];
            if let Ok(mut json_obj) = serde_json::from_str::<serde_json::Value>(json_str) {
                if let Some(obj) = json_obj.as_object_mut() {
                    obj.insert(
                        "model".to_string(),
                        serde_json::Value::String(original_model.to_string()),
                    );
                }
                if let Ok(rewritten_json) = serde_json::to_string(&json_obj) {
                    rewritten_lines.push(format!("data: {}", rewritten_json));
                    continue;
                }
            }
        }
        rewritten_lines.push(line.to_string());
    }

    rewritten_lines.join("\n")
}

/// Extract token usage from a streaming chunk.
fn extract_usage_from_chunk(chunk: &str) -> Option<Usage> {
    let lines: Vec<&str> = chunk.split('\n').collect();

    for line in lines {
        if line.starts_with("data: ") && line != "data: [DONE]" {
            let json_str = &line[6..];
            if let Ok(chunk_obj) = serde_json::from_str::<StreamChunk>(json_str) {
                if chunk_obj.usage.is_some() {
                    return chunk_obj.usage;
                }
            }
        }
    }

    None
}

/// Record token usage metrics.
fn record_token_usage(usage: &Usage, model: &str, provider: &str) {
    let metrics = get_metrics();

    metrics
        .token_usage
        .with_label_values(&[model, provider, "prompt"])
        .inc_by(usage.prompt_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "completion"])
        .inc_by(usage.completion_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "total"])
        .inc_by(usage.total_tokens as u64);

    tracing::debug!(
        "Token usage - model={} provider={} prompt={} completion={} total={}",
        model,
        provider,
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.total_tokens
    );
}

/// Rewrite model name in a non-streaming response.
pub fn rewrite_model_in_response(
    mut response: serde_json::Value,
    original_model: &str,
) -> serde_json::Value {
    if let Some(obj) = response.as_object_mut() {
        obj.insert(
            "model".to_string(),
            serde_json::Value::String(original_model.to_string()),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::metrics::init_metrics;

    #[test]
    fn test_rewrite_model_in_chunk() {
        let chunk = r#"data: {"id":"123","model":"old-model","choices":[]}"#;
        let rewritten = rewrite_model_in_chunk(chunk, "new-model");
        assert!(rewritten.contains("\"model\":\"new-model\""));
    }

    #[test]
    fn test_rewrite_model_preserves_done() {
        let chunk = "data: [DONE]";
        let rewritten = rewrite_model_in_chunk(chunk, "new-model");
        assert_eq!(rewritten, "data: [DONE]");
    }

    #[test]
    fn test_rewrite_model_in_response() {
        let response = serde_json::json!({
            "id": "123",
            "model": "old-model",
            "choices": []
        });
        
        let rewritten = rewrite_model_in_response(response, "new-model");
        assert_eq!(rewritten["model"], "new-model");
    }

    #[test]
    fn test_rewrite_model_in_chunk_multiple_lines() {
        let chunk = r#"data: {"id":"1","model":"old-model","choices":[]}
data: {"id":"2","model":"old-model","choices":[]}
data: [DONE]"#;
        let rewritten = rewrite_model_in_chunk(chunk, "new-model");
        assert!(rewritten.contains("\"model\":\"new-model\""));
        assert!(rewritten.contains("data: [DONE]"));
    }

    #[test]
    fn test_rewrite_model_no_model_field() {
        let chunk = r#"data: {"id":"123","choices":[]}"#;
        let rewritten = rewrite_model_in_chunk(chunk, "new-model");
        // Should return unchanged if no model field
        assert_eq!(rewritten, chunk);
    }

    #[test]
    fn test_rewrite_model_invalid_json() {
        let chunk = r#"data: {invalid json}"#;
        let rewritten = rewrite_model_in_chunk(chunk, "new-model");
        // Should return unchanged if JSON is invalid
        assert_eq!(rewritten, chunk);
    }

    #[test]
    fn test_rewrite_model_in_response_preserves_other_fields() {
        let response = serde_json::json!({
            "id": "123",
            "model": "old-model",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "Hello"}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}
        });
        
        let rewritten = rewrite_model_in_response(response.clone(), "new-model");
        assert_eq!(rewritten["model"], "new-model");
        assert_eq!(rewritten["id"], response["id"]);
        assert_eq!(rewritten["choices"], response["choices"]);
        assert_eq!(rewritten["usage"], response["usage"]);
    }

    #[test]
    fn test_rewrite_model_in_response_non_object() {
        let response = serde_json::json!("not an object");
        let rewritten = rewrite_model_in_response(response.clone(), "new-model");
        // Should return unchanged if not an object
        assert_eq!(rewritten, response);
    }

    #[test]
    fn test_extract_usage_from_chunk_with_usage() {
        init_metrics();
        
        let chunk = r#"data: {"id":"123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}"#;
        let usage = extract_usage_from_chunk(chunk);
        
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_extract_usage_from_chunk_without_usage() {
        let chunk = r#"data: {"id":"123","model":"gpt-4","choices":[]}"#;
        let usage = extract_usage_from_chunk(chunk);
        assert!(usage.is_none());
    }

    #[test]
    fn test_extract_usage_from_chunk_done() {
        let chunk = "data: [DONE]";
        let usage = extract_usage_from_chunk(chunk);
        assert!(usage.is_none());
    }

    #[test]
    fn test_extract_usage_from_chunk_invalid_json() {
        let chunk = r#"data: {invalid}"#;
        let usage = extract_usage_from_chunk(chunk);
        assert!(usage.is_none());
    }

    #[test]
    fn test_extract_usage_from_multiline_chunk() {
        let chunk = r#"data: {"id":"1","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}
data: {"id":"2","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":10,"total_tokens":15}}
data: [DONE]"#;
        let usage = extract_usage_from_chunk(chunk);
        
        assert!(usage.is_some(), "Usage should be extracted from multiline chunk");
        let usage = usage.unwrap();
        assert_eq!(usage.prompt_tokens, 5);
        assert_eq!(usage.completion_tokens, 10);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn test_record_token_usage() {
        init_metrics();
        
        let usage = Usage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        
        record_token_usage(&usage, "gpt-4", "openai");
        
        let metrics = get_metrics();
        let prompt_count = metrics
            .token_usage
            .with_label_values(&["gpt-4", "openai", "prompt"])
            .get();
        
        assert!(prompt_count >= 100);
    }

    #[test]
    fn test_rewrite_model_with_special_characters() {
        let chunk = r#"data: {"id":"123","model":"old-model","choices":[]}"#;
        let rewritten = rewrite_model_in_chunk(chunk, "new-model-v2.0");
        assert!(rewritten.contains("\"model\":\"new-model-v2.0\""));
    }

    #[test]
    fn test_rewrite_model_empty_string() {
        let chunk = r#"data: {"id":"123","model":"old-model","choices":[]}"#;
        let rewritten = rewrite_model_in_chunk(chunk, "");
        assert!(rewritten.contains("\"model\":\"\""));
    }
}