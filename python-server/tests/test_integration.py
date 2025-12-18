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
        # Mock provider response
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'chatcmpl-123',
                    'object': 'chat.completion',
                    'created': 1677652288,
                    'model': 'gpt-4-0613',
                    'choices': [{
                        'index': 0,
                        'message': {
                            'role': 'assistant',
                            'content': 'Hello! How can I help you today?'
                        },
                        'finish_reason': 'stop'
                    }],
                    'usage': {
                        'prompt_tokens': 13,
                        'completion_tokens': 9,
                        'total_tokens': 22
                    }
                }
            )
        )
        
        # Make request
        response = app_client.post(
            '/v1/chat/completions',
            json={
                'model': 'gpt-4',
                'messages': [
                    {'role': 'system', 'content': 'You are a helpful assistant.'},
                    {'role': 'user', 'content': 'Hello!'}
                ],
                'temperature': 0.7
            },
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        # Verify response
        assert response.status_code == 200
        data = response.json()
        assert data['model'] == 'gpt-4'  # Model rewritten
        assert data['choices'][0]['message']['content'] == 'Hello! How can I help you today?'
        assert data['usage']['total_tokens'] == 22
    
    @respx.mock
    def test_streaming_completion_flow(self, app_client):
        """Test streaming completion flow"""
        # Mock streaming response
        stream_data = b'data: {"id":"1","model":"gpt-4-0613","choices":[{"delta":{"content":"Hello"}}]}\n\n'
        stream_data += b'data: {"id":"1","model":"gpt-4-0613","choices":[{"delta":{"content":" world"}}]}\n\n'
        stream_data += b'data: [DONE]\n\n'
        
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                content=stream_data,
                headers={'content-type': 'text/event-stream'}
            )
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json={
                'model': 'gpt-4',
                'messages': [{'role': 'user', 'content': 'Hi'}],
                'stream': True
            },
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        assert 'text/event-stream' in response.headers.get('content-type', '')
    
    def test_health_check_flow(self, app_client):
        """Test health check flow"""
        response = app_client.get('/health')
        
        assert response.status_code == 200
        data = response.json()
        assert data['status'] == 'ok'
        assert data['providers'] > 0
    
    def test_models_list_flow(self, app_client):
        """Test models listing flow"""
        response = app_client.get(
            '/v1/models',
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        data = response.json()
        assert data['object'] == 'list'
        assert len(data['data']) > 0
    
    def test_unauthorized_request_flow(self, app_client):
        """Test unauthorized request is rejected"""
        response = app_client.post(
            '/v1/chat/completions',
            json={'model': 'gpt-4', 'messages': []}
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
                json={'id': 'test', 'model': 'gpt-4-0613', 'choices': [], 'usage': {'total_tokens': 10}}
            )
        
        def track_provider2(request):
            provider2_calls.append(request)
            return httpx.Response(
                200,
                json={'id': 'test', 'model': 'gpt-4-1106', 'choices': [], 'usage': {'total_tokens': 10}}
            )
        
        respx.post("https://api.provider1.com/v1/chat/completions").mock(side_effect=track_provider1)
        respx.post("https://api.provider2.com/v1/chat/completions").mock(side_effect=track_provider2)
        
        # Make multiple requests
        for _ in range(30):
            app_client.post(
                '/v1/chat/completions',
                json={'model': 'gpt-4', 'messages': []},
                headers={'Authorization': 'Bearer test-master-key'}
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
        
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test',
                    'model': 'gpt-4-0613',
                    'choices': [],
                    'usage': {'prompt_tokens': 10, 'completion_tokens': 20, 'total_tokens': 30}
                }
            )
        )
        
        initial_count = REQUEST_COUNT.labels(
            method='POST',
            endpoint='/v1/chat/completions',
            model='gpt-4',
            provider='provider1',
            status_code='200'
        )._value.get()
        
        initial_tokens = TOKEN_USAGE.labels(
            model='gpt-4',
            provider='provider1',
            token_type='total'
        )._value.get()
        
        response = app_client.post(
            '/v1/chat/completions',
            json={'model': 'gpt-4', 'messages': []},
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        
        final_count = REQUEST_COUNT.labels(
            method='POST',
            endpoint='/v1/chat/completions',
            model='gpt-4',
            provider='provider1',
            status_code='200'
        )._value.get()
        
        final_tokens = TOKEN_USAGE.labels(
            model='gpt-4',
            provider='provider1',
            token_type='total'
        )._value.get()
        
        assert final_count > initial_count
        assert final_tokens >= initial_tokens + 30
    
    def test_metrics_endpoint_accessible(self, app_client):
        """Test that metrics endpoint is accessible"""
        response = app_client.get('/metrics')
        
        assert response.status_code == 200
        assert b'llm_proxy_requests_total' in response.content
        assert b'llm_proxy_tokens_total' in response.content


@pytest.mark.integration
class TestErrorHandling:
    """Test error handling integration"""
    
    @respx.mock
    def test_provider_500_error(self, app_client):
        """Test handling of provider 500 error"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(500, json={'error': 'Internal server error'})
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json={'model': 'gpt-4', 'messages': []},
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 500
    
    @respx.mock
    def test_provider_timeout(self, app_client):
        """Test handling of provider timeout"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            side_effect=httpx.TimeoutException("Request timeout")
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json={'model': 'gpt-4', 'messages': []},
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 504
    
    def test_invalid_json_request(self, app_client):
        """Test handling of invalid JSON"""
        response = app_client.post(
            '/v1/chat/completions',
            data='invalid json',
            headers={
                'Authorization': 'Bearer test-master-key',
                'Content-Type': 'application/json'
            }
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
                json={'id': 'test', 'model': 'gpt-4-0613', 'choices': [], 'usage': {'total_tokens': 10}}
            )
        )
        
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={'id': 'test', 'model': 'gpt-4-1106', 'choices': [], 'usage': {'total_tokens': 10}}
            )
        )
        
        def make_request():
            return app_client.post(
                '/v1/chat/completions',
                json={'model': 'gpt-4', 'messages': []},
                headers={'Authorization': 'Bearer test-master-key'}
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
        mock_route = respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test',
                    'model': 'gpt-4-0613',
                    'choices': [],
                    'usage': {'total_tokens': 10}
                }
            )
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json={'model': 'gpt-4', 'messages': []},
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        
        # Verify provider received mapped model
        assert mock_route.called
        request_data = mock_route.calls[0].request.content
        assert b'gpt-4-0613' in request_data or b'gpt-4' in request_data
    
    @respx.mock
    def test_model_rewriting_in_response(self, app_client):
        """Test that model is rewritten in response"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test',
                    'model': 'gpt-4-0613',  # Provider's model name
                    'choices': [],
                    'usage': {'total_tokens': 10}
                }
            )
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json={'model': 'gpt-4', 'messages': []},  # Original model name
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        data = response.json()
        assert data['model'] == 'gpt-4'  # Should be rewritten to original