"""Security utilities"""

import hmac
from typing import Optional, Tuple, Set
from fastapi import HTTPException, status

from app.core.config import get_config
from app.core.rate_limiter import RateLimiter
from app.core.database import hash_key
from app.core.logging import set_api_key_context
from app.models.config import CredentialConfig


# Paths that should be exempt from rate limiting
# These endpoints perform local computation and don't consume upstream LLM resources
RATE_LIMIT_EXEMPT_PATHS: Set[str] = {
    "/v1/messages/count_tokens",
    "/v2/messages/count_tokens",
}


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
    x_api_key: Optional[str] = None,
    request_path: Optional[str] = None,
) -> Tuple[bool, Optional[CredentialConfig]]:
    """
    Verify the credential API key and check rate limits.

    Supports both Claude API style (x-api-key header) and OpenAI style
    (Authorization: Bearer header). x-api-key takes precedence.

    Args:
        authorization: The Authorization header value (e.g., "Bearer sk-xxx")
        x_api_key: The x-api-key header value (Claude API style)
        request_path: The request path (used to skip rate limiting for certain endpoints)

    Returns:
        Tuple[bool, Optional[CredentialConfig]]: (is_valid, credential_config)
        - is_valid: True if authentication passed and rate limit not exceeded
        - credential_config: The CredentialConfig if found, None otherwise

    Raises:
        HTTPException: 429 if rate limit exceeded, 401 if authentication failed
    """
    config = get_config()

    # x-api-key takes precedence over Authorization: Bearer
    provided_key = None
    if x_api_key:
        provided_key = x_api_key
    elif authorization and authorization.startswith("Bearer "):
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
        # Skip rate limiting for exempt paths (e.g., token counting endpoints)
        if request_path and request_path in RATE_LIMIT_EXEMPT_PATHS:
            return True, matching_credential

        rate_limiter = get_rate_limiter()
        if rate_limiter and not rate_limiter.check_rate_limit(
            matching_credential.credential_key
        ):
            # Set api_key context before raising exception so middleware can log it
            set_api_key_context(matching_credential.name)
            raise HTTPException(
                status_code=status.HTTP_429_TOO_MANY_REQUESTS,
                detail="Rate limit exceeded for this credential key",
            )

    return True, matching_credential


# Alias for backward compatibility
verify_master_key = verify_credential_key
