//! Mock-based tests for external API interactions.
//!
//! These tests use wiremock to simulate LLM provider responses
//! without making actual HTTP requests.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use llm_proxy_rust::{
    api::{chat_completions, AppState},
    core::{init_metrics, AppConfig, MetricsMiddleware},
    services::ProviderService,
};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

/// Create a test app with mocked provider
async fn create_test_app_with_mock(mock_server: &MockServer) -> Router {
    create_test_app_with_timeout(mock_server, 300).await
}

/// Create a test app with mocked provider and custom timeout
async fn create_test_app_with_timeout(mock_server: &MockServer, timeout_secs: u64) -> Router {
    use llm_proxy_rust::core::config::{ProviderConfig, ServerConfig};
    use llm_proxy_rust::core::RateLimiter;
    use std::collections::HashMap;

    init_metrics();

    let mut model_mapping = HashMap::new();
    model_mapping.insert("gpt-4".to_string(), "test-gpt-4".to_string());

    let config = AppConfig {
        providers: vec![ProviderConfig {
            name: "MockProvider".to_string(),
            api_base: mock_server.uri(),
            api_key: "test_key".to_string(),
            weight: 1,
            model_mapping,
        }],
        server: ServerConfig {
            host: "0.0.0.0".to_string(),
            port: 18000,
        },
        verify_ssl: false,
        request_timeout_secs: timeout_secs,
        ttft_timeout_secs: None,
        credentials: vec![],
        provider_suffix: None,
    };

    let provider_service = ProviderService::new(config.clone());
    let rate_limiter = Arc::new(RateLimiter::new());
    
    // Create shared HTTP client for tests
    let http_client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
        .pool_max_idle_per_host(20)
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to build HTTP client");
    
    let state = Arc::new(AppState {
        config,
        provider_service,
        rate_limiter,
        http_client,
    });

    Router::new()
        .route(
            "/v1/chat/completions",
            axum::routing::post(chat_completions),
        )
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            MetricsMiddleware::track_metrics,
        ))
        .with_state(state)
}

#[tokio::test]
async fn test_successful_chat_completion() {
    let mock_server = MockServer::start().await;

    // Mock successful response
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "test-gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help you?"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 9,
                "total_tokens": 19
            }
        })))
        .mount(&mock_server)
        .await;

    let app = create_test_app_with_mock(&mock_server).await;

    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hello"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Model should be rewritten to original
    assert_eq!(json["model"], "gpt-4");
    assert_eq!(
        json["choices"][0]["message"]["content"],
        "Hello! How can I help you?"
    );
}

#[tokio::test]
async fn test_provider_error_response() {
    let mock_server = MockServer::start().await;

    // Mock error response
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_json(json!({
            "error": {
                "message": "Internal server error",
                "type": "server_error",
                "code": 500
            }
        })))
        .mount(&mock_server)
        .await;

    let app = create_test_app_with_mock(&mock_server).await;

    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hello"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should return 500 for backend API errors
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Should contain error message
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Internal server error"));
}

#[tokio::test]
async fn test_provider_401_error() {
    let mock_server = MockServer::start().await;

    // Mock 401 error response
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": {
                "message": "Invalid API key",
                "type": "authentication_error"
            }
        })))
        .mount(&mock_server)
        .await;

    let app = create_test_app_with_mock(&mock_server).await;

    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hello"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should faithfully return the backend's 401 status code
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Should contain error message
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Invalid API key"));
}

#[tokio::test]
async fn test_provider_error_with_string_error() {
    let mock_server = MockServer::start().await;

    // Mock error response with string error field
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(503).set_body_json(json!({
            "error": "Service temporarily unavailable"
        })))
        .mount(&mock_server)
        .await;

    let app = create_test_app_with_mock(&mock_server).await;

    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hello"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should faithfully return the backend's 503 status code
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // The backend returns {"error": "Service temporarily unavailable"}
    // which gets passed through as-is
    let error_msg = if let Some(msg) = json["error"]["message"].as_str() {
        msg
    } else if let Some(msg) = json["error"].as_str() {
        msg
    } else {
        panic!("Expected error message in response");
    };
    
    assert!(error_msg.contains("Service temporarily unavailable"));
}

#[tokio::test]
async fn test_provider_timeout() {
    let mock_server = MockServer::start().await;

    // Mock delayed response (simulating timeout)
    // Use 3 seconds delay which is longer than our test client timeout (2s)
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(3))
                .set_body_json(json!({
                    "id": "chatcmpl-123",
                    "object": "chat.completion",
                    "created": 1677652288,
                    "model": "test-gpt-4",
                    "choices": []
                })),
        )
        .mount(&mock_server)
        .await;

    // Create app with 2 second timeout
    let app = create_test_app_with_timeout(&mock_server, 2).await;

    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Hello"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should return timeout error
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
}

#[tokio::test]
async fn test_model_mapping() {
    let mock_server = MockServer::start().await;

    // Verify that the model is mapped correctly in the request
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "test-gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Response"
                },
                "finish_reason": "stop"
            }]
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let app = create_test_app_with_mock(&mock_server).await;

    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Test"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Response should have original model name
    assert_eq!(json["model"], "gpt-4");
}

#[tokio::test]
async fn test_token_usage_tracking() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "test-gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Response"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 50,
                "completion_tokens": 25,
                "total_tokens": 75
            }
        })))
        .mount(&mock_server)
        .await;

    let app = create_test_app_with_mock(&mock_server).await;

    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Test"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify usage is present in response
    assert_eq!(json["usage"]["prompt_tokens"], 50);
    assert_eq!(json["usage"]["completion_tokens"], 25);
    assert_eq!(json["usage"]["total_tokens"], 75);
}

#[tokio::test]
async fn test_multiple_requests_to_mock() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "test-gpt-4",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Response"
                },
                "finish_reason": "stop"
            }]
        })))
        .expect(3)
        .mount(&mock_server)
        .await;

    let app = create_test_app_with_mock(&mock_server).await;

    for _ in 0..3 {
        let request = Request::builder()
            .uri("/v1/chat/completions")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "model": "gpt-4",
                    "messages": [
                        {"role": "user", "content": "Test"}
                    ]
                })
                .to_string(),
            ))
            .unwrap();

        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn test_invalid_json_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string("invalid json"))
        .mount(&mock_server)
        .await;

    let app = create_test_app_with_mock(&mock_server).await;

    let request = Request::builder()
        .uri("/v1/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "user", "content": "Test"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Should return BAD_GATEWAY for invalid JSON from provider
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
}
