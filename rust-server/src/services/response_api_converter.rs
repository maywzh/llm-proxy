//! Response API to OpenAI format conversion utilities.
//!
//! This module provides bidirectional conversion between OpenAI Response API format
//! and OpenAI Chat Completions API format for request/response handling.
//!
//! Response API is OpenAI's newer API format (/v1/responses) that supports:
//! - Stateful conversations with session management
//! - Computer use and other advanced tools
//! - Structured output with JSON schemas

use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::pin::Pin;
use uuid::Uuid;

// ============================================================================
// Response API Types
// ============================================================================

/// Response API input types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseInput {
    Text(String),
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
// Request Conversion: Response API -> OpenAI Chat Completions
// ============================================================================

/// Convert Response API request to OpenAI Chat Completions format.
pub fn response_api_to_openai_request(request: &ResponseApiRequest) -> Value {
    let mut messages = Vec::new();

    // Add system message from instructions
    if let Some(ref instructions) = request.instructions {
        messages.push(json!({
            "role": "system",
            "content": instructions
        }));
    }

    // Convert input to messages
    if let Some(ref input) = request.input {
        let converted = convert_input_to_messages(input);
        messages.extend(converted);
    }

    // Build OpenAI request
    let mut openai_request = json!({
        "model": request.model,
        "messages": messages,
    });

    // Add optional parameters
    if let Some(max_tokens) = request.max_output_tokens {
        openai_request["max_tokens"] = json!(max_tokens);
    }
    if let Some(temperature) = request.temperature {
        openai_request["temperature"] = json!(temperature);
    }
    if let Some(top_p) = request.top_p {
        openai_request["top_p"] = json!(top_p);
    }
    if request.stream {
        openai_request["stream"] = json!(true);
    }

    // Convert tools
    if let Some(ref tools) = request.tools {
        let openai_tools = convert_response_tools_to_openai(tools);
        if !openai_tools.is_empty() {
            openai_request["tools"] = json!(openai_tools);
        }
    }

    // Pass through tool_choice
    if let Some(ref tool_choice) = request.tool_choice {
        openai_request["tool_choice"] = tool_choice.clone();
    }

    // Pass through response_format
    if let Some(ref response_format) = request.response_format {
        openai_request["response_format"] = response_format.clone();
    }

    openai_request
}

/// Convert Response API input to OpenAI messages.
fn convert_input_to_messages(input: &ResponseInput) -> Vec<Value> {
    match input {
        ResponseInput::Text(text) => {
            vec![json!({
                "role": "user",
                "content": text
            })]
        }
        ResponseInput::Items(items) => items
            .iter()
            .filter_map(|item| match item {
                ResponseInputItem::Message { role, content } => {
                    let openai_content = convert_response_content_to_openai(content);
                    Some(json!({
                        "role": role,
                        "content": openai_content
                    }))
                }
                ResponseInputItem::ItemReference { .. } => None,
            })
            .collect(),
    }
}

/// Convert Response API content to OpenAI format.
fn convert_response_content_to_openai(content: &ResponseContent) -> Value {
    match content {
        ResponseContent::Text(text) => json!(text),
        ResponseContent::Parts(parts) => {
            let openai_parts: Vec<Value> = parts
                .iter()
                .filter_map(|part| match part {
                    ResponseContentPart::InputText { text }
                    | ResponseContentPart::OutputText { text } => Some(json!({
                        "type": "text",
                        "text": text
                    })),
                    ResponseContentPart::InputImage { image_url } => Some(json!({
                        "type": "image_url",
                        "image_url": { "url": image_url }
                    })),
                    ResponseContentPart::ToolUse { .. }
                    | ResponseContentPart::ToolResult { .. } => {
                        None // Handled separately as tool_calls
                    }
                })
                .collect();

            if openai_parts.len() == 1 {
                if let Some(text) = openai_parts[0].get("text") {
                    return text.clone();
                }
            }
            json!(openai_parts)
        }
    }
}

/// Convert Response API tools to OpenAI format.
fn convert_response_tools_to_openai(tools: &[ResponseTool]) -> Vec<Value> {
    tools
        .iter()
        .filter_map(|tool| match tool {
            ResponseTool::Function {
                name,
                description,
                parameters,
            } => Some(json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": parameters
                }
            })),
            // Other tool types are not directly supported in OpenAI Chat Completions
            _ => None,
        })
        .collect()
}

// ============================================================================
// Response Conversion: OpenAI Chat Completions -> Response API
// ============================================================================

/// Convert OpenAI Chat Completions response to Response API format.
pub fn openai_to_response_api_response(
    openai_response: &Value,
    original_model: &str,
) -> ResponseApiResponse {
    let id = openai_response
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("resp_{}", &Uuid::new_v4().simple().to_string()[..24]));

    let created_at = openai_response
        .get("created")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    let mut output = Vec::new();
    let mut status = "completed".to_string();

    // Process choices
    if let Some(choices) = openai_response.get("choices").and_then(|c| c.as_array()) {
        if let Some(first_choice) = choices.first() {
            // Check finish_reason
            if let Some(finish_reason) = first_choice.get("finish_reason").and_then(|r| r.as_str())
            {
                status = match finish_reason {
                    "stop" => "completed".to_string(),
                    "length" => "incomplete".to_string(),
                    "content_filter" => "failed".to_string(),
                    "tool_calls" => "completed".to_string(),
                    _ => "completed".to_string(),
                };
            }

            if let Some(message) = first_choice.get("message") {
                // Extract text content
                let text_content = message
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string());

                if let Some(text) = text_content {
                    if !text.is_empty() {
                        output.push(ResponseOutputItem::Message {
                            id: format!("msg_{}", &Uuid::new_v4().simple().to_string()[..24]),
                            role: "assistant".to_string(),
                            content: vec![ResponseOutputContent::OutputText { text }],
                            status: "completed".to_string(),
                        });
                    }
                }

                // Extract tool_calls
                if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tool_calls {
                        let tc_id = tc
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let function = tc.get("function");
                        let name = function
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = function
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("{}")
                            .to_string();

                        output.push(ResponseOutputItem::FunctionCall {
                            id: format!("fc_{}", &Uuid::new_v4().simple().to_string()[..24]),
                            call_id: tc_id,
                            name,
                            arguments,
                            status: "completed".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Extract usage
    let usage = if let Some(usage_obj) = openai_response.get("usage") {
        let input_tokens = usage_obj
            .get("prompt_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let output_tokens = usage_obj
            .get("completion_tokens")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        ResponseUsage {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
        }
    } else {
        ResponseUsage::default()
    };

    ResponseApiResponse {
        id,
        object: "response".to_string(),
        created_at,
        model: original_model.to_string(),
        output,
        status,
        status_details: None,
        usage,
    }
}

// ============================================================================
// Streaming Conversion: OpenAI -> Response API
// ============================================================================

/// State for streaming conversion.
struct StreamingState {
    response_id: String,
    original_model: String,
    created_at: i64,
    /// Accumulated text for the current message
    text_buffer: String,
    /// Current tool calls being built
    current_tool_calls: HashMap<i32, ToolCallState>,
    /// Final status from finish_reason
    final_status: String,
    /// Usage data
    usage: ResponseUsage,
    /// Whether message_started event has been emitted (on-demand synthesis)
    response_created: bool,
    /// Whether output_item events have been started
    output_item_started: bool,
}

/// State for tracking tool calls during streaming.
#[derive(Default)]
struct ToolCallState {
    id: String,
    name: String,
    arguments: String,
}

/// Convert OpenAI streaming response to Response API streaming format.
pub fn convert_openai_streaming_to_response_api(
    openai_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    original_model: String,
) -> Pin<Box<dyn Stream<Item = String> + Send>> {
    let response_id = format!("resp_{}", &Uuid::new_v4().simple().to_string()[..24]);
    let created_at = chrono::Utc::now().timestamp();

    let state = StreamingState {
        response_id: response_id.clone(),
        original_model: original_model.clone(),
        created_at,
        text_buffer: String::new(),
        current_tool_calls: HashMap::new(),
        final_status: "completed".to_string(),
        usage: ResponseUsage::default(),
        response_created: false,
        output_item_started: false,
    };

    // On-demand synthesis pattern - start with empty pending events
    let stream = futures::stream::unfold(
        (
            openai_stream,
            state,
            Vec::<String>::new(),
            false,
            String::new(),
        ),
        |(mut stream, mut state, mut pending_events, mut stream_done, mut sse_buffer)| async move {
            // First, send pending events
            if !pending_events.is_empty() {
                let event = pending_events.remove(0);
                return Some((
                    event,
                    (stream, state, pending_events, stream_done, sse_buffer),
                ));
            }

            if stream_done {
                return None;
            }

            loop {
                // Process buffered SSE events
                while let Some(event_end) = sse_buffer.find("\n\n") {
                    let event_str = sse_buffer[..event_end].to_string();
                    sse_buffer = sse_buffer[event_end + 2..].to_string();

                    for line in event_str.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with(':') {
                            continue;
                        }

                        if let Some(chunk_data) = line.strip_prefix("data: ") {
                            if chunk_data.trim() == "[DONE]" {
                                stream_done = true;
                                let final_events = generate_response_api_final_events(&state);
                                if !final_events.is_empty() {
                                    let mut events_iter = final_events.into_iter();
                                    if let Some(first_event) = events_iter.next() {
                                        let remaining: Vec<String> = events_iter.collect();
                                        return Some((
                                            first_event,
                                            (stream, state, remaining, stream_done, sse_buffer),
                                        ));
                                    }
                                }
                                return None;
                            }

                            if let Ok(chunk_json) = serde_json::from_str::<Value>(chunk_data) {
                                // Extract usage
                                if let Some(usage) = chunk_json.get("usage") {
                                    state.usage = extract_usage(usage);
                                }

                                let choices = chunk_json.get("choices").and_then(|c| c.as_array());
                                if choices.is_none() || choices.unwrap().is_empty() {
                                    continue;
                                }

                                let choice = &choices.unwrap()[0];
                                let delta = choice.get("delta");
                                let finish_reason =
                                    choice.get("finish_reason").and_then(|r| r.as_str());

                                if let Some(reason) = finish_reason {
                                    state.final_status = map_finish_reason_to_status(reason);
                                }

                                if let Some(delta) = delta {
                                    let mut events_to_send = Vec::new();

                                    // On-demand synthesis: emit response.created on first content
                                    let has_content = delta.get("content").is_some()
                                        || delta.get("tool_calls").is_some();

                                    if has_content && !state.response_created {
                                        events_to_send.push(format_response_api_event(
                                            "response.created",
                                            &json!({
                                                "type": "response.created",
                                                "response": {
                                                    "id": state.response_id,
                                                    "object": "response",
                                                    "created_at": state.created_at,
                                                    "model": state.original_model,
                                                    "status": "in_progress",
                                                    "output": []
                                                }
                                            }),
                                        ));
                                        state.response_created = true;
                                    }

                                    // Handle text delta
                                    if let Some(content) =
                                        delta.get("content").and_then(|c| c.as_str())
                                    {
                                        // Emit output_item.added on first text
                                        if !state.output_item_started && !content.is_empty() {
                                            events_to_send.push(format_response_api_event(
                                                "response.output_item.added",
                                                &json!({
                                                    "type": "response.output_item.added",
                                                    "output_index": 0,
                                                    "item": {
                                                        "type": "message",
                                                        "id": format!("msg_{}", &Uuid::new_v4().simple().to_string()[..24]),
                                                        "role": "assistant",
                                                        "content": [],
                                                        "status": "in_progress"
                                                    }
                                                }),
                                            ));
                                            events_to_send.push(format_response_api_event(
                                                "response.content_part.added",
                                                &json!({
                                                    "type": "response.content_part.added",
                                                    "output_index": 0,
                                                    "content_index": 0,
                                                    "part": {
                                                        "type": "output_text",
                                                        "text": ""
                                                    }
                                                }),
                                            ));
                                            state.output_item_started = true;
                                        }

                                        state.text_buffer.push_str(content);

                                        events_to_send.push(format_response_api_event(
                                            "response.output_text.delta",
                                            &json!({
                                                "type": "response.output_text.delta",
                                                "output_index": 0,
                                                "content_index": 0,
                                                "delta": content
                                            }),
                                        ));
                                    }

                                    // Handle tool_calls
                                    if let Some(tool_calls) =
                                        delta.get("tool_calls").and_then(|t| t.as_array())
                                    {
                                        for tc_delta in tool_calls {
                                            let events = process_tool_call_for_response_api(
                                                tc_delta, &mut state,
                                            );
                                            events_to_send.extend(events);
                                        }
                                    }

                                    if !events_to_send.is_empty() {
                                        let mut events_iter = events_to_send.into_iter();
                                        if let Some(first_event) = events_iter.next() {
                                            let remaining: Vec<String> = events_iter.collect();
                                            pending_events.extend(remaining);
                                            return Some((
                                                first_event,
                                                (
                                                    stream,
                                                    state,
                                                    pending_events,
                                                    stream_done,
                                                    sse_buffer,
                                                ),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Need more data from stream
                match stream.next().await {
                    Some(Ok(bytes)) => {
                        let chunk_str = String::from_utf8_lossy(&bytes);
                        sse_buffer.push_str(&chunk_str);
                    }
                    Some(Err(e)) => {
                        tracing::error!("Streaming error: {}", e);
                        stream_done = true;
                        let error_event = format_response_api_event(
                            "error",
                            &json!({
                                "type": "error",
                                "error": {"type": "api_error", "message": format!("Streaming error: {}", e)}
                            }),
                        );
                        return Some((
                            error_event,
                            (stream, state, pending_events, stream_done, sse_buffer),
                        ));
                    }
                    None => {
                        stream_done = true;
                        let final_events = generate_response_api_final_events(&state);
                        if !final_events.is_empty() {
                            let mut events_iter = final_events.into_iter();
                            if let Some(first_event) = events_iter.next() {
                                let remaining: Vec<String> = events_iter.collect();
                                return Some((
                                    first_event,
                                    (stream, state, remaining, stream_done, sse_buffer),
                                ));
                            }
                        }
                        return None;
                    }
                }
            }
        },
    );

    Box::pin(stream)
}

/// Generate final events for Response API stream.
fn generate_response_api_final_events(state: &StreamingState) -> Vec<String> {
    let mut events = Vec::new();

    // Only emit events if response was created
    if state.response_created {
        // Emit content_part.done if text was sent
        if state.output_item_started && !state.text_buffer.is_empty() {
            events.push(format_response_api_event(
                "response.content_part.done",
                &json!({
                    "type": "response.content_part.done",
                    "output_index": 0,
                    "content_index": 0,
                    "part": {
                        "type": "output_text",
                        "text": state.text_buffer
                    }
                }),
            ));

            events.push(format_response_api_event(
                "response.output_item.done",
                &json!({
                    "type": "response.output_item.done",
                    "output_index": 0,
                    "item": {
                        "type": "message",
                        "id": format!("msg_{}", &Uuid::new_v4().simple().to_string()[..24]),
                        "role": "assistant",
                        "content": [{
                            "type": "output_text",
                            "text": state.text_buffer
                        }],
                        "status": "completed"
                    }
                }),
            ));
        }

        // Emit response.done
        events.push(format_response_api_event(
            "response.done",
            &json!({
                "type": "response.done",
                "response": {
                    "id": state.response_id,
                    "object": "response",
                    "created_at": state.created_at,
                    "model": state.original_model,
                    "status": state.final_status,
                    "output": build_output_for_response(state),
                    "usage": state.usage
                }
            }),
        ));
    }

    events
}

/// Build output array for final response.
fn build_output_for_response(state: &StreamingState) -> Vec<Value> {
    let mut output = Vec::new();

    // Add text message
    if !state.text_buffer.is_empty() {
        output.push(json!({
            "type": "message",
            "id": format!("msg_{}", &Uuid::new_v4().simple().to_string()[..24]),
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": state.text_buffer
            }],
            "status": "completed"
        }));
    }

    // Add tool calls
    for tc in state.current_tool_calls.values() {
        output.push(json!({
            "type": "function_call",
            "id": format!("fc_{}", &Uuid::new_v4().simple().to_string()[..24]),
            "call_id": tc.id,
            "name": tc.name,
            "arguments": tc.arguments,
            "status": "completed"
        }));
    }

    output
}

/// Process tool call delta for Response API streaming.
fn process_tool_call_for_response_api(tc_delta: &Value, state: &mut StreamingState) -> Vec<String> {
    let mut events = Vec::new();
    let tc_index = tc_delta.get("index").and_then(|i| i.as_i64()).unwrap_or(0) as i32;

    state.current_tool_calls.entry(tc_index).or_default();
    let tool_call = state.current_tool_calls.get_mut(&tc_index).unwrap();

    // Extract tool call data
    if let Some(id) = tc_delta.get("id").and_then(|i| i.as_str()) {
        tool_call.id = id.to_string();
    }

    if let Some(function) = tc_delta.get("function") {
        if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
            tool_call.name = name.to_string();

            // Emit function_call_arguments.delta for new tool call
            events.push(format_response_api_event(
                "response.function_call_arguments.delta",
                &json!({
                    "type": "response.function_call_arguments.delta",
                    "output_index": tc_index,
                    "call_id": tool_call.id,
                    "delta": ""
                }),
            ));
        }

        if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
            tool_call.arguments.push_str(args);

            events.push(format_response_api_event(
                "response.function_call_arguments.delta",
                &json!({
                    "type": "response.function_call_arguments.delta",
                    "output_index": tc_index,
                    "call_id": tool_call.id,
                    "delta": args
                }),
            ));
        }
    }

    events
}

/// Format a Response API SSE event.
fn format_response_api_event(event_type: &str, data: &Value) -> String {
    format!(
        "event: {}\ndata: {}\n\n",
        event_type,
        serde_json::to_string(data).unwrap_or_default()
    )
}

/// Extract usage from OpenAI response.
fn extract_usage(usage: &Value) -> ResponseUsage {
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;

    ResponseUsage {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens + output_tokens,
    }
}

/// Map OpenAI finish_reason to Response API status.
fn map_finish_reason_to_status(reason: &str) -> String {
    match reason {
        "stop" => "completed".to_string(),
        "length" => "incomplete".to_string(),
        "content_filter" => "failed".to_string(),
        "tool_calls" => "completed".to_string(),
        _ => "completed".to_string(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_api_to_openai_request_basic() {
        let request = ResponseApiRequest {
            model: "gpt-4".to_string(),
            input: Some(ResponseInput::Text("Hello".to_string())),
            instructions: Some("You are helpful".to_string()),
            max_output_tokens: Some(100),
            temperature: Some(0.7),
            top_p: None,
            tools: None,
            tool_choice: None,
            response_format: None,
            stream: false,
            extra: HashMap::new(),
        };

        let openai_request = response_api_to_openai_request(&request);

        assert_eq!(openai_request["model"], "gpt-4");
        assert_eq!(openai_request["max_tokens"], 100);
        assert_eq!(openai_request["temperature"], 0.7);

        let messages = openai_request["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are helpful");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Hello");
    }

    #[test]
    fn test_openai_to_response_api_response() {
        let openai_response = json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });

        let response = openai_to_response_api_response(&openai_response, "gpt-4");

        assert_eq!(response.model, "gpt-4");
        assert_eq!(response.status, "completed");
        assert_eq!(response.usage.input_tokens, 10);
        assert_eq!(response.usage.output_tokens, 5);
        assert_eq!(response.output.len(), 1);

        if let ResponseOutputItem::Message { content, .. } = &response.output[0] {
            if let ResponseOutputContent::OutputText { text } = &content[0] {
                assert_eq!(text, "Hello! How can I help?");
            }
        }
    }

    #[test]
    fn test_map_finish_reason_to_status() {
        assert_eq!(map_finish_reason_to_status("stop"), "completed");
        assert_eq!(map_finish_reason_to_status("length"), "incomplete");
        assert_eq!(map_finish_reason_to_status("content_filter"), "failed");
        assert_eq!(map_finish_reason_to_status("tool_calls"), "completed");
    }
}
