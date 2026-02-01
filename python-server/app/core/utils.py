"""Shared utility functions for V1 and V2 APIs."""

from app.core.config import get_env_config


def strip_provider_suffix(model: str) -> str:
    """Strip provider suffix from model name.

    When PROVIDER_SUFFIX is set (e.g., "openrouter"), model names like
    "openrouter/gpt-4" are treated as "gpt-4".

    Args:
        model: The model name, possibly with provider suffix prefix.

    Returns:
        The model name with provider suffix stripped if present.

    Examples:
        >>> # With PROVIDER_SUFFIX="openrouter"
        >>> strip_provider_suffix("openrouter/gpt-4")
        "gpt-4"
        >>> strip_provider_suffix("gpt-4")
        "gpt-4"
    """
    env_config = get_env_config()
    provider_suffix = env_config.provider_suffix

    if provider_suffix and "/" in model:
        prefix, base_model = model.split("/", 1)
        if prefix == provider_suffix:
            return base_model

    return model
