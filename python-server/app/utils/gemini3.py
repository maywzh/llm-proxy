"""Gemini 3 thought_signature support utilities.

This module aligns with LiteLLM's Gemini 3 handling by:
- Detecting Gemini 3 models by model name
- Preserving thought_signature in provider_specific_fields
- Embedding thought_signature in tool_call_id for OpenAI client compatibility
- Injecting dummy signatures for Gemini 3 when missing

See: https://ai.google.dev/gemini-api/docs/thought-signatures
"""

from __future__ import annotations

import base64
from typing import Any, Optional

from app.core.logging import get_logger

logger = get_logger()

THOUGHT_SIGNATURE_SEPARATOR = "__thought__"
_DUMMY_THOUGHT_SIGNATURE = base64.b64encode(
    b"skip_thought_signature_validator"
).decode("utf-8")


def is_gemini3_model(model: Optional[str]) -> bool:
    """Check if model is Gemini 3 (not Gemini 1.x or 2.x).

    Args:
        model: Model name to check

    Returns:
        True if provider is Gemini 3, False otherwise
    """
    if not model or not isinstance(model, str):
        return False
    model_lower = model.lower()
    return "gemini-3" in model_lower


def _is_gemini3_flash(model: str) -> bool:
    model_lower = model.lower()
    return "gemini-3-flash" in model_lower


def _get_dummy_thought_signature() -> str:
    """Return the recommended dummy thought_signature for Gemini 3."""
    return _DUMMY_THOUGHT_SIGNATURE


def _map_reasoning_effort_to_thinking_level(
    reasoning_effort: str, model: str
) -> Optional[str]:
    is_flash = _is_gemini3_flash(model)
    effort = reasoning_effort.lower()

    if effort == "minimal":
        return "minimal" if is_flash else "low"
    if effort == "low":
        return "low"
    if effort == "medium":
        return "medium" if is_flash else "high"
    if effort == "high":
        return "high"
    if effort in ("disable", "none"):
        return "minimal" if is_flash else "low"
    return None


def _encode_tool_call_id_with_signature(
    tool_call_id: str, thought_signature: Optional[str]
) -> str:
    """Embed thought signature into tool_call_id for OpenAI client compatibility."""
    if thought_signature:
        return f"{tool_call_id}{THOUGHT_SIGNATURE_SEPARATOR}{thought_signature}"
    return tool_call_id


def _extract_signature_from_id(tool_call_id: Optional[str]) -> Optional[str]:
    if not tool_call_id or THOUGHT_SIGNATURE_SEPARATOR not in tool_call_id:
        return None
    parts = tool_call_id.split(THOUGHT_SIGNATURE_SEPARATOR, 1)
    if len(parts) != 2:
        return None
    return parts[1] or None


def _strip_signature_from_id(tool_call_id: str) -> str:
    if THOUGHT_SIGNATURE_SEPARATOR not in tool_call_id:
        return tool_call_id
    return tool_call_id.split(THOUGHT_SIGNATURE_SEPARATOR, 1)[0]


def _signature_from_extra_content(content: dict[str, Any]) -> Optional[str]:
    extra_content = content.get("extra_content", {})
    if isinstance(extra_content, dict):
        google_content = extra_content.get("google", {})
        if isinstance(google_content, dict):
            thought_sig = google_content.get("thought_signature")
            if isinstance(thought_sig, str) and thought_sig:
                return thought_sig
    return None


def _signature_from_provider_fields(fields: Any) -> Optional[str]:
    if not isinstance(fields, dict):
        return None
    sig = fields.get("thought_signature")
    if isinstance(sig, str) and sig:
        return sig
    return None


def _extract_thought_signature_from_tool_call(
    tool_call: dict[str, Any],
    model: Optional[str],
    allow_dummy: bool,
) -> Optional[str]:
    signature = _signature_from_provider_fields(tool_call.get("provider_specific_fields"))
    if signature:
        return signature

    function = tool_call.get("function")
    if isinstance(function, dict):
        signature = _signature_from_provider_fields(
            function.get("provider_specific_fields")
        )
        if signature:
            return signature

    signature = _signature_from_extra_content(tool_call)
    if signature:
        return signature

    signature = _extract_signature_from_id(tool_call.get("id"))
    if signature:
        return signature

    if allow_dummy and is_gemini3_model(model):
        return _get_dummy_thought_signature()

    return None


def _ensure_provider_specific_fields(target: dict[str, Any]) -> dict[str, Any]:
    provider_fields = target.get("provider_specific_fields")
    if not isinstance(provider_fields, dict):
        provider_fields = {}
        target["provider_specific_fields"] = provider_fields
    return provider_fields


def _check_and_log_signatures(content: dict[str, Any], location: str) -> int:
    """Check for thought_signature in content and log if found.

    Args:
        content: Message or delta content dict
        location: Description of where this content is from (for logging)

    Returns:
        Number of signatures found in tool_calls
    """
    sig_count = 0

    # Check provider_specific_fields at content level
    provider_fields = content.get("provider_specific_fields", {})
    if isinstance(provider_fields, dict):
        thought_sig = provider_fields.get("thought_signatures") or provider_fields.get(
            "thought_signature"
        )
        if isinstance(thought_sig, list) and thought_sig:
            logger.debug(
                f"Found thought_signatures in {location}.provider_specific_fields "
                f"(count={len(thought_sig)})"
            )
        elif isinstance(thought_sig, str) and thought_sig:
            logger.debug(
                f"Found thought_signature in {location}.provider_specific_fields "
                f"(len={len(thought_sig)})"
            )

    # Check extra_content at content level (backward compatibility)
    extra_sig = _signature_from_extra_content(content)
    if extra_sig:
        logger.debug(
            f"Found thought_signature in {location}.extra_content "
            f"(len={len(extra_sig)})"
        )

    # Check extra_content in tool_calls
    tool_calls = content.get("tool_calls")
    if isinstance(tool_calls, list):
        for tc in tool_calls:
            if not isinstance(tc, dict):
                continue
            if _extract_thought_signature_from_tool_call(tc, None, allow_dummy=False):
                sig_count += 1

        if sig_count > 0:
            logger.debug(
                f"Found {sig_count} thought_signatures in {location}.tool_calls"
            )

    return sig_count


def log_gemini_response_signatures(response_data: dict[str, Any], model: Optional[str]) -> None:
    """Log thought_signature presence in Gemini 3 response for debugging.

    This is for debugging only - no modifications are made here.

    Args:
        response_data: The response JSON from the provider
        model: Model name used for Gemini detection
    """
    if not is_gemini3_model(model):
        return

    choices = response_data.get("choices")
    if not isinstance(choices, list):
        return

    for choice in choices:
        if not isinstance(choice, dict):
            continue

        # Check message (non-streaming)
        message = choice.get("message")
        if isinstance(message, dict):
            _check_and_log_signatures(message, "message")

        # Check delta (streaming)
        delta = choice.get("delta")
        if isinstance(delta, dict):
            _check_and_log_signatures(delta, "delta")


def log_gemini_request_signatures(data: dict[str, Any], model: Optional[str]) -> None:
    """Log thought_signature presence in request for debugging (pass-through).

    This is for debugging only - no modifications are made here.

    Args:
        data: The request payload
        model: Model name used for Gemini detection
    """
    if not is_gemini3_model(model):
        return

    messages = data.get("messages")
    if not isinstance(messages, list):
        return

    for message in messages:
        if not isinstance(message, dict):
            continue

        tool_calls = message.get("tool_calls")
        if not isinstance(tool_calls, list):
            continue

        signatures_count = 0
        for tc in tool_calls:
            if not isinstance(tc, dict):
                continue
            if _extract_thought_signature_from_tool_call(tc, None, allow_dummy=False):
                signatures_count += 1

        if signatures_count > 0:
            logger.debug(
                f"Gemini 3 request contains {signatures_count} "
                f"thought_signatures in tool_calls (pass-through)"
            )


def normalize_gemini3_request(data: dict[str, Any], model: Optional[str]) -> bool:
    """Normalize Gemini 3 request payload to align with LiteLLM handling."""
    if not isinstance(data, dict):
        return False

    messages = data.get("messages")
    if not isinstance(messages, list):
        return False

    if model is None:
        return False

    changed = False

    if not is_gemini3_model(model):
        for message in messages:
            if not isinstance(message, dict):
                continue
            if message.get("role") == "assistant":
                tool_calls = message.get("tool_calls")
                if isinstance(tool_calls, list):
                    for tc in tool_calls:
                        if not isinstance(tc, dict):
                            continue
                        tc_id = tc.get("id")
                        if isinstance(tc_id, str) and THOUGHT_SIGNATURE_SEPARATOR in tc_id:
                            tc["id"] = _strip_signature_from_id(tc_id)
                            changed = True
            if message.get("role") == "tool":
                tc_id = message.get("tool_call_id")
                if isinstance(tc_id, str) and THOUGHT_SIGNATURE_SEPARATOR in tc_id:
                    message["tool_call_id"] = _strip_signature_from_id(tc_id)
                    changed = True
        return changed

    if "temperature" not in data or data.get("temperature") is None:
        data["temperature"] = 1.0
        changed = True

    if "reasoning_effort" in data:
        reasoning_effort = data.get("reasoning_effort")
        if "thinking_level" not in data and isinstance(reasoning_effort, str):
            thinking_level = _map_reasoning_effort_to_thinking_level(
                reasoning_effort, model
            )
            if thinking_level:
                data["thinking_level"] = thinking_level
                changed = True
        if "reasoning_effort" in data:
            data.pop("reasoning_effort", None)
            changed = True

    for message in messages:
        if not isinstance(message, dict):
            continue

        tool_calls = message.get("tool_calls")
        if isinstance(tool_calls, list):
            for tc in tool_calls:
                if not isinstance(tc, dict):
                    continue
                signature = _extract_thought_signature_from_tool_call(
                    tc, model, allow_dummy=True
                )
                if signature:
                    provider_fields = _ensure_provider_specific_fields(tc)
                    if provider_fields.get("thought_signature") != signature:
                        provider_fields["thought_signature"] = signature
                        changed = True

        function_call = message.get("function_call")
        if isinstance(function_call, dict):
            signature = _signature_from_provider_fields(
                function_call.get("provider_specific_fields")
            )
            if not signature:
                signature = (
                    _get_dummy_thought_signature() if is_gemini3_model(model) else None
                )
            if signature:
                provider_fields = _ensure_provider_specific_fields(function_call)
                if provider_fields.get("thought_signature") != signature:
                    provider_fields["thought_signature"] = signature
                    changed = True

    return changed


def normalize_gemini3_response(response_data: dict[str, Any], model: Optional[str]) -> bool:
    """Normalize Gemini 3 response payload to align with LiteLLM handling."""
    if not is_gemini3_model(model):
        return False

    choices = response_data.get("choices")
    if not isinstance(choices, list):
        return False

    changed = False

    for choice in choices:
        if not isinstance(choice, dict):
            continue

        for field in ("message", "delta"):
            message = choice.get(field)
            if not isinstance(message, dict):
                continue

            # Map message-level extra_content to provider_specific_fields.thought_signatures
            extra_sig = _signature_from_extra_content(message)
            if extra_sig:
                provider_fields = _ensure_provider_specific_fields(message)
                if "thought_signatures" not in provider_fields:
                    provider_fields["thought_signatures"] = [extra_sig]
                    changed = True

            tool_calls = message.get("tool_calls")
            if isinstance(tool_calls, list):
                for tc in tool_calls:
                    if not isinstance(tc, dict):
                        continue
                    signature = _extract_thought_signature_from_tool_call(
                        tc, None, allow_dummy=False
                    )
                    if signature:
                        provider_fields = _ensure_provider_specific_fields(tc)
                        if provider_fields.get("thought_signature") != signature:
                            provider_fields["thought_signature"] = signature
                            changed = True

                        tc_id = tc.get("id")
                        if isinstance(tc_id, str) and THOUGHT_SIGNATURE_SEPARATOR not in tc_id:
                            tc["id"] = _encode_tool_call_id_with_signature(tc_id, signature)
                            changed = True

    return changed
