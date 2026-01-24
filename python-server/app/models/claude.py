"""Claude API Pydantic models for request/response handling."""

from typing import Any, Dict, List, Literal, Optional, Union

from pydantic import BaseModel, Field


class ClaudeContentBlockText(BaseModel):
    """Text content block in Claude messages."""

    type: Literal["text"] = "text"
    text: str


class ClaudeImageSource(BaseModel):
    """Image source for Claude image content blocks."""

    type: Literal["base64"] = "base64"
    media_type: str = Field(description="MIME type of the image (e.g., image/png)")
    data: str = Field(description="Base64-encoded image data")


class ClaudeContentBlockImage(BaseModel):
    """Image content block in Claude messages."""

    type: Literal["image"] = "image"
    source: ClaudeImageSource


class ClaudeContentBlockToolUse(BaseModel):
    """Tool use content block in Claude messages."""

    type: Literal["tool_use"] = "tool_use"
    id: str = Field(description="Unique identifier for this tool use")
    name: str = Field(description="Name of the tool being used")
    input: Dict[str, Any] = Field(description="Input parameters for the tool")


class ClaudeContentBlockToolResult(BaseModel):
    """Tool result content block in Claude messages."""

    type: Literal["tool_result"] = "tool_result"
    tool_use_id: str = Field(
        description="ID of the tool use this result corresponds to"
    )
    content: Union[str, List[Dict[str, Any]], Dict[str, Any]] = Field(
        description="Result content from the tool execution"
    )
    is_error: Optional[bool] = Field(
        default=None, description="Whether the tool execution resulted in an error"
    )


# Union type for all content block types
ClaudeContentBlock = Union[
    ClaudeContentBlockText,
    ClaudeContentBlockImage,
    ClaudeContentBlockToolUse,
    ClaudeContentBlockToolResult,
]


class ClaudeSystemContent(BaseModel):
    """System content block for Claude messages."""

    type: Literal["text"] = "text"
    text: str


class ClaudeMessage(BaseModel):
    """A message in Claude conversation format."""

    role: Literal["user", "assistant"]
    content: Union[str, List[ClaudeContentBlock]]


class ClaudeTool(BaseModel):
    """Tool definition for Claude API."""

    name: str = Field(description="Name of the tool")
    description: Optional[str] = Field(
        default=None, description="Description of what the tool does"
    )
    input_schema: Dict[str, Any] = Field(
        description="JSON Schema for the tool's input parameters"
    )


class ClaudeThinkingConfig(BaseModel):
    """Configuration for Claude's extended thinking feature."""

    type: Literal["enabled", "disabled"] = "enabled"
    budget_tokens: Optional[int] = Field(
        default=None, description="Maximum tokens for thinking (required when enabled)"
    )


class ClaudeMessagesRequest(BaseModel):
    """Request model for Claude Messages API."""

    model: str = Field(description="The model to use for completion")
    max_tokens: int = Field(description="Maximum number of tokens to generate")
    messages: List[ClaudeMessage] = Field(
        description="List of messages in the conversation"
    )
    system: Optional[Union[str, List[ClaudeSystemContent]]] = Field(
        default=None, description="System prompt or instructions"
    )
    stop_sequences: Optional[List[str]] = Field(
        default=None, description="Sequences that will stop generation"
    )
    stream: Optional[bool] = Field(
        default=False, description="Whether to stream the response"
    )
    temperature: Optional[float] = Field(
        default=1.0, ge=0.0, le=1.0, description="Sampling temperature"
    )
    top_p: Optional[float] = Field(
        default=None, ge=0.0, le=1.0, description="Nucleus sampling probability"
    )
    top_k: Optional[int] = Field(
        default=None, ge=0, description="Top-k sampling parameter"
    )
    metadata: Optional[Dict[str, Any]] = Field(
        default=None, description="Optional metadata for the request"
    )
    tools: Optional[List[ClaudeTool]] = Field(
        default=None, description="List of tools available to the model"
    )
    tool_choice: Optional[Dict[str, Any]] = Field(
        default=None, description="How the model should use tools"
    )
    thinking: Optional[ClaudeThinkingConfig] = Field(
        default=None, description="Extended thinking configuration"
    )


class ClaudeUsage(BaseModel):
    """Token usage information from Claude API."""

    input_tokens: int = Field(description="Number of input tokens")
    output_tokens: int = Field(description="Number of output tokens")
    cache_creation_input_tokens: Optional[int] = Field(
        default=None, description="Tokens used for cache creation"
    )
    cache_read_input_tokens: Optional[int] = Field(
        default=None, description="Tokens read from cache"
    )


class ClaudeResponse(BaseModel):
    """Response model from Claude Messages API."""

    id: str = Field(description="Unique identifier for the response")
    type: Literal["message"] = "message"
    role: Literal["assistant"] = "assistant"
    content: List[ClaudeContentBlock] = Field(description="Response content blocks")
    model: str = Field(description="Model used for generation")
    stop_reason: Optional[str] = Field(
        default=None,
        description="Reason generation stopped (end_turn, max_tokens, stop_sequence, tool_use)",
    )
    stop_sequence: Optional[str] = Field(
        default=None, description="The stop sequence that triggered stopping, if any"
    )
    usage: ClaudeUsage = Field(description="Token usage information")


class ClaudeTokenCountRequest(BaseModel):
    """Request model for Claude token counting API."""

    model: str = Field(description="The model to use for token counting")
    messages: List[ClaudeMessage] = Field(description="Messages to count tokens for")
    system: Optional[Union[str, List[ClaudeSystemContent]]] = Field(
        default=None, description="System prompt"
    )
    tools: Optional[List[ClaudeTool]] = Field(
        default=None, description="Tools to include in count"
    )
    thinking: Optional[ClaudeThinkingConfig] = Field(
        default=None, description="Thinking configuration"
    )
    tool_choice: Optional[Dict[str, Any]] = Field(
        default=None, description="Tool choice config"
    )


class ClaudeTokenCountResponse(BaseModel):
    """Response model from Claude token counting API."""

    input_tokens: int = Field(description="Number of input tokens")
