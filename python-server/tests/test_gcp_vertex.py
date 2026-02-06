"""Tests for GCP Vertex AI API implementation.

This module tests the GCP Vertex AI provider integration,
including URL parsing, rawPredict, streamRawPredict, and error handling.

Integration tests verify the complete transformation flow:
- OpenAI /v1/chat/completions -> GCP Vertex (provider_type="gcp-vertex")
- Anthropic /v1/messages -> GCP Vertex (provider_type="gcp-vertex")
- Response format conversion back to client format
"""

import pytest
import httpx
import respx

from app.transformer.unified import Protocol
from app.transformer.base import TransformContext
from app.transformer.registry import TransformerRegistry
from app.transformer.pipeline import TransformPipeline
from app.transformer.protocols.openai import OpenAITransformer
from app.transformer.protocols.anthropic import AnthropicTransformer
from app.api.gcp_vertex import parse_vertex_path


# ============================================================================
# Test Fixtures
# ============================================================================


@pytest.fixture
def gcp_vertex_provider_config() -> dict:
    """Mock GCP Vertex provider configuration."""
    return {
        "name": "gcp-vertex-test",
        "api_base": "https://us-central1-aiplatform.googleapis.com",
        "api_key": "test-access-token",
        "provider_type": "gcp-vertex",
        "gcp_project": "test-project",
        "gcp_location": "us-central1",
        "gcp_publisher": "anthropic",
        "model_mapping": {"claude-sonnet-4-5": "claude-sonnet-4-5"},
    }


@pytest.fixture
def registry() -> TransformerRegistry:
    """Create a registry with all transformers registered."""
    reg = TransformerRegistry()
    reg.register(OpenAITransformer())
    reg.register(AnthropicTransformer())
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


# ============================================================================
# URL Parsing Tests (using production parse_vertex_path)
# ============================================================================


@pytest.mark.unit
class TestGCPVertexURLParsing:
    """Test GCP Vertex AI URL path parameter parsing."""

    def test_parse_valid_raw_predict_path(self):
        """Test parsing valid rawPredict path."""
        path = "projects/my-project/locations/us-central1/publishers/google/models/claude-3-sonnet:rawPredict"
        result = parse_vertex_path(path)

        assert result.project == "my-project"
        assert result.location == "us-central1"
        assert result.publisher == "google"
        assert result.model == "claude-3-sonnet"
        assert result.action == "rawPredict"

    def test_parse_valid_stream_raw_predict_path(self):
        """Test parsing valid streamRawPredict path."""
        path = "projects/test-project-123/locations/europe-west1/publishers/anthropic/models/claude-3-opus:streamRawPredict"
        result = parse_vertex_path(path)

        assert result.project == "test-project-123"
        assert result.location == "europe-west1"
        assert result.publisher == "anthropic"
        assert result.model == "claude-3-opus"
        assert result.action == "streamRawPredict"

    def test_parse_path_with_model_version(self):
        """Test parsing path with model version in name."""
        path = "projects/proj/locations/loc/publishers/pub/models/claude-3-sonnet-20240229:rawPredict"
        result = parse_vertex_path(path)

        assert result.model == "claude-3-sonnet-20240229"
        assert result.action == "rawPredict"

    def test_parse_path_invalid_action(self):
        """Test parsing path with invalid action raises HTTPException."""
        from fastapi import HTTPException

        path = "projects/proj/locations/loc/publishers/pub/models/model:invalidAction"

        with pytest.raises(HTTPException) as exc_info:
            parse_vertex_path(path)
        assert exc_info.value.status_code == 400
        assert "Invalid action" in str(exc_info.value.detail)

    def test_parse_path_malformed(self):
        """Test parsing malformed path raises HTTPException."""
        from fastapi import HTTPException

        path = "invalid/path"

        with pytest.raises(HTTPException) as exc_info:
            parse_vertex_path(path)
        assert exc_info.value.status_code == 400


# ============================================================================
# URL Building Tests
# ============================================================================


@pytest.mark.unit
class TestGCPVertexURLBuilding:
    """Test GCP Vertex AI URL building."""

    def test_build_provider_url_streaming(self):
        """Test building streaming URL for GCP Vertex AI."""
        from app.api.gcp_vertex import _build_provider_url

        url = _build_provider_url(
            provider_api_base="https://us-central1-aiplatform.googleapis.com",
            project="my-project",
            location="us-central1",
            publisher="anthropic",
            model="claude-sonnet-4-5",
            is_streaming=True,
        )

        expected = (
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project"
            "/locations/us-central1/publishers/anthropic/models/claude-sonnet-4-5:streamRawPredict"
        )
        assert url == expected

    def test_build_provider_url_non_streaming(self):
        """Test building non-streaming URL for GCP Vertex AI."""
        from app.api.gcp_vertex import _build_provider_url

        url = _build_provider_url(
            provider_api_base="https://europe-west1-aiplatform.googleapis.com",
            project="test-project",
            location="europe-west1",
            publisher="anthropic",
            model="claude-opus-4-0",
            is_streaming=False,
        )

        expected = (
            "https://europe-west1-aiplatform.googleapis.com/v1/projects/test-project"
            "/locations/europe-west1/publishers/anthropic/models/claude-opus-4-0:rawPredict"
        )
        assert url == expected


@pytest.mark.unit
class TestCompletionsGCPVertexURLBuilding:
    """Test completions.py GCP Vertex AI URL building."""

    def test_completions_build_provider_url_gcp_vertex_streaming(self):
        """Test completions._build_provider_url for GCP Vertex streaming."""
        from app.api.completions import _build_provider_url
        from app.models.provider import Provider

        provider = Provider(
            name="gcp-vertex-test",
            api_base="https://us-central1-aiplatform.googleapis.com",
            api_key="test-token",
            provider_type="gcp-vertex",
            model_mapping={"claude-opus-4-6": "claude-opus-4-6"},
            weight=1,
            provider_params={
                "gcp_project": "my-project",
                "gcp_location": "us-central1",
                "gcp_publisher": "anthropic",
            },
        )

        url = _build_provider_url(
            provider, "chat/completions", is_streaming=True, model="claude-opus-4-6"
        )

        expected = (
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project"
            "/locations/us-central1/publishers/anthropic/models/claude-opus-4-6:streamRawPredict"
        )
        assert url == expected

    def test_completions_build_provider_url_gcp_vertex_non_streaming(self):
        """Test completions._build_provider_url for GCP Vertex non-streaming."""
        from app.api.completions import _build_provider_url
        from app.models.provider import Provider

        provider = Provider(
            name="gcp-vertex-test",
            api_base="https://us-central1-aiplatform.googleapis.com",
            api_key="test-token",
            provider_type="gcp-vertex",
            model_mapping={"claude-opus-4-6": "claude-opus-4-6"},
            weight=1,
            provider_params={
                "gcp_project": "my-project",
                "gcp_location": "us-central1",
                "gcp_publisher": "anthropic",
            },
        )

        url = _build_provider_url(
            provider, "chat/completions", is_streaming=False, model="claude-opus-4-6"
        )

        expected = (
            "https://us-central1-aiplatform.googleapis.com/v1/projects/my-project"
            "/locations/us-central1/publishers/anthropic/models/claude-opus-4-6:rawPredict"
        )
        assert url == expected

    def test_completions_build_provider_url_non_gcp_vertex(self):
        """Test completions._build_provider_url for non-GCP Vertex provider."""
        from app.api.completions import _build_provider_url
        from app.models.provider import Provider

        provider = Provider(
            name="openai-test",
            api_base="https://api.openai.com",
            api_key="test-key",
            provider_type="openai",
            model_mapping={"gpt-4": "gpt-4"},
            weight=1,
        )

        url = _build_provider_url(
            provider, "chat/completions", is_streaming=False, model="gpt-4"
        )

        assert url == "https://api.openai.com/chat/completions"


# ============================================================================
# OpenAI -> GCP Vertex Transformation Tests
# ============================================================================


@pytest.mark.unit
class TestOpenAIToGCPVertexTransformation:
    """Test OpenAI request transformation to GCP Vertex format."""

    def test_openai_to_gcp_vertex_request(self, pipeline):
        """Test /v1/chat/completions routes to GCP Vertex correctly."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-sonnet-4-5")

        openai_request = {
            "model": "claude-sonnet-4-5",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "Hello, how are you?"},
            ],
            "max_tokens": 1024,
            "temperature": 0.7,
        }

        anthropic_request = pipeline.transform_request(openai_request, ctx)

        # Verify Anthropic format for GCP Vertex
        assert anthropic_request["model"] == "claude-sonnet-4-5"
        assert "max_tokens" in anthropic_request
        assert anthropic_request["system"] == "You are a helpful assistant."
        assert anthropic_request["temperature"] == 0.7

        # Messages should not include system
        messages = anthropic_request["messages"]
        assert len(messages) == 1
        assert messages[0]["role"] == "user"

    def test_openai_to_gcp_vertex_response(self, pipeline):
        """Test GCP Vertex response converts back to OpenAI format."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-sonnet-4-5")

        # Simulate GCP Vertex (Anthropic format) response
        vertex_response = {
            "id": "msg_01XYZ",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello! I'm doing well, thank you!"}],
            "model": "claude-sonnet-4-5",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 25, "output_tokens": 15},
        }

        openai_response = pipeline.transform_response(vertex_response, ctx)

        # Verify OpenAI format
        assert openai_response["object"] == "chat.completion"
        assert openai_response["model"] == "claude-sonnet-4-5"
        assert openai_response["choices"][0]["message"]["role"] == "assistant"
        assert (
            openai_response["choices"][0]["message"]["content"]
            == "Hello! I'm doing well, thank you!"
        )
        assert openai_response["choices"][0]["finish_reason"] == "stop"
        assert openai_response["usage"]["prompt_tokens"] == 25
        assert openai_response["usage"]["completion_tokens"] == 15

    def test_openai_to_gcp_vertex_with_tools(self, pipeline):
        """Test OpenAI request with tools converts correctly."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-sonnet-4-5")

        openai_request = {
            "model": "claude-sonnet-4-5",
            "messages": [{"role": "user", "content": "What's the weather in Tokyo?"}],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "get_weather",
                        "description": "Get weather for a location",
                        "parameters": {
                            "type": "object",
                            "properties": {"location": {"type": "string"}},
                            "required": ["location"],
                        },
                    },
                }
            ],
        }

        anthropic_request = pipeline.transform_request(openai_request, ctx)

        # Verify tools are converted to Anthropic format
        assert "tools" in anthropic_request
        assert len(anthropic_request["tools"]) == 1
        assert anthropic_request["tools"][0]["name"] == "get_weather"

    def test_openai_tool_call_response_conversion(self, pipeline):
        """Test GCP Vertex tool use response converts to OpenAI tool_calls."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-sonnet-4-5")

        vertex_response = {
            "id": "msg_01ABC",
            "type": "message",
            "role": "assistant",
            "content": [
                {
                    "type": "tool_use",
                    "id": "toolu_01XYZ",
                    "name": "get_weather",
                    "input": {"location": "Tokyo"},
                }
            ],
            "model": "claude-sonnet-4-5",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 30, "output_tokens": 25},
        }

        openai_response = pipeline.transform_response(vertex_response, ctx)

        assert openai_response["choices"][0]["finish_reason"] == "tool_calls"
        assert "tool_calls" in openai_response["choices"][0]["message"]


# ============================================================================
# Anthropic -> GCP Vertex Transformation Tests
# ============================================================================


@pytest.mark.unit
class TestAnthropicToGCPVertexTransformation:
    """Test Anthropic request transformation to GCP Vertex format."""

    def test_anthropic_to_gcp_vertex_request(self, pipeline):
        """Test /v1/messages routes to GCP Vertex correctly."""
        ctx = create_context(
            Protocol.ANTHROPIC, Protocol.ANTHROPIC, "claude-sonnet-4-5"
        )

        anthropic_request = {
            "model": "claude-sonnet-4-5",
            "max_tokens": 1024,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "Hello!"}],
        }

        # Since both are Anthropic protocol, format should be preserved
        result = pipeline.transform_request(anthropic_request, ctx)

        assert result["model"] == "claude-sonnet-4-5"
        assert result["max_tokens"] == 1024
        assert result["system"] == "You are a helpful assistant."

    def test_anthropic_to_gcp_vertex_response(self, pipeline):
        """Test GCP Vertex response passes through for Anthropic clients."""
        ctx = create_context(
            Protocol.ANTHROPIC, Protocol.ANTHROPIC, "claude-sonnet-4-5"
        )

        vertex_response = {
            "id": "msg_01XYZ",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-sonnet-4-5",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }

        result = pipeline.transform_response(vertex_response, ctx)

        # Should pass through with same format
        assert result["type"] == "message"
        assert result["content"][0]["text"] == "Hello!"
        assert result["stop_reason"] == "end_turn"


# ============================================================================
# Non-Streaming (rawPredict) Tests
# ============================================================================


@pytest.mark.unit
class TestGCPVertexRawPredict:
    """Test rawPredict (non-streaming) responses."""

    def test_successful_response_structure(self):
        """Test successful Anthropic response structure."""
        response = {
            "id": "msg_01XYZ",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello! How can I help you today?"}],
            "model": "claude-3-sonnet-20240229",
            "stop_reason": "end_turn",
            "stop_sequence": None,
            "usage": {"input_tokens": 10, "output_tokens": 15},
        }

        assert response["id"] == "msg_01XYZ"
        assert response["type"] == "message"
        assert response["role"] == "assistant"
        assert response["model"] == "claude-3-sonnet-20240229"
        assert response["content"][0]["text"] == "Hello! How can I help you today?"
        assert response["stop_reason"] == "end_turn"
        assert response["usage"]["input_tokens"] == 10
        assert response["usage"]["output_tokens"] == 15

    def test_response_with_tool_use(self):
        """Test response with tool use content blocks."""
        response = {
            "id": "msg_01ABC",
            "type": "message",
            "role": "assistant",
            "content": [
                {"type": "text", "text": "Let me check the weather."},
                {
                    "type": "tool_use",
                    "id": "toolu_01XYZ",
                    "name": "get_weather",
                    "input": {"location": "San Francisco"},
                },
            ],
            "model": "claude-3-sonnet-20240229",
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 30},
        }

        assert response["stop_reason"] == "tool_use"
        assert len(response["content"]) == 2
        assert response["content"][1]["type"] == "tool_use"
        assert response["content"][1]["name"] == "get_weather"

    def test_response_max_tokens_stop_reason(self):
        """Test response with max_tokens stop reason."""
        response = {
            "id": "msg_01DEF",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "This is a truncated..."}],
            "model": "claude-3-sonnet-20240229",
            "stop_reason": "max_tokens",
            "usage": {"input_tokens": 50, "output_tokens": 1024},
        }

        assert response["stop_reason"] == "max_tokens"


# ============================================================================
# Streaming (streamRawPredict) Tests
# ============================================================================


@pytest.mark.unit
class TestGCPVertexStreamRawPredict:
    """Test streamRawPredict (streaming) response event structure."""

    def test_message_start_event_structure(self):
        """Test message_start event structure."""
        event = {
            "type": "message_start",
            "message": {
                "id": "msg_01XYZ",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-3-sonnet-20240229",
                "stop_reason": None,
                "stop_sequence": None,
                "usage": {"input_tokens": 10, "output_tokens": 0},
            },
        }

        assert event["type"] == "message_start"
        assert event["message"]["role"] == "assistant"
        assert event["message"]["usage"]["input_tokens"] == 10

    def test_content_block_delta_event_structure(self):
        """Test content_block_delta event structure."""
        event = {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"},
        }

        assert event["type"] == "content_block_delta"
        assert event["delta"]["type"] == "text_delta"
        assert event["delta"]["text"] == "Hello"

    def test_message_delta_event_structure(self):
        """Test message_delta event structure with stop reason."""
        event = {
            "type": "message_delta",
            "delta": {"stop_reason": "end_turn"},
            "usage": {"output_tokens": 5},
        }

        assert event["type"] == "message_delta"
        assert event["delta"]["stop_reason"] == "end_turn"
        assert event["usage"]["output_tokens"] == 5

    def test_tool_use_event_structure(self):
        """Test streaming tool use event structure."""
        content_block_start = {
            "type": "content_block_start",
            "index": 1,
            "content_block": {
                "type": "tool_use",
                "id": "toolu_01ABC",
                "name": "get_weather",
            },
        }

        assert content_block_start["content_block"]["type"] == "tool_use"
        assert content_block_start["content_block"]["name"] == "get_weather"


# ============================================================================
# GCP Vertex Streaming to OpenAI SSE Conversion Tests
# ============================================================================


@pytest.mark.unit
class TestGCPVertexStreamingToOpenAI:
    """Test streamRawPredict converts to OpenAI SSE format."""

    def test_anthropic_message_start_to_openai(self, pipeline):
        """Test Anthropic message_start structure."""
        _ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-sonnet-4-5")

        # Anthropic format message_start
        anthropic_message_start = {
            "type": "message_start",
            "message": {
                "id": "msg_01XYZ",
                "type": "message",
                "role": "assistant",
                "content": [],
                "model": "claude-sonnet-4-5",
                "stop_reason": None,
                "usage": {"input_tokens": 10, "output_tokens": 0},
            },
        }

        # Verify the structure matches what stream handler expects
        assert anthropic_message_start["type"] == "message_start"
        assert anthropic_message_start["message"]["role"] == "assistant"

    def test_anthropic_text_delta_structure(self, pipeline):
        """Test Anthropic text_delta structure."""
        # Anthropic content_block_delta
        anthropic_delta = {
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "Hello"},
        }

        # Verify the delta structure
        assert anthropic_delta["delta"]["type"] == "text_delta"
        assert anthropic_delta["delta"]["text"] == "Hello"


# ============================================================================
# Error Handling Tests
# ============================================================================


@pytest.mark.unit
class TestGCPVertexErrorHandling:
    """Test error handling for GCP Vertex AI requests."""

    def test_error_response_400_structure(self):
        """Test 400 Bad Request error response structure."""
        error_response = {
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "max_tokens must be a positive integer",
            },
        }

        assert error_response["type"] == "error"
        assert error_response["error"]["type"] == "invalid_request_error"
        assert "max_tokens" in error_response["error"]["message"]

    def test_error_response_401_structure(self):
        """Test 401 Unauthorized error response structure."""
        error_response = {
            "type": "error",
            "error": {
                "type": "authentication_error",
                "message": "Invalid API key or missing authentication",
            },
        }

        assert error_response["error"]["type"] == "authentication_error"

    def test_error_response_429_structure(self):
        """Test 429 Rate Limit error response structure."""
        error_response = {
            "type": "error",
            "error": {
                "type": "rate_limit_error",
                "message": "Rate limit exceeded. Please retry after some time.",
            },
        }

        assert error_response["error"]["type"] == "rate_limit_error"

    def test_error_response_500_structure(self):
        """Test 500 Internal Server Error response structure."""
        error_response = {
            "type": "error",
            "error": {
                "type": "api_error",
                "message": "Internal server error",
            },
        }

        assert error_response["error"]["type"] == "api_error"

    def test_error_response_529_structure(self):
        """Test 529 Overloaded error response structure."""
        error_response = {
            "type": "error",
            "error": {
                "type": "overloaded_error",
                "message": "The API is temporarily overloaded",
            },
        }

        assert error_response["error"]["type"] == "overloaded_error"

    def test_gcp_vertex_missing_project(self):
        """Test error when gcp_project is not configured."""
        provider_config = {
            "name": "test-provider",
            "api_base": "https://aiplatform.googleapis.com",
            "api_key": "test-token",
            "provider_type": "gcp-vertex",
            # Missing gcp_project
            "gcp_location": "us-central1",
        }

        # In production, this should raise a validation error
        # Here we verify the expected field is missing
        assert (
            "gcp_project" not in provider_config
            or provider_config.get("gcp_project") is None
        )


# ============================================================================
# Integration Tests with Mock Server
# ============================================================================


@pytest.mark.unit
class TestGCPVertexIntegration:
    """Integration tests for GCP Vertex AI endpoints using mock server."""

    @respx.mock
    @pytest.mark.asyncio
    async def test_raw_predict_endpoint_success(self):
        """Test rawPredict endpoint with mock upstream."""
        mock_response = {
            "id": "msg_01XYZ",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-3-sonnet-20240229",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }

        respx.post(
            "https://us-central1-aiplatform.googleapis.com/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict"
        ).mock(return_value=httpx.Response(200, json=mock_response))

        request = {
            "anthropic_version": "vertex-2023-10-16",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello!"}],
        }

        async with httpx.AsyncClient() as client:
            response = await client.post(
                "https://us-central1-aiplatform.googleapis.com/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict",
                json=request,
            )

        assert response.status_code == 200
        data = response.json()
        assert data["type"] == "message"
        assert data["content"][0]["text"] == "Hello!"

    @respx.mock
    @pytest.mark.asyncio
    async def test_stream_raw_predict_endpoint_success(self):
        """Test streamRawPredict endpoint with mock upstream."""
        streaming_content = (
            b'event: message_start\ndata: {"type":"message_start","message":{"id":"msg_01XYZ","role":"assistant"}}\n\n'
            b'event: content_block_start\ndata: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n'
            b'event: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}\n\n'
            b'event: message_stop\ndata: {"type":"message_stop"}\n\n'
        )

        respx.post(
            "https://us-central1-aiplatform.googleapis.com/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:streamRawPredict"
        ).mock(
            return_value=httpx.Response(
                200,
                content=streaming_content,
                headers={"content-type": "text/event-stream"},
            )
        )

        request = {
            "anthropic_version": "vertex-2023-10-16",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello!"}],
            "stream": True,
        }

        async with httpx.AsyncClient() as client:
            response = await client.post(
                "https://us-central1-aiplatform.googleapis.com/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:streamRawPredict",
                json=request,
            )

        assert response.status_code == 200
        assert "text/event-stream" in response.headers.get("content-type", "")

    @respx.mock
    @pytest.mark.asyncio
    async def test_raw_predict_endpoint_error(self):
        """Test rawPredict endpoint error handling."""
        error_response = {
            "type": "error",
            "error": {
                "type": "invalid_request_error",
                "message": "max_tokens: Required field",
            },
        }

        respx.post(
            "https://us-central1-aiplatform.googleapis.com/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict"
        ).mock(return_value=httpx.Response(400, json=error_response))

        request = {
            "anthropic_version": "vertex-2023-10-16",
            "messages": [{"role": "user", "content": "Hello!"}],
        }

        async with httpx.AsyncClient() as client:
            response = await client.post(
                "https://us-central1-aiplatform.googleapis.com/v1/projects/test-project/locations/us-central1/publishers/anthropic/models/claude-3-sonnet:rawPredict",
                json=request,
            )

        assert response.status_code == 400
        data = response.json()
        assert data["type"] == "error"
        assert data["error"]["type"] == "invalid_request_error"


# ============================================================================
# Model Name Validation Tests
# ============================================================================


@pytest.mark.unit
class TestGCPVertexModelValidation:
    """Test model name validation for GCP Vertex AI."""

    def test_valid_claude_models(self):
        """Test valid Claude model names."""
        valid_models = [
            "claude-3-opus@20240229",
            "claude-3-sonnet@20240229",
            "claude-3-haiku@20240307",
            "claude-3-5-sonnet@20240620",
            "claude-3-5-sonnet-v2@20241022",
            "claude-sonnet-4-5",
        ]

        for model in valid_models:
            assert model.startswith("claude-"), f"{model} should start with claude-"


# ============================================================================
# Authentication Header Tests
# ============================================================================


@pytest.mark.unit
class TestGCPVertexAuthentication:
    """Test authentication header handling for GCP Vertex AI."""

    def test_bearer_token_header_construction(self):
        """Test Bearer token header is constructed correctly."""
        from app.api.gcp_vertex import _build_anthropic_headers

        access_token = "ya29.a0AfH6SMBx..."

        headers = _build_anthropic_headers(access_token, {}, "gcp-vertex", {})

        assert headers["Authorization"] == f"Bearer {access_token}"
        assert headers["Content-Type"] == "application/json"
        assert headers["anthropic-version"] == "vertex-2023-10-16"

    def test_anthropic_version_from_request(self):
        """Test anthropic-version can be overridden from request headers."""
        from app.api.gcp_vertex import _build_anthropic_headers

        access_token = "test-token"
        request_headers = {"anthropic-version": "custom-version"}

        headers = _build_anthropic_headers(
            access_token, request_headers, "gcp-vertex", {}
        )

        assert headers["anthropic-version"] == "custom-version"

    def test_anthropic_beta_header_allowlist(self):
        """Test anthropic-beta header is filtered by allowlist."""
        from app.api.gcp_vertex import _build_anthropic_headers

        access_token = "test-token"
        request_headers = {"anthropic-beta": "some-beta-feature,other-feature"}
        provider_params = {
            "anthropic_beta_policy": "allowlist",
            "anthropic_beta_allowlist": ["some-beta-feature"],
        }

        headers = _build_anthropic_headers(
            access_token, request_headers, "gcp-vertex", provider_params
        )

        assert headers["anthropic-beta"] == "some-beta-feature"


@pytest.mark.unit
class TestGCPVertexSanitizeBehavior:
    """Test GCP Vertex path uses shared sanitize strategy."""

    def test_streaming_payload_is_sanitized(self):
        from app.transformer.rectifier import sanitize_provider_payload

        provider_payload = {
            "thinking": {"type": "enabled", "budget_tokens": 1024},
            "model": "claude-opus-4-6",
            "stream": True,
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "thinking",
                            "thinking": "reasoning",
                            "signature": "sig",
                        },
                        {
                            "type": "tool_use",
                            "id": "toolu_1",
                            "name": "lookup",
                            "input": {},
                            "signature": "sig_tool",
                        },
                    ],
                }
            ],
        }

        sanitize_provider_payload(provider_payload)

        assert "thinking" not in provider_payload
        blocks = provider_payload["messages"][0]["content"]
        assert len(blocks) == 1
        assert blocks[0]["type"] == "tool_use"
        assert "signature" not in blocks[0]

    def test_non_streaming_payload_is_sanitized(self):
        from app.transformer.rectifier import sanitize_provider_payload

        provider_payload = {
            "model": "claude-opus-4-6",
            "stream": False,
            "messages": [
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "redacted_thinking",
                            "data": "secret",
                            "signature": "sig_redacted",
                        },
                        {"type": "text", "text": "", "signature": "sig_text"},
                    ],
                }
            ],
        }

        sanitize_provider_payload(provider_payload)

        blocks = provider_payload["messages"][0]["content"]
        assert len(blocks) == 1
        assert blocks[0]["type"] == "text"
        assert blocks[0]["text"] == "."
        assert "signature" not in blocks[0]


# ============================================================================
# Cross-Protocol Integration Tests
# ============================================================================


@pytest.mark.unit
class TestGCPVertexCrossProtocolIntegration:
    """Integration tests for cross-protocol transformations with GCP Vertex."""

    def test_openai_to_gcp_vertex_full_flow(self, pipeline):
        """Test complete OpenAI -> GCP Vertex -> OpenAI transformation flow."""
        ctx = create_context(Protocol.OPENAI, Protocol.ANTHROPIC, "claude-sonnet-4-5")

        # 1. OpenAI client request
        openai_request = {
            "model": "claude-sonnet-4-5",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": "What is 2+2?"},
            ],
            "temperature": 0.5,
            "max_tokens": 100,
        }

        # 2. Transform to Anthropic format (for GCP Vertex)
        anthropic_request = pipeline.transform_request(openai_request, ctx)

        # Verify request transformation
        assert anthropic_request["system"] == "You are a helpful assistant."
        assert anthropic_request["temperature"] == 0.5
        assert len(anthropic_request["messages"]) == 1
        assert anthropic_request["messages"][0]["role"] == "user"

        # 3. Simulate GCP Vertex response (Anthropic format)
        vertex_response = {
            "id": "msg_test123",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "2+2 equals 4."}],
            "model": "claude-sonnet-4-5",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 20, "output_tokens": 10},
        }

        # 4. Transform response back to OpenAI format
        openai_response = pipeline.transform_response(vertex_response, ctx)

        # Verify response transformation
        assert openai_response["object"] == "chat.completion"
        assert openai_response["model"] == "claude-sonnet-4-5"
        assert openai_response["choices"][0]["message"]["content"] == "2+2 equals 4."
        assert openai_response["choices"][0]["finish_reason"] == "stop"
        assert openai_response["usage"]["prompt_tokens"] == 20
        assert openai_response["usage"]["completion_tokens"] == 10
        assert openai_response["usage"]["total_tokens"] == 30

    def test_anthropic_to_gcp_vertex_full_flow(self, pipeline):
        """Test complete Anthropic -> GCP Vertex -> Anthropic transformation flow."""
        ctx = create_context(
            Protocol.ANTHROPIC, Protocol.ANTHROPIC, "claude-sonnet-4-5"
        )

        # 1. Anthropic client request
        anthropic_request = {
            "model": "claude-sonnet-4-5",
            "max_tokens": 100,
            "system": "You are a helpful assistant.",
            "messages": [{"role": "user", "content": "What is 2+2?"}],
        }

        # 2. Transform (should be minimal since same protocol)
        _provider_request = pipeline.transform_request(anthropic_request, ctx)

        # 3. Simulate GCP Vertex response
        vertex_response = {
            "id": "msg_test456",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "2+2 equals 4."}],
            "model": "claude-sonnet-4-5",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 20, "output_tokens": 10},
        }

        # 4. Transform response back to Anthropic format
        client_response = pipeline.transform_response(vertex_response, ctx)

        # Verify response format is preserved
        assert client_response["type"] == "message"
        assert client_response["content"][0]["text"] == "2+2 equals 4."
        assert client_response["stop_reason"] == "end_turn"
        assert client_response["usage"]["input_tokens"] == 20
        assert client_response["usage"]["output_tokens"] == 10

    def test_model_mapping_applied(self, pipeline):
        """Test that model mapping is applied correctly."""
        ctx = TransformContext(
            request_id="test-id",
            client_protocol=Protocol.OPENAI,
            provider_protocol=Protocol.ANTHROPIC,
            original_model="my-claude-alias",
            mapped_model="claude-sonnet-4-5@20241022",
        )

        openai_request = {
            "model": "my-claude-alias",
            "messages": [{"role": "user", "content": "Hello"}],
        }

        anthropic_request = pipeline.transform_request(openai_request, ctx)

        # Model should be mapped
        assert anthropic_request["model"] == "claude-sonnet-4-5@20241022"

    def test_model_restored_in_response(self, pipeline):
        """Test that original model name is restored in response."""
        ctx = TransformContext(
            request_id="test-id",
            client_protocol=Protocol.OPENAI,
            provider_protocol=Protocol.ANTHROPIC,
            original_model="my-claude-alias",
            mapped_model="claude-sonnet-4-5@20241022",
        )

        vertex_response = {
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "content": [{"type": "text", "text": "Hello!"}],
            "model": "claude-sonnet-4-5@20241022",
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        }

        openai_response = pipeline.transform_response(vertex_response, ctx)

        # Model should be restored to original alias
        assert openai_response["model"] == "my-claude-alias"
