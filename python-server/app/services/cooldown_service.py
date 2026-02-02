"""Provider cooldown management service.

Manages temporary provider disabling based on error responses.
Uses in-memory storage with TTL-based automatic expiration.
Thread-safe implementation using locks.
"""

from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Optional
import threading
import time

from loguru import logger


@dataclass
class CooldownEntry:
    """Cooldown cache value for a provider.

    Attributes:
        provider_key: Unique provider identifier
        exception_type: Type of exception (rate_limit, auth_error, timeout, server_error)
        status_code: HTTP status code that triggered cooldown
        timestamp: Unix timestamp when cooldown started
        cooldown_time: Cooldown duration in seconds
        error_message: Optional error message for logging
    """

    provider_key: str
    exception_type: str
    status_code: int
    timestamp: float
    cooldown_time: int
    error_message: Optional[str] = None

    @property
    def expires_at(self) -> float:
        """Calculate expiration timestamp."""
        return self.timestamp + self.cooldown_time

    @property
    def is_expired(self) -> bool:
        """Check if cooldown has expired."""
        return time.time() >= self.expires_at

    @property
    def remaining_seconds(self) -> int:
        """Get remaining cooldown seconds."""
        remaining = self.expires_at - time.time()
        return max(0, int(remaining))

    @property
    def started_at_iso(self) -> str:
        """Get start time in ISO format."""
        return datetime.fromtimestamp(self.timestamp, tz=timezone.utc).isoformat()

    @property
    def expires_at_iso(self) -> str:
        """Get expiration time in ISO format."""
        return datetime.fromtimestamp(self.expires_at, tz=timezone.utc).isoformat()


@dataclass
class CooldownConfig:
    """Cooldown configuration.

    Attributes:
        enabled: Whether cooldown feature is enabled
        default_cooldown_secs: Default cooldown duration in seconds
        max_cooldown_secs: Maximum cooldown time cap
        cooldown_status_codes: Status codes that trigger cooldown
        non_cooldown_status_codes: Status codes that should NOT trigger cooldown
        cooldown_durations: Status code specific cooldown durations
    """

    enabled: bool = True
    default_cooldown_secs: int = 60
    max_cooldown_secs: int = 600

    # Status codes that trigger cooldown: only 429 (rate limit) and 5xx (server errors)
    cooldown_status_codes: set[int] = field(
        default_factory=lambda: {
            429,  # Rate limit - provider is overloaded
            500,  # Internal server error
            501,  # Not implemented
            502,  # Bad gateway
            503,  # Service unavailable
            504,  # Gateway timeout
        }
    )

    # Status codes that should NOT trigger cooldown (client errors, auth errors)
    # 4xx errors (except 429) are typically client-side issues, not provider issues
    non_cooldown_status_codes: set[int] = field(
        default_factory=lambda: {400, 401, 403, 404, 408, 422}
    )

    # Status code specific cooldown durations
    cooldown_durations: dict[int, int] = field(
        default_factory=lambda: {
            429: 60,  # Rate limit - 1 minute
            500: 30,  # Internal server error - 30 seconds
            502: 30,  # Bad gateway - 30 seconds
            503: 60,  # Service unavailable - 1 minute
            504: 30,  # Gateway timeout - 30 seconds
        }
    )


def _get_exception_type(status_code: int) -> str:
    """Determine exception type based on status code."""
    if status_code == 429:
        return "rate_limit"
    elif 500 <= status_code < 600:
        return "server_error"
    else:
        return "unknown_error"


class CooldownService:
    """Provider cooldown management service.

    Manages temporary provider disabling based on error responses.
    Uses in-memory storage with TTL-based automatic expiration.
    Thread-safe implementation using locks.
    """

    def __init__(self, config: Optional[CooldownConfig] = None):
        self._config = config or CooldownConfig()
        self._cooldowns: dict[str, CooldownEntry] = {}
        self._lock = threading.Lock()

    @property
    def config(self) -> CooldownConfig:
        """Get current cooldown configuration."""
        return self._config

    def update_config(self, config: CooldownConfig) -> None:
        """Update cooldown configuration."""
        with self._lock:
            self._config = config

    def should_trigger_cooldown(self, status_code: int) -> bool:
        """Check if a status code should trigger cooldown.

        Args:
            status_code: HTTP status code from provider response

        Returns:
            True if cooldown should be triggered
        """
        if not self._config.enabled:
            return False

        # Explicitly excluded codes (client errors)
        if status_code in self._config.non_cooldown_status_codes:
            return False

        # Explicitly included codes
        if status_code in self._config.cooldown_status_codes:
            return True

        # Default: trigger for 5xx errors
        return 500 <= status_code < 600

    def add_cooldown(
        self,
        provider_key: str,
        status_code: int,
        error_message: Optional[str] = None,
        cooldown_time: Optional[int] = None,
    ) -> CooldownEntry:
        """Add a provider to cooldown.

        Args:
            provider_key: Unique provider identifier
            status_code: HTTP status code that triggered cooldown
            error_message: Optional error message for logging
            cooldown_time: Override cooldown duration

        Returns:
            Created CooldownEntry
        """
        exception_type = _get_exception_type(status_code)

        if cooldown_time is None:
            cooldown_time = self._config.cooldown_durations.get(
                status_code, self._config.default_cooldown_secs
            )

        # Cap cooldown time
        cooldown_time = min(cooldown_time, self._config.max_cooldown_secs)

        entry = CooldownEntry(
            provider_key=provider_key,
            exception_type=exception_type,
            status_code=status_code,
            timestamp=time.time(),
            cooldown_time=cooldown_time,
            error_message=error_message,
        )

        with self._lock:
            self._cooldowns[provider_key] = entry

        logger.warning(
            f"Provider '{provider_key}' entered cooldown: "
            f"status_code={status_code}, type={exception_type}, "
            f"duration={cooldown_time}s"
        )

        return entry

    def is_in_cooldown(self, provider_key: str) -> bool:
        """Check if a provider is currently in cooldown.

        Args:
            provider_key: Unique provider identifier

        Returns:
            True if provider is in cooldown (not expired)
        """
        if not self._config.enabled:
            return False

        with self._lock:
            entry = self._cooldowns.get(provider_key)
            if entry is None:
                return False

            if entry.is_expired:
                # Auto-cleanup expired entry
                del self._cooldowns[provider_key]
                logger.info(
                    f"Provider '{provider_key}' cooldown expired, now available"
                )
                return False

            return True

    def get_cooldown(self, provider_key: str) -> Optional[CooldownEntry]:
        """Get cooldown entry for a provider.

        Args:
            provider_key: Unique provider identifier

        Returns:
            CooldownEntry if in cooldown, None otherwise
        """
        with self._lock:
            entry = self._cooldowns.get(provider_key)
            if entry is None:
                return None

            if entry.is_expired:
                del self._cooldowns[provider_key]
                return None

            return entry

    def remove_cooldown(self, provider_key: str) -> bool:
        """Manually remove a provider from cooldown.

        Args:
            provider_key: Unique provider identifier

        Returns:
            True if cooldown was removed, False if not found
        """
        with self._lock:
            if provider_key in self._cooldowns:
                del self._cooldowns[provider_key]
                logger.info(f"Provider '{provider_key}' cooldown manually removed")
                return True
            return False

    def get_all_cooldowns(self) -> dict[str, CooldownEntry]:
        """Get all active cooldowns (excluding expired).

        Returns:
            Dict of provider_key -> CooldownEntry
        """
        with self._lock:
            # Cleanup expired entries
            expired_keys = [k for k, v in self._cooldowns.items() if v.is_expired]
            for key in expired_keys:
                del self._cooldowns[key]
                logger.info(f"Provider '{key}' cooldown expired, now available")

            # Return copy to avoid modification issues
            return dict(self._cooldowns)

    def clear_all_cooldowns(self) -> int:
        """Clear all cooldowns (admin operation).

        Returns:
            Number of cooldowns cleared
        """
        with self._lock:
            count = len(self._cooldowns)
            self._cooldowns.clear()
            if count > 0:
                logger.warning(f"All cooldowns cleared by admin: {count} entries")
            return count

    def filter_available_providers(
        self,
        providers: list[Any],
        weights: list[int],
    ) -> tuple[list[Any], list[int]]:
        """Filter out providers that are in cooldown.

        Args:
            providers: List of Provider objects
            weights: Corresponding weights

        Returns:
            Tuple of (filtered_providers, filtered_weights)
        """
        if not self._config.enabled:
            return providers, weights

        # Get all cooldowns once with a single lock acquisition
        with self._lock:
            # Cleanup expired entries first
            expired_keys = [k for k, v in self._cooldowns.items() if v.is_expired]
            for key in expired_keys:
                del self._cooldowns[key]
                logger.info(f"Provider '{key}' cooldown expired, now available")

            # Take snapshot of current cooldowns
            cooldown_snapshot = dict(self._cooldowns)

        filtered_providers = []
        filtered_weights = []

        for provider, weight in zip(providers, weights):
            provider_name = getattr(provider, "name", str(provider))
            entry = cooldown_snapshot.get(provider_name)

            if entry is None:
                filtered_providers.append(provider)
                filtered_weights.append(weight)
            else:
                logger.debug(
                    f"Provider '{provider_name}' skipped (in cooldown, "
                    f"{entry.remaining_seconds}s remaining)"
                )

        return filtered_providers, filtered_weights


# Global singleton
_cooldown_service: Optional[CooldownService] = None


def get_cooldown_service() -> CooldownService:
    """Get global cooldown service instance."""
    global _cooldown_service
    if _cooldown_service is None:
        _cooldown_service = CooldownService()
    return _cooldown_service


def init_cooldown_service(config: Optional[CooldownConfig] = None) -> CooldownService:
    """Initialize cooldown service with config."""
    global _cooldown_service
    _cooldown_service = CooldownService(config)
    return _cooldown_service


def trigger_cooldown_if_needed(
    provider_key: str,
    status_code: int,
    error_message: Optional[str] = None,
) -> Optional[CooldownEntry]:
    """Trigger cooldown for provider if status code warrants it.

    This is a convenience function for use in error handling code.

    Args:
        provider_key: Provider that returned the error
        status_code: HTTP status code from provider
        error_message: Optional error message for logging

    Returns:
        CooldownEntry if cooldown was triggered, None otherwise
    """
    cooldown_svc = get_cooldown_service()

    if not cooldown_svc.should_trigger_cooldown(status_code):
        return None

    return cooldown_svc.add_cooldown(
        provider_key=provider_key,
        status_code=status_code,
        error_message=error_message,
    )
