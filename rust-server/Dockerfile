# syntax=docker/dockerfile:1.4
# Multi-stage build for Rust LLM Proxy with cargo-chef for dependency caching

# =============================================================================
# Stage 1: Chef base - Install cargo-chef and configure mirrors
# =============================================================================
FROM atm-base.artifactory.teslamotors.com/rust:1.85-bookworm AS chef

# Copy and use China mirror sources
COPY debian-cn-sources.list /etc/apt/sources.list.d/debian.sources
RUN rm -f /etc/apt/sources.list.d/debian.sources && \
    echo "deb https://mirrors.ustc.edu.cn/debian/ bookworm main contrib non-free non-free-firmware" > /etc/apt/sources.list && \
    echo "deb https://mirrors.ustc.edu.cn/debian/ bookworm-updates main contrib non-free non-free-firmware" >> /etc/apt/sources.list && \
    echo "deb https://mirrors.ustc.edu.cn/debian-security/ bookworm-security main contrib non-free non-free-firmware" >> /etc/apt/sources.list

# Install build dependencies
RUN apt-get update && \
    apt-get install -y pkg-config libssl-dev && \
    rm -rf /var/lib/apt/lists/*

# Set up Cargo config to use tesla-mirrors as crates.io replacement
RUN mkdir -p ${CARGO_HOME:-/usr/local/cargo} && \
    printf '[source.crates-io]\nreplace-with = "tesla-mirrors"\n\n[source.tesla-mirrors]\nregistry = "sparse+https://mirrors.tesla.cn/cargo/index/"\n' > ${CARGO_HOME:-/usr/local/cargo}/config.toml

# Install cargo-chef for dependency caching
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo install cargo-chef --locked

WORKDIR /app

# =============================================================================
# Stage 2: Planner - Generate dependency recipe
# =============================================================================
FROM chef AS planner

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches

# Generate recipe.json containing dependency information
RUN cargo chef prepare --recipe-path recipe.json

# =============================================================================
# Stage 3: Builder - Build dependencies then source code
# =============================================================================
FROM chef AS builder

# Copy recipe from planner
COPY --from=planner /app/recipe.json recipe.json

# Build dependencies only (this layer is cached if dependencies don't change)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo chef cook --release --recipe-path recipe.json

# Copy source code
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches

# Build the application (only recompiles changed source files)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp /app/target/release/llm-proxy-rust /app/llm-proxy-rust

# =============================================================================
# Stage 4: Runtime - Minimal image for running the binary
# =============================================================================
FROM atm-base.artifactory.teslamotors.com/rust:1.85-bookworm AS runtime

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/llm-proxy-rust /usr/local/bin/llm-proxy-rust

# Expose port
EXPOSE 18000

# Ensure the container starts by executing the compiled binary directly
ENTRYPOINT ["/usr/local/bin/llm-proxy-rust"]
CMD []
