# LLM Proxy Helm Chart

A Helm chart for deploying [LLM Proxy](https://github.com/maywzh/llm-proxy) — a high-performance, OpenAI-compatible LLM API proxy with weighted load balancing, streaming support, and built-in observability.

## Prerequisites

- Kubernetes 1.24+
- Helm 3.x
- PostgreSQL database (user-managed, e.g. RDS, Cloud SQL, or self-hosted)

## Quick Start

```bash
helm install llm-proxy ./charts/llm-proxy \
  --namespace llm-proxy --create-namespace \
  --set database.url="postgresql://user:pass@your-host:5432/llm_proxy" \
  --set adminKey.value="your-admin-key"
```

## Architecture

```
┌───────────────────────────────────────────────┐
│               Kubernetes Cluster              │
│                                               │
│  ┌─────────┐  ┌──────────┐  ┌─────────────┐  │
│  │  Ingress │→│LLM Proxy │→│  PostgreSQL  │  │
│  └─────────┘  └──────────┘  └─────────────┘  │
│                    │          (user-managed)   │
│        ┌───────────┼───────────┐              │
│        ▼           ▼           ▼              │
│  ┌──────────┐ ┌──────────┐ ┌────────┐        │
│  │ Admin UI │ │Prometheus│ │Grafana │        │
│  └──────────┘ └──────────┘ └────────┘        │
└───────────────────────────────────────────────┘
```

## Database Configuration

This chart requires an **external PostgreSQL** database. Two ways to provide credentials:

### Inline URL (simple / dev)

```yaml
database:
  url: "postgresql://user:pass@your-host:5432/llm_proxy?sslmode=require"
```

### Kubernetes Secret (recommended for production)

```bash
kubectl create secret generic llm-proxy-db \
  --from-literal=DB_URL="postgresql://user:pass@your-host:5432/llm_proxy?sslmode=require"
```

```yaml
database:
  existingSecret: "llm-proxy-db"
  existingSecretKey: "DB_URL"
```

## Deployment Presets

The `examples/` directory contains ready-to-use value files:

| Preset | File | Credentials | Monitoring | Use Case |
|--------|------|-------------|------------|----------|
| **Minimal** | `values-minimal.yaml` | Inline | No | Quick start / dev |
| **Production** | `values-production.yaml` | K8s Secrets | Yes + persistence + HPA + TLS | Production |

```bash
# Minimal
helm install llm-proxy ./charts/llm-proxy \
  -f charts/llm-proxy/examples/values-minimal.yaml

# Production
helm install llm-proxy ./charts/llm-proxy \
  -f charts/llm-proxy/examples/values-production.yaml

# Override values on top of a preset
helm install llm-proxy ./charts/llm-proxy \
  -f charts/llm-proxy/examples/values-production.yaml \
  --set replicaCount=3
```

## Configuration

### Server Type

Choose between Python (FastAPI) or Rust (Axum) implementation:

```yaml
serverType: python  # or "rust"
```

### Required Configuration

| Parameter | Description |
|-----------|-------------|
| `database.url` or `database.existingSecret` | PostgreSQL connection (one is required) |
| `adminKey.value` or `adminKey.existingSecret` | Admin API authentication key |

### Optional Components

| Component | Enable | Description |
|-----------|--------|-------------|
| Admin UI | `adminUI.enabled: true` | Web-based management dashboard |
| Prometheus | `prometheus.enabled: true` | Metrics collection |
| Grafana | `grafana.enabled: true` | Metrics visualization (auto-provisions Prometheus datasource) |
| HPA | `autoscaling.enabled: true` | Horizontal Pod Autoscaler |

### Key Values

| Parameter | Default | Description |
|-----------|---------|-------------|
| `replicaCount` | `1` | Number of proxy replicas |
| `serverType` | `python` | Server implementation (`python` or `rust`) |
| `service.port` | `18000` | Service port |
| `ingress.enabled` | `false` | Enable ingress |
| `env.VERIFY_SSL` | - | SSL verification toggle |
| `env.REQUEST_TIMEOUT_SECS` | - | Request timeout in seconds |
| `env.LOG_LEVEL` | - | Log level (debug/info/warn/error) |

See [values.yaml](values.yaml) for the full list of configurable parameters.

## Upgrading

```bash
helm upgrade llm-proxy ./charts/llm-proxy -f my-values.yaml
```

## Uninstalling

```bash
helm uninstall llm-proxy -n llm-proxy
```
