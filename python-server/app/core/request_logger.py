"""Async request logger that batches ALL request records into the database."""

import asyncio
import os
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Optional

from app.core.error_logger import _truncate_body
from app.core.logging import get_logger

logger = get_logger()

_REQUEST_LOG_BODY_ENABLED = os.environ.get(
    "REQUEST_LOG_BODY_ENABLED", "false"
).lower() in ("true", "1", "yes", "on")


@dataclass
class RequestLogRecord:
    request_id: str
    endpoint: Optional[str] = None
    credential_name: Optional[str] = None
    model_requested: Optional[str] = None
    model_mapped: Optional[str] = None
    provider_name: Optional[str] = None
    provider_type: Optional[str] = None
    client_protocol: Optional[str] = None
    provider_protocol: Optional[str] = None
    is_streaming: bool = False
    status_code: Optional[int] = None
    input_tokens: int = 0
    output_tokens: int = 0
    total_tokens: int = 0
    total_duration_ms: Optional[int] = None
    ttft_ms: Optional[int] = None
    error_category: Optional[str] = None
    error_message: Optional[str] = None
    request_headers: Optional[str] = None
    request_body: Optional[str] = None
    response_body: Optional[str] = None
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))


class RequestLogger:
    def __init__(self) -> None:
        self._queue: asyncio.Queue[RequestLogRecord] = asyncio.Queue(maxsize=1000)
        self._task: Optional[asyncio.Task[None]] = None
        self._running = False
        self._dropped = 0

    async def start(self) -> None:
        self._running = True
        self._task = asyncio.create_task(self._writer_loop())
        logger.info(
            "Request logger started "
            f"(body_logging={'enabled' if _REQUEST_LOG_BODY_ENABLED else 'disabled'})"
        )

    def log(self, record: RequestLogRecord) -> None:
        record.request_headers = _truncate_body(record.request_headers)
        if not _REQUEST_LOG_BODY_ENABLED:
            record.request_body = None
            record.response_body = None
        else:
            record.request_body = _truncate_body(record.request_body)
            record.response_body = _truncate_body(record.response_body)
        try:
            self._queue.put_nowait(record)
        except asyncio.QueueFull:
            self._dropped += 1
            if self._dropped % 100 == 1:
                logger.warning(f"Request log queue full, dropped {self._dropped} total")

    async def _writer_loop(self) -> None:
        buffer: list[RequestLogRecord] = []
        while self._running or not self._queue.empty():
            try:
                try:
                    record = await asyncio.wait_for(self._queue.get(), timeout=2.0)
                    buffer.append(record)
                    if len(buffer) >= 50:
                        await self._flush(buffer)
                        buffer = []
                except asyncio.TimeoutError:
                    if buffer:
                        await self._flush(buffer)
                        buffer = []
            except asyncio.CancelledError:
                if buffer:
                    await self._flush(buffer)
                break
            except Exception as e:
                logger.error(f"Error in request logger writer loop: {e}")

        if buffer:
            await self._flush(buffer)
        logger.info("Request logger writer task stopped")

    async def _flush(self, buffer: list[RequestLogRecord]) -> None:
        if not buffer:
            return
        try:
            from app.core.database import RequestLogModel, get_database

            db = get_database()
            if db is None:
                logger.warning("Database not available, discarding request log records")
                return

            async with db.session() as session:
                models = []
                for rec in buffer:
                    models.append(
                        RequestLogModel(
                            timestamp=rec.timestamp,
                            request_id=rec.request_id,
                            endpoint=rec.endpoint,
                            credential_name=rec.credential_name,
                            model_requested=rec.model_requested,
                            model_mapped=rec.model_mapped,
                            provider_name=rec.provider_name,
                            provider_type=rec.provider_type,
                            client_protocol=rec.client_protocol,
                            provider_protocol=rec.provider_protocol,
                            is_streaming=rec.is_streaming,
                            status_code=rec.status_code,
                            input_tokens=rec.input_tokens,
                            output_tokens=rec.output_tokens,
                            total_tokens=rec.total_tokens,
                            total_duration_ms=rec.total_duration_ms,
                            ttft_ms=rec.ttft_ms,
                            error_category=rec.error_category,
                            error_message=rec.error_message,
                            request_headers=rec.request_headers,
                            request_body=rec.request_body,
                            response_body=rec.response_body,
                        )
                    )
                session.add_all(models)
        except Exception as e:
            logger.error(f"Failed to flush request log records: {e}")

    async def shutdown(self) -> None:
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass


_request_logger: Optional[RequestLogger] = None


async def init_request_logger() -> None:
    global _request_logger
    enabled = os.environ.get("REQUEST_LOG_ENABLED", "true").lower() in (
        "true",
        "1",
        "yes",
        "on",
    )
    if not enabled:
        logger.info("Request logging is disabled")
        return
    _request_logger = RequestLogger()
    await _request_logger.start()


def get_request_logger() -> Optional[RequestLogger]:
    return _request_logger


async def shutdown_request_logger() -> None:
    global _request_logger
    if _request_logger:
        await _request_logger.shutdown()
        _request_logger = None


def log_request_record(
    *,
    request_id: str,
    endpoint: Optional[str] = None,
    credential_name: Optional[str] = None,
    model_requested: Optional[str] = None,
    model_mapped: Optional[str] = None,
    provider_name: Optional[str] = None,
    provider_type: Optional[str] = None,
    client_protocol: Optional[str] = None,
    provider_protocol: Optional[str] = None,
    is_streaming: bool = False,
    status_code: Optional[int] = None,
    input_tokens: int = 0,
    output_tokens: int = 0,
    total_tokens: int = 0,
    total_duration_ms: Optional[int] = None,
    ttft_ms: Optional[int] = None,
    error_category: Optional[str] = None,
    error_message: Optional[str] = None,
    request_headers: Optional[str] = None,
    request_body: Optional[str] = None,
    response_body: Optional[str] = None,
) -> None:
    rl = get_request_logger()
    if rl is None:
        return
    record = RequestLogRecord(
        request_id=request_id,
        endpoint=endpoint,
        credential_name=credential_name,
        model_requested=model_requested,
        model_mapped=model_mapped,
        provider_name=provider_name,
        provider_type=provider_type,
        client_protocol=client_protocol,
        provider_protocol=provider_protocol,
        is_streaming=is_streaming,
        status_code=status_code,
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        total_tokens=total_tokens,
        total_duration_ms=total_duration_ms,
        ttft_ms=ttft_ms,
        error_category=error_category,
        error_message=error_message,
        request_headers=request_headers,
        request_body=request_body,
        response_body=response_body,
    )
    rl.log(record)
