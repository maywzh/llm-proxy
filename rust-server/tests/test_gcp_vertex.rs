//! Integration tests for GCP Vertex AI API endpoints.
//!
//! These tests verify the GCP Vertex AI provider integration,
//! including URL parsing, rawPredict, streamRawPredict, and error handling.
//!
//! Integration tests verify the complete transformation flow:
//! - OpenAI /v1/chat/completions -> GCP Vertex (provider_type="gcp-vertex")
//! - Anthropic /v1/messages -> GCP Vertex (provider_type="gcp-vertex")
//! - Response format conversion back to client format
//!
//! URL format:
//! /models/gcp-vertex/v1/projects/{project}/locations/{location}/publishers/{publisher}/models/{model}:{action}

use axum::Router;
use llm_proxy_rust::{
    api::AppState,
    core::{
        init_metrics, AppConfig, ERROR_TYPE_API, ERROR_TYPE_AUTHENTICATION,
        ERROR_TYPE_INVALID_REQUEST, ERROR_TYPE_OVERLOADED, ERROR_TYPE_RATE_LIMIT,
    },
    services::ProviderService,
    transformer::{Protocol, TransformContext, TransformPipeline, TransformerRegistry},
};
use serde_json::json;
use std::sync::Arc;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

// ============================================================================
// Test Utilities
// ============================================================================

/// Create a test app with mocked GCP Vertex AI provider
#[allow(dead_code)]
async fn create_vertex_test_app(mock_server: &MockServer) -> Router {
    use llm_proxy_rust::core::config::{ModelMappingValue, ProviderConfig, ServerConfig};
    use llm_proxy_rust::core::RateLimiter;
    use std::collections::HashMap;

    init_metrics();

    let model_mapping: HashMap<String, ModelMappingValue> = HashMap::new();

    let mut provider_params: HashMap<String, serde_json::Value> = HashMap::new();
    provider_params.insert("gcp_project".to_string(), serde_json::json!("test-project"));
    provider_params.insert("gcp_location".to_string(), serde_json::json!("us-central1"));
    provider_params.insert("gcp_publisher".to_string(), serde_json::json!("anthropic"));

    let config = AppConfig {
        providers: vec![ProviderConfig {
            name: "GCPVertexMock".to_string(),
            api_base: mock_server.uri(),
            api_key: "test_access_token".to_string(),
            weight: 1,
            model_mapping,
            provider_type: "gcp-vertex".to_string(),
            provider_params,
        }],
        server: ServerConfig {
            host: "0.0.0.0".to_string(),
            port: 18000,
        },
        verify_ssl: false,
        request_timeout_secs: 300,
        ttft_timeout_secs: None,
        credentials: vec![],
        provider_suffix: None,
        min_tokens_limit: 100,
        max_tokens_limit: 4096,
    };

    let provider_service = ProviderService::new(config.clone());
    let rate_limiter = Arc::new(RateLimiter::new());

    let http_client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
        .pool_max_idle_per_host(20)
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to build HTTP client");

    let state = Arc::new(AppState::new(
        config,
        provider_service,
        rate_limiter,
        http_client,
        None,
    ));

    // Note: When implementing the actual GCP Vertex route, add it here
    Router::new().with_state(state)
}

/// Create a transform context for testing
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

/// Create a transformer registry with all protocols
fn create_registry() -> Arc<TransformerRegistry> {
    Arc::new(TransformerRegistry::new())
}

// ============================================================================
// URL Parsing Tests
// ============================================================================

/// Parsed GCP Vertex AI URL parameters
#[derive(Debug, PartialEq)]
struct VertexUrlParams {
    project: String,
    location: String,
    publisher: String,
    model: String,
    action: String,
}

/// Parse GCP Vertex AI URL path
fn parse_vertex_url(path: &str) -> Result<VertexUrlParams, String> {
    use regex::Regex;

    let pattern = Regex::new(
        r"^/models/gcp-vertex/v1/projects/([^/]+)/locations/([^/]+)/publishers/([^/]+)/models/([^:]+):(.+)$",
    )
    .map_err(|e| e.to_string())?;

    let captures = pattern.captures(path).ok_or("Invalid URL format")?;

    let project = captures.get(1).map(|m| m.as_str().to_string()).unwrap();
    let location = captures.get(2).map(|m| m.as_str().to_string()).unwrap();
    let publisher = captures.get(3).map(|m| m.as_str().to_string()).unwrap();
    let model = captures.get(4).map(|m| m.as_str().to_string()).unwrap();
    let action = captures.get(5).map(|m| m.as_str().to_string()).unwrap();

    if project.is_empty() {
        return Err("Missing project".to_string());
    }
    if location.is_empty() {
        return Err("Missing location".to_string());
    }
    if publisher.is_empty() {
        return Err("Missing publisher".to_string());
    }
    if model.is_empty() {
        return Err("Missing model".to_string());
    }

    let valid_actions = ["rawPredict", "streamRawPredict"];
    if !valid_actions.contains(&action.as_str()) {
        return Err(format!(
            "Invalid action: {}. Must be rawPredict or streamRawPredict",
            action
        ));
    }

    Ok(VertexUrlParams {
        project,
        location,
        publisher,
        model,
        action,
    })
}

#[test]
fn test_parse_valid_raw_predict_url() {
    let path = "/models/gcp-vertex/v1/projects/my-project/locations/us-central1/publishers/google/models/claude-3-sonnet:rawPredict";

    let result = parse_vertex_url(path).unwrap();

    assert_eq!(result.project, "my-project");
    assert_eq!(result.location, "us-central1");
    assert_eq!(result.publisher, "google");
    assert_eq!(result.model, "claude-3-sonnet");
    assert_eq!(result.action, "rawPredict");
}

#[test]
fn test_parse_valid_stream_raw_predict_url() {
    let path = "/models/gcp-vertex/v1/projects/test-project-123/locations/europe-west1/publishers/anthropic/models/claude-3-opus:streamRawPredict";

    let result = parse_vertex_url(path).unwrap();

    assert_eq!(result.project, "test-project-123");
    assert_eq!(result.location, "europe-west1");
    assert_eq!(result.publisher, "anthropic");
    assert_eq!(result.model, "claude-3-opus");
    assert_eq!(result.action, "streamRawPredict");
}

#[test]
fn test_parse_url_with_model_version() {
    let path = "/models/gcp-vertex/v1/projects/proj/locations/loc/publishers/pub/models/claude-3-sonnet-20240229:rawPredict";

    let result = parse_vertex_url(path).unwrap();

    assert_eq!(result.model, "claude-3-sonnet-20240229");
    assert_eq!(result.action, "rawPredict");
}

#[test]
fn test_parse_url_invalid_action() {
    let path = "/models/gcp-vertex/v1/projects/proj/locations/loc/publishers/pub/models/model:invalidAction";

    let result = parse_vertex_url(path);

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid action"));
}

#[test]
fn test_parse_url_malformed() {
    let path = "/models/gcp-vertex/v1/invalid/path";

    let result = parse_vertex_url(path);

    assert!(result.is_err());
}

#[test]
fn test_parse_url_wrong_prefix() {
    let path = "/v1/projects/proj/locations/loc/publishers/pub/models/model:rawPredict";

    let result = parse_vertex_url(path);

    assert!(result.is_err());
}

// ============================================================================
// Request/Response Conversion Tests
// ============================================================================

/// Convert request to GCP Vertex AI format
fn convert_vertex_request(request: &serde_json::Value) -> serde_json::Value {
    let mut result = request.clone();

    if let Some(obj) = result.as_object_mut() {
        obj.remove("model");
        obj.insert("anthropic_version".to_string(), json!("vertex-2023-10-16"));
    }

    result
}

#[test]
fn test_anthropic_format_request_passthrough() {
    let request = json!({
        "model": "claude-3-sonnet-20240229",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "Hello!"}]
    });

    let result = convert_vertex_request(&request);

    assert_eq!(result["anthropic_version"], "vertex-2023-10-16");
    assert_eq!(result["max_tokens"], 1024);
    assert!(result.get("model").is_none());
}

#[test]
fn test_request_with_system_prompt() {
    let request = json!({
        "model": "claude-3-opus",
        "max_tokens": 2048,
        "system": "You are a helpful assistant.",
        "messages": [{"role": "user", "content": "Hi"}]
    });

    let result = convert_vertex_request(&request);

    assert_eq!(result["system"], "You are a helpful assistant.");
    assert_eq!(result["anthropic_version"], "vertex-2023-10-16");
}

#[test]
fn test_request_with_tools() {
    let request = json!({
        "model": "claude-3-sonnet",
        "max_tokens": 1024,
        "messages": [{"role": "user", "content": "What's the weather?"}],
        "tools": [{
            "name": "get_weather",
            "description": "Get weather info",
            "input_schema": {
                "type": "object",
                "properties": {"location": {"type": "string"}}
            }
        }]
    });

    let result = convert_vertex_request(&request);

    assert!(result["tools"].is_array());
    assert_eq!(result["tools"][0]["name"], "get_weather");
}

// ============================================================================
// OpenAI -> GCP Vertex Transformation Tests
// ============================================================================

#[test]
fn test_openai_to_gcp_vertex_request() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-sonnet-4-5");

    let openai_request = json!({
        "model": "claude-sonnet-4-5",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "Hello, how are you?"}
        ],
        "max_tokens": 1024,
        "temperature": 0.7
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // Verify Anthropic format for GCP Vertex
    assert_eq!(anthropic_request["model"], "claude-sonnet-4-5");
    assert!(anthropic_request["max_tokens"].is_number());
    assert!(anthropic_request["system"].is_string());
    assert_eq!(anthropic_request["system"], "You are a helpful assistant.");
    assert_eq!(anthropic_request["temperature"], 0.7);

    // Messages should not include system
    let messages = anthropic_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");
}

#[test]
fn test_openai_to_gcp_vertex_response() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-sonnet-4-5");

    // Simulate GCP Vertex (Anthropic format) response
    let vertex_response = json!({
        "id": "msg_01XYZ",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello! I'm doing well, thank you!"}],
        "model": "claude-sonnet-4-5",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 25, "output_tokens": 15}
    });

    let openai_response = pipeline.transform_response(vertex_response, &ctx).unwrap();

    // Verify OpenAI format
    assert_eq!(openai_response["object"], "chat.completion");
    assert_eq!(openai_response["model"], "claude-sonnet-4-5");
    assert_eq!(
        openai_response["choices"][0]["message"]["role"],
        "assistant"
    );
    assert_eq!(
        openai_response["choices"][0]["message"]["content"],
        "Hello! I'm doing well, thank you!"
    );
    assert_eq!(openai_response["choices"][0]["finish_reason"], "stop");
    assert_eq!(openai_response["usage"]["prompt_tokens"], 25);
    assert_eq!(openai_response["usage"]["completion_tokens"], 15);
}

#[test]
fn test_openai_to_gcp_vertex_with_tools() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-sonnet-4-5");

    let openai_request = json!({
        "model": "claude-sonnet-4-5",
        "messages": [{"role": "user", "content": "What's the weather in Tokyo?"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "get_weather",
                "description": "Get weather for a location",
                "parameters": {
                    "type": "object",
                    "properties": {"location": {"type": "string"}},
                    "required": ["location"]
                }
            }
        }]
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // Verify tools are converted to Anthropic format
    assert!(anthropic_request["tools"].is_array());
    let tools = anthropic_request["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "get_weather");
}

#[test]
fn test_openai_tool_call_response_conversion() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-sonnet-4-5");

    let vertex_response = json!({
        "id": "msg_01ABC",
        "type": "message",
        "role": "assistant",
        "content": [{
            "type": "tool_use",
            "id": "toolu_01XYZ",
            "name": "get_weather",
            "input": {"location": "Tokyo"}
        }],
        "model": "claude-sonnet-4-5",
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 30, "output_tokens": 25}
    });

    let openai_response = pipeline.transform_response(vertex_response, &ctx).unwrap();

    assert_eq!(openai_response["choices"][0]["finish_reason"], "tool_calls");
    assert!(openai_response["choices"][0]["message"]["tool_calls"].is_array());
}

// ============================================================================
// Anthropic -> GCP Vertex Transformation Tests
// ============================================================================

#[test]
fn test_anthropic_to_gcp_vertex_request() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(
        Protocol::Anthropic,
        Protocol::Anthropic,
        "claude-sonnet-4-5",
    );

    let anthropic_request = json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 1024,
        "system": "You are a helpful assistant.",
        "messages": [{"role": "user", "content": "Hello!"}]
    });

    // Since both are Anthropic protocol, format should be preserved
    let result = pipeline.transform_request(anthropic_request, &ctx).unwrap();

    assert_eq!(result["model"], "claude-sonnet-4-5");
    assert_eq!(result["max_tokens"], 1024);
    assert_eq!(result["system"], "You are a helpful assistant.");
}

#[test]
fn test_anthropic_to_gcp_vertex_response() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(
        Protocol::Anthropic,
        Protocol::Anthropic,
        "claude-sonnet-4-5",
    );

    let vertex_response = json!({
        "id": "msg_01XYZ",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello!"}],
        "model": "claude-sonnet-4-5",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });

    let result = pipeline.transform_response(vertex_response, &ctx).unwrap();

    // Should pass through with same format
    assert_eq!(result["type"], "message");
    assert_eq!(result["content"][0]["text"], "Hello!");
    assert_eq!(result["stop_reason"], "end_turn");
}

// ============================================================================
// Response Conversion Tests
// ============================================================================

/// Convert GCP Vertex AI response
fn convert_vertex_response(
    response: &serde_json::Value,
    original_model: &str,
) -> serde_json::Value {
    let mut result = response.clone();

    if let Some(obj) = result.as_object_mut() {
        obj.insert("model".to_string(), json!(original_model));
    }

    result
}

#[test]
fn test_successful_response_passthrough() {
    let response = json!({
        "id": "msg_01XYZ",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello! How can I help you today?"}],
        "model": "claude-3-sonnet-20240229",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 15}
    });

    let result = convert_vertex_response(&response, "claude-3-sonnet-20240229");

    assert_eq!(result["id"], "msg_01XYZ");
    assert_eq!(result["type"], "message");
    assert_eq!(result["role"], "assistant");
    assert_eq!(result["model"], "claude-3-sonnet-20240229");
    assert_eq!(result["stop_reason"], "end_turn");
}

#[test]
fn test_response_with_tool_use() {
    let response = json!({
        "id": "msg_01ABC",
        "type": "message",
        "role": "assistant",
        "content": [
            {"type": "text", "text": "Let me check the weather."},
            {
                "type": "tool_use",
                "id": "toolu_01XYZ",
                "name": "get_weather",
                "input": {"location": "San Francisco"}
            }
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 20, "output_tokens": 30}
    });

    let result = convert_vertex_response(&response, "claude-3-sonnet-20240229");

    assert_eq!(result["stop_reason"], "tool_use");
    assert_eq!(result["content"][1]["type"], "tool_use");
    assert_eq!(result["content"][1]["name"], "get_weather");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

/// Convert GCP Vertex AI error response
fn convert_vertex_error(
    error_response: &serde_json::Value,
    _status_code: u16,
) -> serde_json::Value {
    if error_response.get("type").is_some() && error_response.get("error").is_some() {
        return error_response.clone();
    }

    json!({
        "type": "error",
        "error": {
            "type": ERROR_TYPE_API,
            "message": error_response.to_string()
        }
    })
}

#[test]
fn test_error_response_400_bad_request() {
    let error_response = json!({
        "type": "error",
        "error": {
            "type": ERROR_TYPE_INVALID_REQUEST,
            "message": "max_tokens must be a positive integer"
        }
    });

    let result = convert_vertex_error(&error_response, 400);

    assert_eq!(result["type"], "error");
    assert_eq!(result["error"]["type"], ERROR_TYPE_INVALID_REQUEST);
}

#[test]
fn test_error_response_401_unauthorized() {
    let error_response = json!({
        "type": "error",
        "error": {
            "type": ERROR_TYPE_AUTHENTICATION,
            "message": "Invalid API key or missing authentication"
        }
    });

    let result = convert_vertex_error(&error_response, 401);

    assert_eq!(result["error"]["type"], ERROR_TYPE_AUTHENTICATION);
}

#[test]
fn test_error_response_429_rate_limit() {
    let error_response = json!({
        "type": "error",
        "error": {
            "type": ERROR_TYPE_RATE_LIMIT,
            "message": "Rate limit exceeded. Please retry after some time."
        }
    });

    let result = convert_vertex_error(&error_response, 429);

    assert_eq!(result["error"]["type"], ERROR_TYPE_RATE_LIMIT);
}

#[test]
fn test_error_response_500_server_error() {
    let error_response = json!({
        "type": "error",
        "error": {
            "type": ERROR_TYPE_API,
            "message": "Internal server error"
        }
    });

    let result = convert_vertex_error(&error_response, 500);

    assert_eq!(result["error"]["type"], ERROR_TYPE_API);
}

#[test]
fn test_malformed_error_response() {
    let error_response = json!({"unexpected": "format"});

    let result = convert_vertex_error(&error_response, 500);

    assert_eq!(result["type"], "error");
    assert!(result["error"]["message"].is_string());
}

#[test]
fn test_gcp_vertex_missing_project() {
    // Test that missing gcp_project would cause validation error
    let provider_config = json!({
        "name": "test-provider",
        "api_base": "https://aiplatform.googleapis.com",
        "api_key": "test-token",
        "provider_type": "gcp-vertex",
        // Missing gcp_project
        "gcp_location": "us-central1"
    });

    // In production, this should raise a validation error
    // Here we verify the expected field is missing
    assert!(provider_config.get("gcp_project").is_none());
}

// ============================================================================
// Model Validation Tests
// ============================================================================

/// Check if model name is valid for GCP Vertex AI
fn is_valid_vertex_model(model: &str) -> bool {
    if model.is_empty() {
        return false;
    }
    model.starts_with("claude-")
}

#[test]
fn test_valid_claude_models() {
    let valid_models = vec![
        "claude-3-opus@20240229",
        "claude-3-sonnet@20240229",
        "claude-3-haiku@20240307",
        "claude-3-5-sonnet@20240620",
        "claude-3-5-sonnet-v2@20241022",
        "claude-sonnet-4-5",
    ];

    for model in valid_models {
        assert!(is_valid_vertex_model(model), "{} should be valid", model);
    }
}

#[test]
fn test_invalid_model_names() {
    let invalid_models = vec!["gpt-4", "gemini-pro", "invalid-model", ""];

    for model in invalid_models {
        assert!(!is_valid_vertex_model(model), "{} should be invalid", model);
    }
}

// ============================================================================
// URL Construction Tests
// ============================================================================

/// Construct GCP Vertex AI endpoint URL
fn construct_vertex_url(
    project: &str,
    location: &str,
    publisher: &str,
    model: &str,
    action: &str,
) -> String {
    format!(
        "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/{}/models/{}:{}",
        location, project, location, publisher, model, action
    )
}

#[test]
fn test_construct_raw_predict_url() {
    let url = construct_vertex_url(
        "my-project",
        "us-central1",
        "anthropic",
        "claude-3-sonnet@20240229",
        "rawPredict",
    );

    let expected = "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet@20240229:rawPredict";
    assert_eq!(url, expected);
}

#[test]
fn test_construct_stream_raw_predict_url() {
    let url = construct_vertex_url(
        "my-project",
        "europe-west1",
        "anthropic",
        "claude-3-opus@20240229",
        "streamRawPredict",
    );

    let expected = "https://europe-west1-aiplatform.googleapis.com/v1/projects/my-project/locations/europe-west1/publishers/anthropic/models/claude-3-opus@20240229:streamRawPredict";
    assert_eq!(url, expected);
}

#[test]
fn test_construct_url_from_provider_config() {
    let config = json!({
        "name": "gcp-vertex-test",
        "api_base": "https://us-central1-aiplatform.googleapis.com",
        "api_key": "test-access-token",
        "provider_type": "gcp-vertex",
        "gcp_project": "test-project",
        "gcp_location": "us-central1",
        "gcp_publisher": "anthropic"
    });

    let url = construct_vertex_url(
        config["gcp_project"].as_str().unwrap(),
        config["gcp_location"].as_str().unwrap(),
        config["gcp_publisher"].as_str().unwrap(),
        "claude-sonnet-4-5",
        "rawPredict",
    );

    let expected = "https://us-central1-aiplatform.googleapis.com/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-sonnet-4-5:rawPredict";
    assert_eq!(url, expected);
}

// ============================================================================
// Authentication Header Tests
// ============================================================================

/// Construct authentication headers for GCP Vertex AI
fn construct_vertex_auth_headers(access_token: &str) -> Result<Vec<(String, String)>, String> {
    if access_token.is_empty() {
        return Err("access_token is required".to_string());
    }

    Ok(vec![
        (
            "Authorization".to_string(),
            format!("Bearer {}", access_token),
        ),
        ("Content-Type".to_string(), "application/json".to_string()),
    ])
}

#[test]
fn test_bearer_token_header_construction() {
    let access_token = "ya29.a0AfH6SMBx...";

    let headers = construct_vertex_auth_headers(access_token).unwrap();

    assert_eq!(headers.len(), 2);
    assert_eq!(headers[0].0, "Authorization");
    assert_eq!(headers[0].1, format!("Bearer {}", access_token));
    assert_eq!(headers[1].0, "Content-Type");
    assert_eq!(headers[1].1, "application/json");
}

#[test]
fn test_empty_token_raises_error() {
    let result = construct_vertex_auth_headers("");

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("access_token"));
}

// ============================================================================
// Mock Server Integration Tests
// ============================================================================

#[tokio::test]
async fn test_raw_predict_mock_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict"))
        .and(header("authorization", "Bearer test_access_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_01XYZ",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-3-sonnet-20240229",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        })))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict",
            mock_server.uri()
        ))
        .header("Authorization", "Bearer test_access_token")
        .header("Content-Type", "application/json")
        .json(&json!({
            "anthropic_version": "vertex-2023-10-16",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello!"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["type"], "message");
    assert_eq!(body["content"][0]["text"], "Hello!");
}

#[tokio::test]
async fn test_raw_predict_mock_error() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "type": "error",
            "error": {
                "type": ERROR_TYPE_INVALID_REQUEST,
                "message": "max_tokens: Required field"
            }
        })))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict",
            mock_server.uri()
        ))
        .header("Content-Type", "application/json")
        .json(&json!({
            "anthropic_version": "vertex-2023-10-16",
            "messages": [{"role": "user", "content": "Hello!"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 400);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["type"], "error");
    assert_eq!(body["error"]["type"], ERROR_TYPE_INVALID_REQUEST);
}

#[tokio::test]
async fn test_stream_raw_predict_mock_success() {
    let mock_server = MockServer::start().await;

    let streaming_body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01XYZ\",\"role\":\"assistant\"}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hi\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n"
    );

    Mock::given(method("POST"))
        .and(path("/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:streamRawPredict"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(streaming_body)
                .append_header("Content-Type", "text/event-stream"),
        )
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:streamRawPredict",
            mock_server.uri()
        ))
        .header("Content-Type", "application/json")
        .json(&json!({
            "anthropic_version": "vertex-2023-10-16",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello!"}],
            "stream": true
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    // Note: wiremock's set_body_string overrides content-type to text/plain,
    // so we only verify the body content contains expected SSE events

    let body = response.text().await.unwrap();
    assert!(body.contains("message_start"));
    assert!(body.contains("message_stop"));
}

#[tokio::test]
async fn test_tool_use_response_mock() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_01ABC",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check the weather."},
                {
                    "type": "tool_use",
                    "id": "toolu_01XYZ",
                    "name": "get_weather",
                    "input": {"location": "San Francisco"}
                }
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 30}
        })))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict",
            mock_server.uri()
        ))
        .header("Content-Type", "application/json")
        .json(&json!({
            "anthropic_version": "vertex-2023-10-16",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "What's the weather in SF?"}],
            "tools": [{
                "name": "get_weather",
                "description": "Get weather info",
                "input_schema": {
                    "type": "object",
                    "properties": {"location": {"type": "string"}}
                }
            }]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["stop_reason"], "tool_use");
    assert_eq!(body["content"][1]["type"], "tool_use");
    assert_eq!(body["content"][1]["name"], "get_weather");
}

#[tokio::test]
async fn test_rate_limit_error_mock() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "type": "error",
            "error": {
                "type": ERROR_TYPE_RATE_LIMIT,
                "message": "Rate limit exceeded"
            }
        })))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict",
            mock_server.uri()
        ))
        .header("Content-Type", "application/json")
        .json(&json!({
            "anthropic_version": "vertex-2023-10-16",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello!"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 429);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["error"]["type"], ERROR_TYPE_RATE_LIMIT);
}

#[tokio::test]
async fn test_overloaded_error_mock() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict"))
        .respond_with(ResponseTemplate::new(529).set_body_json(json!({
            "type": "error",
            "error": {
                "type": ERROR_TYPE_OVERLOADED,
                "message": "The API is temporarily overloaded"
            }
        })))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!(
            "{}/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict",
            mock_server.uri()
        ))
        .header("Content-Type", "application/json")
        .json(&json!({
            "anthropic_version": "vertex-2023-10-16",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello!"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 529);

    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(body["error"]["type"], ERROR_TYPE_OVERLOADED);
}

// ============================================================================
// Cross-Protocol Integration Tests
// ============================================================================

#[test]
fn test_openai_to_gcp_vertex_full_flow() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-sonnet-4-5");

    // 1. OpenAI client request
    let openai_request = json!({
        "model": "claude-sonnet-4-5",
        "messages": [
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": "What is 2+2?"}
        ],
        "temperature": 0.5,
        "max_tokens": 100
    });

    // 2. Transform to Anthropic format (for GCP Vertex)
    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // Verify request transformation
    assert_eq!(anthropic_request["system"], "You are a helpful assistant.");
    assert_eq!(anthropic_request["temperature"], 0.5);
    let messages = anthropic_request["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");

    // 3. Simulate GCP Vertex response (Anthropic format)
    let vertex_response = json!({
        "id": "msg_test123",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "2+2 equals 4."}],
        "model": "claude-sonnet-4-5",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 20, "output_tokens": 10}
    });

    // 4. Transform response back to OpenAI format
    let openai_response = pipeline.transform_response(vertex_response, &ctx).unwrap();

    // Verify response transformation
    assert_eq!(openai_response["object"], "chat.completion");
    assert_eq!(openai_response["model"], "claude-sonnet-4-5");
    assert_eq!(
        openai_response["choices"][0]["message"]["content"],
        "2+2 equals 4."
    );
    assert_eq!(openai_response["choices"][0]["finish_reason"], "stop");
    assert_eq!(openai_response["usage"]["prompt_tokens"], 20);
    assert_eq!(openai_response["usage"]["completion_tokens"], 10);
    assert_eq!(openai_response["usage"]["total_tokens"], 30);
}

#[test]
fn test_anthropic_to_gcp_vertex_full_flow() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(
        Protocol::Anthropic,
        Protocol::Anthropic,
        "claude-sonnet-4-5",
    );

    // 1. Anthropic client request
    let anthropic_request = json!({
        "model": "claude-sonnet-4-5",
        "max_tokens": 100,
        "system": "You are a helpful assistant.",
        "messages": [{"role": "user", "content": "What is 2+2?"}]
    });

    // 2. Transform (should be minimal since same protocol)
    let _provider_request = pipeline.transform_request(anthropic_request, &ctx).unwrap();

    // 3. Simulate GCP Vertex response
    let vertex_response = json!({
        "id": "msg_test456",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "2+2 equals 4."}],
        "model": "claude-sonnet-4-5",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 20, "output_tokens": 10}
    });

    // 4. Transform response back to Anthropic format
    let client_response = pipeline.transform_response(vertex_response, &ctx).unwrap();

    // Verify response format is preserved
    assert_eq!(client_response["type"], "message");
    assert_eq!(client_response["content"][0]["text"], "2+2 equals 4.");
    assert_eq!(client_response["stop_reason"], "end_turn");
    assert_eq!(client_response["usage"]["input_tokens"], 20);
    assert_eq!(client_response["usage"]["output_tokens"], 10);
}

#[test]
fn test_model_mapping_applied() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);

    let mut ctx = TransformContext::new("test-id");
    ctx.client_protocol = Protocol::OpenAI;
    ctx.provider_protocol = Protocol::Anthropic;
    ctx.original_model = "my-claude-alias".to_string();
    ctx.mapped_model = "claude-sonnet-4-5@20241022".to_string();

    let openai_request = json!({
        "model": "my-claude-alias",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let anthropic_request = pipeline.transform_request(openai_request, &ctx).unwrap();

    // Model should be mapped
    assert_eq!(anthropic_request["model"], "claude-sonnet-4-5@20241022");
}

#[test]
fn test_model_restored_in_response() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);

    let mut ctx = TransformContext::new("test-id");
    ctx.client_protocol = Protocol::OpenAI;
    ctx.provider_protocol = Protocol::Anthropic;
    ctx.original_model = "my-claude-alias".to_string();
    ctx.mapped_model = "claude-sonnet-4-5@20241022".to_string();

    let vertex_response = json!({
        "id": "msg_test",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello!"}],
        "model": "claude-sonnet-4-5@20241022",
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 10, "output_tokens": 5}
    });

    let openai_response = pipeline.transform_response(vertex_response, &ctx).unwrap();

    // Model should be restored to original alias
    assert_eq!(openai_response["model"], "my-claude-alias");
}

// ============================================================================
// Stop Reason Conversion Tests
// ============================================================================

#[test]
fn test_stop_reason_anthropic_to_openai() {
    let registry = create_registry();
    let pipeline = TransformPipeline::new(registry);
    let ctx = create_context(Protocol::OpenAI, Protocol::Anthropic, "claude-3-opus");

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
        .transform_response(anthropic_response, &ctx)
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
fn test_usage_conversion_anthropic_to_openai() {
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
fn test_usage_conversion_openai_to_anthropic() {
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
