"""Validate the Lua Gemini transformer against the hardcoded GeminiTransformer.

Run:  cd python-server && python -m pytest tests/test_lua_gemini_transformer.py -v
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
    assert engine.has_script(PROVIDER_NAME)
    assert engine.has_transform_hooks(PROVIDER_NAME)

    registry = TransformerRegistry()
    registry.register(OpenAITransformer())
    registry.register(AnthropicTransformer())
    registry.register(GeminiTransformer())
    registry.register(ResponseApiTransformer())

    pipeline = TransformPipeline(registry, lua_engine=engine)
    return pipeline


@pytest.fixture()
def hardcoded_pipeline():
    """Pipeline without Lua (hardcoded only)."""
    registry = TransformerRegistry()
    registry.register(OpenAITransformer())
    registry.register(AnthropicTransformer())
    registry.register(GeminiTransformer())
    registry.register(ResponseApiTransformer())
    return TransformPipeline(registry)


def _make_ctx(
    client_proto: Protocol = Protocol.OPENAI,
    provider_proto: Protocol = Protocol.GEMINI,
    model: str = "gemini-2.0-flash",
):
    return TransformContext(
        client_protocol=client_proto,
        provider_protocol=provider_proto,
        original_model=model,
        mapped_model=model,
        provider_name=PROVIDER_NAME,
    )


# =========================================================================
# Test: simple text request  OpenAI → Gemini
# =========================================================================


class TestRequestTransform:
    """Test OpenAI → UIF → Gemini request transformation."""

    def test_simple_text(self, lua_pipeline, hardcoded_pipeline):
        openai_req = {
            "model": "gemini-2.0-flash",
            "messages": [
                {"role": "system", "content": "You are helpful."},
                {"role": "user", "content": "Hello"},
            ],
            "temperature": 0.7,
            "max_tokens": 100,
        }
        ctx = _make_ctx()

        lua_result = lua_pipeline.transform_request(openai_req, ctx)
        hardcoded_result = hardcoded_pipeline.transform_request(openai_req, ctx)

        # Both should produce Gemini format
        assert "contents" in lua_result
        assert "contents" in hardcoded_result

        # System instruction
        assert "systemInstruction" in lua_result
        assert "systemInstruction" in hardcoded_result

        # Generation config
        assert lua_result.get("generationConfig", {}).get("temperature") == 0.7
        assert lua_result.get("generationConfig", {}).get("maxOutputTokens") == 100

        # Message content should match
        lua_parts = lua_result["contents"][0]["parts"]
        hc_parts = hardcoded_result["contents"][0]["parts"]
        assert lua_parts[0]["text"] == hc_parts[0]["text"]

    def test_with_tools(self, lua_pipeline, hardcoded_pipeline):
        openai_req = {
            "model": "gemini-2.0-flash",
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather for a city",
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

        lua_result = lua_pipeline.transform_request(openai_req, ctx)
        hardcoded_result = hardcoded_pipeline.transform_request(openai_req, ctx)

        # Both should have tools
        assert "tools" in lua_result
        assert "tools" in hardcoded_result

        lua_decls = lua_result["tools"][0]["functionDeclarations"]
        hc_decls = hardcoded_result["tools"][0]["functionDeclarations"]
        assert lua_decls[0]["name"] == hc_decls[0]["name"]
        assert lua_decls[0]["name"] == "get_weather"

    def test_multi_turn(self, lua_pipeline, hardcoded_pipeline):
        openai_req = {
            "model": "gemini-2.0-flash",
            "messages": [
                {"role": "user", "content": "Hi"},
                {"role": "assistant", "content": "Hello! How can I help?"},
                {"role": "user", "content": "Tell me a joke"},
            ],
        }
        ctx = _make_ctx()

        lua_result = lua_pipeline.transform_request(openai_req, ctx)
        hardcoded_result = hardcoded_pipeline.transform_request(openai_req, ctx)

        # Both should have 3 content entries
        assert len(lua_result["contents"]) == len(hardcoded_result["contents"])
        # Roles should match
        for lc, hc in zip(lua_result["contents"], hardcoded_result["contents"]):
            assert lc["role"] == hc["role"]


# =========================================================================
# Test: Gemini → UIF → Gemini (round-trip within Gemini protocol)
# =========================================================================


class TestGeminiRoundTrip:
    """Test Gemini → UIF → Gemini via Lua (client=gemini, provider=gemini)."""

    def test_text_roundtrip(self, lua_pipeline):
        gemini_req = {
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "Hello, world!"}],
                }
            ],
            "systemInstruction": {"parts": [{"text": "Be helpful."}]},
            "generationConfig": {"temperature": 0.5, "maxOutputTokens": 200},
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)

        result = lua_pipeline.transform_request(gemini_req, ctx)

        assert result["contents"][0]["parts"][0]["text"] == "Hello, world!"
        assert result["systemInstruction"]["parts"][0]["text"] == "Be helpful."
        assert result["generationConfig"]["temperature"] == 0.5
        assert result["generationConfig"]["maxOutputTokens"] == 200

    def test_tool_call_roundtrip(self, lua_pipeline):
        gemini_req = {
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "What's the weather in SF?"}],
                },
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

        # Should have 3 content entries
        assert len(result["contents"]) == 3

        # Model message should have functionCall
        model_msg = result["contents"][1]
        assert model_msg["role"] == "model"
        assert "functionCall" in model_msg["parts"][0]
        assert model_msg["parts"][0]["functionCall"]["name"] == "get_weather"

        # User message should have functionResponse
        user_msg = result["contents"][2]
        assert user_msg["role"] == "user"
        assert "functionResponse" in user_msg["parts"][0]

        # Tools should be preserved
        assert "tools" in result
        assert result["tools"][0]["functionDeclarations"][0]["name"] == "get_weather"


# =========================================================================
# Test: Response transformation
# =========================================================================


class TestResponseTransform:
    """Test response transformation Gemini → UIF → OpenAI."""

    def test_simple_response(self, lua_pipeline, hardcoded_pipeline):
        gemini_response = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [{"text": "Hello! I'm Gemini."}],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 20,
                "totalTokenCount": 30,
            },
            "modelVersion": "gemini-2.0-flash",
        }
        ctx = _make_ctx()

        lua_result = lua_pipeline.transform_response(gemini_response, ctx)
        hardcoded_result = hardcoded_pipeline.transform_response(gemini_response, ctx)

        # Both should produce OpenAI format
        assert "choices" in lua_result
        assert "choices" in hardcoded_result

        # Text content should match
        lua_text = lua_result["choices"][0]["message"]["content"]
        hc_text = hardcoded_result["choices"][0]["message"]["content"]
        assert lua_text == hc_text == "Hello! I'm Gemini."

        # Usage should match
        assert lua_result["usage"]["prompt_tokens"] == 10
        assert lua_result["usage"]["completion_tokens"] == 20

    def test_tool_call_response(self, lua_pipeline):
        gemini_response = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [
                            {
                                "functionCall": {
                                    "name": "get_weather",
                                    "args": {"city": "Tokyo"},
                                }
                            }
                        ],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 15,
                "candidatesTokenCount": 10,
                "totalTokenCount": 25,
            },
        }
        ctx = _make_ctx()

        result = lua_pipeline.transform_response(gemini_response, ctx)

        # Should have tool calls in OpenAI format
        choices = result["choices"]
        assert len(choices) == 1
        msg = choices[0]["message"]
        assert "tool_calls" in msg
        assert msg["tool_calls"][0]["function"]["name"] == "get_weather"

    def test_gemini_to_gemini_response(self, lua_pipeline):
        """Test Gemini → UIF → Gemini response (same protocol)."""
        gemini_response = {
            "candidates": [
                {
                    "content": {
                        "role": "model",
                        "parts": [{"text": "Test response"}],
                    },
                    "finishReason": "STOP",
                }
            ],
            "usageMetadata": {
                "promptTokenCount": 5,
                "candidatesTokenCount": 10,
                "totalTokenCount": 15,
            },
            "modelVersion": "gemini-2.0-flash",
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)

        result = lua_pipeline.transform_response(gemini_response, ctx)

        # Should stay in Gemini format
        assert "candidates" in result
        parts = result["candidates"][0]["content"]["parts"]
        assert parts[0]["text"] == "Test response"
        assert result["candidates"][0]["finishReason"] == "STOP"


# =========================================================================
# Test: Bypass disabled when Lua transform hooks exist
# =========================================================================


class TestBypass:
    def test_bypass_disabled_with_lua_hooks(self, lua_pipeline):
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        # Even same protocol, bypass should be disabled because Lua hooks exist
        assert not lua_pipeline.should_bypass(ctx)

    def test_bypass_enabled_without_lua(self, hardcoded_pipeline):
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)
        # No Lua → bypass should be enabled for same protocol
        assert hardcoded_pipeline.should_bypass(ctx)


# =========================================================================
# Test: Thinking / reasoning blocks
# =========================================================================


class TestThinkingBlocks:
    def test_thinking_roundtrip(self, lua_pipeline):
        gemini_req = {
            "contents": [
                {
                    "role": "user",
                    "parts": [{"text": "Think step by step"}],
                },
                {
                    "role": "model",
                    "parts": [
                        {"thought": True, "text": "Let me think..."},
                        {"text": "Here's my answer", "thoughtSignature": "sig_abc"},
                    ],
                },
            ],
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)

        result = lua_pipeline.transform_request(gemini_req, ctx)

        model_parts = result["contents"][1]["parts"]
        # First part: thinking
        assert model_parts[0].get("thought") is True
        assert model_parts[0]["text"] == "Let me think..."
        # Second part: text with signature re-attached
        assert model_parts[1]["text"] == "Here's my answer"
        assert model_parts[1].get("thoughtSignature") == "sig_abc"

    def test_thinking_response(self, lua_pipeline):
        gemini_response = {
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

        result = lua_pipeline.transform_response(gemini_response, ctx)

        parts = result["candidates"][0]["content"]["parts"]
        assert parts[0].get("thought") is True
        assert parts[0]["text"] == "Reasoning..."
        assert parts[1]["text"] == "Final answer"
        assert parts[1].get("thoughtSignature") == "sig_xyz"


# =========================================================================
# Test: Image content
# =========================================================================


class TestImageContent:
    def test_inline_image_roundtrip(self, lua_pipeline):
        gemini_req = {
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {"text": "What's in this image?"},
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
        assert parts[0]["text"] == "What's in this image?"
        assert "inlineData" in parts[1]
        assert parts[1]["inlineData"]["mimeType"] == "image/png"
        assert parts[1]["inlineData"]["data"] == "iVBORw0KGgo="

    def test_file_data_roundtrip(self, lua_pipeline):
        gemini_req = {
            "contents": [
                {
                    "role": "user",
                    "parts": [
                        {
                            "fileData": {
                                "mimeType": "image/jpeg",
                                "fileUri": "gs://bucket/image.jpg",
                            }
                        },
                    ],
                }
            ],
        }
        ctx = _make_ctx(client_proto=Protocol.GEMINI, provider_proto=Protocol.GEMINI)

        result = lua_pipeline.transform_request(gemini_req, ctx)

        parts = result["contents"][0]["parts"]
        assert "fileData" in parts[0]
        assert parts[0]["fileData"]["fileUri"] == "gs://bucket/image.jpg"


# =========================================================================
# Test: Script validation
# =========================================================================


class TestScriptValidation:
    def test_script_compiles(self):
        from app.scripting.sandbox import validate_script

        with open(SCRIPT_PATH) as f:
            source = f.read()

        error = validate_script(source)
        assert error is None, f"Script validation failed: {error}"

    def test_hooks_detected(self):
        from app.scripting.sandbox import create_sandboxed_lua, parse_hooks

        with open(SCRIPT_PATH) as f:
            source = f.read()

        lua = create_sandboxed_lua()
        lua.execute(source)
        hooks = parse_hooks(lua)

        assert hooks.on_transform_request_out
        assert hooks.on_transform_request_in
        assert hooks.on_transform_response_in
        assert hooks.on_transform_response_out
        assert hooks.has_transform_hooks()
        # Old hooks should NOT be defined
        assert not hooks.on_request
        assert not hooks.on_response
        assert not hooks.on_stream_chunk
