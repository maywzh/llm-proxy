#!/bin/bash
# Quick start script for development

set -e

echo "Starting LLM API Proxy..."
echo ""

# Check if .env exists
if [ ! -f ".env" ]; then
    echo "Warning: .env file not found"
    echo "Please copy .env.example to .env and configure it"
fi

# Check required environment variables
if [ -z "$DB_URL" ]; then
    echo "Error: DB_URL environment variable is required"
    echo "Set it in .env file or export it directly"
    exit 1
fi

# Run with uv
exec uv run python main.py