"""Client extraction utilities for User-Agent header parsing."""

from typing import Tuple
from fastapi import Request

# Known client patterns for User-Agent mapping
# Each tuple: (pattern to match in UA, normalized client name)
# Order matters - more specific patterns should come first
CLIENT_PATTERNS: Tuple[Tuple[str, str], ...] = (
    # Claude CLI / Claude Code (claude-cli/2.1.25, claude-vscode, etc.)
    ("claude-cli", "claude-code"),
    ("claude-code", "claude-code"),
    ("claude-proxy", "claude-proxy"),
    # Kilo-Code (VSCode extension)
    ("Kilo-Code", "kilo-code"),
    # Codex CLI (codex_cli_rs, codex_vscode)
    ("codex_cli_rs", "codex-cli"),
    ("codex_vscode", "codex-vscode"),
    ("codex", "codex"),
    ("Codex", "codex"),
    # AI SDK (ai-sdk/openai-compatible, ai-sdk/anthropic, etc.)
    ("ai-sdk/openai-compatible", "ai-sdk-openai"),
    ("ai-sdk/anthropic", "ai-sdk-anthropic"),
    ("ai-sdk", "ai-sdk"),
    ("ai/", "ai-sdk"),
    # OpenAI SDK (OpenAI/JS, openai-python, etc.)
    ("OpenAI/JS", "openai-js"),
    ("openai-python", "openai-python"),
    ("OpenAI-Python", "openai-python"),
    ("openai-node", "openai-node"),
    ("OpenAI-Node", "openai-node"),
    ("openai/", "openai-sdk"),
    ("OpenAI/", "openai-sdk"),
    # Anthropic SDK
    ("anthropic-sdk", "anthropic-sdk"),
    ("Anthropic", "anthropic-sdk"),
    # Other AI coding assistants
    ("opencode", "opencode"),
    ("OpenCode", "opencode"),
    ("cursor", "cursor"),
    ("Cursor", "cursor"),
    ("copilot", "copilot"),
    ("Copilot", "copilot"),
    ("continue", "continue"),
    ("Continue", "continue"),
    ("aider", "aider"),
    ("Aider", "aider"),
    ("cline", "cline"),
    ("Cline", "cline"),
    # API testing tools
    ("Apifox", "apifox"),
    ("PostmanRuntime", "postman"),
    ("insomnia", "insomnia"),
    # Common HTTP clients
    ("python-httpx", "python-httpx"),
    ("python-requests", "python-requests"),
    ("httpx", "httpx"),
    ("axios", "axios"),
    ("node-fetch", "node-fetch"),
    ("curl", "curl"),
    ("wget", "wget"),
    # Terminal apps
    ("iTerm2", "iterm2"),
    # Browsers (low priority - usually not direct API calls)
    ("Mozilla", "browser"),
    # LangChain / LlamaIndex
    ("langchain", "langchain"),
    ("LangChain", "langchain"),
    ("llama-index", "llama-index"),
    ("LlamaIndex", "llama-index"),
)


def extract_client(request: Request) -> str:
    """Extract normalized client name from User-Agent header"""
    user_agent = request.headers.get("user-agent", "")

    if not user_agent:
        return "unknown"

    # Try to match known client patterns
    for pattern, client_name in CLIENT_PATTERNS:
        if pattern in user_agent:
            return client_name

    # Fallback: extract first token (before space or slash) and truncate to 30 chars
    first_token = user_agent.split(" ")[0].split("/")[0]
    # Keep only alphanumeric, dash, underscore, dot
    cleaned = "".join(c for c in first_token if c.isalnum() or c in "-_.")[:30]

    return cleaned if cleaned else "other"
