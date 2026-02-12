"""API dependencies"""

from typing import Optional, List

from fastapi import Header, HTTPException, status, Request

from app.core.security import verify_credential_key
from app.core.logging import set_api_key_context
from app.models.config import CredentialConfig
from app.models.provider import _is_pattern, _compile_pattern
from app.services.provider_service import get_provider_service, ProviderService


async def verify_auth(
    request: Request,
    authorization: Optional[str] = Header(None),
    x_api_key: Optional[str] = Header(None, alias="x-api-key"),
) -> Optional[CredentialConfig]:
    """
    Verify credential API key and check rate limits.

    Supports both Claude API style (x-api-key header) and OpenAI style
    (Authorization: Bearer header). x-api-key takes precedence.

    Returns:
        Optional[CredentialConfig]: The CredentialConfig if authenticated, None otherwise

    Raises:
        HTTPException: 401 if authentication failed, 429 if rate limit exceeded
    """
    _, credential_config = verify_credential_key(
        authorization=authorization,
        x_api_key=x_api_key,
        request_path=request.url.path,
    )
    key_name = credential_config.name if credential_config else None
    set_api_key_context(key_name or "anonymous")
    return credential_config


def get_provider_svc() -> ProviderService:
    """Get provider service dependency"""
    return get_provider_service()


def model_matches_allowed_list(model: str, allowed_models: List[str]) -> bool:
    """Check if a model matches any entry in allowed_models list.

    Supports:
    - Exact match: "gpt-4" matches "gpt-4"
    - Simple wildcard: "gpt-*" matches "gpt-4", "gpt-4o"
    - Regex pattern: "claude-opus-4-5-.*" matches "claude-opus-4-5-20240620"

    Args:
        model: The model name to check
        allowed_models: List of allowed model patterns

    Returns:
        True if model matches any pattern, False otherwise
    """
    for pattern in allowed_models:
        if _is_pattern(pattern):
            try:
                compiled = _compile_pattern(pattern)
                if compiled.match(model):
                    return True
            except Exception:
                continue
        elif model == pattern:
            return True
    return False


def check_model_permission(
    model: Optional[str],
    credential_config: Optional[CredentialConfig],
) -> None:
    """Check if the model is allowed for the given credential.

    Args:
        model: The model name from the request
        credential_config: The credential configuration (may be None if auth disabled)

    Raises:
        HTTPException 403: If model is not in allowed_models list
    """
    if not model:
        return

    if not credential_config:
        return

    if not credential_config.allowed_models:
        return

    if model_matches_allowed_list(model, credential_config.allowed_models):
        return

    raise HTTPException(
        status_code=status.HTTP_403_FORBIDDEN,
        detail=f"Model '{model}' is not allowed for this credential. "
        f"Allowed models: {credential_config.allowed_models}",
    )
