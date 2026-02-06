//! Request rectifier for provider-bound payloads.

use serde_json::{json, Value};

/// Sanitize provider payload to avoid cross-provider validation errors.
pub(crate) fn sanitize_provider_payload(payload: &mut Value) {
    {
        if let Some(messages) = payload.get_mut("messages").and_then(|m| m.as_array_mut()) {
            for msg in messages {
                let is_assistant = msg.get("role").and_then(|r| r.as_str()) == Some("assistant");
                if let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) {
                    content.retain(|block| {
                        !matches!(
                            block.get("type").and_then(|t| t.as_str()),
                            Some("thinking") | Some("redacted_thinking")
                        )
                    });

                    for block in content.iter_mut() {
                        if let Some(obj) = block.as_object_mut() {
                            obj.remove("signature");
                        }

                        if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                if text.trim().is_empty() {
                                    block["text"] = json!(".");
                                }
                            }
                        }
                    }

                    if content.is_empty() && is_assistant {
                        content.push(json!({"type": "text", "text": "."}));
                    }
                }
            }
        }
    }

    if should_remove_top_level_thinking(payload) {
        if let Some(obj) = payload.as_object_mut() {
            obj.remove("thinking");
        }
    }
}

fn should_remove_top_level_thinking(payload: &Value) -> bool {
    let thinking_enabled = payload
        .get("thinking")
        .and_then(|t| t.get("type"))
        .and_then(|t| t.as_str())
        == Some("enabled");

    if !thinking_enabled {
        return false;
    }

    let messages = match payload.get("messages").and_then(|m| m.as_array()) {
        Some(messages) => messages,
        None => return false,
    };

    let last_assistant_content = messages
        .iter()
        .rev()
        .find(|msg| msg.get("role").and_then(|r| r.as_str()) == Some("assistant"))
        .and_then(|msg| msg.get("content"))
        .and_then(|content| content.as_array())
        .filter(|content| !content.is_empty());

    let last_assistant_content = match last_assistant_content {
        Some(content) => content,
        None => return false,
    };

    let first_block_type = last_assistant_content
        .first()
        .and_then(|block| block.get("type"))
        .and_then(|t| t.as_str());

    if matches!(
        first_block_type,
        Some("thinking") | Some("redacted_thinking")
    ) {
        return false;
    }

    last_assistant_content
        .iter()
        .any(|block| block.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_provider_payload() {
        let mut payload = json!({
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "Let me think...", "signature": "abc123"},
                    {"type": "text", "text": "Here's my answer"}
                ]}
            ]
        });
        sanitize_provider_payload(&mut payload);
        let blocks = payload["messages"][1]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "Here's my answer");
    }

    #[test]
    fn test_sanitize_no_thinking() {
        let mut payload = json!({
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Response"}
                ]}
            ]
        });
        let original = payload.clone();
        sanitize_provider_payload(&mut payload);
        assert_eq!(payload, original);
    }

    #[test]
    fn test_sanitize_string_content() {
        let mut payload = json!({
            "messages": [
                {"role": "user", "content": "Hello"},
                {"role": "assistant", "content": "Simple text response"}
            ]
        });
        let original = payload.clone();
        sanitize_provider_payload(&mut payload);
        assert_eq!(payload, original);
    }

    #[test]
    fn test_sanitize_multiple_messages() {
        let mut payload = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "First thought", "signature": "sig1"},
                    {"type": "text", "text": "Answer 1"}
                ]},
                {"role": "user", "content": "Follow up"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "Second thought", "signature": "sig2"},
                    {"type": "text", "text": "Answer 2"}
                ]}
            ]
        });
        sanitize_provider_payload(&mut payload);
        let blocks0 = payload["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks0.len(), 1);
        assert_eq!(blocks0[0]["text"], "Answer 1");
        let blocks2 = payload["messages"][2]["content"].as_array().unwrap();
        assert_eq!(blocks2.len(), 1);
        assert_eq!(blocks2[0]["text"], "Answer 2");
    }

    #[test]
    fn test_sanitize_only_thinking() {
        let mut payload = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "Just thinking", "signature": "sig1"}
                ]},
                {"role": "user", "content": "Next question"}
            ]
        });
        sanitize_provider_payload(&mut payload);
        let blocks = payload["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], ".");
    }

    #[test]
    fn test_sanitize_blank_text_fields() {
        let mut payload = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "text", "text": ""},
                ]},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "  "},
                ]},
                {"role": "user", "content": [
                    {"type": "text", "text": ""},
                    {"type": "text", "text": "real content"},
                ]},
            ]
        });
        sanitize_provider_payload(&mut payload);
        assert_eq!(payload["messages"][0]["content"][0]["text"], ".");
        assert_eq!(payload["messages"][1]["content"][0]["text"], ".");
        assert_eq!(payload["messages"][2]["content"][0]["text"], ".");
        assert_eq!(payload["messages"][2]["content"][1]["text"], "real content");
    }

    #[test]
    fn test_sanitize_removes_redacted_and_signatures() {
        let mut payload = json!({
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "redacted_thinking", "data": "xxx", "signature": "sig_redacted"},
                    {"type": "text", "text": "hello", "signature": "sig_text"},
                    {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {}, "signature": "sig_tool"}
                ]}
            ]
        });

        sanitize_provider_payload(&mut payload);

        let content = payload["messages"][0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 2);
        assert_eq!(content[0]["type"], "text");
        assert!(content[0].get("signature").is_none());
        assert_eq!(content[1]["type"], "tool_use");
        assert!(content[1].get("signature").is_none());
    }

    #[test]
    fn test_sanitize_removes_top_level_thinking_for_tool_chain_without_prefix() {
        let mut payload = json!({
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "reasoning", "signature": "sig"},
                    {"type": "tool_use", "id": "toolu_1", "name": "lookup", "input": {}}
                ]}
            ]
        });

        sanitize_provider_payload(&mut payload);

        assert!(payload.get("thinking").is_none());
    }

    #[test]
    fn test_sanitize_keeps_top_level_thinking_without_tool_use() {
        let mut payload = json!({
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "messages": [
                {"role": "assistant", "content": [
                    {"type": "text", "text": "hello"}
                ]}
            ]
        });

        sanitize_provider_payload(&mut payload);

        assert!(payload.get("thinking").is_some());
    }
}
