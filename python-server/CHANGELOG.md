# Changelog

## [Unreleased]

### Added

- **Provider Health Check API**: New endpoint `POST /admin/v1/providers/{id}/health` for checking provider health
  - Test all mapped models or specific models with configurable concurrency
  - Configurable timeout and concurrent request limits
  - Returns detailed health status for each model including latency and error information
  - Implemented in [`app/api/health.py`](app/api/health.py) and [`app/services/health_check_service.py`](app/services/health_check_service.py)

### Fixed

- **Faithful Error Passthrough**: Fixed proxy to faithfully return backend provider errors
  - **BREAKING**: Backend 4xx/5xx errors now return original status code (was: always 500)
  - Backend error response body is now passed through unchanged
  - Only network-level errors (connection failures, timeouts) return proxy-generated errors
  
- **Network Error Handling**: Added specific handling for `httpx.RemoteProtocolError`
  - Catch and log `RemoteProtocolError` when provider closes connection unexpectedly
  - Return HTTP 502 Bad Gateway for connection failures
  - Improved error logging to distinguish network errors from backend errors
  - Added handling for `httpx.RequestError` to catch general network issues

- **Client Disconnect Handling**: Added graceful handling for `ClientDisconnect` exceptions
  - When client cancels request before body is read, log at INFO level instead of ERROR
  - Returns HTTP 499 (Client Closed Request) status code
  - Prevents unnecessary error logs for normal client cancellation scenarios

### Changed

- **Error Classification**:
  - **Backend errors (4xx/5xx)** → Pass through original status code and body (faithful proxy)
  - **Network errors**:
    - `RemoteProtocolError` → HTTP 502 (Bad Gateway) - Connection closed by provider
    - `TimeoutException` → HTTP 504 (Gateway Timeout) - Request timeout
    - `RequestError` → HTTP 502 (Bad Gateway) - Network error
  - **Client errors**:
    - `ClientDisconnect` → HTTP 499 (Client Closed Request) - Client cancelled request
  
- **Streaming Error Handling**:
  - Log `RemoteProtocolError` during streaming with clear context
  - Backend errors in streaming also pass through faithfully

### Added

- **Tests**: Added comprehensive tests for `RemoteProtocolError` handling in [test_remote_protocol_error.py](tests/test_remote_protocol_error.py)
- **Documentation**: Created detailed analysis document in [.analyse/202512181432_bottlerocket_connection_error.md](.analyse/202512181432_bottlerocket_connection_error.md)

## Background

This fix addresses intermittent errors from BottleRocket-Claude-1 provider where the remote server closes the connection prematurely during chunked transfer encoding. The error manifests as:

```
httpx.RemoteProtocolError: peer closed connection without sending complete message body (incomplete chunked read)
```

### Root Causes

1. Server-side timeouts on long-running requests
2. Network instability between proxy and provider
3. Load balancer connection management
4. Provider service overload

### Solution Approach

- Catch and handle the specific error with appropriate HTTP status code
- Improve error logging for better debugging
- Faithfully return provider errors without retry (to maintain transparency)

## Related Issues

- Fixes intermittent connection errors from BottleRocket-Claude-1
- Improves error reporting for all provider network issues
- Maintains faithful error reporting from providers
