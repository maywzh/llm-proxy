"""Test RemoteProtocolError handling"""

import pytest
import httpx
from unittest.mock import AsyncMock, MagicMock, patch
from fastapi.testclient import TestClient


@pytest.mark.asyncio
async def test_remote_protocol_error_non_streaming(app_with_config):
    """Test that RemoteProtocolError is properly handled for non-streaming requests"""

    with patch("app.api.proxy.get_http_client") as mock_get_client:
        mock_client = MagicMock()
        mock_get_client.return_value = mock_client

        # Simulate RemoteProtocolError
        mock_client.post = AsyncMock(
            side_effect=httpx.RemoteProtocolError(
                "peer closed connection without sending complete message body"
            )
        )

        client = TestClient(app_with_config)
        response = client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": [{"role": "user", "content": "test"}]},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        # Should return 502 Bad Gateway
        assert response.status_code == 502
        # proxy.py returns {"detail": "..."} format
        error_msg = response.json().get("detail", "").lower()
        assert "network error" in error_msg


@pytest.mark.asyncio
async def test_remote_protocol_error_streaming(app_with_config):
    """Test that streaming connection errors are handled gracefully"""

    with patch("app.api.proxy.get_http_client") as mock_get_client:
        mock_client = MagicMock()
        mock_get_client.return_value = mock_client

        # Simulate connection error during stream setup
        mock_stream_ctx = MagicMock()

        async def enter():
            raise httpx.RemoteProtocolError("Connection closed during streaming")

        mock_stream_ctx.__aenter__.side_effect = enter
        mock_stream_ctx.__aexit__ = AsyncMock(return_value=None)
        mock_client.stream.return_value = mock_stream_ctx

        client = TestClient(app_with_config)

        response = client.post(
            "/v1/chat/completions",
            json={
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "test"}],
                "stream": True,
            },
            headers={"Authorization": "Bearer test-credential-key"},
        )

        # Should return 502 Bad Gateway when connection fails
        assert response.status_code == 502
        error_msg = response.json().get("detail", "").lower()
        assert "connection failed" in error_msg or "network error" in error_msg


@pytest.mark.asyncio
async def test_other_network_errors_handled(app_with_config):
    """Test that other network errors are also properly handled"""

    with patch("app.api.proxy.get_http_client") as mock_get_client:
        mock_client = MagicMock()
        mock_get_client.return_value = mock_client

        # Test ConnectError
        mock_client.post = AsyncMock(
            side_effect=httpx.ConnectError("Connection refused")
        )

        client = TestClient(app_with_config)
        response = client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": [{"role": "user", "content": "test"}]},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        # Should return 502
        assert response.status_code == 502
        # proxy.py returns {"detail": "..."} format
        error_msg = response.json().get("detail", "").lower()
        assert "network error" in error_msg


@pytest.mark.asyncio
async def test_timeout_error_returns_504(app_with_config):
    """Test that timeout errors return 504 Gateway Timeout"""

    with patch("app.api.proxy.get_http_client") as mock_get_client:
        mock_client = MagicMock()
        mock_get_client.return_value = mock_client

        # Simulate timeout
        mock_client.post = AsyncMock(side_effect=httpx.ReadTimeout("Read timeout"))

        client = TestClient(app_with_config)
        response = client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": [{"role": "user", "content": "test"}]},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        # Should return 504 Gateway Timeout
        assert response.status_code == 504
        # proxy.py returns {"detail": "..."} format
        error_msg = response.json().get("detail", "").lower()
        assert "timeout" in error_msg
