"""JSONL file logging for request/response pairs.

This module provides async JSONL logging for debugging and analysis.
Requests and responses are logged as separate JSONL lines, linked by request_id.
"""

import asyncio
import json
import os
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Optional, Union

from app.core.logging import get_logger

logger = get_logger()


# =============================================================================
# Configuration
# =============================================================================


@dataclass
class JsonlLoggerConfig:
    """JSONL logger configuration."""

    log_path: Path = field(default_factory=lambda: Path("./logs/requests.jsonl"))
    enabled: bool = False
    buffer_size: int = 1000

    @classmethod
    def from_env(cls) -> "JsonlLoggerConfig":
        """Load configuration from environment variables."""
        enabled_str = os.environ.get("JSONL_LOG_ENABLED", "false").lower()
        enabled = enabled_str in ("true", "1", "yes", "on")

        log_path = Path(os.environ.get("JSONL_LOG_PATH", "./logs/requests.jsonl"))

        buffer_size_str = os.environ.get("JSONL_LOG_BUFFER_SIZE", "1000")
        try:
            buffer_size = int(buffer_size_str)
        except ValueError:
            buffer_size = 1000

        return cls(log_path=log_path, enabled=enabled, buffer_size=buffer_size)


# =============================================================================
# Record Types
# =============================================================================


@dataclass
class RequestRecord:
    """Request record - logged immediately when request is received."""

    request_id: str
    endpoint: str
    provider: str
    payload: dict[str, Any]
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    record_type: str = "request"

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        return {
            "type": self.record_type,
            "timestamp": self.timestamp.isoformat(),
            "request_id": self.request_id,
            "endpoint": self.endpoint,
            "provider": self.provider,
            "payload": self.payload,
        }


@dataclass
class ResponseRecord:
    """Response record - logged when response is completed."""

    request_id: str
    status_code: int
    error_msg: Optional[str] = None
    body: Optional[dict[str, Any]] = None
    chunk_sequence: Optional[list[str]] = None
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    record_type: str = "response"

    @classmethod
    def new_non_streaming(
        cls,
        request_id: str,
        status_code: int,
        error_msg: Optional[str],
        body: dict[str, Any],
    ) -> "ResponseRecord":
        """Create a new non-streaming response record."""
        return cls(
            request_id=request_id,
            status_code=status_code,
            error_msg=error_msg,
            body=body,
            chunk_sequence=None,
        )

    @classmethod
    def new_streaming(
        cls,
        request_id: str,
        status_code: int,
        error_msg: Optional[str],
        chunk_sequence: list[str],
    ) -> "ResponseRecord":
        """Create a new streaming response record."""
        return cls(
            request_id=request_id,
            status_code=status_code,
            error_msg=error_msg,
            body=None,
            chunk_sequence=chunk_sequence,
        )

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        result = {
            "type": self.record_type,
            "timestamp": self.timestamp.isoformat(),
            "request_id": self.request_id,
            "status_code": self.status_code,
        }
        if self.error_msg is not None:
            result["error_msg"] = self.error_msg
        if self.body is not None:
            result["body"] = self.body
        if self.chunk_sequence is not None:
            result["chunk_sequence"] = self.chunk_sequence
        return result


@dataclass
class ProviderRequestRecord:
    """Provider request record - logged when request is sent to provider."""

    request_id: str
    provider: str
    api_base: str
    endpoint: str
    payload: dict[str, Any]
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    record_type: str = "provider_request"

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        return {
            "type": self.record_type,
            "timestamp": self.timestamp.isoformat(),
            "request_id": self.request_id,
            "provider": self.provider,
            "api_base": self.api_base,
            "endpoint": self.endpoint,
            "payload": self.payload,
        }


@dataclass
class ProviderResponseRecord:
    """Provider response record - logged when response is received from provider."""

    request_id: str
    provider: str
    status_code: int
    error_msg: Optional[str] = None
    body: Optional[dict[str, Any]] = None
    chunk_sequence: Optional[list[str]] = None
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    record_type: str = "provider_response"

    @classmethod
    def new_non_streaming(
        cls,
        request_id: str,
        provider: str,
        status_code: int,
        error_msg: Optional[str],
        body: dict[str, Any],
    ) -> "ProviderResponseRecord":
        """Create a new non-streaming provider response record."""
        return cls(
            request_id=request_id,
            provider=provider,
            status_code=status_code,
            error_msg=error_msg,
            body=body,
            chunk_sequence=None,
        )

    @classmethod
    def new_streaming(
        cls,
        request_id: str,
        provider: str,
        status_code: int,
        error_msg: Optional[str],
        chunk_sequence: list[str],
    ) -> "ProviderResponseRecord":
        """Create a new streaming provider response record."""
        return cls(
            request_id=request_id,
            provider=provider,
            status_code=status_code,
            error_msg=error_msg,
            body=None,
            chunk_sequence=chunk_sequence,
        )

    def to_dict(self) -> dict[str, Any]:
        """Convert to dictionary for JSON serialization."""
        result = {
            "type": self.record_type,
            "timestamp": self.timestamp.isoformat(),
            "request_id": self.request_id,
            "provider": self.provider,
            "status_code": self.status_code,
        }
        if self.error_msg is not None:
            result["error_msg"] = self.error_msg
        if self.body is not None:
            result["body"] = self.body
        if self.chunk_sequence is not None:
            result["chunk_sequence"] = self.chunk_sequence
        return result


# Union type for log records
LogRecord = Union[
    RequestRecord, ResponseRecord, ProviderRequestRecord, ProviderResponseRecord
]


# =============================================================================
# Logger Implementation
# =============================================================================


class JsonlLogger:
    """Async JSONL logger with buffered writes."""

    def __init__(self, config: JsonlLoggerConfig):
        """Initialize the logger (private, use create() instead)."""
        self._config = config
        self._queue: asyncio.Queue[LogRecord] = asyncio.Queue(
            maxsize=config.buffer_size
        )
        self._writer_task: Optional[asyncio.Task] = None
        self._running = False
        self._dropped_count = 0

    @classmethod
    async def create(cls, config: JsonlLoggerConfig) -> Optional["JsonlLogger"]:
        """Create a new JSONL logger.

        Returns None if logging is disabled.
        """
        if not config.enabled:
            logger.info("JSONL logging is disabled")
            return None

        # Ensure parent directory exists
        parent = config.log_path.parent
        if parent:
            try:
                parent.mkdir(parents=True, exist_ok=True)
            except OSError as e:
                logger.error(f"Failed to create JSONL log directory: {e}")
                return None

        instance = cls(config)
        instance._running = True
        instance._writer_task = asyncio.create_task(instance._writer_loop())

        logger.info(f"JSONL logging enabled, writing to: {config.log_path}")
        return instance

    async def _writer_loop(self):
        """Background task that writes records to the file."""
        buffer: list[LogRecord] = []
        flush_interval = 1.0  # seconds

        while self._running or not self._queue.empty():
            try:
                # Wait for records with timeout for periodic flush
                try:
                    record = await asyncio.wait_for(
                        self._queue.get(), timeout=flush_interval
                    )
                    buffer.append(record)

                    # Flush if buffer is full
                    if len(buffer) >= 100:
                        await self._flush_buffer(buffer)
                        buffer = []
                except asyncio.TimeoutError:
                    # Periodic flush
                    if buffer:
                        await self._flush_buffer(buffer)
                        buffer = []

            except asyncio.CancelledError:
                # Flush remaining on shutdown
                if buffer:
                    await self._flush_buffer(buffer)
                break
            except Exception as e:
                logger.error(f"Error in JSONL writer loop: {e}")

        # Final flush
        if buffer:
            await self._flush_buffer(buffer)

        logger.info("JSONL logger writer task stopped")

    async def _flush_buffer(self, buffer: list[LogRecord]):
        """Flush buffered records to file."""
        if not buffer:
            return

        output_lines = []
        for record in buffer:
            try:
                json_str = json.dumps(record.to_dict(), ensure_ascii=False)
                output_lines.append(json_str)
            except Exception as e:
                logger.error(f"Failed to serialize JSONL record: {e}")

        if output_lines:
            output = "\n".join(output_lines) + "\n"
            try:
                # Use asyncio file I/O
                loop = asyncio.get_event_loop()
                await loop.run_in_executor(None, self._write_to_file, output)
            except Exception as e:
                logger.error(f"Failed to write to JSONL log file: {e}")

    def _write_to_file(self, content: str):
        """Synchronous file write (called in executor)."""
        with open(self._config.log_path, "a", encoding="utf-8") as f:
            f.write(content)
            f.flush()

    def log(self, record: LogRecord):
        """Log a record (non-blocking)."""
        try:
            self._queue.put_nowait(record)
        except asyncio.QueueFull:
            self._dropped_count += 1
            if self._dropped_count % 100 == 1:
                logger.warning(
                    f"JSONL log queue is full, dropped {self._dropped_count} records total"
                )

    def log_request(
        self,
        request_id: str,
        endpoint: str,
        provider: str,
        payload: dict[str, Any],
    ):
        """Log a request immediately when received."""
        record = RequestRecord(
            request_id=request_id,
            endpoint=endpoint,
            provider=provider,
            payload=payload,
        )
        self.log(record)

    def log_response(
        self,
        request_id: str,
        status_code: int,
        error_msg: Optional[str],
        body: dict[str, Any],
    ):
        """Log a non-streaming response when completed."""
        record = ResponseRecord.new_non_streaming(
            request_id=request_id,
            status_code=status_code,
            error_msg=error_msg,
            body=body,
        )
        self.log(record)

    def log_streaming_response(
        self,
        request_id: str,
        status_code: int,
        error_msg: Optional[str],
        chunk_sequence: list[str],
    ):
        """Log a streaming response when stream completes."""
        record = ResponseRecord.new_streaming(
            request_id=request_id,
            status_code=status_code,
            error_msg=error_msg,
            chunk_sequence=chunk_sequence,
        )
        self.log(record)

    def log_provider_request(
        self,
        request_id: str,
        provider: str,
        api_base: str,
        endpoint: str,
        payload: dict[str, Any],
    ):
        """Log a provider request when sent to upstream."""
        record = ProviderRequestRecord(
            request_id=request_id,
            provider=provider,
            api_base=api_base,
            endpoint=endpoint,
            payload=payload,
        )
        self.log(record)

    def log_provider_response(
        self,
        request_id: str,
        provider: str,
        status_code: int,
        error_msg: Optional[str],
        body: dict[str, Any],
    ):
        """Log a non-streaming provider response when received."""
        record = ProviderResponseRecord.new_non_streaming(
            request_id=request_id,
            provider=provider,
            status_code=status_code,
            error_msg=error_msg,
            body=body,
        )
        self.log(record)

    def log_provider_streaming_response(
        self,
        request_id: str,
        provider: str,
        status_code: int,
        error_msg: Optional[str],
        chunk_sequence: list[str],
    ):
        """Log a streaming provider response when stream completes."""
        record = ProviderResponseRecord.new_streaming(
            request_id=request_id,
            provider=provider,
            status_code=status_code,
            error_msg=error_msg,
            chunk_sequence=chunk_sequence,
        )
        self.log(record)

    def is_enabled(self) -> bool:
        """Check if logging is enabled."""
        return self._config.enabled

    @property
    def log_path(self) -> Path:
        """Get the log file path."""
        return self._config.log_path

    async def shutdown(self):
        """Shutdown the logger gracefully."""
        self._running = False
        if self._writer_task:
            self._writer_task.cancel()
            try:
                await self._writer_task
            except asyncio.CancelledError:
                pass


# =============================================================================
# Global Logger Instance
# =============================================================================

_jsonl_logger: Optional[JsonlLogger] = None


async def init_jsonl_logger():
    """Initialize the global JSONL logger."""
    global _jsonl_logger
    config = JsonlLoggerConfig.from_env()
    _jsonl_logger = await JsonlLogger.create(config)


def get_jsonl_logger() -> Optional[JsonlLogger]:
    """Get the global JSONL logger."""
    return _jsonl_logger


async def shutdown_jsonl_logger():
    """Shutdown the global JSONL logger."""
    global _jsonl_logger
    if _jsonl_logger:
        await _jsonl_logger.shutdown()
        _jsonl_logger = None


# =============================================================================
# Helper Functions
# =============================================================================


def log_request(request_id: str, endpoint: str, provider: str, payload: dict[str, Any]):
    """Log a request immediately when received."""
    if logger_instance := get_jsonl_logger():
        logger_instance.log_request(request_id, endpoint, provider, payload)


def log_response(
    request_id: str, status_code: int, error_msg: Optional[str], body: dict[str, Any]
):
    """Log a non-streaming response when completed."""
    if logger_instance := get_jsonl_logger():
        logger_instance.log_response(request_id, status_code, error_msg, body)


def log_streaming_response(
    request_id: str,
    status_code: int,
    error_msg: Optional[str],
    chunk_sequence: list[str],
):
    """Log a streaming response when stream completes."""
    if logger_instance := get_jsonl_logger():
        logger_instance.log_streaming_response(
            request_id, status_code, error_msg, chunk_sequence
        )


def log_provider_request(
    request_id: str,
    provider: str,
    api_base: str,
    endpoint: str,
    payload: dict[str, Any],
):
    """Log a provider request when sent to upstream."""
    if logger_instance := get_jsonl_logger():
        logger_instance.log_provider_request(
            request_id, provider, api_base, endpoint, payload
        )


def log_provider_response(
    request_id: str,
    provider: str,
    status_code: int,
    error_msg: Optional[str],
    body: dict[str, Any],
):
    """Log a non-streaming provider response when received."""
    if logger_instance := get_jsonl_logger():
        logger_instance.log_provider_response(
            request_id, provider, status_code, error_msg, body
        )


def log_provider_streaming_response(
    request_id: str,
    provider: str,
    status_code: int,
    error_msg: Optional[str],
    chunk_sequence: list[str],
):
    """Log a streaming provider response when stream completes."""
    if logger_instance := get_jsonl_logger():
        logger_instance.log_provider_streaming_response(
            request_id, provider, status_code, error_msg, chunk_sequence
        )
