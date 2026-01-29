"""Tests for image and tool token calculation"""

import pytest
from app.utils.streaming import (
    calculate_image_tokens,
    calculate_tools_tokens,
    calculate_message_tokens,
)


class TestImageTokens:
    """Test image token calculation"""

    def test_image_tokens_low_detail(self):
        """Low detail mode returns fixed 85 tokens"""
        tokens = calculate_image_tokens("https://example.com/image.jpg", "low")
        assert tokens == 85

    def test_image_tokens_high_detail(self):
        """High detail mode with conservative estimate"""
        tokens = calculate_image_tokens("https://example.com/image.jpg", "high")
        # Conservative estimate: 1024x1024 → 4 tiles
        assert tokens == 765  # 85 + 4 * 170

    def test_image_tokens_auto_mode(self):
        """Auto mode uses conservative high estimate"""
        tokens = calculate_image_tokens("https://example.com/image.jpg", "auto")
        assert tokens == 765  # Conservative estimate

    def test_image_tokens_base64(self):
        """Base64 image data URI"""
        data_uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAACklEQVR4nGMAAQAABQABDQottAAAAABJRU5ErkJggg=="
        tokens = calculate_image_tokens(data_uri, "low")
        assert tokens == 85


class TestToolsTokens:
    """Test tool definition token calculation"""

    def test_tools_tokens_empty(self):
        """Empty tools list returns 0"""
        tokens = calculate_tools_tokens([], "gpt-4")
        assert tokens == 0

    def test_tools_tokens_single_tool(self):
        """Single tool definition"""
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get current weather",
                    "parameters": {
                        "type": "object",
                        "properties": {"location": {"type": "string"}},
                        "required": ["location"],
                    },
                },
            }
        ]
        tokens = calculate_tools_tokens(tools, "gpt-4")
        assert tokens > 0
        # Should be roughly 50-80 tokens for this simple tool
        assert 30 < tokens < 100

    def test_tools_tokens_multiple(self):
        """Multiple tool definitions"""
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get current weather",
                    "parameters": {
                        "type": "object",
                        "properties": {"location": {"type": "string"}},
                        "required": ["location"],
                    },
                },
            },
            {
                "type": "function",
                "function": {
                    "name": "get_time",
                    "description": "Get current time",
                    "parameters": {"type": "object", "properties": {}},
                },
            },
        ]
        tokens = calculate_tools_tokens(tools, "gpt-4")
        assert tokens > 0
        # Should be roughly 80-150 tokens for two simple tools
        assert 50 < tokens < 200


class TestMessageTokensWithMultimodal:
    """Test message token calculation with multimodal content"""

    def test_message_tokens_text_only(self):
        """Text-only message (baseline)"""
        messages = [{"role": "user", "content": "Hello, how are you?"}]
        tokens = calculate_message_tokens(messages, "gpt-4")
        # Should include text tokens + role + format overhead (4 + 2)
        assert tokens > 6

    def test_message_tokens_with_image_low(self):
        """Message with image (low detail)"""
        messages = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "What's in this image?"},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://example.com/image.jpg",
                            "detail": "low",
                        },
                    },
                ],
            }
        ]
        tokens = calculate_message_tokens(messages, "gpt-4")
        # Should include text tokens + 85 (image) + format overhead
        assert tokens > 85

    def test_message_tokens_with_image_high(self):
        """Message with image (high detail)"""
        messages = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "Analyze this image"},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://example.com/image.jpg",
                            "detail": "high",
                        },
                    },
                ],
            }
        ]
        tokens = calculate_message_tokens(messages, "gpt-4")
        # Should include text tokens + 765 (image high) + format overhead
        assert tokens > 765

    def test_message_tokens_with_multiple_images(self):
        """Message with multiple images"""
        messages = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "Compare these images"},
                    {
                        "type": "image_url",
                        "image_url": {"url": "https://example.com/img1.jpg"},
                    },
                    {
                        "type": "image_url",
                        "image_url": {"url": "https://example.com/img2.jpg"},
                    },
                ],
            }
        ]
        tokens = calculate_message_tokens(messages, "gpt-4")
        # Should include text tokens + 2 * 765 (two images) + format overhead
        assert tokens > 1530

    def test_message_tokens_image_url_string_format(self):
        """Image URL as string (simplified format)"""
        messages = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "What's this?"},
                    {
                        "type": "image_url",
                        "image_url": "https://example.com/image.jpg",
                    },
                ],
            }
        ]
        tokens = calculate_message_tokens(messages, "gpt-4")
        # Should handle string format and use auto detail
        assert tokens > 765


class TestClaudeInputTokensIntegration:
    """Integration tests for Claude input token calculation with tools"""

    def test_claude_tokens_with_tools(self):
        """Claude request with tool definitions"""
        from app.api.claude import _calculate_claude_input_tokens
        from app.models.claude import ClaudeMessagesRequest, ClaudeMessage

        request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            messages=[ClaudeMessage(role="user", content="Hello")],
            tools=[
                {
                    "name": "get_weather",
                    "description": "Get weather",
                    "input_schema": {
                        "type": "object",
                        "properties": {"location": {"type": "string"}},
                        "required": ["location"],
                    },
                }
            ],
        )
        tokens = _calculate_claude_input_tokens(request)
        # Should include message tokens + tool tokens
        assert tokens > 30  # Basic message + tool definition

    def test_claude_tokens_with_system_and_tools(self):
        """Claude request with system prompt and tools"""
        from app.api.claude import _calculate_claude_input_tokens
        from app.models.claude import ClaudeMessagesRequest, ClaudeMessage

        request = ClaudeMessagesRequest(
            model="claude-3-opus-20240229",
            max_tokens=1024,
            system="You are a helpful assistant.",
            messages=[ClaudeMessage(role="user", content="What's the weather?")],
            tools=[
                {
                    "name": "get_weather",
                    "description": "Get current weather information",
                    "input_schema": {
                        "type": "object",
                        "properties": {
                            "location": {"type": "string", "description": "City name"},
                            "unit": {
                                "type": "string",
                                "enum": ["celsius", "fahrenheit"],
                            },
                        },
                        "required": ["location"],
                    },
                }
            ],
        )
        tokens = _calculate_claude_input_tokens(request)
        # Should include system + message + tool tokens
        assert tokens > 50


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
