"""Tests for completion API endpoints"""

from unittest.mock import Mock, AsyncMock, MagicMock, patch

import pytest
import httpx
import respx
from fastapi import HTTPException

from app.api.completions import (
    proxy_completion_request,
    _attach_response_metadata,
    _strip_provider_suffix,
)
from starlette.responses import JSONResponse


@pytest.mark.unit
class TestChatCompletionsEndpoint:
    """Test /chat/completions endpoint"""

    @respx.mock
    def test_chat_completions_success(self, app_client, sample_chat_request):
        """Test successful chat completion request"""
        # Mock both provider APIs (provider selection is weighted random)
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test-id",
                    "model": "gpt-4-0613",
                    "choices": [{"message": {"content": "Hello!"}}],
                    "usage": {
                        "prompt_tokens": 10,
                        "completion_tokens": 5,
                        "total_tokens": 15,
                    },
                },
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test-id",
                    "model": "gpt-4-1106-preview",
                    "choices": [{"message": {"content": "Hello!"}}],
                    "usage": {
                        "prompt_tokens": 10,
                        "completion_tokens": 5,
                        "total_tokens": 15,
                    },
                },
            )
        )

        response = app_client.post(
            "/v1/chat/completions",
            json=sample_chat_request,
            headers={"Authorization": "Bearer test-master-key"},
        )

        assert response.status_code == 200
        data = response.json()
        assert data["model"] == "gpt-4"  # Should be rewritten to original
        assert "choices" in data

    @respx.mock
    def test_chat_completions_unauthorized(self, app_client, sample_chat_request):
        """Test chat completion without authorization"""
        response = app_client.post("/v1/chat/completions", json=sample_chat_request)

        assert response.status_code == 401

    @respx.mock
    def test_chat_completions_model_mapping(self, app_client):
        """Test that model is mapped correctly"""
        request_data = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
        }

        # Mock both provider APIs
        mock_route1 = respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test-id",
                    "model": "gpt-4-0613",
                    "choices": [{"message": {"content": "Hi"}}],
                    "usage": {"total_tokens": 10},
                },
            )
        )
        mock_route2 = respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test-id",
                    "model": "gpt-4-1106-preview",
                    "choices": [{"message": {"content": "Hi"}}],
                    "usage": {"total_tokens": 10},
                },
            )
        )

        response = app_client.post(
            "/v1/chat/completions",
            json=request_data,
            headers={"Authorization": "Bearer test-master-key"},
        )

        assert response.status_code == 200
        # Verify one of the providers received the request
        assert mock_route1.called or mock_route2.called

    @respx.mock
    def test_chat_completions_streaming(self, app_client):
        """Test streaming chat completion"""
        request_data = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
            "stream": True,
        }

        # Mock streaming response for both providers (provider selection is weighted random)
        streaming_response = httpx.Response(
            200,
            content=b'data: {"model":"gpt-4-0613","choices":[{"delta":{"content":"Hi"}}]}\n\ndata: [DONE]\n\n',
            headers={"content-type": "text/event-stream"},
        )
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=streaming_response
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                content=b'data: {"model":"gpt-4-1106","choices":[{"delta":{"content":"Hi"}}]}\n\ndata: [DONE]\n\n',
                headers={"content-type": "text/event-stream"},
            )
        )

        response = app_client.post(
            "/v1/chat/completions",
            json=request_data,
            headers={"Authorization": "Bearer test-master-key"},
        )

        assert response.status_code == 200
        assert "text/event-stream" in response.headers.get("content-type", "")

    def test_chat_completions_streaming_uses_client_stream(self, app_client):
        """Ensure streaming path relies on httpx.AsyncClient.stream"""
        with patch("app.api.completions.get_http_client") as mock_get_client:
            mock_client = MagicMock()
            mock_client.aclose = AsyncMock()
            mock_client.post = AsyncMock(
                side_effect=AssertionError("stream path should not call post")
            )
            mock_get_client.return_value = mock_client

            mock_response = MagicMock()
            mock_response.status_code = 200
            mock_response.headers = {"content-type": "text/event-stream"}

            async def iter_bytes():
                yield b'data: {"model":"gpt-4-0613","choices":[{"delta":{"content":"Hi"}}]}\n\n'
                yield b"data: [DONE]\n\n"

            mock_response.aiter_bytes = iter_bytes

            mock_stream_ctx = MagicMock()

            async def enter():
                return mock_response

            mock_stream_ctx.__aenter__.side_effect = enter
            mock_stream_ctx.__aexit__ = AsyncMock(return_value=None)
            mock_client.stream.return_value = mock_stream_ctx

            response = app_client.post(
                "/v1/chat/completions",
                json={
                    "model": "gpt-4",
                    "messages": [{"role": "user", "content": "Hi"}],
                    "stream": True,
                },
                headers={"Authorization": "Bearer test-master-key"},
            )

            assert response.status_code == 200
            mock_client.stream.assert_called_once()

    @respx.mock
    def test_chat_completions_provider_error(self, app_client, sample_chat_request):
        """Test handling provider error"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(500, json={"error": "Internal error"})
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(500, json={"error": "Internal error"})
        )

        response = app_client.post(
            "/v1/chat/completions",
            json=sample_chat_request,
            headers={"Authorization": "Bearer test-master-key"},
        )

        assert response.status_code == 500

    @respx.mock
    def test_chat_completions_timeout(self, app_client, sample_chat_request):
        """Test handling provider timeout"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            side_effect=httpx.TimeoutException("Timeout")
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            side_effect=httpx.TimeoutException("Timeout")
        )

        response = app_client.post(
            "/v1/chat/completions",
            json=sample_chat_request,
            headers={"Authorization": "Bearer test-master-key"},
        )

        assert response.status_code == 504


@pytest.mark.unit
class TestCompletionsEndpoint:
    """Test /completions endpoint"""

    @respx.mock
    def test_completions_success(self, app_client, sample_completion_request):
        """Test successful completion request"""
        respx.post("https://api.provider1.com/v1/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test-id",
                    "model": "gpt-3.5-turbo-0613",
                    "choices": [{"text": "Once upon a time..."}],
                    "usage": {"total_tokens": 20},
                },
            )
        )
        respx.post("https://api.provider2.com/v1/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test-id",
                    "model": "gpt-3.5-turbo-0613",
                    "choices": [{"text": "Once upon a time..."}],
                    "usage": {"total_tokens": 20},
                },
            )
        )

        response = app_client.post(
            "/v1/completions",
            json=sample_completion_request,
            headers={"Authorization": "Bearer test-master-key"},
        )

        assert response.status_code == 200
        data = response.json()
        assert data["model"] == "gpt-3.5-turbo"  # Rewritten
        assert "choices" in data

    @respx.mock
    def test_completions_unauthorized(self, app_client, sample_completion_request):
        """Test completion without authorization"""
        response = app_client.post("/v1/completions", json=sample_completion_request)

        assert response.status_code == 401


@pytest.mark.unit
class TestProxyCompletionRequest:
    """Test proxy_completion_request function"""

    @pytest.mark.asyncio
    @respx.mock
    async def test_proxy_sets_request_state(self, provider_service):
        """Test that proxy sets model and provider in request state"""
        request = Mock()
        request.json = AsyncMock(
            return_value={
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hi"}],
            }
        )
        request.state = Mock()

        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-0613",
                    "choices": [],
                    "usage": {"total_tokens": 10},
                },
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-1106-preview",
                    "choices": [],
                    "usage": {"total_tokens": 10},
                },
            )
        )

        await proxy_completion_request(request, "chat/completions", provider_service)

        assert hasattr(request.state, "model")
        assert hasattr(request.state, "provider")
        assert request.state.model == "gpt-4"

    @pytest.mark.asyncio
    @respx.mock
    async def test_proxy_handles_missing_model(self, provider_service):
        """Test proxy handles request without model field"""
        request = Mock()
        request.json = AsyncMock(
            return_value={"messages": [{"role": "user", "content": "Hi"}]}
        )
        request.state = Mock()

        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200, json={"id": "test", "choices": [], "usage": {"total_tokens": 10}}
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200, json={"id": "test", "choices": [], "usage": {"total_tokens": 10}}
            )
        )

        await proxy_completion_request(request, "chat/completions", provider_service)

        assert request.state.model == "unknown"

    @pytest.mark.asyncio
    @respx.mock
    async def test_proxy_tracks_token_usage(self, provider_service):
        """Test that proxy tracks token usage"""
        from app.core.metrics import TOKEN_USAGE

        request = Mock()
        request.json = AsyncMock(
            return_value={
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hi"}],
            }
        )
        request.state = Mock()

        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-0613",
                    "choices": [],
                    "usage": {
                        "prompt_tokens": 10,
                        "completion_tokens": 20,
                        "total_tokens": 30,
                    },
                },
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-1106",
                    "choices": [],
                    "usage": {
                        "prompt_tokens": 10,
                        "completion_tokens": 20,
                        "total_tokens": 30,
                    },
                },
            )
        )

        await proxy_completion_request(request, "chat/completions", provider_service)

        # Verify token metrics were recorded - check for any provider
        # Since provider selection is random, we check that at least one provider recorded tokens
        # The api_key_name is "anonymous" when no auth context is set
        provider1_metric = TOKEN_USAGE.labels(
            model="gpt-4",
            provider="provider1",
            token_type="total",
            api_key_name="anonymous",
        )
        provider2_metric = TOKEN_USAGE.labels(
            model="gpt-4",
            provider="provider2",
            token_type="total",
            api_key_name="anonymous",
        )
        assert (
            provider1_metric._value.get() >= 30 or provider2_metric._value.get() >= 30
        )


@pytest.mark.unit
class TestModelsEndpoint:
    """Test /models endpoint"""

    def test_list_models_success(self, app_client):
        """Test listing available models"""
        response = app_client.get(
            "/v1/models", headers={"Authorization": "Bearer test-master-key"}
        )

        assert response.status_code == 200
        data = response.json()
        assert data["object"] == "list"
        assert "data" in data
        assert len(data["data"]) > 0

        # Check model format
        model = data["data"][0]
        assert "id" in model
        assert "object" in model
        assert model["object"] == "model"

    def test_list_models_unauthorized(self, app_client):
        """Test listing models without authorization"""
        response = app_client.get("/v1/models")
        assert response.status_code == 401

    def test_list_models_contains_expected_models(self, app_client):
        """Test that response contains expected models"""
        response = app_client.get(
            "/v1/models", headers={"Authorization": "Bearer test-master-key"}
        )

        assert response.status_code == 200
        data = response.json()
        model_ids = [m["id"] for m in data["data"]]

        # From test config: gpt-4, gpt-3.5-turbo, claude-3
        assert "gpt-4" in model_ids
        assert "gpt-3.5-turbo" in model_ids
        assert "claude-3" in model_ids

    def test_list_models_sorted(self, app_client):
        """Test that models are returned sorted"""
        response = app_client.get(
            "/v1/models", headers={"Authorization": "Bearer test-master-key"}
        )

        assert response.status_code == 200
        data = response.json()
        model_ids = [m["id"] for m in data["data"]]

        # Should be sorted alphabetically
        assert model_ids == sorted(model_ids)


@pytest.mark.unit
class TestAPIEdgeCases:
    """Test API edge cases"""

    @respx.mock
    def test_empty_request_body(self, app_client):
        """Test handling empty request body"""
        response = app_client.post(
            "/v1/chat/completions",
            json={},
            headers={"Authorization": "Bearer test-master-key"},
        )

        # Should handle gracefully (may return error from provider)
        assert response.status_code in [200, 400, 500]

    @respx.mock
    def test_very_large_request(self, app_client):
        """Test handling very large request"""
        large_content = "x" * 100000
        request_data = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": large_content}],
        }

        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-0613",
                    "choices": [{"message": {"content": "Response"}}],
                    "usage": {"total_tokens": 50000},
                },
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-1106",
                    "choices": [{"message": {"content": "Response"}}],
                    "usage": {"total_tokens": 50000},
                },
            )
        )

        response = app_client.post(
            "/v1/chat/completions",
            json=request_data,
            headers={"Authorization": "Bearer test-master-key"},
        )

        assert response.status_code == 200


@pytest.mark.unit
class TestResponseMetadata:
    """Test response metadata helper"""

    def test_attach_response_metadata_creates_extensions(self):
        """Ensure metadata populates response extensions"""
        response = JSONResponse({"ok": True})
        result = _attach_response_metadata(response, "gpt-4", "provider1")

        assert result.extensions["model"] == "gpt-4"
        assert result.extensions["provider"] == "provider1"

    def test_attach_response_metadata_updates_existing(self):
        """Ensure metadata updates existing extensions dict"""
        response = JSONResponse({"ok": True})
        # Initialize extensions dict first (JSONResponse doesn't have it by default)
        response.extensions = {"model": "old", "provider": "old-provider"}

        result = _attach_response_metadata(response, "gpt-3.5-turbo", "provider2")

        assert result.extensions["model"] == "gpt-3.5-turbo"
        assert result.extensions["provider"] == "provider2"

    @respx.mock
    def test_special_characters_in_content(self, app_client):
        """Test handling special characters in content"""
        request_data = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "特殊字符 🔥 \n\t\r"}],
        }

        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-0613",
                    "choices": [{"message": {"content": "Response"}}],
                    "usage": {"total_tokens": 10},
                },
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-1106",
                    "choices": [{"message": {"content": "Response"}}],
                    "usage": {"total_tokens": 10},
                },
            )
        )

        response = app_client.post(
            "/v1/chat/completions",
            json=request_data,
            headers={"Authorization": "Bearer test-master-key"},
        )

        assert response.status_code == 200


@pytest.mark.unit
class TestStripProviderSuffix:
    """Test _strip_provider_suffix function"""

    def test_strip_provider_suffix_with_matching_prefix(self, monkeypatch):
        """Test stripping provider suffix when prefix matches"""
        import app.api.completions as completions_module

        # Create a mock EnvConfig with provider_suffix set
        mock_env_config = Mock()
        mock_env_config.provider_suffix = "Proxy"
        monkeypatch.setattr(
            completions_module, "get_env_config", lambda: mock_env_config
        )

        result = _strip_provider_suffix("Proxy/gpt-4")
        assert result == "gpt-4"

    def test_strip_provider_suffix_without_prefix(self, monkeypatch):
        """Test that model without prefix is returned unchanged"""
        import app.api.completions as completions_module

        mock_env_config = Mock()
        mock_env_config.provider_suffix = "Proxy"
        monkeypatch.setattr(
            completions_module, "get_env_config", lambda: mock_env_config
        )

        result = _strip_provider_suffix("gpt-4")
        assert result == "gpt-4"

    def test_strip_provider_suffix_with_different_prefix(self, monkeypatch):
        """Test that model with different prefix is returned unchanged"""
        import app.api.completions as completions_module

        mock_env_config = Mock()
        mock_env_config.provider_suffix = "Proxy"
        monkeypatch.setattr(
            completions_module, "get_env_config", lambda: mock_env_config
        )

        result = _strip_provider_suffix("Other/gpt-4")
        assert result == "Other/gpt-4"

    def test_strip_provider_suffix_no_suffix_configured(self, monkeypatch):
        """Test that model is returned unchanged when no suffix is configured"""
        import app.api.completions as completions_module

        mock_env_config = Mock()
        mock_env_config.provider_suffix = None
        monkeypatch.setattr(
            completions_module, "get_env_config", lambda: mock_env_config
        )

        result = _strip_provider_suffix("Proxy/gpt-4")
        assert result == "Proxy/gpt-4"

    def test_strip_provider_suffix_with_multiple_slashes(self, monkeypatch):
        """Test stripping prefix when model name contains multiple slashes"""
        import app.api.completions as completions_module

        mock_env_config = Mock()
        mock_env_config.provider_suffix = "Proxy"
        monkeypatch.setattr(
            completions_module, "get_env_config", lambda: mock_env_config
        )

        result = _strip_provider_suffix("Proxy/org/model-name")
        assert result == "org/model-name"

    def test_strip_provider_suffix_empty_suffix(self, monkeypatch):
        """Test that empty suffix is treated as no suffix"""
        import app.api.completions as completions_module

        mock_env_config = Mock()
        mock_env_config.provider_suffix = ""
        monkeypatch.setattr(
            completions_module, "get_env_config", lambda: mock_env_config
        )

        result = _strip_provider_suffix("Proxy/gpt-4")
        assert result == "Proxy/gpt-4"
