#!/bin/bash
# Quick start script for development

set -e

echo "Starting LLM API Proxy..."
echo ""

# Check if config exists
if [ ! -f "config.yaml" ]; then
    echo "Error: config.yaml not found"
    echo "Please copy config.example.yaml to config.yaml and configure it"
    exit 1
fi

# Check if .env exists
if [ ! -f ".env" ]; then
    echo "Warning: .env file not found"
    echo "Using environment variables from config.yaml only"
fi

# Run with uv
exec uv run python main.py --config config.yaml