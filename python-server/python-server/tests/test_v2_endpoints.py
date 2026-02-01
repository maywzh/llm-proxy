"""Tests for V2 API endpoints authentication and model permissions."""

import pytest
from unittest.mock import AsyncMock, MagicMock, patch
from httpx import AsyncClient

from app.main import app


@pytest.mark.unit
class TestV2ChatCompletions:
    """Tests for /v2/chat/completions endpoint."""

    @pytest.mark.asyncio
    async def test_v2_chat_completions_auth_required(self):
        """Test that /v2/chat/completions requires authentication."""
        with patch("app.core.config.get_config") as mock_get_config:
            # Configure mock to return config with credentials
            mock_config = MagicMock()
            mock_config.credentials = [MagicMock()]  # Non-empty credentials list
            mock_config.providers = []
            mock_get_config.return_value = mock_config

            async with AsyncClient(app=app, base_url="http://test") as client:
                response = await client.post(
                    "/v2/chat/completions",
                    json={
                        "model": "gpt-4",
                        "messages": [{"role": "user", "content": "hi"}],
                    },
                )
                assert response.status_code == 401

    @pytest.mark.asyncio
    async def test_v2_chat_completions_with_x_api_key(self):
        """Test that /v2/chat/completions accepts x-api-key header."""
        from app.core.database import hash_key

        raw_key = "test-key"
        hashed_key = hash_key(raw_key)

        with patch("app.core.config.get_config") as mock_get_config:
            mock_credential = MagicMock()
            mock_credential.credential_key = hashed_key
            mock_credential.name = "test-key"
            mock_credential.allowed_models = []
            mock_credential.rate_limit = None
            mock_credential.enabled = True

            mock_config = MagicMock()
            mock_config.credentials = [mock_credential]
            mock_config.providers = []
            mock_get_config.return_value = mock_config

            async with AsyncClient(app=app, base_url="http://test") as client:
                response = await client.post(
                    "/v2/chat/completions",
                    headers={"x-api-key": raw_key},
                    json={
                        "model": "gpt-4",
                        "messages": [{"role": "user", "content": "hi"}],
                    },
                )
                # Should pass auth (may fail at provider selection, but not 401)
                assert response.status_code != 401


@pytest.mark.unit
class TestV2Messages:
    """Tests for /v2/messages endpoint."""

    @pytest.mark.asyncio
    async def test_v2_messages_auth_required(self):
        """Test that /v2/messages requires authentication."""
        with patch("app.core.config.get_config") as mock_get_config:
            mock_config = MagicMock()
            mock_config.credentials = [MagicMock()]
            mock_config.providers = []
            mock_get_config.return_value = mock_config

            async with AsyncClient(app=app, base_url="http://test") as client:
                response = await client.post(
                    "/v2/messages",
                    json={
                        "model": "claude-3",
                        "max_tokens": 100,
                        "messages": [{"role": "user", "content": "hi"}],
                    },
                )
                assert response.status_code == 401


@pytest.mark.unit
class TestV2Responses:
    """Tests for /v2/responses endpoint."""

    @pytest.mark.asyncio
    async def test_v2_responses_auth_required(self):
        """Test that /v2/responses requires authentication."""
        with patch("app.core.config.get_config") as mock_get_config:
            mock_config = MagicMock()
            mock_config.credentials = [MagicMock()]
            mock_config.providers = []
            mock_get_config.return_value = mock_config

            async with AsyncClient(app=app, base_url="http://test") as client:
                response = await client.post(
                    "/v2/responses",
                    json={"model": "gpt-4", "input": "hi"},
                )
                assert response.status_code == 401


@pytest.mark.unit
class TestV2Models:
    """Tests for /v2/models endpoint."""

    @pytest.mark.asyncio
    async def test_v2_models_auth_required(self):
        """Test that /v2/models requires authentication."""
        with patch("app.core.config.get_config") as mock_get_config:
            mock_config = MagicMock()
            mock_config.credentials = [MagicMock()]
            mock_config.providers = []
            mock_get_config.return_value = mock_config

            async with AsyncClient(app=app, base_url="http://test") as client:
                response = await client.get("/v2/models")
                assert response.status_code == 401

    @pytest.mark.asyncio
    async def test_v2_models_returns_filtered_models(self):
        """Test that /v2/models returns models filtered by credential permissions."""
        from app.core.database import hash_key

        raw_key = "test-key"
        hashed_key = hash_key(raw_key)

        with patch("app.core.config.get_config") as mock_get_config, patch(
            "app.services.provider_service.get_provider_service"
        ) as mock_get_provider_svc:
            mock_credential = MagicMock()
            mock_credential.credential_key = hashed_key
            mock_credential.name = "test-key"
            mock_credential.allowed_models = ["gpt-4", "gpt-3.5-*"]
            mock_credential.rate_limit = None
            mock_credential.enabled = True

            mock_config = MagicMock()
            mock_config.credentials = [mock_credential]
            mock_config.providers = []
            mock_get_config.return_value = mock_config

            mock_provider_svc = MagicMock()
            mock_provider_svc.get_all_models.return_value = {
                "gpt-4",
                "gpt-3.5-turbo",
                "claude-3-opus",
            }
            mock_get_provider_svc.return_value = mock_provider_svc

            async with AsyncClient(app=app, base_url="http://test") as client:
                response = await client.get(
                    "/v2/models",
                    headers={"Authorization": f"Bearer {raw_key}"},
                )
                assert response.status_code == 200
                data = response.json()
                model_ids = [m["id"] for m in data["data"]]
                # gpt-4 should be included (exact match)
                assert "gpt-4" in model_ids
                # gpt-3.5-turbo should be included (matches gpt-3.5-*)
                assert "gpt-3.5-turbo" in model_ids
                # claude-3-opus should NOT be included
                assert "claude-3-opus" not in model_ids


@pytest.mark.unit
class TestV2ModelInfo:
    """Tests for /v2/model/info endpoint."""

    @pytest.mark.asyncio
    async def test_v2_model_info_auth_required(self):
        """Test that /v2/model/info requires authentication."""
        with patch("app.core.config.get_config") as mock_get_config:
            mock_config = MagicMock()
            mock_config.credentials = [MagicMock()]
            mock_config.providers = []
            mock_get_config.return_value = mock_config

            async with AsyncClient(app=app, base_url="http://test") as client:
                response = await client.get(
                    "/v2/model/info", params={"model": "gpt-4"}
                )
                assert response.status_code == 401


@pytest.mark.unit
class TestV2Completions:
    """Tests for /v2/completions endpoint (legacy)."""

    @pytest.mark.asyncio
    async def test_v2_completions_auth_required(self):
        """Test that /v2/completions requires authentication."""
        with patch("app.core.config.get_config") as mock_get_config:
            mock_config = MagicMock()
            mock_config.credentials = [MagicMock()]
            mock_config.providers = []
            mock_get_config.return_value = mock_config

            async with AsyncClient(app=app, base_url="http://test") as client:
                response = await client.post(
                    "/v2/completions",
                    json={"model": "gpt-3.5-turbo", "prompt": "Hello"},
                )
                assert response.status_code == 401


@pytest.mark.unit
class TestV2CountTokens:
    """Tests for /v2/messages/count_tokens endpoint."""

    @pytest.mark.asyncio
    async def test_v2_count_tokens_auth_required(self):
        """Test that /v2/messages/count_tokens requires authentication."""
        with patch("app.core.config.get_config") as mock_get_config:
            mock_config = MagicMock()
            mock_config.credentials = [MagicMock()]
            mock_config.providers = []
            mock_get_config.return_value = mock_config

            async with AsyncClient(app=app, base_url="http://test") as client:
                response = await client.post(
                    "/v2/messages/count_tokens",
                    json={
                        "model": "claude-3-opus",
                        "messages": [{"role": "user", "content": "hi"}],
                    },
                )
                assert response.status_code == 401
