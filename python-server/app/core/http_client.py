"""Shared HTTP client for making requests to providers"""
import httpx
from typing import Optional

from app.core.config import get_config


_http_client: Optional[httpx.AsyncClient] = None


def get_http_client() -> httpx.AsyncClient:
    """
    Get or create the shared HTTP client instance
    
    Returns:
        Shared httpx.AsyncClient configured with app settings
    """
    global _http_client
    if _http_client is None:
        config = get_config()
        _http_client = httpx.AsyncClient(
            verify=config.verify_ssl,
            timeout=float(config.request_timeout_secs),
            limits=httpx.Limits(
                max_keepalive_connections=20,
                max_connections=100,
                keepalive_expiry=30.0
            )
        )
    return _http_client


async def close_http_client() -> None:
    """Close the shared HTTP client"""
    global _http_client
    if _http_client is not None:
        await _http_client.aclose()
        _http_client = None