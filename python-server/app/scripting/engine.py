"""Lua script engine: compilation, caching, and execution."""

import threading
from typing import Any, Dict, List, Optional, Tuple

from loguru import logger
from lupa import LuaRuntime  # type: ignore[import-untyped]

from app.scripting.sandbox import MAX_SCRIPT_SIZE, create_sandboxed_lua, parse_hooks


class LuaScriptError(Exception):
    """Raised when a Lua hook execution fails."""


_CTX_HELPERS = """
function _create_ctx(req, resp, provider, model)
    local ctx = {}
    ctx._request = req
    ctx._response = resp
    ctx._provider = provider
    ctx._model = model
    ctx._meta = {}

    function ctx:get_request() return self._request end
    function ctx:set_request(v) self._request = v end
    function ctx:get_response() return self._response end
    function ctx:set_response(v) self._response = v end
    function ctx:get_provider() return self._provider end
    function ctx:get_model() return self._model end
    function ctx:get_meta(k) return self._meta[k] end
    function ctx:set_meta(k, v) self._meta[k] = v end
    return ctx
end
"""


class _CompiledScript:
    __slots__ = ("lua", "has_on_request", "has_on_response", "has_on_stream_chunk")

    def __init__(
        self,
        lua: LuaRuntime,
        has_on_request: bool,
        has_on_response: bool,
        has_on_stream_chunk: bool,
    ) -> None:
        self.lua = lua
        self.has_on_request = has_on_request
        self.has_on_response = has_on_response
        self.has_on_stream_chunk = has_on_stream_chunk


class LuaEngine:
    """Manages compiled Lua scripts keyed by provider name."""

    def __init__(self) -> None:
        self._scripts: Dict[str, _CompiledScript] = {}
        self._lock = threading.Lock()

    def reload(self, sources: List[Tuple[str, str]]) -> None:
        """Reload scripts from (provider_name, source) pairs.

        On compilation failure the previous script is preserved.
        Providers absent from *sources* are removed.
        """
        new_scripts: Dict[str, _CompiledScript] = {}
        failed: list[str] = []

        for name, source in sources:
            compiled = self._compile(source)
            if compiled is not None:
                logger.info(f"Lua script compiled for provider: {name}")
                new_scripts[name] = compiled
            else:
                logger.error(
                    f"Failed to compile Lua script for provider: {name}, "
                    "keeping previous version"
                )
                failed.append(name)

        with self._lock:
            # Preserve old scripts for providers that failed to compile
            for name in failed:
                old = self._scripts.get(name)
                if old is not None:
                    logger.warning(f"Retaining previous Lua script version for: {name}")
                    new_scripts[name] = old
            self._scripts = new_scripts

    def reload_from_providers(self, providers: list[Any]) -> None:
        """Reload scripts from provider objects."""
        sources: List[Tuple[str, str]] = []
        for p in providers:
            name = getattr(p, "name", None) or getattr(p, "provider_key", "")
            script = getattr(p, "lua_script", None)
            if script:
                sources.append((name, script))
        self.reload(sources)

    def has_script(self, provider_name: str) -> bool:
        with self._lock:
            return provider_name in self._scripts

    def has_stream_chunk_hook(self, provider_name: str) -> bool:
        with self._lock:
            s = self._scripts.get(provider_name)
            return s.has_on_stream_chunk if s else False

    def call_on_request(
        self,
        provider_name: str,
        request: dict,
        model: str,
    ) -> Optional[dict]:
        """Call on_request hook. Returns modified request or None.

        Raises LuaScriptError on execution failure.
        """
        with self._lock:
            compiled = self._scripts.get(provider_name)

        if compiled is None or not compiled.has_on_request:
            return None

        return self._call_hook(
            compiled, "on_request", request, None, provider_name, model
        )

    def call_on_response(
        self,
        provider_name: str,
        response: dict,
        model: str,
    ) -> Optional[dict]:
        """Call on_response hook. Returns modified response or None.

        Raises LuaScriptError on execution failure.
        """
        with self._lock:
            compiled = self._scripts.get(provider_name)

        if compiled is None or not compiled.has_on_response:
            return None

        return self._call_hook(
            compiled, "on_response", None, response, provider_name, model
        )

    def call_on_stream_chunk(
        self,
        provider_name: str,
        chunk: dict,
        model: str,
    ) -> Optional[dict]:
        """Call on_stream_chunk hook. Returns modified chunk or None.

        Raises LuaScriptError on execution failure.
        """
        with self._lock:
            compiled = self._scripts.get(provider_name)

        if compiled is None or not compiled.has_on_stream_chunk:
            return None

        return self._call_hook(
            compiled, "on_stream_chunk", None, chunk, provider_name, model
        )

    @staticmethod
    def _compile(source: str) -> Optional[_CompiledScript]:
        if len(source) > MAX_SCRIPT_SIZE:
            logger.error(
                f"Lua script too large ({len(source)} bytes, max {MAX_SCRIPT_SIZE})"
            )
            return None

        try:
            lua = create_sandboxed_lua()
            # Install context helpers once at compile time
            lua.execute(_CTX_HELPERS)
            lua.execute(source)
        except Exception as e:
            logger.error(f"Lua compilation error: {e}")
            return None

        has_req, has_resp, has_chunk = parse_hooks(lua)
        return _CompiledScript(
            lua=lua,
            has_on_request=has_req,
            has_on_response=has_resp,
            has_on_stream_chunk=has_chunk,
        )

    @staticmethod
    def _call_hook(
        compiled: _CompiledScript,
        hook_name: str,
        request: Optional[dict],
        response: Optional[dict],
        provider_name: str,
        model: str,
    ) -> Optional[dict]:
        lua = compiled.lua
        g = lua.globals()
        func = g[hook_name]
        if func is None:
            return None

        try:
            req_table = _dict_to_lua_table(lua, request) if request else None
            resp_table = _dict_to_lua_table(lua, response) if response else None

            create_ctx = g["_create_ctx"]
            ctx = create_ctx(req_table, resp_table, provider_name, model)

            func(ctx)

            if hook_name == "on_request":
                result = ctx["_request"]
            else:
                result = ctx["_response"]

            if result is None:
                return None

            return _lua_table_to_dict(result)

        except Exception as e:
            raise LuaScriptError(
                f"Lua {hook_name} error for {provider_name}: {e}"
            ) from e


def _dict_to_lua_table(lua: LuaRuntime, d: dict) -> Any:
    """Recursively convert a Python dict to a Lua table."""
    return lua.table_from(_prepare_for_lua(d))


def _prepare_for_lua(obj: Any) -> Any:
    """Prepare a Python object for lupa table_from conversion.

    lupa's table_from only handles flat dicts/lists. Nested structures
    need manual conversion.
    """
    if isinstance(obj, dict):
        return {k: _prepare_for_lua(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return {i + 1: _prepare_for_lua(v) for i, v in enumerate(obj)}
    return obj


def _lua_table_to_dict(lua_table: Any) -> Any:
    """Recursively convert a lupa Lua table to a Python dict/list."""
    if not hasattr(lua_table, "keys"):
        return lua_table

    keys = list(lua_table.keys())
    if not keys:
        return {}

    # Check if array-like (consecutive integer keys starting from 1)
    if all(isinstance(k, (int, float)) for k in keys):
        int_keys = sorted(int(k) for k in keys)
        if int_keys == list(range(1, len(int_keys) + 1)):
            return [_lua_table_to_dict(lua_table[k]) for k in int_keys]

    return {
        str(k) if not isinstance(k, str) else k: _lua_table_to_dict(lua_table[k])
        for k in keys
    }


_engine: Optional[LuaEngine] = None
_engine_lock = threading.Lock()


def get_lua_engine() -> LuaEngine:
    """Get or create the global LuaEngine singleton."""
    global _engine
    if _engine is None:
        with _engine_lock:
            if _engine is None:
                _engine = LuaEngine()
    return _engine
