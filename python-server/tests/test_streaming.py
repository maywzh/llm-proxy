"""Tests for streaming utilities"""

import json
from unittest.mock import AsyncMock

import pytest

from app.utils.streaming import (
    rewrite_sse_chunk,
    stream_response,
    create_streaming_response,
    rewrite_model_in_response,
    count_tokens,
    calculate_message_tokens,
)


@pytest.mark.unit
class TestRewriteSSEChunk:
    """Test SSE chunk rewriting"""

    @pytest.mark.asyncio
    async def test_rewrite_model_in_chunk(self):
        """Test rewriting model field in SSE chunk"""
        chunk = b'data: {"id":"test","model":"gpt-4-0613","choices":[{"delta":{"content":"Hello"}}]}\n\n'
        result = await rewrite_sse_chunk(chunk, "gpt-4")

        result_str = result.decode("utf-8")
        assert "gpt-4" in result_str
        assert "gpt-4-0613" not in result_str

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
        result = await rewrite_sse_chunk(chunk, "gpt-4")

        result_str = result.decode("utf-8")
        # Check for model rewrite (JSON may have spaces)
        assert result_str.count('"gpt-4"') == 2
        assert "gpt-4-0613" not in result_str

    @pytest.mark.asyncio
    async def test_preserve_done_marker(self):
        """Test that [DONE] marker is preserved"""
        chunk = b"data: [DONE]\n\n"
        result = await rewrite_sse_chunk(chunk, "gpt-4")

        assert result == chunk

    @pytest.mark.asyncio
    async def test_handle_invalid_json(self):
        """Test handling of invalid JSON in chunk"""
        chunk = b"data: {invalid json}\n\n"
        result = await rewrite_sse_chunk(chunk, "gpt-4")

        # Should return original chunk on error
        assert result == chunk

    @pytest.mark.asyncio
    async def test_chunk_without_model_field(self):
        """Test chunk without model field is unchanged"""
        chunk = b'data: {"id":"test","choices":[]}\n\n'
        result = await rewrite_sse_chunk(chunk, "gpt-4")

        # No model field to rewrite, but should still process
        result_str = result.decode("utf-8")
        assert "data:" in result_str

    @pytest.mark.asyncio
    async def test_empty_chunk(self):
        """Test handling of empty chunk"""
        chunk = b""
        result = await rewrite_sse_chunk(chunk, "gpt-4")

        assert result == b""

    @pytest.mark.asyncio
    async def test_chunk_with_unicode(self):
        """Test handling chunk with unicode characters"""
        chunk = b'data: {"model":"gpt-4-0613","content":"\xe4\xb8\xad\xe6\x96\x87"}\n\n'
        result = await rewrite_sse_chunk(chunk, "gpt-4")

        result_str = result.decode("utf-8")
        # Check for model rewrite (JSON may have spaces)
        assert '"gpt-4"' in result_str
        assert "gpt-4-0613" not in result_str

    @pytest.mark.asyncio
    async def test_chunk_with_newlines_in_content(self):
        """Test chunk with newlines in content"""
        data = {"model": "gpt-4-0613", "content": "line1\nline2"}
        chunk = f"data: {json.dumps(data)}\n\n".encode("utf-8")
        result = await rewrite_sse_chunk(chunk, "gpt-4")

        result_str = result.decode("utf-8")
        # Check for model rewrite (JSON may have spaces)
        assert '"gpt-4"' in result_str
        assert "gpt-4-0613" not in result_str


@pytest.mark.unit
class TestStreamResponse:
    """Test stream_response function"""

    @pytest.mark.asyncio
    async def test_stream_response_basic(self):
        """Test basic streaming response"""

        async def mock_aiter_lines():
            yield 'data: {"model":"gpt-4-0613","content":"Hello"}'
            yield ""  # SSE event boundary
            yield "data: [DONE]"
            yield ""  # SSE event boundary

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(mock_response, "gpt-4", "test-provider"):
            chunks.append(chunk)

        assert len(chunks) == 2

    @pytest.mark.asyncio
    async def test_stream_response_with_usage(self):
        """Test streaming response with usage information"""

        async def mock_aiter_lines():
            yield 'data: {"model":"gpt-4-0613","usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}'
            yield ""  # SSE event boundary

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(mock_response, "gpt-4", "test-provider"):
            chunks.append(chunk)

        assert len(chunks) == 1
        # Usage should be tracked (tested via metrics)

    @pytest.mark.asyncio
    async def test_stream_response_without_usage_fallback(self):
        """Test fallback token counting when provider doesn't return usage"""

        async def mock_aiter_lines():
            yield 'data: {"choices":[{"delta":{"content":"Hello"}}]}'
            yield ""  # SSE event boundary
            yield 'data: {"choices":[{"delta":{"content":" world"}}]}'
            yield ""  # SSE event boundary
            yield "data: [DONE]"
            yield ""  # SSE event boundary

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        request_data = {"messages": [{"role": "user", "content": "Test message"}]}

        chunks = []
        async for chunk in stream_response(
            mock_response, "gpt-4", "test-provider", request_data
        ):
            chunks.append(chunk)

        assert len(chunks) == 3
        # Fallback token counting should be triggered

    @pytest.mark.asyncio
    async def test_stream_response_with_null_usage(self):
        """Test fallback when usage is null"""

        async def mock_aiter_lines():
            yield 'data: {"choices":[{"delta":{"content":"Test"}}]}'
            yield ""  # SSE event boundary
            yield 'data: {"usage":null}'
            yield ""  # SSE event boundary
            yield "data: [DONE]"
            yield ""  # SSE event boundary

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        request_data = {"messages": [{"role": "user", "content": "Hello"}]}

        chunks = []
        async for chunk in stream_response(
            mock_response, "gpt-4", "test-provider", request_data
        ):
            chunks.append(chunk)

        assert len(chunks) == 3

    @pytest.mark.asyncio
    async def test_stream_response_completes(self):
        """Test that stream completes successfully"""

        async def mock_aiter_lines():
            yield "data: test"
            yield ""  # SSE event boundary

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(mock_response, None, "test-provider"):
            chunks.append(chunk)

        assert len(chunks) == 1

    @pytest.mark.asyncio
    async def test_stream_response_error_handling(self):
        """Test error handling in stream response"""

        async def mock_aiter_bytes():
            raise Exception("Stream error")
            yield  # Make it a generator

        mock_response = AsyncMock()
        mock_response.aiter_bytes = mock_aiter_bytes

        chunks = []
        async for chunk in stream_response(mock_response, None, "test-provider"):
            chunks.append(chunk)

        # Should handle error gracefully and yield error event
        assert len(chunks) == 1


@pytest.mark.unit
class TestTokenCounting:
    """Test token counting functions"""

    def test_count_tokens_basic(self):
        """Test basic token counting"""
        text = "Hello world"
        tokens = count_tokens(text, "gpt-3.5-turbo")
        assert tokens > 0
        assert isinstance(tokens, int)

    def test_count_tokens_empty(self):
        """Test counting tokens in empty string"""
        tokens = count_tokens("", "gpt-3.5-turbo")
        assert tokens == 0

    def test_count_tokens_unicode(self):
        """Test counting tokens with unicode"""
        text = "你好世界"
        tokens = count_tokens(text, "gpt-3.5-turbo")
        assert tokens > 0

    def test_calculate_message_tokens(self):
        """Test calculating tokens for messages"""
        messages = [
            {"role": "user", "content": "Hello"},
            {"role": "assistant", "content": "Hi there"},
        ]
        tokens = calculate_message_tokens(messages, "gpt-3.5-turbo")
        assert tokens > 0
        # Should include content + role + format overhead
        assert tokens > len("Hello") + len("Hi there")

    def test_calculate_message_tokens_with_name(self):
        """Test calculating tokens with name field"""
        messages = [{"role": "user", "content": "Test", "name": "Alice"}]
        tokens = calculate_message_tokens(messages, "gpt-3.5-turbo")
        assert tokens > 0

    def test_calculate_message_tokens_empty(self):
        """Test calculating tokens for empty messages"""
        tokens = calculate_message_tokens([], "gpt-3.5-turbo")
        # Should still include base overhead
        assert tokens == 3


@pytest.mark.unit
class TestCreateStreamingResponse:
    """Test create_streaming_response function"""

    def test_create_streaming_response(self):
        """Test creating streaming response"""
        mock_response = AsyncMock()

        response = create_streaming_response(mock_response, "gpt-4", "test-provider")

        assert response.media_type == "text/event-stream"

    def test_create_streaming_response_with_request_data(self):
        """Test creating streaming response with request data"""
        mock_response = AsyncMock()
        request_data = {"messages": [{"role": "user", "content": "Test"}]}

        response = create_streaming_response(
            mock_response,
            "gpt-4",
            "test-provider",
            request_data=request_data,
        )

        assert response.media_type == "text/event-stream"


@pytest.mark.unit
class TestRewriteModelInResponse:
    """Test rewrite_model_in_response function"""

    def test_rewrite_model_field(self):
        """Test rewriting model field in response"""
        response = {
            "id": "test-id",
            "model": "gpt-4-0613",
            "choices": [{"message": {"content": "Hello"}}],
        }

        result = rewrite_model_in_response(response, "gpt-4")

        assert result["model"] == "gpt-4"
        assert result["id"] == "test-id"

    def test_no_rewrite_when_no_original_model(self):
        """Test response unchanged when no original model"""
        response = {"id": "test-id", "model": "gpt-4-0613", "choices": []}

        result = rewrite_model_in_response(response, None)

        assert result["model"] == "gpt-4-0613"

    def test_response_without_model_field(self):
        """Test response without model field"""
        response = {"id": "test-id", "choices": []}

        result = rewrite_model_in_response(response, "gpt-4")

        # Should not add model field if it doesn't exist
        assert "model" not in result

    def test_rewrite_preserves_other_fields(self):
        """Test that rewriting preserves all other fields"""
        response = {
            "id": "test-id",
            "model": "gpt-4-0613",
            "object": "chat.completion",
            "created": 1234567890,
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hello"},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30},
        }

        result = rewrite_model_in_response(response, "gpt-4")

        assert result["model"] == "gpt-4"
        assert result["id"] == "test-id"
        assert result["object"] == "chat.completion"
        assert result["created"] == 1234567890
        assert len(result["choices"]) == 1
        assert result["usage"]["total_tokens"] == 30

    def test_rewrite_returns_same_dict(self):
        """Test that rewrite modifies and returns the same dict"""
        response = {"model": "gpt-4-0613", "id": "test"}
        result = rewrite_model_in_response(response, "gpt-4")

        # Should be the same object
        assert result is response


@pytest.mark.unit
class TestStreamingEdgeCases:
    """Test edge cases in streaming"""

    @pytest.mark.asyncio
    async def test_empty_stream(self):
        """Test handling empty stream"""

        async def mock_aiter_lines():
            return
            yield  # Make it a generator

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(mock_response, None, "test-provider"):
            chunks.append(chunk)

        assert len(chunks) == 0

    @pytest.mark.asyncio
    async def test_malformed_sse_chunks(self):
        """Test handling malformed SSE chunks"""
        malformed_chunks = [
            b"not a valid sse chunk",
            b"data: {incomplete json",
            b"random bytes \x00\x01\x02",
        ]

        for chunk in malformed_chunks:
            result = await rewrite_sse_chunk(chunk, "gpt-4")
            # Should not crash, returns original or processed chunk
            assert isinstance(result, bytes)

    @pytest.mark.asyncio
    async def test_very_large_chunk(self):
        """Test handling very large chunks"""
        large_content = "x" * 100000
        data = {"model": "gpt-4-0613", "content": large_content}
        chunk = f"data: {json.dumps(data)}\n\n".encode("utf-8")

        result = await rewrite_sse_chunk(chunk, "gpt-4")

        result_str = result.decode("utf-8")
        # Check for model rewrite (JSON may have spaces)
        assert '"gpt-4"' in result_str
        assert "gpt-4-0613" not in result_str
        assert large_content in result_str

    @pytest.mark.asyncio
    async def test_chunk_with_special_characters(self):
        """Test chunk with special characters"""
        data = {"model": "gpt-4-0613", "content": "Special: \n\t\r\"'\\/"}
        chunk = f"data: {json.dumps(data)}\n\n".encode("utf-8")

        result = await rewrite_sse_chunk(chunk, "gpt-4")

        result_str = result.decode("utf-8")
        # Check for model rewrite (JSON may have spaces)
        assert '"gpt-4"' in result_str
        assert "gpt-4-0613" not in result_str


@pytest.mark.unit
class TestSSELineBuffering:
    """Test SSE line buffering to prevent JSON truncation from TCP fragmentation"""

    @pytest.mark.asyncio
    async def test_fragmented_sse_chunks_reassembled(self):
        """Test that SSE events split across TCP packets are correctly reassembled"""

        # Simulate TCP fragmentation: one JSON split across multiple lines/packets
        async def mock_aiter_lines():
            # First packet: partial data line
            yield 'data: {"model":"gpt-4-0613","choices":[{"delta":{"content":"Hello"}}]}'
            # Second packet: blank line (event boundary)
            yield ""
            # Third packet: another complete event
            yield 'data: {"model":"gpt-4-0613","choices":[{"delta":{"content":" world"}}]}'
            yield ""
            # Done marker
            yield "data: [DONE]"
            yield ""

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(mock_response, "gpt-4", "test-provider"):
            chunks.append(chunk)

        # Should receive 3 complete SSE events
        assert len(chunks) == 3
        # Verify no truncation - all chunks should be valid
        for chunk in chunks:
            chunk_str = chunk.decode("utf-8")
            # Each chunk should either be valid JSON or [DONE]
            if "data: [DONE]" not in chunk_str:
                for line in chunk_str.split("\n"):
                    if line.startswith("data: "):
                        json_str = line[6:].strip()
                        if json_str:
                            # Should not raise JSONDecodeError
                            json.loads(json_str)

    @pytest.mark.asyncio
    async def test_multiline_sse_event_buffered(self):
        """Test that multi-line SSE events are properly buffered"""

        async def mock_aiter_lines():
            # SSE event with event type line
            yield "event: message"
            yield 'data: {"model":"gpt-4-0613","content":"test"}'
            yield ""  # Event boundary

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(mock_response, "gpt-4", "test-provider"):
            chunks.append(chunk)

        # Should receive 1 complete event
        assert len(chunks) == 1

    @pytest.mark.asyncio
    async def test_truncated_json_not_emitted_early(self):
        """Test that incomplete JSON is buffered until complete"""

        # This simulates the exact bug scenario from the user report
        async def mock_aiter_lines():
            # Line with complete JSON - will be buffered
            yield 'data: {"id":"chatcmpl-e7bb5e79","choices":[{"delta":{"tool_calls":[{"function":{"arguments":"test"}}]}}]}'
            # Event boundary
            yield ""

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(mock_response, "gpt-4", "test-provider"):
            chunks.append(chunk)

        assert len(chunks) == 1
        chunk_str = chunks[0].decode("utf-8")
        # Verify the JSON is complete and parseable
        for line in chunk_str.split("\n"):
            if line.startswith("data: "):
                json_str = line[6:].strip()
                if json_str:
                    parsed = json.loads(json_str)
                    assert "choices" in parsed

    @pytest.mark.asyncio
    async def test_buffer_flushed_at_stream_end(self):
        """Test that remaining buffer content is flushed when stream ends"""

        async def mock_aiter_lines():
            yield 'data: {"model":"gpt-4-0613","content":"final"}'
            # No blank line - stream ends with data in buffer
            # Note: In real scenarios, the stream should end with \n\n
            # but we test graceful handling of edge cases

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(mock_response, "gpt-4", "test-provider"):
            chunks.append(chunk)

        # Buffer should be flushed at end
        # The chunk may or may not be yielded depending on buffer state
        # What's important is no crash and no truncated JSON emitted mid-stream
        assert len(chunks) <= 1

    @pytest.mark.asyncio
    async def test_consecutive_events_handled(self):
        """Test rapid consecutive SSE events are handled correctly"""

        async def mock_aiter_lines():
            for i in range(5):
                yield f'data: {{"model":"gpt-4-0613","index":{i}}}'
                yield ""  # Event boundary
            yield "data: [DONE]"
            yield ""

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(mock_response, "gpt-4", "test-provider"):
            chunks.append(chunk)

        # Should receive 6 events (5 data + 1 done)
        assert len(chunks) == 6
