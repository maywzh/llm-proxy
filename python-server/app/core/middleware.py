"""Middleware for metrics collection and model permission checking"""

import json
import time
import uuid
from typing import Callable, Set
from fastapi import Request, Response, HTTPException
from starlette.middleware.base import BaseHTTPMiddleware
from starlette.responses import JSONResponse

from app.core.metrics import REQUEST_COUNT, REQUEST_DURATION, ACTIVE_REQUESTS
from app.core.logging import get_logger, get_api_key_name, clear_api_key_context
from app.core.security import verify_credential_key
from app.api.dependencies import model_matches_allowed_list
from app.utils.client import extract_client

logger = get_logger()

# Paths that require model permission checking
MODEL_CHECK_PATHS: Set[str] = {
    "/v1/chat/completions",
    "/v1/completions",
    "/v1/messages",
    "/v1/responses",
    "/messages/count_tokens",
    "/v2/chat/completions",
    "/v2/messages",
    "/v2/responses",
    "/chat/completions",
    "/messages",
    "/responses",
}


class MetricsMiddleware(BaseHTTPMiddleware):
    """Middleware to collect request metrics"""

    async def dispatch(self, request: Request, call_next: Callable) -> Response:
        """Process request and collect metrics"""
        endpoint = request.url.path
        method = request.method
        client = extract_client(request)

        # LLM endpoints that should show detailed logging
        llm_endpoints = (
            "/v1/chat/completions",
            "/v1/messages",
            "/v1/completions",
            "/v1/responses",
            "/v2/chat/completions",
            "/v2/messages",
            "/v2/completions",
            "/v2/responses",
            "/chat/completions",
            "/messages",
            "/responses",
        )

        # Skip metrics endpoint itself
        if endpoint == "/metrics":
            return await call_next(request)

        # Skip event_logging endpoint (Claude Code telemetry) - handled by endpoint itself
        if endpoint == "/api/event_logging/batch":
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
            model = getattr(request.state, "model", "unknown")
            provider = getattr(request.state, "provider", "unknown")
            api_key_name = get_api_key_name()
            status_code = response.status_code

            # Record metrics only for LLM requests (where provider is set)
            # Skip non-LLM endpoints like /v1/models, /health, etc.
            if provider != "unknown":
                REQUEST_COUNT.labels(
                    method=method,
                    endpoint=endpoint,
                    model=model,
                    provider=provider,
                    status_code=status_code,
                    api_key_name=api_key_name,
                    client=client,
                ).inc()

                REQUEST_DURATION.labels(
                    method=method,
                    endpoint=endpoint,
                    model=model,
                    provider=provider,
                    api_key_name=api_key_name,
                    client=client,
                ).observe(duration)

            # Log request details - show model/provider/key/client for LLM endpoints
            log_message = f"{method} {endpoint}"
            if endpoint in llm_endpoints:
                log_message += f" - status={status_code} client={client} key={api_key_name} model={model} provider={provider}"
            log_message += f" duration={duration:.3f}s"
            logger.info(log_message)

            return response

        except Exception as e:
            duration = time.time() - start_time
            log_message = f"{method} {endpoint}"
            if endpoint in llm_endpoints:
                model = getattr(request.state, "model", "unknown")
                provider = getattr(request.state, "provider", "unknown")
                api_key_name = get_api_key_name()
                log_message += f" - client={client} key={api_key_name} model={model} provider={provider}"
            log_message += (
                f" - Error: {type(e).__name__}: {str(e)} duration={duration:.3f}s"
            )
            logger.error(log_message)
            raise

        finally:
            # Decrement active requests
            ACTIVE_REQUESTS.labels(endpoint=endpoint).dec()
            # Clear API key context to prevent leaking between requests
            clear_api_key_context()


class ModelPermissionMiddleware(BaseHTTPMiddleware):
    """Middleware to check model permissions for specific endpoints.

    Only checks model permissions for paths in MODEL_CHECK_PATHS.
    Other paths pass through without any check.
    """

    async def dispatch(self, request: Request, call_next: Callable) -> Response:
        """Check model permission before processing request"""
        endpoint = request.url.path

        # Skip if not in whitelist
        if endpoint not in MODEL_CHECK_PATHS:
            return await call_next(request)

        # Skip non-POST requests (GET /v1/models etc.)
        if request.method != "POST":
            return await call_next(request)

        try:
            # Read and cache body for later use
            body = await request.body()

            # Parse JSON to extract model
            try:
                payload = json.loads(body) if body else {}
            except json.JSONDecodeError:
                # Let the handler deal with invalid JSON
                return await call_next(request)

            model = payload.get("model")
            if not model:
                # No model in request, let handler deal with it
                return await call_next(request)

            # Get credential from headers
            authorization = request.headers.get("authorization")
            x_api_key = request.headers.get("x-api-key")

            try:
                is_valid, credential_config = verify_credential_key(
                    authorization=authorization,
                    x_api_key=x_api_key,
                )
            except HTTPException:
                # Auth failure will be handled by the actual auth dependency
                return await call_next(request)

            # Check model permission
            if credential_config and credential_config.allowed_models:
                if not model_matches_allowed_list(
                    model, credential_config.allowed_models
                ):
                    logger.warning(
                        f"Model permission denied: model={model}, "
                        f"credential={credential_config.name}, "
                        f"allowed_models={credential_config.allowed_models}"
                    )
                    return JSONResponse(
                        status_code=403,
                        content={
                            "error": {
                                "message": f"Model '{model}' is not allowed for this credential. "
                                f"Allowed models: {credential_config.allowed_models}",
                                "type": "permission_error",
                                "code": "model_not_allowed",
                            }
                        },
                    )

            return await call_next(request)

        except Exception as e:
            logger.error(f"Error in ModelPermissionMiddleware: {e}")
            # On any error, let the request proceed to handler
            return await call_next(request)


class RequestIdMiddleware(BaseHTTPMiddleware):
    """Middleware to assign a request ID to each request."""

    async def dispatch(self, request: Request, call_next: Callable) -> Response:
        request_id = request.headers.get("x-request-id") or str(uuid.uuid4())
        request.state.request_id = request_id
        response = await call_next(request)
        response.headers["x-request-id"] = request_id
        return response
