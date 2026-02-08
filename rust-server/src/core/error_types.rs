//! Shared constants for structured API errors and runtime error semantics.

use std::fmt;

pub const ERROR_TYPE_API: &str = "api_error";
pub const ERROR_TYPE_TIMEOUT: &str = "timeout_error";
pub const ERROR_TYPE_INVALID_REQUEST: &str = "invalid_request_error";
pub const ERROR_TYPE_AUTHENTICATION: &str = "authentication_error";
pub const ERROR_TYPE_RATE_LIMIT: &str = "rate_limit_error";
pub const ERROR_TYPE_OVERLOADED: &str = "overloaded_error";
pub const ERROR_TYPE_STREAM: &str = "stream_error";

pub const ERROR_CODE_PROVIDER: &str = "provider_error";
pub const ERROR_CODE_TTFT_TIMEOUT: &str = "ttft_timeout";

pub const ERROR_CATEGORY_PROVIDER_4XX: &str = "provider_4xx";
pub const ERROR_CATEGORY_PROVIDER_5XX: &str = "provider_5xx";
pub const ERROR_CATEGORY_TIMEOUT: &str = "timeout";
pub const ERROR_CATEGORY_NETWORK_ERROR: &str = "network_error";
pub const ERROR_CATEGORY_CONNECT_ERROR: &str = "connect_error";
pub const ERROR_CATEGORY_STREAM_ERROR: &str = "stream_error";
pub const ERROR_CATEGORY_INTERNAL_ERROR: &str = "internal_error";

pub const PROVIDER_EJECTION_REASON_RATE_LIMIT: &str = "429";
pub const PROVIDER_EJECTION_REASON_SERVER_5XX: &str = "5xx";
pub const PROVIDER_EJECTION_REASON_TRANSPORT: &str = "transport";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategoryCode {
    Provider4xx,
    Provider5xx,
    Timeout,
    NetworkError,
    ConnectError,
    StreamError,
    InternalError,
}

impl ErrorCategoryCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Provider4xx => ERROR_CATEGORY_PROVIDER_4XX,
            Self::Provider5xx => ERROR_CATEGORY_PROVIDER_5XX,
            Self::Timeout => ERROR_CATEGORY_TIMEOUT,
            Self::NetworkError => ERROR_CATEGORY_NETWORK_ERROR,
            Self::ConnectError => ERROR_CATEGORY_CONNECT_ERROR,
            Self::StreamError => ERROR_CATEGORY_STREAM_ERROR,
            Self::InternalError => ERROR_CATEGORY_INTERNAL_ERROR,
        }
    }
}

impl fmt::Display for ErrorCategoryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderEjectionReason {
    RateLimit429,
    Server5xx,
    Transport,
}

impl ProviderEjectionReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RateLimit429 => PROVIDER_EJECTION_REASON_RATE_LIMIT,
            Self::Server5xx => PROVIDER_EJECTION_REASON_SERVER_5XX,
            Self::Transport => PROVIDER_EJECTION_REASON_TRANSPORT,
        }
    }
}

impl fmt::Display for ProviderEjectionReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_category_code_as_str() {
        assert_eq!(ErrorCategoryCode::Provider4xx.as_str(), "provider_4xx");
        assert_eq!(ErrorCategoryCode::Provider5xx.as_str(), "provider_5xx");
        assert_eq!(ErrorCategoryCode::Timeout.as_str(), "timeout");
        assert_eq!(ErrorCategoryCode::NetworkError.as_str(), "network_error");
        assert_eq!(ErrorCategoryCode::ConnectError.as_str(), "connect_error");
        assert_eq!(ErrorCategoryCode::StreamError.as_str(), "stream_error");
        assert_eq!(ErrorCategoryCode::InternalError.as_str(), "internal_error");
    }

    #[test]
    fn test_error_category_code_display() {
        assert_eq!(
            format!("{}", ErrorCategoryCode::Provider4xx),
            "provider_4xx"
        );
        assert_eq!(format!("{}", ErrorCategoryCode::Timeout), "timeout");
        assert_eq!(
            format!("{}", ErrorCategoryCode::InternalError),
            "internal_error"
        );
    }

    #[test]
    fn test_error_category_code_equality() {
        assert_eq!(
            ErrorCategoryCode::Provider4xx,
            ErrorCategoryCode::Provider4xx
        );
        assert_ne!(
            ErrorCategoryCode::Provider4xx,
            ErrorCategoryCode::Provider5xx
        );
    }

    #[test]
    fn test_provider_ejection_reason_as_str() {
        assert_eq!(ProviderEjectionReason::RateLimit429.as_str(), "429");
        assert_eq!(ProviderEjectionReason::Server5xx.as_str(), "5xx");
        assert_eq!(ProviderEjectionReason::Transport.as_str(), "transport");
    }

    #[test]
    fn test_provider_ejection_reason_display() {
        assert_eq!(format!("{}", ProviderEjectionReason::RateLimit429), "429");
        assert_eq!(format!("{}", ProviderEjectionReason::Server5xx), "5xx");
        assert_eq!(
            format!("{}", ProviderEjectionReason::Transport),
            "transport"
        );
    }

    #[test]
    fn test_provider_ejection_reason_equality() {
        assert_eq!(
            ProviderEjectionReason::RateLimit429,
            ProviderEjectionReason::RateLimit429
        );
        assert_ne!(
            ProviderEjectionReason::RateLimit429,
            ProviderEjectionReason::Server5xx
        );
    }

    #[test]
    fn test_constants_match_enum_values() {
        assert_eq!(
            ErrorCategoryCode::Provider4xx.as_str(),
            ERROR_CATEGORY_PROVIDER_4XX
        );
        assert_eq!(
            ErrorCategoryCode::Provider5xx.as_str(),
            ERROR_CATEGORY_PROVIDER_5XX
        );
        assert_eq!(ErrorCategoryCode::Timeout.as_str(), ERROR_CATEGORY_TIMEOUT);
        assert_eq!(
            ErrorCategoryCode::NetworkError.as_str(),
            ERROR_CATEGORY_NETWORK_ERROR
        );
        assert_eq!(
            ErrorCategoryCode::ConnectError.as_str(),
            ERROR_CATEGORY_CONNECT_ERROR
        );
        assert_eq!(
            ErrorCategoryCode::StreamError.as_str(),
            ERROR_CATEGORY_STREAM_ERROR
        );
        assert_eq!(
            ErrorCategoryCode::InternalError.as_str(),
            ERROR_CATEGORY_INTERNAL_ERROR
        );
        assert_eq!(
            ProviderEjectionReason::RateLimit429.as_str(),
            PROVIDER_EJECTION_REASON_RATE_LIMIT
        );
        assert_eq!(
            ProviderEjectionReason::Server5xx.as_str(),
            PROVIDER_EJECTION_REASON_SERVER_5XX
        );
        assert_eq!(
            ProviderEjectionReason::Transport.as_str(),
            PROVIDER_EJECTION_REASON_TRANSPORT
        );
    }
}
