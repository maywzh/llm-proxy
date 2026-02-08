//! LLM Proxy Server - Main entry point
//!
//! This binary creates and runs the HTTP server with all configured routes and middleware.
//! Configuration is loaded from the database via Admin API.

use anyhow::Result;
use axum::{
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::Local;
use llm_proxy_rust::{
    admin_router,
    api::{
        chat_completions_v2, claude_count_tokens, completions, completions_v2, count_tokens_v2,
        gcp_vertex_proxy, list_model_info_v1, list_model_info_v2, list_models, list_models_v2,
        messages_v2, metrics_handler, responses_v2, AdminState, AppState, ProxyState,
    },
    combined_openapi,
    core::{
        admin_logging_middleware, init_error_logger, init_jsonl_logger, init_langfuse_service,
        init_metrics, model_permission_middleware, request_id_middleware, AppConfig, Database,
        DatabaseConfig, DynamicConfig, MetricsMiddleware, RateLimiter, RuntimeConfig,
    },
    services::ProviderService,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa_swagger_ui::SwaggerUi;

fn main() -> Result<()> {
    // Load .env file if present (before reading any environment variables)
    dotenvy::dotenv().ok();

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

/// Custom time formatter that uses local timezone (respects TZ environment variable)
struct LocalTime;

impl tracing_subscriber::fmt::time::FormatTime for LocalTime {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        let now = Local::now();
        write!(w, "{}", now.format("%Y-%m-%d %H:%M:%S"))
    }
}

async fn async_main() -> Result<()> {
    // Check if NO_COLOR environment variable is set (for file logging without ANSI codes)
    let no_color = std::env::var("NO_COLOR").is_ok();

    // Initialize logging with local timezone (respects TZ environment variable)
    // Default filter: info level for most crates, debug for llm_proxy_rust,
    // and warn level for hyper/reqwest (to hide verbose chunked header logs from debug output)
    // Also filter out hyper::proto trace logs which are very noisy
    //
    // IMPORTANT: We always append noise-suppression filters for hyper/h2/reqwest
    // because if RUST_LOG is set to just "info" or "trace", it would override
    // our defaults and allow noisy trace logs through.
    let base_filter =
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info,llm_proxy_rust=debug".to_string());

    // Always suppress noisy HTTP library logs regardless of RUST_LOG setting
    let filter_str = format!(
        "{},hyper=warn,hyper::proto=warn,h2=warn,reqwest=warn",
        base_filter
    );
    let filter = tracing_subscriber::EnvFilter::new(filter_str);

    if no_color {
        // Disable ANSI colors for file logging
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_timer(LocalTime)
                    .with_ansi(false),
            )
            .init();
    } else {
        // Enable ANSI colors for terminal
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_timer(LocalTime))
            .init();
    }

    // Initialize metrics
    init_metrics();

    // Initialize Langfuse service (optional, fails gracefully if not configured)
    init_langfuse_service(None);

    // Initialize JSONL logger (optional, disabled by default)
    init_jsonl_logger().await;

    // Get required environment variables
    let db_url = std::env::var("DB_URL")
        .map_err(|_| anyhow::anyhow!("DB_URL environment variable is required"))?;
    let admin_key = std::env::var("ADMIN_KEY")
        .map_err(|_| anyhow::anyhow!("ADMIN_KEY environment variable is required"))?;

    // Connect to database
    let db_config = DatabaseConfig::from_url(&db_url);
    tracing::info!("Connecting to database...");
    let db = Database::connect(&db_config).await?;
    tracing::info!("Database connected successfully");

    // Check if migrations have been applied
    if !db.check_migrations().await? {
        return Err(anyhow::anyhow!(
            "Database migrations not applied. Please run './scripts/db_migrate.sh' first."
        ));
    }

    let db = Arc::new(db);

    // Initialize error logger with database pool
    init_error_logger(db.pool().clone());

    // Load configuration from database (empty config if database is empty)
    let runtime_config = if db.is_empty().await? {
        tracing::info!("Database is empty. Server will start with no providers/credentials.");
        tracing::info!("Use Admin API to add providers and credentials.");
        RuntimeConfig {
            providers: vec![],
            credentials: vec![],
            version: 0,
            loaded_at: chrono::Utc::now(),
        }
    } else {
        tracing::info!("Loading configuration from database...");
        let config = RuntimeConfig::load_from_db(&db).await?;
        tracing::info!(
            "Configuration loaded: {} providers, {} credentials, version {}",
            config.providers.len(),
            config.credentials.len(),
            config.version
        );
        config
    };

    // Create dynamic config manager
    let dynamic_config = Arc::new(DynamicConfig::new(runtime_config, db.clone()));

    // Get server config from environment
    let base_config = AppConfig::from_env()?;
    let port = base_config.server.port;

    // Create HTTP client
    let http_client = create_http_client(&base_config);

    // Create admin state
    let admin_state = Arc::new(AdminState {
        dynamic_config: dynamic_config.clone(),
        admin_key,
        http_client: http_client.clone(),
    });

    // Build router
    let app = build_router(dynamic_config, admin_state, base_config, http_client);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Starting LLM API Proxy on {}", addr);
    tracing::info!("OpenAI API: /v1/chat/completions, /v1/completions, /v1/models");
    tracing::info!("Claude API: /v1/messages, /v1/messages/count_tokens");
    tracing::info!("Admin API: /admin/v1/*");
    tracing::info!("Swagger UI: /swagger-ui");
    tracing::info!("Metrics endpoint: /metrics");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Build router with all endpoints
fn build_router(
    dynamic_config: Arc<DynamicConfig>,
    admin_state: Arc<AdminState>,
    base_config: AppConfig,
    http_client: reqwest::Client,
) -> Router {
    // Admin routes with logging middleware
    let admin_routes =
        admin_router(admin_state).layer(axum::middleware::from_fn(admin_logging_middleware));

    // Swagger UI for API documentation (includes both V1 and Admin APIs)
    let swagger_ui =
        SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", combined_openapi());

    // Get current config
    let config = dynamic_config.get_full();

    // Convert database entities to AppConfig format
    let app_config = convert_runtime_to_app_config(&config, &base_config);

    let provider_service = ProviderService::new(app_config.clone());
    provider_service.log_providers();

    let rate_limiter = Arc::new(RateLimiter::new());
    // Register rate limits from database credentials
    for credential in &config.credentials {
        if credential.is_enabled {
            if let Some(rps) = credential.rate_limit {
                let rate_config = llm_proxy_rust::core::config::RateLimitConfig {
                    requests_per_second: rps as u32,
                    burst_size: (rps as u32).saturating_mul(2),
                };
                rate_limiter.register_key(&credential.credential_key, &rate_config);
                tracing::info!(
                    "Registered rate limit for credential '{}': {} req/s",
                    credential.name,
                    rps
                );
            }
        }
    }

    let state = Arc::new(AppState::new(
        app_config,
        provider_service,
        rate_limiter,
        http_client,
        Some(dynamic_config),
    ));

    // Build proxy state with transformer support
    let proxy_state = Arc::new(ProxyState::new(state.clone()));

    // Build API routes with AppState (v1 - legacy endpoints)
    let api_routes = Router::new()
        // OpenAI-compatible endpoints
        .route("/v1/completions", post(completions))
        .route("/v1/models", get(list_models))
        // Claude-compatible endpoints (legacy, without transformer)
        .route("/v1/messages/count_tokens", post(claude_count_tokens))
        .layer(axum::middleware::from_fn(MetricsMiddleware::track_metrics))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            model_permission_middleware,
        ))
        .with_state(state);

    // Build v2 API routes with ProxyState (transformer-based)
    let api_routes_v2 = Router::new()
        .route("/v2/chat/completions", post(chat_completions_v2))
        .route("/v2/completions", post(completions_v2))
        .route("/v2/messages", post(messages_v2))
        .route("/v2/messages/count_tokens", post(count_tokens_v2))
        .route("/v2/responses", post(responses_v2))
        .route("/v2/models", get(list_models_v2))
        .route("/v2/model/info", get(list_model_info_v2))
        // v1 endpoints (uses transformer for GCP Vertex support)
        .route("/v1/messages", post(messages_v2))
        .route("/v1/chat/completions", post(chat_completions_v2))
        .route("/v1/responses", post(responses_v2))
        // Root API routes (map to v2 handlers)
        .route("/chat/completions", post(chat_completions_v2))
        .route("/messages", post(messages_v2))
        .route("/responses", post(responses_v2))
        // LiteLLM v1 compatible endpoint (no pagination)
        .route("/v1/model/info", get(list_model_info_v1))
        // Default /model/info uses v2 format (with pagination)
        .route("/model/info", get(list_model_info_v2))
        // GCP Vertex AI Anthropic Claude models
        .route(
            "/models/gcp-vertex/v1/projects/:project/locations/:location/publishers/:publisher/models/:model_and_action",
            post(gcp_vertex_proxy),
        )
        .layer(axum::middleware::from_fn(MetricsMiddleware::track_metrics))
        .layer(axum::middleware::from_fn_with_state(
            proxy_state.clone(),
            model_permission_middleware,
        ))
        .with_state(proxy_state);

    // Merge admin routes (with AdminState) and API routes (with AppState)
    Router::new()
        .nest("/admin/v1", admin_routes)
        .merge(swagger_ui)
        .merge(api_routes)
        .merge(api_routes_v2)
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        // Placeholder for Claude Code telemetry (returns 200 to suppress 404 noise)
        .route("/api/event_logging/batch", post(event_logging_placeholder))
        .layer(axum::middleware::from_fn(request_id_middleware))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

/// Convert RuntimeConfig to AppConfig for compatibility
fn convert_runtime_to_app_config(runtime: &RuntimeConfig, base: &AppConfig) -> AppConfig {
    use llm_proxy_rust::core::config::{CredentialConfig, ProviderConfig};

    let providers: Vec<ProviderConfig> = runtime
        .providers
        .iter()
        .map(|p| ProviderConfig {
            name: p.provider_key.clone(),
            api_base: p.api_base.clone(),
            api_key: p.api_key.clone(),
            weight: p.weight as u32,
            model_mapping: p.model_mapping.0.clone(),
            provider_type: p.provider_type.clone(),
            provider_params: p.provider_params.0.clone(),
        })
        .collect();

    let credentials: Vec<CredentialConfig> = runtime
        .credentials
        .iter()
        .map(|c| CredentialConfig {
            credential_key: c.credential_key.clone(), // Use hash for comparison
            name: c.name.clone(),
            description: None,
            rate_limit: c
                .rate_limit
                .map(|rps| llm_proxy_rust::core::config::RateLimitConfig {
                    requests_per_second: rps as u32,
                    burst_size: (rps as u32).saturating_mul(2),
                }),
            enabled: c.is_enabled,
            allowed_models: c.allowed_models.clone(),
        })
        .collect();

    AppConfig {
        providers,
        server: base.server.clone(),
        verify_ssl: base.verify_ssl,
        request_timeout_secs: base.request_timeout_secs,
        ttft_timeout_secs: base.ttft_timeout_secs,
        credentials,
        provider_suffix: base.provider_suffix.clone(),
        min_tokens_limit: base.min_tokens_limit,
        max_tokens_limit: base.max_tokens_limit,
    }
}

/// Create HTTP client with connection pooling
fn create_http_client(config: &AppConfig) -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
        .pool_max_idle_per_host(100)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .http2_keep_alive_interval(std::time::Duration::from_secs(30))
        .http2_keep_alive_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client")
}

/// Health check endpoint
async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok"
    }))
}

/// Placeholder for Claude Code telemetry endpoint
/// This endpoint is called by Claude Code CLI for usage telemetry.
/// We return 200 OK to suppress 404 errors in logs.
async fn event_logging_placeholder() -> impl IntoResponse {
    tracing::debug!("POST /api/event_logging/batch - telemetry request ignored");
    Json(serde_json::json!({
        "status": "ok"
    }))
}

/// Detect CPU limit from cgroup (for containerized environments)
fn detect_cpu_limit() -> Option<usize> {
    // Try cgroup v2 first
    if let Ok(max) = std::fs::read_to_string("/sys/fs/cgroup/cpu.max") {
        let parts: Vec<&str> = max.split_whitespace().collect();
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
