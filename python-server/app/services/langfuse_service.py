"""Langfuse tracing service.

This module provides the LangfuseService for creating and managing Langfuse traces.
All Langfuse operations are async and non-blocking to avoid impacting request latency.
"""

import asyncio
import random
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Dict, List, Optional, Tuple, TYPE_CHECKING

from app.core.langfuse_config import LangfuseConfig, get_langfuse_config
from app.core.logging import get_logger

if TYPE_CHECKING:
    from starlette.requests import Request

logger = get_logger()


# ============================================================================
# Helper Functions for Langfuse Tracing
# ============================================================================


def extract_client_metadata(request: "Request") -> Dict[str, str]:
    """Extract client metadata from request headers for Langfuse tracing.

    Args:
        request: The FastAPI/Starlette request object

    Returns:
        Dictionary containing client metadata (user_agent, x_forwarded_for, etc.)
    """
    client_metadata = {}
    user_agent = request.headers.get("user-agent")
    if user_agent:
        client_metadata["user_agent"] = user_agent
    x_forwarded_for = request.headers.get("x-forwarded-for")
    if x_forwarded_for:
        client_metadata["x_forwarded_for"] = x_forwarded_for
    x_real_ip = request.headers.get("x-real-ip")
    if x_real_ip:
        client_metadata["x_real_ip"] = x_real_ip
    origin = request.headers.get("origin")
    if origin:
        client_metadata["origin"] = origin
    referer = request.headers.get("referer")
    if referer:
        client_metadata["referer"] = referer
    return client_metadata


def build_langfuse_tags(
    endpoint: str,
    credential_name: str,
    user_agent: Optional[str] = None,
) -> List[str]:
    """Build tags for Langfuse tracing.

    Args:
        endpoint: The API endpoint (e.g., "chat/completions", "messages")
        credential_name: Name of the credential used
        user_agent: Optional user-agent string (will be truncated to 50 chars)

    Returns:
        List of tags for Langfuse
    """
    tags = [
        f"endpoint:{endpoint}",
        f"credential:{credential_name}",
    ]
    if user_agent and isinstance(user_agent, str):
        # Truncate user-agent for tag (tags should be short)
        ua_short = user_agent[:50] if len(user_agent) > 50 else user_agent
        tags.append(f"user_agent:{ua_short}")
    return tags


def init_langfuse_trace(
    request: "Request",
    endpoint: str,
    generation_name: str = "chat-completion",
) -> Tuple[Optional[str], "GenerationData", "LangfuseService"]:
    """Initialize Langfuse tracing for a request.

    This is a convenience function that extracts client metadata, builds tags,
    creates a trace, and initializes generation data.

    Args:
        request: The FastAPI/Starlette request object
        endpoint: The API endpoint (e.g., "/v1/chat/completions", "/v1/messages")
        generation_name: Name for the generation span

    Returns:
        Tuple of (trace_id, generation_data, langfuse_service)
    """
    langfuse_service = get_langfuse_service()
    request_id = getattr(request.state, "request_id", str(uuid.uuid4()))
    credential_name = getattr(request.state, "credential_name", "anonymous")

    # Extract client metadata from headers
    client_metadata = extract_client_metadata(request)
    user_agent = client_metadata.get("user_agent")

    # Build tags
    # Extract endpoint name for tag (e.g., "/v1/chat/completions" -> "chat/completions")
    endpoint_tag = (
        endpoint.lstrip("/v1/").replace("/", "_")
        if endpoint.startswith("/v1/")
        else endpoint
    )
    tags = build_langfuse_tags(endpoint_tag, credential_name, user_agent)

    # Create trace
    trace_id = langfuse_service.create_trace(
        request_id=request_id,
        credential_name=credential_name,
        endpoint=endpoint,
        tags=tags,
        client_metadata=client_metadata,
    )

    # Initialize generation data
    generation_data = GenerationData(
        trace_id=trace_id or "",
        name=generation_name,
        request_id=request_id,
        credential_name=credential_name,
        endpoint=endpoint,
        start_time=datetime.now(timezone.utc),
    )

    return trace_id, generation_data, langfuse_service


# Langfuse client is imported conditionally to avoid import errors when disabled
_langfuse_client: Optional[Any] = None


@dataclass
class GenerationData:
    """Data collected for a generation span.

    This dataclass holds all the information needed to create a Langfuse generation.
    It is populated during request processing and sent to Langfuse after completion.
    """

    # Identification
    trace_id: str = ""
    generation_id: str = field(default_factory=lambda: str(uuid.uuid4()))
    name: str = "chat-completion"

    # Provider info
    provider_key: str = ""
    provider_type: str = ""
    provider_api_base: str = ""

    # Model info
    original_model: str = ""
    mapped_model: str = ""
    model_parameters: Dict[str, Any] = field(default_factory=dict)

    # Input/Output
    input_messages: List[Dict[str, Any]] = field(default_factory=list)
    output_content: str = ""
    finish_reason: Optional[str] = None

    # Token usage
    prompt_tokens: int = 0
    completion_tokens: int = 0
    total_tokens: int = 0

    # Timing
    start_time: datetime = field(default_factory=lambda: datetime.now(timezone.utc))
    end_time: Optional[datetime] = None
    ttft_time: Optional[datetime] = None  # Time to first token

    # Status
    is_streaming: bool = False
    is_error: bool = False
    error_message: Optional[str] = None

    # Request context
    request_id: str = ""
    credential_name: str = ""
    endpoint: str = ""


class LangfuseService:
    """Service for creating and managing Langfuse traces.

    This service handles all Langfuse operations including:
    - Initializing the Langfuse client
    - Creating traces and generations
    - Sampling logic
    - Graceful shutdown with flush

    All operations are designed to be non-blocking and fail gracefully.
    """

    def __init__(self) -> None:
        """Initialize the Langfuse service."""
        self._client: Optional[Any] = None
        self._config: Optional[LangfuseConfig] = None
        self._initialized: bool = False

    def initialize(self, config: Optional[LangfuseConfig] = None) -> None:
        """Initialize the Langfuse client.

        Args:
            config: Optional LangfuseConfig. If not provided, loads from environment.
        """
        self._config = config or get_langfuse_config()

        if not self._config.enabled:
            logger.info("Langfuse tracing is disabled")
            self._initialized = True
            return

        try:
            from langfuse import Langfuse

            self._client = Langfuse(
                public_key=self._config.public_key,
                secret_key=self._config.secret_key,
                host=self._config.host,
                debug=self._config.debug,
                flush_interval=self._config.flush_interval,
            )
            self._initialized = True
            logger.info(f"Langfuse client initialized, host={self._config.host}")
        except ImportError:
            logger.warning("Langfuse package not installed, tracing disabled")
            self._initialized = True
        except Exception as e:
            logger.error(f"Failed to initialize Langfuse client: {e}")
            self._initialized = True

    @property
    def enabled(self) -> bool:
        """Check if Langfuse is enabled and properly configured."""
        return (
            self._initialized
            and self._client is not None
            and self._config is not None
            and self._config.enabled
        )

    def should_sample(self) -> bool:
        """Determine if this request should be sampled.

        Returns:
            True if the request should be traced, False otherwise.
        """
        if not self.enabled or self._config is None:
            return False
        if self._config.sample_rate >= 1.0:
            return True
        return random.random() < self._config.sample_rate

    def create_trace(
        self,
        request_id: str,
        credential_name: str,
        endpoint: str,
        tags: Optional[List[str]] = None,
        client_metadata: Optional[Dict[str, str]] = None,
    ) -> Optional[str]:
        """Create a new trace and return its ID.

        Args:
            request_id: Internal request ID
            credential_name: Name of the credential used
            endpoint: API endpoint (e.g., "/v1/chat/completions")
            tags: Optional list of tags for filtering
            client_metadata: Optional client metadata (user-agent, x-forwarded-for, etc.)

        Returns:
            Trace ID if created, None if disabled or not sampled
        """
        if not self.enabled or not self.should_sample():
            return None

        trace_id = str(uuid.uuid4())

        try:
            # Build metadata with request info and client metadata
            metadata = {
                "request_id": request_id,
                "credential_name": credential_name,
                "endpoint": endpoint,
            }
            # Add client metadata (user-agent, x-forwarded-for, etc.)
            if client_metadata:
                metadata.update(client_metadata)

            self._client.trace(
                id=trace_id,
                name="llm-proxy-request",
                metadata=metadata,
                tags=tags or [],
                user_id=credential_name,
            )
            logger.debug(f"Created Langfuse trace: {trace_id}")
            return trace_id
        except Exception as e:
            logger.warning(f"Failed to create Langfuse trace: {e}")
            return None

    def update_trace_provider(
        self,
        trace_id: str,
        provider_key: str,
        provider_api_base: str,
        model: str,
    ) -> None:
        """Update trace with provider information.

        This method updates the trace metadata with provider info after provider selection.
        It also adds provider and model as tags.

        Args:
            trace_id: The trace ID to update
            provider_key: The selected provider key
            provider_api_base: The provider's API base URL
            model: The model name
        """
        if not self.enabled or not trace_id:
            return

        try:
            # Update trace with provider info in metadata and tags
            self._client.trace(
                id=trace_id,
                metadata={
                    "provider_key": provider_key,
                    "provider_api_base": provider_api_base,
                    "model": model,
                },
                tags=[
                    f"provider:{provider_key}",
                    f"model:{model}",
                ],
            )
            logger.debug(f"Updated Langfuse trace with provider: {provider_key}")
        except Exception as e:
            logger.warning(f"Failed to update Langfuse trace: {e}")

    def trace_generation(self, data: GenerationData) -> None:
        """Create a generation span with collected data.

        This method is called after a request completes (success or error).
        It creates a generation span in Langfuse with all collected data.

        Args:
            data: GenerationData containing all trace information
        """
        if not self.enabled or not data.trace_id:
            return

        try:
            # Determine level based on error status
            level = "ERROR" if data.is_error else "DEFAULT"

            # Build usage dict
            usage = None
            if data.prompt_tokens > 0 or data.completion_tokens > 0:
                usage = {
                    "prompt_tokens": data.prompt_tokens,
                    "completion_tokens": data.completion_tokens,
                    "total_tokens": data.total_tokens,
                }

            # Build metadata
            metadata = {
                "provider_key": data.provider_key,
                "provider_type": data.provider_type,
                "provider_api_base": data.provider_api_base,
                "mapped_model": data.mapped_model,
                "is_streaming": data.is_streaming,
                "request_id": data.request_id,
            }
            if data.finish_reason:
                metadata["finish_reason"] = data.finish_reason

            # Create generation
            self._client.generation(
                id=data.generation_id,
                trace_id=data.trace_id,
                name=data.name,
                model=data.original_model,
                model_parameters=(
                    data.model_parameters if data.model_parameters else None
                ),
                input=(
                    {"messages": data.input_messages} if data.input_messages else None
                ),
                output=data.output_content if data.output_content else None,
                usage=usage,
                metadata=metadata,
                start_time=data.start_time,
                end_time=data.end_time,
                completion_start_time=data.ttft_time,
                level=level,
                status_message=data.error_message,
            )
            logger.debug(f"Created Langfuse generation: {data.generation_id}")
        except Exception as e:
            logger.warning(f"Failed to create Langfuse generation: {e}")

    async def flush(self) -> None:
        """Flush pending events to Langfuse.

        This method runs the flush operation in a thread pool to avoid blocking.
        """
        if not self._client:
            return

        try:
            loop = asyncio.get_event_loop()
            await loop.run_in_executor(None, self._client.flush)
            logger.debug("Langfuse events flushed")
        except Exception as e:
            logger.warning(f"Failed to flush Langfuse events: {e}")

    def shutdown(self) -> None:
        """Shutdown the client and flush remaining events.

        This method should be called during application shutdown.
        """
        if not self._client:
            return

        try:
            self._client.flush()
            self._client.shutdown()
            logger.info("Langfuse client shutdown complete")
        except Exception as e:
            logger.warning(f"Error during Langfuse shutdown: {e}")


# Global service instance
_langfuse_service: Optional[LangfuseService] = None


def get_langfuse_service() -> LangfuseService:
    """Get the global Langfuse service instance.

    Returns:
        LangfuseService instance (may not be initialized)
    """
    global _langfuse_service
    if _langfuse_service is None:
        _langfuse_service = LangfuseService()
    return _langfuse_service


def init_langfuse_service(config: Optional[LangfuseConfig] = None) -> LangfuseService:
    """Initialize the global Langfuse service.

    Args:
        config: Optional LangfuseConfig. If not provided, loads from environment.

    Returns:
        Initialized LangfuseService instance
    """
    service = get_langfuse_service()
    service.initialize(config)
    return service


def shutdown_langfuse_service() -> None:
    """Shutdown the global Langfuse service."""
    global _langfuse_service
    if _langfuse_service is not None:
        _langfuse_service.shutdown()
        _langfuse_service = None


def reset_langfuse_service() -> None:
    """Reset the global Langfuse service (for testing)."""
    global _langfuse_service
    _langfuse_service = None
