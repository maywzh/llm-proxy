"""Tests for middleware"""

from unittest.mock import Mock, AsyncMock, patch
import time

import pytest
from fastapi import Request, Response
from starlette.middleware.base import RequestResponseEndpoint

from app.core.middleware import MetricsMiddleware
from app.utils.client import extract_client


@pytest.mark.unit
class TestExtractClient:
    """Test extract_client function"""

    def test_extract_client_empty_ua(self):
        """Test extract_client returns unknown for empty user agent"""
        request = Mock(spec=Request)
        request.headers = {}
        assert extract_client(request) == "unknown"

    def test_extract_client_claude_code(self):
        """Test extract_client identifies Claude Code"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "claude-cli/2.1.25 (external, claude-vscode)"}
        assert extract_client(request) == "claude-code"

    def test_extract_client_kilo_code(self):
        """Test extract_client identifies Kilo-Code"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "Kilo-Code/5.2.2"}
        assert extract_client(request) == "kilo-code"

    def test_extract_client_codex_cli(self):
        """Test extract_client identifies Codex CLI"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "codex_cli_rs/0.89.0 (Mac OS 26.2.0; arm64)"}
        assert extract_client(request) == "codex-cli"

    def test_extract_client_ai_sdk_openai(self):
        """Test extract_client identifies AI SDK OpenAI"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "ai-sdk/openai-compatible/1.0.31 ai-sdk/provider-ut"}
        assert extract_client(request) == "ai-sdk-openai"

    def test_extract_client_openai_js(self):
        """Test extract_client identifies OpenAI JS"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "OpenAI/JS 6.16.0"}
        assert extract_client(request) == "openai-js"

    def test_extract_client_python_httpx(self):
        """Test extract_client identifies python-httpx"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "python-httpx/0.28.1"}
        assert extract_client(request) == "python-httpx"

    def test_extract_client_curl(self):
        """Test extract_client identifies curl"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "curl/8.7.1"}
        assert extract_client(request) == "curl"

    def test_extract_client_browser(self):
        """Test extract_client identifies browser"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)"}
        assert extract_client(request) == "browser"

    def test_extract_client_unknown_returns_first_token(self):
        """Test extract_client returns first token for unknown clients"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "MyCustomClient/1.0.0"}
        assert extract_client(request) == "MyCustomClient"

    def test_extract_client_apifox(self):
        """Test extract_client identifies Apifox"""
        request = Mock(spec=Request)
        request.headers = {"user-agent": "Apifox/1.0.0 (https://apifox.com)"}
        assert extract_client(request) == "apifox"


@pytest.mark.unit
class TestMetricsMiddleware:
    """Test MetricsMiddleware class"""

    @pytest.mark.asyncio
    async def test_middleware_tracks_request(self):
        """Test middleware tracks request metrics"""
        middleware = MetricsMiddleware(app=Mock())

        # Create mock request
        request = Mock(spec=Request)
        request.url.path = "/chat/completions"
        request.method = "POST"
        request.headers = {"user-agent": "test-client/1.0"}
        request.state = Mock()
        request.state.model = "gpt-4"
        request.state.provider = "test-provider"

        # Create mock response
        response = Mock(spec=Response)
        response.status_code = 200

        # Mock call_next
        async def mock_call_next(req):
            return response

        result = await middleware.dispatch(request, mock_call_next)

        assert result == response

    @pytest.mark.asyncio
    async def test_middleware_skips_metrics_endpoint(self):
        """Test middleware skips /metrics endpoint"""
        middleware = MetricsMiddleware(app=Mock())

        request = Mock(spec=Request)
        request.url.path = "/metrics"
        request.method = "GET"
        request.headers = {}

        response = Mock(spec=Response)

        async def mock_call_next(req):
            return response

        result = await middleware.dispatch(request, mock_call_next)

        # Should pass through without tracking
        assert result == response

    @pytest.mark.asyncio
    async def test_middleware_increments_active_requests(self):
        """Test middleware increments and decrements active requests"""
        from app.core.metrics import ACTIVE_REQUESTS

        middleware = MetricsMiddleware(app=Mock())

        request = Mock(spec=Request)
        request.url.path = "/test"
        request.method = "GET"
        request.headers = {"user-agent": "test-client/1.0"}
        request.state = Mock()
        request.state.model = "test"
        request.state.provider = "test"

        response = Mock(spec=Response)
        response.status_code = 200

        # Get initial value
        initial_value = ACTIVE_REQUESTS.labels(endpoint="/test")._value.get()

        async def mock_call_next(req):
            # Check that active requests was incremented
            current_value = ACTIVE_REQUESTS.labels(endpoint="/test")._value.get()
            assert current_value == initial_value + 1
            return response

        await middleware.dispatch(request, mock_call_next)

        # After dispatch, should be back to initial value
        final_value = ACTIVE_REQUESTS.labels(endpoint="/test")._value.get()
        assert final_value == initial_value

    @pytest.mark.asyncio
    async def test_middleware_records_duration(self):
        """Test middleware records request duration"""
        from app.core.metrics import REQUEST_DURATION

        middleware = MetricsMiddleware(app=Mock())

        request = Mock(spec=Request)
        request.url.path = "/test"
        request.method = "POST"
        request.headers = {"user-agent": "test-client/1.0"}
        request.state = Mock()
        request.state.model = "gpt-4"
        request.state.provider = "test"

        response = Mock(spec=Response)
        response.status_code = 200

        async def mock_call_next(req):
            # Simulate some processing time
            await asyncio.sleep(0.1)
            return response

        import asyncio

        await middleware.dispatch(request, mock_call_next)

        # Duration should have been recorded (api_key_name defaults to 'anonymous' in tests)
        metric = REQUEST_DURATION.labels(
            method="POST",
            endpoint="/test",
            model="gpt-4",
            provider="test",
            api_key_name="anonymous",
            client="test-client",
        )
        assert metric._sum.get() > 0

    @pytest.mark.asyncio
    async def test_middleware_handles_missing_state_attributes(self):
        """Test middleware handles missing model/provider in state"""
        middleware = MetricsMiddleware(app=Mock())

        request = Mock(spec=Request)
        request.url.path = "/test"
        request.method = "GET"
        request.headers = {"user-agent": "test-client/1.0"}
        request.state = Mock(spec=[])  # Empty state

        response = Mock(spec=Response)
        response.status_code = 200

        async def mock_call_next(req):
            return response

        # Should not raise error
        result = await middleware.dispatch(request, mock_call_next)
        assert result == response

    @pytest.mark.asyncio
    async def test_middleware_handles_exceptions(self):
        """Test middleware handles exceptions properly"""
        from app.core.metrics import ACTIVE_REQUESTS

        middleware = MetricsMiddleware(app=Mock())

        request = Mock(spec=Request)
        request.url.path = "/test-error"
        request.method = "POST"
        request.headers = {"user-agent": "test-client/1.0"}
        request.state = Mock()

        initial_value = ACTIVE_REQUESTS.labels(endpoint="/test-error")._value.get()

        async def mock_call_next(req):
            raise ValueError("Test error")

        with pytest.raises(ValueError, match="Test error"):
            await middleware.dispatch(request, mock_call_next)

        # Active requests should still be decremented
        final_value = ACTIVE_REQUESTS.labels(endpoint="/test-error")._value.get()
        assert final_value == initial_value

    @pytest.mark.asyncio
    async def test_middleware_records_status_code(self):
        """Test middleware records different status codes"""
        from app.core.metrics import REQUEST_COUNT

        middleware = MetricsMiddleware(app=Mock())

        for status_code in [200, 400, 500]:
            request = Mock(spec=Request)
            request.url.path = "/test"
            request.method = "POST"
            request.headers = {"user-agent": "test-client/1.0"}
            request.state = Mock()
            request.state.model = "gpt-4"
            request.state.provider = "test"

            response = Mock(spec=Response)
            response.status_code = status_code

            async def mock_call_next(req):
                return response

            await middleware.dispatch(request, mock_call_next)

            # Verify status code was recorded (api_key_name defaults to 'anonymous' in tests)
            metric = REQUEST_COUNT.labels(
                method="POST",
                endpoint="/test",
                model="gpt-4",
                provider="test",
                status_code=str(status_code),
                api_key_name="anonymous",
                client="test-client",
            )
            assert metric._value.get() > 0


@pytest.mark.unit
class TestMiddlewareIntegration:
    """Test middleware integration with FastAPI"""

    @pytest.mark.asyncio
    async def test_middleware_with_fastapi_app(self, app_client):
        """Test middleware works with FastAPI application"""
        # The app_client fixture already has middleware configured
        # Just verify it doesn't break basic requests
        response = app_client.get("/health")

        # Should work without errors
        assert response.status_code in [200, 401]  # May require auth

    @pytest.mark.asyncio
    async def test_middleware_timing_accuracy(self):
        """Test middleware timing is reasonably accurate"""
        from app.core.metrics import REQUEST_DURATION

        middleware = MetricsMiddleware(app=Mock())

        request = Mock(spec=Request)
        request.url.path = "/test-timing"
        request.method = "GET"
        request.headers = {"user-agent": "test-client/1.0"}
        request.state = Mock()
        request.state.model = "test"
        request.state.provider = "test"

        response = Mock(spec=Response)
        response.status_code = 200

        sleep_duration = 0.2

        async def mock_call_next(req):
            import asyncio

            await asyncio.sleep(sleep_duration)
            return response

        import asyncio

        start = time.time()
        await middleware.dispatch(request, mock_call_next)
        actual_duration = time.time() - start

        # Recorded duration should be close to actual duration (api_key_name defaults to 'anonymous' in tests)
        metric = REQUEST_DURATION.labels(
            method="GET",
            endpoint="/test-timing",
            model="test",
            provider="test",
            api_key_name="anonymous",
            client="test-client",
        )
        recorded_duration = metric._sum.get()

        # Allow 10% variance
        assert abs(recorded_duration - actual_duration) < actual_duration * 0.1


@pytest.mark.unit
class TestMiddlewareEdgeCases:
    """Test middleware edge cases"""

    @pytest.mark.asyncio
    async def test_middleware_with_very_long_path(self):
        """Test middleware handles very long URL paths"""
        middleware = MetricsMiddleware(app=Mock())

        long_path = "/api/" + "x" * 1000
        request = Mock(spec=Request)
        request.url.path = long_path
        request.method = "GET"
        request.headers = {"user-agent": "test-client/1.0"}
        request.state = Mock()

        response = Mock(spec=Response)
        response.status_code = 200

        async def mock_call_next(req):
            return response

        result = await middleware.dispatch(request, mock_call_next)
        assert result == response

    @pytest.mark.asyncio
    async def test_middleware_with_special_characters_in_path(self):
        """Test middleware handles special characters in path"""
        middleware = MetricsMiddleware(app=Mock())

        request = Mock(spec=Request)
        request.url.path = "/api/test%20path/with-special_chars"
        request.method = "POST"
        request.headers = {"user-agent": "test-client/1.0"}
        request.state = Mock()
        request.state.model = "test"
        request.state.provider = "test"

        response = Mock(spec=Response)
        response.status_code = 200

        async def mock_call_next(req):
            return response

        result = await middleware.dispatch(request, mock_call_next)
        assert result == response

    @pytest.mark.asyncio
    async def test_middleware_concurrent_requests(self):
        """Test middleware handles concurrent requests"""
        from app.core.metrics import ACTIVE_REQUESTS
        import asyncio

        middleware = MetricsMiddleware(app=Mock())

        async def make_request():
            request = Mock(spec=Request)
            request.url.path = "/concurrent"
            request.method = "GET"
            request.headers = {"user-agent": "test-client/1.0"}
            request.state = Mock()
            request.state.model = "test"
            request.state.provider = "test"

            response = Mock(spec=Response)
            response.status_code = 200

            async def mock_call_next(req):
                await asyncio.sleep(0.1)
                return response

            return await middleware.dispatch(request, mock_call_next)

        # Run multiple requests concurrently
        results = await asyncio.gather(*[make_request() for _ in range(5)])

        assert len(results) == 5
        assert all(r.status_code == 200 for r in results)
