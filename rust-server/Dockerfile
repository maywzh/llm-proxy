# Multi-stage build for Rust LLM Proxy

# Build stage
FROM atm-base.artifactory.teslamotors.com/chainguard/rust:latest AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock* ./

# Copy source code
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM cndevops-base-docker-local.arf.tesla.cn/base/alpine:3.17

# Install runtime dependencies
RUN apk add --no-cache ca-certificates libgcc

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/llm-proxy-rust /usr/local/bin/llm-proxy-rust

# Copy default config if exists
COPY config.yaml* ./

# Expose port
EXPOSE 18000

# Set default config path
ENV CONFIG_PATH=/app/config.yaml

# Run the application
CMD ["llm-proxy-rust"]