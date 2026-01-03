//! Core functionality for the LLM proxy server.
//!
//! This module contains fundamental components used throughout the application:
//! - Configuration management
//! - Database abstraction
//! - Error handling
//! - Metrics collection
//! - HTTP middleware
//! - Rate limiting

pub mod config;
pub mod database;
pub mod error;
pub mod logging;
pub mod metrics;
pub mod middleware;
pub mod rate_limiter;

// Re-export commonly used types
pub use config::{AppConfig, ProviderConfig, ServerConfig};
pub use database::{
    Database, DatabaseConfig, DynamicConfig, RuntimeConfig,
    ProviderEntity, MasterKeyEntity, CreateProvider, UpdateProvider,
    CreateMasterKey, UpdateMasterKey, hash_key, create_key_preview,
};
pub use error::{AppError, Result};
pub use logging::{get_provider_context, PROVIDER_CONTEXT};
pub use metrics::{get_metrics, init_metrics, Metrics};
pub use middleware::{admin_logging_middleware, MetricsMiddleware, ModelName, ProviderName};
pub use rate_limiter::RateLimiter;
