"""Test RemoteProtocolError handling"""
import pytest
import httpx
from unittest.mock import AsyncMock, MagicMock, patch
from fastapi.testclient import TestClient

from app.main import app


@pytest.mark.asyncio
async def test_remote_protocol_error_non_streaming():
    """Test that RemoteProtocolError is properly handled for non-streaming requests"""
    
    with patch('app.api.completions.httpx.AsyncClient') as mock_client_class:
        mock_client = AsyncMock()
        mock_client_class.return_value.__aenter__.return_value = mock_client
        
        # Simulate RemoteProtocolError
        mock_client.post.side_effect = httpx.RemoteProtocolError(
            "peer closed connection without sending complete message body"
        )
        
        client = TestClient(app)
        response = client.post(
            '/v1/chat/completions',
            json={'model': 'gpt-4', 'messages': [{'role': 'user', 'content': 'test'}]},
            headers={'Authorization': 'Bearer test-key'}
        )
        
        # Should return 502 Bad Gateway
        assert response.status_code == 502
        assert 'connection closed unexpectedly' in response.json()['detail'].lower()


@pytest.mark.asyncio
async def test_remote_protocol_error_streaming():
    """Test that RemoteProtocolError is logged during streaming"""
    
    with patch('app.api.completions.httpx.AsyncClient') as mock_client_class:
        mock_client = AsyncMock()
        mock_client_class.return_value.__aenter__.return_value = mock_client
        
        # Create a mock response that raises RemoteProtocolError during iteration
        mock_response = MagicMock()
        mock_response.status_code = 200
        
        async def failing_iter():
            yield b'data: {"id": "test"}\n\n'
            raise httpx.RemoteProtocolError("Connection closed during streaming")
        
        mock_response.aiter_bytes = failing_iter
        mock_client.post.return_value = mock_response
        
        client = TestClient(app)
        
        # For streaming, the initial request succeeds but stream fails
        with client.stream(
            'POST',
            '/v1/chat/completions',
            json={
                'model': 'gpt-4',
                'messages': [{'role': 'user', 'content': 'test'}],
                'stream': True
            },
            headers={'Authorization': 'Bearer test-key'}
        ) as response:
            # Initial response should be 200
            assert response.status_code == 200
            
            # But reading the stream will encounter the error
            chunks = []
            try:
                for chunk in response.iter_bytes():
                    chunks.append(chunk)
            except Exception:
                # Stream may be incomplete
                pass
            
            # Should have received at least the first chunk
            assert len(chunks) >= 1


@pytest.mark.asyncio  
async def test_other_network_errors_handled():
    """Test that other network errors are also properly handled"""
    
    with patch('app.api.completions.httpx.AsyncClient') as mock_client_class:
        mock_client = AsyncMock()
        mock_client_class.return_value.__aenter__.return_value = mock_client
        
        # Test ConnectError
        mock_client.post.side_effect = httpx.ConnectError("Connection refused")
        
        client = TestClient(app)
        response = client.post(
            '/v1/chat/completions',
            json={'model': 'gpt-4', 'messages': [{'role': 'user', 'content': 'test'}]},
            headers={'Authorization': 'Bearer test-key'}
        )
        
        # Should return 502
        assert response.status_code == 502
        assert 'network error' in response.json()['detail'].lower()


@pytest.mark.asyncio
async def test_timeout_error_returns_504():
    """Test that timeout errors return 504 Gateway Timeout"""
    
    with patch('app.api.completions.httpx.AsyncClient') as mock_client_class:
        mock_client = AsyncMock()
        mock_client_class.return_value.__aenter__.return_value = mock_client
        
        # Simulate timeout
        mock_client.post.side_effect = httpx.ReadTimeout("Read timeout")
        
        client = TestClient(app)
        response = client.post(
            '/v1/chat/completions',
            json={'model': 'gpt-4', 'messages': [{'role': 'user', 'content': 'test'}]},
            headers={'Authorization': 'Bearer test-key'}
        )
        
        # Should return 504 Gateway Timeout
        assert response.status_code == 504
        assert 'timeout' in response.json()['detail'].lower()