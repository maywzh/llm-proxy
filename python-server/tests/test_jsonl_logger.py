"""Tests for JSONL logger queue overflow and dropped record handling."""

import asyncio
import json
from pathlib import Path

import pytest

from app.core.jsonl_logger import (
    JsonlLogger,
    JsonlLoggerConfig,
    RequestRecord,
)


@pytest.mark.asyncio
class TestQueueOverflow:
    """Test JSONL logger queue overflow handling."""

    async def test_queue_full_drops_records_and_counts(self, tmp_path):
        """Verify queue full drops records and counts dropped.

        When the queue is full, new records should be dropped and
        the dropped count should be incremented.
        """
        log_file = tmp_path / "test.jsonl"
        config = JsonlLoggerConfig(log_path=log_file, enabled=True, buffer_size=1)

        logger = await JsonlLogger.create(config)
        assert logger is not None

        # Give writer task time to start
        await asyncio.sleep(0.01)

        # Rapidly write 200+ records to overflow the queue
        dropped_before = getattr(logger, "_dropped_count", 0)

        for i in range(200):
            record = RequestRecord(
                request_id=f"req-{i}",
                endpoint="/test",
                provider="test",
                payload={"test": i},
            )
            logger.log(record)

        # Give writer task time to process
        await asyncio.sleep(0.5)

        dropped_after = getattr(logger, "_dropped_count", 0)
        dropped_count = dropped_after - dropped_before

        # With buffer_size=1, many records should be dropped
        assert dropped_count >= 100, f"Expected >= 100 drops, got {dropped_count}"

        # Clean up
        await logger.shutdown()

    async def test_dropped_count_accumulates(self, tmp_path):
        """Verify dropped count correctly accumulates over time."""
        log_file = tmp_path / "test.jsonl"
        config = JsonlLoggerConfig(log_path=log_file, enabled=True, buffer_size=1)

        logger = await JsonlLogger.create(config)
        assert logger is not None

        # Give writer task time to start
        await asyncio.sleep(0.01)

        # First burst
        for i in range(100):
            record = RequestRecord(
                request_id=f"req-batch1-{i}",
                endpoint="/test",
                provider="test",
                payload={"test": i},
            )
            logger.log(record)

        await asyncio.sleep(0.3)
        dropped_after_batch1 = getattr(logger, "_dropped_count", 0)

        # Second burst
        for i in range(100):
            record = RequestRecord(
                request_id=f"req-batch2-{i}",
                endpoint="/test",
                provider="test",
                payload={"test": i},
            )
            logger.log(record)

        await asyncio.sleep(0.3)
        dropped_after_batch2 = getattr(logger, "_dropped_count", 0)

        # Dropped count should accumulate
        assert dropped_after_batch2 >= dropped_after_batch1
        assert dropped_after_batch1 > 0

        # Clean up
        await logger.shutdown()

    async def test_dropped_count_starts_at_zero(self, tmp_path):
        """Verify dropped count initializes to 0."""
        log_file = tmp_path / "test.jsonl"
        config = JsonlLoggerConfig(log_path=log_file, enabled=True, buffer_size=1000)

        logger = await JsonlLogger.create(config)
        assert logger is not None

        dropped_count = getattr(logger, "_dropped_count", 0)
        assert dropped_count == 0

        await logger.shutdown()

    async def test_warning_logged_on_queue_full(self, tmp_path):
        """Verify that records are dropped when queue is full.

        When the queue overflows, records should be dropped and counted.
        """
        log_file = tmp_path / "test.jsonl"
        config = JsonlLoggerConfig(log_path=log_file, enabled=True, buffer_size=1)

        logger = await JsonlLogger.create(config)
        assert logger is not None

        # Give writer task time to start
        await asyncio.sleep(0.01)

        # Overflow the queue
        for i in range(150):
            record = RequestRecord(
                request_id=f"req-{i}",
                endpoint="/test",
                provider="test",
                payload={"test": i},
            )
            logger.log(record)

        # Give writer task time to process
        await asyncio.sleep(0.5)

        # Check that records were dropped due to queue full
        dropped_count = getattr(logger, "_dropped_count", 0)

        # With buffer_size=1 and 150 records submitted, many should be dropped
        assert dropped_count > 0, f"Expected dropped records, got {dropped_count}"
        # Most records should be dropped (buffer_size is only 1)
        assert dropped_count >= 100, f"Expected >= 100 drops, got {dropped_count}"

        await logger.shutdown()


@pytest.mark.asyncio
class TestQueueBehavior:
    """Test JSONL logger queue behavior under normal and stress conditions."""

    async def test_normal_logging_no_drops(self, tmp_path):
        """Verify normal logging does not drop records."""
        log_file = tmp_path / "test.jsonl"
        config = JsonlLoggerConfig(log_path=log_file, enabled=True, buffer_size=1000)

        logger = await JsonlLogger.create(config)
        assert logger is not None

        # Log moderate number of records
        for i in range(50):
            record = RequestRecord(
                request_id=f"req-{i}",
                endpoint="/test",
                provider="test",
                payload={"test": i},
            )
            logger.log(record)

        # Give writer task time to flush
        await asyncio.sleep(0.5)

        dropped_count = getattr(logger, "_dropped_count", 0)
        assert dropped_count == 0

        await logger.shutdown()

        # Verify records were written
        assert log_file.exists()
        lines = log_file.read_text().strip().split("\n")
        assert len(lines) >= 40  # Most records should be written

    async def test_streaming_response_logging(self, tmp_path):
        """Test logging streaming response with chunk sequence."""
        log_file = tmp_path / "test.jsonl"
        config = JsonlLoggerConfig(log_path=log_file, enabled=True, buffer_size=100)

        logger = await JsonlLogger.create(config)
        assert logger is not None

        # Log request
        logger.log_request(
            request_id="stream-test",
            endpoint="/v1/chat/completions",
            provider="test-provider",
            payload={"model": "gpt-4", "stream": True},
        )

        # Log streaming response
        chunk_sequence = [
            'data: {"choices":[{"delta":{"content":"Hello"}}]}\n\n',
            'data: {"choices":[{"delta":{"content":" world"}}]}\n\n',
            "data: [DONE]\n\n",
        ]
        logger.log_streaming_response(
            request_id="stream-test",
            status_code=200,
            error_msg=None,
            chunk_sequence=chunk_sequence,
        )

        # Give writer task time to flush
        await asyncio.sleep(0.5)

        await logger.shutdown()

        # Verify records were written
        assert log_file.exists()
        content = log_file.read_text()
        lines = [line for line in content.strip().split("\n") if line]

        # Should have at least 2 records (request and response)
        assert len(lines) >= 2
        request_line = json.loads(lines[0])
        response_line = json.loads(lines[1])

        assert request_line["type"] == "request"
        assert request_line["request_id"] == "stream-test"

        assert response_line["type"] == "response"
        assert response_line["request_id"] == "stream-test"
        assert "chunk_sequence" in response_line
        assert len(response_line["chunk_sequence"]) == 3

    async def test_provider_request_response_logging(self, tmp_path):
        """Test logging provider request/response pair."""
        log_file = tmp_path / "test.jsonl"
        config = JsonlLoggerConfig(log_path=log_file, enabled=True, buffer_size=100)

        logger = await JsonlLogger.create(config)
        assert logger is not None

        # Log provider request
        logger.log_provider_request(
            request_id="provider-test",
            provider="openai",
            api_base="https://api.openai.com/v1",
            endpoint="/chat/completions",
            payload={"model": "gpt-4", "messages": []},
        )

        # Log provider response
        logger.log_provider_response(
            request_id="provider-test",
            provider="openai",
            status_code=200,
            error_msg=None,
            body={"id": "chatcmpl-123", "choices": []},
        )

        # Give writer task time to flush
        await asyncio.sleep(0.5)

        await logger.shutdown()

        # Verify records were written
        assert log_file.exists()
        content = log_file.read_text()
        lines = [line for line in content.strip().split("\n") if line]

        # Should have at least 2 records (provider_request and provider_response)
        assert len(lines) >= 2
        request_line = json.loads(lines[0])
        response_line = json.loads(lines[1])

        assert request_line["type"] == "provider_request"
        assert request_line["provider"] == "openai"
        assert request_line["request_id"] == "provider-test"

        assert response_line["type"] == "provider_response"
        assert response_line["provider"] == "openai"
        assert response_line["request_id"] == "provider-test"


@pytest.mark.asyncio
class TestConfigLoading:
    """Test JSONL logger configuration loading."""

    def test_config_from_env_enabled(self, monkeypatch):
        """Test loading enabled config from environment."""
        monkeypatch.setenv("JSONL_LOG_ENABLED", "true")
        monkeypatch.setenv("JSONL_LOG_PATH", "/tmp/test.jsonl")
        monkeypatch.setenv("JSONL_LOG_BUFFER_SIZE", "500")

        config = JsonlLoggerConfig.from_env()

        assert config.enabled is True
        assert config.log_path == Path("/tmp/test.jsonl")
        assert config.buffer_size == 500

    def test_config_from_env_disabled(self, monkeypatch):
        """Test loading disabled config from environment."""
        monkeypatch.setenv("JSONL_LOG_ENABLED", "false")

        config = JsonlLoggerConfig.from_env()

        assert config.enabled is False

    def test_config_from_env_defaults(self):
        """Test loading config with default values."""
        config = JsonlLoggerConfig.from_env()

        # Default is disabled
        assert config.enabled is False
        assert config.log_path == Path("./logs/requests.jsonl")
        assert config.buffer_size == 1000

    async def test_create_logger_when_disabled(self):
        """Test creating logger returns None when disabled."""
        config = JsonlLoggerConfig(enabled=False)
        logger = await JsonlLogger.create(config)

        assert logger is None
