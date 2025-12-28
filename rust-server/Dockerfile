# Multi-stage build for Rust LLM Proxy

# Build stage - Use Tesla Rust Bookworm image with build tools
FROM atm-base.artifactory.teslamotors.com/rust:1.85-bookworm AS builder

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



# Set up Cargo config to use tesla-mirrors as default registry FIRST (before WORKDIR)
RUN mkdir -p ${CARGO_HOME:-/usr/local/cargo} && \
    printf '[registry]\ndefault = "tesla-mirrors"\n\n[registries.tesla-mirrors]\nindex = "sparse+https://mirrors.tesla.cn/cargo/index/"\n' > ${CARGO_HOME:-/usr/local/cargo}/config.toml && \
    cat ${CARGO_HOME:-/usr/local/cargo}/config.toml

WORKDIR /app

# Also create project-level cargo config as backup
RUN mkdir -p .cargo && \
    printf '[registry]\ndefault = "tesla-mirrors"\n\n[registries.tesla-mirrors]\nindex = "sparse+https://mirrors.tesla.cn/cargo/index/"\n' > .cargo/config.toml

# Copy manifests (Cargo.lock will be regenerated with mirror source)
COPY Cargo.toml ./

# Copy benches directory (needed by Cargo.toml)
COPY benches ./benches

# Copy source code
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage - Use Tesla Rust Bookworm image
FROM atm-base.artifactory.teslamotors.com/rust:1.85-bookworm

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
