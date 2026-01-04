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
│   │   ├── api/            # HTTP handlers, admin API, models, streaming
│   │   ├── core/           # Config, database, error handling, metrics, middleware
│   │   └── services/       # Provider service
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
    Proxy->>Proxy: Verify Master Key
    Proxy->>Proxy: Check Rate Limit
    Proxy->>Proxy: Select Provider by Weight
    Proxy->>Proxy: Map Model Name
    Proxy->>Provider: Forward Request
    Provider-->>Proxy: Response or Stream
    Proxy->>Proxy: Record Metrics
    Proxy-->>Client: Response or Stream
```

### Data Flow

1. **Authentication**: Master key validated against SHA-256 hash stored in database
2. **Rate Limiting**: Per-key rate limits enforced using token bucket algorithm
3. **Provider Selection**: Weighted random selection based on configured weights
4. **Model Mapping**: Request model names mapped to provider-specific names
5. **Request Forwarding**: Request proxied to selected provider with provider's API key
6. **Response Handling**: Streaming (SSE) or non-streaming responses passed through
7. **Metrics Collection**: Request count, latency, token usage recorded for Prometheus

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

### Authentication & Security

- **Master Key Authentication**: Bearer token authentication
  - Keys stored as SHA-256 hashes in database
  - Python: [`verify_auth()`](python-server/app/api/dependencies.py#L13)
  - Rust: [`verify_auth()`](rust-server/src/api/handlers.rs#L43)

- **Per-Key Rate Limiting**: Token bucket rate limiting
  - Configurable requests per second and burst size
  - Python: [`RateLimiter`](python-server/app/core/rate_limiter.py#L9)
  - Rust: [`RateLimiter`](rust-server/src/core/rate_limiter.rs#L20)

- **Model Access Control**: Restrict keys to specific models
  - `allowed_models` field in master_keys table

### Dynamic Configuration

- **Database-Backed Config**: PostgreSQL storage for runtime configuration
  - Python: [`DynamicConfig`](python-server/app/core/database.py#L327)
  - Rust: [`DynamicConfig`](rust-server/src/core/database.rs#L587)

- **Hot Reload**: Update configuration without server restart
  - Admin API: `POST /admin/v1/config/reload`

- **Version Tracking**: Automatic version increment on config changes
  - Database triggers increment version on provider/key changes

### Admin API

Full CRUD operations for providers and master keys:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/admin/v1/providers` | GET | List all providers |
| `/admin/v1/providers` | POST | Create provider |
| `/admin/v1/providers/{id}` | GET | Get provider |
| `/admin/v1/providers/{id}` | PUT | Update provider |
| `/admin/v1/providers/{id}` | DELETE | Delete provider |
| `/admin/v1/master-keys` | GET | List all master keys |
| `/admin/v1/master-keys` | POST | Create master key |
| `/admin/v1/master-keys/{id}` | GET | Get master key |
| `/admin/v1/master-keys/{id}` | PUT | Update master key |
| `/admin/v1/master-keys/{id}` | DELETE | Delete master key |
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

---

## 5. Database Schema

### Tables

```sql
-- Providers table
CREATE TABLE providers (
    id VARCHAR(255) PRIMARY KEY,
    provider_type VARCHAR(50) NOT NULL,
    api_base VARCHAR(500) NOT NULL,
    api_key VARCHAR(500) NOT NULL,
    model_mapping JSONB NOT NULL DEFAULT '{}',
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Master keys table
CREATE TABLE master_keys (
    id VARCHAR(255) PRIMARY KEY,
    key_hash VARCHAR(255) NOT NULL,
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
        varchar id PK
        varchar provider_type
        varchar api_base
        varchar api_key
        jsonb model_mapping
        boolean is_enabled
        timestamptz created_at
        timestamptz updated_at
    }
    
    master_keys {
        varchar id PK
        varchar key_hash
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
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy?sslmode=disable'
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

### Chat Completions

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

### Create Provider

```bash
POST /admin/v1/providers
Authorization: Bearer <admin_key>
Content-Type: application/json

{
  "id": "openai-main",
  "provider_type": "openai",
  "api_base": "https://api.openai.com/v1",
  "api_key": "sk-xxx",
  "model_mapping": {
    "gpt-4": "gpt-4-turbo"
  },
  "is_enabled": true
}
```

### Create Master Key

```bash
POST /admin/v1/master-keys
Authorization: Bearer <admin_key>
Content-Type: application/json

{
  "id": "key-1",
  "key": "sk-my-secret-key",
  "name": "Production Key",
  "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
  "rate_limit": 100,
  "is_enabled": true
}
```

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
