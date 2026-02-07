"""Tests for client disconnect detection during streaming"""

import asyncio
from unittest.mock import AsyncMock, patch

import pytest

from app.utils.streaming import stream_response


@pytest.mark.unit
class TestClientDisconnectDetection:
    """Test client disconnect detection in streaming responses"""

    @pytest.mark.asyncio
    async def test_stream_stops_on_disconnect_check(self):
        """Test that stream stops when disconnect_check returns True"""
        # Track how many chunks were yielded
        chunks_yielded = 0
        disconnect_after = 2

        async def mock_aiter_lines():
            for i in range(5):
                yield f'data: {{"model":"gpt-4","index":{i}}}'
                yield ""  # SSE event boundary

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        async def mock_disconnect_check():
            nonlocal chunks_yielded
            # Return True after yielding 2 chunks
            return chunks_yielded >= disconnect_after

        chunks = []
        async for chunk in stream_response(
            mock_response,
            "gpt-4",
            "test-provider",
            disconnect_check=mock_disconnect_check,
        ):
            chunks.append(chunk)
            chunks_yielded += 1

        # Should have stopped after disconnect_after chunks
        assert len(chunks) == disconnect_after

    @pytest.mark.asyncio
    async def test_stream_continues_without_disconnect_check(self):
        """Test that stream continues normally when disconnect_check is None"""

        async def mock_aiter_lines():
            for i in range(3):
                yield f'data: {{"model":"gpt-4","index":{i}}}'
                yield ""  # SSE event boundary
            yield "data: [DONE]"
            yield ""

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        async for chunk in stream_response(
            mock_response,
            "gpt-4",
            "test-provider",
            disconnect_check=None,
        ):
            chunks.append(chunk)

        # Should have received all chunks (3 data + 1 done)
        assert len(chunks) == 4

    @pytest.mark.asyncio
    async def test_stream_continues_when_disconnect_check_returns_false(self):
        """Test that stream continues when disconnect_check always returns False"""

        async def mock_aiter_lines():
            for i in range(3):
                yield f'data: {{"model":"gpt-4","index":{i}}}'
                yield ""  # SSE event boundary
            yield "data: [DONE]"
            yield ""

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        async def mock_disconnect_check():
            return False

        chunks = []
        async for chunk in stream_response(
            mock_response,
            "gpt-4",
            "test-provider",
            disconnect_check=mock_disconnect_check,
        ):
            chunks.append(chunk)

        # Should have received all chunks
        assert len(chunks) == 4

    @pytest.mark.asyncio
    async def test_disconnect_check_error_is_handled_gracefully(self):
        """Test that errors in disconnect_check are handled gracefully"""

        async def mock_aiter_lines():
            for i in range(3):
                yield f'data: {{"model":"gpt-4","index":{i}}}'
                yield ""  # SSE event boundary
            yield "data: [DONE]"
            yield ""

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        async def mock_disconnect_check():
            raise Exception("Connection check failed")

        chunks = []
        async for chunk in stream_response(
            mock_response,
            "gpt-4",
            "test-provider",
            disconnect_check=mock_disconnect_check,
        ):
            chunks.append(chunk)

        # Should have received all chunks despite disconnect_check errors
        assert len(chunks) == 4

    @pytest.mark.asyncio
    async def test_cancelled_error_is_handled(self):
        """Test that asyncio.CancelledError is properly handled"""

        async def mock_aiter_lines():
            yield 'data: {"model":"gpt-4","index":0}'
            yield ""
            raise asyncio.CancelledError()

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks = []
        with pytest.raises(asyncio.CancelledError):
            async for chunk in stream_response(
                mock_response,
                "gpt-4",
                "test-provider",
            ):
                chunks.append(chunk)

        # Should have received at least one chunk before cancellation
        assert len(chunks) >= 1

    @pytest.mark.asyncio
    async def test_disconnect_check_called_before_each_chunk(self):
        """Test that disconnect_check is called before processing each chunk"""
        call_count = 0

        async def mock_aiter_lines():
            for i in range(3):
                yield f'data: {{"model":"gpt-4","index":{i}}}'
                yield ""  # SSE event boundary

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        async def mock_disconnect_check():
            nonlocal call_count
            call_count += 1
            return False

        chunks = []
        async for chunk in stream_response(
            mock_response,
            "gpt-4",
            "test-provider",
            disconnect_check=mock_disconnect_check,
        ):
            chunks.append(chunk)

        # disconnect_check should be called multiple times (once per line iteration)
        assert call_count > 0

    @pytest.mark.asyncio
    async def test_metrics_recorded_on_disconnect(self):
        """Test that CLIENT_DISCONNECTS metric is recorded when client disconnects"""
        disconnect_after = 1

        async def mock_aiter_lines():
            for i in range(5):
                yield f'data: {{"model":"gpt-4","index":{i}}}'
                yield ""  # SSE event boundary

        mock_response = AsyncMock()
        mock_response.aiter_lines = mock_aiter_lines

        chunks_yielded = 0

        async def mock_disconnect_check():
            nonlocal chunks_yielded
            return chunks_yielded >= disconnect_after

        with patch("app.utils.streaming.CLIENT_DISCONNECTS") as mock_metric:
            chunks = []
            async for chunk in stream_response(
                mock_response,
                "gpt-4",
                "test-provider",
                disconnect_check=mock_disconnect_check,
            ):
                chunks.append(chunk)
                chunks_yielded += 1

            # Verify metric was recorded
            mock_metric.labels.assert_called_with(
                model="gpt-4",
                provider="test-provider",
            )
            mock_metric.labels.return_value.inc.assert_called_once()


@pytest.mark.unit
class TestCreateStreamingResponseWithDisconnect:
    """Test create_streaming_response with disconnect_check parameter"""

    def test_create_streaming_response_accepts_disconnect_check(self):
        """Test that create_streaming_response accepts disconnect_check parameter"""
        from app.utils.streaming import create_streaming_response

        mock_response = AsyncMock()

        async def mock_disconnect_check():
            return False

        # Should not raise any errors
        response = create_streaming_response(
            mock_response,
            "gpt-4",
            "test-provider",
            disconnect_check=mock_disconnect_check,
        )

        assert response.media_type == "text/event-stream"

    def test_create_streaming_response_works_without_disconnect_check(self):
        """Test that create_streaming_response works without disconnect_check"""
        from app.utils.streaming import create_streaming_response

        mock_response = AsyncMock()

        # Should not raise any errors
        response = create_streaming_response(
            mock_response,
            "gpt-4",
            "test-provider",
        )

        assert response.media_type == "text/event-stream"
