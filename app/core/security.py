"""Security utilities"""
from typing import Optional

from app.core.config import get_config


def verify_master_key(authorization: Optional[str] = None) -> bool:
    """Verify the master API key if configured"""
    config = get_config()
    master_api_key = config.server.master_api_key
    
    if master_api_key is None:
        return True
    
    if authorization and authorization.startswith('Bearer '):
        provided_key = authorization[7:]
        return provided_key == master_api_key
    
    return False