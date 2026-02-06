# Transformer Pipeline Module
#
# This module provides cross-protocol transformation capabilities for the LLM Proxy.
# It enables accepting requests in any supported protocol format (OpenAI, Anthropic, Response API)
# and routing to any provider regardless of their native protocol.

from .unified import (
    # Enums
    Protocol,
    Role,
    StopReason,
    ChunkType,
    # Content types
    TextContent,
    ImageContent,
    ToolUseContent,
    ToolResultContent,
    ThinkingContent,
    FileContent,
    AudioContent,
    RefusalContent,
    ToolInputDeltaContent,
    UnifiedContent,
    # Factory functions
    create_text_content,
    create_tool_use_content,
    create_image_base64,
    create_image_url,
    create_thinking_content,
    create_tool_result_content,
    create_refusal_content,
    # Tool types
    UnifiedToolCall,
    UnifiedTool,
    # Message types
    UnifiedMessage,
    # Parameter types
    UnifiedParameters,
    # Request/Response types
    UnifiedRequest,
    UnifiedUsage,
    UnifiedResponse,
    # Streaming types
    UnifiedStreamChunk,
)

from .base import Transformer, TransformContext

from .registry import TransformerRegistry

from .detector import ProtocolDetector

from .protocols import (
    AnthropicTransformer,
    GcpVertexTransformer,
    OpenAITransformer,
    ResponseApiTransformer,
)

from .pipeline import (
    TransformPipeline,
    FeatureTransformer,
    FeatureTransformerChain,
    ReasoningTransformer,
    TokenLimitTransformer,
)

from .rectifier import sanitize_provider_payload

from .passthrough import (
    PassthroughTransformer,
    should_bypass,
    transform_request_bypass,
    transform_response_bypass,
    transform_stream_chunk_bypass,
)

from .stream import (
    SseEvent,
    SseParser,
    format_sse_event,
    format_sse_data,
    format_sse_done,
    CrossProtocolStreamState,
    StreamAccumulator,
    ChunkAccumulator,
    ToolInfo,
)

__all__ = [
    # Enums
    "Protocol",
    "Role",
    "StopReason",
    "ChunkType",
    # Content types
    "TextContent",
    "ImageContent",
    "ToolUseContent",
    "ToolResultContent",
    "ThinkingContent",
    "FileContent",
    "AudioContent",
    "RefusalContent",
    "ToolInputDeltaContent",
    "UnifiedContent",
    # Factory functions
    "create_text_content",
    "create_tool_use_content",
    "create_image_base64",
    "create_image_url",
    "create_thinking_content",
    "create_tool_result_content",
    "create_refusal_content",
    # Tool types
    "UnifiedToolCall",
    "UnifiedTool",
    # Message types
    "UnifiedMessage",
    # Parameter types
    "UnifiedParameters",
    # Request/Response types
    "UnifiedRequest",
    "UnifiedUsage",
    "UnifiedResponse",
    # Streaming types
    "UnifiedStreamChunk",
    # Base classes
    "Transformer",
    "TransformContext",
    # Registry
    "TransformerRegistry",
    # Detector
    "ProtocolDetector",
    # Protocol Transformers
    "AnthropicTransformer",
    "GcpVertexTransformer",
    "OpenAITransformer",
    "ResponseApiTransformer",
    # Pipeline
    "TransformPipeline",
    "FeatureTransformer",
    "FeatureTransformerChain",
    "ReasoningTransformer",
    "TokenLimitTransformer",
    "sanitize_provider_payload",
    # Passthrough
    "PassthroughTransformer",
    "should_bypass",
    "transform_request_bypass",
    "transform_response_bypass",
    "transform_stream_chunk_bypass",
    # Stream utilities
    "SseEvent",
    "SseParser",
    "format_sse_event",
    "format_sse_data",
    "format_sse_done",
    "CrossProtocolStreamState",
    "StreamAccumulator",
    "ChunkAccumulator",
    "ToolInfo",
]
