"""Models API endpoints"""

from typing import Any, Dict, Literal, Optional

from fastapi import APIRouter, Depends, Query

from app.api.dependencies import (
    verify_auth,
    get_provider_svc,
    model_matches_allowed_list,
)
from app.models.config import CredentialConfig, ModelMappingEntry, get_mapped_model_name
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


def _extract_model_info_metadata(entry: ModelMappingEntry) -> Dict[str, Any]:
    """Extract metadata fields from ModelMappingEntry for model_info response."""
    return entry.model_dump(exclude_none=True, exclude={"mapped_model"})


def _build_model_data(
    provider_svc: ProviderService, allowed_models: list[str]
) -> list[Dict[str, Any]]:
    """Build model data list from providers (shared logic for v1/v2)."""
    providers = provider_svc.get_all_providers()
    data = []
    for provider in providers:
        for model_name in sorted(provider.model_mapping.keys()):
            if not _model_allowed_for_info(model_name, allowed_models):
                continue

            entry = provider.model_mapping[model_name]
            mapped_model = get_mapped_model_name(entry)

            model_info_dict: Dict[str, Any] = {
                "provider_name": provider.name,
                "provider_type": provider.provider_type,
                "weight": provider.weight,
                "is_pattern": _is_pattern(model_name),
            }

            if isinstance(entry, ModelMappingEntry):
                model_info_dict.update(_extract_model_info_metadata(entry))

            data.append(
                {
                    "model_name": model_name,
                    "litellm_params": {
                        "model": mapped_model,
                        "api_base": provider.api_base,
                        "custom_llm_provider": provider.provider_type,
                    },
                    "model_info": model_info_dict,
                }
            )
    return data


@router.get("/model/info")
async def model_info_v1(
    model: Optional[str] = Query(default=None, description="Filter by exact model name"),
    litellm_model_id: Optional[str] = Query(
        default=None, description="Filter by LiteLLM model ID (mapped model name)"
    ),
    credential_config: Optional[CredentialConfig] = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """List model deployments with optional filtering (LiteLLM v1 compatible).

    V1 format: No pagination, returns all data in {"data": [...]} format.
    """
    allowed_models = credential_config.allowed_models if credential_config else []
    data = _build_model_data(provider_svc, allowed_models)

    # Filter by model (exact match)
    if model:
        data = [e for e in data if e["model_name"] == model]

    # Filter by litellm_model_id (exact match on mapped model)
    if litellm_model_id:
        data = [e for e in data if e["litellm_params"]["model"] == litellm_model_id]

    return {"data": data}


async def model_info_v2(
    model: Optional[str] = Query(default=None, description="Filter by exact model name"),
    litellm_model_id: Optional[str] = Query(
        default=None, alias="modelId", description="Filter by model ID (mapped model name)"
    ),
    search: Optional[str] = Query(default=None, description="Fuzzy search in model_name"),
    sort_by: Optional[str] = Query(
        default=None,
        alias="sortBy",
        description="Sort field: model_name, provider_name, weight",
    ),
    sort_order: Literal["asc", "desc"] = Query(default="asc", alias="sortOrder"),
    page: int = Query(default=1, ge=1),
    size: int = Query(default=50, ge=1, le=100),
    credential_config: Optional[CredentialConfig] = Depends(verify_auth),
    provider_svc: ProviderService = Depends(get_provider_svc),
):
    """List model deployments with filtering, sorting and pagination (LiteLLM v2 compatible).

    V2 format: Paginated response with total_count, current_page, size, total_pages.
    """
    allowed_models = credential_config.allowed_models if credential_config else []
    data = _build_model_data(provider_svc, allowed_models)

    # 1. Filter by model (exact match)
    if model:
        data = [e for e in data if e["model_name"] == model]

    # 2. Filter by litellm_model_id (exact match on mapped model)
    if litellm_model_id:
        data = [e for e in data if e["litellm_params"]["model"] == litellm_model_id]

    # 3. Search (fuzzy match in model_name)
    if search:
        lower_search = search.lower()
        data = [e for e in data if lower_search in e["model_name"].lower()]

    # 4. Sort (only if sort_by is specified)
    if sort_by:
        reverse = sort_order == "desc"
        if sort_by == "provider_name":
            data.sort(key=lambda e: e["model_info"]["provider_name"], reverse=reverse)
        elif sort_by == "weight":
            data.sort(key=lambda e: e["model_info"]["weight"], reverse=reverse)
        else:  # default: model_name
            data.sort(key=lambda e: e["model_name"], reverse=reverse)

    # 5. Paginate
    total = len(data)
    total_pages = (total + size - 1) // size
    start = (page - 1) * size
    end = start + size
    paginated_data = data[start:end]

    return {
        "data": paginated_data,
        "total_count": total,
        "current_page": page,
        "size": size,
        "total_pages": total_pages,
    }
