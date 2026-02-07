//! Unit tests for Gemini 3 thought_signature handling.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use llm_proxy_rust::api::gemini3::{
    encode_tool_call_id_with_signature, extract_thought_signature_from_tool_call, is_gemini3_model,
    normalize_request_payload, normalize_response_payload, strip_gemini3_provider_fields,
    THOUGHT_SIGNATURE_SEPARATOR,
};
use serde_json::json;

#[test]
fn test_is_gemini3_model() {
    assert!(is_gemini3_model("gemini-3-pro"));
    assert!(is_gemini3_model("vertex_ai/gemini-3-pro-preview"));
    assert!(!is_gemini3_model("gemini-2.5-pro"));
    assert!(!is_gemini3_model("gemini3-pro"));
    assert!(!is_gemini3_model("gpt-4"));
}

#[test]
fn test_encode_decode_tool_call_id_with_signature() {
    let base_id = "call_abc123";
    let signature = "sig_value";

    let encoded = encode_tool_call_id_with_signature(base_id, signature);
    assert!(encoded.contains(THOUGHT_SIGNATURE_SEPARATOR));

    let tool_call = json!({
        "id": encoded,
        "type": "function",
        "function": {"name": "test", "arguments": "{}"}
    });

    let extracted = extract_thought_signature_from_tool_call(&tool_call, None, false);
    assert_eq!(extracted, Some(signature.to_string()));
}

#[test]
fn test_normalize_request_adds_dummy_signature() {
    let mut payload = json!({
        "model": "gemini-3-pro",
        "messages": [
            {
                "role": "assistant",
                "tool_calls": [
                    {
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "do", "arguments": "{}"}
                    }
                ]
            }
        ]
    });

    let changed = normalize_request_payload(&mut payload, Some("gemini-3-pro"));
    assert!(changed);

    let expected_dummy = BASE64_STANDARD.encode("skip_thought_signature_validator");
    let thought_signature = payload["messages"][0]["tool_calls"][0]["provider_specific_fields"]
        ["thought_signature"]
        .as_str()
        .unwrap();
    assert_eq!(thought_signature, expected_dummy);
    let fn_signature = payload["messages"][0]["tool_calls"][0]["function"]
        ["provider_specific_fields"]["thought_signature"]
        .as_str()
        .unwrap();
    assert_eq!(fn_signature, expected_dummy);
    let extra_signature = payload["messages"][0]["tool_calls"][0]["extra_content"]["google"]
        ["thought_signature"]
        .as_str()
        .unwrap();
    assert_eq!(extra_signature, expected_dummy);
}

#[test]
fn test_normalize_request_strips_signature_for_non_gemini() {
    let encoded = "call_abc__thought__sig";
    let mut payload = json!({
        "model": "gpt-4",
        "messages": [
            {
                "role": "assistant",
                "tool_calls": [
                    {
                        "id": encoded,
                        "type": "function",
                        "function": {"name": "do", "arguments": "{}"}
                    }
                ]
            },
            {
                "role": "tool",
                "tool_call_id": encoded,
                "content": "ok"
            }
        ]
    });

    let changed = normalize_request_payload(&mut payload, Some("gpt-4"));
    assert!(changed);

    assert_eq!(payload["messages"][0]["tool_calls"][0]["id"], "call_abc");
    assert_eq!(payload["messages"][1]["tool_call_id"], "call_abc");
}

#[test]
fn test_normalize_response_embeds_signature() {
    let mut response = json!({
        "choices": [
            {
                "message": {
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "do", "arguments": "{}"},
                            "extra_content": {
                                "google": {"thought_signature": "sig_resp"}
                            }
                        }
                    ]
                }
            }
        ]
    });

    let changed = normalize_response_payload(&mut response, Some("gemini-3-pro"));
    assert!(changed);

    let tool_call = &response["choices"][0]["message"]["tool_calls"][0];
    assert_eq!(
        tool_call["provider_specific_fields"]["thought_signature"],
        "sig_resp"
    );
    let tool_call_id = tool_call["id"].as_str().unwrap();
    assert!(tool_call_id.contains(THOUGHT_SIGNATURE_SEPARATOR));
}

#[test]
fn test_normalize_response_noop_for_non_gemini() {
    let mut response = json!({
        "choices": [
            {
                "message": {
                    "tool_calls": [
                        {
                            "id": "call_1__thought__sig",
                            "type": "function",
                            "function": {"name": "do", "arguments": "{}"},
                            "extra_content": {
                                "google": {"thought_signature": "sig_resp"}
                            }
                        }
                    ]
                }
            }
        ]
    });

    let changed = normalize_response_payload(&mut response, Some("gpt-4"));
    assert!(!changed);

    let tool_call = &response["choices"][0]["message"]["tool_calls"][0];
    assert!(tool_call.get("provider_specific_fields").is_none());
    assert_eq!(tool_call["id"], "call_1__thought__sig");
}

#[test]
fn test_strip_gemini3_provider_fields_non_gemini_noop() {
    let mut payload = json!({
        "model": "gpt-4",
        "thinkingConfig": {"thinkingLevel": "low"},
        "thinking_level": "low",
        "thinking_config": {"thinkingLevel": "high"}
    });

    let changed = strip_gemini3_provider_fields(&mut payload, Some("gpt-4"));
    assert!(!changed);
    assert!(payload.get("thinkingConfig").is_some());
    assert!(payload.get("thinking_level").is_some());
    assert!(payload.get("thinking_config").is_some());
}

#[test]
fn test_extra_content_roundtrip_preserved() {
    let original = json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_123",
                    "type": "function",
                    "function": {"name": "read_file", "arguments": "{}"},
                    "extra_content": {
                        "google": {"thought_signature": "CvcQAdHN2OekY10ClPFkYA=="}
                    }
                }]
            }
        }]
    });

    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        deserialized["choices"][0]["message"]["tool_calls"][0]["extra_content"]["google"]
            ["thought_signature"],
        "CvcQAdHN2OekY10ClPFkYA=="
    );
}

#[test]
fn test_normalize_request_sets_default_temperature() {
    let mut payload = json!({
        "model": "gemini-3-pro",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let changed = normalize_request_payload(&mut payload, Some("gemini-3-pro"));
    assert!(changed);
    assert_eq!(payload["temperature"], 1.0);
}

#[test]
fn test_reasoning_effort_maps_thinking_level() {
    let mut payload = json!({
        "model": "gemini-3-pro",
        "reasoning_effort": "medium",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let changed = normalize_request_payload(&mut payload, Some("gemini-3-pro"));
    assert!(changed);
    assert_eq!(payload["thinking_level"], "high");
    assert!(payload.get("reasoning_effort").is_none());
}

#[test]
fn test_reasoning_effort_maps_flash() {
    let mut payload = json!({
        "model": "gemini-3-flash-preview",
        "reasoning_effort": "medium",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let changed = normalize_request_payload(&mut payload, Some("gemini-3-flash-preview"));
    assert!(changed);
    assert_eq!(payload["thinking_level"], "medium");
    assert!(payload.get("reasoning_effort").is_none());
}

#[test]
fn test_default_thinking_level_sets_config() {
    let mut payload = json!({
        "model": "gemini-3-pro",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let changed = normalize_request_payload(&mut payload, Some("gemini-3-pro"));
    assert!(changed);
    assert_eq!(payload["thinking_level"], "low");
    assert_eq!(payload["thinkingConfig"]["thinkingLevel"], "low");
}

#[test]
fn test_default_thinking_level_flash_sets_config() {
    let mut payload = json!({
        "model": "gemini-3-flash-preview",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let changed = normalize_request_payload(&mut payload, Some("gemini-3-flash-preview"));
    assert!(changed);
    assert_eq!(payload["thinking_level"], "minimal");
    assert_eq!(payload["thinkingConfig"]["thinkingLevel"], "minimal");
}

#[test]
fn test_default_thinking_level_skips_image_models() {
    let mut payload = json!({
        "model": "gemini-3-pro-image-preview",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let changed = normalize_request_payload(&mut payload, Some("gemini-3-pro-image-preview"));
    assert!(changed);
    assert!(payload.get("thinking_level").is_none());
    assert!(payload.get("thinkingConfig").is_none());
}

#[test]
fn test_normalize_request_strips_penalty_params() {
    let mut payload = json!({
        "model": "gemini-3-pro",
        "frequency_penalty": 0.1,
        "presence_penalty": 0.2,
        "messages": [{"role": "user", "content": "hi"}]
    });

    let changed = normalize_request_payload(&mut payload, Some("gemini-3-pro"));
    assert!(changed);
    assert!(payload.get("frequency_penalty").is_none());
    assert!(payload.get("presence_penalty").is_none());
}

#[test]
fn test_reasoning_effort_sets_thinking_config() {
    let mut payload = json!({
        "model": "gemini-3-pro",
        "reasoning_effort": "medium",
        "messages": [{"role": "user", "content": "hi"}]
    });

    let changed = normalize_request_payload(&mut payload, Some("gemini-3-pro"));
    assert!(changed);
    assert_eq!(payload["thinkingConfig"]["thinkingLevel"], "high");
    assert_eq!(payload["thinkingConfig"]["includeThoughts"], true);
}

#[test]
fn test_strip_gemini3_provider_fields_removes_thinking_config() {
    let mut payload = json!({
        "model": "gemini-3-pro",
        "thinking_level": "low",
        "thinkingConfig": {"thinkingLevel": "low"},
        "messages": [{"role": "user", "content": "hi"}]
    });

    let changed = strip_gemini3_provider_fields(&mut payload, Some("gemini-3-pro"));
    assert!(changed);
    assert!(payload.get("thinking_level").is_none());
    assert!(payload.get("thinkingConfig").is_none());
}

#[test]
fn test_normalize_response_parses_parts_thinking_blocks() {
    let mut response = json!({
        "choices": [
            {
                "message": {
                    "parts": [
                        {"text": "hidden", "thought": true, "thoughtSignature": "sig1"},
                        {"text": "visible"}
                    ]
                }
            }
        ]
    });

    let changed = normalize_response_payload(&mut response, Some("gemini-3-pro"));
    assert!(changed);
    let message = &response["choices"][0]["message"];
    assert_eq!(message["content"], "visible");
    assert_eq!(message["reasoning_content"], "hidden");
    assert_eq!(message["thinking_blocks"][0]["thinking"], "hidden");
    assert_eq!(message["thinking_blocks"][0]["signature"], "sig1");
    assert_eq!(
        message["provider_specific_fields"]["thought_signatures"],
        json!(["sig1"])
    );
}
