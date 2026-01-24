//! Core functionality for the LLM proxy server.
//!
//! This module contains fundamental components used throughout the application:
//! - Configuration management
//! - Database abstraction
//! - Error handling
//! - Metrics collection
//! - HTTP middleware
//! - Rate limiting
//! - Langfuse observability

pub mod config;
pub mod database;
pub mod error;
pub mod langfuse;
pub mod logging;
pub mod metrics;
pub mod middleware;
pub mod rate_limiter;

// Re-export commonly used types
pub use config::{AppConfig, ProviderConfig, ServerConfig};
pub use database::{
    Database, DatabaseConfig, DynamicConfig, RuntimeConfig,
    ProviderEntity, CredentialEntity, CreateProvider, UpdateProvider,
    CreateCredential, UpdateCredential, hash_key, create_key_preview,
};
pub use error::{AppError, Result};
pub use langfuse::{
    get_langfuse_service, init_langfuse_service, shutdown_langfuse_service,
    GenerationData, LangfuseConfig, LangfuseService,
    extract_client_metadata, build_langfuse_tags,
};
pub use logging::{get_provider_context, PROVIDER_CONTEXT};
pub use metrics::{get_metrics, init_metrics, Metrics};
pub use middleware::{admin_logging_middleware, MetricsMiddleware, ModelName, ProviderName};
pub use rate_limiter::RateLimiter;
