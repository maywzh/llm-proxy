//! API layer for the LLM proxy server.
//!
//! This module contains all HTTP handlers, request/response models,
//! streaming support, and admin API for the endpoints.

pub mod admin;
pub mod handlers;
pub mod models;
pub mod streaming;

// Re-export commonly used types
pub use admin::{admin_router, combined_openapi, AdminApiDoc, AdminState, V1ApiDoc};
pub use handlers::{
    chat_completions, completions, list_models, metrics_handler, AppState,
};
pub use models::{
    ApiErrorDetail, ApiErrorResponse, ChatCompletionRequest, ChatCompletionResponse,
    ModelList, Provider,
};
pub use streaming::{create_sse_stream, rewrite_model_in_response};
