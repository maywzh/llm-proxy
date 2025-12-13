"""Models API endpoints"""
from fastapi import APIRouter, Depends

from app.api.dependencies import verify_auth, get_provider_svc
from app.services.provider_service import ProviderService

router = APIRouter()


@router.get('/models')
async def list_models(
    _: None = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc)
):
    """List all available models (OpenAI compatible)"""
    models_set = provider_svc.get_all_models()
    
    models_list = [
        {
            'id': model,
            'object': 'model',
            'created': 1677610602,
            'owned_by': 'system',
            'permission': [],
            'root': model,
            'parent': None
        }
        for model in sorted(models_set)
    ]
    
    return {'object': 'list', 'data': models_list}