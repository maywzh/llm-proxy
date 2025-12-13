"""API dependencies"""
from typing import Optional

from fastapi import Header, HTTPException

from app.core.security import verify_master_key
from app.services.provider_service import get_provider_service, ProviderService


async def verify_auth(authorization: Optional[str] = Header(None)) -> None:
    """Verify master API key"""
    if not verify_master_key(authorization):
        raise HTTPException(status_code=401, detail='Unauthorized')


def get_provider_svc() -> ProviderService:
    """Get provider service dependency"""
    return get_provider_service()