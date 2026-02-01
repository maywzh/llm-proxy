use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde_json::{Map, Value};

pub const THOUGHT_SIGNATURE_SEPARATOR: &str = "__thought__";

pub fn is_gemini3_model(model: &str) -> bool {
    model.to_lowercase().contains("gemini-3")
}

fn is_gemini3_flash(model: &str) -> bool {
    model.to_lowercase().contains("gemini-3-flash")
}

fn dummy_thought_signature() -> String {
    BASE64_STANDARD.encode("skip_thought_signature_validator")
}

fn map_reasoning_effort_to_thinking_level(
    reasoning_effort: &str,
    model: &str,
) -> Option<&'static str> {
    let effort = reasoning_effort.to_lowercase();
    let is_flash = is_gemini3_flash(model);

    match effort.as_str() {
        "minimal" => Some(if is_flash { "minimal" } else { "low" }),
        "low" => Some("low"),
        "medium" => Some(if is_flash { "medium" } else { "high" }),
        "high" => Some("high"),
        "disable" | "none" => Some(if is_flash { "minimal" } else { "low" }),
        _ => None,
    }
}

fn signature_from_provider_fields(value: &Value) -> Option<String> {
    value
        .get("provider_specific_fields")
        .and_then(|fields| fields.as_object())
        .and_then(|fields| fields.get("thought_signature"))
        .and_then(|sig| sig.as_str())
        .map(|sig| sig.to_string())
}

fn signature_from_function_provider_fields(value: &Value) -> Option<String> {
    value
        .get("function")
        .and_then(|function| function.as_object())
        .and_then(|function| function.get("provider_specific_fields"))
        .and_then(|fields| fields.as_object())
        .and_then(|fields| fields.get("thought_signature"))
        .and_then(|sig| sig.as_str())
        .map(|sig| sig.to_string())
}

fn signature_from_extra_content(value: &Value) -> Option<String> {
    value
        .get("extra_content")
        .and_then(|extra| extra.as_object())
        .and_then(|extra| extra.get("google"))
        .and_then(|google| google.as_object())
        .and_then(|google| google.get("thought_signature"))
        .and_then(|sig| sig.as_str())
        .map(|sig| sig.to_string())
}

fn signature_from_tool_call_id(value: &Value) -> Option<String> {
    let tool_call_id = value.get("id").and_then(|id| id.as_str())?;
    if !tool_call_id.contains(THOUGHT_SIGNATURE_SEPARATOR) {
        return None;
    }
    let mut parts = tool_call_id.splitn(2, THOUGHT_SIGNATURE_SEPARATOR);
    let _ = parts.next();
    parts.next().map(|sig| sig.to_string())
}

pub fn encode_tool_call_id_with_signature(tool_call_id: &str, signature: &str) -> String {
    format!(
        "{}{}{}",
        tool_call_id, THOUGHT_SIGNATURE_SEPARATOR, signature
    )
}

fn strip_signature_from_id(tool_call_id: &str) -> Option<String> {
    if !tool_call_id.contains(THOUGHT_SIGNATURE_SEPARATOR) {
        return None;
    }
    let base = tool_call_id
        .splitn(2, THOUGHT_SIGNATURE_SEPARATOR)
        .next()
        .unwrap_or("");
    Some(base.to_string())
}

pub fn extract_thought_signature_from_tool_call(
    tool_call: &Value,
    model: Option<&str>,
    allow_dummy: bool,
) -> Option<String> {
    if let Some(sig) = signature_from_provider_fields(tool_call) {
        return Some(sig);
    }
    if let Some(sig) = signature_from_function_provider_fields(tool_call) {
        return Some(sig);
    }
    if let Some(sig) = signature_from_extra_content(tool_call) {
        return Some(sig);
    }
    if let Some(sig) = signature_from_tool_call_id(tool_call) {
        return Some(sig);
    }
    if allow_dummy {
        if let Some(model_name) = model {
            if is_gemini3_model(model_name) {
                return Some(dummy_thought_signature());
            }
        }
    }
    None
}

fn ensure_provider_specific_fields(obj: &mut Map<String, Value>) -> &mut Map<String, Value> {
    let entry = obj
        .entry("provider_specific_fields".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry
        .as_object_mut()
        .expect("provider_specific_fields must be object")
}

pub fn normalize_request_payload(payload: &mut Value, model: Option<&str>) -> bool {
    let model_name = match model {
        Some(name) => name,
        None => return false,
    };

    let mut changed = false;

    if !is_gemini3_model(model_name) {
        let messages = match payload.get_mut("messages") {
            Some(messages) => match messages.as_array_mut() {
                Some(messages) => messages,
                None => return false,
            },
            None => return false,
        };
        for message in messages.iter_mut() {
            let Some(msg_obj) = message.as_object_mut() else {
                continue;
            };

            if msg_obj.get("role").and_then(|role| role.as_str()) == Some("assistant") {
                if let Some(tool_calls) =
                    msg_obj.get_mut("tool_calls").and_then(|v| v.as_array_mut())
                {
                    for tool_call in tool_calls.iter_mut() {
                        let Some(tc_obj) = tool_call.as_object_mut() else {
                            continue;
                        };
                        if let Some(id_value) = tc_obj.get("id").and_then(|id| id.as_str()) {
                            if let Some(base) = strip_signature_from_id(id_value) {
                                tc_obj.insert("id".to_string(), Value::String(base));
                                changed = true;
                            }
                        }
                    }
                }
            }

            if msg_obj.get("role").and_then(|role| role.as_str()) == Some("tool") {
                if let Some(tool_call_id) = msg_obj.get("tool_call_id").and_then(|id| id.as_str()) {
                    if let Some(base) = strip_signature_from_id(tool_call_id) {
                        msg_obj.insert("tool_call_id".to_string(), Value::String(base));
                        changed = true;
                    }
                }
            }
        }
        return changed;
    }

    if let Some(obj) = payload.as_object_mut() {
        if obj.get("temperature").is_none() || obj.get("temperature") == Some(&Value::Null) {
            obj.insert(
                "temperature".to_string(),
                Value::Number(serde_json::Number::from_f64(1.0).unwrap()),
            );
            changed = true;
        }

        if let Some(reasoning_effort) = obj.get("reasoning_effort").and_then(|v| v.as_str()) {
            if obj.get("thinking_level").is_none() {
                if let Some(thinking_level) =
                    map_reasoning_effort_to_thinking_level(reasoning_effort, model_name)
                {
                    obj.insert(
                        "thinking_level".to_string(),
                        Value::String(thinking_level.to_string()),
                    );
                    changed = true;
                }
            }
        }

        if obj.contains_key("reasoning_effort") {
            obj.remove("reasoning_effort");
            changed = true;
        }
    }

    let messages = match payload.get_mut("messages") {
        Some(messages) => match messages.as_array_mut() {
            Some(messages) => messages,
            None => return false,
        },
        None => return false,
    };

    for message in messages.iter_mut() {
        let Some(msg_obj) = message.as_object_mut() else {
            continue;
        };

        if let Some(tool_calls) = msg_obj.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
            for tool_call in tool_calls.iter_mut() {
                let signature =
                    extract_thought_signature_from_tool_call(tool_call, Some(model_name), true);
                if let Some(signature) = signature {
                    if let Some(tc_obj) = tool_call.as_object_mut() {
                        let provider_fields = ensure_provider_specific_fields(tc_obj);
                        let current = provider_fields
                            .get("thought_signature")
                            .and_then(|sig| sig.as_str())
                            .unwrap_or("");
                        if current != signature {
                            provider_fields
                                .insert("thought_signature".to_string(), Value::String(signature));
                            changed = true;
                        }
                    }
                }
            }
        }

        if let Some(function_call) = msg_obj.get_mut("function_call") {
            let existing = signature_from_provider_fields(function_call);
            if existing.is_none() {
                let signature = dummy_thought_signature();
                if let Some(fn_obj) = function_call.as_object_mut() {
                    let provider_fields = ensure_provider_specific_fields(fn_obj);
                    provider_fields
                        .insert("thought_signature".to_string(), Value::String(signature));
                    changed = true;
                }
            }
        }
    }

    changed
}

pub fn normalize_response_payload(response: &mut Value, model: Option<&str>) -> bool {
    let model_name = match model {
        Some(name) if is_gemini3_model(name) => name,
        _ => return false,
    };

    let choices = match response.get_mut("choices") {
        Some(choices) => match choices.as_array_mut() {
            Some(choices) => choices,
            None => return false,
        },
        None => return false,
    };

    let mut changed = false;

    for choice in choices.iter_mut() {
        let Some(choice_obj) = choice.as_object_mut() else {
            continue;
        };

        for field in ["message", "delta"] {
            let Some(message) = choice_obj.get_mut(field) else {
                continue;
            };
            let extra_signature = signature_from_extra_content(message);
            let Some(msg_obj) = message.as_object_mut() else {
                continue;
            };

            if let Some(signature) = extra_signature {
                let provider_fields = ensure_provider_specific_fields(msg_obj);
                if !provider_fields.contains_key("thought_signatures") {
                    provider_fields.insert(
                        "thought_signatures".to_string(),
                        Value::Array(vec![Value::String(signature)]),
                    );
                    changed = true;
                }
            }

            if let Some(tool_calls) = msg_obj.get_mut("tool_calls").and_then(|v| v.as_array_mut()) {
                for tool_call in tool_calls.iter_mut() {
                    let signature = extract_thought_signature_from_tool_call(
                        tool_call,
                        Some(model_name),
                        false,
                    );
                    if let Some(signature) = signature {
                        if let Some(tc_obj) = tool_call.as_object_mut() {
                            let provider_fields = ensure_provider_specific_fields(tc_obj);
                            let current = provider_fields
                                .get("thought_signature")
                                .and_then(|sig| sig.as_str())
                                .unwrap_or("");
                            if current != signature {
                                provider_fields.insert(
                                    "thought_signature".to_string(),
                                    Value::String(signature.clone()),
                                );
                                changed = true;
                            }

                            if let Some(id_value) = tc_obj.get("id").and_then(|id| id.as_str()) {
                                if !id_value.contains(THOUGHT_SIGNATURE_SEPARATOR) {
                                    let encoded =
                                        encode_tool_call_id_with_signature(id_value, &signature);
                                    tc_obj.insert("id".to_string(), Value::String(encoded));
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    changed
}

pub fn log_gemini_request_signatures(payload: &Value, model: Option<&str>) {
    let Some(model_name) = model else {
        return;
    };
    if !is_gemini3_model(model_name) {
        return;
    }

    let Some(messages) = payload.get("messages").and_then(|m| m.as_array()) else {
        return;
    };

    let mut sig_count = 0;
    for message in messages {
        let Some(tool_calls) = message.get("tool_calls").and_then(|t| t.as_array()) else {
            continue;
        };
        for tool_call in tool_calls {
            if extract_thought_signature_from_tool_call(tool_call, Some(model_name), false)
                .is_some()
            {
                sig_count += 1;
            }
        }
    }

    if sig_count > 0 {
        tracing::debug!(
            sig_count,
            "Gemini 3 request contains thought_signatures in tool_calls (pass-through)"
        );
    }
}

pub fn log_gemini_response_signatures(response: &Value, model: Option<&str>) {
    let Some(model_name) = model else {
        return;
    };
    if !is_gemini3_model(model_name) {
        return;
    };

    let Some(choices) = response.get("choices").and_then(|c| c.as_array()) else {
        return;
    };

    for choice in choices {
        if let Some(message) = choice.get("message") {
            check_and_log_signatures(message, model_name, "message");
        }
        if let Some(delta) = choice.get("delta") {
            check_and_log_signatures(delta, model_name, "delta");
        }
    }
}

fn check_and_log_signatures(content: &Value, model: &str, location: &str) {
    if let Some(sig) = signature_from_extra_content(content) {
        tracing::debug!(
            location,
            sig_len = sig.len(),
            "Found thought_signature in {}.extra_content",
            location
        );
    }

    let Some(tool_calls) = content.get("tool_calls").and_then(|t| t.as_array()) else {
        return;
    };

    let sig_count = tool_calls
        .iter()
        .filter(|tool_call| {
            extract_thought_signature_from_tool_call(tool_call, Some(model), false).is_some()
        })
        .count();

    if sig_count > 0 {
        tracing::debug!(
            location,
            sig_count,
            "Found {} thought_signatures in {}.tool_calls",
            sig_count,
            location
        );
    }
}
