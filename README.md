# tvs-llm-proxy

High-performance, OpenAI-compatible LLM API proxy with weighted load balancing, streaming support, and built-in observability. The repository contains two first-class implementations:

- Python (FastAPI) service in [python-server/](python-server/)
- Rust (Axum) service in [rust-server/](rust-server/)

Both variants expose the same endpoints (`/v1/chat/completions`, `/v1/completions`, `/v1/models`, `/health`, `/health/detailed`, `/metrics`) and share the same configuration concepts (providers, weighted routing, model mapping, master API key, SSL verification toggle).

## Features

- Weighted round-robin across multiple providers with model name mapping
- Streaming and non-streaming responses, OpenAI-compatible request/response schema
- Master API key enforcement, per-provider API keys, optional SSL verification
- Prometheus metrics and prebuilt Grafana dashboards; health checks for providers
- Docker/Docker Compose support (includes Prometheus + Grafana stack)
- Kubernetes manifests for dev testing under [k8s/dev/](k8s/dev/)

## Repo Layout

- [python-server/](python-server/) — FastAPI implementation, Makefile/scripts, monitoring assets
- [rust-server/](rust-server/) — Axum implementation with parity features
- [k8s/dev/](k8s/dev/) — Sample deployment YAML and helper script
- Monitoring assets: [python-server/prometheus/](python-server/prometheus/) and [python-server/grafana/](python-server/grafana/)

## Quick Start (Python)

1) Install dependencies (uses uv):

```bash
cd python-server
uv sync
```

2) Configure providers:

```bash
cp config.example.yaml config.yaml
cp .env.example .env  # if present
# edit config.yaml to set providers[].api_base/api_key, weights, model_mapping
```

3) Run locally:

```bash
uv run python main.py --config config.yaml
# or
./run.sh
```

4) Run with Docker Compose (includes Prometheus + Grafana):

```bash
docker-compose up -d
# LLM Proxy http://localhost:18000, Prometheus http://localhost:9090, Grafana http://localhost:3000
```

5) Tests and coverage:

```bash
cd python-server
make test      # all tests
make coverage  # htmlcov report
```

More details: [python-server/README.md](python-server/README.md).

## Quick Start (Rust)

```bash
cd rust-server
cargo build --release
CONFIG_PATH=config.yaml cargo run --release

# Docker
docker build -t llm-proxy-rust:latest .
docker run -p 18000:18000 -v $(pwd)/config.yaml:/app/config.yaml -e CONFIG_PATH=/app/config.yaml llm-proxy-rust:latest

# Tests
cargo test
```

More details: [rust-server/README.md](rust-server/README.md).

## Configuration Snapshot

Key fields used by both implementations (see examples in each subproject):

```yaml
providers:
  - name: Provider-1
    api_base: "${API_BASE_URL}"
    api_key: "${API_KEY_1}"
    weight: 1
    model_mapping:
      "claude-4.5-sonnet": "actual-provider-model"

server:
  host: 0.0.0.0
  port: 18000
  master_api_key: "sk-your-master-key"

verify_ssl: false
```

Priority: environment variables > .env > YAML values. Use `MASTER_API_KEY` to protect endpoints. Set `verify_ssl=false` if calling providers with custom cert chains (see `cacerts.pem`).

## Monitoring

- Metrics exposed at `/metrics`; collected by Prometheus via [python-server/prometheus/prometheus.yml](python-server/prometheus/prometheus.yml).
- Preconfigured Grafana dashboards under [python-server/grafana/dashboards/](python-server/grafana/dashboards/), auto-provisioned via [python-server/grafana/provisioning/](python-server/grafana/provisioning/).
- Common panels: request rate, P95/P99 latency, token usage, status code distribution, provider health/latency, active requests.

## Kubernetes (dev sample)

- [k8s/dev/k8s.yaml](k8s/dev/k8s.yaml): deployment/service example for the proxy.
- [k8s/dev/deploy.sh](k8s/dev/deploy.sh): helper script to apply dev manifests.

## Related Docs

- Python impl: [python-server/README.md](python-server/README.md), [python-server/REFACTORING.md](python-server/REFACTORING.md)
- Rust impl: [rust-server/README.md](rust-server/README.md), [rust-server/CONFIGURATION.md](rust-server/CONFIGURATION.md)

## License

MIT (see project files).
