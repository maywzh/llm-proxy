//! Server-Sent Events (SSE) streaming support for chat completions.
//!
//! This module handles streaming responses from LLM providers, including
//! model name rewriting and token usage tracking.

use crate::api::models::Usage;
use crate::core::error::AppError;
use crate::core::logging::get_api_key_name;
use crate::core::metrics::get_metrics;
use axum::body::Body;
use axum::response::{IntoResponse, Response as AxumResponse};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use reqwest::Response;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

// Global cache for BPE encoders to avoid repeated initialization
lazy_static::lazy_static! {
    static ref BPE_CACHE: RwLock<HashMap<String, Arc<tiktoken_rs::CoreBPE>>> = RwLock::new(HashMap::new());
}

/// Get or create a cached BPE encoder for the given model
fn get_cached_bpe(model: &str) -> Option<Arc<tiktoken_rs::CoreBPE>> {
    // Try to read from cache first (fast path)
    {
        let cache = BPE_CACHE.read().ok()?;
        if let Some(bpe) = cache.get(model) {
            return Some(Arc::clone(bpe));
        }
    }
    
    // Cache miss - create new encoder
    let bpe = tiktoken_rs::get_bpe_from_model(model)
        .or_else(|_| tiktoken_rs::cl100k_base())
        .ok()?;
    
    let bpe_arc = Arc::new(bpe);
    
    // Store in cache
    if let Ok(mut cache) = BPE_CACHE.write() {
        cache.insert(model.to_string(), Arc::clone(&bpe_arc));
    }
    
    Some(bpe_arc)
}

/// Stream state for token counting - NO synchronization needed!
/// Each stream has its own state, processed sequentially
struct StreamState {
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    output_tokens: usize,
    usage_found: bool,
    start_time: Instant,
    provider_first_token_time: Option<Instant>,
    input_tokens: usize,
    original_model: String,
    provider_name: String,
    api_key_name: String,
}

impl StreamState {
    fn new(
        stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
        input_tokens: usize,
        original_model: String,
        provider_name: String,
        api_key_name: String,
    ) -> Self {
        Self {
            stream: Box::pin(stream),
            output_tokens: 0,
            usage_found: false,
            start_time: Instant::now(),
            provider_first_token_time: None,
            input_tokens,
            original_model,
            provider_name,
            api_key_name,
        }
    }
}

/// Calculate tokens for messages using tiktoken with caching
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

    // Use cached encoder
    if let Some(bpe) = get_cached_bpe(model) {
        count_with_encoder(messages, &bpe)
    } else {
        0
    }
}

/// Count tokens in text using cached encoder
pub fn count_tokens(text: &str, model: &str) -> usize {
    if let Some(bpe) = get_cached_bpe(model) {
        bpe.encode_with_special_tokens(text).len()
    } else {
        0
    }
}

/// Create an SSE stream from a provider response with token counting and optional TTFT timeout.
///
/// This function uses `unfold` to maintain state sequentially - NO synchronization needed!
/// Each stream processes chunks sequentially, so state is local and lock-free.
///
/// Reads api_key_name from context (set by API_KEY_NAME.scope() in handlers).
///
/// # Arguments
///
/// * `response` - HTTP response from the provider
/// * `original_model` - Model name from the original request
/// * `provider_name` - Name of the provider for metrics
/// * `input_tokens` - Optional input token count for fallback calculation
/// * `ttft_timeout_secs` - Optional TTFT timeout in seconds
pub async fn create_sse_stream(
    response: Response,
    original_model: String,
    provider_name: String,
    input_tokens: Option<usize>,
    ttft_timeout_secs: Option<u64>,
) -> Result<AxumResponse, AppError> {
    let mut stream = response.bytes_stream();
    
    // Get api_key_name from context
    let api_key_name = get_api_key_name();
    
    // Handle TTFT timeout for the first chunk
    let first_chunk = if let Some(timeout_secs) = ttft_timeout_secs {
        match tokio::time::timeout(Duration::from_secs(timeout_secs), stream.next()).await {
            Ok(chunk) => chunk,
            Err(_) => {
                tracing::warn!(
                    "TTFT timeout: first token not received within {} seconds from {}",
                    timeout_secs,
                    provider_name
                );
                return Err(AppError::TTFTTimeout {
                    timeout_secs,
                    provider_name,
                });
            }
        }
    } else {
        stream.next().await
    };

    // Process the first chunk if available
    let (first_chunk_data, stream_ended) = match first_chunk {
        Some(Ok(bytes)) => (Some(bytes), false),
        Some(Err(e)) => {
            tracing::error!("Stream error on first chunk: {}", e);
            let error_event = json!({
                "error": {
                    "message": e.to_string(),
                    "type": "stream_error",
                    "code": "provider_error"
                }
            });
            let error_message = format!(
                "event: error\ndata: {}\n\ndata: [DONE]\n\n",
                error_event
            );
            let body = Body::from(error_message);
            return Ok(AxumResponse::builder()
                .status(200)
                .header("Content-Type", "text/event-stream")
                .header("Cache-Control", "no-cache")
                .header("Connection", "keep-alive")
                .body(body)
                .unwrap());
        }
        None => (None, true),
    };

    // Create initial state - all local, no Arc/Atomic needed!
    let mut initial_state = StreamState::new(
        stream,
        input_tokens.unwrap_or(0),
        original_model,
        provider_name,
        api_key_name,
    );

    // If we have a first chunk, we need to process it and prepend to the stream
    let byte_stream = if let Some(first_bytes) = first_chunk_data {
        // Process the first chunk through the same logic
        let first_output = process_chunk(&mut initial_state, first_bytes);
        
        // Create a stream that yields the first chunk, then continues with the rest
        let first_stream = futures::stream::once(async move { Ok::<Vec<u8>, std::io::Error>(first_output) });
        let rest_stream = futures::stream::unfold(initial_state, |mut state| async move {
            match state.stream.next().await {
                Some(Ok(bytes)) => {
                    let output = process_chunk(&mut state, bytes);
                    Some((Ok(output), state))
                }
                Some(Err(e)) => {
                    tracing::error!("Stream error: {}", e);
                    let error_event = json!({
                        "error": {
                            "message": e.to_string(),
                            "type": "stream_error",
                            "code": "provider_error"
                        }
                    });
                    let error_message = format!(
                        "event: error\ndata: {}\n\ndata: [DONE]\n\n",
                        error_event
                    );
                    
                    let terminated_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>> =
                        Box::pin(futures::stream::empty());
                    let mut final_state = state;
                    final_state.stream = terminated_stream;
                    
                    Some((Ok(error_message.into_bytes()), final_state))
                }
                None => {
                    finalize_stream(&state);
                    None
                }
            }
        });
        
        Box::pin(first_stream.chain(rest_stream)) as Pin<Box<dyn Stream<Item = Result<Vec<u8>, std::io::Error>> + Send>>
    } else if stream_ended {
        // Stream ended immediately
        finalize_stream(&initial_state);
        Box::pin(futures::stream::empty()) as Pin<Box<dyn Stream<Item = Result<Vec<u8>, std::io::Error>> + Send>>
    } else {
        // No first chunk data but stream didn't end - shouldn't happen
        Box::pin(futures::stream::empty()) as Pin<Box<dyn Stream<Item = Result<Vec<u8>, std::io::Error>> + Send>>
    };

    let body = Body::from_stream(byte_stream);

    Ok(AxumResponse::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(body)
        .unwrap())
}

/// Process a chunk of streaming data
fn process_chunk(state: &mut StreamState, bytes: Bytes) -> Vec<u8> {
    // Fast check: Does this chunk contain SSE data?
    let has_data_line = bytes.windows(6).any(|w| w == b"data: ");
    if !has_data_line {
        return bytes.to_vec();
    }
    
    // Parse and process chunk, rewrite model field
    let chunk_str = String::from_utf8_lossy(&bytes);
    let mut rewritten_lines = Vec::new();
    let mut chunk_modified = false;
    
    for line in chunk_str.split('\n') {
        if !line.starts_with("data: ") || line == "data: [DONE]" {
            rewritten_lines.push(line.to_string());
            continue;
        }
        
        let json_str = line[6..].trim();
        if json_str.is_empty() || !json_str.ends_with('}') {
            rewritten_lines.push(line.to_string());
            continue;
        }
        
        if let Ok(mut json_obj) = serde_json::from_str::<serde_json::Value>(json_str) {
            // Rewrite model field to original model (like Python does)
            if json_obj.get("model").is_some() {
                json_obj["model"] = serde_json::Value::String(state.original_model.clone());
                chunk_modified = true;
            }
            
            // Check for usage first
            if !state.usage_found {
                if let Some(usage_value) = json_obj.get("usage") {
                    if let Ok(usage) = serde_json::from_value::<Usage>(usage_value.clone()) {
                        record_token_usage(&usage, &state.original_model, &state.provider_name, &state.api_key_name);
                        state.usage_found = true;
                    }
                }
            }
            
            // Token counting - direct state modification, no locks!
            if !state.usage_found {
                let contents = extract_stream_text(&json_obj);
                if !contents.is_empty() {
                    let now = Instant::now();
                    
                    // Record provider TTFT
                    if state.provider_first_token_time.is_none() {
                        state.provider_first_token_time = Some(now);
                        let provider_ttft = (now - state.start_time).as_secs_f64();
                        let metrics = get_metrics();
                        metrics.ttft.with_label_values(&["provider", &state.original_model, &state.provider_name]).observe(provider_ttft);
                        
                        tracing::debug!(
                            "Provider TTFT recorded - model={} provider={} ttft={:.3}s",
                            state.original_model,
                            state.provider_name,
                            provider_ttft
                        );
                    }
                    
                    // Accumulate tokens - simple addition, no atomics!
                    for content in contents {
                        state.output_tokens += count_tokens(&content, &state.original_model);
                    }
                }
            }
            
            // Rebuild the line with rewritten JSON
            let rewritten_json = serde_json::to_string(&json_obj).unwrap_or_else(|_| json_str.to_string());
            rewritten_lines.push(format!("data: {}", rewritten_json));
        } else {
            rewritten_lines.push(line.to_string());
        }
    }
    
    // Return rewritten chunk if modified, otherwise original
    if chunk_modified {
        rewritten_lines.join("\n").into_bytes()
    } else {
        bytes.to_vec()
    }
}

/// Finalize stream and record metrics
fn finalize_stream(state: &StreamState) {
    if !state.usage_found && state.output_tokens > 0 {
        record_fallback_token_usage(
            state.input_tokens,
            state.output_tokens,
            &state.original_model,
            &state.provider_name,
            &state.api_key_name,
        );
        
        // Calculate and record TPS
        let total_duration = state.start_time.elapsed().as_secs_f64();
        if total_duration > 0.0 {
            let metrics = get_metrics();
            
            // Provider TPS
            if let Some(provider_first_time) = state.provider_first_token_time {
                let provider_duration = provider_first_time.elapsed().as_secs_f64();
                if provider_duration > 0.0 {
                    let provider_tps = state.output_tokens as f64 / provider_duration;
                    metrics
                        .tokens_per_second
                        .with_label_values(&["provider", &state.original_model, &state.provider_name])
                        .observe(provider_tps);
                    
                    tracing::info!(
                        "Provider TPS recorded - model={} provider={} tokens={} duration={:.3}s tps={:.2}",
                        state.original_model,
                        state.provider_name,
                        state.output_tokens,
                        provider_duration,
                        provider_tps
                    );
                }
            }
        }
    }
}

/// Create an SSE stream from a provider response with token counting (legacy version without TTFT timeout).
///
/// This is a convenience wrapper that calls create_sse_stream with no TTFT timeout.
pub async fn create_sse_stream_legacy(
    response: Response,
    original_model: String,
    provider_name: String,
    input_tokens: Option<usize>,
) -> AxumResponse {
    match create_sse_stream(response, original_model.clone(), provider_name.clone(), input_tokens, None).await {
        Ok(response) => response,
        Err(e) => e.into_response(),
    }
}

// Use unfold to process stream with sequential state updates (kept for reference but no longer used directly)
#[allow(dead_code)]
fn create_stream_unfold(initial_state: StreamState) -> impl Stream<Item = Result<Vec<u8>, std::io::Error>> {
    futures::stream::unfold(initial_state, |mut state| async move {
        match state.stream.next().await {
            Some(Ok(bytes)) => {
                let output = process_chunk(&mut state, bytes);
                Some((Ok(output), state))
            }
            Some(Err(e)) => {
                tracing::error!("Stream error: {}", e);
                let error_event = json!({
                    "error": {
                        "message": e.to_string(),
                        "type": "stream_error",
                        "code": "provider_error"
                    }
                });
                let error_message = format!(
                    "event: error\ndata: {}\n\ndata: [DONE]\n\n",
                    error_event
                );
                
                let terminated_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>> =
                    Box::pin(futures::stream::empty());
                let mut final_state = state;
                final_state.stream = terminated_stream;
                
                Some((Ok(error_message.into_bytes()), final_state))
            }
            None => {
                finalize_stream(&state);
                None
            }
        }
    })
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
fn record_token_usage(usage: &Usage, model: &str, provider: &str, api_key_name: &str) {
    let metrics = get_metrics();

    metrics
        .token_usage
        .with_label_values(&[model, provider, "prompt", api_key_name])
        .inc_by(usage.prompt_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "completion", api_key_name])
        .inc_by(usage.completion_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "total", api_key_name])
        .inc_by(usage.total_tokens as u64);

    tracing::debug!(
        "Token usage - model={} provider={} key={} prompt={} completion={} total={}",
        model,
        provider,
        api_key_name,
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
    api_key_name: &str,
) {
    let metrics = get_metrics();
    let total_tokens = input_tokens + output_tokens;

    metrics
        .token_usage
        .with_label_values(&[model, provider, "prompt", api_key_name])
        .inc_by(input_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "completion", api_key_name])
        .inc_by(output_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "total", api_key_name])
        .inc_by(total_tokens as u64);

    tracing::info!(
        "Token usage calculated (fallback) - model={} provider={} key={} prompt={} completion={} total={}",
        model,
        provider,
        api_key_name,
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
    fn test_stream_state_creation() {
        use futures::stream;
        let empty_stream = stream::empty();
        let state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            "test-provider".to_string(),
            "test-key".to_string(),
        );
        
        assert_eq!(state.input_tokens, 100);
        assert_eq!(state.output_tokens, 0);
        assert_eq!(state.usage_found, false);
        assert!(state.provider_first_token_time.is_none());
        assert_eq!(state.api_key_name, "test-key");
    }
}
