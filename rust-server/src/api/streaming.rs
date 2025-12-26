//! Server-Sent Events (SSE) streaming support for chat completions.
//!
//! This module handles streaming responses from LLM providers, including
//! model name rewriting and token usage tracking.

use crate::api::models::{StreamChunk, Usage, Message};
use crate::core::metrics::get_metrics;
use axum::body::Body;
use axum::response::Response as AxumResponse;
use futures::stream::StreamExt;
use reqwest::Response;
use std::sync::{Arc, Mutex};

/// Token counter for fallback calculation
#[derive(Clone)]
pub struct TokenCounter {
    pub input_tokens: usize,
    pub output_tokens: Arc<Mutex<usize>>,
    pub usage_found: Arc<Mutex<bool>>,
}

impl TokenCounter {
    pub fn new(input_tokens: usize) -> Self {
        Self {
            input_tokens,
            output_tokens: Arc::new(Mutex::new(0)),
            usage_found: Arc::new(Mutex::new(false)),
        }
    }
}

/// Calculate tokens for messages using tiktoken
pub fn calculate_message_tokens(messages: &[Message], model: &str) -> usize {
    match tiktoken_rs::get_bpe_from_model(model) {
        Ok(bpe) => {
            let mut total_tokens = 0;
            for message in messages {
                // Count tokens in content
                total_tokens += bpe.encode_with_special_tokens(&message.content).len();
                // Count tokens in role
                total_tokens += bpe.encode_with_special_tokens(&message.role).len();
                // Format overhead per message
                total_tokens += 4;
            }
            // Conversation format overhead
            total_tokens + 2
        }
        Err(_) => {
            // Fallback to cl100k_base
            if let Ok(bpe) = tiktoken_rs::cl100k_base() {
                let mut total_tokens = 0;
                for message in messages {
                    total_tokens += bpe.encode_with_special_tokens(&message.content).len();
                    total_tokens += bpe.encode_with_special_tokens(&message.role).len();
                    total_tokens += 4;
                }
                total_tokens + 2
            } else {
                0
            }
        }
    }
}

/// Count tokens in text
pub fn count_tokens(text: &str, model: &str) -> usize {
    match tiktoken_rs::get_bpe_from_model(model) {
        Ok(bpe) => bpe.encode_with_special_tokens(text).len(),
        Err(_) => {
            // Fallback to cl100k_base
            tiktoken_rs::cl100k_base()
                .map(|bpe| bpe.encode_with_special_tokens(text).len())
                .unwrap_or(0)
        }
    }
}

/// Create an SSE stream from a provider response with token counting.
///
/// This function:
/// - Converts the provider's byte stream to raw SSE format
/// - Rewrites model names to match the original request
/// - Tracks token usage from streaming responses
/// - Accumulates output tokens for fallback calculation
/// - Maintains OpenAI-compatible SSE format (data: prefix, double newlines, [DONE] message)
///
/// # Arguments
///
/// * `response` - HTTP response from the provider
/// * `original_model` - Model name from the original request
/// * `provider_name` - Name of the provider for metrics
/// * `token_counter` - Optional token counter for fallback calculation
pub async fn create_sse_stream(
    response: Response,
    original_model: String,
    provider_name: String,
    token_counter: Option<TokenCounter>,
) -> AxumResponse {
    let stream = response.bytes_stream();

    let byte_stream = stream.filter_map(move |chunk_result| {
        let original_model = original_model.clone();
        let provider_name = provider_name.clone();
        let token_counter = token_counter.clone();

        async move {
            match chunk_result {
                Ok(bytes) => {
                    let chunk_str = String::from_utf8_lossy(&bytes);

                    // Track token usage if present
                    if chunk_str.contains("\"usage\":") {
                        if let Some(usage) = extract_usage_from_chunk(&chunk_str) {
                            record_token_usage(&usage, &original_model, &provider_name);
                            if let Some(ref counter) = token_counter {
                                *counter.usage_found.lock().unwrap() = true;
                            }
                        }
                    }

                    // Accumulate output tokens for fallback (only if usage not found yet)
                    if let Some(ref counter) = token_counter {
                        if !*counter.usage_found.lock().unwrap() {
                            if let Some(content) = extract_content_from_chunk(&chunk_str) {
                                let tokens = count_tokens(&content, &original_model);
                                *counter.output_tokens.lock().unwrap() += tokens;
                            }
                        }
                    }

                    // Rewrite model in chunk and return as bytes
                    let rewritten = rewrite_model_in_chunk(&chunk_str, &original_model);
                    Some(Ok::<Vec<u8>, std::io::Error>(rewritten.into_bytes()))
                }
                Err(e) => {
                    tracing::error!("Stream error: {}", e);
                    None
                }
            }
        }
    });

    // Convert to Body and create response with proper SSE headers
    let body = Body::from_stream(byte_stream);
    
    AxumResponse::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(body)
        .unwrap()
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

/// Extract content from a streaming chunk for token counting.
fn extract_content_from_chunk(chunk: &str) -> Option<String> {
    let lines: Vec<&str> = chunk.split('\n').collect();

    for line in lines {
        if line.starts_with("data: ") && line != "data: [DONE]" {
            let json_str = &line[6..];
            if let Ok(chunk_obj) = serde_json::from_str::<StreamChunk>(json_str) {
                for choice in chunk_obj.choices {
                    if let Some(content) = choice.delta.content {
                        return Some(content);
                    }
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

/// Record fallback token usage when provider doesn't return usage.
pub fn record_fallback_token_usage(
    input_tokens: usize,
    output_tokens: usize,
    model: &str,
    provider: &str,
) {
    let metrics = get_metrics();
    let total_tokens = input_tokens + output_tokens;

    metrics
        .token_usage
        .with_label_values(&[model, provider, "prompt"])
        .inc_by(input_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "completion"])
        .inc_by(output_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "total"])
        .inc_by(total_tokens as u64);

    tracing::info!(
        "Token usage calculated (fallback) - model={} provider={} prompt={} completion={} total={}",
        model,
        provider,
        input_tokens,
        output_tokens,
        total_tokens
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
    fn test_extract_content_from_chunk() {
        let chunk = r#"data: {"id":"123","object":"chat.completion.chunk","created":1234567890,"model":"gpt-4","choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}"#;
        let content = extract_content_from_chunk(chunk);
        assert_eq!(content, Some("Hello".to_string()));
    }

    #[test]
    fn test_extract_content_from_chunk_no_content() {
        let chunk = r#"data: {"id":"123","model":"gpt-4","choices":[]}"#;
        let content = extract_content_from_chunk(chunk);
        assert!(content.is_none());
    }

    #[test]
    fn test_count_tokens() {
        let text = "Hello world";
        let tokens = count_tokens(text, "gpt-3.5-turbo");
        assert!(tokens > 0);
    }

    #[test]
    fn test_calculate_message_tokens() {
        let messages = vec![
            Message {
                role: "user".to_string(),
                content: "Hello".to_string(),
            },
            Message {
                role: "assistant".to_string(),
                content: "Hi there".to_string(),
            },
        ];
        let tokens = calculate_message_tokens(&messages, "gpt-3.5-turbo");
        assert!(tokens > 0);
        // Should include content + role + format overhead
        assert!(tokens > 10);
    }

    #[test]
    fn test_token_counter() {
        let counter = TokenCounter::new(100);
        assert_eq!(counter.input_tokens, 100);
        assert_eq!(*counter.output_tokens.lock().unwrap(), 0);
        assert_eq!(*counter.usage_found.lock().unwrap(), false);
        
        *counter.output_tokens.lock().unwrap() = 50;
        assert_eq!(*counter.output_tokens.lock().unwrap(), 50);
    }
}