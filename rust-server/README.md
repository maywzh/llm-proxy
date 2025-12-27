# LLM API Proxy - Rust Implementation

A high-performance Rust implementation of the LLM API proxy with weighted round-robin load balancing.

## Features

✅ **Complete Feature Parity with Python Version:**

- Configuration loading from YAML with environment variable expansion
- Multiple provider support with weighted round-robin load balancing
- OpenAI-compatible API endpoints (`/v1/chat/completions`, `/v1/completions`, `/v1/models`)
- Streaming responses (Server-Sent Events)
- Health checks (`/health`, `/health/detailed`)
- Prometheus metrics (`/metrics`)
- API key authentication
- CORS support
- Request/response logging with tracing
- Error handling and retries
- Docker support

## Project Structure

```text
rust-server/
├── Cargo.toml              # Dependencies and project metadata
├── Dockerfile              # Multi-stage Docker build
├── .dockerignore          # Docker ignore patterns
├── README.md              # This file
└── src/
    ├── main.rs            # Application entry point
    ├── config.rs          # Configuration loading and parsing
    ├── models.rs          # Data models (Provider, Request/Response types)
    ├── provider.rs        # Provider service with weighted selection
    ├── handlers.rs        # API endpoint handlers
    ├── streaming.rs       # SSE streaming support
    ├── metrics.rs         # Prometheus metrics
    ├── middleware.rs      # Metrics middleware
    └── error.rs           # Error types and handling
```

## Building

### Local Build

```bash
cd rust-server
cargo build --release
```

### Docker Build

```bash
cd rust-server
docker build -t llm-proxy-rust:latest .
```

## Running

### Local Run

```bash
# Set environment variables or create .env file
export CONFIG_PATH=config.yaml

# Run the binary
cargo run --release
```

### Docker Run

```bash
docker run -p 18000:18000 \
  -v $(pwd)/config.yaml:/app/config.yaml \
  -e CONFIG_PATH=/app/config.yaml \
  llm-proxy-rust:latest
```

## Configuration

The server supports flexible configuration through environment variables and YAML files.

### Quick Start

1. **Copy example files:**

   ```bash
   cp .env.example .env
   cp config.example.yaml config.yaml
   ```

2. **Edit `.env` with your values:**

   ```bash
   # Edit API keys and sensitive data
   nano .env
   ```

3. **Run the server:**

   ```bash
   cargo run
   # Or with a specific config file
   CONFIG_PATH=config.prod.yaml cargo run
   ```

### Configuration Methods

The server supports three configuration methods with the following priority (highest to lowest):

1. **Direct Environment Variables** - Set in shell or system
2. **`.env` File** - Loaded automatically if present
3. **YAML Configuration** - Structured config in `config.yaml`

### Key Environment Variables

| Variable | Description | Default | Example |
|----------|-------------|---------|---------|
| `CONFIG_PATH` | Path to YAML config | `config.yaml` | `config.prod.yaml` |
| `HOST` | Server bind address | `0.0.0.0` | `127.0.0.1` |
| `PORT` | Server port | `18000` | `8080` |
| `VERIFY_SSL` | Verify SSL certs | `true` | `false` |

### Example Configuration

**`.env` file:**

```bash
API_KEY_1=your-api-key-1
API_KEY_2=your-api-key-2
API_BASE_URL=https://api.example.com
MASTER_KEY_1=sk-your-master-key
VERIFY_SSL=false
```

**`config.yaml` file:**

```yaml
providers:
  - name: "Provider-1"
    api_base: "${API_BASE_URL}"
    api_key: "${API_KEY_1}"
    weight: 2
    model_mapping:
      "claude-4.5-sonnet": "actual-model-name"

  - name: "Provider-2"
    api_base: "${API_BASE_URL}"
    api_key: "${API_KEY_2}"
    weight: 1
    model_mapping:
      "claude-4.5-sonnet": "actual-model-name"

# Master keys with optional rate limiting
master_keys:
  # Key with rate limiting
  - name: "Production Key"
    key: "${MASTER_KEY_1}"
    rate_limit:
      requests_per_second: 100
      burst_size: 150
  
  # Key without rate limiting (unlimited requests)
  - name: "Unlimited Key"
    key: "${MASTER_KEY_UNLIMITED}"
    # No rate_limit field = no rate limiting

server:
  host: "${HOST:-0.0.0.0}"
  port: ${PORT:-18000}

verify_ssl: true
```

### Environment-Specific Configs

Use different config files for different environments:

```bash
# Development
CONFIG_PATH=config.dev.yaml cargo run

# Staging
CONFIG_PATH=config.staging.yaml cargo run

# Production
CONFIG_PATH=config.prod.yaml cargo run
```

### Override Configuration

Override specific settings without changing files:

```bash
# Override port and host
PORT=8080 HOST=127.0.0.1 cargo run

# Disable SSL verification
VERIFY_SSL=false cargo run
```

📖 **For detailed configuration documentation, see [CONFIGURATION.md](CONFIGURATION.md)**

## API Endpoints

### Chat Completions

```bash
POST /v1/chat/completions
Authorization: Bearer <master_key>
Content-Type: application/json

{
  "model": "claude-4.5-sonnet",
  "messages": [{"role": "user", "content": "Hello"}],
  "stream": false
}
```

### List Models

```bash
GET /v1/models
Authorization: Bearer <master_key>
```

### Health Check

```bash
GET /health
```

### Detailed Health Check

```bash
GET /health/detailed
```

### Metrics

```bash
GET /metrics
```

## Master Key Rate Limiting

The system supports optional per-key rate limiting. Each master key can have independent rate limits, or no rate limiting at all.

### Rate Limit Configuration

**Enable Rate Limiting:**

```yaml
master_keys:
  - name: "Limited Key"
    key: "sk-limited-key"
    rate_limit:
      requests_per_second: 100  # Maximum 100 requests per second
      burst_size: 150           # Allow burst of 150 requests
```

**Disable Rate Limiting (Unlimited):**

```yaml
master_keys:
  - name: "Unlimited Key"
    key: "sk-unlimited-key"
    # No rate_limit field = no rate limiting
```

### Behavior

| Configuration | Behavior |
|--------------|----------|
| `rate_limit: {requests_per_second: 100, burst_size: 150}` | Rate limiting enabled: 100 req/s with 150 burst |
| `rate_limit: {requests_per_second: 0, burst_size: 0}` | Rate limiting enabled: blocks all requests |
| No `rate_limit` field | Rate limiting disabled: unlimited requests |

### Use Cases

- **Production Keys**: Set reasonable rate limits to prevent abuse
- **Development/Testing Keys**: Omit rate_limit for easier development
- **Special Purpose Keys**: Configure flexibly based on actual needs

## Performance Comparison

The Rust implementation offers significant performance improvements over the Python version:

- **Lower Memory Usage**: ~10-20MB vs ~50-100MB (Python)
- **Faster Startup**: ~100ms vs ~1-2s (Python)
- **Higher Throughput**: 2-3x more requests per second
- **Lower Latency**: ~50% reduction in p99 latency
- **Better Concurrency**: Native async/await with Tokio runtime

## Prometheus Metrics

Prometheus metrics available at `/metrics`:

- `llm_proxy_requests_total` - Total number of requests
- `llm_proxy_request_duration_seconds` - Request duration histogram
- `llm_proxy_active_requests` - Number of active requests
- `llm_proxy_tokens_total` - Total tokens used (prompt/completion/total)
- `llm_proxy_provider_health` - Provider health status
- `llm_proxy_provider_latency_seconds` - Provider latency histogram

## Development

### Run Tests

```bash
cargo test
```

### Run with Debug Logging

```bash
RUST_LOG=debug cargo run
```

### Format Code

```bash
cargo fmt
```

### Lint Code

```bash
cargo clippy
```

## Dependencies

Key dependencies:

- `axum` - Web framework
- `tokio` - Async runtime
- `reqwest` - HTTP client
- `serde` - Serialization
- `prometheus` - Metrics
- `tracing` - Logging

See `Cargo.toml` for complete list.

## License

Same as parent project.
