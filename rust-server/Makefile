# Makefile for Rust LLM Proxy

.PHONY: help build run test clean docker-build docker-run fmt clippy check

help:
	@echo "Available targets:"
	@echo "  build        - Build release binary"
	@echo "  run          - Run the application"
	@echo "  test         - Run tests"
	@echo "  check        - Check code without building"
	@echo "  fmt          - Format code"
	@echo "  clippy       - Run clippy linter"
	@echo "  clean        - Clean build artifacts"
	@echo "  docker-build - Build Docker image"
	@echo "  docker-run   - Run Docker container"

build:
	cargo build --release

run:
	cargo run --release

test:
	cargo test

check:
	cargo check

fmt:
	cargo fmt

clippy:
	cargo clippy -- -D warnings

clean:
	cargo clean

docker-build:
	docker build -t llm-proxy-rust:latest .

docker-run:
	docker run -p 18000:18000 \
		-v $(PWD)/config.yaml:/app/config.yaml \
		-e CONFIG_PATH=/app/config.yaml \
		llm-proxy-rust:latest

# Development targets
dev:
	RUST_LOG=debug cargo run

watch:
	cargo watch -x run

# Release build with optimizations
release:
	cargo build --release
	strip target/release/llm-proxy-rust