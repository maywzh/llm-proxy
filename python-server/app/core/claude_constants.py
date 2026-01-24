"""Claude API constants for better maintainability."""


class ClaudeConstants:
    """Constants for Claude API integration."""

    # Role constants
    ROLE_USER = "user"
    ROLE_ASSISTANT = "assistant"
    ROLE_SYSTEM = "system"
    ROLE_TOOL = "tool"

    # Content type constants
    CONTENT_TEXT = "text"
    CONTENT_IMAGE = "image"
    CONTENT_TOOL_USE = "tool_use"
    CONTENT_TOOL_RESULT = "tool_result"

    # Tool type constants
    TOOL_FUNCTION = "function"

    # Stop reason constants
    STOP_END_TURN = "end_turn"
    STOP_MAX_TOKENS = "max_tokens"
    STOP_STOP_SEQUENCE = "stop_sequence"
    STOP_TOOL_USE = "tool_use"
    STOP_ERROR = "error"
    # Reserved for future Claude API compatibility - not currently used in conversion
    STOP_PAUSE_TURN = "pause_turn"
    STOP_REFUSAL = "refusal"

    # SSE event type constants
    EVENT_MESSAGE_START = "message_start"
    EVENT_MESSAGE_STOP = "message_stop"
    EVENT_MESSAGE_DELTA = "message_delta"
    EVENT_CONTENT_BLOCK_START = "content_block_start"
    EVENT_CONTENT_BLOCK_STOP = "content_block_stop"
    EVENT_CONTENT_BLOCK_DELTA = "content_block_delta"
    EVENT_PING = "ping"

    # Delta type constants
    DELTA_TEXT = "text_delta"
    DELTA_INPUT_JSON = "input_json_delta"
