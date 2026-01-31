"""Tests for model permission matching logic"""

import pytest

from app.api.dependencies import model_matches_allowed_list


@pytest.mark.unit
class TestModelMatchesAllowedList:
    """Test model matching against allowed_models list"""

    def test_exact_match(self):
        """Test exact string matching"""
        allowed = ["gpt-4", "gpt-3.5-turbo", "claude-3-opus"]

        assert model_matches_allowed_list("gpt-4", allowed) is True
        assert model_matches_allowed_list("gpt-3.5-turbo", allowed) is True
        assert model_matches_allowed_list("claude-3-opus", allowed) is True
        assert model_matches_allowed_list("gpt-4o", allowed) is False
        assert model_matches_allowed_list("unknown", allowed) is False

    def test_simple_wildcard(self):
        """Test simple wildcard (*) matching"""
        allowed = ["gpt-*", "claude-3-*"]

        assert model_matches_allowed_list("gpt-4", allowed) is True
        assert model_matches_allowed_list("gpt-4o", allowed) is True
        assert model_matches_allowed_list("gpt-3.5-turbo", allowed) is True
        assert model_matches_allowed_list("claude-3-opus", allowed) is True
        assert model_matches_allowed_list("claude-3-sonnet", allowed) is True
        assert model_matches_allowed_list("claude-2", allowed) is False
        assert model_matches_allowed_list("llama-2", allowed) is False

    def test_regex_pattern_dotstar(self):
        """Test regex pattern with .* matching"""
        allowed = ["claude-opus-4-5-.*", "gpt-4-.*"]

        assert model_matches_allowed_list("claude-opus-4-5-20240620", allowed) is True
        assert model_matches_allowed_list("claude-opus-4-5-latest", allowed) is True
        assert model_matches_allowed_list("gpt-4-turbo", allowed) is True
        assert model_matches_allowed_list("gpt-4-0125-preview", allowed) is True
        assert model_matches_allowed_list("claude-opus-4-5", allowed) is False
        assert model_matches_allowed_list("gpt-4", allowed) is False
        assert model_matches_allowed_list("gpt-3.5-turbo", allowed) is False

    def test_regex_pattern_alternation(self):
        """Test regex pattern with alternation (|)"""
        allowed = ["(gpt-4|gpt-3.5-turbo)"]

        assert model_matches_allowed_list("gpt-4", allowed) is True
        assert model_matches_allowed_list("gpt-3.5-turbo", allowed) is True
        assert model_matches_allowed_list("gpt-4o", allowed) is False
        assert model_matches_allowed_list("claude-3", allowed) is False

    def test_mixed_patterns(self):
        """Test mixed exact and wildcard patterns"""
        allowed = ["gpt-4", "claude-*", "llama-2-.*"]

        assert model_matches_allowed_list("gpt-4", allowed) is True
        assert model_matches_allowed_list("gpt-4o", allowed) is False
        assert model_matches_allowed_list("claude-3-opus", allowed) is True
        assert model_matches_allowed_list("claude-2", allowed) is True
        assert model_matches_allowed_list("llama-2-70b", allowed) is True
        assert model_matches_allowed_list("llama-2", allowed) is False
        assert model_matches_allowed_list("llama-3", allowed) is False

    def test_empty_list(self):
        """Test empty allowed_models list"""
        assert model_matches_allowed_list("any-model", []) is False

    def test_model_with_dots_not_treated_as_pattern(self):
        """Test that models with dots (like gpt-3.5-turbo) are not treated as patterns"""
        allowed = ["gpt-3.5-turbo"]

        assert model_matches_allowed_list("gpt-3.5-turbo", allowed) is True
        assert model_matches_allowed_list("gpt-345-turbo", allowed) is False
