"""Provider selection and management service with adaptive routing."""

import enum
import os
import random
import threading
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from typing import Optional

from app.core.config import get_config
from app.core.error_types import (
    EJECTION_REASON_RATE_LIMIT,
    EJECTION_REASON_SERVER_5XX,
    EJECTION_REASON_TRANSPORT,
)
from app.core.logging import get_logger
from app.core.metrics import (
    PROVIDER_CIRCUIT_STATE,
    PROVIDER_EFFECTIVE_WEIGHT,
    PROVIDER_EJECTIONS_TOTAL,
    PROVIDER_HEALTH,
)
from app.models.provider import Provider, _is_pattern

logger = get_logger()

CIRCUIT_CLOSED = "closed"
CIRCUIT_OPEN = "open"
CIRCUIT_HALF_OPEN = "half_open"


class CircuitState(enum.Enum):
    CLOSED = CIRCUIT_CLOSED
    OPEN = CIRCUIT_OPEN
    HALF_OPEN = CIRCUIT_HALF_OPEN


@dataclass
class ProviderRuntimeState:
    multiplier: float = 1.0
    circuit_state: CircuitState = field(default=CircuitState.CLOSED)
    cooldown_until: Optional[float] = None
    open_until: Optional[float] = None
    recovery_started_at: Optional[float] = None
    half_open_successes: int = 0
    half_open_in_flight: int = 0
    consecutive_429: int = 0
    consecutive_5xx: int = 0
    consecutive_transport: int = 0
    ejection_count: int = 0


@dataclass
class AdaptiveRoutingConfig:
    enabled: bool
    min_multiplier: float
    half_open_weight_factor: float
    success_recovery_step: float
    slow_start_window: float
    consecutive_429_threshold: int
    consecutive_5xx_threshold: int
    consecutive_transport_threshold: int
    half_open_success_threshold: int
    half_open_max_probes: int
    base_429_cooldown: float
    max_429_cooldown: float
    base_open_duration: float
    max_open_duration: float

    @classmethod
    def from_env(cls) -> "AdaptiveRoutingConfig":
        enabled = _env_bool("ADAPTIVE_ROUTING_ENABLED", False)
        return cls(
            enabled=enabled,
            min_multiplier=_env_float("ADAPTIVE_MIN_MULTIPLIER", 0.05),
            half_open_weight_factor=_env_float("ADAPTIVE_HALF_OPEN_WEIGHT_FACTOR", 0.2),
            success_recovery_step=_env_float("ADAPTIVE_SUCCESS_RECOVERY_STEP", 0.1),
            slow_start_window=_env_float("ADAPTIVE_SLOW_START_WINDOW_SECS", 60.0),
            consecutive_429_threshold=_env_int("ADAPTIVE_429_THRESHOLD", 3),
            consecutive_5xx_threshold=_env_int("ADAPTIVE_5XX_THRESHOLD", 5),
            consecutive_transport_threshold=_env_int("ADAPTIVE_TRANSPORT_THRESHOLD", 4),
            half_open_success_threshold=_env_int(
                "ADAPTIVE_HALF_OPEN_SUCCESS_THRESHOLD", 2
            ),
            half_open_max_probes=_env_int("ADAPTIVE_HALF_OPEN_MAX_PROBES", 1),
            base_429_cooldown=_env_float("ADAPTIVE_BASE_429_COOLDOWN_SECS", 15.0),
            max_429_cooldown=_env_float("ADAPTIVE_MAX_429_COOLDOWN_SECS", 300.0),
            base_open_duration=_env_float("ADAPTIVE_BASE_OPEN_DURATION_SECS", 30.0),
            max_open_duration=_env_float("ADAPTIVE_MAX_OPEN_DURATION_SECS", 300.0),
        )


class ProviderService:
    """Manages provider selection with weighted load balancing and adaptive routing.

    Supports dynamic configuration updates via reinitialize().
    When ADAPTIVE_ROUTING_ENABLED=true, adjusts provider weights at runtime
    based on 429/5xx/transport errors using a circuit breaker state machine.
    """

    def __init__(self):
        self._providers: list[Provider] = []
        self._weights: list[int] = []
        self._initialized = False
        self._runtime_states: dict[str, ProviderRuntimeState] = {}
        self._lock = threading.Lock()
        self._adaptive_config = AdaptiveRoutingConfig.from_env()

    def initialize(self) -> None:
        if self._initialized:
            return
        self._load_providers()
        self._initialized = True

    def reinitialize(self) -> None:
        with self._lock:
            old_states = self._runtime_states.copy()
        self._load_providers()
        with self._lock:
            new_states = {}
            for p in self._providers:
                new_states[p.name] = old_states.get(p.name, ProviderRuntimeState())
            self._runtime_states = new_states
        self._initialized = True

    def _load_providers(self) -> None:
        config = get_config()
        self._providers = [
            Provider(
                name=p.name,
                api_base=p.api_base,
                api_key=p.api_key,
                weight=p.weight,
                model_mapping=p.model_mapping,
                provider_type=p.provider_type,
                provider_params=p.provider_params,
            )
            for p in config.providers
        ]
        self._weights = [p.weight for p in self._providers]
        with self._lock:
            for p in self._providers:
                if p.name not in self._runtime_states:
                    self._runtime_states[p.name] = ProviderRuntimeState()
        if self._adaptive_config.enabled:
            for p in self._providers:
                self._update_runtime_metrics(
                    p.name, float(p.weight), CircuitState.CLOSED
                )

    def get_next_provider(self, model: Optional[str] = None) -> Provider:
        if not self._initialized:
            self.initialize()

        if self._adaptive_config.enabled:
            return self._get_next_provider_adaptive(model)

        if model is None:
            return random.choices(self._providers, weights=self._weights, k=1)[0]

        available_providers = []
        available_weights = []
        for provider, weight in zip(self._providers, self._weights):
            if provider.supports_model(model):
                available_providers.append(provider)
                available_weights.append(weight)

        if not available_providers:
            raise ValueError(f"No provider supports model: {model}")

        return random.choices(available_providers, weights=available_weights, k=1)[0]

    def report_http_status(
        self,
        provider_name: str,
        status_code: int,
        retry_after: Optional[str] = None,
    ) -> None:
        if not self._adaptive_config.enabled:
            return
        if status_code < 400:
            self.report_success(provider_name)
            return
        if status_code == 429:
            self._on_provider_429(provider_name, parse_retry_after_seconds(retry_after))
            return
        if status_code >= 500:
            self._on_provider_5xx(provider_name)
            return
        # 4xx (non-429): only 408 penalizes
        with self._lock:
            state = self._runtime_states.get(provider_name)
            if state is None:
                return
            if state.circuit_state == CircuitState.HALF_OPEN:
                state.half_open_in_flight = max(0, state.half_open_in_flight - 1)
            state.consecutive_429 = 0
            state.consecutive_5xx = 0
            state.consecutive_transport = 0
            if status_code == 408:
                state.multiplier = max(
                    state.multiplier * 0.9, self._adaptive_config.min_multiplier
                )
                state.half_open_successes = 0
            multiplier = state.multiplier
            circuit_state = state.circuit_state
        self._update_runtime_metrics(provider_name, multiplier, circuit_state)

    def report_transport_error(self, provider_name: str) -> None:
        if not self._adaptive_config.enabled:
            return
        now = time.monotonic()
        with self._lock:
            state = self._runtime_states.get(provider_name)
            if state is None:
                return
            state.consecutive_429 = 0
            state.consecutive_5xx = 0
            state.consecutive_transport += 1
            state.multiplier = max(
                state.multiplier * 0.7, self._adaptive_config.min_multiplier
            )
            state.half_open_successes = 0
            if state.circuit_state == CircuitState.HALF_OPEN:
                state.half_open_in_flight = max(0, state.half_open_in_flight - 1)
            should_open = (
                state.circuit_state == CircuitState.HALF_OPEN
                or state.consecutive_transport
                >= self._adaptive_config.consecutive_transport_threshold
            )
            if should_open:
                open_duration = next_backoff(
                    state.ejection_count,
                    self._adaptive_config.base_open_duration,
                    self._adaptive_config.max_open_duration,
                )
                self._open_circuit(
                    provider_name, state, now, EJECTION_REASON_TRANSPORT, open_duration
                )
                return
            multiplier = state.multiplier
            circuit_state = state.circuit_state
        self._update_runtime_metrics(provider_name, multiplier, circuit_state)

    def report_success(self, provider_name: str) -> None:
        if not self._adaptive_config.enabled:
            return
        now = time.monotonic()
        with self._lock:
            state = self._runtime_states.get(provider_name)
            if state is None:
                return
            self._promote_open_to_half_open_if_needed(state, now)
            state.consecutive_429 = 0
            state.consecutive_5xx = 0
            state.consecutive_transport = 0
            state.cooldown_until = None
            if state.circuit_state == CircuitState.HALF_OPEN:
                state.half_open_in_flight = max(0, state.half_open_in_flight - 1)
                state.half_open_successes += 1
                if (
                    state.half_open_successes
                    >= self._adaptive_config.half_open_success_threshold
                ):
                    state.circuit_state = CircuitState.CLOSED
                    state.half_open_successes = 0
                    state.half_open_in_flight = 0
                    state.ejection_count = 0
                    state.recovery_started_at = now
                    state.multiplier = max(state.multiplier, 0.3)
            else:
                state.multiplier = min(
                    state.multiplier + self._adaptive_config.success_recovery_step, 1.0
                )
            multiplier = state.multiplier
            circuit_state = state.circuit_state
        self._update_runtime_metrics(provider_name, multiplier, circuit_state)

    def adaptive_enabled(self) -> bool:
        return self._adaptive_config.enabled

    def get_all_providers(self) -> list[Provider]:
        if not self._initialized:
            self.initialize()
        return self._providers

    def get_provider_weights(self) -> list[int]:
        if not self._initialized:
            self.initialize()
        return self._weights

    def get_all_models(self) -> set[str]:
        if not self._initialized:
            self.initialize()
        models = set()
        for provider in self._providers:
            for key in provider.model_mapping.keys():
                if not _is_pattern(key):
                    models.add(key)
        return models

    def log_providers(self) -> None:
        total_weight = sum(self._weights) or 1
        logger.info(f"Starting LLM API Proxy with {len(self._providers)} providers")
        for i, provider in enumerate(self._providers):
            w = self._weights[i]
            pct = (w / total_weight) * 100.0
            logger.info(f"  - {provider.name}: weight={w} ({pct:.1f}%)")
        if self._adaptive_config.enabled:
            logger.info("Adaptive provider routing is enabled")

    # -- private methods --

    def _get_next_provider_adaptive(self, model: Optional[str]) -> Provider:
        now = time.monotonic()
        eligible_providers: list[Provider] = []
        eligible_weights: list[int] = []
        fallback_candidates: list[tuple[Provider, float]] = []
        metrics_updates: list[tuple[str, float, CircuitState]] = []

        with self._lock:
            for provider, weight in zip(self._providers, self._weights):
                if model is not None and not provider.supports_model(model):
                    continue
                effective_weight = float(weight)
                eligible = True
                state = self._runtime_states.get(provider.name)
                if state:
                    promoted = self._promote_open_to_half_open_if_needed(state, now)
                    if state.cooldown_until is not None:
                        if now < state.cooldown_until:
                            eligible = False
                        else:
                            state.cooldown_until = None
                    if state.circuit_state == CircuitState.OPEN:
                        eligible = False
                    if (
                        state.circuit_state == CircuitState.HALF_OPEN
                        and state.half_open_in_flight
                        >= self._adaptive_config.half_open_max_probes
                    ):
                        eligible = False
                    state_factor = (
                        self._adaptive_config.half_open_weight_factor
                        if state.circuit_state == CircuitState.HALF_OPEN
                        else 1.0
                    )
                    slow_start_factor = self._get_slow_start_factor(state, now)
                    effective_weight *= (
                        state.multiplier * state_factor * slow_start_factor
                    )
                    effective_weight = max(
                        effective_weight, self._adaptive_config.min_multiplier
                    )
                    if promoted:
                        metrics_updates.append(
                            (provider.name, state.multiplier, state.circuit_state)
                        )
                fallback_candidates.append((provider, effective_weight))
                if eligible:
                    scaled = max(1, round(effective_weight * 1000))
                    eligible_providers.append(provider)
                    eligible_weights.append(scaled)

        for name, mult, cs in metrics_updates:
            self._update_runtime_metrics(name, mult, cs)

        if not eligible_providers:
            return self._select_probe_fallback(model, fallback_candidates)

        selected = random.choices(eligible_providers, weights=eligible_weights, k=1)[0]

        with self._lock:
            state = self._runtime_states.get(selected.name)
            if state and state.circuit_state == CircuitState.HALF_OPEN:
                state.half_open_in_flight += 1

        return selected

    def _select_probe_fallback(
        self,
        model: Optional[str],
        fallback_candidates: list[tuple[Provider, float]],
    ) -> Provider:
        if not fallback_candidates:
            if model is not None:
                raise ValueError(f"No provider supports model: {model}")
            raise ValueError("No provider configured")

        selected = max(fallback_candidates, key=lambda x: x[1])[0]
        logger.warning(
            f"All providers are temporarily degraded; "
            f"selecting probe provider={selected.name} model={model or '*'}"
        )
        return selected

    def _on_provider_429(
        self, provider_name: str, retry_after_secs: Optional[int]
    ) -> None:
        now = time.monotonic()
        with self._lock:
            state = self._runtime_states.get(provider_name)
            if state is None:
                return
            state.consecutive_429 += 1
            state.consecutive_5xx = 0
            state.consecutive_transport = 0
            state.half_open_successes = 0
            state.multiplier = max(
                state.multiplier * 0.5, self._adaptive_config.min_multiplier
            )
            fallback_cooldown = next_backoff(
                max(0, state.consecutive_429 - 1),
                self._adaptive_config.base_429_cooldown,
                self._adaptive_config.max_429_cooldown,
            )
            retry_after_cooldown = (
                float(retry_after_secs) if retry_after_secs is not None else None
            )
            cooldown = min(
                retry_after_cooldown
                if retry_after_cooldown is not None
                else fallback_cooldown,
                self._adaptive_config.max_429_cooldown,
            )
            state.cooldown_until = now + cooldown
            if state.circuit_state == CircuitState.HALF_OPEN:
                state.half_open_in_flight = max(0, state.half_open_in_flight - 1)
            should_open = (
                state.circuit_state == CircuitState.HALF_OPEN
                or state.consecutive_429
                >= self._adaptive_config.consecutive_429_threshold
            )
            if should_open:
                self._open_circuit(
                    provider_name,
                    state,
                    now,
                    EJECTION_REASON_RATE_LIMIT,
                    cooldown,
                )
                return
            multiplier = state.multiplier
            circuit_state = state.circuit_state
        self._update_runtime_metrics(provider_name, multiplier, circuit_state)

    def _on_provider_5xx(self, provider_name: str) -> None:
        now = time.monotonic()
        with self._lock:
            state = self._runtime_states.get(provider_name)
            if state is None:
                return
            state.consecutive_429 = 0
            state.consecutive_transport = 0
            state.consecutive_5xx += 1
            state.half_open_successes = 0
            state.multiplier = max(
                state.multiplier * 0.7, self._adaptive_config.min_multiplier
            )
            if state.circuit_state == CircuitState.HALF_OPEN:
                state.half_open_in_flight = max(0, state.half_open_in_flight - 1)
            should_open = (
                state.circuit_state == CircuitState.HALF_OPEN
                or state.consecutive_5xx
                >= self._adaptive_config.consecutive_5xx_threshold
            )
            if should_open:
                open_duration = next_backoff(
                    state.ejection_count,
                    self._adaptive_config.base_open_duration,
                    self._adaptive_config.max_open_duration,
                )
                self._open_circuit(
                    provider_name,
                    state,
                    now,
                    EJECTION_REASON_SERVER_5XX,
                    open_duration,
                )
                return
            multiplier = state.multiplier
            circuit_state = state.circuit_state
        self._update_runtime_metrics(provider_name, multiplier, circuit_state)

    def _open_circuit(
        self,
        provider_name: str,
        state: ProviderRuntimeState,
        now: float,
        reason: str,
        open_duration: float,
    ) -> None:
        """Transition provider to Open. Must be called with self._lock held."""
        state.circuit_state = CircuitState.OPEN
        state.half_open_successes = 0
        state.half_open_in_flight = 0
        state.ejection_count += 1
        state.open_until = now + open_duration
        state.recovery_started_at = None
        PROVIDER_EJECTIONS_TOTAL.labels(provider=provider_name, reason=reason).inc()
        # Release lock before metrics I/O is not needed since Prometheus
        # client handles its own locking, but we still update outside for consistency
        self._update_runtime_metrics(provider_name, 0.0, state.circuit_state)

    def _promote_open_to_half_open_if_needed(
        self, state: ProviderRuntimeState, now: float
    ) -> bool:
        """Check and promote Open -> HalfOpen if timer expired. Must hold lock."""
        if state.circuit_state == CircuitState.OPEN:
            if state.open_until is not None and now >= state.open_until:
                state.circuit_state = CircuitState.HALF_OPEN
                state.open_until = None
                state.cooldown_until = None
                state.half_open_successes = 0
                state.half_open_in_flight = 0
                state.recovery_started_at = now
                state.multiplier = max(
                    state.multiplier,
                    self._adaptive_config.half_open_weight_factor,
                )
                return True
        return False

    def _get_slow_start_factor(self, state: ProviderRuntimeState, now: float) -> float:
        """Calculate slow-start weight factor during recovery. Must hold lock."""
        if state.circuit_state != CircuitState.CLOSED:
            return 1.0
        if state.recovery_started_at is None:
            return 1.0
        elapsed = now - state.recovery_started_at
        if elapsed >= self._adaptive_config.slow_start_window:
            state.recovery_started_at = None
            return 1.0
        progress = elapsed / self._adaptive_config.slow_start_window
        return max(progress, self._adaptive_config.half_open_weight_factor)

    def _update_runtime_metrics(
        self,
        provider_name: str,
        effective_weight: float,
        circuit_state: CircuitState,
    ) -> None:
        PROVIDER_EFFECTIVE_WEIGHT.labels(provider=provider_name).set(
            max(0.0, effective_weight)
        )
        for s in (CIRCUIT_CLOSED, CIRCUIT_OPEN, CIRCUIT_HALF_OPEN):
            val = 1.0 if s == circuit_state.value else 0.0
            PROVIDER_CIRCUIT_STATE.labels(provider=provider_name, state=s).set(val)
        health = 0.0 if circuit_state == CircuitState.OPEN else 1.0
        PROVIDER_HEALTH.labels(provider=provider_name).set(health)


# -- module-level helpers --


def next_backoff(step: int, base: float, max_val: float) -> float:
    exponent = min(step, 10)
    multiplier = 1 << exponent
    return min(base * multiplier, max_val)


def parse_retry_after_seconds(value: Optional[str]) -> Optional[int]:
    if not value or not value.strip():
        return None
    raw = value.strip()
    try:
        return int(raw)
    except ValueError:
        pass
    try:
        parsed = parsedate_to_datetime(raw)
        now = datetime.now(timezone.utc)
        if parsed <= now:
            return 0
        return int((parsed - now).total_seconds())
    except Exception:
        return None


def _env_bool(key: str, default: bool) -> bool:
    v = os.environ.get(key, "")
    return v.lower() in ("1", "true", "yes", "on") if v else default


def _env_float(key: str, default: float) -> float:
    try:
        return float(os.environ.get(key, ""))
    except (ValueError, TypeError):
        return default


def _env_int(key: str, default: int) -> int:
    try:
        return int(os.environ.get(key, ""))
    except (ValueError, TypeError):
        return default


_provider_service: ProviderService | None = None


def get_provider_service() -> ProviderService:
    global _provider_service
    if _provider_service is None:
        _provider_service = ProviderService()
    return _provider_service
