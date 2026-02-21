//! Cross-protocol integration tests for the transformer pipeline.
//!
//! These tests verify that the transformer pipeline correctly converts
//! between different LLM API protocols (OpenAI, Anthropic, Response API).
//!
//! Test scenarios:
//! - OpenAI request → Anthropic provider → OpenAI response
//! - Anthropic request → OpenAI provider → Anthropic response
//! - Response API request → OpenAI provider → Response API response
//! - Response API request → Anthropic provider → Response API response
//! - Multi-turn conversations across protocols
//! - Tool/function calls across protocols
//! - Edge cases (empty messages, special characters, large content)

use llm_proxy_rust::transformer::{
    anthropic::AnthropicTransformer, openai::OpenAITransformer,
    response_api::ResponseApiTransformer, Protocol, Role, TransformContext, TransformPipeline,
    Transformer, TransformerRegistry, UnifiedMessage, UnifiedRequest, UnifiedResponse, UnifiedTool,
    UnifiedUsage,
};
use serde_json::json;
use std::sync::Arc;

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a transformer registry with all protocols
fn create_registry() -> Arc<TransformerRegistry> {
    Arc::new(TransformerRegistry::new())
}

/// Create a transform context for cross-protocol testing
fn create_context(
    client_protocol: Protocol,
    provider_protocol: Protocol,
    model: &str,
) -> TransformContext {
    let mut ctx = TransformContext::new("test-request-id");
    ctx.client_protocol = client_protocol;
    ctx.provider_protocol = provider_protocol;
    ctx.original_model = model.to_string();
    ctx.mapped_model = model.to_string();
    ctx
}

// ============================================================================
// OpenAI → Anthropic → OpenAI Tests
// ============================================================================

#[test]
fn test_openai_to_anthropic_simple_message() {
    // Test: OpenAI client sends request, Anthropic provider responds
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    // OpenAI format request
    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [
            {"role": "user", "content": "Hello, how are you?"}
        ]
    });

    // Transform request: OpenAI → Anthropic
    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // Verify Anthropic format
    assert_eq!(anthropic_request["model"], "claude-3-opus");
    assert!(anthropic_request["max_tokens"].is_number());
    assert!(anthropic_request["messages"].is_array());

    let messages = anthropic_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");

    // Simulate Anthropic response
    let anthropic_response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "I'm doing well, thank you!"}],
        "model": "claude-3-opus",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 8
        }
    });

    // Transform response: Anthropic → OpenAI
    let openai_response = pipeline
        .transform_response(anthropic_response, &ctx)
        .unwrap();

    // Verify OpenAI format
    assert_eq!(openai_response["object"], "chat.completion");
    assert!(openai_response["choices"].is_array());

    let choices = openai_response["choices"].as_array().unwrap();
    assert_eq!(choices.len(), 1);
    assert_eq!(choices[0]["message"]["role"], "assistant");
    assert_eq!(
        choices[0]["message"]["content"],
        "I'm doing well, thank you!"
    );
    assert_eq!(choices[0]["finish_reason"], "stop");
}

#[test]
fn test_openai_to_anthropic_with_system_message() {
    // Test: OpenAI request with system message → Anthropic format
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "What is 2+2?"}
        ]
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // System message should be extracted to separate field
    assert!(anthropic_request["system"].is_string());
    assert_eq!(anthropic_request["system"], "You are a helpful assistant.");

    // Messages should not include system message
    let messages = anthropic_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

#[test]
fn test_openai_to_anthropic_multi_turn() {
    // Test: Multi-turn conversation from OpenAI to Anthropic
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hi there!"},
            {"role": "user", "content": "How are you?"}
        ]
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    let messages = anthropic_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[2]["role"], "user");
}

// ============================================================================
// Anthropic → OpenAI → Anthropic Tests
// ============================================================================

#[test]
fn test_anthropic_to_openai_simple_message() {
    // Test: Anthropic client sends request, OpenAI provider responds
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::Anthropic, Protocol::OpenAI, "gpt-4");

    // Anthropic format request
    let anthropic_request = json!({
        "model": "gpt-4",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "What is the capital of France?"}
        ]
    });

    // Transform request: Anthropic → OpenAI
    let openai_request = pipeline.transform_request(anthropic_request, &ctx).unwrap();

    // Verify OpenAI format
    assert_eq!(openai_request["model"], "gpt-4");
    assert!(openai_request["messages"].is_array());
    assert_eq!(openai_request["max_tokens"], 1024);

    // Simulate OpenAI response
    let openai_response = json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1677652288,
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "The capital of France is Paris."
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 15,
            "completion_tokens": 10,
            "total_tokens": 25
        }
    });

    // Transform response: OpenAI → Anthropic
    let anthropic_response = pipeline.transform_response(openai_response, &ctx).unwrap();

    // Verify Anthropic format
    assert_eq!(anthropic_response["type"], "message");
    assert_eq!(anthropic_response["role"], "assistant");
    assert!(anthropic_response["content"].is_array());

    let content = anthropic_response["content"].as_array().unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "The capital of France is Paris.");
    assert_eq!(anthropic_response["stop_reason"], "end_turn");
}

#[test]
fn test_anthropic_to_openai_with_system() {
    // Test: Anthropic request with system field → OpenAI format
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::Anthropic, Protocol::OpenAI, "gpt-4");

    let anthropic_request = json!({
        "model": "gpt-4",
        "max_tokens": 1024,
        "system": "You are a geography expert.",
        "messages": [
            {"role": "user", "content": "What is the capital of Japan?"}
        ]
    });

    let openai_request = pipeline.transform_request(anthropic_request, &ctx).unwrap();

    // System should be converted to system message
    let messages = openai_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "You are a geography expert.");
    assert_eq!(messages[1]["role"], "user");
}

#[test]
fn test_anthropic_to_openai_content_blocks() {
    // Test: Anthropic content blocks → OpenAI format
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::Anthropic, Protocol::OpenAI, "gpt-4");

    let anthropic_request = json!({
        "model": "gpt-4",
        "max_tokens": 1024,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "Describe this image:"},
                    {"type": "text", "text": "It shows a sunset."}
                ]
            }
        ]
    });

    let openai_request = pipeline.transform_request(anthropic_request, &ctx).unwrap();

    let messages = openai_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    // Content should be converted to OpenAI format
    assert!(messages[0]["content"].is_array() || messages[0]["content"].is_string());
}

// ============================================================================
// Response API → OpenAI → Response API Tests
// ============================================================================

#[test]
fn test_response_api_to_openai_simple() {
    // Test: Response API client sends request, OpenAI provider responds
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::ResponseApi, Protocol::OpenAI, "gpt-4");

    // Response API format request
    let response_api_request = json!({
        "model": "gpt-4",
        "input": "What is machine learning?"
    });

    // Transform request: Response API → OpenAI
    let openai_request = pipeline
        .transform_request(response_api_request, &ctx)
        .unwrap();

    // Verify OpenAI format
    assert_eq!(openai_request["model"], "gpt-4");
    assert!(openai_request["messages"].is_array());

    let messages = openai_request["messages"].as_array().unwrap();
    assert!(!messages.is_empty());

    // Simulate OpenAI response
    let openai_response = json!({
        "id": "chatcmpl-456",
        "object": "chat.completion",
        "created": 1677652288,
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Machine learning is a subset of AI..."
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 20,
            "total_tokens": 30
        }
    });

    // Transform response: OpenAI → Response API
    let response_api_response = pipeline.transform_response(openai_response, &ctx).unwrap();

    // Verify Response API format
    assert_eq!(response_api_response["object"], "response");
    assert!(response_api_response["output"].is_array());
    assert_eq!(response_api_response["status"], "completed");
}

#[test]
fn test_response_api_to_openai_with_instructions() {
    // Test: Response API with instructions → OpenAI format
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::ResponseApi, Protocol::OpenAI, "gpt-4");

    let response_api_request = json!({
        "model": "gpt-4",
        "instructions": "You are a helpful coding assistant.",
        "input": "Write a hello world in Python"
    });

    let openai_request = pipeline
        .transform_request(response_api_request, &ctx)
        .unwrap();

    // Instructions should become system message
    let messages = openai_request["messages"].as_array().unwrap();
    assert!(!messages.is_empty());

    // Check for system message
    let has_system = messages.iter().any(|m| m["role"] == "system");
    assert!(has_system, "Should have system message from instructions");
}

// ============================================================================
// Response API → Anthropic → Response API Tests
// ============================================================================

#[test]
fn test_response_api_to_anthropic_simple() {
    // Test: Response API client sends request, Anthropic provider responds
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::ResponseApi, Protocol::Anthropic, "claude-3-opus");

    // Response API format request
    let response_api_request = json!({
        "model": "claude-3-opus",
        "input": "Explain quantum computing"
    });

    // Transform request: Response API → Anthropic
    let anthropic_request = pipeline
        .transform_request(response_api_request, &ctx)
        .unwrap();

    // Verify Anthropic format
    assert_eq!(anthropic_request["model"], "claude-3-opus");
    assert!(anthropic_request["max_tokens"].is_number());
    assert!(anthropic_request["messages"].is_array());

    // Simulate Anthropic response
    let anthropic_response = json!({
        "id": "msg_789",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Quantum computing uses quantum mechanics..."}],
        "model": "claude-3-opus",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 8,
            "output_tokens": 15
        }
    });

    // Transform response: Anthropic → Response API
    let response_api_response = pipeline
        .transform_response(anthropic_response, &ctx)
        .unwrap();

    // Verify Response API format
    assert_eq!(response_api_response["object"], "response");
    assert!(response_api_response["output"].is_array());
}

// ============================================================================
// Tool/Function Call Tests
// ============================================================================

#[test]
fn test_openai_to_anthropic_with_tools() {
    // Test: OpenAI request with tools → Anthropic format
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [
            {"role": "user", "content": "What's the weather in Tokyo?"}
        ],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather for a location",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    },
                    "required": ["location"]
                }
            }
        }]
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // Verify tools are converted
    assert!(anthropic_request["tools"].is_array());
    let tools = anthropic_request["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "get_weather");
    assert!(tools[0]["input_schema"].is_object());
}

#[test]
fn test_anthropic_to_openai_with_tools() {
    // Test: Anthropic request with tools → OpenAI format
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::Anthropic, Protocol::OpenAI, "gpt-4");

    let anthropic_request = json!({
        "model": "gpt-4",
        "max_tokens": 1024,
        "messages": [
            {"role": "user", "content": "Search for restaurants nearby"}
        ],
        "tools": [{
            "name": "search_places",
            "description": "Search for places",
            "input_schema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                }
            }
        }]
    });

    let openai_request = pipeline.transform_request(anthropic_request, &ctx).unwrap();

    // Verify tools are converted to OpenAI format
    assert!(openai_request["tools"].is_array());
    let tools = openai_request["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["function"]["name"], "search_places");
}

#[test]
fn test_tool_call_response_openai_to_anthropic() {
    // Test: OpenAI tool call response → Anthropic format
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    // Simulate Anthropic response with tool use
    let anthropic_response = json!({
        "id": "msg_tool",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "tool_use",
            "id": "tool_123",
            "name": "get_weather",
            "input": {"location": "Tokyo"}
        }],
        "model": "claude-3-opus",
        "stop_reason": "tool_use",
        "usage": {
            "input_tokens": 20,
            "output_tokens": 15
        }
    });

    let openai_response = pipeline
        .transform_response(anthropic_response, &ctx)
        .unwrap();

    // Verify OpenAI format with tool calls
    assert!(openai_response["choices"].is_array());
    let choice = &openai_response["choices"][0];
    assert_eq!(choice["finish_reason"], "tool_calls");
    // Tool calls should be present
    assert!(choice["message"]["tool_calls"].is_array());
}

// ============================================================================
// Edge Case Tests
// ============================================================================

#[test]
fn test_empty_content_handling() {
    // Test: Handle messages with empty content
    let openai = OpenAITransformer::new();

    // OpenAI request with empty content
    let request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": ""}
        ]
    });

    let unified = openai.transform_request_out(request).unwrap();
    assert!(
        unified.messages[0].content.is_empty() || unified.messages[0].text_content().is_empty()
    );
}

#[test]
fn test_special_characters_handling() {
    // Test: Handle special characters in content
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let special_content =
        "Hello! 你好! مرحبا! 🎉 <script>alert('test')</script> \"quotes\" 'apostrophe'";

    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [
            {"role": "user", "content": special_content}
        ]
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // Content should be preserved
    let messages = anthropic_request["messages"].as_array().unwrap();
    let content = &messages[0]["content"];
    if content.is_string() {
        assert_eq!(content.as_str().unwrap(), special_content);
    }
}

#[test]
fn test_large_content_handling() {
    // Test: Handle large content blocks
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    // Create large content (10KB)
    let large_content = "x".repeat(10000);

    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [
            {"role": "user", "content": large_content}
        ]
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // Should handle large content without error
    assert!(anthropic_request["messages"].is_array());
}

#[test]
fn test_missing_optional_fields() {
    // Test: Handle requests with missing optional fields
    let openai = OpenAITransformer::new();

    // Minimal OpenAI request
    let request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Hello"}
        ]
    });

    let unified = openai.transform_request_out(request).unwrap();

    // Should have defaults for optional fields
    assert!(unified.parameters.temperature.is_none());
    assert!(unified.parameters.max_tokens.is_none());
    assert!(!unified.parameters.stream);
}

#[test]
fn test_null_content_handling() {
    // Test: Handle null content in messages
    let openai = OpenAITransformer::new();

    // OpenAI message with null content (tool call response)
    let request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Hello"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_123",
                    "type": "function",
                    "function": {
                        "name": "test_func",
                        "arguments": "{}"
                    }
                }]
            }
        ]
    });

    let unified = openai.transform_request_out(request).unwrap();
    assert_eq!(unified.messages.len(), 2);
}

// ============================================================================
// Streaming Tests
// ============================================================================

#[test]
fn test_openai_stream_chunk_transformation() {
    // Test: OpenAI streaming chunk transformation
    let openai = OpenAITransformer::new();

    let chunk_data = b"data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n";

    let chunks = openai
        .transform_stream_chunk_in(&bytes::Bytes::from_static(chunk_data))
        .unwrap();

    assert!(!chunks.is_empty());
    // Should have content delta
    let has_delta = chunks.iter().any(|c| {
        matches!(
            c.chunk_type,
            llm_proxy_rust::transformer::ChunkType::ContentBlockDelta
        )
    });
    assert!(has_delta);
}

#[test]
fn test_anthropic_stream_chunk_transformation() {
    // Test: Anthropic streaming chunk transformation
    let anthropic = AnthropicTransformer::new();

    let chunk_data = b"data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n";

    let chunks = anthropic
        .transform_stream_chunk_in(&bytes::Bytes::from_static(chunk_data))
        .unwrap();

    assert!(!chunks.is_empty());
}

#[test]
fn test_stream_done_handling() {
    // Test: Handle [DONE] marker in streams
    let openai = OpenAITransformer::new();

    let chunk_data = b"data: [DONE]\n\n";

    let chunks = openai
        .transform_stream_chunk_in(&bytes::Bytes::from_static(chunk_data))
        .unwrap();

    assert!(!chunks.is_empty());
    let has_stop = chunks.iter().any(|c| {
        matches!(
            c.chunk_type,
            llm_proxy_rust::transformer::ChunkType::MessageStop
        )
    });
    assert!(has_stop);
}

// ============================================================================
// Protocol Detection Tests
// ============================================================================

#[test]
fn test_protocol_detection_openai() {
    let openai = OpenAITransformer::new();

    // OpenAI format
    let request = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    assert!(openai.can_handle(&request));

    // Anthropic format (should not handle)
    let request = json!({
        "model": "claude-3",
        "system": "Be helpful",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    assert!(!openai.can_handle(&request));
}

#[test]
fn test_protocol_detection_anthropic() {
    let anthropic = AnthropicTransformer::new();

    // Anthropic format with system
    let request = json!({
        "model": "claude-3",
        "max_tokens": 1024,
        "system": "Be helpful",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    assert!(anthropic.can_handle(&request));

    // OpenAI format (should not handle)
    let request = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    assert!(!anthropic.can_handle(&request));
}

#[test]
fn test_protocol_detection_response_api() {
    let response_api = ResponseApiTransformer::new();

    // Response API format with input
    let request = json!({
        "model": "gpt-4",
        "input": "Hello"
    });
    assert!(response_api.can_handle(&request));

    // Response API format with instructions
    let request = json!({
        "model": "gpt-4",
        "instructions": "Be helpful"
    });
    assert!(response_api.can_handle(&request));

    // OpenAI format (should not handle)
    let request = json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });
    assert!(!response_api.can_handle(&request));
}

// ============================================================================
// Unified Format Tests
// ============================================================================

#[test]
fn test_unified_message_creation() {
    let msg = UnifiedMessage::user("Hello, world!");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.text_content(), "Hello, world!");
}

#[test]
fn test_unified_request_builder() {
    let request = UnifiedRequest::new("gpt-4", vec![UnifiedMessage::user("Hello")])
        .with_system("You are helpful")
        .with_stream(true)
        .with_max_tokens(100);

    assert_eq!(request.model, "gpt-4");
    assert_eq!(request.system, Some("You are helpful".to_string()));
    assert!(request.is_streaming());
    assert_eq!(request.parameters.max_tokens, Some(100));
}

#[test]
fn test_unified_response_text() {
    let response = UnifiedResponse::text("resp_123", "gpt-4", "Hello!", UnifiedUsage::new(10, 5));

    assert_eq!(response.id, "resp_123");
    assert_eq!(response.model, "gpt-4");
    assert_eq!(response.text_content(), "Hello!");
    assert_eq!(response.usage.total_tokens(), 15);
}

#[test]
fn test_unified_tool_creation() {
    let tool = UnifiedTool::function(
        "search",
        Some("Search the web".to_string()),
        json!({"type": "object", "properties": {"query": {"type": "string"}}}),
    );

    assert_eq!(tool.name, "search");
    assert_eq!(tool.description, Some("Search the web".to_string()));
    assert_eq!(tool.tool_type, Some("function".to_string()));
}

// ============================================================================
// Stop Reason Conversion Tests
// ============================================================================

#[test]
fn test_stop_reason_openai_to_anthropic() {
    // OpenAI "stop" → Anthropic "end_turn"
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);

    // For OpenAI client, Anthropic provider response
    let ctx_reverse = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");
    let anthropic_response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Done"}],
        "model": "claude-3-opus",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });

    let result = pipeline
        .transform_response(anthropic_response, &ctx_reverse)
        .unwrap();
    assert_eq!(result["choices"][0]["finish_reason"], "stop");
}

#[test]
fn test_stop_reason_max_tokens() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let anthropic_response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Truncated..."}],
        "model": "claude-3-opus",
        "stop_reason": "max_tokens",
        "usage": {"input_tokens": 10, "output_tokens": 100}
    });

    let result = pipeline
        .transform_response(anthropic_response, &ctx)
        .unwrap();
    assert_eq!(result["choices"][0]["finish_reason"], "length");
}

// ============================================================================
// Usage Statistics Tests
// ============================================================================

#[test]
fn test_usage_conversion_openai_to_anthropic() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let anthropic_response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello"}],
        "model": "claude-3-opus",
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50
        }
    });

    let result = pipeline
        .transform_response(anthropic_response, &ctx)
        .unwrap();

    assert_eq!(result["usage"]["prompt_tokens"], 100);
    assert_eq!(result["usage"]["completion_tokens"], 50);
    assert_eq!(result["usage"]["total_tokens"], 150);
}

#[test]
fn test_usage_conversion_anthropic_to_openai() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::Anthropic, Protocol::OpenAI, "gpt-4");

    let openai_response = json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1677652288,
        "model": "gpt-4",
        "choices": [{
            "index": 0,
            "message": {"role": "assistant", "content": "Hello"},
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        }
    });

    let result = pipeline.transform_response(openai_response, &ctx).unwrap();

    assert_eq!(result["usage"]["input_tokens"], 100);
    assert_eq!(result["usage"]["output_tokens"], 50);
}

// ============================================================================
// Model Name Preservation Tests
// ============================================================================

#[test]
fn test_model_name_preserved_in_response() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);

    let mut ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "my-custom-model");
    ctx.original_model = "my-custom-model".to_string();
    ctx.mapped_model = "claude-3-opus".to_string();

    let anthropic_response = json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello"}],
        "model": "claude-3-opus",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });

    let result = pipeline
        .transform_response(anthropic_response, &ctx)
        .unwrap();

    // Model name should be restored to original
    assert_eq!(result["model"], "my-custom-model");
}

// ============================================================================
// Same Protocol Bypass Tests
// ============================================================================

#[test]
fn test_same_protocol_context() {
    let ctx = create_context(Protocol::OpenAI, Protocol::OpenAI, "gpt-4");
    assert!(ctx.is_same_protocol());

    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3");
    assert!(!ctx.is_same_protocol());
}

#[test]
fn test_openai_to_openai_passthrough() {
    // When client and provider use same protocol, transformation should still work
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::OpenAI, "gpt-4");

    let openai_request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "user", "content": "Hello"}
        ],
        "temperature": 0.7
    });

    let result = pipeline
        .transform_request(openai_request.clone(), &ctx)
        .unwrap();

    // Should preserve the format
    assert_eq!(result["model"], "gpt-4");
    assert!(result["messages"].is_array());
    assert_eq!(result["temperature"], 0.7);
}

// ============================================================================
// Complex Multi-Protocol Scenarios
// ============================================================================

#[test]
fn test_complex_conversation_openai_to_anthropic() {
    // Test: Complex multi-turn conversation with mixed content
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [
            {"role": "system", "content": "You are a helpful coding assistant."},
            {"role": "user", "content": "Write a Python function to calculate factorial"},
            {"role": "assistant", "content": "Here's a factorial function:\n```python\ndef factorial(n):\n    if n <= 1:\n        return 1\n    return n * factorial(n-1)\n```"},
            {"role": "user", "content": "Now make it iterative"}
        ],
        "temperature": 0.5,
        "max_tokens": 500
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // Verify system is extracted
    assert_eq!(
        anthropic_request["system"],
        "You are a helpful coding assistant."
    );

    // Verify messages (excluding system)
    let messages = anthropic_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 3);

    // Verify parameters
    assert_eq!(anthropic_request["temperature"], 0.5);
    assert_eq!(anthropic_request["max_tokens"], 500);
}

#[test]
fn test_response_api_complex_input() {
    // Test: Response API with complex input items
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::ResponseApi, Protocol::OpenAI, "gpt-4");

    let response_api_request = json!({
        "model": "gpt-4",
        "instructions": "You are a helpful assistant.",
        "input": [
            {
                "type": "message",
                "role": "user",
                "content": "What is 2+2?"
            },
            {
                "type": "message",
                "role": "assistant",
                "content": "2+2 equals 4."
            },
            {
                "type": "message",
                "role": "user",
                "content": "And 3+3?"
            }
        ],
        "max_output_tokens": 100
    });

    let openai_request = pipeline
        .transform_request(response_api_request, &ctx)
        .unwrap();

    // Verify conversion
    assert_eq!(openai_request["model"], "gpt-4");
    let messages = openai_request["messages"].as_array().unwrap();
    // Should have system + 3 conversation messages
    assert!(messages.len() >= 3);
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_invalid_json_handling() {
    let openai = OpenAITransformer::new();

    // Invalid JSON structure (missing required fields)
    let request = json!({
        "model": "gpt-4"
        // Missing "messages" field
    });

    let result = openai.transform_request_out(request);
    assert!(result.is_err());
}

#[test]
fn test_invalid_role_handling() {
    let openai = OpenAITransformer::new();

    // Invalid role should default to user
    let request = json!({
        "model": "gpt-4",
        "messages": [
            {"role": "invalid_role", "content": "Hello"}
        ]
    });

    let unified = openai.transform_request_out(request).unwrap();
    // Should default to user role
    assert_eq!(unified.messages[0].role, Role::User);
}

// ============================================================================
// Parameter Preservation Tests
// ============================================================================

#[test]
fn test_temperature_preservation() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": "Hello"}],
        "temperature": 0.8
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();
    assert_eq!(anthropic_request["temperature"], 0.8);
}

#[test]
fn test_top_p_preservation() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": "Hello"}],
        "top_p": 0.9
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();
    assert_eq!(anthropic_request["top_p"], 0.9);
}

#[test]
fn test_stop_sequences_preservation() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

    let openai_request = json!({
        "model": "claude-3-opus",
        "messages": [{"role": "user", "content": "Hello"}],
        "stop": ["END", "STOP"]
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();
    assert!(anthropic_request["stop_sequences"].is_array());
    let stop_seqs = anthropic_request["stop_sequences"].as_array().unwrap();
    assert_eq!(stop_seqs.len(), 2);
}

// ============================================================================
// SseParser Integration Tests
// ============================================================================

#[cfg(test)]
mod sse_parser_integration {
    use llm_proxy_rust::transformer::SseParser;

    #[test]
    fn test_sse_parser_handles_fragmented_chunks() {
        let mut parser = SseParser::new();

        // Simulates TCP fragmentation: event split across two chunks
        let chunk1 = b"data: {\"type\":\"ping\"}\n";
        let chunk2 = b"\ndata: {\"type\":\"content_block_delta\"}\n\n";

        let events1 = parser.parse(chunk1);
        assert!(events1.is_empty(), "incomplete event should yield nothing");

        let events2 = parser.parse(chunk2);
        assert_eq!(
            events2.len(),
            2,
            "both events should be emitted on second chunk"
        );
        assert_eq!(events2[0].data.as_deref(), Some("{\"type\":\"ping\"}"));
        assert_eq!(
            events2[1].data.as_deref(),
            Some("{\"type\":\"content_block_delta\"}")
        );
    }

    #[test]
    fn test_sse_parser_skips_comment_lines() {
        let mut parser = SseParser::new();
        let chunk = b": heartbeat\ndata: hello\n\n";
        let events = parser.parse(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data.as_deref(), Some("hello"));
    }

    #[test]
    fn test_sse_parser_done_event() {
        let mut parser = SseParser::new();
        let chunk = b"data: [DONE]\n\n";
        let events = parser.parse(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data.as_deref(), Some("[DONE]"));
    }

    #[test]
    fn test_sse_parser_handles_utf8_fragmented_chunks() {
        let mut parser = SseParser::new();

        let events1 = parser.parse(b"data: \xE4\xB8");
        assert!(
            events1.is_empty(),
            "incomplete utf8 should wait for next chunk"
        );

        let events2 = parser.parse(b"\xAD\n\n");
        assert_eq!(events2.len(), 1);
        assert_eq!(events2[0].data.as_deref(), Some("中"));
    }

    #[test]
    fn test_sse_parser_multiline_data_payload() {
        let mut parser = SseParser::new();
        let chunk = b"data: {\"a\":1}\ndata: {\"b\":2}\n\n";
        let events = parser.parse(chunk);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data.as_deref(), Some("{\"a\":1}\n{\"b\":2}"));
    }
}
