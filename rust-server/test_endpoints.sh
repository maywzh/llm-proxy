#!/bin/bash

# Test script for Rust LLM Proxy Server
# This script tests all available endpoints

BASE_URL="http://localhost:18002"

echo "=== Testing Rust LLM Proxy Server ==="
echo ""

echo "1. Testing Health Endpoint"
echo "GET $BASE_URL/health"
curl -s "$BASE_URL/health" | jq .
echo ""

echo "2. Testing Detailed Health Endpoint"
echo "GET $BASE_URL/health/detailed"
curl -s "$BASE_URL/health/detailed" | jq .
echo ""

echo "3. Testing Models Endpoint"
echo "GET $BASE_URL/v1/models"
curl -s "$BASE_URL/v1/models" | jq .
echo ""

echo "4. Testing Metrics Endpoint"
echo "GET $BASE_URL/metrics"
curl -s "$BASE_URL/metrics" | head -30
echo ""

echo "5. Testing Chat Completions Endpoint (non-streaming)"
echo "POST $BASE_URL/v1/chat/completions"
curl -s -X POST "$BASE_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 10
  }' | jq .
echo ""

echo "6. Testing Chat Completions Endpoint (streaming)"
echo "POST $BASE_URL/v1/chat/completions (stream=true)"
curl -s -X POST "$BASE_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Hello"}],
    "max_tokens": 10,
    "stream": true
  }' | head -20
echo ""

echo "=== All endpoint tests completed ==="