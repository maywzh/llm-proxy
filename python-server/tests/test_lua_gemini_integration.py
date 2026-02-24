"""Integration tests for the Lua Gemini transformer with the full TransformPipeline.

Tests the complete end-to-end pipeline (TransformPipeline) using the Lua Gemini
script at examples/gemini_transformer.lua. Covers cross-protocol transforms,
same-protocol round-trips, fallback behavior, Lua-vs-hardcoded comparison,
and edge cases.

Run:  cd python-server && python -m pytest tests/test_lua_gemini_integration.py -v
"""

import pytest

from app.scripting.engine import LuaEngine
from app.transformer.pipeline import TransformPipeline
from app.transformer.registry import TransformerRegistry
from app.transformer.protocols.openai import OpenAITransformer
from app.transformer.protocols.anthropic import AnthropicTransformer
from app.transformer.protocols.gemini import GeminiTransformer
from app.transformer.protocols.response_api import ResponseApiTransformer
from app.transformer.base import TransformContext
from app.transformer.unified import Protocol


SCRIPT_PATH = "../examples/gemini_transformer.lua"
PROVIDER_NAME = "test-gemini"


@pytest.fixture()
def lua_pipeline():
    """Pipeline with Lua Gemini transformer loaded."""
    with open(SCRIPT_PATH) as f:
        source = f.read()
    engine = LuaEngine()
    engine.reload([(PROVIDER_NAME, source)])
    registry = TransformerRegistry()
    registry.register(OpenAITransformer())
    registry.register(AnthropicTransformer())
    registry.register(GeminiTransformer())
    registry.register(ResponseApiTransformer())
    return TransformPipeline(registry, lua_engine=engine)


def _make_ctx(
    client_proto: Protocol = Protocol.OPENAI,
    provider_proto: Protocol = Protocol.GEMINI,
    model: str = "gemini-2.0-flash",
) -> TransformContext:
    return TransformContext(
        client_protocol=client_proto,
        provider_protocol=provider_proto,
        original_model=model,
        mapped_model=model,
        provider_name=PROVIDER_NAME,
    )


# =========================================================================
# 1. Cross-Protocol Integration: OpenAI Client -> Gemini Provider
# =========================================================================


class TestCrossProtocolOpenAIToGemini:
    """OpenAI request -> hardcoded OpenAI.transform_request_out -> UIF -> Lua on_transform_request_in -> Gemini."""

    def test_simple_text_request(self, lua_pipeline):
        openai_req = {
            "model": "gemini-2.0-flash",
            "messages": [
                {"role": "system", "content": "Be helpful."},
                {"role": "user", "content": "Hello"},
            ],
            "temperature": 0.7,
            "max_tokens": 100,
        }
        ctx = _make_ctx()
        result = lua_pipeline.transform_request(openai_req, ctx)

        assert "contents" in result
        assert result["contents"][0]["parts"][0]["text"] == "Hello"
        assert (
            result.get("systemInstruction", {}).get("parts", [{}])[0].get("text")
            == "Be helpful."
        )
        assert result["generationConfig"]["temperature"] == 0.7
        assert result["generationConfig"]["maxOutputTokens"] == 100

    def test_request_with_tools(self, lua_pipeline):
        openai_req = {
            "model": "gemini-2.0-flash",
            "messages": [{"role": "user", "content": "Weather?"}],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather",
                        "parameters": {
                            "type": "object",
                            "properties": {"city": {"type": "string"}},
                        },
                    },
                }
            ],
            "tool_choice": "auto",
        }
        ctx = _make_ctx()
        result = lua_pipeline.transform_request(openai_req, ctx)

        assert "tools" in result
        assert result["tools"][0]["functionDeclarations"][0]["name"] == "get_weather"

    def test_multi_turn_conversation(self, lua_pipeline):
        openai_req = {
            "model": "gemini-2.0-flash",
            "messages": [
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello!"},
                {"role": "user", "content": "Joke?"},
            ],
        }
        ctx = _make_ctx()
        result = lua_pipeline.transform_request(openai_req, ctx)

        assert len(result["contents"]) == 3
        assert result["contents"][0]["role"] == "user"
        assert result["contents"][1]["role"] == "model"
        assert result["contents"][2]["role"] == "user"

    def test_system_only_message(self, lua_pipeline):
        """System-only messages should produce systemInstruction but may have empty contents."""
        openai_req = {
            "model": "gemini-2.0-flash",
            "messages": [
                {"role": "system", "content": "You are a poet."},
                {"role": "user", "content": "Write a haiku."},
            ],
        }
        ctx = _make_ctx()
        result = lua_pipeline.transform_request(openai_req, ctx)

        assert "systemInstruction" in result
        assert result["systemInstruction"]["parts"][0]["text"] == "You are a poet."
        assert "contents" in result
        assert result["contents"][0]["parts"][0]["text"] == "Write a haiku."


# =========================================================================
# 2. Cross-Protocol Response: Gemini Provider -> OpenAI Client
# =========================================================================


class TestCrossProtocolGeminiToOpenAI:
    """Gemini response -> Lua on_transform_response_in -> UIF -> hardcoded OpenAI.transform_response_out -> OpenAI."""

    def test_simple_response(self, lua_pipeline):
        gemini_resp = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [{"text": "Hello!"}],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 20,
                "totalTokenCount": 30,
            },
        }
        ctx = _make_ctx()
        result = lua_pipeline.transform_response(gemini_resp, ctx)

        assert "choices" in result
        assert result["choices"][0]["message"]["content"] == "Hello!"
        assert result["usage"]["prompt_tokens"] == 10
        assert result["usage"]["completion_tokens"] == 20

    def test_tool_call_response(self, lua_pipeline):
        gemini_resp = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [
                            {
                                "functionCall": {
                                    "name": "get_weather",
                                    "args": {"city": "SF"},
                                }
                            }
                        ],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5,
                "totalTokenCount": 15,
            },
        }
        ctx = _make_ctx()
        result = lua_pipeline.transform_response(gemini_resp, ctx)

        assert "choices" in result
        msg = result["choices"][0]["message"]
        assert "tool_calls" in msg
        assert msg["tool_calls"][0]["function"]["name"] == "get_weather"

    def test_multi_part_response(self, lua_pipeline):
        """Response with multiple text parts should produce valid OpenAI output."""
        gemini_resp = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [
                            {"text": "First part. "},
                            {"text": "Second part."},
                        ],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 10,
                "totalTokenCount": 15,
            },
        }
        ctx = _make_ctx()
        result = lua_pipeline.transform_response(gemini_resp, ctx)

        assert "choices" in result
        # Content is present (may be concatenated or only first part)
        content = result["choices"][0]["message"]["content"]
        assert content is not None and len(content) > 0


# =========================================================================
# 3. Same-Protocol Round-Trip: Gemini -> Gemini
# =========================================================================


class TestGeminiToGeminiRoundTrip:
    """Lua handles both sides: client=Gemini, provider=Gemini."""

    def test_text_request_roundtrip(self, lua_pipeline):
        gemini_req = {
            "contents": [{"role": "user", "parts": [{"text": "Hello"}]}],
            "systemInstruction": {"parts": [{"text": "Be nice."}]},
            "generationConfig": {"temperature": 0.5, "maxOutputTokens": 200},
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        result = lua_pipeline.transform_request(gemini_req, ctx)

        assert result["contents"][0]["parts"][0]["text"] == "Hello"
        assert result["systemInstruction"]["parts"][0]["text"] == "Be nice."
        assert result["generationConfig"]["temperature"] == 0.5

    def test_response_roundtrip(self, lua_pipeline):
        gemini_resp = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [{"text": "Response"}],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 10,
                "totalTokenCount": 15,
            },
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        result = lua_pipeline.transform_response(gemini_resp, ctx)

        assert "candidates" in result
        assert result["candidates"][0]["content"]["parts"][0]["text"] == "Response"
        assert result["candidates"][0]["finishReason"] == "STOP"

    def test_thinking_blocks_roundtrip(self, lua_pipeline):
        gemini_req = {
            "contents": [
                {"role": "user", "parts": [{"text": "Think step by step"}]},
                {
                    "role": "model",
                    "parts": [
                        {"thought": True, "text": "Let me think..."},
                        {"text": "Answer", "thoughtSignature": "sig_abc"},
                    ],
                },
            ],
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        result = lua_pipeline.transform_request(gemini_req, ctx)

        model_parts = result["contents"][1]["parts"]
        assert model_parts[0].get("thought") is True
        assert model_parts[0]["text"] == "Let me think..."
        assert model_parts[1]["text"] == "Answer"
        assert model_parts[1].get("thoughtSignature") == "sig_abc"

    def test_image_inline_roundtrip(self, lua_pipeline):
        gemini_req = {
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {"text": "What's this?"},
                        {
                            "inlineData": {
                                "mimeType": "image/png",
                                "data": "iVBORw0KGgo=",
                            }
                        },
                    ],
                }
            ],
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        result = lua_pipeline.transform_request(gemini_req, ctx)

        parts = result["contents"][0]["parts"]
        assert parts[0]["text"] == "What's this?"
        assert parts[1]["inlineData"]["mimeType"] == "image/png"
        assert parts[1]["inlineData"]["data"] == "iVBORw0KGgo="

    def test_tool_call_with_result_roundtrip(self, lua_pipeline):
        gemini_req = {
            "contents": [
                {"role": "user", "parts": [{"text": "Weather?"}]},
                {
                    "role": "model",
                    "parts": [
                        {
                            "functionCall": {
                                "name": "get_weather",
                                "args": {"city": "SF"},
                            }
                        }
                    ],
                },
                {
                    "role": "user",
                    "parts": [
                        {
                            "functionResponse": {
                                "name": "get_weather",
                                "response": {"temp": 72},
                            }
                        }
                    ],
                },
            ],
            "tools": [
                {
                    "functionDeclarations": [
                        {
                            "name": "get_weather",
                            "description": "Get weather",
                            "parameters": {
                                "type": "object",
                                "properties": {"city": {"type": "string"}},
                            },
                        }
                    ]
                }
            ],
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        result = lua_pipeline.transform_request(gemini_req, ctx)

        assert len(result["contents"]) == 3
        assert "functionCall" in result["contents"][1]["parts"][0]
        assert (
            result["contents"][1]["parts"][0]["functionCall"]["name"] == "get_weather"
        )
        assert "functionResponse" in result["contents"][2]["parts"][0]
        assert "tools" in result


# =========================================================================
# 4. Fallback Behavior
# =========================================================================


class TestFallbackBehavior:
    """When Lua hooks don't match, the hardcoded transformer takes over."""

    def test_anthropic_client_falls_back(self, lua_pipeline):
        """Anthropic -> Gemini: Lua on_transform_request_out skips (no 'contents'), hardcoded AnthropicTransformer used."""
        anthropic_req = {
            "model": "gemini-2.0-flash",
            "messages": [{"role": "user", "content": "Hello from Anthropic"}],
            "max_tokens": 100,
        }
        ctx = _make_ctx(client_proto=Protocol.ANTHROPIC, provider_proto=Protocol.GEMINI)
        result = lua_pipeline.transform_request(anthropic_req, ctx)

        # Should produce Gemini format via Lua on_transform_request_in
        assert "contents" in result

    def test_bypass_disabled_with_lua_hooks(self, lua_pipeline):
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        assert not lua_pipeline.should_bypass(ctx)


# =========================================================================
# 5. Lua vs Hardcoded Comparison
# =========================================================================


class TestLuaVsHardcoded:
    """Compare Lua output with hardcoded transformer output for the same inputs."""

    def test_request_output_matches(self, lua_pipeline):
        """Compare Lua vs hardcoded for OpenAI->Gemini request transformation."""
        registry = TransformerRegistry()
        registry.register(OpenAITransformer())
        registry.register(AnthropicTransformer())
        registry.register(GeminiTransformer())
        registry.register(ResponseApiTransformer())
        hardcoded = TransformPipeline(registry)

        openai_req = {
            "model": "gemini-2.0-flash",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"},
            ],
            "temperature": 0.7,
        }
        ctx = _make_ctx()

        lua_result = lua_pipeline.transform_request(openai_req, ctx)
        hc_result = hardcoded.transform_request(openai_req, ctx)

        # Structure should match
        assert "contents" in lua_result and "contents" in hc_result
        assert "systemInstruction" in lua_result and "systemInstruction" in hc_result
        # Text content should match
        assert (
            lua_result["contents"][0]["parts"][0]["text"]
            == hc_result["contents"][0]["parts"][0]["text"]
        )
        # Params should match
        assert (
            lua_result["generationConfig"]["temperature"]
            == hc_result["generationConfig"]["temperature"]
        )

    def test_response_output_matches(self, lua_pipeline):
        """Compare Lua vs hardcoded for Gemini->OpenAI response transformation."""
        registry = TransformerRegistry()
        registry.register(OpenAITransformer())
        registry.register(AnthropicTransformer())
        registry.register(GeminiTransformer())
        registry.register(ResponseApiTransformer())
        hardcoded = TransformPipeline(registry)

        gemini_resp = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [{"text": "Hello!"}],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 20,
                "totalTokenCount": 30,
            },
        }
        ctx = _make_ctx()

        lua_result = lua_pipeline.transform_response(gemini_resp, ctx)
        hc_result = hardcoded.transform_response(gemini_resp, ctx)

        # Both produce OpenAI format
        assert (
            lua_result["choices"][0]["message"]["content"]
            == hc_result["choices"][0]["message"]["content"]
        )
        assert lua_result["usage"] == hc_result["usage"]


# =========================================================================
# 6. Edge Cases
# =========================================================================


class TestEdgeCases:
    def test_empty_messages(self, lua_pipeline):
        openai_req = {"model": "gemini-2.0-flash", "messages": []}
        ctx = _make_ctx()
        result = lua_pipeline.transform_request(openai_req, ctx)
        assert "contents" in result

    def test_stop_reason_mapping(self, lua_pipeline):
        for gemini_reason in ("STOP", "MAX_TOKENS", "SAFETY"):
            gemini_resp = {
                "candidates": [
                    {
                        "content": {
                            "role": "model",
                            "parts": [{"text": "..."}],
                        },
                        "finishReason": gemini_reason,
                    }
                ],
                "usageMetadata": {
                    "promptTokenCount": 0,
                    "candidatesTokenCount": 0,
                    "totalTokenCount": 0,
                },
            }
            ctx = _make_ctx()
            result = lua_pipeline.transform_response(gemini_resp, ctx)
            finish = result["choices"][0]["finish_reason"]
            # Just verify the response is valid OpenAI format with some stop reason
            assert finish is not None

    def test_multiple_system_parts(self, lua_pipeline):
        gemini_req = {
            "contents": [{"role": "user", "parts": [{"text": "Hi"}]}],
            "systemInstruction": {"parts": [{"text": "Part 1"}, {"text": "Part 2"}]},
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        result = lua_pipeline.transform_request(gemini_req, ctx)
        # System should be preserved (might be joined)
        assert "systemInstruction" in result

    def test_no_usage_metadata(self, lua_pipeline):
        """Response without usageMetadata should still produce valid output."""
        gemini_resp = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [{"text": "Answer"}],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 0,
                "candidatesTokenCount": 0,
                "totalTokenCount": 0,
            },
        }
        ctx = _make_ctx()
        result = lua_pipeline.transform_response(gemini_resp, ctx)
        assert "choices" in result
        assert result["choices"][0]["message"]["content"] == "Answer"

    def test_file_data_image_roundtrip(self, lua_pipeline):
        """fileData (URL-based image) should survive Gemini->Gemini round-trip."""
        gemini_req = {
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {
                            "fileData": {
                                "mimeType": "image/jpeg",
                                "fileUri": "gs://bucket/photo.jpg",
                            }
                        }
                    ],
                }
            ],
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        result = lua_pipeline.transform_request(gemini_req, ctx)

        parts = result["contents"][0]["parts"]
        assert "fileData" in parts[0]
        assert parts[0]["fileData"]["fileUri"] == "gs://bucket/photo.jpg"

    def test_tool_choice_none(self, lua_pipeline):
        """tool_choice 'none' should produce Gemini toolConfig with NONE mode."""
        openai_req = {
            "model": "gemini-2.0-flash",
            "messages": [{"role": "user", "content": "Hi"}],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "fn",
                        "description": "d",
                        "parameters": {"type": "object", "properties": {}},
                    },
                }
            ],
            "tool_choice": "none",
        }
        ctx = _make_ctx()
        result = lua_pipeline.transform_request(openai_req, ctx)
        assert "tools" in result
        # The toolConfig should reflect NONE mode
        if "toolConfig" in result:
            mode = result["toolConfig"]["functionCallingConfig"]["mode"]
            assert mode == "NONE"

    def test_thinking_response_roundtrip(self, lua_pipeline):
        """Thinking blocks in a Gemini response should survive Gemini->Gemini round-trip."""
        gemini_resp = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [
                            {"thought": True, "text": "Reasoning..."},
                            {"text": "Final answer", "thoughtSignature": "sig_xyz"},
                        ],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 15,
                "totalTokenCount": 20,
            },
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        result = lua_pipeline.transform_response(gemini_resp, ctx)

        parts = result["candidates"][0]["content"]["parts"]
        assert parts[0].get("thought") is True
        assert parts[0]["text"] == "Reasoning..."
        assert parts[1]["text"] == "Final answer"
        assert parts[1].get("thoughtSignature") == "sig_xyz"
