"""Streaming response utilities"""

import asyncio
import base64
import io
import json
import os
import struct
import time
from datetime import datetime, timezone
from typing import TYPE_CHECKING, AsyncIterator, Optional, Dict, Any, Callable, Mapping

import httpx
import tiktoken
from fastapi.responses import StreamingResponse

from app.core.config import get_config
from app.core.exceptions import TTFTTimeoutError
from app.core.metrics import TOKEN_USAGE, TTFT, TOKENS_PER_SECOND
from app.core.stream_metrics import StreamStats, record_stream_metrics
from app.core.logging import (
    set_provider_context,
    clear_provider_context,
    get_logger,
    get_api_key_name,
)
from app.core.token_counter import OutboundTokenCounter
from app.core.tokenizer import count_tokens as tokenizer_count_tokens
from app.utils.gemini3 import log_gemini_response_signatures, normalize_gemini3_response

if TYPE_CHECKING:
    from app.services.langfuse_service import GenerationData

DEFAULT_IMAGE_TOKEN_COUNT = int(os.getenv("DEFAULT_IMAGE_TOKEN_COUNT", 250))
DEFAULT_IMAGE_WIDTH = int(os.getenv("DEFAULT_IMAGE_WIDTH", 300))
DEFAULT_IMAGE_HEIGHT = int(os.getenv("DEFAULT_IMAGE_HEIGHT", 300))
MAX_SHORT_SIDE_FOR_IMAGE_HIGH_RES = int(
    os.getenv("MAX_SHORT_SIDE_FOR_IMAGE_HIGH_RES", 768)
)
MAX_LONG_SIDE_FOR_IMAGE_HIGH_RES = int(
    os.getenv("MAX_LONG_SIDE_FOR_IMAGE_HIGH_RES", 2000)
)
MAX_TILE_WIDTH = int(os.getenv("MAX_TILE_WIDTH", 512))
MAX_TILE_HEIGHT = int(os.getenv("MAX_TILE_HEIGHT", 512))

TokenCounterFunction = Callable[[str], int]


def _fix_model_name(model: str) -> str:
    if "gpt-35" in model:
        return model.replace("-35", "-3.5")
    if model.startswith("gpt-"):
        return model
    return "gpt-3.5-turbo"


def _get_count_function(model: Optional[str]) -> TokenCounterFunction:
    model_to_use = _fix_model_name(model or "")
    try:
        if "gpt-4o" in model_to_use:
            encoding = tiktoken.get_encoding("o200k_base")
        else:
            encoding = tiktoken.encoding_for_model(model_to_use)
    except KeyError:
        encoding = tiktoken.get_encoding("cl100k_base")

    def count(text: str) -> int:
        return len(encoding.encode(text, disallowed_special=()))

    return count


def count_tokens(text: str, model: str) -> int:
    """
    Count tokens for the given text.

    This function uses the new unified tokenizer module which supports
    both tiktoken (OpenAI) and HuggingFace (Claude) tokenizers.
    """
    return tokenizer_count_tokens(text, model)


def calculate_tools_tokens(tools: list, model: str) -> int:
    if not tools:
        return 0
    tools_str = _format_function_definitions(tools)
    return count_tokens(tools_str, model)


def get_image_type(image_data: bytes) -> Optional[str]:
    if image_data[0:8] == b"\x89\x50\x4e\x47\x0d\x0a\x1a\x0a":
        return "png"
    if image_data[0:4] == b"GIF8" and image_data[5:6] == b"a":
        return "gif"
    if image_data[0:3] == b"\xff\xd8\xff":
        return "jpeg"
    if image_data[4:8] == b"ftyp":
        return "heic"
    if image_data[0:4] == b"RIFF" and image_data[8:12] == b"WEBP":
        return "webp"
    return None


def get_image_dimensions(data: str) -> tuple[int, int]:
    img_data: bytes
    try:
        response = httpx.get(data)
        img_data = response.read()
    except Exception:
        _, encoded = data.split(",", 1)
        img_data = base64.b64decode(encoded)

    img_type = get_image_type(img_data)
    if img_type == "png":
        width, height = struct.unpack(">LL", img_data[16:24])
        return width, height
    if img_type == "gif":
        width, height = struct.unpack("<HH", img_data[6:10])
        return width, height
    if img_type == "jpeg":
        with io.BytesIO(img_data) as fhandle:
            fhandle.seek(0)
            size = 2
            ftype = 0
            while not 0xC0 <= ftype <= 0xCF or ftype in (0xC4, 0xC8, 0xCC):
                fhandle.seek(size, 1)
                byte = fhandle.read(1)
                while ord(byte) == 0xFF:
                    byte = fhandle.read(1)
                ftype = ord(byte)
                size = struct.unpack(">H", fhandle.read(2))[0] - 2
            fhandle.seek(1, 1)
            height, width = struct.unpack(">HH", fhandle.read(4))
        return width, height
    if img_type == "webp":
        if img_data[12:16] == b"VP8X":
            width = struct.unpack("<I", img_data[24:27] + b"\x00")[0] + 1
            height = struct.unpack("<I", img_data[27:30] + b"\x00")[0] + 1
            return width, height
        if img_data[12:16] == b"VP8 ":
            width = struct.unpack("<H", img_data[26:28])[0] & 0x3FFF
            height = struct.unpack("<H", img_data[28:30])[0] & 0x3FFF
            return width, height
        if img_data[12:16] == b"VP8L":
            bits = struct.unpack("<I", img_data[21:25])[0]
            width = (bits & 0x3FFF) + 1
            height = ((bits >> 14) & 0x3FFF) + 1
            return width, height

    return DEFAULT_IMAGE_WIDTH, DEFAULT_IMAGE_HEIGHT


def resize_image_high_res(width: int, height: int) -> tuple[int, int]:
    if (
        width <= MAX_SHORT_SIDE_FOR_IMAGE_HIGH_RES
        and height <= MAX_SHORT_SIDE_FOR_IMAGE_HIGH_RES
    ):
        return width, height

    longer_side = max(width, height)
    shorter_side = min(width, height)
    aspect_ratio = longer_side / shorter_side

    if width <= height:
        resized_width = MAX_SHORT_SIDE_FOR_IMAGE_HIGH_RES
        resized_height = int(resized_width * aspect_ratio)
        if resized_height > MAX_LONG_SIDE_FOR_IMAGE_HIGH_RES:
            resized_height = MAX_LONG_SIDE_FOR_IMAGE_HIGH_RES
            resized_width = int(resized_height / aspect_ratio)
    else:
        resized_height = MAX_SHORT_SIDE_FOR_IMAGE_HIGH_RES
        resized_width = int(resized_height * aspect_ratio)
        if resized_width > MAX_LONG_SIDE_FOR_IMAGE_HIGH_RES:
            resized_width = MAX_LONG_SIDE_FOR_IMAGE_HIGH_RES
            resized_height = int(resized_width / aspect_ratio)

    return resized_width, resized_height


def calculate_tiles_needed(
    resized_width: int,
    resized_height: int,
    tile_width: int = MAX_TILE_WIDTH,
    tile_height: int = MAX_TILE_HEIGHT,
) -> int:
    tiles_across = (resized_width + tile_width - 1) // tile_width
    tiles_down = (resized_height + tile_height - 1) // tile_height
    return tiles_across * tiles_down


def calculate_img_tokens(
    data: str,
    mode: str = "auto",
    base_tokens: int = 85,
    use_default_image_token_count: bool = False,
) -> int:
    if use_default_image_token_count:
        return DEFAULT_IMAGE_TOKEN_COUNT
    if mode in ("low", "auto"):
        return base_tokens
    if mode == "high":
        width, height = get_image_dimensions(data=data)
        resized_width, resized_height = resize_image_high_res(
            width=width, height=height
        )
        tiles_needed = calculate_tiles_needed(
            resized_width=resized_width, resized_height=resized_height
        )
        tile_tokens = (base_tokens * 2) * tiles_needed
        return base_tokens + tile_tokens
    raise ValueError(f"Invalid detail value: {mode}")


def _count_image_tokens(
    image_url: Any,
    use_default_image_token_count: bool,
) -> int:
    if isinstance(image_url, dict):
        detail = image_url.get("detail", "auto")
        if detail not in ["low", "high", "auto"]:
            raise ValueError(f"Invalid detail value: {detail}")
        url = image_url.get("url")
        if not url:
            raise ValueError("Missing required key 'url' in image_url dict.")
        return calculate_img_tokens(
            data=url,
            mode=detail,
            use_default_image_token_count=use_default_image_token_count,
        )
    if isinstance(image_url, str):
        if not image_url.strip():
            raise ValueError("Empty image_url string is not valid.")
        return calculate_img_tokens(
            data=image_url,
            mode="auto",
            use_default_image_token_count=use_default_image_token_count,
        )
    raise ValueError("Invalid image_url type")


def calculate_image_tokens(
    image_url: str, detail: str = "auto", use_default_image_token_count: bool = False
) -> int:
    if use_default_image_token_count:
        return DEFAULT_IMAGE_TOKEN_COUNT
    return calculate_img_tokens(image_url, detail, use_default_image_token_count=False)


class _MessageCountParams:
    def __init__(self, model: str):
        actual_model = _fix_model_name(model)
        if actual_model == "gpt-3.5-turbo-0301":
            self.tokens_per_message = 4
            self.tokens_per_name = -1
        else:
            self.tokens_per_message = 3
            self.tokens_per_name = 1
        self.count_function = _get_count_function(model)


def _count_messages(
    params: _MessageCountParams,
    messages: list,
    use_default_image_token_count: bool,
    default_token_count: Optional[int],
) -> int:
    num_tokens = 0
    if len(messages) == 0:
        return num_tokens
    for message in messages:
        num_tokens += params.tokens_per_message
        for key, value in message.items():
            if value is None:
                continue
            if key == "tool_calls":
                if isinstance(value, list):
                    for tool_call in value:
                        if "function" in tool_call:
                            function_arguments = tool_call["function"].get(
                                "arguments", []
                            )
                            num_tokens += params.count_function(str(function_arguments))
                        else:
                            if default_token_count is not None:
                                return default_token_count
                            raise ValueError("Unsupported tool call: missing function")
                else:
                    if default_token_count is not None:
                        return default_token_count
                    raise ValueError("Unsupported type for tool_calls")
            elif isinstance(value, str):
                num_tokens += params.count_function(value)
                if key == "name":
                    num_tokens += params.tokens_per_name
            elif key == "content" and isinstance(value, list):
                num_tokens += _count_content_list(
                    params.count_function,
                    value,
                    use_default_image_token_count,
                    default_token_count,
                )
            else:
                continue
    return num_tokens


def _count_extra(
    count_function: TokenCounterFunction,
    tools: Optional[list],
    tool_choice: Optional[Any],
    includes_system_message: bool,
) -> int:
    num_tokens = 3
    if tools:
        num_tokens += count_function(_format_function_definitions(tools))
        num_tokens += 9
    if tools and includes_system_message:
        num_tokens -= 4
    if tool_choice == "none":
        num_tokens += 1
    elif isinstance(tool_choice, dict):
        num_tokens += 7
        num_tokens += count_function(str(tool_choice["function"]["name"]))
    return num_tokens


def _count_anthropic_content(
    content: Mapping[str, Any],
    count_function: TokenCounterFunction,
    use_default_image_token_count: bool,
    default_token_count: Optional[int],
) -> int:
    content_type = content.get("type")
    if not content_type:
        raise ValueError("Anthropic content missing required field: type")
    if content_type == "tool_use":
        fields_to_count = ("name", "input", "caller")
    elif content_type == "tool_result":
        fields_to_count = ("content",)
    else:
        raise ValueError(f"Unknown Anthropic content type: {content_type}")
    tokens = 0
    for field_name in fields_to_count:
        field_value = content.get(field_name)
        if field_value is None:
            continue
        try:
            if isinstance(field_value, str):
                tokens += count_function(field_value)
            elif isinstance(field_value, list):
                tokens += _count_content_list(
                    count_function,
                    field_value,
                    use_default_image_token_count,
                    default_token_count,
                )
            elif isinstance(field_value, dict):
                tokens += count_function(str(field_value))
        except Exception as exc:
            if default_token_count is not None:
                return default_token_count
            raise ValueError(f"Error counting field '{field_name}': {exc}") from exc
    return tokens


def _count_content_list(
    count_function: TokenCounterFunction,
    content_list: list,
    use_default_image_token_count: bool,
    default_token_count: Optional[int],
) -> int:
    try:
        num_tokens = 0
        for item in content_list:
            if isinstance(item, str):
                num_tokens += count_function(item)
            elif item["type"] == "text":
                num_tokens += count_function(item.get("text", ""))
            elif item["type"] == "image_url":
                image_url = item.get("image_url")
                num_tokens += _count_image_tokens(
                    image_url, use_default_image_token_count
                )
            elif item["type"] in ("tool_use", "tool_result"):
                num_tokens += _count_anthropic_content(
                    item,
                    count_function,
                    use_default_image_token_count,
                    default_token_count,
                )
            elif item["type"] == "thinking":
                thinking_text = item.get("thinking", "")
                if thinking_text:
                    num_tokens += count_function(thinking_text)
            else:
                raise ValueError("Invalid content item type")
        return num_tokens
    except Exception as exc:
        if default_token_count is not None:
            return default_token_count
        raise ValueError(f"Error getting number of tokens: {exc}") from exc


def _format_function_definitions(tools: list) -> str:
    lines = ["namespace functions {", ""]
    for tool in tools:
        function = tool.get("function")
        if function_description := function.get("description"):
            lines.append(f"// {function_description}")
        function_name = function.get("name")
        parameters = function.get("parameters", {})
        properties = parameters.get("properties")
        if properties and properties.keys():
            lines.append(f"type {function_name} = (_: {{")
            lines.append(_format_object_parameters(parameters, 0))
            lines.append("}) => any;")
        else:
            lines.append(f"type {function_name} = () => any;")
        lines.append("")
    lines.append("} // namespace functions")
    return "\n".join(lines)


def _format_object_parameters(parameters: dict, indent: int) -> str:
    properties = parameters.get("properties")
    if not properties:
        return ""
    required_params = parameters.get("required", [])
    lines = []
    for key, props in properties.items():
        description = props.get("description")
        if description:
            lines.append(f"// {description}")
        question = "?"
        if required_params and key in required_params:
            question = ""
        lines.append(f"{key}{question}: {_format_type(props, indent)},")
    return "\n".join([" " * max(0, indent) + line for line in lines])


def _format_type(props: dict, indent: int) -> str:
    if props.get("type") == "string":
        if "enum" in props:
            return " | ".join([f'"{item}"' for item in props["enum"]])
        return "string"
    if props.get("type") == "array":
        return f"{_format_type(props['items'], indent)}[]"
    if props.get("type") == "object":
        return f"{{\n{_format_object_parameters(props, indent + 2)}\n}}"
    if props.get("type") in ["integer", "number"]:
        if "enum" in props:
            return " | ".join([f'"{item}"' for item in props["enum"]])
        return "number"
    if props.get("type") == "boolean":
        return "boolean"
    if props.get("type") == "null":
        return "null"
    return "any"


def calculate_message_tokens(
    messages: list,
    model: str,
    tools: Optional[list] = None,
    tool_choice: Optional[Any] = None,
    use_default_image_token_count: bool = False,
    default_token_count: Optional[int] = None,
    count_response_tokens: bool = False,
) -> int:
    params = _MessageCountParams(model)
    num_tokens = _count_messages(
        params, messages, use_default_image_token_count, default_token_count
    )
    if count_response_tokens is False:
        includes_system_message = any(
            [message.get("role", None) == "system" for message in messages]
        )
        num_tokens += _count_extra(
            params.count_function, tools, tool_choice, includes_system_message
        )
    return num_tokens


def build_messages_for_token_count(messages: list, system: Optional[Any]) -> list:
    combined: list = []
    if system is not None:
        if isinstance(system, str):
            system_content = system
        elif isinstance(system, list):
            system_content = system
        else:
            system_content = ""
        combined.append({"role": "system", "content": system_content})
    combined.extend(messages or [])
    return combined


async def rewrite_sse_chunk(
    chunk: bytes,
    original_model: Optional[str],
    gemini_model: Optional[str] = None,
) -> bytes:
    """Rewrite model field in SSE chunk (compat wrapper)."""
    state = {
        "input_tokens": 0,
        "output_tokens": 0,
        "usage_found": True,
        "usage_chunk_sent": True,
    }
    return await rewrite_sse_chunk_with_usage(
        chunk, original_model, state, gemini_model=gemini_model
    )


async def rewrite_sse_chunk_with_usage(
    chunk: bytes,
    original_model: Optional[str],
    state: Dict[str, Any],
    gemini_model: Optional[str] = None,
) -> bytes:
    """Rewrite model field and inject fallback usage into finish_reason chunk

    Args:
        chunk: SSE chunk bytes
        original_model: Original model name to rewrite
        state: Mutable state dict with keys:
            - input_tokens: int
            - output_tokens: int (accumulated in real-time)
            - usage_found: bool
            - usage_chunk_sent: bool

    Returns:
        Rewritten chunk bytes
    """
    chunk_str = chunk.decode("utf-8", errors="ignore")

    # Early return if nothing to rewrite
    if not original_model and state["usage_found"]:
        return chunk

    # Split by SSE event delimiter (\n\n) to preserve event boundaries
    rewritten_lines = []
    chunk_modified = False
    has_done = False

    for event in chunk_str.split("\n\n"):
        event = event.strip()
        if not event:
            continue

        # Process each line in the event
        event_lines = []
        event_has_data = False

        for line in event.split("\n"):
            line = line.strip()
            if not line:
                continue

            # Check for [DONE] marker
            if line == "data: [DONE]":
                has_done = True
                continue

            if not line.startswith("data: "):
                event_lines.append(line)
                continue

            event_has_data = True
            json_str = line[6:].strip()

            # Skip empty or incomplete JSON
            if not json_str or not json_str.endswith("}"):
                event_lines.append(line)
                continue

            try:
                json_obj = json.loads(json_str)

                # Rewrite model field to original model
                if original_model and "model" in json_obj:
                    json_obj["model"] = original_model
                    chunk_modified = True

                # Check for finish_reason
                has_finish_reason = False
                if "choices" in json_obj:
                    for choice in json_obj["choices"]:
                        if choice.get("finish_reason"):
                            has_finish_reason = True
                            break

                # Inject fallback usage into finish_reason chunk if provider didn't provide it
                if (
                    has_finish_reason
                    and not state["usage_found"]
                    and not state["usage_chunk_sent"]
                    and (state["input_tokens"] > 0 or state["output_tokens"] > 0)
                ):
                    json_obj["usage"] = {
                        "prompt_tokens": state["input_tokens"],
                        "completion_tokens": state["output_tokens"],
                        "total_tokens": state["input_tokens"] + state["output_tokens"],
                    }
                    state["usage_chunk_sent"] = True
                    chunk_modified = True

                # Normalize Gemini 3 tool call signatures (align with LiteLLM handling)
                if normalize_gemini3_response(json_obj, gemini_model):
                    chunk_modified = True

                # Rebuild the line with rewritten JSON
                rewritten_json = json.dumps(json_obj, separators=(", ", ": "))
                event_lines.append(f"data: {rewritten_json}")
            except json.JSONDecodeError:
                event_lines.append(line)

        # Add event to rewritten_lines if it had data
        if event_has_data and event_lines:
            rewritten_lines.append("\n".join(event_lines))

    # Add [DONE] if it was detected
    if has_done:
        rewritten_lines.append("data: [DONE]")
        chunk_modified = True

    # Return rewritten chunk with proper SSE formatting (\n\n between events)
    if chunk_modified:
        result = "\n\n".join(rewritten_lines)
        if result:
            result += "\n\n"
        return result.encode("utf-8")
    else:
        return chunk


async def stream_response(
    response: httpx.Response,
    original_model: Optional[str],
    provider_name: str,
    gemini_model: Optional[str] = None,
    request_data: Optional[Dict[str, Any]] = None,
    ttft_timeout_secs: Optional[int] = None,
    generation_data: Optional["GenerationData"] = None,
    client: str = "unknown",
) -> AsyncIterator[bytes]:
    """Stream response from provider with model rewriting and token tracking

    Args:
        response: HTTP response from provider
        original_model: Original model name from request
        provider_name: Provider name for metrics
        gemini_model: Model name used for Gemini 3 detection
        request_data: Original request data for fallback token counting
        ttft_timeout_secs: Time To First Token timeout in seconds. If None or 0, disabled.
        generation_data: Optional Langfuse generation data for tracing
        client: Client name from User-Agent header
    """
    logger = get_logger()
    api_key_name = get_api_key_name()

    # Import langfuse service here to avoid circular imports
    from app.services.langfuse_service import get_langfuse_service

    langfuse_service = get_langfuse_service()

    # Calculate input tokens for fallback using OutboundTokenCounter
    model_for_counting = original_model or "gpt-3.5-turbo"
    input_tokens = 0
    if request_data:
        messages = request_data.get("messages", [])
        tools = request_data.get("tools")
        tool_choice = request_data.get("tool_choice")
        system = request_data.get("system")
        combined_messages = build_messages_for_token_count(messages, system)
        if combined_messages:
            input_tokens = calculate_message_tokens(
                combined_messages,
                model_for_counting,
                tools=tools,
                tool_choice=tool_choice,
            )
            logger.debug(f"Calculated input tokens: {input_tokens}")

    # Create OutboundTokenCounter for unified token tracking
    token_counter = OutboundTokenCounter(
        model=model_for_counting,
        input_tokens=input_tokens,
    )

    # Track usage status for SSE rewriting
    usage_found = False
    usage_chunk_sent = False

    # Performance tracking
    start_time = time.time()
    provider_first_token_time: Optional[float] = None
    token_count = 0

    # Track finish_reason for Langfuse
    finish_reason: Optional[str] = None

    try:
        # Set provider context for logging
        set_provider_context(provider_name)

        # Use aiter_lines() instead of aiter_bytes() to ensure complete SSE lines
        # This prevents JSON truncation issues caused by TCP packet fragmentation
        response_iter = response.aiter_lines()
        first_chunk_received = False
        sse_buffer = ""  # Buffer for accumulating partial SSE events

        # Apply TTFT timeout for the first chunk if configured
        ttft_enabled = ttft_timeout_secs is not None and ttft_timeout_secs > 0

        while True:
            try:
                if not first_chunk_received and ttft_enabled:
                    try:
                        line = await asyncio.wait_for(
                            response_iter.__anext__(), timeout=float(ttft_timeout_secs)
                        )
                    except asyncio.TimeoutError:
                        raise TTFTTimeoutError(ttft_timeout_secs, provider_name)
                else:
                    line = await response_iter.__anext__()
            except StopAsyncIteration:
                break

            first_chunk_received = True
            now = time.time()

            # Accumulate lines into SSE events (events are separated by blank lines)
            # Each SSE event ends with \n\n, so we buffer until we have complete events
            sse_buffer += line + "\n"

            # Check if we have a complete SSE event (ends with double newline or is a blank line)
            if not line.strip():
                # We have a complete event, process the buffer
                chunk_str = sse_buffer.rstrip("\n")
                sse_buffer = ""
            else:
                # Continue accumulating lines
                continue

            # Convert back to bytes for downstream processing
            chunk = chunk_str.encode("utf-8")

            # Record provider TTFT on first token from provider
            if provider_first_token_time is None:
                provider_first_token_time = now

                # Capture TTFT for Langfuse
                if generation_data:
                    generation_data.ttft_time = datetime.now(timezone.utc)

            # Try to extract usage from provider (preferred method)
            # Only process complete lines to avoid parsing incomplete JSON
            if '"usage":' in chunk_str:
                try:
                    sse_lines = chunk_str.split("\n")
                    for sse_line in sse_lines:
                        # Only process complete SSE data lines
                        if sse_line.startswith("data: ") and sse_line != "data: [DONE]":
                            json_str = sse_line[6:].strip()
                            # Skip if line appears incomplete (no closing brace)
                            if not json_str or not json_str.endswith("}"):
                                continue

                            try:
                                json_obj = json.loads(json_str)
                                # Log Gemini 3 response signatures for debugging
                                log_gemini_response_signatures(json_obj, gemini_model)
                                if "usage" in json_obj and json_obj["usage"]:
                                    usage = json_obj["usage"]

                                    # Update token counter with provider usage
                                    usage_found = True
                                    token_counter.update_provider_usage(usage)

                                    # Capture usage for Langfuse
                                    if generation_data:
                                        generation_data.prompt_tokens = usage.get(
                                            "prompt_tokens", 0
                                        )
                                        generation_data.completion_tokens = usage.get(
                                            "completion_tokens", 0
                                        )
                                        generation_data.total_tokens = usage.get(
                                            "total_tokens", 0
                                        )

                                    logger.debug(
                                        f"Token usage from provider - "
                                        f"model={original_model or 'unknown'} provider={provider_name} "
                                        f"prompt={usage.get('prompt_tokens', 0)} "
                                        f"completion={usage.get('completion_tokens', 0)} "
                                        f"total={usage.get('total_tokens', 0)}"
                                    )
                            except json.JSONDecodeError:
                                # Skip incomplete JSON lines silently
                                pass
                except Exception as e:
                    # Log unexpected errors only
                    logger.debug(f"Error processing usage chunk: {e}")

            # Count tokens in this chunk for TPS calculation and accumulate output
            try:
                sse_lines = chunk_str.split("\n")
                for sse_line in sse_lines:
                    if sse_line.startswith("data: ") and sse_line != "data: [DONE]":
                        json_str = sse_line[6:]
                        json_obj = json.loads(json_str)
                        if "choices" in json_obj:
                            for choice in json_obj["choices"]:
                                delta = choice.get("delta", {})
                                content = delta.get("content", "")
                                if content:
                                    token_count += count_tokens(
                                        content, model_for_counting
                                    )
                                    # Accumulate content in token counter
                                    token_counter.accumulate_content(content)
                                # Capture finish_reason for Langfuse
                                if choice.get("finish_reason"):
                                    finish_reason = choice.get("finish_reason")
            except Exception as e:
                logger.debug(f"Failed to count tokens for TPS: {e}")

            # Get current usage from token counter for SSE rewriting
            current_usage = token_counter.finalize()

            # Create state dict for usage injection
            rewrite_state = {
                "input_tokens": current_usage["prompt_tokens"],
                "output_tokens": current_usage["completion_tokens"],
                "usage_found": usage_found,
                "usage_chunk_sent": usage_chunk_sent,
            }

            # Rewrite chunk with model and inject fallback usage if needed
            rewritten_chunk = await rewrite_sse_chunk_with_usage(
                chunk,
                original_model,
                rewrite_state,
                gemini_model=gemini_model,
            )

            # Update usage_chunk_sent from state
            usage_chunk_sent = rewrite_state["usage_chunk_sent"]

            yield rewritten_chunk

        # Process any remaining content in the SSE buffer
        if sse_buffer.strip():
            chunk_str = sse_buffer.rstrip("\n")
            chunk = chunk_str.encode("utf-8")

            # Get current usage from token counter for SSE rewriting
            current_usage = token_counter.finalize()

            # Create state dict for usage injection
            rewrite_state = {
                "input_tokens": current_usage["prompt_tokens"],
                "output_tokens": current_usage["completion_tokens"],
                "usage_found": usage_found,
                "usage_chunk_sent": usage_chunk_sent,
            }

            # Rewrite chunk with model and inject fallback usage if needed
            rewritten_chunk = await rewrite_sse_chunk_with_usage(
                chunk,
                original_model,
                rewrite_state,
                gemini_model=gemini_model,
            )

            yield rewritten_chunk

        # Finalize token counter and get final usage
        final_usage = token_counter.finalize()
        input_tokens = final_usage["prompt_tokens"]
        output_tokens = final_usage["completion_tokens"]

        # Capture fallback usage for Langfuse if provider didn't provide it
        if not usage_found and generation_data:
            generation_data.prompt_tokens = input_tokens
            generation_data.completion_tokens = output_tokens
            generation_data.total_tokens = input_tokens + output_tokens

        # Record all stream metrics using unified function
        stats = StreamStats(
            model=original_model or "unknown",
            provider=provider_name,
            api_key_name=api_key_name,
            client=client,
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            start_time=start_time,
            first_token_time=provider_first_token_time,
        )
        record_stream_metrics(stats)

        # Finalize Langfuse generation data
        if generation_data:
            generation_data.output_content = token_counter.output_content
            generation_data.finish_reason = finish_reason
            generation_data.end_time = datetime.now(timezone.utc)
            langfuse_service.trace_generation(generation_data)

    except TTFTTimeoutError:
        # Record error in Langfuse
        if generation_data:
            generation_data.is_error = True
            generation_data.error_message = f"TTFT timeout: first token not received within {ttft_timeout_secs} seconds"
            generation_data.end_time = datetime.now(timezone.utc)
            langfuse_service.trace_generation(generation_data)
        raise
    except httpx.RemoteProtocolError as e:
        # Handle connection closed by remote server during streaming
        logger.error(
            f"Remote protocol error during streaming from provider {provider_name}: {str(e)} - "
            f"Provider closed connection unexpectedly"
        )

        # Record error in Langfuse
        if generation_data:
            generation_data.is_error = True
            generation_data.error_message = (
                f"Provider {provider_name} closed connection unexpectedly"
            )
            generation_data.output_content = token_counter.output_content
            generation_data.end_time = datetime.now(timezone.utc)
            langfuse_service.trace_generation(generation_data)

        # Send error event to client using SSE format
        error_message = f"Provider {provider_name} closed connection unexpectedly"
        error_event = json.dumps(
            {
                "error": {
                    "message": error_message,
                    "type": "provider_disconnected",
                    "code": "remote_protocol_error",
                }
            }
        )
        yield f"event: error\ndata: {error_event}\n\n".encode("utf-8")
    except Exception as e:
        # Handle any unexpected errors during streaming
        error_detail = str(e)
        logger.exception(
            f"Unexpected error during streaming from provider {provider_name}"
        )

        # Record error in Langfuse
        if generation_data:
            generation_data.is_error = True
            generation_data.error_message = error_detail
            generation_data.output_content = token_counter.output_content
            generation_data.end_time = datetime.now(timezone.utc)
            langfuse_service.trace_generation(generation_data)

        # Send error event to client using SSE format
        error_event = json.dumps(
            {
                "error": {
                    "message": error_detail,
                    "type": "stream_error",
                    "code": "internal_error",
                }
            }
        )
        yield f"event: error\ndata: {error_event}\n\n".encode("utf-8")
    finally:
        # Clear provider context after streaming completes
        clear_provider_context()


def create_streaming_response(
    response: httpx.Response,
    original_model: Optional[str],
    provider_name: str,
    gemini_model: Optional[str] = None,
    request_data: Optional[Dict[str, Any]] = None,
    ttft_timeout_secs: Optional[int] = None,
    generation_data: Optional["GenerationData"] = None,
    client: str = "unknown",
) -> StreamingResponse:
    """Create streaming response with proper cleanup

    Args:
        response: HTTP response from provider
        original_model: Original model name from request
        provider_name: Provider name for metrics
        gemini_model: Model name used for Gemini 3 detection
        request_data: Original request data for fallback token counting
        ttft_timeout_secs: Time To First Token timeout in seconds. If None or 0, disabled.
        generation_data: Optional Langfuse generation data for tracing
        client: Client name from User-Agent header
    """
    return StreamingResponse(
        stream_response(
            response,
            original_model,
            provider_name,
            gemini_model,
            request_data,
            ttft_timeout_secs,
            generation_data,
            client,
        ),
        media_type="text/event-stream",
    )


def rewrite_model_in_response(
    response_data: dict, original_model: Optional[str]
) -> dict:
    """Rewrite model field in non-streaming response"""
    if original_model and "model" in response_data:
        response_data["model"] = original_model
    return response_data
