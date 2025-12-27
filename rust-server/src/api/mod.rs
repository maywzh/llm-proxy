//! API layer for the LLM proxy server.
//!
//! This module contains all HTTP handlers, request/response models,
//! and streaming support for the API endpoints.

pub mod handlers;
pub mod models;
pub mod streaming;

// Re-export commonly used types
pub use handlers::{
    chat_completions, completions, health, health_detailed, list_models, metrics_handler, AppState,
};
pub use models::{
    ChatCompletionRequest, ChatCompletionResponse, DetailedHealthResponse, HealthResponse,
    ModelList, Provider,
};
pub use streaming::{create_sse_stream, rewrite_model_in_response};
