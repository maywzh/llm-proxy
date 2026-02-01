"""Tests for model permission matching logic"""

import pytest
from unittest.mock import MagicMock
from fastapi import HTTPException

from app.api.dependencies import model_matches_allowed_list, check_model_permission


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


@pytest.mark.unit
class TestCheckModelPermission:
    """Tests for check_model_permission function."""

    def test_allows_when_no_credential(self):
        """Test that permission check passes when no credential is provided."""
        # Should not raise
        check_model_permission("gpt-4", None)

    def test_allows_when_no_restrictions(self):
        """Test that permission check passes when credential has no restrictions."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = []
        # Should not raise
        check_model_permission("gpt-4", mock_credential)

    def test_allows_when_no_model(self):
        """Test that permission check passes when no model is provided."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["gpt-4"]
        # Should not raise
        check_model_permission(None, mock_credential)

    def test_allows_matching_model(self):
        """Test that permission check passes for allowed models."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["gpt-4", "gpt-3.5-turbo"]
        # Should not raise
        check_model_permission("gpt-4", mock_credential)

    def test_denies_unauthorized_model(self):
        """Test that permission check raises 403 for unauthorized models."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["gpt-4"]

        with pytest.raises(HTTPException) as exc_info:
            check_model_permission("claude-3", mock_credential)
        assert exc_info.value.status_code == 403
        assert "not allowed" in exc_info.value.detail.lower()

    def test_supports_wildcard(self):
        """Test that permission check supports wildcard patterns."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["gpt-*"]

        # Should not raise for matching pattern
        check_model_permission("gpt-4", mock_credential)
        check_model_permission("gpt-3.5-turbo", mock_credential)

        # Should raise for non-matching
        with pytest.raises(HTTPException) as exc_info:
            check_model_permission("claude-3", mock_credential)
        assert exc_info.value.status_code == 403

    def test_supports_regex(self):
        """Test that permission check supports regex patterns."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = [
            "claude-haiku-4-5-.+"
        ]  # Requires at least one char after dash

        # Should not raise for matching pattern
        check_model_permission("claude-haiku-4-5-20241022", mock_credential)

        # Should raise for non-matching (no suffix after dash)
        with pytest.raises(HTTPException) as exc_info:
            check_model_permission("claude-haiku-4-5-", mock_credential)
        assert exc_info.value.status_code == 403

    def test_exact_match_no_partial(self):
        """Test that exact model names don't allow partial matches."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["grok-4"]

        # Exact match should work
        check_model_permission("grok-4", mock_credential)

        # Partial match should fail
        with pytest.raises(HTTPException) as exc_info:
            check_model_permission("grok-4-vision", mock_credential)
        assert exc_info.value.status_code == 403

    def test_error_message_includes_model_and_allowed_list(self):
        """Test that error message includes the model and allowed list."""
        mock_credential = MagicMock()
        mock_credential.allowed_models = ["gpt-4", "gpt-3.5-*"]

        with pytest.raises(HTTPException) as exc_info:
            check_model_permission("claude-3", mock_credential)

        detail = exc_info.value.detail
        assert "claude-3" in detail
        assert "gpt-4" in detail or "allowed" in detail.lower()
