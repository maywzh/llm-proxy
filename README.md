# llm-proxy

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.12+](https://img.shields.io/badge/python-3.12+-blue.svg)](https://www.python.org/downloads/)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)

[中文文档](README_CN.md) | English

High-performance, OpenAI-compatible LLM API proxy with weighted load balancing, streaming support, and built-in observability. This repository contains two first-class implementations with feature parity:

- **Python Service** ([python-server/](python-server/)) - Built with FastAPI + Uvicorn for rapid development and deployment
- **Rust Service** ([rust-server/](rust-server/)) - Built with Axum + Tokio for ultimate performance and resource efficiency

Both implementations expose identical endpoints (`/v1/chat/completions`, `/v1/completions`, `/v1/models`, `/health`, `/health/detailed`, `/metrics`) and share the same configuration concepts (providers, weighted routing, model mapping, master keys, SSL verification).

## 📋 Table of Contents

- [Core Features](#-core-features)
- [Architecture](#️-architecture)
- [Quick Start](#-quick-start)
- [Configuration](#️-configuration)
- [Dynamic Configuration](#️-dynamic-configuration)
- [Monitoring](#-monitoring)
- [Performance Comparison](#-performance-comparison)
- [Project Structure](#-project-structure)
- [Development Guide](#️-development-guide)
- [License](#-license)

## ✨ Core Features

### Load Balancing & Routing
- ⚖️ **Weighted Round-Robin** - Intelligent load distribution across multiple API providers
- 🔄 **Model Mapping** - Flexible model name transformation and routing
- 🎯 **Smart Selection** - Automatic provider selection based on weights and health status

### API Compatibility
- 🔌 **OpenAI Compatible** - 100% compatible with OpenAI API format
- 📡 **Streaming Support** - Real-time SSE streaming responses
- 🔄 **Non-Streaming** - Standard JSON response mode

### Security & Authentication
- 🔐 **Master Key Auth** - Unified API key management
- 🚦 **Rate Limiting** - Optional per-key rate limiting with burst support
- 🔓 **Flexible Auth** - Support for unlimited keys in development environments

### Observability
- 📊 **Prometheus Metrics** - Complete metrics collection and export
- 📈 **Grafana Dashboards** - Pre-configured visualization panels
- 💊 **Health Checks** - Basic and detailed health check endpoints
- 📝 **Request Tracing** - Detailed request/response logging
- 🔍 **Langfuse Integration** - Optional LLM observability and tracing

### Configuration Management
- 🗄️ **Dynamic Config** - PostgreSQL-based runtime configuration
- 🔥 **Hot Reload** - Configuration updates without restart
- 📝 **YAML Mode** - Simple file-based configuration
- 🔧 **Admin API** - RESTful configuration management interface

### Deployment Options
- 🐳 **Docker Support** - Complete containerization solution
- ☸️ **Kubernetes Manifests** - Development environment deployment examples
- 📦 **Docker Compose** - One-command monitoring stack deployment
- 🚀 **Binary Deployment** - Standalone executables (Rust version)

## 🏗️ Architecture

### Python Implementation (FastAPI)

**Core Tech Stack:**
- **Web Framework**: FastAPI 0.110+ (High-performance async framework)
- **ASGI Server**: Uvicorn (Production-grade async server)
- **HTTP Client**: httpx (Async HTTP client)
- **Data Validation**: Pydantic 2.0+ (Type-safe data models)
- **Database**: PostgreSQL + SQLAlchemy 2.0 + asyncpg (Async ORM)
- **Monitoring**: prometheus-client (Metrics collection)
- **Logging**: loguru (Structured logging)
- **Rate Limiting**: limits 3.10+ (Token bucket algorithm)
- **Token Counting**: tiktoken (Accurate token usage stats)

**Project Structure:**
```
python-server/
├── app/
│   ├── api/          # API routes and endpoints
│   │   ├── admin.py      # Admin API (dynamic config management)
│   │   ├── chat.py       # Chat completions endpoint
│   │   ├── completions.py # Text completions endpoint
│   │   ├── health.py     # Health check endpoints
│   │   ├── models.py     # Model list endpoint
│   │   └── metrics.py    # Prometheus metrics endpoint
│   ├── core/         # Core functionality
│   │   ├── config.py     # Configuration loading and management
│   │   ├── database.py   # Database connections and operations
│   │   ├── security.py   # Authentication and authorization
│   │   ├── rate_limiter.py # Rate limiter
│   │   ├── metrics.py    # Prometheus metrics definitions
│   │   ├── logging.py    # Logging configuration
│   │   ├── http_client.py # HTTP client wrapper
│   │   ├── middleware.py # Middleware
│   │   └── exceptions.py # Custom exceptions
│   ├── models/       # Data models
│   │   ├── config.py     # Configuration models
│   │   ├── provider.py   # Provider models
│   │   └── health.py     # Health check models
│   ├── services/     # Business logic
│   │   ├── provider_service.py # Provider selection and management
│   │   └── health_check_service.py # Health check service
│   └── utils/        # Utility functions
│       └── streaming.py  # SSE streaming response handling
├── tests/           # Test suite
├── grafana/         # Grafana configuration and dashboards
└── prometheus/      # Prometheus configuration
```

### Rust Implementation (Axum)

**Core Tech Stack:**
- **Web Framework**: Axum 0.7 (Tokio-based high-performance framework)
- **Async Runtime**: Tokio 1.x (Most popular async runtime in Rust ecosystem)
- **HTTP Client**: reqwest 0.11 (Async HTTP client)
- **Serialization**: serde + serde_json (Zero-cost serialization)
- **Database**: SQLx 0.8 (Compile-time checked SQL client)
- **Monitoring**: prometheus 0.13 (Official Rust client)
- **Logging**: tracing + tracing-subscriber (Structured tracing)
- **Rate Limiting**: governor 0.7 (Efficient rate limiter)
- **Token Counting**: tiktoken-rs (Rust port)
- **Hot Reload**: arc-swap 1.7 (Lock-free configuration updates)

**Project Structure:**
```
rust-server/
└── src/
    ├── main.rs       # Application entry point
    ├── lib.rs        # Library entry
    ├── api/          # API layer
    │   ├── handlers.rs   # Request handlers
    │   ├── health.rs     # Health checks
    │   ├── models.rs     # API data models
    │   ├── streaming.rs  # SSE streaming responses
    │   └── admin.rs      # Admin API
    ├── core/         # Core functionality
    │   ├── config.rs     # Configuration loading
    │   ├── database.rs   # Database operations
    │   ├── error.rs      # Error handling
    │   ├── metrics.rs    # Prometheus metrics
    │   ├── middleware.rs # Middleware
    │   ├── logging.rs    # Logging configuration
    │   └── rate_limiter.rs # Rate limiter
    └── services/     # Business logic
        ├── provider_service.rs # Provider service
        └── health_check_service.rs # Health check service
```

## 🚀 Quick Start

### Option 1: Python Service (Recommended for Rapid Development)

**1. Install Dependencies:**
```bash
cd python-server
# Install uv (if not already installed)
curl -LsSf https://astral.sh/uv/install.sh | sh
# Sync dependencies
uv sync
```

**2. Setup Database and Environment Variables:**
```bash
# Set environment variables
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'
export ADMIN_KEY='your-admin-key'
export PORT=18000

# Run database migrations
./scripts/db_migrate.sh up
```

**3. Start the Service:**
```bash
# Using quick start script
./run.sh

# Or using uv
uv run python main.py
```

**4. Using Docker Compose (includes Prometheus + Grafana):**
```bash
docker-compose up -d
# LLM Proxy: http://localhost:18000
# Prometheus: http://localhost:9090
# Grafana: http://localhost:3000 (admin/admin)
```

**5. Run Tests:**
```bash
make test       # Run all tests
make coverage   # Generate coverage report
```

More details: [python-server/README.md](python-server/README.md)

### Option 2: Rust Service (Recommended for Production)

**1. Build the Project:**
```bash
cd rust-server
cargo build --release
```

**2. Setup Environment Variables:**
```bash
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'
export ADMIN_KEY='your-admin-key'
export PORT=18000

# Run database migrations
./scripts/db_migrate.sh up
```

**3. Start the Service:**
```bash
# Run directly
CONFIG_PATH=config.yaml cargo run --release

# Or use the built binary
./target/release/llm-proxy-rust
```

**4. Using Docker:**
```bash
# Build image
docker build -t llm-proxy-rust:latest .

# Run container
docker run -p 18000:18000 \
  -v $(pwd)/config.yaml:/app/config.yaml \
  -e CONFIG_PATH=/app/config.yaml \
  -e DB_URL='postgresql://user:pass@localhost:5432/llm_proxy' \
  -e ADMIN_KEY='your-admin-key' \
  llm-proxy-rust:latest
```

**5. Run Tests:**
```bash
cargo test
cargo clippy  # Linting
cargo fmt     # Formatting
```

More details: [rust-server/README.md](rust-server/README.md)

## ⚙️ Configuration

The system supports two configuration modes. Both implementations share the same configuration format:

### Environment Variable Priority

```
Environment Variables > .env File > YAML Configuration File
```

### Core Configuration Fields

```yaml
# Provider configuration
providers:
  - name: Provider-1
    api_base: "${API_BASE_URL}"
    api_key: "${API_KEY_1}"
    weight: 2  # Higher weight = more requests
    model_mapping:
      # Exact match
      "claude-4.5-sonnet": "actual-provider-model"
      # Wildcard/regex patterns supported:
      "claude-opus-4-5-.*": "claude-opus-mapped"  # Regex pattern
      "gemini-*": "gemini-pro"                     # Simple wildcard (* -> .*)

# Master Key configuration
master_keys:
  # Key with rate limiting
  - name: "Production Key"
    key: "sk-prod-key"
    rate_limit:
      requests_per_second: 100  # Requests per second
      burst_size: 150            # Burst requests

  # Key without rate limiting (for development)
  - name: "Unlimited Key"
    key: "sk-dev-key"
    # No rate_limit field = unlimited

# Server configuration
server:
  host: 0.0.0.0
  port: 18000

# SSL verification (when calling providers)
verify_ssl: false
```

### Environment Variables

| Variable | Description | Required | Default |
|----------|-------------|----------|---------|
| `DB_URL` | PostgreSQL connection string | Yes | - |
| `ADMIN_KEY` | Admin API authentication key | Yes | - |
| `PORT` | Service port | No | 18000 |
| `PROVIDER_SUFFIX` | Model name prefix filter | No | - |
| `VERIFY_SSL` | Verify provider SSL certificates | No | true |

### Langfuse Observability (Optional)

| Variable | Description | Default |
|----------|-------------|---------|
| `LANGFUSE_ENABLED` | Enable Langfuse tracing | `false` |
| `LANGFUSE_PUBLIC_KEY` | Langfuse public key (required when enabled) | - |
| `LANGFUSE_SECRET_KEY` | Langfuse secret key (required when enabled) | - |
| `LANGFUSE_HOST` | Langfuse server URL | `https://cloud.langfuse.com` |
| `LANGFUSE_SAMPLE_RATE` | Sampling rate (0.0-1.0) | `1.0` |
| `LANGFUSE_FLUSH_INTERVAL` | Flush interval in seconds | `5` |
| `LANGFUSE_DEBUG` | Enable debug logging | `false` |

### Model Name Prefix Feature

When `PROVIDER_SUFFIX=Proxy` is set:
- `Proxy/gpt-4` → Automatically converted to `gpt-4`
- `gpt-4` → Remains unchanged
- `Other/gpt-4` → Remains unchanged (different prefix)

This feature is useful for standardizing model name formats when switching between proxy services.

## 🗄️ Dynamic Configuration

The system supports two configuration modes:

### YAML Mode (Simple Deployment)

- **Do not set** `DB_URL` environment variable
- Use `config.yaml` file for configuration
- Suitable for development and simple deployments
- Configuration changes require service restart

### Database Mode (Production Recommended)

- **Set** `DB_URL` and `ADMIN_KEY` environment variables
- Configuration stored in PostgreSQL database
- Supports runtime hot-reload without restart
- Suitable for production environments
- Manage configuration via Admin API

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
```

### Migrate Existing YAML Config to Database

```bash
# Migrate configuration file to database
./scripts/migrate_config.sh config.yaml
```

### Admin API Usage Examples

```bash
export ADMIN_KEY='your-admin-key'

# Create Provider
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

# Check Provider Health
curl -X POST http://localhost:18000/admin/v1/providers/1/health \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "models": ["gpt-4", "gpt-3.5-turbo"],
    "max_concurrent": 2,
    "timeout_secs": 30
  }'

# Create Master Key
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

# Hot reload configuration (no restart required)
curl -X POST http://localhost:18000/admin/v1/config/reload \
  -H "Authorization: Bearer $ADMIN_KEY"
```

Complete Admin API documentation: [rust-server/README.md](rust-server/README.md) or [python-server/README.md](python-server/README.md)

## 📊 Monitoring

### Prometheus Metrics

The system automatically exposes `/metrics` endpoint, collected via [python-server/prometheus/prometheus.yml](python-server/prometheus/prometheus.yml), including:

**Request Metrics:**
- `llm_proxy_requests_total` - Total requests (labeled by method, endpoint, model, provider, status_code)
- `llm_proxy_request_duration_seconds` - Request latency histogram (P50/P95/P99)
- `llm_proxy_active_requests` - Current active requests

**Token Usage Metrics:**
- `llm_proxy_tokens_total` - Total token usage (labeled by model, provider, token_type)
  - `token_type`: prompt_tokens, completion_tokens, total_tokens

**Provider Health Metrics:**
- `llm_proxy_provider_health` - Provider health status (1=healthy, 0=unhealthy)
- `llm_proxy_provider_latency_seconds` - Provider response latency histogram

### Grafana Dashboards

Pre-configured dashboards located in [python-server/grafana/dashboards/](python-server/grafana/dashboards/), auto-provisioned via [python-server/grafana/provisioning/](python-server/grafana/provisioning/), including:

- 📈 **Request Rate Trends** - Requests per second (RPS) time series
- ⏱️ **Latency Analysis** - P50/P95/P99 latency percentiles
- 🎫 **Token Usage Stats** - Token consumption by model and provider
- 📊 **Status Code Distribution** - HTTP status code pie chart
- ⚖️ **Provider Load** - Request distribution across providers
- 🏥 **Health Status** - Provider health checks and availability
- 🔥 **Active Requests** - Current concurrent requests

### Start Monitoring Stack

```bash
cd python-server
docker-compose up -d

# Access URLs:
# - Grafana: http://localhost:3000 (admin/admin)
# - Prometheus: http://localhost:9090
```

## 🚄 Performance Comparison

Rust implementation performance advantages over Python:

| Metric | Python (FastAPI) | Rust (Axum) | Improvement |
|--------|------------------|-------------|-------------|
| **Memory Usage** | ~50-100 MB | ~10-20 MB | **5x less** |
| **Startup Time** | ~1-2 seconds | ~100 milliseconds | **10x faster** |
| **Throughput** | Baseline | 2-3x Baseline | **2-3x higher** |
| **P99 Latency** | Baseline | ~50% Baseline | **50% lower** |
| **Concurrency** | Good | Excellent | **Native async** |

**Recommendations:**
- **Python**: Rapid development, prototyping, teams familiar with Python
- **Rust**: Production environments, high-performance requirements, resource-constrained environments

## 📁 Project Structure

```
llm-proxy/
├── python-server/          # Python FastAPI implementation
│   ├── app/               # Application code
│   │   ├── api/          # API routing layer
│   │   ├── core/         # Core functionality (config, database, security)
│   │   ├── models/       # Pydantic data models
│   │   ├── services/     # Business logic layer
│   │   └── utils/        # Utility functions
│   ├── tests/            # Test suite
│   ├── grafana/          # Grafana configuration and dashboards
│   ├── prometheus/       # Prometheus configuration
│   ├── Makefile          # Development commands
│   ├── pyproject.toml    # Python dependencies
│   └── README.md         # Python service documentation
│
├── rust-server/           # Rust Axum implementation
│   ├── src/              # Source code
│   │   ├── api/         # API layer
│   │   ├── core/        # Core functionality
│   │   └── services/    # Business logic
│   ├── Cargo.toml       # Rust dependencies
│   └── README.md        # Rust service documentation
│
├── migrations/           # Database migration scripts
├── scripts/             # Operations scripts
│   ├── db_migrate.sh   # Database migration
│   └── migrate_config.sh # Configuration migration
├── k8s/                 # Kubernetes deployment manifests
│   └── dev/            # Development environment examples
├── web/                 # Admin UI (optional)
└── README.md           # This file
```

## 🛠️ Development Guide

### Python Development

```bash
cd python-server

# Install development dependencies
uv sync

# Run tests
make test

# Generate coverage report
make coverage

# Format code
make format

# Lint code
make lint
```

### Rust Development

```bash
cd rust-server

# Run tests
cargo test

# Lint code
cargo clippy

# Format code
cargo fmt

# Build release version
cargo build --release
```

### Kubernetes Deployment

Development environment deployment example:

```bash
cd k8s/dev

# Apply configuration
./deploy.sh

# Or manually apply
kubectl apply -f k8s.yaml
```

Deployment manifests: [k8s/dev/k8s.yaml](k8s/dev/k8s.yaml)

## 📚 Related Documentation

### Python Implementation
- [python-server/README.md](python-server/README.md) - Detailed usage documentation
- [python-server/REFACTORING.md](python-server/REFACTORING.md) - Architecture design notes

### Rust Implementation
- [rust-server/README.md](rust-server/README.md) - Detailed usage documentation
- [rust-server/CONFIGURATION.md](rust-server/CONFIGURATION.md) - Configuration guide

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for branch naming, commit format, and PR process.

**Branch strategy:** Simplified Gitflow — `feature/`, `fix/`, `refactor/` branches target `develop`; `hotfix/` targets `main`.

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for release history, auto-generated from Conventional Commits via [git-cliff](https://git-cliff.org/).

## 📄 License

MIT License - See project files for details
