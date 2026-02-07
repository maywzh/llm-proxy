//! Passthrough transformer for same-protocol scenarios.
//!
//! This module provides a passthrough transformer that performs minimal transformation
//! when client and provider use the same protocol. This enables zero-copy streaming
//! for optimal performance in same-protocol scenarios.
//!
//! # Architecture
//!
//! The passthrough transformer is used when:
//! 1. Client and provider use the same protocol (e.g., both OpenAI)
//! 2. No feature transformers are configured that require UIF processing
//!
//! In bypass mode, the transformer:
//! - Passes through requests/responses without full parsing
//! - Only applies model name mapping if needed
//! - Enables zero-copy streaming for responses
//!
//! # Performance Benefits
//!
//! - Avoids JSON parsing/serialization overhead
//! - Enables zero-copy streaming (bytes pass through unchanged)
//! - Reduces memory allocations
//! - Lower latency for same-protocol scenarios

use bytes::Bytes;
use serde_json::Value;

use crate::core::error::Result;
use crate::core::AppError;

use super::unified::{
    ChunkType, Protocol, Role, StopReason, UnifiedContent, UnifiedMessage, UnifiedRequest,
    UnifiedResponse, UnifiedStreamChunk, UnifiedUsage,
};
use super::Transformer;

// ============================================================================
// Passthrough Transformer
// ============================================================================

/// Passthrough transformer for same-protocol scenarios.
///
/// This transformer performs minimal transformation when client and provider
/// use the same protocol. It only applies model name mapping if needed.
///
/// # Example
///
/// ```ignore
/// use llm_proxy_rust::transformer::passthrough::PassthroughTransformer;
/// use llm_proxy_rust::transformer::Protocol;
///
/// let transformer = PassthroughTransformer::new(Protocol::OpenAI);
/// ```
#[derive(Debug, Clone)]
pub struct PassthroughTransformer {
    protocol: Protocol,
}

impl PassthroughTransformer {
    /// Create a new passthrough transformer for the given protocol.
    pub fn new(protocol: Protocol) -> Self {
        Self { protocol }
    }

    /// Check if a model name contains only safe characters.
    ///
    /// Safe characters are: alphanumeric, hyphen, underscore, period, forward slash, colon.
    /// This prevents JSON injection attacks when using string replacement.
    fn is_safe_model_name(name: &str) -> bool {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':'))
    }

    /// Apply model name mapping to a JSON payload.
    ///
    /// This modifies the "model" field in-place if present.
    pub fn apply_model_mapping(payload: &mut Value, mapped_model: &str) {
        if let Some(obj) = payload.as_object_mut() {
            if obj.contains_key("model") {
                obj.insert("model".to_string(), Value::String(mapped_model.to_string()));
            }
        }
    }

    /// Apply model name mapping to raw bytes using safe JSON parsing.
    ///
    /// This is the safe fallback that always works but is slower.
    fn apply_model_mapping_json_safe(payload: &[u8], mapped_model: &str) -> Option<Vec<u8>> {
        let mut json: Value = serde_json::from_slice(payload).ok()?;
        Self::apply_model_mapping(&mut json, mapped_model);
        serde_json::to_vec(&json).ok()
    }

    /// Apply model name mapping to raw bytes.
    ///
    /// This is a fast path that avoids full JSON parsing when possible.
    /// Falls back to safe JSON parsing if model names contain unsafe characters.
    /// Returns the modified bytes if mapping was applied, or None if the
    /// original bytes should be used.
    pub fn apply_model_mapping_bytes(
        payload: &[u8],
        original_model: &str,
        mapped_model: &str,
    ) -> Option<Vec<u8>> {
        // If models are the same, no mapping needed
        if original_model == mapped_model {
            return None;
        }

        // Security check: if model names contain unsafe characters, use safe JSON parsing
        if !Self::is_safe_model_name(original_model) || !Self::is_safe_model_name(mapped_model) {
            tracing::debug!(
                "Model name contains special characters, using safe JSON parsing: {} -> {}",
                original_model,
                mapped_model
            );
            return Self::apply_model_mapping_json_safe(payload, mapped_model);
        }

        // Try to find and replace the model field without full parsing
        // This is a fast path for simple cases
        let payload_str = std::str::from_utf8(payload).ok()?;

        // Look for "model": "original_model" pattern
        let search_pattern = format!("\"model\":\"{}\"", original_model);
        let replace_pattern = format!("\"model\":\"{}\"", mapped_model);

        if payload_str.contains(&search_pattern) {
            return Some(
                payload_str
                    .replace(&search_pattern, &replace_pattern)
                    .into_bytes(),
            );
        }

        // Try with space after colon
        let search_pattern = format!("\"model\": \"{}\"", original_model);
        let replace_pattern = format!("\"model\": \"{}\"", mapped_model);

        if payload_str.contains(&search_pattern) {
            return Some(
                payload_str
                    .replace(&search_pattern, &replace_pattern)
                    .into_bytes(),
            );
        }

        // Fall back to full JSON parsing
        Self::apply_model_mapping_json_safe(payload, mapped_model)
    }

    /// Rewrite model name in streaming chunk bytes.
    ///
    /// This is optimized for SSE streaming where we need to rewrite
    /// the model field in each chunk without full parsing.
    pub fn rewrite_model_in_chunk(
        chunk: &[u8],
        original_model: &str,
        mapped_model: &str,
    ) -> Option<Vec<u8>> {
        // If models are the same, no rewriting needed
        if original_model == mapped_model {
            return None;
        }

        // Security check: if model names contain unsafe characters, skip rewriting
        // (streaming chunks are less critical than requests)
        if !Self::is_safe_model_name(original_model) || !Self::is_safe_model_name(mapped_model) {
            tracing::debug!(
                "Model name contains special characters, skipping chunk rewrite: {} -> {}",
                original_model,
                mapped_model
            );
            return None;
        }

        let chunk_str = std::str::from_utf8(chunk).ok()?;

        // Fast check: does this chunk contain the model we need to replace?
        if !chunk_str.contains(mapped_model) {
            return None;
        }

        // Replace model name in the chunk
        // Provider returns mapped_model, we need to restore original_model
        let search_pattern = format!("\"model\":\"{}\"", mapped_model);
        let replace_pattern = format!("\"model\":\"{}\"", original_model);

        if chunk_str.contains(&search_pattern) {
            return Some(
                chunk_str
                    .replace(&search_pattern, &replace_pattern)
                    .into_bytes(),
            );
        }

        // Try with space after colon
        let search_pattern = format!("\"model\": \"{}\"", mapped_model);
        let replace_pattern = format!("\"model\": \"{}\"", original_model);

        if chunk_str.contains(&search_pattern) {
            return Some(
                chunk_str
                    .replace(&search_pattern, &replace_pattern)
                    .into_bytes(),
            );
        }

        None
    }
}

impl Transformer for PassthroughTransformer {
    fn protocol(&self) -> Protocol {
        self.protocol
    }

    fn transform_request_out(&self, raw: Value) -> Result<UnifiedRequest> {
        // For passthrough, we create a minimal UnifiedRequest
        // This is only used when we need to go through the full pipeline
        let model = raw
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown")
            .to_string();

        let messages = raw
            .get("messages")
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|msg| {
                        let role_str = msg.get("role")?.as_str()?;
                        let role: Role = role_str.parse().ok()?;
                        let content = msg.get("content")?.as_str()?;
                        Some(UnifiedMessage::new(role, content))
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(UnifiedRequest::new(&model, messages))
    }

    fn transform_request_in(&self, unified: &UnifiedRequest) -> Result<Value> {
        // For passthrough, we reconstruct a minimal request
        // This preserves the original structure as much as possible
        let messages: Vec<Value> = unified
            .messages
            .iter()
            .map(|msg| {
                let content = msg
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        UnifiedContent::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");

                serde_json::json!({
                    "role": msg.role,
                    "content": content
                })
            })
            .collect();

        let mut result = serde_json::json!({
            "model": unified.model,
            "messages": messages
        });

        // Add optional parameters
        if let Some(max_tokens) = unified.parameters.max_tokens {
            result["max_tokens"] = Value::Number(max_tokens.into());
        }
        if let Some(temperature) = unified.parameters.temperature {
            result["temperature"] = Value::Number(
                serde_json::Number::from_f64(temperature).unwrap_or(serde_json::Number::from(1)),
            );
        }
        if unified.parameters.stream {
            result["stream"] = Value::Bool(true);
        }

        Ok(result)
    }

    fn transform_response_in(&self, raw: Value, original_model: &str) -> Result<UnifiedResponse> {
        // For passthrough, we create a minimal UnifiedResponse
        let id = raw
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or("unknown")
            .to_string();

        let content_text = raw
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let usage = raw
            .get("usage")
            .map(|u| UnifiedUsage {
                input_tokens: u.get("prompt_tokens").and_then(|t| t.as_i64()).unwrap_or(0) as i32,
                output_tokens: u
                    .get("completion_tokens")
                    .and_then(|t| t.as_i64())
                    .unwrap_or(0) as i32,
                cache_read_tokens: None,
                cache_write_tokens: None,
            })
            .unwrap_or_default();

        Ok(UnifiedResponse::text(
            &id,
            original_model,
            &content_text,
            usage,
        ))
    }

    fn transform_response_out(
        &self,
        unified: &UnifiedResponse,
        _client_protocol: Protocol,
    ) -> Result<Value> {
        // For passthrough, we reconstruct a minimal response
        let content = unified
            .content
            .iter()
            .filter_map(|c| match c {
                UnifiedContent::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        let finish_reason = unified
            .stop_reason
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_else(|| "stop".to_string());

        Ok(serde_json::json!({
            "id": unified.id,
            "model": unified.model,
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": content
                },
                "finish_reason": finish_reason
            }],
            "usage": {
                "prompt_tokens": unified.usage.input_tokens,
                "completion_tokens": unified.usage.output_tokens,
                "total_tokens": unified.usage.input_tokens + unified.usage.output_tokens
            }
        }))
    }

    fn transform_stream_chunk_in(&self, chunk: &Bytes) -> Result<Vec<UnifiedStreamChunk>> {
        // For passthrough, we create minimal unified chunks
        let chunk_str = std::str::from_utf8(chunk)
            .map_err(|e| AppError::BadRequest(format!("Invalid UTF-8 in chunk: {}", e)))?;

        let mut chunks = Vec::new();

        for line in chunk_str.lines() {
            if !line.starts_with("data: ") || line == "data: [DONE]" {
                continue;
            }

            let json_str = &line[6..];
            if let Ok(json) = serde_json::from_str::<Value>(json_str) {
                // Extract content delta
                if let Some(content) = json
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|choice| choice.get("delta"))
                    .and_then(|delta| delta.get("content"))
                    .and_then(|c| c.as_str())
                {
                    chunks.push(UnifiedStreamChunk {
                        chunk_type: ChunkType::ContentBlockDelta,
                        index: 0,
                        delta: Some(UnifiedContent::text(content)),
                        usage: None,
                        stop_reason: None,
                        message: None,
                        content_block: None,
                    });
                }

                // Check for finish reason
                if let Some(finish_reason_str) = json
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|choice| choice.get("finish_reason"))
                    .and_then(|r| r.as_str())
                {
                    // Convert string to StopReason
                    let stop_reason = match finish_reason_str {
                        "stop" | "end_turn" => StopReason::EndTurn,
                        "length" | "max_tokens" => StopReason::MaxTokens,
                        "tool_calls" | "tool_use" => StopReason::ToolUse,
                        "content_filter" => StopReason::ContentFilter,
                        _ => StopReason::EndTurn,
                    };
                    chunks.push(UnifiedStreamChunk {
                        chunk_type: ChunkType::MessageDelta,
                        index: 0,
                        delta: None,
                        usage: None,
                        stop_reason: Some(stop_reason),
                        message: None,
                        content_block: None,
                    });
                }
            }
        }

        Ok(chunks)
    }

    fn transform_stream_chunk_out(
        &self,
        chunk: &UnifiedStreamChunk,
        _client_protocol: Protocol,
    ) -> Result<String> {
        // For passthrough, we reconstruct SSE format
        match chunk.chunk_type {
            ChunkType::ContentBlockDelta => {
                if let Some(UnifiedContent::Text { text }) = &chunk.delta {
                    let json = serde_json::json!({
                        "choices": [{
                            "index": chunk.index,
                            "delta": {
                                "content": text
                            }
                        }]
                    });
                    Ok(format!("data: {}\n\n", json))
                } else {
                    Ok(String::new())
                }
            }
            ChunkType::MessageDelta => {
                if let Some(reason) = &chunk.stop_reason {
                    let json = serde_json::json!({
                        "choices": [{
                            "index": chunk.index,
                            "delta": {},
                            "finish_reason": reason
                        }]
                    });
                    Ok(format!("data: {}\n\n", json))
                } else {
                    Ok(String::new())
                }
            }
            ChunkType::MessageStop => Ok("data: [DONE]\n\n".to_string()),
            _ => Ok(String::new()),
        }
    }

    fn endpoint(&self) -> &'static str {
        match self.protocol {
            Protocol::OpenAI => "/v1/chat/completions",
            Protocol::Anthropic => "/v1/messages",
            Protocol::ResponseApi => "/v1/responses",
            Protocol::GcpVertex => "/v1/messages", // GCP Vertex uses Anthropic format
        }
    }

    fn can_handle(&self, _raw: &Value) -> bool {
        // Passthrough transformer can handle any request of its protocol
        true
    }
}

// ============================================================================
// Bypass Utilities
// ============================================================================

/// Check if bypass mode should be used for a transformation.
///
/// Bypass mode is used when:
/// 1. Client and provider use the same protocol
/// 2. No feature transformers are configured
///
/// # Arguments
///
/// * `client_protocol` - The protocol used by the client
/// * `provider_protocol` - The protocol used by the provider
/// * `has_features` - Whether feature transformers are configured
pub fn should_bypass(
    client_protocol: Protocol,
    provider_protocol: Protocol,
    has_features: bool,
) -> bool {
    client_protocol == provider_protocol && !has_features
}

/// Transform request bytes with bypass optimization.
///
/// If bypass is possible, only applies model name mapping.
/// Otherwise, returns None to indicate full transformation is needed.
///
/// # Arguments
///
/// * `payload` - Raw request bytes
/// * `original_model` - Original model name from client
/// * `mapped_model` - Mapped model name for provider
/// * `client_protocol` - Client protocol
/// * `provider_protocol` - Provider protocol
/// * `has_features` - Whether feature transformers are configured
pub fn transform_request_bypass(
    payload: &[u8],
    original_model: &str,
    mapped_model: &str,
    client_protocol: Protocol,
    provider_protocol: Protocol,
    has_features: bool,
) -> Option<Vec<u8>> {
    if !should_bypass(client_protocol, provider_protocol, has_features) {
        return None;
    }

    // Apply model mapping if needed
    if original_model != mapped_model {
        PassthroughTransformer::apply_model_mapping_bytes(payload, original_model, mapped_model)
            .or_else(|| Some(payload.to_vec()))
    } else {
        Some(payload.to_vec())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_passthrough_transformer_creation() {
        let transformer = PassthroughTransformer::new(Protocol::OpenAI);
        assert_eq!(transformer.protocol(), Protocol::OpenAI);
        assert_eq!(transformer.endpoint(), "/v1/chat/completions");
    }

    #[test]
    fn test_passthrough_transformer_anthropic() {
        let transformer = PassthroughTransformer::new(Protocol::Anthropic);
        assert_eq!(transformer.protocol(), Protocol::Anthropic);
        assert_eq!(transformer.endpoint(), "/v1/messages");
    }

    #[test]
    fn test_apply_model_mapping() {
        let mut payload = serde_json::json!({
            "model": "gpt-4",
            "messages": []
        });

        PassthroughTransformer::apply_model_mapping(&mut payload, "gpt-4-turbo");
        assert_eq!(payload["model"], "gpt-4-turbo");
    }

    #[test]
    fn test_apply_model_mapping_bytes_same_model() {
        let payload = br#"{"model":"gpt-4","messages":[]}"#;
        let result = PassthroughTransformer::apply_model_mapping_bytes(payload, "gpt-4", "gpt-4");
        assert!(result.is_none());
    }

    #[test]
    fn test_apply_model_mapping_bytes_different_model() {
        let payload = br#"{"model":"gpt-4","messages":[]}"#;
        let result =
            PassthroughTransformer::apply_model_mapping_bytes(payload, "gpt-4", "gpt-4-turbo");
        assert!(result.is_some());
        let result_str = String::from_utf8(result.unwrap()).unwrap();
        assert!(result_str.contains("gpt-4-turbo"));
    }

    #[test]
    fn test_apply_model_mapping_bytes_with_space() {
        let payload = br#"{"model": "gpt-4", "messages": []}"#;
        let result =
            PassthroughTransformer::apply_model_mapping_bytes(payload, "gpt-4", "gpt-4-turbo");
        assert!(result.is_some());
        let result_str = String::from_utf8(result.unwrap()).unwrap();
        assert!(result_str.contains("gpt-4-turbo"));
    }

    #[test]
    fn test_rewrite_model_in_chunk() {
        let chunk = br#"data: {"model":"gpt-4-turbo","choices":[{"delta":{"content":"Hi"}}]}"#;
        let result = PassthroughTransformer::rewrite_model_in_chunk(chunk, "gpt-4", "gpt-4-turbo");
        assert!(result.is_some());
        let result_str = String::from_utf8(result.unwrap()).unwrap();
        assert!(result_str.contains("\"model\":\"gpt-4\""));
        assert!(!result_str.contains("gpt-4-turbo"));
    }

    #[test]
    fn test_rewrite_model_in_chunk_same_model() {
        let chunk = br#"data: {"model":"gpt-4","choices":[{"delta":{"content":"Hi"}}]}"#;
        let result = PassthroughTransformer::rewrite_model_in_chunk(chunk, "gpt-4", "gpt-4");
        assert!(result.is_none());
    }

    #[test]
    fn test_should_bypass_same_protocol_no_features() {
        assert!(should_bypass(Protocol::OpenAI, Protocol::OpenAI, false));
        assert!(should_bypass(
            Protocol::Anthropic,
            Protocol::Anthropic,
            false
        ));
    }

    #[test]
    fn test_should_bypass_same_protocol_with_features() {
        assert!(!should_bypass(Protocol::OpenAI, Protocol::OpenAI, true));
    }

    #[test]
    fn test_should_bypass_different_protocol() {
        assert!(!should_bypass(Protocol::OpenAI, Protocol::Anthropic, false));
        assert!(!should_bypass(Protocol::Anthropic, Protocol::OpenAI, false));
    }

    #[test]
    fn test_transform_request_bypass_same_protocol() {
        let payload = br#"{"model":"gpt-4","messages":[]}"#;
        let result = transform_request_bypass(
            payload,
            "gpt-4",
            "gpt-4-turbo",
            Protocol::OpenAI,
            Protocol::OpenAI,
            false,
        );
        assert!(result.is_some());
        let result_str = String::from_utf8(result.unwrap()).unwrap();
        assert!(result_str.contains("gpt-4-turbo"));
    }

    #[test]
    fn test_transform_request_bypass_different_protocol() {
        let payload = br#"{"model":"gpt-4","messages":[]}"#;
        let result = transform_request_bypass(
            payload,
            "gpt-4",
            "claude-3",
            Protocol::OpenAI,
            Protocol::Anthropic,
            false,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_transform_request_bypass_with_features() {
        let payload = br#"{"model":"gpt-4","messages":[]}"#;
        let result = transform_request_bypass(
            payload,
            "gpt-4",
            "gpt-4-turbo",
            Protocol::OpenAI,
            Protocol::OpenAI,
            true,
        );
        assert!(result.is_none());
    }

    #[test]
    fn test_passthrough_can_handle() {
        let transformer = PassthroughTransformer::new(Protocol::OpenAI);
        let payload = serde_json::json!({"model": "gpt-4"});
        assert!(transformer.can_handle(&payload));
    }

    #[test]
    fn test_passthrough_transform_request_out() {
        let transformer = PassthroughTransformer::new(Protocol::OpenAI);
        let payload = serde_json::json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello"}
            ]
        });

        let unified = transformer.transform_request_out(payload).unwrap();
        assert_eq!(unified.model, "gpt-4");
        assert_eq!(unified.messages.len(), 1);
    }

    #[test]
    fn test_passthrough_transform_response_in() {
        let transformer = PassthroughTransformer::new(Protocol::OpenAI);
        let response = serde_json::json!({
            "id": "chatcmpl-123",
            "model": "gpt-4-turbo",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                }
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5
            }
        });

        let unified = transformer
            .transform_response_in(response, "gpt-4")
            .unwrap();
        assert_eq!(unified.id, "chatcmpl-123");
        assert_eq!(unified.model, "gpt-4");
        assert_eq!(unified.usage.input_tokens, 10);
        assert_eq!(unified.usage.output_tokens, 5);
    }

    #[test]
    fn test_passthrough_transform_stream_chunk_in() {
        let transformer = PassthroughTransformer::new(Protocol::OpenAI);
        let chunk = Bytes::from("data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n");

        let chunks = transformer.transform_stream_chunk_in(&chunk).unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(matches!(chunks[0].chunk_type, ChunkType::ContentBlockDelta));
    }

    // =========================================================================
    // Model Name Safety Tests
    // =========================================================================

    #[test]
    fn test_is_safe_model_name_valid() {
        assert!(PassthroughTransformer::is_safe_model_name("gpt-4"));
        assert!(PassthroughTransformer::is_safe_model_name("gpt-4-turbo"));
        assert!(PassthroughTransformer::is_safe_model_name(
            "claude-3-opus-20240229"
        ));
        assert!(PassthroughTransformer::is_safe_model_name(
            "models/gemini-pro"
        ));
        assert!(PassthroughTransformer::is_safe_model_name("gpt_4_turbo"));
        assert!(PassthroughTransformer::is_safe_model_name("model.v1"));
        assert!(PassthroughTransformer::is_safe_model_name("provider:model"));
    }

    #[test]
    fn test_is_safe_model_name_invalid() {
        // Empty string
        assert!(!PassthroughTransformer::is_safe_model_name(""));
        // Contains quotes (JSON injection risk)
        assert!(!PassthroughTransformer::is_safe_model_name("gpt-4\""));
        assert!(!PassthroughTransformer::is_safe_model_name(
            "gpt-4\",\"hack\":\""
        ));
        // Contains backslash
        assert!(!PassthroughTransformer::is_safe_model_name("gpt-4\\n"));
        // Contains spaces
        assert!(!PassthroughTransformer::is_safe_model_name("gpt 4"));
        // Contains special characters
        assert!(!PassthroughTransformer::is_safe_model_name("gpt-4<script>"));
        assert!(!PassthroughTransformer::is_safe_model_name(
            "gpt-4;drop table"
        ));
    }

    #[test]
    fn test_apply_model_mapping_bytes_with_unsafe_model_uses_json_parsing() {
        // Model name with quote - should fall back to safe JSON parsing
        let payload = br#"{"model":"gpt-4","messages":[]}"#;
        let unsafe_model = "gpt-4\",\"injected\":\"value";

        // This should use safe JSON parsing and properly escape the model name
        let result =
            PassthroughTransformer::apply_model_mapping_bytes(payload, "gpt-4", unsafe_model);

        assert!(result.is_some());
        let result_str = String::from_utf8(result.unwrap()).unwrap();
        // The unsafe characters should be properly escaped by serde_json
        assert!(result_str.contains("\\\""));
        // Verify it's valid JSON
        assert!(serde_json::from_str::<serde_json::Value>(&result_str).is_ok());
    }

    #[test]
    fn test_rewrite_model_in_chunk_with_unsafe_model_returns_none() {
        let chunk = br#"data: {"model":"gpt-4-turbo","choices":[{"delta":{"content":"Hi"}}]}"#;
        let unsafe_model = "gpt-4\"injected";

        // Should return None for unsafe model names (skip rewriting)
        let result =
            PassthroughTransformer::rewrite_model_in_chunk(chunk, unsafe_model, "gpt-4-turbo");
        assert!(result.is_none());
    }
}
