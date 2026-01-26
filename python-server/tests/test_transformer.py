"""
Comprehensive tests for the Transformer Pipeline.

This module tests:
1. Unified models (Protocol, ContentBlock, UnifiedMessage, UnifiedRequest/Response)
2. Protocol transformers (OpenAI, Anthropic, Response API)
3. Pipeline (bypass mode, cross-protocol transformation, feature transformers)
4. Protocol detection (ProtocolDetector)
"""

import json
import pytest

from app.transformer.unified import (
    Protocol,
    Role,
    StopReason,
    ChunkType,
    TextContent,
    ImageContent,
    ToolUseContent,
    ToolResultContent,
    ThinkingContent,
    FileContent,
    AudioContent,
    RefusalContent,
    ToolInputDeltaContent,
    UnifiedMessage,
    UnifiedRequest,
    UnifiedResponse,
    UnifiedUsage,
    UnifiedTool,
    UnifiedToolCall,
    UnifiedParameters,
    UnifiedStreamChunk,
    create_text_content,
    create_tool_use_content,
    create_image_base64,
    create_image_url,
    create_thinking_content,
    create_tool_result_content,
    create_refusal_content,
)
from app.transformer.base import TransformContext, Transformer
from app.transformer.registry import TransformerRegistry
from app.transformer.detector import ProtocolDetector
from app.transformer.pipeline import (
    TransformPipeline,
    FeatureTransformer,
    FeatureTransformerChain,
    ReasoningTransformer,
    TokenLimitTransformer,
)
from app.transformer.protocols.openai import OpenAITransformer
from app.transformer.protocols.anthropic import AnthropicTransformer
from app.transformer.protocols.response_api import ResponseApiTransformer


# =============================================================================
# Test Fixtures
# =============================================================================


@pytest.fixture
def openai_transformer() -> OpenAITransformer:
    """Create an OpenAI transformer instance."""
    return OpenAITransformer()


@pytest.fixture
def anthropic_transformer() -> AnthropicTransformer:
    """Create an Anthropic transformer instance."""
    return AnthropicTransformer()


@pytest.fixture
def response_api_transformer() -> ResponseApiTransformer:
    """Create a Response API transformer instance."""
    return ResponseApiTransformer()


@pytest.fixture
def registry() -> TransformerRegistry:
    """Create a registry with all transformers registered."""
    reg = TransformerRegistry()
    reg.register(OpenAITransformer())
    reg.register(AnthropicTransformer())
    reg.register(ResponseApiTransformer())
    return reg


@pytest.fixture
def pipeline(registry: TransformerRegistry) -> TransformPipeline:
    """Create a transform pipeline with all transformers."""
    return TransformPipeline(registry)


def create_context(
    client_protocol: Protocol,
    provider_protocol: Protocol,
    model: str,
) -> TransformContext:
    """Create a transform context for testing."""
    return TransformContext(
        request_id="test-request-id",
        client_protocol=client_protocol,
        provider_protocol=provider_protocol,
        original_model=model,
        mapped_model=model,
    )


# =============================================================================
# Protocol Enum Tests
# =============================================================================


class TestProtocolEnum:
    """Tests for Protocol enum."""

    def test_protocol_values(self):
        """Test Protocol enum values."""
        assert Protocol.OPENAI.value == "openai"
        assert Protocol.ANTHROPIC.value == "anthropic"
        assert Protocol.RESPONSE_API.value == "response_api"

    def test_from_provider_type_anthropic(self):
        """Test Protocol.from_provider_type for Anthropic."""
        assert Protocol.from_provider_type("anthropic") == Protocol.ANTHROPIC
        assert Protocol.from_provider_type("Anthropic") == Protocol.ANTHROPIC
        assert Protocol.from_provider_type("claude") == Protocol.ANTHROPIC
        assert Protocol.from_provider_type("CLAUDE") == Protocol.ANTHROPIC

    def test_from_provider_type_openai(self):
        """Test Protocol.from_provider_type for OpenAI."""
        assert Protocol.from_provider_type("openai") == Protocol.OPENAI
        assert Protocol.from_provider_type("OpenAI") == Protocol.OPENAI
        assert Protocol.from_provider_type("azure") == Protocol.OPENAI
        assert Protocol.from_provider_type("unknown") == Protocol.OPENAI


class TestRoleEnum:
    """Tests for Role enum."""

    def test_role_values(self):
        """Test Role enum values."""
        assert Role.SYSTEM.value == "system"
        assert Role.USER.value == "user"
        assert Role.ASSISTANT.value == "assistant"
        assert Role.TOOL.value == "tool"

    def test_from_string(self):
        """Test Role.from_string conversion."""
        assert Role.from_string("user") == Role.USER
        assert Role.from_string("USER") == Role.USER
        assert Role.from_string("assistant") == Role.ASSISTANT
        assert Role.from_string("system") == Role.SYSTEM
        assert Role.from_string("tool") == Role.TOOL
        assert Role.from_string("function") == Role.TOOL


class TestStopReasonEnum:
    """Tests for StopReason enum."""

    def test_stop_reason_values(self):
        """Test StopReason enum values."""
        assert StopReason.END_TURN.value == "end_turn"
        assert StopReason.MAX_TOKENS.value == "max_tokens"
        assert StopReason.STOP_SEQUENCE.value == "stop_sequence"
        assert StopReason.TOOL_USE.value == "tool_use"
        assert StopReason.CONTENT_FILTER.value == "content_filter"
        assert StopReason.LENGTH.value == "length"


class TestChunkTypeEnum:
    """Tests for ChunkType enum."""

    def test_chunk_type_values(self):
        """Test ChunkType enum values."""
        assert ChunkType.MESSAGE_START.value == "message_start"
        assert ChunkType.CONTENT_BLOCK_START.value == "content_block_start"
        assert ChunkType.CONTENT_BLOCK_DELTA.value == "content_block_delta"
        assert ChunkType.CONTENT_BLOCK_STOP.value == "content_block_stop"
        assert ChunkType.MESSAGE_DELTA.value == "message_delta"
        assert ChunkType.MESSAGE_STOP.value == "message_stop"
        assert ChunkType.PING.value == "ping"


# =============================================================================
# Content Block Tests
# =============================================================================


class TestContentBlocks:
    """Tests for content block types."""

    def test_text_content(self):
        """Test TextContent creation."""
        content = TextContent(text="Hello, world!")
        assert content.type == "text"
        assert content.text == "Hello, world!"

    def test_text_content_factory(self):
        """Test create_text_content factory."""
        content = create_text_content("Hello!")
        assert isinstance(content, TextContent)
        assert content.text == "Hello!"

    def test_image_content_base64(self):
        """Test ImageContent with base64 data."""
        content = ImageContent(
            source_type="base64",
            media_type="image/png",
            data="iVBORw0KGgo...",
        )
        assert content.type == "image"
        assert content.source_type == "base64"
        assert content.media_type == "image/png"

    def test_image_content_url(self):
        """Test ImageContent with URL."""
        content = create_image_url("https://example.com/image.png")
        assert content.type == "image"
        assert content.source_type == "url"
        assert content.data == "https://example.com/image.png"

    def test_image_base64_factory(self):
        """Test create_image_base64 factory."""
        content = create_image_base64("image/jpeg", "base64data")
        assert content.source_type == "base64"
        assert content.media_type == "image/jpeg"
        assert content.data == "base64data"

    def test_tool_use_content(self):
        """Test ToolUseContent creation."""
        content = ToolUseContent(
            id="tool_123",
            name="get_weather",
            input={"location": "Tokyo"},
        )
        assert content.type == "tool_use"
        assert content.id == "tool_123"
        assert content.name == "get_weather"
        assert content.input == {"location": "Tokyo"}

    def test_tool_use_content_factory(self):
        """Test create_tool_use_content factory."""
        content = create_tool_use_content("id1", "func", {"arg": "value"})
        assert isinstance(content, ToolUseContent)
        assert content.id == "id1"
        assert content.name == "func"

    def test_tool_result_content(self):
        """Test ToolResultContent creation."""
        content = ToolResultContent(
            tool_use_id="tool_123",
            content="Sunny, 25°C",
            is_error=False,
        )
        assert content.type == "tool_result"
        assert content.tool_use_id == "tool_123"
        assert content.content == "Sunny, 25°C"
        assert content.is_error is False

    def test_tool_result_content_factory(self):
        """Test create_tool_result_content factory."""
        content = create_tool_result_content("id1", "result", True)
        assert isinstance(content, ToolResultContent)
        assert content.is_error is True

    def test_thinking_content(self):
        """Test ThinkingContent creation."""
        content = ThinkingContent(text="Let me think...", signature="sig123")
        assert content.type == "thinking"
        assert content.text == "Let me think..."
        assert content.signature == "sig123"

    def test_thinking_content_factory(self):
        """Test create_thinking_content factory."""
        content = create_thinking_content("thinking text", "signature")
        assert isinstance(content, ThinkingContent)
        assert content.text == "thinking text"
        assert content.signature == "signature"

    def test_file_content(self):
        """Test FileContent creation."""
        content = FileContent(file_id="file_123", filename="document.pdf")
        assert content.type == "file"
        assert content.file_id == "file_123"
        assert content.filename == "document.pdf"

    def test_audio_content(self):
        """Test AudioContent creation."""
        content = AudioContent(data="audio_base64", format="mp3")
        assert content.type == "audio"
        assert content.data == "audio_base64"
        assert content.format == "mp3"

    def test_refusal_content(self):
        """Test RefusalContent creation."""
        content = RefusalContent(reason="I cannot help with that.")
        assert content.type == "refusal"
        assert content.reason == "I cannot help with that."

    def test_refusal_content_factory(self):
        """Test create_refusal_content factory."""
        content = create_refusal_content("Not allowed")
        assert isinstance(content, RefusalContent)
        assert content.reason == "Not allowed"

    def test_tool_input_delta_content(self):
        """Test ToolInputDeltaContent creation."""
        content = ToolInputDeltaContent(index=0, partial_json='{"loc')
        assert content.type == "tool_input_delta"
        assert content.index == 0
        assert content.partial_json == '{"loc'


# =============================================================================
# UnifiedMessage Tests
# =============================================================================


class TestUnifiedMessage:
    """Tests for UnifiedMessage."""

    def test_user_message_factory(self):
        """Test UnifiedMessage.user factory."""
        msg = UnifiedMessage.user("Hello!")
        assert msg.role == Role.USER
        assert len(msg.content) == 1
        assert isinstance(msg.content[0], TextContent)
        assert msg.content[0].text == "Hello!"

    def test_assistant_message_factory(self):
        """Test UnifiedMessage.assistant factory."""
        msg = UnifiedMessage.assistant("Hi there!")
        assert msg.role == Role.ASSISTANT
        assert msg.text_content() == "Hi there!"

    def test_system_message_factory(self):
        """Test UnifiedMessage.system factory."""
        msg = UnifiedMessage.system("You are helpful.")
        assert msg.role == Role.SYSTEM
        assert msg.text_content() == "You are helpful."

    def test_tool_result_factory(self):
        """Test UnifiedMessage.tool_result factory."""
        msg = UnifiedMessage.tool_result("tool_123", "Result data", False)
        assert msg.role == Role.TOOL
        assert msg.tool_call_id == "tool_123"
        assert len(msg.content) == 1
        assert isinstance(msg.content[0], ToolResultContent)

    def test_with_content_factory(self):
        """Test UnifiedMessage.with_content factory."""
        content = [create_text_content("Part 1"), create_text_content("Part 2")]
        msg = UnifiedMessage.with_content(Role.USER, content)
        assert msg.role == Role.USER
        assert len(msg.content) == 2

    def test_text_content_method(self):
        """Test text_content method concatenates text blocks."""
        msg = UnifiedMessage(
            role=Role.ASSISTANT,
            content=[
                create_text_content("Hello "),
                create_text_content("world!"),
            ],
        )
        assert msg.text_content() == "Hello world!"

    def test_text_content_ignores_non_text(self):
        """Test text_content ignores non-text content blocks."""
        msg = UnifiedMessage(
            role=Role.ASSISTANT,
            content=[
                create_text_content("Text"),
                create_tool_use_content("id", "func", {}),
            ],
        )
        assert msg.text_content() == "Text"


# =============================================================================
# UnifiedRequest Tests
# =============================================================================


class TestUnifiedRequest:
    """Tests for UnifiedRequest."""

    def test_basic_request(self):
        """Test basic UnifiedRequest creation."""
        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
        )
        assert request.model == "gpt-4"
        assert len(request.messages) == 1
        assert request.system is None
        assert request.client_protocol == Protocol.OPENAI

    def test_request_with_system(self):
        """Test UnifiedRequest with system prompt."""
        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
            system="Be helpful",
        )
        assert request.system == "Be helpful"

    def test_with_system_method(self):
        """Test with_system builder method."""
        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
        ).with_system("System prompt")
        assert request.system == "System prompt"

    def test_with_stream_method(self):
        """Test with_stream builder method."""
        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
        ).with_stream(True)
        assert request.is_streaming() is True

    def test_with_max_tokens_method(self):
        """Test with_max_tokens builder method."""
        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
        ).with_max_tokens(100)
        assert request.parameters.max_tokens == 100

    def test_is_streaming(self):
        """Test is_streaming method."""
        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
            parameters=UnifiedParameters(stream=True),
        )
        assert request.is_streaming() is True

    def test_request_with_tools(self):
        """Test UnifiedRequest with tools."""
        tool = UnifiedTool(
            name="get_weather",
            description="Get weather",
            input_schema={"type": "object"},
        )
        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
            tools=[tool],
        )
        assert len(request.tools) == 1
        assert request.tools[0].name == "get_weather"


# =============================================================================
# UnifiedResponse Tests
# =============================================================================


class TestUnifiedResponse:
    """Tests for UnifiedResponse."""

    def test_basic_response(self):
        """Test basic UnifiedResponse creation."""
        response = UnifiedResponse(
            id="resp_123",
            model="gpt-4",
            content=[create_text_content("Hello!")],
        )
        assert response.id == "resp_123"
        assert response.model == "gpt-4"
        assert response.text_content() == "Hello!"

    def test_text_factory(self):
        """Test UnifiedResponse.text factory."""
        usage = UnifiedUsage(input_tokens=10, output_tokens=5)
        response = UnifiedResponse.text("id", "model", "Hello", usage)
        assert response.id == "id"
        assert response.model == "model"
        assert response.text_content() == "Hello"
        assert response.stop_reason == StopReason.END_TURN
        assert response.usage.input_tokens == 10

    def test_response_with_tool_calls(self):
        """Test UnifiedResponse with tool calls."""
        tool_call = UnifiedToolCall(
            id="call_123",
            name="get_weather",
            arguments={"location": "Tokyo"},
        )
        response = UnifiedResponse(
            id="resp_123",
            model="gpt-4",
            content=[],
            tool_calls=[tool_call],
        )
        assert len(response.tool_calls) == 1
        assert response.tool_calls[0].name == "get_weather"


# =============================================================================
# UnifiedUsage Tests
# =============================================================================


class TestUnifiedUsage:
    """Tests for UnifiedUsage."""

    def test_basic_usage(self):
        """Test basic UnifiedUsage creation."""
        usage = UnifiedUsage(input_tokens=100, output_tokens=50)
        assert usage.input_tokens == 100
        assert usage.output_tokens == 50
        assert usage.total_tokens() == 150

    def test_usage_with_cache(self):
        """Test UnifiedUsage with cache tokens."""
        usage = UnifiedUsage(
            input_tokens=100,
            output_tokens=50,
            cache_read_tokens=20,
            cache_write_tokens=10,
        )
        assert usage.cache_read_tokens == 20
        assert usage.cache_write_tokens == 10

    def test_default_usage(self):
        """Test default UnifiedUsage values."""
        usage = UnifiedUsage()
        assert usage.input_tokens == 0
        assert usage.output_tokens == 0
        assert usage.total_tokens() == 0


# =============================================================================
# UnifiedStreamChunk Tests
# =============================================================================


class TestUnifiedStreamChunk:
    """Tests for UnifiedStreamChunk."""

    def test_message_start_factory(self):
        """Test message_start factory."""
        response = UnifiedResponse(
            id="resp_123",
            model="gpt-4",
            content=[],
            usage=UnifiedUsage(input_tokens=10),
        )
        chunk = UnifiedStreamChunk.message_start(response)
        assert chunk.chunk_type == ChunkType.MESSAGE_START
        assert chunk.message == response
        assert chunk.usage.input_tokens == 10

    def test_content_block_start_factory(self):
        """Test content_block_start factory."""
        content = create_text_content("")
        chunk = UnifiedStreamChunk.content_block_start(0, content)
        assert chunk.chunk_type == ChunkType.CONTENT_BLOCK_START
        assert chunk.index == 0
        assert chunk.content_block == content

    def test_content_block_delta_factory(self):
        """Test content_block_delta factory."""
        delta = create_text_content("Hello")
        chunk = UnifiedStreamChunk.content_block_delta(0, delta)
        assert chunk.chunk_type == ChunkType.CONTENT_BLOCK_DELTA
        assert chunk.index == 0
        assert chunk.delta == delta

    def test_content_block_stop_factory(self):
        """Test content_block_stop factory."""
        chunk = UnifiedStreamChunk.content_block_stop(0)
        assert chunk.chunk_type == ChunkType.CONTENT_BLOCK_STOP
        assert chunk.index == 0

    def test_message_delta_factory(self):
        """Test message_delta factory."""
        usage = UnifiedUsage(input_tokens=10, output_tokens=5)
        chunk = UnifiedStreamChunk.message_delta(StopReason.END_TURN, usage)
        assert chunk.chunk_type == ChunkType.MESSAGE_DELTA
        assert chunk.stop_reason == StopReason.END_TURN
        assert chunk.usage == usage

    def test_message_stop_factory(self):
        """Test message_stop factory."""
        chunk = UnifiedStreamChunk.message_stop()
        assert chunk.chunk_type == ChunkType.MESSAGE_STOP

    def test_ping_factory(self):
        """Test ping factory."""
        chunk = UnifiedStreamChunk.ping()
        assert chunk.chunk_type == ChunkType.PING


# =============================================================================
# UnifiedTool Tests
# =============================================================================


class TestUnifiedTool:
    """Tests for UnifiedTool."""

    def test_basic_tool(self):
        """Test basic UnifiedTool creation."""
        tool = UnifiedTool(
            name="search",
            description="Search the web",
            input_schema={
                "type": "object",
                "properties": {"query": {"type": "string"}},
            },
        )
        assert tool.name == "search"
        assert tool.description == "Search the web"

    def test_function_factory(self):
        """Test UnifiedTool.function factory."""
        tool = UnifiedTool.function(
            "search",
            "Search the web",
            {"type": "object"},
        )
        assert tool.name == "search"
        assert tool.tool_type == "function"


# =============================================================================
# TransformContext Tests
# =============================================================================


class TestTransformContext:
    """Tests for TransformContext."""

    def test_basic_context(self):
        """Test basic TransformContext creation."""
        ctx = TransformContext(
            request_id="req_123",
            client_protocol=Protocol.OPENAI,
            provider_protocol=Protocol.ANTHROPIC,
            original_model="gpt-4",
            mapped_model="claude-3-opus",
        )
        assert ctx.request_id == "req_123"
        assert ctx.client_protocol == Protocol.OPENAI
        assert ctx.provider_protocol == Protocol.ANTHROPIC

    def test_is_same_protocol_true(self):
        """Test is_same_protocol returns True for same protocols."""
        ctx = TransformContext(
            client_protocol=Protocol.OPENAI,
            provider_protocol=Protocol.OPENAI,
        )
        assert ctx.is_same_protocol() is True

    def test_is_same_protocol_false(self):
        """Test is_same_protocol returns False for different protocols."""
        ctx = TransformContext(
            client_protocol=Protocol.OPENAI,
            provider_protocol=Protocol.ANTHROPIC,
        )
        assert ctx.is_same_protocol() is False


# =============================================================================
# TransformerRegistry Tests
# =============================================================================


class TestTransformerRegistry:
    """Tests for TransformerRegistry."""

    def test_register_and_get(self):
        """Test registering and retrieving transformers."""
        registry = TransformerRegistry()
        transformer = OpenAITransformer()
        registry.register(transformer)

        result = registry.get(Protocol.OPENAI)
        assert result is transformer

    def test_get_nonexistent(self):
        """Test getting non-existent transformer returns None."""
        registry = TransformerRegistry()
        result = registry.get(Protocol.OPENAI)
        assert result is None

    def test_get_or_error_raises(self):
        """Test get_or_error raises for non-existent transformer."""
        registry = TransformerRegistry()
        with pytest.raises(ValueError, match="Unsupported protocol"):
            registry.get_or_error(Protocol.OPENAI)

    def test_unregister(self):
        """Test unregistering a transformer."""
        registry = TransformerRegistry()
        transformer = OpenAITransformer()
        registry.register(transformer)

        removed = registry.unregister(Protocol.OPENAI)
        assert removed is transformer
        assert registry.get(Protocol.OPENAI) is None

    def test_protocols_list(self):
        """Test listing registered protocols."""
        registry = TransformerRegistry()
        registry.register(OpenAITransformer())
        registry.register(AnthropicTransformer())

        protocols = registry.protocols()
        assert Protocol.OPENAI in protocols
        assert Protocol.ANTHROPIC in protocols

    def test_is_registered(self):
        """Test is_registered method."""
        registry = TransformerRegistry()
        registry.register(OpenAITransformer())

        assert registry.is_registered(Protocol.OPENAI) is True
        assert registry.is_registered(Protocol.ANTHROPIC) is False

    def test_len(self):
        """Test __len__ method."""
        registry = TransformerRegistry()
        assert len(registry) == 0

        registry.register(OpenAITransformer())
        assert len(registry) == 1

    def test_contains(self):
        """Test __contains__ method."""
        registry = TransformerRegistry()
        registry.register(OpenAITransformer())

        assert Protocol.OPENAI in registry
        assert Protocol.ANTHROPIC not in registry

    def test_detect_and_get(self):
        """Test detect_and_get method."""
        registry = TransformerRegistry()
        registry.register(OpenAITransformer())
        registry.register(AnthropicTransformer())

        # OpenAI format
        openai_request = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hi"}],
        }
        transformer = registry.detect_and_get(openai_request)
        assert transformer.protocol == Protocol.OPENAI

        # Anthropic format
        anthropic_request = {"model": "claude", "system": "Be helpful", "messages": []}
        transformer = registry.detect_and_get(anthropic_request)
        assert transformer.protocol == Protocol.ANTHROPIC


# =============================================================================
# ProtocolDetector Tests
# =============================================================================


class TestProtocolDetector:
    """Tests for ProtocolDetector."""

    def test_detect_openai_format(self):
        """Test detecting OpenAI format."""
        request = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
        }
        assert ProtocolDetector.detect(request) == Protocol.OPENAI

    def test_detect_anthropic_format_with_system(self):
        """Test detecting Anthropic format with system field and max_tokens."""
        request = {
            "model": "claude-3",
            "max_tokens": 1024,
            "system": "Be helpful",
            "messages": [{"role": "user", "content": "Hello"}],
        }
        assert ProtocolDetector.detect(request) == Protocol.ANTHROPIC

    def test_detect_anthropic_format_with_content_blocks(self):
        """Test detecting Anthropic format with content blocks."""
        request = {
            "model": "claude-3",
            "max_tokens": 1024,
            "messages": [
                {
                    "role": "user",
                    "content": [{"type": "text", "text": "Hello"}],
                }
            ],
        }
        assert ProtocolDetector.detect(request) == Protocol.ANTHROPIC

    def test_detect_response_api_with_input(self):
        """Test detecting Response API format with input field."""
        request = {
            "model": "gpt-4",
            "input": "Hello",
        }
        assert ProtocolDetector.detect(request) == Protocol.RESPONSE_API

    def test_detect_response_api_with_instructions(self):
        """Test detecting Response API format with instructions."""
        request = {
            "model": "gpt-4",
            "instructions": "Be helpful",
        }
        assert ProtocolDetector.detect(request) == Protocol.RESPONSE_API

    def test_detect_response_api_with_max_output_tokens(self):
        """Test detecting Response API format with max_output_tokens."""
        request = {
            "model": "gpt-4",
            "max_output_tokens": 100,
        }
        assert ProtocolDetector.detect(request) == Protocol.RESPONSE_API

    def test_detect_from_path_chat_completions(self):
        """Test detecting protocol from /chat/completions path."""
        assert (
            ProtocolDetector.detect_from_path("/v1/chat/completions") == Protocol.OPENAI
        )
        assert (
            ProtocolDetector.detect_from_path("/v2/chat/completions") == Protocol.OPENAI
        )

    def test_detect_from_path_messages(self):
        """Test detecting protocol from /messages path."""
        assert ProtocolDetector.detect_from_path("/v1/messages") == Protocol.ANTHROPIC

    def test_detect_from_path_responses(self):
        """Test detecting protocol from /responses path."""
        assert (
            ProtocolDetector.detect_from_path("/v1/responses") == Protocol.RESPONSE_API
        )

    def test_detect_from_path_unknown(self):
        """Test detecting protocol from unknown path returns None."""
        assert ProtocolDetector.detect_from_path("/unknown") is None

    def test_detect_with_path_hint(self):
        """Test detect_with_path_hint prioritizes path."""
        request = {"model": "gpt-4", "messages": []}
        # Path should take priority
        result = ProtocolDetector.detect_with_path_hint(request, "/v1/messages")
        assert result == Protocol.ANTHROPIC

    def test_detect_from_headers_anthropic(self):
        """Test detecting protocol from Anthropic headers."""
        headers = {"anthropic-version": "2024-01-01"}
        assert ProtocolDetector.detect_from_headers(headers) == Protocol.ANTHROPIC

    def test_detect_from_headers_openai(self):
        """Test detecting protocol from OpenAI headers."""
        headers = {"openai-organization": "org-123"}
        assert ProtocolDetector.detect_from_headers(headers) == Protocol.OPENAI

    def test_detect_comprehensive(self):
        """Test comprehensive detection with all signals."""
        request = {"model": "gpt-4", "messages": []}
        headers = {"anthropic-version": "2024-01-01"}

        # Path takes priority
        result = ProtocolDetector.detect_comprehensive(
            request, path="/v1/responses", headers=headers
        )
        assert result == Protocol.RESPONSE_API

        # Headers take priority over content
        result = ProtocolDetector.detect_comprehensive(
            request, path=None, headers=headers
        )
        assert result == Protocol.ANTHROPIC


# =============================================================================
# OpenAI Transformer Tests
# =============================================================================


class TestOpenAITransformer:
    """Tests for OpenAITransformer."""

    def test_protocol_property(self, openai_transformer):
        """Test protocol property."""
        assert openai_transformer.protocol == Protocol.OPENAI

    def test_endpoint_property(self, openai_transformer):
        """Test endpoint property."""
        assert openai_transformer.endpoint == "/v1/chat/completions"

    def test_can_handle_openai_format(self, openai_transformer):
        """Test can_handle for OpenAI format."""
        request = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
        }
        assert openai_transformer.can_handle(request) is True

    def test_can_handle_rejects_anthropic(self, openai_transformer):
        """Test can_handle rejects Anthropic format."""
        request = {
            "model": "claude-3",
            "system": "Be helpful",
            "messages": [{"role": "user", "content": "Hello"}],
        }
        assert openai_transformer.can_handle(request) is False

    def test_transform_request_out_simple(self, openai_transformer):
        """Test transform_request_out with simple message."""
        request = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
        }
        unified = openai_transformer.transform_request_out(request)

        assert unified.model == "gpt-4"
        assert len(unified.messages) == 1
        assert unified.messages[0].role == Role.USER
        assert unified.messages[0].text_content() == "Hello"

    def test_transform_request_out_with_system(self, openai_transformer):
        """Test transform_request_out extracts system message."""
        request = {
            "model": "gpt-4",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hello"},
            ],
        }
        unified = openai_transformer.transform_request_out(request)

        assert unified.system == "Be helpful"
        assert len(unified.messages) == 1  # System not in messages

    def test_transform_request_out_with_parameters(self, openai_transformer):
        """Test transform_request_out preserves parameters."""
        request = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "temperature": 0.7,
            "max_tokens": 100,
            "top_p": 0.9,
            "stream": True,
        }
        unified = openai_transformer.transform_request_out(request)

        assert unified.parameters.temperature == 0.7
        assert unified.parameters.max_tokens == 100
        assert unified.parameters.top_p == 0.9
        assert unified.parameters.stream is True

    def test_transform_request_out_with_tools(self, openai_transformer):
        """Test transform_request_out converts tools."""
        request = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather",
                        "parameters": {"type": "object"},
                    },
                }
            ],
        }
        unified = openai_transformer.transform_request_out(request)

        assert len(unified.tools) == 1
        assert unified.tools[0].name == "get_weather"

    def test_transform_request_in_simple(self, openai_transformer):
        """Test transform_request_in with simple request."""
        unified = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
        )
        result = openai_transformer.transform_request_in(unified)

        assert result["model"] == "gpt-4"
        assert len(result["messages"]) == 1
        assert result["messages"][0]["role"] == "user"

    def test_transform_request_in_with_system(self, openai_transformer):
        """Test transform_request_in adds system message."""
        unified = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
            system="Be helpful",
        )
        result = openai_transformer.transform_request_in(unified)

        assert len(result["messages"]) == 2
        assert result["messages"][0]["role"] == "system"
        assert result["messages"][0]["content"] == "Be helpful"

    def test_transform_response_in(self, openai_transformer):
        """Test transform_response_in converts OpenAI response."""
        response = {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "gpt-4",
            "choices": [
                {
                    "index": 0,
                    "message": {"role": "assistant", "content": "Hello!"},
                    "finish_reason": "stop",
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
            },
        }
        unified = openai_transformer.transform_response_in(response, "gpt-4")

        assert unified.id == "chatcmpl-123"
        assert unified.model == "gpt-4"
        assert unified.text_content() == "Hello!"
        assert unified.stop_reason == StopReason.END_TURN
        assert unified.usage.input_tokens == 10
        assert unified.usage.output_tokens == 5

    def test_transform_response_out(self, openai_transformer):
        """Test transform_response_out converts to OpenAI format."""
        unified = UnifiedResponse(
            id="resp_123",
            model="gpt-4",
            content=[create_text_content("Hello!")],
            stop_reason=StopReason.END_TURN,
            usage=UnifiedUsage(input_tokens=10, output_tokens=5),
        )
        result = openai_transformer.transform_response_out(unified, Protocol.OPENAI)

        assert result["id"] == "resp_123"
        assert result["object"] == "chat.completion"
        assert result["model"] == "gpt-4"
        assert result["choices"][0]["message"]["content"] == "Hello!"
        assert result["choices"][0]["finish_reason"] == "stop"
        assert result["usage"]["prompt_tokens"] == 10

    def test_transform_stream_chunk_in_text_delta(self, openai_transformer):
        """Test transform_stream_chunk_in with text delta."""
        chunk = b'data: {"id":"chatcmpl-123","choices":[{"index":0,"delta":{"content":"Hello"}}]}\n\n'
        chunks = openai_transformer.transform_stream_chunk_in(chunk)

        assert len(chunks) >= 1
        # Should have content delta
        delta_chunks = [
            c for c in chunks if c.chunk_type == ChunkType.CONTENT_BLOCK_DELTA
        ]
        assert len(delta_chunks) == 1
        assert isinstance(delta_chunks[0].delta, TextContent)
        assert delta_chunks[0].delta.text == "Hello"

    def test_transform_stream_chunk_in_done(self, openai_transformer):
        """Test transform_stream_chunk_in with [DONE] marker."""
        chunk = b"data: [DONE]\n\n"
        chunks = openai_transformer.transform_stream_chunk_in(chunk)

        assert len(chunks) == 1
        assert chunks[0].chunk_type == ChunkType.MESSAGE_STOP

    def test_transform_stream_chunk_out_text_delta(self, openai_transformer):
        """Test transform_stream_chunk_out with text delta."""
        chunk = UnifiedStreamChunk.content_block_delta(0, create_text_content("Hello"))
        result = openai_transformer.transform_stream_chunk_out(chunk, Protocol.OPENAI)

        assert result.startswith("data: ")
        data = json.loads(result[6:].strip())
        assert data["choices"][0]["delta"]["content"] == "Hello"

    def test_transform_stream_chunk_out_done(self, openai_transformer):
        """Test transform_stream_chunk_out with message stop."""
        chunk = UnifiedStreamChunk.message_stop()
        result = openai_transformer.transform_stream_chunk_out(chunk, Protocol.OPENAI)

        assert result == "data: [DONE]\n\n"


# =============================================================================
# Anthropic Transformer Tests
# =============================================================================


class TestAnthropicTransformer:
    """Tests for AnthropicTransformer."""

    def test_protocol_property(self, anthropic_transformer):
        """Test protocol property."""
        assert anthropic_transformer.protocol == Protocol.ANTHROPIC

    def test_endpoint_property(self, anthropic_transformer):
        """Test endpoint property."""
        assert anthropic_transformer.endpoint == "/v1/messages"

    def test_can_handle_anthropic_format(self, anthropic_transformer):
        """Test can_handle for Anthropic format."""
        request = {
            "model": "claude-3",
            "system": "Be helpful",
            "messages": [{"role": "user", "content": "Hello"}],
        }
        assert anthropic_transformer.can_handle(request) is True

    def test_can_handle_rejects_openai(self, anthropic_transformer):
        """Test can_handle rejects OpenAI format."""
        request = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
        }
        assert anthropic_transformer.can_handle(request) is False

    def test_transform_request_out_simple(self, anthropic_transformer):
        """Test transform_request_out with simple message."""
        request = {
            "model": "claude-3",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}],
        }
        unified = anthropic_transformer.transform_request_out(request)

        assert unified.model == "claude-3"
        assert len(unified.messages) == 1
        assert unified.messages[0].role == Role.USER

    def test_transform_request_out_with_system(self, anthropic_transformer):
        """Test transform_request_out extracts system field."""
        request = {
            "model": "claude-3",
            "max_tokens": 1024,
            "system": "Be helpful",
            "messages": [{"role": "user", "content": "Hello"}],
        }
        unified = anthropic_transformer.transform_request_out(request)

        assert unified.system == "Be helpful"

    def test_transform_request_out_with_content_blocks(self, anthropic_transformer):
        """Test transform_request_out with content blocks."""
        request = {
            "model": "claude-3",
            "max_tokens": 1024,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": "Hello"},
                        {"type": "text", "text": " world"},
                    ],
                }
            ],
        }
        unified = anthropic_transformer.transform_request_out(request)

        assert len(unified.messages[0].content) == 2

    def test_transform_request_in_simple(self, anthropic_transformer):
        """Test transform_request_in with simple request."""
        unified = UnifiedRequest(
            model="claude-3",
            messages=[UnifiedMessage.user("Hello")],
        )
        result = anthropic_transformer.transform_request_in(unified)

        assert result["model"] == "claude-3"
        assert result["max_tokens"] == 4096  # Default
        assert len(result["messages"]) == 1

    def test_transform_request_in_with_system(self, anthropic_transformer):
        """Test transform_request_in adds system field."""
        unified = UnifiedRequest(
            model="claude-3",
            messages=[UnifiedMessage.user("Hello")],
            system="Be helpful",
        )
        result = anthropic_transformer.transform_request_in(unified)

        assert result["system"] == "Be helpful"

    def test_transform_response_in(self, anthropic_transformer):
        """Test transform_response_in converts Anthropic response."""
        response = {
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-3",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }
        unified = anthropic_transformer.transform_response_in(response, "claude-3")

        assert unified.id == "msg_123"
        assert unified.model == "claude-3"
        assert unified.text_content() == "Hello!"
        assert unified.stop_reason == StopReason.END_TURN

    def test_transform_response_out(self, anthropic_transformer):
        """Test transform_response_out converts to Anthropic format."""
        unified = UnifiedResponse(
            id="resp_123",
            model="claude-3",
            content=[create_text_content("Hello!")],
            stop_reason=StopReason.END_TURN,
            usage=UnifiedUsage(input_tokens=10, output_tokens=5),
        )
        result = anthropic_transformer.transform_response_out(
            unified, Protocol.ANTHROPIC
        )

        assert result["id"] == "resp_123"
        assert result["type"] == "message"
        assert result["role"] == "assistant"
        assert result["content"][0]["type"] == "text"
        assert result["content"][0]["text"] == "Hello!"
        assert result["stop_reason"] == "end_turn"

    def test_transform_stream_chunk_in_text_delta(self, anthropic_transformer):
        """Test transform_stream_chunk_in with text delta."""
        chunk = b'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}\n\n'
        chunks = anthropic_transformer.transform_stream_chunk_in(chunk)

        assert len(chunks) >= 1
        delta_chunks = [
            c for c in chunks if c.chunk_type == ChunkType.CONTENT_BLOCK_DELTA
        ]
        assert len(delta_chunks) == 1

    def test_transform_stream_chunk_out_text_delta(self, anthropic_transformer):
        """Test transform_stream_chunk_out with text delta."""
        chunk = UnifiedStreamChunk.content_block_delta(0, create_text_content("Hello"))
        result = anthropic_transformer.transform_stream_chunk_out(
            chunk, Protocol.ANTHROPIC
        )

        assert "event: content_block_delta" in result
        assert "text_delta" in result


# =============================================================================
# Response API Transformer Tests
# =============================================================================


class TestResponseApiTransformer:
    """Tests for ResponseApiTransformer."""

    def test_protocol_property(self, response_api_transformer):
        """Test protocol property."""
        assert response_api_transformer.protocol == Protocol.RESPONSE_API

    def test_endpoint_property(self, response_api_transformer):
        """Test endpoint property."""
        assert response_api_transformer.endpoint == "/v1/responses"

    def test_can_handle_response_api_format(self, response_api_transformer):
        """Test can_handle for Response API format."""
        request = {"model": "gpt-4", "input": "Hello"}
        assert response_api_transformer.can_handle(request) is True

    def test_can_handle_with_instructions(self, response_api_transformer):
        """Test can_handle with instructions field."""
        request = {"model": "gpt-4", "instructions": "Be helpful"}
        assert response_api_transformer.can_handle(request) is True

    def test_can_handle_rejects_openai(self, response_api_transformer):
        """Test can_handle rejects OpenAI format."""
        request = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
        }
        assert response_api_transformer.can_handle(request) is False

    def test_transform_request_out_simple_input(self, response_api_transformer):
        """Test transform_request_out with simple string input."""
        request = {"model": "gpt-4", "input": "Hello"}
        unified = response_api_transformer.transform_request_out(request)

        assert unified.model == "gpt-4"
        assert len(unified.messages) == 1
        assert unified.messages[0].role == Role.USER

    def test_transform_request_out_with_instructions(self, response_api_transformer):
        """Test transform_request_out extracts instructions."""
        request = {
            "model": "gpt-4",
            "instructions": "Be helpful",
            "input": "Hello",
        }
        unified = response_api_transformer.transform_request_out(request)

        assert unified.system == "Be helpful"

    def test_transform_request_in_simple(self, response_api_transformer):
        """Test transform_request_in with simple request."""
        unified = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
        )
        result = response_api_transformer.transform_request_in(unified)

        assert result["model"] == "gpt-4"
        assert "input" in result

    def test_transform_response_in(self, response_api_transformer):
        """Test transform_response_in converts Response API response."""
        response = {
            "id": "resp_123",
            "object": "response",
            "model": "gpt-4",
            "output": [
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "Hello!"}],
                }
            ],
            "status": "completed",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }
        unified = response_api_transformer.transform_response_in(response, "gpt-4")

        assert unified.id == "resp_123"
        assert unified.model == "gpt-4"
        assert unified.text_content() == "Hello!"

    def test_transform_response_out(self, response_api_transformer):
        """Test transform_response_out converts to Response API format."""
        unified = UnifiedResponse(
            id="resp_123",
            model="gpt-4",
            content=[create_text_content("Hello!")],
            stop_reason=StopReason.END_TURN,
            usage=UnifiedUsage(input_tokens=10, output_tokens=5),
        )
        result = response_api_transformer.transform_response_out(
            unified, Protocol.RESPONSE_API
        )

        assert result["id"] == "resp_123"
        assert result["object"] == "response"
        assert result["status"] == "completed"
        assert result["output"][0]["type"] == "message"


# =============================================================================
# TransformPipeline Tests
# =============================================================================


class TestTransformPipeline:
    """Tests for TransformPipeline."""

    def test_default_factory(self):
        """Test TransformPipeline.default factory."""
        pipeline = TransformPipeline.default()
        assert pipeline.registry is not None

    def test_with_default_transformers(self):
        """Test TransformPipeline.with_default_transformers factory."""
        pipeline = TransformPipeline.with_default_transformers()
        assert Protocol.OPENAI in pipeline.registry
        assert Protocol.ANTHROPIC in pipeline.registry
        assert Protocol.RESPONSE_API in pipeline.registry

    def test_should_bypass_same_protocol(self, pipeline):
        """Test should_bypass returns True for same protocol."""
        ctx = create_context(Protocol.OPENAI, Protocol.OPENAI, "gpt-4")
        assert pipeline.should_bypass(ctx) is True

    def test_should_bypass_different_protocol(self, pipeline):
        """Test should_bypass returns False for different protocols."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")
        assert pipeline.should_bypass(ctx) is False

    def test_should_bypass_with_features(self, pipeline):
        """Test should_bypass returns False when features are set."""
        pipeline.set_features(ReasoningTransformer())
        ctx = create_context(Protocol.OPENAI, Protocol.OPENAI, "gpt-4")
        assert pipeline.should_bypass(ctx) is False

    def test_transform_request_openai_to_anthropic(self, pipeline):
        """Test transforming OpenAI request to Anthropic format."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")
        request = {
            "model": "claude-3",
            "messages": [{"role": "user", "content": "Hello"}],
        }

        result = pipeline.transform_request(request, ctx)

        assert result["model"] == "claude-3"
        assert "max_tokens" in result
        assert result["messages"][0]["role"] == "user"

    def test_transform_request_anthropic_to_openai(self, pipeline):
        """Test transforming Anthropic request to OpenAI format."""
        ctx = create_context(Protocol.ANTHROPIC, Protocol.OPENAI, "gpt-4")
        request = {
            "model": "gpt-4",
            "max_tokens": 1024,
            "system": "Be helpful",
            "messages": [{"role": "user", "content": "Hello"}],
        }

        result = pipeline.transform_request(request, ctx)

        assert result["model"] == "gpt-4"
        # System should be converted to message
        assert result["messages"][0]["role"] == "system"

    def test_transform_response_anthropic_to_openai(self, pipeline):
        """Test transforming Anthropic response to OpenAI format."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")
        response = {
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-3",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }

        result = pipeline.transform_response(response, ctx)

        assert result["object"] == "chat.completion"
        assert result["choices"][0]["message"]["content"] == "Hello!"
        assert result["choices"][0]["finish_reason"] == "stop"

    def test_transform_response_openai_to_anthropic(self, pipeline):
        """Test transforming OpenAI response to Anthropic format."""
        ctx = create_context(Protocol.ANTHROPIC, Protocol.OPENAI, "gpt-4")
        response = {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "gpt-4",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hello!"},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5},
        }

        result = pipeline.transform_response(response, ctx)

        assert result["type"] == "message"
        assert result["content"][0]["type"] == "text"
        assert result["stop_reason"] == "end_turn"

    def test_transform_request_with_bypass(self, pipeline):
        """Test transform_request_with_bypass for same protocol."""
        ctx = create_context(Protocol.OPENAI, Protocol.OPENAI, "gpt-4")
        request = {
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}],
        }

        result, bypassed = pipeline.transform_request_with_bypass(request, ctx)

        assert bypassed is True
        assert result["model"] == "gpt-4"

    def test_transform_request_with_bypass_cross_protocol(self, pipeline):
        """Test transform_request_with_bypass for different protocols."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")
        request = {
            "model": "claude-3",
            "messages": [{"role": "user", "content": "Hello"}],
        }

        result, bypassed = pipeline.transform_request_with_bypass(request, ctx)

        assert bypassed is False
        assert "max_tokens" in result  # Anthropic format

    def test_transform_request_with_model_mapping(self, pipeline):
        """Test transform_request applies model mapping."""
        ctx = TransformContext(
            client_protocol=Protocol.OPENAI,
            provider_protocol=Protocol.ANTHROPIC,
            original_model="my-model",
            mapped_model="claude-3-opus",
        )
        request = {
            "model": "my-model",
            "messages": [{"role": "user", "content": "Hello"}],
        }

        result = pipeline.transform_request(request, ctx)

        assert result["model"] == "claude-3-opus"

    def test_transform_response_restores_model(self, pipeline):
        """Test transform_response restores original model name."""
        ctx = TransformContext(
            client_protocol=Protocol.OPENAI,
            provider_protocol=Protocol.ANTHROPIC,
            original_model="my-model",
            mapped_model="claude-3-opus",
        )
        response = {
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-3-opus",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }

        result = pipeline.transform_response(response, ctx)

        assert result["model"] == "my-model"


# =============================================================================
# Feature Transformer Tests
# =============================================================================


class TestFeatureTransformers:
    """Tests for feature transformers."""

    def test_reasoning_transformer_include(self):
        """Test ReasoningTransformer includes thinking by default."""
        transformer = ReasoningTransformer(include_thinking=True)
        response = UnifiedResponse(
            id="resp",
            model="model",
            content=[
                create_thinking_content("Let me think..."),
                create_text_content("Answer"),
            ],
        )

        transformer.transform_response(response)

        assert len(response.content) == 2

    def test_reasoning_transformer_exclude(self):
        """Test ReasoningTransformer excludes thinking when configured."""
        transformer = ReasoningTransformer(include_thinking=False)
        response = UnifiedResponse(
            id="resp",
            model="model",
            content=[
                create_thinking_content("Let me think..."),
                create_text_content("Answer"),
            ],
        )

        transformer.transform_response(response)

        assert len(response.content) == 1
        assert isinstance(response.content[0], TextContent)

    def test_token_limit_transformer_caps(self):
        """Test TokenLimitTransformer caps max_tokens."""
        transformer = TokenLimitTransformer(max_tokens=100, cap_instead_of_reject=True)
        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
            parameters=UnifiedParameters(max_tokens=200),
        )

        transformer.transform_request(request)

        assert request.parameters.max_tokens == 100

    def test_token_limit_transformer_rejects(self):
        """Test TokenLimitTransformer rejects when configured."""
        transformer = TokenLimitTransformer(max_tokens=100, cap_instead_of_reject=False)
        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
            parameters=UnifiedParameters(max_tokens=200),
        )

        with pytest.raises(ValueError, match="exceeds limit"):
            transformer.transform_request(request)

    def test_feature_transformer_chain(self):
        """Test FeatureTransformerChain applies all transformers."""
        chain = FeatureTransformerChain()
        chain.add(ReasoningTransformer(include_thinking=False))
        chain.add(TokenLimitTransformer(max_tokens=100))

        request = UnifiedRequest(
            model="gpt-4",
            messages=[UnifiedMessage.user("Hello")],
            parameters=UnifiedParameters(max_tokens=200),
        )

        chain.transform_request(request)

        assert request.parameters.max_tokens == 100

    def test_feature_transformer_chain_is_empty(self):
        """Test FeatureTransformerChain.is_empty method."""
        chain = FeatureTransformerChain()
        assert chain.is_empty() is True

        chain.add(ReasoningTransformer())
        assert chain.is_empty() is False

    def test_feature_transformer_chain_names(self):
        """Test FeatureTransformerChain.names method."""
        chain = FeatureTransformerChain()
        chain.add(ReasoningTransformer())
        chain.add(TokenLimitTransformer())

        names = chain.names()
        assert "reasoning" in names
        assert "token_limit" in names


# =============================================================================
# Cross-Protocol Integration Tests
# =============================================================================


class TestCrossProtocolIntegration:
    """Integration tests for cross-protocol transformations."""

    def test_openai_to_anthropic_roundtrip(self, pipeline):
        """Test OpenAI → Anthropic → OpenAI roundtrip."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")

        # OpenAI request
        openai_request = {
            "model": "claude-3",
            "messages": [
                {"role": "system", "content": "Be helpful"},
                {"role": "user", "content": "Hello"},
            ],
            "temperature": 0.7,
        }

        # Transform to Anthropic
        anthropic_request = pipeline.transform_request(openai_request, ctx)
        assert anthropic_request["system"] == "Be helpful"
        assert anthropic_request["temperature"] == 0.7

        # Simulate Anthropic response
        anthropic_response = {
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hi there!"}],
            "model": "claude-3",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }

        # Transform back to OpenAI
        openai_response = pipeline.transform_response(anthropic_response, ctx)
        assert openai_response["object"] == "chat.completion"
        assert openai_response["choices"][0]["message"]["content"] == "Hi there!"

    def test_anthropic_to_openai_roundtrip(self, pipeline):
        """Test Anthropic → OpenAI → Anthropic roundtrip."""
        ctx = create_context(Protocol.ANTHROPIC, Protocol.OPENAI, "gpt-4")

        # Anthropic request
        anthropic_request = {
            "model": "gpt-4",
            "max_tokens": 1024,
            "system": "Be helpful",
            "messages": [{"role": "user", "content": "Hello"}],
        }

        # Transform to OpenAI
        openai_request = pipeline.transform_request(anthropic_request, ctx)
        assert openai_request["messages"][0]["role"] == "system"

        # Simulate OpenAI response
        openai_response = {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "gpt-4",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hi!"},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5},
        }

        # Transform back to Anthropic
        anthropic_response = pipeline.transform_response(openai_response, ctx)
        assert anthropic_response["type"] == "message"
        assert anthropic_response["content"][0]["text"] == "Hi!"

    def test_response_api_to_openai_roundtrip(self, pipeline):
        """Test Response API → OpenAI → Response API roundtrip."""
        ctx = create_context(Protocol.RESPONSE_API, Protocol.OPENAI, "gpt-4")

        # Response API request
        response_api_request = {
            "model": "gpt-4",
            "instructions": "Be helpful",
            "input": "Hello",
        }

        # Transform to OpenAI
        openai_request = pipeline.transform_request(response_api_request, ctx)
        assert "messages" in openai_request

        # Simulate OpenAI response
        openai_response = {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "gpt-4",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hi!"},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5},
        }

        # Transform back to Response API
        response_api_response = pipeline.transform_response(openai_response, ctx)
        assert response_api_response["object"] == "response"
        assert response_api_response["status"] == "completed"

    def test_tool_calls_openai_to_anthropic(self, pipeline):
        """Test tool calls transformation from OpenAI to Anthropic."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")

        # OpenAI request with tools
        openai_request = {
            "model": "claude-3",
            "messages": [{"role": "user", "content": "What's the weather?"}],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather",
                        "parameters": {"type": "object"},
                    },
                }
            ],
        }

        # Transform to Anthropic
        anthropic_request = pipeline.transform_request(openai_request, ctx)
        assert "tools" in anthropic_request
        assert anthropic_request["tools"][0]["name"] == "get_weather"

    def test_special_characters_preserved(self, pipeline):
        """Test special characters are preserved through transformation."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")

        special_content = "Hello! 你好! مرحبا! 🎉 <script>alert('test')</script>"

        openai_request = {
            "model": "claude-3",
            "messages": [{"role": "user", "content": special_content}],
        }

        anthropic_request = pipeline.transform_request(openai_request, ctx)

        # Content should be preserved
        msg_content = anthropic_request["messages"][0]["content"]
        if isinstance(msg_content, str):
            assert msg_content == special_content
        else:
            # Content blocks format
            assert msg_content[0]["text"] == special_content

    def test_empty_content_handling(self, pipeline):
        """Test empty content is handled gracefully."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")

        openai_request = {
            "model": "claude-3",
            "messages": [{"role": "user", "content": ""}],
        }

        # Should not raise
        anthropic_request = pipeline.transform_request(openai_request, ctx)
        assert "messages" in anthropic_request


# =============================================================================
# Edge Case Tests
# =============================================================================


class TestEdgeCases:
    """Tests for edge cases and error handling."""

    def test_missing_model_field(self, openai_transformer):
        """Test handling of missing model field."""
        request = {
            "messages": [{"role": "user", "content": "Hello"}],
        }
        unified = openai_transformer.transform_request_out(request)
        assert unified.model == ""

    def test_empty_messages_array(self, openai_transformer):
        """Test handling of empty messages array."""
        request = {
            "model": "gpt-4",
            "messages": [],
        }
        unified = openai_transformer.transform_request_out(request)
        assert len(unified.messages) == 0

    def test_null_content_in_message(self, openai_transformer):
        """Test handling of null content in message."""
        request = {
            "model": "gpt-4",
            "messages": [
                {"role": "user", "content": "Hello"},
                {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {"name": "test", "arguments": "{}"},
                        }
                    ],
                },
            ],
        }
        unified = openai_transformer.transform_request_out(request)
        assert len(unified.messages) == 2

    def test_invalid_json_in_stream_chunk(self, openai_transformer):
        """Test handling of invalid JSON in stream chunk."""
        chunk = b"data: {invalid json}\n\n"
        chunks = openai_transformer.transform_stream_chunk_in(chunk)
        # Should not raise, just return empty
        assert chunks == []

    def test_empty_stream_chunk(self, openai_transformer):
        """Test handling of empty stream chunk."""
        chunk = b""
        chunks = openai_transformer.transform_stream_chunk_in(chunk)
        assert chunks == []

    def test_stream_chunk_with_comments(self, openai_transformer):
        """Test handling of SSE comments in stream."""
        chunk = b': this is a comment\ndata: {"id":"123","choices":[{"delta":{"content":"Hi"}}]}\n\n'
        chunks = openai_transformer.transform_stream_chunk_in(chunk)
        # Should skip comment and parse data
        assert len(chunks) >= 1

    def test_large_content_handling(self, pipeline):
        """Test handling of large content."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")

        # 10KB content
        large_content = "x" * 10000

        openai_request = {
            "model": "claude-3",
            "messages": [{"role": "user", "content": large_content}],
        }

        # Should not raise
        anthropic_request = pipeline.transform_request(openai_request, ctx)
        assert "messages" in anthropic_request


# =============================================================================
# Stop Reason Conversion Tests
# =============================================================================


class TestStopReasonConversion:
    """Tests for stop reason conversion between protocols."""

    def test_openai_stop_to_anthropic_end_turn(self, pipeline):
        """Test OpenAI 'stop' converts to Anthropic 'end_turn'."""
        ctx = create_context(Protocol.ANTHROPIC, Protocol.OPENAI, "gpt-4")

        openai_response = {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "gpt-4",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Done"},
                    "finish_reason": "stop",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5},
        }

        anthropic_response = pipeline.transform_response(openai_response, ctx)
        assert anthropic_response["stop_reason"] == "end_turn"

    def test_openai_length_to_anthropic_max_tokens(self, pipeline):
        """Test OpenAI 'length' converts to Anthropic 'max_tokens'."""
        ctx = create_context(Protocol.ANTHROPIC, Protocol.OPENAI, "gpt-4")

        openai_response = {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "gpt-4",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Truncated..."},
                    "finish_reason": "length",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 100},
        }

        anthropic_response = pipeline.transform_response(openai_response, ctx)
        assert anthropic_response["stop_reason"] == "max_tokens"

    def test_anthropic_end_turn_to_openai_stop(self, pipeline):
        """Test Anthropic 'end_turn' converts to OpenAI 'stop'."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")

        anthropic_response = {
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Done"}],
            "model": "claude-3",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }

        openai_response = pipeline.transform_response(anthropic_response, ctx)
        assert openai_response["choices"][0]["finish_reason"] == "stop"

    def test_anthropic_tool_use_to_openai_tool_calls(self, pipeline):
        """Test Anthropic 'tool_use' converts to OpenAI 'tool_calls'."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")

        anthropic_response = {
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "tool_use",
                    "id": "tool_123",
                    "name": "get_weather",
                    "input": {"location": "Tokyo"},
                }
            ],
            "model": "claude-3",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 10, "output_tokens": 15},
        }

        openai_response = pipeline.transform_response(anthropic_response, ctx)
        assert openai_response["choices"][0]["finish_reason"] == "tool_calls"


# =============================================================================
# Usage Statistics Conversion Tests
# =============================================================================


class TestUsageConversion:
    """Tests for usage statistics conversion between protocols."""

    def test_openai_to_anthropic_usage(self, pipeline):
        """Test OpenAI usage converts to Anthropic format."""
        ctx = create_context(Protocol.ANTHROPIC, Protocol.OPENAI, "gpt-4")

        openai_response = {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "model": "gpt-4",
            "choices": [
                {
                    "message": {"role": "assistant", "content": "Hello"},
                    "finish_reason": "stop",
                }
            ],
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150,
            },
        }

        anthropic_response = pipeline.transform_response(openai_response, ctx)
        assert anthropic_response["usage"]["input_tokens"] == 100
        assert anthropic_response["usage"]["output_tokens"] == 50

    def test_anthropic_to_openai_usage(self, pipeline):
        """Test Anthropic usage converts to OpenAI format."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-3")

        anthropic_response = {
            "id": "msg_123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello"}],
            "model": "claude-3",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 100, "output_tokens": 50},
        }

        openai_response = pipeline.transform_response(anthropic_response, ctx)
        assert openai_response["usage"]["prompt_tokens"] == 100
        assert openai_response["usage"]["completion_tokens"] == 50
        assert openai_response["usage"]["total_tokens"] == 150
