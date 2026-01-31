"""Models API endpoints"""

from typing import Optional

from fastapi import APIRouter, Depends

from app.api.dependencies import verify_auth, get_provider_svc, model_matches_allowed_list
from app.models.config import CredentialConfig
from app.services.provider_service import ProviderService

router = APIRouter()


@router.get("/models")
async def list_models(
    credential_config: Optional[CredentialConfig] = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """List all available models (OpenAI compatible)"""
    models_set = provider_svc.get_all_models()

    if credential_config and credential_config.allowed_models:
        # Filter models using wildcard/regex matching
        filtered_models = set()
        for model in models_set:
            if model_matches_allowed_list(model, credential_config.allowed_models):
                filtered_models.add(model)
        models_set = filtered_models

    models_list = [
        {
            "id": model,
            "object": "model",
            "created": 1677610602,
            "owned_by": "system",
            "permission": [],
            "root": model,
            "parent": None,
        }
        for model in sorted(models_set)
    ]

    return {"object": "list", "data": models_list}
