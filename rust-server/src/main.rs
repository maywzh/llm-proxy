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
use llm_proxy_rust::{
    admin_router, CombinedApiDoc,
    api::{chat_completions, completions, list_models, metrics_handler, AdminState, AppState},
    core::{
        admin_logging_middleware, init_metrics, AppConfig, Database, DatabaseConfig,
        DynamicConfig, MetricsMiddleware, RateLimiter, RuntimeConfig,
    },
    services::ProviderService,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use utoipa::OpenApi;
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

async fn async_main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,llm_proxy_rust=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Initialize metrics
    init_metrics();

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

    // Load configuration from database (empty config if database is empty)
    let runtime_config = if db.is_empty().await? {
        tracing::info!("Database is empty. Server will start with no providers/master_keys.");
        tracing::info!("Use Admin API to add providers and master keys.");
        RuntimeConfig {
            providers: vec![],
            master_keys: vec![],
            version: 0,
            loaded_at: chrono::Utc::now(),
        }
    } else {
        tracing::info!("Loading configuration from database...");
        let config = RuntimeConfig::load_from_db(&db).await?;
        tracing::info!(
            "Configuration loaded: {} providers, {} master keys, version {}",
            config.providers.len(),
            config.master_keys.len(),
            config.version
        );
        config
    };

    // Create dynamic config manager
    let dynamic_config = Arc::new(DynamicConfig::new(runtime_config, db.clone()));

    // Create admin state
    let admin_state = Arc::new(AdminState {
        dynamic_config: dynamic_config.clone(),
        admin_key,
    });

    // Get server config from environment
    let base_config = AppConfig::from_env()?;
    let port = base_config.server.port;

    // Create HTTP client
    let http_client = create_http_client(&base_config);

    // Build router
    let app = build_router(dynamic_config, admin_state, base_config, http_client);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Starting LLM API Proxy on {}", addr);
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
    let admin_routes = admin_router(admin_state)
        .layer(axum::middleware::from_fn(admin_logging_middleware));

    // Swagger UI for API documentation (includes both V1 and Admin APIs)
    let swagger_ui = SwaggerUi::new("/swagger-ui")
        .url("/api-docs/openapi.json", CombinedApiDoc::openapi());

    // Get current config
    let config = dynamic_config.get_full();

    // Convert database entities to AppConfig format
    let app_config = convert_runtime_to_app_config(&config, &base_config);

    let provider_service = ProviderService::new(app_config.clone());
    provider_service.log_providers();

    let rate_limiter = Arc::new(RateLimiter::new());
    // Register rate limits from database master keys
    for key in &config.master_keys {
        if key.is_enabled {
            if let Some(rps) = key.rate_limit {
                let rate_config = llm_proxy_rust::core::config::RateLimitConfig {
                    requests_per_second: rps as u32,
                    burst_size: (rps as u32).saturating_mul(2),
                };
                rate_limiter.register_key(&key.key_hash, &rate_config);
                tracing::info!(
                    "Registered rate limit for key '{}': {} req/s",
                    key.name,
                    rps
                );
            }
        }
    }

    let state = Arc::new(AppState {
        config: app_config,
        provider_service,
        rate_limiter,
        http_client,
    });

    // Build API routes with AppState
    let api_routes = Router::new()
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/completions", post(completions))
        .route("/v1/models", get(list_models))
        .layer(axum::middleware::from_fn(MetricsMiddleware::track_metrics))
        .with_state(state);

    // Merge admin routes (with AdminState) and API routes (with AppState)
    Router::new()
        .nest("/admin/v1", admin_routes)
        .merge(swagger_ui)
        .merge(api_routes)
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

/// Convert RuntimeConfig to AppConfig for compatibility
fn convert_runtime_to_app_config(
    runtime: &RuntimeConfig,
    base: &AppConfig,
) -> AppConfig {
    use llm_proxy_rust::core::config::{MasterKeyConfig, ProviderConfig};

    let providers: Vec<ProviderConfig> = runtime
        .providers
        .iter()
        .map(|p| ProviderConfig {
            name: p.id.clone(),
            api_base: p.api_base.clone(),
            api_key: p.api_key.clone(),
            weight: 1, // Default weight
            model_mapping: p.model_mapping.0.clone(),
        })
        .collect();

    let master_keys: Vec<MasterKeyConfig> = runtime
        .master_keys
        .iter()
        .map(|k| MasterKeyConfig {
            key: k.key_hash.clone(), // Use hash for comparison
            name: k.name.clone(),
            description: None,
            rate_limit: k.rate_limit.map(|rps| {
                llm_proxy_rust::core::config::RateLimitConfig {
                    requests_per_second: rps as u32,
                    burst_size: (rps as u32).saturating_mul(2),
                }
            }),
            enabled: k.is_enabled,
            allowed_models: k.allowed_models.clone(),
        })
        .collect();

    AppConfig {
        providers,
        server: base.server.clone(),
        verify_ssl: base.verify_ssl,
        request_timeout_secs: base.request_timeout_secs,
        ttft_timeout_secs: base.ttft_timeout_secs,
        master_keys,
        provider_suffix: base.provider_suffix.clone(),
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
