#!/bin/bash
# Script to run all tests with various configurations

set -e

echo "================================"
echo "Running Rust Server Test Suite"
echo "================================"
echo ""

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Change to rust-server directory
cd "$(dirname "$0")/.."

echo -e "${BLUE}1. Running unit tests...${NC}"
cargo test --lib

echo ""
echo -e "${BLUE}2. Running integration tests...${NC}"
cargo test --test integration_test

echo ""
echo -e "${BLUE}3. Running property-based tests...${NC}"
cargo test --test property_tests

echo ""
echo -e "${BLUE}4. Running mock tests...${NC}"
cargo test --test mock_tests

echo ""
echo -e "${BLUE}5. Running doc tests...${NC}"
cargo test --doc

echo ""
echo -e "${GREEN}✓ All tests passed!${NC}"