//! LLM Proxy Server - A high-performance reverse proxy for LLM APIs
//!
//! This library provides a production-ready proxy server for Large Language Model APIs
//! with features including:
//!
//! - **Weighted Round-Robin Load Balancing**: Distribute requests across multiple providers
//! - **Model Name Mapping**: Translate model names between client and provider formats
//! - **Streaming Support**: Full support for Server-Sent Events (SSE) streaming
//! - **Metrics & Monitoring**: Prometheus metrics for observability
//! - **Authentication**: Master API key authentication with rate limiting
//! - **Dynamic Configuration**: Configuration loaded from database via Admin API
//!
//! # Architecture
//!
//! The codebase is organized into three main layers:
//!
//! - [`core`]: Core functionality (config, database, errors, metrics, middleware)
//! - [`api`]: HTTP handlers, Admin API, and request/response models
//! - [`services`]: Business logic (provider selection, etc.)
//!
//! # Configuration
//!
//! The server requires the following environment variables:
//! - `DB_URL`: PostgreSQL database connection URL
//! - `ADMIN_KEY`: Admin API authentication key
//!
//! Optional environment variables:
//! - `HOST`: Server bind address (default: 0.0.0.0)
//! - `PORT`: Server port (default: 18000)
//! - `VERIFY_SSL`: Verify SSL certificates for upstream (default: true)
//! - `REQUEST_TIMEOUT_SECS`: Request timeout in seconds (default: 300)

pub mod api;
pub mod core;
pub mod services;
pub mod transformer;

// Re-export commonly used types for convenience
pub use api::{
    admin_router, claude_count_tokens, claude_create_message, combined_openapi, AdminApiDoc,
    AdminState, AppState, ChatCompletionRequest, ChatCompletionResponse, ClaudeMessagesRequest,
    ClaudeResponse, V1ApiDoc,
};
pub use core::{
    admin_logging_middleware, init_error_logger, request_id_middleware, AppConfig, AppError,
    Database, DatabaseConfig, DynamicConfig, RequestId, Result, RuntimeConfig,
};
pub use services::{claude_to_openai_request, openai_to_claude_response, ProviderService};
