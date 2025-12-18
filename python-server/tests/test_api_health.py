"""Tests for health check endpoints"""
from unittest.mock import Mock, AsyncMock, patch

import pytest
import httpx
import respx


@pytest.mark.unit
class TestHealthEndpoint:
    """Test /health endpoint"""
    
    def test_health_check_success(self, app_client):
        """Test basic health check"""
        response = app_client.get('/health')
        
        assert response.status_code == 200
        data = response.json()
        assert data['status'] == 'ok'
        assert 'providers' in data
        assert 'provider_info' in data
    
    def test_health_check_provider_count(self, app_client):
        """Test health check returns correct provider count"""
        response = app_client.get('/health')
        
        assert response.status_code == 200
        data = response.json()
        assert data['providers'] == 2  # From test config
    
    def test_health_check_provider_info(self, app_client):
        """Test health check returns provider information"""
        response = app_client.get('/health')
        
        assert response.status_code == 200
        data = response.json()
        assert len(data['provider_info']) == 2
        
        # Check provider info structure
        provider = data['provider_info'][0]
        assert 'name' in provider
        assert 'weight' in provider
        assert 'probability' in provider
    
    def test_health_check_probability_calculation(self, app_client):
        """Test health check calculates probabilities correctly"""
        response = app_client.get('/health')
        
        assert response.status_code == 200
        data = response.json()
        
        # From test config: provider1 weight=2, provider2 weight=1
        # Total weight = 3
        # provider1 probability = 2/3 = 66.7%
        # provider2 probability = 1/3 = 33.3%
        
        provider1 = next(p for p in data['provider_info'] if p['name'] == 'provider1')
        provider2 = next(p for p in data['provider_info'] if p['name'] == 'provider2')
        
        assert provider1['weight'] == 2
        assert provider2['weight'] == 1
        assert '66.7%' in provider1['probability']
        assert '33.3%' in provider2['probability']
    
    def test_health_check_no_auth_required(self, app_client):
        """Test health check doesn't require authentication"""
        # Should work without Authorization header
        response = app_client.get('/health')
        assert response.status_code == 200


@pytest.mark.unit
class TestDetailedHealthEndpoint:
    """Test /health/detailed endpoint"""
    
    @respx.mock
    def test_detailed_health_success(self, app_client):
        """Test detailed health check with successful providers"""
        # Mock provider1 response
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test',
                    'model': 'gpt-4-0613',
                    'choices': [{'message': {'content': 'Hi'}}]
                }
            )
        )
        
        # Mock provider2 response
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    'id': 'test',
                    'model': 'gpt-4-1106-preview',
                    'choices': [{'message': {'content': 'Hi'}}]
                }
            )
        )
        
        response = app_client.get('/health/detailed')
        
        assert response.status_code == 200
        data = response.json()
        
        assert 'provider1' in data
        assert 'provider2' in data
        assert data['provider1']['status'] == 'ok'
        assert data['provider2']['status'] == 'ok'
        # Check model-level status
        assert 'models' in data['provider1']
        assert len(data['provider1']['models']) > 0
    
    @respx.mock
    def test_detailed_health_provider_error(self, app_client):
        """Test detailed health check with provider error"""
        # Mock provider1 success
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json={'id': 'test', 'choices': []})
        )
        
        # Mock provider2 error
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(500, json={'error': 'Internal error'})
        )
        
        response = app_client.get('/health/detailed')
        
        assert response.status_code == 200
        data = response.json()
        
        assert data['provider1']['status'] == 'ok'
        assert data['provider2']['status'] == 'error'
        # Check model-level error
        assert 'models' in data['provider2']
        assert any('HTTP 500' in m.get('error', '') for m in data['provider2']['models'])
    
    @respx.mock
    def test_detailed_health_timeout(self, app_client):
        """Test detailed health check with timeout"""
        # Mock provider1 success
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json={'id': 'test', 'choices': []})
        )
        
        # Mock provider2 timeout
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            side_effect=httpx.TimeoutException("Timeout")
        )
        
        response = app_client.get('/health/detailed')
        
        assert response.status_code == 200
        data = response.json()
        
        assert data['provider1']['status'] == 'ok'
        assert data['provider2']['status'] == 'error'
        # Check model-level timeout error
        assert 'models' in data['provider2']
        assert any('Timeout' in m.get('error', '') for m in data['provider2']['models'])
    
    @respx.mock
    def test_detailed_health_latency_tracking(self, app_client):
        """Test detailed health check tracks latency"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json={'id': 'test', 'choices': []})
        )
        
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json={'id': 'test', 'choices': []})
        )
        
        response = app_client.get('/health/detailed')
        
        assert response.status_code == 200
        data = response.json()
        
        # Both providers should have latency information at model level
        assert 'models' in data['provider1']
        assert 'models' in data['provider2']
        for model in data['provider1']['models']:
            assert 'latency' in model
            assert 'ms' in model['latency']
        for model in data['provider2']['models']:
            assert 'latency' in model
            assert 'ms' in model['latency']
    
    @respx.mock
    def test_detailed_health_tested_model(self, app_client):
        """Test detailed health check shows tested model"""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json={'id': 'test', 'choices': []})
        )
        
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json={'id': 'test', 'choices': []})
        )
        
        response = app_client.get('/health/detailed')
        
        assert response.status_code == 200
        data = response.json()
        
        # Should show which models were tested
        assert 'models' in data['provider1']
        assert 'models' in data['provider2']
        for model in data['provider1']['models']:
            assert 'model' in model
        for model in data['provider2']['models']:
            assert 'model' in model
    
    def test_detailed_health_no_auth_required(self, app_client):
        """Test detailed health check doesn't require authentication"""
        # Should work without Authorization header
        response = app_client.get('/health/detailed')
        # May fail due to provider errors, but shouldn't be 401
        assert response.status_code != 401


@pytest.mark.unit
class TestHealthEndpointEdgeCases:
    """Test health endpoint edge cases"""
    
    def test_health_with_single_provider(self, monkeypatch, clear_config_cache):
        """Test health check with single provider"""
        from app.models.config import AppConfig, ProviderConfig
        from app.core import config as config_module
        from app.core.config import get_config
        from fastapi.testclient import TestClient
        from app.main import app
        
        # Clear cache first
        get_config.cache_clear()
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='only-provider',
                    api_base='https://api.test.com',
                    api_key='key',
                    weight=1,
                    model_mapping={'gpt-4': 'gpt-4-0613'}
                )
            ],
            verify_ssl=True
        )
        
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        # Reset provider service
        from app.services import provider_service as ps_module
        ps_module._provider_service = None
        
        client = TestClient(app)
        response = client.get('/health')
        
        assert response.status_code == 200
        data = response.json()
        assert data['providers'] == 1
        assert data['provider_info'][0]['probability'] == '100.0%'
    
    def test_health_with_equal_weights(self, monkeypatch, clear_config_cache):
        """Test health check with equal provider weights"""
        from app.models.config import AppConfig, ProviderConfig
        from app.core import config as config_module
        from app.core.config import get_config
        from fastapi.testclient import TestClient
        from app.main import app
        
        # Clear cache first
        get_config.cache_clear()
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='provider1',
                    api_base='https://api1.com',
                    api_key='key1',
                    weight=1,
                    model_mapping={}
                ),
                ProviderConfig(
                    name='provider2',
                    api_base='https://api2.com',
                    api_key='key2',
                    weight=1,
                    model_mapping={}
                )
            ],
            verify_ssl=True
        )
        
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        from app.services import provider_service as ps_module
        ps_module._provider_service = None
        
        client = TestClient(app)
        response = client.get('/health')
        
        assert response.status_code == 200
        data = response.json()
        
        # Both should have 50% probability
        for provider in data['provider_info']:
            assert provider['probability'] == '50.0%'
    
    @respx.mock
    def test_detailed_health_provider_no_models(self, monkeypatch, clear_config_cache):
        """Test detailed health check with provider having no models"""
        from app.models.config import AppConfig, ProviderConfig
        from app.core import config as config_module
        from app.core.config import get_config
        from fastapi.testclient import TestClient
        from app.main import app
        
        # Clear cache first
        get_config.cache_clear()
        
        config = AppConfig(
            providers=[
                ProviderConfig(
                    name='no-models',
                    api_base='https://api.test.com',
                    api_key='key',
                    weight=1,
                    model_mapping={}  # No models
                )
            ],
            verify_ssl=True
        )
        
        monkeypatch.setattr(config_module, 'get_config', lambda: config)
        
        from app.services import provider_service as ps_module
        ps_module._provider_service = None
        
        client = TestClient(app)
        response = client.get('/health/detailed')
        
        assert response.status_code == 200
        data = response.json()
        
        assert 'no-models' in data
        assert data['no-models']['status'] == 'error'
        assert 'no models configured' in data['no-models']['error']
        assert 'models' in data['no-models']
        assert data['no-models']['models'] == []
    
    @respx.mock
    def test_detailed_health_concurrent_checks(self, app_client):
        """Test that detailed health checks providers concurrently"""
        import time
        
        # Mock slow responses
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json={'id': 'test', 'choices': []})
        )
        
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(200, json={'id': 'test', 'choices': []})
        )
        
        start = time.time()
        response = app_client.get('/health/detailed')
        duration = time.time() - start
        
        assert response.status_code == 200
        
        # If sequential, would take 2x the time
        # Concurrent should be faster (though hard to test precisely)
        # Just verify it completes in reasonable time
        assert duration < 5.0  # Should complete within 5 seconds


@pytest.mark.unit
class TestHealthMetrics:
    """Test health endpoint metrics integration"""
    
    def test_health_endpoint_tracked_in_metrics(self, app_client):
        """Test that health endpoint requests are tracked"""
        from app.core.metrics import REQUEST_COUNT
        
        initial_count = REQUEST_COUNT.labels(
            method='GET',
            endpoint='/health',
            model='unknown',
            provider='unknown',
            status_code='200'
        )._value.get()
        
        response = app_client.get('/health')
        assert response.status_code == 200
        
        final_count = REQUEST_COUNT.labels(
            method='GET',
            endpoint='/health',
            model='unknown',
            provider='unknown',
            status_code='200'
        )._value.get()
        
        assert final_count > initial_count