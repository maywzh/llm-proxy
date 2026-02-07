//! OpenAI Response API transformer.
//!
//! Handles conversion between OpenAI's new Response API format (/v1/responses)
//! and the Unified Internal Format.
//!
//! Response API is OpenAI's newer API format that supports:
//! - Stateful conversations with session management
//! - Computer use and other advanced tools
//! - Structured output with JSON schemas
//! - Multiple modalities

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
// Response API Request/Response Types
// ============================================================================

/// Response API input types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseInput {
    /// Simple text input
    Text(String),
    /// Array of input items (messages)
    Items(Vec<ResponseInputItem>),
}

/// Response API input item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseInputItem {
    #[serde(rename = "message")]
    Message {
        role: String,
        content: ResponseContent,
    },
    #[serde(rename = "item_reference")]
    ItemReference { id: String },
}

/// Response API content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseContent {
    Text(String),
    Parts(Vec<ResponseContentPart>),
}

/// Response API content part.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseContentPart {
    #[serde(rename = "input_text")]
    InputText { text: String },
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "input_image")]
    InputImage { image_url: String },
    #[serde(rename = "input_file")]
    InputFile { file_id: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        arguments: String,
    },
    #[serde(rename = "tool_result")]
    ToolResult { tool_use_id: String, output: String },
}

/// Response API tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseTool {
    #[serde(rename = "function")]
    Function {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        parameters: Value,
    },
    #[serde(rename = "computer_use_preview")]
    ComputerUse {
        #[serde(skip_serializing_if = "Option::is_none")]
        display_width: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        display_height: Option<i32>,
    },
    #[serde(rename = "web_search_preview")]
    WebSearch {},
    #[serde(rename = "file_search")]
    FileSearch {
        #[serde(skip_serializing_if = "Option::is_none")]
        vector_store_ids: Option<Vec<String>>,
    },
}

/// Response API request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseApiRequest {
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<ResponseInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ResponseTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modalities: Option<Vec<String>>,
    #[serde(default)]
    pub stream: bool,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

/// Response API output item.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseOutputItem {
    #[serde(rename = "message")]
    Message {
        id: String,
        role: String,
        content: Vec<ResponseOutputContent>,
        status: String,
    },
    #[serde(rename = "function_call")]
    FunctionCall {
        id: String,
        call_id: String,
        name: String,
        arguments: String,
        status: String,
    },
}

/// Response API output content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResponseOutputContent {
    #[serde(rename = "output_text")]
    OutputText { text: String },
    #[serde(rename = "refusal")]
    Refusal { refusal: String },
}

/// Response API usage.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResponseUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub total_tokens: i32,
}

/// Response API response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseApiResponse {
    pub id: String,
    pub object: String,
    pub created_at: i64,
    pub model: String,
    pub output: Vec<ResponseOutputItem>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_details: Option<Value>,
    pub usage: ResponseUsage,
}

// ============================================================================
// Response API Transformer Implementation
// ============================================================================

/// Response API protocol transformer.
pub struct ResponseApiTransformer;

impl ResponseApiTransformer {
    /// Create a new Response API transformer.
    pub fn new() -> Self {
        ResponseApiTransformer
    }

    /// Convert Response API input to unified messages.
    fn input_to_messages(input: &Option<ResponseInput>) -> Vec<UnifiedMessage> {
        match input {
            None => vec![],
            Some(ResponseInput::Text(text)) => {
                vec![UnifiedMessage::user(text)]
            }
            Some(ResponseInput::Items(items)) => {
                items
                    .iter()
                    .filter_map(|item| match item {
                        ResponseInputItem::Message { role, content } => {
                            let role = role.parse().unwrap_or(Role::User);
                            let unified_content = Self::content_to_unified(content);
                            Some(UnifiedMessage::with_content(role, unified_content))
                        }
                        ResponseInputItem::ItemReference { .. } => None, // Skip references
                    })
                    .collect()
            }
        }
    }

    /// Convert Response API content to unified content.
    fn content_to_unified(content: &ResponseContent) -> Vec<UnifiedContent> {
        match content {
            ResponseContent::Text(text) => vec![UnifiedContent::text(text)],
            ResponseContent::Parts(parts) => parts
                .iter()
                .map(|part| match part {
                    ResponseContentPart::InputText { text }
                    | ResponseContentPart::OutputText { text } => UnifiedContent::text(text),
                    ResponseContentPart::InputImage { image_url } => {
                        UnifiedContent::image_url(image_url)
                    }
                    ResponseContentPart::InputFile { file_id } => UnifiedContent::File {
                        file_id: file_id.clone(),
                        filename: None,
                    },
                    ResponseContentPart::ToolUse {
                        id,
                        name,
                        arguments,
                    } => {
                        let args: Value = serde_json::from_str(arguments).unwrap_or(json!({}));
                        UnifiedContent::tool_use(id, name, args)
                    }
                    ResponseContentPart::ToolResult {
                        tool_use_id,
                        output,
                    } => UnifiedContent::tool_result(tool_use_id, json!(output), false),
                })
                .collect(),
        }
    }

    /// Convert unified messages to Response API input.
    fn messages_to_input(messages: &[UnifiedMessage]) -> Option<ResponseInput> {
        if messages.is_empty() {
            return None;
        }

        let items: Vec<ResponseInputItem> = messages
            .iter()
            .map(|msg| {
                let content = Self::unified_to_content(&msg.content);
                ResponseInputItem::Message {
                    role: msg.role.to_string(),
                    content,
                }
            })
            .collect();

        Some(ResponseInput::Items(items))
    }

    /// Convert unified content to Response API content.
    fn unified_to_content(content: &[UnifiedContent]) -> ResponseContent {
        if content.len() == 1 {
            if let Some(text) = content[0].as_text() {
                return ResponseContent::Text(text.to_string());
            }
        }

        let parts: Vec<ResponseContentPart> = content
            .iter()
            .filter_map(|c| match c {
                UnifiedContent::Text { text } => {
                    Some(ResponseContentPart::InputText { text: text.clone() })
                }
                UnifiedContent::Image { data, .. } => Some(ResponseContentPart::InputImage {
                    image_url: data.clone(),
                }),
                UnifiedContent::File { file_id, .. } => Some(ResponseContentPart::InputFile {
                    file_id: file_id.clone(),
                }),
                UnifiedContent::ToolUse { id, name, input } => Some(ResponseContentPart::ToolUse {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: serde_json::to_string(input).unwrap_or_default(),
                }),
                UnifiedContent::ToolResult {
                    tool_use_id,
                    content,
                    ..
                } => Some(ResponseContentPart::ToolResult {
                    tool_use_id: tool_use_id.clone(),
                    output: serde_json::to_string(content).unwrap_or_default(),
                }),
                _ => None,
            })
            .collect();

        ResponseContent::Parts(parts)
    }

    /// Convert Response API tools to unified tools.
    fn tools_to_unified(tools: &Option<Vec<ResponseTool>>) -> Vec<UnifiedTool> {
        tools
            .as_ref()
            .map(|tools| {
                tools
                    .iter()
                    .map(|tool| match tool {
                        ResponseTool::Function {
                            name,
                            description,
                            parameters,
                        } => UnifiedTool {
                            name: name.clone(),
                            description: description.clone(),
                            input_schema: parameters.clone(),
                            tool_type: Some("function".to_string()),
                        },
                        ResponseTool::ComputerUse { .. } => UnifiedTool {
                            name: "computer_use".to_string(),
                            description: Some("Computer use capability".to_string()),
                            input_schema: json!({}),
                            tool_type: Some("computer_use_preview".to_string()),
                        },
                        ResponseTool::WebSearch {} => UnifiedTool {
                            name: "web_search".to_string(),
                            description: Some("Web search capability".to_string()),
                            input_schema: json!({}),
                            tool_type: Some("web_search_preview".to_string()),
                        },
                        ResponseTool::FileSearch { .. } => UnifiedTool {
                            name: "file_search".to_string(),
                            description: Some("File search capability".to_string()),
                            input_schema: json!({}),
                            tool_type: Some("file_search".to_string()),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Convert unified tools to Response API tools.
    fn unified_to_tools(tools: &[UnifiedTool]) -> Option<Vec<ResponseTool>> {
        if tools.is_empty() {
            return None;
        }

        let response_tools: Vec<ResponseTool> = tools
            .iter()
            .map(|tool| {
                let tool_type = tool.tool_type.as_deref().unwrap_or("function");
                match tool_type {
                    "computer_use_preview" => ResponseTool::ComputerUse {
                        display_width: None,
                        display_height: None,
                    },
                    "web_search_preview" => ResponseTool::WebSearch {},
                    "file_search" => ResponseTool::FileSearch {
                        vector_store_ids: None,
                    },
                    _ => ResponseTool::Function {
                        name: tool.name.clone(),
                        description: tool.description.clone(),
                        parameters: tool.input_schema.clone(),
                    },
                }
            })
            .collect();

        Some(response_tools)
    }

    /// Convert Response API output to unified content.
    fn output_to_unified(
        output: &[ResponseOutputItem],
    ) -> (Vec<UnifiedContent>, Vec<UnifiedToolCall>) {
        let mut content = vec![];
        let mut tool_calls = vec![];

        for item in output {
            match item {
                ResponseOutputItem::Message {
                    content: msg_content,
                    ..
                } => {
                    for c in msg_content {
                        match c {
                            ResponseOutputContent::OutputText { text } => {
                                content.push(UnifiedContent::text(text));
                            }
                            ResponseOutputContent::Refusal { refusal } => {
                                content.push(UnifiedContent::Refusal {
                                    reason: refusal.clone(),
                                });
                            }
                        }
                    }
                }
                ResponseOutputItem::FunctionCall {
                    id,
                    call_id,
                    name,
                    arguments,
                    ..
                } => {
                    let args: Value = serde_json::from_str(arguments).unwrap_or(json!({}));
                    content.push(UnifiedContent::tool_use(call_id, name, args.clone()));
                    tool_calls.push(UnifiedToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: args,
                    });
                }
            }
        }

        (content, tool_calls)
    }

    /// Convert status to stop reason.
    fn status_to_stop_reason(status: &str) -> StopReason {
        match status {
            "completed" => StopReason::EndTurn,
            "incomplete" => StopReason::MaxTokens,
            "cancelled" => StopReason::EndTurn,
            "failed" => StopReason::ContentFilter,
            _ => StopReason::EndTurn,
        }
    }
}

impl Default for ResponseApiTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl Transformer for ResponseApiTransformer {
    fn protocol(&self) -> Protocol {
        Protocol::ResponseApi
    }

    fn transform_request_out(&self, raw: Value) -> Result<UnifiedRequest> {
        let request: ResponseApiRequest =
            serde_json::from_value(raw).map_err(|e| AppError::BadRequest(e.to_string()))?;

        // Convert input to messages
        let messages = Self::input_to_messages(&request.input);

        // Convert tools
        let tools = Self::tools_to_unified(&request.tools);

        // Build parameters
        let parameters = UnifiedParameters {
            temperature: request.temperature,
            max_tokens: request.max_output_tokens,
            top_p: request.top_p,
            top_k: None,
            stop_sequences: None,
            stream: request.stream,
            extra: request.extra,
        };

        Ok(UnifiedRequest {
            model: request.model,
            messages,
            system: request.instructions,
            parameters,
            tools,
            tool_choice: request.tool_choice,
            request_id: uuid::Uuid::new_v4().to_string(),
            client_protocol: Protocol::ResponseApi,
            metadata: HashMap::new(),
        })
    }

    fn transform_request_in(&self, unified: &UnifiedRequest) -> Result<Value> {
        let input = Self::messages_to_input(&unified.messages);
        let tools = Self::unified_to_tools(&unified.tools);

        let mut request = json!({
            "model": unified.model,
        });

        if let Some(input) = input {
            request["input"] = json!(input);
        }
        if let Some(ref system) = unified.system {
            request["instructions"] = json!(system);
        }
        if let Some(max_tokens) = unified.parameters.max_tokens {
            request["max_output_tokens"] = json!(max_tokens);
        }
        if let Some(temp) = unified.parameters.temperature {
            request["temperature"] = json!(temp);
        }
        if let Some(top_p) = unified.parameters.top_p {
            request["top_p"] = json!(top_p);
        }
        if let Some(tools) = tools {
            request["tools"] = json!(tools);
        }
        if let Some(ref tool_choice) = unified.tool_choice {
            request["tool_choice"] = tool_choice.clone();
        }
        if unified.parameters.stream {
            request["stream"] = json!(true);
        }

        // Add extra parameters
        for (key, value) in &unified.parameters.extra {
            request[key] = value.clone();
        }

        Ok(request)
    }

    fn transform_response_in(&self, raw: Value, original_model: &str) -> Result<UnifiedResponse> {
        let response: ResponseApiResponse =
            serde_json::from_value(raw).map_err(|e| AppError::BadRequest(e.to_string()))?;

        let (content, tool_calls) = Self::output_to_unified(&response.output);
        let stop_reason = Some(Self::status_to_stop_reason(&response.status));

        let usage = UnifiedUsage::new(response.usage.input_tokens, response.usage.output_tokens);

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
        // Convert content to output items
        let mut output = vec![];

        // Collect text content into a message
        let text_content: Vec<ResponseOutputContent> = unified
            .content
            .iter()
            .filter_map(|c| match c {
                UnifiedContent::Text { text } => {
                    Some(ResponseOutputContent::OutputText { text: text.clone() })
                }
                UnifiedContent::Refusal { reason } => Some(ResponseOutputContent::Refusal {
                    refusal: reason.clone(),
                }),
                _ => None,
            })
            .collect();

        if !text_content.is_empty() {
            output.push(ResponseOutputItem::Message {
                id: format!("msg_{}", uuid::Uuid::new_v4()),
                role: "assistant".to_string(),
                content: text_content,
                status: "completed".to_string(),
            });
        }

        // Add function calls
        for tool_call in &unified.tool_calls {
            output.push(ResponseOutputItem::FunctionCall {
                id: format!("fc_{}", uuid::Uuid::new_v4()),
                call_id: tool_call.id.clone(),
                name: tool_call.name.clone(),
                arguments: serde_json::to_string(&tool_call.arguments).unwrap_or_default(),
                status: "completed".to_string(),
            });
        }

        let status = match &unified.stop_reason {
            Some(StopReason::EndTurn) => "completed",
            Some(StopReason::MaxTokens) | Some(StopReason::Length) => "incomplete",
            Some(StopReason::ContentFilter) => "failed",
            _ => "completed",
        };

        let response = ResponseApiResponse {
            id: unified.id.clone(),
            object: "response".to_string(),
            created_at: chrono::Utc::now().timestamp(),
            model: unified.model.clone(),
            output,
            status: status.to_string(),
            status_details: None,
            usage: ResponseUsage {
                input_tokens: unified.usage.input_tokens,
                output_tokens: unified.usage.output_tokens,
                total_tokens: unified.usage.total_tokens(),
            },
        };

        serde_json::to_value(response).map_err(AppError::Serialization)
    }

    fn transform_stream_chunk_in(&self, chunk: &Bytes) -> Result<Vec<UnifiedStreamChunk>> {
        // Response API streaming format is similar to OpenAI
        // This is a simplified implementation
        let chunk_str = std::str::from_utf8(chunk)
            .map_err(|e| AppError::BadRequest(format!("Invalid UTF-8: {}", e)))?;

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

                // Parse as generic JSON and extract relevant fields
                if let Ok(json) = serde_json::from_str::<Value>(data) {
                    // Handle different event types
                    if let Some(event_type) = json.get("type").and_then(|t| t.as_str()) {
                        match event_type {
                            "response.created" | "response.in_progress" => {
                                // Message start event
                                if let Some(response) = json.get("response") {
                                    let id = response
                                        .get("id")
                                        .and_then(|i| i.as_str())
                                        .unwrap_or("resp_stream")
                                        .to_string();
                                    let model = response
                                        .get("model")
                                        .and_then(|m| m.as_str())
                                        .unwrap_or("model")
                                        .to_string();

                                    let unified_response = UnifiedResponse {
                                        id,
                                        model,
                                        content: vec![],
                                        stop_reason: None,
                                        usage: UnifiedUsage::default(),
                                        tool_calls: vec![],
                                    };
                                    chunks
                                        .push(UnifiedStreamChunk::message_start(unified_response));
                                }
                            }
                            "response.output_text.delta" => {
                                if let Some(delta) = json.get("delta").and_then(|d| d.as_str()) {
                                    chunks.push(UnifiedStreamChunk::content_block_delta(
                                        0,
                                        UnifiedContent::text(delta),
                                    ));
                                }
                            }
                            "response.completed" => {
                                let usage = json
                                    .get("response")
                                    .and_then(|r| r.get("usage"))
                                    .map(|u| {
                                        UnifiedUsage::new(
                                            u.get("input_tokens")
                                                .and_then(|t| t.as_i64())
                                                .unwrap_or(0)
                                                as i32,
                                            u.get("output_tokens")
                                                .and_then(|t| t.as_i64())
                                                .unwrap_or(0)
                                                as i32,
                                        )
                                    })
                                    .unwrap_or_default();
                                chunks.push(UnifiedStreamChunk::message_delta(
                                    StopReason::EndTurn,
                                    usage,
                                ));
                            }
                            _ => {}
                        }
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
                    json!({
                        "type": "response.created",
                        "response": {
                            "id": message.id,
                            "object": "response",
                            "model": message.model,
                            "status": "in_progress"
                        }
                    })
                } else {
                    return Ok(String::new());
                }
            }
            ChunkType::ContentBlockDelta => {
                if let Some(UnifiedContent::Text { text }) = chunk.delta.as_ref() {
                    json!({
                        "type": "response.output_text.delta",
                        "item_id": format!("item_{}", chunk.index),
                        "output_index": chunk.index,
                        "delta": text
                    })
                } else {
                    return Ok(String::new());
                }
            }
            ChunkType::MessageDelta => {
                json!({
                    "type": "response.completed",
                    "response": {
                        "status": "completed",
                        "usage": chunk.usage.as_ref().map(|u| json!({
                            "input_tokens": u.input_tokens,
                            "output_tokens": u.output_tokens,
                            "total_tokens": u.total_tokens()
                        }))
                    }
                })
            }
            ChunkType::MessageStop => {
                return Ok("data: [DONE]\n\n".to_string());
            }
            _ => return Ok(String::new()),
        };

        Ok(format!("data: {}\n\n", event))
    }

    fn endpoint(&self) -> &'static str {
        "/v1/responses"
    }

    fn can_handle(&self, raw: &Value) -> bool {
        // Response API indicators
        raw.get("input").is_some()
            || (raw.get("instructions").is_some() && raw.get("messages").is_none())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_api_transformer_protocol() {
        let transformer = ResponseApiTransformer::new();
        assert_eq!(transformer.protocol(), Protocol::ResponseApi);
    }

    #[test]
    fn test_transform_request_out_simple() {
        let transformer = ResponseApiTransformer::new();
        let raw = json!({
            "model": "gpt-4",
            "input": "What is the weather?"
        });

        let unified = transformer.transform_request_out(raw).unwrap();
        assert_eq!(unified.model, "gpt-4");
        assert_eq!(unified.messages.len(), 1);
        assert_eq!(unified.messages[0].text_content(), "What is the weather?");
    }

    #[test]
    fn test_transform_request_out_with_instructions() {
        let transformer = ResponseApiTransformer::new();
        let raw = json!({
            "model": "gpt-4",
            "instructions": "You are a weather assistant.",
            "input": "What is the weather?"
        });

        let unified = transformer.transform_request_out(raw).unwrap();
        assert_eq!(
            unified.system,
            Some("You are a weather assistant.".to_string())
        );
    }

    #[test]
    fn test_transform_request_in() {
        let transformer = ResponseApiTransformer::new();
        let unified = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello!")])
            .with_system("Be helpful")
            .with_max_tokens(100);

        let raw = transformer.transform_request_in(&unified).unwrap();
        assert_eq!(raw["model"], "gpt-4");
        assert_eq!(raw["instructions"], "Be helpful");
        assert_eq!(raw["max_output_tokens"], 100);
    }

    #[test]
    fn test_transform_response_in() {
        let transformer = ResponseApiTransformer::new();
        let raw = json!({
            "id": "resp_123",
            "object": "response",
            "created_at": 1234567890,
            "model": "gpt-4",
            "output": [{
                "type": "message",
                "id": "msg_1",
                "role": "assistant",
                "content": [{"type": "output_text", "text": "The weather is sunny."}],
                "status": "completed"
            }],
            "status": "completed",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15
            }
        });

        let unified = transformer.transform_response_in(raw, "gpt-4").unwrap();
        assert_eq!(unified.id, "resp_123");
        assert_eq!(unified.text_content(), "The weather is sunny.");
    }

    #[test]
    fn test_can_handle() {
        let transformer = ResponseApiTransformer::new();

        // Response API format with input
        let request = json!({
            "model": "gpt-4",
            "input": "Hello"
        });
        assert!(transformer.can_handle(&request));

        // Response API format with instructions
        let request = json!({
            "model": "gpt-4",
            "instructions": "Be helpful"
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
