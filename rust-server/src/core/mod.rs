//! Core functionality for the LLM proxy server.
//!
//! This module contains fundamental components used throughout the application:
//! - Configuration management
//! - Error handling
//! - Metrics collection
//! - HTTP middleware

pub mod config;
pub mod error;
pub mod logging;
pub mod metrics;
pub mod middleware;

// Re-export commonly used types
pub use config::{AppConfig, ProviderConfig, ServerConfig};
pub use error::{AppError, Result};
pub use logging::{get_provider_context, PROVIDER_CONTEXT};
pub use metrics::{get_metrics, init_metrics, Metrics};
pub use middleware::MetricsMiddleware;