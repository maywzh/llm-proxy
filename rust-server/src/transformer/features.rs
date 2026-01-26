//! Feature transformers for cross-cutting concerns.
//!
//! This module provides pluggable feature transformers that can be added to the
//! transformation pipeline to handle cross-cutting concerns like reasoning/thinking
//! blocks and token limits.
//!
//! # Architecture
//!
//! Feature transformers are applied after protocol transformation:
//!
//! ```text
//! Client Request
//!     ↓
//! [Protocol Transform: request_out]
//!     ↓
//! Unified Internal Format
//!     ↓
//! [Feature Transformers: transform_request]  ← Applied here
//!     ↓
//! [Protocol Transform: request_in]
//!     ↓
//! Provider Backend
//!     ↓
//! [Protocol Transform: response_in]
//!     ↓
//! Unified Internal Format
//!     ↓
//! [Feature Transformers: transform_response]  ← Applied here
//!     ↓
//! [Protocol Transform: response_out]
//!     ↓
//! Client Response
//! ```

use crate::core::error::Result;
use crate::core::AppError;

use super::unified::{UnifiedContent, UnifiedRequest, UnifiedResponse, UnifiedStreamChunk};

// ============================================================================
// Feature Transformer Trait
// ============================================================================

/// Feature transformer trait for cross-cutting concerns.
///
/// Feature transformers are applied to the Unified Internal Format (UIF) to handle
/// concerns that span multiple protocols, such as:
/// - Reasoning/thinking block handling
/// - Token limit enforcement
/// - Content filtering
/// - Logging/observability
///
/// # Example
///
/// ```ignore
/// use llm_proxy_rust::transformer::features::{FeatureTransformer, ReasoningTransformer};
///
/// let transformer = ReasoningTransformer::new(true);
/// let mut request = UnifiedRequest::new("gpt-4", vec![]);
/// transformer.transform_request(&mut request)?;
/// ```
pub trait FeatureTransformer: Send + Sync {
    /// Transform request before sending to provider.
    ///
    /// This is called after the client request has been converted to UIF
    /// but before it's converted to the provider format.
    fn transform_request(&self, request: &mut UnifiedRequest) -> Result<()>;

    /// Transform response before returning to client.
    ///
    /// This is called after the provider response has been converted to UIF
    /// but before it's converted to the client format.
    fn transform_response(&self, response: &mut UnifiedResponse) -> Result<()>;

    /// Transform streaming chunk.
    ///
    /// This is called for each streaming chunk after it's been converted to UIF
    /// but before it's converted to the client format.
    fn transform_stream_chunk(&self, chunk: &mut UnifiedStreamChunk) -> Result<()>;

    /// Feature name for logging/debugging.
    fn name(&self) -> &'static str;
}

// ============================================================================
// Reasoning Transformer
// ============================================================================

/// Transformer for handling thinking/reasoning blocks across protocols.
///
/// Different LLM providers handle reasoning/thinking differently:
/// - Anthropic: Uses `thinking` content blocks with `thinking` parameter
/// - OpenAI: Uses `reasoning_effort` parameter (low/medium/high)
/// - Some models: Include thinking in the response content
///
/// This transformer normalizes reasoning handling across protocols.
///
/// # Example
///
/// ```ignore
/// use llm_proxy_rust::transformer::features::ReasoningTransformer;
///
/// // Include thinking blocks in output
/// let transformer = ReasoningTransformer::new(true);
///
/// // Strip thinking blocks from output
/// let transformer = ReasoningTransformer::new(false);
/// ```
#[derive(Debug, Clone)]
pub struct ReasoningTransformer {
    /// Whether to include thinking blocks in output
    include_thinking: bool,
}

impl ReasoningTransformer {
    /// Create a new reasoning transformer.
    ///
    /// # Arguments
    ///
    /// * `include_thinking` - If true, thinking blocks are preserved in responses.
    ///   If false, thinking blocks are stripped from responses.
    pub fn new(include_thinking: bool) -> Self {
        Self { include_thinking }
    }

    /// Check if a content block is a thinking block.
    fn is_thinking_content(content: &UnifiedContent) -> bool {
        matches!(content, UnifiedContent::Thinking { .. })
    }
}

impl FeatureTransformer for ReasoningTransformer {
    fn transform_request(&self, _request: &mut UnifiedRequest) -> Result<()> {
        // Request transformation is a no-op for now.
        // Future: Could add reasoning parameters based on model capabilities.
        Ok(())
    }

    fn transform_response(&self, response: &mut UnifiedResponse) -> Result<()> {
        if !self.include_thinking {
            // Remove thinking blocks from response content
            response.content.retain(|c| !Self::is_thinking_content(c));
        }
        Ok(())
    }

    fn transform_stream_chunk(&self, chunk: &mut UnifiedStreamChunk) -> Result<()> {
        if !self.include_thinking {
            // Check if the delta is a thinking block
            if let Some(ref delta) = chunk.delta {
                if Self::is_thinking_content(delta) {
                    // Replace with empty text to effectively skip this chunk
                    chunk.delta = None;
                }
            }

            // Check content_block for content_block_start events
            if let Some(ref content_block) = chunk.content_block {
                if Self::is_thinking_content(content_block) {
                    chunk.content_block = None;
                }
            }
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "reasoning"
    }
}

// ============================================================================
// Token Limit Transformer
// ============================================================================

/// Transformer for enforcing token limits.
///
/// This transformer ensures that requests don't exceed configured token limits.
/// It can either:
/// - Reject requests that exceed the limit
/// - Cap the max_tokens to the configured limit
///
/// # Example
///
/// ```ignore
/// use llm_proxy_rust::transformer::features::TokenLimitTransformer;
///
/// // Enforce a maximum of 4096 tokens
/// let transformer = TokenLimitTransformer::new(Some(4096));
///
/// // No limit enforcement
/// let transformer = TokenLimitTransformer::new(None);
/// ```
#[derive(Debug, Clone)]
pub struct TokenLimitTransformer {
    /// Maximum allowed tokens (None means no limit)
    max_tokens: Option<u32>,
    /// Whether to cap tokens instead of rejecting
    cap_instead_of_reject: bool,
}

impl TokenLimitTransformer {
    /// Create a new token limit transformer.
    ///
    /// # Arguments
    ///
    /// * `max_tokens` - Maximum allowed tokens. None means no limit.
    pub fn new(max_tokens: Option<u32>) -> Self {
        Self {
            max_tokens,
            cap_instead_of_reject: true, // Default to capping
        }
    }

    /// Create a token limit transformer that rejects requests exceeding the limit.
    ///
    /// # Arguments
    ///
    /// * `max_tokens` - Maximum allowed tokens.
    pub fn new_strict(max_tokens: u32) -> Self {
        Self {
            max_tokens: Some(max_tokens),
            cap_instead_of_reject: false,
        }
    }

    /// Set whether to cap tokens instead of rejecting.
    pub fn with_cap_mode(mut self, cap: bool) -> Self {
        self.cap_instead_of_reject = cap;
        self
    }
}

impl FeatureTransformer for TokenLimitTransformer {
    fn transform_request(&self, request: &mut UnifiedRequest) -> Result<()> {
        if let Some(limit) = self.max_tokens {
            if let Some(requested) = request.parameters.max_tokens {
                if requested > limit as i32 {
                    if self.cap_instead_of_reject {
                        // Cap to the limit
                        request.parameters.max_tokens = Some(limit as i32);
                        tracing::debug!(
                            "Capped max_tokens from {} to {} for request {}",
                            requested,
                            limit,
                            request.request_id
                        );
                    } else {
                        // Reject the request
                        return Err(AppError::BadRequest(format!(
                            "max_tokens {} exceeds limit {}",
                            requested, limit
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn transform_response(&self, _response: &mut UnifiedResponse) -> Result<()> {
        // Response transformation is a no-op for token limits
        Ok(())
    }

    fn transform_stream_chunk(&self, _chunk: &mut UnifiedStreamChunk) -> Result<()> {
        // Stream chunk transformation is a no-op for token limits
        Ok(())
    }

    fn name(&self) -> &'static str {
        "token_limit"
    }
}

// ============================================================================
// Feature Transformer Chain
// ============================================================================

/// A chain of feature transformers applied in sequence.
///
/// This allows composing multiple feature transformers together.
///
/// # Example
///
/// ```ignore
/// use llm_proxy_rust::transformer::features::{
///     FeatureTransformerChain, ReasoningTransformer, TokenLimitTransformer
/// };
///
/// let chain = FeatureTransformerChain::new()
///     .add_transformer(ReasoningTransformer::new(false))
///     .add_transformer(TokenLimitTransformer::new(Some(4096)));
/// ```
#[derive(Default)]
pub struct FeatureTransformerChain {
    transformers: Vec<Box<dyn FeatureTransformer>>,
}

impl FeatureTransformerChain {
    /// Create a new empty transformer chain.
    pub fn new() -> Self {
        Self {
            transformers: Vec::new(),
        }
    }

    /// Add a transformer to the chain.
    pub fn add_transformer<T: FeatureTransformer + 'static>(mut self, transformer: T) -> Self {
        self.transformers.push(Box::new(transformer));
        self
    }

    /// Add a boxed transformer to the chain.
    pub fn add_boxed(mut self, transformer: Box<dyn FeatureTransformer>) -> Self {
        self.transformers.push(transformer);
        self
    }

    /// Check if the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.transformers.is_empty()
    }

    /// Get the number of transformers in the chain.
    pub fn len(&self) -> usize {
        self.transformers.len()
    }

    /// Get the names of all transformers in the chain.
    pub fn names(&self) -> Vec<&'static str> {
        self.transformers.iter().map(|t| t.name()).collect()
    }
}

impl FeatureTransformer for FeatureTransformerChain {
    fn transform_request(&self, request: &mut UnifiedRequest) -> Result<()> {
        for transformer in &self.transformers {
            transformer.transform_request(request)?;
        }
        Ok(())
    }

    fn transform_response(&self, response: &mut UnifiedResponse) -> Result<()> {
        for transformer in &self.transformers {
            transformer.transform_response(response)?;
        }
        Ok(())
    }

    fn transform_stream_chunk(&self, chunk: &mut UnifiedStreamChunk) -> Result<()> {
        for transformer in &self.transformers {
            transformer.transform_stream_chunk(chunk)?;
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "chain"
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transformer::unified::{ChunkType, UnifiedMessage, UnifiedUsage};

    // -------------------------------------------------------------------------
    // ReasoningTransformer Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_reasoning_transformer_include_thinking() {
        let transformer = ReasoningTransformer::new(true);
        assert_eq!(transformer.name(), "reasoning");

        let mut response = UnifiedResponse::new(
            "msg_123",
            "claude-3",
            vec![
                UnifiedContent::thinking("Let me think...", None),
                UnifiedContent::text("The answer is 42."),
            ],
            None,
            UnifiedUsage::new(10, 20),
        );

        transformer.transform_response(&mut response).unwrap();

        // Thinking block should be preserved
        assert_eq!(response.content.len(), 2);
        assert!(matches!(
            &response.content[0],
            UnifiedContent::Thinking { .. }
        ));
    }

    #[test]
    fn test_reasoning_transformer_strip_thinking() {
        let transformer = ReasoningTransformer::new(false);

        let mut response = UnifiedResponse::new(
            "msg_123",
            "claude-3",
            vec![
                UnifiedContent::thinking("Let me think...", None),
                UnifiedContent::text("The answer is 42."),
            ],
            None,
            UnifiedUsage::new(10, 20),
        );

        transformer.transform_response(&mut response).unwrap();

        // Thinking block should be removed
        assert_eq!(response.content.len(), 1);
        assert!(matches!(&response.content[0], UnifiedContent::Text { .. }));
    }

    #[test]
    fn test_reasoning_transformer_stream_chunk_include() {
        let transformer = ReasoningTransformer::new(true);

        let mut chunk = UnifiedStreamChunk {
            chunk_type: ChunkType::ContentBlockDelta,
            index: 0,
            delta: Some(UnifiedContent::thinking("thinking...", None)),
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
        };

        transformer.transform_stream_chunk(&mut chunk).unwrap();

        // Delta should be preserved
        assert!(chunk.delta.is_some());
    }

    #[test]
    fn test_reasoning_transformer_stream_chunk_strip() {
        let transformer = ReasoningTransformer::new(false);

        let mut chunk = UnifiedStreamChunk {
            chunk_type: ChunkType::ContentBlockDelta,
            index: 0,
            delta: Some(UnifiedContent::thinking("thinking...", None)),
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
        };

        transformer.transform_stream_chunk(&mut chunk).unwrap();

        // Delta should be removed
        assert!(chunk.delta.is_none());
    }

    #[test]
    fn test_reasoning_transformer_request_passthrough() {
        let transformer = ReasoningTransformer::new(false);

        let mut request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")]);

        // Should not error
        transformer.transform_request(&mut request).unwrap();
    }

    // -------------------------------------------------------------------------
    // TokenLimitTransformer Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_token_limit_transformer_no_limit() {
        let transformer = TokenLimitTransformer::new(None);
        assert_eq!(transformer.name(), "token_limit");

        let mut request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")]);
        request.parameters.max_tokens = Some(10000);

        transformer.transform_request(&mut request).unwrap();

        // Should not be modified
        assert_eq!(request.parameters.max_tokens, Some(10000));
    }

    #[test]
    fn test_token_limit_transformer_under_limit() {
        let transformer = TokenLimitTransformer::new(Some(4096));

        let mut request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")]);
        request.parameters.max_tokens = Some(1000);

        transformer.transform_request(&mut request).unwrap();

        // Should not be modified
        assert_eq!(request.parameters.max_tokens, Some(1000));
    }

    #[test]
    fn test_token_limit_transformer_cap_mode() {
        let transformer = TokenLimitTransformer::new(Some(4096));

        let mut request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")]);
        request.parameters.max_tokens = Some(10000);

        transformer.transform_request(&mut request).unwrap();

        // Should be capped to limit
        assert_eq!(request.parameters.max_tokens, Some(4096));
    }

    #[test]
    fn test_token_limit_transformer_strict_mode() {
        let transformer = TokenLimitTransformer::new_strict(4096);

        let mut request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")]);
        request.parameters.max_tokens = Some(10000);

        let result = transformer.transform_request(&mut request);

        // Should return error
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn test_token_limit_transformer_no_max_tokens_in_request() {
        let transformer = TokenLimitTransformer::new(Some(4096));

        let mut request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")]);
        // No max_tokens set

        transformer.transform_request(&mut request).unwrap();

        // Should remain None
        assert!(request.parameters.max_tokens.is_none());
    }

    #[test]
    fn test_token_limit_transformer_response_passthrough() {
        let transformer = TokenLimitTransformer::new(Some(4096));

        let mut response =
            UnifiedResponse::text("msg_123", "gpt-4", "Hello!", UnifiedUsage::new(10, 5));

        // Should not error
        transformer.transform_response(&mut response).unwrap();
    }

    // -------------------------------------------------------------------------
    // FeatureTransformerChain Tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_chain_empty() {
        let chain = FeatureTransformerChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
        assert_eq!(chain.name(), "chain");
    }

    #[test]
    fn test_chain_add_transformers() {
        let chain = FeatureTransformerChain::new()
            .add_transformer(ReasoningTransformer::new(false))
            .add_transformer(TokenLimitTransformer::new(Some(4096)));

        assert!(!chain.is_empty());
        assert_eq!(chain.len(), 2);
        assert_eq!(chain.names(), vec!["reasoning", "token_limit"]);
    }

    #[test]
    fn test_chain_transform_request() {
        let chain =
            FeatureTransformerChain::new().add_transformer(TokenLimitTransformer::new(Some(4096)));

        let mut request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")]);
        request.parameters.max_tokens = Some(10000);

        chain.transform_request(&mut request).unwrap();

        // Token limit should be applied
        assert_eq!(request.parameters.max_tokens, Some(4096));
    }

    #[test]
    fn test_chain_transform_response() {
        let chain =
            FeatureTransformerChain::new().add_transformer(ReasoningTransformer::new(false));

        let mut response = UnifiedResponse::new(
            "msg_123",
            "claude-3",
            vec![
                UnifiedContent::thinking("thinking...", None),
                UnifiedContent::text("answer"),
            ],
            None,
            UnifiedUsage::new(10, 20),
        );

        chain.transform_response(&mut response).unwrap();

        // Thinking should be stripped
        assert_eq!(response.content.len(), 1);
    }

    #[test]
    fn test_chain_multiple_transformers_order() {
        // Create a chain with both transformers
        let chain = FeatureTransformerChain::new()
            .add_transformer(ReasoningTransformer::new(false))
            .add_transformer(TokenLimitTransformer::new(Some(4096)));

        // Test request transformation
        let mut request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")]);
        request.parameters.max_tokens = Some(10000);

        chain.transform_request(&mut request).unwrap();
        assert_eq!(request.parameters.max_tokens, Some(4096));

        // Test response transformation
        let mut response = UnifiedResponse::new(
            "msg_123",
            "claude-3",
            vec![
                UnifiedContent::thinking("thinking...", None),
                UnifiedContent::text("answer"),
            ],
            None,
            UnifiedUsage::new(10, 20),
        );

        chain.transform_response(&mut response).unwrap();
        assert_eq!(response.content.len(), 1);
    }

    #[test]
    fn test_chain_add_boxed() {
        let boxed: Box<dyn FeatureTransformer> = Box::new(ReasoningTransformer::new(false));
        let chain = FeatureTransformerChain::new().add_boxed(boxed);

        assert_eq!(chain.len(), 1);
    }

    #[test]
    fn test_token_limit_with_cap_mode_builder() {
        let transformer = TokenLimitTransformer::new(Some(4096)).with_cap_mode(false);

        let mut request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")]);
        request.parameters.max_tokens = Some(10000);

        let result = transformer.transform_request(&mut request);
        assert!(result.is_err());
    }
}
