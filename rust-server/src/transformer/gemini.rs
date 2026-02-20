//! Gemini protocol transformer.
//!
//! Handles conversion between Google Gemini (Vertex AI) API format
//! and the Unified Internal Format.

use super::{
    ChunkType, Protocol, Result, Role, StopReason, Transformer, UnifiedContent, UnifiedMessage,
    UnifiedParameters, UnifiedRequest, UnifiedResponse, UnifiedStreamChunk, UnifiedTool,
    UnifiedToolCall, UnifiedUsage,
};
use crate::core::AppError;
use bytes::Bytes;
use serde_json::{json, Value};
use std::sync::Mutex;

// ============================================================================
// Streaming State
// ============================================================================

#[derive(Default)]
struct GeminiStreamState {
    first_chunk_seen: bool,
    content_block_index: usize,
    active_text_block: bool,
}

// ============================================================================
// Gemini Transformer
// ============================================================================

pub struct GeminiTransformer {
    stream_state: Mutex<GeminiStreamState>,
}

impl GeminiTransformer {
    pub fn new() -> Self {
        Self {
            stream_state: Mutex::new(GeminiStreamState::default()),
        }
    }

    // -- Helpers: Part ↔ UnifiedContent --

    /// Convert a Gemini part to one or more UnifiedContent blocks.
    /// Returns a Vec because a part with `thoughtSignature` produces an extra Thinking block.
    fn part_to_unified_vec(part: &Value) -> Vec<UnifiedContent> {
        let mut result = Vec::new();
        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
            if part
                .get("thought")
                .and_then(|t| t.as_bool())
                .unwrap_or(false)
            {
                result.push(UnifiedContent::thinking(text, None));
                return result;
            }
            result.push(UnifiedContent::text(text));
            // Gemini attaches thoughtSignature to non-thought parts
            if let Some(sig) = part.get("thoughtSignature").and_then(|s| s.as_str()) {
                result.push(UnifiedContent::thinking("", Some(sig.to_string())));
            }
            return result;
        }
        if let Some(fc) = part.get("functionCall") {
            let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let args = fc.get("args").cloned().unwrap_or(json!({}));
            let id = format!("call_{}", uuid::Uuid::new_v4().simple());
            result.push(UnifiedContent::tool_use(id, name, args));
            if let Some(sig) = part.get("thoughtSignature").and_then(|s| s.as_str()) {
                result.push(UnifiedContent::thinking("", Some(sig.to_string())));
            }
            return result;
        }
        if let Some(fr) = part.get("functionResponse") {
            let name = fr.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let response = fr.get("response").cloned().unwrap_or(Value::Null);
            result.push(UnifiedContent::tool_result(name, response, false));
            return result;
        }
        if let Some(inline) = part.get("inlineData") {
            let mime = inline
                .get("mimeType")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            let data = inline.get("data").and_then(|d| d.as_str()).unwrap_or("");
            result.push(UnifiedContent::image_base64(mime, data));
            if let Some(sig) = part.get("thoughtSignature").and_then(|s| s.as_str()) {
                result.push(UnifiedContent::thinking("", Some(sig.to_string())));
            }
            return result;
        }
        result
    }

    /// Convert a sequence of UnifiedContent to Gemini parts,
    /// re-attaching thoughtSignature from Thinking(text="", sig=Some) to the preceding part.
    fn unified_contents_to_parts(contents: &[UnifiedContent]) -> Vec<Value> {
        let mut parts: Vec<Value> = Vec::new();
        for content in contents {
            match content {
                UnifiedContent::Thinking {
                    text,
                    signature: Some(sig),
                } if text.is_empty() => {
                    // This is a signature block — attach to the previous part
                    if let Some(last) = parts.last_mut() {
                        last["thoughtSignature"] = json!(sig);
                    }
                }
                _ => {
                    if let Some(part) = Self::unified_to_part(content) {
                        parts.push(part);
                    }
                }
            }
        }
        parts
    }

    fn unified_to_part(content: &UnifiedContent) -> Option<Value> {
        match content {
            UnifiedContent::Text { text } => Some(json!({"text": text})),
            UnifiedContent::Thinking { text, signature } => {
                if text.is_empty() && signature.is_some() {
                    // Signature-only block — handled by unified_contents_to_parts
                    None
                } else {
                    Some(json!({"thought": true, "text": text}))
                }
            }
            UnifiedContent::ToolUse { name, input, .. } => {
                Some(json!({"functionCall": {"name": name, "args": input}}))
            }
            UnifiedContent::ToolResult {
                tool_use_id,
                content,
                ..
            } => Some(json!({"functionResponse": {"name": tool_use_id, "response": content}})),
            UnifiedContent::Image {
                media_type, data, ..
            } => Some(json!({"inlineData": {"mimeType": media_type, "data": data}})),
            _ => None,
        }
    }

    // -- Helpers: Message conversion --

    fn gemini_content_to_unified(content: &Value) -> UnifiedMessage {
        let role_str = content
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("user");
        let role = match role_str {
            "model" => Role::Assistant,
            _ => Role::User,
        };

        let parts = content
            .get("parts")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let mut unified_content = Vec::new();
        let mut tool_calls = Vec::new();
        let mut tool_call_id = None;

        for part in &parts {
            for uc in Self::part_to_unified_vec(part) {
                match &uc {
                    UnifiedContent::ToolUse { id, name, input } => {
                        tool_calls.push(UnifiedToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: input.clone(),
                        });
                    }
                    UnifiedContent::ToolResult {
                        tool_use_id: tuid, ..
                    } => {
                        tool_call_id = Some(tuid.clone());
                    }
                    _ => {}
                }
                unified_content.push(uc);
            }
        }

        // If this is a user message that only contains functionResponse parts,
        // mark it as Tool role
        let effective_role = if role == Role::User
            && tool_call_id.is_some()
            && unified_content
                .iter()
                .all(|c| matches!(c, UnifiedContent::ToolResult { .. }))
        {
            Role::Tool
        } else {
            role
        };

        UnifiedMessage {
            role: effective_role,
            content: unified_content,
            name: None,
            tool_calls,
            tool_call_id,
        }
    }

    fn unified_to_gemini_content(msg: &UnifiedMessage, all_messages: &[UnifiedMessage]) -> Value {
        let role = match msg.role {
            Role::Assistant => "model",
            _ => "user",
        };

        // First pass: build raw parts (without signature attachment)
        let mut raw_parts: Vec<Value> = Vec::new();
        // Track which indices are signature-only Thinking blocks
        // so we can attach them to the preceding part
        let mut i = 0;
        while i < msg.content.len() {
            let content = &msg.content[i];
            // For ToolResult, we need to find the function name from previous messages
            if let UnifiedContent::ToolResult {
                tool_use_id,
                content: result_content,
                ..
            } = content
            {
                let fn_name = Self::find_function_name(tool_use_id, all_messages)
                    .unwrap_or_else(|| tool_use_id.clone());
                raw_parts.push(
                    json!({"functionResponse": {"name": fn_name, "response": result_content}}),
                );
            } else if let UnifiedContent::Thinking {
                text,
                signature: Some(sig),
            } = content
            {
                if text.is_empty() {
                    // Signature-only block — attach to previous part
                    if let Some(last) = raw_parts.last_mut() {
                        last["thoughtSignature"] = json!(sig);
                    }
                } else if let Some(part) = Self::unified_to_part(content) {
                    raw_parts.push(part);
                }
            } else if let Some(part) = Self::unified_to_part(content) {
                raw_parts.push(part);
            }
            i += 1;
        }

        // Append tool_calls not already in content
        let existing_tc_ids: std::collections::HashSet<&str> = msg
            .content
            .iter()
            .filter_map(|c| match c {
                UnifiedContent::ToolUse { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();

        for tc in &msg.tool_calls {
            if !existing_tc_ids.contains(tc.id.as_str()) {
                raw_parts.push(json!({"functionCall": {"name": tc.name, "args": tc.arguments}}));
            }
        }

        json!({"role": role, "parts": raw_parts})
    }

    fn find_function_name(tool_use_id: &str, messages: &[UnifiedMessage]) -> Option<String> {
        for msg in messages {
            for content in &msg.content {
                if let UnifiedContent::ToolUse { id, name, .. } = content {
                    if id == tool_use_id {
                        return Some(name.clone());
                    }
                }
            }
            for tc in &msg.tool_calls {
                if tc.id == tool_use_id {
                    return Some(tc.name.clone());
                }
            }
        }
        None
    }

    // -- Helpers: Stop reason --

    fn finish_reason_to_unified(reason: &str) -> StopReason {
        match reason {
            "STOP" => StopReason::EndTurn,
            "MAX_TOKENS" => StopReason::MaxTokens,
            "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" => {
                StopReason::ContentFilter
            }
            _ => StopReason::EndTurn,
        }
    }

    fn unified_to_finish_reason(reason: &StopReason) -> &'static str {
        match reason {
            StopReason::EndTurn => "STOP",
            StopReason::MaxTokens | StopReason::Length => "MAX_TOKENS",
            StopReason::ToolUse => "STOP",
            StopReason::StopSequence => "STOP",
            StopReason::ContentFilter => "SAFETY",
        }
    }

    // -- Helpers: Tool definitions --

    fn unified_tool_to_gemini(tool: &UnifiedTool) -> Value {
        let mut decl = json!({
            "name": tool.name,
            "parameters": tool.input_schema,
        });
        if let Some(ref desc) = tool.description {
            decl["description"] = json!(desc);
        }
        decl
    }

    fn gemini_tool_to_unified(decl: &Value) -> UnifiedTool {
        UnifiedTool {
            name: decl
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string(),
            description: decl
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from),
            input_schema: decl.get("parameters").cloned().unwrap_or(json!({})),
            tool_type: Some("function".to_string()),
        }
    }

    // -- Helpers: Usage --

    fn parse_usage(usage_meta: &Value) -> UnifiedUsage {
        UnifiedUsage {
            input_tokens: usage_meta
                .get("promptTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            output_tokens: usage_meta
                .get("candidatesTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32,
            cache_read_tokens: usage_meta
                .get("cachedContentTokenCount")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32),
            cache_write_tokens: None,
        }
    }
}

impl Default for GeminiTransformer {
    fn default() -> Self {
        Self::new()
    }
}

impl Transformer for GeminiTransformer {
    fn protocol(&self) -> Protocol {
        Protocol::Gemini
    }

    fn transform_request_out(&self, raw: Value) -> Result<UnifiedRequest> {
        // Gemini request → UIF
        let contents = raw
            .get("contents")
            .and_then(|c| c.as_array())
            .ok_or_else(|| AppError::BadRequest("Missing 'contents' field".into()))?;

        let messages: Vec<UnifiedMessage> = contents
            .iter()
            .map(Self::gemini_content_to_unified)
            .collect();

        // System instruction
        let system = raw
            .get("systemInstruction")
            .and_then(|si| si.get("parts"))
            .and_then(|p| p.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            });

        // Generation config
        let gen_config = raw.get("generationConfig").cloned().unwrap_or(json!({}));
        let parameters = UnifiedParameters {
            temperature: gen_config.get("temperature").and_then(|v| v.as_f64()),
            max_tokens: gen_config
                .get("maxOutputTokens")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32),
            top_p: gen_config.get("topP").and_then(|v| v.as_f64()),
            top_k: gen_config
                .get("topK")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32),
            stop_sequences: gen_config
                .get("stopSequences")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|s| s.as_str().map(String::from))
                        .collect()
                }),
            stream: false,
            extra: Default::default(),
        };

        // Tools
        let tools: Vec<UnifiedTool> = raw
            .get("tools")
            .and_then(|t| t.as_array())
            .map(|tool_groups| {
                tool_groups
                    .iter()
                    .flat_map(|group| {
                        group
                            .get("functionDeclarations")
                            .and_then(|fd| fd.as_array())
                            .map(|decls| {
                                decls
                                    .iter()
                                    .map(Self::gemini_tool_to_unified)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Tool choice
        let tool_choice = raw
            .get("toolConfig")
            .and_then(|tc| tc.get("functionCallingConfig"))
            .and_then(|fcc| {
                let mode = fcc.get("mode").and_then(|m| m.as_str())?;
                match mode {
                    "AUTO" => Some(json!({"type": "auto"})),
                    "ANY" => {
                        if let Some(names) =
                            fcc.get("allowedFunctionNames").and_then(|n| n.as_array())
                        {
                            if names.len() == 1 {
                                return Some(json!({"type": "tool", "name": names[0]}));
                            }
                        }
                        Some(json!({"type": "any"}))
                    }
                    "NONE" => Some(json!({"type": "none"})),
                    _ => None,
                }
            });

        // Model name (not in Gemini request body, use empty)
        let model = raw
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();

        Ok(UnifiedRequest {
            model,
            messages,
            system,
            parameters,
            tools,
            tool_choice,
            request_id: uuid::Uuid::new_v4().to_string(),
            client_protocol: Protocol::Gemini,
            metadata: Default::default(),
        })
    }

    fn transform_request_in(&self, unified: &UnifiedRequest) -> Result<Value> {
        // UIF → Gemini request
        let mut contents = Vec::new();

        // Merge consecutive same-role messages (Gemini requires alternating user/model)
        let mut pending_role: Option<&str> = None;
        let mut pending_parts: Vec<Value> = Vec::new();

        for msg in &unified.messages {
            let gemini_msg = Self::unified_to_gemini_content(msg, &unified.messages);
            let role = gemini_msg
                .get("role")
                .and_then(|r| r.as_str())
                .unwrap_or("user");
            let parts = gemini_msg
                .get("parts")
                .and_then(|p| p.as_array())
                .cloned()
                .unwrap_or_default();

            if pending_role == Some(role) {
                pending_parts.extend(parts);
            } else {
                if let Some(prev_role) = pending_role {
                    if !pending_parts.is_empty() {
                        contents.push(json!({"role": prev_role, "parts": pending_parts}));
                    }
                }
                pending_role = Some(if role == "model" { "model" } else { "user" });
                pending_parts = parts;
            }
        }
        if let Some(role) = pending_role {
            if !pending_parts.is_empty() {
                contents.push(json!({"role": role, "parts": pending_parts}));
            }
        }

        let mut request = json!({"contents": contents});

        // System instruction
        if let Some(ref system) = unified.system {
            request["systemInstruction"] = json!({"parts": [{"text": system}]});
        }

        // Generation config
        let mut gen_config = json!({});
        if let Some(temp) = unified.parameters.temperature {
            gen_config["temperature"] = json!(temp);
        }
        if let Some(max_tokens) = unified.parameters.max_tokens {
            gen_config["maxOutputTokens"] = json!(max_tokens);
        }
        if let Some(top_p) = unified.parameters.top_p {
            gen_config["topP"] = json!(top_p);
        }
        if let Some(top_k) = unified.parameters.top_k {
            gen_config["topK"] = json!(top_k);
        }
        if let Some(ref stop) = unified.parameters.stop_sequences {
            gen_config["stopSequences"] = json!(stop);
        }
        if gen_config
            .as_object()
            .map(|o| !o.is_empty())
            .unwrap_or(false)
        {
            request["generationConfig"] = gen_config;
        }

        // Tools
        if !unified.tools.is_empty() {
            let decls: Vec<Value> = unified
                .tools
                .iter()
                .map(Self::unified_tool_to_gemini)
                .collect();
            request["tools"] = json!([{"functionDeclarations": decls}]);
        }

        // Tool choice
        if let Some(ref tc) = unified.tool_choice {
            let mode = tc.get("type").and_then(|t| t.as_str()).unwrap_or("auto");
            let gemini_mode = match mode {
                "auto" => "AUTO",
                "any" | "required" => "ANY",
                "none" => "NONE",
                "tool" => "ANY",
                _ => "AUTO",
            };
            let mut fcc = json!({"mode": gemini_mode});
            if mode == "tool" {
                if let Some(name) = tc.get("name").and_then(|n| n.as_str()) {
                    fcc["allowedFunctionNames"] = json!([name]);
                }
            }
            request["toolConfig"] = json!({"functionCallingConfig": fcc});
        }

        Ok(request)
    }

    fn transform_response_in(&self, raw: Value, original_model: &str) -> Result<UnifiedResponse> {
        let candidates = raw
            .get("candidates")
            .and_then(|c| c.as_array())
            .ok_or_else(|| AppError::BadRequest("Missing 'candidates' field".into()))?;

        let candidate = candidates
            .first()
            .ok_or_else(|| AppError::BadRequest("Empty candidates array".into()))?;

        let parts = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let mut content = Vec::new();
        let mut tool_calls = Vec::new();

        for part in &parts {
            for uc in Self::part_to_unified_vec(part) {
                if let UnifiedContent::ToolUse { id, name, input } = &uc {
                    tool_calls.push(UnifiedToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: input.clone(),
                    });
                }
                content.push(uc);
            }
        }

        let stop_reason = candidate
            .get("finishReason")
            .and_then(|r| r.as_str())
            .map(Self::finish_reason_to_unified);

        let usage = raw
            .get("usageMetadata")
            .map(Self::parse_usage)
            .unwrap_or_default();

        let id = raw
            .get("responseId")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        Ok(UnifiedResponse {
            id: if id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                id
            },
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
        let mut parts = Self::unified_contents_to_parts(&unified.content);
        // Append tool_calls not in content
        for tc in &unified.tool_calls {
            parts.push(json!({"functionCall": {"name": tc.name, "args": tc.arguments}}));
        }

        let finish_reason = unified
            .stop_reason
            .as_ref()
            .map(Self::unified_to_finish_reason)
            .unwrap_or("STOP");

        let response = json!({
            "candidates": [{
                "content": {"role": "model", "parts": parts},
                "finishReason": finish_reason,
            }],
            "usageMetadata": {
                "promptTokenCount": unified.usage.input_tokens,
                "candidatesTokenCount": unified.usage.output_tokens,
                "totalTokenCount": unified.usage.input_tokens + unified.usage.output_tokens,
            },
            "modelVersion": unified.model,
        });

        Ok(response)
    }

    fn transform_stream_chunk_in(&self, chunk: &Bytes) -> Result<Vec<UnifiedStreamChunk>> {
        let chunk_str = std::str::from_utf8(chunk)
            .map_err(|e| AppError::BadRequest(format!("Invalid UTF-8: {}", e)))?;

        let mut chunks = vec![];

        for line in chunk_str.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            if let Some(data_str) = line.strip_prefix("data: ") {
                let data: Value = serde_json::from_str(data_str)
                    .map_err(|e| AppError::BadRequest(format!("Invalid JSON: {}", e)))?;

                let mut state = self.stream_state.lock().unwrap();

                // First chunk: emit MessageStart + ContentBlockStart
                if !state.first_chunk_seen {
                    state.first_chunk_seen = true;

                    let usage = data
                        .get("usageMetadata")
                        .map(Self::parse_usage)
                        .unwrap_or_default();

                    let model = data
                        .get("modelVersion")
                        .and_then(|m| m.as_str())
                        .unwrap_or("")
                        .to_string();

                    let msg = UnifiedResponse {
                        id: data
                            .get("responseId")
                            .and_then(|r| r.as_str())
                            .unwrap_or("")
                            .to_string(),
                        model,
                        content: vec![],
                        stop_reason: None,
                        usage,
                        tool_calls: vec![],
                    };
                    chunks.push(UnifiedStreamChunk::message_start(msg));

                    // Start a text content block
                    chunks.push(UnifiedStreamChunk::content_block_start(
                        0,
                        UnifiedContent::text(""),
                    ));
                    state.content_block_index = 1;
                    state.active_text_block = true;
                }

                // Extract parts from candidate
                let parts = data
                    .get("candidates")
                    .and_then(|c| c.as_array())
                    .and_then(|c| c.first())
                    .and_then(|c| c.get("content"))
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array());

                if let Some(parts) = parts {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            if part
                                .get("thought")
                                .and_then(|t| t.as_bool())
                                .unwrap_or(false)
                            {
                                // Thinking content (no signature on thought parts)
                                chunks.push(UnifiedStreamChunk::content_block_delta(
                                    0,
                                    UnifiedContent::thinking(text, None),
                                ));
                            } else {
                                chunks.push(UnifiedStreamChunk::content_block_delta(
                                    0,
                                    UnifiedContent::text(text),
                                ));
                                // Extract thoughtSignature from non-thought text parts
                                if let Some(sig) =
                                    part.get("thoughtSignature").and_then(|s| s.as_str())
                                {
                                    chunks.push(UnifiedStreamChunk::content_block_delta(
                                        0,
                                        UnifiedContent::thinking("", Some(sig.to_string())),
                                    ));
                                }
                            }
                        } else if let Some(fc) = part.get("functionCall") {
                            // Close text block if active, start tool block
                            if state.active_text_block {
                                chunks.push(UnifiedStreamChunk::content_block_stop(0));
                                state.active_text_block = false;
                            }
                            let name = fc
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("")
                                .to_string();
                            let args = fc.get("args").cloned().unwrap_or(json!({}));
                            let id = format!("call_{}", uuid::Uuid::new_v4().simple());
                            let idx = state.content_block_index;
                            state.content_block_index += 1;

                            chunks.push(UnifiedStreamChunk::content_block_start(
                                idx,
                                UnifiedContent::tool_use(&id, &name, args.clone()),
                            ));
                            // Emit full args as single delta
                            let args_str = serde_json::to_string(&args).unwrap_or_default();
                            chunks.push(UnifiedStreamChunk::content_block_delta(
                                idx,
                                UnifiedContent::tool_input_delta(idx, args_str),
                            ));
                            // Extract thoughtSignature from functionCall parts
                            if let Some(sig) = part.get("thoughtSignature").and_then(|s| s.as_str())
                            {
                                chunks.push(UnifiedStreamChunk::content_block_delta(
                                    idx,
                                    UnifiedContent::thinking("", Some(sig.to_string())),
                                ));
                            }
                            chunks.push(UnifiedStreamChunk::content_block_stop(idx));
                        }
                    }
                }

                // Check for finish
                let finish_reason = data
                    .get("candidates")
                    .and_then(|c| c.as_array())
                    .and_then(|c| c.first())
                    .and_then(|c| c.get("finishReason"))
                    .and_then(|r| r.as_str());

                if let Some(reason) = finish_reason {
                    if state.active_text_block {
                        chunks.push(UnifiedStreamChunk::content_block_stop(0));
                        state.active_text_block = false;
                    }

                    let usage = data
                        .get("usageMetadata")
                        .map(Self::parse_usage)
                        .unwrap_or_default();

                    chunks.push(UnifiedStreamChunk::message_delta(
                        Self::finish_reason_to_unified(reason),
                        usage,
                    ));
                    chunks.push(UnifiedStreamChunk::message_stop());
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
        match chunk.chunk_type {
            ChunkType::MessageStart => {
                // Gemini doesn't have a separate message_start event;
                // first data chunk serves this purpose. Skip.
                Ok(String::new())
            }
            ChunkType::ContentBlockStart => {
                // Gemini doesn't emit separate content block starts
                Ok(String::new())
            }
            ChunkType::ContentBlockDelta => {
                if let Some(ref delta) = chunk.delta {
                    let part = match delta {
                        UnifiedContent::Text { text } => json!({"text": text}),
                        UnifiedContent::Thinking { text, signature } => {
                            if text.is_empty() && signature.is_some() {
                                // Signature-only: emit as thoughtSignature on an empty text part
                                json!({"text": "", "thoughtSignature": signature.as_ref().unwrap()})
                            } else {
                                json!({"thought": true, "text": text})
                            }
                        }
                        UnifiedContent::ToolInputDelta { partial_json, .. } => {
                            // Re-parse as function call args
                            if let Ok(args) = serde_json::from_str::<Value>(partial_json) {
                                json!({"functionCall": {"name": "", "args": args}})
                            } else {
                                return Ok(String::new());
                            }
                        }
                        _ => return Ok(String::new()),
                    };
                    let event = json!({
                        "candidates": [{"content": {"role": "model", "parts": [part]}}]
                    });
                    Ok(format!("data: {}\n\n", event))
                } else {
                    Ok(String::new())
                }
            }
            ChunkType::ContentBlockStop => Ok(String::new()),
            ChunkType::MessageDelta => {
                let finish_reason = chunk
                    .stop_reason
                    .as_ref()
                    .map(Self::unified_to_finish_reason)
                    .unwrap_or("STOP");
                let usage = chunk.usage.as_ref().map(|u| {
                    json!({
                        "promptTokenCount": u.input_tokens,
                        "candidatesTokenCount": u.output_tokens,
                        "totalTokenCount": u.input_tokens + u.output_tokens,
                    })
                });
                let event = json!({
                    "candidates": [{"content": {"role": "model", "parts": []}, "finishReason": finish_reason}],
                    "usageMetadata": usage,
                });
                Ok(format!("data: {}\n\n", event))
            }
            ChunkType::MessageStop => Ok(String::new()),
            ChunkType::Ping => Ok(String::new()),
        }
    }

    fn endpoint(&self) -> &'static str {
        "/v1/projects"
    }

    fn can_handle(&self, raw: &Value) -> bool {
        // Gemini requests have "contents" (not "messages") and optionally "generationConfig"
        raw.get("contents").is_some()
            || (raw.get("generationConfig").is_some() && raw.get("messages").is_none())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol() {
        let t = GeminiTransformer::new();
        assert_eq!(t.protocol(), Protocol::Gemini);
    }

    #[test]
    fn test_can_handle() {
        let t = GeminiTransformer::new();

        assert!(t.can_handle(&json!({"contents": [{"role": "user", "parts": [{"text": "hi"}]}]})));
        assert!(!t.can_handle(&json!({"messages": [{"role": "user", "content": "hi"}]})));
        assert!(!t.can_handle(&json!({"model": "gpt-4"})));
    }

    #[test]
    fn test_transform_request_out_basic() {
        let t = GeminiTransformer::new();
        let raw = json!({
            "contents": [
                {"role": "user", "parts": [{"text": "Hello"}]},
                {"role": "model", "parts": [{"text": "Hi there!"}]},
                {"role": "user", "parts": [{"text": "How are you?"}]}
            ],
            "systemInstruction": {"parts": [{"text": "Be helpful."}]},
            "generationConfig": {
                "temperature": 0.7,
                "maxOutputTokens": 1024,
                "topP": 0.9,
                "topK": 40,
                "stopSequences": ["END"]
            }
        });

        let unified = t.transform_request_out(raw).unwrap();
        assert_eq!(unified.messages.len(), 3);
        assert_eq!(unified.system, Some("Be helpful.".to_string()));
        assert_eq!(unified.parameters.temperature, Some(0.7));
        assert_eq!(unified.parameters.max_tokens, Some(1024));
        assert_eq!(unified.parameters.top_p, Some(0.9));
        assert_eq!(unified.parameters.top_k, Some(40));
        assert_eq!(
            unified.parameters.stop_sequences,
            Some(vec!["END".to_string()])
        );
        assert_eq!(unified.messages[0].role, Role::User);
        assert_eq!(unified.messages[1].role, Role::Assistant);
    }

    #[test]
    fn test_transform_request_in_basic() {
        let t = GeminiTransformer::new();
        let unified = UnifiedRequest::new("gemini-pro", vec![UnifiedMessage::user("Hello")])
            .with_system("Be helpful")
            .with_max_tokens(1024);

        let raw = t.transform_request_in(&unified).unwrap();
        assert!(raw.get("contents").is_some());
        assert!(raw.get("systemInstruction").is_some());
        assert_eq!(raw["systemInstruction"]["parts"][0]["text"], "Be helpful");
        assert_eq!(raw["generationConfig"]["maxOutputTokens"], 1024);
    }

    #[test]
    fn test_transform_response_in() {
        let t = GeminiTransformer::new();
        let raw = json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "Hello!"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "totalTokenCount": 15
            },
            "responseId": "resp_123"
        });

        let unified = t.transform_response_in(raw, "gemini-pro").unwrap();
        assert_eq!(unified.id, "resp_123");
        assert_eq!(unified.text_content(), "Hello!");
        assert_eq!(unified.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(unified.usage.input_tokens, 10);
        assert_eq!(unified.usage.output_tokens, 5);
    }

    #[test]
    fn test_transform_response_out() {
        let t = GeminiTransformer::new();
        let unified =
            UnifiedResponse::text("resp_123", "gemini-pro", "Hello!", UnifiedUsage::new(10, 5));

        let raw = t
            .transform_response_out(&unified, Protocol::Gemini)
            .unwrap();
        assert!(raw.get("candidates").is_some());
        let parts = &raw["candidates"][0]["content"]["parts"];
        assert_eq!(parts[0]["text"], "Hello!");
        assert_eq!(raw["candidates"][0]["finishReason"], "STOP");
    }

    #[test]
    fn test_transform_request_with_tools() {
        let t = GeminiTransformer::new();
        let raw = json!({
            "contents": [{"role": "user", "parts": [{"text": "What's the weather?"}]}],
            "tools": [{"functionDeclarations": [{
                "name": "get_weather",
                "description": "Get weather",
                "parameters": {"type": "OBJECT", "properties": {"city": {"type": "STRING"}}}
            }]}],
            "toolConfig": {"functionCallingConfig": {"mode": "AUTO"}}
        });

        let unified = t.transform_request_out(raw).unwrap();
        assert_eq!(unified.tools.len(), 1);
        assert_eq!(unified.tools[0].name, "get_weather");
        assert_eq!(unified.tool_choice, Some(json!({"type": "auto"})));
    }

    #[test]
    fn test_transform_request_in_with_tools() {
        let t = GeminiTransformer::new();
        let mut unified = UnifiedRequest::new("gemini-pro", vec![UnifiedMessage::user("Weather?")]);
        unified.tools = vec![UnifiedTool {
            name: "get_weather".to_string(),
            description: Some("Get weather".to_string()),
            input_schema: json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            tool_type: Some("function".to_string()),
        }];
        unified.tool_choice = Some(json!({"type": "any"}));

        let raw = t.transform_request_in(&unified).unwrap();
        let tools = raw["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        let decls = tools[0]["functionDeclarations"].as_array().unwrap();
        assert_eq!(decls[0]["name"], "get_weather");
        assert_eq!(raw["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
    }

    #[test]
    fn test_function_call_response() {
        let t = GeminiTransformer::new();
        let raw = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{"functionCall": {"name": "get_weather", "args": {"city": "SF"}}}]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 5}
        });

        let unified = t.transform_response_in(raw, "gemini-pro").unwrap();
        assert_eq!(unified.tool_calls.len(), 1);
        assert_eq!(unified.tool_calls[0].name, "get_weather");
    }

    #[test]
    fn test_thinking_content() {
        let t = GeminiTransformer::new();
        let raw = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        {"thought": true, "text": "Let me think..."},
                        {"text": "The answer is 42."}
                    ]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 20}
        });

        let unified = t.transform_response_in(raw, "gemini-pro").unwrap();
        assert_eq!(unified.content.len(), 2);
        assert!(
            matches!(&unified.content[0], UnifiedContent::Thinking { text, .. } if text == "Let me think...")
        );
        assert!(
            matches!(&unified.content[1], UnifiedContent::Text { text } if text == "The answer is 42.")
        );
    }

    #[test]
    fn test_thought_signature_response_in() {
        let t = GeminiTransformer::new();
        let raw = json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        {"thought": true, "text": "Let me think..."},
                        {"text": "The answer is 42.", "thoughtSignature": "sig_abc123"}
                    ]
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 10, "candidatesTokenCount": 20}
        });

        let unified = t.transform_response_in(raw, "gemini-pro").unwrap();
        // thought part + text part + signature block = 3
        assert_eq!(unified.content.len(), 3);
        assert!(
            matches!(&unified.content[0], UnifiedContent::Thinking { text, signature } if text == "Let me think..." && signature.is_none())
        );
        assert!(
            matches!(&unified.content[1], UnifiedContent::Text { text } if text == "The answer is 42.")
        );
        assert!(
            matches!(&unified.content[2], UnifiedContent::Thinking { text, signature } if text.is_empty() && signature.as_deref() == Some("sig_abc123"))
        );
    }

    #[test]
    fn test_thought_signature_roundtrip() {
        let t = GeminiTransformer::new();
        // UIF with thinking + text + signature
        let unified = UnifiedResponse {
            id: "resp_123".to_string(),
            model: "gemini-pro".to_string(),
            content: vec![
                UnifiedContent::thinking("Let me think...", None),
                UnifiedContent::text("Answer"),
                UnifiedContent::thinking("", Some("sig_xyz".to_string())),
            ],
            stop_reason: Some(StopReason::EndTurn),
            usage: UnifiedUsage::new(10, 20),
            tool_calls: vec![],
        };

        let raw = t
            .transform_response_out(&unified, Protocol::Gemini)
            .unwrap();
        let parts = raw["candidates"][0]["content"]["parts"].as_array().unwrap();
        // Should have 2 Gemini parts: thought + text-with-signature
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["thought"], true);
        assert_eq!(parts[0]["text"], "Let me think...");
        assert!(parts[0].get("thoughtSignature").is_none());
        assert_eq!(parts[1]["text"], "Answer");
        assert_eq!(parts[1]["thoughtSignature"], "sig_xyz");
    }

    #[test]
    fn test_thought_signature_streaming_in() {
        let t = GeminiTransformer::new();

        // First chunk with thought
        let chunk1 = Bytes::from(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"thought\":true,\"text\":\"thinking...\"}]}}],\"modelVersion\":\"gemini-pro\",\"responseId\":\"r1\"}\n\n"
        );
        let result1 = t.transform_stream_chunk_in(&chunk1).unwrap();
        // MessageStart + ContentBlockStart + thinking delta
        assert!(result1.len() >= 3);

        // Second chunk with text + thoughtSignature
        let chunk2 = Bytes::from(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"answer\",\"thoughtSignature\":\"sig_stream\"}]}}]}\n\n"
        );
        let result2 = t.transform_stream_chunk_in(&chunk2).unwrap();
        // text delta + signature delta = 2
        assert_eq!(result2.len(), 2);
        assert!(
            matches!(&result2[0].delta, Some(UnifiedContent::Text { text }) if text == "answer")
        );
        assert!(
            matches!(&result2[1].delta, Some(UnifiedContent::Thinking { text, signature }) if text.is_empty() && signature.as_deref() == Some("sig_stream"))
        );
    }

    #[test]
    fn test_streaming_basic() {
        let t = GeminiTransformer::new();

        // First chunk
        let chunk1 = Bytes::from(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hello\"}]}}],\"modelVersion\":\"gemini-pro\",\"responseId\":\"r1\"}\n\n"
        );
        let result1 = t.transform_stream_chunk_in(&chunk1).unwrap();
        // Should get: MessageStart + ContentBlockStart + ContentBlockDelta
        assert!(result1.len() >= 3);
        assert_eq!(result1[0].chunk_type, ChunkType::MessageStart);
        assert_eq!(result1[1].chunk_type, ChunkType::ContentBlockStart);
        assert_eq!(result1[2].chunk_type, ChunkType::ContentBlockDelta);

        // Middle chunk
        let chunk2 = Bytes::from(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\" World\"}]}}]}\n\n",
        );
        let result2 = t.transform_stream_chunk_in(&chunk2).unwrap();
        assert_eq!(result2.len(), 1);
        assert_eq!(result2[0].chunk_type, ChunkType::ContentBlockDelta);

        // Last chunk with finishReason
        let chunk3 = Bytes::from(
            "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"!\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":3}}\n\n",
        );
        let result3 = t.transform_stream_chunk_in(&chunk3).unwrap();
        // ContentBlockDelta + ContentBlockStop + MessageDelta + MessageStop
        assert!(result3
            .iter()
            .any(|c| c.chunk_type == ChunkType::MessageDelta));
        assert!(result3
            .iter()
            .any(|c| c.chunk_type == ChunkType::MessageStop));
    }

    #[test]
    fn test_finish_reason_mapping() {
        assert_eq!(
            GeminiTransformer::finish_reason_to_unified("STOP"),
            StopReason::EndTurn
        );
        assert_eq!(
            GeminiTransformer::finish_reason_to_unified("MAX_TOKENS"),
            StopReason::MaxTokens
        );
        assert_eq!(
            GeminiTransformer::finish_reason_to_unified("SAFETY"),
            StopReason::ContentFilter
        );
        assert_eq!(
            GeminiTransformer::finish_reason_to_unified("RECITATION"),
            StopReason::ContentFilter
        );
    }

    #[test]
    fn test_consecutive_same_role_merged() {
        let t = GeminiTransformer::new();
        let unified = UnifiedRequest::new(
            "gemini-pro",
            vec![
                UnifiedMessage::user("Hello"),
                UnifiedMessage::tool_result("call_1", json!("result"), false),
            ],
        );

        let raw = t.transform_request_in(&unified).unwrap();
        let contents = raw["contents"].as_array().unwrap();
        // Both should be merged into a single "user" content
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
    }

    #[test]
    fn test_cross_protocol_openai_to_gemini() {
        use super::super::openai::OpenAITransformer;

        let openai = OpenAITransformer::new();
        let gemini = GeminiTransformer::new();

        let request = json!({
            "model": "gemini-pro",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello!"}
            ],
            "temperature": 0.7,
            "max_tokens": 1024
        });

        let unified = openai.transform_request_out(request).unwrap();
        let gemini_req = gemini.transform_request_in(&unified).unwrap();

        assert!(gemini_req.get("contents").is_some());
        assert_eq!(
            gemini_req["systemInstruction"]["parts"][0]["text"],
            "You are helpful."
        );
        assert_eq!(gemini_req["generationConfig"]["maxOutputTokens"], 1024);
        assert_eq!(gemini_req["generationConfig"]["temperature"], 0.7);
    }
}
