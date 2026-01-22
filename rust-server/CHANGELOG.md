# Changelog

## [Unreleased]

### Added

- **Provider Health Check API**: New endpoint `POST /admin/v1/providers/{id}/health` for checking provider health
  - Test all mapped models or specific models with configurable concurrency
  - Configurable timeout and concurrent request limits
  - Returns detailed health status for each model including latency and error information
  - Implemented in [`src/api/health.rs`](src/api/health.rs) and [`src/services/health_check_service.rs`](src/services/health_check_service.rs)

### Fixed

- **Faithful Error Passthrough**: Fixed proxy to faithfully return backend provider errors
  - **BREAKING**: Backend 4xx/5xx errors now return original status code (was: always 500)
  - Backend error response body is now passed through unchanged
  - Only network-level errors (connection failures, timeouts) return proxy-generated errors
  
- **Network Error Handling**: Improved handling of network-level errors
  - Connection failures return 502 Bad Gateway
  - Timeouts return 504 Gateway Timeout
  - Improved error logging to distinguish network errors from backend errors

- **Client Disconnect Handling**: Added `ClientDisconnect` error type
  - Returns HTTP 499 (Client Closed Request) status code
  - Logs at INFO level instead of ERROR for normal client cancellation
  - Note: Axum automatically handles client disconnections gracefully

### Changed

- **Error Classification**:
  - **Backend errors (4xx/5xx)** → Pass through original status code and body (faithful proxy)
  - **Network errors**:
    - Connection failures → 502 Bad Gateway
    - Timeouts → 504 Gateway Timeout
    - Request errors → 502 Bad Gateway
  - **Client errors**:
    - Client disconnect → 499 Client Closed Request
  
- **Error Logging**:
  - Backend errors logged with original status code
  - Network errors logged with error type classification

## Background

This fix ensures the proxy acts as a transparent intermediary, faithfully passing through backend errors while only generating proxy errors for network-level failures.

### Root Causes

The previous implementation always returned HTTP 500 for any backend error (4xx/5xx), which:

- Lost the original error context
- Made it impossible for clients to distinguish between different error types
- Violated the principle of transparent proxying

### Solution Approach

- Pass through backend application errors unchanged (status code + body)
- Only generate proxy errors for network-level failures
- Improve logging to distinguish error sources

## Related Issues

- Fixes incorrect error status code handling
- Improves error transparency for clients
- Maintains faithful error reporting from providers

## BREAKING CHANGE

⚠️ **Important**: This fix includes a breaking change

- **Before**: Backend 4xx/5xx errors always returned as 500
- **After**: Backend 4xx/5xx errors return with original status code

This is a **correct fix** that makes the proxy behave as expected, but may affect clients depending on the old behavior.
