"""Tests for the tokenizer module."""

import pytest

from app.core.tokenizer import (
    TokenizerType,
    count_tokens,
    count_tokens_hf,
    count_tokens_tiktoken,
    get_claude_tokenizer,
    get_tokenizer_info,
    select_tokenizer,
)


class TestSelectTokenizer:
    """Tests for tokenizer selection logic."""

    def test_select_tokenizer_openai(self):
        """OpenAI models should use tiktoken."""
        tokenizer_type, tiktoken_model = select_tokenizer("gpt-4")
        assert tokenizer_type == TokenizerType.TIKTOKEN
        assert tiktoken_model == "gpt-4"

    def test_select_tokenizer_gpt35(self):
        """GPT-35 models should be normalized to GPT-3.5."""
        tokenizer_type, tiktoken_model = select_tokenizer("gpt-35-turbo")
        assert tokenizer_type == TokenizerType.TIKTOKEN
        assert tiktoken_model == "gpt-3.5-turbo"

    def test_select_tokenizer_unknown(self):
        """Unknown models should default to gpt-3.5-turbo."""
        tokenizer_type, tiktoken_model = select_tokenizer("unknown-model")
        assert tokenizer_type == TokenizerType.TIKTOKEN
        assert tiktoken_model == "gpt-3.5-turbo"

    def test_select_tokenizer_claude(self):
        """Claude models should use HuggingFace tokenizer."""
        tokenizer_type, tiktoken_model = select_tokenizer("claude-3-opus")
        assert tokenizer_type == TokenizerType.HUGGINGFACE
        assert tiktoken_model is None

    def test_select_tokenizer_claude_bedrock(self):
        """Claude models with -bedrock suffix should use HuggingFace tokenizer."""
        tokenizer_type, tiktoken_model = select_tokenizer("claude-3-opus-bedrock")
        assert tokenizer_type == TokenizerType.HUGGINGFACE
        assert tiktoken_model is None

    def test_select_tokenizer_claude_vertex(self):
        """Claude models with -vertex suffix should use HuggingFace tokenizer."""
        tokenizer_type, tiktoken_model = select_tokenizer("claude-3-sonnet-vertex")
        assert tokenizer_type == TokenizerType.HUGGINGFACE
        assert tiktoken_model is None

    def test_select_tokenizer_o1_models(self):
        """O1 models should use tiktoken."""
        tokenizer_type, tiktoken_model = select_tokenizer("o1-preview")
        assert tokenizer_type == TokenizerType.TIKTOKEN
        assert tiktoken_model == "o1-preview"

    def test_select_tokenizer_o3_models(self):
        """O3 models should use tiktoken."""
        tokenizer_type, tiktoken_model = select_tokenizer("o3-mini")
        assert tokenizer_type == TokenizerType.TIKTOKEN
        assert tiktoken_model == "o3-mini"


class TestClaudeTokenizer:
    """Tests for embedded Claude tokenizer."""

    def test_get_claude_tokenizer(self):
        """Should load embedded Claude tokenizer."""
        tokenizer = get_claude_tokenizer()
        assert tokenizer is not None

    def test_claude_tokenizer_caching(self):
        """Claude tokenizer should be cached."""
        tokenizer1 = get_claude_tokenizer()
        tokenizer2 = get_claude_tokenizer()
        assert tokenizer1 is tokenizer2

    def test_count_tokens_hf(self):
        """Should count tokens using HuggingFace tokenizer."""
        tokenizer = get_claude_tokenizer()
        if tokenizer:
            count = count_tokens_hf("Hello, world!", tokenizer)
            assert count > 0
            assert isinstance(count, int)


class TestCountTokens:
    """Tests for unified token counting."""

    def test_count_tokens_tiktoken(self):
        """Should count tokens using tiktoken."""
        count = count_tokens_tiktoken("Hello, world!", "gpt-4")
        assert count > 0
        assert isinstance(count, int)

    def test_count_tokens_gpt4o(self):
        """GPT-4o should use o200k_base encoding."""
        count = count_tokens_tiktoken("Hello, world!", "gpt-4o")
        assert count > 0
        assert isinstance(count, int)

    def test_count_tokens_unified_openai(self):
        """Unified count_tokens should work for OpenAI models."""
        count = count_tokens("Hello, world!", "gpt-4")
        assert count > 0
        assert isinstance(count, int)

    def test_count_tokens_unified_claude(self):
        """Unified count_tokens should work for Claude models."""
        count = count_tokens("Hello, world!", "claude-3-opus")
        assert count > 0
        assert isinstance(count, int)

    def test_count_tokens_empty_string(self):
        """Should handle empty strings."""
        count = count_tokens("", "gpt-4")
        assert count == 0

    def test_count_tokens_long_text(self):
        """Should handle longer text."""
        long_text = "Hello, world! " * 100
        count = count_tokens(long_text, "gpt-4")
        assert count > 100  # Should be more than 100 tokens


class TestGetTokenizerInfo:
    """Tests for tokenizer info."""

    def test_get_tokenizer_info_openai(self):
        """Should return tiktoken info for OpenAI models."""
        info = get_tokenizer_info("gpt-4")
        assert "tiktoken" in info

    def test_get_tokenizer_info_claude(self):
        """Should return HuggingFace info for Claude models."""
        info = get_tokenizer_info("claude-3-opus")
        assert "HuggingFace" in info
