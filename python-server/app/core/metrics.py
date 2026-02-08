"""Prometheus metrics for LLM API Proxy"""

from prometheus_client import Counter, Histogram, Gauge, Info

# Request metrics
REQUEST_COUNT = Counter(
    "llm_proxy_requests_total",
    "Total number of requests",
    [
        "method",
        "endpoint",
        "model",
        "provider",
        "status_code",
        "api_key_name",
        "client",
    ],
)

REQUEST_DURATION = Histogram(
    "llm_proxy_request_duration_seconds",
    "Request duration in seconds",
    ["method", "endpoint", "model", "provider", "api_key_name", "client"],
    buckets=(0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, float("inf")),
)

ACTIVE_REQUESTS = Gauge(
    "llm_proxy_active_requests", "Number of active requests", ["endpoint"]
)

# Token usage metrics
TOKEN_USAGE = Counter(
    "llm_proxy_tokens_total",
    "Total number of tokens used",
    ["model", "provider", "token_type", "api_key_name", "client"],
)

# Provider health metrics
PROVIDER_HEALTH = Gauge(
    "llm_proxy_provider_health",
    "Provider health status (1=healthy, 0=unhealthy)",
    ["provider"],
)

PROVIDER_LATENCY = Histogram(
    "llm_proxy_provider_latency_seconds",
    "Provider response latency in seconds",
    ["provider"],
    buckets=(0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, float("inf")),
)

# Performance metrics for streaming requests
TTFT = Histogram(
    "llm_proxy_ttft_seconds",
    "Time to first token (TTFT) in seconds for streaming requests (upstream provider latency)",
    ["source", "model", "provider"],
    buckets=(0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0),
)

TOKENS_PER_SECOND = Histogram(
    "llm_proxy_tokens_per_second",
    "Tokens per second (TPS) throughput for streaming requests (upstream provider throughput)",
    ["source", "model", "provider"],
    buckets=(1.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0),
)

# Bypass and cross-protocol metrics
BYPASS_REQUESTS = Counter(
    "llm_proxy_bypass_requests_total",
    "Total number of bypassed requests (same-protocol optimization)",
    ["client_protocol", "provider_protocol", "provider"],
)

BYPASS_STREAMING_BYTES = Counter(
    "llm_proxy_bypass_streaming_bytes_total",
    "Total bytes passed through without transformation (zero-copy streaming)",
    ["provider"],
)

CROSS_PROTOCOL_REQUESTS = Counter(
    "llm_proxy_cross_protocol_requests_total",
    "Total number of cross-protocol requests (protocol transformation required)",
    ["client_protocol", "provider_protocol", "provider"],
)

# Adaptive routing metrics
PROVIDER_EFFECTIVE_WEIGHT = Gauge(
    "llm_proxy_provider_effective_weight",
    "Runtime effective weight used for provider selection",
    ["provider"],
)

PROVIDER_CIRCUIT_STATE = Gauge(
    "llm_proxy_provider_circuit_state",
    "Provider circuit state flag (1 for active state, 0 otherwise)",
    ["provider", "state"],
)

PROVIDER_EJECTIONS_TOTAL = Counter(
    "llm_proxy_provider_ejections_total",
    "Total number of provider ejections by reason",
    ["provider", "reason"],
)

# Client disconnect metrics
CLIENT_DISCONNECTS = Counter(
    "llm_proxy_client_disconnects_total",
    "Total number of client disconnections during streaming",
    ["model", "provider"],
)

# Application info
APP_INFO = Info("llm_proxy_app", "Application information")
