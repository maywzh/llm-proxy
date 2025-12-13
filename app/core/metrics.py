"""Prometheus metrics for LLM API Proxy"""
from prometheus_client import Counter, Histogram, Gauge, Info

# Request metrics
REQUEST_COUNT = Counter(
    'llm_proxy_requests_total',
    'Total number of requests',
    ['method', 'endpoint', 'model', 'provider', 'status_code']
)

REQUEST_DURATION = Histogram(
    'llm_proxy_request_duration_seconds',
    'Request duration in seconds',
    ['method', 'endpoint', 'model', 'provider'],
    buckets=(0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0, 120.0, float('inf'))
)

ACTIVE_REQUESTS = Gauge(
    'llm_proxy_active_requests',
    'Number of active requests',
    ['endpoint']
)

# Token usage metrics
TOKEN_USAGE = Counter(
    'llm_proxy_tokens_total',
    'Total number of tokens used',
    ['model', 'provider', 'token_type']
)

# Provider health metrics
PROVIDER_HEALTH = Gauge(
    'llm_proxy_provider_health',
    'Provider health status (1=healthy, 0=unhealthy)',
    ['provider']
)

PROVIDER_LATENCY = Histogram(
    'llm_proxy_provider_latency_seconds',
    'Provider response latency in seconds',
    ['provider'],
    buckets=(0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, float('inf'))
)

# Application info
APP_INFO = Info('llm_proxy_app', 'Application information')