"""Middleware for metrics collection"""
import time
from typing import Callable
from fastapi import Request, Response
from starlette.middleware.base import BaseHTTPMiddleware

from app.core.metrics import REQUEST_COUNT, REQUEST_DURATION, ACTIVE_REQUESTS
from app.core.logging import get_logger

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
        
        # Increment active requests
        ACTIVE_REQUESTS.labels(endpoint=endpoint).inc()
        
        # Track request duration
        start_time = time.time()
        
        try:
            response = await call_next(request)
            duration = time.time() - start_time
            
            # Get model and provider from request state (set by API handlers)
            model = getattr(request.state, 'model', 'unknown')
            provider = getattr(request.state, 'provider', 'unknown')
            status_code = response.status_code
            
            # Record metrics
            REQUEST_COUNT.labels(
                method=method,
                endpoint=endpoint,
                model=model,
                provider=provider,
                status_code=status_code
            ).inc()
            
            REQUEST_DURATION.labels(
                method=method,
                endpoint=endpoint,
                model=model,
                provider=provider
            ).observe(duration)
            
            # Log request details
            logger.info(
                f"{method} {endpoint} - model={model} provider={provider} "
                f"status={status_code} duration={duration:.3f}s"
            )
            
            return response
            
        except Exception as e:
            duration = time.time() - start_time
            logger.error(
                f"{method} {endpoint} - Error: {type(e).__name__}: {str(e)} "
                f"duration={duration:.3f}s"
            )
            raise
            
        finally:
            # Decrement active requests
            ACTIVE_REQUESTS.labels(endpoint=endpoint).dec()