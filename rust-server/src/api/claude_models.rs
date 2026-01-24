//! Claude API request and response models.
//!
//! This module defines all data structures used in the Claude Messages API,
//! including requests, responses, content blocks, and streaming events.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

// ============================================================================
// Constants
// ============================================================================

/// Constants for Claude API integration.
pub mod constants {
    // Role constants
    pub const ROLE_USER: &str = "user";
    pub const ROLE_ASSISTANT: &str = "assistant";
    pub const ROLE_SYSTEM: &str = "system";
    pub const ROLE_TOOL: &str = "tool";

    // Content type constants
    pub const CONTENT_TEXT: &str = "text";
    pub const CONTENT_IMAGE: &str = "image";
    pub const CONTENT_TOOL_USE: &str = "tool_use";
    pub const CONTENT_TOOL_RESULT: &str = "tool_result";

    // Tool type constants
    pub const TOOL_FUNCTION: &str = "function";

    // Stop reason constants
    pub const STOP_END_TURN: &str = "end_turn";
    pub const STOP_MAX_TOKENS: &str = "max_tokens";
    pub const STOP_STOP_SEQUENCE: &str = "stop_sequence";
    pub const STOP_TOOL_USE: &str = "tool_use";
    // Reserved for future Claude API compatibility - not currently used in conversion
    pub const STOP_PAUSE_TURN: &str = "pause_turn";
    pub const STOP_REFUSAL: &str = "refusal";

    // SSE event type constants
    pub const EVENT_MESSAGE_START: &str = "message_start";
    pub const EVENT_MESSAGE_STOP: &str = "message_stop";
    pub const EVENT_MESSAGE_DELTA: &str = "message_delta";
    pub const EVENT_CONTENT_BLOCK_START: &str = "content_block_start";
    pub const EVENT_CONTENT_BLOCK_STOP: &str = "content_block_stop";
    pub const EVENT_CONTENT_BLOCK_DELTA: &str = "content_block_delta";
    pub const EVENT_PING: &str = "ping";

    // Delta type constants
    pub const DELTA_TEXT: &str = "text_delta";
    pub const DELTA_INPUT_JSON: &str = "input_json_delta";
}

// ============================================================================
// Content Block Types
// ============================================================================

/// Text content block in Claude messages.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeContentBlockText {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl Default for ClaudeContentBlockText {
    fn default() -> Self {
        Self {
            content_type: constants::CONTENT_TEXT.to_string(),
            text: String::new(),
        }
    }
}

/// Image source for Claude image content blocks.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// Image content block in Claude messages.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeContentBlockImage {
    #[serde(rename = "type")]
    pub content_type: String,
    pub source: ClaudeImageSource,
}

/// Tool use content block in Claude messages.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeContentBlockToolUse {
    #[serde(rename = "type")]
    pub content_type: String,
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// Tool result content block in Claude messages.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeContentBlockToolResult {
    #[serde(rename = "type")]
    pub content_type: String,
    pub tool_use_id: String,
    pub content: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// Union type for all content block types.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ClaudeContentBlock {
    Text(ClaudeContentBlockText),
    Image(ClaudeContentBlockImage),
    ToolUse(ClaudeContentBlockToolUse),
    ToolResult(ClaudeContentBlockToolResult),
}

impl ClaudeContentBlock {
    /// Get the type of this content block.
    pub fn get_type(&self) -> &str {
        match self {
            ClaudeContentBlock::Text(b) => &b.content_type,
            ClaudeContentBlock::Image(b) => &b.content_type,
            ClaudeContentBlock::ToolUse(b) => &b.content_type,
            ClaudeContentBlock::ToolResult(b) => &b.content_type,
        }
    }

    /// Create a text content block.
    pub fn text(text: impl Into<String>) -> Self {
        ClaudeContentBlock::Text(ClaudeContentBlockText {
            content_type: constants::CONTENT_TEXT.to_string(),
            text: text.into(),
        })
    }

    /// Create a tool use content block.
    pub fn tool_use(id: impl Into<String>, name: impl Into<String>, input: serde_json::Value) -> Self {
        ClaudeContentBlock::ToolUse(ClaudeContentBlockToolUse {
            content_type: constants::CONTENT_TOOL_USE.to_string(),
            id: id.into(),
            name: name.into(),
            input,
        })
    }
}

// ============================================================================
// Message Types
// ============================================================================

/// System content block for Claude messages.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeSystemContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// Content that can be either a string or a list of content blocks.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ClaudeMessageContent {
    Text(String),
    Blocks(Vec<ClaudeContentBlock>),
}

/// A message in Claude conversation format.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeMessage {
    pub role: String,
    pub content: ClaudeMessageContent,
}

// ============================================================================
// Tool Types
// ============================================================================

/// Tool definition for Claude API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

/// Configuration for Claude's extended thinking feature.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeThinkingConfig {
    #[serde(rename = "type")]
    pub thinking_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<i32>,
}

// ============================================================================
// Request Types
// ============================================================================

/// System prompt that can be either a string or a list of system content blocks.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ClaudeSystemPrompt {
    Text(String),
    Blocks(Vec<ClaudeSystemContent>),
}

/// Request model for Claude Messages API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "model": "claude-3-opus-20240229",
    "max_tokens": 1024,
    "messages": [
        {"role": "user", "content": "Hello!"}
    ]
}))]
pub struct ClaudeMessagesRequest {
    /// The model to use for completion
    pub model: String,
    
    /// Maximum number of tokens to generate
    pub max_tokens: i32,
    
    /// List of messages in the conversation
    pub messages: Vec<ClaudeMessage>,
    
    /// System prompt or instructions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<ClaudeSystemPrompt>,
    
    /// Sequences that will stop generation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    
    /// Whether to stream the response
    #[serde(default)]
    pub stream: bool,
    
    /// Sampling temperature (0.0 to 1.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    
    /// Nucleus sampling probability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    
    /// Top-k sampling parameter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    
    /// Optional metadata for the request
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    
    /// List of tools available to the model
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ClaudeTool>>,
    
    /// How the model should use tools
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
    
    /// Extended thinking configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ClaudeThinkingConfig>,
}

/// Request model for Claude token counting API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeTokenCountRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<ClaudeSystemPrompt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ClaudeTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ClaudeThinkingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<serde_json::Value>,
}

// ============================================================================
// Response Types
// ============================================================================

/// Token usage information from Claude API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct ClaudeUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<i32>,
}

/// Response model from Claude Messages API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "id": "msg_01XFDUDYJgAACzvnptvVoYEL",
    "type": "message",
    "role": "assistant",
    "content": [{"type": "text", "text": "Hello!"}],
    "model": "claude-3-opus-20240229",
    "stop_reason": "end_turn",
    "usage": {"input_tokens": 10, "output_tokens": 5}
}))]
pub struct ClaudeResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub role: String,
    pub content: Vec<ClaudeContentBlock>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: ClaudeUsage,
}

impl ClaudeResponse {
    /// Create a new Claude response.
    pub fn new(
        id: impl Into<String>,
        model: impl Into<String>,
        content: Vec<ClaudeContentBlock>,
        stop_reason: Option<String>,
        usage: ClaudeUsage,
    ) -> Self {
        Self {
            id: id.into(),
            response_type: "message".to_string(),
            role: constants::ROLE_ASSISTANT.to_string(),
            content,
            model: model.into(),
            stop_reason,
            stop_sequence: None,
            usage,
        }
    }
}

/// Response model from Claude token counting API.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeTokenCountResponse {
    pub input_tokens: i32,
}

// ============================================================================
// Error Types
// ============================================================================

/// Claude API error detail.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeErrorDetail {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

/// Claude API error response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeErrorResponse {
    #[serde(rename = "type")]
    pub response_type: String,
    pub error: ClaudeErrorDetail,
}

impl ClaudeErrorResponse {
    /// Create a new Claude error response.
    pub fn new(error_type: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            response_type: "error".to_string(),
            error: ClaudeErrorDetail {
                error_type: error_type.into(),
                message: message.into(),
            },
        }
    }

    /// Create an API error response.
    pub fn api_error(message: impl Into<String>) -> Self {
        Self::new("api_error", message)
    }

    /// Create a timeout error response.
    pub fn timeout_error(message: impl Into<String>) -> Self {
        Self::new("timeout_error", message)
    }

    /// Create an invalid request error response.
    pub fn invalid_request_error(message: impl Into<String>) -> Self {
        Self::new("invalid_request_error", message)
    }
}

// ============================================================================
// Streaming Types
// ============================================================================

/// Message start event data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeMessageStartData {
    #[serde(rename = "type")]
    pub event_type: String,
    pub message: ClaudeStreamMessage,
}

/// Partial message for streaming.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeStreamMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub role: String,
    pub model: String,
    pub content: Vec<ClaudeContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: ClaudeUsage,
}

/// Content block start event data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeContentBlockStartData {
    #[serde(rename = "type")]
    pub event_type: String,
    pub index: i32,
    pub content_block: ClaudeContentBlock,
}

/// Content block delta event data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeContentBlockDeltaData {
    #[serde(rename = "type")]
    pub event_type: String,
    pub index: i32,
    pub delta: ClaudeStreamDelta,
}

/// Delta content in streaming responses.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ClaudeStreamDelta {
    Text {
        #[serde(rename = "type")]
        delta_type: String,
        text: String,
    },
    InputJson {
        #[serde(rename = "type")]
        delta_type: String,
        partial_json: String,
    },
}

impl ClaudeStreamDelta {
    /// Create a text delta.
    pub fn text(text: impl Into<String>) -> Self {
        ClaudeStreamDelta::Text {
            delta_type: constants::DELTA_TEXT.to_string(),
            text: text.into(),
        }
    }

    /// Create an input JSON delta.
    pub fn input_json(partial_json: impl Into<String>) -> Self {
        ClaudeStreamDelta::InputJson {
            delta_type: constants::DELTA_INPUT_JSON.to_string(),
            partial_json: partial_json.into(),
        }
    }
}

/// Content block stop event data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeContentBlockStopData {
    #[serde(rename = "type")]
    pub event_type: String,
    pub index: i32,
}

/// Message delta event data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeMessageDeltaData {
    #[serde(rename = "type")]
    pub event_type: String,
    pub delta: ClaudeMessageDelta,
    pub usage: ClaudeUsage,
}

/// Message delta content.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeMessageDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

/// Message stop event data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudeMessageStopData {
    #[serde(rename = "type")]
    pub event_type: String,
}

/// Ping event data.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClaudePingData {
    #[serde(rename = "type")]
    pub event_type: String,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_content_block_text_serialization() {
        let block = ClaudeContentBlockText {
            content_type: "text".to_string(),
            text: "Hello".to_string(),
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_claude_message_content_text() {
        let content = ClaudeMessageContent::Text("Hello".to_string());
        let json = serde_json::to_string(&content).unwrap();
        assert_eq!(json, "\"Hello\"");
    }

    #[test]
    fn test_claude_message_content_blocks() {
        let content = ClaudeMessageContent::Blocks(vec![
            ClaudeContentBlock::text("Hello"),
        ]);
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"text\""));
    }

    #[test]
    fn test_claude_messages_request_deserialization() {
        let json = r#"{
            "model": "claude-3-opus-20240229",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Hello!"}
            ]
        }"#;
        let request: ClaudeMessagesRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.model, "claude-3-opus-20240229");
        assert_eq!(request.max_tokens, 1024);
        assert_eq!(request.messages.len(), 1);
    }

    #[test]
    fn test_claude_response_serialization() {
        let response = ClaudeResponse::new(
            "msg_123",
            "claude-3-opus-20240229",
            vec![ClaudeContentBlock::text("Hello!")],
            Some("end_turn".to_string()),
            ClaudeUsage {
                input_tokens: 10,
                output_tokens: 5,
                ..Default::default()
            },
        );
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"id\":\"msg_123\""));
        assert!(json.contains("\"type\":\"message\""));
        assert!(json.contains("\"role\":\"assistant\""));
    }

    #[test]
    fn test_claude_error_response() {
        let error = ClaudeErrorResponse::api_error("Something went wrong");
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"type\":\"error\""));
        assert!(json.contains("\"type\":\"api_error\""));
        assert!(json.contains("Something went wrong"));
    }

    #[test]
    fn test_claude_stream_delta_text() {
        let delta = ClaudeStreamDelta::text("Hello");
        let json = serde_json::to_string(&delta).unwrap();
        assert!(json.contains("\"type\":\"text_delta\""));
        assert!(json.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_claude_usage_default() {
        let usage = ClaudeUsage::default();
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
        assert!(usage.cache_creation_input_tokens.is_none());
    }

    #[test]
    fn test_claude_content_block_helper_methods() {
        let text_block = ClaudeContentBlock::text("Hello");
        assert_eq!(text_block.get_type(), "text");

        let tool_block = ClaudeContentBlock::tool_use("tool_1", "my_tool", serde_json::json!({}));
        assert_eq!(tool_block.get_type(), "tool_use");
    }
}