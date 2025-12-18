#!/bin/bash
# Run all tests for the LLM proxy

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}Running LLM Proxy Tests${NC}"
echo "================================"

# Check if uv is installed
if ! command -v uv &> /dev/null; then
    echo -e "${RED}Error: uv is not installed${NC}"
    echo "Please install uv: curl -LsSf https://astral.sh/uv/install.sh | sh"
    exit 1
fi

# Install test dependencies if needed
echo -e "${YELLOW}Installing dependencies...${NC}"
uv pip install -e ".[test]"

# Run tests with different markers
echo ""
echo -e "${YELLOW}Running unit tests...${NC}"
uv run pytest tests/ -m unit -v

echo ""
echo -e "${YELLOW}Running integration tests...${NC}"
uv run pytest tests/ -m integration -v

echo ""
echo -e "${YELLOW}Running property-based tests...${NC}"
uv run pytest tests/ -m property -v

echo ""
echo -e "${YELLOW}Running all tests with coverage...${NC}"
uv run pytest tests/ -v --cov=app --cov-report=term-missing

echo ""
echo -e "${GREEN}All tests completed successfully!${NC}"