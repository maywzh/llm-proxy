"""Health check endpoints"""
import asyncio
import time

import httpx
from fastapi import APIRouter, Depends

from app.api.dependencies import get_provider_svc
from app.services.provider_service import ProviderService
from app.core.config import get_config

router = APIRouter()


@router.get('/health')
async def health(provider_svc: ProviderService = Depends(get_provider_svc)):
    """Basic health check endpoint"""
    providers = provider_svc.get_all_providers()
    weights = provider_svc.get_provider_weights()
    total_weight = sum(weights)
    
    return {
        'status': 'ok',
        'providers': len(providers),
        'provider_info': [
            {
                'name': p.name,
                'weight': weights[i],
                'probability': f"{(weights[i] / total_weight) * 100:.1f}%"
            }
            for i, p in enumerate(providers)
        ]
    }


@router.get('/health/detailed')
async def health_detailed(provider_svc: ProviderService = Depends(get_provider_svc)):
    """Detailed health check - tests each provider's models concurrently"""
    config = get_config()
    
    async def test_provider(provider):
        """Test a single provider and return its status"""
        start_time = time.time()
        
        if not provider.model_mapping:
            return {
                provider.name: {
                    'status': 'error',
                    'error': 'no models configured',
                    'latency': '0ms'
                }
            }
        
        model_name = list(provider.model_mapping.keys())[0]
        actual_model = provider.model_mapping[model_name]
        
        try:
            headers = {
                'Authorization': f"Bearer {provider.api_key}",
                'Content-Type': 'application/json'
            }
            
            data = {
                'model': actual_model,
                'messages': [{'role': 'user', 'content': 'Hi'}],
                'max_tokens': 10
            }
            
            url = f"{provider.api_base}/chat/completions"
            
            async with httpx.AsyncClient(verify=config.verify_ssl, timeout=30.0) as client:
                response = await client.post(url, json=data, headers=headers)
                latency_ms = int((time.time() - start_time) * 1000)
                
                if response.status_code == 200:
                    return {
                        provider.name: {
                            'status': 'ok',
                            'latency': f'{latency_ms}ms',
                            'tested_model': model_name
                        }
                    }
                else:
                    return {
                        provider.name: {
                            'status': 'error',
                            'error': f'HTTP {response.status_code}',
                            'latency': f'{latency_ms}ms'
                        }
                    }
        except Exception as e:
            latency_ms = int((time.time() - start_time) * 1000)
            return {
                provider.name: {
                    'status': 'error',
                    'error': str(e)[:100],
                    'latency': f'{latency_ms}ms'
                }
            }
    
    providers = provider_svc.get_all_providers()
    tasks = [test_provider(provider) for provider in providers]
    results = await asyncio.gather(*tasks)
    
    response = {}
    for result in results:
        response.update(result)
    
    return response