"""Models API endpoints"""

from typing import Optional

from fastapi import APIRouter, Depends

from app.api.dependencies import verify_auth, get_provider_svc, model_matches_allowed_list
from app.models.config import CredentialConfig
from app.models.provider import _compile_pattern, _is_pattern
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


def _model_allowed_for_info(model_name: str, allowed_models: list[str]) -> bool:
    if not allowed_models:
        return True
    if model_matches_allowed_list(model_name, allowed_models):
        return True
    if not _is_pattern(model_name):
        return False
    try:
        compiled = _compile_pattern(model_name)
    except Exception:
        return False
    for allowed in allowed_models:
        if _is_pattern(allowed):
            continue
        if compiled.match(allowed):
            return True
    return False


@router.get("/model/info")
async def model_info(
    credential_config: Optional[CredentialConfig] = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """List model deployments in LiteLLM-compatible format."""
    providers = provider_svc.get_all_providers()
    allowed_models = credential_config.allowed_models if credential_config else []

    data = []
    for provider in providers:
        for model_name in sorted(provider.model_mapping.keys()):
            if not _model_allowed_for_info(model_name, allowed_models):
                continue
            mapped_model = provider.model_mapping[model_name]
            data.append(
                {
                    "model_name": model_name,
                    "litellm_params": {
                        "model": mapped_model,
                        "api_base": provider.api_base,
                        "custom_llm_provider": provider.provider_type,
                    },
                    "model_info": {
                        "provider_name": provider.name,
                        "provider_type": provider.provider_type,
                        "weight": provider.weight,
                        "is_pattern": _is_pattern(model_name),
                    },
                }
            )

    return {"data": data}
