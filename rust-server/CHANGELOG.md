# Changelog

## [Unreleased]

### Added

- **Provider Health Check API**: New endpoint `POST /admin/v1/providers/{id}/health` for checking provider health
  - Test all mapped models or specific models with configurable concurrency
  - Configurable timeout and concurrent request limits
  - Returns detailed health status for each model including latency and error information
  - Implemented in [`src/api/health.rs`](src/api/health.rs) and [`src/services/health_check_service.rs`](src/services/health_check_service.rs)

### Fixed

- **Middleware Duration Logging for Streaming**: Fixed misleading `duration` metric for streaming requests
  - **Before**: Streaming requests logged `duration` which was actually TTFB (time to first byte)
  - **After**: Streaming requests now log `ttfb` instead of `duration` for semantic accuracy
  - Non-streaming requests continue to log `duration` as before
  - Streaming detection based on `Content-Type: text/event-stream` header
  - Implemented in [`src/core/middleware.rs`](src/core/middleware.rs)

- **V1 Streaming Response Header Preservation**: Fixed V1 `/v1/chat/completions` endpoint not showing `ttfb` for streaming responses
  - **Root Cause**: Unnecessary `.into_response()` call was potentially affecting header preservation
  - **Fix**: Removed redundant `.into_response()` call in `handle_streaming_response` function
  - V1 streaming responses now correctly show `ttfb` instead of `duration` in logs
  - Implemented in [`src/api/handlers.rs`](src/api/handlers.rs#L795)

- **Streaming Connection Establishment**: Fixed delayed downstream connection establishment for streaming requests
  - **Before**: Downstream connection was established only after receiving the first token from upstream provider
  - **After**: Downstream connection is established immediately, matching Python server behavior
  - TTFT timeout is now handled inside the stream, not before returning the response
  - This improves perceived latency for streaming requests as clients receive response headers immediately
  - Implemented in [`src/api/streaming.rs`](src/api/streaming.rs)

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
