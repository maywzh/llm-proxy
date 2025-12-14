#!/bin/bash
# Script to run benchmarks

set -e

echo "================================"
echo "Running Benchmarks"
echo "================================"
echo ""

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Change to rust-server directory
cd "$(dirname "$0")/.."

echo -e "${BLUE}Running provider selection benchmarks...${NC}"
cargo bench --bench provider_selection

echo ""
echo -e "${BLUE}Running request handling benchmarks...${NC}"
cargo bench --bench request_handling

echo ""
echo -e "${GREEN}✓ Benchmarks complete!${NC}"
echo ""
echo "Results available at: target/criterion/report/index.html"
echo ""
echo "Open report with:"
echo "  open target/criterion/report/index.html  # macOS"
echo "  xdg-open target/criterion/report/index.html  # Linux"