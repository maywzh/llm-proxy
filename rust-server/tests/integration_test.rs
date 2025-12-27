//! Integration tests for the LLM proxy server.
//!
//! These tests verify end-to-end functionality including:
//! - API endpoint behavior
//! - Authentication
//! - Error handling
//! - Provider selection
//! - Metrics collection

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use llm_proxy_rust::{
    api::{
        chat_completions, completions, health, health_detailed, list_models, metrics_handler,
        AppState,
    },
    core::{init_metrics, AppConfig, MetricsMiddleware},
    services::ProviderService,
};
// use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;
use tower_http::{cors::CorsLayer, trace::TraceLayer};

/// Create a test application with the given config
fn create_test_app(config: AppConfig) -> Router {
    use llm_proxy_rust::core::RateLimiter;

    init_metrics();

    let provider_service = ProviderService::new(config.clone());
    let rate_limiter = Arc::new(RateLimiter::new());
    let state = Arc::new(AppState {
        config,
        provider_service,
        rate_limiter,
    });

    Router::new()
        .route(
            "/v1/chat/completions",
            axum::routing::post(chat_completions),
        )
        .route("/v1/completions", axum::routing::post(completions))
        .route("/v1/models", axum::routing::get(list_models))
        .route("/health", axum::routing::get(health))
        .route("/health/detailed", axum::routing::get(health_detailed))
        .route("/metrics", axum::routing::get(metrics_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            MetricsMiddleware::track_metrics,
        ))
        .with_state(state)
}

/// Create a test config without authentication
fn create_test_config_no_auth() -> AppConfig {
    use llm_proxy_rust::core::config::{ProviderConfig, ServerConfig};
    use std::collections::HashMap;

    AppConfig {
        providers: vec![
            ProviderConfig {
                name: "TestProvider1".to_string(),
                api_base: "http://localhost:8000".to_string(),
                api_key: "test_key_1".to_string(),
                weight: 2,
                model_mapping: {
                    let mut map = HashMap::new();
                    map.insert("gpt-4".to_string(), "test-gpt-4".to_string());
                    map.insert("gpt-3.5-turbo".to_string(), "test-gpt-3.5".to_string());
                    map
                },
            },
            ProviderConfig {
                name: "TestProvider2".to_string(),
                api_base: "http://localhost:8001".to_string(),
                api_key: "test_key_2".to_string(),
                weight: 1,
                model_mapping: {
                    let mut map = HashMap::new();
                    map.insert("claude-3".to_string(), "test-claude-3".to_string());
                    map
                },
            },
        ],
        server: ServerConfig {
            host: "0.0.0.0".to_string(),
            port: 18000,
        },
        verify_ssl: false,
        master_keys: vec![],
    }
}

/// Create a test config with authentication
fn create_test_config_with_auth() -> AppConfig {
    use llm_proxy_rust::core::config::MasterKeyConfig;

    let mut config = create_test_config_no_auth();
    config.master_keys = vec![MasterKeyConfig {
        key: "test_master_key".to_string(),
        name: "Test Key".to_string(),
        description: None,
        rate_limit: None,
        enabled: true,
    }];
    config
}

#[tokio::test]
async fn test_health_endpoint() {
    let app = create_test_app(create_test_config_no_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert_eq!(json["providers"], 2);
    assert!(json["provider_info"].is_array());
}

#[tokio::test]
async fn test_health_detailed_endpoint() {
    let app = create_test_app(create_test_config_no_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/detailed")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert!(json.is_object());
    assert!(json.get("TestProvider1").is_some());
    assert!(json.get("TestProvider2").is_some());
}

#[tokio::test]
async fn test_list_models_endpoint() {
    let app = create_test_app(create_test_config_no_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["object"], "list");
    assert!(json["data"].is_array());

    let models: Vec<String> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();

    assert!(models.contains(&"gpt-4".to_string()));
    assert!(models.contains(&"gpt-3.5-turbo".to_string()));
    assert!(models.contains(&"claude-3".to_string()));
}

#[tokio::test]
async fn test_metrics_endpoint() {
    let app = create_test_app(create_test_config_no_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(body.to_vec()).unwrap();

    // Check for Prometheus metrics format
    assert!(text.contains("llm_proxy_requests_total"));
    assert!(text.contains("llm_proxy_request_duration_seconds"));
    assert!(text.contains("llm_proxy_active_requests"));
}

#[tokio::test]
async fn test_authentication_required() {
    let app = create_test_app(create_test_config_with_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_authentication_with_valid_key() {
    let app = create_test_app(create_test_config_with_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("Authorization", "Bearer test_master_key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_authentication_with_invalid_key() {
    let app = create_test_app(create_test_config_with_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("Authorization", "Bearer wrong_key")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_authentication_malformed_header() {
    let app = create_test_app(create_test_config_with_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .header("Authorization", "InvalidFormat")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_cors_headers() {
    let app = create_test_app(create_test_config_no_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("Origin", "http://example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // CORS headers should be present
    let headers = response.headers();
    assert!(headers.contains_key("access-control-allow-origin"));
}

#[tokio::test]
async fn test_not_found_endpoint() {
    let app = create_test_app(create_test_config_no_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_concurrent_requests() {
    let app = create_test_app(create_test_config_no_auth());

    let mut handles = vec![];

    for _ in 0..10 {
        let app_clone = app.clone();
        let handle = tokio::spawn(async move {
            let response = app_clone
                .oneshot(
                    Request::builder()
                        .uri("/health")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}

#[tokio::test]
async fn test_health_endpoint_provider_info() {
    let app = create_test_app(create_test_config_no_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let provider_info = json["provider_info"].as_array().unwrap();
    assert_eq!(provider_info.len(), 2);

    // Check provider weights and probabilities
    let provider1 = &provider_info[0];
    assert_eq!(provider1["name"], "TestProvider1");
    assert_eq!(provider1["weight"], 2);
    assert!(provider1["probability"].as_str().unwrap().contains("%"));
}

#[tokio::test]
async fn test_models_endpoint_sorted() {
    let app = create_test_app(create_test_config_no_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let models: Vec<String> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();

    // Models should be sorted alphabetically
    let mut sorted_models = models.clone();
    sorted_models.sort();
    assert_eq!(models, sorted_models);
}

#[tokio::test]
async fn test_error_response_format() {
    let app = create_test_app(create_test_config_with_auth());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Check error response format
    assert!(json["error"].is_object());
    assert!(json["error"]["message"].is_string());
    assert!(json["error"]["type"].is_string());
    assert!(json["error"]["code"].is_number());
}

#[tokio::test]
async fn test_multiple_sequential_requests() {
    let app = create_test_app(create_test_config_no_auth());

    for i in 0..5 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK, "Request {} failed", i);
    }
}
