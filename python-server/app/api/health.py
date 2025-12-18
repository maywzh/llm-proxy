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
    """Detailed health check - tests each provider's models concurrently, models serially within each provider"""
    config = get_config()
    
    async def test_model(provider, model_name: str, actual_model: str):
        """Test a single model for a provider"""
        start_time = time.time()
        
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
                        'model': model_name,
                        'status': 'ok',
                        'latency': f'{latency_ms}ms'
                    }
                else:
                    return {
                        'model': model_name,
                        'status': 'error',
                        'error': f'HTTP {response.status_code}',
                        'latency': f'{latency_ms}ms'
                    }
        except Exception as e:
            latency_ms = int((time.time() - start_time) * 1000)
            return {
                'model': model_name,
                'status': 'error',
                'error': str(e)[:100],
                'latency': f'{latency_ms}ms'
            }
    
    async def test_provider(provider):
        """Test all models for a single provider serially"""
        if not provider.model_mapping:
            return {
                provider.name: {
                    'status': 'error',
                    'error': 'no models configured',
                    'models': []
                }
            }
        
        # Test models serially within this provider
        model_results = []
        for model_name, actual_model in provider.model_mapping.items():
            result = await test_model(provider, model_name, actual_model)
            model_results.append(result)
        
        # Determine overall provider status based on model results
        all_ok = all(m['status'] == 'ok' for m in model_results)
        any_ok = any(m['status'] == 'ok' for m in model_results)
        
        if all_ok:
            provider_status = 'ok'
        elif any_ok:
            provider_status = 'partial'
        else:
            provider_status = 'error'
        
        return {
            provider.name: {
                'status': provider_status,
                'models': model_results
            }
        }
    
    # Test all providers concurrently
    providers = provider_svc.get_all_providers()
    tasks = [test_provider(provider) for provider in providers]
    results = await asyncio.gather(*tasks)
    
    response = {}
    for result in results:
        response.update(result)
    
    return response