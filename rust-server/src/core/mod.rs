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
//! - JSONL request/response logging

pub mod cancel;
pub mod config;
pub mod database;
pub mod error;
pub mod error_logger;
pub mod header_policy;
pub mod jsonl_logger;
pub mod langfuse;
pub mod logging;
pub mod metrics;
pub mod middleware;
pub mod rate_limiter;
pub mod stream_metrics;
pub mod token_counter;
pub mod tokenizer;
pub mod utils;

// Re-export commonly used types
pub use cancel::StreamCancelHandle;
pub use config::{AppConfig, ProviderConfig, ServerConfig};
pub use database::{
    create_key_preview, hash_key, CreateCredential, CreateProvider, CredentialEntity, Database,
    DatabaseConfig, DynamicConfig, ProviderEntity, RuntimeConfig, UpdateCredential, UpdateProvider,
};
pub use error::{AppError, Result};
pub use error_logger::{
    init_error_logger, log_error, mask_headers, ErrorCategory, ErrorLogRecord, ErrorLogger,
};
pub use jsonl_logger::{
    get_jsonl_logger, init_jsonl_logger, log_request, log_response, log_streaming_response,
    JsonlLogger, JsonlLoggerConfig, LogRecord, RequestRecord, ResponseRecord,
};
pub use langfuse::{
    build_langfuse_tags, extract_client_metadata, get_langfuse_service, init_langfuse_service,
    shutdown_langfuse_service, GenerationData, LangfuseConfig, LangfuseService,
};
pub use logging::{get_provider_context, PROVIDER_CONTEXT};
pub use metrics::{get_metrics, init_metrics, Metrics};
pub use middleware::{
    admin_logging_middleware, model_permission_middleware, request_id_middleware, HasCredentials,
    MetricsMiddleware, ModelName, ProviderName, RequestId,
};
pub use rate_limiter::RateLimiter;
pub use stream_metrics::{record_stream_metrics, StreamStats};
pub use token_counter::OutboundTokenCounter;
pub use tokenizer::{
    count_tokens_hf, get_hf_tokenizer, get_tokenizer_info, select_tokenizer, TokenizerSelection,
    TokenizerType,
};
