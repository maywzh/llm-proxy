"""
Tokenizer Selection Module

This module provides a unified interface for token counting across different
LLM providers. It supports:
- tiktoken (OpenAI models, default fallback)
- HuggingFace tokenizer (Claude models)

Claude tokenizer is embedded in the package for offline usage.
"""

from enum import Enum
from importlib import resources
from typing import Optional

import tiktoken
from tokenizers import Tokenizer as HfTokenizer

from app.core.logging import get_logger

# Embedded Claude tokenizer (loaded once on first use)
_claude_tokenizer: Optional[HfTokenizer] = None


class TokenizerType(Enum):
    """Tokenizer type enum."""

    TIKTOKEN = "tiktoken"
    HUGGINGFACE = "huggingface"


def get_claude_tokenizer() -> Optional[HfTokenizer]:
    """
    Get the embedded Claude tokenizer.

    Returns:
        HfTokenizer if loaded successfully, None otherwise.
    """
    global _claude_tokenizer
    logger = get_logger()

    if _claude_tokenizer is None:
        try:
            logger.debug("Loading embedded Claude tokenizer from package")
            tokenizer_path = resources.files("app.core.tokenizers").joinpath(
                "anthropic_tokenizer.json"
            )
            with tokenizer_path.open("r") as f:
                _claude_tokenizer = HfTokenizer.from_str(f.read())
            logger.debug("Successfully loaded embedded Claude tokenizer")
        except Exception as e:
            logger.warning(
                f"Failed to load embedded Claude tokenizer: {e}. "
                "Falling back to tiktoken."
            )
            return None

    return _claude_tokenizer


def select_tokenizer(model: str) -> tuple[TokenizerType, Optional[str]]:
    """
    Select the appropriate tokenizer for a given model.

    Args:
        model: The model name (e.g., "claude-3-opus", "gpt-4")

    Returns:
        A tuple of (TokenizerType, tiktoken_model_name or None)

    Claude Model Handling:
        - All Claude models (containing "claude") use embedded Claude tokenizer
        - All other models use tiktoken as fallback
    """
    model_lower = model.lower()

    # All Claude models use Anthropic's official tokenizer (embedded)
    if "claude" in model_lower:
        return TokenizerType.HUGGINGFACE, None

    # Default to tiktoken for all other models (OpenAI, etc.)
    return TokenizerType.TIKTOKEN, _normalize_tiktoken_model(model)


def _normalize_tiktoken_model(model: str) -> str:
    """Normalize model name for tiktoken."""
    if "gpt-35" in model:
        return model.replace("-35", "-3.5")
    if model.startswith("gpt-") or model.startswith("o1") or model.startswith("o3"):
        return model
    # Default to gpt-3.5-turbo encoding for unknown models
    return "gpt-3.5-turbo"


def count_tokens_hf(text: str, tokenizer: HfTokenizer) -> int:
    """
    Count tokens using HuggingFace tokenizer.

    Args:
        text: The text to tokenize
        tokenizer: The HuggingFace tokenizer

    Returns:
        The number of tokens
    """
    logger = get_logger()
    try:
        encoding = tokenizer.encode(text, add_special_tokens=False)
        return len(encoding.ids)
    except Exception as e:
        logger.warning(f"HuggingFace tokenization failed: {e}. Returning 0.")
        return 0


def count_tokens_tiktoken(text: str, model: str) -> int:
    """
    Count tokens using tiktoken.

    Args:
        text: The text to tokenize
        model: The model name for encoding selection

    Returns:
        The number of tokens
    """
    try:
        if "gpt-4o" in model:
            encoding = tiktoken.get_encoding("o200k_base")
        else:
            encoding = tiktoken.encoding_for_model(model)
    except KeyError:
        encoding = tiktoken.get_encoding("cl100k_base")

    return len(encoding.encode(text, disallowed_special=()))


def count_tokens(text: str, model: str) -> int:
    """
    Count tokens for the given text using the appropriate tokenizer.

    This is the main entry point for token counting. It automatically
    selects the correct tokenizer based on the model name.

    Args:
        text: The text to tokenize
        model: The model name

    Returns:
        The number of tokens
    """
    tokenizer_type, tiktoken_model = select_tokenizer(model)

    if tokenizer_type == TokenizerType.HUGGINGFACE:
        claude_tokenizer = get_claude_tokenizer()
        if claude_tokenizer:
            return count_tokens_hf(text, claude_tokenizer)
        # Fallback to tiktoken if Claude tokenizer failed to load
        return count_tokens_tiktoken(text, "gpt-3.5-turbo")

    return count_tokens_tiktoken(text, tiktoken_model or "gpt-3.5-turbo")


def get_tokenizer_info(model: str) -> str:
    """Get tokenizer info for a model (useful for debugging/logging)."""
    tokenizer_type, tiktoken_model = select_tokenizer(model)

    if tokenizer_type == TokenizerType.HUGGINGFACE:
        return "HuggingFace (embedded Claude)"

    return f"tiktoken ({tiktoken_model or 'default'})"
