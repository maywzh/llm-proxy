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
        chat_completions, completions, list_models, metrics_handler,
        AppState,
    },
    core::{init_metrics, AppConfig, MetricsMiddleware, RateLimiter},
    services::ProviderService,
};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> Result<()> {
    // Detect optimal worker threads from environment or cgroup
    let worker_threads = std::env::var("TOKIO_WORKER_THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or_else(|| {
            // Try to detect CPU limit from cgroup
            detect_cpu_limit().unwrap_or(1)
        });

    println!("Tokio runtime: using {} worker threads", worker_threads);

    // Build custom Tokio runtime with explicit thread count
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .build()?;

    runtime.block_on(async_main())
}

async fn async_main() -> Result<()> {
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

    // Create shared HTTP client with connection pooling
    // Increased limits to support high concurrency for the same model
    let http_client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
        .pool_max_idle_per_host(100)  // Increased from 20 to 100
        .pool_idle_timeout(std::time::Duration::from_secs(90))  // Increased from 30s to 90s
        .tcp_keepalive(std::time::Duration::from_secs(60))  // Enable TCP keepalive
        .http2_keep_alive_interval(std::time::Duration::from_secs(30))  // HTTP/2 keepalive
        .http2_keep_alive_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client");

    tracing::info!("HTTP client initialized with high-concurrency connection pooling (max_idle=100, timeout=90s)");

    // Create shared state
    let state = std::sync::Arc::new(AppState {
        config: config.clone(),
        provider_service,
        rate_limiter,
        http_client,
    });

    // Build router
    let app = Router::new()
        // API routes
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(completions))
        .route("/v1/models", get(list_models))
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

/// Detect CPU limit from cgroup (for containerized environments)
fn detect_cpu_limit() -> Option<usize> {
    // Try cgroup v2 first
    if let Ok(max) = std::fs::read_to_string("/sys/fs/cgroup/cpu.max") {
        let parts: Vec<&str> = max.trim().split_whitespace().collect();
        if parts.len() == 2 {
            if let (Ok(quota), Ok(period)) = (parts[0].parse::<i64>(), parts[1].parse::<i64>()) {
                if quota > 0 {
                    let cores = ((quota as f64 / period as f64).ceil() as usize).max(1);
                    println!("Detected CPU limit from cgroup v2: {} cores", cores);
                    return Some(cores);
                }
            }
        }
    }

    // Fallback to cgroup v1
    let quota = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_quota_us")
        .ok()?
        .trim()
        .parse::<i64>()
        .ok()?;

    let period = std::fs::read_to_string("/sys/fs/cgroup/cpu/cpu.cfs_period_us")
        .ok()?
        .trim()
        .parse::<i64>()
        .ok()?;

    if quota > 0 {
        let cores = ((quota as f64 / period as f64).ceil() as usize).max(1);
        println!("Detected CPU limit from cgroup v1: {} cores", cores);
        Some(cores)
    } else {
        None
    }
}
