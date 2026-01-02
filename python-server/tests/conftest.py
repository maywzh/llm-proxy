"""Shared test fixtures and configuration"""
import os
from unittest.mock import Mock

import pytest
from fastapi.testclient import TestClient

from app.models.config import AppConfig, ProviderConfig, ServerConfig
from app.services.provider_service import ProviderService


@pytest.fixture
def test_config() -> AppConfig:
    """Sample AppConfig instance for testing"""
    return AppConfig(
        providers=[
            ProviderConfig(
                name='provider1',
                api_base='https://api.provider1.com/v1',
                api_key='test-key-1',
                weight=2,
                model_mapping={
                    'gpt-4': 'gpt-4-0613',
                    'gpt-3.5-turbo': 'gpt-3.5-turbo-0613'
                }
            ),
            ProviderConfig(
                name='provider2',
                api_base='https://api.provider2.com/v1',
                api_key='test-key-2',
                weight=1,
                model_mapping={
                    'gpt-4': 'gpt-4-1106-preview',
                    'claude-3': 'claude-3-opus-20240229'
                }
            )
        ],
        server=ServerConfig(host='0.0.0.0', port=18000),
        verify_ssl=True
    )


@pytest.fixture
def provider_service(test_config: AppConfig, monkeypatch, clear_config_cache) -> ProviderService:
    """Provider service instance with test configuration"""
    from app.services import provider_service as ps_module
    from app.core import config as config_module
    
    config_module._cached_config = test_config
    monkeypatch.setattr(ps_module, 'get_config', lambda: test_config)
    
    ps_module._provider_service = None
    
    service = ProviderService()
    service.initialize()
    return service


@pytest.fixture
def mock_httpx_client():
    """Mock httpx.AsyncClient for testing"""
    mock_client = Mock()
    mock_response = Mock()
    mock_response.status_code = 200
    mock_response.json.return_value = {
        'id': 'test-id',
        'model': 'gpt-4-0613',
        'choices': [{'message': {'content': 'Test response'}}],
        'usage': {'prompt_tokens': 10, 'completion_tokens': 20, 'total_tokens': 30}
    }
    mock_client.post.return_value = mock_response
    return mock_client


@pytest.fixture(autouse=True)
def reset_metrics():
    """Reset Prometheus metrics before each test"""
    from prometheus_client import REGISTRY
    
    # Clear all collectors
    collectors = list(REGISTRY._collector_to_names.keys())
    for collector in collectors:
        try:
            REGISTRY.unregister(collector)
        except Exception:
            pass
    
    yield
    
    # Clear again after test
    collectors = list(REGISTRY._collector_to_names.keys())
    for collector in collectors:
        try:
            REGISTRY.unregister(collector)
        except Exception:
            pass


@pytest.fixture
def sample_chat_request() -> dict:
    """Sample chat completion request"""
    return {
        'model': 'gpt-4',
        'messages': [
            {'role': 'system', 'content': 'You are a helpful assistant.'},
            {'role': 'user', 'content': 'Hello!'}
        ],
        'temperature': 0.7,
        'max_tokens': 100
    }


@pytest.fixture
def sample_completion_request() -> dict:
    """Sample completion request"""
    return {
        'model': 'gpt-3.5-turbo',
        'prompt': 'Once upon a time',
        'max_tokens': 50,
        'temperature': 0.8
    }


@pytest.fixture
def sample_streaming_request() -> dict:
    """Sample streaming request"""
    return {
        'model': 'gpt-4',
        'messages': [{'role': 'user', 'content': 'Tell me a story'}],
        'stream': True,
        'max_tokens': 100
    }


@pytest.fixture
def sample_sse_chunk() -> bytes:
    """Sample SSE chunk with model field"""
    return b'data: {"id":"test","model":"gpt-4-0613","choices":[{"delta":{"content":"Hello"}}]}\n\n'


@pytest.fixture
def sample_sse_chunk_with_usage() -> bytes:
    """Sample SSE chunk with usage information"""
    return b'data: {"id":"test","model":"gpt-4-0613","usage":{"prompt_tokens":10,"completion_tokens":20,"total_tokens":30}}\n\n'


@pytest.fixture(autouse=True)
def clear_config_cache():
    """Clear the config cache before and after tests"""
    from app.core.config import get_config, clear_config_cache as clear_cache
    from app.core import config as config_module
    
    config_module._cached_config = None
    get_config.cache_clear()
    
    from app.services import provider_service as ps_module
    ps_module._provider_service = None
    
    yield
    
    config_module._cached_config = None
    get_config.cache_clear()
    ps_module._provider_service = None