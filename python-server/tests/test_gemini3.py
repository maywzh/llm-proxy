"""Unit tests for Gemini 3 thought_signature support."""

import pytest
from unittest.mock import patch

from app.utils.gemini3 import (
    THOUGHT_SIGNATURE_SEPARATOR,
    _encode_tool_call_id_with_signature,
    _get_dummy_thought_signature,
    is_gemini3_model,
    log_gemini_response_signatures,
    log_gemini_request_signatures,
    normalize_gemini3_request,
    normalize_gemini3_response,
)


class TestIsGemini3Model:
    """Tests for is_gemini3_model function."""

    def test_gemini3_with_hyphen(self):
        """Should detect gemini-3 variants."""
        assert is_gemini3_model("gemini-3-pro") is True
        assert is_gemini3_model("Gemini-3-Pro") is True
        assert is_gemini3_model("GEMINI-3-FLASH") is True
        assert is_gemini3_model("vertex_ai/gemini-3-pro-preview") is True

    def test_non_gemini3_models(self):
        """Should NOT detect non-Gemini 3 models."""
        assert is_gemini3_model("gemini-2.5-pro") is False
        assert is_gemini3_model("gemini-flash") is False  # Gemini 1.x
        assert is_gemini3_model("gemini-pro") is False  # Gemini 1.x
        assert is_gemini3_model("gemini3-pro") is False
        assert is_gemini3_model("gemini_3_pro") is False
        assert is_gemini3_model("gpt-4") is False
        assert is_gemini3_model("claude-3-opus") is False
        assert is_gemini3_model("openai") is False

    def test_empty_and_edge_cases(self):
        """Should handle edge cases."""
        assert is_gemini3_model("") is False
        assert is_gemini3_model("gemini") is False
        assert is_gemini3_model("3") is False


class TestLogGeminiResponseSignatures:
    """Tests for log_gemini_response_signatures function."""

    @patch("app.utils.gemini3.logger")
    def test_non_gemini3_provider_no_logging(self, mock_logger):
        """Should not log for non-Gemini 3 models."""
        response_data = {
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "extra_content": {
                            "google": {"thought_signature": "test_signature"}
                        }
                    }]
                }
            }]
        }
        log_gemini_response_signatures(response_data, "gpt-4")
        mock_logger.debug.assert_not_called()

    @patch("app.utils.gemini3.logger")
    def test_gemini3_provider_with_tool_call_signature(self, mock_logger):
        """Should log when thought_signature is found in tool_calls."""
        response_data = {
            "choices": [{
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {"name": "read_file", "arguments": "{}"},
                        "extra_content": {
                            "google": {"thought_signature": "CvcQAdHN2OekY10ClPFkYA=="}
                        }
                    }]
                }
            }]
        }
        log_gemini_response_signatures(response_data, "gemini-3-pro")
        mock_logger.debug.assert_called()
        # Check that the log message mentions tool_calls
        call_args = str(mock_logger.debug.call_args)
        assert "tool_calls" in call_args

    @patch("app.utils.gemini3.logger")
    def test_gemini3_provider_with_content_signature(self, mock_logger):
        """Should log when thought_signature is found at content level."""
        response_data = {
            "choices": [{
                "message": {
                    "content": "I can help with that",
                    "extra_content": {
                        "google": {"thought_signature": "CrICAdHtim827fQ..."}
                    }
                }
            }]
        }
        log_gemini_response_signatures(response_data, "gemini-3-flash")
        mock_logger.debug.assert_called()
        call_args = str(mock_logger.debug.call_args)
        assert "extra_content" in call_args

    @patch("app.utils.gemini3.logger")
    def test_gemini3_provider_streaming_delta(self, mock_logger):
        """Should log when thought_signature is found in streaming delta."""
        response_data = {
            "choices": [{
                "delta": {
                    "role": "assistant",
                    "tool_calls": [{
                        "extra_content": {
                            "google": {"thought_signature": "test_signature"}
                        },
                        "function": {"name": "get_weather"},
                        "id": "call_456",
                        "type": "function"
                    }]
                },
                "index": 0
            }]
        }
        log_gemini_response_signatures(response_data, "gemini-3-pro")
        mock_logger.debug.assert_called()

    @patch("app.utils.gemini3.logger")
    def test_gemini3_provider_no_signature_no_logging(self, mock_logger):
        """Should not log when no thought_signature is present."""
        response_data = {
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello!"
                }
            }]
        }
        log_gemini_response_signatures(response_data, "gemini-3-pro")
        mock_logger.debug.assert_not_called()

    @patch("app.utils.gemini3.logger")
    def test_gemini3_provider_multiple_signatures(self, mock_logger):
        """Should count multiple signatures correctly."""
        response_data = {
            "choices": [{
                "message": {
                    "tool_calls": [
                        {
                            "id": "call_1",
                            "extra_content": {"google": {"thought_signature": "sig1"}}
                        },
                        {
                            "id": "call_2",
                            "extra_content": {"google": {"thought_signature": "sig2"}}
                        },
                        {
                            "id": "call_3",
                            # No extra_content
                        }
                    ]
                }
            }]
        }
        log_gemini_response_signatures(response_data, "gemini-3-pro")
        mock_logger.debug.assert_called()
        call_args = str(mock_logger.debug.call_args)
        assert "2" in call_args  # Should find 2 signatures

    @patch("app.utils.gemini3.logger")
    def test_malformed_response_no_error(self, mock_logger):
        """Should handle malformed responses gracefully."""
        # Empty response
        log_gemini_response_signatures({}, "gemini-3-pro")
        mock_logger.debug.assert_not_called()

        # Choices is not a list
        log_gemini_response_signatures({"choices": "invalid"}, "gemini-3-pro")
        mock_logger.debug.assert_not_called()

        # extra_content is not a dict
        log_gemini_response_signatures({
            "choices": [{
                "message": {
                    "extra_content": "invalid"
                }
            }]
        }, "gemini-3-pro")
        mock_logger.debug.assert_not_called()


class TestLogGeminiRequestSignatures:
    """Tests for log_gemini_request_signatures function."""

    @patch("app.utils.gemini3.logger")
    def test_non_gemini3_provider_no_logging(self, mock_logger):
        """Should not log for non-Gemini 3 models."""
        request_data = {
            "messages": [{
                "role": "assistant",
                "tool_calls": [{
                    "extra_content": {"google": {"thought_signature": "test"}}
                }]
            }]
        }
        log_gemini_request_signatures(request_data, "gpt-4")
        mock_logger.debug.assert_not_called()

    @patch("app.utils.gemini3.logger")
    def test_gemini3_request_with_signature(self, mock_logger):
        """Should log when request contains thought_signature."""
        request_data = {
            "messages": [
                {"role": "user", "content": "Get weather for NYC"},
                {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": '{"location": "NYC"}'
                        },
                        "extra_content": {
                            "google": {"thought_signature": "CvcQAdHN2OekY10ClPFkYA=="}
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_123",
                    "content": "72°F and sunny"
                }
            ]
        }
        log_gemini_request_signatures(request_data, "gemini-3-pro")
        mock_logger.debug.assert_called()
        call_args = str(mock_logger.debug.call_args)
        assert "pass-through" in call_args
        assert "1" in call_args  # 1 signature

    @patch("app.utils.gemini3.logger")
    def test_gemini3_request_no_signature_no_logging(self, mock_logger):
        """Should not log when no thought_signature in request."""
        request_data = {
            "messages": [
                {"role": "user", "content": "Hello!"},
                {"role": "assistant", "content": "Hi there!"}
            ]
        }
        log_gemini_request_signatures(request_data, "gemini-3-pro")
        mock_logger.debug.assert_not_called()

    @patch("app.utils.gemini3.logger")
    def test_gemini3_request_multiple_signatures(self, mock_logger):
        """Should count multiple signatures in request."""
        request_data = {
            "messages": [{
                "role": "assistant",
                "tool_calls": [
                    {"extra_content": {"google": {"thought_signature": "sig1"}}},
                    {"extra_content": {"google": {"thought_signature": "sig2"}}},
                    {"extra_content": {"google": {"thought_signature": "sig3"}}},
                ]
            }]
        }
        log_gemini_request_signatures(request_data, "gemini-3-pro")
        mock_logger.debug.assert_called()
        call_args = str(mock_logger.debug.call_args)
        assert "3" in call_args  # 3 signatures

    @patch("app.utils.gemini3.logger")
    def test_malformed_request_no_error(self, mock_logger):
        """Should handle malformed requests gracefully."""
        # No messages
        log_gemini_request_signatures({}, "gemini-3-pro")
        mock_logger.debug.assert_not_called()

        # Messages not a list
        log_gemini_request_signatures({"messages": "invalid"}, "gemini-3-pro")
        mock_logger.debug.assert_not_called()

        # tool_calls not a list
        log_gemini_request_signatures({
            "messages": [{"tool_calls": "invalid"}]
        }, "gemini-3-pro")
        mock_logger.debug.assert_not_called()


class TestExtraContentPreservation:
    """Tests verifying that extra_content is preserved through JSON handling.

    These tests verify the pass-through strategy: extra_content should not be
    stripped during JSON serialization/deserialization.
    """

    def test_extra_content_roundtrip(self):
        """Verify extra_content survives JSON roundtrip."""
        import json

        original = {
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": '{"location": "NYC"}'
                        },
                        "extra_content": {
                            "google": {
                                "thought_signature": "CvcQAdHN2OekY10ClPFkYA=="
                            }
                        }
                    }]
                }
            }]
        }

        # Simulate pass-through: serialize and deserialize
        serialized = json.dumps(original)
        deserialized = json.loads(serialized)

        # Verify extra_content is preserved
        tool_call = deserialized["choices"][0]["message"]["tool_calls"][0]
        assert "extra_content" in tool_call
        assert "google" in tool_call["extra_content"]
        assert "thought_signature" in tool_call["extra_content"]["google"]
        assert tool_call["extra_content"]["google"]["thought_signature"] == "CvcQAdHN2OekY10ClPFkYA=="

    def test_request_format_with_extra_content(self):
        """Verify the expected request format with extra_content."""
        import json

        # This is the format that should be sent back to Gemini 3
        request = {
            "model": "gemini-3-pro",
            "messages": [
                {"role": "user", "content": "Get weather for funny city names"},
                {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [{
                        "id": "function-call-5873527561210830497",
                        "type": "function",
                        "function": {
                            "name": "get_current_weather",
                            "arguments": '{"location":"Intercourse, PA","unit":"fahrenheit"}'
                        },
                        "extra_content": {
                            "google": {
                                "thought_signature": "CvcQAdHN2OekY10ClPFkYA=="
                            }
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "function-call-5873527561210830497",
                    "content": "66°F and Sunny"
                }
            ]
        }

        # Verify structure is valid JSON
        serialized = json.dumps(request)
        deserialized = json.loads(serialized)

        # Verify the nested structure is preserved
        assistant_msg = deserialized["messages"][1]
        assert assistant_msg["role"] == "assistant"
        assert len(assistant_msg["tool_calls"]) == 1

        tool_call = assistant_msg["tool_calls"][0]
        assert tool_call["extra_content"]["google"]["thought_signature"] == "CvcQAdHN2OekY10ClPFkYA=="


class TestGemini3Normalization:
    """Tests for Gemini 3 request/response normalization."""

    def test_normalize_request_adds_dummy_signature(self):
        request = {
            "model": "gemini-3-pro",
            "messages": [{
                "role": "assistant",
                "tool_calls": [{
                    "id": "call_123",
                    "type": "function",
                    "function": {"name": "get_weather", "arguments": "{}"}
                }]
            }]
        }
        changed = normalize_gemini3_request(request, request["model"])
        assert changed is True
        tool_call = request["messages"][0]["tool_calls"][0]
        assert tool_call["provider_specific_fields"]["thought_signature"] == _get_dummy_thought_signature()

    def test_normalize_request_extracts_signature_from_id(self):
        signature = "sig_from_id"
        encoded_id = _encode_tool_call_id_with_signature("call_abc", signature)
        request = {
            "model": "gemini-3-pro",
            "messages": [{
                "role": "assistant",
                "tool_calls": [{
                    "id": encoded_id,
                    "type": "function",
                    "function": {"name": "do", "arguments": "{}"}
                }]
            }]
        }
        changed = normalize_gemini3_request(request, request["model"])
        assert changed is True
        tool_call = request["messages"][0]["tool_calls"][0]
        assert tool_call["provider_specific_fields"]["thought_signature"] == signature

    def test_normalize_request_strips_signature_for_non_gemini(self):
        signature = "sig"
        encoded_id = _encode_tool_call_id_with_signature("call_abc", signature)
        request = {
            "model": "gpt-4",
            "messages": [
                {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": encoded_id,
                        "type": "function",
                        "function": {"name": "do", "arguments": "{}"}
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": encoded_id,
                    "content": "ok"
                }
            ]
        }
        changed = normalize_gemini3_request(request, request["model"])
        assert changed is True
        tool_call_id = request["messages"][0]["tool_calls"][0]["id"]
        tool_msg_id = request["messages"][1]["tool_call_id"]
        assert THOUGHT_SIGNATURE_SEPARATOR not in tool_call_id
        assert THOUGHT_SIGNATURE_SEPARATOR not in tool_msg_id

    def test_normalize_response_embeds_signature(self):
        signature = "sig_resp"
        response = {
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {"name": "do", "arguments": "{}"},
                        "extra_content": {"google": {"thought_signature": signature}}
                    }]
                }
            }]
        }
        changed = normalize_gemini3_response(response, "gemini-3-pro")
        assert changed is True
        tool_call = response["choices"][0]["message"]["tool_calls"][0]
        assert tool_call["provider_specific_fields"]["thought_signature"] == signature
        assert THOUGHT_SIGNATURE_SEPARATOR in tool_call["id"]

    def test_normalize_response_sets_message_thought_signatures(self):
        signature = "sig_msg"
        response = {
            "choices": [{
                "message": {
                    "content": "hello",
                    "extra_content": {"google": {"thought_signature": signature}}
                }
            }]
        }
        changed = normalize_gemini3_response(response, "gemini-3-pro")
        assert changed is True
        provider_fields = response["choices"][0]["message"]["provider_specific_fields"]
        assert provider_fields["thought_signatures"] == [signature]

    def test_normalize_request_sets_default_temperature(self):
        request = {
            "model": "gemini-3-pro",
            "messages": [{"role": "user", "content": "hi"}],
        }
        changed = normalize_gemini3_request(request, request["model"])
        assert changed is True
        assert request["temperature"] == 1.0

    def test_normalize_request_keeps_existing_temperature(self):
        request = {
            "model": "gemini-3-pro",
            "temperature": 0.2,
            "messages": [{"role": "user", "content": "hi"}],
        }
        changed = normalize_gemini3_request(request, request["model"])
        assert changed is False
        assert request["temperature"] == 0.2

    def test_normalize_reasoning_effort_maps_thinking_level(self):
        request = {
            "model": "gemini-3-pro",
            "reasoning_effort": "medium",
            "messages": [{"role": "user", "content": "hi"}],
        }
        changed = normalize_gemini3_request(request, request["model"])
        assert changed is True
        assert request["thinking_level"] == "high"
        assert "reasoning_effort" not in request

    def test_normalize_reasoning_effort_maps_flash(self):
        request = {
            "model": "gemini-3-flash-preview",
            "reasoning_effort": "medium",
            "messages": [{"role": "user", "content": "hi"}],
        }
        changed = normalize_gemini3_request(request, request["model"])
        assert changed is True
        assert request["thinking_level"] == "medium"
        assert "reasoning_effort" not in request
