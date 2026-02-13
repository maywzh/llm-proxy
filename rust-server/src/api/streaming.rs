//! Server-Sent Events (SSE) streaming support for chat completions.
//!
//! This module handles streaming responses from LLM providers, including
//! model name rewriting and token usage tracking.
//!
//! Token counting is unified using `OutboundTokenCounter`:
//! - Input tokens are pre-calculated at inbound (passed to this module)
//! - Output tokens are accumulated during streaming
//! - Final usage is calculated at outbound (finalize) with provider usage priority
//!
//! Tokenizer selection:
//! - tiktoken for OpenAI models (default)
//! - HuggingFace tokenizers for Claude/Cohere/Llama models (when available)

use crate::api::gemini3::normalize_response_payload;
use crate::api::models::Usage;
use crate::core::error::AppError;
use crate::core::error_types::{
    ERROR_CODE_PROVIDER, ERROR_CODE_TTFT_TIMEOUT, ERROR_TYPE_STREAM, ERROR_TYPE_TIMEOUT,
};
use crate::core::jsonl_logger::{log_provider_streaming_response, log_streaming_response};
use crate::core::langfuse::{finish_generation_if_sampled, GenerationData};
use crate::core::logging::get_api_key_name;
use crate::core::metrics::get_metrics;
use crate::core::stream_metrics::{record_stream_metrics, StreamStats};
use crate::core::tokenizer::{count_tokens_hf, get_hf_tokenizer, select_tokenizer, TokenizerType};
use crate::core::OutboundTokenCounter;
use crate::core::StreamCancelHandle;
use crate::transformer::unified::UnifiedUsage;
use axum::body::Body;
use axum::response::Response as AxumResponse;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use bytes::Bytes;
use chrono::Utc;
use dashmap::DashMap;
use futures::stream::{Stream, StreamExt};
use reqwest::Response;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::select;
use tokio::sync::watch;

// Global cache for BPE encoders to avoid repeated initialization
// Uses DashMap for lock-free reads and fine-grained locking on writes
lazy_static::lazy_static! {
    static ref BPE_CACHE: DashMap<String, Arc<tiktoken_rs::CoreBPE>> = DashMap::new();
}

const DEFAULT_IMAGE_TOKEN_COUNT: u32 = 250;
const DEFAULT_IMAGE_WIDTH: u32 = 300;
const DEFAULT_IMAGE_HEIGHT: u32 = 300;
const MAX_SHORT_SIDE_FOR_IMAGE_HIGH_RES: u32 = 768;
const MAX_LONG_SIDE_FOR_IMAGE_HIGH_RES: u32 = 2000;
const MAX_TILE_WIDTH: u32 = 512;
const MAX_TILE_HEIGHT: u32 = 512;

type TokenCountResult<T> = Result<T, String>;

/// Get or create a cached BPE encoder for the given model
fn get_cached_bpe(model: &str) -> Option<Arc<tiktoken_rs::CoreBPE>> {
    let normalized = normalize_model_name(model);
    let cache_key = normalized.to_string();

    // Try to read from cache first (lock-free read via DashMap)
    if let Some(bpe) = BPE_CACHE.get(&cache_key) {
        return Some(Arc::clone(&bpe));
    }

    let normalized_lower = normalized.to_lowercase();
    let bpe = if normalized_lower.contains("gpt-4o") {
        tiktoken_rs::o200k_base()
    } else {
        tiktoken_rs::get_bpe_from_model(normalized.as_ref()).or_else(|_| tiktoken_rs::cl100k_base())
    }
    .ok()?;

    let bpe_arc = Arc::new(bpe);

    // Store in cache (fine-grained locking via DashMap)
    BPE_CACHE.insert(cache_key, Arc::clone(&bpe_arc));

    Some(bpe_arc)
}

fn normalize_model_name(model: &str) -> Cow<'_, str> {
    if model.contains("gpt-35") {
        Cow::Owned(model.replace("-35", "-3.5"))
    } else if model.starts_with("gpt-") {
        Cow::Borrowed(model)
    } else {
        Cow::Borrowed("gpt-3.5-turbo")
    }
}

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|val| val.parse::<u32>().ok())
        .unwrap_or(default)
}

/// Stream state for token counting - NO synchronization needed!
/// Each stream has its own state, processed sequentially
struct StreamState {
    stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    /// Outbound token counter for unified usage calculation
    token_counter: OutboundTokenCounter,
    usage_found: bool,
    usage_chunk_sent: bool, // Track if fallback usage chunk has been sent
    start_time: Instant,
    provider_first_token_time: Option<Instant>,
    original_model: String,
    gemini_model: Option<String>,
    provider_name: String,
    api_key_name: String,
    client: String,
    /// TTFT timeout in seconds (None = disabled)
    ttft_timeout_secs: Option<u64>,
    /// Whether the first chunk has been received
    first_chunk_received: bool,
    /// Langfuse generation data (None if Langfuse disabled or not sampled)
    generation_data: Option<GenerationData>,
    /// Accumulated output content for Langfuse (kept for backward compatibility)
    accumulated_output: Vec<String>,
    /// Finish reason from the stream
    finish_reason: Option<String>,
    /// Chunk counter for logging
    chunk_count: usize,
    /// Whether stream start has been logged
    stream_start_logged: bool,
    /// Accumulated raw SSE data strings for JSONL logging
    accumulated_sse_data: Vec<String>,
    /// Request ID for JSONL logging
    request_id: String,
    /// SSE line buffer for handling TCP fragmentation
    sse_line_buffer: String,
    /// Cancellation receiver
    cancel_rx: Option<watch::Receiver<bool>>,
}

impl StreamState {
    #[allow(clippy::too_many_arguments)]
    fn new(
        stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
        input_tokens: usize,
        original_model: String,
        gemini_model: Option<String>,
        provider_name: String,
        api_key_name: String,
        client: String,
        ttft_timeout_secs: Option<u64>,
        generation_data: Option<GenerationData>,
        cancel_rx: Option<watch::Receiver<bool>>,
    ) -> Self {
        // Create OutboundTokenCounter with pre-calculated input tokens
        let token_counter = OutboundTokenCounter::new(&original_model, input_tokens as i32);

        Self {
            stream: Box::pin(stream),
            token_counter,
            usage_found: false,
            usage_chunk_sent: false,
            start_time: Instant::now(),
            provider_first_token_time: None,
            original_model,
            gemini_model,
            provider_name,
            api_key_name,
            client,
            ttft_timeout_secs,
            first_chunk_received: false,
            generation_data,
            accumulated_output: Vec::new(),
            finish_reason: None,
            chunk_count: 0,
            stream_start_logged: false,
            accumulated_sse_data: Vec::new(),
            request_id: String::new(),
            sse_line_buffer: String::new(),
            cancel_rx,
        }
    }
}

#[derive(Clone, Copy)]
struct MessageCountParams {
    tokens_per_message: i64,
    tokens_per_name: i64,
}

fn get_message_count_params(model: &str) -> MessageCountParams {
    let normalized = normalize_model_name(model);
    if normalized.as_ref() == "gpt-3.5-turbo-0301" {
        MessageCountParams {
            tokens_per_message: 4,
            tokens_per_name: -1,
        }
    } else {
        MessageCountParams {
            tokens_per_message: 3,
            tokens_per_name: 1,
        }
    }
}

/// Calculate tokens for messages using LiteLLM-compatible logic.
pub fn calculate_message_tokens(messages: &[Value], model: &str) -> TokenCountResult<usize> {
    calculate_message_tokens_with_tools(messages, model, None, None)
}

/// Calculate tokens for messages, tools, and tool_choice using LiteLLM-compatible logic.
pub fn calculate_message_tokens_with_tools(
    messages: &[Value],
    model: &str,
    tools: Option<&[Value]>,
    tool_choice: Option<&Value>,
) -> TokenCountResult<usize> {
    calculate_message_tokens_internal(messages, model, tools, tool_choice, false, None)
}

fn calculate_message_tokens_internal(
    messages: &[Value],
    model: &str,
    tools: Option<&[Value]>,
    tool_choice: Option<&Value>,
    use_default_image_token_count: bool,
    default_token_count: Option<usize>,
) -> TokenCountResult<usize> {
    let message_tokens = count_messages(
        messages,
        model,
        use_default_image_token_count,
        default_token_count,
    )?;
    let includes_system_message = messages
        .iter()
        .any(|message| message.get("role").and_then(|role| role.as_str()) == Some("system"));
    let extra_tokens = count_extra(model, tools, tool_choice, includes_system_message);
    Ok(message_tokens + extra_tokens)
}

fn count_messages(
    messages: &[Value],
    model: &str,
    use_default_image_token_count: bool,
    default_token_count: Option<usize>,
) -> TokenCountResult<usize> {
    if messages.is_empty() {
        return Ok(0);
    }

    let params = get_message_count_params(model);
    let mut total_tokens: i64 = 0;

    for message in messages {
        total_tokens += params.tokens_per_message;
        let Some(message_obj) = message.as_object() else {
            continue;
        };

        for (key, value) in message_obj {
            if value.is_null() {
                continue;
            }

            if key == "tool_calls" {
                let Value::Array(tool_calls) = value else {
                    return fallback_or_err(
                        default_token_count,
                        "Unsupported type for tool_calls".to_string(),
                    );
                };
                for tool_call in tool_calls {
                    let Some(function_obj) = tool_call.get("function") else {
                        return fallback_or_err(
                            default_token_count,
                            "tool_calls must contain function".to_string(),
                        );
                    };
                    let args_value = function_obj.get("arguments").unwrap_or(&Value::Null);
                    let args_str = match args_value {
                        Value::String(text) => text.clone(),
                        _ => args_value.to_string(),
                    };
                    total_tokens += count_tokens(&args_str, model) as i64;
                }
                continue;
            }

            if let Value::String(text) = value {
                total_tokens += count_tokens(text, model) as i64;
                if key == "name" {
                    total_tokens += params.tokens_per_name;
                }
                continue;
            }

            if key == "content" {
                if let Value::Array(content_list) = value {
                    let content_tokens = count_content_list(
                        content_list,
                        model,
                        use_default_image_token_count,
                        default_token_count,
                    )?;
                    total_tokens += content_tokens as i64;
                }
                continue;
            }
        }
    }

    if total_tokens < 0 {
        total_tokens = 0;
    }

    Ok(total_tokens as usize)
}

fn count_content_list(
    content_list: &[Value],
    model: &str,
    use_default_image_token_count: bool,
    default_token_count: Option<usize>,
) -> TokenCountResult<usize> {
    let mut tokens = 0usize;

    for content in content_list {
        let content_tokens = match content {
            Value::String(text) => count_tokens(text, model),
            Value::Object(obj) => match obj.get("type").and_then(|t| t.as_str()) {
                Some("text") => obj
                    .get("text")
                    .and_then(|t| t.as_str())
                    .map(|text| count_tokens(text, model))
                    .unwrap_or(0),
                Some("image_url") => {
                    let image_url = obj.get("image_url").unwrap_or(&Value::Null);
                    count_image_tokens_value(image_url, use_default_image_token_count)?
                }
                Some("tool_use") | Some("tool_result") => count_anthropic_content(
                    obj,
                    model,
                    use_default_image_token_count,
                    default_token_count,
                )?,
                Some("thinking") => obj
                    .get("thinking")
                    .and_then(|t| t.as_str())
                    .map(|text| count_tokens(text, model))
                    .unwrap_or(0),
                _ => {
                    return fallback_or_err(
                        default_token_count,
                        format!("Invalid content item type: {}", content),
                    );
                }
            },
            _ => {
                return fallback_or_err(
                    default_token_count,
                    "Invalid content item type".to_string(),
                );
            }
        };

        tokens += content_tokens;
    }

    Ok(tokens)
}

fn count_anthropic_content(
    content: &serde_json::Map<String, Value>,
    model: &str,
    use_default_image_token_count: bool,
    default_token_count: Option<usize>,
) -> TokenCountResult<usize> {
    let content_type = content
        .get("type")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "Anthropic content missing required field: type".to_string())?;

    let fields_to_count: &[&str] = match content_type {
        "tool_use" => &["name", "input", "caller"],
        "tool_result" => &["content"],
        _ => {
            return fallback_or_err(
                default_token_count,
                format!("Unknown Anthropic content type: {}", content_type),
            );
        }
    };

    let mut tokens = 0usize;
    for field_name in fields_to_count {
        let Some(field_value) = content.get(*field_name) else {
            continue;
        };
        if field_value.is_null() {
            continue;
        }

        let field_tokens = match field_value {
            Value::String(text) => count_tokens(text, model),
            Value::Array(items) => count_content_list(
                items,
                model,
                use_default_image_token_count,
                default_token_count,
            )?,
            Value::Object(_) => count_tokens(&field_value.to_string(), model),
            _ => 0,
        };

        tokens += field_tokens;
    }

    Ok(tokens)
}

fn count_extra(
    model: &str,
    tools: Option<&[Value]>,
    tool_choice: Option<&Value>,
    includes_system_message: bool,
) -> usize {
    let mut tokens = 3;

    if let Some(tool_list) = tools {
        if !tool_list.is_empty() {
            tokens += calculate_tools_tokens(tool_list, model);
            tokens += 9;
            if includes_system_message {
                tokens = tokens.saturating_sub(4);
            }
        }
    }

    if let Some(choice) = tool_choice {
        match choice {
            Value::String(text) if text == "none" => {
                tokens += 1;
            }
            Value::Object(obj) => {
                tokens += 7;
                if let Some(name) = obj
                    .get("function")
                    .and_then(|func| func.get("name"))
                    .and_then(|n| n.as_str())
                {
                    tokens += count_tokens(name, model);
                }
            }
            _ => {}
        }
    }

    tokens
}

fn fallback_or_err(default_token_count: Option<usize>, err: String) -> TokenCountResult<usize> {
    if let Some(default_tokens) = default_token_count {
        Ok(default_tokens)
    } else {
        Err(err)
    }
}

/// Count tokens in text using the appropriate tokenizer for the model
///
/// This function selects the appropriate tokenizer based on the model name:
/// - HuggingFace tokenizers for Llama, Cohere, Mistral models
/// - tiktoken for OpenAI and other models (default)
pub fn count_tokens(text: &str, model: &str) -> usize {
    let selection = select_tokenizer(model);

    match selection.tokenizer_type {
        TokenizerType::HuggingFace => {
            if let Some(repo) = &selection.hf_repo {
                if let Some(tokenizer) = get_hf_tokenizer(repo) {
                    return count_tokens_hf(text, &tokenizer);
                }
            }
            // Fallback to tiktoken if HuggingFace tokenizer fails
            count_tokens_tiktoken(text, model)
        }
        TokenizerType::Tiktoken => count_tokens_tiktoken(text, model),
    }
}

/// Count tokens using tiktoken (BPE encoder)
fn count_tokens_tiktoken(text: &str, model: &str) -> usize {
    if let Some(bpe) = get_cached_bpe(model) {
        bpe.encode_with_special_tokens(text).len()
    } else {
        0
    }
}

/// Calculate tokens for tool definitions using LiteLLM-compatible formatting.
pub fn calculate_tools_tokens(tools: &[Value], model: &str) -> usize {
    if tools.is_empty() {
        return 0;
    }

    let tools_str = format_function_definitions(tools);
    count_tokens(&tools_str, model)
}

/// Calculate tokens for an image content block using LiteLLM-compatible logic.
pub fn calculate_image_tokens(image_url: &str, detail: &str) -> TokenCountResult<usize> {
    calculate_image_tokens_with_default(image_url, detail, false)
}

fn calculate_image_tokens_with_default(
    image_url: &str,
    detail: &str,
    use_default_image_token_count: bool,
) -> TokenCountResult<usize> {
    if use_default_image_token_count {
        let default_tokens = env_u32("DEFAULT_IMAGE_TOKEN_COUNT", DEFAULT_IMAGE_TOKEN_COUNT);
        return Ok(default_tokens as usize);
    }

    let base_tokens = 85u32;
    match detail {
        "low" | "auto" => Ok(base_tokens as usize),
        "high" => {
            let (width, height) = get_image_dimensions(image_url)?;
            let (resized_width, resized_height) = resize_image_high_res(width, height);
            let tiles_needed = calculate_tiles_needed(
                resized_width,
                resized_height,
                MAX_TILE_WIDTH,
                MAX_TILE_HEIGHT,
            );
            let tile_tokens = (base_tokens * 2) * tiles_needed;
            Ok((base_tokens + tile_tokens) as usize)
        }
        _ => Err(format!("Invalid detail value: {}", detail)),
    }
}

fn count_image_tokens_value(
    image_url: &Value,
    use_default_image_token_count: bool,
) -> TokenCountResult<usize> {
    match image_url {
        Value::Object(obj) => {
            let detail = obj.get("detail").and_then(|d| d.as_str()).unwrap_or("auto");
            if !matches!(detail, "low" | "high" | "auto") {
                return Err(format!("Invalid detail value: {}", detail));
            }
            let url = obj
                .get("url")
                .and_then(|u| u.as_str())
                .ok_or_else(|| "Missing required key 'url' in image_url".to_string())?;
            if url.trim().is_empty() {
                return Err("Empty image_url string is not valid.".to_string());
            }
            calculate_image_tokens_with_default(url, detail, use_default_image_token_count)
        }
        Value::String(url) => {
            if url.trim().is_empty() {
                return Err("Empty image_url string is not valid.".to_string());
            }
            calculate_image_tokens_with_default(url, "auto", use_default_image_token_count)
        }
        _ => Err("Invalid image_url type".to_string()),
    }
}

fn get_image_dimensions(data: &str) -> TokenCountResult<(u32, u32)> {
    let img_data = fetch_image_bytes(data)?;

    match get_image_type(&img_data) {
        Some("png") => {
            if img_data.len() >= 24 {
                let width =
                    u32::from_be_bytes([img_data[16], img_data[17], img_data[18], img_data[19]]);
                let height =
                    u32::from_be_bytes([img_data[20], img_data[21], img_data[22], img_data[23]]);
                return Ok((width, height));
            }
        }
        Some("gif") => {
            if img_data.len() >= 10 {
                let width = u16::from_le_bytes([img_data[6], img_data[7]]) as u32;
                let height = u16::from_le_bytes([img_data[8], img_data[9]]) as u32;
                return Ok((width, height));
            }
        }
        Some("jpeg") => {
            if let Some((width, height)) = parse_jpeg_dimensions(&img_data) {
                return Ok((width, height));
            }
        }
        Some("webp") => {
            if img_data.len() >= 30 {
                if &img_data[12..16] == b"VP8X" {
                    let width =
                        u32::from_le_bytes([img_data[24], img_data[25], img_data[26], 0]) + 1;
                    let height =
                        u32::from_le_bytes([img_data[27], img_data[28], img_data[29], 0]) + 1;
                    return Ok((width, height));
                } else if &img_data[12..16] == b"VP8 " {
                    let width = u16::from_le_bytes([img_data[26], img_data[27]]) & 0x3FFF;
                    let height = u16::from_le_bytes([img_data[28], img_data[29]]) & 0x3FFF;
                    return Ok((width as u32, height as u32));
                } else if &img_data[12..16] == b"VP8L" {
                    let bits = u32::from_le_bytes([
                        img_data[21],
                        img_data[22],
                        img_data[23],
                        img_data[24],
                    ]);
                    let width = (bits & 0x3FFF) + 1;
                    let height = ((bits >> 14) & 0x3FFF) + 1;
                    return Ok((width, height));
                }
            }
        }
        _ => {}
    }

    Ok((default_image_width(), default_image_height()))
}

fn fetch_image_bytes(data: &str) -> TokenCountResult<Vec<u8>> {
    if let Ok(response) = reqwest::blocking::get(data) {
        if let Ok(bytes) = response.bytes() {
            return Ok(bytes.to_vec());
        }
    }

    let (_, encoded) = data
        .split_once(',')
        .ok_or_else(|| "Invalid image data URI".to_string())?;
    BASE64_STANDARD
        .decode(encoded)
        .map_err(|err| format!("Invalid base64 image data: {}", err))
}

fn get_image_type(image_data: &[u8]) -> Option<&'static str> {
    if image_data.len() >= 8 && image_data[..8] == [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
    {
        return Some("png");
    }
    if image_data.len() >= 6 && &image_data[..4] == b"GIF8" && image_data[5] == b'a' {
        return Some("gif");
    }
    if image_data.len() >= 3 && image_data[..3] == [0xff, 0xd8, 0xff] {
        return Some("jpeg");
    }
    if image_data.len() >= 8 && &image_data[4..8] == b"ftyp" {
        return Some("heic");
    }
    if image_data.len() >= 12 && &image_data[..4] == b"RIFF" && &image_data[8..12] == b"WEBP" {
        return Some("webp");
    }
    None
}

fn parse_jpeg_dimensions(image_data: &[u8]) -> Option<(u32, u32)> {
    if image_data.len() < 4 || image_data[0] != 0xFF || image_data[1] != 0xD8 {
        return None;
    }

    let mut index = 2usize;
    while index + 9 < image_data.len() {
        if image_data[index] != 0xFF {
            index += 1;
            continue;
        }
        let mut marker = image_data[index + 1];
        while marker == 0xFF {
            index += 1;
            if index + 1 >= image_data.len() {
                return None;
            }
            marker = image_data[index + 1];
        }
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            if index + 8 >= image_data.len() {
                return None;
            }
            let height = u16::from_be_bytes([image_data[index + 5], image_data[index + 6]]) as u32;
            let width = u16::from_be_bytes([image_data[index + 7], image_data[index + 8]]) as u32;
            return Some((width, height));
        }

        if index + 3 >= image_data.len() {
            return None;
        }
        let size = u16::from_be_bytes([image_data[index + 2], image_data[index + 3]]) as usize;
        if size < 2 {
            return None;
        }
        index += size + 2;
    }

    None
}

fn resize_image_high_res(width: u32, height: u32) -> (u32, u32) {
    let max_short_side = env_u32(
        "MAX_SHORT_SIDE_FOR_IMAGE_HIGH_RES",
        MAX_SHORT_SIDE_FOR_IMAGE_HIGH_RES,
    );
    let max_long_side = env_u32(
        "MAX_LONG_SIDE_FOR_IMAGE_HIGH_RES",
        MAX_LONG_SIDE_FOR_IMAGE_HIGH_RES,
    );

    if width <= max_short_side && height <= max_short_side {
        return (width, height);
    }

    let longer_side = width.max(height) as f64;
    let shorter_side = width.min(height) as f64;
    let aspect_ratio = if shorter_side > 0.0 {
        longer_side / shorter_side
    } else {
        1.0
    };

    if width <= height {
        let mut resized_width = max_short_side as f64;
        let mut resized_height = resized_width * aspect_ratio;
        if resized_height > max_long_side as f64 {
            resized_height = max_long_side as f64;
            resized_width = resized_height / aspect_ratio;
        }
        (resized_width as u32, resized_height as u32)
    } else {
        let mut resized_height = max_short_side as f64;
        let mut resized_width = resized_height * aspect_ratio;
        if resized_width > max_long_side as f64 {
            resized_width = max_long_side as f64;
            resized_height = resized_width / aspect_ratio;
        }
        (resized_width as u32, resized_height as u32)
    }
}

fn calculate_tiles_needed(
    resized_width: u32,
    resized_height: u32,
    tile_width: u32,
    tile_height: u32,
) -> u32 {
    let tiles_across = resized_width.div_ceil(tile_width);
    let tiles_down = resized_height.div_ceil(tile_height);
    tiles_across * tiles_down
}

fn default_image_width() -> u32 {
    env_u32("DEFAULT_IMAGE_WIDTH", DEFAULT_IMAGE_WIDTH)
}

fn default_image_height() -> u32 {
    env_u32("DEFAULT_IMAGE_HEIGHT", DEFAULT_IMAGE_HEIGHT)
}

fn format_function_definitions(tools: &[Value]) -> String {
    let mut lines = Vec::new();
    lines.push("namespace functions {".to_string());
    lines.push(String::new());

    for tool in tools {
        let Some(function) = tool.get("function") else {
            continue;
        };

        if let Some(description) = function.get("description").and_then(|d| d.as_str()) {
            lines.push(format!("// {}", description));
        }

        let function_name = function
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");
        let parameters = function.get("parameters").and_then(|p| p.as_object());
        let properties = parameters
            .and_then(|p| p.get("properties"))
            .and_then(|p| p.as_object());

        if let Some(properties) = properties {
            if !properties.is_empty() {
                lines.push(format!("type {} = (_: {{", function_name));
                lines.push(format_object_parameters(parameters, 0));
                lines.push("}) => any;".to_string());
            } else {
                lines.push(format!("type {} = () => any;", function_name));
            }
        } else {
            lines.push(format!("type {} = () => any;", function_name));
        }

        lines.push(String::new());
    }

    lines.push("} // namespace functions".to_string());
    lines.join("\n")
}

fn format_object_parameters(
    parameters: Option<&serde_json::Map<String, Value>>,
    indent: usize,
) -> String {
    let Some(params) = parameters else {
        return String::new();
    };
    let Some(properties) = params.get("properties").and_then(|p| p.as_object()) else {
        return String::new();
    };
    if properties.is_empty() {
        return String::new();
    }

    let required_params = params
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();

    let mut lines = Vec::new();
    for (key, props) in properties {
        if let Some(description) = props.get("description").and_then(|d| d.as_str()) {
            lines.push(format!("// {}", description));
        }
        let optional = if required_params.contains(&key.as_str()) {
            ""
        } else {
            "?"
        };
        let prop_type = format_type(props, indent);
        lines.push(format!("{}{}: {},", key, optional, prop_type));
    }

    lines
        .into_iter()
        .map(|line| format!("{}{}", " ".repeat(indent), line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_type(props: &Value, indent: usize) -> String {
    let type_value = props.get("type").and_then(|t| t.as_str());
    match type_value {
        Some("string") => {
            if let Some(enum_values) = props.get("enum").and_then(|e| e.as_array()) {
                return enum_values
                    .iter()
                    .map(|item| format!("\"{}\"", item))
                    .collect::<Vec<_>>()
                    .join(" | ");
            }
            "string".to_string()
        }
        Some("array") => {
            let items = props.get("items").unwrap_or(&Value::Null);
            format!("{}[]", format_type(items, indent))
        }
        Some("object") => {
            let nested = format_object_parameters(props.as_object(), indent + 2);
            format!("{{\n{}\n}}", nested)
        }
        Some("integer") | Some("number") => {
            if let Some(enum_values) = props.get("enum").and_then(|e| e.as_array()) {
                return enum_values
                    .iter()
                    .map(|item| format!("\"{}\"", item))
                    .collect::<Vec<_>>()
                    .join(" | ");
            }
            "number".to_string()
        }
        Some("boolean") => "boolean".to_string(),
        Some("null") => "null".to_string(),
        _ => "any".to_string(),
    }
}

/// Create an SSE stream from a provider response with token counting and optional TTFT timeout.
///
/// This function establishes the downstream connection IMMEDIATELY and handles TTFT timeout
/// inside the stream itself. This matches Python's behavior where the connection is established
/// first, then streaming begins.
///
/// Uses `unfold` to maintain state sequentially - NO synchronization needed!
/// Each stream processes chunks sequentially, so state is local and lock-free.
///
/// Reads api_key_name from context (set by API_KEY_NAME.scope() in handlers).
///
/// # Arguments
///
/// * `response` - HTTP response from the provider
/// * `original_model` - Model name from the original request
/// * `provider_name` - Name of the provider for metrics
/// * `gemini_model` - Model name for Gemini 3 normalization
/// * `input_tokens` - Optional input token count for fallback calculation
/// * `ttft_timeout_secs` - Optional TTFT timeout in seconds
/// * `generation_data` - Optional Langfuse generation data for tracing
/// * `request_id` - Optional request ID for JSONL logging
/// * `endpoint` - Optional endpoint for JSONL logging
/// * `request_payload` - Optional request payload for JSONL logging
/// * `client` - Optional client name from User-Agent header
#[allow(clippy::too_many_arguments)]
pub async fn create_sse_stream(
    response: Response,
    original_model: String,
    provider_name: String,
    gemini_model: Option<String>,
    input_tokens: Option<usize>,
    ttft_timeout_secs: Option<u64>,
    generation_data: Option<GenerationData>,
    request_id: Option<String>,
    endpoint: Option<String>,
    request_payload: Option<Value>,
    client: Option<String>,
    cancel_handle: Option<StreamCancelHandle>,
) -> Result<AxumResponse, AppError> {
    let stream = response.bytes_stream();

    // Get api_key_name from context
    let api_key_name = get_api_key_name();
    let client_name = client.unwrap_or_else(|| "unknown".to_string());

    // Get cancellation receiver if handle provided
    let cancel_rx = cancel_handle.map(|h| h.subscribe());

    // Create initial state with TTFT timeout config - connection established immediately!
    // TTFT timeout is handled inside the stream, not before returning the response.
    let mut initial_state = StreamState::new(
        stream,
        input_tokens.unwrap_or(0),
        original_model,
        gemini_model,
        provider_name,
        api_key_name,
        client_name,
        ttft_timeout_secs,
        generation_data,
        cancel_rx,
    );

    // Set JSONL logging parameters (endpoint and request_payload are no longer needed - request is logged separately)
    initial_state.request_id = request_id.unwrap_or_default();
    // Note: endpoint and request_payload parameters are kept for API compatibility but not used
    let _ = endpoint;
    let _ = request_payload;

    // Create the byte stream using unfold - TTFT timeout handled inside
    let byte_stream = futures::stream::unfold(initial_state, |mut state| async move {
        // NOTE: We do NOT check cancellation synchronously here.
        // The initial check can cause false positives because the cancel signal
        // may be set by DisconnectStream's Drop handler even for normal completions.
        // Instead, we only detect disconnects via select! which properly races
        // against data arrival.

        // Get the next chunk, applying TTFT timeout for the first chunk only
        // We use select! to handle cancellation during await
        let chunk_result = if !state.first_chunk_received {
            // First chunk - apply TTFT timeout if configured
            if let Some(timeout_secs) = state.ttft_timeout_secs {
                let stream_future = state.stream.next();
                let timeout_future = tokio::time::sleep(Duration::from_secs(timeout_secs));

                // Create cancellation future
                let cancel_future = async {
                    if let Some(rx) = &mut state.cancel_rx {
                        let _ = rx.changed().await;
                        true
                    } else {
                        futures::future::pending::<bool>().await
                    }
                };

                select! {
                    chunk = stream_future => {
                        chunk
                    }
                    _ = timeout_future => {
                        // TTFT timeout - send error event and terminate stream
                        tracing::warn!(
                            "TTFT timeout: first token not received within {} seconds from {}",
                            timeout_secs,
                            state.provider_name
                        );
                        let error_event = json!({
                            "error": {
                                "message": format!(
                                    "TTFT timeout: first token not received within {} seconds",
                                    timeout_secs
                                ),
                                "type": ERROR_TYPE_TIMEOUT,
                                "code": ERROR_CODE_TTFT_TIMEOUT
                            }
                        });
                        let error_message =
                            format!("event: error\ndata: {}\n\ndata: [DONE]\n\n", error_event);

                        // Terminate the stream
                        let terminated_stream: Pin<
                            Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>,
                        > = Box::pin(futures::stream::empty());
                        state.stream = terminated_stream;
                        state.first_chunk_received = true;

                        return Some((
                            Ok::<Vec<u8>, std::io::Error>(error_message.into_bytes()),
                            state,
                        ));
                    }
                    _ = cancel_future => {
                        tracing::info!(
                            provider = %state.provider_name,
                            model = %state.original_model,
                            request_id = %state.request_id,
                            "Client disconnected during TTFT wait, cancelling stream"
                        );
                        get_metrics().client_disconnects_total.inc();
                        finalize_stream(&mut state);
                        return None;
                    }
                }
            } else {
                // No timeout, but still need to check cancellation
                let stream_future = state.stream.next();
                let cancel_future = async {
                    if let Some(rx) = &mut state.cancel_rx {
                        let _ = rx.changed().await;
                        true
                    } else {
                        futures::future::pending::<bool>().await
                    }
                };

                select! {
                    chunk = stream_future => chunk,
                    _ = cancel_future => {
                        tracing::info!(
                            provider = %state.provider_name,
                            model = %state.original_model,
                            request_id = %state.request_id,
                            "Client disconnected, cancelling stream"
                        );
                        get_metrics().client_disconnects_total.inc();
                        finalize_stream(&mut state);
                        return None;
                    }
                }
            }
        } else {
            // Subsequent chunks - no timeout, but check cancellation
            let stream_future = state.stream.next();
            let cancel_future = async {
                if let Some(rx) = &mut state.cancel_rx {
                    let _ = rx.changed().await;
                    true
                } else {
                    futures::future::pending::<bool>().await
                }
            };

            select! {
                chunk = stream_future => chunk,
                _ = cancel_future => {
                    tracing::info!(
                        provider = %state.provider_name,
                        model = %state.original_model,
                        request_id = %state.request_id,
                        "Client disconnected, cancelling stream"
                    );
                    get_metrics().client_disconnects_total.inc();
                    finalize_stream(&mut state);
                    return None;
                }
            }
        };

        match chunk_result {
            Some(Ok(bytes)) => {
                state.first_chunk_received = true;
                let output = process_chunk(&mut state, bytes);
                Some((Ok(output), state))
            }
            Some(Err(e)) => {
                tracing::error!("Stream error: {}", e);
                let error_event = json!({
                    "error": {
                        "message": e.to_string(),
                        "type": ERROR_TYPE_STREAM,
                        "code": ERROR_CODE_PROVIDER
                    }
                });
                let error_message =
                    format!("event: error\ndata: {}\n\ndata: [DONE]\n\n", error_event);

                let terminated_stream: Pin<
                    Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>,
                > = Box::pin(futures::stream::empty());
                state.stream = terminated_stream;
                state.first_chunk_received = true;

                Some((Ok(error_message.into_bytes()), state))
            }
            None => {
                // Process any remaining data in the SSE line buffer
                if !state.sse_line_buffer.trim().is_empty() {
                    let remaining = std::mem::take(&mut state.sse_line_buffer);
                    let output = process_remaining_buffer(&mut state, &remaining);
                    if !output.is_empty() {
                        // Need to emit remaining data before finalizing
                        // We'll use a marker to know we need to finalize next iteration
                        return Some((Ok(output), state));
                    }
                }
                // Finalize and end stream
                finalize_stream(&mut state);
                None
            }
        }
    });

    let body = Body::from_stream(byte_stream);

    // Return response IMMEDIATELY - connection established before first token arrives
    Ok(AxumResponse::builder()
        .status(200)
        .header("Content-Type", "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .body(body)
        .unwrap())
}

/// Process a chunk of streaming data with SSE line buffering
/// Returns complete SSE events only, buffering incomplete data
fn process_chunk(state: &mut StreamState, bytes: Bytes) -> Vec<u8> {
    // Log stream start on first chunk
    if !state.stream_start_logged {
        state.stream_start_logged = true;
        tracing::debug!(
            provider = %state.provider_name,
            model = %state.original_model,
            "[Stream Start] Streaming response started"
        );
    }

    // Increment chunk counter
    state.chunk_count += 1;

    // Append incoming bytes to SSE line buffer
    let chunk_str = String::from_utf8_lossy(&bytes);
    state.sse_line_buffer.push_str(&chunk_str);

    // Extract complete SSE events from buffer (events end with \n\n)
    let mut complete_events = Vec::new();
    while let Some(pos) = state.sse_line_buffer.find("\n\n") {
        let event = state.sse_line_buffer[..pos].to_string();
        state.sse_line_buffer = state.sse_line_buffer[pos + 2..].to_string();
        if !event.trim().is_empty() {
            complete_events.push(event);
        }
    }

    // If no complete events yet, return empty (wait for more data)
    if complete_events.is_empty() {
        return Vec::new();
    }

    // Process complete events
    let mut rewritten_lines = Vec::new();
    let mut chunk_modified = false;
    let mut has_done = false;

    for event in complete_events {
        // Log SSE chunk at DEBUG level
        if tracing::enabled!(tracing::Level::DEBUG) {
            for line in event.lines() {
                if line.starts_with("data: ") {
                    tracing::debug!(
                        provider = %state.provider_name,
                        "[Stream Chunk #{}] {}",
                        state.chunk_count,
                        line
                    );
                }
            }
        }

        // Process each line in the event
        let mut event_lines = Vec::new();
        let mut event_has_data = false;

        for line in event.split('\n') {
            if line.trim().is_empty() {
                continue;
            }

            // Check for [DONE] marker
            if line == "data: [DONE]" {
                has_done = true;
                // Don't push [DONE] yet
                continue;
            }

            if !line.starts_with("data: ") {
                event_lines.push(line.to_string());
                continue;
            }

            event_has_data = true;
            let json_str = line[6..].trim();
            if json_str.is_empty() || !json_str.ends_with('}') {
                event_lines.push(line.to_string());
                continue;
            }

            if let Ok(mut json_obj) = serde_json::from_str::<serde_json::Value>(json_str) {
                // Rewrite model field to original model (like Python does)
                if json_obj.get("model").is_some() {
                    json_obj["model"] = serde_json::Value::String(state.original_model.clone());
                    chunk_modified = true;
                }

                // Check for usage first - only accept if prompt_tokens > 0
                // Update provider usage in OutboundTokenCounter
                if !state.usage_found {
                    if let Some(usage_value) = json_obj.get("usage") {
                        if let Ok(usage) = serde_json::from_value::<Usage>(usage_value.clone()) {
                            // Only accept valid usage (prompt_tokens > 0), otherwise keep fallback
                            if usage.prompt_tokens > 0 {
                                // Update provider usage in OutboundTokenCounter
                                let unified_usage = UnifiedUsage {
                                    input_tokens: usage.prompt_tokens as i32,
                                    output_tokens: usage.completion_tokens as i32,
                                    ..Default::default()
                                };
                                state.token_counter.update_provider_usage(&unified_usage);
                                state.usage_found = true;

                                // Capture usage for Langfuse
                                if let Some(ref mut gen_data) = state.generation_data {
                                    gen_data.prompt_tokens = usage.prompt_tokens;
                                    gen_data.completion_tokens = usage.completion_tokens;
                                    gen_data.total_tokens = usage.total_tokens;
                                }
                            }
                        }
                    }
                }

                // Extract content and finish_reason for token counting and Langfuse
                let contents = extract_stream_text(&json_obj);

                // Capture finish_reason for Langfuse and check if we need to inject usage
                let mut has_finish_reason = false;
                if let Some(choices) = json_obj.get("choices").and_then(|c| c.as_array()) {
                    for choice in choices {
                        if let Some(reason) = choice.get("finish_reason").and_then(|r| r.as_str()) {
                            state.finish_reason = Some(reason.to_string());
                            has_finish_reason = true;
                        }
                    }
                }

                if !contents.is_empty() {
                    let now = Instant::now();

                    // Record provider first token time (TTFT metrics recorded at stream end)
                    if state.provider_first_token_time.is_none() {
                        state.provider_first_token_time = Some(now);

                        // Capture TTFT for Langfuse
                        if let Some(ref mut gen_data) = state.generation_data {
                            gen_data.ttft_time = Some(Utc::now());
                        }
                    }

                    // Accumulate output content using OutboundTokenCounter
                    for content in &contents {
                        state.token_counter.accumulate_content(content);
                    }

                    // Also keep accumulated_output for Langfuse (backward compatibility)
                    if state.generation_data.is_some() {
                        for content in contents {
                            state.accumulated_output.push(content);
                        }
                    }
                }

                // Inject fallback usage into finish_reason chunk if provider didn't provide it
                if has_finish_reason && !state.usage_found {
                    // Use OutboundTokenCounter to get final usage
                    let final_usage = state.token_counter.finalize();
                    if final_usage.input_tokens > 0 || final_usage.output_tokens > 0 {
                        json_obj["usage"] = json!({
                            "prompt_tokens": final_usage.input_tokens,
                            "completion_tokens": final_usage.output_tokens,
                            "total_tokens": final_usage.input_tokens + final_usage.output_tokens
                        });
                        chunk_modified = true;
                        state.usage_chunk_sent = true;
                    }
                }

                // Normalize Gemini 3 response payload (align with LiteLLM handling)
                let gemini_model = state
                    .gemini_model
                    .as_deref()
                    .or(Some(state.original_model.as_str()));
                if normalize_response_payload(&mut json_obj, gemini_model) {
                    chunk_modified = true;
                }

                // Accumulate raw SSE data for JSONL logging
                if !state.request_id.is_empty() {
                    let rewritten_json =
                        serde_json::to_string(&json_obj).unwrap_or_else(|_| json_str.to_string());
                    state
                        .accumulated_sse_data
                        .push(format!("data: {}", rewritten_json));
                }

                // Rebuild the line with rewritten JSON
                let rewritten_json =
                    serde_json::to_string(&json_obj).unwrap_or_else(|_| json_str.to_string());
                event_lines.push(format!("data: {}", rewritten_json));
            } else {
                event_lines.push(line.to_string());
            }
        }

        // Add event to rewritten_lines if it had data
        if event_has_data && !event_lines.is_empty() {
            rewritten_lines.push(event_lines.join("\n"));
        }
    }

    // Now add [DONE] if it was detected
    if has_done {
        rewritten_lines.push("data: [DONE]".to_string());
        chunk_modified = true;
    }

    // Return rewritten chunk with proper SSE formatting (\n\n between events)
    // Always use rewritten_lines when events were extracted from the SSE buffer,
    // because the original bytes may be a TCP fragment that doesn't align with
    // SSE event boundaries (the buffer consumed and reassembled them).
    if !rewritten_lines.is_empty() {
        let mut result = rewritten_lines.join("\n\n");
        if !result.is_empty() {
            result.push_str("\n\n");
        }
        result.into_bytes()
    } else if chunk_modified {
        Vec::new()
    } else {
        bytes.to_vec()
    }
}

/// Process remaining data in the SSE line buffer at stream end
fn process_remaining_buffer(state: &mut StreamState, remaining: &str) -> Vec<u8> {
    if remaining.trim().is_empty() {
        return Vec::new();
    }

    let mut rewritten_lines = Vec::new();
    let mut chunk_modified = false;
    let mut has_done = false;

    // Process each line in the remaining buffer
    let mut event_lines = Vec::new();
    let mut event_has_data = false;

    for line in remaining.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // Check for [DONE] marker
        if line == "data: [DONE]" {
            has_done = true;
            continue;
        }

        if !line.starts_with("data: ") {
            event_lines.push(line.to_string());
            continue;
        }

        event_has_data = true;
        let json_str = line[6..].trim();
        if json_str.is_empty() || !json_str.ends_with('}') {
            event_lines.push(line.to_string());
            continue;
        }

        if let Ok(mut json_obj) = serde_json::from_str::<serde_json::Value>(json_str) {
            // Rewrite model field
            if json_obj.get("model").is_some() {
                json_obj["model"] = serde_json::Value::String(state.original_model.clone());
                chunk_modified = true;
            }

            let rewritten_json =
                serde_json::to_string(&json_obj).unwrap_or_else(|_| json_str.to_string());
            event_lines.push(format!("data: {}", rewritten_json));
        } else {
            event_lines.push(line.to_string());
        }
    }

    if event_has_data && !event_lines.is_empty() {
        rewritten_lines.push(event_lines.join("\n"));
    }

    if has_done {
        rewritten_lines.push("data: [DONE]".to_string());
        chunk_modified = true;
    }

    if chunk_modified || !rewritten_lines.is_empty() {
        let mut result = rewritten_lines.join("\n\n");
        if !result.is_empty() {
            result.push_str("\n\n");
        }
        result.into_bytes()
    } else {
        Vec::new()
    }
}

/// Finalize stream and record metrics
fn finalize_stream(state: &mut StreamState) {
    // Log stream end
    tracing::debug!(
        provider = %state.provider_name,
        model = %state.original_model,
        chunks = state.chunk_count,
        "[Stream End] Streaming completed"
    );

    // Get final usage from OutboundTokenCounter (handles provider usage priority)
    let final_usage = state.token_counter.finalize();
    let final_input_tokens = final_usage.input_tokens as usize;
    let final_output_tokens = final_usage.output_tokens as usize;

    // Record all stream metrics using unified function
    let stats = StreamStats {
        model: state.original_model.clone(),
        provider: state.provider_name.clone(),
        api_key_name: state.api_key_name.clone(),
        client: state.client.clone(),
        input_tokens: final_input_tokens,
        output_tokens: final_output_tokens,
        start_time: state.start_time,
        first_token_time: state.provider_first_token_time,
    };
    record_stream_metrics(&stats);

    // Record Langfuse generation
    if let Some(ref gen_data) = state.generation_data {
        let mut final_gen_data = gen_data.clone();

        // Set output content from accumulated output (or from token_counter)
        let output_content = if !state.accumulated_output.is_empty() {
            state.accumulated_output.join("")
        } else {
            state.token_counter.output_content().to_string()
        };
        final_gen_data.output_content = Some(output_content);
        final_gen_data.finish_reason = state.finish_reason.clone();
        // Set token usage from OutboundTokenCounter
        if !state.usage_found {
            final_gen_data.prompt_tokens = final_usage.input_tokens as u32;
            final_gen_data.completion_tokens = final_usage.output_tokens as u32;
            final_gen_data.total_tokens =
                (final_usage.input_tokens + final_usage.output_tokens) as u32;
        }

        let sampled_trace_id =
            (!final_gen_data.trace_id.is_empty()).then(|| final_gen_data.trace_id.clone());
        finish_generation_if_sampled(&sampled_trace_id, &mut final_gen_data);
    }

    // Log streaming response to JSONL if enabled and request_id is set
    if !state.request_id.is_empty() {
        // Log client-facing streaming response
        log_streaming_response(
            &state.request_id,
            200, // Streaming responses are always 200 if we reach finalize
            None,
            state.accumulated_sse_data.clone(),
        );

        // Log provider streaming response (raw SSE data from provider)
        log_provider_streaming_response(
            &state.request_id,
            &state.provider_name,
            200,
            None,
            state.accumulated_sse_data.clone(),
        );
    }
}

fn extract_text_segments(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items.iter().flat_map(extract_text_segments).collect(),
        Value::Object(obj) => {
            if obj
                .get("type")
                .and_then(|t| t.as_str())
                .map(|t| t == "text")
                .unwrap_or(false)
            {
                if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                    vec![text.to_string()]
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        }
        _ => vec![],
    }
}

fn extract_stream_text(chunk_obj: &Value) -> Vec<String> {
    let mut contents = Vec::new();
    if let Some(choices) = chunk_obj.get("choices").and_then(|c| c.as_array()) {
        for choice in choices {
            if let Some(delta) = choice.get("delta") {
                // Extract content field
                if let Some(content_field) = delta.get("content") {
                    match content_field {
                        Value::String(text) => contents.push(text.clone()),
                        Value::Array(parts) => {
                            for part in parts {
                                contents.extend(extract_text_segments(part));
                            }
                        }
                        Value::Object(_) => {
                            contents.extend(extract_text_segments(content_field));
                        }
                        _ => {}
                    }
                }
                // Extract reasoning_content field (for OpenAI-compatible APIs like Grok)
                if let Some(reasoning) = delta.get("reasoning_content").and_then(|r| r.as_str()) {
                    contents.push(reasoning.to_string());
                }
                // Extract tool_calls arguments for token counting
                // When LLM returns tool_calls instead of text content, we need to count
                // the arguments JSON as output tokens
                if let Some(tool_calls) = delta.get("tool_calls").and_then(|tc| tc.as_array()) {
                    for tool_call in tool_calls {
                        if let Some(function) = tool_call.get("function") {
                            // Extract function arguments (partial JSON string in streaming)
                            if let Some(arguments) =
                                function.get("arguments").and_then(|a| a.as_str())
                            {
                                if !arguments.is_empty() {
                                    contents.push(arguments.to_string());
                                }
                            }
                            // Also count function name if present (first chunk of tool_call)
                            if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                                if !name.is_empty() {
                                    contents.push(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    contents
}

/// Record fallback token usage when provider doesn't return usage.
pub fn record_fallback_token_usage(
    input_tokens: usize,
    output_tokens: usize,
    model: &str,
    provider: &str,
    api_key_name: &str,
    client: &str,
) {
    let metrics = get_metrics();
    let total_tokens = input_tokens + output_tokens;

    metrics
        .token_usage
        .with_label_values(&[model, provider, "prompt", api_key_name, client])
        .inc_by(input_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "completion", api_key_name, client])
        .inc_by(output_tokens as u64);

    metrics
        .token_usage
        .with_label_values(&[model, provider, "total", api_key_name, client])
        .inc_by(total_tokens as u64);

    tracing::info!(
        "Token usage calculated (fallback) - model={} provider={} key={} client={} prompt={} completion={} total={}",
        model,
        provider,
        api_key_name,
        client,
        input_tokens,
        output_tokens,
        total_tokens
    );
}

/// Rewrite model name in a non-streaming response.
pub fn rewrite_model_in_response(
    mut response: serde_json::Value,
    original_model: &str,
) -> serde_json::Value {
    if let Some(obj) = response.as_object_mut() {
        obj.insert(
            "model".to_string(),
            serde_json::Value::String(original_model.to_string()),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE64_PNG_1X1: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR4nGNgYAAAAAMAAWgmWQ0AAAAASUVORK5CYII=";

    #[test]
    fn test_rewrite_model_in_response() {
        let response = serde_json::json!({
            "id": "123",
            "model": "old-model",
            "choices": []
        });

        let rewritten = rewrite_model_in_response(response, "new-model");
        assert_eq!(rewritten["model"], "new-model");
    }

    #[test]
    fn test_count_tokens() {
        let text = "Hello world";
        let tokens = count_tokens(text, "gpt-3.5-turbo");
        assert!(tokens > 0);
    }

    #[test]
    fn test_calculate_message_tokens() {
        let messages = vec![
            serde_json::json!({"role": "user", "content": "Hello"}),
            serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "Hi there"}]}),
        ];
        let tokens = calculate_message_tokens(&messages, "gpt-3.5-turbo")
            .expect("message token calculation should succeed");
        assert!(tokens > 0);
        // Should include content + role + format overhead
        assert!(tokens > 10);
    }

    #[test]
    fn test_calculate_image_tokens_low() {
        let tokens = calculate_image_tokens(BASE64_PNG_1X1, "low")
            .expect("image token calculation should succeed");
        assert_eq!(tokens, 85);
    }

    #[test]
    fn test_calculate_image_tokens_high() {
        let tokens = calculate_image_tokens(BASE64_PNG_1X1, "high")
            .expect("image token calculation should succeed");
        assert_eq!(tokens, 255);
    }

    #[test]
    fn test_calculate_image_tokens_auto() {
        let tokens = calculate_image_tokens(BASE64_PNG_1X1, "auto")
            .expect("image token calculation should succeed");
        assert_eq!(tokens, 85);
    }

    #[test]
    fn test_calculate_tools_tokens_empty() {
        let tools: Vec<Value> = vec![];
        let tokens = calculate_tools_tokens(&tools, "gpt-4");
        assert_eq!(tokens, 0);
    }

    #[test]
    fn test_calculate_tools_tokens_single() {
        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get current weather",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "location": {"type": "string"}
                    },
                    "required": ["location"]
                }
            }
        })];
        let tokens = calculate_tools_tokens(&tools, "gpt-4");
        assert!(tokens > 0);
        // Should be roughly 20-60 tokens
        assert!(tokens > 20 && tokens < 80);
    }

    #[test]
    fn test_calculate_message_tokens_with_image() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "What's in this image?"},
                {
                    "type": "image_url",
                    "image_url": {
                        "url": BASE64_PNG_1X1,
                        "detail": "low"
                    }
                }
            ]
        })];
        let tokens = calculate_message_tokens(&messages, "gpt-4")
            .expect("message token calculation should succeed");
        // Should include text tokens + 85 (image) + format overhead
        assert!(tokens > 85);
    }

    #[test]
    fn test_calculate_message_tokens_with_image_high_detail() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "Analyze"},
                {
                    "type": "image_url",
                    "image_url": {
                        "url": BASE64_PNG_1X1,
                        "detail": "high"
                    }
                }
            ]
        })];
        let tokens = calculate_message_tokens(&messages, "gpt-4")
            .expect("message token calculation should succeed");
        // Should include text tokens + high detail image + format overhead
        assert!(tokens > 255);
    }

    #[test]
    fn test_calculate_message_tokens_with_image_string_url() {
        let messages = vec![serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "Check this"},
                {
                    "type": "image_url",
                    "image_url": BASE64_PNG_1X1
                }
            ]
        })];
        let tokens = calculate_message_tokens(&messages, "gpt-4")
            .expect("message token calculation should succeed");
        // Should handle string format (auto detail)
        assert!(tokens > 85);
    }

    #[test]
    fn test_stream_state_creation() {
        use futures::stream;
        let empty_stream = stream::empty();
        let state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            Some(30), // ttft_timeout_secs
            None,     // generation_data (Langfuse disabled)
            None,     // cancel_rx
        );

        // Verify token_counter is initialized with input_tokens
        assert_eq!(state.token_counter.input_tokens(), 100);
        assert!(state.token_counter.output_content().is_empty());
        assert!(!state.usage_found);
        assert!(state.provider_first_token_time.is_none());
        assert_eq!(state.api_key_name, "test-key");
        assert_eq!(state.client, "test-client");
        assert_eq!(state.ttft_timeout_secs, Some(30));
        assert!(!state.first_chunk_received);
        assert!(state.generation_data.is_none());
        assert!(state.accumulated_output.is_empty());
        assert!(state.finish_reason.is_none());
    }

    #[test]
    fn test_stream_state_with_langfuse() {
        use futures::stream;
        let empty_stream = stream::empty();

        let gen_data = GenerationData {
            trace_id: "test-trace-id".to_string(),
            request_id: "test-request-id".to_string(),
            credential_name: "test-credential".to_string(),
            endpoint: "/v1/chat/completions".to_string(),
            ..Default::default()
        };

        let state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            Some(30),
            Some(gen_data),
            None, // cancel_rx
        );

        assert!(state.generation_data.is_some());
        let gen = state.generation_data.as_ref().unwrap();
        assert_eq!(gen.trace_id, "test-trace-id");
        assert_eq!(gen.request_id, "test-request-id");
    }

    #[test]
    fn test_extract_stream_text_with_tool_calls() {
        // Test that tool_calls arguments are extracted for token counting
        let chunk = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"location\": \"San Francisco\"}"
                        }
                    }]
                },
                "finish_reason": null
            }]
        });

        let contents = extract_stream_text(&chunk);

        // Should extract both function name and arguments
        assert_eq!(contents.len(), 2);
        assert!(contents.contains(&"get_weather".to_string()));
        assert!(contents.contains(&"{\"location\": \"San Francisco\"}".to_string()));
    }

    #[test]
    fn test_extract_stream_text_with_tool_calls_partial_arguments() {
        // Test streaming tool_calls where arguments come in chunks
        let chunk = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "arguments": "{\"city\":"
                        }
                    }]
                },
                "finish_reason": null
            }]
        });

        let contents = extract_stream_text(&chunk);

        // Should extract partial arguments
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0], "{\"city\":");
    }

    #[test]
    fn test_extract_stream_text_with_tool_calls_empty_arguments() {
        // Test that empty arguments are not extracted
        let chunk = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "arguments": ""
                        }
                    }]
                },
                "finish_reason": null
            }]
        });

        let contents = extract_stream_text(&chunk);

        // Should not extract empty arguments
        assert!(contents.is_empty());
    }

    #[test]
    fn test_extract_stream_text_mixed_content_and_tool_calls() {
        // Test chunk with both content and tool_calls (unusual but possible)
        let chunk = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "content": "Let me check the weather",
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "name": "get_weather",
                            "arguments": "{}"
                        }
                    }]
                },
                "finish_reason": null
            }]
        });

        let contents = extract_stream_text(&chunk);

        // Should extract both content and tool_calls
        assert_eq!(contents.len(), 3);
        assert!(contents.contains(&"Let me check the weather".to_string()));
        assert!(contents.contains(&"get_weather".to_string()));
        assert!(contents.contains(&"{}".to_string()));
    }

    #[test]
    fn test_extract_stream_text_multiple_tool_calls() {
        // Test multiple tool_calls in a single chunk
        let chunk = serde_json::json!({
            "id": "chatcmpl-123",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [
                        {
                            "index": 0,
                            "function": {
                                "name": "get_weather",
                                "arguments": "{\"city\":\"NYC\"}"
                            }
                        },
                        {
                            "index": 1,
                            "function": {
                                "name": "get_time",
                                "arguments": "{\"timezone\":\"EST\"}"
                            }
                        }
                    ]
                },
                "finish_reason": null
            }]
        });

        let contents = extract_stream_text(&chunk);

        // Should extract all tool_calls
        assert_eq!(contents.len(), 4);
        assert!(contents.contains(&"get_weather".to_string()));
        assert!(contents.contains(&"{\"city\":\"NYC\"}".to_string()));
        assert!(contents.contains(&"get_time".to_string()));
        assert!(contents.contains(&"{\"timezone\":\"EST\"}".to_string()));
    }

    #[test]
    fn test_sse_line_buffer_initialization() {
        use futures::stream;
        let empty_stream = stream::empty();
        let state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            None,
            None,
            None, // cancel_rx
        );

        // SSE line buffer should be initialized empty
        assert!(state.sse_line_buffer.is_empty());
    }

    #[test]
    fn test_process_chunk_buffers_incomplete_event() {
        use futures::stream;
        let empty_stream = stream::empty();
        let mut state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            None,
            None,
            None, // cancel_rx
        );

        // Send incomplete SSE event (no \n\n terminator)
        let incomplete_chunk = Bytes::from("data: {\"model\":\"gpt-4\",\"content\":\"Hello\"");
        let output = process_chunk(&mut state, incomplete_chunk);

        // Should return empty - data is buffered waiting for event boundary
        assert!(output.is_empty());
        // Buffer should contain the incomplete data
        assert!(state.sse_line_buffer.contains("data:"));
    }

    #[test]
    fn test_process_chunk_emits_complete_event() {
        use futures::stream;
        let empty_stream = stream::empty();
        let mut state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            None,
            None,
            None, // cancel_rx
        );

        // Send complete SSE event with \n\n terminator
        let complete_chunk = Bytes::from("data: {\"model\":\"gpt-4\",\"content\":\"Hello\"}\n\n");
        let output = process_chunk(&mut state, complete_chunk);

        // Should return the processed event
        assert!(!output.is_empty());
        // Buffer should be empty after processing
        assert!(state.sse_line_buffer.is_empty());
    }

    #[test]
    fn test_process_chunk_reassembles_fragmented_event() {
        use futures::stream;
        let empty_stream = stream::empty();
        let mut state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            None,
            None,
            None, // cancel_rx
        );

        // First chunk: incomplete event
        let chunk1 = Bytes::from("data: {\"model\":\"gpt-4\",");
        let output1 = process_chunk(&mut state, chunk1);
        assert!(output1.is_empty()); // Buffered

        // Second chunk: completes the event
        let chunk2 = Bytes::from("\"content\":\"Hello\"}\n\n");
        let output2 = process_chunk(&mut state, chunk2);

        // Now we should get output
        assert!(!output2.is_empty());
        let output_str = String::from_utf8_lossy(&output2);
        assert!(output_str.contains("Hello"));
        // Buffer should be empty
        assert!(state.sse_line_buffer.is_empty());
    }

    #[test]
    fn test_process_chunk_handles_multiple_events() {
        use futures::stream;
        let empty_stream = stream::empty();
        let mut state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            None,
            None,
            None, // cancel_rx
        );

        // Send two complete events in one chunk
        let multi_chunk =
            Bytes::from("data: {\"content\":\"Hello\"}\n\ndata: {\"content\":\"World\"}\n\n");
        let output = process_chunk(&mut state, multi_chunk);

        // Should contain both events
        let output_str = String::from_utf8_lossy(&output);
        assert!(output_str.contains("Hello"));
        assert!(output_str.contains("World"));
    }

    #[test]
    fn test_process_chunk_handles_done_marker() {
        use futures::stream;
        let empty_stream = stream::empty();
        let mut state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            None,
            None,
            None, // cancel_rx
        );

        // Send [DONE] marker
        let done_chunk = Bytes::from("data: [DONE]\n\n");
        let output = process_chunk(&mut state, done_chunk);

        let output_str = String::from_utf8_lossy(&output);
        assert!(output_str.contains("[DONE]"));
    }

    #[test]
    fn test_process_remaining_buffer_handles_leftover_data() {
        use futures::stream;
        let empty_stream = stream::empty();
        let mut state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            None,
            None,
            None, // cancel_rx
        );

        // Process remaining buffer with valid JSON
        let remaining = "data: {\"model\":\"gpt-4\",\"content\":\"final\"}";
        let output = process_remaining_buffer(&mut state, remaining);

        // Should process the remaining data
        let output_str = String::from_utf8_lossy(&output);
        assert!(output_str.contains("final"));
    }

    #[test]
    fn test_process_remaining_buffer_empty() {
        use futures::stream;
        let empty_stream = stream::empty();
        let mut state = StreamState::new(
            empty_stream,
            100,
            "gpt-3.5-turbo".to_string(),
            None,
            "test-provider".to_string(),
            "test-key".to_string(),
            "test-client".to_string(),
            None,
            None,
            None, // cancel_rx
        );

        // Empty remaining buffer
        let output = process_remaining_buffer(&mut state, "");
        assert!(output.is_empty());

        // Whitespace-only remaining buffer
        let output2 = process_remaining_buffer(&mut state, "   \n  ");
        assert!(output2.is_empty());
    }
}
