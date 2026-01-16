"""Log service for async background logging of request/response data.

This module provides a non-blocking logging system that queues log entries
and writes them to PostgreSQL in batches to minimize latency impact.
"""

import asyncio
import json
import uuid
from dataclasses import dataclass, field
from datetime import datetime
from typing import Optional, Any

from loguru import logger

from app.core.config import get_env_config


SENSITIVE_FIELDS = {
    "api_key",
    "authorization",
    "x-api-key",
    "password",
    "secret",
    "token",
}


@dataclass
class LogEntry:
    """Log entry data structure"""

    request_id: str
    timestamp: datetime
    credential_id: Optional[int]
    credential_name: str
    provider_id: Optional[int]
    provider_name: str
    endpoint: str
    method: str
    model: Optional[str]
    is_streaming: bool
    status_code: int
    duration_ms: int
    ttft_ms: Optional[int] = None
    prompt_tokens: int = 0
    completion_tokens: int = 0
    total_tokens: int = 0
    request_body: Optional[dict] = None
    response_body: Optional[dict] = None
    error_message: Optional[str] = None
    client_ip: Optional[str] = None
    user_agent: Optional[str] = None


class LogCollector:
    """Collects and preprocesses log entries before queuing."""

    def __init__(self, buffer: "LogBuffer", log_request_bodies: bool = True):
        self.buffer = buffer
        self.log_request_bodies = log_request_bodies

    async def log(self, entry: LogEntry) -> None:
        """Queue a log entry for async writing."""
        try:
            entry = self._apply_privacy_filters(entry)
            entry = self._truncate_bodies(entry)
            await self.buffer.add(entry)
        except Exception as e:
            logger.warning(f"Failed to queue log entry: {e}")

    def _apply_privacy_filters(self, entry: LogEntry) -> LogEntry:
        """Mask sensitive fields in request/response bodies."""
        if entry.request_body:
            entry.request_body = self._mask_sensitive_fields(entry.request_body)
        if entry.response_body:
            entry.response_body = self._mask_sensitive_fields(entry.response_body)
        return entry

    def _mask_sensitive_fields(self, data: Any) -> Any:
        """Recursively mask sensitive fields in a dictionary."""
        if not isinstance(data, dict):
            return data

        result = {}
        for key, value in data.items():
            if key.lower() in SENSITIVE_FIELDS:
                result[key] = "***MASKED***"
            elif isinstance(value, dict):
                result[key] = self._mask_sensitive_fields(value)
            elif isinstance(value, list):
                result[key] = [
                    self._mask_sensitive_fields(item) if isinstance(item, dict) else item
                    for item in value
                ]
            else:
                result[key] = value
        return result

    def _truncate_bodies(self, entry: LogEntry) -> LogEntry:
        """Truncate large request/response bodies."""
        max_size = 65536
        if entry.request_body and self.log_request_bodies:
            body_str = json.dumps(entry.request_body)
            if len(body_str) > max_size:
                entry.request_body = {
                    "_truncated": True,
                    "_size": len(body_str),
                    "_max_size": max_size,
                }

        if entry.response_body and self.log_request_bodies:
            body_str = json.dumps(entry.response_body)
            if len(body_str) > max_size:
                entry.response_body = {
                    "_truncated": True,
                    "_size": len(body_str),
                    "_max_size": max_size,
                }

        if not self.log_request_bodies:
            entry.request_body = None
            entry.response_body = None

        return entry


class LogBuffer:
    """Async queue buffer for log entries with backpressure."""

    def __init__(self, max_size: int = 1000):
        self.queue: asyncio.Queue[LogEntry] = asyncio.Queue(maxsize=max_size)
        self.max_size = max_size
        self._running = False

    async def add(self, entry: LogEntry) -> None:
        """Add entry to buffer with backpressure handling."""
        try:
            self.queue.put_nowait(entry)
        except asyncio.QueueFull:
            logger.warning("Log buffer full, dropping entry")

    async def get_batch(self, batch_size: int, timeout: float) -> list[LogEntry]:
        """Get a batch of entries, waiting up to timeout seconds."""
        entries = []
        try:
            while len(entries) < batch_size:
                try:
                    entry = await asyncio.wait_for(self.queue.get(), timeout=timeout)
                    entries.append(entry)
                except asyncio.TimeoutError:
                    break
        except Exception as e:
            logger.error(f"Error getting log batch: {e}")
        return entries

    async def start(self, writer: "LogWriter") -> None:
        """Start the background buffer processor."""
        self._running = True
        asyncio.create_task(self._process_buffer(writer))

    async def stop(self) -> None:
        """Stop the background buffer processor."""
        self._running = False

    async def _process_buffer(self, writer: "LogWriter") -> None:
        """Background task to process buffer and write batches."""
        while self._running:
            try:
                entries = await self.get_batch(batch_size=100, timeout=5.0)
                if entries:
                    await writer.write_batch(entries)
            except Exception as e:
                logger.error(f"Error processing log buffer: {e}")


class LogWriter:
    """Writes log entries to PostgreSQL in batches."""

    def __init__(self, db: "Database"):
        self.db = db

    async def write_batch(self, entries: list[LogEntry]) -> None:
        """Write a batch of log entries to the database."""
        if not entries:
            return

        try:
            from app.core.database import RequestLogModel, insert_log_entries

            log_data = [
                {
                    "id": str(uuid.uuid4()),
                    "created_at": e.timestamp,
                    "credential_id": e.credential_id,
                    "credential_name": e.credential_name,
                    "provider_id": e.provider_id,
                    "provider_name": e.provider_name,
                    "endpoint": e.endpoint,
                    "method": e.method,
                    "model": e.model,
                    "is_streaming": e.is_streaming,
                    "status_code": e.status_code,
                    "duration_ms": e.duration_ms,
                    "ttft_ms": e.ttft_ms,
                    "prompt_tokens": e.prompt_tokens,
                    "completion_tokens": e.completion_tokens,
                    "total_tokens": e.total_tokens,
                    "request_body": e.request_body,
                    "response_body": e.response_body,
                    "error_message": e.error_message,
                    "client_ip": e.client_ip,
                    "user_agent": e.user_agent,
                }
                for e in entries
            ]

            await insert_log_entries(self.db, log_data)
            logger.debug(f"Wrote {len(entries)} log entries to database")
        except Exception as e:
            logger.error(f"Failed to write log batch: {e}")


class LogService:
    """Singleton log service managing the entire logging pipeline."""

    _instance: Optional["LogService"] = None

    def __init__(self, db: "Database"):
        self.db = db
        env_config = get_env_config()
        self.buffer = LogBuffer(max_size=1000)
        self.writer = LogWriter(db)
        self.collector = LogCollector(
            self.buffer, log_request_bodies=env_config.log_request_bodies
        )
        self._started = False

    @classmethod
    def get_instance(cls) -> Optional["LogService"]:
        """Get the singleton LogService instance."""
        return cls._instance

    @classmethod
    async def initialize(cls, db: "Database") -> "LogService":
        """Initialize the singleton LogService instance."""
        if cls._instance is None:
            cls._instance = cls(db)
            await cls._instance.start()
        return cls._instance

    async def start(self) -> None:
        """Start the log service background writer."""
        if self._started:
            return

        await self.buffer.start(self.writer)
        self._started = True
        logger.info("Log service started")

    async def stop(self) -> None:
        """Stop the log service and flush remaining entries."""
        if not self._started:
            return

        await self.buffer.stop()
        await self._flush_remaining()
        self._started = False
        logger.info("Log service stopped")

    async def _flush_remaining(self) -> None:
        """Flush any remaining entries in the buffer."""
        entries = await self.buffer.get_batch(batch_size=1000, timeout=0.1)
        if entries:
            await self.writer.write_batch(entries)
            logger.info(f"Flushed {len(entries)} remaining log entries")

    async def log_request(self, entry: LogEntry) -> None:
        """Log a request asynchronously."""
        if not self._started:
            logger.warning("Log service not started, dropping log entry")
            return

        await self.collector.log(entry)


def get_log_service() -> Optional[LogService]:
    """Get the global LogService instance."""
    return LogService.get_instance()
