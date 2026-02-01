"""
Outbound Token Counter Module

This module provides a unified token counting component for V1/V2 endpoints.
It handles:
- Pre-calculated input tokens
- Accumulated output content for fallback token counting
- Provider usage (preferred when available)
- Final usage calculation with provider priority
"""

from dataclasses import dataclass, field
from typing import Any, Optional

from app.core.tokenizer import count_tokens


@dataclass
class OutboundTokenCounter:
    """
    Unified token counter for outbound requests.

    This component tracks token usage during streaming responses and provides
    a unified interface for both V1 and V2 endpoints.

    Usage:
        1. Create with model and pre-calculated input_tokens
        2. Call accumulate_content() for each text chunk during streaming
        3. Call update_provider_usage() if provider sends usage info
        4. Call finalize() at stream end to get final usage dict

    The finalize() method prioritizes provider usage over local calculation.
    """

    model: str
    input_tokens: int = 0
    output_content: str = ""
    provider_usage: Optional[dict[str, Any]] = None
    _output_tokens_calculated: bool = field(default=False, init=False)
    _cached_output_tokens: int = field(default=0, init=False)

    def accumulate_content(self, content: str) -> None:
        """
        Accumulate output content for fallback token counting.

        Args:
            content: Text content from streaming chunk
        """
        if content:
            self.output_content += content

    def update_provider_usage(self, usage: dict[str, Any]) -> None:
        """
        Update with provider-reported usage.

        Args:
            usage: Usage dict from provider (e.g., {"prompt_tokens": 10, "completion_tokens": 20})
        """
        if usage:
            self.provider_usage = usage

    def calculate_output_tokens(self) -> int:
        """
        Calculate output tokens from accumulated content.

        Returns:
            Number of output tokens
        """
        if not self._output_tokens_calculated:
            if self.output_content:
                self._cached_output_tokens = count_tokens(
                    self.output_content, self.model
                )
            self._output_tokens_calculated = True
        return self._cached_output_tokens

    def finalize(self) -> dict[str, int]:
        """
        Finalize and return usage dict.

        Prioritizes provider usage over local calculation.

        Returns:
            Usage dict with prompt_tokens, completion_tokens, total_tokens
        """
        # Prefer provider usage if available and has valid values
        if self.provider_usage:
            input_tokens = self.provider_usage.get(
                "prompt_tokens", self.provider_usage.get("input_tokens", 0)
            )
            output_tokens = self.provider_usage.get(
                "completion_tokens", self.provider_usage.get("output_tokens", 0)
            )

            if input_tokens > 0 or output_tokens > 0:
                return {
                    "prompt_tokens": input_tokens,
                    "completion_tokens": output_tokens,
                    "total_tokens": input_tokens + output_tokens,
                }

        # Fallback to local calculation
        output_tokens = self.calculate_output_tokens()
        return {
            "prompt_tokens": self.input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": self.input_tokens + output_tokens,
        }

    def finalize_unified(self) -> dict[str, int]:
        """
        Finalize and return usage dict in unified format (input_tokens/output_tokens).

        This is for V2 endpoints that use the unified format.

        Returns:
            Usage dict with input_tokens, output_tokens
        """
        usage = self.finalize()
        return {
            "input_tokens": usage["prompt_tokens"],
            "output_tokens": usage["completion_tokens"],
        }

    def has_provider_usage(self) -> bool:
        """
        Check if provider usage is available and valid.

        Returns:
            True if provider usage has valid values
        """
        if not self.provider_usage:
            return False

        input_tokens = self.provider_usage.get(
            "prompt_tokens", self.provider_usage.get("input_tokens", 0)
        )
        output_tokens = self.provider_usage.get(
            "completion_tokens", self.provider_usage.get("output_tokens", 0)
        )

        return input_tokens > 0 or output_tokens > 0
