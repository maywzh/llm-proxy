#!/bin/bash
# Script to generate test coverage reports using cargo-tarpaulin

set -e

echo "================================"
echo "Generating Test Coverage Report"
echo "================================"
echo ""

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Change to rust-server directory
cd "$(dirname "$0")/.."

# Check if cargo-tarpaulin is installed
if ! command -v cargo-tarpaulin &> /dev/null; then
    echo -e "${YELLOW}cargo-tarpaulin not found. Installing...${NC}"
    cargo install cargo-tarpaulin
fi

echo -e "${BLUE}Running tests with coverage...${NC}"
echo ""

# Run tarpaulin with HTML and XML output
cargo tarpaulin \
    --out Html \
    --out Xml \
    --output-dir coverage \
    --exclude-files 'tests/*' 'benches/*' \
    --timeout 300 \
    --verbose

echo ""
echo -e "${GREEN}✓ Coverage report generated!${NC}"
echo ""
echo "Reports available at:"
echo "  - HTML: coverage/tarpaulin-report.html"
echo "  - XML:  coverage/cobertura.xml"
echo ""
echo "Open HTML report with:"
echo "  open coverage/tarpaulin-report.html  # macOS"
echo "  xdg-open coverage/tarpaulin-report.html  # Linux"