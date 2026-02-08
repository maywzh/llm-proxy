//! API request and response models.
//!
//! This module defines all data structures used in the API, including
//! chat completion requests/responses, health checks, and model listings.

use crate::core::config::{ModelMappingEntry, ModelMappingValue};
use crate::core::error_types::ERROR_TYPE_API;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use utoipa::ToSchema;

lazy_static! {
    /// Cache for compiled regex patterns
    static ref PATTERN_CACHE: RwLock<HashMap<String, Regex>> = RwLock::new(HashMap::new());
}

/// Check if a model mapping key contains wildcard/regex patterns.
///
/// Supports:
/// - Regex patterns: .* .+ [abc] etc.
/// - Simple wildcards: * (converted to .*)
///
/// Note: A single dot (.) in model names like "gpt-3.5-turbo" is NOT considered a pattern.
/// Only regex-specific patterns like .* .+ or metacharacters like [, (, |, etc. are detected.
pub fn is_pattern(key: &str) -> bool {
    // Check for regex-specific patterns (not just a single dot)
    // .* or .+ are regex patterns
    if key.contains(".*") || key.contains(".+") {
        return true;
    }
    // Check for other regex metacharacters (excluding dot which is common in model names)
    if key.contains('[')
        || key.contains('(')
        || key.contains('|')
        || key.contains('^')
        || key.contains('$')
        || key.contains('\\')
    {
        return true;
    }
    // Simple wildcard: * without preceding dot
    if key.contains('*') && !key.contains(".*") {
        return true;
    }
    false
}

/// Compile a pattern string to regex, caching the result.
///
/// Converts simple wildcards (*) to regex (.*) if needed.
pub fn compile_pattern(pattern: &str) -> Option<Regex> {
    // Check cache first
    {
        let cache = PATTERN_CACHE.read().ok()?;
        if let Some(regex) = cache.get(pattern) {
            return Some(regex.clone());
        }
    }

    // Build regex pattern
    let mut regex_pattern = pattern.to_string();

    // If pattern doesn't look like regex but has *, convert to regex
    if regex_pattern.contains('*') && !regex_pattern.contains(".*") && !regex_pattern.contains(".+")
    {
        regex_pattern = regex_pattern.replace('*', ".*");
    }

    // Anchor the pattern to match the full string
    if !regex_pattern.starts_with('^') {
        regex_pattern = format!("^{}", regex_pattern);
    }
    if !regex_pattern.ends_with('$') {
        regex_pattern = format!("{}$", regex_pattern);
    }

    // Compile and cache
    match Regex::new(&regex_pattern) {
        Ok(regex) => {
            if let Ok(mut cache) = PATTERN_CACHE.write() {
                cache.insert(pattern.to_string(), regex.clone());
            }
            Some(regex)
        }
        Err(_) => None,
    }
}

/// Match a model name against model_mapping keys, supporting wildcards/regex.
///
/// Returns the ModelMappingValue if found, None otherwise.
/// Exact matches take priority over pattern matches.
pub fn match_model_pattern(
    model: &str,
    model_mapping: &HashMap<String, ModelMappingValue>,
) -> Option<ModelMappingValue> {
    // First, try exact match (highest priority)
    if let Some(value) = model_mapping.get(model) {
        return Some(value.clone());
    }

    // Then, try pattern matching
    for (pattern, value) in model_mapping.iter() {
        if is_pattern(pattern) {
            if let Some(regex) = compile_pattern(pattern) {
                if regex.is_match(model) {
                    return Some(value.clone());
                }
            }
        }
    }

    None
}

/// Check if a model matches any key in model_mapping (exact or pattern).
pub fn model_matches_mapping(
    model: &str,
    model_mapping: &HashMap<String, ModelMappingValue>,
) -> bool {
    // First, try exact match
    if model_mapping.contains_key(model) {
        return true;
    }

    // Then, try pattern matching
    for pattern in model_mapping.keys() {
        if is_pattern(pattern) {
            if let Some(regex) = compile_pattern(pattern) {
                if regex.is_match(model) {
                    return true;
                }
            }
        }
    }

    false
}

/// Get the mapped model name for a given model.
///
/// Returns the mapped model name if found, otherwise the original model name.
pub fn get_mapped_model(model: &str, model_mapping: &HashMap<String, ModelMappingValue>) -> String {
    match_model_pattern(model, model_mapping)
        .map(|v| v.mapped_model().to_string())
        .unwrap_or_else(|| model.to_string())
}

/// Get the model metadata for a given model if available.
///
/// Returns ModelMappingEntry with metadata if model uses extended format, None otherwise.
pub fn get_model_metadata(
    model: &str,
    model_mapping: &HashMap<String, ModelMappingValue>,
) -> Option<ModelMappingEntry> {
    match_model_pattern(model, model_mapping).and_then(|v| v.metadata().cloned())
}

/// Check if a model is allowed for the model info endpoint.
///
/// Returns `true` when `allowed_models` is empty (no filtering), the model
/// matches the allowed list directly, or the model is a pattern that matches
/// at least one concrete entry in the allowed list.
pub fn model_allowed_for_info(model_name: &str, allowed_models: &[String]) -> bool {
    if allowed_models.is_empty() {
        return true;
    }
    if crate::api::auth::model_matches_allowed_list(model_name, allowed_models) {
        return true;
    }
    if !is_pattern(model_name) {
        return false;
    }
    if let Some(regex) = compile_pattern(model_name) {
        for allowed in allowed_models {
            if is_pattern(allowed) {
                continue;
            }
            if regex.is_match(allowed) {
                return true;
            }
        }
    }
    false
}

/// Provider information for internal use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub name: String,
    pub api_base: String,
    pub api_key: String,
    pub weight: u32,
    pub model_mapping: HashMap<String, ModelMappingValue>,
    /// Provider type (e.g., "openai", "azure", "anthropic", "gcp-vertex")
    #[serde(default = "default_provider_type")]
    pub provider_type: String,
    /// Provider-specific parameters (e.g., GCP project, location, publisher)
    #[serde(default)]
    pub provider_params: HashMap<String, serde_json::Value>,
}

fn default_provider_type() -> String {
    "openai".to_string()
}

impl Provider {
    /// Check if this provider supports the given model (exact or pattern match).
    pub fn supports_model(&self, model: &str) -> bool {
        model_matches_mapping(model, &self.model_mapping)
    }

    /// Get the mapped model name for the given model.
    pub fn get_mapped_model(&self, model: &str) -> String {
        get_mapped_model(model, &self.model_mapping)
    }

    /// Get the model metadata for the given model if available.
    pub fn get_model_metadata(&self, model: &str) -> Option<ModelMappingEntry> {
        get_model_metadata(model, &self.model_mapping)
    }

    /// Get a string parameter from provider_params.
    pub fn get_param(&self, key: &str) -> Option<&str> {
        self.provider_params.get(key).and_then(|v| v.as_str())
    }
}

/// GCP Vertex AI specific configuration extracted from provider_params.
#[derive(Debug, Clone)]
pub struct GcpVertexConfig {
    pub project: String,
    pub location: String,
    pub publisher: String,
}

impl GcpVertexConfig {
    /// Extract GCP Vertex configuration from a provider.
    /// Returns None if any required field is missing.
    pub fn from_provider(provider: &Provider) -> Option<Self> {
        Some(Self {
            project: provider.get_param("gcp_project")?.to_string(),
            location: provider.get_param("gcp_location")?.to_string(),
            publisher: provider.get_param("gcp_publisher")?.to_string(),
        })
    }

    /// Extract GCP Vertex configuration with defaults for missing fields.
    pub fn from_provider_with_defaults(provider: &Provider) -> Self {
        Self {
            project: provider
                .get_param("gcp_project")
                .unwrap_or_default()
                .to_string(),
            location: provider
                .get_param("gcp_location")
                .unwrap_or("us-central1")
                .to_string(),
            publisher: provider
                .get_param("gcp_publisher")
                .unwrap_or("anthropic")
                .to_string(),
        }
    }
}

/// Chat completion request following OpenAI API format.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "model": "gpt-4",
    "messages": [
        {"role": "system", "content": "You are a helpful assistant."},
        {"role": "user", "content": "Hello!"}
    ],
    "temperature": 0.7,
    "max_tokens": 1000,
    "stream": false
}))]
pub struct ChatCompletionRequest {
    /// Model identifier
    pub model: String,

    /// Conversation messages
    pub messages: Vec<Message>,

    /// Sampling temperature (0.0 to 2.0)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,

    /// Maximum tokens to generate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Whether to stream the response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,

    /// Additional provider-specific parameters
    #[serde(flatten)]
    #[schema(additional_properties)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// A single message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({"role": "user", "content": "Hello!"}))]
pub struct Message {
    /// Role: "system", "user", or "assistant"
    pub role: String,

    /// Message content
    pub content: String,
}

/// Chat completion response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "id": "chatcmpl-abc123",
    "object": "chat.completion",
    "created": 1677858242,
    "model": "gpt-4",
    "choices": [{
        "index": 0,
        "message": {"role": "assistant", "content": "Hello! How can I help you today?"},
        "finish_reason": "stop"
    }],
    "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}
}))]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A single choice in the response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Choice {
    pub index: u32,
    pub message: Message,
    pub finish_reason: Option<String>,
}

/// Token usage statistics.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30}))]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Streaming response chunk.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<StreamChoice>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A single choice in a streaming response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct StreamChoice {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

/// Delta content in streaming responses.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Model information.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({"id": "gpt-4", "object": "model", "created": 1677610602, "owned_by": "system", "permission": [], "root": "gpt-4", "parent": null}))]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
    /// Model permissions (OpenAI compatibility)
    #[serde(default)]
    pub permission: Vec<String>,
    /// Root model identifier (OpenAI compatibility)
    pub root: String,
    /// Parent model identifier (OpenAI compatibility)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// List of available models.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "object": "list",
    "data": [
        {"id": "gpt-4", "object": "model", "created": 1677610602, "owned_by": "system"},
        {"id": "gpt-3.5-turbo", "object": "model", "created": 1677610602, "owned_by": "system"}
    ]
}))]
pub struct ModelList {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

/// LiteLLM-compatible model params for /v1/model/info.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "model": "gpt-4-0613",
    "api_base": "https://api.provider1.com/v1",
    "custom_llm_provider": "openai"
}))]
pub struct LiteLlmParams {
    pub model: String,
    pub api_base: String,
    pub custom_llm_provider: String,
}

/// LiteLLM-compatible model info for /v1/model/info.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "provider_name": "provider1",
    "provider_type": "openai",
    "weight": 2,
    "is_pattern": false,
    "max_tokens": 128000,
    "supports_vision": true
}))]
pub struct ModelInfoDetails {
    pub provider_name: String,
    pub provider_type: String,
    pub weight: u32,
    pub is_pattern: bool,

    // Extended metadata fields (all optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_cost_per_1k_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_cost_per_1k_tokens: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_function_calling: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_response_schema: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_computer_use: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_pdf_input: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

impl ModelInfoDetails {
    /// Create a new ModelInfoDetails with base fields only.
    pub fn new(
        provider_name: String,
        provider_type: String,
        weight: u32,
        is_pattern: bool,
    ) -> Self {
        Self {
            provider_name,
            provider_type,
            weight,
            is_pattern,
            ..Default::default()
        }
    }

    /// Apply metadata from ModelMappingEntry.
    pub fn with_metadata(mut self, entry: &ModelMappingEntry) -> Self {
        self.max_tokens = entry.max_tokens;
        self.max_input_tokens = entry.max_input_tokens;
        self.max_output_tokens = entry.max_output_tokens;
        self.input_cost_per_1k_tokens = entry.input_cost_per_1k_tokens;
        self.output_cost_per_1k_tokens = entry.output_cost_per_1k_tokens;
        self.supports_vision = entry.supports_vision;
        self.supports_function_calling = entry.supports_function_calling;
        self.supports_streaming = entry.supports_streaming;
        self.supports_response_schema = entry.supports_response_schema;
        self.supports_reasoning = entry.supports_reasoning;
        self.supports_computer_use = entry.supports_computer_use;
        self.supports_pdf_input = entry.supports_pdf_input;
        self.mode = entry.mode.clone();
        self
    }
}

/// LiteLLM-compatible model entry for /v1/model/info.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "model_name": "gpt-4",
    "litellm_params": {
        "model": "gpt-4-0613",
        "api_base": "https://api.provider1.com/v1",
        "custom_llm_provider": "openai"
    },
    "model_info": {
        "provider_name": "provider1",
        "provider_type": "openai",
        "weight": 2,
        "is_pattern": false
    }
}))]
pub struct ModelInfoEntry {
    pub model_name: String,
    pub litellm_params: LiteLlmParams,
    pub model_info: ModelInfoDetails,
}

/// LiteLLM-compatible model info list.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "data": [
        {
            "model_name": "gpt-4",
            "litellm_params": {
                "model": "gpt-4-0613",
                "api_base": "https://api.provider1.com/v1",
                "custom_llm_provider": "openai"
            },
            "model_info": {
                "provider_name": "provider1",
                "provider_type": "openai",
                "weight": 2,
                "is_pattern": false
            }
        }
    ]
}))]
pub struct ModelInfoList {
    pub data: Vec<ModelInfoEntry>,
}

/// Query parameters for /model/info endpoint (LiteLLM v2 compatible)
#[derive(Debug, Clone, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfoQueryParams {
    /// Filter by exact model name
    pub model: Option<String>,
    /// Filter by model ID (mapped model name) - LiteLLM v2 compatible
    pub model_id: Option<String>,
    /// Fuzzy search in model_name
    pub search: Option<String>,
    /// Sort field: model_name, created_at, updated_at, costs, status
    #[serde(default = "default_sort_by")]
    pub sort_by: Option<String>,
    /// Sort order: asc, desc
    #[serde(default = "default_sort_order")]
    pub sort_order: String,
    /// Page number (1-indexed)
    #[serde(default = "default_page")]
    pub page: usize,
    /// Page size (default 50)
    #[serde(default = "default_size")]
    pub size: usize,
}

fn default_sort_by() -> Option<String> {
    None
}

fn default_sort_order() -> String {
    "asc".to_string()
}

fn default_page() -> usize {
    1
}

fn default_size() -> usize {
    50
}

/// Paginated model info response (LiteLLM v2 compatible)
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "data": [
        {
            "model_name": "gpt-4",
            "litellm_params": {
                "model": "gpt-4-0613",
                "api_base": "https://api.provider1.com/v1",
                "custom_llm_provider": "openai"
            },
            "model_info": {
                "provider_name": "provider1",
                "provider_type": "openai",
                "weight": 2,
                "is_pattern": false
            }
        }
    ],
    "total_count": 100,
    "current_page": 1,
    "size": 50,
    "total_pages": 5
}))]
pub struct PaginatedModelInfoList {
    pub data: Vec<ModelInfoEntry>,
    pub total_count: usize,
    pub current_page: usize,
    pub size: usize,
    pub total_pages: usize,
}

/// LiteLLM v1 compatible model info list (no pagination)
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "data": [
        {
            "model_name": "gpt-4",
            "litellm_params": {
                "model": "gpt-4-0613",
                "api_base": "https://api.provider1.com/v1",
                "custom_llm_provider": "openai"
            },
            "model_info": {
                "provider_name": "provider1",
                "provider_type": "openai",
                "weight": 2,
                "is_pattern": false
            }
        }
    ]
}))]
pub struct ModelInfoListV1 {
    pub data: Vec<ModelInfoEntry>,
}

/// Query parameters for /v1/model/info endpoint (LiteLLM v1 compatible)
#[derive(Debug, Clone, Deserialize, ToSchema, Default)]
pub struct ModelInfoQueryParamsV1 {
    /// Filter by exact model name
    pub model: Option<String>,
    /// Filter by model ID (mapped model name) - LiteLLM v1 uses litellm_model_id
    pub litellm_model_id: Option<String>,
}

/// Error response for API errors.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "error": {
        "message": "Invalid API key",
        "type": ERROR_TYPE_API,
        "code": 401
    }
}))]
pub struct ApiErrorResponse {
    pub error: ApiErrorDetail,
}

/// Error detail in API error responses.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ApiErrorDetail {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    pub code: u16,
}

/// Request body for checking a single provider's health with concurrent model testing.
#[derive(Debug, Clone, Deserialize, ToSchema)]
#[schema(example = json!({
    "models": ["gpt-4", "gpt-3.5-turbo"],
    "max_concurrent": 2,
    "timeout_secs": 30
}))]
pub struct CheckProviderHealthRequest {
    /// Specific models to test. If empty/null, tests ALL models from provider's model_mapping
    #[serde(default)]
    pub models: Option<Vec<String>>,

    /// Maximum number of models to test concurrently (default: 2, range: 1-10)
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    /// Timeout for each model test in seconds (default: 30, range: 1-120)
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_max_concurrent() -> usize {
    2
}

fn default_timeout_secs() -> u64 {
    30
}

impl Default for CheckProviderHealthRequest {
    fn default() -> Self {
        Self {
            models: None,
            max_concurrent: 2,
            timeout_secs: 30,
        }
    }
}

/// Summary statistics for provider health check.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "total_models": 3,
    "healthy_models": 2,
    "unhealthy_models": 1
}))]
pub struct ProviderHealthSummary {
    /// Total number of models tested
    pub total_models: usize,
    /// Number of healthy models
    pub healthy_models: usize,
    /// Number of unhealthy models
    pub unhealthy_models: usize,
}

/// Response for checking a single provider's health with concurrent model testing.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "provider_id": 1,
    "provider_key": "openai-primary",
    "status": "healthy",
    "models": [{
        "model": "gpt-4",
        "status": "healthy",
        "response_time_ms": 1234,
        "error": null
    }, {
        "model": "gpt-3.5-turbo",
        "status": "unhealthy",
        "response_time_ms": 5000,
        "error": "Timeout after 30s"
    }],
    "summary": {
        "total_models": 2,
        "healthy_models": 1,
        "unhealthy_models": 1
    },
    "avg_response_time_ms": 3117,
    "checked_at": "2024-01-15T10:30:00Z"
}))]
pub struct CheckProviderHealthResponse {
    /// Provider ID
    pub provider_id: i32,
    /// Provider key identifier
    pub provider_key: String,
    /// Overall provider status: healthy, unhealthy, disabled, unknown
    pub status: String,
    /// Health status for each tested model
    pub models: Vec<ModelHealthResult>,
    /// Summary statistics
    pub summary: ProviderHealthSummary,
    /// Average response time across all models in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_response_time_ms: Option<i32>,
    /// ISO 8601 timestamp of when health check was performed
    pub checked_at: String,
}

/// Model health result for the check provider health response.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "model": "gpt-4",
    "status": "healthy",
    "response_time_ms": 1234,
    "error": null
}))]
pub struct ModelHealthResult {
    /// Model name (client-facing, from mapping key)
    pub model: String,
    /// Model status: healthy, unhealthy
    pub status: String,
    /// Response time in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<i32>,
    /// Error message if unhealthy, null otherwise
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a simple model mapping for tests
    fn simple_mapping(entries: &[(&str, &str)]) -> HashMap<String, ModelMappingValue> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), ModelMappingValue::Simple(v.to_string())))
            .collect()
    }

    #[test]
    fn test_message_serialization() {
        let msg = Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Hello\""));
    }

    #[test]
    fn test_message_deserialization() {
        let json = r#"{"role":"assistant","content":"Hi there!"}"#;
        let msg: Message = serde_json::from_str(json).unwrap();
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "Hi there!");
    }

    #[test]
    fn test_usage_calculation() {
        let usage = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
        };

        assert_eq!(
            usage.total_tokens,
            usage.prompt_tokens + usage.completion_tokens
        );
    }

    #[test]
    fn test_chat_completion_request_serialization() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
            temperature: Some(0.7),
            max_tokens: Some(100),
            stream: Some(false),
            extra: HashMap::new(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"gpt-4\""));
        assert!(json.contains("\"temperature\":0.7"));
        assert!(json.contains("\"max_tokens\":100"));
    }

    #[test]
    fn test_chat_completion_request_optional_fields() {
        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra: HashMap::new(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(!json.contains("temperature"));
        assert!(!json.contains("max_tokens"));
        assert!(!json.contains("stream"));
    }

    #[test]
    fn test_chat_completion_request_with_extra_params() {
        let mut extra = HashMap::new();
        extra.insert("top_p".to_string(), serde_json::json!(0.9));
        extra.insert("frequency_penalty".to_string(), serde_json::json!(0.5));

        let request = ChatCompletionRequest {
            model: "gpt-4".to_string(),
            messages: vec![],
            temperature: None,
            max_tokens: None,
            stream: None,
            extra,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"top_p\":0.9"));
        assert!(json.contains("\"frequency_penalty\":0.5"));
    }

    #[test]
    fn test_choice_serialization() {
        let choice = Choice {
            index: 0,
            message: Message {
                role: "assistant".to_string(),
                content: "Response".to_string(),
            },
            finish_reason: Some("stop".to_string()),
        };

        let json = serde_json::to_string(&choice).unwrap();
        assert!(json.contains("\"index\":0"));
        assert!(json.contains("\"finish_reason\":\"stop\""));
    }

    #[test]
    fn test_stream_chunk_serialization() {
        let chunk = StreamChunk {
            id: "chunk-1".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 1234567890,
            model: "gpt-4".to_string(),
            choices: vec![],
            usage: None,
        };

        let json = serde_json::to_string(&chunk).unwrap();
        assert!(json.contains("\"id\":\"chunk-1\""));
        assert!(json.contains("\"object\":\"chat.completion.chunk\""));
    }

    #[test]
    fn test_delta_with_role() {
        let delta = Delta {
            role: Some("assistant".to_string()),
            content: None,
        };

        let json = serde_json::to_string(&delta).unwrap();
        assert!(json.contains("\"role\":\"assistant\""));
        assert!(!json.contains("content"));
    }

    #[test]
    fn test_delta_with_content() {
        let delta = Delta {
            role: None,
            content: Some("Hello".to_string()),
        };

        let json = serde_json::to_string(&delta).unwrap();
        assert!(json.contains("\"content\":\"Hello\""));
        assert!(!json.contains("role"));
    }

    #[test]
    fn test_model_list_serialization() {
        let model_list = ModelList {
            object: "list".to_string(),
            data: vec![ModelInfo {
                id: "gpt-4".to_string(),
                object: "model".to_string(),
                created: 1234567890,
                owned_by: "openai".to_string(),
                permission: vec![],
                root: "gpt-4".to_string(),
                parent: None,
            }],
        };

        let json = serde_json::to_string(&model_list).unwrap();
        assert!(json.contains("\"object\":\"list\""));
        assert!(json.contains("\"id\":\"gpt-4\""));
        assert!(json.contains("\"root\":\"gpt-4\""));
        assert!(json.contains("\"permission\":[]"));
    }

    #[test]
    fn test_provider_clone() {
        let provider = Provider {
            name: "Test".to_string(),
            api_base: "http://test".to_string(),
            api_key: "key".to_string(),
            weight: 1,
            model_mapping: HashMap::new(),
            provider_type: "openai".to_string(),
            provider_params: HashMap::new(),
        };

        let cloned = provider.clone();
        assert_eq!(cloned.name, provider.name);
        assert_eq!(cloned.api_base, provider.api_base);
        assert_eq!(cloned.weight, provider.weight);
        assert_eq!(cloned.provider_type, provider.provider_type);
    }

    #[test]
    fn test_usage_deserialization() {
        let json = r#"{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}"#;
        let usage: Usage = serde_json::from_str(json).unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_stream_choice_with_finish_reason() {
        let choice = StreamChoice {
            index: 0,
            delta: Delta {
                role: None,
                content: Some("text".to_string()),
            },
            finish_reason: Some("stop".to_string()),
        };

        let json = serde_json::to_string(&choice).unwrap();
        assert!(json.contains("\"finish_reason\":\"stop\""));
    }

    #[test]
    fn test_wildcard_model_mapping_exact_match() {
        let mapping = simple_mapping(&[
            ("gpt-4", "gpt-4-exact"),
            ("claude-opus-4-5-.*", "claude-opus-mapped"),
        ]);

        // Exact match should work
        assert!(model_matches_mapping("gpt-4", &mapping));
        assert_eq!(get_mapped_model("gpt-4", &mapping), "gpt-4-exact");
    }

    #[test]
    fn test_wildcard_model_mapping_regex_pattern() {
        let mapping = simple_mapping(&[("claude-opus-4-5-.*", "claude-opus-mapped")]);

        // Regex pattern should match
        assert!(model_matches_mapping("claude-opus-4-5-20240620", &mapping));
        assert_eq!(
            get_mapped_model("claude-opus-4-5-20240620", &mapping),
            "claude-opus-mapped"
        );

        assert!(model_matches_mapping("claude-opus-4-5-latest", &mapping));
        assert_eq!(
            get_mapped_model("claude-opus-4-5-latest", &mapping),
            "claude-opus-mapped"
        );

        // Non-matching should return original
        assert!(!model_matches_mapping("claude-sonnet", &mapping));
        assert_eq!(get_mapped_model("claude-sonnet", &mapping), "claude-sonnet");
    }

    #[test]
    fn test_wildcard_model_mapping_simple_wildcard() {
        let mapping = simple_mapping(&[("gemini-*", "gemini-mapped")]);

        // Simple wildcard (*) should be converted to regex (.*)
        assert!(model_matches_mapping("gemini-pro", &mapping));
        assert_eq!(get_mapped_model("gemini-pro", &mapping), "gemini-mapped");

        assert!(model_matches_mapping("gemini-ultra", &mapping));
        assert_eq!(get_mapped_model("gemini-ultra", &mapping), "gemini-mapped");
    }

    #[test]
    fn test_wildcard_model_mapping_exact_priority() {
        let mapping = simple_mapping(&[
            ("claude-.*", "claude-pattern"),
            ("claude-opus", "claude-opus-exact"),
        ]);

        // Exact match should take priority over pattern
        assert_eq!(
            get_mapped_model("claude-opus", &mapping),
            "claude-opus-exact"
        );

        // Pattern should match other claude models
        assert_eq!(
            get_mapped_model("claude-sonnet", &mapping),
            "claude-pattern"
        );
    }

    #[test]
    fn test_provider_supports_model_with_patterns() {
        let mapping = simple_mapping(&[
            ("gpt-4", "gpt-4-mapped"),
            ("claude-opus-4-5-.*", "claude-mapped"),
        ]);

        let provider = Provider {
            name: "test".to_string(),
            api_base: "http://test".to_string(),
            api_key: "key".to_string(),
            weight: 1,
            model_mapping: mapping,
            provider_type: "openai".to_string(),
            provider_params: HashMap::new(),
        };

        // Test exact match
        assert!(provider.supports_model("gpt-4"));
        assert_eq!(provider.get_mapped_model("gpt-4"), "gpt-4-mapped");

        // Test pattern match
        assert!(provider.supports_model("claude-opus-4-5-20240620"));
        assert_eq!(
            provider.get_mapped_model("claude-opus-4-5-20240620"),
            "claude-mapped"
        );

        // Test non-matching
        assert!(!provider.supports_model("gpt-3.5-turbo"));
        assert_eq!(provider.get_mapped_model("gpt-3.5-turbo"), "gpt-3.5-turbo");
    }
}
