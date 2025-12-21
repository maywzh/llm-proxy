//! Error types and handling for the LLM proxy server.
//!
//! This module provides a unified error type [`AppError`] that wraps various error sources
//! and implements proper HTTP response conversion.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use thiserror::Error;

/// Main error type for the application.
///
/// All errors in the application should be converted to this type for consistent handling.
#[derive(Error, Debug)]
pub enum AppError {
    /// Configuration-related errors (file not found, parse errors, etc.)
    #[error("Configuration error: {0}")]
    Config(#[from] anyhow::Error),

    /// HTTP request errors from the reqwest client
    #[error("HTTP request error: {0}")]
    Request(#[from] reqwest::Error),

    /// JSON serialization/deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Authentication/authorization failures
    #[error("Unauthorized")]
    Unauthorized,

    /// Request timeout errors
    #[error("Gateway timeout")]
    Timeout,

    /// Rate limit exceeded errors
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    /// Generic internal server errors with custom message
    #[error("Internal server error: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_message) = match self {
            AppError::Config(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            AppError::Request(e) => {
                if e.is_timeout() {
                    (StatusCode::GATEWAY_TIMEOUT, "Gateway timeout".to_string())
                } else if let Some(status) = e.status() {
                    (
                        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                        e.to_string(),
                    )
                } else {
                    (StatusCode::BAD_GATEWAY, e.to_string())
                }
            }
            AppError::Serialization(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized".to_string()),
            AppError::Timeout => (StatusCode::GATEWAY_TIMEOUT, "Gateway timeout".to_string()),
            AppError::RateLimitExceeded(msg) => (StatusCode::TOO_MANY_REQUESTS, msg),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        let body = Json(json!({
            "error": {
                "message": error_message,
                "type": "error",
                "code": status.as_u16()
            }
        }));

        (status, body).into_response()
    }
}

/// Convenience type alias for Results using [`AppError`].
pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = AppError::Unauthorized;
        assert_eq!(err.to_string(), "Unauthorized");

        let err = AppError::Internal("test error".to_string());
        assert_eq!(err.to_string(), "Internal server error: test error");

        let err = AppError::Timeout;
        assert_eq!(err.to_string(), "Gateway timeout");
    }

    #[test]
    fn test_error_into_response() {
        let err = AppError::Unauthorized;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_unauthorized_response() {
        let err = AppError::Unauthorized;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_timeout_response() {
        let err = AppError::Timeout;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    }

    #[test]
    fn test_internal_error_response() {
        let err = AppError::Internal("custom error".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_config_error_response() {
        let err = AppError::Config(anyhow::anyhow!("config error"));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_serialization_error_response() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid json")
            .unwrap_err();
        let err = AppError::Serialization(json_err);
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_error_from_anyhow() {
        let anyhow_err = anyhow::anyhow!("test error");
        let app_err: AppError = anyhow_err.into();
        assert!(matches!(app_err, AppError::Config(_)));
    }

    #[test]
    fn test_error_from_serde_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
        let app_err: AppError = json_err.into();
        assert!(matches!(app_err, AppError::Serialization(_)));
    }

    #[tokio::test]
    async fn test_result_type_alias() {
        fn returns_result() -> Result<String> {
            Ok("success".to_string())
        }

        let result = returns_result();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
    }

    #[tokio::test]
    async fn test_result_type_alias_error() {
        fn returns_error() -> Result<String> {
            Err(AppError::Unauthorized)
        }

        let result = returns_error();
        assert!(result.is_err());
    }
}