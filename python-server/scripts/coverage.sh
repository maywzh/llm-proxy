#!/bin/bash
# Generate detailed coverage report for the LLM proxy

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${GREEN}Generating Coverage Report${NC}"
echo "================================"

# Check if uv is installed
if ! command -v uv &> /dev/null; then
    echo -e "${RED}Error: uv is not installed${NC}"
    echo "Please install uv: curl -LsSf https://astral.sh/uv/install.sh | sh"
    exit 1
fi

# Install test dependencies
echo -e "${YELLOW}Installing dependencies...${NC}"
uv pip install -e ".[test]"

# Clean previous coverage data
echo -e "${YELLOW}Cleaning previous coverage data...${NC}"
rm -rf .coverage htmlcov coverage.xml

# Run tests with coverage
echo ""
echo -e "${YELLOW}Running tests with coverage...${NC}"
uv run pytest tests/ \
    --cov=app \
    --cov-report=term-missing \
    --cov-report=html \
    --cov-report=xml \
    --cov-report=json \
    -v

# Display coverage summary
echo ""
echo -e "${GREEN}Coverage Summary:${NC}"
uv run coverage report --skip-covered

# Check coverage threshold
echo ""
echo -e "${YELLOW}Checking coverage threshold (80%)...${NC}"
COVERAGE=$(uv run coverage report | grep TOTAL | awk '{print $4}' | sed 's/%//')

if (( $(echo "$COVERAGE >= 80" | bc -l) )); then
    echo -e "${GREEN}✓ Coverage threshold met: ${COVERAGE}%${NC}"
else
    echo -e "${RED}✗ Coverage below threshold: ${COVERAGE}% (minimum: 80%)${NC}"
    exit 1
fi

# Generate HTML report
echo ""
echo -e "${BLUE}HTML coverage report generated at: htmlcov/index.html${NC}"
echo -e "${BLUE}XML coverage report generated at: coverage.xml${NC}"
echo -e "${BLUE}JSON coverage report generated at: coverage.json${NC}"

# Open HTML report if on macOS
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo ""
    echo -e "${YELLOW}Opening HTML report in browser...${NC}"
    open htmlcov/index.html
fi

echo ""
echo -e "${GREEN}Coverage report generation completed!${NC}"