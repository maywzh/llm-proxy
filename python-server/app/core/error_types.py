"""Shared constants for structured API errors and provider ejection reasons."""

# Error type constants (for client-facing error responses)
ERROR_TYPE_API = "api_error"
ERROR_TYPE_TIMEOUT = "timeout_error"
ERROR_TYPE_INVALID_REQUEST = "invalid_request_error"
ERROR_TYPE_AUTHENTICATION = "authentication_error"
ERROR_TYPE_RATE_LIMIT = "rate_limit_error"
ERROR_TYPE_OVERLOADED = "overloaded_error"
ERROR_TYPE_STREAM = "stream_error"

# Error code constants
ERROR_CODE_PROVIDER = "provider_error"
ERROR_CODE_TTFT_TIMEOUT = "ttft_timeout"

# Provider ejection reason constants (for adaptive routing metrics labels)
EJECTION_REASON_RATE_LIMIT = "429"
EJECTION_REASON_SERVER_5XX = "5xx"
EJECTION_REASON_TRANSPORT = "transport"
