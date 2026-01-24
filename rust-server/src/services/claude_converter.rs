//! Claude to OpenAI format conversion utilities.
//!
//! This module provides bidirectional conversion between Claude API format
//! and OpenAI API format for request/response handling.

use crate::api::claude_models::{
    constants, ClaudeContentBlock, ClaudeContentBlockText, ClaudeContentBlockToolUse,
    ClaudeMessage, ClaudeMessageContent, ClaudeMessagesRequest, ClaudeResponse,
    ClaudeSystemPrompt, ClaudeTool, ClaudeUsage,
};
use crate::api::models::get_mapped_model;
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::pin::Pin;
use uuid::Uuid;

// ============================================================================
// Request Conversion: Claude -> OpenAI
// ============================================================================

/// Convert Claude API request format to OpenAI format.
///
/// # Arguments
///
/// * `claude_request` - The Claude Messages API request
/// * `model_mapping` - Optional model name mapping dict
/// * `min_tokens_limit` - Minimum tokens limit for clamping
/// * `max_tokens_limit` - Maximum tokens limit for clamping
///
/// # Returns
///
/// OpenAI-compatible request as JSON Value
pub fn claude_to_openai_request(
    claude_request: &ClaudeMessagesRequest,
    model_mapping: Option<&HashMap<String, String>>,
    min_tokens_limit: u32,
    max_tokens_limit: u32,
) -> Value {
    // Map model using provider's model_mapping if available (supports wildcard patterns)
    let openai_model = model_mapping
        .map(|m| get_mapped_model(&claude_request.model, m))
        .unwrap_or_else(|| claude_request.model.clone());

    // Convert messages
    let mut openai_messages: Vec<Value> = Vec::new();

    // Add system message if present
    if let Some(ref system) = claude_request.system {
        let system_text = extract_system_text(system);
        if !system_text.trim().is_empty() {
            openai_messages.push(json!({
                "role": constants::ROLE_SYSTEM,
                "content": system_text.trim()
            }));
        }
    }

    // Process Claude messages
    let mut i = 0;
    while i < claude_request.messages.len() {
        let msg = &claude_request.messages[i];

        if msg.role == constants::ROLE_USER {
            let openai_message = convert_claude_user_message(msg);
            openai_messages.push(openai_message);
        } else if msg.role == constants::ROLE_ASSISTANT {
            let openai_message = convert_claude_assistant_message(msg);
            openai_messages.push(openai_message);

            // Check if next message contains tool results
            if i + 1 < claude_request.messages.len() {
                let next_msg = &claude_request.messages[i + 1];
                if is_tool_result_message(next_msg) {
                    // Process tool results
                    i += 1; // Skip to tool result message
                    let tool_results = convert_claude_tool_results(next_msg);
                    openai_messages.extend(tool_results);
                }
            }
        }

        i += 1;
    }

    // Clamp max_tokens to configured limits
    let clamped_max_tokens = claude_request
        .max_tokens
        .max(min_tokens_limit as i32)
        .min(max_tokens_limit as i32);

    // Build OpenAI request
    let mut openai_request = json!({
        "model": openai_model,
        "messages": openai_messages,
        "max_tokens": clamped_max_tokens,
        "stream": claude_request.stream,
    });

    // Add optional parameters
    if let Some(temp) = claude_request.temperature {
        openai_request["temperature"] = json!(temp);
    }
    if let Some(ref stop_sequences) = claude_request.stop_sequences {
        openai_request["stop"] = json!(stop_sequences);
    }
    if let Some(top_p) = claude_request.top_p {
        openai_request["top_p"] = json!(top_p);
    }

    // Convert tools
    if let Some(ref tools) = claude_request.tools {
        let openai_tools = convert_claude_tools(tools);
        if !openai_tools.is_empty() {
            openai_request["tools"] = json!(openai_tools);
        }
    }

    // Convert tool choice
    if let Some(ref tool_choice) = claude_request.tool_choice {
        openai_request["tool_choice"] = convert_tool_choice(tool_choice);
    }

    tracing::debug!(
        "Converted Claude request to OpenAI format: model={}, messages_count={}",
        openai_model,
        openai_messages.len()
    );

    openai_request
}

// ============================================================================
// Response Conversion: OpenAI -> Claude
// ============================================================================

/// Convert OpenAI response to Claude format.
///
/// # Arguments
///
/// * `openai_response` - The OpenAI API response
/// * `original_model` - The original Claude model name from request
///
/// # Returns
///
/// Claude-compatible response
pub fn openai_to_claude_response(
    openai_response: &Value,
    original_model: &str,
) -> Result<ClaudeResponse, String> {
    // Extract response data
    let choices = openai_response
        .get("choices")
        .and_then(|c| c.as_array())
        .ok_or("No choices in OpenAI response")?;

    if choices.is_empty() {
        return Err("No choices in OpenAI response".to_string());
    }

    let choice = &choices[0];
    let message = choice.get("message").ok_or("No message in choice")?;

    // Build Claude content blocks
    let content_blocks = build_content_blocks(message);

    // Map finish reason
    let finish_reason = choice
        .get("finish_reason")
        .and_then(|r| r.as_str())
        .unwrap_or("stop");
    let stop_reason = map_finish_reason(finish_reason);

    // Extract usage
    let usage = openai_response
        .get("usage")
        .map(|u| ClaudeUsage {
            input_tokens: u.get("prompt_tokens").and_then(|t| t.as_i64()).unwrap_or(0) as i32,
            output_tokens: u
                .get("completion_tokens")
                .and_then(|t| t.as_i64())
                .unwrap_or(0) as i32,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        })
        .unwrap_or_default();

    // Generate message ID
    let id = openai_response
        .get("id")
        .and_then(|id| id.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("msg_{}", Uuid::new_v4().simple()));

    Ok(ClaudeResponse::new(
        id,
        original_model,
        content_blocks,
        Some(stop_reason),
        usage,
    ))
}

// ============================================================================
// Streaming Conversion: OpenAI -> Claude
// ============================================================================

/// State for streaming conversion.
#[allow(dead_code)]
struct StreamingState {
    message_id: String,
    original_model: String,
    text_block_index: i32,
    tool_block_counter: i32,
    current_tool_calls: HashMap<i32, ToolCallState>,
    final_stop_reason: String,
    usage_data: ClaudeUsage,
}

/// State for tracking tool calls during streaming.
struct ToolCallState {
    id: Option<String>,
    name: Option<String>,
    args_buffer: String,
    json_sent: bool,
    claude_index: Option<i32>,
    started: bool,
}

impl Default for ToolCallState {
    fn default() -> Self {
        Self {
            id: None,
            name: None,
            args_buffer: String::new(),
            json_sent: false,
            claude_index: None,
            started: false,
        }
    }
}

/// Convert OpenAI streaming response to Claude streaming format.
///
/// # Arguments
///
/// * `openai_stream` - Async iterator of OpenAI SSE chunks (bytes)
/// * `original_model` - The original Claude model name from request
///
/// # Returns
///
/// Stream of Claude-formatted SSE event strings
pub fn convert_openai_streaming_to_claude(
    openai_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    original_model: String,
) -> Pin<Box<dyn Stream<Item = String> + Send>> {
    let message_id = format!("msg_{}", Uuid::new_v4().simple().to_string()[..24].to_string());

    let state = StreamingState {
        message_id: message_id.clone(),
        original_model: original_model.clone(),
        text_block_index: 0,
        tool_block_counter: 0,
        current_tool_calls: HashMap::new(),
        final_stop_reason: constants::STOP_END_TURN.to_string(),
        usage_data: ClaudeUsage::default(),
    };

    // Create initial events
    let initial_events = vec![
        // message_start event
        format_sse_event(
            constants::EVENT_MESSAGE_START,
            &json!({
                "type": constants::EVENT_MESSAGE_START,
                "message": {
                    "id": message_id,
                    "type": "message",
                    "role": constants::ROLE_ASSISTANT,
                    "model": original_model,
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {"input_tokens": 0, "output_tokens": 0}
                }
            }),
        ),
        // content_block_start event
        format_sse_event(
            constants::EVENT_CONTENT_BLOCK_START,
            &json!({
                "type": constants::EVENT_CONTENT_BLOCK_START,
                "index": 0,
                "content_block": {"type": constants::CONTENT_TEXT, "text": ""}
            }),
        ),
        // ping event
        format_sse_event(
            constants::EVENT_PING,
            &json!({"type": constants::EVENT_PING}),
        ),
    ];

    // Create the stream
    // State tuple: (stream, state, pending_events, initial_sent, stream_done, sse_buffer)
    // stream_done: true when [DONE] received or stream ended - no more polling
    let stream = futures::stream::unfold(
        (openai_stream, state, initial_events, false, false, String::new()),
        |(mut stream, mut state, mut pending_events, mut initial_sent, mut stream_done, mut sse_buffer)| async move {
            // First, send pending events (initial events or queued final events)
            if !pending_events.is_empty() {
                let event = pending_events.remove(0);
                if !initial_sent && pending_events.is_empty() {
                    initial_sent = true;
                }
                return Some((event, (stream, state, pending_events, initial_sent, stream_done, sse_buffer)));
            }
            
            // If stream is done and no more pending events, terminate immediately
            if stream_done {
                return None;
            }

            // Process stream chunks
            loop {
                // First, try to extract complete SSE events from buffer
                while let Some(event_end) = sse_buffer.find("\n\n") {
                    let event_str = sse_buffer[..event_end].to_string();
                    sse_buffer = sse_buffer[event_end + 2..].to_string();
                    
                    // Process each line in the event
                    for line in event_str.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with(':') {
                            continue;
                        }
                        
                        if line.starts_with("data: ") {
                            let chunk_data = &line[6..];
                            if chunk_data.trim() == "[DONE]" {
                                // Stream is done - generate final events and mark as done
                                stream_done = true;
                                let final_events = generate_final_events(&state);
                                if !final_events.is_empty() {
                                    let mut events_iter = final_events.into_iter();
                                    if let Some(first_event) = events_iter.next() {
                                        let remaining: Vec<String> = events_iter.collect();
                                        return Some((
                                            first_event,
                                            (stream, state, remaining, initial_sent, stream_done, sse_buffer),
                                        ));
                                    }
                                }
                                return None;
                            }

                            match serde_json::from_str::<Value>(chunk_data) {
                                Ok(chunk_json) => {
                                    // Extract usage if present
                                    if let Some(usage) = chunk_json.get("usage") {
                                        state.usage_data = extract_usage_data(usage);
                                    }

                                    let choices = chunk_json.get("choices").and_then(|c| c.as_array());
                                    if choices.is_none() || choices.unwrap().is_empty() {
                                        continue;
                                    }

                                    let choice = &choices.unwrap()[0];
                                    let delta = choice.get("delta");
                                    let finish_reason = choice.get("finish_reason").and_then(|r| r.as_str());

                                    // Handle finish reason first (may come before or with delta)
                                    if let Some(reason) = finish_reason {
                                        state.final_stop_reason = map_finish_reason(reason);
                                    }

                                    // Handle text delta
                                    if let Some(delta) = delta {
                                        if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                                            // Send text delta even if empty to match Python/claude-code-proxy behavior
                                            let event = format_sse_event(
                                                constants::EVENT_CONTENT_BLOCK_DELTA,
                                                &json!({
                                                    "type": constants::EVENT_CONTENT_BLOCK_DELTA,
                                                    "index": state.text_block_index,
                                                    "delta": {
                                                        "type": constants::DELTA_TEXT,
                                                        "text": content
                                                    }
                                                }),
                                            );
                                            return Some((event, (stream, state, pending_events, initial_sent, stream_done, sse_buffer)));
                                        }

                                        // Handle tool call deltas
                                        if let Some(tool_calls) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                                            let events = process_tool_call_delta(tool_calls, &mut state);
                                            if !events.is_empty() {
                                                let mut events_iter = events.into_iter();
                                                if let Some(first_event) = events_iter.next() {
                                                    let remaining: Vec<String> = events_iter.collect();
                                                    let mut new_pending = pending_events;
                                                    new_pending.extend(remaining);
                                                    return Some((first_event, (stream, state, new_pending, initial_sent, stream_done, sse_buffer)));
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to parse chunk: {}, error: {}", chunk_data, e);
                                    continue;
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
                        // Continue loop to process buffer
                    }
                    Some(Err(e)) => {
                        tracing::error!("Streaming error: {}", e);
                        stream_done = true;
                        let error_event = format!(
                            "event: error\ndata: {}\n\n",
                            json!({
                                "type": "error",
                                "error": {"type": "api_error", "message": format!("Streaming error: {}", e)}
                            })
                        );
                        return Some((error_event, (stream, state, pending_events, initial_sent, stream_done, sse_buffer)));
                    }
                    None => {
                        // Stream ended without [DONE], send final events
                        stream_done = true;
                        let final_events = generate_final_events(&state);
                        if !final_events.is_empty() {
                            let mut events_iter = final_events.into_iter();
                            if let Some(first_event) = events_iter.next() {
                                let remaining: Vec<String> = events_iter.collect();
                                return Some((first_event, (stream, state, remaining, initial_sent, stream_done, sse_buffer)));
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

/// Generate final SSE events for stream completion.
fn generate_final_events(state: &StreamingState) -> Vec<String> {
    let mut events = Vec::new();

    // content_block_stop for text block
    events.push(format_sse_event(
        constants::EVENT_CONTENT_BLOCK_STOP,
        &json!({
            "type": constants::EVENT_CONTENT_BLOCK_STOP,
            "index": state.text_block_index
        }),
    ));

    // Send any remaining args_buffer content and content_block_stop for tool blocks
    for tool_data in state.current_tool_calls.values() {
        if tool_data.started {
            if let Some(claude_index) = tool_data.claude_index {
                // Send remaining args_buffer content even if not valid JSON
                // This prevents tool calls from appearing with empty input objects
                if !tool_data.json_sent && !tool_data.args_buffer.is_empty() {
                    events.push(format_sse_event(
                        constants::EVENT_CONTENT_BLOCK_DELTA,
                        &json!({
                            "type": constants::EVENT_CONTENT_BLOCK_DELTA,
                            "index": claude_index,
                            "delta": {
                                "type": constants::DELTA_INPUT_JSON,
                                "partial_json": tool_data.args_buffer
                            }
                        }),
                    ));
                }
                events.push(format_sse_event(
                    constants::EVENT_CONTENT_BLOCK_STOP,
                    &json!({
                        "type": constants::EVENT_CONTENT_BLOCK_STOP,
                        "index": claude_index
                    }),
                ));
            }
        }
    }

    // message_delta
    events.push(format_sse_event(
        constants::EVENT_MESSAGE_DELTA,
        &json!({
            "type": constants::EVENT_MESSAGE_DELTA,
            "delta": {"stop_reason": state.final_stop_reason, "stop_sequence": null},
            "usage": state.usage_data
        }),
    ));

    // message_stop
    events.push(format_sse_event(
        constants::EVENT_MESSAGE_STOP,
        &json!({"type": constants::EVENT_MESSAGE_STOP}),
    ));

    events
}

/// Process tool call deltas and return SSE events.
fn process_tool_call_delta(tool_call_deltas: &[Value], state: &mut StreamingState) -> Vec<String> {
    let mut events = Vec::new();

    for tc_delta in tool_call_deltas {
        let tc_index = tc_delta.get("index").and_then(|i| i.as_i64()).unwrap_or(0) as i32;

        // Initialize tool call tracking by index if not exists
        if !state.current_tool_calls.contains_key(&tc_index) {
            state.current_tool_calls.insert(tc_index, ToolCallState::default());
        }

        let tool_call = state.current_tool_calls.get_mut(&tc_index).unwrap();

        // Update tool call ID if provided
        if let Some(id) = tc_delta.get("id").and_then(|i| i.as_str()) {
            tool_call.id = Some(id.to_string());
        }

        // Update function name
        if let Some(function_data) = tc_delta.get(constants::TOOL_FUNCTION) {
            if let Some(name) = function_data.get("name").and_then(|n| n.as_str()) {
                tool_call.name = Some(name.to_string());
            }

            // Handle function arguments
            if let Some(arguments) = function_data.get("arguments").and_then(|a| a.as_str()) {
                if tool_call.started {
                    tool_call.args_buffer.push_str(arguments);

                    // Try to parse complete JSON and send delta
                    if serde_json::from_str::<Value>(&tool_call.args_buffer).is_ok() && !tool_call.json_sent {
                        if let Some(claude_index) = tool_call.claude_index {
                            events.push(format_sse_event(
                                constants::EVENT_CONTENT_BLOCK_DELTA,
                                &json!({
                                    "type": constants::EVENT_CONTENT_BLOCK_DELTA,
                                    "index": claude_index,
                                    "delta": {
                                        "type": constants::DELTA_INPUT_JSON,
                                        "partial_json": tool_call.args_buffer
                                    }
                                }),
                            ));
                            tool_call.json_sent = true;
                        }
                    }
                }
            }
        }

        // Start content block when we have complete initial data
        if tool_call.id.is_some() && tool_call.name.is_some() && !tool_call.started {
            state.tool_block_counter += 1;
            let claude_index = state.text_block_index + state.tool_block_counter;
            tool_call.claude_index = Some(claude_index);
            tool_call.started = true;

            events.push(format_sse_event(
                constants::EVENT_CONTENT_BLOCK_START,
                &json!({
                    "type": constants::EVENT_CONTENT_BLOCK_START,
                    "index": claude_index,
                    "content_block": {
                        "type": constants::CONTENT_TOOL_USE,
                        "id": tool_call.id,
                        "name": tool_call.name,
                        "input": {}
                    }
                }),
            ));
        }
    }

    events
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extract system text from Claude system field.
fn extract_system_text(system: &ClaudeSystemPrompt) -> String {
    match system {
        ClaudeSystemPrompt::Text(text) => text.clone(),
        ClaudeSystemPrompt::Blocks(blocks) => {
            blocks
                .iter()
                .filter(|b| b.content_type == constants::CONTENT_TEXT)
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n")
        }
    }
}

/// Check if a message contains tool results.
fn is_tool_result_message(msg: &ClaudeMessage) -> bool {
    if msg.role != constants::ROLE_USER {
        return false;
    }
    match &msg.content {
        ClaudeMessageContent::Text(_) => false,
        ClaudeMessageContent::Blocks(blocks) => {
            blocks.iter().any(|block| block.get_type() == constants::CONTENT_TOOL_RESULT)
        }
    }
}

/// Convert Claude user message to OpenAI format.
fn convert_claude_user_message(msg: &ClaudeMessage) -> Value {
    match &msg.content {
        ClaudeMessageContent::Text(text) => {
            json!({"role": constants::ROLE_USER, "content": text})
        }
        ClaudeMessageContent::Blocks(blocks) => {
            let mut openai_content: Vec<Value> = Vec::new();

            for block in blocks {
                match block {
                    ClaudeContentBlock::Text(text_block) => {
                        openai_content.push(json!({"type": "text", "text": text_block.text}));
                    }
                    ClaudeContentBlock::Image(image_block) => {
                        let url = format!(
                            "data:{};base64,{}",
                            image_block.source.media_type, image_block.source.data
                        );
                        openai_content.push(json!({
                            "type": "image_url",
                            "image_url": {"url": url}
                        }));
                    }
                    _ => {}
                }
            }

            if openai_content.len() == 1 {
                if let Some(text) = openai_content[0].get("text").and_then(|t| t.as_str()) {
                    return json!({"role": constants::ROLE_USER, "content": text});
                }
            }

            json!({"role": constants::ROLE_USER, "content": openai_content})
        }
    }
}

/// Convert Claude assistant message to OpenAI format.
fn convert_claude_assistant_message(msg: &ClaudeMessage) -> Value {
    match &msg.content {
        ClaudeMessageContent::Text(text) => {
            json!({"role": constants::ROLE_ASSISTANT, "content": text})
        }
        ClaudeMessageContent::Blocks(blocks) => {
            let mut text_parts: Vec<String> = Vec::new();
            let mut tool_calls: Vec<Value> = Vec::new();

            for block in blocks {
                match block {
                    ClaudeContentBlock::Text(text_block) => {
                        text_parts.push(text_block.text.clone());
                    }
                    ClaudeContentBlock::ToolUse(tool_block) => {
                        tool_calls.push(json!({
                            "id": tool_block.id,
                            "type": constants::TOOL_FUNCTION,
                            constants::TOOL_FUNCTION: {
                                "name": tool_block.name,
                                "arguments": serde_json::to_string(&tool_block.input).unwrap_or_default()
                            }
                        }));
                    }
                    _ => {}
                }
            }

            let mut openai_message = json!({"role": constants::ROLE_ASSISTANT});

            if !text_parts.is_empty() {
                openai_message["content"] = json!(text_parts.join(""));
            } else {
                openai_message["content"] = Value::Null;
            }

            if !tool_calls.is_empty() {
                openai_message["tool_calls"] = json!(tool_calls);
            }

            openai_message
        }
    }
}

/// Convert Claude tool results to OpenAI format.
fn convert_claude_tool_results(msg: &ClaudeMessage) -> Vec<Value> {
    let mut tool_messages: Vec<Value> = Vec::new();

    if let ClaudeMessageContent::Blocks(blocks) = &msg.content {
        for block in blocks {
            if let ClaudeContentBlock::ToolResult(result_block) = block {
                let content = parse_tool_result_content(&result_block.content);
                tool_messages.push(json!({
                    "role": constants::ROLE_TOOL,
                    "tool_call_id": result_block.tool_use_id,
                    "content": content
                }));
            }
        }
    }

    tool_messages
}

/// Parse and normalize tool result content into a string format.
fn parse_tool_result_content(content: &Value) -> String {
    match content {
        Value::Null => "No content provided".to_string(),
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|item| {
                    if let Some(obj) = item.as_object() {
                        if obj.get("type").and_then(|t| t.as_str()) == Some(constants::CONTENT_TEXT) {
                            return obj.get("text").and_then(|t| t.as_str()).map(|s| s.to_string());
                        }
                    }
                    if let Some(s) = item.as_str() {
                        return Some(s.to_string());
                    }
                    serde_json::to_string(item).ok()
                })
                .collect();
            parts.join("\n").trim().to_string()
        }
        Value::Object(obj) => {
            if obj.get("type").and_then(|t| t.as_str()) == Some(constants::CONTENT_TEXT) {
                return obj
                    .get("text")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            serde_json::to_string(content).unwrap_or_else(|_| content.to_string())
        }
        _ => content.to_string(),
    }
}

/// Convert Claude tools to OpenAI format.
fn convert_claude_tools(tools: &[ClaudeTool]) -> Vec<Value> {
    tools
        .iter()
        .filter(|tool| !tool.name.trim().is_empty())
        .map(|tool| {
            json!({
                "type": constants::TOOL_FUNCTION,
                constants::TOOL_FUNCTION: {
                    "name": tool.name,
                    "description": tool.description.as_deref().unwrap_or(""),
                    "parameters": tool.input_schema
                }
            })
        })
        .collect()
}

/// Convert Claude tool_choice to OpenAI format.
fn convert_tool_choice(tool_choice: &Value) -> Value {
    let choice_type = tool_choice.get("type").and_then(|t| t.as_str());

    match choice_type {
        Some("auto") | Some("any") => json!("auto"),
        Some("tool") => {
            if let Some(name) = tool_choice.get("name").and_then(|n| n.as_str()) {
                json!({
                    "type": constants::TOOL_FUNCTION,
                    constants::TOOL_FUNCTION: {"name": name}
                })
            } else {
                json!("auto")
            }
        }
        _ => json!("auto"),
    }
}

/// Build Claude content blocks from OpenAI message.
fn build_content_blocks(message: &Value) -> Vec<ClaudeContentBlock> {
    let mut content_blocks: Vec<ClaudeContentBlock> = Vec::new();

    // Add text content
    if let Some(text_content) = message.get("content").and_then(|c| c.as_str()) {
        content_blocks.push(ClaudeContentBlock::Text(ClaudeContentBlockText {
            content_type: constants::CONTENT_TEXT.to_string(),
            text: text_content.to_string(),
        }));
    }

    // Add tool calls
    if let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) {
        for tool_call in tool_calls {
            if tool_call.get("type").and_then(|t| t.as_str()) == Some(constants::TOOL_FUNCTION) {
                if let Some(function_data) = tool_call.get(constants::TOOL_FUNCTION) {
                    let arguments: Value = function_data
                        .get("arguments")
                        .and_then(|a| a.as_str())
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_else(|| {
                            json!({"raw_arguments": function_data.get("arguments").and_then(|a| a.as_str()).unwrap_or("")})
                        });

                    content_blocks.push(ClaudeContentBlock::ToolUse(ClaudeContentBlockToolUse {
                        content_type: constants::CONTENT_TOOL_USE.to_string(),
                        id: tool_call
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or(&format!("tool_{}", Uuid::new_v4().simple()))
                            .to_string(),
                        name: function_data
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string(),
                        input: arguments,
                    }));
                }
            }
        }
    }

    // Ensure at least one content block
    if content_blocks.is_empty() {
        content_blocks.push(ClaudeContentBlock::Text(ClaudeContentBlockText {
            content_type: constants::CONTENT_TEXT.to_string(),
            text: String::new(),
        }));
    }

    content_blocks
}

/// Map OpenAI finish reason to Claude stop reason.
fn map_finish_reason(finish_reason: &str) -> String {
    match finish_reason {
        "stop" => constants::STOP_END_TURN.to_string(),
        "length" => constants::STOP_MAX_TOKENS.to_string(),
        "tool_calls" | "function_call" => constants::STOP_TOOL_USE.to_string(),
        _ => constants::STOP_END_TURN.to_string(),
    }
}

/// Format an SSE event string.
fn format_sse_event(event_type: &str, data: &Value) -> String {
    format!(
        "event: {}\ndata: {}\n\n",
        event_type,
        serde_json::to_string(data).unwrap_or_default()
    )
}

/// Extract usage data from OpenAI response.
fn extract_usage_data(usage: &Value) -> ClaudeUsage {
    let cache_read_input_tokens = usage
        .get("prompt_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(|t| t.as_i64())
        .map(|t| t as i32);

    ClaudeUsage {
        input_tokens: usage.get("prompt_tokens").and_then(|t| t.as_i64()).unwrap_or(0) as i32,
        output_tokens: usage
            .get("completion_tokens")
            .and_then(|t| t.as_i64())
            .unwrap_or(0) as i32,
        cache_creation_input_tokens: None,
        cache_read_input_tokens,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_claude_to_openai_request_basic() {
        let request = ClaudeMessagesRequest {
            model: "claude-3-opus-20240229".to_string(),
            max_tokens: 1024,
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Text("Hello!".to_string()),
            }],
            system: None,
            stop_sequences: None,
            stream: false,
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            metadata: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };

        let openai_request = claude_to_openai_request(&request, None, 100, 4096);

        assert_eq!(openai_request["model"], "claude-3-opus-20240229");
        assert_eq!(openai_request["max_tokens"], 1024);
        assert_eq!(openai_request["temperature"], 0.7);
        assert_eq!(openai_request["stream"], false);
    }

    #[test]
    fn test_claude_to_openai_request_with_system() {
        let request = ClaudeMessagesRequest {
            model: "claude-3-opus-20240229".to_string(),
            max_tokens: 1024,
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Text("Hello!".to_string()),
            }],
            system: Some(ClaudeSystemPrompt::Text("You are a helpful assistant.".to_string())),
            stop_sequences: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            metadata: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };

        let openai_request = claude_to_openai_request(&request, None, 100, 4096);

        let messages = openai_request["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"], "You are a helpful assistant.");
    }

    #[test]
    fn test_claude_to_openai_request_max_tokens_clamping() {
        // Test clamping to minimum
        let request = ClaudeMessagesRequest {
            model: "claude-3-opus-20240229".to_string(),
            max_tokens: 50, // Below minimum
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Text("Hello!".to_string()),
            }],
            system: None,
            stop_sequences: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            metadata: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };

        let openai_request = claude_to_openai_request(&request, None, 100, 4096);
        assert_eq!(openai_request["max_tokens"], 100); // Clamped to minimum

        // Test clamping to maximum
        let request2 = ClaudeMessagesRequest {
            model: "claude-3-opus-20240229".to_string(),
            max_tokens: 10000, // Above maximum
            messages: vec![ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Text("Hello!".to_string()),
            }],
            system: None,
            stop_sequences: None,
            stream: false,
            temperature: None,
            top_p: None,
            top_k: None,
            metadata: None,
            tools: None,
            tool_choice: None,
            thinking: None,
        };

        let openai_request2 = claude_to_openai_request(&request2, None, 100, 4096);
        assert_eq!(openai_request2["max_tokens"], 4096); // Clamped to maximum
    }

    #[test]
    fn test_openai_to_claude_response() {
        let openai_response = json!({
            "id": "chatcmpl-123",
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

        let claude_response = openai_to_claude_response(&openai_response, "claude-3-opus-20240229").unwrap();

        assert_eq!(claude_response.model, "claude-3-opus-20240229");
        assert_eq!(claude_response.stop_reason, Some("end_turn".to_string()));
        assert_eq!(claude_response.usage.input_tokens, 10);
        assert_eq!(claude_response.usage.output_tokens, 5);
    }

    #[test]
    fn test_map_finish_reason() {
        assert_eq!(map_finish_reason("stop"), "end_turn");
        assert_eq!(map_finish_reason("length"), "max_tokens");
        assert_eq!(map_finish_reason("tool_calls"), "tool_use");
        assert_eq!(map_finish_reason("unknown"), "end_turn");
    }

    #[test]
    fn test_format_sse_event() {
        let event = format_sse_event("ping", &json!({"type": "ping"}));
        assert!(event.starts_with("event: ping\n"));
        assert!(event.contains("data: "));
        assert!(event.ends_with("\n\n"));
    }

    #[test]
    fn test_extract_system_text_string() {
        let system = ClaudeSystemPrompt::Text("Hello".to_string());
        assert_eq!(extract_system_text(&system), "Hello");
    }

    #[test]
    fn test_convert_tool_choice() {
        assert_eq!(convert_tool_choice(&json!({"type": "auto"})), json!("auto"));
        assert_eq!(convert_tool_choice(&json!({"type": "any"})), json!("auto"));

        let tool_choice = convert_tool_choice(&json!({"type": "tool", "name": "my_tool"}));
        assert_eq!(tool_choice["type"], "function");
        assert_eq!(tool_choice["function"]["name"], "my_tool");
    }
}