# Multi-stage build for Rust LLM Proxy

# Build stage - Use standard Rust Alpine image with build tools
FROM rust:1.83-alpine AS builder

# Install build dependencies
RUN sed -i 's/dl-cdn.alpinelinux.org/mirrors.tesla.cn/g' /etc/apk/repositories && \
    apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig


WORKDIR /app

# Set up Cargo config to use tesla-mirrors as default registry FIRST
RUN mkdir -p .cargo && \
    printf '[registry]\ndefault = "tesla-mirrors"\n\n[registries.tesla-mirrors]\nindex = "sparse+https://mirrors.tesla.cn/cargo/index/"\n' > .cargo/config.toml

# Copy manifests
COPY Cargo.toml Cargo.lock* ./

# Copy benches directory (needed by Cargo.toml)
COPY benches ./benches

# Copy source code
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage - Use Chainguard Rust image
FROM atm-base.artifactory.teslamotors.com/chainguard/rust:latest

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/llm-proxy-rust /usr/local/bin/llm-proxy-rust

# Copy default config if exists
COPY config.yaml* ./

# Expose port
EXPOSE 18000

# Set default config path
ENV CONFIG_PATH=/app/config.yaml

# Ensure the container starts by executing the compiled binary directly
ENTRYPOINT ["/usr/local/bin/llm-proxy-rust"]
CMD []
