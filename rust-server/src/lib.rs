//! LLM Proxy Server - A high-performance reverse proxy for LLM APIs
//!
//! This library provides a production-ready proxy server for Large Language Model APIs
//! with features including:
//!
//! - **Weighted Round-Robin Load Balancing**: Distribute requests across multiple providers
//! - **Model Name Mapping**: Translate model names between client and provider formats
//! - **Streaming Support**: Full support for Server-Sent Events (SSE) streaming
//! - **Metrics & Monitoring**: Prometheus metrics for observability
//! - **Authentication**: Optional master API key for access control
//!
//! # Architecture
//!
//! The codebase is organized into three main layers:
//!
//! - [`core`]: Core functionality (config, errors, metrics, middleware)
//! - [`api`]: HTTP handlers and request/response models
//! - [`services`]: Business logic (provider selection, etc.)
//!
//! # Example
//!
//! ```no_run
//! use llm_proxy_rust::{core::AppConfig, services::ProviderService, api::AppState};
//! use llm_proxy_rust::core::RateLimiter;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Load configuration
//!     let config = AppConfig::load("config.yaml")?;
//!
//!     // Initialize services
//!     let provider_service = ProviderService::new(config.clone());
//!     let rate_limiter = Arc::new(RateLimiter::new());
//!
//!     // Create shared HTTP client with connection pooling
//!     let http_client = reqwest::Client::builder()
//!         .pool_max_idle_per_host(20)
//!         .build()?;
//!
//!     // Create application state
//!     let state = Arc::new(AppState {
//!         config,
//!         provider_service,
//!         rate_limiter,
//!         http_client,
//!     });
//!
//!     // Build and run server...
//!     Ok(())
//! }
//! ```

pub mod api;
pub mod core;
pub mod services;

// Re-export commonly used types for convenience
pub use api::{AppState, ChatCompletionRequest, ChatCompletionResponse};
pub use core::{AppConfig, AppError, Result};
pub use services::ProviderService;
