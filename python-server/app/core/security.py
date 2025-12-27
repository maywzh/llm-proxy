"""Security utilities"""
from typing import Optional, Tuple
from fastapi import HTTPException, status

from app.core.config import get_config
from app.core.rate_limiter import RateLimiter


_rate_limiter: Optional[RateLimiter] = None


def init_rate_limiter() -> None:
    """Initialize the global rate limiter with master keys from config"""
    global _rate_limiter
    config = get_config()
    
    if config.master_keys:
        _rate_limiter = RateLimiter()
        for key_config in config.master_keys:
            if not key_config.enabled:
                continue
            # Only register keys that have rate limiting configured
            if key_config.rate_limit is not None:
                _rate_limiter.register_key(
                    key_config.key,
                    key_config.rate_limit.requests_per_second,
                    key_config.rate_limit.burst_size
                )


def get_rate_limiter() -> Optional[RateLimiter]:
    """Get the global rate limiter instance"""
    return _rate_limiter


def verify_master_key(authorization: Optional[str] = None) -> Tuple[bool, Optional[str]]:
    """
    Verify the master API key and check rate limits
    
    Returns:
        Tuple[bool, Optional[str]]: (is_valid, key_id)
        - is_valid: True if authentication passed and rate limit not exceeded
        - key_id: The master key ID if found, None otherwise
    
    Raises:
        HTTPException: 429 if rate limit exceeded, 401 if authentication failed
    """
    config = get_config()
    
    # Extract the provided key from Authorization header
    provided_key = None
    if authorization and authorization.startswith('Bearer '):
        provided_key = authorization[7:]
    
    # If no master keys configured, allow all requests
    if not config.master_keys:
        return True, None
    
    # Require authentication when master keys are configured
    if not provided_key:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Missing or invalid authorization header"
        )
    
    # Find matching master key - MUST check enabled field
    matching_key = None
    for key_config in config.master_keys:
        if key_config.key == provided_key:
            # Check if key is enabled before accepting it
            if not key_config.enabled:
                raise HTTPException(
                    status_code=status.HTTP_401_UNAUTHORIZED,
                    detail="Master key is disabled"
                )
            matching_key = key_config
            break
    
    if not matching_key:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Invalid master key"
        )
    
    # Check rate limit only if the key has rate limiting configured
    if matching_key.rate_limit is not None:
        rate_limiter = get_rate_limiter()
        if rate_limiter and not rate_limiter.check_rate_limit(provided_key):
            raise HTTPException(
                status_code=status.HTTP_429_TOO_MANY_REQUESTS,
                detail="Rate limit exceeded for this master key"
            )
    
    return True, provided_key
