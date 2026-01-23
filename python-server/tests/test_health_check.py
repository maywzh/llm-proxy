"""Tests for health check functionality"""

import asyncio
import os
from datetime import datetime
from unittest.mock import AsyncMock, MagicMock, patch

import httpx
import pytest
from fastapi import status

from app.core.database import Database, ProviderModel
from app.models.health import (
    CheckProviderHealthRequest,
    CheckProviderHealthResponse,
    HealthCheckRequest,
    HealthCheckResponse,
    HealthStatus,
    ModelHealthStatus,
    ProviderHealthStatus,
    ProviderHealthSummary,
)
from app.services.health_check_service import (
    HealthCheckService,
    check_providers_health,
)


# Fixtures for API tests
@pytest.fixture
def admin_headers():
    """Admin authentication headers"""
    admin_key = os.getenv("ADMIN_KEY", "test-admin-key")
    return {"Authorization": f"Bearer {admin_key}"}


@pytest.fixture
def mock_db():
    """Create mock database"""
    db = MagicMock(spec=Database)
    return db


# Test HealthStatus enum
def test_health_status_enum():
    """Test HealthStatus enum values"""
    assert HealthStatus.HEALTHY == "healthy"
    assert HealthStatus.UNHEALTHY == "unhealthy"
    assert HealthStatus.DISABLED == "disabled"
    assert HealthStatus.UNKNOWN == "unknown"


# Test HealthCheckService
class TestHealthCheckService:
    """Tests for HealthCheckService"""

    @pytest.fixture
    def mock_db(self):
        """Create mock database"""
        db = MagicMock(spec=Database)
        return db

    @pytest.fixture
    def service(self, mock_db):
        """Create health check service"""
        return HealthCheckService(mock_db, timeout_secs=10)

    @pytest.fixture
    def mock_provider(self):
        """Create mock provider"""
        provider = MagicMock(spec=ProviderModel)
        provider.id = 1
        provider.provider_key = "test-provider"
        provider.provider_type = "openai"
        provider.api_base = "https://api.openai.com/v1"
        provider.api_key = "sk-test-key"
        provider.is_enabled = True
        provider.get_model_mapping.return_value = {"gpt-4": "gpt-4-turbo"}
        return provider

    @pytest.mark.asyncio
    async def test_check_provider_health_disabled(self, service, mock_provider):
        """Test checking health of disabled provider"""
        mock_provider.is_enabled = False

        result = await service.check_provider_health(mock_provider)

        assert result.provider_id == 1
        assert result.provider_key == "test-provider"
        assert result.status == HealthStatus.DISABLED
        assert result.models == []
        assert result.avg_response_time_ms is None
        assert result.checked_at.endswith("Z")

    @pytest.mark.asyncio
    async def test_check_provider_health_success(self, service, mock_provider):
        """Test successful health check"""
        # Mock successful HTTP response
        mock_response = MagicMock()
        mock_response.status_code = 200

        with patch("app.services.health_check_service.get_http_client") as mock_get_client:
            mock_client = AsyncMock()
            mock_client.post.return_value = mock_response
            mock_get_client.return_value = mock_client

            result = await service.check_provider_health(
                mock_provider, models=["gpt-4"]
            )

            assert result.provider_id == 1
            assert result.status == HealthStatus.HEALTHY
            assert len(result.models) == 1
            assert result.models[0].model == "gpt-4"
            assert result.models[0].status == HealthStatus.HEALTHY
            assert result.models[0].response_time_ms is not None
            assert result.models[0].error is None
            assert result.avg_response_time_ms is not None

    @pytest.mark.asyncio
    async def test_check_provider_health_timeout(self, service, mock_provider):
        """Test health check with timeout"""
        with patch("app.services.health_check_service.get_http_client") as mock_get_client:
            mock_client = AsyncMock()
            mock_client.post.side_effect = asyncio.TimeoutError()
            mock_get_client.return_value = mock_client

            result = await service.check_provider_health(
                mock_provider, models=["gpt-4"]
            )

            assert result.status == HealthStatus.UNHEALTHY
            assert len(result.models) == 1
            assert result.models[0].status == HealthStatus.UNHEALTHY
            assert "Timeout" in result.models[0].error

    @pytest.mark.asyncio
    async def test_check_provider_health_error(self, service, mock_provider):
        """Test health check with HTTP error"""
        mock_response = MagicMock()
        mock_response.status_code = 401
        mock_response.json.return_value = {"error": {"message": "Invalid API key"}}

        with patch("app.services.health_check_service.get_http_client") as mock_get_client:
            mock_client = AsyncMock()
            mock_client.post.return_value = mock_response
            mock_get_client.return_value = mock_client

            result = await service.check_provider_health(
                mock_provider, models=["gpt-4"]
            )

            assert result.status == HealthStatus.UNHEALTHY
            assert len(result.models) == 1
            assert result.models[0].status == HealthStatus.UNHEALTHY
            assert result.models[0].error == "Invalid API key"

    def test_determine_provider_status_all_healthy(self, service):
        """Test determining provider status when all models are healthy"""
        model_statuses = [
            ModelHealthStatus(
                model="gpt-4",
                status=HealthStatus.HEALTHY,
                response_time_ms=100,
                error=None,
            ),
            ModelHealthStatus(
                model="gpt-3.5-turbo",
                status=HealthStatus.HEALTHY,
                response_time_ms=200,
                error=None,
            ),
        ]

        result = service._determine_provider_status(model_statuses)
        assert result == HealthStatus.HEALTHY

    def test_determine_provider_status_partially_healthy(self, service):
        """Test determining provider status when some models are healthy"""
        model_statuses = [
            ModelHealthStatus(
                model="gpt-4",
                status=HealthStatus.HEALTHY,
                response_time_ms=100,
                error=None,
            ),
            ModelHealthStatus(
                model="gpt-3.5-turbo",
                status=HealthStatus.UNHEALTHY,
                response_time_ms=200,
                error="Error",
            ),
        ]

        result = service._determine_provider_status(model_statuses)
        assert result == HealthStatus.HEALTHY

    def test_determine_provider_status_all_unhealthy(self, service):
        """Test determining provider status when all models are unhealthy"""
        model_statuses = [
            ModelHealthStatus(
                model="gpt-4",
                status=HealthStatus.UNHEALTHY,
                response_time_ms=100,
                error="Error 1",
            ),
            ModelHealthStatus(
                model="gpt-3.5-turbo",
                status=HealthStatus.UNHEALTHY,
                response_time_ms=200,
                error="Error 2",
            ),
        ]

        result = service._determine_provider_status(model_statuses)
        assert result == HealthStatus.UNHEALTHY

    def test_determine_provider_status_empty(self, service):
        """Test determining provider status with no models"""
        result = service._determine_provider_status([])
        assert result == HealthStatus.UNKNOWN

    def test_calculate_avg_response_time(self, service):
        """Test calculating average response time"""
        model_statuses = [
            ModelHealthStatus(
                model="gpt-4",
                status=HealthStatus.HEALTHY,
                response_time_ms=100,
                error=None,
            ),
            ModelHealthStatus(
                model="gpt-3.5-turbo",
                status=HealthStatus.HEALTHY,
                response_time_ms=200,
                error=None,
            ),
            ModelHealthStatus(
                model="claude-3",
                status=HealthStatus.HEALTHY,
                response_time_ms=300,
                error=None,
            ),
        ]

        result = service._calculate_avg_response_time(model_statuses)
        assert result == 200

    def test_calculate_avg_response_time_with_none(self, service):
        """Test calculating average response time with some None values"""
        model_statuses = [
            ModelHealthStatus(
                model="gpt-4",
                status=HealthStatus.HEALTHY,
                response_time_ms=100,
                error=None,
            ),
            ModelHealthStatus(
                model="gpt-3.5-turbo",
                status=HealthStatus.UNHEALTHY,
                response_time_ms=None,
                error="Error",
            ),
        ]

        result = service._calculate_avg_response_time(model_statuses)
        assert result == 100

    def test_calculate_avg_response_time_empty(self, service):
        """Test calculating average response time with no valid times"""
        result = service._calculate_avg_response_time([])
        assert result is None

    def test_get_default_test_models_with_mapping(self, service, mock_provider):
        """Test getting default test models when provider has model mapping"""
        mock_provider.get_model_mapping.return_value = {
            "model1": "provider-model1",
            "model2": "provider-model2",
            "model3": "provider-model3",
            "model4": "provider-model4",
        }

        result = service._get_default_test_models(mock_provider)
        assert len(result) == 4
        assert set(result) == {"model1", "model2", "model3", "model4"}

    def test_get_default_test_models_openai(self, service, mock_provider):
        """Test getting default test models for OpenAI provider"""
        mock_provider.get_model_mapping.return_value = {}
        mock_provider.provider_type = "openai"

        result = service._get_default_test_models(mock_provider)
        assert result == ["gpt-3.5-turbo"]

    def test_get_default_test_models_anthropic(self, service, mock_provider):
        """Test getting default test models for Anthropic provider"""
        mock_provider.get_model_mapping.return_value = {}
        mock_provider.provider_type = "anthropic"

        result = service._get_default_test_models(mock_provider)
        assert result == ["claude-3-haiku-20240307"]

    def test_get_default_test_models_azure(self, service, mock_provider):
        """Test getting default test models for Azure provider"""
        mock_provider.get_model_mapping.return_value = {}
        mock_provider.provider_type = "azure"

        result = service._get_default_test_models(mock_provider)
        assert result == ["gpt-35-turbo"]


class TestConcurrentHealthCheck:
    """Tests for concurrent health check functionality"""

    @pytest.fixture
    def mock_db(self):
        """Create mock database"""
        db = MagicMock(spec=Database)
        return db

    @pytest.fixture
    def service(self, mock_db):
        """Create health check service"""
        return HealthCheckService(mock_db, timeout_secs=10)

    @pytest.fixture
    def mock_provider(self):
        """Create mock provider with multiple models"""
        provider = MagicMock(spec=ProviderModel)
        provider.id = 1
        provider.provider_key = "test-provider"
        provider.provider_type = "openai"
        provider.api_base = "https://api.openai.com/v1"
        provider.api_key = "sk-test-key"
        provider.is_enabled = True
        provider.get_model_mapping.return_value = {
            "gpt-4": "gpt-4-turbo",
            "gpt-3.5-turbo": "gpt-3.5-turbo",
            "gpt-4o": "gpt-4o",
        }
        return provider

    @pytest.mark.asyncio
    async def test_check_provider_health_concurrent_disabled(
        self, service, mock_provider
    ):
        """Test concurrent health check for disabled provider"""
        mock_provider.is_enabled = False

        result = await service.check_provider_health_concurrent(mock_provider)

        assert result.provider_id == 1
        assert result.provider_key == "test-provider"
        assert result.status == HealthStatus.DISABLED
        assert result.models == []
        assert result.summary.total_models == 0
        assert result.summary.healthy_models == 0
        assert result.summary.unhealthy_models == 0
        assert result.avg_response_time_ms is None
        assert result.checked_at.endswith("Z")

    @pytest.mark.asyncio
    async def test_check_provider_health_concurrent_success(
        self, service, mock_provider
    ):
        """Test successful concurrent health check"""
        mock_response = MagicMock()
        mock_response.status_code = 200

        with patch("app.services.health_check_service.get_http_client") as mock_get_client:
            mock_client = AsyncMock()
            mock_client.post.return_value = mock_response
            mock_get_client.return_value = mock_client

            result = await service.check_provider_health_concurrent(
                mock_provider, models=["gpt-4", "gpt-3.5-turbo"], max_concurrent=2
            )

            assert result.provider_id == 1
            assert result.status == HealthStatus.HEALTHY
            assert len(result.models) == 2
            assert result.summary.total_models == 2
            assert result.summary.healthy_models == 2
            assert result.summary.unhealthy_models == 0
            assert result.avg_response_time_ms is not None

    @pytest.mark.asyncio
    async def test_check_provider_health_concurrent_partial_failure(
        self, service, mock_provider
    ):
        """Test concurrent health check with some models failing"""
        call_count = 0

        async def mock_post(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            mock_response = MagicMock()
            if call_count == 1:
                mock_response.status_code = 200
            else:
                mock_response.status_code = 401
                mock_response.json.return_value = {"error": {"message": "Invalid key"}}
            return mock_response

        with patch("app.services.health_check_service.get_http_client") as mock_get_client:
            mock_client = AsyncMock()
            mock_client.post = mock_post
            mock_get_client.return_value = mock_client

            result = await service.check_provider_health_concurrent(
                mock_provider, models=["gpt-4", "gpt-3.5-turbo"], max_concurrent=2
            )

            assert result.status == HealthStatus.HEALTHY  # At least one model healthy
            assert result.summary.total_models == 2
            assert result.summary.healthy_models == 1
            assert result.summary.unhealthy_models == 1

    @pytest.mark.asyncio
    async def test_check_provider_health_concurrent_all_models(
        self, service, mock_provider
    ):
        """Test concurrent health check with all mapped models"""
        mock_response = MagicMock()
        mock_response.status_code = 200

        with patch("app.services.health_check_service.get_http_client") as mock_get_client:
            mock_client = AsyncMock()
            mock_client.post.return_value = mock_response
            mock_get_client.return_value = mock_client

            # Don't specify models - should test all mapped models
            result = await service.check_provider_health_concurrent(
                mock_provider, max_concurrent=2
            )

            assert result.summary.total_models == 3  # All 3 mapped models
            assert result.summary.healthy_models == 3

    @pytest.mark.asyncio
    async def test_check_provider_health_concurrent_no_models(
        self, service, mock_provider
    ):
        """Test concurrent health check with no mapped models"""
        mock_provider.get_model_mapping.return_value = {}

        result = await service.check_provider_health_concurrent(mock_provider)

        assert result.status == HealthStatus.UNKNOWN
        assert result.models == []
        assert result.summary.total_models == 0

    @pytest.mark.asyncio
    async def test_check_provider_health_concurrent_concurrency_limit(
        self, service, mock_provider
    ):
        """Test that concurrency limit is respected"""
        concurrent_calls = []
        max_concurrent_observed = 0
        lock = asyncio.Lock()

        async def mock_post(*args, **kwargs):
            nonlocal max_concurrent_observed
            async with lock:
                concurrent_calls.append(1)
                current = len(concurrent_calls)
                if current > max_concurrent_observed:
                    max_concurrent_observed = current

            # Simulate some work
            await asyncio.sleep(0.05)

            async with lock:
                concurrent_calls.pop()

            mock_response = MagicMock()
            mock_response.status_code = 200
            return mock_response

        with patch("app.services.health_check_service.get_http_client") as mock_get_client:
            mock_client = AsyncMock()
            mock_client.post = mock_post
            mock_get_client.return_value = mock_client

            # Test with max_concurrent=2 and 3 models
            await service.check_provider_health_concurrent(
                mock_provider, max_concurrent=2
            )

            # Should never exceed max_concurrent
            assert max_concurrent_observed <= 2

    def test_get_all_mapped_models(self, service, mock_provider):
        """Test getting all mapped models from provider"""
        result = service._get_all_mapped_models(mock_provider)
        assert set(result) == {"gpt-4", "gpt-3.5-turbo", "gpt-4o"}

    def test_get_all_mapped_models_empty(self, service, mock_provider):
        """Test getting all mapped models when mapping is empty"""
        mock_provider.get_model_mapping.return_value = {}
        result = service._get_all_mapped_models(mock_provider)
        assert result == []


class TestCheckProviderHealthModels:
    """Tests for CheckProviderHealthRequest and CheckProviderHealthResponse models"""

    def test_check_provider_health_request_defaults(self):
        """Test CheckProviderHealthRequest default values"""
        request = CheckProviderHealthRequest()
        assert request.models is None
        assert request.max_concurrent == 2
        assert request.timeout_secs == 30

    def test_check_provider_health_request_custom_values(self):
        """Test CheckProviderHealthRequest with custom values"""
        request = CheckProviderHealthRequest(
            models=["gpt-4", "gpt-3.5-turbo"],
            max_concurrent=5,
            timeout_secs=60,
        )
        assert request.models == ["gpt-4", "gpt-3.5-turbo"]
        assert request.max_concurrent == 5
        assert request.timeout_secs == 60

    def test_check_provider_health_request_validation(self):
        """Test CheckProviderHealthRequest validation"""
        # max_concurrent must be between 1 and 10
        with pytest.raises(ValueError):
            CheckProviderHealthRequest(max_concurrent=0)
        with pytest.raises(ValueError):
            CheckProviderHealthRequest(max_concurrent=11)

        # timeout_secs must be between 1 and 120
        with pytest.raises(ValueError):
            CheckProviderHealthRequest(timeout_secs=0)
        with pytest.raises(ValueError):
            CheckProviderHealthRequest(timeout_secs=121)

    def test_provider_health_summary(self):
        """Test ProviderHealthSummary model"""
        summary = ProviderHealthSummary(
            total_models=5,
            healthy_models=3,
            unhealthy_models=2,
        )
        assert summary.total_models == 5
        assert summary.healthy_models == 3
        assert summary.unhealthy_models == 2

    def test_check_provider_health_response(self):
        """Test CheckProviderHealthResponse model"""
        response = CheckProviderHealthResponse(
            provider_id=1,
            provider_key="test-provider",
            status=HealthStatus.HEALTHY,
            models=[
                ModelHealthStatus(
                    model="gpt-4",
                    status=HealthStatus.HEALTHY,
                    response_time_ms=100,
                    error=None,
                )
            ],
            summary=ProviderHealthSummary(
                total_models=1,
                healthy_models=1,
                unhealthy_models=0,
            ),
            avg_response_time_ms=100,
            checked_at="2024-01-15T10:30:00Z",
        )
        assert response.provider_id == 1
        assert response.provider_key == "test-provider"
        assert response.status == HealthStatus.HEALTHY
        assert len(response.models) == 1
        assert response.summary.total_models == 1
        assert response.avg_response_time_ms == 100
