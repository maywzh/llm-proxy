//! Unified Internal Format (UIF) for protocol-neutral LLM message representation.
//!
//! This module defines the lingua franca used internally by the proxy to convert
//! between different LLM API protocols (OpenAI, Anthropic, Response API).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ============================================================================
// Protocol Types
// ============================================================================

/// Supported LLM API protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    OpenAI,
    Anthropic,
    ResponseApi,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::OpenAI => write!(f, "openai"),
            Protocol::Anthropic => write!(f, "anthropic"),
            Protocol::ResponseApi => write!(f, "response_api"),
        }
    }
}

impl std::str::FromStr for Protocol {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" => Ok(Protocol::OpenAI),
            "anthropic" | "claude" => Ok(Protocol::Anthropic),
            "response_api" | "responses" => Ok(Protocol::ResponseApi),
            _ => Err(format!("Unknown protocol: {}", s)),
        }
    }
}

/// Convert a provider_type string from the database to a Protocol enum.
///
/// This function maps the `provider_type` field stored in the database
/// to the appropriate Protocol for request/response transformation.
///
/// # Arguments
/// * `provider_type` - The provider type string (e.g., "openai", "anthropic", "azure")
///
/// # Returns
/// The corresponding Protocol enum value. Unknown types default to OpenAI.
///
/// # Examples
/// ```
/// use llm_proxy_rust::transformer::provider_type_to_protocol;
/// use llm_proxy_rust::transformer::Protocol;
///
/// assert_eq!(provider_type_to_protocol("openai"), Protocol::OpenAI);
/// assert_eq!(provider_type_to_protocol("anthropic"), Protocol::Anthropic);
/// assert_eq!(provider_type_to_protocol("azure"), Protocol::OpenAI);
/// assert_eq!(provider_type_to_protocol("unknown"), Protocol::OpenAI);
/// ```
pub fn provider_type_to_protocol(provider_type: &str) -> Protocol {
    match provider_type.to_lowercase().as_str() {
        "anthropic" | "claude" => Protocol::Anthropic,
        "openai" | "azure" | _ => Protocol::OpenAI,
    }
}

// ============================================================================
// Message Types
// ============================================================================

/// Unified message role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Default for Role {
    fn default() -> Self {
        Role::User
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::System => write!(f, "system"),
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
            Role::Tool => write!(f, "tool"),
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "system" => Ok(Role::System),
            "user" => Ok(Role::User),
            "assistant" => Ok(Role::Assistant),
            "tool" | "function" => Ok(Role::Tool),
            _ => Err(format!("Unknown role: {}", s)),
        }
    }
}

// ============================================================================
// Content Types
// ============================================================================

/// Unified content block types.
///
/// This enum represents all possible content types that can appear in messages
/// across different LLM protocols.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedContent {
    /// Plain text content
    Text { text: String },

    /// Image content (base64 or URL)
    Image {
        source_type: String, // "base64" | "url"
        media_type: String,  // MIME type
        data: String,        // base64 data or URL
    },

    /// Tool/function call from assistant
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    /// Result from a tool execution
    ToolResult {
        tool_use_id: String,
        content: Value,
        #[serde(default)]
        is_error: bool,
    },

    /// Thinking/reasoning content (for extended thinking models)
    Thinking {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>, // For Gemini 3 thought_signature
    },

    /// File reference (for Response API)
    File {
        file_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },

    /// Audio content
    Audio { data: String, format: String },

    /// Refusal content (when model declines to respond)
    Refusal { reason: String },

    /// Tool input JSON delta (for streaming tool_use)
    /// This represents partial JSON being streamed for tool inputs
    ToolInputDelta {
        /// Index of the tool call (for OpenAI compatibility)
        index: usize,
        /// Partial JSON string
        partial_json: String,
    },
}

impl UnifiedContent {
    /// Create a text content block.
    pub fn text(text: impl Into<String>) -> Self {
        UnifiedContent::Text { text: text.into() }
    }

    /// Create a tool use content block.
    pub fn tool_use(id: impl Into<String>, name: impl Into<String>, input: Value) -> Self {
        UnifiedContent::ToolUse {
            id: id.into(),
            name: name.into(),
            input,
        }
    }

    /// Create a tool result content block.
    pub fn tool_result(tool_use_id: impl Into<String>, content: Value, is_error: bool) -> Self {
        UnifiedContent::ToolResult {
            tool_use_id: tool_use_id.into(),
            content,
            is_error,
        }
    }

    /// Create an image content block from base64.
    pub fn image_base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        UnifiedContent::Image {
            source_type: "base64".to_string(),
            media_type: media_type.into(),
            data: data.into(),
        }
    }

    /// Create an image content block from URL.
    pub fn image_url(url: impl Into<String>) -> Self {
        UnifiedContent::Image {
            source_type: "url".to_string(),
            media_type: String::new(),
            data: url.into(),
        }
    }

    /// Create a thinking content block.
    pub fn thinking(text: impl Into<String>, signature: Option<String>) -> Self {
        UnifiedContent::Thinking {
            text: text.into(),
            signature,
        }
    }

    /// Create a tool input delta content block (for streaming tool_use).
    pub fn tool_input_delta(index: usize, partial_json: impl Into<String>) -> Self {
        UnifiedContent::ToolInputDelta {
            index,
            partial_json: partial_json.into(),
        }
    }

    /// Get the type name of this content block.
    pub fn content_type(&self) -> &'static str {
        match self {
            UnifiedContent::Text { .. } => "text",
            UnifiedContent::Image { .. } => "image",
            UnifiedContent::ToolUse { .. } => "tool_use",
            UnifiedContent::ToolResult { .. } => "tool_result",
            UnifiedContent::Thinking { .. } => "thinking",
            UnifiedContent::File { .. } => "file",
            UnifiedContent::Audio { .. } => "audio",
            UnifiedContent::Refusal { .. } => "refusal",
            UnifiedContent::ToolInputDelta { .. } => "tool_input_delta",
        }
    }

    /// Extract text if this is a text content block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            UnifiedContent::Text { text } => Some(text),
            _ => None,
        }
    }
}

// ============================================================================
// Tool Types
// ============================================================================

/// Unified tool call structure (from assistant).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Unified tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
    /// Tool type for Response API (e.g., "function", "computer_use_preview")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_type: Option<String>,
}

impl UnifiedTool {
    /// Create a new function tool.
    pub fn function(
        name: impl Into<String>,
        description: Option<String>,
        input_schema: Value,
    ) -> Self {
        UnifiedTool {
            name: name.into(),
            description,
            input_schema,
            tool_type: Some("function".to_string()),
        }
    }
}

// ============================================================================
// Message Types
// ============================================================================

/// Unified message structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMessage {
    pub role: Role,
    pub content: Vec<UnifiedContent>,
    /// Optional name for the message author
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Tool calls for assistant messages
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<UnifiedToolCall>,
    /// Tool call ID for tool response messages
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl UnifiedMessage {
    /// Create a new message with text content.
    pub fn new(role: Role, text: impl Into<String>) -> Self {
        UnifiedMessage {
            role,
            content: vec![UnifiedContent::text(text)],
            name: None,
            tool_calls: vec![],
            tool_call_id: None,
        }
    }

    /// Create a user message.
    pub fn user(text: impl Into<String>) -> Self {
        Self::new(Role::User, text)
    }

    /// Create an assistant message.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::new(Role::Assistant, text)
    }

    /// Create a system message.
    pub fn system(text: impl Into<String>) -> Self {
        Self::new(Role::System, text)
    }

    /// Create a tool result message.
    pub fn tool_result(tool_call_id: impl Into<String>, content: Value, is_error: bool) -> Self {
        let id = tool_call_id.into();
        UnifiedMessage {
            role: Role::Tool,
            content: vec![UnifiedContent::tool_result(&id, content, is_error)],
            name: None,
            tool_calls: vec![],
            tool_call_id: Some(id),
        }
    }

    /// Create a message with content blocks.
    pub fn with_content(role: Role, content: Vec<UnifiedContent>) -> Self {
        UnifiedMessage {
            role,
            content,
            name: None,
            tool_calls: vec![],
            tool_call_id: None,
        }
    }

    /// Add a tool call to this message.
    pub fn with_tool_call(mut self, tool_call: UnifiedToolCall) -> Self {
        self.tool_calls.push(tool_call);
        self
    }

    /// Get concatenated text from all text content blocks.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("")
    }
}

// ============================================================================
// Parameters Types
// ============================================================================

/// Unified model parameters.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnifiedParameters {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    pub stream: bool,
    /// Extended parameters (protocol-specific, passed through)
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

// ============================================================================
// Request Types
// ============================================================================

/// Unified request structure (lingua franca).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedRequest {
    /// Original model name from client
    pub model: String,
    /// Messages in unified format
    pub messages: Vec<UnifiedMessage>,
    /// System prompt (separate from messages for Anthropic compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Model parameters
    #[serde(default)]
    pub parameters: UnifiedParameters,
    /// Available tools
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<UnifiedTool>,
    /// Tool choice configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    /// Original request ID for tracing
    #[serde(default)]
    pub request_id: String,
    /// Client protocol (detected or explicit)
    #[serde(default)]
    pub client_protocol: Protocol,
    /// Metadata for observability
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, Value>,
}

impl UnifiedRequest {
    /// Create a new unified request.
    pub fn new(model: impl Into<String>, messages: Vec<UnifiedMessage>) -> Self {
        UnifiedRequest {
            model: model.into(),
            messages,
            system: None,
            parameters: UnifiedParameters::default(),
            tools: vec![],
            tool_choice: None,
            request_id: uuid::Uuid::new_v4().to_string(),
            client_protocol: Protocol::default(),
            metadata: HashMap::new(),
        }
    }

    /// Set the system prompt.
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set streaming mode.
    pub fn with_stream(mut self, stream: bool) -> Self {
        self.parameters.stream = stream;
        self
    }

    /// Set max tokens.
    pub fn with_max_tokens(mut self, max_tokens: i32) -> Self {
        self.parameters.max_tokens = Some(max_tokens);
        self
    }

    /// Set the client protocol.
    pub fn with_client_protocol(mut self, protocol: Protocol) -> Self {
        self.client_protocol = protocol;
        self
    }

    /// Check if streaming is enabled.
    pub fn is_streaming(&self) -> bool {
        self.parameters.stream
    }
}

// ============================================================================
// Response Types
// ============================================================================

/// Stop reason enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    StopSequence,
    ToolUse,
    ContentFilter,
    /// Length limit reached (OpenAI style)
    Length,
}

impl Default for StopReason {
    fn default() -> Self {
        StopReason::EndTurn
    }
}

impl std::fmt::Display for StopReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StopReason::EndTurn => write!(f, "end_turn"),
            StopReason::MaxTokens => write!(f, "max_tokens"),
            StopReason::StopSequence => write!(f, "stop_sequence"),
            StopReason::ToolUse => write!(f, "tool_use"),
            StopReason::ContentFilter => write!(f, "content_filter"),
            StopReason::Length => write!(f, "length"),
        }
    }
}

/// Unified usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UnifiedUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<i32>,
}

impl UnifiedUsage {
    /// Create new usage statistics.
    pub fn new(input_tokens: i32, output_tokens: i32) -> Self {
        UnifiedUsage {
            input_tokens,
            output_tokens,
            cache_read_tokens: None,
            cache_write_tokens: None,
        }
    }

    /// Get total tokens.
    pub fn total_tokens(&self) -> i32 {
        self.input_tokens + self.output_tokens
    }
}

/// Unified response structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResponse {
    pub id: String,
    pub model: String,
    pub content: Vec<UnifiedContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    #[serde(default)]
    pub usage: UnifiedUsage,
    /// Tool calls (if any)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<UnifiedToolCall>,
}

impl UnifiedResponse {
    /// Create a new unified response.
    pub fn new(
        id: impl Into<String>,
        model: impl Into<String>,
        content: Vec<UnifiedContent>,
        stop_reason: Option<StopReason>,
        usage: UnifiedUsage,
    ) -> Self {
        UnifiedResponse {
            id: id.into(),
            model: model.into(),
            content,
            stop_reason,
            usage,
            tool_calls: vec![],
        }
    }

    /// Create a simple text response.
    pub fn text(
        id: impl Into<String>,
        model: impl Into<String>,
        text: impl Into<String>,
        usage: UnifiedUsage,
    ) -> Self {
        Self::new(
            id,
            model,
            vec![UnifiedContent::text(text)],
            Some(StopReason::EndTurn),
            usage,
        )
    }

    /// Get concatenated text from all text content blocks.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("")
    }
}

// ============================================================================
// Streaming Types
// ============================================================================

/// Unified streaming chunk type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChunkType {
    MessageStart,
    ContentBlockStart,
    ContentBlockDelta,
    ContentBlockStop,
    MessageDelta,
    MessageStop,
    Ping,
}

/// Unified streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedStreamChunk {
    pub chunk_type: ChunkType,
    #[serde(default)]
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<UnifiedContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UnifiedUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
    /// Full message data for message_start events
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<UnifiedResponse>,
    /// Content block for content_block_start events
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_block: Option<UnifiedContent>,
}

impl UnifiedStreamChunk {
    /// Create a message start chunk.
    pub fn message_start(message: UnifiedResponse) -> Self {
        UnifiedStreamChunk {
            chunk_type: ChunkType::MessageStart,
            index: 0,
            delta: None,
            usage: Some(message.usage.clone()),
            stop_reason: None,
            message: Some(message),
            content_block: None,
        }
    }

    /// Create a content block start chunk.
    pub fn content_block_start(index: usize, content_block: UnifiedContent) -> Self {
        UnifiedStreamChunk {
            chunk_type: ChunkType::ContentBlockStart,
            index,
            delta: None,
            usage: None,
            stop_reason: None,
            message: None,
            content_block: Some(content_block),
        }
    }

    /// Create a content block delta chunk.
    pub fn content_block_delta(index: usize, delta: UnifiedContent) -> Self {
        UnifiedStreamChunk {
            chunk_type: ChunkType::ContentBlockDelta,
            index,
            delta: Some(delta),
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
        }
    }

    /// Create a content block stop chunk.
    pub fn content_block_stop(index: usize) -> Self {
        UnifiedStreamChunk {
            chunk_type: ChunkType::ContentBlockStop,
            index,
            delta: None,
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
        }
    }

    /// Create a message delta chunk.
    pub fn message_delta(stop_reason: StopReason, usage: UnifiedUsage) -> Self {
        UnifiedStreamChunk {
            chunk_type: ChunkType::MessageDelta,
            index: 0,
            delta: None,
            usage: Some(usage),
            stop_reason: Some(stop_reason),
            message: None,
            content_block: None,
        }
    }

    /// Create a message stop chunk.
    pub fn message_stop() -> Self {
        UnifiedStreamChunk {
            chunk_type: ChunkType::MessageStop,
            index: 0,
            delta: None,
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
        }
    }

    /// Create a ping chunk.
    pub fn ping() -> Self {
        UnifiedStreamChunk {
            chunk_type: ChunkType::Ping,
            index: 0,
            delta: None,
            usage: None,
            stop_reason: None,
            message: None,
            content_block: None,
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
    fn test_protocol_display() {
        assert_eq!(Protocol::OpenAI.to_string(), "openai");
        assert_eq!(Protocol::Anthropic.to_string(), "anthropic");
        assert_eq!(Protocol::ResponseApi.to_string(), "response_api");
    }

    #[test]
    fn test_protocol_from_str() {
        assert_eq!("openai".parse::<Protocol>().unwrap(), Protocol::OpenAI);
        assert_eq!(
            "anthropic".parse::<Protocol>().unwrap(),
            Protocol::Anthropic
        );
        assert_eq!("claude".parse::<Protocol>().unwrap(), Protocol::Anthropic);
        assert_eq!(
            "response_api".parse::<Protocol>().unwrap(),
            Protocol::ResponseApi
        );
    }

    #[test]
    fn test_role_display() {
        assert_eq!(Role::User.to_string(), "user");
        assert_eq!(Role::Assistant.to_string(), "assistant");
        assert_eq!(Role::System.to_string(), "system");
        assert_eq!(Role::Tool.to_string(), "tool");
    }

    #[test]
    fn test_unified_content_text() {
        let content = UnifiedContent::text("Hello");
        assert_eq!(content.content_type(), "text");
        assert_eq!(content.as_text(), Some("Hello"));
    }

    #[test]
    fn test_unified_message_new() {
        let msg = UnifiedMessage::user("Hello");
        assert_eq!(msg.role, Role::User);
        assert_eq!(msg.text_content(), "Hello");
    }

    #[test]
    fn test_unified_request_builder() {
        let request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")])
            .with_system("You are a helpful assistant.")
            .with_stream(true)
            .with_max_tokens(100);

        assert_eq!(request.model, "gpt-4");
        assert_eq!(
            request.system,
            Some("You are a helpful assistant.".to_string())
        );
        assert!(request.is_streaming());
        assert_eq!(request.parameters.max_tokens, Some(100));
    }

    #[test]
    fn test_unified_response_text() {
        let response =
            UnifiedResponse::text("msg_123", "gpt-4", "Hello!", UnifiedUsage::new(10, 5));

        assert_eq!(response.id, "msg_123");
        assert_eq!(response.text_content(), "Hello!");
        assert_eq!(response.usage.total_tokens(), 15);
    }

    #[test]
    fn test_unified_usage() {
        let usage = UnifiedUsage::new(100, 50);
        assert_eq!(usage.total_tokens(), 150);
    }

    #[test]
    fn test_unified_stream_chunk() {
        let chunk = UnifiedStreamChunk::content_block_delta(0, UnifiedContent::text("Hi"));
        assert_eq!(chunk.chunk_type, ChunkType::ContentBlockDelta);
        assert_eq!(chunk.index, 0);
    }

    #[test]
    fn test_content_serialization() {
        let content = UnifiedContent::text("Hello");
        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        assert!(json.contains("\"text\":\"Hello\""));
    }

    #[test]
    fn test_tool_use_content() {
        let content =
            UnifiedContent::tool_use("tool_1", "search", serde_json::json!({"query": "test"}));
        assert_eq!(content.content_type(), "tool_use");
    }

    #[test]
    fn test_provider_type_to_protocol_openai() {
        assert_eq!(provider_type_to_protocol("openai"), Protocol::OpenAI);
        assert_eq!(provider_type_to_protocol("OpenAI"), Protocol::OpenAI);
        assert_eq!(provider_type_to_protocol("OPENAI"), Protocol::OpenAI);
    }

    #[test]
    fn test_provider_type_to_protocol_anthropic() {
        assert_eq!(provider_type_to_protocol("anthropic"), Protocol::Anthropic);
        assert_eq!(provider_type_to_protocol("Anthropic"), Protocol::Anthropic);
        assert_eq!(provider_type_to_protocol("ANTHROPIC"), Protocol::Anthropic);
        assert_eq!(provider_type_to_protocol("claude"), Protocol::Anthropic);
        assert_eq!(provider_type_to_protocol("Claude"), Protocol::Anthropic);
    }

    #[test]
    fn test_provider_type_to_protocol_azure() {
        // Azure uses OpenAI protocol
        assert_eq!(provider_type_to_protocol("azure"), Protocol::OpenAI);
        assert_eq!(provider_type_to_protocol("Azure"), Protocol::OpenAI);
        assert_eq!(provider_type_to_protocol("AZURE"), Protocol::OpenAI);
    }

    #[test]
    fn test_provider_type_to_protocol_unknown_defaults_to_openai() {
        // Unknown provider types should default to OpenAI
        assert_eq!(provider_type_to_protocol("unknown"), Protocol::OpenAI);
        assert_eq!(provider_type_to_protocol("custom"), Protocol::OpenAI);
        assert_eq!(provider_type_to_protocol(""), Protocol::OpenAI);
        assert_eq!(
            provider_type_to_protocol("some-random-provider"),
            Protocol::OpenAI
        );
    }
}
