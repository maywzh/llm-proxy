# Streaming Utilities
#
# This module provides utilities for handling SSE (Server-Sent Events) streams
# and converting between different streaming formats.

from dataclasses import dataclass, field
from typing import Optional
import uuid

from .unified import (
    ChunkType,
    StopReason,
    TextContent,
    ThinkingContent,
    ToolInputDeltaContent,
    ToolUseContent,
    UnifiedContent,
    UnifiedResponse,
    UnifiedStreamChunk,
    UnifiedUsage,
    create_text_content,
    create_thinking_content,
    create_tool_use_content,
)


# =============================================================================
# Tool Info Cache
# =============================================================================


@dataclass
class ToolInfo:
    """
    Cached tool information for content block synthesis.

    When transforming OpenAI streams to Anthropic format, we need to synthesize
    content_block_start events. This cache stores tool information (id, name)
    from earlier chunks so we can create accurate tool_use content blocks
    instead of using placeholder values like "unknown_tool".
    """

    id: str
    name: str


# =============================================================================
# SSE Event Types
# =============================================================================


@dataclass
class SseEvent:
    """SSE event parsed from stream."""

    event: Optional[str] = None
    data: Optional[str] = None
    id: Optional[str] = None
    retry: Optional[int] = None


# =============================================================================
# SSE Parser
# =============================================================================


class SseParser:
    """SSE parser state."""

    def __init__(self) -> None:
        self._buffer = ""

    def parse(self, chunk: bytes) -> list[SseEvent]:
        """Parse incoming bytes and return complete events."""
        try:
            chunk_str = chunk.decode("utf-8")
        except UnicodeDecodeError:
            return []

        self._buffer += chunk_str
        events: list[SseEvent] = []
        current_event = SseEvent()

        # Split by double newlines (event boundaries)
        while "\n\n" in self._buffer:
            pos = self._buffer.index("\n\n")
            event_block = self._buffer[:pos]
            self._buffer = self._buffer[pos + 2 :]

            for line in event_block.split("\n"):
                if not line or line.startswith(":"):
                    continue

                if ":" in line:
                    field, _, value = line.partition(":")
                    value = value.lstrip(" ")

                    if field == "event":
                        current_event.event = value
                    elif field == "data":
                        if current_event.data is not None:
                            current_event.data += "\n" + value
                        else:
                            current_event.data = value
                    elif field == "id":
                        current_event.id = value
                    elif field == "retry":
                        try:
                            current_event.retry = int(value)
                        except ValueError:
                            pass

            if current_event.data is not None or current_event.event is not None:
                events.append(current_event)
                current_event = SseEvent()

        return events

    def remaining(self) -> str:
        """Get remaining buffer content."""
        return self._buffer

    def clear(self) -> None:
        """Clear the buffer."""
        self._buffer = ""


# =============================================================================
# SSE Serializer Functions
# =============================================================================


def format_sse_event(event: Optional[str], data: str) -> str:
    """Format an SSE event for transmission."""
    output = ""
    if event:
        output += f"event: {event}\n"
    for line in data.split("\n"):
        output += f"data: {line}\n"
    output += "\n"
    return output


def format_sse_data(data: str) -> str:
    """Format a simple data-only SSE event."""
    return f"data: {data}\n\n"


def format_sse_done() -> str:
    """Format the SSE done marker."""
    return "data: [DONE]\n\n"


# =============================================================================
# Cross-Protocol Stream State
# =============================================================================


@dataclass
class CrossProtocolStreamState:
    """
    State tracker for cross-protocol streaming transformation.

    When transforming streams between protocols (e.g., OpenAI → Anthropic),
    we need to track state to properly emit all required events in the target
    protocol's format.

    For example, Anthropic requires:
    - `message_start` at the beginning
    - `content_block_start` before each content block
    - `content_block_stop` after each content block
    - `message_delta` with stop_reason
    - `message_stop` at the end

    OpenAI doesn't have these events, so we need to synthesize them.
    """

    message_started: bool = False
    ping_emitted: bool = False
    current_block_index: int = 0
    started_blocks: set[int] = field(default_factory=set)
    stopped_blocks: set[int] = field(default_factory=set)
    message_delta_emitted: bool = False
    message_stopped: bool = False
    model: str = ""
    message_id: str = field(default_factory=lambda: f"msg_{uuid.uuid4().hex[:24]}")
    usage: Optional[UnifiedUsage] = None
    stop_reason: Optional[StopReason] = None
    # Cache tool info (id, name) by block index for accurate content_block_start synthesis
    tool_info_cache: dict[int, ToolInfo] = field(default_factory=dict)
    # Input tokens for usage tracking (pre-calculated)
    input_tokens: int = 0

    def __post_init__(self):
        """Initialize usage with input tokens if provided."""
        if self.input_tokens > 0 and self.usage is None:
            self.usage = UnifiedUsage(
                input_tokens=self.input_tokens,
                output_tokens=0,
            )

    def accumulate_output_tokens(self, text: str) -> None:
        """
        Accumulate output tokens from chunk text.

        This method counts tokens in generated text and adds them to the usage.
        Used for fallback usage calculation when provider doesn't provide usage.
        """
        from app.utils.streaming import count_tokens

        tokens = count_tokens(text, self.model or "gpt-4")

        if self.usage is None:
            self.usage = UnifiedUsage(
                input_tokens=self.input_tokens,
                output_tokens=tokens,
            )
        else:
            self.usage.output_tokens += tokens

    def get_final_usage(
        self, provider_usage: Optional[UnifiedUsage] = None
    ) -> Optional[UnifiedUsage]:
        """
        Get final usage, prioritizing provider usage over calculated usage.

        Args:
            provider_usage: Usage information from the provider (if available)

        Returns:
            Final usage information (provider usage takes precedence)
        """
        if provider_usage:
            # Provider usage takes priority
            return provider_usage
        # Fall back to accumulated usage
        return self.usage

    def process_chunks(
        self, chunks: list[UnifiedStreamChunk]
    ) -> list[UnifiedStreamChunk]:
        """
        Process unified chunks and emit additional synthetic events as needed.

        This method takes unified chunks from the source protocol and returns
        a complete sequence of chunks that includes all events required by
        the target protocol.
        """
        result: list[UnifiedStreamChunk] = []

        for chunk in chunks:
            # Emit message_start if not yet emitted and we have content
            if not self.message_started and self._should_emit_message_start(chunk):
                result.append(self._create_message_start())
                self.message_started = True
                # Emit ping event after message_start for Anthropic compatibility
                if not self.ping_emitted:
                    result.append(UnifiedStreamChunk.ping())
                    self.ping_emitted = True

            if chunk.chunk_type == ChunkType.MESSAGE_START:
                # Already have a message_start from source, use it
                if not self.message_started:
                    if chunk.message:
                        self.model = chunk.message.model
                        self.message_id = chunk.message.id
                    self.message_started = True
                    result.append(chunk)
                    # Emit ping event after message_start for Anthropic compatibility
                    if not self.ping_emitted:
                        result.append(UnifiedStreamChunk.ping())
                        self.ping_emitted = True

            elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_START:
                index = chunk.index
                if index not in self.started_blocks:
                    self.started_blocks.add(index)
                    self.current_block_index = index
                # Cache tool info for later content_block_start synthesis
                self._cache_tool_info(chunk)
                result.append(chunk)

            elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_DELTA:
                index = chunk.index

                # Emit content_block_start if not yet emitted for this index
                if index not in self.started_blocks:
                    result.append(self._create_content_block_start(index, chunk))
                    self.started_blocks.add(index)
                    self.current_block_index = index

                # Accumulate output tokens from text content
                if chunk.delta and isinstance(chunk.delta, TextContent):
                    if chunk.delta.text:
                        self.accumulate_output_tokens(chunk.delta.text)

                result.append(chunk)

            elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_STOP:
                index = chunk.index
                if index not in self.stopped_blocks:
                    self.stopped_blocks.add(index)
                result.append(chunk)

            elif chunk.chunk_type == ChunkType.MESSAGE_DELTA:
                # Close any open content blocks before message_delta
                for idx in list(self.started_blocks):
                    if idx not in self.stopped_blocks:
                        result.append(UnifiedStreamChunk.content_block_stop(idx))
                        self.stopped_blocks.add(idx)

                # Get final usage (prioritizing provider usage)
                final_usage = self.get_final_usage(chunk.usage)

                # Create a new chunk with final usage
                output_chunk = chunk.model_copy()
                if final_usage:
                    output_chunk.usage = final_usage

                self.message_delta_emitted = True
                result.append(output_chunk)

            elif chunk.chunk_type == ChunkType.MESSAGE_STOP:
                # Ensure all content blocks are closed
                for idx in list(self.started_blocks):
                    if idx not in self.stopped_blocks:
                        result.append(UnifiedStreamChunk.content_block_stop(idx))
                        self.stopped_blocks.add(idx)

                self.message_stopped = True
                result.append(chunk)

            elif chunk.chunk_type == ChunkType.PING:
                result.append(chunk)

        return result

    def _should_emit_message_start(self, chunk: UnifiedStreamChunk) -> bool:
        """Check if we should emit a synthetic message_start."""
        return chunk.chunk_type in (
            ChunkType.CONTENT_BLOCK_START,
            ChunkType.CONTENT_BLOCK_DELTA,
            ChunkType.MESSAGE_DELTA,
        )

    def _create_message_start(self) -> UnifiedStreamChunk:
        """Create a synthetic message_start event."""
        message = UnifiedResponse(
            id=self.message_id,
            model=self.model,
            content=[],
            stop_reason=None,
            usage=UnifiedUsage(),
            tool_calls=[],
        )
        return UnifiedStreamChunk.message_start(message)

    def _create_content_block_start(
        self, index: int, delta_chunk: UnifiedStreamChunk
    ) -> UnifiedStreamChunk:
        """Create a synthetic content_block_start event."""
        # Determine content type from the delta
        content_block: UnifiedContent
        if delta_chunk.delta:
            if isinstance(delta_chunk.delta, TextContent):
                content_block = create_text_content("")
            elif isinstance(delta_chunk.delta, ToolInputDeltaContent):
                # For tool input delta, use cached tool info if available
                cached_info = self.tool_info_cache.get(index)
                if cached_info:
                    content_block = create_tool_use_content(
                        cached_info.id, cached_info.name, {}
                    )
                else:
                    # Fallback to generated id and unknown name
                    content_block = create_tool_use_content(
                        f"toolu_{uuid.uuid4().hex[:24]}", "unknown", {}
                    )
            elif isinstance(delta_chunk.delta, ThinkingContent):
                content_block = create_thinking_content("")
            else:
                content_block = create_text_content("")
        else:
            content_block = create_text_content("")

        return UnifiedStreamChunk.content_block_start(index, content_block)

    def _cache_tool_info(self, chunk: UnifiedStreamChunk) -> None:
        """
        Cache tool information from content_block_start events.

        When we receive a content_block_start with a tool_use content block,
        we cache the tool id and name. This allows us to create accurate
        tool_use content blocks when synthesizing content_block_start events
        from tool_input_delta chunks in cross-protocol transformations.
        """
        if chunk.chunk_type != ChunkType.CONTENT_BLOCK_START:
            return

        if chunk.content_block and isinstance(chunk.content_block, ToolUseContent):
            self.tool_info_cache[chunk.index] = ToolInfo(
                id=chunk.content_block.id,
                name=chunk.content_block.name,
            )

    def finalize(self) -> list[UnifiedStreamChunk]:
        """Finalize the stream, emitting any missing closing events."""
        result: list[UnifiedStreamChunk] = []

        # Close any open content blocks
        for idx in list(self.started_blocks):
            if idx not in self.stopped_blocks:
                result.append(UnifiedStreamChunk.content_block_stop(idx))
                self.stopped_blocks.add(idx)

        # Emit message_delta if not yet emitted
        if not self.message_delta_emitted and self.message_started:
            # Use accumulated usage
            final_usage = self.usage or UnifiedUsage()
            result.append(
                UnifiedStreamChunk.message_delta(StopReason.END_TURN, final_usage)
            )
            self.message_delta_emitted = True

        # Emit message_stop if not yet emitted
        if not self.message_stopped and self.message_started:
            result.append(UnifiedStreamChunk.message_stop())
            self.message_stopped = True

        return result


# =============================================================================
# Stream Accumulator
# =============================================================================


@dataclass
class StreamAccumulator:
    """
    Accumulates streaming chunks for final response assembly.

    This is useful when you need to build a complete response from
    streaming chunks, for example for logging or caching purposes.
    """

    content: list[str] = field(default_factory=list)
    tool_calls: list[dict] = field(default_factory=list)
    usage: Optional[UnifiedUsage] = None
    stop_reason: Optional[StopReason] = None
    message_id: Optional[str] = None
    model: Optional[str] = None

    def add_chunk(self, chunk: UnifiedStreamChunk) -> None:
        """Add a unified stream chunk."""
        if chunk.chunk_type == ChunkType.MESSAGE_START:
            if chunk.message:
                self.message_id = chunk.message.id
                self.model = chunk.message.model

        elif chunk.chunk_type == ChunkType.CONTENT_BLOCK_DELTA:
            if chunk.delta and isinstance(chunk.delta, TextContent):
                self.content.append(chunk.delta.text)

        elif chunk.chunk_type == ChunkType.MESSAGE_DELTA:
            if chunk.usage:
                self.usage = chunk.usage
            if chunk.stop_reason:
                self.stop_reason = chunk.stop_reason

    def text_content(self) -> str:
        """Get accumulated text content."""
        return "".join(self.content)

    def build_response(self) -> UnifiedResponse:
        """Build a unified response from accumulated chunks."""
        return UnifiedResponse(
            id=self.message_id or str(uuid.uuid4()),
            model=self.model or "unknown",
            content=[create_text_content(self.text_content())],
            stop_reason=self.stop_reason,
            usage=self.usage or UnifiedUsage(),
            tool_calls=[],
        )


# =============================================================================
# Chunk Accumulator (Alias for backward compatibility)
# =============================================================================

# Alias for backward compatibility with architecture document
ChunkAccumulator = StreamAccumulator
