//! Integration tests for Claude API endpoints.
//!
//! These tests verify the Claude Messages API compatibility layer,
//! including request/response conversion and streaming support.

use llm_proxy_rust::api::claude_models::{
    constants, ClaudeContentBlock, ClaudeContentBlockToolResult, ClaudeMessage,
    ClaudeMessageContent, ClaudeMessagesRequest, ClaudeResponse, ClaudeSystemPrompt, ClaudeTool,
    ClaudeUsage,
};
use llm_proxy_rust::services::{claude_to_openai_request, openai_to_claude_response};
use serde_json::json;
use std::collections::HashMap;

// ============================================================================
// Request Conversion Tests
// ============================================================================

#[test]
fn test_claude_to_openai_basic_request() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeMessageContent::Text("Hello, how are you?".to_string()),
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

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    assert_eq!(openai_request["model"], "claude-3-opus-20240229");
    assert_eq!(openai_request["max_tokens"], 1024);
    assert_eq!(openai_request["temperature"], 0.7);
    assert_eq!(openai_request["stream"], false);

    let messages = openai_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "Hello, how are you?");
}

#[test]
fn test_claude_to_openai_with_system_prompt() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeMessageContent::Text("Hello!".to_string()),
        }],
        system: Some(ClaudeSystemPrompt::Text(
            "You are a helpful assistant.".to_string(),
        )),
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

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    let messages = openai_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "You are a helpful assistant.");
    assert_eq!(messages[1]["role"], "user");
}

#[test]
fn test_claude_to_openai_with_model_mapping() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus".to_string(),
        max_tokens: 1024,
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

    let mut model_mapping = HashMap::new();
    model_mapping.insert(
        "claude-3-opus".to_string(),
        "gpt-4-turbo-preview".to_string(),
    );

    let openai_request = claude_to_openai_request(&request, Some(&model_mapping), 1, 8192);

    assert_eq!(openai_request["model"], "gpt-4-turbo-preview");
}

#[test]
fn test_claude_to_openai_with_tools() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeMessageContent::Text("What's the weather?".to_string()),
        }],
        system: None,
        stop_sequences: None,
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
        tools: Some(vec![ClaudeTool {
            name: "get_weather".to_string(),
            description: Some("Get the current weather".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
        }]),
        tool_choice: None,
        thinking: None,
    };

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    let tools = openai_request["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["function"]["name"], "get_weather");
}

#[test]
fn test_claude_to_openai_with_stop_sequences() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeMessageContent::Text("Hello!".to_string()),
        }],
        system: None,
        stop_sequences: Some(vec!["STOP".to_string(), "END".to_string()]),
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
        tools: None,
        tool_choice: None,
        thinking: None,
    };

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    let stop = openai_request["stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(stop[0], "STOP");
    assert_eq!(stop[1], "END");
}

// ============================================================================
// Response Conversion Tests
// ============================================================================

#[test]
fn test_openai_to_claude_basic_response() {
    let openai_response = json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1677858242,
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello! How can I help you today?"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 8,
            "total_tokens": 18
        }
    });

    let claude_response =
        openai_to_claude_response(&openai_response, "claude-3-opus-20240229").unwrap();

    assert_eq!(claude_response.model, "claude-3-opus-20240229");
    assert_eq!(claude_response.role, constants::ROLE_ASSISTANT);
    assert_eq!(claude_response.response_type, "message");
    assert_eq!(claude_response.stop_reason, Some("end_turn".to_string()));
    assert_eq!(claude_response.usage.input_tokens, 10);
    assert_eq!(claude_response.usage.output_tokens, 8);
    assert_eq!(claude_response.content.len(), 1);
}

#[test]
fn test_openai_to_claude_with_tool_calls() {
    let openai_response = json!({
        "id": "chatcmpl-123",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_123",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\": \"San Francisco\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 15,
            "total_tokens": 25
        }
    });

    let claude_response =
        openai_to_claude_response(&openai_response, "claude-3-opus-20240229").unwrap();

    assert_eq!(claude_response.stop_reason, Some("tool_use".to_string()));
    assert!(claude_response.content.len() >= 1);

    // Check for tool use content block
    let has_tool_use = claude_response.content.iter().any(|block| {
        matches!(block, ClaudeContentBlock::ToolUse(_))
    });
    assert!(has_tool_use);
}

#[test]
fn test_openai_to_claude_finish_reason_mapping() {
    // Test "stop" -> "end_turn"
    let response_stop = json!({
        "id": "chatcmpl-123",
        "choices": [{
            "message": {"role": "assistant", "content": "Done"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 1, "total_tokens": 6}
    });
    let claude_stop = openai_to_claude_response(&response_stop, "claude-3").unwrap();
    assert_eq!(claude_stop.stop_reason, Some("end_turn".to_string()));

    // Test "length" -> "max_tokens"
    let response_length = json!({
        "id": "chatcmpl-123",
        "choices": [{
            "message": {"role": "assistant", "content": "Truncated..."},
            "finish_reason": "length"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 100, "total_tokens": 105}
    });
    let claude_length = openai_to_claude_response(&response_length, "claude-3").unwrap();
    assert_eq!(claude_length.stop_reason, Some("max_tokens".to_string()));

    // Test "tool_calls" -> "tool_use"
    let response_tool = json!({
        "id": "chatcmpl-123",
        "choices": [{
            "message": {"role": "assistant", "content": null},
            "finish_reason": "tool_calls"
        }],
        "usage": {"prompt_tokens": 5, "completion_tokens": 10, "total_tokens": 15}
    });
    let claude_tool = openai_to_claude_response(&response_tool, "claude-3").unwrap();
    assert_eq!(claude_tool.stop_reason, Some("tool_use".to_string()));
}

#[test]
fn test_openai_to_claude_error_no_choices() {
    let openai_response = json!({
        "id": "chatcmpl-123",
        "choices": [],
        "usage": {"prompt_tokens": 5, "completion_tokens": 0, "total_tokens": 5}
    });

    let result = openai_to_claude_response(&openai_response, "claude-3");
    assert!(result.is_err());
}

// ============================================================================
// Model Tests
// ============================================================================

#[test]
fn test_claude_response_creation() {
    let response = ClaudeResponse::new(
        "msg_123",
        "claude-3-opus-20240229",
        vec![ClaudeContentBlock::text("Hello!")],
        Some("end_turn".to_string()),
        ClaudeUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
    );

    assert_eq!(response.id, "msg_123");
    assert_eq!(response.model, "claude-3-opus-20240229");
    assert_eq!(response.role, "assistant");
    assert_eq!(response.response_type, "message");
    assert_eq!(response.stop_reason, Some("end_turn".to_string()));
}

#[test]
fn test_claude_content_block_helpers() {
    let text_block = ClaudeContentBlock::text("Hello");
    assert_eq!(text_block.get_type(), "text");

    let tool_block = ClaudeContentBlock::tool_use("tool_1", "my_tool", json!({"key": "value"}));
    assert_eq!(tool_block.get_type(), "tool_use");
}

#[test]
fn test_claude_message_content_serialization() {
    // Test text content
    let text_content = ClaudeMessageContent::Text("Hello".to_string());
    let json = serde_json::to_string(&text_content).unwrap();
    assert_eq!(json, "\"Hello\"");

    // Test blocks content
    let blocks_content =
        ClaudeMessageContent::Blocks(vec![ClaudeContentBlock::text("Hello")]);
    let json = serde_json::to_string(&blocks_content).unwrap();
    assert!(json.contains("\"type\":\"text\""));
}

#[test]
fn test_claude_usage_default() {
    let usage = ClaudeUsage::default();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert!(usage.cache_creation_input_tokens.is_none());
    assert!(usage.cache_read_input_tokens.is_none());
}

// ============================================================================
// Constants Tests
// ============================================================================

#[test]
fn test_constants_values() {
    assert_eq!(constants::ROLE_USER, "user");
    assert_eq!(constants::ROLE_ASSISTANT, "assistant");
    assert_eq!(constants::ROLE_SYSTEM, "system");
    assert_eq!(constants::ROLE_TOOL, "tool");

    assert_eq!(constants::CONTENT_TEXT, "text");
    assert_eq!(constants::CONTENT_IMAGE, "image");
    assert_eq!(constants::CONTENT_TOOL_USE, "tool_use");
    assert_eq!(constants::CONTENT_TOOL_RESULT, "tool_result");

    assert_eq!(constants::STOP_END_TURN, "end_turn");
    assert_eq!(constants::STOP_MAX_TOKENS, "max_tokens");
    assert_eq!(constants::STOP_TOOL_USE, "tool_use");

    assert_eq!(constants::EVENT_MESSAGE_START, "message_start");
    assert_eq!(constants::EVENT_MESSAGE_STOP, "message_stop");
    assert_eq!(constants::EVENT_CONTENT_BLOCK_START, "content_block_start");
    assert_eq!(constants::EVENT_CONTENT_BLOCK_DELTA, "content_block_delta");
}

// ============================================================================
// Multi-turn Conversation Tests
// ============================================================================

#[test]
fn test_claude_to_openai_multi_turn_conversation() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Text("Hello!".to_string()),
            },
            ClaudeMessage {
                role: "assistant".to_string(),
                content: ClaudeMessageContent::Text("Hi there! How can I help?".to_string()),
            },
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Text("What's 2+2?".to_string()),
            },
        ],
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

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    let messages = openai_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[2]["role"], "user");
}

// ============================================================================
// Streaming Tests
// ============================================================================

#[test]
fn test_claude_streaming_request() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeMessageContent::Text("Hello!".to_string()),
        }],
        system: None,
        stop_sequences: None,
        stream: true,
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
        tools: None,
        tool_choice: None,
        thinking: None,
    };

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    assert_eq!(openai_request["stream"], true);
}

// ============================================================================
// Langfuse Input Capture Tests
// ============================================================================

#[test]
fn test_langfuse_input_messages_capture_basic() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeMessageContent::Text("Hello, how are you?".to_string()),
        }],
        system: None,
        stop_sequences: None,
        stream: false,
        temperature: Some(0.7),
        top_p: Some(0.9),
        top_k: None,
        metadata: None,
        tools: None,
        tool_choice: None,
        thinking: None,
    };

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    // Simulate what claude.rs does for Langfuse tracing
    let input_messages: Vec<serde_json::Value> = openai_request
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| arr.to_vec())
        .unwrap_or_default();

    assert_eq!(input_messages.len(), 1);
    assert_eq!(input_messages[0]["role"], "user");
    assert_eq!(input_messages[0]["content"], "Hello, how are you?");
}

#[test]
fn test_langfuse_input_messages_capture_with_system() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeMessageContent::Text("Hello!".to_string()),
        }],
        system: Some(ClaudeSystemPrompt::Text(
            "You are a helpful assistant.".to_string(),
        )),
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

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    let input_messages: Vec<serde_json::Value> = openai_request
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| arr.to_vec())
        .unwrap_or_default();

    // System message should be first
    assert_eq!(input_messages.len(), 2);
    assert_eq!(input_messages[0]["role"], "system");
    assert_eq!(input_messages[0]["content"], "You are a helpful assistant.");
    assert_eq!(input_messages[1]["role"], "user");
    assert_eq!(input_messages[1]["content"], "Hello!");
}

#[test]
fn test_langfuse_input_messages_capture_multi_turn() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Text("Hello!".to_string()),
            },
            ClaudeMessage {
                role: "assistant".to_string(),
                content: ClaudeMessageContent::Text("Hi there! How can I help?".to_string()),
            },
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Text("What's 2+2?".to_string()),
            },
        ],
        system: Some(ClaudeSystemPrompt::Text("Be concise.".to_string())),
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

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    let input_messages: Vec<serde_json::Value> = openai_request
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| arr.to_vec())
        .unwrap_or_default();

    assert_eq!(input_messages.len(), 4);
    assert_eq!(input_messages[0]["role"], "system");
    assert_eq!(input_messages[1]["role"], "user");
    assert_eq!(input_messages[2]["role"], "assistant");
    assert_eq!(input_messages[3]["role"], "user");
    assert_eq!(input_messages[3]["content"], "What's 2+2?");
}

#[test]
fn test_langfuse_model_parameters_capture() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 2048,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeMessageContent::Text("Hello!".to_string()),
        }],
        system: None,
        stop_sequences: Some(vec!["STOP".to_string(), "END".to_string()]),
        stream: false,
        temperature: Some(0.8),
        top_p: Some(0.95),
        top_k: None,
        metadata: None,
        tools: None,
        tool_choice: None,
        thinking: None,
    };

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    // Simulate what claude.rs does for Langfuse model parameters capture
    let mut model_parameters: HashMap<String, serde_json::Value> = HashMap::new();
    let param_keys = ["temperature", "max_tokens", "top_p", "stop"];
    for key in param_keys {
        if let Some(val) = openai_request.get(key) {
            model_parameters.insert(key.to_string(), val.clone());
        }
    }

    assert_eq!(model_parameters.get("temperature"), Some(&json!(0.8)));
    assert_eq!(model_parameters.get("max_tokens"), Some(&json!(2048)));
    assert_eq!(model_parameters.get("top_p"), Some(&json!(0.95)));
    assert_eq!(
        model_parameters.get("stop"),
        Some(&json!(["STOP", "END"]))
    );
}

#[test]
fn test_langfuse_model_parameters_partial() {
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
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
        tools: None,
        tool_choice: None,
        thinking: None,
    };

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    let mut model_parameters: HashMap<String, serde_json::Value> = HashMap::new();
    let param_keys = ["temperature", "max_tokens", "top_p", "stop"];
    for key in param_keys {
        if let Some(val) = openai_request.get(key) {
            model_parameters.insert(key.to_string(), val.clone());
        }
    }

    // Only max_tokens should be present
    assert_eq!(model_parameters.get("max_tokens"), Some(&json!(1024)));
    assert!(model_parameters.get("temperature").is_none());
    assert!(model_parameters.get("top_p").is_none());
    assert!(model_parameters.get("stop").is_none());
}

#[test]
fn test_langfuse_input_messages_with_content_blocks() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![ClaudeMessage {
            role: "user".to_string(),
            content: ClaudeMessageContent::Blocks(vec![ClaudeContentBlock::text(
                "Hello from blocks!",
            )]),
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

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    let input_messages: Vec<serde_json::Value> = openai_request
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| arr.to_vec())
        .unwrap_or_default();

    assert_eq!(input_messages.len(), 1);
    assert_eq!(input_messages[0]["role"], "user");
    // Content could be string or array depending on conversion
    let content = &input_messages[0]["content"];
    assert!(
        content.is_string() || content.is_array(),
        "content should be string or array"
    );
}

#[test]
fn test_langfuse_input_messages_with_tool_result() {
    let request = ClaudeMessagesRequest {
        model: "claude-3-opus-20240229".to_string(),
        max_tokens: 1024,
        messages: vec![
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Text("What's the weather?".to_string()),
            },
            ClaudeMessage {
                role: "assistant".to_string(),
                content: ClaudeMessageContent::Blocks(vec![ClaudeContentBlock::tool_use(
                    "call_123",
                    "get_weather",
                    json!({"location": "San Francisco"}),
                )]),
            },
            ClaudeMessage {
                role: "user".to_string(),
                content: ClaudeMessageContent::Blocks(vec![ClaudeContentBlock::ToolResult(
                    ClaudeContentBlockToolResult {
                        content_type: "tool_result".to_string(),
                        tool_use_id: "call_123".to_string(),
                        content: json!("Sunny, 72°F"),
                        is_error: None,
                    },
                )]),
            },
        ],
        system: None,
        stop_sequences: None,
        stream: false,
        temperature: None,
        top_p: None,
        top_k: None,
        metadata: None,
        tools: Some(vec![ClaudeTool {
            name: "get_weather".to_string(),
            description: Some("Get weather information".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                }
            }),
        }]),
        tool_choice: None,
        thinking: None,
    };

    let openai_request = claude_to_openai_request(&request, None, 1, 8192);

    let input_messages: Vec<serde_json::Value> = openai_request
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| arr.to_vec())
        .unwrap_or_default();

    // Should have user, assistant with tool_calls, and tool response
    assert!(input_messages.len() >= 3, "Should have at least 3 messages");
    assert_eq!(input_messages[0]["role"], "user");
}