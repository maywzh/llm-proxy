//! Protocol detection for incoming requests.
//!
//! This module provides automatic detection of the LLM API protocol format
//! based on request structure, endpoint path, and explicit headers.

use super::Protocol;
use axum::http::HeaderMap;
use serde_json::Value;

/// Protocol detector for auto-detecting request format.
pub struct ProtocolDetector;

impl ProtocolDetector {
    /// Detect protocol based on request structure.
    ///
    /// Uses heuristics to identify the format:
    /// - Anthropic: has `max_tokens` (required), may have `system` as top-level field,
    ///   messages may have content as array of blocks with `type` field
    /// - Response API: has `input` field or specific Response API fields
    /// - OpenAI: default fallback (most common format)
    pub fn detect(request: &Value) -> Protocol {
        // Check for Anthropic format first (more specific indicators)
        if Self::is_anthropic_format(request) {
            return Protocol::Anthropic;
        }

        // Check for Response API format
        if Self::is_response_api_format(request) {
            return Protocol::ResponseApi;
        }

        // Default to OpenAI (most common)
        Protocol::OpenAI
    }

    /// Detect protocol from explicit `x-protocol` header.
    ///
    /// Supported values: "openai", "anthropic", "claude", "response", "response-api"
    pub fn detect_from_explicit_header(headers: &HeaderMap) -> Option<Protocol> {
        headers
            .get("x-protocol")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| match s.to_lowercase().as_str() {
                "openai" => Some(Protocol::OpenAI),
                "anthropic" | "claude" => Some(Protocol::Anthropic),
                "response" | "response-api" => Some(Protocol::ResponseApi),
                _ => None,
            })
    }

    /// Comprehensive protocol detection with all available signals.
    ///
    /// Priority order (highest to lowest):
    /// 1. Explicit `x-protocol` header (most reliable, user-specified)
    /// 2. Path-based detection
    /// 3. Request structure analysis (fallback)
    pub fn detect_with_headers(request: &Value, headers: &HeaderMap, path: &str) -> Protocol {
        // 1. Highest priority: explicit x-protocol header
        if let Some(protocol) = Self::detect_from_explicit_header(headers) {
            return protocol;
        }

        // 2. Path-based detection
        if let Some(protocol) = Self::detect_from_path(path) {
            return protocol;
        }

        // 3. Fallback to request structure analysis
        Self::detect(request)
    }

    /// Check if request matches Anthropic format.
    ///
    /// Requires multiple conditions to reduce false positives:
    /// - system + max_tokens together, OR
    /// - max_tokens + Anthropic-style content blocks
    fn is_anthropic_format(request: &Value) -> bool {
        let has_system_field = request.get("system").is_some();
        let has_max_tokens = request.get("max_tokens").is_some();

        // Check for Anthropic-style content blocks in messages
        let has_anthropic_content = request
            .get("messages")
            .and_then(|m| m.as_array())
            .map(|msgs| {
                msgs.iter().any(|msg| {
                    // Check if content is an array of typed blocks
                    msg.get("content")
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter().any(|block| {
                                let block_type = block.get("type").and_then(|t| t.as_str());
                                matches!(
                                    block_type,
                                    Some("text")
                                        | Some("image")
                                        | Some("tool_use")
                                        | Some("tool_result")
                                )
                            })
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);

        // Require multiple conditions to reduce false positives:
        // - system field + max_tokens (strong Anthropic indicator)
        // - OR max_tokens + Anthropic-style content blocks
        (has_anthropic_content || has_system_field) && has_max_tokens
    }

    /// Check if request matches Response API format.
    fn is_response_api_format(request: &Value) -> bool {
        // Response API indicators:
        // 1. Has "input" field instead of "messages"
        // 2. Has "instructions" field (similar to system prompt)
        // 3. Has specific Response API fields like "modalities", "response_format" with type

        let has_input = request.get("input").is_some();
        let has_instructions = request.get("instructions").is_some();

        // Primary indicator: has "input" field
        if has_input {
            return true;
        }

        // Secondary indicator: has "instructions" without "messages"
        let has_messages = request.get("messages").is_some();
        if has_instructions && !has_messages {
            return true;
        }

        // Check for Response API specific "response_format" with "type" = "json_schema"
        let has_json_schema_format = request
            .get("response_format")
            .and_then(|rf| rf.get("type"))
            .and_then(|t| t.as_str())
            .map(|t| t == "json_schema")
            .unwrap_or(false);

        if has_json_schema_format {
            // Could be either, but leaning towards Response API if combined with other indicators
            let has_modalities = request.get("modalities").is_some();
            if has_modalities {
                return true;
            }
        }

        false
    }

    /// Detect protocol from endpoint path.
    ///
    /// Returns `Some(Protocol)` if the path clearly indicates a protocol,
    /// `None` if the path is ambiguous or unknown.
    pub fn detect_from_path(path: &str) -> Option<Protocol> {
        let path_lower = path.to_lowercase();

        if path_lower.contains("/chat/completions") {
            Some(Protocol::OpenAI)
        } else if path_lower.contains("/messages") && !path_lower.contains("/responses") {
            Some(Protocol::Anthropic)
        } else if path_lower.contains("/responses") {
            Some(Protocol::ResponseApi)
        } else if path_lower.contains("/completions") && !path_lower.contains("/chat/") {
            // Legacy completions endpoint - OpenAI
            Some(Protocol::OpenAI)
        } else {
            None
        }
    }

    /// Detect protocol with path hint.
    ///
    /// First tries path-based detection, then falls back to request structure analysis.
    pub fn detect_with_path_hint(request: &Value, path: &str) -> Protocol {
        // Path-based detection takes priority if available
        if let Some(protocol) = Self::detect_from_path(path) {
            return protocol;
        }

        // Fall back to request structure analysis
        Self::detect(request)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use serde_json::json;

    #[test]
    fn test_detect_openai_format() {
        let request = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ]
        });

        assert_eq!(ProtocolDetector::detect(&request), Protocol::OpenAI);
    }

    #[test]
    fn test_detect_anthropic_format_with_system() {
        let request = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ]
        });

        assert_eq!(ProtocolDetector::detect(&request), Protocol::Anthropic);
    }

    #[test]
    fn test_detect_anthropic_format_with_content_blocks() {
        let request = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Hello!"}
                    ]
                }
            ]
        });

        assert_eq!(ProtocolDetector::detect(&request), Protocol::Anthropic);
    }

    #[test]
    fn test_detect_response_api_format_with_input() {
        let request = json!({
            "model": "gpt-4",
            "input": "What is the weather?"
        });

        assert_eq!(ProtocolDetector::detect(&request), Protocol::ResponseApi);
    }

    #[test]
    fn test_detect_response_api_format_with_instructions() {
        let request = json!({
            "model": "gpt-4",
            "instructions": "You are a weather assistant.",
            "input": "What is the weather?"
        });

        assert_eq!(ProtocolDetector::detect(&request), Protocol::ResponseApi);
    }

    #[test]
    fn test_detect_from_path_openai() {
        assert_eq!(
            ProtocolDetector::detect_from_path("/v1/chat/completions"),
            Some(Protocol::OpenAI)
        );
        assert_eq!(
            ProtocolDetector::detect_from_path("/api/chat/completions"),
            Some(Protocol::OpenAI)
        );
    }

    #[test]
    fn test_detect_from_path_anthropic() {
        assert_eq!(
            ProtocolDetector::detect_from_path("/v1/messages"),
            Some(Protocol::Anthropic)
        );
        assert_eq!(
            ProtocolDetector::detect_from_path("/api/messages"),
            Some(Protocol::Anthropic)
        );
    }

    #[test]
    fn test_detect_from_path_response_api() {
        assert_eq!(
            ProtocolDetector::detect_from_path("/v1/responses"),
            Some(Protocol::ResponseApi)
        );
    }

    #[test]
    fn test_detect_from_path_unknown() {
        assert_eq!(ProtocolDetector::detect_from_path("/v1/models"), None);
        assert_eq!(ProtocolDetector::detect_from_path("/health"), None);
    }

    #[test]
    fn test_detect_with_path_hint() {
        // Path takes priority
        let request = json!({
            "model": "gpt-4",
            "max_tokens": 1024,
            "system": "You are helpful.",
            "messages": []
        });

        // Even though request looks like Anthropic, path overrides
        assert_eq!(
            ProtocolDetector::detect_with_path_hint(&request, "/v1/chat/completions"),
            Protocol::OpenAI
        );

        // Unknown path falls back to request analysis
        assert_eq!(
            ProtocolDetector::detect_with_path_hint(&request, "/v1/unknown"),
            Protocol::Anthropic
        );
    }

    #[test]
    fn test_openai_with_max_tokens_not_anthropic() {
        // OpenAI can also have max_tokens, but without Anthropic-specific content blocks
        let request = json!({
            "model": "gpt-4",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello!"}
            ]
        });

        // String content (not array of blocks) = OpenAI
        assert_eq!(ProtocolDetector::detect(&request), Protocol::OpenAI);
    }

    #[test]
    fn test_anthropic_tool_result_content() {
        let request = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "tool_result",
                            "tool_use_id": "tool_1",
                            "content": "Result data"
                        }
                    ]
                }
            ]
        });

        assert_eq!(ProtocolDetector::detect(&request), Protocol::Anthropic);
    }

    // =========================================================================
    // x-protocol Header Tests
    // =========================================================================

    #[test]
    fn test_detect_from_explicit_header_openai() {
        let mut headers = HeaderMap::new();
        headers.insert("x-protocol", "openai".parse().unwrap());
        assert_eq!(
            ProtocolDetector::detect_from_explicit_header(&headers),
            Some(Protocol::OpenAI)
        );
    }

    #[test]
    fn test_detect_from_explicit_header_anthropic() {
        let mut headers = HeaderMap::new();
        headers.insert("x-protocol", "anthropic".parse().unwrap());
        assert_eq!(
            ProtocolDetector::detect_from_explicit_header(&headers),
            Some(Protocol::Anthropic)
        );

        // Also test "claude" alias
        headers.insert("x-protocol", "claude".parse().unwrap());
        assert_eq!(
            ProtocolDetector::detect_from_explicit_header(&headers),
            Some(Protocol::Anthropic)
        );
    }

    #[test]
    fn test_detect_from_explicit_header_response_api() {
        let mut headers = HeaderMap::new();
        headers.insert("x-protocol", "response".parse().unwrap());
        assert_eq!(
            ProtocolDetector::detect_from_explicit_header(&headers),
            Some(Protocol::ResponseApi)
        );

        // Also test "response-api" alias
        headers.insert("x-protocol", "response-api".parse().unwrap());
        assert_eq!(
            ProtocolDetector::detect_from_explicit_header(&headers),
            Some(Protocol::ResponseApi)
        );
    }

    #[test]
    fn test_detect_from_explicit_header_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert("x-protocol", "OPENAI".parse().unwrap());
        assert_eq!(
            ProtocolDetector::detect_from_explicit_header(&headers),
            Some(Protocol::OpenAI)
        );

        headers.insert("x-protocol", "Anthropic".parse().unwrap());
        assert_eq!(
            ProtocolDetector::detect_from_explicit_header(&headers),
            Some(Protocol::Anthropic)
        );
    }

    #[test]
    fn test_detect_from_explicit_header_invalid() {
        let mut headers = HeaderMap::new();
        headers.insert("x-protocol", "unknown".parse().unwrap());
        assert_eq!(
            ProtocolDetector::detect_from_explicit_header(&headers),
            None
        );
    }

    #[test]
    fn test_detect_from_explicit_header_missing() {
        let headers = HeaderMap::new();
        assert_eq!(
            ProtocolDetector::detect_from_explicit_header(&headers),
            None
        );
    }

    #[test]
    fn test_detect_with_headers_explicit_overrides_path() {
        let mut headers = HeaderMap::new();
        headers.insert("x-protocol", "anthropic".parse().unwrap());

        // Request looks like OpenAI, path is OpenAI, but x-protocol header says Anthropic
        let request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        assert_eq!(
            ProtocolDetector::detect_with_headers(&request, &headers, "/v1/chat/completions"),
            Protocol::Anthropic
        );
    }

    #[test]
    fn test_detect_with_headers_path_fallback() {
        let headers = HeaderMap::new(); // No x-protocol header

        let request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        // Should use path detection
        assert_eq!(
            ProtocolDetector::detect_with_headers(&request, &headers, "/v1/messages"),
            Protocol::Anthropic
        );
    }

    #[test]
    fn test_detect_with_headers_content_fallback() {
        let headers = HeaderMap::new(); // No x-protocol header

        // Anthropic-style request with unknown path
        let request = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "system": "You are helpful.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        // Should fall back to content detection
        assert_eq!(
            ProtocolDetector::detect_with_headers(&request, &headers, "/v1/unknown"),
            Protocol::Anthropic
        );
    }

    // =========================================================================
    // Improved Anthropic Detection Tests (False Positive Prevention)
    // =========================================================================

    #[test]
    fn test_system_only_without_max_tokens_is_openai() {
        // Some OpenAI clients might send "system" field incorrectly
        // Without max_tokens, we should default to OpenAI
        let request = json!({
            "model": "gpt-4",
            "system": "You are helpful.",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        // Now requires both system AND max_tokens for Anthropic
        assert_eq!(ProtocolDetector::detect(&request), Protocol::OpenAI);
    }
}
