"""Health check service for testing provider availability"""

import asyncio
import time
from datetime import datetime
from typing import Optional

import httpx
from loguru import logger

from app.core.database import Database, ProviderModel, get_provider_by_id
from app.models.health import (
    HealthStatus,
    ModelHealthStatus,
    ProviderHealthStatus,
)


class HealthCheckService:
    """Service for checking provider health"""

    def __init__(self, db: Database, timeout_secs: int = 10):
        self.db = db
        self.timeout_secs = timeout_secs

    async def check_provider_health(
        self,
        provider: ProviderModel,
        models: Optional[list[str]] = None,
    ) -> ProviderHealthStatus:
        """Check health of a single provider

        Args:
            provider: Provider model to check
            models: List of models to test (None = use default test models)

        Returns:
            ProviderHealthStatus with test results
        """
        # Check if provider is disabled
        if not provider.is_enabled:
            return ProviderHealthStatus(
                provider_id=provider.id,
                provider_key=provider.provider_key,
                status=HealthStatus.DISABLED,
                models=[],
                avg_response_time_ms=None,
                checked_at=datetime.utcnow().isoformat() + "Z",
            )

        # Determine which models to test
        test_models = models or self._get_default_test_models(provider)

        # Test each model sequentially (not concurrently) to avoid overwhelming the provider
        model_statuses = []
        for model in test_models:
            try:
                result = await self._test_model(provider, model)
                model_statuses.append(result)
            except Exception as e:
                logger.error(f"Error testing model {model}: {e}")
                continue

        # Determine overall provider status
        provider_status = self._determine_provider_status(model_statuses)

        # Calculate average response time
        avg_response_time = self._calculate_avg_response_time(model_statuses)

        return ProviderHealthStatus(
            provider_id=provider.id,
            provider_key=provider.provider_key,
            status=provider_status,
            models=model_statuses,
            avg_response_time_ms=avg_response_time,
            checked_at=datetime.utcnow().isoformat() + "Z",
        )

    async def _test_model(
        self,
        provider: ProviderModel,
        model: str,
    ) -> ModelHealthStatus:
        """Test a single model on a provider

        Args:
            provider: Provider to test
            model: Model name to test

        Returns:
            ModelHealthStatus with test result
        """
        # Get the actual model name from mapping
        actual_model = provider.get_model_mapping().get(model, model)

        # Prepare test request (minimal tokens to reduce cost)
        test_payload = {
            "model": actual_model,
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 5,
            "stream": False,
        }

        start_time = time.time()

        try:
            async with httpx.AsyncClient(timeout=self.timeout_secs) as client:
                response = await client.post(
                    f"{provider.api_base}/chat/completions",
                    json=test_payload,
                    headers={
                        "Authorization": f"Bearer {provider.api_key}",
                        "Content-Type": "application/json",
                    },
                )

                elapsed_ms = int((time.time() - start_time) * 1000)

                if response.status_code == 200:
                    return ModelHealthStatus(
                        model=model,
                        status=HealthStatus.HEALTHY,
                        response_time_ms=elapsed_ms,
                        error=None,
                    )
                else:
                    error_msg = f"HTTP {response.status_code}"
                    try:
                        error_data = response.json()
                        if "error" in error_data:
                            error_msg = error_data["error"].get("message", error_msg)
                    except Exception:
                        pass

                    return ModelHealthStatus(
                        model=model,
                        status=HealthStatus.UNHEALTHY,
                        response_time_ms=elapsed_ms,
                        error=error_msg,
                    )

        except asyncio.TimeoutError:
            elapsed_ms = int((time.time() - start_time) * 1000)
            return ModelHealthStatus(
                model=model,
                status=HealthStatus.UNHEALTHY,
                response_time_ms=elapsed_ms,
                error=f"Timeout after {self.timeout_secs}s",
            )
        except Exception as e:
            elapsed_ms = int((time.time() - start_time) * 1000)
            error_msg = str(e)
            # Don't expose sensitive information
            if "api" in error_msg.lower() and "key" in error_msg.lower():
                error_msg = "Authentication error"

            return ModelHealthStatus(
                model=model,
                status=HealthStatus.UNHEALTHY,
                response_time_ms=elapsed_ms,
                error=error_msg,
            )

    def _get_default_test_models(self, provider: ProviderModel) -> list[str]:
        """Get default models to test for a provider

        Args:
            provider: Provider to get test models for

        Returns:
            List of model names to test
        """
        model_mapping = provider.get_model_mapping()

        # If provider has model mapping, test all configured models
        if model_mapping:
            return list(model_mapping.keys())

        # Otherwise, use common model names based on provider type
        provider_type = provider.provider_type.lower()

        if "openai" in provider_type:
            return ["gpt-3.5-turbo"]
        elif "anthropic" in provider_type:
            return ["claude-3-haiku-20240307"]
        elif "azure" in provider_type:
            return ["gpt-35-turbo"]
        else:
            # Generic fallback
            return ["gpt-3.5-turbo"]

    def _determine_provider_status(
        self, model_statuses: list[ModelHealthStatus]
    ) -> HealthStatus:
        """Determine overall provider status from model statuses

        Args:
            model_statuses: List of model health statuses

        Returns:
            Overall provider health status
        """
        if not model_statuses:
            return HealthStatus.UNKNOWN

        # If all models are healthy, provider is healthy
        if all(m.status == HealthStatus.HEALTHY for m in model_statuses):
            return HealthStatus.HEALTHY

        # If any model is healthy, provider is partially healthy (still mark as healthy)
        if any(m.status == HealthStatus.HEALTHY for m in model_statuses):
            return HealthStatus.HEALTHY

        # Otherwise, provider is unhealthy
        return HealthStatus.UNHEALTHY

    def _calculate_avg_response_time(
        self, model_statuses: list[ModelHealthStatus]
    ) -> Optional[int]:
        """Calculate average response time from model statuses

        Args:
            model_statuses: List of model health statuses

        Returns:
            Average response time in milliseconds, or None if no valid times
        """
        valid_times = [
            m.response_time_ms for m in model_statuses if m.response_time_ms is not None
        ]

        if not valid_times:
            return None

        return int(sum(valid_times) / len(valid_times))


async def check_providers_health(
    db: Database,
    provider_ids: Optional[list[int]] = None,
    models: Optional[list[str]] = None,
    timeout_secs: int = 10,
    max_concurrent: int = 2,
) -> list[ProviderHealthStatus]:
    """Check health of multiple providers with controlled concurrency

    Args:
        db: Database instance
        provider_ids: List of provider IDs to check (None = all enabled providers)
        models: List of models to test (None = default test models)
        timeout_secs: Timeout for each model test
        max_concurrent: Maximum number of providers to check concurrently (default: 2)

    Returns:
        List of provider health statuses
    """
    service = HealthCheckService(db, timeout_secs)

    # Get providers to check
    if provider_ids:
        providers = []
        for provider_id in provider_ids:
            provider = await get_provider_by_id(db, provider_id)
            if provider:
                providers.append(provider)
    else:
        # Get all enabled providers
        from app.core.database import list_providers

        providers = await list_providers(db, enabled_only=False)

    # Use semaphore to limit concurrent provider checks
    semaphore = asyncio.Semaphore(max_concurrent)

    async def check_with_limit(provider: ProviderModel) -> ProviderHealthStatus:
        """Check provider health with concurrency limit"""
        async with semaphore:
            return await service.check_provider_health(provider, models)

    # Check all providers with controlled concurrency
    results = await asyncio.gather(
        *[check_with_limit(p) for p in providers],
        return_exceptions=True,
    )

    # Filter out exceptions
    health_statuses = []
    for result in results:
        if isinstance(result, Exception):
            logger.error(f"Error checking provider health: {result}")
            continue
        health_statuses.append(result)

    return health_statuses
