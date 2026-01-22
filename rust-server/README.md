# LLM Proxy - Rust Service

[![Rust 1.85+](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)
[![Axum](https://img.shields.io/badge/Axum-0.7-blue.svg)](https://github.com/tokio-rs/axum)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[中文文档](README_CN.md) | English

High-performance LLM API proxy implementation built with Rust + Axum + Tokio, delivering exceptional performance and resource efficiency.

> For complete project overview, see the [main README](../README.md)

## 📋 Table of Contents

- [Core Features](#-core-features)
- [Tech Stack](#-tech-stack)
- [Performance Advantages](#-performance-advantages)
- [Quick Start](#-quick-start)
- [Development Guide](#️-development-guide)

## ✨ Core Features

**Complete feature parity with Python version:**

- ✅ **YAML Configuration** - Environment variable expansion support
- ✅ **Multi-Provider Support** - Weighted round-robin load balancing
- ✅ **OpenAI Compatible API** - Full support for `/v1/chat/completions`, `/v1/completions`, `/v1/models`
- ✅ **Streaming Responses** - Server-Sent Events (SSE) support
- ✅ **Health Checks** - Basic and detailed health check endpoints
- ✅ **Prometheus Monitoring** - Complete `/metrics` endpoint
- ✅ **API Key Auth** - Master key authentication mechanism
- ✅ **CORS Support** - Cross-Origin Resource Sharing configuration
- ✅ **Request Logging** - Structured logging with tracing
- ✅ **Error Handling & Retries** - Robust error handling mechanism
- ✅ **Docker Support** - Multi-stage build optimization
- ✅ **Dynamic Configuration** - PostgreSQL database configuration storage
- ✅ **Hot Reload** - Runtime configuration updates without restart
- ✅ **Admin API** - RESTful configuration management interface
- ✅ **Rate Limiting** - Optional master key rate limiting

## 🔧 Tech Stack

### Core Framework
- **Web Framework**: Axum 0.7 - Modular web framework built on Tokio
- **Async Runtime**: Tokio 1.x - Most popular async runtime in Rust
- **Routing**: Tower + Tower-HTTP - Middleware and service abstractions

### HTTP & Networking
- **HTTP Client**: reqwest 0.11 - Feature-rich async HTTP client
- **Streaming**: async-stream - Async stream processing
- **Byte Handling**: bytes 1.5 - Efficient byte buffers

### Data Processing
- **Serialization**: serde + serde_json - Zero-cost serialization/deserialization
- **Configuration**: config 0.14 + dotenvy 0.15 - Config loading and env variables
- **Database**: SQLx 0.8 - Compile-time checked SQL client
- **Hot Reload**: arc-swap 1.7 - Lock-free atomic config updates

### Monitoring & Logging
- **Monitoring**: prometheus 0.13 - Official Prometheus Rust client
- **Logging**: tracing + tracing-subscriber - Structured logging and tracing
- **Token Counting**: tiktoken-rs 0.5 - Rust port of token counting

### Security & Rate Limiting
- **Rate Limiting**: governor 0.7 - High-performance rate limiter
- **Concurrency Control**: DashMap 6.1 - Concurrent hash map
- **Key Hashing**: sha2 + hex - Secure key storage

### Error Handling
- **Error Types**: thiserror 1.0 - Derive macros for error definitions
- **Error Propagation**: anyhow 1.0 - Flexible error handling

### Development Tools
- **Testing**: tokio-test + mockito + wiremock
- **Property Testing**: proptest + quickcheck
- **Assertions**: assert_matches + pretty_assertions

## 🚀 Performance Advantages

Performance improvements over Python implementation:

| Metric | Python (FastAPI) | Rust (Axum) | Improvement |
|--------|------------------|-------------|-------------|
| **Memory Usage** | ~50-100 MB | ~10-20 MB | **↓ 5x** |
| **Startup Time** | ~1-2 seconds | ~100 milliseconds | **↑ 10-20x** |
| **Throughput (RPS)** | Baseline | 2-3x Baseline | **↑ 2-3x** |
| **P99 Latency** | Baseline | ~50% Baseline | **↓ 50%** |
| **Concurrency** | Good (asyncio) | Excellent (native async) | **Significant improvement** |
| **CPU Efficiency** | Medium (interpreted) | High (compiled) | **5-10x** |

**Key Advantages:**
- 🚀 **Ultra-Low Latency** - Native compilation, zero runtime overhead
- 💪 **High Concurrency** - Tokio runtime provides exceptional concurrent performance
- 💾 **Memory Efficiency** - Precise memory management, no GC pauses
- 🔥 **High Throughput** - Zero-copy and optimized I/O processing
- 📦 **Small Footprint** - Standalone binary, no dependencies

**Recommended For:**
- ✅ Production environments with high load
- ✅ Resource-constrained environments (containers, edge computing)
- ✅ Latency-sensitive applications
- ✅ Scenarios requiring ultimate performance

## 🚀 Quick Start

### Prerequisites

- Rust 1.85+ (install via rustup)
- PostgreSQL database
- Cargo (Rust package manager)

### 1. Install Rust

```bash
# Install Rust (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Update to latest version
rustup update
```

### 2. Build the Project

```bash
# Development build
cargo build

# Release build (for production)
cargo build --release
```

### 3. Configure Environment Variables

Create `.env` file or set environment variables:

```bash
# Required: Database connection
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# Required: Admin API authentication key
export ADMIN_KEY='your-admin-key'

# Optional: Service port (default 18000)
export PORT=18000

# Optional: Configuration file path
export CONFIG_PATH=config.yaml

# Optional: Model name prefix
export PROVIDER_SUFFIX='Proxy'
```

### 4. Run Database Migrations

```bash
# Install golang-migrate
brew install golang-migrate

# Run migrations
../scripts/db_migrate.sh up
```

### 5. Start the Service

**Option 1: Local Run**
```bash
# Development mode (with debug logging)
RUST_LOG=debug cargo run

# Release mode
cargo run --release

# Or use the built binary
./target/release/llm-proxy-rust
```

**Option 2: Docker Run**
```bash
# Build Docker image
docker build -t llm-proxy-rust:latest .

# Run container
docker run -p 18000:18000 \
  -e DB_URL='postgresql://user:pass@host.docker.internal:5432/llm_proxy' \
  -e ADMIN_KEY='your-admin-key' \
  -e PORT=18000 \
  llm-proxy-rust:latest
```

**Service Access URLs:**
- LLM Proxy: <http://localhost:18000>
- Health Check: <http://localhost:18000/health>
- Metrics: <http://localhost:18000/metrics>
- Swagger UI: <http://localhost:18000/swagger-ui/>

## ⚙️ Configuration

For detailed configuration documentation, see the [main README](../README.md#-configuration) or [CONFIGURATION.md](CONFIGURATION.md).

## 🛠️ Development Guide

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
```

### Code Quality

```bash
# Lint code
cargo clippy

# Format code
cargo fmt

# Check code without building
cargo check
```

### Building

```bash
# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Build for specific target
cargo build --release --target x86_64-unknown-linux-gnu
```

### Docker Development

```bash
# Build Docker image
docker build -t llm-proxy-rust:dev .

# Run with Docker
docker run -p 18000:18000 llm-proxy-rust:dev
```

For more details, see:
- [Main README](../README.md) - Complete project documentation
- [CONFIGURATION.md](CONFIGURATION.md) - Detailed configuration guide

## 📁 Project Structure

```text
rust-server/
├── Cargo.toml              # Dependencies and project metadata
├── Dockerfile              # Multi-stage Docker build
├── .dockerignore           # Docker ignore patterns
├── README.md               # This file
├── CONFIGURATION.md        # Detailed configuration documentation
└── src/
    ├── main.rs             # Application entry point
    ├── lib.rs              # Library entry
    ├── api/                # API layer
    │   ├── mod.rs          # API module definition
    │   ├── handlers.rs     # Request handlers
    │   ├── health.rs       # Health check endpoints
    │   ├── models.rs       # API data models
    │   ├── streaming.rs    # SSE streaming responses
    │   └── admin.rs        # Admin API endpoints
    ├── core/               # Core functionality
    │   ├── mod.rs          # Core module definition
    │   ├── config.rs       # Configuration loading and parsing
    │   ├── database.rs     # Database operations
    │   ├── error.rs        # Error types and handling
    │   ├── metrics.rs      # Prometheus metrics
    │   ├── middleware.rs   # Request middleware
    │   ├── logging.rs      # Logging configuration
    │   └── rate_limiter.rs # Rate limiter
    └── services/           # Business logic
        ├── mod.rs          # Service module definition
        ├── provider_service.rs      # Provider service
        └── health_check_service.rs  # Health check service
```

## 📄 License

MIT License

---

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

The server supports flexible configuration through environment variables and YAML files, with an optional database-backed dynamic configuration mode.

## Dynamic Configuration Mode

LLM Proxy supports two configuration modes:

### YAML Mode (Default)

- Do not set `DB_URL` environment variable
- Use `config.yaml` file for configuration
- Suitable for development and simple deployments
- Configuration changes require server restart

### Database Mode

- Set `DB_URL` and `ADMIN_KEY` environment variables
- Configuration stored in PostgreSQL database
- Supports runtime hot-reload without restart
- Suitable for production environments
- Manage configuration via Admin API

### Environment Variables for Dynamic Config

| Variable | Description | Required |
|----------|-------------|----------|
| `DB_URL` | PostgreSQL connection string | Required for database mode |
| `ADMIN_KEY` | Admin API authentication key | Required for database mode |
| `PORT` | Server port | No (default: 18000) |
| `PROVIDER_SUFFIX` | Optional prefix for model names. When set, model names like `{PROVIDER_SUFFIX}/{model}` are treated as `{model}` | No |

### Database Migration

```bash
# Install golang-migrate
brew install golang-migrate

# Set database URL
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# Run migrations
./scripts/db_migrate.sh up

# Check migration version
./scripts/db_migrate.sh version

# Rollback one migration
./scripts/db_migrate.sh down
```

### Migrate Existing YAML Config to Database

```bash
# Set environment variables
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# Run migration script
./scripts/migrate_config.sh config.yaml
```

### Admin API Examples

```bash
# Set your admin key
export ADMIN_KEY='your-admin-key'

# Create a Provider
curl -X POST http://localhost:18000/admin/v1/providers \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "openai-main",
    "provider_type": "openai",
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-xxx",
    "model_mapping": {},
    "is_enabled": true
  }'

# List all Providers
curl http://localhost:18000/admin/v1/providers \
  -H "Authorization: Bearer $ADMIN_KEY"

# Get a specific Provider
curl http://localhost:18000/admin/v1/providers/openai-main \
  -H "Authorization: Bearer $ADMIN_KEY"

# Update a Provider
curl -X PUT http://localhost:18000/admin/v1/providers/openai-main \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-new-key",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "is_enabled": true
  }'

# Delete a Provider
curl -X DELETE http://localhost:18000/admin/v1/providers/openai-main \
  -H "Authorization: Bearer $ADMIN_KEY"

# Check Provider Health
curl -X POST http://localhost:18000/admin/v1/providers/1/health \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "models": ["gpt-4", "gpt-3.5-turbo"],
    "max_concurrent": 2,
    "timeout_secs": 30
  }'

# Create a Master Key
curl -X POST http://localhost:18000/admin/v1/master-keys \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "key-1",
    "key": "mk-xxx",
    "name": "Default Key",
    "allowed_models": ["*"],
    "is_enabled": true
  }'

# List all Master Keys
curl http://localhost:18000/admin/v1/master-keys \
  -H "Authorization: Bearer $ADMIN_KEY"

# Reload configuration (hot-reload)
curl -X POST http://localhost:18000/admin/v1/config/reload \
  -H "Authorization: Bearer $ADMIN_KEY"

# Get current config version
curl http://localhost:18000/admin/v1/config/version \
  -H "Authorization: Bearer $ADMIN_KEY"
```

---

## YAML Configuration

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
| `PROVIDER_SUFFIX` | Model name prefix | None | `Proxy` |

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

### Model Name Prefix Feature

When `PROVIDER_SUFFIX` environment variable is set, you can use prefixed model names:

```bash
# Set the provider suffix
export PROVIDER_SUFFIX=Proxy

# Both of these requests are equivalent:
# 1. Using prefixed model name
curl -X POST http://localhost:18000/v1/chat/completions \
  -H "Authorization: Bearer <master_key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Proxy/gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# 2. Using original model name
curl -X POST http://localhost:18000/v1/chat/completions \
  -H "Authorization: Bearer <master_key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

#### Prefix Behavior

- If `PROVIDER_SUFFIX` is not set, model names are used as-is
- If `PROVIDER_SUFFIX` is set (e.g., "Proxy"):
  - `Proxy/gpt-4` → `gpt-4` (prefix stripped)
  - `gpt-4` → `gpt-4` (unchanged)
  - `Other/gpt-4` → `Other/gpt-4` (unchanged, different prefix)

This feature is useful for standardizing model name formats, especially when switching between different proxy services.

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
