//! LLM Proxy Server - Main entry point
//!
//! This binary creates and runs the HTTP server with all configured routes and middleware.

use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use llm_proxy_rust::{
    api::{chat_completions, completions, health, health_detailed, list_models, metrics_handler, AppState},
    core::{init_metrics, AppConfig, MetricsMiddleware},
    services::ProviderService,
};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,llm_proxy_rust=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load configuration
    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.yaml".to_string());
    let config = AppConfig::load(&config_path)?;

    let _host = std::env::var("HOST").unwrap_or_else(|_| config.server.host.clone());
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(config.server.port);

    // Initialize metrics
    init_metrics();

    // Initialize provider service
    let provider_service = ProviderService::new(config.clone());
    provider_service.log_providers();

    // Create shared state
    let state = std::sync::Arc::new(AppState {
        config: config.clone(),
        provider_service,
    });

    // Build router
    let app = Router::new()
        // API routes
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(completions))
        .route("/v1/models", get(list_models))
        // Health routes
        .route("/health", get(health))
        .route("/health/detailed", get(health_detailed))
        // Metrics route
        .route("/metrics", get(metrics_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            MetricsMiddleware::track_metrics,
        ))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Starting LLM API Proxy on {}", addr);
    tracing::info!("Using config file: {}", config_path);
    tracing::info!(
        "Master API key: {}",
        if config.server.master_api_key.is_some() {
            "Enabled"
        } else {
            "Disabled"
        }
    );
    tracing::info!("Metrics endpoint: /metrics");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}