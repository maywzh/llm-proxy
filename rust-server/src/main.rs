//! LLM Proxy Server - Main entry point
//!
//! This binary creates and runs the HTTP server with all configured routes and middleware.

use anyhow::Result;
use axum::{
    routing::{get, post},
    Router,
};
use llm_proxy_rust::{
    api::{
        chat_completions, completions, health, health_detailed, list_models, metrics_handler,
        AppState,
    },
    core::{init_metrics, AppConfig, MetricsMiddleware, RateLimiter},
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

    // Initialize rate limiter and register master keys
    let rate_limiter = std::sync::Arc::new(RateLimiter::new());
    for key_config in &config.master_keys {
        if key_config.enabled {
            if let Some(rate_limit) = &key_config.rate_limit {
                rate_limiter.register_key(&key_config.key, rate_limit);
                tracing::info!(
                    "Registered rate limit for key '{}': {} req/s (burst: {})",
                    key_config.name,
                    rate_limit.requests_per_second,
                    rate_limit.burst_size
                );
            } else {
                tracing::info!(
                    "Master key '{}' registered without rate limiting",
                    key_config.name
                );
            }
        }
    }

    // Create shared state
    let state = std::sync::Arc::new(AppState {
        config: config.clone(),
        provider_service,
        rate_limiter,
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

    // Log authentication status
    if !config.master_keys.is_empty() {
        tracing::info!(
            "Master keys: {} configured ({} enabled)",
            config.master_keys.len(),
            config.master_keys.iter().filter(|k| k.enabled).count()
        );
    } else {
        tracing::info!("Authentication: Disabled");
    }

    tracing::info!("Metrics endpoint: /metrics");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
