"""Tests for V2 API token calculation"""

import pytest
from app.utils.streaming import (
    calculate_message_tokens,
    calculate_tools_tokens,
    count_tokens,
)
from app.transformer.stream import CrossProtocolStreamState
from app.transformer.unified import UnifiedUsage


class TestV2InputTokenCalculation:
    """Test V2 API input token pre-calculation"""

    def test_calculate_messages_tokens(self):
        """Calculate tokens from messages"""
        messages = [{"role": "user", "content": "Hello, how are you?"}]
        tokens = calculate_message_tokens(messages, "gpt-4")
        assert tokens > 0
        assert tokens < 20  # Should be around 10 tokens

    def test_calculate_with_system_prompt(self):
        """Calculate tokens including system prompt"""
        messages = [{"role": "user", "content": "Hello"}]
        system = "You are a helpful assistant."

        msg_tokens = calculate_message_tokens(messages, "gpt-4")
        sys_tokens = count_tokens(system, "gpt-4")
        total = msg_tokens + sys_tokens

        assert total > msg_tokens
        assert sys_tokens > 0

    def test_calculate_with_tools(self):
        """Calculate tokens including tool definitions"""
        tools = [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather information",
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
        assert tokens < 100  # Should be around 50-80 tokens

    def test_calculate_with_images(self):
        """Calculate tokens with image content"""
        from app.utils.streaming import calculate_image_tokens

        base64_png_1x1 = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR4nGNgYAAAAAMAAWgmWQ0AAAAASUVORK5CYII="
        messages = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "What's in this image?"},
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": base64_png_1x1,
                            "detail": "high",
                        },
                    },
                ],
            }
        ]

        tokens = calculate_message_tokens(messages, "gpt-4")
        # Should include text tokens + high detail image
        assert tokens > 255


class TestCrossProtocolStreamState:
    """Test cross-protocol stream state with token accumulation"""

    def test_initialize_with_input_tokens(self):
        """Initialize stream state with input tokens"""
        state = CrossProtocolStreamState(model="gpt-4", input_tokens=100)
        assert state.usage is not None
        assert state.usage.input_tokens == 100
        assert state.usage.output_tokens == 0

    def test_accumulate_output_tokens(self):
        """Accumulate output tokens from chunks"""
        state = CrossProtocolStreamState(model="gpt-4", input_tokens=50)
        state.accumulate_output_tokens("Hello")
        state.accumulate_output_tokens(" world")

        assert state.usage is not None
        assert state.usage.input_tokens == 50
        assert state.usage.output_tokens > 0
        # "Hello world" should be around 2-3 tokens
        assert state.usage.output_tokens < 10

    def test_accumulate_output_tokens_without_input(self):
        """Accumulate output tokens without pre-calculated input tokens"""
        state = CrossProtocolStreamState(model="gpt-4", input_tokens=0)
        state.accumulate_output_tokens("This is a test response")

        assert state.usage is not None
        assert state.usage.input_tokens == 0
        assert state.usage.output_tokens > 0

    def test_get_final_usage_with_provider(self):
        """Get final usage, prioritizing provider usage"""
        state = CrossProtocolStreamState(model="gpt-4", input_tokens=50)
        state.accumulate_output_tokens("Test")

        # Provider usage takes priority
        provider_usage = UnifiedUsage(
            input_tokens=60, output_tokens=20, cache_read_tokens=10
        )
        final = state.get_final_usage(provider_usage)

        assert final is not None
        assert final.input_tokens == 60
        assert final.output_tokens == 20
        assert final.cache_read_tokens == 10

    def test_get_final_usage_without_provider(self):
        """Get final usage without provider usage"""
        state = CrossProtocolStreamState(model="gpt-4", input_tokens=50)
        state.accumulate_output_tokens("Test response text")

        final = state.get_final_usage(None)

        assert final is not None
        assert final.input_tokens == 50
        assert final.output_tokens > 0

    def test_process_chunks_accumulates_tokens(self):
        """Test that process_chunks accumulates tokens from text deltas"""
        from app.transformer.unified import (
            UnifiedStreamChunk,
            ChunkType,
            create_text_content,
        )

        state = CrossProtocolStreamState(model="gpt-4", input_tokens=100)

        # Create content block delta chunks with text
        chunks = [
            UnifiedStreamChunk(
                chunk_type=ChunkType.CONTENT_BLOCK_START,
                index=0,
                content_block=create_text_content(""),
            ),
            UnifiedStreamChunk(
                chunk_type=ChunkType.CONTENT_BLOCK_DELTA,
                index=0,
                delta=create_text_content("Hello"),
            ),
            UnifiedStreamChunk(
                chunk_type=ChunkType.CONTENT_BLOCK_DELTA,
                index=0,
                delta=create_text_content(" world"),
            ),
        ]

        result = state.process_chunks(chunks)

        # Verify tokens were accumulated
        assert state.usage is not None
        assert state.usage.input_tokens == 100
        assert state.usage.output_tokens > 0

        # Result should contain processed chunks
        assert len(result) > len(chunks)  # Includes synthetic events


class TestV2InputTokenEdgeCases:
    """Test edge cases for V2 input token calculation"""

    def test_empty_messages(self):
        """Calculate tokens for empty messages list"""
        messages = []
        tokens = calculate_message_tokens(messages, "gpt-4")
        # Empty messages still have base overhead
        assert tokens >= 0
        assert tokens < 5

    def test_empty_tools(self):
        """Calculate tokens for empty tools list"""
        tokens = calculate_tools_tokens([], "gpt-4")
        assert tokens == 0

    def test_none_tools(self):
        """Calculate tokens for None tools"""
        tokens = calculate_tools_tokens(None, "gpt-4")
        assert tokens == 0

    def test_multimodal_content(self):
        """Calculate tokens for multimodal messages"""
        base64_png_1x1 = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR4nGNgYAAAAAMAAWgmWQ0AAAAASUVORK5CYII="
        messages = [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "Analyze this"},
                    {
                        "type": "image_url",
                        "image_url": {"url": base64_png_1x1},
                    },
                ],
            }
        ]
        tokens = calculate_message_tokens(messages, "gpt-4")
        # Should include text + image (auto mode = 85 tokens)
        assert tokens > 85


class TestCrossProtocolStreamStateEdgeCases:
    """Test edge cases for CrossProtocolStreamState"""

    def test_initialize_with_zero_input_tokens(self):
        """Initialize with zero input tokens"""
        state = CrossProtocolStreamState(model="gpt-4", input_tokens=0)
        # Usage should not be initialized for zero tokens
        assert state.usage is None or state.usage.input_tokens == 0

    def test_accumulate_empty_text(self):
        """Accumulate tokens from empty text"""
        state = CrossProtocolStreamState(model="gpt-4", input_tokens=50)
        initial_output = state.usage.output_tokens if state.usage else 0

        state.accumulate_output_tokens("")

        assert state.usage is not None
        # Should not add tokens for empty string
        assert state.usage.output_tokens == initial_output

    def test_accumulate_large_text(self):
        """Accumulate tokens from large text"""
        state = CrossProtocolStreamState(model="gpt-4", input_tokens=100)

        # Generate a large text response
        large_text = " ".join(["word"] * 1000)
        state.accumulate_output_tokens(large_text)

        assert state.usage is not None
        assert state.usage.input_tokens == 100
        # 1000 words should be roughly 750-1000 tokens
        assert state.usage.output_tokens > 700


if __name__ == "__main__":
    pytest.main([__file__, "-v"])
