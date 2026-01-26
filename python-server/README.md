# LLM Proxy - Python Service

[![Python 3.12+](https://img.shields.io/badge/python-3.12+-blue.svg)](https://www.python.org/downloads/)
[![FastAPI](https://img.shields.io/badge/FastAPI-0.110+-green.svg)](https://fastapi.tiangolo.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[中文文档](README_CN.md) | English

High-performance LLM API proxy service built with FastAPI, supporting weighted load balancing, complete Prometheus monitoring, and Grafana visualization.

> For complete project overview, see the [main README](../README.md)

## 📋 Table of Contents

- [Core Features](#-core-features)
- [Tech Stack](#-tech-stack)
- [Quick Start](#-quick-start)
- [Configuration](#️-configuration)
- [Usage](#-usage)
- [Admin API](#-admin-api)
- [Rate Limiting](#️-rate-limiting)
- [Monitoring](#-monitoring)
- [Project Structure](#-project-structure)
- [Development Guide](#️-development-guide)
- [License](#-license)

## ✨ Core Features

- ✅ **Weighted Load Balancing** - Intelligent weighted round-robin algorithm for request distribution
- ✅ **Streaming Responses** - Complete SSE streaming response support
- ✅ **OpenAI Compatible** - 100% compatible with OpenAI API format
- ✅ **Cross-Protocol Transformation** - Accept requests in any protocol (OpenAI, Anthropic, Response API) and route to any provider
- ✅ **V2 API Endpoints** - New endpoints with full cross-protocol support
- ✅ **Model Mapping** - Flexible model name transformation and routing
- ✅ **Prometheus Monitoring** - Complete metrics collection and export
- ✅ **Grafana Visualization** - Pre-configured dashboards and alerts
- ✅ **Token Statistics** - Accurate token usage tracking (using tiktoken)
- ✅ **Latency Tracking** - P50/P95/P99 latency percentile monitoring
- ✅ **Health Checks** - Real-time provider health monitoring
- ✅ **Modular Architecture** - Clear layered architecture design
- ✅ **Type Safety** - Pydantic 2.0+ data validation
- ✅ **Rate Limiting** - Optional per-key rate limiting
- ✅ **Dynamic Configuration** - PostgreSQL-based hot-reload configuration
- ✅ **Async Processing** - Full async architecture with FastAPI + httpx
- ✅ **Langfuse Integration** - Optional LLM observability and tracing
- ✅ **JSONL Logging** - Optional async JSONL file logging for debugging

## 🔧 Tech Stack

### Core Framework
- **Web Framework**: FastAPI 0.110+ - High-performance async Python web framework
- **ASGI Server**: Uvicorn - Production-grade ASGI server
- **Python Version**: Python 3.12+

### Data Processing
- **Data Validation**: Pydantic 2.0+ - Type-safe data models and validation
- **Database ORM**: SQLAlchemy 2.0+ - Async ORM
- **Database Driver**: asyncpg - High-performance async PostgreSQL driver

### HTTP & Networking
- **HTTP Client**: httpx - Async HTTP client
- **Streaming**: SSE (Server-Sent Events)

### Monitoring & Logging
- **Metrics Collection**: prometheus-client - Official Prometheus Python client
- **Logging System**: loguru - Modern Python logging library
- **Token Counting**: tiktoken - OpenAI's official token counting library

### Security & Rate Limiting
- **Rate Limiting**: limits 3.10+ - Token bucket algorithm implementation
- **Authentication**: Bearer Token authentication

### Development Tools
- **Package Manager**: uv - Ultra-fast Python package manager
- **Testing Framework**: pytest + pytest-asyncio + pytest-cov
- **Testing Tools**: hypothesis (property testing) + respx (HTTP mocking)

## 🚀 Quick Start

### Prerequisites

- Python 3.12+
- PostgreSQL database
- uv (Python package manager)

### 1. Install Dependencies

```bash
# Install uv (if not already installed)
curl -LsSf https://astral.sh/uv/install.sh | sh

# Sync dependencies
uv sync
```

### 2. Configure Environment Variables

Create `.env` file or set environment variables:

```bash
# Required: Database connection
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# Required: Admin API authentication key
export ADMIN_KEY='your-admin-key'

# Optional: Service port (default 18000)
export PORT=18000

# Optional: Model name prefix (for standardizing model name formats)
export PROVIDER_SUFFIX='Proxy'
```

### 3. Run Database Migrations

```bash
# Install golang-migrate
brew install golang-migrate

# Set database URL
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# Run migrations
../scripts/db_migrate.sh up

# Check migration version
../scripts/db_migrate.sh version

# Rollback one migration
../scripts/db_migrate.sh down
```

### 4. Start the Service

**Option 1: Direct Run**
```bash
# Using quick start script
./run.sh

# Or using uv
uv run python main.py
```

**Option 2: Docker Compose (Recommended, includes monitoring)**
```bash
# Start all services (LLM Proxy + Prometheus + Grafana)
docker-compose up -d

# View logs
docker-compose logs -f llm-proxy

# Stop services
docker-compose down
```

**Service Access URLs:**
- LLM Proxy: <http://localhost:18000>
- Prometheus: <http://localhost:9090>
- Grafana: <http://localhost:3000> (admin/admin)
- API Documentation: <http://localhost:18000/docs>

## ⚙️ Configuration

For detailed configuration documentation, see the [main README](../README.md#-configuration).

## 📖 Usage

Once the proxy service is running, you can use it just like the OpenAI API:

### Chat Completions

```bash
curl http://localhost:18000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Streaming Responses

```bash
curl http://localhost:18000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

### Model Name Prefix Feature

When `PROVIDER_SUFFIX` environment variable is set, you can use prefixed model names:

```bash
# Set prefix
export PROVIDER_SUFFIX=Proxy

# The following two requests are equivalent:
# 1. Using prefixed model name
curl http://localhost:18000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Proxy/gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# 2. Using original model name
curl http://localhost:18000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

#### Prefix Feature Behavior

- If `PROVIDER_SUFFIX` is not set, model names remain unchanged
- If `PROVIDER_SUFFIX` is set (e.g., "Proxy"):
  - `Proxy/gpt-4` → `gpt-4` (prefix removed)
  - `gpt-4` → `gpt-4` (unchanged)
  - `Other/gpt-4` → `Other/gpt-4` (different prefix, unchanged)

This feature is useful for scenarios requiring unified model name formats, especially when switching between multiple proxy services.

### Health Checks

```bash
# Basic health check
curl http://localhost:18000/health

# Detailed health check (tests all providers)
curl http://localhost:18000/health/detailed
```

### Supported Endpoints

**V1 API (Legacy)**
- `/v1/chat/completions` - Chat completions API
- `/v1/completions` - Legacy completions API
- `/v1/models` - List all available models

**V2 API (Cross-Protocol Support)**
- `/v2/chat/completions` - OpenAI-compatible with cross-protocol transformation
- `/v2/messages` - Anthropic-compatible with cross-protocol transformation
- `/v2/responses` - Response API with cross-protocol transformation

**Other Endpoints**
- `/health` - Basic health check
- `/health/detailed` - Detailed health check (tests all providers)
- `/metrics` - Prometheus metrics endpoint
- `/docs` - OpenAPI documentation

### V2 API: Cross-Protocol Transformation

The V2 API endpoints support cross-protocol transformation, allowing you to send requests in any supported format and route them to any provider:

```bash
# Send OpenAI request to Anthropic provider
curl http://localhost:18000/v2/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "claude-3-opus",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Send Anthropic request to OpenAI provider
curl http://localhost:18000/v2/messages \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "gpt-4",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# Send Response API request to any provider
curl http://localhost:18000/v2/responses \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "gpt-4",
    "instructions": "You are a helpful assistant.",
    "input": "Hello!",
    "max_output_tokens": 1000
  }'
```

**How It Works:**
1. The proxy detects the client protocol from the request format
2. Transforms the request to the provider's native protocol
3. Forwards the request to the selected provider
4. Transforms the response back to the client's expected format

**Performance:**
- Same-protocol requests use bypass optimization (minimal overhead)
- Cross-protocol requests use the full transformation pipeline
- Metrics track bypass vs. cross-protocol usage

## 🔑 Admin API

### Provider Management

```bash
# Set Admin Key
export ADMIN_KEY='your-admin-key'

# Create Provider
curl -X POST http://localhost:18000/admin/v1/providers \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "provider_key": "openai-main",
    "provider_type": "openai",
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-xxx",
    "model_mapping": {},
    "is_enabled": true
  }'

# List all Providers
curl http://localhost:18000/admin/v1/providers \
  -H "Authorization: Bearer $ADMIN_KEY"

# Get specific Provider
curl http://localhost:18000/admin/v1/providers/1 \
  -H "Authorization: Bearer $ADMIN_KEY"

# Update Provider
curl -X PUT http://localhost:18000/admin/v1/providers/1 \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-new-key",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "is_enabled": true
  }'

# Delete Provider
curl -X DELETE http://localhost:18000/admin/v1/providers/1 \
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
```

### Credential Management

```bash
# Create Credential
curl -X POST http://localhost:18000/admin/v1/credentials \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "key": "sk-my-secret-key",
    "name": "Default Key",
    "allowed_models": ["*"],
    "is_enabled": true
  }'

# List all Credentials
curl http://localhost:18000/admin/v1/credentials \
  -H "Authorization: Bearer $ADMIN_KEY"

# Reload configuration (hot update)
curl -X POST http://localhost:18000/admin/v1/config/reload \
  -H "Authorization: Bearer $ADMIN_KEY"

# Get current config version
curl http://localhost:18000/admin/v1/config/version \
  -H "Authorization: Bearer $ADMIN_KEY"
```

## ⏱️ Rate Limiting

The system supports independent rate limiting for each credential key, or rate limiting can be completely disabled.

### Configuration

Configure rate limiting when creating credentials via Admin API:

```bash
# Create key with rate limiting
curl -X POST http://localhost:18000/admin/v1/credentials \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "key": "sk-limited",
    "name": "Limited Key",
    "rate_limit": 100,
    "is_enabled": true
  }'

# Create key without rate limiting (rate_limit set to null or omitted)
curl -X POST http://localhost:18000/admin/v1/credentials \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "key": "sk-unlimited",
    "name": "Unlimited Key",
    "is_enabled": true
  }'
```

### Behavior

| Configuration | Behavior |
|---------------|----------|
| `rate_limit: 100` | Rate limiting enabled: 100 requests per second |
| `rate_limit: 0` | Rate limiting enabled: blocks all requests |
| `rate_limit` field not set | Rate limiting disabled: unlimited requests |

### Use Cases

- **Production Keys**: Set reasonable rate limits to prevent abuse
- **Development/Test Keys**: Can disable rate limiting for easier debugging
- **Special Purpose Keys**: Configure flexibly based on actual needs

## 🔍 Langfuse Integration

LLM Proxy supports optional [Langfuse](https://langfuse.com) integration for LLM observability and tracing.

### Features

- **Request Tracing**: Captures provider info, request/response data, token usage
- **TTFT Tracking**: Time to First Token metrics for streaming requests
- **Sampling Support**: Configurable sampling rate for high-traffic scenarios
- **Background Batching**: Minimal latency impact with async batching

### Configuration

Set the following environment variables to enable Langfuse:

| Variable | Description | Default |
|----------|-------------|---------|
| `LANGFUSE_ENABLED` | Enable Langfuse tracing | `false` |
| `LANGFUSE_PUBLIC_KEY` | Langfuse public key (required when enabled) | - |
| `LANGFUSE_SECRET_KEY` | Langfuse secret key (required when enabled) | - |
| `LANGFUSE_HOST` | Langfuse server URL | `https://cloud.langfuse.com` |
| `LANGFUSE_SAMPLE_RATE` | Sampling rate (0.0-1.0) | `1.0` |
| `LANGFUSE_FLUSH_INTERVAL` | Flush interval in seconds | `5` |
| `LANGFUSE_DEBUG` | Enable debug logging | `false` |

### Enabling Langfuse

1. Sign up at [Langfuse](https://langfuse.com) and create a project
2. Get your public and secret keys from the project settings
3. Set environment variables:

```bash
export LANGFUSE_ENABLED=true
export LANGFUSE_PUBLIC_KEY=pk-lf-...
export LANGFUSE_SECRET_KEY=sk-lf-...
```

1. Restart the server

Traces will appear in your Langfuse dashboard with:
- Provider information (which provider handled the request)
- Model mapping (original vs mapped model names)
- Token usage (prompt, completion, total)
- Timing metrics (duration, TTFT for streaming)
- Error details (if request failed)

## 📝 JSONL Logging

LLM Proxy supports optional JSONL file logging for debugging and analysis. Requests and responses are logged as separate JSONL lines, linked by `request_id`.

### Features

- **Async Logging**: Non-blocking buffered writes with periodic flushing
- **Separate Records**: Client requests, provider requests, and responses logged separately
- **Streaming Support**: Captures full chunk sequences for streaming responses
- **Linked Records**: All records share the same `request_id` for correlation

### Configuration

Set the following environment variables to enable JSONL logging:

| Variable | Description | Default |
|----------|-------------|---------|
| `JSONL_LOG_ENABLED` | Enable JSONL logging | `false` |
| `JSONL_LOG_PATH` | Path to JSONL log file | `./logs/requests.jsonl` |
| `JSONL_LOG_BUFFER_SIZE` | Queue buffer size | `1000` |

### Enabling JSONL Logging

```bash
export JSONL_LOG_ENABLED=true
export JSONL_LOG_PATH=./logs/requests.jsonl
export JSONL_LOG_BUFFER_SIZE=1000
```

### Log Record Types

Each JSONL line contains one of these record types:

1. **`request`** - Client request received
   ```json
   {
     "type": "request",
     "timestamp": "2026-01-23T19:30:00.000Z",
     "request_id": "req-123",
     "endpoint": "/v2/chat/completions",
     "provider": "openai-main",
     "payload": {...}
   }
   ```

2. **`provider_request`** - Request sent to provider
   ```json
   {
     "type": "provider_request",
     "timestamp": "2026-01-23T19:30:00.100Z",
     "request_id": "req-123",
     "provider": "openai-main",
     "api_base": "https://api.openai.com/v1",
     "endpoint": "/chat/completions",
     "payload": {...}
   }
   ```

3. **`provider_response`** - Response from provider
   ```json
   {
     "type": "provider_response",
     "timestamp": "2026-01-23T19:30:01.000Z",
     "request_id": "req-123",
     "provider": "openai-main",
     "status_code": 200,
     "body": {...}
   }
   ```

4. **`response`** - Response sent to client
   ```json
   {
     "type": "response",
     "timestamp": "2026-01-23T19:30:01.100Z",
     "request_id": "req-123",
     "status_code": 200,
     "body": {...}
   }
   ```

For streaming responses, `body` is replaced with `chunk_sequence` containing all SSE chunks.

## � Monitoring

### Prometheus Metrics

The system automatically collects the following metrics:

- **Request Metrics**
  - `llm_proxy_requests_total`: Total request count (by method, endpoint, model, provider, status_code)
  - `llm_proxy_request_duration_seconds`: Request latency histogram
  - `llm_proxy_active_requests`: Current active request count

- **Token Usage Metrics**
  - `llm_proxy_tokens_total`: Total token usage (by model, provider, token_type)

- **Provider Health Metrics**
  - `llm_proxy_provider_health`: Provider health status
  - `llm_proxy_provider_latency_seconds`: Provider response latency

### Grafana Dashboard

Pre-configured dashboards include:

- Request rate trends
- P95/P99 latency
- Token usage statistics
- Status code distribution
- Provider load distribution
- Real-time active request count

For detailed documentation, see [MONITORING.md](MONITORING.md)

## 📁 Project Structure

```
app/
├── api/          # API routes
├── core/         # Core functionality (config, security, monitoring)
├── models/       # Pydantic data models
├── services/     # Business logic layer
└── utils/        # Utility functions

grafana/          # Grafana configuration and dashboards
prometheus/       # Prometheus configuration
```

For detailed architecture notes, see [REFACTORING.md](REFACTORING.md)

## 🛠️ Development Guide

### Running Tests

```bash
# Run all tests
make test

# Generate coverage report
make coverage

# Run specific test file
pytest tests/test_specific.py -v
```

### Code Quality

```bash
# Format code
make format

# Lint code
make lint

# Type checking
mypy app
```

### Docker Development

```bash
# Build Docker image
docker build -t llm-proxy:dev .

# Run with Docker Compose
docker-compose up -d

# View logs
docker-compose logs -f
```

## How It Works

1. The proxy reads multiple API provider configurations from the database
2. Uses weighted random algorithm to select a provider
3. Forwards the request to the selected provider
4. Returns the provider's response to the client

Based on configured weights, requests are distributed proportionally to different providers, achieving load balancing.

## Notes

- Ensure all providers use the same API format (default OpenAI format)
- API keys must be valid and have sufficient quota
- It's recommended to configure Grafana alert rules in production environments

## Related Documentation

- [Main README](../README.md) - Complete project documentation
- [REFACTORING.md](REFACTORING.md) - Architecture design notes
- [MONITORING.md](MONITORING.md) - Monitoring system documentation
- [DOCKER_USAGE.md](DOCKER_USAGE.md) - Docker usage guide

## 📄 License

MIT License
