"""Streaming response utilities"""
import json
from typing import AsyncIterator, Optional

import httpx
from fastapi.responses import StreamingResponse

from app.core.config import get_config
from app.core.metrics import TOKEN_USAGE
from app.core.logging import set_provider_context, clear_provider_context


async def rewrite_sse_chunk(chunk: bytes, original_model: Optional[str]) -> bytes:
    """Rewrite model field in SSE chunk"""
    if not original_model:
        return chunk
    
    chunk_str = chunk.decode('utf-8', errors='ignore')
    if '"model":' not in chunk_str:
        return chunk
    
    lines = chunk_str.split('\n')
    rewritten_lines = []
    
    for line in lines:
        if line.startswith('data: ') and line != 'data: [DONE]':
            try:
                json_str = line[6:]
                json_obj = json.loads(json_str)
                if 'model' in json_obj:
                    json_obj['model'] = original_model
                rewritten_lines.append('data: ' + json.dumps(json_obj, separators=(', ', ': ')))
            except:
                rewritten_lines.append(line)
        else:
            rewritten_lines.append(line)
    
    return '\n'.join(rewritten_lines).encode('utf-8')


async def stream_response(
    client: httpx.AsyncClient,
    url: str,
    data: dict,
    headers: dict,
    original_model: Optional[str],
    provider_name: str
) -> AsyncIterator[bytes]:
    """Stream response from provider with model rewriting and token tracking"""
    try:
        # Set provider context for logging
        set_provider_context(provider_name)
        
        async with client.stream('POST', url, json=data, headers=headers) as response:
            async for chunk in response.aiter_bytes():
                # Track token usage from streaming chunks
                chunk_str = chunk.decode('utf-8', errors='ignore')
                if '"usage":' in chunk_str:
                    try:
                        lines = chunk_str.split('\n')
                        for line in lines:
                            if line.startswith('data: ') and line != 'data: [DONE]':
                                json_str = line[6:]
                                json_obj = json.loads(json_str)
                                if 'usage' in json_obj:
                                    usage = json_obj['usage']
                                    model_name = original_model or 'unknown'
                                    
                                    if 'prompt_tokens' in usage:
                                        TOKEN_USAGE.labels(
                                            model=model_name,
                                            provider=provider_name,
                                            token_type='prompt'
                                        ).inc(usage['prompt_tokens'])
                                    
                                    if 'completion_tokens' in usage:
                                        TOKEN_USAGE.labels(
                                            model=model_name,
                                            provider=provider_name,
                                            token_type='completion'
                                        ).inc(usage['completion_tokens'])
                                    
                                    if 'total_tokens' in usage:
                                        TOKEN_USAGE.labels(
                                            model=model_name,
                                            provider=provider_name,
                                            token_type='total'
                                        ).inc(usage['total_tokens'])
                    except:
                        pass
                
                yield await rewrite_sse_chunk(chunk, original_model)
    finally:
        # Clear provider context after streaming completes
        clear_provider_context()
        await client.aclose()


def create_streaming_response(
    url: str,
    data: dict,
    headers: dict,
    original_model: Optional[str],
    provider_name: str
) -> StreamingResponse:
    """Create streaming response with proper cleanup"""
    config = get_config()
    client = httpx.AsyncClient(verify=config.verify_ssl, timeout=300.0)
    
    return StreamingResponse(
        stream_response(client, url, data, headers, original_model, provider_name),
        media_type='text/event-stream'
    )


def rewrite_model_in_response(response_data: dict, original_model: Optional[str]) -> dict:
    """Rewrite model field in non-streaming response"""
    if original_model and 'model' in response_data:
        response_data['model'] = original_model
    return response_data