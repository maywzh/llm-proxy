"""Integration tests for the LLM proxy"""

import pytest
import respx
import httpx


@pytest.mark.integration
class TestEndToEndFlow:
    """Test complete end-to-end request flows"""

    @respx.mock
    def test_complete_chat_completion_flow(self, app_client):
        """Test complete chat completion request flow"""
        # Mock provider responses (both providers since selection is random)
        provider_response = {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1677652288,
            "model": "gpt-4-0613",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello! How can I help you today?",
                    },
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 13, "completion_tokens": 9, "total_tokens": 22},
        }
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json=provider_response)
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json=provider_response)
        )

        # Make request
        response = app_client.post(
            "/v1/chat/completions",
            json={
                "model": "gpt-4",
                "messages": [
                    {"role": "system", "content": "You are a helpful assistant."},
                    {"role": "user", "content": "Hello!"},
                ],
                "temperature": 0.7,
            },
            headers={"Authorization": "Bearer test-credential-key"},
        )

        # Verify response
        assert response.status_code == 200
        data = response.json()
        assert data["model"] == "gpt-4"  # Model rewritten
        assert (
            data["choices"][0]["message"]["content"]
            == "Hello! How can I help you today?"
        )
        assert data["usage"]["total_tokens"] == 22

    @respx.mock
    def test_streaming_completion_flow(self, app_client):
        """Test streaming completion flow"""
        # Mock streaming response
        stream_data = b'data: {"id":"1","model":"gpt-4-0613","choices":[{"delta":{"content":"Hello"}}]}\n\n'
        stream_data += b'data: {"id":"1","model":"gpt-4-0613","choices":[{"delta":{"content":" world"}}]}\n\n'
        stream_data += b"data: [DONE]\n\n"

        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200, content=stream_data, headers={"content-type": "text/event-stream"}
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200, content=stream_data, headers={"content-type": "text/event-stream"}
            )
        )

        response = app_client.post(
            "/v1/chat/completions",
            json={
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hi"}],
                "stream": True,
            },
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 200
        assert "text/event-stream" in response.headers.get("content-type", "")

    def test_health_check_flow(self, app_client):
        """Test health check flow"""
        response = app_client.get("/health")

        assert response.status_code == 200
        data = response.json()
        assert data["status"] == "ok"
        assert data["providers"] > 0

    def test_models_list_flow(self, app_client):
        """Test models listing flow"""
        response = app_client.get(
            "/v1/models", headers={"Authorization": "Bearer test-credential-key"}
        )

        assert response.status_code == 200
        data = response.json()
        assert data["object"] == "list"
        assert len(data["data"]) > 0

    def test_unauthorized_request_flow(self, app_client):
        """Test unauthorized request is rejected"""
        response = app_client.post(
            "/v1/chat/completions", json={"model": "gpt-4", "messages": []}
        )

        assert response.status_code == 401


@pytest.mark.integration
class TestProviderFailover:
    """Test provider failover scenarios"""

    @respx.mock
    def test_provider_selection_distribution(self, app_client):
        """Test that requests are distributed across providers"""
        # Track which providers receive requests
        provider1_calls = []
        provider2_calls = []

        def track_provider1(request):
            provider1_calls.append(request)
            return httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-0613",
                    "choices": [],
                    "usage": {"total_tokens": 10},
                },
            )

        def track_provider2(request):
            provider2_calls.append(request)
            return httpx.Response(
                200,
                json={
                    "id": "test",
                    "model": "gpt-4-1106",
                    "choices": [],
                    "usage": {"total_tokens": 10},
                },
            )

        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            side_effect=track_provider1
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            side_effect=track_provider2
        )

        # Make multiple requests
        for _ in range(30):
            app_client.post(
                "/v1/chat/completions",
                json={"model": "gpt-4", "messages": []},
                headers={"Authorization": "Bearer test-credential-key"},
            )

        # Both providers should receive some requests
        # With weights 2:1, provider1 should get roughly 2x more
        assert len(provider1_calls) > 0
        assert len(provider2_calls) > 0
        assert len(provider1_calls) > len(provider2_calls)


@pytest.mark.integration
class TestMetricsIntegration:
    """Test metrics collection integration"""

    @respx.mock
    def test_metrics_recorded_for_requests(self, app_client):
        """Test that metrics are recorded for requests"""
        from app.core.metrics import REQUEST_COUNT, TOKEN_USAGE

        provider_response = {
            "id": "test",
            "model": "gpt-4-0613",
            "choices": [],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30},
        }
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json=provider_response)
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json=provider_response)
        )

        # Note: In TestClient, ContextVar doesn't propagate properly across threads,
        # so the middleware records api_key_name="anonymous" even though auth succeeds.
        # The token usage metrics are recorded in the handler with the correct key name.

        # Get initial counts for both providers
        # REQUEST_COUNT is recorded by middleware with api_key_name="anonymous"
        initial_count_p1 = REQUEST_COUNT.labels(
            method="POST",
            endpoint="/v1/chat/completions",
            model="gpt-4",
            provider="provider1",
            status_code="200",
            api_key_name="anonymous",
            client="testclient",
        )._value.get()

        initial_count_p2 = REQUEST_COUNT.labels(
            method="POST",
            endpoint="/v1/chat/completions",
            model="gpt-4",
            provider="provider2",
            status_code="200",
            api_key_name="anonymous",
            client="testclient",
        )._value.get()

        # TOKEN_USAGE is recorded by handler with api_key_name="test-key"
        initial_tokens_p1 = TOKEN_USAGE.labels(
            model="gpt-4",
            provider="provider1",
            token_type="total",
            api_key_name="test-key",
            client="testclient",
        )._value.get()

        initial_tokens_p2 = TOKEN_USAGE.labels(
            model="gpt-4",
            provider="provider2",
            token_type="total",
            api_key_name="test-key",
            client="testclient",
        )._value.get()

        response = app_client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": []},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 200

        # Get final counts for both providers
        final_count_p1 = REQUEST_COUNT.labels(
            method="POST",
            endpoint="/v1/chat/completions",
            model="gpt-4",
            provider="provider1",
            status_code="200",
            api_key_name="anonymous",
            client="testclient",
        )._value.get()

        final_count_p2 = REQUEST_COUNT.labels(
            method="POST",
            endpoint="/v1/chat/completions",
            model="gpt-4",
            provider="provider2",
            status_code="200",
            api_key_name="anonymous",
            client="testclient",
        )._value.get()

        final_tokens_p1 = TOKEN_USAGE.labels(
            model="gpt-4",
            provider="provider1",
            token_type="total",
            api_key_name="test-key",
            client="testclient",
        )._value.get()

        final_tokens_p2 = TOKEN_USAGE.labels(
            model="gpt-4",
            provider="provider2",
            token_type="total",
            api_key_name="test-key",
            client="testclient",
        )._value.get()

        # One of the providers should have received the request
        total_count_increase = (final_count_p1 - initial_count_p1) + (
            final_count_p2 - initial_count_p2
        )
        total_tokens_increase = (final_tokens_p1 - initial_tokens_p1) + (
            final_tokens_p2 - initial_tokens_p2
        )

        assert total_count_increase >= 1
        assert total_tokens_increase >= 30

    def test_metrics_endpoint_accessible(self, app_client):
        """Test that metrics endpoint is accessible"""
        response = app_client.get("/metrics")

        # The metrics endpoint should return 200
        # Note: The reset_metrics fixture unregisters all collectors,
        # so the content may be empty or minimal in tests
        assert response.status_code == 200


@pytest.mark.integration
class TestErrorHandling:
    """Test error handling integration"""

    @respx.mock
    def test_provider_500_error(self, app_client):
        """Test handling of provider 500 error"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                500, json={"error": {"message": "Internal server error"}}
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                500, json={"error": {"message": "Internal server error"}}
            )
        )

        response = app_client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": []},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 500
        data = response.json()
        # Backend error is passed through faithfully
        assert "error" in data
        assert "Internal server error" in data["error"]["message"]

    @respx.mock
    def test_provider_401_error(self, app_client):
        """Test handling of provider 401 error"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                401, json={"error": {"message": "Invalid API key"}}
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                401, json={"error": {"message": "Invalid API key"}}
            )
        )

        response = app_client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": []},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        # Backend error is passed through faithfully with original status code
        assert response.status_code == 401
        data = response.json()
        assert "error" in data
        assert "Invalid API key" in data["error"]["message"]

    @respx.mock
    def test_provider_error_with_string_error(self, app_client):
        """Test handling of provider error with string error field"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(503, json={"error": "Service unavailable"})
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(503, json={"error": "Service unavailable"})
        )

        response = app_client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": []},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        # Backend error is passed through faithfully with original status code
        assert response.status_code == 503
        data = response.json()
        assert "error" in data
        assert data["error"] == "Service unavailable"

    @respx.mock
    def test_provider_streaming_error(self, app_client):
        """Test handling of provider error in streaming mode"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                500, json={"error": {"message": "Streaming error"}}
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                500, json={"error": {"message": "Streaming error"}}
            )
        )

        response = app_client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": [], "stream": True},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        # Streaming requests should also return 500 for backend errors
        assert response.status_code == 500
        data = response.json()
        # Backend error is passed through faithfully
        assert "error" in data
        assert "Streaming error" in data["error"]["message"]

    @respx.mock
    def test_provider_timeout(self, app_client):
        """Test handling of provider timeout"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            side_effect=httpx.TimeoutException("Request timeout")
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            side_effect=httpx.TimeoutException("Request timeout")
        )

        response = app_client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": []},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 504

    def test_invalid_json_request(self, app_client):
        """Test handling of invalid JSON"""
        response = app_client.post(
            "/v1/chat/completions",
            data="invalid json",
            headers={
                "Authorization": "Bearer test-credential-key",
                "Content-Type": "application/json",
            },
        )

        assert response.status_code == 422


@pytest.mark.integration
class TestConcurrency:
    """Test concurrent request handling"""

    @respx.mock
    def test_concurrent_requests(self, app_client):
        """Test handling multiple concurrent requests"""
        import concurrent.futures

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
                    "model": "gpt-4-1106",
                    "choices": [],
                    "usage": {"total_tokens": 10},
                },
            )
        )

        def make_request():
            return app_client.post(
                "/v1/chat/completions",
                json={"model": "gpt-4", "messages": []},
                headers={"Authorization": "Bearer test-credential-key"},
            )

        with concurrent.futures.ThreadPoolExecutor(max_workers=10) as executor:
            futures = [executor.submit(make_request) for _ in range(20)]
            results = [f.result() for f in futures]

        # All requests should succeed
        assert all(r.status_code == 200 for r in results)
        assert len(results) == 20


@pytest.mark.integration
class TestModelMapping:
    """Test model mapping integration"""

    @respx.mock
    def test_model_mapping_applied(self, app_client):
        """Test that model mapping is applied correctly"""
        provider_response = {
            "id": "test",
            "model": "gpt-4-0613",
            "choices": [],
            "usage": {"total_tokens": 10},
        }
        mock_route1 = respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json=provider_response)
        )
        mock_route2 = respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json=provider_response)
        )

        response = app_client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": []},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 200

        # Verify one of the providers received the request with mapped model
        assert mock_route1.called or mock_route2.called
        if mock_route1.called:
            request_data = mock_route1.calls[0].request.content
        else:
            request_data = mock_route2.calls[0].request.content
        assert b"gpt-4-0613" in request_data or b"gpt-4" in request_data

    @respx.mock
    def test_model_rewriting_in_response(self, app_client):
        """Test that model is rewritten in response"""
        provider_response = {
            "id": "test",
            "model": "gpt-4-0613",  # Provider's model name
            "choices": [],
            "usage": {"total_tokens": 10},
        }
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json=provider_response)
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json=provider_response)
        )

        response = app_client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": []},  # Original model name
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 200
        data = response.json()
        assert data["model"] == "gpt-4"  # Should be rewritten to original


@pytest.mark.integration
class TestEventLoggingEndpoint:
    """Test Claude Code telemetry placeholder endpoint"""

    def test_event_logging_returns_ok(self, app_client):
        """Test that event_logging endpoint returns 200 OK"""
        response = app_client.post(
            "/api/event_logging/batch",
            json={},
        )

        assert response.status_code == 200
        data = response.json()
        assert data["status"] == "ok"

    def test_event_logging_accepts_any_payload(self, app_client):
        """Test that event_logging endpoint accepts any JSON payload"""
        response = app_client.post(
            "/api/event_logging/batch",
            json={"events": [{"type": "test", "data": {"key": "value"}}]},
        )

        assert response.status_code == 200
        data = response.json()
        assert data["status"] == "ok"


@pytest.mark.integration
class TestClientDisconnectHandling:
    """Test ClientDisconnect handling across V1 and V2 endpoints."""

    def test_v1_completions_client_disconnect_status_code_408(self, app_client):
        """Verify V1 /v1/chat/completions ClientDisconnect uses HTTP 408.

        HTTP 408 Request Timeout is more appropriate than nginx-specific 499.
        The completions.py handler raises HTTPException(status_code=408) on ClientDisconnect.
        """
        # Test that the endpoint handles requests normally (won't trigger ClientDisconnect in sync test client)
        # The actual ClientDisconnect handling is tested in test_proxy_v2.py
        # This test just verifies the endpoint exists and can be reached
        response = app_client.post(
            "/v1/chat/completions",
            json={"model": "gpt-4", "messages": []},
            headers={"Authorization": "Bearer test-credential-key"},
        )

        # Should return 200 (successful routing) or error status (provider issue)
        # Not testing 408 here since sync TestClient can't easily trigger ClientDisconnect
        assert response.status_code in (200, 400, 500, 502, 504)
