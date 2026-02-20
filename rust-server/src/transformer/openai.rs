//! OpenAI protocol transformer.
//!
//! Handles conversion between OpenAI Chat Completions API format and
//! the Unified Internal Format.

use super::{
    ChunkType, Protocol, Result, Role, StopReason, Transformer, UnifiedContent, UnifiedMessage,
    UnifiedParameters, UnifiedRequest, UnifiedResponse, UnifiedStreamChunk, UnifiedTool,
    UnifiedToolCall, UnifiedUsage,
};
use crate::core::AppError;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;

// ============================================================================
// OpenAI Request/Response Types
// ============================================================================

/// OpenAI message format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<OpenAIContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_blocks: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_specific_fields: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// OpenAI content can be string or array of content parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OpenAIContent {
    Text(String),
    Parts(Vec<OpenAIContentPart>),
}

/// OpenAI content part for multimodal messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OpenAIContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: OpenAIImageUrl },
}

/// OpenAI image URL structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// OpenAI tool call structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: OpenAIFunctionCall,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_specific_fields: Option<Value>,
}

/// OpenAI function call structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIFunctionCall {
    pub name: String,
    pub arguments: String,
}

/// OpenAI tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAITool {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: OpenAIFunction,
}

/// OpenAI function definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIFunction {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

/// OpenAI chat completion request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChatRequest {
    pub model: String,
    pub messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<OpenAITool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// OpenAI chat completion response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChatResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<OpenAIChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAIUsage>,
}

/// OpenAI choice structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIChoice {
    pub index: i32,
    pub message: OpenAIMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// OpenAI usage structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIUsage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

/// OpenAI streaming chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIStreamChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<OpenAIStreamChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<OpenAIUsage>,
}

/// OpenAI streaming choice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIStreamChoice {
    pub index: i32,
    pub delta: OpenAIDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// OpenAI delta content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_blocks: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_specific_fields: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAIDeltaToolCall>>,
}

/// OpenAI delta tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIDeltaToolCall {
    pub index: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub call_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<OpenAIDeltaFunction>,
}

/// OpenAI delta function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAIDeltaFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ============================================================================
// OpenAI Transformer Implementation
// ============================================================================

/// OpenAI protocol transformer.
pub struct OpenAITransformer;

impl OpenAITransformer {
    /// Create a new OpenAI transformer.
    pub fn new() -> Self {
        OpenAITransformer
    }

    /// Convert OpenAI message to unified message.
    fn message_to_unified(msg: &OpenAIMessage) -> Result<UnifiedMessage> {
        let role = msg.role.parse().unwrap_or(Role::User);

        let content = match &msg.content {
            Some(OpenAIContent::Text(text)) => vec![UnifiedContent::text(text)],
            Some(OpenAIContent::Parts(parts)) => {
                parts
                    .iter()
                    .map(|part| match part {
                        OpenAIContentPart::Text { text } => UnifiedContent::text(text),
                        OpenAIContentPart::ImageUrl { image_url } => {
                            // Check if it's a data URL (base64)
                            if image_url.url.starts_with("data:") {
                                // Parse data URL: data:image/jpeg;base64,/9j/4AAQ...
                                let parts: Vec<&str> = image_url.url.splitn(2, ',').collect();
                                if parts.len() == 2 {
                                    let media_type = parts[0]
                                        .trim_start_matches("data:")
                                        .split(';')
                                        .next()
                                        .unwrap_or("image/jpeg");
                                    UnifiedContent::image_base64(media_type, parts[1])
                                } else {
                                    UnifiedContent::image_url(&image_url.url)
                                }
                            } else {
                                UnifiedContent::image_url(&image_url.url)
                            }
                        }
                    })
                    .collect()
            }
            None => vec![],
        };

        let mut tc_signatures: Vec<String> = Vec::new();
        let tool_calls = msg
            .tool_calls
            .as_ref()
            .map(|calls| {
                calls
                    .iter()
                    .map(|tc| {
                        let arguments: Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                        // Extract thought_signature from tool_call provider_specific_fields
                        if let Some(ref psf) = tc.provider_specific_fields {
                            if let Some(sig) = psf.get("thought_signature").and_then(|s| s.as_str())
                            {
                                tc_signatures.push(sig.to_string());
                            }
                        }
                        // Strip __thought__ signature from tool_call_id if present
                        let id = if let Some((base, sig)) = tc
                            .id
                            .split_once(crate::api::gemini3::THOUGHT_SIGNATURE_SEPARATOR)
                        {
                            if !sig.is_empty() && !tc_signatures.iter().any(|s| s == sig) {
                                tc_signatures.push(sig.to_string());
                            }
                            base.to_string()
                        } else {
                            tc.id.clone()
                        };
                        UnifiedToolCall {
                            id,
                            name: tc.function.name.clone(),
                            arguments,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Add reasoning_content as Thinking content if present
        let mut content = content;
        if let Some(ref reasoning) = msg.reasoning_content {
            if !reasoning.is_empty() {
                content.insert(0, UnifiedContent::thinking(reasoning, None));
            }
        }
        // Parse thinking_blocks for structured thinking content with signatures
        if let Some(ref blocks) = msg.thinking_blocks {
            for block in blocks {
                if block.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                    let text = block.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                    let sig = block
                        .get("signature")
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string());
                    if sig.is_some() && !text.is_empty() {
                        // thinking_blocks with both text+sig: add thinking then signature
                        content.insert(0, UnifiedContent::thinking(text, None));
                        content.push(UnifiedContent::thinking("", sig));
                    } else if sig.is_some() {
                        // Signature-only block
                        content.push(UnifiedContent::thinking("", sig));
                    }
                    // text-only blocks already covered by reasoning_content above
                }
            }
        }
        // Parse provider_specific_fields.thought_signatures as signature-only blocks
        // (fallback if no thinking_blocks with signatures)
        let has_signature = content.iter().any(|c| matches!(c, UnifiedContent::Thinking { text, signature } if text.is_empty() && signature.is_some()));
        if !has_signature {
            if let Some(ref psf) = msg.provider_specific_fields {
                if let Some(sigs) = psf.get("thought_signatures").and_then(|s| s.as_array()) {
                    for sig_val in sigs {
                        if let Some(sig) = sig_val.as_str() {
                            content.push(UnifiedContent::thinking("", Some(sig.to_string())));
                        }
                    }
                }
            }
        }
        // Extract signatures from tool_call_ids (__thought__ encoding)
        // and tool_call provider_specific_fields.thought_signature as final fallback
        let has_signature_now = content.iter().any(|c| matches!(c, UnifiedContent::Thinking { text, signature } if text.is_empty() && signature.is_some()));
        if !has_signature_now {
            for sig in &tc_signatures {
                content.push(UnifiedContent::thinking("", Some(sig.clone())));
            }
        }

        // Strip __thought__ from tool_call_id if present (tool result messages)
        let tool_call_id = msg.tool_call_id.as_ref().map(|id| {
            if id.contains(crate::api::gemini3::THOUGHT_SIGNATURE_SEPARATOR) {
                id.split(crate::api::gemini3::THOUGHT_SIGNATURE_SEPARATOR)
                    .next()
                    .unwrap_or(id)
                    .to_string()
            } else {
                id.clone()
            }
        });

        Ok(UnifiedMessage {
            role,
            content,
            name: msg.name.clone(),
            tool_calls,
            tool_call_id,
        })
    }

    /// Convert unified message to OpenAI message.
    fn unified_to_message(msg: &UnifiedMessage) -> OpenAIMessage {
        let role = msg.role.to_string();

        // Build content
        let content = if msg.content.is_empty() {
            None
        } else if msg.content.len() == 1 {
            // Single text content - use simple string format
            if let Some(text) = msg.content[0].as_text() {
                Some(OpenAIContent::Text(text.to_string()))
            } else {
                Some(OpenAIContent::Parts(
                    msg.content
                        .iter()
                        .filter_map(Self::unified_to_content_part)
                        .collect(),
                ))
            }
        } else {
            // Multiple content blocks - use array format
            Some(OpenAIContent::Parts(
                msg.content
                    .iter()
                    .filter_map(Self::unified_to_content_part)
                    .collect(),
            ))
        };

        // Build tool calls
        let tool_calls = if msg.tool_calls.is_empty() {
            None
        } else {
            Some(
                msg.tool_calls
                    .iter()
                    .map(|tc| OpenAIToolCall {
                        id: tc.id.clone(),
                        call_type: "function".to_string(),
                        function: OpenAIFunctionCall {
                            name: tc.name.clone(),
                            arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                        },
                        provider_specific_fields: None,
                    })
                    .collect(),
            )
        };

        OpenAIMessage {
            role,
            content,
            reasoning_content: None,
            thinking_blocks: None,
            provider_specific_fields: None,
            name: msg.name.clone(),
            tool_calls,
            tool_call_id: msg.tool_call_id.clone(),
        }
    }

    /// Convert unified content to OpenAI content part.
    fn unified_to_content_part(content: &UnifiedContent) -> Option<OpenAIContentPart> {
        match content {
            UnifiedContent::Text { text } => Some(OpenAIContentPart::Text { text: text.clone() }),
            UnifiedContent::Image {
                source_type,
                media_type,
                data,
            } => {
                let url = if source_type == "base64" {
                    format!("data:{};base64,{}", media_type, data)
                } else {
                    data.clone()
                };
                Some(OpenAIContentPart::ImageUrl {
                    image_url: OpenAIImageUrl { url, detail: None },
                })
            }
            // Other content types don't have direct OpenAI equivalents in content
            _ => None,
        }
    }

    /// Convert Anthropic tool_result content to OpenAI-compatible string.
    ///
    /// Anthropic tool_result content can be: string, null, or structured array
    /// of content blocks like `[{type:"text",text:"..."}, {type:"image",...}]`.
    /// OpenAI tool messages only support string content, so we serialize
    /// structured content to preserve as much information as possible.
    fn tool_result_content_to_string(content: &Value, is_error: bool) -> String {
        let text = match content {
            Value::String(s) => s.clone(),
            Value::Null => String::new(),
            Value::Array(arr) => arr
                .iter()
                .filter_map(|block| {
                    block
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                })
                .collect::<Vec<_>>()
                .join("\n"),
            other => serde_json::to_string(other).unwrap_or_default(),
        };
        if is_error && !text.is_empty() {
            format!("[Error] {}", text)
        } else {
            text
        }
    }

    /// Convert OpenAI finish reason to unified stop reason.
    fn finish_reason_to_stop_reason(reason: &str) -> StopReason {
        match reason {
            "stop" => StopReason::EndTurn,
            "length" => StopReason::MaxTokens,
            "tool_calls" => StopReason::ToolUse,
            "content_filter" => StopReason::ContentFilter,
            _ => StopReason::EndTurn,
        }
    }

    /// Convert unified stop reason to OpenAI finish reason.
    fn stop_reason_to_finish_reason(reason: &StopReason) -> &'static str {
        match reason {
            StopReason::EndTurn => "stop",
            StopReason::MaxTokens | StopReason::Length => "length",
            StopReason::StopSequence => "stop",
            StopReason::ToolUse => "tool_calls",
            StopReason::ContentFilter => "content_filter",
        }
    }
}

impl Default for OpenAITransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAITransformer {
    /// Normalize OpenAI tool_choice to UIF dict format.
    ///
    /// OpenAI formats:
    /// - "auto" -> {"type": "auto"}
    /// - "none" -> {"type": "none"}
    /// - "required" -> {"type": "any"}
    /// - {"type": "function", "function": {"name": "xxx"}} -> {"type": "tool", "name": "xxx"}
    fn normalize_tool_choice_to_uif(tool_choice: Option<Value>) -> Option<Value> {
        tool_choice.map(|tc| {
            if let Some(s) = tc.as_str() {
                // String format: "auto", "none", "required"
                match s {
                    "auto" => json!({"type": "auto"}),
                    "none" => json!({"type": "none"}),
                    "required" => json!({"type": "any"}),
                    other => json!({"type": other}),
                }
            } else if let Some(obj) = tc.as_object() {
                // Object format: already a dict, possibly needs normalization
                if obj.get("type").and_then(|t| t.as_str()) == Some("function") {
                    // OpenAI specific function format
                    // {"type": "function", "function": {"name": "xxx"}} -> {"type": "tool", "name": "xxx"}
                    if let Some(name) = obj
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                    {
                        return json!({"type": "tool", "name": name});
                    }
                }
                tc
            } else {
                tc
            }
        })
    }
}

impl Transformer for OpenAITransformer {
    fn protocol(&self) -> Protocol {
        Protocol::OpenAI
    }

    fn transform_request_out(&self, raw: Value) -> Result<UnifiedRequest> {
        let request: OpenAIChatRequest =
            serde_json::from_value(raw).map_err(|e| AppError::BadRequest(e.to_string()))?;

        // Convert messages
        let messages: Vec<UnifiedMessage> = request
            .messages
            .iter()
            .map(Self::message_to_unified)
            .collect::<Result<Vec<_>>>()?;

        // Extract system message if present
        let system = messages
            .iter()
            .find(|m| m.role == Role::System)
            .map(|m| m.text_content());

        // Filter out system messages from the message list
        let messages: Vec<UnifiedMessage> = messages
            .into_iter()
            .filter(|m| m.role != Role::System)
            .collect();

        // Convert tools
        let tools: Vec<UnifiedTool> = request
            .tools
            .unwrap_or_default()
            .into_iter()
            .map(|t| UnifiedTool {
                name: t.function.name,
                description: t.function.description,
                input_schema: t.function.parameters,
                tool_type: Some(t.tool_type),
            })
            .collect();

        // Build parameters
        let parameters = UnifiedParameters {
            temperature: request.temperature,
            max_tokens: request.max_tokens.or(request.max_completion_tokens),
            top_p: request.top_p,
            top_k: None,
            stop_sequences: request.stop,
            stream: request.stream.unwrap_or(false),
            extra: request.extra,
        };

        // Normalize tool_choice from OpenAI format to UIF format
        let tool_choice = Self::normalize_tool_choice_to_uif(request.tool_choice);

        Ok(UnifiedRequest {
            model: request.model,
            messages,
            system,
            parameters,
            tools,
            tool_choice,
            request_id: uuid::Uuid::new_v4().to_string(),
            client_protocol: Protocol::OpenAI,
            metadata: HashMap::new(),
        })
    }

    fn transform_request_in(&self, unified: &UnifiedRequest) -> Result<Value> {
        // Convert messages back to OpenAI format
        let mut messages: Vec<OpenAIMessage> = vec![];

        // Add system message if present
        if let Some(ref system) = unified.system {
            messages.push(OpenAIMessage {
                role: "system".to_string(),
                content: Some(OpenAIContent::Text(system.clone())),
                reasoning_content: None,
                thinking_blocks: None,
                provider_specific_fields: None,
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Add other messages
        for msg in &unified.messages {
            // Check if message contains ToolResult content blocks (from Anthropic format).
            // Anthropic puts multiple tool_results in a single user message,
            // but OpenAI requires each as a separate {role: "tool"} message.
            let has_tool_results = msg
                .content
                .iter()
                .any(|c| matches!(c, UnifiedContent::ToolResult { .. }));

            if has_tool_results {
                // Emit each tool_result FIRST as an independent role: "tool" message.
                // This must come before any non-tool-result user content to maintain
                // the assistant(tool_calls) → tool(result) adjacency required by
                // downstream providers (e.g. Bedrock Converse).
                for content in &msg.content {
                    if let UnifiedContent::ToolResult {
                        tool_use_id,
                        content: result_content,
                        is_error,
                    } = content
                    {
                        let content_str =
                            Self::tool_result_content_to_string(result_content, *is_error);
                        messages.push(OpenAIMessage {
                            role: "tool".to_string(),
                            content: Some(OpenAIContent::Text(content_str)),
                            reasoning_content: None,
                            thinking_blocks: None,
                            provider_specific_fields: None,
                            name: None,
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id.clone()),
                        });
                    }
                }

                // Emit non-tool-result content as a user message after tool results
                let non_tool_parts: Vec<OpenAIContentPart> = msg
                    .content
                    .iter()
                    .filter(|c| !matches!(c, UnifiedContent::ToolResult { .. }))
                    .filter_map(Self::unified_to_content_part)
                    .collect();

                if !non_tool_parts.is_empty() {
                    messages.push(OpenAIMessage {
                        role: "user".to_string(),
                        content: Some(OpenAIContent::Parts(non_tool_parts)),
                        reasoning_content: None,
                        thinking_blocks: None,
                        provider_specific_fields: None,
                        name: msg.name.clone(),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
            } else {
                messages.push(Self::unified_to_message(msg));
            }
        }

        // Convert tools
        let tools: Option<Vec<OpenAITool>> = if unified.tools.is_empty() {
            None
        } else {
            Some(
                unified
                    .tools
                    .iter()
                    .map(|t| OpenAITool {
                        tool_type: t
                            .tool_type
                            .clone()
                            .unwrap_or_else(|| "function".to_string()),
                        function: OpenAIFunction {
                            name: t.name.clone(),
                            description: t.description.clone(),
                            parameters: t.input_schema.clone(),
                        },
                    })
                    .collect(),
            )
        };

        let mut request = json!({
            "model": unified.model,
            "messages": messages,
        });

        // Add optional parameters
        if let Some(temp) = unified.parameters.temperature {
            request["temperature"] = json!(temp);
        }
        if let Some(max_tokens) = unified.parameters.max_tokens {
            request["max_tokens"] = json!(max_tokens);
        }
        if let Some(top_p) = unified.parameters.top_p {
            request["top_p"] = json!(top_p);
        }
        if let Some(ref stop) = unified.parameters.stop_sequences {
            request["stop"] = json!(stop);
        }
        if unified.parameters.stream {
            request["stream"] = json!(true);
        }
        if let Some(ref tools) = tools {
            request["tools"] = json!(tools);
        }
        if let Some(ref tool_choice) = unified.tool_choice {
            // Convert Anthropic tool_choice format to OpenAI format
            // Anthropic: {"type": "auto"} | {"type": "any"} | {"type": "tool", "name": "xxx"}
            // OpenAI: "auto" | "none" | "required" | {"type": "function", "function": {"name": "xxx"}}
            let openai_tool_choice =
                if let Some(tc_type) = tool_choice.get("type").and_then(|t| t.as_str()) {
                    match tc_type {
                        "auto" => json!("auto"),
                        "any" => json!("required"),
                        "none" => json!("none"),
                        "tool" => {
                            // Anthropic {"type": "tool", "name": "xxx"} -> OpenAI {"type": "function", "function": {"name": "xxx"}}
                            if let Some(name) = tool_choice.get("name").and_then(|n| n.as_str()) {
                                json!({
                                    "type": "function",
                                    "function": {"name": name}
                                })
                            } else {
                                tool_choice.clone()
                            }
                        }
                        _ => tool_choice.clone(),
                    }
                } else if tool_choice.is_string() {
                    // Already in OpenAI string format
                    tool_choice.clone()
                } else {
                    tool_choice.clone()
                };
            request["tool_choice"] = openai_tool_choice;
        }

        // Add extra parameters
        for (key, value) in &unified.parameters.extra {
            request[key] = value.clone();
        }

        Ok(request)
    }

    fn transform_response_in(&self, raw: Value, original_model: &str) -> Result<UnifiedResponse> {
        let response: OpenAIChatResponse =
            serde_json::from_value(raw).map_err(|e| AppError::BadRequest(e.to_string()))?;

        let choice = response
            .choices
            .first()
            .ok_or_else(|| AppError::BadRequest("No choices in response".to_string()))?;

        // Reuse message_to_unified to parse content, thinking_blocks,
        // provider_specific_fields, and tool calls consistently
        let unified_msg = Self::message_to_unified(&choice.message)?;

        let stop_reason = choice
            .finish_reason
            .as_ref()
            .map(|r| Self::finish_reason_to_stop_reason(r));

        let usage = response
            .usage
            .map(|u| UnifiedUsage::new(u.prompt_tokens, u.completion_tokens))
            .unwrap_or_default();

        Ok(UnifiedResponse {
            id: response.id,
            model: original_model.to_string(),
            content: unified_msg.content,
            stop_reason,
            usage,
            tool_calls: unified_msg.tool_calls,
        })
    }

    fn transform_response_out(
        &self,
        unified: &UnifiedResponse,
        _client_protocol: Protocol,
    ) -> Result<Value> {
        // Convert content to OpenAI format - separate text and thinking content
        let mut text_parts: Vec<OpenAIContentPart> = Vec::new();
        let mut reasoning_content: Option<String> = None;
        let mut thinking_blocks: Vec<Value> = Vec::new();
        let mut thought_signatures: Vec<String> = Vec::new();

        for c in &unified.content {
            match c {
                UnifiedContent::Text { text } => {
                    text_parts.push(OpenAIContentPart::Text { text: text.clone() });
                }
                UnifiedContent::Thinking { text, signature } => {
                    if text.is_empty() && signature.is_some() {
                        // Signature-only block — collect for thinking_blocks and provider_specific_fields
                        let sig = signature.as_ref().unwrap();
                        thought_signatures.push(sig.clone());
                        // If there's already a thinking block without signature, attach to it
                        if let Some(last_block) = thinking_blocks.last_mut() {
                            if last_block.get("signature").is_none() {
                                last_block["signature"] = json!(sig);
                            }
                        }
                    } else {
                        // Collect thinking content for reasoning_content field
                        match &mut reasoning_content {
                            Some(existing) => {
                                existing.push_str(text);
                            }
                            None => {
                                reasoning_content = Some(text.clone());
                            }
                        }
                        // Also add to thinking_blocks
                        thinking_blocks.push(json!({
                            "type": "thinking",
                            "thinking": text,
                        }));
                    }
                }
                _ => {}
            }
        }

        // Build provider_specific_fields with thought_signatures
        let provider_specific_fields = if !thought_signatures.is_empty() {
            Some(json!({ "thought_signatures": thought_signatures }))
        } else {
            None
        };

        // Only include thinking_blocks if non-empty
        let thinking_blocks_opt = if thinking_blocks.is_empty() {
            None
        } else {
            Some(thinking_blocks)
        };

        // Build content field
        let content = if text_parts.is_empty() {
            None
        } else if text_parts.len() == 1 {
            if let OpenAIContentPart::Text { text } = &text_parts[0] {
                Some(OpenAIContent::Text(text.clone()))
            } else {
                None
            }
        } else {
            Some(OpenAIContent::Parts(text_parts))
        };

        // Convert tool calls — encode thought_signatures into tool_call_id
        // and set provider_specific_fields.thought_signature on each tool call
        let tool_calls: Option<Vec<OpenAIToolCall>> = if unified.tool_calls.is_empty() {
            None
        } else {
            // Collect the last signature to encode into all tool call IDs (litellm compat)
            let last_sig = thought_signatures.last().cloned();
            Some(
                unified
                    .tool_calls
                    .iter()
                    .map(|tc| {
                        let (id, psf) = if let Some(ref sig) = last_sig {
                            (
                                format!(
                                    "{}{}{}",
                                    tc.id,
                                    crate::api::gemini3::THOUGHT_SIGNATURE_SEPARATOR,
                                    sig
                                ),
                                Some(json!({ "thought_signature": sig })),
                            )
                        } else {
                            (tc.id.clone(), None)
                        };
                        OpenAIToolCall {
                            id,
                            call_type: "function".to_string(),
                            function: OpenAIFunctionCall {
                                name: tc.name.clone(),
                                arguments: serde_json::to_string(&tc.arguments).unwrap_or_default(),
                            },
                            provider_specific_fields: psf,
                        }
                    })
                    .collect(),
            )
        };

        let finish_reason = unified
            .stop_reason
            .as_ref()
            .map(Self::stop_reason_to_finish_reason);

        let response = OpenAIChatResponse {
            id: unified.id.clone(),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: unified.model.clone(),
            choices: vec![OpenAIChoice {
                index: 0,
                message: OpenAIMessage {
                    role: "assistant".to_string(),
                    content,
                    reasoning_content,
                    thinking_blocks: thinking_blocks_opt,
                    provider_specific_fields,
                    name: None,
                    tool_calls,
                    tool_call_id: None,
                },
                finish_reason: finish_reason.map(|s| s.to_string()),
            }],
            usage: Some(OpenAIUsage {
                prompt_tokens: unified.usage.input_tokens,
                completion_tokens: unified.usage.output_tokens,
                total_tokens: unified.usage.total_tokens(),
            }),
        };

        serde_json::to_value(response).map_err(AppError::Serialization)
    }

    fn transform_stream_chunk_in(&self, chunk: &Bytes) -> Result<Vec<UnifiedStreamChunk>> {
        let chunk_str = std::str::from_utf8(chunk)
            .map_err(|e| AppError::BadRequest(format!("Invalid UTF-8: {}", e)))?;

        // Parse SSE format: "data: {...}\n\n"
        let mut chunks = vec![];

        for line in chunk_str.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    chunks.push(UnifiedStreamChunk::message_stop());
                    continue;
                }

                let stream_chunk: OpenAIStreamChunk = serde_json::from_str(data)
                    .map_err(|e| AppError::BadRequest(format!("Invalid JSON: {}", e)))?;

                // Track if we've emitted a message_delta for this chunk
                let mut emitted_message_delta = false;

                for choice in &stream_chunk.choices {
                    // Handle content delta (text content is always at index 0)
                    if let Some(ref content) = choice.delta.content {
                        chunks.push(UnifiedStreamChunk::content_block_delta(
                            0, // Text content is always index 0
                            UnifiedContent::text(content),
                        ));
                    }

                    // Handle reasoning_content delta (thinking content)
                    if let Some(ref reasoning) = choice.delta.reasoning_content {
                        chunks.push(UnifiedStreamChunk::content_block_delta(
                            0,
                            UnifiedContent::thinking(reasoning, None),
                        ));
                    }

                    // Handle thinking_blocks delta (structured thinking with signatures)
                    if let Some(ref blocks) = choice.delta.thinking_blocks {
                        for block in blocks {
                            if block.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                                let text =
                                    block.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                                let sig = block
                                    .get("signature")
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.to_string());
                                if !text.is_empty() {
                                    chunks.push(UnifiedStreamChunk::content_block_delta(
                                        0,
                                        UnifiedContent::thinking(text, None),
                                    ));
                                }
                                if let Some(s) = sig {
                                    chunks.push(UnifiedStreamChunk::content_block_delta(
                                        0,
                                        UnifiedContent::thinking("", Some(s)),
                                    ));
                                }
                            }
                        }
                    }

                    // Handle provider_specific_fields.thought_signatures delta
                    if let Some(ref psf) = choice.delta.provider_specific_fields {
                        if let Some(sigs) = psf.get("thought_signatures").and_then(|s| s.as_array())
                        {
                            for sig_val in sigs {
                                if let Some(sig) = sig_val.as_str() {
                                    chunks.push(UnifiedStreamChunk::content_block_delta(
                                        0,
                                        UnifiedContent::thinking("", Some(sig.to_string())),
                                    ));
                                }
                            }
                        }
                    }

                    // Handle tool_calls delta (streaming tool use)
                    // Tool calls start at index 1 (after text content at index 0)
                    if let Some(ref tool_calls) = choice.delta.tool_calls {
                        for tc in tool_calls {
                            // Calculate the actual content block index:
                            // - Index 0 is reserved for text content
                            // - Tool calls start at index 1
                            // - Handle negative indices safely (some providers may send -1 or other invalid values)
                            let content_block_index = if tc.index < 0 {
                                // Treat negative index as 0, so content block index becomes 1
                                1usize
                            } else {
                                (tc.index as usize).saturating_add(1)
                            };

                            // If this is the first chunk for this tool call (has id and name),
                            // emit a content_block_start
                            if tc.id.is_some() {
                                let id = tc.id.clone().unwrap_or_default();
                                let name = tc
                                    .function
                                    .as_ref()
                                    .and_then(|f| f.name.clone())
                                    .unwrap_or_default();
                                chunks.push(UnifiedStreamChunk::content_block_start(
                                    content_block_index,
                                    UnifiedContent::tool_use(id, name, serde_json::json!({})),
                                ));
                            }

                            // If there are arguments, emit a tool_input_delta
                            if let Some(ref func) = tc.function {
                                if let Some(ref args) = func.arguments {
                                    if !args.is_empty() {
                                        chunks.push(UnifiedStreamChunk::content_block_delta(
                                            content_block_index,
                                            UnifiedContent::tool_input_delta(
                                                content_block_index,
                                                args,
                                            ),
                                        ));
                                    }
                                }
                            }
                        }
                    }

                    // Handle finish reason
                    if let Some(ref reason) = choice.finish_reason {
                        let stop_reason = Self::finish_reason_to_stop_reason(reason);
                        // Extract usage from chunk level if available
                        let usage = stream_chunk
                            .usage
                            .as_ref()
                            .map(|u| UnifiedUsage::new(u.prompt_tokens, u.completion_tokens))
                            .unwrap_or_default();
                        chunks.push(UnifiedStreamChunk::message_delta(stop_reason, usage));
                        emitted_message_delta = true;
                    }
                }

                // Handle usage in chunks without finish_reason
                // OpenAI may send usage in a separate final chunk or in a chunk with empty choices
                if !emitted_message_delta {
                    if let Some(ref usage) = stream_chunk.usage {
                        // Emit a message_delta with usage but no stop_reason change
                        // This handles the case where usage comes in a separate chunk
                        let unified_usage =
                            UnifiedUsage::new(usage.prompt_tokens, usage.completion_tokens);
                        chunks.push(UnifiedStreamChunk::message_delta(
                            StopReason::EndTurn,
                            unified_usage,
                        ));
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
        match chunk.chunk_type {
            ChunkType::ContentBlockStart => {
                // Handle tool_use content block start
                if let Some(UnifiedContent::ToolUse { id, name, .. }) = chunk.content_block.as_ref()
                {
                    let openai_chunk = json!({
                        "id": "chatcmpl-stream",
                        "object": "chat.completion.chunk",
                        "created": chrono::Utc::now().timestamp(),
                        "model": "model",
                        "choices": [{
                            "index": 0,
                            "delta": {
                                "tool_calls": [{
                                    "index": chunk.index,
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": ""
                                    }
                                }]
                            },
                            "finish_reason": null
                        }]
                    });
                    return Ok(format!("data: {}\n\n", openai_chunk));
                }
                Ok(String::new())
            }
            ChunkType::ContentBlockDelta => {
                if let Some(ref delta) = chunk.delta {
                    match delta {
                        UnifiedContent::Text { text } => {
                            let openai_chunk = json!({
                                "id": "chatcmpl-stream",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": "model",
                                "choices": [{
                                    "index": chunk.index,
                                    "delta": { "content": text },
                                    "finish_reason": null
                                }]
                            });
                            return Ok(format!("data: {}\n\n", openai_chunk));
                        }
                        UnifiedContent::Thinking { text, signature } => {
                            if text.is_empty() && signature.is_some() {
                                // Signature-only: output as thinking_blocks with signature + provider_specific_fields
                                let sig = signature.as_ref().unwrap();
                                let openai_chunk = json!({
                                    "id": "chatcmpl-stream",
                                    "object": "chat.completion.chunk",
                                    "created": chrono::Utc::now().timestamp(),
                                    "model": "model",
                                    "choices": [{
                                        "index": chunk.index,
                                        "delta": {
                                            "provider_specific_fields": {
                                                "thought_signatures": [sig]
                                            }
                                        },
                                        "finish_reason": null
                                    }]
                                });
                                return Ok(format!("data: {}\n\n", openai_chunk));
                            }
                            // Regular thinking content as reasoning_content
                            let openai_chunk = json!({
                                "id": "chatcmpl-stream",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": "model",
                                "choices": [{
                                    "index": chunk.index,
                                    "delta": { "reasoning_content": text },
                                    "finish_reason": null
                                }]
                            });
                            return Ok(format!("data: {}\n\n", openai_chunk));
                        }
                        UnifiedContent::ToolInputDelta {
                            index,
                            partial_json,
                        } => {
                            // Convert to OpenAI tool_calls streaming format
                            let openai_chunk = json!({
                                "id": "chatcmpl-stream",
                                "object": "chat.completion.chunk",
                                "created": chrono::Utc::now().timestamp(),
                                "model": "model",
                                "choices": [{
                                    "index": 0,
                                    "delta": {
                                        "tool_calls": [{
                                            "index": index,
                                            "function": {
                                                "arguments": partial_json
                                            }
                                        }]
                                    },
                                    "finish_reason": null
                                }]
                            });
                            return Ok(format!("data: {}\n\n", openai_chunk));
                        }
                        _ => {}
                    }
                }
                Ok(String::new())
            }
            ChunkType::MessageDelta => {
                let finish_reason = chunk
                    .stop_reason
                    .as_ref()
                    .map(Self::stop_reason_to_finish_reason);

                let mut openai_chunk = json!({
                    "id": "chatcmpl-stream",
                    "object": "chat.completion.chunk",
                    "created": chrono::Utc::now().timestamp(),
                    "model": "model",
                    "choices": [{
                        "index": 0,
                        "delta": {},
                        "finish_reason": finish_reason
                    }]
                });

                if let Some(ref usage) = chunk.usage {
                    openai_chunk["usage"] = json!({
                        "prompt_tokens": usage.input_tokens,
                        "completion_tokens": usage.output_tokens,
                        "total_tokens": usage.total_tokens()
                    });
                }

                Ok(format!("data: {}\n\n", openai_chunk))
            }
            ChunkType::MessageStop => Ok("data: [DONE]\n\n".to_string()),
            _ => Ok(String::new()),
        }
    }

    fn endpoint(&self) -> &'static str {
        "/v1/chat/completions"
    }

    fn can_handle(&self, raw: &Value) -> bool {
        // OpenAI format: has "messages" array and no Anthropic-specific fields
        raw.get("messages").is_some()
            && raw.get("system").is_none()
            && !raw
                .get("messages")
                .and_then(|m| m.as_array())
                .map(|msgs| {
                    msgs.iter().any(|msg| {
                        msg.get("content")
                            .and_then(|c| c.as_array())
                            .map(|arr| {
                                arr.iter().any(|block| {
                                    let t = block.get("type").and_then(|t| t.as_str());
                                    matches!(t, Some("tool_use") | Some("tool_result"))
                                })
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_transformer_protocol() {
        let transformer = OpenAITransformer::new();
        assert_eq!(transformer.protocol(), Protocol::OpenAI);
    }

    #[test]
    fn test_transform_request_out() {
        let transformer = OpenAITransformer::new();
        let raw = json!({
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello!"}
            ],
            "temperature": 0.7,
            "max_tokens": 100
        });

        let unified = transformer.transform_request_out(raw).unwrap();
        assert_eq!(unified.model, "gpt-4");
        assert_eq!(unified.system, Some("You are helpful.".to_string()));
        assert_eq!(unified.messages.len(), 1);
        assert_eq!(unified.parameters.temperature, Some(0.7));
        assert_eq!(unified.parameters.max_tokens, Some(100));
    }

    #[test]
    fn test_transform_request_in() {
        let transformer = OpenAITransformer::new();
        let unified = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello!")])
            .with_system("Be helpful")
            .with_max_tokens(100);

        let raw = transformer.transform_request_in(&unified).unwrap();
        assert_eq!(raw["model"], "gpt-4");
        assert_eq!(raw["max_tokens"], 100);
        assert!(raw["messages"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn test_transform_response_in() {
        let transformer = OpenAITransformer::new();
        let raw = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello there!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let unified = transformer.transform_response_in(raw, "gpt-4").unwrap();
        assert_eq!(unified.id, "chatcmpl-123");
        assert_eq!(unified.text_content(), "Hello there!");
        assert_eq!(unified.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(unified.usage.input_tokens, 10);
    }

    #[test]
    fn test_transform_response_out() {
        let transformer = OpenAITransformer::new();
        let unified = UnifiedResponse::text("msg_123", "gpt-4", "Hello!", UnifiedUsage::new(10, 5));

        let raw = transformer
            .transform_response_out(&unified, Protocol::OpenAI)
            .unwrap();
        assert_eq!(raw["id"], "msg_123");
        assert_eq!(raw["object"], "chat.completion");
        assert_eq!(raw["choices"][0]["message"]["content"], "Hello!");
    }

    #[test]
    fn test_can_handle() {
        let transformer = OpenAITransformer::new();

        // OpenAI format
        let openai_request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        assert!(transformer.can_handle(&openai_request));

        // Anthropic format (has system field)
        let anthropic_request = json!({
            "model": "claude-3",
            "system": "Be helpful",
            "messages": [{"role": "user", "content": "Hello"}]
        });
        assert!(!transformer.can_handle(&anthropic_request));
    }

    #[test]
    fn test_finish_reason_conversion() {
        assert_eq!(
            OpenAITransformer::finish_reason_to_stop_reason("stop"),
            StopReason::EndTurn
        );
        assert_eq!(
            OpenAITransformer::finish_reason_to_stop_reason("length"),
            StopReason::MaxTokens
        );
        assert_eq!(
            OpenAITransformer::finish_reason_to_stop_reason("tool_calls"),
            StopReason::ToolUse
        );
    }

    #[test]
    fn test_anthropic_tools_to_openai() {
        use crate::transformer::anthropic::AnthropicTransformer;

        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        // Anthropic format request with tools
        let request = json!({
            "model": "claude-3-opus",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{
                "name": "generate_image",
                "description": "Generate an image",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "prompt": {"type": "string"}
                    },
                    "required": ["prompt"]
                }
            }],
            "tool_choice": {"type": "auto"}
        });

        // Transform: Anthropic -> Unified
        let unified = anthropic.transform_request_out(request).unwrap();
        assert_eq!(unified.tools.len(), 1);
        assert_eq!(unified.tools[0].name, "generate_image");

        // Transform: Unified -> OpenAI
        let openai_req = openai.transform_request_in(&unified).unwrap();

        // Verify OpenAI tool format
        let tools = openai_req["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "generate_image");
        assert_eq!(tools[0]["function"]["description"], "Generate an image");
        assert!(tools[0]["function"]["parameters"]["properties"]["prompt"].is_object());

        // tool_choice should be converted from Anthropic {"type": "auto"} to OpenAI "auto"
        assert_eq!(openai_req["tool_choice"], "auto");
    }

    #[test]
    fn test_tool_choice_conversion() {
        use crate::transformer::anthropic::AnthropicTransformer;

        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        // Test {"type": "auto"} -> "auto"
        let request = json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{"name": "t", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "auto"}
        });
        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();
        assert_eq!(openai_req["tool_choice"], "auto");

        // Test {"type": "any"} -> "required"
        let request = json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{"name": "t", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "any"}
        });
        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();
        assert_eq!(openai_req["tool_choice"], "required");

        // Test {"type": "tool", "name": "xxx"} -> {"type": "function", "function": {"name": "xxx"}}
        let request = json!({
            "model": "claude-3",
            "max_tokens": 100,
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{"name": "my_func", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "tool", "name": "my_func"}
        });
        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();
        assert_eq!(openai_req["tool_choice"]["type"], "function");
        assert_eq!(openai_req["tool_choice"]["function"]["name"], "my_func");
    }

    #[test]
    fn test_streaming_tool_calls_in() {
        let transformer = OpenAITransformer::new();

        // Test tool_calls streaming - first chunk with id and name
        // OpenAI tool_call index 0 should map to content block index 1 (index 0 is reserved for text)
        let chunk_start = b"data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_123\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n";
        let chunks = transformer
            .transform_stream_chunk_in(&Bytes::from_static(chunk_start))
            .unwrap();
        assert!(!chunks.is_empty());
        // Should have content_block_start for tool_use
        let start_chunk = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::ContentBlockStart);
        assert!(start_chunk.is_some());
        let start_chunk = start_chunk.unwrap();
        // Tool call index 0 should map to content block index 1
        assert_eq!(
            start_chunk.index, 1,
            "Tool call index 0 should map to content block index 1"
        );
        if let Some(UnifiedContent::ToolUse { id, name, .. }) = &start_chunk.content_block {
            assert_eq!(id, "call_123");
            assert_eq!(name, "get_weather");
        } else {
            panic!("Expected ToolUse content block");
        }

        // Test tool_calls streaming - arguments delta
        let chunk_args = b"data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]},\"finish_reason\":null}]}\n\n";
        let chunks = transformer
            .transform_stream_chunk_in(&Bytes::from_static(chunk_args))
            .unwrap();
        assert!(!chunks.is_empty());
        // Should have content_block_delta with ToolInputDelta
        let delta_chunk = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::ContentBlockDelta);
        assert!(delta_chunk.is_some());
        let delta_chunk = delta_chunk.unwrap();
        // Tool call index 0 should map to content block index 1
        assert_eq!(
            delta_chunk.index, 1,
            "Tool call delta index 0 should map to content block index 1"
        );
        if let Some(UnifiedContent::ToolInputDelta {
            partial_json,
            index,
        }) = &delta_chunk.delta
        {
            assert_eq!(partial_json, "{\"city\":");
            assert_eq!(*index, 1, "ToolInputDelta index should be 1");
        } else {
            panic!(
                "Expected ToolInputDelta content, got {:?}",
                delta_chunk.delta
            );
        }
    }

    #[test]
    fn test_streaming_tool_calls_out() {
        let transformer = OpenAITransformer::new();

        // Test outputting content_block_start for tool_use
        let chunk = UnifiedStreamChunk::content_block_start(
            0,
            UnifiedContent::tool_use("call_123", "get_weather", serde_json::json!({})),
        );
        let output = transformer
            .transform_stream_chunk_out(&chunk, Protocol::OpenAI)
            .unwrap();
        assert!(output.contains("tool_calls"));
        assert!(output.contains("call_123"));
        assert!(output.contains("get_weather"));

        // Test outputting tool_input_delta
        let chunk = UnifiedStreamChunk::content_block_delta(
            0,
            UnifiedContent::tool_input_delta(0, "{\"city\":"),
        );
        let output = transformer
            .transform_stream_chunk_out(&chunk, Protocol::OpenAI)
            .unwrap();
        assert!(output.contains("tool_calls"));
        assert!(output.contains("arguments"));
        assert!(output.contains("{\\\"city\\\":"));
    }

    #[test]
    fn test_streaming_usage_with_finish_reason() {
        let transformer = OpenAITransformer::new();

        // Test chunk with finish_reason AND usage in the same chunk
        let chunk_with_usage = b"data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":173,\"completion_tokens\":23,\"total_tokens\":196}}\n\n";
        let chunks = transformer
            .transform_stream_chunk_in(&Bytes::from_static(chunk_with_usage))
            .unwrap();

        // Should have a message_delta with usage
        let message_delta = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::MessageDelta);
        assert!(message_delta.is_some(), "Should have message_delta chunk");

        let delta = message_delta.unwrap();
        assert_eq!(delta.stop_reason, Some(StopReason::EndTurn));
        assert!(delta.usage.is_some(), "Should have usage in message_delta");

        let usage = delta.usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, 173, "input_tokens should be 173");
        assert_eq!(usage.output_tokens, 23, "output_tokens should be 23");
    }

    #[test]
    fn test_streaming_usage_in_separate_chunk() {
        let transformer = OpenAITransformer::new();

        // Test chunk with usage but no finish_reason (separate usage chunk)
        // This simulates OpenAI sending usage in a separate final chunk
        let usage_only_chunk = b"data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"completion_tokens\":50,\"total_tokens\":150}}\n\n";
        let chunks = transformer
            .transform_stream_chunk_in(&Bytes::from_static(usage_only_chunk))
            .unwrap();

        // Should have a message_delta with usage
        let message_delta = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::MessageDelta);
        assert!(
            message_delta.is_some(),
            "Should have message_delta chunk for usage-only chunk"
        );

        let delta = message_delta.unwrap();
        assert!(delta.usage.is_some(), "Should have usage in message_delta");

        let usage = delta.usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, 100, "input_tokens should be 100");
        assert_eq!(usage.output_tokens, 50, "output_tokens should be 50");
    }

    #[test]
    fn test_streaming_usage_without_usage_field() {
        let transformer = OpenAITransformer::new();

        // Test chunk with finish_reason but NO usage (older OpenAI behavior)
        let chunk_no_usage = b"data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n";
        let chunks = transformer
            .transform_stream_chunk_in(&Bytes::from_static(chunk_no_usage))
            .unwrap();

        // Should have a message_delta with default (zero) usage
        let message_delta = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::MessageDelta);
        assert!(message_delta.is_some(), "Should have message_delta chunk");

        let delta = message_delta.unwrap();
        assert_eq!(delta.stop_reason, Some(StopReason::EndTurn));
        assert!(delta.usage.is_some(), "Should have usage (even if default)");

        let usage = delta.usage.as_ref().unwrap();
        // Default usage when not provided
        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 0);
    }

    #[test]
    fn test_streaming_tool_calls_negative_index() {
        let transformer = OpenAITransformer::new();

        // Test tool_calls streaming with negative index (some providers may send -1)
        // This should NOT panic and should handle gracefully
        let chunk_negative_index = b"data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":-1,\"id\":\"call_123\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n";
        let result =
            transformer.transform_stream_chunk_in(&Bytes::from_static(chunk_negative_index));
        assert!(result.is_ok(), "Should handle negative index without panic");

        let chunks = result.unwrap();
        // Should have content_block_start for tool_use
        let start_chunk = chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::ContentBlockStart);
        assert!(start_chunk.is_some());
        let start_chunk = start_chunk.unwrap();
        // Negative index should be treated as 0, so content block index becomes 1
        assert_eq!(
            start_chunk.index, 1,
            "Negative tool call index should map to content block index 1"
        );
    }

    #[test]
    fn test_cross_protocol_streaming_usage_openai_to_anthropic() {
        use crate::transformer::anthropic::AnthropicTransformer;

        let openai = OpenAITransformer::new();
        let anthropic = AnthropicTransformer::new();

        // Simulate OpenAI streaming chunk with usage
        let openai_chunk = b"data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1234567890,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":173,\"completion_tokens\":23,\"total_tokens\":196}}\n\n";

        // Transform OpenAI chunk to unified format
        let unified_chunks = openai
            .transform_stream_chunk_in(&Bytes::from_static(openai_chunk))
            .unwrap();

        // Find the message_delta chunk
        let message_delta = unified_chunks
            .iter()
            .find(|c| c.chunk_type == ChunkType::MessageDelta)
            .unwrap();

        // Transform unified chunk to Anthropic format
        let anthropic_output = anthropic
            .transform_stream_chunk_out(message_delta, Protocol::Anthropic)
            .unwrap();

        // Verify Anthropic output contains correct usage
        assert!(
            anthropic_output.contains("message_delta"),
            "Should contain message_delta event"
        );
        assert!(
            anthropic_output.contains("\"input_tokens\":173"),
            "Should contain input_tokens: 173"
        );
        assert!(
            anthropic_output.contains("\"output_tokens\":23"),
            "Should contain output_tokens: 23"
        );
    }

    #[test]
    fn test_openai_tool_choice_string_normalization() {
        let transformer = OpenAITransformer::new();

        // Test "auto" string -> {"type": "auto"}
        let request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{"type": "function", "function": {"name": "t", "parameters": {"type": "object"}}}],
            "tool_choice": "auto"
        });
        let unified = transformer.transform_request_out(request).unwrap();
        assert_eq!(unified.tool_choice, Some(json!({"type": "auto"})));

        // Test "none" string -> {"type": "none"}
        let request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{"type": "function", "function": {"name": "t", "parameters": {"type": "object"}}}],
            "tool_choice": "none"
        });
        let unified = transformer.transform_request_out(request).unwrap();
        assert_eq!(unified.tool_choice, Some(json!({"type": "none"})));

        // Test "required" string -> {"type": "any"}
        let request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{"type": "function", "function": {"name": "t", "parameters": {"type": "object"}}}],
            "tool_choice": "required"
        });
        let unified = transformer.transform_request_out(request).unwrap();
        assert_eq!(unified.tool_choice, Some(json!({"type": "any"})));

        // Test {"type": "function", "function": {"name": "xxx"}} -> {"type": "tool", "name": "xxx"}
        let request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{"type": "function", "function": {"name": "my_func", "parameters": {"type": "object"}}}],
            "tool_choice": {"type": "function", "function": {"name": "my_func"}}
        });
        let unified = transformer.transform_request_out(request).unwrap();
        assert_eq!(
            unified.tool_choice,
            Some(json!({"type": "tool", "name": "my_func"}))
        );
    }

    #[test]
    fn test_openai_to_anthropic_tool_choice() {
        use crate::transformer::anthropic::AnthropicTransformer;

        let openai = OpenAITransformer::new();
        let anthropic = AnthropicTransformer::new();

        // OpenAI request with tool_choice: "auto" (string)
        let request = json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "test"}],
            "tools": [{"type": "function", "function": {"name": "generate_image", "description": "Generate an image", "parameters": {"type": "object", "properties": {"prompt": {"type": "string"}}, "required": ["prompt"]}}}],
            "tool_choice": "auto"
        });

        // Transform: OpenAI -> Unified
        let unified = openai.transform_request_out(request).unwrap();
        // Verify tool_choice is normalized to dict format
        assert_eq!(unified.tool_choice, Some(json!({"type": "auto"})));

        // Transform: Unified -> Anthropic
        let anthropic_req = anthropic.transform_request_in(&unified).unwrap();

        // Verify Anthropic tool_choice format is correct
        assert_eq!(anthropic_req["tool_choice"]["type"], "auto");
    }

    #[test]
    fn test_anthropic_tool_results_to_openai_single() {
        use crate::transformer::anthropic::AnthropicTransformer;

        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        // Simulate Anthropic request with tool_use in assistant + tool_result in user
        let request = json!({
            "model": "claude-4-sonnet",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Read the file"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Let me read that."},
                    {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {"file": "a.rs"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_1", "content": "file content here"}
                ]}
            ]
        });

        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();

        let messages = openai_req["messages"].as_array().unwrap();
        // user, assistant, tool
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "tu_1");
        assert_eq!(messages[2]["content"], "file content here");
    }

    #[test]
    fn test_anthropic_tool_results_to_openai_multiple() {
        use crate::transformer::anthropic::AnthropicTransformer;

        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        // Simulate Anthropic request with multiple parallel tool_use + tool_results
        let request = json!({
            "model": "claude-4-sonnet",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Help me"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {"file": "a.rs"}},
                    {"type": "tool_use", "id": "tu_2", "name": "Read", "input": {"file": "b.rs"}},
                    {"type": "tool_use", "id": "tu_3", "name": "Grep", "input": {"pattern": "foo"}},
                    {"type": "tool_use", "id": "tu_4", "name": "Edit", "input": {"file": "a.rs"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_1", "content": "content a"},
                    {"type": "tool_result", "tool_use_id": "tu_2", "content": "content b"},
                    {"type": "tool_result", "tool_use_id": "tu_3", "content": "grep result"},
                    {"type": "tool_result", "tool_use_id": "tu_4", "content": "edit done"}
                ]}
            ]
        });

        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();

        let messages = openai_req["messages"].as_array().unwrap();
        // user, assistant (with tool_calls), tool*4
        assert_eq!(messages.len(), 6);

        // Verify assistant has 4 tool_calls
        let assistant_tool_calls = messages[1]["tool_calls"].as_array().unwrap();
        assert_eq!(assistant_tool_calls.len(), 4);

        // Verify each tool result is a separate tool message with correct mapping
        for (i, (expected_id, expected_content)) in [
            ("tu_1", "content a"),
            ("tu_2", "content b"),
            ("tu_3", "grep result"),
            ("tu_4", "edit done"),
        ]
        .iter()
        .enumerate()
        {
            let tool_msg = &messages[2 + i];
            assert_eq!(tool_msg["role"], "tool");
            assert_eq!(tool_msg["tool_call_id"], *expected_id);
            assert_eq!(tool_msg["content"], *expected_content);
        }
    }

    #[test]
    fn test_anthropic_tool_results_with_mixed_content() {
        use crate::transformer::anthropic::AnthropicTransformer;

        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        // User message with both text and tool_result blocks
        let request = json!({
            "model": "claude-4-sonnet",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Read it"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_1", "content": "result"},
                    {"type": "text", "text": "Now edit it please"}
                ]}
            ]
        });

        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();

        let messages = openai_req["messages"].as_array().unwrap();
        // user, assistant, tool, user(text) — tool MUST come before user to
        // preserve assistant(tool_calls) → tool adjacency
        assert_eq!(messages.len(), 4);

        // Tool result emitted first (adjacent to assistant)
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "tu_1");
        assert_eq!(messages[2]["content"], "result");

        // Non-tool content emitted as user message after
        assert_eq!(messages[3]["role"], "user");
    }

    #[test]
    fn test_tool_result_adjacency_with_system_reminder() {
        // Reproduces the real-world bug: Claude Code injects a <system-reminder>
        // text block into the same user message that carries tool_result.
        // The tool message MUST stay adjacent to the preceding assistant(tool_calls).
        use crate::transformer::anthropic::AnthropicTransformer;

        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        let request = json!({
            "model": "claude-4-sonnet",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "search for chromium size"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Let me search."},
                    {"type": "tool_use", "id": "tu_search", "name": "web_search", "input": {"q": "chromium"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_search", "content": "Found 5 results"},
                    {"type": "text", "text": "<system-reminder>Use TodoWrite</system-reminder>"}
                ]}
            ]
        });

        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();
        let messages = openai_req["messages"].as_array().unwrap();

        // user, assistant(tool_calls), tool, user(system-reminder)
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[1]["role"], "assistant");
        assert!(messages[1]["tool_calls"].is_array());
        // tool MUST be right after assistant
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "tu_search");
        // user text comes last
        assert_eq!(messages[3]["role"], "user");
        assert!(
            messages[3]["content"].as_str().is_none()
                || messages[3]["content"]
                    .to_string()
                    .contains("system-reminder")
        );
    }

    #[test]
    fn test_tool_result_adjacency_multiple_tools_with_interrupt() {
        // assistant calls 2 tools, user interrupts with text + provides both results
        use crate::transformer::anthropic::AnthropicTransformer;

        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        let request = json!({
            "model": "claude-4-sonnet",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "do two things"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tu_a", "name": "tool_a", "input": {}},
                    {"type": "tool_use", "id": "tu_b", "name": "tool_b", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_a", "content": "result_a"},
                    {"type": "tool_result", "tool_use_id": "tu_b", "content": "result_b"},
                    {"type": "text", "text": "[Request interrupted by user for tool use]"},
                    {"type": "text", "text": "new user instruction"}
                ]}
            ]
        });

        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();
        let messages = openai_req["messages"].as_array().unwrap();

        // user, assistant(tool_calls), tool(a), tool(b), user(text+text)
        assert_eq!(messages.len(), 5);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "tu_a");
        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "tu_b");
        assert_eq!(messages[4]["role"], "user");
    }

    #[test]
    fn test_tool_result_only_no_text_unchanged() {
        // Pure tool_result without text should work as before (no user message emitted)
        use crate::transformer::anthropic::AnthropicTransformer;

        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        let request = json!({
            "model": "claude-4-sonnet",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "read file"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_1", "content": "file contents"}
                ]}
            ]
        });

        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();
        let messages = openai_req["messages"].as_array().unwrap();

        // user, assistant(tool_calls), tool — no extra user message
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "tu_1");
    }

    #[test]
    fn test_anthropic_tool_result_with_error() {
        use crate::transformer::anthropic::AnthropicTransformer;

        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        let request = json!({
            "model": "claude-4-sonnet",
            "max_tokens": 1024,
            "messages": [
                {"role": "user", "content": "Read it"},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "tu_1", "name": "Read", "input": {}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tu_1", "content": "file not found", "is_error": true}
                ]}
            ]
        });

        let unified = anthropic.transform_request_out(request).unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();

        let messages = openai_req["messages"].as_array().unwrap();
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["content"], "[Error] file not found");
    }

    #[test]
    fn test_tool_result_content_to_string_variants() {
        // String content
        assert_eq!(
            OpenAITransformer::tool_result_content_to_string(&json!("hello"), false),
            "hello"
        );

        // Null content
        assert_eq!(
            OpenAITransformer::tool_result_content_to_string(&Value::Null, false),
            ""
        );

        // Array content (Anthropic structured blocks)
        assert_eq!(
            OpenAITransformer::tool_result_content_to_string(
                &json!([{"type": "text", "text": "line1"}, {"type": "text", "text": "line2"}]),
                false
            ),
            "line1\nline2"
        );

        // Error flag
        assert_eq!(
            OpenAITransformer::tool_result_content_to_string(&json!("oops"), true),
            "[Error] oops"
        );

        // Error with empty content
        assert_eq!(
            OpenAITransformer::tool_result_content_to_string(&Value::Null, true),
            ""
        );
    }

    // ========================================================================
    // Shared fixture tests — cross-language consistency verification
    // ========================================================================

    fn shared_fixture_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("shared-fixtures")
            .join("anthropic-to-openai")
    }

    fn load_shared_fixture(name: &str) -> Value {
        let path = shared_fixture_dir().join(name);
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to load fixture {}: {}", path.display(), e));
        serde_json::from_str(&content).unwrap()
    }

    fn run_pipeline_fixture(fixture_name: &str) {
        use crate::transformer::anthropic::AnthropicTransformer;

        let fixture = load_shared_fixture(fixture_name);
        let anthropic = AnthropicTransformer::new();
        let openai = OpenAITransformer::new();

        let unified = anthropic
            .transform_request_out(fixture["input"].clone())
            .unwrap();
        let openai_req = openai.transform_request_in(&unified).unwrap();
        let messages = openai_req["messages"].as_array().unwrap();

        let expected = &fixture["expected"];
        if let Some(count) = expected.get("message_count") {
            assert_eq!(
                messages.len(),
                count.as_u64().unwrap() as usize,
                "fixture {}: message_count mismatch",
                fixture_name
            );
        }

        if let Some(expected_msgs) = expected.get("messages").and_then(|m| m.as_array()) {
            for exp in expected_msgs {
                let idx = exp["index"].as_u64().unwrap() as usize;
                let msg = &messages[idx];
                let ctx = format!("fixture {}, index {}", fixture_name, idx);

                if let Some(role) = exp.get("role").and_then(|v| v.as_str()) {
                    assert_eq!(msg["role"].as_str().unwrap(), role, "{}: role", ctx);
                }
                if let Some(content) = exp.get("content").and_then(|v| v.as_str()) {
                    assert_eq!(
                        msg["content"].as_str().unwrap(),
                        content,
                        "{}: content",
                        ctx
                    );
                }
                if let Some(content_contains) = exp.get("content_contains").and_then(|v| v.as_str())
                {
                    let actual = msg["content"].to_string();
                    assert!(
                        actual.contains(content_contains),
                        "{}: content should contain '{}', got '{}'",
                        ctx,
                        content_contains,
                        actual
                    );
                }
                if let Some(tcid) = exp.get("tool_call_id").and_then(|v| v.as_str()) {
                    assert_eq!(
                        msg["tool_call_id"].as_str().unwrap(),
                        tcid,
                        "{}: tool_call_id",
                        ctx
                    );
                }
                if let Some(has_tc) = exp.get("has_tool_calls").and_then(|v| v.as_bool()) {
                    assert_eq!(
                        msg["tool_calls"].is_array(),
                        has_tc,
                        "{}: has_tool_calls",
                        ctx
                    );
                }
            }
        }
    }

    #[test]
    fn test_shared_fixture_01_single_tool_result() {
        run_pipeline_fixture("01_single_tool_result.json");
    }

    #[test]
    fn test_shared_fixture_02_multiple_tool_results() {
        run_pipeline_fixture("02_multiple_tool_results.json");
    }

    #[test]
    fn test_shared_fixture_03_mixed_content() {
        run_pipeline_fixture("03_mixed_content_tool_result_and_text.json");
    }

    #[test]
    fn test_shared_fixture_04_adjacency_system_reminder() {
        run_pipeline_fixture("04_adjacency_with_system_reminder.json");
    }

    #[test]
    fn test_shared_fixture_05_adjacency_multiple_tools_interrupt() {
        run_pipeline_fixture("05_adjacency_multiple_tools_with_interrupt.json");
    }

    #[test]
    fn test_shared_fixture_06_tool_result_only() {
        run_pipeline_fixture("06_tool_result_only_no_text.json");
    }

    #[test]
    fn test_shared_fixture_07_tool_result_with_error() {
        run_pipeline_fixture("07_tool_result_with_error.json");
    }

    #[test]
    fn test_shared_fixture_08_content_string_variants() {
        let fixture = load_shared_fixture("08_content_string_variants.json");
        let variants = fixture["variants"].as_array().unwrap();
        for (i, v) in variants.iter().enumerate() {
            let content = &v["content"];
            let is_error = v["is_error"].as_bool().unwrap();
            let expected = v["expected"].as_str().unwrap();
            let actual = OpenAITransformer::tool_result_content_to_string(content, is_error);
            assert_eq!(actual, expected, "variant {}", i);
        }
    }
}
