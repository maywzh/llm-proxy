"""Shared test fixtures and configuration"""
import os
import tempfile
from typing import Generator
from unittest.mock import Mock

import pytest
import yaml
from fastapi.testclient import TestClient

from app.models.config import AppConfig, ProviderConfig, ServerConfig
from app.services.provider_service import ProviderService

# Set CONFIG_PATH for tests - use absolute path to ensure it's found
os.environ['CONFIG_PATH'] = os.path.join(os.path.dirname(os.path.dirname(__file__)), 'config.test.yaml')


@pytest.fixture
def test_config_dict() -> dict:
    """Sample configuration dictionary for testing"""
    return {
        'providers': [
            {
                'name': 'provider1',
                'api_base': 'https://api.provider1.com/v1',
                'api_key': 'test-key-1',
                'weight': 2,
                'model_mapping': {
                    'gpt-4': 'gpt-4-0613',
                    'gpt-3.5-turbo': 'gpt-3.5-turbo-0613'
                }
            },
            {
                'name': 'provider2',
                'api_base': 'https://api.provider2.com/v1',
                'api_key': 'test-key-2',
                'weight': 1,
                'model_mapping': {
                    'gpt-4': 'gpt-4-1106-preview',
                    'claude-3': 'claude-3-opus-20240229'
                }
            }
        ],
        'server': {
            'host': '0.0.0.0',
            'port': 18000,
            'master_api_key': 'test-master-key'
        },
        'verify_ssl': True
    }


@pytest.fixture
def test_config(test_config_dict: dict) -> AppConfig:
    """Sample AppConfig instance for testing"""
    return AppConfig(**test_config_dict)


@pytest.fixture
def test_config_file(test_config_dict: dict) -> Generator[str, None, None]:
    """Create a temporary config file for testing"""
    with tempfile.NamedTemporaryFile(mode='w', suffix='.yaml', delete=False) as f:
        yaml.dump(test_config_dict, f)
        config_path = f.name
    
    yield config_path
    
    # Cleanup
    if os.path.exists(config_path):
        os.unlink(config_path)


@pytest.fixture
def test_config_with_env_vars() -> dict:
    """Config dictionary with environment variable placeholders"""
    return {
        'providers': [
            {
                'name': 'provider1',
                'api_base': '${API_BASE}',
                'api_key': '${API_KEY:-default-key}',
                'weight': 1,
                'model_mapping': {'gpt-4': 'gpt-4-0613'}
            }
        ],
        'server': {
            'host': '${HOST:-0.0.0.0}',
            'port': 18000
        },
        'verify_ssl': True
    }


@pytest.fixture
def provider_service(test_config: AppConfig, monkeypatch, clear_config_cache) -> ProviderService:
    """Provider service instance with test configuration"""
    # Mock get_config in the provider_service module where it's imported
    from app.services import provider_service as ps_module
    monkeypatch.setattr(ps_module, 'get_config', lambda: test_config)
    
    # Reset provider service singleton
    ps_module._provider_service = None
    
    # Create fresh provider service
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


@pytest.fixture
def app_client(test_config: AppConfig, monkeypatch, clear_config_cache):
    """FastAPI test client with mocked configuration"""
    # Clear config cache first
    from app.core.config import get_config
    get_config.cache_clear()
    
    from app.core import config as config_module
    from app.main import app
    
    # Mock get_config
    monkeypatch.setattr(config_module, 'get_config', lambda: test_config)
    
    # Reset provider service singleton
    from app.services import provider_service as ps_module
    ps_module._provider_service = None
    
    return TestClient(app)


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


@pytest.fixture
def mock_env_vars(monkeypatch):
    """Set up mock environment variables"""
    monkeypatch.setenv('API_BASE', 'https://test.api.com')
    monkeypatch.setenv('API_KEY', 'test-env-key')
    monkeypatch.setenv('HOST', '127.0.0.1')


@pytest.fixture(autouse=True)
def clear_config_cache():
    """Clear the config cache before and after tests"""
    from app.core.config import get_config
    get_config.cache_clear()
    
    # Also reset provider service singleton
    from app.services import provider_service as ps_module
    ps_module._provider_service = None
    
    yield
    
    get_config.cache_clear()
    ps_module._provider_service = None