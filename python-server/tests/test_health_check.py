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
    HealthCheckRequest,
    HealthCheckResponse,
    HealthStatus,
    ModelHealthStatus,
    ProviderHealthStatus,
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

        with patch("httpx.AsyncClient") as mock_client_class:
            mock_client = AsyncMock()
            mock_client.__aenter__.return_value = mock_client
            mock_client.__aexit__.return_value = None
            mock_client.post.return_value = mock_response
            mock_client_class.return_value = mock_client

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
        with patch("httpx.AsyncClient") as mock_client_class:
            mock_client = AsyncMock()
            mock_client.__aenter__.return_value = mock_client
            mock_client.__aexit__.return_value = None
            mock_client.post.side_effect = asyncio.TimeoutError()
            mock_client_class.return_value = mock_client

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

        with patch("httpx.AsyncClient") as mock_client_class:
            mock_client = AsyncMock()
            mock_client.__aenter__.return_value = mock_client
            mock_client.__aexit__.return_value = None
            mock_client.post.return_value = mock_response
            mock_client_class.return_value = mock_client

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


# Note: API endpoint tests require database integration and are better suited
# for integration tests. The unit tests above cover the core service logic.
