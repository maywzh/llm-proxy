"""Async error logger that batches error records into the database."""

import asyncio
import json
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Optional

from app.core.logging import get_logger

logger = get_logger()

_MAX_STRING_LEN = 200

_SENSITIVE_HEADERS = re.compile(
    r"^(authorization|x-api-key|cookie|set-cookie|proxy-authorization)$",
    re.IGNORECASE,
)


class ErrorCategory:
    PROVIDER_4XX = "provider_4xx"
    PROVIDER_5XX = "provider_5xx"
    TIMEOUT = "timeout"
    NETWORK_ERROR = "network_error"
    CONNECT_ERROR = "connect_error"
    STREAM_ERROR = "stream_error"
    INTERNAL_ERROR = "internal_error"


@dataclass
class ErrorLogRecord:
    error_category: str
    error_message: Optional[str] = None
    error_code: Optional[int] = None
    request_id: Optional[str] = None
    provider_name: Optional[str] = None
    credential_name: Optional[str] = None
    model_requested: Optional[str] = None
    model_mapped: Optional[str] = None
    endpoint: Optional[str] = None
    client_protocol: Optional[str] = None
    provider_protocol: Optional[str] = None
    is_streaming: Optional[bool] = None
    request_body: Optional[str] = None
    response_body: Optional[str] = None
    total_duration_ms: Optional[int] = None
    provider_request_body: Optional[str] = None
    provider_request_headers: Optional[str] = None
    timestamp: datetime = field(default_factory=lambda: datetime.now(timezone.utc))


def mask_headers(headers: dict) -> dict:
    masked = {}
    for k, v in headers.items():
        if _SENSITIVE_HEADERS.match(k):
            masked[k] = "***"
        else:
            masked[k] = v
    return masked


_MAX_TRUNCATE_DEPTH = 10


def truncate_json_strings(value: Any, _depth: int = _MAX_TRUNCATE_DEPTH) -> Any:
    """Recursively truncate long string values in a JSON-like structure.

    Preserves the complete JSON shape. If a string value is itself valid JSON,
    parse and recurse into it, then serialize back. Plain strings longer than
    _MAX_STRING_LEN are shortened; keys, numbers, booleans, None are kept intact.
    """
    if isinstance(value, str):
        if _depth > 0 and (value.startswith("{") or value.startswith("[")):
            try:
                parsed = json.loads(value)
                truncated = truncate_json_strings(parsed, _depth - 1)
                return json.dumps(truncated, ensure_ascii=False)
            except (json.JSONDecodeError, ValueError):
                pass
        byte_len = len(value.encode("utf-8"))
        if byte_len <= _MAX_STRING_LEN:
            return value
        encoded = value.encode("utf-8")
        end = _MAX_STRING_LEN
        while end > 0 and (encoded[end] & 0xC0) == 0x80:
            end -= 1
        return f"{encoded[:end].decode('utf-8')}... [truncated, {byte_len} bytes total]"
    if isinstance(value, dict):
        return {k: truncate_json_strings(v, _depth) for k, v in value.items()}
    if isinstance(value, list):
        return [truncate_json_strings(item, _depth) for item in value]
    return value


def _truncate_body(body: Optional[str]) -> Optional[str]:
    if body is None:
        return None
    try:
        parsed = json.loads(body)
        truncated = truncate_json_strings(parsed)
        return json.dumps(truncated, ensure_ascii=False)
    except (json.JSONDecodeError, ValueError):
        byte_len = len(body.encode("utf-8"))
        if byte_len <= _MAX_STRING_LEN:
            return body
        encoded = body.encode("utf-8")
        end = _MAX_STRING_LEN
        while end > 0 and (encoded[end] & 0xC0) == 0x80:
            end -= 1
        return f"{encoded[:end].decode('utf-8')}... [truncated, {byte_len} bytes total]"


class ErrorLogger:
    def __init__(self):
        self._queue: asyncio.Queue[ErrorLogRecord] = asyncio.Queue(maxsize=500)
        self._task: Optional[asyncio.Task] = None
        self._running = False
        self._dropped = 0

    async def start(self):
        self._running = True
        self._task = asyncio.create_task(self._writer_loop())
        logger.info("Error logger started")

    def log_error(self, record: ErrorLogRecord):
        record.request_body = _truncate_body(record.request_body)
        record.response_body = _truncate_body(record.response_body)
        record.provider_request_body = _truncate_body(record.provider_request_body)
        record.provider_request_headers = _truncate_body(
            record.provider_request_headers
        )
        try:
            self._queue.put_nowait(record)
        except asyncio.QueueFull:
            self._dropped += 1
            if self._dropped % 100 == 1:
                logger.warning(f"Error log queue full, dropped {self._dropped} total")

    async def _writer_loop(self):
        buffer: list[ErrorLogRecord] = []
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
                logger.error(f"Error in error logger writer loop: {e}")

        if buffer:
            await self._flush(buffer)
        logger.info("Error logger writer task stopped")

    async def _flush(self, buffer: list[ErrorLogRecord]):
        if not buffer:
            return
        try:
            from app.core.database import get_database, ErrorLogModel

            db = get_database()
            if db is None:
                logger.warning("Database not available, discarding error log records")
                return

            async with db.session() as session:
                for rec in buffer:
                    model = ErrorLogModel(
                        timestamp=rec.timestamp,
                        request_id=rec.request_id,
                        error_category=rec.error_category,
                        error_code=rec.error_code,
                        error_message=rec.error_message,
                        provider_name=rec.provider_name,
                        credential_name=rec.credential_name,
                        model_requested=rec.model_requested,
                        model_mapped=rec.model_mapped,
                        endpoint=rec.endpoint,
                        client_protocol=rec.client_protocol,
                        provider_protocol=rec.provider_protocol,
                        is_streaming=rec.is_streaming,
                        request_body=rec.request_body,
                        response_body=rec.response_body,
                        total_duration_ms=rec.total_duration_ms,
                        provider_request_body=rec.provider_request_body,
                        provider_request_headers=rec.provider_request_headers,
                    )
                    session.add(model)
        except Exception as e:
            logger.error(f"Failed to flush error log records: {e}")

    async def shutdown(self):
        self._running = False
        if self._task:
            self._task.cancel()
            try:
                await self._task
            except asyncio.CancelledError:
                pass


_error_logger: Optional[ErrorLogger] = None


async def init_error_logger():
    global _error_logger
    _error_logger = ErrorLogger()
    await _error_logger.start()


def get_error_logger() -> Optional[ErrorLogger]:
    return _error_logger


async def shutdown_error_logger():
    global _error_logger
    if _error_logger:
        await _error_logger.shutdown()
        _error_logger = None


def log_error(
    *,
    error_category: str,
    error_message: Optional[str] = None,
    error_code: Optional[int] = None,
    request_id: Optional[str] = None,
    provider_name: Optional[str] = None,
    credential_name: Optional[str] = None,
    model_requested: Optional[str] = None,
    model_mapped: Optional[str] = None,
    endpoint: Optional[str] = None,
    client_protocol: Optional[str] = None,
    provider_protocol: Optional[str] = None,
    is_streaming: Optional[bool] = None,
    request_body: Optional[str] = None,
    response_body: Optional[str] = None,
    total_duration_ms: Optional[int] = None,
    provider_request_body: Optional[str] = None,
    provider_request_headers: Optional[str] = None,
):
    el = get_error_logger()
    if el is None:
        return
    record = ErrorLogRecord(
        error_category=error_category,
        error_message=error_message,
        error_code=error_code,
        request_id=request_id,
        provider_name=provider_name,
        credential_name=credential_name,
        model_requested=model_requested,
        model_mapped=model_mapped,
        endpoint=endpoint,
        client_protocol=client_protocol,
        provider_protocol=provider_protocol,
        is_streaming=is_streaming,
        request_body=request_body,
        response_body=response_body,
        total_duration_ms=total_duration_ms,
        provider_request_body=provider_request_body,
        provider_request_headers=provider_request_headers,
    )
    el.log_error(record)
