"""
Unified stream metrics recording.

Single responsibility: convert StreamStats to Prometheus metrics.

Records:
- TPS (tokens per second): output_tokens / (now - first_token_time)
- TTFT (time to first token): first_token_time - start_time
- Token Usage: input/output/total tokens
"""

import logging
import time
from dataclasses import dataclass, field
from typing import Optional

from app.core.metrics import TOKENS_PER_SECOND, TTFT, TOKEN_USAGE

logger = logging.getLogger(__name__)


@dataclass
class StreamingUsageTracker:
    """Tracks token usage and timing during streaming for metrics recording at exit."""

    input_tokens: int = 0
    output_tokens: int = 0
    usage_from_provider: bool = False
    start_time: float = field(default_factory=time.time)
    first_token_time: Optional[float] = None


@dataclass(frozen=True)
class StreamStats:
    """Stream statistics - pure data structure, immutable."""

    model: str
    provider: str
    api_key_name: str
    client: str
    input_tokens: int
    output_tokens: int
    start_time: float
    first_token_time: Optional[float]


def record_stream_metrics(stats: StreamStats) -> None:
    """
    Record all stream metrics in one place.

    Single responsibility: convert StreamStats to Prometheus metrics.
    """
    now = time.time()

    # TPS (only if we have first token and output tokens)
    if stats.first_token_time is not None and stats.output_tokens > 0:
        duration = now - stats.first_token_time
        if duration > 0:
            tps = stats.output_tokens / duration
            TOKENS_PER_SECOND.labels(
                source="provider",
                model=stats.model,
                provider=stats.provider,
            ).observe(tps)
            logger.debug(
                f"Stream TPS - model={stats.model} provider={stats.provider} "
                f"tokens={stats.output_tokens} duration={duration:.3f}s tps={tps:.2f}"
            )

    # TTFT (only if we have first token time)
    if stats.first_token_time is not None:
        ttft = stats.first_token_time - stats.start_time
        TTFT.labels(
            source="provider",
            model=stats.model,
            provider=stats.provider,
        ).observe(ttft)
        logger.debug(
            f"Stream TTFT - model={stats.model} provider={stats.provider} "
            f"ttft={ttft:.3f}s"
        )

    # Token Usage
    if stats.input_tokens > 0 or stats.output_tokens > 0:
        TOKEN_USAGE.labels(
            model=stats.model,
            provider=stats.provider,
            token_type="prompt",
            api_key_name=stats.api_key_name,
            client=stats.client,
        ).inc(stats.input_tokens)

        TOKEN_USAGE.labels(
            model=stats.model,
            provider=stats.provider,
            token_type="completion",
            api_key_name=stats.api_key_name,
            client=stats.client,
        ).inc(stats.output_tokens)

        TOKEN_USAGE.labels(
            model=stats.model,
            provider=stats.provider,
            token_type="total",
            api_key_name=stats.api_key_name,
            client=stats.client,
        ).inc(stats.input_tokens + stats.output_tokens)

        logger.debug(
            f"Stream tokens - model={stats.model} provider={stats.provider} "
            f"client={stats.client} input={stats.input_tokens} output={stats.output_tokens}"
        )
