# Unified Internal Format (UIF) Pydantic Models
#
# This module defines the Unified Internal Format (UIF) that serves as the
# lingua franca for all protocol conversions in the transformer pipeline.

from enum import Enum
from typing import Any, Literal, Optional, Union
import uuid

from pydantic import BaseModel, Field


# =============================================================================
# Core Enums
# =============================================================================


class Protocol(str, Enum):
    """Supported LLM API protocols."""

    OPENAI = "openai"
    ANTHROPIC = "anthropic"
    RESPONSE_API = "response_api"
    GCP_VERTEX = "gcp_vertex"
    GEMINI = "gemini"

    @classmethod
    def from_provider_type(cls, provider_type: str) -> "Protocol":
        """Convert provider_type string to Protocol enum."""
        provider_lower = provider_type.lower()
        if provider_lower in ("anthropic", "claude"):
            return cls.ANTHROPIC
        if provider_lower in ("gcp-vertex", "gcp_vertex", "vertex"):
            return cls.GCP_VERTEX
        if provider_lower in ("gemini", "gcp-gemini"):
            return cls.GEMINI
        if provider_lower in ("response_api", "response-api", "responses"):
            return cls.RESPONSE_API
        # OpenAI, Azure, and unknown types default to OpenAI
        return cls.OPENAI


class Role(str, Enum):
    """Unified message role."""

    SYSTEM = "system"
    USER = "user"
    ASSISTANT = "assistant"
    TOOL = "tool"

    @classmethod
    def from_string(cls, role: str) -> "Role":
        """Parse role from string."""
        role_lower = role.lower()
        if role_lower in ("function", "tool"):
            return cls.TOOL
        return cls(role_lower)


class StopReason(str, Enum):
    """Stop reason enum."""

    END_TURN = "end_turn"
    MAX_TOKENS = "max_tokens"
    STOP_SEQUENCE = "stop_sequence"
    TOOL_USE = "tool_use"
    CONTENT_FILTER = "content_filter"
    LENGTH = "length"


class ChunkType(str, Enum):
    """Unified streaming chunk type."""

    MESSAGE_START = "message_start"
    CONTENT_BLOCK_START = "content_block_start"
    CONTENT_BLOCK_DELTA = "content_block_delta"
    CONTENT_BLOCK_STOP = "content_block_stop"
    MESSAGE_DELTA = "message_delta"
    MESSAGE_STOP = "message_stop"
    PING = "ping"


# =============================================================================
# Content Types (Discriminated Union)
# =============================================================================


class TextContent(BaseModel):
    """Plain text content."""

    type: Literal["text"] = "text"
    text: str


class ImageContent(BaseModel):
    """Image content (base64 or URL)."""

    type: Literal["image"] = "image"
    source_type: str  # "base64" | "url"
    media_type: str  # MIME type
    data: str  # base64 data or URL


class ToolUseContent(BaseModel):
    """Tool/function call from assistant."""

    type: Literal["tool_use"] = "tool_use"
    id: str
    name: str
    input: dict[str, Any]


class ToolResultContent(BaseModel):
    """Result from a tool execution."""

    type: Literal["tool_result"] = "tool_result"
    tool_use_id: str
    content: Any
    is_error: bool = False


class ThinkingContent(BaseModel):
    """Thinking/reasoning content."""

    type: Literal["thinking"] = "thinking"
    text: str
    signature: Optional[str] = None


class FileContent(BaseModel):
    """File reference."""

    type: Literal["file"] = "file"
    file_id: str
    filename: Optional[str] = None


class AudioContent(BaseModel):
    """Audio content."""

    type: Literal["audio"] = "audio"
    data: str
    format: str


class RefusalContent(BaseModel):
    """Refusal content."""

    type: Literal["refusal"] = "refusal"
    reason: str


class ToolInputDeltaContent(BaseModel):
    """Tool input JSON delta for streaming."""

    type: Literal["tool_input_delta"] = "tool_input_delta"
    index: int
    partial_json: str


# Union type for all content blocks
UnifiedContent = Union[
    TextContent,
    ImageContent,
    ToolUseContent,
    ToolResultContent,
    ThinkingContent,
    FileContent,
    AudioContent,
    RefusalContent,
    ToolInputDeltaContent,
]


# =============================================================================
# Factory Functions for Content Types
# =============================================================================


def create_text_content(text: str) -> TextContent:
    """Factory function for text content."""
    return TextContent(text=text)


def create_tool_use_content(
    id: str, name: str, input: dict[str, Any]
) -> ToolUseContent:
    """Factory function for tool use content."""
    return ToolUseContent(id=id, name=name, input=input)


def create_image_base64(media_type: str, data: str) -> ImageContent:
    """Factory function for base64 image content."""
    return ImageContent(source_type="base64", media_type=media_type, data=data)


def create_image_url(url: str) -> ImageContent:
    """Factory function for URL image content."""
    return ImageContent(source_type="url", media_type="", data=url)


def create_thinking_content(
    text: str, signature: Optional[str] = None
) -> ThinkingContent:
    """Factory function for thinking content."""
    return ThinkingContent(text=text, signature=signature)


def create_tool_result_content(
    tool_use_id: str, content: Any, is_error: bool = False
) -> ToolResultContent:
    """Factory function for tool result content."""
    return ToolResultContent(
        tool_use_id=tool_use_id, content=content, is_error=is_error
    )


def create_refusal_content(reason: str) -> RefusalContent:
    """Factory function for refusal content."""
    return RefusalContent(reason=reason)


# =============================================================================
# Tool Types
# =============================================================================


class UnifiedToolCall(BaseModel):
    """Unified tool call structure."""

    id: str
    name: str
    arguments: dict[str, Any]


class UnifiedTool(BaseModel):
    """Unified tool definition."""

    name: str
    description: Optional[str] = None
    input_schema: dict[str, Any]
    tool_type: Optional[str] = None  # "function", "computer_use_preview", etc.

    @classmethod
    def function(
        cls, name: str, description: Optional[str], input_schema: dict[str, Any]
    ) -> "UnifiedTool":
        """Create a function tool."""
        return cls(
            name=name,
            description=description,
            input_schema=input_schema,
            tool_type="function",
        )


# =============================================================================
# Message Types
# =============================================================================


class UnifiedMessage(BaseModel):
    """Unified message structure."""

    role: Role
    content: list[UnifiedContent]
    name: Optional[str] = None
    tool_calls: list[UnifiedToolCall] = Field(default_factory=list)
    tool_call_id: Optional[str] = None

    @classmethod
    def user(cls, text: str) -> "UnifiedMessage":
        """Create a user message."""
        return cls(role=Role.USER, content=[create_text_content(text)])

    @classmethod
    def assistant(cls, text: str) -> "UnifiedMessage":
        """Create an assistant message."""
        return cls(role=Role.ASSISTANT, content=[create_text_content(text)])

    @classmethod
    def system(cls, text: str) -> "UnifiedMessage":
        """Create a system message."""
        return cls(role=Role.SYSTEM, content=[create_text_content(text)])

    @classmethod
    def tool_result(
        cls, tool_call_id: str, content: Any, is_error: bool = False
    ) -> "UnifiedMessage":
        """Create a tool result message."""
        return cls(
            role=Role.TOOL,
            content=[
                ToolResultContent(
                    tool_use_id=tool_call_id, content=content, is_error=is_error
                )
            ],
            tool_call_id=tool_call_id,
        )

    @classmethod
    def with_content(
        cls, role: Role, content: list[UnifiedContent]
    ) -> "UnifiedMessage":
        """Create a message with content blocks."""
        return cls(role=role, content=content)

    def text_content(self) -> str:
        """Get concatenated text from all text content blocks."""
        return "".join(c.text for c in self.content if isinstance(c, TextContent))


# =============================================================================
# Parameter Types
# =============================================================================


class UnifiedParameters(BaseModel):
    """Unified model parameters."""

    temperature: Optional[float] = None
    max_tokens: Optional[int] = None
    top_p: Optional[float] = None
    top_k: Optional[int] = None
    stop_sequences: Optional[list[str]] = None
    stream: bool = False
    extra: dict[str, Any] = Field(default_factory=dict)


# =============================================================================
# Request Types
# =============================================================================


class UnifiedRequest(BaseModel):
    """Unified request structure (lingua franca)."""

    model: str
    messages: list[UnifiedMessage]
    system: Optional[str] = None
    parameters: UnifiedParameters = Field(default_factory=UnifiedParameters)
    tools: list[UnifiedTool] = Field(default_factory=list)
    tool_choice: Optional[dict[str, Any]] = None
    request_id: str = Field(default_factory=lambda: str(uuid.uuid4()))
    client_protocol: Protocol = Protocol.OPENAI
    metadata: dict[str, Any] = Field(default_factory=dict)

    def is_streaming(self) -> bool:
        """Check if streaming is enabled."""
        return self.parameters.stream

    def with_system(self, system: str) -> "UnifiedRequest":
        """Set the system prompt."""
        self.system = system
        return self

    def with_stream(self, stream: bool) -> "UnifiedRequest":
        """Set streaming mode."""
        self.parameters.stream = stream
        return self

    def with_max_tokens(self, max_tokens: int) -> "UnifiedRequest":
        """Set max tokens."""
        self.parameters.max_tokens = max_tokens
        return self


# =============================================================================
# Response Types
# =============================================================================


class UnifiedUsage(BaseModel):
    """Unified usage statistics."""

    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: Optional[int] = None
    cache_write_tokens: Optional[int] = None

    def total_tokens(self) -> int:
        """Get total tokens."""
        return self.input_tokens + self.output_tokens


class UnifiedResponse(BaseModel):
    """Unified response structure."""

    id: str
    model: str
    content: list[UnifiedContent]
    stop_reason: Optional[StopReason] = None
    usage: UnifiedUsage = Field(default_factory=UnifiedUsage)
    tool_calls: list[UnifiedToolCall] = Field(default_factory=list)

    @classmethod
    def text(
        cls, id: str, model: str, text: str, usage: UnifiedUsage
    ) -> "UnifiedResponse":
        """Create a simple text response."""
        return cls(
            id=id,
            model=model,
            content=[create_text_content(text)],
            stop_reason=StopReason.END_TURN,
            usage=usage,
        )

    def text_content(self) -> str:
        """Get concatenated text from all text content blocks."""
        return "".join(c.text for c in self.content if isinstance(c, TextContent))


# =============================================================================
# Streaming Types
# =============================================================================


class UnifiedStreamChunk(BaseModel):
    """Unified streaming chunk."""

    chunk_type: ChunkType
    index: int = 0
    delta: Optional[UnifiedContent] = None
    usage: Optional[UnifiedUsage] = None
    stop_reason: Optional[StopReason] = None
    message: Optional[UnifiedResponse] = None
    content_block: Optional[UnifiedContent] = None

    @classmethod
    def message_start(cls, message: UnifiedResponse) -> "UnifiedStreamChunk":
        """Create a message start chunk."""
        return cls(
            chunk_type=ChunkType.MESSAGE_START, message=message, usage=message.usage
        )

    @classmethod
    def content_block_start(
        cls, index: int, content_block: UnifiedContent
    ) -> "UnifiedStreamChunk":
        """Create a content block start chunk."""
        return cls(
            chunk_type=ChunkType.CONTENT_BLOCK_START,
            index=index,
            content_block=content_block,
        )

    @classmethod
    def content_block_delta(
        cls, index: int, delta: UnifiedContent
    ) -> "UnifiedStreamChunk":
        """Create a content block delta chunk."""
        return cls(chunk_type=ChunkType.CONTENT_BLOCK_DELTA, index=index, delta=delta)

    @classmethod
    def content_block_stop(cls, index: int) -> "UnifiedStreamChunk":
        """Create a content block stop chunk."""
        return cls(chunk_type=ChunkType.CONTENT_BLOCK_STOP, index=index)

    @classmethod
    def message_delta(
        cls, stop_reason: StopReason, usage: UnifiedUsage
    ) -> "UnifiedStreamChunk":
        """Create a message delta chunk."""
        return cls(
            chunk_type=ChunkType.MESSAGE_DELTA, stop_reason=stop_reason, usage=usage
        )

    @classmethod
    def message_stop(cls) -> "UnifiedStreamChunk":
        """Create a message stop chunk."""
        return cls(chunk_type=ChunkType.MESSAGE_STOP)

    @classmethod
    def ping(cls) -> "UnifiedStreamChunk":
        """Create a ping chunk."""
        return cls(chunk_type=ChunkType.PING)
