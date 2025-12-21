# Test Scripts

This directory contains scripts for testing and verifying the LLM proxy servers.

## Authentication Verification Script

### `verify_auth.sh`

Tests authentication with master_keys configuration for both Python and Rust servers.

**Usage:**
```bash
./verify_auth.sh [SERVER_TYPE]
```

**Parameters:**
- `SERVER_TYPE`: Optional. Can be `python`, `rust`, or `both` (default: `both`)

**Examples:**
```bash
# Test both servers
./verify_auth.sh

# Test only Python server
./verify_auth.sh python

# Test only Rust server
./verify_auth.sh rust
```

**What it tests:**
1. ✓ Requests without authentication are rejected (401) when master_keys are configured
2. ✓ Requests with invalid keys are rejected (401)
3. ✓ Requests with valid keys succeed (200)
4. ✓ Rate limiting works correctly per key
5. ✓ Malformed authorization headers are rejected (401)
6. ✓ Both servers behave consistently

**Prerequisites:**
- Server must be running with master_keys configured
- For testing, use the provided `config.auth-test.yaml` files:
  ```bash
  # Python server
  cd python-server
  CONFIG_PATH=config.auth-test.yaml .venv/bin/python -m app.main
  
  # Rust server
  cd rust-server
  CONFIG_PATH=config.auth-test.yaml cargo run
  ```

**Test Keys:**
The script uses these test keys (must match your config):
- `sk-test-key-1`: Valid key with moderate rate limit (10 req/s, burst 20)
- `sk-test-key-2`: Valid key with low rate limit (5 req/s, burst 10)
- `sk-unlimited`: Valid key without rate limit
- `sk-invalid-key`: Invalid key for testing rejection

## Manual Rate Limiting Test Script

### `test_rate_limit_manual.sh`

Tests concurrent requests to verify rate limiting works correctly.

**Usage:**
```bash
./test_rate_limit_manual.sh [KEY] [CONCURRENT_COUNT]
```

**Parameters:**
- `KEY`: Master key to test (default: `sk-dev-key`)
- `CONCURRENT_COUNT`: Number of concurrent requests (default: `4`)

**Examples:**
```bash
# Test with default settings
./test_rate_limit_manual.sh

# Test specific key with 10 concurrent requests
./test_rate_limit_manual.sh sk-test-key-1 10

# Test rate limiting behavior
./test_rate_limit_manual.sh sk-test-key-2 20
```

**What it tests:**
1. Sequential requests
2. Concurrent burst requests
3. Rapid sequential requests
4. Rate limit enforcement (429 responses)

**Environment Variables:**
- `BASE_URL`: Server base URL (default: `http://localhost:18000`)

## Configuration Files for Testing

### Authentication Test Configs

Both servers have `config.auth-test.yaml` files with master_keys configured:

**Python:** `python-server/config.auth-test.yaml`
**Rust:** `rust-server/config.auth-test.yaml`

These configs include:
- Multiple master keys with different rate limits
- Keys for testing authentication and rate limiting
- Consistent configuration across both servers

## Running Tests

### Complete Test Flow

1. **Run unit tests:**
   ```bash
   # Python
   cd python-server
   .venv/bin/python -m pytest tests/test_security.py tests/test_rate_limiting.py -v
   
   # Rust
   cd rust-server
   cargo test --test integration_test authentication
   cargo test --test test_rate_limiting
   ```

2. **Start servers with auth config:**
   ```bash
   # Terminal 1 - Python server
   cd python-server
   CONFIG_PATH=config.auth-test.yaml .venv/bin/python -m app.main
   
   # Terminal 2 - Rust server
   cd rust-server
   CONFIG_PATH=config.auth-test.yaml cargo run
   ```

3. **Run verification script:**
   ```bash
   # Terminal 3
   ./scripts/verify_auth.sh
   ```

4. **Test rate limiting:**
   ```bash
   # Test Python server
   BASE_URL=http://localhost:18000 ./scripts/test_rate_limit_manual.sh sk-test-key-1 10
   
   # Test Rust server
   BASE_URL=http://localhost:18001 ./scripts/test_rate_limit_manual.sh sk-test-key-1 10
   ```

## Expected Results

### With master_keys Configured

- ✓ Requests without keys: **401 Unauthorized**
- ✓ Requests with invalid keys: **401 Unauthorized**
- ✓ Requests with valid keys: **200 OK**
- ✓ Requests exceeding rate limit: **429 Too Many Requests**
- ✓ Malformed authorization headers: **401 Unauthorized**

### Without master_keys Configured

- ⚠ All requests allowed regardless of authentication
- ⚠ No rate limiting enforced

## Troubleshooting

### Server Not Running
```
Error: Server is not running at http://localhost:18000
```
**Solution:** Start the server with the appropriate config file.

### Authentication Always Succeeds
```
⚠ Allowed without authentication (no master_keys configured)
```
**Solution:** Ensure you're using `config.auth-test.yaml` which has master_keys configured.

### Rate Limiting Not Working
```
⚠ No rate limiting detected (may have high limits)
```
**Solution:** 
- Check that rate_limit is configured for the key
- Try with a lower rate limit (e.g., `sk-test-key-2` with 5 req/s)
- Increase concurrent request count

## Notes

- The verification script tests both servers independently
- Both servers should behave identically with the same configuration
- Rate limiting is per-key, so different keys have independent limits
- Keys without rate_limit configuration are not rate limited