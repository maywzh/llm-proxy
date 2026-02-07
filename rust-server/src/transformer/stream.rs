//! Streaming utilities for protocol conversion.
//!
//! This module provides utilities for handling SSE (Server-Sent Events) streams
//! and converting between different streaming formats.

use super::UnifiedStreamChunk;
use crate::core::OutboundTokenCounter;

// ============================================================================
// SSE Parser
// ============================================================================

/// SSE event parsed from stream.
#[derive(Debug, Clone, Default)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: Option<String>,
    pub id: Option<String>,
    pub retry: Option<u64>,
}

/// SSE parser state.
pub struct SseParser {
    buffer: String,
}

impl SseParser {
    /// Create a new SSE parser.
    pub fn new() -> Self {
        SseParser {
            buffer: String::new(),
        }
    }

    /// Parse incoming bytes and return complete events.
    pub fn parse(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        let chunk_str = match std::str::from_utf8(chunk) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        self.buffer.push_str(chunk_str);

        let mut events = vec![];
        let mut current_event = SseEvent::default();

        // Split by double newlines (event boundaries)
        while let Some(pos) = self.buffer.find("\n\n") {
            let event_block = self.buffer[..pos].to_string();
            self.buffer = self.buffer[pos + 2..].to_string();

            for line in event_block.lines() {
                if line.is_empty() {
                    continue;
                }

                if line.starts_with(':') {
                    // Comment, ignore
                    continue;
                }

                if let Some((field, value)) = line.split_once(':') {
                    let value = value.strip_prefix(' ').unwrap_or(value);
                    match field {
                        "event" => current_event.event = Some(value.to_string()),
                        "data" => {
                            if let Some(ref mut data) = current_event.data {
                                data.push('\n');
                                data.push_str(value);
                            } else {
                                current_event.data = Some(value.to_string());
                            }
                        }
                        "id" => current_event.id = Some(value.to_string()),
                        "retry" => current_event.retry = value.parse().ok(),
                        _ => {}
                    }
                } else if let Some(value) = line.strip_prefix("data:") {
                    let value = value.strip_prefix(' ').unwrap_or(value);
                    if let Some(ref mut data) = current_event.data {
                        data.push('\n');
                        data.push_str(value);
                    } else {
                        current_event.data = Some(value.to_string());
                    }
                }
            }

            if current_event.data.is_some() || current_event.event.is_some() {
                events.push(current_event);
                current_event = SseEvent::default();
            }
        }

        events
    }

    /// Get remaining buffer content.
    pub fn remaining(&self) -> &str {
        &self.buffer
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

impl Default for SseParser {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SSE Serializer
// ============================================================================

/// Format an SSE event for transmission.
pub fn format_sse_event(event: Option<&str>, data: &str) -> String {
    let mut output = String::new();

    if let Some(event_name) = event {
        output.push_str("event: ");
        output.push_str(event_name);
        output.push('\n');
    }

    for line in data.lines() {
        output.push_str("data: ");
        output.push_str(line);
        output.push('\n');
    }

    output.push('\n');
    output
}

/// Format a simple data-only SSE event.
pub fn format_sse_data(data: &str) -> String {
    format!("data: {}\n\n", data)
}

/// Format the SSE done marker.
pub fn format_sse_done() -> String {
    "data: [DONE]\n\n".to_string()
}

// ============================================================================
// Cross-Protocol Stream State
// ============================================================================

/// Cached tool information for synthesizing content_block_start.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    /// Tool call ID
    pub id: String,
    /// Tool name
    pub name: String,
}

/// State tracker for cross-protocol streaming transformation.
///
/// When transforming streams between protocols (e.g., OpenAI → Anthropic),
/// we need to track state to properly emit all required events in the target
/// protocol's format.
///
/// For example, Anthropic requires:
/// - `message_start` at the beginning
/// - `content_block_start` before each content block
/// - `content_block_stop` after each content block
/// - `message_delta` with stop_reason
/// - `message_stop` at the end
///
/// OpenAI doesn't have these events, so we need to synthesize them.
#[derive(Debug, Clone)]
pub struct CrossProtocolStreamState {
    /// Whether message_start has been emitted
    pub message_started: bool,
    /// Whether ping has been emitted after message_start
    pub ping_emitted: bool,
    /// Current content block index
    pub current_block_index: usize,
    /// Set of content block indices that have been started
    pub started_blocks: std::collections::HashSet<usize>,
    /// Set of content block indices that have been stopped
    pub stopped_blocks: std::collections::HashSet<usize>,
    /// Whether message_delta with stop_reason has been emitted
    pub message_delta_emitted: bool,
    /// Whether message_stop has been emitted
    pub message_stopped: bool,
    /// Model name for synthetic events
    pub model: String,
    /// Message ID for synthetic events
    pub message_id: String,
    /// Token counter for outbound usage calculation
    token_counter: OutboundTokenCounter,
    /// Accumulated stop_reason for final message_delta
    pub stop_reason: Option<super::StopReason>,
    /// Provider-reported input_tokens from message_start event
    /// (Anthropic sends input_tokens in message_start, not message_delta)
    pub provider_input_tokens: Option<i32>,
    /// Cached tool information for synthesizing content_block_start
    pub tool_info_cache: std::collections::HashMap<usize, ToolInfo>,
}

impl Default for CrossProtocolStreamState {
    fn default() -> Self {
        Self {
            message_started: false,
            ping_emitted: false,
            current_block_index: 0,
            started_blocks: std::collections::HashSet::new(),
            stopped_blocks: std::collections::HashSet::new(),
            message_delta_emitted: false,
            message_stopped: false,
            model: String::new(),
            message_id: format!(
                "msg_{}",
                &uuid::Uuid::new_v4().to_string().replace("-", "")[..24]
            ),
            token_counter: OutboundTokenCounter::new_lazy(""),
            stop_reason: None,
            provider_input_tokens: None,
            tool_info_cache: std::collections::HashMap::new(),
        }
    }
}

impl CrossProtocolStreamState {
    /// Create a new stream state with model name.
    pub fn new(model: impl Into<String>) -> Self {
        let model_str = model.into();
        Self {
            model: model_str.clone(),
            token_counter: OutboundTokenCounter::new_lazy(&model_str),
            ..Default::default()
        }
    }

    /// Create a new stream state with model name and input tokens.
    ///
    /// Pre-calculates input tokens for usage tracking in fallback scenarios
    /// when the provider doesn't return usage information.
    pub fn with_input_tokens(model: impl Into<String>, input_tokens: Option<usize>) -> Self {
        let model_str = model.into();
        let input = input_tokens.unwrap_or(0) as i32;

        Self {
            model: model_str.clone(),
            token_counter: OutboundTokenCounter::new(&model_str, input),
            ..Default::default()
        }
    }

    /// Accumulate output tokens from chunk text.
    ///
    /// This method counts tokens in generated text and adds them to the usage.
    /// Used for fallback usage calculation when provider doesn't provide usage.
    pub fn accumulate_output_tokens(&mut self, text: &str) {
        self.token_counter.accumulate_content(text);
    }

    /// Get final usage, prioritizing non-zero provider usage over calculated usage.
    ///
    /// Returns provider usage if available and non-zero, otherwise returns accumulated usage.
    /// This handles the case where provider sends zero usage (e.g., OpenAI streaming
    /// where usage comes in a separate chunk or not at all).
    pub fn get_final_usage(
        &self,
        provider_usage: Option<super::UnifiedUsage>,
    ) -> Option<super::UnifiedUsage> {
        // Update token counter with provider usage if available
        if let Some(ref usage) = provider_usage {
            if usage.input_tokens > 0 || usage.output_tokens > 0 {
                return provider_usage;
            }
        }
        // Use token counter's finalize for fallback calculation
        Some(self.token_counter.finalize())
    }

    /// Get a reference to the accumulated usage from token counter.
    /// This is for backward compatibility with tests that access `state.usage`.
    pub fn usage(&self) -> Option<super::UnifiedUsage> {
        Some(self.token_counter.finalize())
    }

    /// Process unified chunks and emit additional synthetic events as needed.
    ///
    /// This method takes unified chunks from the source protocol and returns
    /// a complete sequence of chunks that includes all events required by
    /// the target protocol.
    pub fn process_chunks(&mut self, chunks: Vec<UnifiedStreamChunk>) -> Vec<UnifiedStreamChunk> {
        let mut result = Vec::new();

        for chunk in chunks {
            // Cache tool information from content blocks for later use
            self.cache_tool_info(&chunk);

            // Emit message_start if not yet emitted and we have content
            if !self.message_started && self.should_emit_message_start(&chunk) {
                result.push(self.create_message_start());
                self.message_started = true;
                // Emit ping event after message_start for Anthropic compatibility
                if !self.ping_emitted {
                    result.push(UnifiedStreamChunk::ping());
                    self.ping_emitted = true;
                }
            }

            match chunk.chunk_type {
                super::ChunkType::MessageStart => {
                    // Already have a message_start from source, use it
                    if !self.message_started {
                        if let Some(ref msg) = chunk.message {
                            self.model = msg.model.clone();
                            self.message_id = msg.id.clone();
                            // Capture input_tokens from message_start usage
                            // (Anthropic sends input_tokens here, not in message_delta)
                            if msg.usage.input_tokens > 0 {
                                self.provider_input_tokens = Some(msg.usage.input_tokens);
                            }
                        }
                        self.message_started = true;
                        result.push(chunk);
                        // Emit ping event after message_start for Anthropic compatibility
                        if !self.ping_emitted {
                            result.push(UnifiedStreamChunk::ping());
                            self.ping_emitted = true;
                        }
                    }
                }
                super::ChunkType::ContentBlockStart => {
                    let index = chunk.index;
                    if !self.started_blocks.contains(&index) {
                        self.started_blocks.insert(index);
                        self.current_block_index = index;
                    }
                    result.push(chunk);
                }
                super::ChunkType::ContentBlockDelta => {
                    let index = chunk.index;

                    // Emit content_block_start if not yet emitted for this index
                    if !self.started_blocks.contains(&index) {
                        result.push(self.create_content_block_start(index, &chunk));
                        self.started_blocks.insert(index);
                        self.current_block_index = index;
                    }

                    // Accumulate output tokens from content
                    if let Some(ref delta) = chunk.delta {
                        match delta {
                            // Text content block
                            super::UnifiedContent::Text { text } => {
                                self.accumulate_output_tokens(text);
                            }
                            // Tool input delta - accumulate the partial JSON for token counting
                            // When LLM returns tool_calls, the arguments are streamed as partial JSON
                            super::UnifiedContent::ToolInputDelta { partial_json, .. } => {
                                if !partial_json.is_empty() {
                                    self.accumulate_output_tokens(partial_json);
                                }
                            }
                            // Thinking content - also accumulate for token counting
                            super::UnifiedContent::Thinking { text, .. } => {
                                self.accumulate_output_tokens(text);
                            }
                            _ => {}
                        }
                    }

                    result.push(chunk);
                }
                super::ChunkType::ContentBlockStop => {
                    let index = chunk.index;
                    if !self.stopped_blocks.contains(&index) {
                        self.stopped_blocks.insert(index);
                    }
                    result.push(chunk);
                }
                super::ChunkType::MessageDelta => {
                    // Close any open content blocks before message_delta
                    for idx in self.started_blocks.iter() {
                        if !self.stopped_blocks.contains(idx) {
                            result.push(UnifiedStreamChunk::content_block_stop(*idx));
                            self.stopped_blocks.insert(*idx);
                        }
                    }

                    // DEBUG: Log usage information before get_final_usage
                    tracing::debug!(
                        chunk_usage = ?chunk.usage,
                        accumulated_usage = ?self.usage(),
                        "MessageDelta: before get_final_usage"
                    );

                    // Get final usage (prioritizing provider usage)
                    let mut final_usage = self.get_final_usage(chunk.usage.clone());

                    // Merge input_tokens from message_start if final usage has zero input_tokens
                    // (Anthropic sends input_tokens in message_start, output_tokens in message_delta)
                    if let (Some(ref mut usage), Some(input_tokens)) =
                        (&mut final_usage, self.provider_input_tokens)
                    {
                        if usage.input_tokens == 0 {
                            usage.input_tokens = input_tokens;
                        }
                    }

                    // DEBUG: Log final usage after get_final_usage
                    tracing::debug!(
                        final_usage = ?final_usage,
                        "MessageDelta: after get_final_usage"
                    );

                    // Create a new chunk with final usage
                    let mut output_chunk = chunk.clone();
                    output_chunk.usage = final_usage;

                    self.message_delta_emitted = true;
                    result.push(output_chunk);
                }
                super::ChunkType::MessageStop => {
                    // Ensure all content blocks are closed
                    for idx in self.started_blocks.clone().iter() {
                        if !self.stopped_blocks.contains(idx) {
                            result.push(UnifiedStreamChunk::content_block_stop(*idx));
                            self.stopped_blocks.insert(*idx);
                        }
                    }

                    self.message_stopped = true;
                    result.push(chunk);
                }
                super::ChunkType::Ping => {
                    result.push(chunk);
                }
            }
        }

        result
    }

    /// Cache tool information from content blocks for later synthesizing.
    fn cache_tool_info(&mut self, chunk: &UnifiedStreamChunk) {
        // Cache tool info from ContentBlockStart
        if chunk.chunk_type == super::ChunkType::ContentBlockStart {
            if let Some(super::UnifiedContent::ToolUse { id, name, .. }) =
                chunk.content_block.as_ref()
            {
                self.tool_info_cache.insert(
                    chunk.index,
                    ToolInfo {
                        id: id.clone(),
                        name: name.clone(),
                    },
                );
            }
        }

        // Also cache from ToolInputDelta if it contains tool info
        if chunk.chunk_type == super::ChunkType::ContentBlockDelta {
            if let Some(super::UnifiedContent::ToolInputDelta {
                index: tool_idx, ..
            }) = chunk.delta.as_ref()
            {
                // If we have a tool index, it might help correlate tool calls
                // Store the index mapping for potential future use
                if *tool_idx > 0 && !self.tool_info_cache.contains_key(&chunk.index) {
                    // We don't have full info yet, but mark that this index is a tool
                    // The actual info might come from a previous ContentBlockStart
                }
            }
        }
    }

    /// Check if we should emit a synthetic message_start.
    fn should_emit_message_start(&self, chunk: &UnifiedStreamChunk) -> bool {
        matches!(
            chunk.chunk_type,
            super::ChunkType::ContentBlockStart
                | super::ChunkType::ContentBlockDelta
                | super::ChunkType::MessageDelta
        )
    }

    /// Create a synthetic message_start event.
    fn create_message_start(&self) -> UnifiedStreamChunk {
        let message = super::UnifiedResponse {
            id: self.message_id.clone(),
            model: self.model.clone(),
            content: vec![],
            stop_reason: None,
            usage: super::UnifiedUsage::default(),
            tool_calls: vec![],
        };
        UnifiedStreamChunk::message_start(message)
    }

    /// Create a synthetic content_block_start event.
    fn create_content_block_start(
        &self,
        index: usize,
        delta_chunk: &UnifiedStreamChunk,
    ) -> UnifiedStreamChunk {
        // First, try to use cached tool info for this index
        if let Some(tool_info) = self.tool_info_cache.get(&index) {
            return UnifiedStreamChunk::content_block_start(
                index,
                super::UnifiedContent::tool_use(
                    &tool_info.id,
                    &tool_info.name,
                    serde_json::json!({}),
                ),
            );
        }

        // Determine content type from the delta
        let content_block = if let Some(ref delta) = delta_chunk.delta {
            match delta {
                super::UnifiedContent::Text { .. } => super::UnifiedContent::text(""),
                super::UnifiedContent::ToolInputDelta { .. } => {
                    // For tool input delta without cached info, generate placeholder
                    // This is a fallback when we didn't receive a ContentBlockStart
                    super::UnifiedContent::tool_use(
                        format!(
                            "toolu_{}",
                            &uuid::Uuid::new_v4().to_string().replace("-", "")[..24]
                        ),
                        "unknown_tool",
                        serde_json::json!({}),
                    )
                }
                super::UnifiedContent::Thinking { .. } => super::UnifiedContent::thinking("", None),
                _ => super::UnifiedContent::text(""),
            }
        } else {
            super::UnifiedContent::text("")
        };

        UnifiedStreamChunk::content_block_start(index, content_block)
    }

    /// Finalize the stream, emitting any missing closing events.
    pub fn finalize(&mut self) -> Vec<UnifiedStreamChunk> {
        let mut result = Vec::new();

        // DEBUG: Log finalize state
        tracing::debug!(
            message_started = self.message_started,
            message_delta_emitted = self.message_delta_emitted,
            message_stopped = self.message_stopped,
            accumulated_usage = ?self.usage(),
            "finalize() called"
        );

        // Close any open content blocks
        for idx in self.started_blocks.clone().iter() {
            if !self.stopped_blocks.contains(idx) {
                result.push(UnifiedStreamChunk::content_block_stop(*idx));
                self.stopped_blocks.insert(*idx);
            }
        }

        // Emit message_delta if not yet emitted
        if !self.message_delta_emitted && self.message_started {
            let mut usage = self.token_counter.finalize();
            // Merge input_tokens from message_start if needed
            if usage.input_tokens == 0 {
                if let Some(input_tokens) = self.provider_input_tokens {
                    usage.input_tokens = input_tokens;
                }
            }
            tracing::debug!(
                usage = ?usage,
                "finalize(): emitting message_delta with usage"
            );
            result.push(UnifiedStreamChunk::message_delta(
                super::StopReason::EndTurn,
                usage,
            ));
            self.message_delta_emitted = true;
        }

        // Emit message_stop if not yet emitted
        if !self.message_stopped && self.message_started {
            result.push(UnifiedStreamChunk::message_stop());
            self.message_stopped = true;
        }

        tracing::debug!(result_len = result.len(), "finalize(): returning chunks");

        result
    }
}

// ============================================================================
// Chunk Accumulator
// ============================================================================

/// Accumulates streaming chunks for final response assembly.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct ChunkAccumulator {
    content: Vec<String>,
    tool_calls: Vec<serde_json::Value>,
    usage: Option<super::UnifiedUsage>,
    stop_reason: Option<super::StopReason>,
    message_id: Option<String>,
    model: Option<String>,
}

impl ChunkAccumulator {
    /// Create a new chunk accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a unified stream chunk.
    pub fn add_chunk(&mut self, chunk: &UnifiedStreamChunk) {
        match chunk.chunk_type {
            super::ChunkType::MessageStart => {
                if let Some(ref message) = chunk.message {
                    self.message_id = Some(message.id.clone());
                    self.model = Some(message.model.clone());
                }
            }
            super::ChunkType::ContentBlockDelta => {
                if let Some(ref delta) = chunk.delta {
                    if let Some(text) = delta.as_text() {
                        self.content.push(text.to_string());
                    }
                }
            }
            super::ChunkType::MessageDelta => {
                if let Some(ref usage) = chunk.usage {
                    self.usage = Some(usage.clone());
                }
                if let Some(ref reason) = chunk.stop_reason {
                    self.stop_reason = Some(reason.clone());
                }
            }
            _ => {}
        }
    }

    /// Get accumulated text content.
    pub fn text_content(&self) -> String {
        self.content.join("")
    }

    /// Get usage statistics.
    pub fn usage(&self) -> Option<&super::UnifiedUsage> {
        self.usage.as_ref()
    }

    /// Get stop reason.
    pub fn stop_reason(&self) -> Option<&super::StopReason> {
        self.stop_reason.as_ref()
    }

    /// Get message ID.
    pub fn message_id(&self) -> Option<&str> {
        self.message_id.as_deref()
    }

    /// Get model name.
    pub fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// Build a unified response from accumulated chunks.
    pub fn build_response(&self) -> super::UnifiedResponse {
        super::UnifiedResponse {
            id: self
                .message_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            model: self.model.clone().unwrap_or_else(|| "unknown".to_string()),
            content: vec![super::UnifiedContent::text(self.text_content())],
            stop_reason: self.stop_reason.clone(),
            usage: self.usage.clone().unwrap_or_default(),
            tool_calls: vec![],
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
    fn test_sse_parser_simple() {
        let mut parser = SseParser::new();
        let events = parser.parse(b"data: hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, Some("hello".to_string()));
    }

    #[test]
    fn test_sse_parser_with_event() {
        let mut parser = SseParser::new();
        let events = parser.parse(b"event: message\ndata: hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, Some("message".to_string()));
        assert_eq!(events[0].data, Some("hello".to_string()));
    }

    #[test]
    fn test_sse_parser_multiline_data() {
        let mut parser = SseParser::new();
        let events = parser.parse(b"data: line1\ndata: line2\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, Some("line1\nline2".to_string()));
    }

    #[test]
    fn test_sse_parser_multiple_events() {
        let mut parser = SseParser::new();
        let events = parser.parse(b"data: first\n\ndata: second\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, Some("first".to_string()));
        assert_eq!(events[1].data, Some("second".to_string()));
    }

    #[test]
    fn test_sse_parser_partial() {
        let mut parser = SseParser::new();

        // First chunk - incomplete
        let events = parser.parse(b"data: hel");
        assert_eq!(events.len(), 0);

        // Second chunk - completes the event
        let events = parser.parse(b"lo\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, Some("hello".to_string()));
    }

    #[test]
    fn test_sse_parser_comment() {
        let mut parser = SseParser::new();
        let events = parser.parse(b": comment\ndata: hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, Some("hello".to_string()));
    }

    #[test]
    fn test_format_sse_event() {
        let output = format_sse_event(Some("message"), "hello");
        assert_eq!(output, "event: message\ndata: hello\n\n");
    }

    #[test]
    fn test_format_sse_data() {
        let output = format_sse_data("hello");
        assert_eq!(output, "data: hello\n\n");
    }

    #[test]
    fn test_format_sse_done() {
        let output = format_sse_done();
        assert_eq!(output, "data: [DONE]\n\n");
    }

    #[test]
    fn test_chunk_accumulator() {
        use super::super::{
            ChunkType, StopReason, UnifiedContent, UnifiedStreamChunk, UnifiedUsage,
        };

        let mut acc = ChunkAccumulator::new();

        // Add text deltas
        acc.add_chunk(&UnifiedStreamChunk {
            chunk_type: ChunkType::ContentBlockDelta,
            index: 0,
            delta: Some(UnifiedContent::text("Hello")),
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
        });

        acc.add_chunk(&UnifiedStreamChunk {
            chunk_type: ChunkType::ContentBlockDelta,
            index: 0,
            delta: Some(UnifiedContent::text(" World")),
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
        });

        // Add message delta with usage
        acc.add_chunk(&UnifiedStreamChunk {
            chunk_type: ChunkType::MessageDelta,
            index: 0,
            delta: None,
            usage: Some(UnifiedUsage::new(10, 5)),
            stop_reason: Some(StopReason::EndTurn),
            message: None,
            content_block: None,
        });

        assert_eq!(acc.text_content(), "Hello World");
        assert_eq!(acc.usage().unwrap().input_tokens, 10);
        assert_eq!(acc.stop_reason(), Some(&StopReason::EndTurn));
    }

    // =========================================================================
    // CrossProtocolStreamState Tests
    // =========================================================================

    #[test]
    fn test_cross_protocol_stream_state_new() {
        let state = CrossProtocolStreamState::new("gpt-4");
        assert_eq!(state.model, "gpt-4");
        assert!(!state.message_started);
        assert!(!state.ping_emitted);
        assert!(!state.message_delta_emitted);
        assert!(!state.message_stopped);
        assert!(state.started_blocks.is_empty());
        assert!(state.stopped_blocks.is_empty());
    }

    #[test]
    fn test_cross_protocol_stream_state_emits_message_start() {
        use super::super::{ChunkType, UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::new("gpt-4");

        // Process a content delta without message_start
        let chunks = vec![UnifiedStreamChunk::content_block_delta(
            0,
            UnifiedContent::text("Hello"),
        )];

        let result = state.process_chunks(chunks);

        // Should emit message_start + ping before the delta
        assert_eq!(result.len(), 4); // message_start + ping + content_block_start + delta
        assert_eq!(result[0].chunk_type, ChunkType::MessageStart);
        assert_eq!(result[1].chunk_type, ChunkType::Ping);
        assert_eq!(result[2].chunk_type, ChunkType::ContentBlockStart);
        assert_eq!(result[3].chunk_type, ChunkType::ContentBlockDelta);
        assert!(state.message_started);
        assert!(state.ping_emitted);
    }

    #[test]
    fn test_cross_protocol_stream_state_emits_content_block_start() {
        use super::super::{ChunkType, UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::new("gpt-4");
        state.message_started = true; // Simulate message already started

        // Process a content delta without content_block_start
        let chunks = vec![UnifiedStreamChunk::content_block_delta(
            0,
            UnifiedContent::text("Hello"),
        )];

        let result = state.process_chunks(chunks);

        // Should emit content_block_start before the delta
        assert_eq!(result.len(), 2); // content_block_start + delta
        assert_eq!(result[0].chunk_type, ChunkType::ContentBlockStart);
        assert_eq!(result[1].chunk_type, ChunkType::ContentBlockDelta);
        assert!(state.started_blocks.contains(&0));
    }

    #[test]
    fn test_cross_protocol_stream_state_emits_content_block_stop() {
        use super::super::{ChunkType, StopReason, UnifiedStreamChunk, UnifiedUsage};

        let mut state = CrossProtocolStreamState::new("gpt-4");
        state.message_started = true;
        state.started_blocks.insert(0);

        // Process a message_delta (should close open content blocks)
        let chunks = vec![UnifiedStreamChunk::message_delta(
            StopReason::EndTurn,
            UnifiedUsage::new(10, 5),
        )];

        let result = state.process_chunks(chunks);

        // Should emit content_block_stop before message_delta
        assert_eq!(result.len(), 2); // content_block_stop + message_delta
        assert_eq!(result[0].chunk_type, ChunkType::ContentBlockStop);
        assert_eq!(result[0].index, 0);
        assert_eq!(result[1].chunk_type, ChunkType::MessageDelta);
        assert!(state.stopped_blocks.contains(&0));
    }

    #[test]
    fn test_cross_protocol_stream_state_full_sequence() {
        use super::super::{
            ChunkType, StopReason, UnifiedContent, UnifiedStreamChunk, UnifiedUsage,
        };

        let mut state = CrossProtocolStreamState::new("gpt-4");

        // Simulate OpenAI streaming sequence (no message_start, no content_block_start/stop)
        let chunks = vec![
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("Hello")),
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text(" World")),
            UnifiedStreamChunk::message_delta(StopReason::EndTurn, UnifiedUsage::new(10, 5)),
            UnifiedStreamChunk::message_stop(),
        ];

        let result = state.process_chunks(chunks);

        // Verify the complete Anthropic sequence (with ping after message_start)
        let chunk_types: Vec<ChunkType> = result.iter().map(|c| c.chunk_type.clone()).collect();

        assert_eq!(chunk_types[0], ChunkType::MessageStart);
        assert_eq!(chunk_types[1], ChunkType::Ping);
        assert_eq!(chunk_types[2], ChunkType::ContentBlockStart);
        assert_eq!(chunk_types[3], ChunkType::ContentBlockDelta);
        assert_eq!(chunk_types[4], ChunkType::ContentBlockDelta);
        assert_eq!(chunk_types[5], ChunkType::ContentBlockStop);
        assert_eq!(chunk_types[6], ChunkType::MessageDelta);
        assert_eq!(chunk_types[7], ChunkType::MessageStop);
    }

    #[test]
    fn test_cross_protocol_stream_state_tool_use_sequence() {
        use super::super::{
            ChunkType, StopReason, UnifiedContent, UnifiedStreamChunk, UnifiedUsage,
        };

        let mut state = CrossProtocolStreamState::new("gpt-4");

        // Simulate OpenAI tool use streaming
        let chunks = vec![
            // Text content first
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("Let me check")),
            // Tool use starts (OpenAI sends content_block_start for tool_use)
            UnifiedStreamChunk::content_block_start(
                1,
                UnifiedContent::tool_use("call_123", "get_weather", serde_json::json!({})),
            ),
            // Tool input delta
            UnifiedStreamChunk::content_block_delta(
                1,
                UnifiedContent::tool_input_delta(1, "{\"city\":\"NYC\"}"),
            ),
            // Finish
            UnifiedStreamChunk::message_delta(StopReason::ToolUse, UnifiedUsage::new(20, 15)),
            UnifiedStreamChunk::message_stop(),
        ];

        let result = state.process_chunks(chunks);

        // Verify proper sequencing
        let chunk_types: Vec<ChunkType> = result.iter().map(|c| c.chunk_type.clone()).collect();

        // Should have: message_start, ping, content_block_start(0), delta(0), content_block_start(1), delta(1),
        // content_block_stop(0), content_block_stop(1), message_delta, message_stop
        assert!(chunk_types.contains(&ChunkType::MessageStart));
        assert!(chunk_types.contains(&ChunkType::Ping));
        assert_eq!(
            chunk_types
                .iter()
                .filter(|t| **t == ChunkType::ContentBlockStart)
                .count(),
            2
        );
        assert_eq!(
            chunk_types
                .iter()
                .filter(|t| **t == ChunkType::ContentBlockStop)
                .count(),
            2
        );
    }

    #[test]
    fn test_cross_protocol_stream_state_finalize() {
        use super::super::ChunkType;

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(10));
        state.message_started = true;
        state.started_blocks.insert(0);
        // Accumulate some output tokens
        state.accumulate_output_tokens("Hello");

        // Finalize without message_delta or message_stop
        let result = state.finalize();

        // Should emit content_block_stop, message_delta, message_stop
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].chunk_type, ChunkType::ContentBlockStop);
        assert_eq!(result[1].chunk_type, ChunkType::MessageDelta);
        assert_eq!(result[2].chunk_type, ChunkType::MessageStop);
    }

    #[test]
    fn test_cross_protocol_stream_state_preserves_existing_events() {
        use super::super::{ChunkType, UnifiedResponse, UnifiedStreamChunk, UnifiedUsage};

        let mut state = CrossProtocolStreamState::new("gpt-4");

        // If source already has message_start, use it
        let message = UnifiedResponse {
            id: "msg_existing".to_string(),
            model: "claude-3".to_string(),
            content: vec![],
            stop_reason: None,
            usage: UnifiedUsage::default(),
            tool_calls: vec![],
        };
        let chunks = vec![UnifiedStreamChunk::message_start(message)];

        let result = state.process_chunks(chunks);

        // Should emit message_start + ping
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].chunk_type, ChunkType::MessageStart);
        assert_eq!(result[1].chunk_type, ChunkType::Ping);
        assert!(state.message_started);
        assert!(state.ping_emitted);
        assert_eq!(state.message_id, "msg_existing");
        assert_eq!(state.model, "claude-3");
    }

    #[test]
    fn test_cross_protocol_stream_state_sequential_indices() {
        use super::super::{ChunkType, UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::new("gpt-4");
        state.message_started = true;

        // Process deltas with different indices
        let chunks = vec![
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("First")),
            UnifiedStreamChunk::content_block_delta(1, UnifiedContent::text("Second")),
            UnifiedStreamChunk::content_block_delta(2, UnifiedContent::text("Third")),
        ];

        let result = state.process_chunks(chunks);

        // Each delta should have a content_block_start before it
        // Total: 3 content_block_start + 3 delta = 6
        assert_eq!(result.len(), 6);

        // Verify indices are sequential
        let indices: Vec<usize> = result
            .iter()
            .filter(|c| c.chunk_type == ChunkType::ContentBlockStart)
            .map(|c| c.index)
            .collect();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_cross_protocol_stream_state_ping_event() {
        use super::super::{ChunkType, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::new("gpt-4");

        // Process a ping event directly
        let chunks = vec![UnifiedStreamChunk::ping()];
        let result = state.process_chunks(chunks);

        // Ping should pass through
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].chunk_type, ChunkType::Ping);
    }

    #[test]
    fn test_cross_protocol_stream_state_ping_only_emitted_once() {
        use super::super::{ChunkType, UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::new("gpt-4");

        // Process multiple content deltas
        let chunks = vec![
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("Hello")),
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text(" World")),
        ];

        let result = state.process_chunks(chunks);

        // Count ping events - should only be one
        let ping_count = result
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Ping)
            .count();
        assert_eq!(ping_count, 1);
        assert!(state.ping_emitted);
    }

    #[test]
    fn test_unified_stream_chunk_ping() {
        use super::super::{ChunkType, UnifiedStreamChunk};

        let chunk = UnifiedStreamChunk::ping();
        assert_eq!(chunk.chunk_type, ChunkType::Ping);
        assert_eq!(chunk.index, 0);
        assert!(chunk.delta.is_none());
        assert!(chunk.usage.is_none());
        assert!(chunk.stop_reason.is_none());
        assert!(chunk.message.is_none());
        assert!(chunk.content_block.is_none());
    }

    #[test]
    fn test_cross_protocol_stream_state_usage_accumulation() {
        use super::super::{
            ChunkType, StopReason, UnifiedContent, UnifiedStreamChunk, UnifiedUsage,
        };

        // Test with input_tokens pre-calculated for fallback
        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(100));
        state.message_started = true;
        state.started_blocks.insert(0);

        // Simulate content chunks that accumulate output tokens
        let chunks = vec![
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("Hello")),
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text(" world")),
        ];

        state.process_chunks(chunks);

        // Verify tokens were accumulated
        let usage = state.usage();
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert!(
            usage.output_tokens > 0,
            "output_tokens should be accumulated"
        );

        // Now send message_delta without provider usage (should use fallback)
        state.stopped_blocks.insert(0);
        let chunks = vec![UnifiedStreamChunk::message_delta(
            StopReason::EndTurn,
            UnifiedUsage::new(0, 0), // Provider sends zero usage
        )];

        let result = state.process_chunks(chunks);

        // message_delta should use accumulated usage since provider sent zero
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].chunk_type, ChunkType::MessageDelta);
        let chunk_usage = result[0].usage.as_ref().unwrap();
        // Zero provider usage is ignored, accumulated usage is used instead
        assert_eq!(chunk_usage.input_tokens, 100); // From with_input_tokens
        assert!(chunk_usage.output_tokens > 0); // Accumulated from text chunks
    }

    #[test]
    fn test_cross_protocol_stream_state_usage_preserved_across_chunks() {
        use super::super::{
            ChunkType, StopReason, UnifiedContent, UnifiedStreamChunk, UnifiedUsage,
        };

        let mut state = CrossProtocolStreamState::new("gpt-4");

        // Simulate full OpenAI streaming sequence where usage comes in a separate chunk
        // This is the actual scenario: OpenAI sends finish_reason in one chunk, usage in another
        let chunks = vec![
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("Hello")),
            // First message_delta with finish_reason but no usage
            UnifiedStreamChunk::message_delta(StopReason::EndTurn, UnifiedUsage::new(0, 0)),
        ];

        let result = state.process_chunks(chunks);

        // Find the message_delta in the result
        let message_delta = result
            .iter()
            .find(|c| c.chunk_type == ChunkType::MessageDelta)
            .unwrap();
        // First message_delta uses accumulated usage (since provider sent zero)
        // The accumulated usage has output_tokens from "Hello" text
        assert!(message_delta.usage.is_some());
        let usage = message_delta.usage.as_ref().unwrap();
        assert!(usage.output_tokens > 0); // Accumulated from "Hello"

        // Now process the usage-only chunk with actual provider usage
        let chunks = vec![UnifiedStreamChunk::message_delta(
            StopReason::EndTurn,
            UnifiedUsage::new(100, 50),
        )];

        let result = state.process_chunks(chunks);

        // Second message_delta should have the actual provider usage
        let message_delta = result
            .iter()
            .find(|c| c.chunk_type == ChunkType::MessageDelta)
            .unwrap();
        assert_eq!(message_delta.usage.as_ref().unwrap().input_tokens, 100);
        assert_eq!(message_delta.usage.as_ref().unwrap().output_tokens, 50);
    }

    #[test]
    fn test_cross_protocol_stream_state_usage_not_overwritten_by_zero() {
        use super::super::{StopReason, UnifiedContent, UnifiedStreamChunk, UnifiedUsage};

        // Test that accumulated usage is used when provider doesn't send usage
        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(100));
        state.message_started = true;
        state.started_blocks.insert(0);

        // Accumulate some output tokens
        let chunks = vec![UnifiedStreamChunk::content_block_delta(
            0,
            UnifiedContent::text("Hello world"),
        )];
        state.process_chunks(chunks);

        state.stopped_blocks.insert(0);

        // Receive a message_delta with actual usage from provider
        let chunks = vec![UnifiedStreamChunk::message_delta(
            StopReason::EndTurn,
            UnifiedUsage::new(100, 50),
        )];

        let result = state.process_chunks(chunks);
        assert_eq!(result[0].usage.as_ref().unwrap().input_tokens, 100);
        assert_eq!(result[0].usage.as_ref().unwrap().output_tokens, 50);

        // Then receive another message_delta without provider usage (None)
        // This should use the accumulated usage
        let mut second_chunk =
            UnifiedStreamChunk::message_delta(StopReason::EndTurn, UnifiedUsage::new(0, 0));
        // Simulate no provider usage by setting it to None
        second_chunk.usage = None;

        let chunks = vec![second_chunk];
        let result = state.process_chunks(chunks);

        // Should use accumulated usage (input_tokens from initialization + accumulated output)
        assert!(result[0].usage.is_some());
        let usage = result[0].usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, 100); // From with_input_tokens
        assert!(usage.output_tokens > 0); // Accumulated from text chunks
    }

    // ========================================================================
    // V2 API Token Calculation Tests
    // ========================================================================

    #[test]
    fn test_v2_api_with_input_tokens_initialization() {
        // Test initialization with input_tokens for V2 API
        let state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(150));

        let usage = state.usage();
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 150);
        assert_eq!(usage.output_tokens, 0);
    }

    #[test]
    fn test_v2_api_output_token_accumulation() {
        use super::super::{UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(50));
        state.message_started = true;
        state.started_blocks.insert(0);

        // Simulate streaming text chunks
        let chunks = vec![
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("Hello")),
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text(" world")),
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("!")),
        ];

        state.process_chunks(chunks);

        // Verify output tokens were accumulated
        let usage = state.usage();
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 50);
        assert!(usage.output_tokens > 0);
        // "Hello world!" should be about 3-4 tokens
        assert!(usage.output_tokens >= 2 && usage.output_tokens <= 6);
    }

    #[test]
    fn test_v2_api_provider_usage_priority() {
        use super::super::{StopReason, UnifiedContent, UnifiedStreamChunk, UnifiedUsage};

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(50));
        state.message_started = true;
        state.started_blocks.insert(0);

        // Accumulate some tokens
        let chunks = vec![UnifiedStreamChunk::content_block_delta(
            0,
            UnifiedContent::text("Test content"),
        )];
        state.process_chunks(chunks);

        // Provider sends accurate usage
        state.stopped_blocks.insert(0);
        let chunks = vec![UnifiedStreamChunk::message_delta(
            StopReason::EndTurn,
            UnifiedUsage::new(60, 25),
        )];

        let result = state.process_chunks(chunks);

        // Should use provider usage (priority over calculated)
        assert!(result[0].usage.is_some());
        let usage = result[0].usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, 60); // Provider value
        assert_eq!(usage.output_tokens, 25); // Provider value
    }

    #[test]
    fn test_v2_api_fallback_when_no_provider_usage() {
        use super::super::{StopReason, UnifiedContent, UnifiedStreamChunk, UnifiedUsage};

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(100));
        state.message_started = true;
        state.started_blocks.insert(0);

        // Accumulate tokens
        let chunks = vec![UnifiedStreamChunk::content_block_delta(
            0,
            UnifiedContent::text("Hello world"),
        )];
        state.process_chunks(chunks);

        // Provider doesn't send usage (None)
        state.stopped_blocks.insert(0);
        let mut chunk =
            UnifiedStreamChunk::message_delta(StopReason::EndTurn, UnifiedUsage::new(0, 0));
        chunk.usage = None; // Simulate no provider usage

        let chunks = vec![chunk];
        let result = state.process_chunks(chunks);

        // Should use accumulated usage
        assert!(result[0].usage.is_some());
        let usage = result[0].usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, 100); // From initialization
        assert!(usage.output_tokens > 0); // Accumulated
    }

    #[test]
    fn test_v2_api_multiple_content_blocks() {
        use super::super::{UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(80));
        state.message_started = true;

        // Multiple content blocks with text
        let chunks = vec![
            UnifiedStreamChunk::content_block_start(0, UnifiedContent::text("")),
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("First block")),
            UnifiedStreamChunk::content_block_stop(0),
            UnifiedStreamChunk::content_block_start(1, UnifiedContent::text("")),
            UnifiedStreamChunk::content_block_delta(1, UnifiedContent::text("Second block")),
            UnifiedStreamChunk::content_block_stop(1),
        ];

        state.process_chunks(chunks);

        // Verify tokens from both blocks
        let usage = state.usage();
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 80);
        assert!(usage.output_tokens > 0);
        // "First block" + "Second block" should be about 4-6 tokens
        assert!(usage.output_tokens >= 3);
    }

    #[test]
    fn test_v2_api_empty_text_no_tokens() {
        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(100));
        let initial_output = state.usage().unwrap().output_tokens;

        // Empty text should not add tokens
        state.accumulate_output_tokens("");

        let usage = state.usage().unwrap();
        assert_eq!(usage.output_tokens, initial_output);
    }

    #[test]
    fn test_v2_api_get_final_usage_priority() {
        use super::super::UnifiedUsage;

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(50));
        state.accumulate_output_tokens("Test");

        // Provider usage takes priority
        let provider_usage = Some(UnifiedUsage::new(60, 20));
        let final_usage = state.get_final_usage(provider_usage);

        assert!(final_usage.is_some());
        let usage = final_usage.unwrap();
        assert_eq!(usage.input_tokens, 60);
        assert_eq!(usage.output_tokens, 20);

        // Without provider usage, use accumulated
        let final_usage = state.get_final_usage(None);

        assert!(final_usage.is_some());
        let usage = final_usage.unwrap();
        assert_eq!(usage.input_tokens, 50);
        assert!(usage.output_tokens > 0);
    }

    // ========================================================================
    // Tool Input Delta Token Accumulation Tests
    // ========================================================================

    #[test]
    fn test_v2_api_tool_input_delta_token_accumulation() {
        use super::super::{UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(100));
        state.message_started = true;
        state.started_blocks.insert(0);

        // Simulate tool_calls streaming with ToolInputDelta
        let chunks = vec![
            UnifiedStreamChunk::content_block_delta(
                0,
                UnifiedContent::tool_input_delta(0, "{\"location\":"),
            ),
            UnifiedStreamChunk::content_block_delta(
                0,
                UnifiedContent::tool_input_delta(0, " \"San Francisco\"}"),
            ),
        ];

        state.process_chunks(chunks);

        // Verify output tokens were accumulated from tool_input_delta
        let usage = state.usage();
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert!(
            usage.output_tokens > 0,
            "output_tokens should be accumulated from tool_input_delta"
        );
    }

    #[test]
    fn test_v2_api_tool_input_delta_empty_not_accumulated() {
        use super::super::{UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(100));
        state.message_started = true;
        state.started_blocks.insert(0);

        let initial_output = state.usage().unwrap().output_tokens;

        // Empty tool_input_delta should not add tokens
        let chunks = vec![UnifiedStreamChunk::content_block_delta(
            0,
            UnifiedContent::tool_input_delta(0, ""),
        )];

        state.process_chunks(chunks);

        let usage = state.usage().unwrap();
        assert_eq!(usage.output_tokens, initial_output);
    }

    #[test]
    fn test_v2_api_mixed_text_and_tool_input_delta() {
        use super::super::{UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(100));
        state.message_started = true;

        // Simulate mixed content: text first, then tool_calls
        let chunks = vec![
            // Text content block
            UnifiedStreamChunk::content_block_start(0, UnifiedContent::text("")),
            UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("Let me check")),
            UnifiedStreamChunk::content_block_stop(0),
            // Tool use content block
            UnifiedStreamChunk::content_block_start(
                1,
                UnifiedContent::tool_use("call_123", "get_weather", serde_json::json!({})),
            ),
            UnifiedStreamChunk::content_block_delta(
                1,
                UnifiedContent::tool_input_delta(0, "{\"city\":\"NYC\"}"),
            ),
            UnifiedStreamChunk::content_block_stop(1),
        ];

        state.process_chunks(chunks);

        // Verify tokens from both text and tool_input_delta
        let usage = state.usage();
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        // Should have tokens from "Let me check" + "{\"city\":\"NYC\"}"
        assert!(
            usage.output_tokens > 0,
            "output_tokens should include both text and tool_input_delta"
        );
    }

    #[test]
    fn test_v2_api_thinking_content_token_accumulation() {
        use super::super::{UnifiedContent, UnifiedStreamChunk};

        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(100));
        state.message_started = true;
        state.started_blocks.insert(0);

        // Simulate thinking content streaming
        let chunks = vec![
            UnifiedStreamChunk::content_block_delta(
                0,
                UnifiedContent::thinking("Let me think about this...", None),
            ),
            UnifiedStreamChunk::content_block_delta(
                0,
                UnifiedContent::thinking("The answer is 42.", None),
            ),
        ];

        state.process_chunks(chunks);

        // Verify output tokens were accumulated from thinking content
        let usage = state.usage();
        assert!(usage.is_some());
        let usage = usage.unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert!(
            usage.output_tokens > 0,
            "output_tokens should be accumulated from thinking content"
        );
    }

    #[test]
    fn test_v2_api_tool_use_only_fallback_usage() {
        use super::super::{StopReason, UnifiedContent, UnifiedStreamChunk, UnifiedUsage};

        // This test simulates the exact scenario from the bug report:
        // Provider returns tool_calls with no text content, and provider doesn't send usage
        let mut state = CrossProtocolStreamState::with_input_tokens("gpt-4", Some(150));
        state.message_started = true;

        // Simulate tool_calls only response (no text content)
        let chunks = vec![
            UnifiedStreamChunk::content_block_start(
                0,
                UnifiedContent::tool_use("call_abc", "get_weather", serde_json::json!({})),
            ),
            UnifiedStreamChunk::content_block_delta(
                0,
                UnifiedContent::tool_input_delta(
                    0,
                    "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}",
                ),
            ),
            UnifiedStreamChunk::content_block_stop(0),
        ];

        state.process_chunks(chunks);

        // Provider doesn't send usage (simulating the bug scenario)
        let mut message_delta =
            UnifiedStreamChunk::message_delta(StopReason::ToolUse, UnifiedUsage::new(0, 0));
        message_delta.usage = None;

        let result = state.process_chunks(vec![message_delta]);

        // Verify fallback usage is calculated correctly
        let message_delta_chunk = result
            .iter()
            .find(|c| c.chunk_type == super::super::ChunkType::MessageDelta)
            .unwrap();
        let usage = message_delta_chunk.usage.as_ref().unwrap();

        assert_eq!(usage.input_tokens, 150); // From initialization
        assert!(
            usage.output_tokens > 0,
            "output_tokens should be > 0 from tool_input_delta accumulation"
        );
        // The tool arguments JSON should contribute to output tokens
        // "{\"location\":\"San Francisco\",\"unit\":\"celsius\"}" is about 10-15 tokens
        assert!(
            usage.output_tokens >= 5,
            "output_tokens should reflect the tool arguments JSON"
        );
    }
}
