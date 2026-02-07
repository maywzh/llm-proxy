//! Gemini 3 thought_signature handling aligned with LiteLLM.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde_json::{Map, Value};

pub const THOUGHT_SIGNATURE_SEPARATOR: &str = "__thought__";
const GEMINI3_UNSUPPORTED_PENALTY_PARAMS: [&str; 2] = ["frequency_penalty", "presence_penalty"];

pub fn is_gemini3_model(model: &str) -> bool {
    model.to_lowercase().contains("gemini-3")
}

fn is_gemini3_flash(model: &str) -> bool {
    model.to_lowercase().contains("gemini-3-flash")
}

fn is_gemini3_image(model: &str) -> bool {
    model.to_lowercase().contains("image")
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

fn map_reasoning_effort_to_thinking_config(
    reasoning_effort: &str,
    model: &str,
) -> Option<(String, bool)> {
    let thinking_level = map_reasoning_effort_to_thinking_level(reasoning_effort, model)?;
    let include_thoughts = !matches!(reasoning_effort.to_lowercase().as_str(), "disable" | "none");
    Some((thinking_level.to_string(), include_thoughts))
}

fn default_thinking_level(model: &str) -> Option<&'static str> {
    if is_gemini3_image(model) {
        return None;
    }
    Some(if is_gemini3_flash(model) {
        "minimal"
    } else {
        "low"
    })
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
        .split(THOUGHT_SIGNATURE_SEPARATOR)
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

fn ensure_extra_content_google(obj: &mut Map<String, Value>) -> &mut Map<String, Value> {
    let entry = obj
        .entry("extra_content".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    let extra = entry.as_object_mut().expect("extra_content must be object");
    let google = extra
        .entry("google".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !google.is_object() {
        *google = Value::Object(Map::new());
    }
    google
        .as_object_mut()
        .expect("extra_content.google must be object")
}

fn merge_thought_signatures(
    provider_fields: &mut Map<String, Value>,
    signatures: Vec<String>,
) -> bool {
    if signatures.is_empty() {
        return false;
    }
    if let Some(Value::Array(existing)) = provider_fields.get_mut("thought_signatures") {
        let mut changed = false;
        for sig in signatures {
            if !existing.iter().any(|v| v.as_str() == Some(sig.as_str())) {
                existing.push(Value::String(sig));
                changed = true;
            }
        }
        return changed;
    }
    provider_fields.insert(
        "thought_signatures".to_string(),
        Value::Array(signatures.into_iter().map(Value::String).collect()),
    );
    true
}

fn apply_parts_thought_signatures(message: &mut Map<String, Value>) -> bool {
    let parts = message
        .get("parts")
        .and_then(|v| v.as_array())
        .cloned()
        .or_else(|| {
            message
                .get("content")
                .and_then(|v| v.as_object())
                .and_then(|obj| obj.get("parts"))
                .and_then(|v| v.as_array())
                .cloned()
        });

    let Some(parts) = parts else {
        return false;
    };

    let mut thought_signatures = Vec::new();
    let mut thinking_blocks: Vec<Value> = Vec::new();
    let mut thinking_texts: Vec<String> = Vec::new();
    let mut content_texts: Vec<String> = Vec::new();
    let mut changed = false;

    for part in parts {
        let Some(part_obj) = part.as_object() else {
            continue;
        };
        let signature = part_obj.get("thoughtSignature").and_then(|v| v.as_str());
        if let Some(sig) = signature {
            thought_signatures.push(sig.to_string());
        }
        if let Some(text) = part_obj.get("text").and_then(|v| v.as_str()) {
            if part_obj.get("thought").and_then(|v| v.as_bool()) == Some(true) {
                let mut block = Map::new();
                block.insert("type".to_string(), Value::String("thinking".to_string()));
                block.insert("thinking".to_string(), Value::String(text.to_string()));
                if let Some(sig) = signature {
                    block.insert("signature".to_string(), Value::String(sig.to_string()));
                }
                thinking_blocks.push(Value::Object(block));
                thinking_texts.push(text.to_string());
            } else {
                content_texts.push(text.to_string());
            }
        }
    }

    if !content_texts.is_empty() && message.get("content").and_then(|v| v.as_str()).is_none() {
        message.insert("content".to_string(), Value::String(content_texts.join("")));
        changed = true;
    }

    if !thinking_blocks.is_empty() {
        match message.get_mut("thinking_blocks") {
            Some(Value::Array(existing)) => existing.extend(thinking_blocks),
            _ => {
                message.insert("thinking_blocks".to_string(), Value::Array(thinking_blocks));
            }
        }
        changed = true;

        if !message.contains_key("reasoning_content") && !thinking_texts.is_empty() {
            message.insert(
                "reasoning_content".to_string(),
                Value::String(thinking_texts.join("\n")),
            );
            changed = true;
        }
    }

    if !thought_signatures.is_empty() {
        let provider_fields = ensure_provider_specific_fields(message);
        if merge_thought_signatures(provider_fields, thought_signatures) {
            changed = true;
        }
    }

    changed
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

        for penalty_param in GEMINI3_UNSUPPORTED_PENALTY_PARAMS {
            if obj.remove(penalty_param).is_some() {
                changed = true;
            }
        }

        let mut mapped_thinking_level: Option<String> = None;
        let mut mapped_include_thoughts: Option<bool> = None;
        let reasoning_effort = obj
            .get("reasoning_effort")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        if let Some(reasoning_effort) = reasoning_effort.as_deref() {
            if obj.get("thinking_level").is_none() {
                if let Some(thinking_level) =
                    map_reasoning_effort_to_thinking_level(reasoning_effort, model_name)
                {
                    obj.insert(
                        "thinking_level".to_string(),
                        Value::String(thinking_level.to_string()),
                    );
                    mapped_thinking_level = Some(thinking_level.to_string());
                    changed = true;
                }
            }
            if let Some((level, include_thoughts)) =
                map_reasoning_effort_to_thinking_config(reasoning_effort, model_name)
            {
                if mapped_thinking_level.is_none() {
                    mapped_thinking_level = Some(level);
                }
                mapped_include_thoughts = Some(include_thoughts);
            }
        }

        if obj.contains_key("reasoning_effort") {
            obj.remove("reasoning_effort");
            changed = true;
        }

        let mut thinking_config = obj
            .get("thinkingConfig")
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        if let Some(Value::Object(thinking_config_snake)) = obj.remove("thinking_config") {
            for (key, value) in thinking_config_snake {
                if key == "thinking_level" && !thinking_config.contains_key("thinkingLevel") {
                    thinking_config.insert("thinkingLevel".to_string(), value);
                } else if key == "include_thoughts"
                    && !thinking_config.contains_key("includeThoughts")
                {
                    thinking_config.insert("includeThoughts".to_string(), value);
                } else if !thinking_config.contains_key(&key) {
                    thinking_config.insert(key, value);
                }
            }
            changed = true;
        }

        let mut thinking_level = thinking_config
            .get("thinkingLevel")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        if thinking_level.is_none() {
            thinking_level = obj
                .get("thinking_level")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
        }
        if thinking_level.is_none() {
            thinking_level = mapped_thinking_level;
        }
        if thinking_level.is_none() {
            if let Some(default_level) = default_thinking_level(model_name) {
                obj.insert(
                    "thinking_level".to_string(),
                    Value::String(default_level.to_string()),
                );
                thinking_level = Some(default_level.to_string());
                changed = true;
            }
        }

        if let Some(level) = thinking_level {
            if !thinking_config.contains_key("thinkingLevel") {
                thinking_config.insert("thinkingLevel".to_string(), Value::String(level));
                changed = true;
            }
        }

        if let Some(include_thoughts) = mapped_include_thoughts {
            if !thinking_config.contains_key("includeThoughts") {
                thinking_config
                    .insert("includeThoughts".to_string(), Value::Bool(include_thoughts));
                changed = true;
            }
        }

        if !thinking_config.is_empty() {
            let existing = obj.get("thinkingConfig").and_then(|v| v.as_object());
            if existing.map(|v| v != &thinking_config).unwrap_or(true) {
                obj.insert("thinkingConfig".to_string(), Value::Object(thinking_config));
                changed = true;
            }
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
                            provider_fields.insert(
                                "thought_signature".to_string(),
                                Value::String(signature.clone()),
                            );
                            changed = true;
                        }
                        let google_fields = ensure_extra_content_google(tc_obj);
                        let google_current = google_fields
                            .get("thought_signature")
                            .and_then(|sig| sig.as_str())
                            .unwrap_or("");
                        if google_current != signature {
                            google_fields.insert(
                                "thought_signature".to_string(),
                                Value::String(signature.clone()),
                            );
                            changed = true;
                        }
                        if let Some(function) = tc_obj.get_mut("function") {
                            if let Some(fn_obj) = function.as_object_mut() {
                                let fn_provider_fields = ensure_provider_specific_fields(fn_obj);
                                let fn_current = fn_provider_fields
                                    .get("thought_signature")
                                    .and_then(|sig| sig.as_str())
                                    .unwrap_or("");
                                if fn_current != signature {
                                    fn_provider_fields.insert(
                                        "thought_signature".to_string(),
                                        Value::String(signature.clone()),
                                    );
                                    changed = true;
                                }
                            }
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

pub fn strip_gemini3_provider_fields(payload: &mut Value, model: Option<&str>) -> bool {
    if !matches!(model, Some(name) if is_gemini3_model(name)) {
        return false;
    }
    let Some(obj) = payload.as_object_mut() else {
        return false;
    };

    let mut changed = false;
    for key in ["thinkingConfig", "thinking_level", "thinking_config"] {
        if obj.remove(key).is_some() {
            changed = true;
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

            if apply_parts_thought_signatures(msg_obj) {
                changed = true;
            }

            if let Some(signature) = extra_signature {
                let provider_fields = ensure_provider_specific_fields(msg_obj);
                if merge_thought_signatures(provider_fields, vec![signature]) {
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
