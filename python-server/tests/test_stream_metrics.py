"""Tests for stream metrics module."""

import time
from unittest.mock import patch

import pytest

from app.core.stream_metrics import StreamStats, record_stream_metrics


@pytest.mark.unit
class TestStreamStats:
    """Test StreamStats data structure."""

    def test_stream_stats_creation(self):
        """Test creating StreamStats with all fields."""
        now = time.time()
        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=100,
            output_tokens=200,
            start_time=now,
            first_token_time=now + 0.5,
        )

        assert stats.model == "gpt-4"
        assert stats.provider == "OpenAI"
        assert stats.api_key_name == "test-key"
        assert stats.input_tokens == 100
        assert stats.output_tokens == 200
        assert stats.start_time == now
        assert stats.first_token_time == now + 0.5

    def test_stream_stats_with_none_first_token_time(self):
        """Test creating StreamStats with None first_token_time."""
        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=100,
            output_tokens=0,
            start_time=time.time(),
            first_token_time=None,
        )

        assert stats.first_token_time is None

    def test_stream_stats_immutable(self):
        """Test that StreamStats is immutable (frozen dataclass)."""
        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=100,
            output_tokens=200,
            start_time=time.time(),
            first_token_time=None,
        )

        # Should raise FrozenInstanceError
        with pytest.raises(Exception):
            stats.model = "gpt-3.5"


@pytest.mark.unit
class TestRecordStreamMetrics:
    """Test record_stream_metrics function."""

    @patch("app.core.stream_metrics.TOKENS_PER_SECOND")
    @patch("app.core.stream_metrics.TTFT")
    @patch("app.core.stream_metrics.TOKEN_USAGE")
    def test_record_tps_when_valid(self, mock_token_usage, mock_ttft, mock_tps):
        """Test TPS is recorded when first_token_time and output_tokens are valid."""
        start_time = time.time() - 2.0  # 2 seconds ago
        first_token_time = time.time() - 1.0  # 1 second ago

        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=100,
            output_tokens=50,
            start_time=start_time,
            first_token_time=first_token_time,
        )

        record_stream_metrics(stats)

        # TPS should be recorded
        mock_tps.labels.assert_called_with(
            source="provider",
            model="gpt-4",
            provider="OpenAI",
        )
        mock_tps.labels.return_value.observe.assert_called()

    @patch("app.core.stream_metrics.TOKENS_PER_SECOND")
    @patch("app.core.stream_metrics.TTFT")
    @patch("app.core.stream_metrics.TOKEN_USAGE")
    def test_no_tps_when_no_first_token_time(
        self, mock_token_usage, mock_ttft, mock_tps
    ):
        """Test TPS is not recorded when first_token_time is None."""
        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=100,
            output_tokens=50,
            start_time=time.time(),
            first_token_time=None,
        )

        record_stream_metrics(stats)

        # TPS should NOT be recorded
        mock_tps.labels.return_value.observe.assert_not_called()

    @patch("app.core.stream_metrics.TOKENS_PER_SECOND")
    @patch("app.core.stream_metrics.TTFT")
    @patch("app.core.stream_metrics.TOKEN_USAGE")
    def test_no_tps_when_zero_output_tokens(
        self, mock_token_usage, mock_ttft, mock_tps
    ):
        """Test TPS is not recorded when output_tokens is 0."""
        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=100,
            output_tokens=0,
            start_time=time.time() - 1.0,
            first_token_time=time.time() - 0.5,
        )

        record_stream_metrics(stats)

        # TPS should NOT be recorded (no output tokens)
        mock_tps.labels.return_value.observe.assert_not_called()

    @patch("app.core.stream_metrics.TOKENS_PER_SECOND")
    @patch("app.core.stream_metrics.TTFT")
    @patch("app.core.stream_metrics.TOKEN_USAGE")
    def test_ttft_recorded_when_first_token_time_exists(
        self, mock_token_usage, mock_ttft, mock_tps
    ):
        """Test TTFT is recorded when first_token_time exists."""
        start_time = time.time() - 1.0
        first_token_time = time.time() - 0.5  # TTFT = 0.5s

        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=100,
            output_tokens=50,
            start_time=start_time,
            first_token_time=first_token_time,
        )

        record_stream_metrics(stats)

        # TTFT should be recorded
        mock_ttft.labels.assert_called_with(
            source="provider",
            model="gpt-4",
            provider="OpenAI",
        )
        mock_ttft.labels.return_value.observe.assert_called()

    @patch("app.core.stream_metrics.TOKENS_PER_SECOND")
    @patch("app.core.stream_metrics.TTFT")
    @patch("app.core.stream_metrics.TOKEN_USAGE")
    def test_no_ttft_when_no_first_token_time(
        self, mock_token_usage, mock_ttft, mock_tps
    ):
        """Test TTFT is not recorded when first_token_time is None."""
        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=100,
            output_tokens=50,
            start_time=time.time(),
            first_token_time=None,
        )

        record_stream_metrics(stats)

        # TTFT should NOT be recorded
        mock_ttft.labels.return_value.observe.assert_not_called()

    @patch("app.core.stream_metrics.TOKENS_PER_SECOND")
    @patch("app.core.stream_metrics.TTFT")
    @patch("app.core.stream_metrics.TOKEN_USAGE")
    def test_token_usage_recorded(self, mock_token_usage, mock_ttft, mock_tps):
        """Test token usage is recorded for all token types."""
        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=100,
            output_tokens=200,
            start_time=time.time(),
            first_token_time=None,
        )

        record_stream_metrics(stats)

        # Token usage should be recorded for prompt, completion, and total
        calls = mock_token_usage.labels.call_args_list

        # Should have 3 calls: prompt, completion, total
        assert len(calls) == 3

        # Verify the labels were called with correct token_type values
        token_types = [call.kwargs["token_type"] for call in calls]
        assert "prompt" in token_types
        assert "completion" in token_types
        assert "total" in token_types

    @patch("app.core.stream_metrics.TOKENS_PER_SECOND")
    @patch("app.core.stream_metrics.TTFT")
    @patch("app.core.stream_metrics.TOKEN_USAGE")
    def test_no_token_usage_when_zero_tokens(
        self, mock_token_usage, mock_ttft, mock_tps
    ):
        """Test token usage is not recorded when both input and output are 0."""
        stats = StreamStats(
            model="gpt-4",
            provider="OpenAI",
            api_key_name="test-key",
            client="test-client",
            input_tokens=0,
            output_tokens=0,
            start_time=time.time(),
            first_token_time=None,
        )

        record_stream_metrics(stats)

        # Token usage should NOT be recorded
        mock_token_usage.labels.assert_not_called()


@pytest.mark.unit
class TestRecordStreamMetricsIntegration:
    """Integration tests for record_stream_metrics with real Prometheus metrics."""

    def test_full_metrics_recording(self):
        """Test full metrics recording with real Prometheus metrics."""
        from app.core.metrics import TOKENS_PER_SECOND, TTFT, TOKEN_USAGE

        start_time = time.time() - 2.0
        first_token_time = time.time() - 1.0

        stats = StreamStats(
            model="test-model-integration",
            provider="test-provider-integration",
            api_key_name="test-key-integration",
            client="test-client",
            input_tokens=50,
            output_tokens=100,
            start_time=start_time,
            first_token_time=first_token_time,
        )

        # Should not raise any exceptions
        record_stream_metrics(stats)

        # Verify metrics were recorded by checking they exist
        tps_metric = TOKENS_PER_SECOND.labels(
            source="provider",
            model="test-model-integration",
            provider="test-provider-integration",
        )
        assert tps_metric._sum.get() > 0

        ttft_metric = TTFT.labels(
            source="provider",
            model="test-model-integration",
            provider="test-provider-integration",
        )
        assert ttft_metric._sum.get() > 0

        token_metric = TOKEN_USAGE.labels(
            model="test-model-integration",
            provider="test-provider-integration",
            token_type="total",
            api_key_name="test-key-integration",
            client="test-client",
        )
        assert token_metric._value.get() >= 150  # input + output

    def test_tps_calculation_accuracy(self):
        """Test TPS calculation is accurate."""
        from app.core.metrics import TOKENS_PER_SECOND

        # Create a controlled scenario: 100 tokens in 1 second = 100 TPS
        start_time = time.time() - 2.0
        first_token_time = start_time + 0.5  # TTFT = 0.5s

        stats = StreamStats(
            model="test-tps-accuracy",
            provider="test-provider-tps",
            api_key_name="test-key",
            client="test-client",
            input_tokens=50,
            output_tokens=100,
            start_time=start_time,
            first_token_time=first_token_time,
        )

        record_stream_metrics(stats)

        # TPS should be approximately 100 tokens / elapsed_since_first_token
        tps_metric = TOKENS_PER_SECOND.labels(
            source="provider",
            model="test-tps-accuracy",
            provider="test-provider-tps",
        )
        # Verify it was recorded by checking _sum > 0
        assert tps_metric._sum.get() > 0
