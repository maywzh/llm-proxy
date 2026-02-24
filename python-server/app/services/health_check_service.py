"""Health check service for testing provider availability"""

import asyncio
import time
from datetime import datetime
from typing import Optional

from loguru import logger

from app.core.database import Database, ProviderModel, get_provider_by_id
from app.core.http_client import get_http_client
from app.models.health import (
    CheckProviderHealthResponse,
    HealthStatus,
    ModelHealthStatus,
    ProviderHealthStatus,
    ProviderHealthSummary,
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

    async def check_provider_health_concurrent(
        self,
        provider: ProviderModel,
        models: Optional[list[str]] = None,
        max_concurrent: int = 2,
    ) -> CheckProviderHealthResponse:
        """Check health of a single provider with concurrent model testing

        Args:
            provider: Provider model to check
            models: List of models to test (None = use all mapped models)
            max_concurrent: Maximum number of models to test concurrently (default: 2)

        Returns:
            CheckProviderHealthResponse with test results and summary
        """
        # Check if provider is disabled
        if not provider.is_enabled:
            return CheckProviderHealthResponse(
                provider_id=provider.id,
                provider_key=provider.provider_key,
                status=HealthStatus.DISABLED,
                models=[],
                summary=ProviderHealthSummary(
                    total_models=0,
                    healthy_models=0,
                    unhealthy_models=0,
                ),
                avg_response_time_ms=None,
                checked_at=datetime.utcnow().isoformat() + "Z",
            )

        # Determine which models to test
        test_models = models or self._get_all_mapped_models(provider)

        if not test_models:
            return CheckProviderHealthResponse(
                provider_id=provider.id,
                provider_key=provider.provider_key,
                status=HealthStatus.UNKNOWN,
                models=[],
                summary=ProviderHealthSummary(
                    total_models=0,
                    healthy_models=0,
                    unhealthy_models=0,
                ),
                avg_response_time_ms=None,
                checked_at=datetime.utcnow().isoformat() + "Z",
            )

        # Use semaphore to limit concurrent model tests
        semaphore = asyncio.Semaphore(max_concurrent)

        async def test_with_limit(model: str) -> ModelHealthStatus:
            """Test a model with concurrency limit"""
            async with semaphore:
                return await self._test_model(provider, model)

        # Run all model tests with controlled concurrency
        results = await asyncio.gather(
            *[test_with_limit(m) for m in test_models],
            return_exceptions=True,
        )

        # Process results and filter out exceptions
        model_statuses = []
        for i, result in enumerate(results):
            if isinstance(result, Exception):
                logger.error(f"Error testing model {test_models[i]}: {result}")
                model_statuses.append(
                    ModelHealthStatus(
                        model=test_models[i],
                        status=HealthStatus.UNHEALTHY,
                        response_time_ms=None,
                        error=str(result),
                    )
                )
            else:
                model_statuses.append(result)

        # Calculate summary statistics
        healthy_count = sum(
            1 for m in model_statuses if m.status == HealthStatus.HEALTHY
        )
        unhealthy_count = len(model_statuses) - healthy_count

        summary = ProviderHealthSummary(
            total_models=len(model_statuses),
            healthy_models=healthy_count,
            unhealthy_models=unhealthy_count,
        )

        # Determine overall provider status
        provider_status = self._determine_provider_status(model_statuses)

        # Calculate average response time
        avg_response_time = self._calculate_avg_response_time(model_statuses)

        return CheckProviderHealthResponse(
            provider_id=provider.id,
            provider_key=provider.provider_key,
            status=provider_status,
            models=model_statuses,
            summary=summary,
            avg_response_time_ms=avg_response_time,
            checked_at=datetime.utcnow().isoformat() + "Z",
        )

    def _get_all_mapped_models(self, provider: ProviderModel) -> list[str]:
        """Get all models from provider's model mapping

        Args:
            provider: Provider to get models for

        Returns:
            List of model names (keys from model_mapping)
        """
        model_mapping = provider.get_model_mapping()
        return list(model_mapping.keys()) if model_mapping else []

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

        provider_type = provider.provider_type.lower()
        test_payload = {
            "model": actual_model,
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 5,
            "stream": False,
        }

        if "vertex" in provider_type:
            # GCP Vertex AI: endpoint with configurable action verb
            params = provider.provider_params or {}
            gcp_project = params.get("gcp_project", "")
            gcp_location = params.get("gcp_location", "us-central1")
            gcp_publisher = params.get("gcp_publisher", "anthropic")

            actions = params.get("gcp_vertex_actions", {})
            if isinstance(actions, dict):
                blocking_action = actions.get("blocking", "rawPredict")
            else:
                blocking_action = "rawPredict"

            url = (
                f"{provider.api_base}/v1/projects/{gcp_project}"
                f"/locations/{gcp_location}/publishers/{gcp_publisher}"
                f"/models/{actual_model}:{blocking_action}"
            )
            headers = {
                "Authorization": f"Bearer {provider.api_key}",
                "Content-Type": "application/json",
                "anthropic-version": "vertex-2023-10-16",
            }
        elif provider_type in ("gemini", "gcp-gemini"):
            # Gemini: uses Gemini format (contents instead of messages)
            params = provider.provider_params or {}
            gcp_project = params.get("gcp_project", "")
            gcp_location = params.get("gcp_location", "us-central1")
            gcp_publisher = params.get("gcp_publisher", "google")

            url = (
                f"{provider.api_base}/v1/projects/{gcp_project}"
                f"/locations/{gcp_location}/publishers/{gcp_publisher}"
                f"/models/{actual_model}:generateContent"
            )
            headers = {
                "Authorization": f"Bearer {provider.api_key}",
                "Content-Type": "application/json",
            }
            test_payload = {
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "generationConfig": {"maxOutputTokens": 1},
            }
        elif provider_type in ("anthropic", "claude"):
            # Anthropic: x-api-key auth + /v1/messages endpoint
            url = f"{provider.api_base}/v1/messages"
            headers = {
                "x-api-key": provider.api_key,
                "anthropic-version": "2023-06-01",
                "Content-Type": "application/json",
            }
        elif provider_type in ("response_api", "response-api", "responses"):
            # Response API: Bearer auth + /responses endpoint
            url = f"{provider.api_base}/responses"
            headers = {
                "Authorization": f"Bearer {provider.api_key}",
            }
            test_payload = {
                "model": actual_model,
                "input": [{"role": "user", "content": "Hi"}],
                "max_output_tokens": 5,
                "stream": True,
            }
        else:
            # OpenAI-compatible (openai, azure, etc.): Bearer auth + /chat/completions
            url = f"{provider.api_base}/chat/completions"
            headers = {
                "Authorization": f"Bearer {provider.api_key}",
            }

        # Apply custom headers from provider_params
        custom_headers = (provider.provider_params or {}).get("custom_headers")
        if isinstance(custom_headers, dict):
            headers.update(custom_headers)

        start_time = time.time()

        try:
            # Use shared HTTP client with per-request timeout override
            client = get_http_client()
            response = await client.post(
                url,
                json=test_payload,
                headers=headers,
                timeout=self.timeout_secs,
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
        elif provider_type in ("gemini", "gcp-gemini"):
            return ["gemini-2.0-flash"]
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

        providers = await list_providers(db, enabled_only=True)

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
