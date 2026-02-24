"""Sandboxed Lua runtime creation using lupa."""

from typing import Optional, Tuple

from lupa import LuaRuntime  # type: ignore[import-untyped]

_DANGEROUS_GLOBALS = ("io", "os", "debug", "loadfile", "dofile", "require")

MAX_SCRIPT_SIZE = 1024 * 1024  # 1 MB


def create_sandboxed_lua() -> LuaRuntime:
    """Create a sandboxed LuaJIT runtime with dangerous globals removed."""
    lua = LuaRuntime(unpack_returned_tuples=True)

    g = lua.globals()
    for name in _DANGEROUS_GLOBALS:
        g[name] = None

    return lua


def parse_hooks(lua: LuaRuntime) -> Tuple[bool, bool, bool]:
    """Return (has_on_request, has_on_response, has_on_stream_chunk)."""
    g = lua.globals()
    return (
        g["on_request"] is not None,
        g["on_response"] is not None,
        g["on_stream_chunk"] is not None,
    )


def validate_script(source: str) -> Optional[str]:
    """Validate a Lua script by compiling and checking for hooks.

    Returns None on success, or an error message string on failure.
    """
    if len(source) > MAX_SCRIPT_SIZE:
        return (
            f"Script size ({len(source)} bytes) exceeds "
            f"maximum ({MAX_SCRIPT_SIZE} bytes)"
        )

    try:
        lua = create_sandboxed_lua()
    except Exception as e:
        return f"Failed to create Lua runtime: {e}"

    try:
        lua.execute(source)
    except Exception as e:
        return f"Syntax error: {e}"

    has_req, has_resp, has_chunk = parse_hooks(lua)
    if not has_req and not has_resp and not has_chunk:
        return (
            "Script must define at least one hook: "
            "on_request, on_response, or on_stream_chunk"
        )

    return None


def sanitize_lua_error(err: str) -> str:
    """Strip internal details from Lua error messages for API responses."""
    if "exceeds maximum" in err:
        return err
    if "must define at least one hook" in err:
        return err
    if err.startswith("Syntax error:"):
        return err
    if "instruction limit" in err:
        return "Script exceeded execution limit (possible infinite loop)"
    return "Script validation failed"
