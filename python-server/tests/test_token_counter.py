"""Tests for the OutboundTokenCounter module."""

import pytest

from app.core.token_counter import OutboundTokenCounter


class TestOutboundTokenCounter:
    """Tests for OutboundTokenCounter."""

    def test_init_with_model_and_input_tokens(self):
        """Should initialize with model and input tokens."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        assert counter.model == "gpt-4"
        assert counter.input_tokens == 100
        assert counter.output_content == ""
        assert counter.provider_usage is None

    def test_accumulate_content(self):
        """Should accumulate output content."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.accumulate_content("Hello, ")
        counter.accumulate_content("world!")
        assert counter.output_content == "Hello, world!"

    def test_accumulate_content_empty(self):
        """Should handle empty content."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.accumulate_content("")
        assert counter.output_content == ""

    def test_update_provider_usage(self):
        """Should update provider usage."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        usage = {"prompt_tokens": 50, "completion_tokens": 30, "total_tokens": 80}
        counter.update_provider_usage(usage)
        assert counter.provider_usage == usage

    def test_finalize_with_provider_usage(self):
        """Should prioritize provider usage in finalize."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.accumulate_content("Hello, world!")
        counter.update_provider_usage(
            {"prompt_tokens": 50, "completion_tokens": 30, "total_tokens": 80}
        )

        result = counter.finalize()
        assert result["prompt_tokens"] == 50
        assert result["completion_tokens"] == 30
        assert result["total_tokens"] == 80

    def test_finalize_with_input_tokens_format(self):
        """Should handle input_tokens/output_tokens format from provider."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.update_provider_usage({"input_tokens": 50, "output_tokens": 30})

        result = counter.finalize()
        assert result["prompt_tokens"] == 50
        assert result["completion_tokens"] == 30
        assert result["total_tokens"] == 80

    def test_finalize_fallback_to_local_calculation(self):
        """Should fallback to local calculation when no provider usage."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.accumulate_content("Hello, world!")

        result = counter.finalize()
        assert result["prompt_tokens"] == 100
        assert result["completion_tokens"] > 0
        assert (
            result["total_tokens"]
            == result["prompt_tokens"] + result["completion_tokens"]
        )

    def test_finalize_empty_provider_usage(self):
        """Should fallback when provider usage has zero values."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.accumulate_content("Hello, world!")
        counter.update_provider_usage({"prompt_tokens": 0, "completion_tokens": 0})

        result = counter.finalize()
        # Should fallback to local calculation
        assert result["prompt_tokens"] == 100
        assert result["completion_tokens"] > 0

    def test_finalize_unified(self):
        """Should return unified format."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.accumulate_content("Hello, world!")

        result = counter.finalize_unified()
        assert "input_tokens" in result
        assert "output_tokens" in result
        assert result["input_tokens"] == 100
        assert result["output_tokens"] > 0

    def test_has_provider_usage_true(self):
        """Should return True when provider usage is valid."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.update_provider_usage({"prompt_tokens": 50, "completion_tokens": 30})
        assert counter.has_provider_usage() is True

    def test_has_provider_usage_false_no_usage(self):
        """Should return False when no provider usage."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        assert counter.has_provider_usage() is False

    def test_has_provider_usage_false_zero_values(self):
        """Should return False when provider usage has zero values."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.update_provider_usage({"prompt_tokens": 0, "completion_tokens": 0})
        assert counter.has_provider_usage() is False

    def test_calculate_output_tokens_caching(self):
        """Should cache output token calculation."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)
        counter.accumulate_content("Hello, world!")

        # First call calculates
        tokens1 = counter.calculate_output_tokens()
        # Second call should return cached value
        tokens2 = counter.calculate_output_tokens()

        assert tokens1 == tokens2
        assert counter._output_tokens_calculated is True

    def test_claude_model_token_counting(self):
        """Should use Claude tokenizer for Claude models."""
        counter = OutboundTokenCounter(model="claude-3-opus", input_tokens=100)
        counter.accumulate_content("Hello, world!")

        result = counter.finalize()
        assert result["completion_tokens"] > 0

    def test_empty_output_content(self):
        """Should handle empty output content."""
        counter = OutboundTokenCounter(model="gpt-4", input_tokens=100)

        result = counter.finalize()
        assert result["prompt_tokens"] == 100
        assert result["completion_tokens"] == 0
        assert result["total_tokens"] == 100
