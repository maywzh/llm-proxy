#!/usr/bin/env bash

# Authentication Verification Script
# Tests authentication with master_keys for both Python and Rust servers
#
# Usage: ./verify_auth.sh [SERVER_TYPE]
#   SERVER_TYPE: "python" or "rust" (default: both)
#
# Example: ./verify_auth.sh python

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Enable echo -e for all shells
if [ "$(echo -e)" = "-e" ]; then
    ECHO="echo"
else
    ECHO="echo -e"
fi

# Parse arguments
SERVER_TYPE="${1:-both}"

# Configuration
PYTHON_PORT="${PYTHON_PORT:-18000}"
RUST_PORT="${RUST_PORT:-18001}"

# Test keys
VALID_KEY="sk-test-key-1"
INVALID_KEY="sk-invalid-key"

$ECHO "${BLUE}=== Authentication Verification Script ===${NC}"
$ECHO "Testing authentication with master_keys configuration"
echo ""

# Function to test a server
test_server() {
    local server_name=$1
    local port=$2
    local base_url="http://localhost:${port}"
    
    $ECHO "${BLUE}Testing ${server_name} server on port ${port}${NC}"
    echo ""
    
    # Check if server is running
    $ECHO "${YELLOW}1. Checking if server is running...${NC}"
    if ! curl -s -f "${base_url}/health" > /dev/null 2>&1; then
        $ECHO "${RED}✗ Server is not running at ${base_url}${NC}"
        $ECHO "  Please start the ${server_name} server first"
        return 1
    fi
    $ECHO "${GREEN}✓ Server is running${NC}"
    echo ""
    
    # Test 1: Request without authentication (should fail if master_keys configured)
    $ECHO "${YELLOW}2. Testing request without authentication...${NC}"
    response=$(curl -s -w "\n%{http_code}" -X GET "${base_url}/v1/models" 2>&1)
    http_code=$(echo "$response" | tail -n 1)
    
    if [ "$http_code" = "401" ]; then
        $ECHO "${GREEN}✓ Correctly rejected (401 Unauthorized)${NC}"
    elif [ "$http_code" = "200" ]; then
        $ECHO "${YELLOW}⚠ Allowed without authentication (no master_keys configured)${NC}"
    else
        $ECHO "${RED}✗ Unexpected response: ${http_code}${NC}"
    fi
    echo ""
    
    # Test 2: Request with invalid key (should fail if master_keys configured)
    $ECHO "${YELLOW}3. Testing request with invalid key...${NC}"
    response=$(curl -s -w "\n%{http_code}" -X GET "${base_url}/v1/models" \
        -H "Authorization: Bearer ${INVALID_KEY}" 2>&1)
    http_code=$(echo "$response" | tail -n 1)
    
    if [ "$http_code" = "401" ]; then
        $ECHO "${GREEN}✓ Correctly rejected (401 Unauthorized)${NC}"
    elif [ "$http_code" = "200" ]; then
        $ECHO "${YELLOW}⚠ Allowed with invalid key (no master_keys configured)${NC}"
    else
        $ECHO "${RED}✗ Unexpected response: ${http_code}${NC}"
    fi
    echo ""
    
    # Test 3: Request with valid key (should succeed)
    $ECHO "${YELLOW}4. Testing request with valid key...${NC}"
    response=$(curl -s -w "\n%{http_code}" -X GET "${base_url}/v1/models" \
        -H "Authorization: Bearer ${VALID_KEY}" 2>&1)
    http_code=$(echo "$response" | tail -n 1)
    body=$(echo "$response" | sed '$d')
    
    if [ "$http_code" = "200" ]; then
        $ECHO "${GREEN}✓ Successfully authenticated (200 OK)${NC}"
        # Check if response contains models
        if echo "$body" | grep -q '"object":"list"'; then
            $ECHO "${GREEN}✓ Response contains model list${NC}"
        fi
    else
        $ECHO "${RED}✗ Authentication failed: ${http_code}${NC}"
        echo "$body"
    fi
    echo ""
    
    # Test 4: Test rate limiting with valid key
    $ECHO "${YELLOW}5. Testing rate limiting with valid key...${NC}"
    success_count=0
    rate_limited_count=0
    
    for i in {1..5}; do
        response=$(curl -s -w "\n%{http_code}" -X POST "${base_url}/v1/chat/completions" \
            -H "Authorization: Bearer ${VALID_KEY}" \
            -H "Content-Type: application/json" \
            -d '{
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "test"}],
                "max_tokens": 10
            }' 2>&1)
        http_code=$(echo "$response" | tail -n 1)
        
        if [ "$http_code" = "200" ]; then
            ((success_count++))
        elif [ "$http_code" = "429" ]; then
            ((rate_limited_count++))
        fi
    done
    
    $ECHO "  Results: ${GREEN}${success_count} succeeded${NC}, ${YELLOW}${rate_limited_count} rate limited${NC}"
    
    if [ "$rate_limited_count" -gt 0 ]; then
        $ECHO "${GREEN}✓ Rate limiting is working${NC}"
    else
        $ECHO "${YELLOW}⚠ No rate limiting detected (may have high limits)${NC}"
    fi
    echo ""
    
    # Test 5: Test malformed authorization header
    $ECHO "${YELLOW}6. Testing malformed authorization header...${NC}"
    response=$(curl -s -w "\n%{http_code}" -X GET "${base_url}/v1/models" \
        -H "Authorization: InvalidFormat" 2>&1)
    http_code=$(echo "$response" | tail -n 1)
    
    if [ "$http_code" = "401" ]; then
        $ECHO "${GREEN}✓ Correctly rejected malformed header (401 Unauthorized)${NC}"
    elif [ "$http_code" = "200" ]; then
        $ECHO "${YELLOW}⚠ Allowed with malformed header (no master_keys configured)${NC}"
    else
        $ECHO "${RED}✗ Unexpected response: ${http_code}${NC}"
    fi
    echo ""
    
    $ECHO "${GREEN}${server_name} server tests completed!${NC}"
    echo ""
    echo "=================================================="
    echo ""
}

# Run tests based on server type
case "$SERVER_TYPE" in
    python)
        test_server "Python" "$PYTHON_PORT"
        ;;
    rust)
        test_server "Rust" "$RUST_PORT"
        ;;
    both)
        test_server "Python" "$PYTHON_PORT"
        test_server "Rust" "$RUST_PORT"
        ;;
    *)
        $ECHO "${RED}Invalid server type: ${SERVER_TYPE}${NC}"
        $ECHO "Usage: $0 [python|rust|both]"
        exit 1
        ;;
esac

$ECHO "${BLUE}=== Summary ===${NC}"
$ECHO "Authentication verification completed!"
$ECHO ""
$ECHO "Expected behavior with master_keys configured:"
$ECHO "  ${GREEN}✓${NC} Requests without keys should be rejected (401)"
$ECHO "  ${GREEN}✓${NC} Requests with invalid keys should be rejected (401)"
$ECHO "  ${GREEN}✓${NC} Requests with valid keys should succeed (200)"
$ECHO "  ${GREEN}✓${NC} Rate limiting should work per key"
$ECHO "  ${GREEN}✓${NC} Malformed headers should be rejected (401)"
$ECHO ""
$ECHO "Expected behavior without master_keys configured:"
$ECHO "  ${YELLOW}⚠${NC} All requests should be allowed regardless of authentication"
$ECHO ""