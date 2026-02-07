//! Anthropic protocol transformer.
//!
//! Handles conversion between Anthropic Messages API format and
//! the Unified Internal Format.

use super::{
    ChunkType, Protocol, Result, Role, StopReason, Transformer, UnifiedContent, UnifiedMessage,
    UnifiedParameters, UnifiedRequest, UnifiedResponse, UnifiedStreamChunk, UnifiedTool,
    UnifiedToolCall, UnifiedUsage,
};
use crate::core::AppError;
use bytes::Bytes;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

lazy_static! {
    /// Regex pattern to strip x-anthropic-billing-header prefix from system text.
    /// Matches "x-anthropic-billing-header: " at the start of a line and removes it.
    static ref BILLING_HEADER_REGEX: Regex =
        Regex::new(r"^x-anthropic-billing-header:\s*").unwrap();
}

/// Strip x-anthropic-billing-header prefix from text if present.
fn strip_billing_header(text: &str) -> String {
    BILLING_HEADER_REGEX.replace(text, "").to_string()
}

// ============================================================================
// Anthropic Request/Response Types
// ============================================================================

/// Anthropic message format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessage {
    pub role: String,
    pub content: AnthropicContent,
}

/// Anthropic content can be string or array of content blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicContent {
    Text(String),
    Blocks(Vec<AnthropicContentBlock>),
}

/// Anthropic content block types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: AnthropicImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    #[serde(rename = "thinking")]
    Thinking {
        thinking: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
}

/// Anthropic image source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    pub media_type: String,
    pub data: String,
}

/// Anthropic system prompt (can be string or array).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicSystem {
    Text(String),
    Blocks(Vec<AnthropicSystemBlock>),
}

/// Anthropic system block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicSystemBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
}

/// Anthropic tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicTool {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: Value,
}

/// Anthropic thinking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicThinking {
    #[serde(rename = "type")]
    pub thinking_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<i32>,
}

/// Anthropic messages request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub max_tokens: i32,
    pub messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<AnthropicSystem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<AnthropicTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<AnthropicThinking>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}

/// Anthropic usage.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AnthropicUsage {
    #[serde(default)]
    pub input_tokens: i32,
    #[serde(default)]
    pub output_tokens: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<i32>,
}

/// Anthropic messages response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub role: String,
    pub content: Vec<AnthropicContentBlock>,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

// ============================================================================
// Streaming Types
// ============================================================================

/// Anthropic SSE event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicStreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: AnthropicStreamMessage },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: i32,
        content_block: AnthropicContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: i32, delta: AnthropicDelta },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: i32 },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: AnthropicMessageDelta,
        usage: AnthropicUsage,
    },
    #[serde(rename = "message_stop")]
    MessageStop {},
    #[serde(rename = "ping")]
    Ping {},
}

/// Anthropic stream message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicStreamMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub role: String,
    pub model: String,
    pub content: Vec<AnthropicContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
    pub usage: AnthropicUsage,
}

/// Anthropic delta.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AnthropicDelta {
    #[serde(rename = "text_delta")]
    TextDelta { text: String },
    #[serde(rename = "input_json_delta")]
    InputJsonDelta { partial_json: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { thinking: String },
    #[serde(rename = "signature_delta")]
    SignatureDelta { signature: String },
}

/// Anthropic message delta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnthropicMessageDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

// ============================================================================
// Bedrock Compatibility
// ============================================================================

/// Check if the model is a Bedrock Claude model.
/// Bedrock Claude models have prefix "claude-" and suffix "-bedrock".
///
/// # Examples
/// ```
/// use llm_proxy_rust::transformer::anthropic::is_bedrock_claude_model;
///
/// assert!(is_bedrock_claude_model("claude-3-opus-bedrock"));
/// assert!(is_bedrock_claude_model("claude-3-sonnet-bedrock"));
/// assert!(is_bedrock_claude_model("claude-4.5-opus-bedrock"));
/// assert!(!is_bedrock_claude_model("claude-3-opus"));
/// assert!(!is_bedrock_claude_model("gpt-4-bedrock"));
/// ```
pub fn is_bedrock_claude_model(model: &str) -> bool {
    model.starts_with("claude-") && model.ends_with("-bedrock")
}

/// Check if messages contain tool_use or tool_result content blocks.
fn messages_contain_tool_content(messages: &[UnifiedMessage]) -> bool {
    messages.iter().any(|msg| {
        msg.content.iter().any(|content| {
            matches!(
                content,
                UnifiedContent::ToolUse { .. } | UnifiedContent::ToolResult { .. }
            )
        })
    })
}

/// Create a placeholder tool for Bedrock compatibility.
/// This is needed when messages contain tool_use/tool_result but no tools are defined.
fn create_placeholder_tool() -> UnifiedTool {
    UnifiedTool {
        name: "_placeholder_tool".to_string(),
        description: Some("Placeholder tool for Bedrock compatibility".to_string()),
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
        tool_type: Some("function".to_string()),
    }
}

// ============================================================================
// Anthropic Transformer Implementation
// ============================================================================

/// Anthropic protocol transformer.
pub struct AnthropicTransformer;

impl AnthropicTransformer {
    /// Create a new Anthropic transformer.
    pub fn new() -> Self {
        AnthropicTransformer
    }

    /// Convert Anthropic content block to unified content.
    fn content_block_to_unified(block: &AnthropicContentBlock) -> UnifiedContent {
        match block {
            AnthropicContentBlock::Text { text } => UnifiedContent::text(text),
            AnthropicContentBlock::Image { source } => {
                if source.source_type == "base64" {
                    UnifiedContent::image_base64(&source.media_type, &source.data)
                } else {
                    UnifiedContent::image_url(&source.data)
                }
            }
            AnthropicContentBlock::ToolUse { id, name, input } => {
                UnifiedContent::tool_use(id, name, input.clone())
            }
            AnthropicContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                UnifiedContent::tool_result(tool_use_id, content.clone(), is_error.unwrap_or(false))
            }
            AnthropicContentBlock::Thinking {
                thinking,
                signature,
            } => UnifiedContent::thinking(thinking, signature.clone()),
        }
    }

    /// Convert unified content to Anthropic content block.
    fn unified_to_content_block(content: &UnifiedContent) -> Option<AnthropicContentBlock> {
        match content {
            UnifiedContent::Text { text } => {
                Some(AnthropicContentBlock::Text { text: text.clone() })
            }
            UnifiedContent::Image {
                source_type,
                media_type,
                data,
            } => Some(AnthropicContentBlock::Image {
                source: AnthropicImageSource {
                    source_type: source_type.clone(),
                    media_type: media_type.clone(),
                    data: data.clone(),
                },
            }),
            UnifiedContent::ToolUse { id, name, input } => Some(AnthropicContentBlock::ToolUse {
                id: id.clone(),
                name: name.clone(),
                input: input.clone(),
            }),
            UnifiedContent::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some(AnthropicContentBlock::ToolResult {
                tool_use_id: tool_use_id.clone(),
                content: content.clone(),
                is_error: if *is_error { Some(true) } else { None },
            }),
            UnifiedContent::Thinking { text, signature } => Some(AnthropicContentBlock::Thinking {
                thinking: text.clone(),
                signature: signature.clone(),
            }),
            _ => None,
        }
    }

    /// Convert Anthropic message to unified message.
    fn message_to_unified(msg: &AnthropicMessage) -> UnifiedMessage {
        let role = msg.role.parse().unwrap_or(Role::User);

        let content = match &msg.content {
            AnthropicContent::Text(text) => vec![UnifiedContent::text(text)],
            AnthropicContent::Blocks(blocks) => {
                blocks.iter().map(Self::content_block_to_unified).collect()
            }
        };

        // Extract tool calls from tool_use content blocks
        let tool_calls: Vec<UnifiedToolCall> = content
            .iter()
            .filter_map(|c| {
                if let UnifiedContent::ToolUse { id, name, input } = c {
                    Some(UnifiedToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: input.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        // Extract tool_call_id from tool_result
        let tool_call_id = content.iter().find_map(|c| {
            if let UnifiedContent::ToolResult { tool_use_id, .. } = c {
                Some(tool_use_id.clone())
            } else {
                None
            }
        });

        UnifiedMessage {
            role,
            content,
            name: None,
            tool_calls,
            tool_call_id,
        }
    }

    /// Convert unified message to Anthropic message.
    fn unified_to_message(msg: &UnifiedMessage) -> AnthropicMessage {
        // Anthropic API only allows "user" and "assistant" roles.
        // Tool results must be sent as "user" role with tool_result content blocks.
        let role = match msg.role {
            Role::Tool => "user".to_string(),
            ref r => r.to_string(),
        };

        // For Role::Tool messages from OpenAI format (text content + tool_call_id),
        // convert to Anthropic tool_result content block.
        if msg.role == Role::Tool {
            if let Some(ref tool_call_id) = msg.tool_call_id {
                let result_content = msg.text_content();
                let content_value = if result_content.is_empty() {
                    Value::Null
                } else {
                    Value::String(result_content)
                };
                return AnthropicMessage {
                    role,
                    content: AnthropicContent::Blocks(vec![AnthropicContentBlock::ToolResult {
                        tool_use_id: tool_call_id.clone(),
                        content: content_value,
                        is_error: None,
                    }]),
                };
            }
        }

        let content = if msg.content.len() == 1 {
            if let Some(text) = msg.content[0].as_text() {
                AnthropicContent::Text(text.to_string())
            } else {
                AnthropicContent::Blocks(
                    msg.content
                        .iter()
                        .filter_map(Self::unified_to_content_block)
                        .collect(),
                )
            }
        } else {
            AnthropicContent::Blocks(
                msg.content
                    .iter()
                    .filter_map(Self::unified_to_content_block)
                    .collect(),
            )
        };

        // Append tool_calls as tool_use content blocks for assistant messages.
        // In OpenAI format, tool calls are separate from content, but Anthropic
        // requires them as content blocks within the message.
        // Skip tool_calls already present in content to avoid duplicates
        // (Anthropic→Anthropic path stores them in both places).
        let content = if !msg.tool_calls.is_empty() {
            let mut blocks = match content {
                AnthropicContent::Text(text) if text.is_empty() => vec![],
                AnthropicContent::Text(text) => {
                    vec![AnthropicContentBlock::Text { text }]
                }
                AnthropicContent::Blocks(blocks) => blocks,
            };
            let existing_ids: std::collections::HashSet<String> = blocks
                .iter()
                .filter_map(|b| match b {
                    AnthropicContentBlock::ToolUse { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect();
            for tc in &msg.tool_calls {
                if !existing_ids.contains(&tc.id) {
                    blocks.push(AnthropicContentBlock::ToolUse {
                        id: tc.id.clone(),
                        name: tc.name.clone(),
                        input: tc.arguments.clone(),
                    });
                }
            }
            AnthropicContent::Blocks(blocks)
        } else {
            content
        };

        AnthropicMessage { role, content }
    }

    /// Check if Anthropic content is empty (empty string or empty blocks).
    fn is_empty_content(content: &AnthropicContent) -> bool {
        match content {
            AnthropicContent::Text(text) => text.is_empty(),
            AnthropicContent::Blocks(blocks) => blocks.is_empty(),
        }
    }

    /// Convert Anthropic stop reason to unified stop reason.
    fn stop_reason_to_unified(reason: &str) -> StopReason {
        match reason {
            "end_turn" => StopReason::EndTurn,
            "max_tokens" => StopReason::MaxTokens,
            "stop_sequence" => StopReason::StopSequence,
            "tool_use" => StopReason::ToolUse,
            _ => StopReason::EndTurn,
        }
    }

    /// Convert unified stop reason to Anthropic stop reason.
    fn unified_to_stop_reason(reason: &StopReason) -> &'static str {
        match reason {
            StopReason::EndTurn => "end_turn",
            StopReason::MaxTokens | StopReason::Length => "max_tokens",
            StopReason::StopSequence => "stop_sequence",
            StopReason::ToolUse => "tool_use",
            StopReason::ContentFilter => "end_turn",
        }
    }

    /// Extract system prompt from Anthropic system field.
    /// Also strips x-anthropic-billing-header prefix from text blocks if present.
    fn extract_system(system: &Option<AnthropicSystem>) -> Option<String> {
        system.as_ref().map(|s| match s {
            AnthropicSystem::Text(text) => strip_billing_header(text),
            AnthropicSystem::Blocks(blocks) => blocks
                .iter()
                .map(|b| strip_billing_header(&b.text))
                .collect::<Vec<_>>()
                .join("\n"),
        })
    }
}

impl Default for AnthropicTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl Transformer for AnthropicTransformer {
    fn protocol(&self) -> Protocol {
        Protocol::Anthropic
    }

    fn transform_request_out(&self, raw: Value) -> Result<UnifiedRequest> {
        let request: AnthropicRequest =
            serde_json::from_value(raw).map_err(|e| AppError::BadRequest(e.to_string()))?;

        // Convert messages
        let messages: Vec<UnifiedMessage> = request
            .messages
            .iter()
            .map(Self::message_to_unified)
            .collect();

        // Extract system prompt
        let system = Self::extract_system(&request.system);

        // Convert tools
        let tools: Vec<UnifiedTool> = request
            .tools
            .unwrap_or_default()
            .into_iter()
            .map(|t| UnifiedTool {
                name: t.name,
                description: t.description,
                input_schema: t.input_schema,
                tool_type: Some("function".to_string()),
            })
            .collect();

        // Build parameters
        let mut extra = HashMap::new();
        if let Some(thinking) = request.thinking {
            extra.insert("thinking".to_string(), json!(thinking));
        }

        let parameters = UnifiedParameters {
            temperature: request.temperature,
            max_tokens: Some(request.max_tokens),
            top_p: request.top_p,
            top_k: request.top_k,
            stop_sequences: request.stop_sequences,
            stream: request.stream,
            extra,
        };

        Ok(UnifiedRequest {
            model: request.model,
            messages,
            system,
            parameters,
            tools,
            tool_choice: request.tool_choice,
            request_id: uuid::Uuid::new_v4().to_string(),
            client_protocol: Protocol::Anthropic,
            metadata: request.metadata.unwrap_or_default(),
        })
    }

    fn transform_request_in(&self, unified: &UnifiedRequest) -> Result<Value> {
        // Convert messages
        let mut messages: Vec<AnthropicMessage> = unified
            .messages
            .iter()
            .map(Self::unified_to_message)
            .collect();

        // Anthropic API requires all messages to have non-empty content,
        // except for the optional final assistant message (prefill).
        // Fill non-final empty assistant messages with a placeholder to avoid 400 errors.
        if messages.len() > 1 {
            let last_idx = messages.len() - 1;
            for (i, msg) in messages.iter_mut().enumerate() {
                if i == last_idx {
                    continue;
                }
                if msg.role == "assistant" && Self::is_empty_content(&msg.content) {
                    msg.content = AnthropicContent::Text("null".to_string());
                }
            }
        }

        // Convert tools with Bedrock compatibility
        // For Bedrock Claude models, if messages contain tool_use/tool_result but no tools are defined,
        // we need to inject a placeholder tool to avoid ValidationException
        let tools: Option<Vec<AnthropicTool>> = if unified.tools.is_empty() {
            // Check if we need to inject placeholder tool for Bedrock compatibility
            if is_bedrock_claude_model(&unified.model)
                && messages_contain_tool_content(&unified.messages)
            {
                let placeholder = create_placeholder_tool();
                Some(vec![AnthropicTool {
                    name: placeholder.name,
                    description: placeholder.description,
                    input_schema: placeholder.input_schema,
                }])
            } else {
                None
            }
        } else {
            Some(
                unified
                    .tools
                    .iter()
                    .map(|t| AnthropicTool {
                        name: t.name.clone(),
                        description: t.description.clone(),
                        input_schema: t.input_schema.clone(),
                    })
                    .collect(),
            )
        };

        // Build system
        let system = unified
            .system
            .as_ref()
            .map(|s| AnthropicSystem::Text(s.clone()));

        // Extract thinking config
        let thinking = unified
            .parameters
            .extra
            .get("thinking")
            .and_then(|v| serde_json::from_value::<AnthropicThinking>(v.clone()).ok());

        let mut request = json!({
            "model": unified.model,
            "max_tokens": unified.parameters.max_tokens.unwrap_or(4096),
            "messages": messages,
        });

        if let Some(system) = system {
            request["system"] = json!(system);
        }
        if let Some(temp) = unified.parameters.temperature {
            request["temperature"] = json!(temp);
        }
        if let Some(top_p) = unified.parameters.top_p {
            request["top_p"] = json!(top_p);
        }
        if let Some(top_k) = unified.parameters.top_k {
            request["top_k"] = json!(top_k);
        }
        if let Some(ref stop) = unified.parameters.stop_sequences {
            request["stop_sequences"] = json!(stop);
        }
        if unified.parameters.stream {
            request["stream"] = json!(true);
        }
        if let Some(ref tools) = tools {
            request["tools"] = json!(tools);
        }
        if let Some(ref tool_choice) = unified.tool_choice {
            request["tool_choice"] = tool_choice.clone();
        }
        if let Some(ref thinking) = thinking {
            request["thinking"] = json!(thinking);
        }
        if !unified.metadata.is_empty() {
            request["metadata"] = json!(unified.metadata);
        }

        Ok(request)
    }

    fn transform_response_in(&self, raw: Value, original_model: &str) -> Result<UnifiedResponse> {
        let response: AnthropicResponse =
            serde_json::from_value(raw).map_err(|e| AppError::BadRequest(e.to_string()))?;

        // Convert content
        let content: Vec<UnifiedContent> = response
            .content
            .iter()
            .map(Self::content_block_to_unified)
            .collect();

        // Extract tool calls
        let tool_calls: Vec<UnifiedToolCall> = content
            .iter()
            .filter_map(|c| {
                if let UnifiedContent::ToolUse { id, name, input } = c {
                    Some(UnifiedToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: input.clone(),
                    })
                } else {
                    None
                }
            })
            .collect();

        let stop_reason = response
            .stop_reason
            .as_ref()
            .map(|r| Self::stop_reason_to_unified(r));

        let usage = UnifiedUsage {
            input_tokens: response.usage.input_tokens,
            output_tokens: response.usage.output_tokens,
            cache_read_tokens: response.usage.cache_read_input_tokens,
            cache_write_tokens: response.usage.cache_creation_input_tokens,
        };

        Ok(UnifiedResponse {
            id: response.id,
            model: original_model.to_string(),
            content,
            stop_reason,
            usage,
            tool_calls,
        })
    }

    fn transform_response_out(
        &self,
        unified: &UnifiedResponse,
        _client_protocol: Protocol,
    ) -> Result<Value> {
        // Convert content
        let mut content: Vec<AnthropicContentBlock> = unified
            .content
            .iter()
            .filter_map(Self::unified_to_content_block)
            .collect();

        // Convert tool_calls to Anthropic tool_use content blocks
        for tool_call in &unified.tool_calls {
            content.push(AnthropicContentBlock::ToolUse {
                id: tool_call.id.clone(),
                name: tool_call.name.clone(),
                input: tool_call.arguments.clone(),
            });
        }

        let stop_reason = unified
            .stop_reason
            .as_ref()
            .map(Self::unified_to_stop_reason);

        let response = AnthropicResponse {
            id: unified.id.clone(),
            response_type: "message".to_string(),
            role: "assistant".to_string(),
            content,
            model: unified.model.clone(),
            stop_reason: stop_reason.map(|s| s.to_string()),
            stop_sequence: None,
            usage: AnthropicUsage {
                input_tokens: unified.usage.input_tokens,
                output_tokens: unified.usage.output_tokens,
                cache_creation_input_tokens: unified.usage.cache_write_tokens,
                cache_read_input_tokens: unified.usage.cache_read_tokens,
            },
        };

        serde_json::to_value(response).map_err(AppError::Serialization)
    }

    fn transform_stream_chunk_in(&self, chunk: &Bytes) -> Result<Vec<UnifiedStreamChunk>> {
        let chunk_str = std::str::from_utf8(chunk)
            .map_err(|e| AppError::BadRequest(format!("Invalid UTF-8: {}", e)))?;

        let mut chunks = vec![];

        for line in chunk_str.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                let event: AnthropicStreamEvent = serde_json::from_str(data)
                    .map_err(|e| AppError::BadRequest(format!("Invalid JSON: {}", e)))?;

                match event {
                    AnthropicStreamEvent::MessageStart { message } => {
                        let content: Vec<UnifiedContent> = message
                            .content
                            .iter()
                            .map(Self::content_block_to_unified)
                            .collect();

                        let unified_response = UnifiedResponse {
                            id: message.id,
                            model: message.model,
                            content,
                            stop_reason: message
                                .stop_reason
                                .as_ref()
                                .map(|r| Self::stop_reason_to_unified(r)),
                            usage: UnifiedUsage {
                                input_tokens: message.usage.input_tokens,
                                output_tokens: message.usage.output_tokens,
                                cache_read_tokens: message.usage.cache_read_input_tokens,
                                cache_write_tokens: message.usage.cache_creation_input_tokens,
                            },
                            tool_calls: vec![],
                        };
                        chunks.push(UnifiedStreamChunk::message_start(unified_response));
                    }
                    AnthropicStreamEvent::ContentBlockStart {
                        index,
                        content_block,
                    } => {
                        let content = Self::content_block_to_unified(&content_block);
                        chunks.push(UnifiedStreamChunk::content_block_start(
                            index as usize,
                            content,
                        ));
                    }
                    AnthropicStreamEvent::ContentBlockDelta { index, delta } => {
                        let content = match delta {
                            AnthropicDelta::TextDelta { text } => UnifiedContent::text(text),
                            AnthropicDelta::InputJsonDelta { partial_json } => {
                                // Use ToolInputDelta to preserve the semantic meaning
                                UnifiedContent::tool_input_delta(index as usize, partial_json)
                            }
                            AnthropicDelta::ThinkingDelta { thinking } => {
                                UnifiedContent::thinking(thinking, None)
                            }
                            AnthropicDelta::SignatureDelta { signature } => {
                                UnifiedContent::thinking("", Some(signature))
                            }
                        };
                        chunks.push(UnifiedStreamChunk::content_block_delta(
                            index as usize,
                            content,
                        ));
                    }
                    AnthropicStreamEvent::ContentBlockStop { index } => {
                        chunks.push(UnifiedStreamChunk::content_block_stop(index as usize));
                    }
                    AnthropicStreamEvent::MessageDelta { delta, usage } => {
                        let stop_reason = delta
                            .stop_reason
                            .as_ref()
                            .map(|r| Self::stop_reason_to_unified(r))
                            .unwrap_or(StopReason::EndTurn);
                        let unified_usage = UnifiedUsage {
                            input_tokens: usage.input_tokens,
                            output_tokens: usage.output_tokens,
                            cache_read_tokens: usage.cache_read_input_tokens,
                            cache_write_tokens: usage.cache_creation_input_tokens,
                        };
                        chunks.push(UnifiedStreamChunk::message_delta(
                            stop_reason,
                            unified_usage,
                        ));
                    }
                    AnthropicStreamEvent::MessageStop {} => {
                        chunks.push(UnifiedStreamChunk::message_stop());
                    }
                    AnthropicStreamEvent::Ping {} => {
                        chunks.push(UnifiedStreamChunk::ping());
                    }
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
        let event = match chunk.chunk_type {
            ChunkType::MessageStart => {
                if let Some(ref message) = chunk.message {
                    let content: Vec<AnthropicContentBlock> = message
                        .content
                        .iter()
                        .filter_map(Self::unified_to_content_block)
                        .collect();

                    json!({
                        "type": "message_start",
                        "message": {
                            "id": message.id,
                            "type": "message",
                            "role": "assistant",
                            "model": message.model,
                            "content": content,
                            "stop_reason": null,
                            "stop_sequence": null,
                            "usage": {
                                "input_tokens": message.usage.input_tokens,
                                "output_tokens": message.usage.output_tokens
                            }
                        }
                    })
                } else {
                    return Ok(String::new());
                }
            }
            ChunkType::ContentBlockStart => {
                if let Some(ref content) = chunk.content_block {
                    if let Some(block) = Self::unified_to_content_block(content) {
                        json!({
                            "type": "content_block_start",
                            "index": chunk.index,
                            "content_block": block
                        })
                    } else {
                        return Ok(String::new());
                    }
                } else {
                    return Ok(String::new());
                }
            }
            ChunkType::ContentBlockDelta => {
                if let Some(ref delta) = chunk.delta {
                    let delta_json = match delta {
                        UnifiedContent::Text { text } => json!({
                            "type": "text_delta",
                            "text": text
                        }),
                        UnifiedContent::Thinking { text, .. } => json!({
                            "type": "thinking_delta",
                            "thinking": text
                        }),
                        UnifiedContent::ToolInputDelta { partial_json, .. } => json!({
                            "type": "input_json_delta",
                            "partial_json": partial_json
                        }),
                        _ => return Ok(String::new()),
                    };
                    json!({
                        "type": "content_block_delta",
                        "index": chunk.index,
                        "delta": delta_json
                    })
                } else {
                    return Ok(String::new());
                }
            }
            ChunkType::ContentBlockStop => {
                json!({
                    "type": "content_block_stop",
                    "index": chunk.index
                })
            }
            ChunkType::MessageDelta => {
                let stop_reason = chunk.stop_reason.as_ref().map(Self::unified_to_stop_reason);
                let usage = chunk.usage.as_ref().map(|u| {
                    json!({
                        "input_tokens": u.input_tokens,
                        "output_tokens": u.output_tokens
                    })
                });
                json!({
                    "type": "message_delta",
                    "delta": {
                        "stop_reason": stop_reason,
                        "stop_sequence": null
                    },
                    "usage": usage
                })
            }
            ChunkType::MessageStop => {
                json!({ "type": "message_stop" })
            }
            ChunkType::Ping => {
                json!({ "type": "ping" })
            }
        };

        // Use the correct event name with underscores (e.g., "message_start", not "messagestart")
        let event_name = chunk.chunk_type.to_string();
        Ok(format!("event: {}\ndata: {}\n\n", event_name, event))
    }

    fn endpoint(&self) -> &'static str {
        "/v1/messages"
    }

    fn can_handle(&self, raw: &Value) -> bool {
        // Anthropic format indicators
        raw.get("system").is_some()
            || (raw.get("max_tokens").is_some()
                && raw
                    .get("messages")
                    .and_then(|m| m.as_array())
                    .map(|msgs| {
                        msgs.iter().any(|msg| {
                            msg.get("content")
                                .and_then(|c| c.as_array())
                                .map(|arr| {
                                    arr.iter().any(|block| {
                                        let t = block.get("type").and_then(|t| t.as_str());
                                        matches!(
                                            t,
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
                    .unwrap_or(false))
    }
}

// ChunkType Display implementation
impl std::fmt::Display for ChunkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChunkType::MessageStart => write!(f, "message_start"),
            ChunkType::ContentBlockStart => write!(f, "content_block_start"),
            ChunkType::ContentBlockDelta => write!(f, "content_block_delta"),
            ChunkType::ContentBlockStop => write!(f, "content_block_stop"),
            ChunkType::MessageDelta => write!(f, "message_delta"),
            ChunkType::MessageStop => write!(f, "message_stop"),
            ChunkType::Ping => write!(f, "ping"),
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
    fn test_anthropic_transformer_protocol() {
        let transformer = AnthropicTransformer::new();
        assert_eq!(transformer.protocol(), Protocol::Anthropic);
    }

    #[test]
    fn test_transform_request_out() {
        let transformer = AnthropicTransformer::new();
        let raw = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "system": "You are helpful.",
            "messages": [
                {"role": "user", "content": "Hello!"}
            ],
            "temperature": 0.7
        });

        let unified = transformer.transform_request_out(raw).unwrap();
        assert_eq!(unified.model, "claude-3-opus");
        assert_eq!(unified.system, Some("You are helpful.".to_string()));
        assert_eq!(unified.messages.len(), 1);
        assert_eq!(unified.parameters.max_tokens, Some(1024));
    }

    #[test]
    fn test_transform_request_in() {
        let transformer = AnthropicTransformer::new();
        let unified = UnifiedRequest::new("claude-3-opus", vec![UnifiedMessage::user("Hello!")])
            .with_system("Be helpful")
            .with_max_tokens(1024);

        let raw = transformer.transform_request_in(&unified).unwrap();
        assert_eq!(raw["model"], "claude-3-opus");
        assert_eq!(raw["max_tokens"], 1024);
        assert!(raw["system"].is_string());
    }

    #[test]
    fn test_transform_response_in() {
        let transformer = AnthropicTransformer::new();
        let raw = json!({
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello there!"}],
            "model": "claude-3-opus",
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5
            }
        });

        let unified = transformer
            .transform_response_in(raw, "claude-3-opus")
            .unwrap();
        assert_eq!(unified.id, "msg_123");
        assert_eq!(unified.text_content(), "Hello there!");
        assert_eq!(unified.stop_reason, Some(StopReason::EndTurn));
    }

    #[test]
    fn test_can_handle() {
        let transformer = AnthropicTransformer::new();

        // Anthropic format with system
        let request = json!({
            "model": "claude-3",
            "max_tokens": 1024,
            "system": "Be helpful",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        assert!(transformer.can_handle(&request));

        // OpenAI format (no system field, no typed content blocks)
        let request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        assert!(!transformer.can_handle(&request));
    }

    #[test]
    fn test_stop_reason_conversion() {
        assert_eq!(
            AnthropicTransformer::stop_reason_to_unified("end_turn"),
            StopReason::EndTurn
        );
        assert_eq!(
            AnthropicTransformer::stop_reason_to_unified("max_tokens"),
            StopReason::MaxTokens
        );
        assert_eq!(
            AnthropicTransformer::stop_reason_to_unified("tool_use"),
            StopReason::ToolUse
        );
    }

    #[test]
    fn test_streaming_tool_use_input_json_delta_in() {
        let transformer = AnthropicTransformer::new();

        // Test content_block_start for tool_use
        let chunk_start = b"data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_123\",\"name\":\"get_weather\",\"input\":{}}}\n\n";
        let chunks = transformer
            .transform_stream_chunk_in(&Bytes::from_static(chunk_start))
            .unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_type, ChunkType::ContentBlockStart);
        assert_eq!(chunks[0].index, 1);
        if let Some(UnifiedContent::ToolUse { id, name, .. }) = &chunks[0].content_block {
            assert_eq!(id, "toolu_123");
            assert_eq!(name, "get_weather");
        } else {
            panic!("Expected ToolUse content block");
        }

        // Test input_json_delta
        let chunk_delta = b"data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\"}}\n\n";
        let chunks = transformer
            .transform_stream_chunk_in(&Bytes::from_static(chunk_delta))
            .unwrap();
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_type, ChunkType::ContentBlockDelta);
        assert_eq!(chunks[0].index, 1);
        if let Some(UnifiedContent::ToolInputDelta {
            index,
            partial_json,
        }) = &chunks[0].delta
        {
            assert_eq!(*index, 1);
            assert_eq!(partial_json, "{\"city\":");
        } else {
            panic!("Expected ToolInputDelta content, got {:?}", chunks[0].delta);
        }
    }

    #[test]
    fn test_streaming_tool_use_input_json_delta_out() {
        let transformer = AnthropicTransformer::new();

        // Test outputting input_json_delta
        let chunk = UnifiedStreamChunk::content_block_delta(
            1,
            UnifiedContent::tool_input_delta(1, "{\"city\":"),
        );
        let output = transformer
            .transform_stream_chunk_out(&chunk, Protocol::Anthropic)
            .unwrap();

        assert!(output.contains("content_block_delta"));
        assert!(output.contains("input_json_delta"));
        assert!(output.contains("{\\\"city\\\":"));
    }

    #[test]
    fn test_streaming_tool_use_content_block_start_out() {
        let transformer = AnthropicTransformer::new();

        // Test outputting content_block_start for tool_use
        let chunk = UnifiedStreamChunk::content_block_start(
            1,
            UnifiedContent::tool_use("toolu_123", "get_weather", json!({})),
        );
        let output = transformer
            .transform_stream_chunk_out(&chunk, Protocol::Anthropic)
            .unwrap();

        assert!(output.contains("content_block_start"));
        assert!(output.contains("tool_use"));
        assert!(output.contains("toolu_123"));
        assert!(output.contains("get_weather"));
    }

    #[test]
    fn test_streaming_message_delta_partial_usage_in() {
        let transformer = AnthropicTransformer::new();

        // Anthropic message_delta events only include output_tokens in usage,
        // not input_tokens. This must parse without error.
        let chunk = b"event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":25}}\n\n";
        let chunks = transformer
            .transform_stream_chunk_in(&Bytes::from_static(chunk))
            .unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_type, ChunkType::MessageDelta);
        assert_eq!(chunks[0].stop_reason, Some(StopReason::EndTurn));
        let usage = chunks[0].usage.as_ref().unwrap();
        assert_eq!(usage.output_tokens, 25);
        assert_eq!(usage.input_tokens, 0);
    }

    #[test]
    fn test_streaming_ping_event_in() {
        let transformer = AnthropicTransformer::new();

        // Test parsing ping event from Anthropic stream
        let chunk = b"data: {\"type\":\"ping\"}\n\n";
        let chunks = transformer
            .transform_stream_chunk_in(&Bytes::from_static(chunk))
            .unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_type, ChunkType::Ping);
    }

    #[test]
    fn test_streaming_ping_event_out() {
        let transformer = AnthropicTransformer::new();

        // Test outputting ping event
        let chunk = UnifiedStreamChunk::ping();
        let output = transformer
            .transform_stream_chunk_out(&chunk, Protocol::Anthropic)
            .unwrap();

        // Verify the output format matches Anthropic's ping event format
        assert!(output.contains("event: ping"));
        assert!(output.contains("\"type\":\"ping\""));
    }

    // ========================================================================
    // Bedrock Compatibility Tests
    // ========================================================================

    #[test]
    fn test_is_bedrock_claude_model() {
        // Valid Bedrock Claude models
        assert!(is_bedrock_claude_model("claude-3-opus-bedrock"));
        assert!(is_bedrock_claude_model("claude-3-sonnet-bedrock"));
        assert!(is_bedrock_claude_model("claude-3-haiku-bedrock"));
        assert!(is_bedrock_claude_model("claude-4.5-opus-bedrock"));
        assert!(is_bedrock_claude_model("claude-3.5-sonnet-bedrock"));

        // Non-Bedrock Claude models
        assert!(!is_bedrock_claude_model("claude-3-opus"));
        assert!(!is_bedrock_claude_model("claude-3-sonnet"));
        assert!(!is_bedrock_claude_model("claude-4.5-opus"));

        // Non-Claude models with bedrock suffix
        assert!(!is_bedrock_claude_model("gpt-4-bedrock"));
        assert!(!is_bedrock_claude_model("llama-3-bedrock"));

        // Edge cases
        assert!(!is_bedrock_claude_model("bedrock"));
        assert!(!is_bedrock_claude_model("claude-"));
        assert!(!is_bedrock_claude_model("-bedrock"));
        assert!(!is_bedrock_claude_model(""));
    }

    #[test]
    fn test_messages_contain_tool_content_with_tool_use() {
        let messages = vec![
            UnifiedMessage::user("Hello"),
            UnifiedMessage::with_content(
                Role::Assistant,
                vec![UnifiedContent::tool_use(
                    "tool_1",
                    "get_weather",
                    json!({"city": "Tokyo"}),
                )],
            ),
        ];
        assert!(messages_contain_tool_content(&messages));
    }

    #[test]
    fn test_messages_contain_tool_content_with_tool_result() {
        let messages = vec![
            UnifiedMessage::user("Hello"),
            UnifiedMessage::with_content(
                Role::User,
                vec![UnifiedContent::tool_result(
                    "tool_1",
                    json!({"temp": 25}),
                    false,
                )],
            ),
        ];
        assert!(messages_contain_tool_content(&messages));
    }

    #[test]
    fn test_messages_contain_tool_content_without_tools() {
        let messages = vec![
            UnifiedMessage::user("Hello"),
            UnifiedMessage::assistant("Hi there!"),
        ];
        assert!(!messages_contain_tool_content(&messages));
    }

    #[test]
    fn test_bedrock_model_with_tool_content_injects_placeholder() {
        let transformer = AnthropicTransformer::new();

        // Create a request with tool_use content but no tools defined
        let mut unified = UnifiedRequest::new(
            "claude-3-opus-bedrock",
            vec![
                UnifiedMessage::user("What's the weather?"),
                UnifiedMessage::with_content(
                    Role::Assistant,
                    vec![UnifiedContent::tool_use(
                        "tool_1",
                        "get_weather",
                        json!({"city": "Tokyo"}),
                    )],
                ),
                UnifiedMessage::with_content(
                    Role::User,
                    vec![UnifiedContent::tool_result(
                        "tool_1",
                        json!({"temp": 25}),
                        false,
                    )],
                ),
            ],
        );
        unified.parameters.max_tokens = Some(1024);

        let raw = transformer.transform_request_in(&unified).unwrap();

        // Verify placeholder tool was injected
        assert!(raw.get("tools").is_some());
        let tools = raw["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "_placeholder_tool");
        assert_eq!(
            tools[0]["description"],
            "Placeholder tool for Bedrock compatibility"
        );
    }

    #[test]
    fn test_non_bedrock_model_with_tool_content_no_injection() {
        let transformer = AnthropicTransformer::new();

        // Create a request with tool_use content but no tools defined for non-Bedrock model
        let mut unified = UnifiedRequest::new(
            "claude-3-opus",
            vec![
                UnifiedMessage::user("What's the weather?"),
                UnifiedMessage::with_content(
                    Role::Assistant,
                    vec![UnifiedContent::tool_use(
                        "tool_1",
                        "get_weather",
                        json!({"city": "Tokyo"}),
                    )],
                ),
                UnifiedMessage::with_content(
                    Role::User,
                    vec![UnifiedContent::tool_result(
                        "tool_1",
                        json!({"temp": 25}),
                        false,
                    )],
                ),
            ],
        );
        unified.parameters.max_tokens = Some(1024);

        let raw = transformer.transform_request_in(&unified).unwrap();

        // Verify no tools were injected for non-Bedrock model
        assert!(raw.get("tools").is_none());
    }

    #[test]
    fn test_bedrock_model_with_existing_tools_no_injection() {
        let transformer = AnthropicTransformer::new();

        // Create a request with existing tools defined
        let mut unified = UnifiedRequest::new(
            "claude-3-opus-bedrock",
            vec![
                UnifiedMessage::user("What's the weather?"),
                UnifiedMessage::with_content(
                    Role::Assistant,
                    vec![UnifiedContent::tool_use(
                        "tool_1",
                        "get_weather",
                        json!({"city": "Tokyo"}),
                    )],
                ),
            ],
        );
        unified.parameters.max_tokens = Some(1024);
        unified.tools = vec![UnifiedTool {
            name: "get_weather".to_string(),
            description: Some("Get weather for a city".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "city": {"type": "string"}
                }
            }),
            tool_type: Some("function".to_string()),
        }];

        let raw = transformer.transform_request_in(&unified).unwrap();

        // Verify existing tools are preserved, not replaced with placeholder
        let tools = raw["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_weather");
        assert_ne!(tools[0]["name"], "_placeholder_tool");
    }

    #[test]
    fn test_bedrock_model_without_tool_content_no_injection() {
        let transformer = AnthropicTransformer::new();

        // Create a simple request without any tool content
        let mut unified = UnifiedRequest::new(
            "claude-3-opus-bedrock",
            vec![UnifiedMessage::user("Hello!")],
        );
        unified.parameters.max_tokens = Some(1024);

        let raw = transformer.transform_request_in(&unified).unwrap();

        // Verify no tools were injected when there's no tool content
        assert!(raw.get("tools").is_none());
    }

    #[test]
    fn test_create_placeholder_tool() {
        let placeholder = create_placeholder_tool();
        assert_eq!(placeholder.name, "_placeholder_tool");
        assert_eq!(
            placeholder.description,
            Some("Placeholder tool for Bedrock compatibility".to_string())
        );
        assert_eq!(placeholder.input_schema["type"], "object");
        assert!(placeholder.input_schema["properties"].is_object());
    }

    // ========================================================================
    // Billing Header Stripping Tests
    // ========================================================================

    #[test]
    fn test_strip_billing_header() {
        // Test with billing header prefix
        assert_eq!(
            strip_billing_header(
                "x-anthropic-billing-header: cc_version=2.1.17.f12; cc_entrypoint=cli"
            ),
            "cc_version=2.1.17.f12; cc_entrypoint=cli"
        );

        // Test without billing header prefix
        assert_eq!(
            strip_billing_header("normal text without header"),
            "normal text without header"
        );

        // Test with extra spaces after colon
        assert_eq!(
            strip_billing_header("x-anthropic-billing-header:   value"),
            "value"
        );
    }

    #[test]
    fn test_extract_system_with_billing_header_text() {
        // Test with Text variant containing billing header
        let system = Some(AnthropicSystem::Text(
            "x-anthropic-billing-header: cc_version=2.1.17.f12; cc_entrypoint=cli".to_string(),
        ));
        let result = AnthropicTransformer::extract_system(&system);
        assert_eq!(
            result,
            Some("cc_version=2.1.17.f12; cc_entrypoint=cli".to_string())
        );
    }

    #[test]
    fn test_extract_system_with_billing_header_blocks() {
        // Test with Blocks variant containing billing header
        let system = Some(AnthropicSystem::Blocks(vec![
            AnthropicSystemBlock {
                block_type: "text".to_string(),
                text: "x-anthropic-billing-header: cc_version=2.1.17.f12; cc_entrypoint=cli"
                    .to_string(),
            },
            AnthropicSystemBlock {
                block_type: "text".to_string(),
                text: "You are a helpful assistant.".to_string(),
            },
        ]));
        let result = AnthropicTransformer::extract_system(&system);
        assert_eq!(
            result,
            Some(
                "cc_version=2.1.17.f12; cc_entrypoint=cli\nYou are a helpful assistant."
                    .to_string()
            )
        );
    }

    #[test]
    fn test_transform_request_out_with_billing_header() {
        let transformer = AnthropicTransformer::new();
        let raw = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "system": [
                {
                    "type": "text",
                    "text": "x-anthropic-billing-header: cc_version=2.1.17.f12; cc_entrypoint=cli"
                },
                {
                    "type": "text",
                    "text": "You are helpful."
                }
            ],
            "messages": [
                {"role": "user", "content": "Hello!"}
            ]
        });

        let unified = transformer.transform_request_out(raw).unwrap();
        // The billing header should be stripped from the system prompt
        assert_eq!(
            unified.system,
            Some("cc_version=2.1.17.f12; cc_entrypoint=cli\nYou are helpful.".to_string())
        );
    }

    #[test]
    fn test_unified_to_message_tool_calls_to_tool_use() {
        // When a UIF message has tool_calls (from OpenAI format),
        // they should be converted to tool_use content blocks in Anthropic format.
        let msg = UnifiedMessage {
            role: Role::Assistant,
            content: vec![],
            name: None,
            tool_calls: vec![UnifiedToolCall {
                id: "call_1".to_string(),
                name: "get_weather".to_string(),
                arguments: json!({"city": "SF"}),
            }],
            tool_call_id: None,
        };

        let anthropic_msg = AnthropicTransformer::unified_to_message(&msg);
        assert_eq!(anthropic_msg.role, "assistant");
        if let AnthropicContent::Blocks(blocks) = &anthropic_msg.content {
            assert_eq!(blocks.len(), 1);
            match &blocks[0] {
                AnthropicContentBlock::ToolUse { id, name, input } => {
                    assert_eq!(id, "call_1");
                    assert_eq!(name, "get_weather");
                    assert_eq!(input, &json!({"city": "SF"}));
                }
                _ => panic!("Expected ToolUse block"),
            }
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn test_unified_to_message_no_duplicate_tool_use() {
        // When content already contains ToolUse (Anthropic→Anthropic path),
        // tool_calls should not create duplicates.
        let msg = UnifiedMessage {
            role: Role::Assistant,
            content: vec![UnifiedContent::ToolUse {
                id: "call_1".to_string(),
                name: "get_weather".to_string(),
                input: json!({"city": "SF"}),
            }],
            name: None,
            tool_calls: vec![UnifiedToolCall {
                id: "call_1".to_string(),
                name: "get_weather".to_string(),
                arguments: json!({"city": "SF"}),
            }],
            tool_call_id: None,
        };

        let anthropic_msg = AnthropicTransformer::unified_to_message(&msg);
        if let AnthropicContent::Blocks(blocks) = &anthropic_msg.content {
            let tool_use_count = blocks
                .iter()
                .filter(|b| matches!(b, AnthropicContentBlock::ToolUse { .. }))
                .count();
            assert_eq!(tool_use_count, 1, "Should not duplicate tool_use blocks");
        } else {
            panic!("Expected Blocks content");
        }
    }

    #[test]
    fn test_openai_to_anthropic_tool_conversation() {
        // Full OpenAI→Anthropic cross-protocol tool conversation.
        use super::super::openai::OpenAITransformer;

        let openai = OpenAITransformer::new();
        let anthropic = AnthropicTransformer::new();

        let request = json!({
            "model": "claude-3-opus",
            "messages": [
                {"role": "user", "content": "What's the weather in SF?"},
                {"role": "assistant", "content": null, "tool_calls": [
                    {"id": "call_1", "type": "function", "function": {"name": "get_weather", "arguments": "{\"city\":\"SF\"}"}}
                ]},
                {"role": "tool", "content": "Sunny, 72°F", "tool_call_id": "call_1"}
            ],
            "tools": [{"type": "function", "function": {"name": "get_weather", "description": "Get weather", "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}}}]
        });

        // OpenAI → Unified
        let unified = openai.transform_request_out(request).unwrap();

        // Unified → Anthropic
        let anthropic_req = anthropic.transform_request_in(&unified).unwrap();

        let messages = anthropic_req["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);

        // messages[0]: user
        assert_eq!(messages[0]["role"], "user");

        // messages[1]: assistant with tool_use
        assert_eq!(messages[1]["role"], "assistant");
        let content1 = messages[1]["content"].as_array().unwrap();
        assert_eq!(content1.len(), 1);
        assert_eq!(content1[0]["type"], "tool_use");
        assert_eq!(content1[0]["id"], "call_1");
        assert_eq!(content1[0]["name"], "get_weather");

        // messages[2]: user with tool_result
        assert_eq!(messages[2]["role"], "user");
        let content2 = messages[2]["content"].as_array().unwrap();
        assert_eq!(content2.len(), 1);
        assert_eq!(content2[0]["type"], "tool_result");
        assert_eq!(content2[0]["tool_use_id"], "call_1");
    }
}
