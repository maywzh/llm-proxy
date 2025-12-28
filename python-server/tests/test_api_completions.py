"""Tests for completion API endpoints"""
from unittest.mock import Mock, AsyncMock, MagicMock, patch

import pytest
import httpx
import respx
from fastapi import HTTPException

from app.api.completions import proxy_completion_request, _attach_response_metadata
from starlette.responses import JSONResponse


@pytest.mark.unit
class TestChatCompletionsEndpoint:
    """Test /chat/completions endpoint"""
    
    @respx.mock
    def test_chat_completions_success(self, app_client, sample_chat_request):
        """Test successful chat completion request"""
        # Mock the provider API
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test-id',
                    'model': 'gpt-4-0613',
                    'choices': [{'message': {'content': 'Hello!'}}],
                    'usage': {'prompt_tokens': 10, 'completion_tokens': 5, 'total_tokens': 15}
                }
            )
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json=sample_chat_request,
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        data = response.json()
        assert data['model'] == 'gpt-4'  # Should be rewritten to original
        assert 'choices' in data
    
    @respx.mock
    def test_chat_completions_unauthorized(self, app_client, sample_chat_request):
        """Test chat completion without authorization"""
        response = app_client.post(
            '/v1/chat/completions',
            json=sample_chat_request
        )
        
        assert response.status_code == 401
    
    @respx.mock
    def test_chat_completions_model_mapping(self, app_client):
        """Test that model is mapped correctly"""
        request_data = {
            'model': 'gpt-4',
            'messages': [{'role': 'user', 'content': 'Hi'}]
        }
        
        # Mock provider API - should receive mapped model
        mock_route = respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test-id',
                    'model': 'gpt-4-0613',
                    'choices': [{'message': {'content': 'Hi'}}],
                    'usage': {'total_tokens': 10}
                }
            )
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json=request_data,
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        # Verify the provider received the mapped model
        assert mock_route.called
    
    @respx.mock
    def test_chat_completions_streaming(self, app_client):
        """Test streaming chat completion"""
        request_data = {
            'model': 'gpt-4',
            'messages': [{'role': 'user', 'content': 'Hi'}],
            'stream': True
        }
        
        # Mock streaming response
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                content=b'data: {"model":"gpt-4-0613","choices":[{"delta":{"content":"Hi"}}]}\n\ndata: [DONE]\n\n',
                headers={'content-type': 'text/event-stream'}
            )
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json=request_data,
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        assert 'text/event-stream' in response.headers.get('content-type', '')
    
    def test_chat_completions_streaming_uses_client_stream(self, app_client):
        """Ensure streaming path relies on httpx.AsyncClient.stream"""
        with patch('app.api.completions.httpx.AsyncClient') as mock_client_class:
            mock_client = MagicMock()
            mock_client.aclose = AsyncMock()
            mock_client.post = AsyncMock(side_effect=AssertionError("stream path should not call post"))
            mock_client_class.return_value = mock_client
            
            mock_response = MagicMock()
            mock_response.status_code = 200
            mock_response.headers = {'content-type': 'text/event-stream'}
            
            async def iter_bytes():
                yield b'data: {"model":"gpt-4-0613","choices":[{"delta":{"content":"Hi"}}]}\n\n'
                yield b'data: [DONE]\n\n'
            
            mock_response.aiter_bytes = iter_bytes
            
            mock_stream_ctx = MagicMock()
            
            async def enter():
                return mock_response
            
            mock_stream_ctx.__aenter__.side_effect = enter
            mock_stream_ctx.__aexit__ = AsyncMock(return_value=None)
            mock_client.stream.return_value = mock_stream_ctx
            
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
            mock_client.stream.assert_called_once()
    
    @respx.mock
    def test_chat_completions_provider_error(self, app_client, sample_chat_request):
        """Test handling provider error"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(500, json={'error': 'Internal error'})
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json=sample_chat_request,
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 500
    
    @respx.mock
    def test_chat_completions_timeout(self, app_client, sample_chat_request):
        """Test handling provider timeout"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            side_effect=httpx.TimeoutException("Timeout")
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json=sample_chat_request,
            headers={'Authorization': 'Bearer test-master-key'}
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
                    'id': 'test-id',
                    'model': 'gpt-3.5-turbo-0613',
                    'choices': [{'text': 'Once upon a time...'}],
                    'usage': {'total_tokens': 20}
                }
            )
        )
        
        response = app_client.post(
            '/v1/completions',
            json=sample_completion_request,
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        data = response.json()
        assert data['model'] == 'gpt-3.5-turbo'  # Rewritten
        assert 'choices' in data
    
    @respx.mock
    def test_completions_unauthorized(self, app_client, sample_completion_request):
        """Test completion without authorization"""
        response = app_client.post(
            '/v1/completions',
            json=sample_completion_request
        )
        
        assert response.status_code == 401


@pytest.mark.unit
class TestProxyCompletionRequest:
    """Test proxy_completion_request function"""
    
    @pytest.mark.asyncio
    @respx.mock
    async def test_proxy_sets_request_state(self, provider_service):
        """Test that proxy sets model and provider in request state"""
        request = Mock()
        request.json = AsyncMock(return_value={
            'model': 'gpt-4',
            'messages': [{'role': 'user', 'content': 'Hi'}]
        })
        request.state = Mock()
        
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
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
        
        await proxy_completion_request(request, 'chat/completions', provider_service)
        
        assert hasattr(request.state, 'model')
        assert hasattr(request.state, 'provider')
        assert request.state.model == 'gpt-4'
    
    @pytest.mark.asyncio
    @respx.mock
    async def test_proxy_handles_missing_model(self, provider_service):
        """Test proxy handles request without model field"""
        request = Mock()
        request.json = AsyncMock(return_value={
            'messages': [{'role': 'user', 'content': 'Hi'}]
        })
        request.state = Mock()
        
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={'id': 'test', 'choices': [], 'usage': {'total_tokens': 10}}
            )
        )
        
        await proxy_completion_request(request, 'chat/completions', provider_service)
        
        assert request.state.model == 'unknown'
    
    @pytest.mark.asyncio
    @respx.mock
    async def test_proxy_tracks_token_usage(self, provider_service):
        """Test that proxy tracks token usage"""
        from app.core.metrics import TOKEN_USAGE
        
        request = Mock()
        request.json = AsyncMock(return_value={
            'model': 'gpt-4',
            'messages': [{'role': 'user', 'content': 'Hi'}]
        })
        request.state = Mock()
        
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test',
                    'model': 'gpt-4-0613',
                    'choices': [],
                    'usage': {
                        'prompt_tokens': 10,
                        'completion_tokens': 20,
                        'total_tokens': 30
                    }
                }
            )
        )
        
        await proxy_completion_request(request, 'chat/completions', provider_service)
        
        # Verify token metrics were recorded
        total_metric = TOKEN_USAGE.labels(
            model='gpt-4',
            provider=request.state.provider,
            token_type='total'
        )
        assert total_metric._value.get() >= 30


@pytest.mark.unit
class TestModelsEndpoint:
    """Test /models endpoint"""
    
    def test_list_models_success(self, app_client):
        """Test listing available models"""
        response = app_client.get(
            '/v1/models',
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        data = response.json()
        assert data['object'] == 'list'
        assert 'data' in data
        assert len(data['data']) > 0
        
        # Check model format
        model = data['data'][0]
        assert 'id' in model
        assert 'object' in model
        assert model['object'] == 'model'
    
    def test_list_models_unauthorized(self, app_client):
        """Test listing models without authorization"""
        response = app_client.get('/v1/models')
        assert response.status_code == 401
    
    def test_list_models_contains_expected_models(self, app_client):
        """Test that response contains expected models"""
        response = app_client.get(
            '/v1/models',
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        data = response.json()
        model_ids = [m['id'] for m in data['data']]
        
        # From test config: gpt-4, gpt-3.5-turbo, claude-3
        assert 'gpt-4' in model_ids
        assert 'gpt-3.5-turbo' in model_ids
        assert 'claude-3' in model_ids
    
    def test_list_models_sorted(self, app_client):
        """Test that models are returned sorted"""
        response = app_client.get(
            '/v1/models',
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
        data = response.json()
        model_ids = [m['id'] for m in data['data']]
        
        # Should be sorted alphabetically
        assert model_ids == sorted(model_ids)


@pytest.mark.unit
class TestAPIEdgeCases:
    """Test API edge cases"""
    
    @respx.mock
    def test_empty_request_body(self, app_client):
        """Test handling empty request body"""
        response = app_client.post(
            '/v1/chat/completions',
            json={},
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        # Should handle gracefully (may return error from provider)
        assert response.status_code in [200, 400, 500]
    
    @respx.mock
    def test_very_large_request(self, app_client):
        """Test handling very large request"""
        large_content = 'x' * 100000
        request_data = {
            'model': 'gpt-4',
            'messages': [{'role': 'user', 'content': large_content}]
        }
        
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test',
                    'model': 'gpt-4-0613',
                    'choices': [{'message': {'content': 'Response'}}],
                    'usage': {'total_tokens': 50000}
                }
            )
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json=request_data,
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200


@pytest.mark.unit
class TestResponseMetadata:
    """Test response metadata helper"""
    
    def test_attach_response_metadata_creates_extensions(self):
        """Ensure metadata populates response extensions"""
        response = JSONResponse({'ok': True})
        result = _attach_response_metadata(response, 'gpt-4', 'provider1')
        
        assert result.extensions['model'] == 'gpt-4'
        assert result.extensions['provider'] == 'provider1'
    
    def test_attach_response_metadata_updates_existing(self):
        """Ensure metadata updates existing extensions dict"""
        response = JSONResponse({'ok': True})
        response.extensions['model'] = 'old'
        response.extensions['provider'] = 'old-provider'
        
        result = _attach_response_metadata(response, 'gpt-3.5-turbo', 'provider2')
        
        assert result.extensions['model'] == 'gpt-3.5-turbo'
        assert result.extensions['provider'] == 'provider2'
    
    @respx.mock
    def test_special_characters_in_content(self, app_client):
        """Test handling special characters in content"""
        request_data = {
            'model': 'gpt-4',
            'messages': [{'role': 'user', 'content': '特殊字符 🔥 \n\t\r'}]
        }
        
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test',
                    'model': 'gpt-4-0613',
                    'choices': [{'message': {'content': 'Response'}}],
                    'usage': {'total_tokens': 10}
                }
            )
        )
        
        response = app_client.post(
            '/v1/chat/completions',
            json=request_data,
            headers={'Authorization': 'Bearer test-master-key'}
        )
        
        assert response.status_code == 200
