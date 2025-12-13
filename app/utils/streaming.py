"""Streaming response utilities"""
import json
from typing import AsyncIterator, Optional

import httpx
from fastapi.responses import StreamingResponse

from app.core.config import get_config


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
                rewritten_lines.append('data: ' + json.dumps(json_obj))
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
    original_model: Optional[str]
) -> AsyncIterator[bytes]:
    """Stream response from provider with model rewriting"""
    try:
        async with client.stream('POST', url, json=data, headers=headers) as response:
            async for chunk in response.aiter_bytes():
                yield await rewrite_sse_chunk(chunk, original_model)
    finally:
        await client.aclose()


def create_streaming_response(
    url: str,
    data: dict,
    headers: dict,
    original_model: Optional[str]
) -> StreamingResponse:
    """Create streaming response with proper cleanup"""
    config = get_config()
    client = httpx.AsyncClient(verify=config.verify_ssl, timeout=300.0)
    
    return StreamingResponse(
        stream_response(client, url, data, headers, original_model),
        media_type='text/event-stream'
    )


def rewrite_model_in_response(response_data: dict, original_model: Optional[str]) -> dict:
    """Rewrite model field in non-streaming response"""
    if original_model and 'model' in response_data:
        response_data['model'] = original_model
    return response_data