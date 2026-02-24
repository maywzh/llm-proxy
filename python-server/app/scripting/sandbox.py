"""Sandboxed Lua runtime creation using lupa."""

from dataclasses import dataclass
from typing import Optional

from lupa import LuaRuntime  # type: ignore[import-untyped]

_DANGEROUS_GLOBALS = ("io", "os", "debug", "loadfile", "dofile", "require")

MAX_SCRIPT_SIZE = 1024 * 1024  # 1 MB

# All recognised hook names
_ALL_HOOKS = (
    "on_request",
    "on_response",
    "on_stream_chunk",
    "on_transform_request_out",
    "on_transform_request_in",
    "on_transform_response_in",
    "on_transform_response_out",
)


@dataclass
class HookFlags:
    on_request: bool = False
    on_response: bool = False
    on_stream_chunk: bool = False
    on_transform_request_out: bool = False
    on_transform_request_in: bool = False
    on_transform_response_in: bool = False
    on_transform_response_out: bool = False

    def has_transform_hooks(self) -> bool:
        return (
            self.on_transform_request_out
            or self.on_transform_request_in
            or self.on_transform_response_in
            or self.on_transform_response_out
        )

    def has_any(self) -> bool:
        return (
            self.on_request
            or self.on_response
            or self.on_stream_chunk
            or self.has_transform_hooks()
        )


def create_sandboxed_lua() -> LuaRuntime:
    """Create a sandboxed LuaJIT runtime with dangerous globals removed."""
    lua = LuaRuntime(unpack_returned_tuples=True)

    g = lua.globals()
    for name in _DANGEROUS_GLOBALS:
        g[name] = None

    return lua


def parse_hooks(lua: LuaRuntime) -> HookFlags:
    """Detect which hooks are defined in a compiled Lua script."""
    g = lua.globals()
    return HookFlags(
        on_request=g["on_request"] is not None,
        on_response=g["on_response"] is not None,
        on_stream_chunk=g["on_stream_chunk"] is not None,
        on_transform_request_out=g["on_transform_request_out"] is not None,
        on_transform_request_in=g["on_transform_request_in"] is not None,
        on_transform_response_in=g["on_transform_response_in"] is not None,
        on_transform_response_out=g["on_transform_response_out"] is not None,
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

    hooks = parse_hooks(lua)
    if not hooks.has_any():
        return "Script must define at least one hook: " + ", ".join(_ALL_HOOKS)

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
