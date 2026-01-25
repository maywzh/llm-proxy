"""Middleware for metrics collection"""
import time
from typing import Callable
from fastapi import Request, Response
from starlette.middleware.base import BaseHTTPMiddleware

from app.core.metrics import REQUEST_COUNT, REQUEST_DURATION, ACTIVE_REQUESTS
from app.core.logging import get_logger, get_api_key_name, clear_api_key_context

logger = get_logger()


class MetricsMiddleware(BaseHTTPMiddleware):
    """Middleware to collect request metrics"""

    async def dispatch(self, request: Request, call_next: Callable) -> Response:
        """Process request and collect metrics"""
        endpoint = request.url.path
        method = request.method

        # Skip metrics endpoint itself
        if endpoint == '/metrics':
            return await call_next(request)

        # Skip event_logging endpoint (Claude Code telemetry) - handled by endpoint itself
        if endpoint == '/api/event_logging/batch':
            return await call_next(request)

        # Increment active requests
        ACTIVE_REQUESTS.labels(endpoint=endpoint).inc()

        # Track request duration
        start_time = time.time()

        try:
            response = await call_next(request)
            duration = time.time() - start_time

            # Get model and provider from request state (set by API handlers)
            # Get api_key_name from context (set by verify_auth dependency)
            model = getattr(request.state, 'model', 'unknown')
            provider = getattr(request.state, 'provider', 'unknown')
            api_key_name = get_api_key_name()
            status_code = response.status_code

            # Record metrics only for LLM requests (where provider is set)
            # Skip non-LLM endpoints like /v1/models, /health, etc.
            if provider != 'unknown':
                REQUEST_COUNT.labels(
                    method=method,
                    endpoint=endpoint,
                    model=model,
                    provider=provider,
                    status_code=status_code,
                    api_key_name=api_key_name
                ).inc()

                REQUEST_DURATION.labels(
                    method=method,
                    endpoint=endpoint,
                    model=model,
                    provider=provider,
                    api_key_name=api_key_name
                ).observe(duration)

            # Log request details - show model/provider/key for LLM endpoints
            log_message = f"{method} {endpoint}"
            if endpoint in ('/v1/chat/completions', '/v1/messages'):
                log_message += f" - model={model} provider={provider} key={api_key_name}"
            log_message += f" status={status_code} duration={duration:.3f}s"
            logger.info(log_message)

            return response

        except Exception as e:
            duration = time.time() - start_time
            log_message = f"{method} {endpoint}"
            if endpoint in ('/v1/chat/completions', '/v1/messages'):
                model = getattr(request.state, 'model', 'unknown')
                provider = getattr(request.state, 'provider', 'unknown')
                api_key_name = get_api_key_name()
                log_message += f" - model={model} provider={provider} key={api_key_name}"
            log_message += f" - Error: {type(e).__name__}: {str(e)} duration={duration:.3f}s"
            logger.error(log_message)
            raise

        finally:
            # Decrement active requests
            ACTIVE_REQUESTS.labels(endpoint=endpoint).dec()
            # Clear API key context to prevent leaking between requests
            clear_api_key_context()
