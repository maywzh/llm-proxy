//! V2 API endpoint authentication tests.
//!
//! These tests verify that V2 endpoints properly enforce authentication:
//! - /v2/chat/completions requires authentication
//! - /v2/messages requires authentication
//! - /v2/responses requires authentication
//! - /v2/models requires authentication
//! - /v2/model/info requires authentication
//! - Invalid credentials return 401
//! - Both Bearer token and x-api-key authentication methods work

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
    Router,
};
use llm_proxy_rust::{
    api::{
        chat_completions_v2, count_tokens_v2, list_model_info_v2, list_models_v2, messages_v2,
        responses_v2, AppState, ProxyState,
    },
    core::{
        config::{CredentialConfig, ModelMappingValue, ProviderConfig, ServerConfig},
        database::hash_key,
        init_metrics, RateLimiter,
    },
    services::ProviderService,
};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tower::ServiceExt;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test config without authentication (no credentials)
fn create_test_config_no_auth(mock_server_uri: &str) -> llm_proxy_rust::core::AppConfig {
    let mut model_mapping: HashMap<String, ModelMappingValue> = HashMap::new();
    model_mapping.insert("gpt-4".to_string(), "test-gpt-4".into());
    model_mapping.insert("claude-3-opus".to_string(), "test-claude-3".into());

    llm_proxy_rust::core::AppConfig {
        providers: vec![ProviderConfig {
            name: "MockProvider".to_string(),
            api_base: mock_server_uri.to_string(),
            api_key: "test_key".to_string(),
            weight: 1,
            model_mapping,
            provider_type: "openai".to_string(),
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
    }
}

/// Create a test config with authentication (credentials configured)
fn create_test_config_with_auth(mock_server_uri: &str) -> llm_proxy_rust::core::AppConfig {
    let mut config = create_test_config_no_auth(mock_server_uri);
    // Store the hash of the key, not the plain text key
    config.credentials = vec![CredentialConfig {
        credential_key: hash_key("test_master_key"),
        name: "Test Key".to_string(),
        description: None,
        rate_limit: None,
        enabled: true,
        allowed_models: vec![],
    }];
    config
}

/// Create a V2 test app with the given config
fn create_v2_test_app_with_config(config: llm_proxy_rust::core::AppConfig) -> Router {
    init_metrics();

    let provider_service = ProviderService::new(config.clone());
    let rate_limiter = Arc::new(RateLimiter::new());

    let http_client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
        .pool_max_idle_per_host(20)
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to build HTTP client");

    let app_state = Arc::new(AppState::new(
        config,
        provider_service,
        rate_limiter,
        http_client,
        None,
    ));

    let proxy_state = Arc::new(ProxyState::new(app_state));

    Router::new()
        .route("/v2/chat/completions", post(chat_completions_v2))
        .route("/v2/messages", post(messages_v2))
        .route("/v2/responses", post(responses_v2))
        .route("/v2/models", get(list_models_v2))
        .route("/v2/model/info", get(list_model_info_v2))
        .route("/v2/messages/count_tokens", post(count_tokens_v2))
        .with_state(proxy_state)
}

/// Standard OpenAI response for mocking
fn openai_response() -> serde_json::Value {
    json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "created": 1677652288,
        "model": "test-gpt-4",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello! How can I help you today?"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 12,
            "total_tokens": 22
        }
    })
}

// ============================================================================
// Authentication Required Tests
// ============================================================================

/// Test that /v2/chat/completions requires authentication when credentials are configured
#[tokio::test]
async fn test_v2_chat_completions_requires_auth() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/chat/completions")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized when no auth header provided"
    );
}

/// Test that /v2/messages requires authentication when credentials are configured
#[tokio::test]
async fn test_v2_messages_requires_auth() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/messages")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "max_tokens": 100,
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized when no auth header provided"
    );
}

/// Test that /v2/responses requires authentication when credentials are configured
#[tokio::test]
async fn test_v2_responses_requires_auth() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/responses")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "input": "hi"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized when no auth header provided"
    );
}

/// Test that /v2/models requires authentication when credentials are configured
#[tokio::test]
async fn test_v2_models_requires_auth() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("GET")
        .uri("/v2/models")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized when no auth header provided"
    );
}

/// Test that /v2/model/info requires authentication when credentials are configured
#[tokio::test]
async fn test_v2_model_info_requires_auth() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("GET")
        .uri("/v2/model/info")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized when no auth header provided"
    );
}

/// Test that /v2/messages/count_tokens requires authentication when credentials are configured
#[tokio::test]
async fn test_v2_count_tokens_requires_auth() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/messages/count_tokens")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized when no auth header provided"
    );
}

// ============================================================================
// Invalid Authentication Tests
// ============================================================================

/// Test V2 chat/completions with invalid Bearer token
#[tokio::test]
async fn test_v2_chat_completions_with_invalid_bearer_auth() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer invalid-key")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized with invalid Bearer token"
    );
}

/// Test V2 messages with invalid x-api-key
#[tokio::test]
async fn test_v2_messages_with_invalid_x_api_key() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/messages")
        .header("Content-Type", "application/json")
        .header("x-api-key", "invalid-key")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "max_tokens": 100,
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized with invalid x-api-key"
    );
}

/// Test V2 models with malformed Authorization header
#[tokio::test]
async fn test_v2_models_with_malformed_auth_header() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("GET")
        .uri("/v2/models")
        .header("Authorization", "InvalidFormat")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 Unauthorized with malformed Authorization header"
    );
}

// ============================================================================
// Valid Authentication Tests
// ============================================================================

/// Test V2 chat/completions with valid Bearer token
#[tokio::test]
async fn test_v2_chat_completions_with_valid_bearer_auth() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer test_master_key")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK with valid Bearer token"
    );
}

/// Test V2 messages with valid x-api-key authentication
#[tokio::test]
async fn test_v2_messages_with_valid_x_api_key() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/messages")
        .header("Content-Type", "application/json")
        .header("x-api-key", "test_master_key")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "max_tokens": 100,
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK with valid x-api-key"
    );
}

/// Test V2 responses with valid Bearer token
#[tokio::test]
async fn test_v2_responses_with_valid_bearer_auth() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/responses")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer test_master_key")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "input": "hi"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK with valid Bearer token"
    );
}

/// Test V2 models with valid Bearer token
#[tokio::test]
async fn test_v2_models_with_valid_bearer_auth() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("GET")
        .uri("/v2/models")
        .header("Authorization", "Bearer test_master_key")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK with valid Bearer token"
    );

    // Verify response contains model list
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["object"], "list");
    assert!(json["data"].is_array());
}

/// Test V2 model/info with valid x-api-key
#[tokio::test]
async fn test_v2_model_info_with_valid_x_api_key() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("GET")
        .uri("/v2/model/info")
        .header("x-api-key", "test_master_key")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK with valid x-api-key"
    );

    // Verify response contains model info
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(json["data"].is_array());
}

// ============================================================================
// No Auth Required Tests (when credentials are empty)
// ============================================================================

/// Test V2 chat/completions works without auth when no credentials configured
#[tokio::test]
async fn test_v2_chat_completions_no_auth_required_when_no_credentials() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let config = create_test_config_no_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/chat/completions")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK when no credentials configured"
    );
}

/// Test V2 models works without auth when no credentials configured
#[tokio::test]
async fn test_v2_models_no_auth_required_when_no_credentials() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_no_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("GET")
        .uri("/v2/models")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK when no credentials configured"
    );
}

// ============================================================================
// Error Response Format Tests
// ============================================================================

/// Test that 401 error response has proper format
#[tokio::test]
async fn test_v2_auth_error_response_format() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    let request = Request::builder()
        .method("POST")
        .uri("/v2/chat/completions")
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Check error response format (OpenAI-compatible)
    assert!(
        json["error"].is_object(),
        "Expected error object in response"
    );
    assert!(
        json["error"]["message"].is_string(),
        "Expected error message"
    );
    assert!(json["error"]["type"].is_string(), "Expected error type");
    assert!(json["error"]["code"].is_number(), "Expected error code");
}

// ============================================================================
// x-api-key Priority Tests
// ============================================================================

/// Test that x-api-key takes precedence over Authorization header
#[tokio::test]
async fn test_v2_x_api_key_takes_precedence() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    // Provide valid x-api-key but invalid Bearer token
    // x-api-key should take precedence and request should succeed
    let request = Request::builder()
        .method("POST")
        .uri("/v2/chat/completions")
        .header("Content-Type", "application/json")
        .header("x-api-key", "test_master_key")
        .header("Authorization", "Bearer invalid-key")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Expected 200 OK when x-api-key is valid (should take precedence over invalid Bearer)"
    );
}

/// Test that invalid x-api-key fails even with valid Bearer token
#[tokio::test]
async fn test_v2_invalid_x_api_key_fails_with_valid_bearer() {
    let mock_server = MockServer::start().await;
    let config = create_test_config_with_auth(&mock_server.uri());
    let app = create_v2_test_app_with_config(config);

    // Provide invalid x-api-key but valid Bearer token
    // x-api-key should take precedence and request should fail
    let request = Request::builder()
        .method("POST")
        .uri("/v2/chat/completions")
        .header("Content-Type", "application/json")
        .header("x-api-key", "invalid-key")
        .header("Authorization", "Bearer test_master_key")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "hi"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Expected 401 when x-api-key is invalid (should take precedence over valid Bearer)"
    );
}
