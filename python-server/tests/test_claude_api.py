"""Tests for Claude API implementation."""

import json
from pathlib import Path

import pytest
import httpx
import respx
from pydantic import ValidationError

from app.core.claude_constants import ClaudeConstants
from app.models.claude import (
    ClaudeMessagesRequest,
    ClaudeMessage,
    ClaudeContentBlockText,
    ClaudeContentBlockImage,
    ClaudeContentBlockToolUse,
    ClaudeContentBlockToolResult,
    ClaudeImageSource,
    ClaudeTool,
    ClaudeResponse,
    ClaudeUsage,
    ClaudeTokenCountRequest,
    ClaudeTokenCountResponse,
)
from app.services.claude_converter import (
    claude_to_openai_request,
    openai_to_claude_response,
    convert_claude_message_to_openai,
    convert_openai_tool_calls_to_claude,
    convert_openai_streaming_to_claude,
)


@pytest.mark.unit
class TestClaudeConstants:
    """Test Claude constants are defined correctly."""

    def test_role_constants(self):
        """Test role constants are defined."""
        assert ClaudeConstants.ROLE_USER == "user"
        assert ClaudeConstants.ROLE_ASSISTANT == "assistant"
        assert ClaudeConstants.ROLE_SYSTEM == "system"
        assert ClaudeConstants.ROLE_TOOL == "tool"

    def test_content_type_constants(self):
        """Test content type constants are defined."""
        assert ClaudeConstants.CONTENT_TEXT == "text"
        assert ClaudeConstants.CONTENT_IMAGE == "image"
        assert ClaudeConstants.CONTENT_TOOL_USE == "tool_use"
        assert ClaudeConstants.CONTENT_TOOL_RESULT == "tool_result"

    def test_tool_type_constants(self):
        """Test tool type constants are defined."""
        assert ClaudeConstants.TOOL_FUNCTION == "function"

    def test_stop_reason_constants(self):
        """Test stop reason constants are defined."""
        assert ClaudeConstants.STOP_END_TURN == "end_turn"
        assert ClaudeConstants.STOP_MAX_TOKENS == "max_tokens"
        assert ClaudeConstants.STOP_STOP_SEQUENCE == "stop_sequence"
        assert ClaudeConstants.STOP_TOOL_USE == "tool_use"
        assert ClaudeConstants.STOP_ERROR == "error"

    def test_sse_event_constants(self):
        """Test SSE event type constants are defined."""
        assert ClaudeConstants.EVENT_MESSAGE_START == "message_start"
        assert ClaudeConstants.EVENT_MESSAGE_STOP == "message_stop"
        assert ClaudeConstants.EVENT_MESSAGE_DELTA == "message_delta"
        assert ClaudeConstants.EVENT_CONTENT_BLOCK_START == "content_block_start"
        assert ClaudeConstants.EVENT_CONTENT_BLOCK_STOP == "content_block_stop"
        assert ClaudeConstants.EVENT_CONTENT_BLOCK_DELTA == "content_block_delta"
        assert ClaudeConstants.EVENT_PING == "ping"

    def test_delta_type_constants(self):
        """Test delta type constants are defined."""
        assert ClaudeConstants.DELTA_TEXT == "text_delta"
        assert ClaudeConstants.DELTA_INPUT_JSON == "input_json_delta"


@pytest.mark.unit
class TestClaudeModels:
    """Test Claude Pydantic models."""

    def test_claude_messages_request_validation(self):
        """Test ClaudeMessagesRequest validation."""
        request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
        )
        assert request.model == "claude-3-opus-20240229"
        assert request.max_tokens == 1024
        assert len(request.messages) == 1
        assert request.stream is False

    def test_claude_messages_request_with_system(self):
        """Test ClaudeMessagesRequest with system message."""
        request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
            system="You are a helpful assistant.",
        )
        assert request.system == "You are a helpful assistant."

    def test_claude_messages_request_with_tools(self):
        """Test ClaudeMessagesRequest with tools."""
        request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="What's the weather?")],
            tools=[
                ClaudeTool(
                    name="get_weather",
                    description="Get weather for a location",
                    input_schema={
                        "type": "object",
                        "properties": {"location": {"type": "string"}},
                        "required": ["location"],
                    },
                )
            ],
        )
        assert len(request.tools) == 1
        assert request.tools[0].name == "get_weather"

    def test_claude_messages_request_temperature_validation(self):
        """Test temperature validation (0.0 to 1.0)."""
        # Valid temperature
        request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
            temperature=0.7,
        )
        assert request.temperature == 0.7

        # Invalid temperature (too high)
        with pytest.raises(ValidationError):
            ClaudeMessagesRequest(
                model="claude-3-opus-20240229",
                max_tokens=1024,
                messages=[ClaudeMessage(role="user", content="Hello!")],
                temperature=1.5,
            )

    def test_claude_response_serialization(self):
        """Test ClaudeResponse serialization."""
        response = ClaudeResponse(
            id="msg_123",
            model="claude-3-opus-20240229",
            content=[ClaudeContentBlockText(text="Hello!")],
            stop_reason="end_turn",
            usage=ClaudeUsage(input_tokens=10, output_tokens=5),
        )
        data = response.model_dump()
        assert data["id"] == "msg_123"
        assert data["type"] == "message"
        assert data["role"] == "assistant"
        assert data["model"] == "claude-3-opus-20240229"
        assert len(data["content"]) == 1
        assert data["content"][0]["type"] == "text"
        assert data["content"][0]["text"] == "Hello!"

    def test_content_block_text(self):
        """Test ClaudeContentBlockText model."""
        block = ClaudeContentBlockText(text="Hello world")
        assert block.type == "text"
        assert block.text == "Hello world"

    def test_content_block_image(self):
        """Test ClaudeContentBlockImage model."""
        block = ClaudeContentBlockImage(
            source=ClaudeImageSource(
                media_type="image/png",
                data="base64encodeddata",
            )
        )
        assert block.type == "image"
        assert block.source.type == "base64"
        assert block.source.media_type == "image/png"

    def test_content_block_tool_use(self):
        """Test ClaudeContentBlockToolUse model."""
        block = ClaudeContentBlockToolUse(
            id="tool_123",
            name="get_weather",
            input={"location": "San Francisco"},
        )
        assert block.type == "tool_use"
        assert block.id == "tool_123"
        assert block.name == "get_weather"
        assert block.input == {"location": "San Francisco"}

    def test_content_block_tool_result(self):
        """Test ClaudeContentBlockToolResult model."""
        block = ClaudeContentBlockToolResult(
            tool_use_id="tool_123",
            content="The weather is sunny.",
        )
        assert block.type == "tool_result"
        assert block.tool_use_id == "tool_123"
        assert block.content == "The weather is sunny."


@pytest.mark.unit
class TestClaudeToOpenAIConverter:
    """Test Claude to OpenAI request conversion."""

    def test_simple_message_conversion(self):
        """Test converting simple Claude request to OpenAI format."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="Hello!"),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        assert openai_request["model"] == "claude-3-opus-20240229"
        assert openai_request["max_tokens"] == 1024
        assert len(openai_request["messages"]) == 1
        assert openai_request["messages"][0]["role"] == "user"
        assert openai_request["messages"][0]["content"] == "Hello!"

    def test_system_message_conversion(self):
        """Test converting Claude request with system message."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="Hello!"),
            ],
            system="You are a helpful assistant.",
        )

        openai_request = claude_to_openai_request(claude_request)

        assert len(openai_request["messages"]) == 2
        assert openai_request["messages"][0]["role"] == "system"
        assert (
            openai_request["messages"][0]["content"] == "You are a helpful assistant."
        )
        assert openai_request["messages"][1]["role"] == "user"

    def test_image_content_conversion(self):
        """Test converting Claude request with image content."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockText(text="What's in this image?"),
                        ClaudeContentBlockImage(
                            source=ClaudeImageSource(
                                media_type="image/png",
                                data="base64data",
                            )
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        assert len(openai_request["messages"]) == 1
        content = openai_request["messages"][0]["content"]
        assert isinstance(content, list)
        assert len(content) == 2
        assert content[0]["type"] == "text"
        assert content[1]["type"] == "image_url"
        assert "data:image/png;base64,base64data" in content[1]["image_url"]["url"]

    def test_tools_conversion(self):
        """Test converting Claude request with tools."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="What's the weather?"),
            ],
            tools=[
                ClaudeTool(
                    name="get_weather",
                    description="Get weather for a location",
                    input_schema={
                        "type": "object",
                        "properties": {"location": {"type": "string"}},
                        "required": ["location"],
                    },
                )
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        assert "tools" in openai_request
        assert len(openai_request["tools"]) == 1
        tool = openai_request["tools"][0]
        assert tool["type"] == "function"
        assert tool["function"]["name"] == "get_weather"
        assert tool["function"]["description"] == "Get weather for a location"

    def test_model_mapping(self):
        """Test model name mapping during conversion."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
        )

        model_mapping = {"claude-3-opus": "gpt-4-turbo"}
        openai_request = claude_to_openai_request(claude_request, model_mapping)

        assert openai_request["model"] == "gpt-4-turbo"

    def test_tool_use_message_conversion(self):
        """Test converting assistant message with tool use."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="What's the weather?"),
                ClaudeMessage(
                    role="assistant",
                    content=[
                        ClaudeContentBlockText(text="Let me check the weather."),
                        ClaudeContentBlockToolUse(
                            id="tool_123",
                            name="get_weather",
                            input={"location": "San Francisco"},
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        assert len(openai_request["messages"]) == 2
        assistant_msg = openai_request["messages"][1]
        assert assistant_msg["role"] == "assistant"
        assert "tool_calls" in assistant_msg
        assert len(assistant_msg["tool_calls"]) == 1
        assert assistant_msg["tool_calls"][0]["id"] == "tool_123"

    def test_tool_result_message_conversion(self):
        """Test converting tool result messages."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="What's the weather?"),
                ClaudeMessage(
                    role="assistant",
                    content=[
                        ClaudeContentBlockToolUse(
                            id="tool_123",
                            name="get_weather",
                            input={"location": "San Francisco"},
                        ),
                    ],
                ),
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockToolResult(
                            tool_use_id="tool_123",
                            content="Sunny, 72°F",
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        # Should have: user, assistant with tool_calls, tool result
        assert len(openai_request["messages"]) == 3
        tool_msg = openai_request["messages"][2]
        assert tool_msg["role"] == "tool"
        assert tool_msg["tool_call_id"] == "tool_123"
        assert tool_msg["content"] == "Sunny, 72°F"


@pytest.mark.unit
class TestOpenAIToClaudeConverter:
    """Test OpenAI to Claude response conversion."""

    def test_text_response_conversion(self):
        """Test converting OpenAI text response to Claude format."""
        openai_response = {
            "id": "chatcmpl-123",
            "model": "gpt-4-turbo",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hello!"},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15},
        }

        claude_response = openai_to_claude_response(openai_response, "claude-3-opus")

        assert claude_response["id"] == "chatcmpl-123"
        assert claude_response["type"] == "message"
        assert claude_response["role"] == "assistant"
        assert claude_response["model"] == "claude-3-opus"
        assert len(claude_response["content"]) == 1
        assert claude_response["content"][0]["type"] == "text"
        assert claude_response["content"][0]["text"] == "Hello!"
        assert claude_response["stop_reason"] == "end_turn"
        assert claude_response["usage"]["input_tokens"] == 10
        assert claude_response["usage"]["output_tokens"] == 5

    def test_tool_calls_response_conversion(self):
        """Test converting OpenAI response with tool calls to Claude format."""
        openai_response = {
            "id": "chatcmpl-123",
            "model": "gpt-4-turbo",
            "choices": [
                {
                    "message": {
                        "role": "assistant",
                        "content": None,
                        "tool_calls": [
                            {
                                "id": "call_123",
                                "type": "function",
                                "function": {
                                    "name": "get_weather",
                                    "arguments": '{"location": "San Francisco"}',
                                },
                            }
                        ],
                    },
                    "finish_reason": "tool_calls",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30},
        }

        claude_response = openai_to_claude_response(openai_response, "claude-3-opus")

        assert claude_response["stop_reason"] == "tool_use"
        # Should have tool_use content block
        tool_blocks = [b for b in claude_response["content"] if b["type"] == "tool_use"]
        assert len(tool_blocks) == 1
        assert tool_blocks[0]["id"] == "call_123"
        assert tool_blocks[0]["name"] == "get_weather"
        assert tool_blocks[0]["input"] == {"location": "San Francisco"}

    def test_finish_reason_mapping(self):
        """Test finish reason to stop reason mapping."""
        test_cases = [
            ("stop", "end_turn"),
            ("length", "max_tokens"),
            ("tool_calls", "tool_use"),
            ("function_call", "tool_use"),
        ]

        for finish_reason, expected_stop_reason in test_cases:
            openai_response = {
                "id": "chatcmpl-123",
                "model": "gpt-4",
                "choices": [
                    {
                        "message": {"role": "assistant", "content": "Test"},
                        "finish_reason": finish_reason,
                    }
                ],
                "usage": {
                    "prompt_tokens": 1,
                    "completion_tokens": 1,
                    "total_tokens": 2,
                },
            }

            claude_response = openai_to_claude_response(openai_response, "claude-3")
            assert claude_response["stop_reason"] == expected_stop_reason

    def test_empty_choices_raises_error(self):
        """Test that empty choices raises ValueError."""
        openai_response = {
            "id": "chatcmpl-123",
            "model": "gpt-4",
            "choices": [],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2},
        }

        with pytest.raises(ValueError, match="No choices"):
            openai_to_claude_response(openai_response, "claude-3")


@pytest.mark.unit
class TestConvertClaudeMessageToOpenAI:
    """Test convert_claude_message_to_openai function."""

    def test_user_message_string_content(self):
        """Test converting user message with string content."""
        message = {"role": "user", "content": "Hello!"}
        result = convert_claude_message_to_openai(message)

        assert result["role"] == "user"
        assert result["content"] == "Hello!"

    def test_user_message_list_content(self):
        """Test converting user message with list content."""
        message = {
            "role": "user",
            "content": [
                {"type": "text", "text": "What's in this image?"},
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "base64data",
                    },
                },
            ],
        }
        result = convert_claude_message_to_openai(message)

        assert result["role"] == "user"
        assert isinstance(result["content"], list)
        assert len(result["content"]) == 2
        assert result["content"][0]["type"] == "text"
        assert result["content"][1]["type"] == "image_url"

    def test_assistant_message_string_content(self):
        """Test converting assistant message with string content."""
        message = {"role": "assistant", "content": "Hello!"}
        result = convert_claude_message_to_openai(message)

        assert result["role"] == "assistant"
        assert result["content"] == "Hello!"

    def test_assistant_message_with_tool_use(self):
        """Test converting assistant message with tool use."""
        message = {
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check."},
                {
                    "type": "tool_use",
                    "id": "tool_123",
                    "name": "get_weather",
                    "input": {"location": "SF"},
                },
            ],
        }
        result = convert_claude_message_to_openai(message)

        assert result["role"] == "assistant"
        assert result["content"] == "Let me check."
        assert "tool_calls" in result
        assert len(result["tool_calls"]) == 1
        assert result["tool_calls"][0]["id"] == "tool_123"

    def test_user_message_none_content(self):
        """Test converting user message with None content."""
        message = {"role": "user", "content": None}
        result = convert_claude_message_to_openai(message)

        assert result["role"] == "user"
        assert result["content"] == ""

    def test_assistant_message_none_content(self):
        """Test converting assistant message with None content."""
        message = {"role": "assistant", "content": None}
        result = convert_claude_message_to_openai(message)

        assert result["role"] == "assistant"
        assert result["content"] is None


@pytest.mark.unit
class TestConvertOpenAIToolCallsToClaude:
    """Test convert_openai_tool_calls_to_claude function."""

    def test_single_tool_call(self):
        """Test converting single tool call."""
        tool_calls = [
            {
                "id": "call_123",
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "arguments": '{"location": "San Francisco"}',
                },
            }
        ]

        result = convert_openai_tool_calls_to_claude(tool_calls)

        assert len(result) == 1
        assert result[0]["type"] == "tool_use"
        assert result[0]["id"] == "call_123"
        assert result[0]["name"] == "get_weather"
        assert result[0]["input"] == {"location": "San Francisco"}

    def test_multiple_tool_calls(self):
        """Test converting multiple tool calls."""
        tool_calls = [
            {
                "id": "call_1",
                "type": "function",
                "function": {"name": "tool1", "arguments": '{"a": 1}'},
            },
            {
                "id": "call_2",
                "type": "function",
                "function": {"name": "tool2", "arguments": '{"b": 2}'},
            },
        ]

        result = convert_openai_tool_calls_to_claude(tool_calls)

        assert len(result) == 2
        assert result[0]["name"] == "tool1"
        assert result[1]["name"] == "tool2"

    def test_invalid_json_arguments(self):
        """Test handling invalid JSON in arguments."""
        tool_calls = [
            {
                "id": "call_123",
                "type": "function",
                "function": {"name": "test", "arguments": "invalid json"},
            }
        ]

        result = convert_openai_tool_calls_to_claude(tool_calls)

        assert len(result) == 1
        assert result[0]["input"] == {"raw_arguments": "invalid json"}


@pytest.mark.unit
class TestClaudeEndpoint:
    """Test Claude API endpoint."""

    @respx.mock
    def test_messages_endpoint_success(self, app_client):
        """Test /v1/messages endpoint returns correct format."""
        # Mock OpenAI provider response
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "chatcmpl-123",
                    "model": "gpt-4-0613",
                    "choices": [
                        {
                            "message": {"role": "assistant", "content": "Hello!"},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {
                        "prompt_tokens": 10,
                        "completion_tokens": 5,
                        "total_tokens": 15,
                    },
                },
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                json={
                    "id": "chatcmpl-123",
                    "model": "gpt-4-1106-preview",
                    "choices": [
                        {
                            "message": {"role": "assistant", "content": "Hello!"},
                            "finish_reason": "stop",
                        }
                    ],
                    "usage": {
                        "prompt_tokens": 10,
                        "completion_tokens": 5,
                        "total_tokens": 15,
                    },
                },
            )
        )

        response = app_client.post(
            "/v1/messages",
            json={
                "model": "gpt-4",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "Hello!"}],
            },
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 200
        data = response.json()
        assert data["type"] == "message"
        assert data["role"] == "assistant"
        assert data["model"] == "gpt-4"
        assert len(data["content"]) >= 1
        assert data["content"][0]["type"] == "text"
        assert "usage" in data

    @respx.mock
    def test_messages_endpoint_unauthorized(self, app_client):
        """Test /v1/messages endpoint requires authentication."""
        response = app_client.post(
            "/v1/messages",
            json={
                "model": "gpt-4",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "Hello!"}],
            },
        )

        assert response.status_code == 401

    @respx.mock
    def test_messages_endpoint_provider_error(self, app_client):
        """Test /v1/messages endpoint handles provider errors."""
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                500,
                json={"error": {"message": "Internal server error"}},
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                500,
                json={"error": {"message": "Internal server error"}},
            )
        )

        response = app_client.post(
            "/v1/messages",
            json={
                "model": "gpt-4",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "Hello!"}],
            },
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 500
        data = response.json()
        assert data["type"] == "error"
        assert "error" in data

    @respx.mock
    def test_messages_endpoint_streaming(self, app_client):
        """Test /v1/messages endpoint with streaming."""
        streaming_content = (
            b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":"Hi"}}]}\n\n'
            b"data: [DONE]\n\n"
        )
        respx.post("https://api.provider1.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                content=streaming_content,
                headers={"content-type": "text/event-stream"},
            )
        )
        respx.post("https://api.provider2.com/v1/chat/completions").mock(
            return_value=httpx.Response(
                200,
                content=streaming_content,
                headers={"content-type": "text/event-stream"},
            )
        )

        response = app_client.post(
            "/v1/messages",
            json={
                "model": "gpt-4",
                "max_tokens": 1024,
                "messages": [{"role": "user", "content": "Hello!"}],
                "stream": True,
            },
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 200
        assert "text/event-stream" in response.headers.get("content-type", "")

    @respx.mock
    def test_count_tokens_endpoint(self, app_client):
        """Test /v1/messages/count_tokens endpoint."""
        response = app_client.post(
            "/v1/messages/count_tokens",
            json={
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hello world!"}],
            },
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 200
        data = response.json()
        assert "input_tokens" in data
        assert data["input_tokens"] > 0

    @respx.mock
    def test_count_tokens_with_system(self, app_client):
        """Test /v1/messages/count_tokens with system message."""
        response = app_client.post(
            "/v1/messages/count_tokens",
            json={
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hello!"}],
                "system": "You are a helpful assistant.",
            },
            headers={"Authorization": "Bearer test-credential-key"},
        )

        assert response.status_code == 200
        data = response.json()
        assert "input_tokens" in data
        # Should include system message tokens
        assert data["input_tokens"] > 1


@pytest.mark.unit
class TestClaudeTokenCountRequest:
    """Test ClaudeTokenCountRequest model."""

    def test_basic_request(self):
        """Test basic token count request."""
        request = ClaudeTokenCountRequest(
            model="claude-3-opus-20240229",
            messages=[ClaudeMessage(role="user", content="Hello!")],
        )
        assert request.model == "claude-3-opus-20240229"
        assert len(request.messages) == 1

    def test_request_with_system(self):
        """Test token count request with system message."""
        request = ClaudeTokenCountRequest(
            model="claude-3-opus-20240229",
            messages=[ClaudeMessage(role="user", content="Hello!")],
            system="You are helpful.",
        )
        assert request.system == "You are helpful."

    def test_request_with_tools(self):
        """Test token count request with tools."""
        request = ClaudeTokenCountRequest(
            model="claude-3-opus-20240229",
            messages=[ClaudeMessage(role="user", content="Hello!")],
            tools=[
                ClaudeTool(
                    name="test_tool",
                    input_schema={"type": "object"},
                )
            ],
        )
        assert len(request.tools) == 1


@pytest.mark.unit
class TestClaudeTokenCountResponse:
    """Test ClaudeTokenCountResponse model."""

    def test_response(self):
        """Test token count response."""
        response = ClaudeTokenCountResponse(input_tokens=42)
        assert response.input_tokens == 42


@pytest.mark.unit
class TestStreamingTermination:
    """Test cases for streaming termination to prevent infinite loops."""

    @pytest.mark.asyncio
    async def test_stream_terminates_on_empty_chunk(self):
        """Verify stream terminates when empty chunk is received.

        This tests the fix for the infinite loop bug where empty chunks
        from httpx stream caused the converter to loop indefinitely.
        """

        async def mock_stream():
            """Mock stream that yields data then empty bytes."""
            # Yield a valid chunk first
            yield b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":"Hi"}}]}\n\n'
            # Yield empty bytes - this should terminate the stream
            yield b""
            # This should never be reached due to empty chunk termination
            yield b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":" there"}}]}\n\n'

        events = []
        async for event in convert_openai_streaming_to_claude(
            mock_stream(), "claude-3-opus"
        ):
            events.append(event)

        # Verify stream terminated properly
        # Should have: message_start, content_block_start, ping, text_delta,
        # content_block_stop, message_delta, message_stop
        event_types = [e.split("\n")[0].replace("event: ", "") for e in events]

        # Verify final events are present (stream terminated properly)
        assert "message_stop" in event_types
        assert "message_delta" in event_types
        assert "content_block_stop" in event_types

        # Verify we got the first text delta but not the second (after empty chunk)
        text_deltas = [e for e in events if "text_delta" in e]
        assert len(text_deltas) == 1
        assert "Hi" in text_deltas[0]
        assert " there" not in str(events)

    @pytest.mark.asyncio
    async def test_stream_terminates_on_done_marker(self):
        """Verify stream terminates when [DONE] marker is received."""

        async def mock_stream():
            """Mock stream that yields data then [DONE]."""
            yield b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":"Hello"}}]}\n\n'
            yield b"data: [DONE]\n\n"
            # This should never be reached
            yield b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":" world"}}]}\n\n'

        events = []
        async for event in convert_openai_streaming_to_claude(
            mock_stream(), "claude-3-opus"
        ):
            events.append(event)

        # Verify stream terminated properly with final events
        event_types = [e.split("\n")[0].replace("event: ", "") for e in events]
        assert "message_stop" in event_types

        # Verify we got "Hello" but not " world"
        text_deltas = [e for e in events if "text_delta" in e]
        assert len(text_deltas) == 1
        assert "Hello" in text_deltas[0]
        assert " world" not in str(events)

    @pytest.mark.asyncio
    async def test_final_events_sent_once(self):
        """Verify content_block_stop, message_delta, message_stop sent exactly once."""

        async def mock_stream():
            """Mock stream with multiple chunks then [DONE]."""
            yield b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":"A"}}]}\n\n'
            yield b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":"B"}}]}\n\n'
            yield b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":"C"},"finish_reason":"stop"}]}\n\n'
            yield b"data: [DONE]\n\n"

        events = []
        async for event in convert_openai_streaming_to_claude(
            mock_stream(), "claude-3-opus"
        ):
            events.append(event)

        # Count occurrences of each final event type
        content_block_stop_count = sum(1 for e in events if "content_block_stop" in e)
        message_delta_count = sum(1 for e in events if "message_delta" in e)
        message_stop_count = sum(1 for e in events if "message_stop" in e)

        # Each should appear exactly once
        assert content_block_stop_count == 1, (
            f"content_block_stop appeared {content_block_stop_count} times"
        )
        assert message_delta_count == 1, (
            f"message_delta appeared {message_delta_count} times"
        )
        assert message_stop_count == 1, (
            f"message_stop appeared {message_stop_count} times"
        )

        # Verify order: content_block_stop before message_delta before message_stop
        event_types = [e.split("\n")[0].replace("event: ", "") for e in events]
        stop_idx = event_types.index("content_block_stop")
        delta_idx = event_types.index("message_delta")
        msg_stop_idx = event_types.index("message_stop")

        assert stop_idx < delta_idx < msg_stop_idx, "Final events not in correct order"

    @pytest.mark.asyncio
    async def test_stream_handles_multiple_empty_lines(self):
        """Verify stream handles empty lines (not empty chunks) correctly."""

        async def mock_stream():
            """Mock stream with empty lines between data."""
            yield b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":"Test"}}]}\n\n'
            yield b"\n\n"  # Empty lines (not empty chunk)
            yield b'data: {"id":"chatcmpl-123","model":"gpt-4","choices":[{"delta":{"content":" passed"}}]}\n\n'
            yield b"data: [DONE]\n\n"

        events = []
        async for event in convert_openai_streaming_to_claude(
            mock_stream(), "claude-3-opus"
        ):
            events.append(event)

        # Both text deltas should be present since empty lines are not empty chunks
        text_deltas = [e for e in events if "text_delta" in e]
        assert len(text_deltas) == 2
        assert "Test" in text_deltas[0]
        assert " passed" in text_deltas[1]


@pytest.mark.unit
class TestImageContentConversion:
    """Test image content conversion between Claude and OpenAI formats."""

    def test_image_content_in_user_message(self):
        """Test converting user message with image content to OpenAI format."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockText(text="Describe this image:"),
                        ClaudeContentBlockImage(
                            source=ClaudeImageSource(
                                media_type="image/jpeg",
                                data="SGVsbG8gV29ybGQ=",  # base64 encoded
                            )
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        assert len(openai_request["messages"]) == 1
        content = openai_request["messages"][0]["content"]
        assert isinstance(content, list)
        assert len(content) == 2
        assert content[0]["type"] == "text"
        assert content[0]["text"] == "Describe this image:"
        assert content[1]["type"] == "image_url"
        assert (
            "data:image/jpeg;base64,SGVsbG8gV29ybGQ=" in content[1]["image_url"]["url"]
        )

    def test_multiple_images_in_message(self):
        """Test converting message with multiple images."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockText(text="Compare these images:"),
                        ClaudeContentBlockImage(
                            source=ClaudeImageSource(
                                media_type="image/png",
                                data="aW1hZ2UxZGF0YQ==",
                            )
                        ),
                        ClaudeContentBlockImage(
                            source=ClaudeImageSource(
                                media_type="image/png",
                                data="aW1hZ2UyZGF0YQ==",
                            )
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        content = openai_request["messages"][0]["content"]
        assert len(content) == 3
        assert content[0]["type"] == "text"
        assert content[1]["type"] == "image_url"
        assert content[2]["type"] == "image_url"
        assert "aW1hZ2UxZGF0YQ==" in content[1]["image_url"]["url"]
        assert "aW1hZ2UyZGF0YQ==" in content[2]["image_url"]["url"]


@pytest.mark.unit
class TestSystemPromptAsListOfBlocks:
    """Test system prompt as list of content blocks."""

    def test_system_prompt_as_list_of_text_blocks(self):
        """Test converting system prompt provided as list of text blocks."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
            system=[
                {"type": "text", "text": "You are a helpful assistant."},
                {"type": "text", "text": "Always be polite and concise."},
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        # System message should be first
        assert len(openai_request["messages"]) == 2
        assert openai_request["messages"][0]["role"] == "system"
        # Text blocks should be joined with double newlines
        assert (
            "You are a helpful assistant." in openai_request["messages"][0]["content"]
        )
        assert (
            "Always be polite and concise." in openai_request["messages"][0]["content"]
        )

    def test_system_prompt_as_single_string(self):
        """Test system prompt as single string (baseline)."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
            system="You are a helpful assistant.",
        )

        openai_request = claude_to_openai_request(claude_request)

        assert openai_request["messages"][0]["role"] == "system"
        assert (
            openai_request["messages"][0]["content"] == "You are a helpful assistant."
        )


@pytest.mark.unit
class TestToolResultWithError:
    """Test tool result with is_error=True."""

    def test_tool_result_with_is_error_true(self):
        """Test converting tool result message with is_error=True."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="What's the weather?"),
                ClaudeMessage(
                    role="assistant",
                    content=[
                        ClaudeContentBlockToolUse(
                            id="tool_123",
                            name="get_weather",
                            input={"location": "InvalidCity"},
                        ),
                    ],
                ),
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockToolResult(
                            tool_use_id="tool_123",
                            content="Error: City not found",
                            is_error=True,
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        # Should have: user, assistant with tool_calls, tool result
        assert len(openai_request["messages"]) == 3
        tool_msg = openai_request["messages"][2]
        assert tool_msg["role"] == "tool"
        assert tool_msg["tool_call_id"] == "tool_123"
        assert tool_msg["content"] == "Error: City not found"

    def test_tool_result_with_is_error_false(self):
        """Test converting tool result message with is_error=False (default)."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="What's the weather?"),
                ClaudeMessage(
                    role="assistant",
                    content=[
                        ClaudeContentBlockToolUse(
                            id="tool_456",
                            name="get_weather",
                            input={"location": "San Francisco"},
                        ),
                    ],
                ),
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockToolResult(
                            tool_use_id="tool_456",
                            content="Sunny, 72°F",
                            is_error=False,
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        tool_msg = openai_request["messages"][2]
        assert tool_msg["role"] == "tool"
        assert tool_msg["content"] == "Sunny, 72°F"

    def test_tool_result_without_is_error_field(self):
        """Test converting tool result message without is_error field."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="What's the weather?"),
                ClaudeMessage(
                    role="assistant",
                    content=[
                        ClaudeContentBlockToolUse(
                            id="tool_789",
                            name="get_weather",
                            input={"location": "New York"},
                        ),
                    ],
                ),
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockToolResult(
                            tool_use_id="tool_789",
                            content="Cloudy, 65°F",
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        tool_msg = openai_request["messages"][2]
        assert tool_msg["role"] == "tool"
        assert tool_msg["content"] == "Cloudy, 65°F"


@pytest.mark.unit
class TestBillingHeaderStripping:
    """Test x-anthropic-billing-header prefix stripping from system prompts."""

    def test_strip_billing_header_from_string_system(self):
        """Test stripping billing header from string system prompt."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
            system="x-anthropic-billing-header: cc_version=2.1.17.f12; cc_entrypoint=cli",
        )

        openai_request = claude_to_openai_request(claude_request)

        assert len(openai_request["messages"]) == 2
        assert openai_request["messages"][0]["role"] == "system"
        assert (
            openai_request["messages"][0]["content"]
            == "cc_version=2.1.17.f12; cc_entrypoint=cli"
        )

    def test_strip_billing_header_from_list_system(self):
        """Test stripping billing header from list system prompt."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
            system=[
                {
                    "type": "text",
                    "text": "x-anthropic-billing-header: cc_version=2.1.17.f12; cc_entrypoint=cli",
                },
                {"type": "text", "text": "You are a helpful assistant."},
            ],
        )

        openai_request = claude_to_openai_request(claude_request)

        assert len(openai_request["messages"]) == 2
        assert openai_request["messages"][0]["role"] == "system"
        # First block should have header stripped, second should be unchanged
        content = openai_request["messages"][0]["content"]
        assert "cc_version=2.1.17.f12; cc_entrypoint=cli" in content
        assert "You are a helpful assistant." in content
        assert "x-anthropic-billing-header:" not in content

    def test_no_stripping_when_no_header(self):
        """Test that normal system prompts are not modified."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
            system="You are a helpful assistant.",
        )

        openai_request = claude_to_openai_request(claude_request)

        assert (
            openai_request["messages"][0]["content"] == "You are a helpful assistant."
        )

    def test_strip_billing_header_with_extra_spaces(self):
        """Test stripping billing header with extra spaces after colon."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
            system="x-anthropic-billing-header:   cc_version=2.1.17.f12",
        )

        openai_request = claude_to_openai_request(claude_request)

        assert openai_request["messages"][0]["content"] == "cc_version=2.1.17.f12"

    def test_strip_billing_header_only_at_start(self):
        """Test that billing header is only stripped from the start of text."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello!")],
            system="Some text x-anthropic-billing-header: should not be stripped",
        )

        openai_request = claude_to_openai_request(claude_request)

        # Header in the middle should not be stripped
        assert (
            openai_request["messages"][0]["content"]
            == "Some text x-anthropic-billing-header: should not be stripped"
        )


@pytest.mark.unit
class TestToolResultTextSplitOrdering:
    """Tests for the tool_result + text split ordering fix.

    When a user message contains both tool_result and text blocks, the tool
    messages must be emitted BEFORE the user(text) message to maintain
    assistant(tool_calls) → tool(result) adjacency.
    """

    def test_tool_result_adjacency_with_system_reminder(self):
        """system-reminder text must NOT break tool adjacency."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="search"),
                ClaudeMessage(
                    role="assistant",
                    content=[
                        ClaudeContentBlockToolUse(
                            id="tu_1", name="search", input={"q": "test"}
                        ),
                    ],
                ),
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockToolResult(
                            tool_use_id="tu_1", content="5 results"
                        ),
                        ClaudeContentBlockText(
                            text="<system-reminder>Use TodoWrite</system-reminder>"
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)
        messages = openai_request["messages"]

        # system(optional) + user + assistant(tool_calls) + tool + user(text)
        assert messages[-3]["role"] == "assistant"
        assert "tool_calls" in messages[-3]
        assert messages[-2]["role"] == "tool"
        assert messages[-2]["tool_call_id"] == "tu_1"
        assert messages[-1]["role"] == "user"

    def test_tool_result_adjacency_multiple_tools_with_interrupt(self):
        """Multiple tool_results + user interrupt text."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="do two things"),
                ClaudeMessage(
                    role="assistant",
                    content=[
                        ClaudeContentBlockToolUse(id="tu_a", name="tool_a", input={}),
                        ClaudeContentBlockToolUse(id="tu_b", name="tool_b", input={}),
                    ],
                ),
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockToolResult(
                            tool_use_id="tu_a", content="result_a"
                        ),
                        ClaudeContentBlockToolResult(
                            tool_use_id="tu_b", content="result_b"
                        ),
                        ClaudeContentBlockText(
                            text="[Request interrupted by user for tool use]"
                        ),
                        ClaudeContentBlockText(text="new instruction"),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)
        messages = openai_request["messages"]

        assert messages[-4]["role"] == "assistant"
        assert messages[-3]["role"] == "tool"
        assert messages[-3]["tool_call_id"] == "tu_a"
        assert messages[-2]["role"] == "tool"
        assert messages[-2]["tool_call_id"] == "tu_b"
        assert messages[-1]["role"] == "user"

    def test_tool_result_only_no_text_unchanged(self):
        """Pure tool_result without text — no extra user message emitted."""
        claude_request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[
                ClaudeMessage(role="user", content="read file"),
                ClaudeMessage(
                    role="assistant",
                    content=[
                        ClaudeContentBlockToolUse(id="tu_1", name="Read", input={}),
                    ],
                ),
                ClaudeMessage(
                    role="user",
                    content=[
                        ClaudeContentBlockToolResult(
                            tool_use_id="tu_1", content="file contents"
                        ),
                    ],
                ),
            ],
        )

        openai_request = claude_to_openai_request(claude_request)
        messages = openai_request["messages"]

        assert messages[-2]["role"] == "assistant"
        assert messages[-1]["role"] == "tool"
        assert messages[-1]["tool_call_id"] == "tu_1"


# =============================================================================
# Shared Fixture Tests — cross-language consistency verification (legacy path)
# =============================================================================

_LEGACY_SHARED_FIXTURE_DIR = (
    Path(__file__).resolve().parent.parent.parent
    / "shared-fixtures"
    / "anthropic-to-openai"
)

# Legacy path does not have a standalone content_to_string method
_LEGACY_SKIP = {"08_content_string_variants.json"}

_LEGACY_FIXTURES = sorted(
    f.name
    for f in _LEGACY_SHARED_FIXTURE_DIR.glob("*.json")
    if f.name not in _LEGACY_SKIP and "variants" not in json.loads(f.read_text())
)


@pytest.mark.unit
class TestSharedFixtures:
    """Shared fixture tests for legacy Claude→OpenAI path."""

    @pytest.mark.parametrize("fixture_name", _LEGACY_FIXTURES)
    def test_pipeline(self, fixture_name):
        fixture = json.loads((_LEGACY_SHARED_FIXTURE_DIR / fixture_name).read_text())
        claude_request = ClaudeMessagesRequest(**fixture["input"])
        openai_request = claude_to_openai_request(claude_request)
        messages = openai_request["messages"]

        expected = fixture["expected"]
        if "message_count" in expected:
            assert len(messages) == expected["message_count"], (
                f"fixture {fixture_name}: message_count mismatch"
            )

        for exp_msg in expected.get("messages", []):
            idx = exp_msg["index"]
            msg = messages[idx]
            ctx = f"fixture {fixture_name}, index {idx}"

            if "role" in exp_msg:
                assert msg["role"] == exp_msg["role"], f"{ctx}: role"
            if "content" in exp_msg:
                assert msg["content"] == exp_msg["content"], f"{ctx}: content"
            if "content_contains" in exp_msg:
                actual = str(msg.get("content", ""))
                assert exp_msg["content_contains"] in actual, (
                    f"{ctx}: content should contain '{exp_msg['content_contains']}'"
                )
            if "tool_call_id" in exp_msg:
                assert msg["tool_call_id"] == exp_msg["tool_call_id"], (
                    f"{ctx}: tool_call_id"
                )
            if "has_tool_calls" in exp_msg:
                assert ("tool_calls" in msg) == exp_msg["has_tool_calls"], (
                    f"{ctx}: has_tool_calls"
                )
