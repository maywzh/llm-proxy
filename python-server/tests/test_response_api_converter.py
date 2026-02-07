"""Tests for Response API converter."""

from app.services.response_api_converter import (
    ResponseApiRequest,
    ResponseApiResponse,
    ResponseUsage,
    openai_to_response_api_response,
    response_api_to_openai_request,
    _map_finish_reason_to_status,
    _convert_input_to_messages,
    _convert_response_content_to_openai,
    _convert_response_tools_to_openai,
)


class TestResponseApiToOpenaiRequest:
    """Tests for response_api_to_openai_request function."""

    def test_basic_text_input(self):
        """Test basic text input conversion."""
        request = ResponseApiRequest(
            model="gpt-4",
            input="Hello",
            instructions="You are helpful",
            max_output_tokens=100,
            temperature=0.7,
        )

        openai_request = response_api_to_openai_request(request)

        assert openai_request["model"] == "gpt-4"
        assert openai_request["max_tokens"] == 100
        assert openai_request["temperature"] == 0.7

        messages = openai_request["messages"]
        assert len(messages) == 2
        assert messages[0]["role"] == "system"
        assert messages[0]["content"] == "You are helpful"
        assert messages[1]["role"] == "user"
        assert messages[1]["content"] == "Hello"

    def test_message_items_input(self):
        """Test message items input conversion."""
        request = ResponseApiRequest(
            model="gpt-4",
            input=[
                {"type": "message", "role": "user", "content": "Hello"},
                {"type": "message", "role": "assistant", "content": "Hi there!"},
                {"type": "message", "role": "user", "content": "How are you?"},
            ],
        )

        openai_request = response_api_to_openai_request(request)

        messages = openai_request["messages"]
        assert len(messages) == 3
        assert messages[0]["content"] == "Hello"
        assert messages[1]["content"] == "Hi there!"
        assert messages[2]["content"] == "How are you?"

    def test_stream_flag(self):
        """Test stream flag is passed through."""
        request = ResponseApiRequest(
            model="gpt-4",
            input="Hello",
            stream=True,
        )

        openai_request = response_api_to_openai_request(request)

        assert openai_request["stream"] is True

    def test_tools_conversion(self):
        """Test tools conversion."""
        request = ResponseApiRequest(
            model="gpt-4",
            input="Hello",
            tools=[
                {
                    "type": "function",
                    "name": "get_weather",
                    "description": "Get weather info",
                    "parameters": {
                        "type": "object",
                        "properties": {"location": {"type": "string"}},
                    },
                }
            ],
        )

        openai_request = response_api_to_openai_request(request)

        assert "tools" in openai_request
        assert len(openai_request["tools"]) == 1
        assert openai_request["tools"][0]["type"] == "function"
        assert openai_request["tools"][0]["function"]["name"] == "get_weather"

    def test_no_input(self):
        """Test request with no input."""
        request = ResponseApiRequest(
            model="gpt-4",
            instructions="You are helpful",
        )

        openai_request = response_api_to_openai_request(request)

        messages = openai_request["messages"]
        assert len(messages) == 1
        assert messages[0]["role"] == "system"


class TestOpenaiToResponseApiResponse:
    """Tests for openai_to_response_api_response function."""

    def test_basic_response(self):
        """Test basic response conversion."""
        openai_response = {
            "id": "chatcmpl-123",
            "object": "chat.completion",
            "created": 1234567890,
            "model": "gpt-4",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "Hello! How can I help?",
                    },
                    "finish_reason": "stop",
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15,
            },
        }

        response = openai_to_response_api_response(openai_response, "gpt-4")

        assert response.model == "gpt-4"
        assert response.status == "completed"
        assert response.usage.input_tokens == 10
        assert response.usage.output_tokens == 5
        assert len(response.output) == 1
        assert response.output[0]["type"] == "message"
        assert response.output[0]["content"][0]["text"] == "Hello! How can I help?"

    def test_tool_calls_response(self):
        """Test response with tool calls."""
        openai_response = {
            "id": "chatcmpl-123",
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
                                    "arguments": '{"location":"NYC"}',
                                },
                            }
                        ],
                    },
                    "finish_reason": "tool_calls",
                }
            ],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5},
        }

        response = openai_to_response_api_response(openai_response, "gpt-4")

        assert response.status == "completed"  # tool_calls maps to completed
        assert len(response.output) == 1
        assert response.output[0]["type"] == "function_call"
        assert response.output[0]["call_id"] == "call_123"
        assert response.output[0]["name"] == "get_weather"

    def test_length_finish_reason(self):
        """Test length finish reason maps to incomplete."""
        openai_response = {
            "id": "chatcmpl-123",
            "choices": [
                {
                    "message": {"content": "Truncated..."},
                    "finish_reason": "length",
                }
            ],
        }

        response = openai_to_response_api_response(openai_response, "gpt-4")

        assert response.status == "incomplete"

    def test_content_filter_finish_reason(self):
        """Test content_filter finish reason maps to failed."""
        openai_response = {
            "id": "chatcmpl-123",
            "choices": [
                {
                    "message": {"content": ""},
                    "finish_reason": "content_filter",
                }
            ],
        }

        response = openai_to_response_api_response(openai_response, "gpt-4")

        assert response.status == "failed"


class TestMapFinishReasonToStatus:
    """Tests for _map_finish_reason_to_status function."""

    def test_stop(self):
        assert _map_finish_reason_to_status("stop") == "completed"

    def test_length(self):
        assert _map_finish_reason_to_status("length") == "incomplete"

    def test_content_filter(self):
        assert _map_finish_reason_to_status("content_filter") == "failed"

    def test_tool_calls(self):
        assert _map_finish_reason_to_status("tool_calls") == "completed"

    def test_unknown(self):
        assert _map_finish_reason_to_status("unknown") == "completed"


class TestConvertInputToMessages:
    """Tests for _convert_input_to_messages function."""

    def test_string_input(self):
        messages = _convert_input_to_messages("Hello")
        assert len(messages) == 1
        assert messages[0]["role"] == "user"
        assert messages[0]["content"] == "Hello"

    def test_message_items(self):
        input_data = [
            {"type": "message", "role": "user", "content": "Hi"},
            {"type": "message", "role": "assistant", "content": "Hello"},
        ]
        messages = _convert_input_to_messages(input_data)
        assert len(messages) == 2
        assert messages[0]["content"] == "Hi"
        assert messages[1]["content"] == "Hello"

    def test_item_reference_skipped(self):
        input_data = [
            {"type": "message", "role": "user", "content": "Hi"},
            {"type": "item_reference", "id": "ref_123"},
        ]
        messages = _convert_input_to_messages(input_data)
        assert len(messages) == 1


class TestConvertResponseContentToOpenai:
    """Tests for _convert_response_content_to_openai function."""

    def test_string_content(self):
        result = _convert_response_content_to_openai("Hello")
        assert result == "Hello"

    def test_none_content(self):
        result = _convert_response_content_to_openai(None)
        assert result == ""

    def test_single_text_part(self):
        result = _convert_response_content_to_openai(
            [{"type": "input_text", "text": "Hello"}]
        )
        assert result == "Hello"

    def test_multiple_parts(self):
        result = _convert_response_content_to_openai(
            [
                {"type": "input_text", "text": "Part 1"},
                {"type": "output_text", "text": "Part 2"},
            ]
        )
        assert isinstance(result, list)
        assert len(result) == 2


class TestConvertResponseToolsToOpenai:
    """Tests for _convert_response_tools_to_openai function."""

    def test_function_tool(self):
        tools = [
            {
                "type": "function",
                "name": "test_func",
                "description": "Test function",
                "parameters": {"type": "object"},
            }
        ]
        result = _convert_response_tools_to_openai(tools)
        assert len(result) == 1
        assert result[0]["type"] == "function"
        assert result[0]["function"]["name"] == "test_func"

    def test_non_function_tool_skipped(self):
        tools = [
            {
                "type": "computer_use_preview",
                "display_width": 1920,
                "display_height": 1080,
            },
            {"type": "web_search_preview"},
        ]
        result = _convert_response_tools_to_openai(tools)
        assert len(result) == 0


class TestResponseUsage:
    """Tests for ResponseUsage dataclass."""

    def test_to_dict(self):
        usage = ResponseUsage(input_tokens=10, output_tokens=5, total_tokens=15)
        result = usage.to_dict()
        assert result == {
            "input_tokens": 10,
            "output_tokens": 5,
            "total_tokens": 15,
        }


class TestResponseApiResponse:
    """Tests for ResponseApiResponse dataclass."""

    def test_to_dict(self):
        response = ResponseApiResponse(
            id="resp_123",
            object="response",
            created_at=1234567890,
            model="gpt-4",
            output=[],
            status="completed",
            status_details=None,
            usage=ResponseUsage(10, 5, 15),
        )
        result = response.to_dict()
        assert result["id"] == "resp_123"
        assert result["object"] == "response"
        assert result["status"] == "completed"
        assert "status_details" not in result  # None should be omitted
