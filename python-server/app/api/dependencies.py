"""API dependencies"""

from typing import Optional

from fastapi import Header

from app.core.security import verify_credential_key
from app.core.logging import set_api_key_context
from app.models.config import CredentialConfig
from app.services.provider_service import get_provider_service, ProviderService


async def verify_auth(
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
    is_valid, credential_config = verify_credential_key(
        authorization=authorization,
        x_api_key=x_api_key,
    )
    key_name = credential_config.name if credential_config else None
    set_api_key_context(key_name or "anonymous")
    return credential_config


def get_provider_svc() -> ProviderService:
    """Get provider service dependency"""
    return get_provider_service()
