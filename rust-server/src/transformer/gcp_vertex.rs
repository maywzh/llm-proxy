//! GCP Vertex AI protocol transformer.
//!
//! GCP Vertex AI uses the same request/response format as Anthropic,
//! so this transformer delegates most of the work to AnthropicTransformer.

use super::anthropic::AnthropicTransformer;
use super::{Protocol, Result, Transformer, UnifiedRequest, UnifiedResponse, UnifiedStreamChunk};
use bytes::Bytes;
use serde_json::Value;

/// GCP Vertex AI protocol transformer.
///
/// This transformer wraps the Anthropic transformer since GCP Vertex AI
/// uses the same request/response format for Claude models.
pub struct GcpVertexTransformer {
    inner: AnthropicTransformer,
}

impl GcpVertexTransformer {
    /// Create a new GCP Vertex transformer.
    pub fn new() -> Self {
        GcpVertexTransformer {
            inner: AnthropicTransformer::new(),
        }
    }
}

impl Default for GcpVertexTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl Transformer for GcpVertexTransformer {
    fn protocol(&self) -> Protocol {
        Protocol::GcpVertex
    }

    fn transform_request_out(&self, raw: Value) -> Result<UnifiedRequest> {
        self.inner.transform_request_out(raw)
    }

    fn transform_request_in(&self, unified: &UnifiedRequest) -> Result<Value> {
        self.inner.transform_request_in(unified)
    }

    fn transform_response_in(&self, raw: Value, original_model: &str) -> Result<UnifiedResponse> {
        self.inner.transform_response_in(raw, original_model)
    }

    fn transform_response_out(
        &self,
        unified: &UnifiedResponse,
        client_protocol: Protocol,
    ) -> Result<Value> {
        self.inner.transform_response_out(unified, client_protocol)
    }

    fn transform_stream_chunk_in(&self, chunk: &Bytes) -> Result<Vec<UnifiedStreamChunk>> {
        self.inner.transform_stream_chunk_in(chunk)
    }

    fn transform_stream_chunk_out(
        &self,
        chunk: &UnifiedStreamChunk,
        client_protocol: Protocol,
    ) -> Result<String> {
        self.inner
            .transform_stream_chunk_out(chunk, client_protocol)
    }

    fn endpoint(&self) -> &'static str {
        // GCP Vertex uses dynamic endpoints, this is a placeholder
        // The actual endpoint is constructed in proxy.rs
        "/v1/messages"
    }

    fn can_handle(&self, raw: &Value) -> bool {
        // GCP Vertex uses Anthropic format
        self.inner.can_handle(raw)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transformer::UnifiedMessage;
    use serde_json::json;

    #[test]
    fn test_gcp_vertex_transformer_protocol() {
        let transformer = GcpVertexTransformer::new();
        assert_eq!(transformer.protocol(), Protocol::GcpVertex);
    }

    #[test]
    fn test_transform_request_out() {
        let transformer = GcpVertexTransformer::new();
        let raw = json!({
            "model": "claude-3-5-sonnet@20241022",
            "max_tokens": 1024,
            "system": "You are helpful.",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ],
            "temperature": 0.7
        });

        let unified = transformer.transform_request_out(raw).unwrap();
        assert_eq!(unified.model, "claude-3-5-sonnet@20241022");
        assert_eq!(unified.system, Some("You are helpful.".to_string()));
        assert_eq!(unified.messages.len(), 1);
        assert_eq!(unified.parameters.max_tokens, Some(1024));
    }

    #[test]
    fn test_transform_request_in() {
        let transformer = GcpVertexTransformer::new();
        let unified = UnifiedRequest::new(
            "claude-3-5-sonnet@20241022",
            vec![UnifiedMessage::user("Hello!")],
        )
        .with_system("Be helpful")
        .with_max_tokens(1024);

        let raw = transformer.transform_request_in(&unified).unwrap();
        assert_eq!(raw["model"], "claude-3-5-sonnet@20241022");
        assert_eq!(raw["max_tokens"], 1024);
        assert!(raw["system"].is_string());
    }

    #[test]
    fn test_transform_response_in() {
        let transformer = GcpVertexTransformer::new();
        let raw = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello there!"}],
            "model": "claude-3-5-sonnet@20241022",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });

        let unified = transformer
            .transform_response_in(raw, "claude-3-5-sonnet@20241022")
            .unwrap();
        assert_eq!(unified.id, "msg_123");
        assert_eq!(unified.text_content(), "Hello there!");
    }

    #[test]
    fn test_can_handle() {
        let transformer = GcpVertexTransformer::new();

        // Anthropic format (which GCP Vertex uses)
        let request = json!({
            "model": "claude-3-5-sonnet@20241022",
            "max_tokens": 1024,
            "system": "Be helpful",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        assert!(transformer.can_handle(&request));

        // OpenAI format
        let request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        assert!(!transformer.can_handle(&request));
    }
}
