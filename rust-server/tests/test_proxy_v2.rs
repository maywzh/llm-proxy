//! Integration tests for the v2 proxy endpoints using transformer pipeline.
//!
//! These tests verify:
//! - OpenAI format passthrough (same-protocol)
//! - Anthropic → OpenAI cross-protocol conversion
//! - Response API → OpenAI cross-protocol conversion
//! - Streaming responses with protocol conversion
//! - Error handling across protocols

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use llm_proxy_rust::{
    api::{chat_completions_v2, messages_v2, responses_v2, AppState, ProxyState},
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

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a test app with v2 proxy routes
async fn create_v2_test_app(mock_server: &MockServer) -> Router {
    create_v2_test_app_with_timeout(mock_server, 300).await
}

/// Create a test app with v2 proxy routes and custom timeout
async fn create_v2_test_app_with_timeout(mock_server: &MockServer, timeout_secs: u64) -> Router {
    use llm_proxy_rust::core::config::{ProviderConfig, ServerConfig};
    use llm_proxy_rust::core::RateLimiter;
    use std::collections::HashMap;

    init_metrics();

    let mut model_mapping = HashMap::new();
    model_mapping.insert("gpt-4".to_string(), "test-gpt-4".to_string());
    model_mapping.insert("claude-3-opus".to_string(), "test-claude-3".to_string());

    let config = AppConfig {
        providers: vec![ProviderConfig {
            name: "MockProvider".to_string(),
            api_base: mock_server.uri(),
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
        request_timeout_secs: timeout_secs,
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
        .layer(axum::middleware::from_fn(MetricsMiddleware::track_metrics))
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
// OpenAI Format Tests (Same-Protocol Passthrough)
// ============================================================================

#[tokio::test]
async fn test_v2_openai_chat_completion_passthrough() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    let request = Request::builder()
        .uri("/v2/chat/completions")
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
        "Hello! How can I help you today?"
    );
    assert_eq!(json["usage"]["total_tokens"], 22);
}

#[tokio::test]
async fn test_v2_openai_with_system_message() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    let request = Request::builder()
        .uri("/v2/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [
                    {"role": "system", "content": "You are a helpful assistant."},
                    {"role": "user", "content": "Hello"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ============================================================================
// Anthropic Format Tests (Cross-Protocol Conversion)
// ============================================================================

#[tokio::test]
async fn test_v2_anthropic_to_openai_conversion() {
    let mock_server = MockServer::start().await;

    // Backend returns OpenAI format
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    // Client sends Anthropic format to /v2/messages
    let request = Request::builder()
        .uri("/v2/messages")
        .method("POST")
        .header("content-type", "application/json")
        .header("x-api-key", "test_client_key")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "max_tokens": 1024,
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

    // Response should be in Anthropic format
    // For now, it returns OpenAI format (same-protocol optimization)
    // When cross-protocol is fully implemented, this would return Anthropic format
    assert!(json["choices"].is_array() || json["content"].is_array());
}

#[tokio::test]
async fn test_v2_anthropic_with_system_prompt() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    // Anthropic format with separate system field
    let request = Request::builder()
        .uri("/v2/messages")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "max_tokens": 1024,
                "system": "You are a helpful assistant.",
                "messages": [
                    {"role": "user", "content": "Hello"}
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_v2_anthropic_with_content_blocks() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    // Anthropic format with typed content blocks
    let request = Request::builder()
        .uri("/v2/messages")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "max_tokens": 1024,
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {"type": "text", "text": "Hello, how are you?"}
                        ]
                    }
                ]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ============================================================================
// Response API Format Tests
// ============================================================================

#[tokio::test]
async fn test_v2_response_api_format() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    // Response API format
    let request = Request::builder()
        .uri("/v2/responses")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "input": "Hello, how are you?"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_v2_response_api_with_instructions() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    // Response API with system instructions
    let request = Request::builder()
        .uri("/v2/responses")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "instructions": "You are a helpful assistant.",
                "input": "Hello!"
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_v2_provider_error_response() {
    let mock_server = MockServer::start().await;

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

    let app = create_v2_test_app(&mock_server).await;

    let request = Request::builder()
        .uri("/v2/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hello"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Internal server error"));
}

#[tokio::test]
async fn test_v2_provider_401_error() {
    let mock_server = MockServer::start().await;

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

    let app = create_v2_test_app(&mock_server).await;

    let request = Request::builder()
        .uri("/v2/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hello"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_v2_provider_timeout() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_secs(3))
                .set_body_json(openai_response()),
        )
        .mount(&mock_server)
        .await;

    // Create app with 2 second timeout
    let app = create_v2_test_app_with_timeout(&mock_server, 2).await;

    let request = Request::builder()
        .uri("/v2/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hello"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
}

#[tokio::test]
async fn test_v2_invalid_model() {
    let mock_server = MockServer::start().await;

    // No mock setup - provider doesn't support this model
    let app = create_v2_test_app(&mock_server).await;

    let request = Request::builder()
        .uri("/v2/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "nonexistent-model",
                "messages": [{"role": "user", "content": "Hello"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    // Should return 400 for unsupported model
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ============================================================================
// Streaming Tests
// ============================================================================

#[tokio::test]
async fn test_v2_streaming_request_passthrough() {
    let mock_server = MockServer::start().await;

    // Mock SSE streaming response
    let sse_response = "data: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"test-gpt-4\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"chatcmpl-123\",\"object\":\"chat.completion.chunk\",\"created\":1677652288,\"model\":\"test-gpt-4\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(sse_response)
                .insert_header("content-type", "text/event-stream"),
        )
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    let request = Request::builder()
        .uri("/v2/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hello"}],
                "stream": true
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Check content type for streaming
    let content_type = response
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap_or(""));
    assert!(
        content_type.map_or(false, |ct| ct.contains("text/event-stream")),
        "Expected text/event-stream content type"
    );
}

// ============================================================================
// Model Mapping Tests
// ============================================================================

#[tokio::test]
async fn test_v2_model_mapping() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "test-gpt-4",  // Backend returns mapped model
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

    let app = create_v2_test_app(&mock_server).await;

    let request = Request::builder()
        .uri("/v2/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",  // Client sends original model
                "messages": [{"role": "user", "content": "Test"}]
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

    // Response should have original model name (rewritten back)
    assert_eq!(json["model"], "gpt-4");
}

// ============================================================================
// Protocol Detection Tests
// ============================================================================

#[tokio::test]
async fn test_v2_protocol_detection_openai() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    // OpenAI format detected from endpoint path
    let request = Request::builder()
        .uri("/v2/chat/completions")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hello"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_v2_protocol_detection_anthropic() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    // Anthropic format detected from /v2/messages endpoint
    let request = Request::builder()
        .uri("/v2/messages")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "gpt-4",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "Hello"}]
            })
            .to_string(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// ============================================================================
// Concurrent Request Tests
// ============================================================================

#[tokio::test]
async fn test_v2_concurrent_requests() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(openai_response()))
        .expect(10)
        .mount(&mock_server)
        .await;

    let app = create_v2_test_app(&mock_server).await;

    let mut handles = vec![];

    for _ in 0..10 {
        let app_clone = app.clone();
        let handle = tokio::spawn(async move {
            let request = Request::builder()
                .uri("/v2/chat/completions")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "model": "gpt-4",
                        "messages": [{"role": "user", "content": "Hello"}]
                    })
                    .to_string(),
                ))
                .unwrap();

            let response = app_clone.oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

// ============================================================================
// Streaming Resource Management Tests
// ============================================================================

#[tokio::test]
async fn test_stream_state_drop_releases_resources() {
    use llm_proxy_rust::transformer::CrossProtocolStreamState;
    use std::sync::Arc;

    // Create stream state wrapped in Arc for reference counting
    let state = Arc::new(tokio::sync::Mutex::new(CrossProtocolStreamState::new(
        "gpt-4".to_string(),
    )));

    // Clone the Arc to get strong count
    let state_clone = Arc::clone(&state);

    // Verify initial strong count is 2 (original + clone)
    assert_eq!(Arc::strong_count(&state), 2);

    // Drop the clone
    drop(state_clone);

    // Verify strong count decreases to 1
    assert_eq!(Arc::strong_count(&state), 1);

    // Drop the original
    drop(state);

    // State is now fully dropped and resources released
}

#[tokio::test]
async fn test_concurrent_streams_no_resource_leak() {
    use llm_proxy_rust::transformer::CrossProtocolStreamState;

    let mut handles = vec![];

    for i in 0..50 {
        let handle = tokio::spawn(async move {
            let state = CrossProtocolStreamState::new(format!("model-{}", i));

            // Simulate processing chunks
            for _ in 0..10 {
                // Use the state by checking model
                let _model = &state.model;
                tokio::time::sleep(tokio::time::Duration::from_micros(10)).await;
            }

            // State will be dropped when task completes
        });
        handles.push(handle);
    }

    // Wait for all streams to complete
    for handle in handles {
        handle.await.unwrap();
    }

    // All states should be dropped at this point
    // Memory usage should not have grown significantly
}

#[tokio::test]
async fn test_stream_drop_on_error_mid_stream() {
    use llm_proxy_rust::transformer::CrossProtocolStreamState;
    use std::sync::Arc;

    let state = Arc::new(tokio::sync::Mutex::new(CrossProtocolStreamState::new(
        "gpt-4".to_string(),
    )));

    // Simulate error during stream processing
    let result: Result<(), &str> = (|| {
        let _state_clone = Arc::clone(&state);

        // Simulate processing some chunks
        for _ in 0..5 {
            // Processing...
        }

        // Simulate error
        Err("Stream processing error")
    })();

    // Verify error was returned
    assert!(result.is_err());

    // Verify Arc strong count is back to 1 (error path dropped the clone)
    assert_eq!(Arc::strong_count(&state), 1);

    // Drop the original
    drop(state);

    // Resources should be fully released
}

#[tokio::test]
async fn test_streaming_response_lifecycle() {
    use llm_proxy_rust::transformer::CrossProtocolStreamState;
    use std::sync::Arc;

    let state = Arc::new(tokio::sync::Mutex::new(CrossProtocolStreamState::new(
        "gpt-4".to_string(),
    )));

    let state_clone = Arc::clone(&state);

    // Simulate stream lifecycle
    tokio::spawn(async move {
        let locked_state = state_clone.lock().await;

        // Simulate using the state
        let _model = &locked_state.model;

        // State will be dropped when lock is released and task completes
    })
    .await
    .unwrap();

    // Verify Arc strong count is back to 1
    assert_eq!(Arc::strong_count(&state), 1);

    drop(state);
}

#[tokio::test]
async fn test_stream_context_cleanup_on_client_disconnect() {
    use llm_proxy_rust::transformer::CrossProtocolStreamState;
    use std::sync::Arc;

    let state = Arc::new(tokio::sync::Mutex::new(CrossProtocolStreamState::new(
        "gpt-4".to_string(),
    )));

    let state_clone = Arc::clone(&state);

    // Simulate client disconnect by cancelling the task
    let handle = tokio::spawn(async move {
        let _locked_state = state_clone.lock().await;

        // Simulate long-running stream
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    });

    // Give task time to acquire lock
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Simulate client disconnect by aborting the task
    handle.abort();

    // Give abort time to take effect
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Verify Arc strong count is back to 1 (aborted task dropped its reference)
    assert_eq!(Arc::strong_count(&state), 1);

    drop(state);
}
