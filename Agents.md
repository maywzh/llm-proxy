# Agents.md

## TL;DR

LLM Proxy is a high-performance, OpenAI-compatible API proxy with weighted load balancing, streaming support, and built-in observability. It provides two first-class server implementations (Python/FastAPI and Rust/Axum), two admin web interfaces (React and Svelte), PostgreSQL-backed dynamic configuration, and Kubernetes deployment manifests.

---

## ⚠️ MANDATORY RULES FOR AI AGENTS

**BEFORE making any code changes:**

1. Read and understand the relevant documentation in the `docs/` directory
2. Follow the coding standards

**AFTER making any code changes:**

1. Update the relevant documentation files in `docs/` to reflect your changes
2. Update the "Last Updated" date at the bottom of any modified documentation file
3. Ensure all code references (file:line) in documentation are still accurate

**Documentation is the source of truth. Keep it synchronized with the code.**

## 1. Project Structure

```
llm-proxy/
├── python-server/          # FastAPI implementation
│   ├── app/
│   │   ├── api/            # API routes (completions, models, admin, metrics)
│   │   ├── core/           # Core functionality (config, database, metrics, middleware)
│   │   ├── models/         # Pydantic data models
│   │   ├── services/       # Business logic (provider selection)
│   │   └── utils/          # Utilities (streaming)
│   ├── tests/              # Unit and integration tests
│   ├── prometheus/         # Prometheus configuration
│   └── Dockerfile
├── rust-server/            # Axum implementation
│   ├── src/
│   │   ├── api/            # HTTP handlers, admin API, models, streaming, proxy
│   │   ├── core/           # Config, database, error handling, metrics, middleware
│   │   ├── services/       # Provider service
│   │   └── transformer/    # Protocol transformation pipeline
│   ├── tests/              # Integration and property tests
│   └── Dockerfile
├── web/
│   ├── react-admin/        # React-based admin UI
│   └── svelte-admin/       # Svelte-based admin UI
├── k8s/dev/                # Kubernetes deployment manifests
├── migrations/             # PostgreSQL database migrations
└── docs/                   # Additional documentation
```

---

## 2. Technology Stack

### Backend Servers

| Component | Python Server | Rust Server |
|-----------|---------------|-------------|
| **Framework** | FastAPI | Axum |
| **Runtime** | Python 3.12+ | Tokio async runtime |
| **HTTP Client** | httpx | reqwest |
| **Database** | SQLAlchemy + asyncpg | sqlx |
| **Metrics** | prometheus-client | prometheus crate |
| **Logging** | loguru | tracing |
| **Rate Limiting** | limits | governor |
| **Token Counting** | tiktoken | tiktoken-rs |
| **Additional** | urllib3, python-dotenv | arc-swap (hot reload), sha2/hex (key hashing), utoipa (OpenAPI docs) |

### Web Admin Interfaces

| Component | React Admin | Svelte Admin |
|-----------|-------------|--------------|
| **Framework** | React 18 | Svelte 5 + SvelteKit |
| **Build Tool** | Vite | Vite |
| **Styling** | Tailwind CSS | Tailwind CSS |
| **Routing** | React Router | SvelteKit routing |
| **Package Manager** | pnpm | pnpm |

### Infrastructure

- **Database**: PostgreSQL (required for dynamic configuration)
- **Migrations**: golang-migrate
- **Monitoring**: Prometheus + Grafana
- **Container**: Docker
- **Orchestration**: Kubernetes

---

## 3. System Architecture

### High-Level Architecture

```mermaid
graph TB
    subgraph "Clients"
        C1["OpenAI SDK"]
        C2["HTTP Client"]
        C3["Admin UI"]
    end
    
    subgraph "LLM Proxy"
        LB["Load Balancer / Ingress"]
        PS["Python Server"]
        RS["Rust Server"]
        DB[("PostgreSQL")]
    end
    
    subgraph "LLM Providers"
        P1["OpenAI"]
        P2["Azure OpenAI"]
        P3["Anthropic"]
        P4["Custom Provider"]
    end
    
    subgraph "Monitoring"
        PROM["Prometheus"]
        GRAF["Grafana"]
    end
    
    C1 --> LB
    C2 --> LB
    C3 --> LB
    LB --> PS
    LB --> RS
    PS --> DB
    RS --> DB
    PS --> P1
    PS --> P2
    PS --> P3
    PS --> P4
    RS --> P1
    RS --> P2
    RS --> P3
    RS --> P4
    PROM --> PS
    PROM --> RS
    GRAF --> PROM
```

### Request Flow

```mermaid
sequenceDiagram
    participant Client
    participant Proxy as LLM Proxy
    participant DB as PostgreSQL
    participant Provider as LLM Provider
    
    Client->>Proxy: POST /v1/chat/completions
    Proxy->>Proxy: Verify Credential Key
    Proxy->>Proxy: Check Rate Limit
    Proxy->>Proxy: Select Provider by Weight
    Proxy->>Proxy: Map Model Name
    Proxy->>Provider: Forward Request
    Provider-->>Proxy: Response or Stream
    Proxy->>Proxy: Record Metrics
    Proxy-->>Client: Response or Stream
```

### Data Flow

1. **Authentication**: Credential key validated against SHA-256 hash stored in database
2. **Rate Limiting**: Per-key rate limits enforced using token bucket algorithm
3. **Provider Selection**: Weighted random selection based on configured weights
4. **Model Mapping**: Request model names mapped to provider-specific names
5. **Request Forwarding**: Request proxied to selected provider with provider's API key
6. **Response Handling**: Streaming (SSE) or non-streaming responses passed through
7. **Metrics Collection**: Request count, latency, token usage recorded for Prometheus

### Transformer Pipeline Architecture (Rust Server)

The Rust server implements a sophisticated protocol transformation pipeline that enables cross-protocol communication between different LLM API formats.

#### 4-Hook Transformation Model

The transformer system uses a 4-hook pipeline for bidirectional protocol conversion:

```mermaid
graph TB
    subgraph "Client Request Flow"
        CR["Client Request<br/>(Any Protocol)"]
        H1["Hook 1: transform_request_out<br/>Client Format → UIF"]
        UIF1["Unified Internal Format"]
        H2["Hook 2: transform_request_in<br/>UIF → Provider Format"]
        PR["Provider Request"]
    end
    
    subgraph "Provider Response Flow"
        PRES["Provider Response"]
        H3["Hook 3: transform_response_in<br/>Provider Format → UIF"]
        UIF2["Unified Internal Format"]
        H4["Hook 4: transform_response_out<br/>UIF → Client Format"]
        CRES["Client Response<br/>(Original Protocol)"]
    end
    
    CR --> H1
    H1 --> UIF1
    UIF1 --> H2
    H2 --> PR
    PR --> PRES
    PRES --> H3
    H3 --> UIF2
    UIF2 --> H4
    H4 --> CRES
```

**Hook Responsibilities:**

1. **`transform_request_out`**: Normalize client request to Unified Internal Format (UIF)
   - Rust: [`Transformer::transform_request_out()`](rust-server/src/transformer/mod.rs#L67)

2. **`transform_request_in`**: Adapt UIF to target provider's protocol
   - Rust: [`Transformer::transform_request_in()`](rust-server/src/transformer/mod.rs#L72)

3. **`transform_response_in`**: Parse provider response into UIF
   - Rust: [`Transformer::transform_response_in()`](rust-server/src/transformer/mod.rs#L77)

4. **`transform_response_out`**: Format UIF for client's expected protocol
   - Rust: [`Transformer::transform_response_out()`](rust-server/src/transformer/mod.rs#L86)

#### Unified Internal Format (UIF)

The UIF serves as the lingua franca for all protocol conversions:

- **Core Types**: [`UnifiedRequest`](rust-server/src/transformer/unified.rs#L381), [`UnifiedResponse`](rust-server/src/transformer/unified.rs#L521), [`UnifiedMessage`](rust-server/src/transformer/unified.rs#L271)
- **Content Blocks**: Text, Image, Tool Use, Tool Result, Thinking, File, Audio, Refusal
- **Streaming**: [`UnifiedStreamChunk`](rust-server/src/transformer/unified.rs#L597) with chunk types (MessageStart, ContentBlockDelta, etc.)
- **Protocol Enum**: [`Protocol`](rust-server/src/transformer/unified.rs#L17) - OpenAI, Anthropic, ResponseApi

#### Protocol Transformers

Each protocol implements the [`Transformer`](rust-server/src/transformer/mod.rs#L60) trait:

| Transformer | Protocol | Endpoint | Implementation |
|-------------|----------|----------|----------------|
| **OpenAI** | OpenAI Chat Completions | `/v1/chat/completions` | [`OpenAITransformer`](rust-server/src/transformer/openai.rs#L207) |
| **Anthropic** | Anthropic Messages API | `/v1/messages` | [`AnthropicTransformer`](rust-server/src/transformer/anthropic.rs#L237) |
| **Response API** | OpenAI Response API | `/v1/responses` | [`ResponseApiTransformer`](rust-server/src/transformer/response_api.rs#L196) |

**Key Features:**

- **Protocol Detection**: [`ProtocolDetector`](rust-server/src/transformer/detector.rs) auto-detects request format
- **Same-Protocol Bypass**: Direct passthrough when client and provider use same protocol
- **Streaming Support**: Chunk-by-chunk transformation for cross-protocol streaming
- **Model Mapping**: Automatic model name translation between protocols

#### V2 API Endpoints

The transformer pipeline powers three V2 endpoints with full cross-protocol support:

| Endpoint | Method | Description | Handler |
|----------|--------|-------------|---------|
| `/v2/chat/completions` | POST | OpenAI-compatible with cross-protocol support | [`chat_completions_v2()`](rust-server/src/api/proxy.rs#L762) |
| `/v2/messages` | POST | Anthropic-compatible with cross-protocol support | [`messages_v2()`](rust-server/src/api/proxy.rs#L771) |
| `/v2/responses` | POST | Response API with cross-protocol support | [`responses_v2()`](rust-server/src/api/proxy.rs#L780) |

**Cross-Protocol Examples:**

```bash
# Send OpenAI request to Anthropic provider
POST /v2/chat/completions
{
  "model": "claude-3-opus",
  "messages": [{"role": "user", "content": "Hello"}]
}
# → Transformed to Anthropic format → Provider → Transformed back to OpenAI format

# Send Anthropic request to OpenAI provider
POST /v2/messages
{
  "model": "gpt-4",
  "max_tokens": 1024,
  "messages": [{"role": "user", "content": "Hello"}]
}
# → Transformed to OpenAI format → Provider → Transformed back to Anthropic format
```

#### Request Flow (V2 Endpoints)

```mermaid
sequenceDiagram
    participant Client
    participant Proxy as V2 Proxy Handler
    participant Detector as Protocol Detector
    participant Pipeline as Transform Pipeline
    participant Provider as LLM Provider
    
    Client->>Proxy: POST /v2/chat/completions
    Proxy->>Proxy: Verify Credential Key
    Proxy->>Proxy: Check Rate Limit
    Proxy->>Detector: Detect Client Protocol
    Detector-->>Proxy: Protocol.OpenAI
    Proxy->>Proxy: Select Provider (Weighted)
    Proxy->>Pipeline: transform_request(payload, ctx)
    Pipeline->>Pipeline: Hook 1: Client → UIF
    Pipeline->>Pipeline: Hook 2: UIF → Provider
    Pipeline-->>Proxy: Transformed Request
    Proxy->>Provider: Forward Request
    Provider-->>Proxy: Response or Stream
    Proxy->>Pipeline: transform_response(response, ctx)
    Pipeline->>Pipeline: Hook 3: Provider → UIF
    Pipeline->>Pipeline: Hook 4: UIF → Client
    Pipeline-->>Proxy: Transformed Response
    Proxy->>Proxy: Record Metrics
    Proxy-->>Client: Response or Stream
```

#### Implementation Components

- **Pipeline Executor**: [`TransformPipeline`](rust-server/src/transformer/mod.rs#L237) - Orchestrates transformation flow
- **Transformer Registry**: [`TransformerRegistry`](rust-server/src/transformer/mod.rs#L127) - Manages protocol transformers
- **Proxy Handler**: [`handle_proxy_request()`](rust-server/src/api/proxy.rs#L200) - Main entry point for V2 endpoints
- **Transform Context**: [`TransformContext`](rust-server/src/transformer/mod.rs#L197) - Carries metadata through pipeline
- **Stream Accumulator**: [`rust-server/src/transformer/stream.rs`](rust-server/src/transformer/stream.rs) - Accumulates streaming chunks

---

## 4. Features

### Core Features

- **OpenAI-Compatible API**: Drop-in replacement for OpenAI API
  - [`/v1/chat/completions`](python-server/app/api/completions.py#L265) - Chat completions
  - [`/v1/completions`](python-server/app/api/completions.py#L275) - Legacy completions
  - [`/v1/models`](python-server/app/api/models.py) - Model listing
  
- **Weighted Load Balancing**: Distribute requests across providers
  - Python: [`ProviderService.get_next_provider()`](python-server/app/services/provider_service.py#L49)
  - Rust: [`ProviderService::get_next_provider()`](rust-server/src/services/provider_service.rs#L97)

- **Streaming Support**: Full SSE streaming with TTFT metrics
  - Python: [`create_streaming_response()`](python-server/app/utils/streaming.py#L361)
  - Rust: [`create_sse_stream()`](rust-server/src/api/streaming.rs#L155)

- **Model Name Mapping**: Translate model names between client and provider
  - Configured per-provider in database
  - Supports wildcard/regex patterns (e.g., `claude-opus-4-5-.*`, `gemini-*`)
  - Exact matches take priority over pattern matches
  - Python: [`match_model_pattern()`](python-server/app/models/provider.py#L40)
  - Rust: [`match_model_pattern()`](rust-server/src/api/models.rs#L78)

### Authentication & Security

- **Credential Authentication**: Bearer token authentication
  - Keys stored as SHA-256 hashes in database
  - Python: [`verify_auth()`](python-server/app/api/dependencies.py#L13)
  - Rust: [`verify_auth()`](rust-server/src/api/handlers.rs#L43)

- **Per-Key Rate Limiting**: Token bucket rate limiting
  - Configurable requests per second and burst size
  - Python: [`RateLimiter`](python-server/app/core/rate_limiter.py#L9)
  - Rust: [`RateLimiter`](rust-server/src/core/rate_limiter.rs#L20)

- **Model Access Control**: Restrict keys to specific models
  - `allowed_models` field in credentials table

### Dynamic Configuration

- **Database-Backed Config**: PostgreSQL storage for runtime configuration
  - Python: [`DynamicConfig`](python-server/app/core/database.py#L327)
  - Rust: [`DynamicConfig`](rust-server/src/core/database.rs#L587)

- **Hot Reload**: Update configuration without server restart
  - Admin API: `POST /admin/v1/config/reload`

- **Version Tracking**: Automatic version increment on config changes
  - Database triggers increment version on provider/credential changes

### Admin API

Full CRUD operations for providers and credentials:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/admin/v1/providers` | GET | List all providers |
| `/admin/v1/providers` | POST | Create provider |
| `/admin/v1/providers/{id}` | GET | Get provider by ID |
| `/admin/v1/providers/{id}` | PUT | Update provider |
| `/admin/v1/providers/{id}` | DELETE | Delete provider |
| `/admin/v1/providers/{id}/health` | POST | Check provider health with concurrent model testing |
| `/admin/v1/credentials` | GET | List all credentials |
| `/admin/v1/credentials` | POST | Create credential |
| `/admin/v1/credentials/{id}` | GET | Get credential by ID |
| `/admin/v1/credentials/{id}` | PUT | Update credential |
| `/admin/v1/credentials/{id}` | DELETE | Delete credential |
| `/admin/v1/config/version` | GET | Get config version |
| `/admin/v1/config/reload` | POST | Reload configuration |
| `/admin/v1/auth/validate` | POST | Validate admin key |

### Monitoring & Observability

- **Prometheus Metrics** at `/metrics`:
  - `llm_proxy_requests_total` - Request count by method, endpoint, model, provider, status
  - `llm_proxy_request_duration_seconds` - Request latency histogram
  - `llm_proxy_active_requests` - Current active requests gauge
  - `llm_proxy_tokens_total` - Token usage by model, provider, type
  - `llm_proxy_provider_health` - Provider health status
  - `llm_proxy_provider_latency_seconds` - Provider latency histogram
  - `llm_proxy_ttft_seconds` - Time to first token for streaming
  - `llm_proxy_tokens_per_second` - Streaming throughput

- **Health Checks**:
  - `/health` - Basic health check
  - `/health/detailed` - Detailed health with provider status

### Langfuse Integration (Optional)

- **LLM Observability**: Optional tracing and analytics
  - Python: [`LangfuseService`](python-server/app/services/langfuse_service.py)
  - Captures provider info, request/response data, token usage
  - TTFT (Time to First Token) tracking for streaming
  - Sampling support for high-traffic scenarios
  - Background batching for minimal latency impact

---

## 5. Database Schema

### Tables

```sql
-- Providers table
CREATE TABLE providers (
    id SERIAL PRIMARY KEY,
    provider_key VARCHAR(255) NOT NULL UNIQUE,
    provider_type VARCHAR(50) NOT NULL,
    api_base VARCHAR(500) NOT NULL,
    api_key VARCHAR(500) NOT NULL,
    model_mapping JSONB NOT NULL DEFAULT '{}',
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Credentials table (API keys for client authentication)
CREATE TABLE credentials (
    id SERIAL PRIMARY KEY,
    credential_key VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    allowed_models TEXT[] NOT NULL DEFAULT '{}',
    rate_limit INTEGER,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Config version table (singleton)
CREATE TABLE config_version (
    id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    version BIGINT NOT NULL DEFAULT 0,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### Entity Relationship

```mermaid
erDiagram
    providers {
        serial id PK
        varchar provider_key UK
        varchar provider_type
        varchar api_base
        varchar api_key
        jsonb model_mapping
        boolean is_enabled
        timestamptz created_at
        timestamptz updated_at
    }
    
    credentials {
        serial id PK
        varchar credential_key UK
        varchar name
        text_array allowed_models
        integer rate_limit
        boolean is_enabled
        timestamptz created_at
        timestamptz updated_at
    }
    
    config_version {
        integer id PK
        bigint version
        timestamptz updated_at
    }
```

---

## 6. Deployment

### Environment Variables

| Variable | Description | Required |
|----------|-------------|----------|
| `DB_URL` | PostgreSQL connection string | Yes |
| `ADMIN_KEY` | Admin API authentication key | Yes |
| `HOST` | Server bind address | No (default: 0.0.0.0) |
| `PORT` | Server port | No (default: 18000) |
| `VERIFY_SSL` | Verify SSL certificates | No (default: true) |
| `REQUEST_TIMEOUT_SECS` | Request timeout | No (default: 300) |
| `TTFT_TIMEOUT_SECS` | Time to first token timeout | No |
| `PROVIDER_SUFFIX` | Optional prefix for model names. When set, model names like `{PROVIDER_SUFFIX}/{model}` are treated as `{model}` | No |

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

### Docker

```bash
# Python Server
cd python-server
docker build -t llm-proxy:latest .
docker run -p 18000:18000 -e DB_URL=... -e ADMIN_KEY=... llm-proxy:latest

# Rust Server
cd rust-server
docker build -t llm-proxy-rust:latest .
docker run -p 18000:18000 -e DB_URL=... -e ADMIN_KEY=... llm-proxy-rust:latest
```

### Kubernetes

Deployment manifests in [`k8s/dev/`](k8s/dev/):

- [`python-server.yaml`](k8s/dev/python-server.yaml) - Python server deployment
- [`rust-server.yaml`](k8s/dev/rust-server.yaml) - Rust server deployment
- [`web-admin.yaml`](k8s/dev/web-admin.yaml) - Admin UI deployment
- [`prometheus.yaml`](k8s/dev/prometheus.yaml) - Prometheus deployment

Deploy with:

```bash
cd k8s/dev
./deploy.sh
```

### Database Migration

```bash
# Install golang-migrate
brew install golang-migrate

# Run migrations
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'
migrate -path migrations -database "$DB_URL" up
```

---

## 7. Development Workflow

### Python Server

```bash
cd python-server

# Install dependencies
uv sync

# Run locally
uv run python main.py

# Run tests
make test

# Run with coverage
make coverage
```

### Rust Server

```bash
cd rust-server

# Build
cargo build --release

# Run locally
CONFIG_PATH=config.yaml cargo run --release

# Run tests
cargo test

# Format and lint
cargo fmt
cargo clippy
```

### Web Admin (React)

```bash
cd web/react-admin

# Install dependencies
pnpm install

# Run development server
pnpm run dev

# Build for production
pnpm run build

# Lint
pnpm run lint
```

### Web Admin (Svelte)

```bash
cd web/svelte-admin

# Install dependencies
pnpm install

# Run development server
pnpm run dev

# Build for production
pnpm run build

# Type check
pnpm run check
```

---

## 8. Code Standards

### Python

- **Type Hints**: Full type annotations with Pydantic models
- **Async/Await**: All I/O operations are async
- **Logging**: loguru for structured logging
- **Testing**: pytest with pytest-asyncio, hypothesis for property tests
- **Coverage**: pytest-cov for coverage reports

### Rust

- **Error Handling**: thiserror for custom errors, anyhow for application errors
- **Async Runtime**: Tokio with full features
- **Logging**: tracing with env-filter
- **Testing**: Built-in test framework, proptest for property tests
- **Documentation**: rustdoc comments on public APIs

### Frontend

- **TypeScript**: Strict mode enabled
- **Linting**: ESLint with Prettier
- **Styling**: Tailwind CSS
- **Package Manager**: pnpm

---

## 9. API Reference

### V1 API Endpoints (Legacy)

#### Chat Completions

```bash
POST /v1/chat/completions
Authorization: Bearer <master_key>
Content-Type: application/json

{
  "model": "gpt-4",
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "stream": false,
  "temperature": 0.7,
  "max_tokens": 1000
}
```

### V2 API Endpoints (Cross-Protocol Support)

The V2 endpoints support cross-protocol transformation, allowing clients to use any protocol format regardless of the provider's native protocol.

#### OpenAI-Compatible Chat Completions

```bash
POST /v2/chat/completions
Authorization: Bearer <master_key>
Content-Type: application/json

{
  "model": "claude-3-opus",  # Can target any provider
  "messages": [
    {"role": "system", "content": "You are a helpful assistant."},
    {"role": "user", "content": "Hello!"}
  ],
  "stream": false,
  "temperature": 0.7,
  "max_tokens": 1000
}
```

#### Anthropic-Compatible Messages

```bash
POST /v2/messages
Authorization: Bearer <master_key>
Content-Type: application/json

{
  "model": "gpt-4",  # Can target any provider
  "max_tokens": 1024,
  "system": "You are a helpful assistant.",
  "messages": [
    {"role": "user", "content": "Hello!"}
  ],
  "temperature": 0.7
}
```

#### Response API

```bash
POST /v2/responses
Authorization: Bearer <master_key>
Content-Type: application/json

{
  "model": "gpt-4",
  "instructions": "You are a helpful assistant.",
  "input": "Hello!",
  "max_output_tokens": 1000,
  "temperature": 0.7
}
```

### Admin API

#### Create Provider

```bash
POST /admin/v1/providers
Authorization: Bearer <admin_key>
Content-Type: application/json

{
  "provider_key": "openai-main",
  "provider_type": "openai",
  "api_base": "https://api.openai.com/v1",
  "api_key": "sk-xxx",
  "model_mapping": {
    "gpt-4": "gpt-4-turbo"
  }
}
```

Response includes auto-generated `id` field.

### Create Credential

```bash
POST /admin/v1/credentials
Authorization: Bearer <admin_key>
Content-Type: application/json

{
  "key": "sk-my-secret-key",
  "name": "Production Key",
  "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
  "rate_limit": 100
}
```

Response includes auto-generated `id` field.

---

## 10. Performance Comparison

| Metric | Python Server | Rust Server |
|--------|---------------|-------------|
| Memory Usage | ~50-100MB | ~10-20MB |
| Startup Time | ~1-2s | ~100ms |
| Throughput | Baseline | 2-3x higher |
| P99 Latency | Baseline | ~50% lower |
| Concurrency | asyncio | Tokio native |

---

## 11. Related Documentation

- [Python Server README](python-server/README.md)
- [Rust Server README](rust-server/README.md)
- [React Admin README](web/react-admin/README.md)
- [Svelte Admin README](web/svelte-admin/README.md)
- [Database Migrations](migrations/)
- [Kubernetes Configs](k8s/dev/)

---

**Last Updated**: 2026-01-22
