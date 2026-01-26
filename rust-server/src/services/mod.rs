//! Business logic services for the LLM proxy.
//!
//! This module contains service layer components that implement
//! core business logic, such as provider selection and management.

pub mod claude_converter;
pub mod health_check_service;
pub mod provider_service;
pub mod response_api_converter;

// Re-export commonly used types
pub use claude_converter::{
    claude_to_openai_request, convert_openai_streaming_to_claude, openai_to_claude_response,
};
pub use health_check_service::{check_providers_health, HealthCheckService};
pub use provider_service::ProviderService;
pub use response_api_converter::{
    convert_openai_streaming_to_response_api, openai_to_response_api_response,
    response_api_to_openai_request, ResponseApiRequest, ResponseApiResponse,
};
