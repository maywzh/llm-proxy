#!/usr/bin/env bash

# Manual Rate Limiting Test Script
# Tests concurrent requests to verify rate limiting works correctly
#
# Usage: ./test_rate_limit_manual.sh [KEY] [CONCURRENT_COUNT]
#   KEY: Master key to test (default: sk-dev-key)
#   CONCURRENT_COUNT: Number of concurrent requests (default: 4)
#
# Example: ./test_rate_limit_manual.sh sk-limit 10

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
TEST_KEY="${1:-sk-dev-key}"
CONCURRENT_COUNT="${2:-4}"

# Configuration
BASE_URL="${BASE_URL:-http://localhost:18000}"
HEALTH_ENDPOINT="${BASE_URL}/health"
API_ENDPOINT="${BASE_URL}/v1/chat/completions"

$ECHO "${BLUE}=== Rate Limiting Manual Test ===${NC}"
$ECHO "Test Key: ${YELLOW}${TEST_KEY}${NC}"
$ECHO "Concurrent Count: ${YELLOW}${CONCURRENT_COUNT}${NC}"
echo ""

# Check if server is running
$ECHO "${YELLOW}Checking if server is running...${NC}"
if ! curl -s -f "${HEALTH_ENDPOINT}" > /dev/null 2>&1; then
    $ECHO "${RED}Error: Server is not running at ${BASE_URL}${NC}"
    echo "Please start the server first: make run"
    exit 1
fi
$ECHO "${GREEN}✓ Server is running${NC}"
echo ""

# Function to make a request
make_request() {
    local key=$1
    local request_num=$2
    
    response=$(curl -s -w "\n%{http_code}" -X POST "${API_ENDPOINT}" \
        -H "Authorization: Bearer ${key}" \
        -H "Content-Type: application/json" \
        -d '{
            "model": "claude-4.5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 10
        }' 2>&1)
    
    # Extract HTTP code (last line) and body (all but last line)
    http_code=$(echo "$response" | tail -n 1)
    body=$(echo "$response" | sed '$d')
    
    if [ "$http_code" = "200" ]; then
        $ECHO "${GREEN}Request #${request_num}: ✓ Success (200)${NC}"
        return 0
    elif [ "$http_code" = "429" ]; then
        $ECHO "${YELLOW}Request #${request_num}: ⚠ Rate Limited (429)${NC}"
        return 1
    elif [ "$http_code" = "401" ]; then
        $ECHO "${RED}Request #${request_num}: ✗ Unauthorized (401)${NC}"
        return 2
    else
        $ECHO "${RED}Request #${request_num}: ✗ Error (${http_code})${NC}"
        return 3
    fi
}

# Test 1: Sequential requests
$ECHO "${BLUE}Test 1: Sequential requests${NC}"
echo "Sending 4 sequential requests..."
success_count=0
for i in {1..4}; do
    if make_request "${TEST_KEY}" "$i"; then
        ((success_count++))
    fi
done
$ECHO "Result: ${GREEN}${success_count}/4 succeeded${NC}"
echo ""

# Test 2: Concurrent burst
$ECHO "${BLUE}Test 2: Concurrent burst (${CONCURRENT_COUNT} requests)${NC}"
echo "Sending ${CONCURRENT_COUNT} concurrent requests..."

# Create temp file for results
TEMP_RESULTS=$(mktemp)

# Run concurrent requests
for i in $(seq 1 $CONCURRENT_COUNT); do
    (
        response=$(curl -s -w "\n%{http_code}" -X POST "${API_ENDPOINT}" \
            -H "Authorization: Bearer ${TEST_KEY}" \
            -H "Content-Type: application/json" \
            -d '{
                "model": "claude-4.5-sonnet",
                "messages": [{"role": "user", "content": "Hello"}],
                "max_tokens": 10
            }' 2>&1)
        
        http_code=$(echo "$response" | tail -n 1)
        echo "$http_code" >> "$TEMP_RESULTS"
        
        if [ "$http_code" = "200" ]; then
            $ECHO "${GREEN}Request #${i}: ✓ Success (200)${NC}"
        elif [ "$http_code" = "429" ]; then
            $ECHO "${YELLOW}Request #${i}: ⚠ Rate Limited (429)${NC}"
        elif [ "$http_code" = "401" ]; then
            $ECHO "${RED}Request #${i}: ✗ Unauthorized (401)${NC}"
        else
            $ECHO "${RED}Request #${i}: ✗ Error (${http_code})${NC}"
        fi
    ) &
done

# Wait for all background jobs
wait

# Count results - use safer method to ensure clean integer values
success_count=$(grep -c "^200$" "$TEMP_RESULTS" 2>/dev/null || echo "0")
rate_limited_count=$(grep -c "^429$" "$TEMP_RESULTS" 2>/dev/null || echo "0")
unauthorized_count=$(grep -c "^401$" "$TEMP_RESULTS" 2>/dev/null || echo "0")
total_count=$(wc -l < "$TEMP_RESULTS" 2>/dev/null || echo "0")

# Strip all whitespace and ensure variables are clean integers
success_count=$(echo "$success_count" | tr -d '[:space:]')
rate_limited_count=$(echo "$rate_limited_count" | tr -d '[:space:]')
unauthorized_count=$(echo "$unauthorized_count" | tr -d '[:space:]')
total_count=$(echo "$total_count" | tr -d '[:space:]')

# Set defaults if empty
: ${success_count:=0}
: ${rate_limited_count:=0}
: ${unauthorized_count:=0}
: ${total_count:=0}

# Calculate error count
error_count=$((total_count - success_count - rate_limited_count - unauthorized_count))

rm -f "$TEMP_RESULTS"

echo ""
$ECHO "Results:"
$ECHO "  ${GREEN}✓ Success (200): ${success_count}${NC}"
$ECHO "  ${YELLOW}⚠ Rate Limited (429): ${rate_limited_count}${NC}"
if [ "$unauthorized_count" -gt 0 ]; then
    $ECHO "  ${RED}✗ Unauthorized (401): ${unauthorized_count}${NC}"
fi
if [ "$error_count" -gt 0 ]; then
    $ECHO "  ${RED}✗ Other Errors: ${error_count}${NC}"
fi
echo ""

# Test 3: Rapid sequential requests
$ECHO "${BLUE}Test 3: Rapid sequential requests${NC}"
echo "Sending $((CONCURRENT_COUNT * 2)) rapid sequential requests..."

success_count=0
rate_limited_count=0

for i in $(seq 1 $((CONCURRENT_COUNT * 2))); do
    response=$(curl -s -w "\n%{http_code}" -X POST "${API_ENDPOINT}" \
        -H "Authorization: Bearer ${TEST_KEY}" \
        -H "Content-Type: application/json" \
        -d '{
            "model": "claude-4.5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "max_tokens": 10
        }' 2>&1)
    
    http_code=$(echo "$response" | tail -n1)
    
    if [ "$http_code" = "200" ]; then
        ((success_count++))
        printf "${GREEN}.${NC}"
    elif [ "$http_code" = "429" ]; then
        ((rate_limited_count++))
        printf "${YELLOW}X${NC}"
    else
        printf "${RED}!${NC}"
    fi
done

echo ""
$ECHO "Results: ${GREEN}${success_count} succeeded${NC}, ${YELLOW}${rate_limited_count} rate limited${NC}"
echo ""

# Summary
$ECHO "${BLUE}=== Test Summary ===${NC}"
$ECHO "Test Key: ${YELLOW}${TEST_KEY}${NC}"
$ECHO "Concurrent Count: ${YELLOW}${CONCURRENT_COUNT}${NC}"
echo ""
if [ "$unauthorized_count" -gt 0 ]; then
    $ECHO "${RED}✗ Authentication failed - check your key${NC}"
elif [ "$rate_limited_count" -gt 0 ]; then
    $ECHO "${GREEN}✓ Rate limiting is working correctly${NC}"
    $ECHO "  - Some requests succeeded"
    $ECHO "  - Some requests were rate limited (429)"
else
    $ECHO "${YELLOW}⚠ No rate limiting detected${NC}"
    $ECHO "  - All requests succeeded"
    $ECHO "  - Rate limit may be too high or not configured"
fi
echo ""
$ECHO "${GREEN}Test completed!${NC}"