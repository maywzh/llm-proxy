# Docker Usage Guide

## Configuration File Management

The LLM Proxy supports flexible configuration file management:

### 1. Using Default Config (Built into Image)

If you include `config.yaml` in the build context, it will be copied into the image as the default configuration:

```bash
docker build -t llm-proxy .
docker run -p 18000:18000 llm-proxy
```

### 2. Using External Config File (Recommended)

Mount an external configuration file to override the default:

```bash
docker run -p 18000:18000 \
  -v /path/to/your/config.yaml:/app/config.yaml:ro \
  llm-proxy
```

### 3. Using Custom Config Path

Specify a different config file path using environment variable:

```bash
docker run -p 18000:18000 \
  -v /path/to/your/custom-config.yaml:/app/custom-config.yaml:ro \
  -e CONFIG_PATH=/app/custom-config.yaml \
  llm-proxy
```

Or using command line argument:

```bash
docker run -p 18000:18000 \
  -v /path/to/your/custom-config.yaml:/app/custom-config.yaml:ro \
  llm-proxy \
  uv run proxy.py --config=/app/custom-config.yaml
```

## Docker Compose Usage

### Basic Usage

```bash
docker-compose up -d
```

This uses the default `config.yaml` mounted from the current directory.

### Custom Config File

Edit `docker-compose.yml` to mount your custom config:

```yaml
services:
  llm-proxy:
    volumes:
      - /path/to/your/config.yaml:/app/config.yaml:ro
    environment:
      - CONFIG_PATH=/app/config.yaml
```

### Multiple Configurations

You can run multiple instances with different configs:

```yaml
services:
  llm-proxy-prod:
    image: llm-proxy
    ports:
      - "18000:18000"
    volumes:
      - ./config-prod.yaml:/app/config.yaml:ro
    environment:
      - CONFIG_PATH=/app/config.yaml

  llm-proxy-dev:
    image: llm-proxy
    ports:
      - "18001:18000"
    volumes:
      - ./config-dev.yaml:/app/config.yaml:ro
    environment:
      - CONFIG_PATH=/app/config.yaml
```

## Certificate File

Similarly, you can mount an external certificate file:

```bash
docker run -p 18000:18000 \
  -v /path/to/your/cacerts.pem:/app/cacerts.pem:ro \
  llm-proxy
```

## Build-time vs Runtime Configuration

### Build-time (Baked into Image)
- Config and certificates are copied during build if present
- Good for immutable deployments
- Requires rebuild to change config

```bash
# Build with default config
docker build -t llm-proxy .
```

### Runtime (External Mount)
- Config and certificates mounted at runtime
- Easy to update without rebuilding
- Better for development and testing

```bash
# Run with external config
docker run -v ./config.yaml:/app/config.yaml:ro llm-proxy
```

## Environment Variables

- `CONFIG_PATH`: Path to configuration file (default: `/app/config.yaml`)
- `HOST`: Server host address (default: `0.0.0.0`)
- `PORT`: Server port number (default: `18000`)
- `PYTHONUNBUFFERED`: Set to `1` for real-time log output

### Priority Order

For host and port configuration, the priority is:
1. Environment variables (`HOST`, `PORT`)
2. Config file (`server.host`, `server.port`)
3. Default values (`0.0.0.0`, `18000`)

Example:
```bash
docker run -p 8080:8080 \
  -e HOST=0.0.0.0 \
  -e PORT=8080 \
  llm-proxy
```

## Health Check

Check if the service is running:

```bash
curl http://localhost:18000/health
```

Detailed health check (tests all providers):

```bash
curl http://localhost:18000/health/detailed