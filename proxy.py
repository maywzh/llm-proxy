#!/usr/bin/env python3
"""
Simple Weighted Load Balancer LLM API Proxy
Distributes requests across multiple providers based on weights.
"""

import yaml
import httpx
from fastapi import FastAPI, Request, HTTPException, Header
from fastapi.responses import StreamingResponse, JSONResponse
import os
import urllib3
import random
from typing import Optional
import asyncio

# Disable SSL warnings when verify_ssl is False
urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)

app = FastAPI(
    title="LLM API Proxy",
    description="Weighted load balancer for LLM API requests",
    version="1.0.0"
)

# Global state
providers = []
provider_weights = []
verify_ssl = True
master_api_key = None

os.environ["SSL_CERT_FILE"] = "./cacerts.pem"

def load_config(config_path='config.yaml'):
    """Load configuration from YAML file"""
    with open(config_path, 'r') as f:
        config = yaml.safe_load(f)
    return config


def get_next_provider():
    """Get the next provider based on weights"""
    return random.choices(providers, weights=provider_weights, k=1)[0]


def verify_master_key(authorization: Optional[str] = None):
    """Verify the master API key if configured"""
    if master_api_key is None:
        return True
    
    if authorization and authorization.startswith('Bearer '):
        provided_key = authorization[7:]
        return provided_key == master_api_key
    
    return False


@app.post('/v1/chat/completions')
async def chat_completions(request: Request, authorization: Optional[str] = Header(None)):
    """Proxy chat completions requests to providers"""
    # Verify master API key
    if not verify_master_key(authorization):
        raise HTTPException(status_code=401, detail='Unauthorized')
    
    provider = get_next_provider()
    
    # Get request data
    data = await request.json()
    
    # Apply model mapping if configured
    if 'model' in data and 'model_mapping' in provider:
        original_model = data['model']
        mapped_model = provider['model_mapping'].get(original_model, original_model)
        data['model'] = mapped_model
    
    # Prepare headers
    headers = {
        'Authorization': f"Bearer {provider['api_key']}",
        'Content-Type': 'application/json'
    }
    
    # Forward request to selected provider
    url = f"{provider['api_base']}/chat/completions"
    
    try:
        # Check if it's a streaming response
        if data.get('stream', False):
            client = httpx.AsyncClient(verify=verify_ssl, timeout=300.0)
            
            async def generate():
                try:
                    async with client.stream('POST', url, json=data, headers=headers) as response:
                        async for chunk in response.aiter_bytes():
                            yield chunk
                finally:
                    await client.aclose()
            
            return StreamingResponse(
                generate(),
                media_type='text/event-stream'
            )
        else:
            async with httpx.AsyncClient(verify=verify_ssl, timeout=300.0) as client:
                response = await client.post(url, json=data, headers=headers)
                return JSONResponse(
                    content=response.json(),
                    status_code=response.status_code
                )
    
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@app.post('/v1/completions')
async def completions(request: Request, authorization: Optional[str] = Header(None)):
    """Proxy completions requests to providers"""
    # Verify master API key
    if not verify_master_key(authorization):
        raise HTTPException(status_code=401, detail='Unauthorized')
    
    provider = get_next_provider()
    
    data = await request.json()
    
    # Apply model mapping if configured
    if 'model' in data and 'model_mapping' in provider:
        original_model = data['model']
        mapped_model = provider['model_mapping'].get(original_model, original_model)
        data['model'] = mapped_model
    
    headers = {
        'Authorization': f"Bearer {provider['api_key']}",
        'Content-Type': 'application/json'
    }
    
    url = f"{provider['api_base']}/completions"
    
    try:
        if data.get('stream', False):
            client = httpx.AsyncClient(verify=verify_ssl, timeout=300.0)
            
            async def generate():
                try:
                    async with client.stream('POST', url, json=data, headers=headers) as response:
                        async for chunk in response.aiter_bytes():
                            yield chunk
                finally:
                    await client.aclose()
            
            return StreamingResponse(
                generate(),
                media_type='text/event-stream'
            )
        else:
            async with httpx.AsyncClient(verify=verify_ssl, timeout=300.0) as client:
                response = await client.post(url, json=data, headers=headers)
                return JSONResponse(
                    content=response.json(),
                    status_code=response.status_code
                )
    
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@app.get('/health')
async def health():
    """Basic health check endpoint"""
    return {
        'status': 'ok',
        'providers': len(providers),
        'provider_info': [
            {
                'name': p['name'],
                'weight': provider_weights[i],
                'probability': f"{(provider_weights[i] / sum(provider_weights)) * 100:.1f}%"
            }
            for i, p in enumerate(providers)
        ]
    }


@app.get('/health/detailed')
async def health_detailed():
    """Detailed health check - tests each provider's models concurrently"""
    import time
    
    async def test_provider(provider):
        """Test a single provider and return its status"""
        provider_name = provider['name']
        start_time = time.time()
        
        # Get all models from model_mapping
        model_mapping = provider.get('model_mapping', {})
        
        if not model_mapping:
            return {
                provider_name: {
                    'status': 'error',
                    'error': 'no models configured',
                    'latency': '0ms'
                }
            }
        
        # Test the first model (or pick one at random)
        model_name = list(model_mapping.keys())[0]
        actual_model = model_mapping[model_name]
        
        try:
            headers = {
                'Authorization': f"Bearer {provider['api_key']}",
                'Content-Type': 'application/json'
            }
            
            data = {
                'model': actual_model,
                'messages': [{'role': 'user', 'content': 'Hi'}],
                'max_tokens': 10
            }
            
            url = f"{provider['api_base']}/chat/completions"
            
            async with httpx.AsyncClient(verify=verify_ssl, timeout=30.0) as client:
                response = await client.post(url, json=data, headers=headers)
                
                end_time = time.time()
                latency_ms = int((end_time - start_time) * 1000)
                
                if response.status_code == 200:
                    return {
                        provider_name: {
                            'status': 'ok',
                            'latency': f'{latency_ms}ms',
                            'tested_model': model_name
                        }
                    }
                else:
                    return {
                        provider_name: {
                            'status': 'error',
                            'error': f'HTTP {response.status_code}',
                            'latency': f'{latency_ms}ms'
                        }
                    }
        
        except Exception as e:
            end_time = time.time()
            latency_ms = int((end_time - start_time) * 1000)
            return {
                provider_name: {
                    'status': 'error',
                    'error': str(e)[:100],
                    'latency': f'{latency_ms}ms'
                }
            }
    
    # Test all providers concurrently
    tasks = [test_provider(provider) for provider in providers]
    results = await asyncio.gather(*tasks)
    
    # Merge all results into a single dict
    response = {}
    for result in results:
        response.update(result)
    
    return response


@app.on_event("startup")
async def startup_event():
    """Initialize configuration on startup"""
    global providers, provider_weights, verify_ssl, master_api_key
    
    # Load configuration
    config = load_config()
    providers = config['providers']
    
    # Extract weights (default to 1 if not specified)
    provider_weights = [p.get('weight', 1) for p in providers]
    
    # Get SSL verification setting
    verify_ssl = config.get('verify_ssl', True)
    
    # Get master API key
    server_config = config.get('server', {})
    master_api_key = server_config.get('master_api_key')
    
    total_weight = sum(provider_weights)
    print(f"Starting LLM API Proxy with {len(providers)} providers")
    for i, provider in enumerate(providers):
        weight = provider_weights[i]
        probability = (weight / total_weight) * 100
        print(f"  - {provider['name']}: weight={weight} ({probability:.1f}%)")
    print(f"Master API key: {'Enabled' if master_api_key else 'Disabled'}")


if __name__ == '__main__':
    import uvicorn
    
    # Load config to get server settings
    config = load_config()
    server_config = config.get('server', {})
    host = server_config.get('host', '0.0.0.0')
    port = server_config.get('port', 8000)
    
    print(f"Listening on {host}:{port}")
    uvicorn.run(app, host=host, port=port)
