"""Gemini 3 thought_signature support utilities.

This module provides helper functions for detecting Gemini 3 providers and
logging thought_signature presence in requests/responses for debugging.

The thought_signature is an encrypted representation of Gemini 3's internal
reasoning process that must be preserved in multi-turn function calling
conversations. The actual implementation follows a pass-through strategy -
no modification is needed, just ensure fields aren't stripped during JSON
handling.

See: https://ai.google.dev/gemini-api/docs/thought-signatures
"""

from typing import Any

from app.core.logging import get_logger

logger = get_logger()


def is_gemini3_provider(provider_name: str) -> bool:
    """Check if provider is Gemini 3 (not Gemini 1.x or 2.x).

    Args:
        provider_name: Name of the provider to check

    Returns:
        True if provider is Gemini 3, False otherwise
    """
    name_lower = provider_name.lower()
    return "gemini-3" in name_lower or "gemini3" in name_lower or "gemini_3" in name_lower


def _check_and_log_signatures(content: dict[str, Any], location: str) -> int:
    """Check for thought_signature in content and log if found.

    Args:
        content: Message or delta content dict
        location: Description of where this content is from (for logging)

    Returns:
        Number of signatures found in tool_calls
    """
    sig_count = 0

    # Check extra_content at content level (text responses with thinking)
    extra_content = content.get("extra_content", {})
    if isinstance(extra_content, dict):
        google_content = extra_content.get("google", {})
        if isinstance(google_content, dict):
            thought_sig = google_content.get("thought_signature")
            if thought_sig:
                logger.debug(
                    f"Found thought_signature in {location}.extra_content "
                    f"(len={len(thought_sig) if isinstance(thought_sig, str) else 0})"
                )

    # Check extra_content in tool_calls
    tool_calls = content.get("tool_calls")
    if isinstance(tool_calls, list):
        for tc in tool_calls:
            if not isinstance(tc, dict):
                continue
            tc_extra = tc.get("extra_content", {})
            if isinstance(tc_extra, dict):
                tc_google = tc_extra.get("google", {})
                if isinstance(tc_google, dict) and tc_google.get("thought_signature"):
                    sig_count += 1

        if sig_count > 0:
            logger.debug(
                f"Found {sig_count} thought_signatures in {location}.tool_calls"
            )

    return sig_count


def log_gemini_response_signatures(response_data: dict[str, Any], provider_name: str) -> None:
    """Log thought_signature presence in Gemini 3 response for debugging.

    This is for debugging only - the pass-through strategy means no
    modification is needed, just verification that fields are preserved.

    Args:
        response_data: The response JSON from the provider
        provider_name: Name of the provider
    """
    if not is_gemini3_provider(provider_name):
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


def log_gemini_request_signatures(data: dict[str, Any], provider_name: str) -> None:
    """Log thought_signature presence in request for debugging (pass-through).

    This is for debugging only - the pass-through strategy means no
    modification is needed, just verification that fields are preserved.

    Args:
        data: The request payload
        provider_name: Name of the provider
    """
    if not is_gemini3_provider(provider_name):
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
            tc_extra = tc.get("extra_content", {})
            if isinstance(tc_extra, dict):
                tc_google = tc_extra.get("google", {})
                if isinstance(tc_google, dict) and tc_google.get("thought_signature"):
                    signatures_count += 1

        if signatures_count > 0:
            logger.debug(
                f"Gemini 3 request contains {signatures_count} "
                f"thought_signatures in extra_content (pass-through)"
            )
