//! API layer for the LLM proxy server.
//!
//! This module contains all HTTP handlers, request/response models,
//! streaming support, and admin API for the endpoints.

pub mod admin;
pub mod auth;
pub mod claude;
pub mod claude_models;
pub mod disconnect;
pub mod gemini3;
pub mod handlers;
pub mod health;
pub mod models;
pub mod proxy;
pub mod streaming;

// Re-export commonly used types
pub use admin::{admin_router, combined_openapi, AdminApiDoc, AdminState, V1ApiDoc};
pub use auth::{hash_key, verify_auth, AuthFormat};
pub use claude::{count_tokens as claude_count_tokens, create_message as claude_create_message};
pub use claude_models::{
    ClaudeErrorResponse, ClaudeMessagesRequest, ClaudeResponse, ClaudeTokenCountRequest,
    ClaudeTokenCountResponse, ClaudeUsage,
};
pub use handlers::{
    chat_completions, completions, list_model_info, list_models, metrics_handler, AppState,
};
pub use health::{
    health_router, HealthCheckRequest, HealthCheckResponse, HealthStatus, ModelHealthStatus,
    ProviderHealthStatus,
};
pub use models::{
    ApiErrorDetail, ApiErrorResponse, ChatCompletionRequest, ChatCompletionResponse,
    ModelInfoListV1, ModelInfoQueryParams, ModelInfoQueryParamsV1, ModelList,
    PaginatedModelInfoList, Provider,
};
pub use proxy::{
    chat_completions_v2, completions_v2, count_tokens_v2, handle_proxy_request, list_model_info_v1,
    list_model_info_v2, list_models_v2, messages_v2, responses_v2, ProxyState,
};
pub use streaming::{create_sse_stream, rewrite_model_in_response};
