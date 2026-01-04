"""API dependencies"""

from typing import Optional

from fastapi import Header

from app.core.security import verify_master_key
from app.core.logging import set_api_key_context
from app.models.config import MasterKeyConfig
from app.services.provider_service import get_provider_service, ProviderService


async def verify_auth(
    authorization: Optional[str] = Header(None),
) -> Optional[MasterKeyConfig]:
    """
    Verify master API key and check rate limits

    Returns:
        Optional[MasterKeyConfig]: The MasterKeyConfig if authenticated, None otherwise

    Raises:
        HTTPException: 401 if authentication failed, 429 if rate limit exceeded
    """
    is_valid, key_config = verify_master_key(authorization)
    key_name = key_config.name if key_config else None
    set_api_key_context(key_name or "anonymous")
    return key_config


def get_provider_svc() -> ProviderService:
    """Get provider service dependency"""
    return get_provider_service()
