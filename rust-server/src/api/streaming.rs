//! Server-Sent Events (SSE) streaming support for chat completions.
//!
//! This module handles streaming responses from LLM providers, including
//! model name rewriting and token usage tracking.

use crate::api::models::Usage;
use crate::core::metrics::get_metrics;
use axum::body::Body;
use axum::response::Response as AxumResponse;
use futures::stream::StreamExt;
use reqwest::Response;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Token counter for fallback calculation and performance tracking
#[derive(Clone)]
pub struct TokenCounter {
    pub input_tokens: usize,
    pub output_tokens: Arc<Mutex<usize>>,
    pub usage_found: Arc<Mutex<bool>>,
    pub start_time: Instant,
    pub first_token_time: Arc<Mutex<Option<Instant>>>,
    pub provider_first_token_time: Arc<Mutex<Option<Instant>>>,
}

impl TokenCounter {
    pub fn new(input_tokens: usize) -> Self {
        Self {
            input_tokens,
            output_tokens: Arc::new(Mutex::new(0)),
            usage_found: Arc::new(Mutex::new(false)),
            start_time: Instant::now(),
            first_token_time: Arc::new(Mutex::new(None)),
            provider_first_token_time: Arc::new(Mutex::new(None)),
        }
    }
}

/// Calculate tokens for messages using tiktoken
pub fn calculate_message_tokens(messages: &[Value], model: &str) -> usize {
    fn count_with_encoder(messages: &[Value], encoder: &tiktoken_rs::CoreBPE) -> usize {
        let mut total_tokens = 0;
        for message in messages {
            if let Some(content) = message.get("content") {
                total_tokens += match content {
                    Value::String(text) => encoder.encode_with_special_tokens(text).len(),
                    Value::Array(parts) => parts
                        .iter()
                        .flat_map(extract_text_segments)
                        .map(|text| encoder.encode_with_special_tokens(&text).len())
                        .sum::<usize>(),
                    Value::Object(_) => extract_text_segments(content)
                        .into_iter()
                        .map(|text| encoder.encode_with_special_tokens(&text).len())
                        .sum::<usize>(),
                    _ => 0,
                };
            }

            if let Some(role) = message.get("role").and_then(|r| r.as_str()) {
                total_tokens += encoder.encode_with_special_tokens(role).len();
            }

            if let Some(name) = message.get("name").and_then(|n| n.as_str()) {
                total_tokens += encoder.encode_with_special_tokens(name).len();
            }

            // Format overhead per message
            total_tokens += 4;
        }

        // Conversation format overhead
        total_tokens + 2
    }

    match tiktoken_rs::get_bpe_from_model(model) {
        Ok(bpe) => count_with_encoder(messages, &bpe),
        Err(_) => {
            if let Ok(bpe) = tiktoken_rs::cl100k_base() {
                count_with_encoder(messages, &bpe)
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
/// - Accumulates output tokens for fallback calculation (synchronously)
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
    
    // Track if this is the last chunk to record fallback tokens synchronously
    let token_counter_for_final = token_counter.clone();
    let model_for_final = original_model.clone();
    let provider_for_final = provider_name.clone();

    let byte_stream = stream.filter_map(move |chunk_result| {
        let original_model = original_model.clone();
        let provider_name = provider_name.clone();
        let token_counter = token_counter.clone();

        async move {
            match chunk_result {
                Ok(bytes) => {
                    // Check if we need token counting
                    let needs_token_count = token_counter.as_ref()
                        .map(|c| !*c.usage_found.lock().unwrap())
                        .unwrap_or(false);
                    
                    // Ultra-fast path: If no token counting needed, pass through directly!
                    if !needs_token_count {
                        return Some(Ok::<Vec<u8>, std::io::Error>(bytes.to_vec()));
                    }
                    
                    // Only parse for token counting when needed
                    let chunk_str = String::from_utf8_lossy(&bytes);
                    
                    for line in chunk_str.split('\n') {
                        if line.starts_with("data: ") && line != "data: [DONE]" {
                            let json_str = line[6..].trim();
                            if json_str.is_empty() || !json_str.ends_with('}') {
                                continue;
                            }
                            
                            if let Ok(json_obj) = serde_json::from_str::<serde_json::Value>(json_str) {
                                // Check for usage
                                if let Some(usage_value) = json_obj.get("usage") {
                                    if let Ok(usage) = serde_json::from_value::<Usage>(usage_value.clone()) {
                                        record_token_usage(&usage, &original_model, &provider_name);
                                        if let Some(ref counter) = token_counter {
                                            *counter.usage_found.lock().unwrap() = true;
                                        }
                                    }
                                }
                                
                                // Token counting
                                if let Some(ref counter) = token_counter {
                                    if !*counter.usage_found.lock().unwrap() {
                                        let contents = extract_stream_text(&json_obj);
                                        if !contents.is_empty() {
                                            let now = Instant::now();
                                            
                                            let mut provider_first_token_guard = counter.provider_first_token_time.lock().unwrap();
                                            if provider_first_token_guard.is_none() {
                                                *provider_first_token_guard = Some(now);
                                                let provider_ttft = (now - counter.start_time).as_secs_f64();
                                                let metrics = get_metrics();
                                                metrics.ttft.with_label_values(&["provider", &original_model, &provider_name]).observe(provider_ttft);
                                                
                                                tracing::debug!(
                                                    "Provider TTFT recorded - model={} provider={} ttft={:.3}s",
                                                    original_model,
                                                    provider_name,
                                                    provider_ttft
                                                );
                                            }
                                            
                                            let mut first_token_guard = counter.first_token_time.lock().unwrap();
                                            if first_token_guard.is_none() {
                                                *first_token_guard = Some(now);
                                                let proxy_ttft = (now - counter.start_time).as_secs_f64();
                                                let metrics = get_metrics();
                                                metrics.ttft.with_label_values(&["proxy", &original_model, &provider_name]).observe(proxy_ttft);
                                                
                                                tracing::debug!(
                                                    "Proxy TTFT recorded - model={} provider={} ttft={:.3}s",
                                                    original_model,
                                                    provider_name,
                                                    proxy_ttft
                                                );
                                            }
                                            
                                            let mut output_guard = counter.output_tokens.lock().unwrap();
                                            for content in contents {
                                                *output_guard += count_tokens(&content, &original_model);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    
                    // Return original bytes without any modification!
                    Some(Ok::<Vec<u8>, std::io::Error>(bytes.to_vec()))

                }
                Err(e) => {
                    tracing::error!("Stream error: {}", e);
                    // Send error event to client using SSE format
                    let error_event = json!({
                        "error": {
                            "message": e.to_string(),
                            "type": "stream_error",
                            "code": "provider_error"
                        }
                    });
                    let error_message = format!("event: error\ndata: {}\n\n", error_event);
                    Some(Ok::<Vec<u8>, std::io::Error>(error_message.into_bytes()))
                }
            }
        }
    });

    // Wrap the stream to record fallback tokens after completion
    let byte_stream_with_cleanup = futures::stream::StreamExt::chain(
        byte_stream,
        futures::stream::once(async move {
            // Record fallback tokens and TPS synchronously after stream completes
            if let Some(counter) = token_counter_for_final {
                if !*counter.usage_found.lock().unwrap() {
                    let output_tokens = *counter.output_tokens.lock().unwrap();
                    record_fallback_token_usage(
                        counter.input_tokens,
                        output_tokens,
                        &model_for_final,
                        &provider_for_final,
                    );
                    
                    // Calculate and record TPS for both provider and proxy
                    let total_duration = counter.start_time.elapsed().as_secs_f64();
                    if total_duration > 0.0 && output_tokens > 0 {
                        let metrics = get_metrics();
                        
                        // Provider TPS (from provider's first token to last token)
                        if let Some(provider_first_time) = *counter.provider_first_token_time.lock().unwrap() {
                            let provider_duration = provider_first_time.elapsed().as_secs_f64();
                            if provider_duration > 0.0 {
                                let provider_tps = output_tokens as f64 / provider_duration;
                                metrics
                                    .tokens_per_second
                                    .with_label_values(&["provider", &model_for_final, &provider_for_final])
                                    .observe(provider_tps);
                                
                                tracing::info!(
                                    "Provider TPS recorded - model={} provider={} tokens={} duration={:.3}s tps={:.2}",
                                    model_for_final,
                                    provider_for_final,
                                    output_tokens,
                                    provider_duration,
                                    provider_tps
                                );
                            }
                        }
                        
                        // Proxy TPS (end-to-end from client request to last token)
                        let proxy_tps = output_tokens as f64 / total_duration;
                        metrics
                            .tokens_per_second
                            .with_label_values(&["proxy", &model_for_final, &provider_for_final])
                            .observe(proxy_tps);
                        
                        tracing::info!(
                            "Proxy TPS recorded - model={} provider={} tokens={} duration={:.3}s tps={:.2}",
                            model_for_final,
                            provider_for_final,
                            output_tokens,
                            total_duration,
                            proxy_tps
                        );
                    }
                }
            }
            // Return empty chunk to signal end
            Ok::<Vec<u8>, std::io::Error>(vec![])
        }),
    );

    // Convert to Body and create response with proper SSE headers
    let body = Body::from_stream(byte_stream_with_cleanup);

    AxumResponse::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(body)
        .unwrap()
}

fn extract_text_segments(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items.iter().flat_map(extract_text_segments).collect(),
        Value::Object(obj) => {
            if obj
                .get("type")
                .and_then(|t| t.as_str())
                .map(|t| t == "text")
                .unwrap_or(false)
            {
                if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                    vec![text.to_string()]
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}

fn extract_stream_text(chunk_obj: &Value) -> Vec<String> {
    let mut contents = Vec::new();
    if let Some(choices) = chunk_obj.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                if let Some(content_field) = delta.get("content") {
                    match content_field {
                        Value::String(text) => contents.push(text.clone()),
                        Value::Array(parts) => {
                            for part in parts {
                                contents.extend(extract_text_segments(part));
                            }
                        }
                        Value::Object(_) => {
                            contents.extend(extract_text_segments(content_field));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    contents
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
    fn test_count_tokens() {
        let text = "Hello world";
        let tokens = count_tokens(text, "gpt-3.5-turbo");
        assert!(tokens > 0);
    }

    #[test]
    fn test_calculate_message_tokens() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "Hello"}),
            serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "Hi there"}]}),
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
        assert!(counter.first_token_time.lock().unwrap().is_none());
        assert!(counter.provider_first_token_time.lock().unwrap().is_none());

        *counter.output_tokens.lock().unwrap() = 50;
        assert_eq!(*counter.output_tokens.lock().unwrap(), 50);
    }
}
