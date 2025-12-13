"""Completions API endpoints"""
from typing import Optional

import httpx
from fastapi import APIRouter, Request, Depends, HTTPException
from fastapi.responses import JSONResponse

from app.api.dependencies import verify_auth, get_provider_svc
from app.services.provider_service import ProviderService
from app.utils.streaming import create_streaming_response, rewrite_model_in_response
from app.core.config import get_config

router = APIRouter()


async def proxy_completion_request(
    request: Request,
    endpoint: str,
    provider_svc: ProviderService
):
    """Common logic for proxying completion requests"""
    provider = provider_svc.get_next_provider()
    data = await request.json()
    original_model = data.get('model')
    
    if 'model' in data and provider.model_mapping:
        data['model'] = provider.model_mapping.get(original_model, original_model)
    
    headers = {
        'Authorization': f"Bearer {provider.api_key}",
        'Content-Type': 'application/json'
    }
    
    url = f"{provider.api_base}/{endpoint}"
    
    try:
        if data.get('stream', False):
            return create_streaming_response(url, data, headers, original_model)
        else:
            config = get_config()
            async with httpx.AsyncClient(verify=config.verify_ssl, timeout=300.0) as client:
                response = await client.post(url, json=data, headers=headers)
                response_data = response.json()
                response_data = rewrite_model_in_response(response_data, original_model)
                return JSONResponse(content=response_data, status_code=response.status_code)
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


@router.post('/chat/completions')
async def chat_completions(
    request: Request,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc)
):
    """Proxy chat completions requests to providers"""
    return await proxy_completion_request(request, 'chat/completions', provider_svc)


@router.post('/completions')
async def completions(
    request: Request,
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc)
):
    """Proxy completions requests to providers"""
    return await proxy_completion_request(request, 'completions', provider_svc)