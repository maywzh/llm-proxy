"""Tests for streaming utilities"""
import json
from unittest.mock import Mock, AsyncMock, patch

import pytest

from app.utils.streaming import (
    rewrite_sse_chunk,
    stream_response,
    create_streaming_response,
    rewrite_model_in_response
)


@pytest.mark.unit
class TestRewriteSSEChunk:
    """Test SSE chunk rewriting"""
    
    @pytest.mark.asyncio
    async def test_rewrite_model_in_chunk(self):
        """Test rewriting model field in SSE chunk"""
        chunk = b'data: {"id":"test","model":"gpt-4-0613","choices":[{"delta":{"content":"Hello"}}]}\n\n'
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        result_str = result.decode('utf-8')
        assert 'gpt-4' in result_str
        assert 'gpt-4-0613' not in result_str
    
    @pytest.mark.asyncio
    async def test_no_rewrite_when_no_original_model(self):
        """Test chunk is unchanged when no original model provided"""
        chunk = b'data: {"id":"test","model":"gpt-4-0613","choices":[]}\n\n'
        result = await rewrite_sse_chunk(chunk, None)
        
        assert result == chunk
    
    @pytest.mark.asyncio
    async def test_rewrite_multiple_lines(self):
        """Test rewriting multiple data lines"""
        chunk = b'data: {"model":"gpt-4-0613"}\n\ndata: {"model":"gpt-4-0613"}\n\n'
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        result_str = result.decode('utf-8')
        assert result_str.count('"model":"gpt-4"') == 2
        assert 'gpt-4-0613' not in result_str
    
    @pytest.mark.asyncio
    async def test_preserve_done_marker(self):
        """Test that [DONE] marker is preserved"""
        chunk = b'data: [DONE]\n\n'
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        assert result == chunk
    
    @pytest.mark.asyncio
    async def test_handle_invalid_json(self):
        """Test handling of invalid JSON in chunk"""
        chunk = b'data: {invalid json}\n\n'
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        # Should return original chunk on error
        assert result == chunk
    
    @pytest.mark.asyncio
    async def test_chunk_without_model_field(self):
        """Test chunk without model field is unchanged"""
        chunk = b'data: {"id":"test","choices":[]}\n\n'
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        # No model field to rewrite, but should still process
        result_str = result.decode('utf-8')
        assert 'data:' in result_str
    
    @pytest.mark.asyncio
    async def test_empty_chunk(self):
        """Test handling of empty chunk"""
        chunk = b''
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        assert result == b''
    
    @pytest.mark.asyncio
    async def test_chunk_with_unicode(self):
        """Test handling chunk with unicode characters"""
        chunk = b'data: {"model":"gpt-4-0613","content":"\xe4\xb8\xad\xe6\x96\x87"}\n\n'
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        result_str = result.decode('utf-8')
        assert '"model":"gpt-4"' in result_str
        assert '中文' in result_str
    
    @pytest.mark.asyncio
    async def test_chunk_with_newlines_in_content(self):
        """Test chunk with newlines in content"""
        data = {"model": "gpt-4-0613", "content": "line1\nline2"}
        chunk = f'data: {json.dumps(data)}\n\n'.encode('utf-8')
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        result_str = result.decode('utf-8')
        assert '"model":"gpt-4"' in result_str


@pytest.mark.unit
class TestStreamResponse:
    """Test stream_response function"""
    
    @pytest.mark.asyncio
    async def test_stream_response_basic(self):
        """Test basic streaming response"""
        mock_client = AsyncMock()
        mock_response = AsyncMock()
        mock_response.aiter_bytes = AsyncMock(return_value=[
            b'data: {"model":"gpt-4-0613","content":"Hello"}\n\n',
            b'data: [DONE]\n\n'
        ])
        mock_client.stream = AsyncMock(return_value=mock_response)
        mock_client.__aenter__ = AsyncMock(return_value=mock_response)
        mock_client.__aexit__ = AsyncMock()
        
        chunks = []
        async for chunk in stream_response(
            mock_client,
            'https://api.test.com/chat/completions',
            {'model': 'gpt-4', 'messages': []},
            {'Authorization': 'Bearer test'},
            'gpt-4',
            'test-provider'
        ):
            chunks.append(chunk)
        
        assert len(chunks) == 2
        assert mock_client.aclose.called
    
    @pytest.mark.asyncio
    async def test_stream_response_with_usage(self):
        """Test streaming response with usage information"""
        usage_chunk = b'data: {"model":"gpt-4-0613","usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}\n\n'
        
        mock_client = AsyncMock()
        mock_response = AsyncMock()
        mock_response.aiter_bytes = AsyncMock(return_value=[usage_chunk])
        mock_client.stream = AsyncMock(return_value=mock_response)
        mock_client.__aenter__ = AsyncMock(return_value=mock_response)
        mock_client.__aexit__ = AsyncMock()
        
        chunks = []
        async for chunk in stream_response(
            mock_client,
            'https://api.test.com/chat/completions',
            {'model': 'gpt-4'},
            {'Authorization': 'Bearer test'},
            'gpt-4',
            'test-provider'
        ):
            chunks.append(chunk)
        
        assert len(chunks) == 1
        # Usage should be tracked (tested via metrics)
    
    @pytest.mark.asyncio
    async def test_stream_response_closes_client(self):
        """Test that client is closed after streaming"""
        mock_client = AsyncMock()
        mock_response = AsyncMock()
        mock_response.aiter_bytes = AsyncMock(return_value=[b'data: test\n\n'])
        mock_client.stream = AsyncMock(return_value=mock_response)
        mock_client.__aenter__ = AsyncMock(return_value=mock_response)
        mock_client.__aexit__ = AsyncMock()
        
        async for _ in stream_response(
            mock_client,
            'https://api.test.com/chat/completions',
            {},
            {},
            None,
            'test-provider'
        ):
            pass
        
        mock_client.aclose.assert_called_once()
    
    @pytest.mark.asyncio
    async def test_stream_response_error_handling(self):
        """Test error handling in stream response"""
        mock_client = AsyncMock()
        mock_response = AsyncMock()
        mock_response.aiter_bytes = AsyncMock(side_effect=Exception("Stream error"))
        mock_client.stream = AsyncMock(return_value=mock_response)
        mock_client.__aenter__ = AsyncMock(return_value=mock_response)
        mock_client.__aexit__ = AsyncMock()
        
        with pytest.raises(Exception, match="Stream error"):
            async for _ in stream_response(
                mock_client,
                'https://api.test.com/chat/completions',
                {},
                {},
                None,
                'test-provider'
            ):
                pass
        
        # Client should still be closed
        mock_client.aclose.assert_called_once()


@pytest.mark.unit
class TestCreateStreamingResponse:
    """Test create_streaming_response function"""
    
    def test_create_streaming_response(self, test_config, monkeypatch):
        """Test creating streaming response"""
        from app.core import config as config_module
        monkeypatch.setattr(config_module, 'get_config', lambda: test_config)
        
        response = create_streaming_response(
            'https://api.test.com/chat/completions',
            {'model': 'gpt-4', 'messages': []},
            {'Authorization': 'Bearer test'},
            'gpt-4',
            'test-provider'
        )
        
        assert response.media_type == 'text/event-stream'
    
    def test_create_streaming_response_with_ssl_verification(self, test_config, monkeypatch):
        """Test streaming response respects SSL verification setting"""
        from app.core import config as config_module
        
        # Test with SSL verification disabled
        test_config.verify_ssl = False
        monkeypatch.setattr(config_module, 'get_config', lambda: test_config)
        
        with patch('app.utils.streaming.httpx.AsyncClient') as mock_client:
            create_streaming_response(
                'https://api.test.com/chat/completions',
                {},
                {},
                None,
                'test-provider'
            )
            
            # Verify client was created with verify=False
            mock_client.assert_called_once()
            call_kwargs = mock_client.call_args[1]
            assert call_kwargs['verify'] is False
            assert call_kwargs['timeout'] == 300.0


@pytest.mark.unit
class TestRewriteModelInResponse:
    """Test rewrite_model_in_response function"""
    
    def test_rewrite_model_field(self):
        """Test rewriting model field in response"""
        response = {
            'id': 'test-id',
            'model': 'gpt-4-0613',
            'choices': [{'message': {'content': 'Hello'}}]
        }
        
        result = rewrite_model_in_response(response, 'gpt-4')
        
        assert result['model'] == 'gpt-4'
        assert result['id'] == 'test-id'
    
    def test_no_rewrite_when_no_original_model(self):
        """Test response unchanged when no original model"""
        response = {
            'id': 'test-id',
            'model': 'gpt-4-0613',
            'choices': []
        }
        
        result = rewrite_model_in_response(response, None)
        
        assert result['model'] == 'gpt-4-0613'
    
    def test_response_without_model_field(self):
        """Test response without model field"""
        response = {
            'id': 'test-id',
            'choices': []
        }
        
        result = rewrite_model_in_response(response, 'gpt-4')
        
        # Should not add model field if it doesn't exist
        assert 'model' not in result
    
    def test_rewrite_preserves_other_fields(self):
        """Test that rewriting preserves all other fields"""
        response = {
            'id': 'test-id',
            'model': 'gpt-4-0613',
            'object': 'chat.completion',
            'created': 1234567890,
            'choices': [
                {
                    'index': 0,
                    'message': {'role': 'assistant', 'content': 'Hello'},
                    'finish_reason': 'stop'
                }
            ],
            'usage': {
                'prompt_tokens': 10,
                'completion_tokens': 20,
                'total_tokens': 30
            }
        }
        
        result = rewrite_model_in_response(response, 'gpt-4')
        
        assert result['model'] == 'gpt-4'
        assert result['id'] == 'test-id'
        assert result['object'] == 'chat.completion'
        assert result['created'] == 1234567890
        assert len(result['choices']) == 1
        assert result['usage']['total_tokens'] == 30
    
    def test_rewrite_returns_same_dict(self):
        """Test that rewrite modifies and returns the same dict"""
        response = {'model': 'gpt-4-0613', 'id': 'test'}
        result = rewrite_model_in_response(response, 'gpt-4')
        
        # Should be the same object
        assert result is response


@pytest.mark.unit
class TestStreamingEdgeCases:
    """Test edge cases in streaming"""
    
    @pytest.mark.asyncio
    async def test_empty_stream(self):
        """Test handling empty stream"""
        mock_client = AsyncMock()
        mock_response = AsyncMock()
        mock_response.aiter_bytes = AsyncMock(return_value=[])
        mock_client.stream = AsyncMock(return_value=mock_response)
        mock_client.__aenter__ = AsyncMock(return_value=mock_response)
        mock_client.__aexit__ = AsyncMock()
        
        chunks = []
        async for chunk in stream_response(
            mock_client,
            'https://api.test.com/chat/completions',
            {},
            {},
            None,
            'test-provider'
        ):
            chunks.append(chunk)
        
        assert len(chunks) == 0
    
    @pytest.mark.asyncio
    async def test_malformed_sse_chunks(self):
        """Test handling malformed SSE chunks"""
        malformed_chunks = [
            b'not a valid sse chunk',
            b'data: {incomplete json',
            b'random bytes \x00\x01\x02'
        ]
        
        for chunk in malformed_chunks:
            result = await rewrite_sse_chunk(chunk, 'gpt-4')
            # Should not crash, returns original or processed chunk
            assert isinstance(result, bytes)
    
    @pytest.mark.asyncio
    async def test_very_large_chunk(self):
        """Test handling very large chunks"""
        large_content = 'x' * 100000
        data = {"model": "gpt-4-0613", "content": large_content}
        chunk = f'data: {json.dumps(data)}\n\n'.encode('utf-8')
        
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        result_str = result.decode('utf-8')
        assert '"model":"gpt-4"' in result_str
        assert large_content in result_str
    
    @pytest.mark.asyncio
    async def test_chunk_with_special_characters(self):
        """Test chunk with special characters"""
        data = {
            "model": "gpt-4-0613",
            "content": "Special: \n\t\r\"'\\/"
        }
        chunk = f'data: {json.dumps(data)}\n\n'.encode('utf-8')
        
        result = await rewrite_sse_chunk(chunk, 'gpt-4')
        
        result_str = result.decode('utf-8')
        assert '"model":"gpt-4"' in result_str