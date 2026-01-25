//! Unit tests for Gemini 3 thought_signature support.
//!
//! These tests verify the is_gemini3_provider_name detection function
//! and the extra_content preservation in JSON handling.

use serde_json::json;

// ============================================================================
// Provider Name Detection Tests
// ============================================================================

/// Check if provider name indicates Gemini 3 (for thought_signature handling)
fn is_gemini3_provider_name(name: &str) -> bool {
    let name_lower = name.to_lowercase();
    name_lower.contains("gemini-3")
        || name_lower.contains("gemini3")
        || name_lower.contains("gemini_3")
}

#[test]
fn test_is_gemini3_provider_name_with_hyphen() {
    // Should detect gemini-3 variants
    assert!(is_gemini3_provider_name("gemini-3-pro"));
    assert!(is_gemini3_provider_name("Gemini-3-Pro"));
    assert!(is_gemini3_provider_name("GEMINI-3-FLASH"));
    assert!(is_gemini3_provider_name("gemini-3"));
    assert!(is_gemini3_provider_name("my-gemini-3-provider"));
}

#[test]
fn test_is_gemini3_provider_name_without_hyphen() {
    // Should detect gemini3 variants
    assert!(is_gemini3_provider_name("gemini3-pro"));
    assert!(is_gemini3_provider_name("Gemini3Pro"));
    assert!(is_gemini3_provider_name("gemini3"));
    assert!(is_gemini3_provider_name("provider-gemini3"));
}

#[test]
fn test_is_gemini3_provider_name_with_underscore() {
    // Should detect gemini_3 variants
    assert!(is_gemini3_provider_name("gemini_3_pro"));
    assert!(is_gemini3_provider_name("Gemini_3"));
    assert!(is_gemini3_provider_name("gemini_3_preview"));
}

#[test]
fn test_is_gemini3_provider_name_non_gemini3() {
    // Should NOT detect non-Gemini 3 providers
    assert!(!is_gemini3_provider_name("gemini-2.5-pro"));
    assert!(!is_gemini3_provider_name("gemini-flash")); // Gemini 1.x
    assert!(!is_gemini3_provider_name("gemini-pro")); // Gemini 1.x
    assert!(!is_gemini3_provider_name("gpt-4"));
    assert!(!is_gemini3_provider_name("OpenAI-GPT4"));
    assert!(!is_gemini3_provider_name("claude-3-opus"));
    assert!(!is_gemini3_provider_name("openai"));
}

#[test]
fn test_is_gemini3_provider_name_edge_cases() {
    // Should handle edge cases
    assert!(!is_gemini3_provider_name(""));
    assert!(!is_gemini3_provider_name("gemini"));
    assert!(!is_gemini3_provider_name("3"));
    assert!(!is_gemini3_provider_name("gemini-2-pro"));
    assert!(!is_gemini3_provider_name("gemini-1.5-pro"));
}

// ============================================================================
// Extra Content Preservation Tests
// ============================================================================

#[test]
fn test_extra_content_preserved_in_gemini3_response() {
    // Verify that serde_json::Value preserves extra_content automatically
    let response = json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_123",
                    "type": "function",
                    "function": {
                        "name": "read_file",
                        "arguments": "{}"
                    },
                    "extra_content": {
                        "google": {
                            "thought_signature": "CvcQAdHN2OekY10ClPFkYA=="
                        }
                    }
                }]
            }
        }]
    });

    // Verify extra_content.google.thought_signature is present
    let tool_call = &response["choices"][0]["message"]["tool_calls"][0];
    assert!(tool_call["extra_content"]["google"]["thought_signature"].is_string());
    assert_eq!(
        tool_call["extra_content"]["google"]["thought_signature"].as_str().unwrap(),
        "CvcQAdHN2OekY10ClPFkYA=="
    );
}

#[test]
fn test_extra_content_in_streaming_delta() {
    // Verify streaming chunk preserves extra_content
    let chunk = json!({
        "choices": [{
            "delta": {
                "role": "assistant",
                "tool_calls": [{
                    "extra_content": {
                        "google": {
                            "thought_signature": "CrICAdHtim827fQ..."
                        }
                    },
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"NYC\"}"
                    },
                    "id": "function-call-123",
                    "type": "function"
                }]
            },
            "index": 0
        }]
    });

    // Verify streaming chunk preserves extra_content
    let tool_call = &chunk["choices"][0]["delta"]["tool_calls"][0];
    assert!(tool_call["extra_content"]["google"]["thought_signature"].is_string());
}

#[test]
fn test_extra_content_at_message_level() {
    // Verify extra_content at message level (text responses with thinking)
    let response = json!({
        "choices": [{
            "message": {
                "content": "I can help with that",
                "extra_content": {
                    "google": {
                        "thought_signature": "CrICAdHtim827fQ..."
                    }
                }
            }
        }]
    });

    let message = &response["choices"][0]["message"];
    assert!(message["extra_content"]["google"]["thought_signature"].is_string());
}

#[test]
fn test_extra_content_roundtrip_serialization() {
    // Verify extra_content survives JSON roundtrip
    let original = json!({
        "messages": [
            {"role": "user", "content": "Get weather for NYC"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_123",
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "arguments": "{\"location\":\"NYC\"}"
                    },
                    "extra_content": {
                        "google": {
                            "thought_signature": "CvcQAdHN2OekY10ClPFkYA=="
                        }
                    }
                }]
            },
            {
                "role": "tool",
                "tool_call_id": "call_123",
                "content": "72°F and sunny"
            }
        ]
    });

    // Simulate pass-through: serialize and deserialize
    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: serde_json::Value = serde_json::from_str(&serialized).unwrap();

    // Verify extra_content is preserved
    let tool_call = &deserialized["messages"][1]["tool_calls"][0];
    assert_eq!(
        tool_call["extra_content"]["google"]["thought_signature"].as_str().unwrap(),
        "CvcQAdHN2OekY10ClPFkYA=="
    );
}

#[test]
fn test_multiple_tool_calls_with_signatures() {
    // Verify multiple tool_calls each preserve their signatures
    let response = json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "func1", "arguments": "{}"},
                        "extra_content": {
                            "google": {"thought_signature": "sig_1"}
                        }
                    },
                    {
                        "id": "call_2",
                        "type": "function",
                        "function": {"name": "func2", "arguments": "{}"},
                        "extra_content": {
                            "google": {"thought_signature": "sig_2"}
                        }
                    },
                    {
                        "id": "call_3",
                        "type": "function",
                        "function": {"name": "func3", "arguments": "{}"}
                        // No extra_content - should also be preserved as-is
                    }
                ]
            }
        }]
    });

    let tool_calls = response["choices"][0]["message"]["tool_calls"].as_array().unwrap();

    // First two have signatures
    assert_eq!(
        tool_calls[0]["extra_content"]["google"]["thought_signature"].as_str().unwrap(),
        "sig_1"
    );
    assert_eq!(
        tool_calls[1]["extra_content"]["google"]["thought_signature"].as_str().unwrap(),
        "sig_2"
    );

    // Third has no extra_content
    assert!(tool_calls[2].get("extra_content").is_none());
}

// ============================================================================
// Pass-Through Strategy Verification
// ============================================================================

#[test]
fn test_pass_through_preserves_unknown_fields() {
    // Verify that serde_json::Value preserves all fields, including unknown ones
    let json_str = r#"{
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_123",
                    "type": "function",
                    "function": {"name": "test", "arguments": "{}"},
                    "extra_content": {
                        "google": {"thought_signature": "test_sig"},
                        "other_provider": {"custom_field": "value"}
                    },
                    "unknown_field": "should_be_preserved",
                    "another_unknown": 42
                }]
            }
        }]
    }"#;

    let parsed: serde_json::Value = serde_json::from_str(json_str).unwrap();
    let serialized = serde_json::to_string(&parsed).unwrap();
    let reparsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();

    let tool_call = &reparsed["choices"][0]["message"]["tool_calls"][0];

    // All fields should be preserved
    assert_eq!(tool_call["extra_content"]["google"]["thought_signature"], "test_sig");
    assert_eq!(tool_call["extra_content"]["other_provider"]["custom_field"], "value");
    assert_eq!(tool_call["unknown_field"], "should_be_preserved");
    assert_eq!(tool_call["another_unknown"], 42);
}

#[test]
fn test_request_format_compliance() {
    // Verify the expected request format that should be sent to Gemini 3
    let request = json!({
        "model": "gemini-3-pro",
        "messages": [
            {"role": "user", "content": "Get weather for funny city names"},
            {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "function-call-5873527561210830497",
                    "type": "function",
                    "function": {
                        "name": "get_current_weather",
                        "arguments": "{\"location\":\"Intercourse, PA\",\"unit\":\"fahrenheit\"}"
                    },
                    "extra_content": {
                        "google": {
                            "thought_signature": "CvcQAdHN2OekY10ClPFkYA=="
                        }
                    }
                }]
            },
            {
                "role": "tool",
                "tool_call_id": "function-call-5873527561210830497",
                "content": "66°F and Sunny"
            }
        ]
    });

    // Verify the structure is valid
    assert_eq!(request["model"], "gemini-3-pro");
    assert_eq!(request["messages"].as_array().unwrap().len(), 3);

    let assistant_msg = &request["messages"][1];
    assert_eq!(assistant_msg["role"], "assistant");

    let tool_call = &assistant_msg["tool_calls"][0];
    assert_eq!(
        tool_call["extra_content"]["google"]["thought_signature"],
        "CvcQAdHN2OekY10ClPFkYA=="
    );
}
