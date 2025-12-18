"""Completions API endpoints"""
from typing import Optional

import httpx
from fastapi import APIRouter, Request, Depends, HTTPException
from fastapi.responses import JSONResponse

from app.api.dependencies import verify_auth, get_provider_svc
from app.services.provider_service import ProviderService
from app.utils.streaming import create_streaming_response, rewrite_model_in_response
from app.core.config import get_config
from app.core.metrics import TOKEN_USAGE
from app.core.logging import get_logger

router = APIRouter()
logger = get_logger()


async def proxy_completion_request(
    request: Request,
    endpoint: str,
    provider_svc: ProviderService
):
    """Common logic for proxying completion requests"""
    data = await request.json()
    original_model = data.get('model')
    
    # Select provider based on the requested model
    try:
        provider = provider_svc.get_next_provider(model=original_model)
    except ValueError as e:
        logger.error(f"Provider selection failed: {str(e)}")
        raise HTTPException(status_code=400, detail=str(e))
    
    # Store model and provider in request state for metrics middleware
    request.state.model = original_model or 'unknown'
    request.state.provider = provider.name
    
    if 'model' in data and provider.model_mapping:
        data['model'] = provider.model_mapping.get(original_model, original_model)
    
    headers = {
        'Authorization': f"Bearer {provider.api_key}",
        'Content-Type': 'application/json'
    }
    
    url = f"{provider.api_base}/{endpoint}"
    
    try:
        if data.get('stream', False):
            logger.debug(f"Streaming request to {provider.name} for model {original_model}")
            return create_streaming_response(url, data, headers, original_model, provider.name)
        else:
            config = get_config()
            logger.debug(f"Non-streaming request to {provider.name} for model {original_model}")
            
            async with httpx.AsyncClient(verify=config.verify_ssl, timeout=300.0) as client:
                response = await client.post(url, json=data, headers=headers)
                response_data = response.json()
                
                # Extract and record token usage
                if 'usage' in response_data:
                    usage = response_data['usage']
                    model_name = original_model or 'unknown'
                    
                    if 'prompt_tokens' in usage:
                        TOKEN_USAGE.labels(
                            model=model_name,
                            provider=provider.name,
                            token_type='prompt'
                        ).inc(usage['prompt_tokens'])
                    
                    if 'completion_tokens' in usage:
                        TOKEN_USAGE.labels(
                            model=model_name,
                            provider=provider.name,
                            token_type='completion'
                        ).inc(usage['completion_tokens'])
                    
                    if 'total_tokens' in usage:
                        TOKEN_USAGE.labels(
                            model=model_name,
                            provider=provider.name,
                            token_type='total'
                        ).inc(usage['total_tokens'])
                        
                        logger.debug(
                            f"Token usage - model={model_name} provider={provider.name} "
                            f"prompt={usage.get('prompt_tokens', 0)} "
                            f"completion={usage.get('completion_tokens', 0)} "
                            f"total={usage.get('total_tokens', 0)}"
                        )
                
                response_data = rewrite_model_in_response(response_data, original_model)
                return JSONResponse(content=response_data, status_code=response.status_code)
                
    except httpx.TimeoutException as e:
        logger.error(f"Timeout error for provider {provider.name}: {str(e)}")
        raise HTTPException(status_code=504, detail="Gateway timeout")
    except httpx.HTTPStatusError as e:
        logger.error(f"HTTP error for provider {provider.name}: {e.response.status_code} - {str(e)}")
        raise HTTPException(status_code=e.response.status_code, detail=str(e))
    except Exception as e:
        logger.exception(f"Unexpected error for provider {provider.name}")
        raise HTTPException(status_code=500, detail="Internal server error")


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