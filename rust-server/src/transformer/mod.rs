//! Transformer module for protocol conversion.
//!
//! This module provides the core transformer infrastructure for converting
//! between different LLM API protocols (OpenAI, Anthropic, Response API).
//!
//! # Architecture
//!
//! The transformer system uses a 4-hook model:
//!
//! 1. `transform_request_out`: Convert external client format → Unified Internal Format (UIF)
//! 2. `transform_request_in`: Convert UIF → target provider format
//! 3. `transform_response_in`: Convert provider response → UIF
//! 4. `transform_response_out`: Convert UIF → client response format
//!
//! ```text
//! Client Request
//!     ↓
//! [transform_request_out]  ← Normalize to UIF
//!     ↓
//! Unified Internal Format
//!     ↓
//! [transform_request_in]   ← Adapt to provider
//!     ↓
//! Provider Backend
//!     ↓
//! [transform_response_in]  ← Parse provider response
//!     ↓
//! Unified Internal Format
//!     ↓
//! [transform_response_out] ← Format for client
//!     ↓
//! Client Response
//! ```

pub mod anthropic;
pub mod detector;
pub mod features;
pub mod gcp_vertex;
pub mod openai;
pub mod passthrough;
pub mod response_api;
pub mod stream;
pub mod unified;

use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;

pub use detector::ProtocolDetector;
pub use features::{
    FeatureTransformer, FeatureTransformerChain, ReasoningTransformer, TokenLimitTransformer,
};
pub use passthrough::{should_bypass, transform_request_bypass, PassthroughTransformer};
pub use stream::CrossProtocolStreamState;
pub use unified::provider_type_to_protocol;
pub use unified::*;

use crate::core::error::Result;
use crate::core::AppError;

// ============================================================================
// Transformer Trait
// ============================================================================

/// Transformer trait for protocol conversion.
///
/// Each protocol implements this trait to provide bidirectional conversion
/// between its native format and the Unified Internal Format.
pub trait Transformer: Send + Sync {
    /// Get the protocol this transformer handles.
    fn protocol(&self) -> Protocol;

    /// Transform external request format to Unified Internal Format.
    ///
    /// Hook: `transform_request_out` (Client → Unified)
    fn transform_request_out(&self, raw: serde_json::Value) -> Result<UnifiedRequest>;

    /// Transform Unified Internal Format to provider request format.
    ///
    /// Hook: `transform_request_in` (Unified → Provider)
    fn transform_request_in(&self, unified: &UnifiedRequest) -> Result<serde_json::Value>;

    /// Transform provider response to Unified Internal Format.
    ///
    /// Hook: `transform_response_in` (Provider → Unified)
    fn transform_response_in(
        &self,
        raw: serde_json::Value,
        original_model: &str,
    ) -> Result<UnifiedResponse>;

    /// Transform Unified Internal Format to client response format.
    ///
    /// Hook: `transform_response_out` (Unified → Client)
    fn transform_response_out(
        &self,
        unified: &UnifiedResponse,
        client_protocol: Protocol,
    ) -> Result<serde_json::Value>;

    /// Transform streaming chunk from provider format to unified chunks.
    ///
    /// Returns a vector because one provider chunk might map to multiple unified chunks.
    fn transform_stream_chunk_in(&self, chunk: &Bytes) -> Result<Vec<UnifiedStreamChunk>>;

    /// Transform unified streaming chunk to client format.
    ///
    /// Returns the SSE-formatted string for the chunk.
    fn transform_stream_chunk_out(
        &self,
        chunk: &UnifiedStreamChunk,
        client_protocol: Protocol,
    ) -> Result<String>;

    /// Get the endpoint path for this protocol.
    ///
    /// Returns the API endpoint path (e.g., "/v1/chat/completions" for OpenAI).
    fn endpoint(&self) -> &'static str;

    /// Get the content type for requests.
    fn content_type(&self) -> &'static str {
        "application/json"
    }

    /// Check if this transformer can handle the given request.
    ///
    /// Used by the protocol detector to auto-detect the format.
    fn can_handle(&self, raw: &serde_json::Value) -> bool;
}

// ============================================================================
// Transformer Registry
// ============================================================================

/// Registry for managing protocol transformers.
pub struct TransformerRegistry {
    transformers: HashMap<Protocol, Arc<dyn Transformer>>,
}

impl TransformerRegistry {
    /// Create a new transformer registry with default transformers.
    pub fn new() -> Self {
        let mut registry = Self {
            transformers: HashMap::new(),
        };

        // Register built-in transformers
        registry.register(Arc::new(openai::OpenAITransformer::new()));
        registry.register(Arc::new(anthropic::AnthropicTransformer::new()));
        registry.register(Arc::new(response_api::ResponseApiTransformer::new()));
        registry.register(Arc::new(gcp_vertex::GcpVertexTransformer::new()));

        registry
    }

    /// Create an empty registry (for testing).
    pub fn empty() -> Self {
        Self {
            transformers: HashMap::new(),
        }
    }

    /// Register a transformer.
    pub fn register(&mut self, transformer: Arc<dyn Transformer>) {
        self.transformers
            .insert(transformer.protocol(), transformer);
    }

    /// Get a transformer by protocol.
    pub fn get(&self, protocol: Protocol) -> Option<&Arc<dyn Transformer>> {
        self.transformers.get(&protocol)
    }

    /// Get a transformer by protocol, returning an error if not found.
    pub fn get_or_error(&self, protocol: Protocol) -> Result<&Arc<dyn Transformer>> {
        self.get(protocol)
            .ok_or_else(|| AppError::BadRequest(format!("Unsupported protocol: {}", protocol)))
    }

    /// Detect the protocol and get the appropriate transformer.
    pub fn detect_and_get(&self, raw: &serde_json::Value) -> Option<&Arc<dyn Transformer>> {
        // Try each transformer to see which one can handle the request
        self.transformers
            .values()
            .find(|transformer| transformer.can_handle(raw))
    }

    /// List all registered protocols.
    pub fn protocols(&self) -> Vec<Protocol> {
        self.transformers.keys().cloned().collect()
    }
}

impl Default for TransformerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Transformation Context
// ============================================================================

/// Context passed through the transformation pipeline.
#[derive(Debug, Clone, Default)]
pub struct TransformContext {
    /// Original request ID for tracing
    pub request_id: String,
    /// Client protocol
    pub client_protocol: Protocol,
    /// Target provider protocol
    pub provider_protocol: Protocol,
    /// Original model name from client
    pub original_model: String,
    /// Mapped model name for provider
    pub mapped_model: String,
    /// Provider name
    pub provider_name: String,
    /// Whether streaming is enabled
    pub stream: bool,
    /// Extra metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl TransformContext {
    /// Create a new transform context.
    pub fn new(request_id: impl Into<String>) -> Self {
        TransformContext {
            request_id: request_id.into(),
            ..Default::default()
        }
    }

    /// Check if this is a same-protocol transformation (can potentially bypass).
    pub fn is_same_protocol(&self) -> bool {
        self.client_protocol == self.provider_protocol
    }
}

// ============================================================================
// Pipeline Executor
// ============================================================================

/// Execute a transformation pipeline.
///
/// The pipeline supports optional feature transformers that are applied to the
/// Unified Internal Format (UIF) between protocol transformations.
///
/// # Pipeline Flow
///
/// ```text
/// Client Request
///     ↓
/// [Protocol: transform_request_out]  ← Client format → UIF
///     ↓
/// [Feature Transformers: transform_request]  ← Applied to UIF
///     ↓
/// [Protocol: transform_request_in]   ← UIF → Provider format
///     ↓
/// Provider Backend
///     ↓
/// [Protocol: transform_response_in]  ← Provider format → UIF
///     ↓
/// [Feature Transformers: transform_response]  ← Applied to UIF
///     ↓
/// [Protocol: transform_response_out] ← UIF → Client format
///     ↓
/// Client Response
/// ```
pub struct TransformPipeline {
    registry: Arc<TransformerRegistry>,
    feature_transformers: Option<Arc<dyn FeatureTransformer>>,
}

impl TransformPipeline {
    /// Create a new transform pipeline.
    pub fn new(registry: Arc<TransformerRegistry>) -> Self {
        TransformPipeline {
            registry,
            feature_transformers: None,
        }
    }

    /// Create a new transform pipeline with feature transformers.
    pub fn with_features(
        registry: Arc<TransformerRegistry>,
        features: impl FeatureTransformer + 'static,
    ) -> Self {
        TransformPipeline {
            registry,
            feature_transformers: Some(Arc::new(features)),
        }
    }

    /// Add feature transformers to the pipeline.
    ///
    /// This replaces any existing feature transformers.
    pub fn set_features(&mut self, features: impl FeatureTransformer + 'static) {
        self.feature_transformers = Some(Arc::new(features));
    }

    /// Add feature transformers from an Arc.
    pub fn set_features_arc(&mut self, features: Arc<dyn FeatureTransformer>) {
        self.feature_transformers = Some(features);
    }

    /// Check if feature transformers are configured.
    pub fn has_features(&self) -> bool {
        self.feature_transformers.is_some()
    }

    /// Transform a client request to provider format.
    pub fn transform_request(
        &self,
        raw: serde_json::Value,
        ctx: &TransformContext,
    ) -> Result<serde_json::Value> {
        let client_transformer = self.registry.get_or_error(ctx.client_protocol)?;
        let provider_transformer = self.registry.get_or_error(ctx.provider_protocol)?;

        // Step 1: Client format → Unified
        let mut unified = client_transformer.transform_request_out(raw)?;

        // Update model name if mapped
        if !ctx.mapped_model.is_empty() && ctx.mapped_model != ctx.original_model {
            unified.model = ctx.mapped_model.clone();
        }

        // Step 2: Apply feature transformers to UIF
        if let Some(ref features) = self.feature_transformers {
            features.transform_request(&mut unified)?;
        }

        // Step 3: Unified → Provider format
        provider_transformer.transform_request_in(&unified)
    }

    /// Transform a provider response to client format.
    pub fn transform_response(
        &self,
        raw: serde_json::Value,
        ctx: &TransformContext,
    ) -> Result<serde_json::Value> {
        let client_transformer = self.registry.get_or_error(ctx.client_protocol)?;
        let provider_transformer = self.registry.get_or_error(ctx.provider_protocol)?;

        // Step 1: Provider format → Unified
        let mut unified = provider_transformer.transform_response_in(raw, &ctx.original_model)?;

        // Restore original model name for client
        unified.model = ctx.original_model.clone();

        // Step 2: Apply feature transformers to UIF
        if let Some(ref features) = self.feature_transformers {
            features.transform_response(&mut unified)?;
        }

        // Step 3: Unified → Client format
        client_transformer.transform_response_out(&unified, ctx.client_protocol)
    }

    /// Transform a streaming chunk.
    ///
    /// This applies feature transformers to the unified stream chunk.
    pub fn transform_stream_chunk(&self, chunk: &mut UnifiedStreamChunk) -> Result<()> {
        if let Some(ref features) = self.feature_transformers {
            features.transform_stream_chunk(chunk)?;
        }
        Ok(())
    }

    /// Get the registry.
    pub fn registry(&self) -> &TransformerRegistry {
        &self.registry
    }

    /// Get the feature transformers (if any).
    pub fn features(&self) -> Option<&Arc<dyn FeatureTransformer>> {
        self.feature_transformers.as_ref()
    }

    // =========================================================================
    // Bypass Mode Methods
    // =========================================================================

    /// Check if bypass mode should be used for the given context.
    ///
    /// Bypass mode is used when:
    /// 1. Client and provider use the same protocol
    /// 2. No feature transformers are configured
    ///
    /// In bypass mode, requests/responses pass through with minimal transformation
    /// (only model name mapping is applied).
    pub fn should_bypass(&self, ctx: &TransformContext) -> bool {
        ctx.is_same_protocol() && !self.has_features()
    }

    /// Transform request with bypass optimization.
    ///
    /// If bypass is possible, only applies model name mapping and returns
    /// the payload with a flag indicating bypass was used.
    ///
    /// # Returns
    ///
    /// A tuple of (transformed_payload, bypassed) where:
    /// - `transformed_payload` is the JSON payload to send to the provider
    /// - `bypassed` is true if bypass mode was used
    pub fn transform_request_with_bypass(
        &self,
        raw: serde_json::Value,
        ctx: &TransformContext,
    ) -> Result<(serde_json::Value, bool)> {
        if self.should_bypass(ctx) {
            // Bypass mode: only apply model name mapping
            let mut payload = raw;
            if !ctx.mapped_model.is_empty() && ctx.mapped_model != ctx.original_model {
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert(
                        "model".to_string(),
                        serde_json::Value::String(ctx.mapped_model.clone()),
                    );
                }
            }
            Ok((payload, true))
        } else {
            // Full transformation
            self.transform_request(raw, ctx).map(|v| (v, false))
        }
    }

    /// Transform response with bypass optimization.
    ///
    /// If bypass is possible, only applies model name restoration and returns
    /// the payload with a flag indicating bypass was used.
    ///
    /// # Returns
    ///
    /// A tuple of (transformed_payload, bypassed) where:
    /// - `transformed_payload` is the JSON payload to return to the client
    /// - `bypassed` is true if bypass mode was used
    pub fn transform_response_with_bypass(
        &self,
        raw: serde_json::Value,
        ctx: &TransformContext,
    ) -> Result<(serde_json::Value, bool)> {
        if self.should_bypass(ctx) {
            // Bypass mode: only restore original model name
            let mut payload = raw;
            if let Some(obj) = payload.as_object_mut() {
                obj.insert(
                    "model".to_string(),
                    serde_json::Value::String(ctx.original_model.clone()),
                );
            }
            Ok((payload, true))
        } else {
            // Full transformation
            self.transform_response(raw, ctx).map(|v| (v, false))
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_new() {
        let registry = TransformerRegistry::new();
        assert!(registry.get(Protocol::OpenAI).is_some());
        assert!(registry.get(Protocol::Anthropic).is_some());
        assert!(registry.get(Protocol::ResponseApi).is_some());
        assert!(registry.get(Protocol::GcpVertex).is_some());
    }

    #[test]
    fn test_registry_protocols() {
        let registry = TransformerRegistry::new();
        let protocols = registry.protocols();
        assert_eq!(protocols.len(), 4);
    }

    #[test]
    fn test_transform_context_same_protocol() {
        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::OpenAI;
        assert!(ctx.is_same_protocol());

        ctx.provider_protocol = Protocol::Anthropic;
        assert!(!ctx.is_same_protocol());
    }

    // -------------------------------------------------------------------------
    // Pipeline with Feature Transformers Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_pipeline_without_features() {
        let registry = Arc::new(TransformerRegistry::new());
        let pipeline = TransformPipeline::new(registry);
        assert!(!pipeline.has_features());
        assert!(pipeline.features().is_none());
    }

    #[test]
    fn test_pipeline_with_features() {
        let registry = Arc::new(TransformerRegistry::new());
        let features = TokenLimitTransformer::new(Some(4096));
        let pipeline = TransformPipeline::with_features(registry, features);
        assert!(pipeline.has_features());
        assert!(pipeline.features().is_some());
    }

    #[test]
    fn test_pipeline_set_features() {
        let registry = Arc::new(TransformerRegistry::new());
        let mut pipeline = TransformPipeline::new(registry);
        assert!(!pipeline.has_features());

        pipeline.set_features(TokenLimitTransformer::new(Some(4096)));
        assert!(pipeline.has_features());
    }

    #[test]
    fn test_pipeline_set_features_arc() {
        let registry = Arc::new(TransformerRegistry::new());
        let mut pipeline = TransformPipeline::new(registry);

        let features: Arc<dyn FeatureTransformer> =
            Arc::new(TokenLimitTransformer::new(Some(4096)));
        pipeline.set_features_arc(features);
        assert!(pipeline.has_features());
    }

    #[test]
    fn test_pipeline_with_feature_chain() {
        let registry = Arc::new(TransformerRegistry::new());
        let chain = FeatureTransformerChain::new()
            .add_transformer(ReasoningTransformer::new(false))
            .add_transformer(TokenLimitTransformer::new(Some(4096)));

        let pipeline = TransformPipeline::with_features(registry, chain);
        assert!(pipeline.has_features());
    }

    #[test]
    fn test_pipeline_transform_request_with_token_limit() {
        let registry = Arc::new(TransformerRegistry::new());
        let features = TokenLimitTransformer::new(Some(100));
        let pipeline = TransformPipeline::with_features(registry, features);

        // Create an OpenAI request with high max_tokens
        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 10000
        });

        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::OpenAI;
        ctx.original_model = "gpt-4".to_string();

        let result = pipeline.transform_request(request, &ctx).unwrap();

        // max_tokens should be capped to 100
        assert_eq!(result["max_tokens"], 100);
    }

    #[test]
    fn test_pipeline_transform_stream_chunk() {
        let registry = Arc::new(TransformerRegistry::new());
        let features = ReasoningTransformer::new(false);
        let pipeline = TransformPipeline::with_features(registry, features);

        let mut chunk = UnifiedStreamChunk {
            chunk_type: ChunkType::ContentBlockDelta,
            index: 0,
            delta: Some(UnifiedContent::thinking("thinking...", None)),
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
        };

        pipeline.transform_stream_chunk(&mut chunk).unwrap();

        // Thinking delta should be removed
        assert!(chunk.delta.is_none());
    }

    #[test]
    fn test_pipeline_transform_stream_chunk_no_features() {
        let registry = Arc::new(TransformerRegistry::new());
        let pipeline = TransformPipeline::new(registry);

        let mut chunk = UnifiedStreamChunk {
            chunk_type: ChunkType::ContentBlockDelta,
            index: 0,
            delta: Some(UnifiedContent::thinking("thinking...", None)),
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
        };

        pipeline.transform_stream_chunk(&mut chunk).unwrap();

        // Without features, thinking delta should be preserved
        assert!(chunk.delta.is_some());
    }

    // -------------------------------------------------------------------------
    // Bypass Mode Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_pipeline_should_bypass_same_protocol_no_features() {
        let registry = Arc::new(TransformerRegistry::new());
        let pipeline = TransformPipeline::new(registry);

        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::OpenAI;

        assert!(pipeline.should_bypass(&ctx));
    }

    #[test]
    fn test_pipeline_should_not_bypass_different_protocol() {
        let registry = Arc::new(TransformerRegistry::new());
        let pipeline = TransformPipeline::new(registry);

        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::Anthropic;

        assert!(!pipeline.should_bypass(&ctx));
    }

    #[test]
    fn test_pipeline_should_not_bypass_with_features() {
        let registry = Arc::new(TransformerRegistry::new());
        let features = TokenLimitTransformer::new(Some(4096));
        let pipeline = TransformPipeline::with_features(registry, features);

        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::OpenAI;

        // Same protocol but has features, should not bypass
        assert!(!pipeline.should_bypass(&ctx));
    }

    #[test]
    fn test_pipeline_transform_request_with_bypass_same_protocol() {
        let registry = Arc::new(TransformerRegistry::new());
        let pipeline = TransformPipeline::new(registry);

        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::OpenAI;
        ctx.original_model = "gpt-4".to_string();
        ctx.mapped_model = "gpt-4-turbo".to_string();

        let (result, bypassed) = pipeline
            .transform_request_with_bypass(request, &ctx)
            .unwrap();

        assert!(bypassed);
        assert_eq!(result["model"], "gpt-4-turbo");
    }

    #[test]
    fn test_pipeline_transform_request_with_bypass_same_model() {
        let registry = Arc::new(TransformerRegistry::new());
        let pipeline = TransformPipeline::new(registry);

        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::OpenAI;
        ctx.original_model = "gpt-4".to_string();
        ctx.mapped_model = "gpt-4".to_string(); // Same model

        let (result, bypassed) = pipeline
            .transform_request_with_bypass(request, &ctx)
            .unwrap();

        assert!(bypassed);
        assert_eq!(result["model"], "gpt-4"); // Model unchanged
    }

    #[test]
    fn test_pipeline_transform_request_no_bypass_different_protocol() {
        let registry = Arc::new(TransformerRegistry::new());
        let pipeline = TransformPipeline::new(registry);

        let request = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::Anthropic;
        ctx.original_model = "gpt-4".to_string();
        ctx.mapped_model = "claude-3".to_string();

        let (_, bypassed) = pipeline
            .transform_request_with_bypass(request, &ctx)
            .unwrap();

        // Different protocol, should not bypass
        assert!(!bypassed);
    }

    #[test]
    fn test_pipeline_transform_response_with_bypass() {
        let registry = Arc::new(TransformerRegistry::new());
        let pipeline = TransformPipeline::new(registry);

        let response = serde_json::json!({
            "id": "chatcmpl-123",
            "model": "gpt-4-turbo",
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::OpenAI;
        ctx.original_model = "gpt-4".to_string();
        ctx.mapped_model = "gpt-4-turbo".to_string();

        let (result, bypassed) = pipeline
            .transform_response_with_bypass(response, &ctx)
            .unwrap();

        assert!(bypassed);
        // Model should be restored to original
        assert_eq!(result["model"], "gpt-4");
    }

    #[test]
    fn test_pipeline_transform_response_no_bypass_with_features() {
        let registry = Arc::new(TransformerRegistry::new());
        let features = TokenLimitTransformer::new(Some(4096));
        let pipeline = TransformPipeline::with_features(registry, features);

        let response = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1700000000,
            "model": "gpt-4-turbo",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let mut ctx = TransformContext::new("test-123");
        ctx.client_protocol = Protocol::OpenAI;
        ctx.provider_protocol = Protocol::OpenAI;
        ctx.original_model = "gpt-4".to_string();
        ctx.mapped_model = "gpt-4-turbo".to_string();

        let (_, bypassed) = pipeline
            .transform_response_with_bypass(response, &ctx)
            .unwrap();

        // Has features, should not bypass
        assert!(!bypassed);
    }
}
