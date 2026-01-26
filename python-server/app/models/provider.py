"""Provider runtime model"""

import re
from dataclasses import dataclass, field
from functools import lru_cache
from typing import Dict, Optional, Tuple


def _is_pattern(key: str) -> bool:
    """Check if a model mapping key contains wildcard/regex patterns.

    Supports:
    - Regex patterns: .* .+ [abc] etc.
    - Simple wildcards: * (converted to .*)

    Note: A single dot (.) in model names like "gpt-3.5-turbo" is NOT considered a pattern.
    Only regex-specific patterns like .* .+ or metacharacters like [, (, |, etc. are detected.
    """
    # Check for regex-specific patterns (not just a single dot)
    # .* or .+ are regex patterns
    if ".*" in key or ".+" in key:
        return True

    # Simple wildcard: * without preceding dot
    if "*" in key and ".*" not in key:
        return True

    # Check for paired regex metacharacters that suggest actual pattern usage
    # Only consider it a pattern if we have matched pairs or pipe symbols
    if (
        ("(" in key and ")" in key)
        or ("[" in key and "]" in key)
        or ("{" in key and "}" in key)
        or ("|" in key)
    ):
        try:
            compiled = _compile_pattern(key)
            # A real pattern typically doesn't match itself but matches other strings
            # For example: [abc] doesn't match "[abc]" but matches "a", "b", "c"
            # But {literal} matches "{literal}" - this is literal, not a pattern
            matches_self = bool(compiled.match(key))
            # Test if it matches various test strings to see if it's actually a pattern
            # Include single chars and common prefixes that patterns might match
            test_strings = list("abcdefg01234") + ["model", "other", "test", "gpt"]
            matches_others = any(compiled.match(s) for s in test_strings if s != key)

            # It's a pattern if it either:
            # 1. Doesn't match itself (definitely a pattern)
            # 2. Matches itself AND matches other strings (could be a pattern like .*)
            if not matches_self or matches_others:
                return True
        except re.error:
            # Invalid regex, not a pattern
            pass

    return False


@lru_cache(maxsize=1024)
def _compile_pattern(pattern: str) -> re.Pattern:
    """Compile a pattern string to regex, caching the result.

    Converts simple wildcards (*) to regex (.*) if needed.
    """
    # If pattern doesn't look like regex but has *, convert to regex
    if "*" in pattern and ".*" not in pattern and ".+" not in pattern:
        # Simple wildcard: convert * to .*
        pattern = pattern.replace("*", ".*")

    # Anchor the pattern to match the full string
    if not pattern.startswith("^"):
        pattern = "^" + pattern
    if not pattern.endswith("$"):
        pattern = pattern + "$"

    return re.compile(pattern)


def match_model_pattern(
    model: str, model_mapping: Dict[str, str]
) -> Optional[Tuple[str, str]]:
    """Match a model name against model_mapping keys, supporting wildcards/regex.

    Args:
        model: The model name to match (e.g., "claude-opus-4-5-20240620")
        model_mapping: Dict of pattern -> mapped_model (e.g., {"claude-opus-4-5-.*": "claude-opus"})

    Returns:
        Tuple of (matched_pattern, mapped_model) if found, None otherwise.
        Exact matches take priority over pattern matches.
    """
    # First, try exact match (highest priority)
    if model in model_mapping:
        return (model, model_mapping[model])

    # Then, try pattern matching
    for pattern, mapped_model in model_mapping.items():
        if _is_pattern(pattern):
            try:
                compiled = _compile_pattern(pattern)
                if compiled.match(model):
                    return (pattern, mapped_model)
            except re.error:
                # Invalid regex, skip this pattern
                continue

    return None


def model_matches_mapping(model: str, model_mapping: Dict[str, str]) -> bool:
    """Check if a model matches any key in model_mapping (exact or pattern).

    Args:
        model: The model name to check
        model_mapping: Dict of pattern -> mapped_model

    Returns:
        True if model matches any key (exact or pattern), False otherwise
    """
    return match_model_pattern(model, model_mapping) is not None


def get_mapped_model(model: str, model_mapping: Dict[str, str]) -> str:
    """Get the mapped model name for a given model.

    Args:
        model: The model name to map
        model_mapping: Dict of pattern -> mapped_model

    Returns:
        The mapped model name if found, otherwise the original model name
    """
    result = match_model_pattern(model, model_mapping)
    if result:
        return result[1]
    return model


@dataclass
class Provider:
    """Runtime provider instance"""

    name: str
    api_base: str
    api_key: str
    weight: int
    model_mapping: Dict[str, str] = field(default_factory=dict)
    provider_type: str = field(default="openai")

    def supports_model(self, model: str) -> bool:
        """Check if this provider supports the given model (exact or pattern match)."""
        return model_matches_mapping(model, self.model_mapping)

    def get_mapped_model(self, model: str) -> str:
        """Get the mapped model name for the given model."""
        return get_mapped_model(model, self.model_mapping)
