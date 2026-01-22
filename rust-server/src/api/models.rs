//! API request and response models.
//!
//! This module defines all data structures used in the API, including
//! chat completion requests/responses, health checks, and model listings.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

/// Provider information for internal use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub name: String,
    pub api_base: String,
    pub api_key: String,
    pub weight: u32,
    pub model_mapping: HashMap<String, String>,
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
#[schema(example = json!({"id": "gpt-4", "object": "model", "created": 1677610602, "owned_by": "system"}))]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
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

/// Error response for API errors.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "error": {
        "message": "Invalid API key",
        "type": "error",
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
            }],
        };

        let json = serde_json::to_string(&model_list).unwrap();
        assert!(json.contains("\"object\":\"list\""));
        assert!(json.contains("\"id\":\"gpt-4\""));
    }

    #[test]
    fn test_provider_clone() {
        let provider = Provider {
            name: "Test".to_string(),
            api_base: "http://test".to_string(),
            api_key: "key".to_string(),
            weight: 1,
            model_mapping: HashMap::new(),
        };

        let cloned = provider.clone();
        assert_eq!(cloned.name, provider.name);
        assert_eq!(cloned.api_base, provider.api_base);
        assert_eq!(cloned.weight, provider.weight);
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
}
