"""Unified upstream request helpers.

Centralizes auth header building, error response formatting,
and transport/status feedback reporting for adaptive routing.
Designed for incremental handler migration.
"""

from dataclasses import dataclass
from typing import Any, Optional

import httpx
from fastapi.responses import JSONResponse, Response

from app.core.error_types import ERROR_TYPE_API, ERROR_TYPE_TIMEOUT
from app.core.logging import get_logger
from app.services.provider_service import ProviderService
from app.transformer import Protocol

logger = get_logger()

MAX_ERROR_MESSAGE_LEN = 500


@dataclass
class UpstreamContext:
    """Common context for upstream operations."""

    protocol: Protocol
    model: Optional[str]
    provider: str
    api_key_name: Optional[str] = None
    request_id: Optional[str] = None


def build_upstream_headers(
    protocol: Protocol,
    api_key: str,
    anthropic_version: Optional[str] = None,
    anthropic_beta: Optional[str] = None,
) -> dict[str, str]:
    """Build auth headers based on provider protocol."""
    headers: dict[str, str] = {"Content-Type": "application/json"}
    if protocol == Protocol.ANTHROPIC:
        headers["x-api-key"] = api_key
        headers["anthropic-version"] = anthropic_version or "2023-06-01"
    elif protocol == Protocol.GCP_VERTEX:
        headers["Authorization"] = f"Bearer {api_key}"
        headers["anthropic-version"] = anthropic_version or "vertex-2023-10-16"
    else:
        headers["Authorization"] = f"Bearer {api_key}"
    if anthropic_beta:
        headers["anthropic-beta"] = anthropic_beta
    return headers


def build_gcp_vertex_url(
    api_base: str,
    gcp_project: str,
    gcp_location: str,
    gcp_publisher: str,
    model: str,
    is_streaming: bool,
    blocking_action: str = "rawPredict",
    streaming_action: str = "streamRawPredict",
) -> str:
    """Build GCP Vertex AI endpoint URL with path traversal validation."""
    for segment in (gcp_project, gcp_location, gcp_publisher, model):
        if not segment or "/" in segment or "\\" in segment or segment in ("..", "."):
            raise ValueError(
                "GCP Vertex URL parameters must not contain path separators "
                "or traversal sequences"
            )
    action = streaming_action if is_streaming else blocking_action
    return (
        f"{api_base}/v1/projects/{gcp_project}"
        f"/locations/{gcp_location}/publishers/{gcp_publisher}"
        f"/models/{model}:{action}"
    )


def build_provider_debug_headers(
    protocol: Protocol,
    url: str,
    headers: dict[str, str],
    anthropic_beta: Optional[str] = None,
) -> dict[str, str]:
    """Build masked provider request headers for error logging."""
    h: dict[str, str] = {"url": url, "content-type": "application/json"}
    if protocol == Protocol.ANTHROPIC:
        h["x-api-key"] = "***"
        h["anthropic-version"] = headers.get("anthropic-version", "2023-06-01")
        if anthropic_beta:
            h["anthropic-beta"] = anthropic_beta
    elif protocol == Protocol.GCP_VERTEX:
        h["authorization"] = "Bearer ***"
        h["anthropic-version"] = headers.get("anthropic-version", "vertex-2023-10-16")
        if anthropic_beta:
            h["anthropic-beta"] = anthropic_beta
    else:
        h["authorization"] = "Bearer ***"
    return h


def build_protocol_error_body(
    protocol: Protocol,
    status_code: int,
    error_type: str,
    message: str,
) -> dict[str, Any]:
    """Build protocol-specific error response body."""
    if protocol in (Protocol.ANTHROPIC, Protocol.GCP_VERTEX):
        return {"type": "error", "error": {"type": error_type, "message": message}}
    return {"error": {"message": message, "type": error_type, "code": status_code}}


def build_protocol_error_response(
    protocol: Protocol,
    status_code: int,
    error_type: str,
    message: str,
    model: Optional[str] = None,
    provider: Optional[str] = None,
) -> Response:
    """Build protocol-aware JSON error response."""
    body = build_protocol_error_body(protocol, status_code, error_type, message)
    response = JSONResponse(content=body, status_code=status_code)
    if model or provider:
        attach_response_metadata(response, model or "unknown", provider or "unknown")
    return response


def extract_error_message(body: dict[str, Any]) -> Optional[str]:
    """Extract canonical error message from provider error payload."""
    error = body.get("error")
    if isinstance(error, dict):
        msg = error.get("message")
        if isinstance(msg, str):
            return msg
    elif isinstance(error, str):
        return error
    msg = body.get("message")
    if isinstance(msg, str):
        return msg
    return None


def truncate_message(message: str) -> str:
    """Truncate long error messages preserving unicode character boundaries."""
    chars = list(message)
    if len(chars) <= MAX_ERROR_MESSAGE_LEN:
        return message
    return "".join(chars[:MAX_ERROR_MESSAGE_LEN]) + "..."


def classify_upstream_error(
    exc: Exception,
) -> tuple[int, str, str]:
    """Classify upstream transport errors into (status_code, error_type, message)."""
    if isinstance(exc, httpx.TimeoutException):
        return (504, ERROR_TYPE_TIMEOUT, "Upstream request timed out")
    if isinstance(exc, httpx.ConnectError):
        return (502, ERROR_TYPE_API, "Failed to connect to upstream provider")
    return (502, ERROR_TYPE_API, "Upstream request failed")


def attach_response_metadata(response: Response, model: str, provider: str) -> Response:
    """Attach model/provider info to response for downstream middleware."""
    extensions = getattr(response, "extensions", None)
    if not isinstance(extensions, dict):
        extensions = {}
        setattr(response, "extensions", extensions)
    extensions["model"] = model
    extensions["provider"] = provider
    return response


async def execute_upstream_request(
    client: httpx.AsyncClient,
    method: str,
    url: str,
    provider_service: ProviderService,
    provider_name: str,
    **kwargs: Any,
) -> httpx.Response:
    """Execute upstream request and report status to ProviderService.

    Transport errors trigger report_transport_error.
    HTTP responses trigger report_http_status with Retry-After support.
    """
    try:
        response = await client.request(method, url, **kwargs)
        retry_after = response.headers.get("retry-after")
        provider_service.report_http_status(
            provider_name, response.status_code, retry_after
        )
        return response
    except (httpx.TimeoutException, httpx.RequestError):
        provider_service.report_transport_error(provider_name)
        raise


async def execute_upstream_request_or_error(
    client: httpx.AsyncClient,
    method: str,
    url: str,
    provider_service: ProviderService,
    ctx: UpstreamContext,
    **kwargs: Any,
) -> httpx.Response | Response:
    """Execute upstream request, return transport error as protocol error response."""
    try:
        return await execute_upstream_request(
            client, method, url, provider_service, ctx.provider, **kwargs
        )
    except httpx.TimeoutException:
        logger.error(
            f"Timeout connecting to provider {ctx.provider}",
            extra={"request_id": ctx.request_id},
        )
        return build_protocol_error_response(
            ctx.protocol,
            504,
            ERROR_TYPE_TIMEOUT,
            "Upstream request timed out",
            ctx.model,
            ctx.provider,
        )
    except httpx.RequestError as exc:
        logger.error(
            f"Network error for provider {ctx.provider}: {exc}",
            extra={"request_id": ctx.request_id},
        )
        return build_protocol_error_response(
            ctx.protocol,
            502,
            ERROR_TYPE_API,
            "Failed to connect to upstream provider",
            ctx.model,
            ctx.provider,
        )
