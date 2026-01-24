"""Rate limiting service for master API keys"""

from typing import Dict, Optional
from limits import parse, storage, strategies
from loguru import logger

from app.models.config import RateLimitConfig


class RateLimiter:
    """Rate limiter for managing per-key request limits using token bucket algorithm"""

    def __init__(self):
        """Initialize the rate limiter with in-memory storage"""
        self._storage = storage.MemoryStorage()
        self._limiters: Dict[str, any] = {}
        self._moving_window = strategies.MovingWindowRateLimiter(self._storage)

    def register_key(
        self,
        key: str,
        requests_per_second: Optional[int] = None,
        burst_size: Optional[int] = None,
        config: Optional[RateLimitConfig] = None,
    ) -> None:
        """
        Register a new key with rate limiting

        Args:
            key: The API key to register
            requests_per_second: Requests per second limit (if config not provided)
            burst_size: Burst size (if config not provided)
            config: Rate limit configuration object (alternative to individual params)
        """
        if config is not None:
            rps = config.requests_per_second
            burst = config.burst_size
        elif requests_per_second is not None and burst_size is not None:
            rps = requests_per_second
            burst = burst_size
        else:
            raise ValueError(
                "Either config or both requests_per_second and burst_size must be provided"
            )

        # Create rate limit using parse function
        # Format: "{amount}/{period}" where period can be second, minute, hour, day
        rate_limit = parse(f"{rps}/second")
        self._limiters[key] = rate_limit
        logger.info(f"Registered rate limit for key: {rps} req/s (burst: {burst})")

        # Warn users when burst_size differs from requests_per_second
        # The limits library's MovingWindow strategy doesn't support burst
        if burst != rps:
            logger.warning(
                f"burst_size ({burst}) differs from requests_per_second ({rps}). "
                f"Using requests_per_second as the effective limit due to library limitations. "
                f"The burst_size parameter is not supported by the MovingWindow rate limiting strategy."
            )

    def check_rate_limit(self, key: str) -> bool:
        """
        Check if a request is allowed for the given key

        Args:
            key: The API key to check

        Returns:
            True if the request is allowed, False if rate limit is exceeded
        """
        if key not in self._limiters:
            # No rate limit configured for this key - allow access
            # This is correct behavior: keys without rate limits should pass through
            logger.debug(
                f"Rate limit check for unregistered key - allowing (no limit configured)"
            )
            return True

        rate_limit = self._limiters[key]
        # Use the key itself as the identifier for the rate limiter
        return self._moving_window.hit(rate_limit, key)

    def remove_key(self, key: str) -> None:
        """
        Remove a key from rate limiting

        Args:
            key: The API key to remove
        """
        if key in self._limiters:
            del self._limiters[key]
            # Also clear from storage
            self._storage.reset()

    def clear(self) -> None:
        """Clear all rate limiters"""
        self._limiters.clear()
        self._storage.reset()


# Global rate limiter instance
_rate_limiter: Optional[RateLimiter] = None


def get_rate_limiter() -> RateLimiter:
    """Get the global rate limiter instance"""
    global _rate_limiter
    if _rate_limiter is None:
        _rate_limiter = RateLimiter()
    return _rate_limiter


def init_rate_limiter() -> RateLimiter:
    """Initialize and return a new rate limiter instance"""
    global _rate_limiter
    _rate_limiter = RateLimiter()
    return _rate_limiter
