from __future__ import annotations

from typing import Any, Iterable, Optional


def _parse_allowlist(value: Any) -> list[str]:
    if value is None:
        return []
    if isinstance(value, str):
        return [item.strip() for item in value.split(",") if item.strip()]
    if isinstance(value, Iterable):
        items = []
        for item in value:
            if isinstance(item, str):
                trimmed = item.strip()
                if trimmed:
                    items.append(trimmed)
        return items
    return []


def sanitize_anthropic_beta_header(
    provider_type: str,
    provider_params: dict[str, Any],
    header_value: Optional[str],
) -> Optional[str]:
    header_value = (header_value or "").strip()
    if not header_value:
        return None

    provider_type = (provider_type or "").lower()
    if provider_type not in {
        "anthropic",
        "claude",
        "gcp-vertex",
        "gcp_vertex",
        "vertex",
    }:
        return None

    provider_params = provider_params or {}
    policy_value = provider_params.get("anthropic_beta_policy", "drop")
    policy = policy_value.lower() if isinstance(policy_value, str) else "drop"

    if policy == "passthrough":
        return header_value
    if policy == "allowlist":
        allowlist = _parse_allowlist(provider_params.get("anthropic_beta_allowlist"))
        if not allowlist:
            return None
        tokens = [item.strip() for item in header_value.split(",") if item.strip()]
        filtered = [item for item in tokens if item in allowlist]
        return ",".join(filtered) if filtered else None

    return None
