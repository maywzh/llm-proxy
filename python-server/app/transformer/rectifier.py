"""Request rectifier utilities for provider-bound payloads."""

from typing import Any


def sanitize_provider_payload(payload: dict[str, Any]) -> None:
    """Sanitize provider payload to avoid cross-provider validation errors."""
    messages = payload.get("messages")
    if not isinstance(messages, list):
        return

    for msg in messages:
        content = msg.get("content")
        if not isinstance(content, list):
            continue

        content = [
            block
            for block in content
            if not (
                isinstance(block, dict)
                and block.get("type") in ("thinking", "redacted_thinking")
            )
        ]

        for block in content:
            if isinstance(block, dict):
                block.pop("signature", None)

            if (
                isinstance(block, dict)
                and block.get("type") == "text"
                and isinstance(block.get("text"), str)
                and not block["text"].strip()
            ):
                block["text"] = "."

        if not content and msg.get("role") == "assistant":
            content = [{"type": "text", "text": "."}]

        msg["content"] = content

    if _should_remove_top_level_thinking(payload):
        payload.pop("thinking", None)


def _should_remove_top_level_thinking(payload: dict[str, Any]) -> bool:
    thinking = payload.get("thinking")
    if not isinstance(thinking, dict) or thinking.get("type") != "enabled":
        return False

    messages = payload.get("messages")
    if not isinstance(messages, list):
        return False

    last_assistant_content = None
    for msg in reversed(messages):
        if not isinstance(msg, dict) or msg.get("role") != "assistant":
            continue
        content = msg.get("content")
        if isinstance(content, list) and content:
            last_assistant_content = content
        break

    if not last_assistant_content:
        return False

    first = last_assistant_content[0]
    first_type = first.get("type") if isinstance(first, dict) else None
    if first_type in ("thinking", "redacted_thinking"):
        return False

    return any(
        isinstance(block, dict) and block.get("type") == "tool_use"
        for block in last_assistant_content
    )
