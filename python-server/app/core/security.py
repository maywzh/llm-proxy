"""Security utilities"""

import hmac
from typing import Optional, Tuple
from fastapi import HTTPException, status

from app.core.config import get_config
from app.core.rate_limiter import RateLimiter
from app.core.database import hash_key
from app.models.config import CredentialConfig


_rate_limiter: Optional[RateLimiter] = None


def init_rate_limiter() -> None:
    """Initialize the global rate limiter with credentials from config"""
    global _rate_limiter
    config = get_config()

    if config.credentials:
        _rate_limiter = RateLimiter()
        for credential_config in config.credentials:
            if not credential_config.enabled:
                continue
            if credential_config.rate_limit is not None:
                _rate_limiter.register_key(
                    credential_config.credential_key,
                    credential_config.rate_limit.requests_per_second,
                    credential_config.rate_limit.burst_size,
                )


def get_rate_limiter() -> Optional[RateLimiter]:
    """Get the global rate limiter instance"""
    return _rate_limiter


def verify_credential_key(
    authorization: Optional[str] = None,
) -> Tuple[bool, Optional[CredentialConfig]]:
    """
    Verify the credential API key and check rate limits

    Returns:
        Tuple[bool, Optional[CredentialConfig]]: (is_valid, credential_config)
        - is_valid: True if authentication passed and rate limit not exceeded
        - credential_config: The CredentialConfig if found, None otherwise

    Raises:
        HTTPException: 429 if rate limit exceeded, 401 if authentication failed
    """
    config = get_config()

    provided_key = None
    if authorization and authorization.startswith("Bearer "):
        provided_key = authorization[7:]

    if not config.credentials:
        return True, None

    if not provided_key:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Missing or invalid authorization header",
        )

    key_to_compare = hash_key(provided_key)

    matching_credential = None
    for credential_config in config.credentials:
        if hmac.compare_digest(credential_config.credential_key, key_to_compare):
            if not credential_config.enabled:
                raise HTTPException(
                    status_code=status.HTTP_401_UNAUTHORIZED,
                    detail="Credential key is disabled",
                )
            matching_credential = credential_config
            break

    if not matching_credential:
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED, detail="Invalid credential key"
        )

    if matching_credential.rate_limit is not None:
        rate_limiter = get_rate_limiter()
        if rate_limiter and not rate_limiter.check_rate_limit(
            matching_credential.credential_key
        ):
            raise HTTPException(
                status_code=status.HTTP_429_TOO_MANY_REQUESTS,
                detail="Rate limit exceeded for this credential key",
            )

    return True, matching_credential


# Alias for backward compatibility
verify_master_key = verify_credential_key
