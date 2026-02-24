"""Lua script engine: compilation, caching, and execution."""

import threading
from typing import Any, Dict, List, Optional, Tuple

from loguru import logger
from lupa import LuaRuntime  # type: ignore[import-untyped]

from app.scripting.sandbox import (
    MAX_SCRIPT_SIZE,
    HookFlags,
    create_sandboxed_lua,
    parse_hooks,
)


class LuaScriptError(Exception):
    """Raised when a Lua hook execution fails."""


_CTX_HELPERS = """
function _create_ctx(req, resp, provider, model)
    local ctx = {}
    ctx._request = req
    ctx._response = resp
    ctx._unified = nil
    ctx._provider = provider
    ctx._model = model
    ctx._client_protocol = ""
    ctx._provider_protocol = ""
    ctx._meta = {}

    function ctx:get_request() return self._request end
    function ctx:set_request(v) self._request = v end
    function ctx:get_response() return self._response end
    function ctx:set_response(v) self._response = v end
    function ctx:get_unified() return self._unified end
    function ctx:set_unified(v) self._unified = v end
    function ctx:get_provider() return self._provider end
    function ctx:get_model() return self._model end
    function ctx:get_client_protocol() return self._client_protocol end
    function ctx:get_provider_protocol() return self._provider_protocol end
    function ctx:get_meta(k) return self._meta[k] end
    function ctx:set_meta(k, v) self._meta[k] = v end
    return ctx
end
"""


class _CompiledScript:
    __slots__ = ("lua", "hooks")

    def __init__(self, lua: LuaRuntime, hooks: HookFlags) -> None:
        self.lua = lua
        self.hooks = hooks


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
            return s.hooks.on_stream_chunk if s else False

    def has_transform_hooks(self, provider_name: str) -> bool:
        with self._lock:
            s = self._scripts.get(provider_name)
            return s.hooks.has_transform_hooks() if s else False

    # =========================================================================
    # Existing hooks (raw JSON level)
    # =========================================================================

    def call_on_request(
        self,
        provider_name: str,
        request: dict,
        model: str,
    ) -> Optional[dict]:
        """Call on_request hook. Returns modified request or None."""
        with self._lock:
            compiled = self._scripts.get(provider_name)
        if compiled is None or not compiled.hooks.on_request:
            return None
        return self._call_hook(
            compiled, "on_request", request, None, None, provider_name, model
        )

    def call_on_response(
        self,
        provider_name: str,
        response: dict,
        model: str,
    ) -> Optional[dict]:
        """Call on_response hook. Returns modified response or None."""
        with self._lock:
            compiled = self._scripts.get(provider_name)
        if compiled is None or not compiled.hooks.on_response:
            return None
        return self._call_hook(
            compiled, "on_response", None, response, None, provider_name, model
        )

    def call_on_stream_chunk(
        self,
        provider_name: str,
        chunk: dict,
        model: str,
    ) -> Optional[dict]:
        """Call on_stream_chunk hook. Returns modified chunk or None."""
        with self._lock:
            compiled = self._scripts.get(provider_name)
        if compiled is None or not compiled.hooks.on_stream_chunk:
            return None
        return self._call_hook(
            compiled, "on_stream_chunk", None, chunk, None, provider_name, model
        )

    # =========================================================================
    # Protocol transform hooks (UIF level)
    # =========================================================================

    def call_on_transform_request_out(
        self,
        provider_name: str,
        request: dict,
        model: str,
        client_protocol: str = "",
        provider_protocol: str = "",
    ) -> Optional[dict]:
        """Call on_transform_request_out (Client → UIF). Returns UIF dict or None."""
        with self._lock:
            compiled = self._scripts.get(provider_name)
        if compiled is None or not compiled.hooks.on_transform_request_out:
            return None
        return self._call_transform_hook(
            compiled,
            "on_transform_request_out",
            request=request,
            response=None,
            unified=None,
            provider_name=provider_name,
            model=model,
            client_protocol=client_protocol,
            provider_protocol=provider_protocol,
            extract="unified",
        )

    def call_on_transform_request_in(
        self,
        provider_name: str,
        unified: dict,
        model: str,
        client_protocol: str = "",
        provider_protocol: str = "",
    ) -> Optional[dict]:
        """Call on_transform_request_in (UIF → Provider). Returns provider dict or None."""
        with self._lock:
            compiled = self._scripts.get(provider_name)
        if compiled is None or not compiled.hooks.on_transform_request_in:
            return None
        return self._call_transform_hook(
            compiled,
            "on_transform_request_in",
            request=None,
            response=None,
            unified=unified,
            provider_name=provider_name,
            model=model,
            client_protocol=client_protocol,
            provider_protocol=provider_protocol,
            extract="request",
        )

    def call_on_transform_response_in(
        self,
        provider_name: str,
        response: dict,
        model: str,
        client_protocol: str = "",
        provider_protocol: str = "",
    ) -> Optional[dict]:
        """Call on_transform_response_in (Provider → UIF). Returns UIF dict or None."""
        with self._lock:
            compiled = self._scripts.get(provider_name)
        if compiled is None or not compiled.hooks.on_transform_response_in:
            return None
        return self._call_transform_hook(
            compiled,
            "on_transform_response_in",
            request=None,
            response=response,
            unified=None,
            provider_name=provider_name,
            model=model,
            client_protocol=client_protocol,
            provider_protocol=provider_protocol,
            extract="unified",
        )

    def call_on_transform_response_out(
        self,
        provider_name: str,
        unified: dict,
        model: str,
        client_protocol: str = "",
        provider_protocol: str = "",
    ) -> Optional[dict]:
        """Call on_transform_response_out (UIF → Client). Returns client dict or None."""
        with self._lock:
            compiled = self._scripts.get(provider_name)
        if compiled is None or not compiled.hooks.on_transform_response_out:
            return None
        return self._call_transform_hook(
            compiled,
            "on_transform_response_out",
            request=None,
            response=None,
            unified=unified,
            provider_name=provider_name,
            model=model,
            client_protocol=client_protocol,
            provider_protocol=provider_protocol,
            extract="response",
        )

    # =========================================================================
    # Internal
    # =========================================================================

    @staticmethod
    def _compile(source: str) -> Optional[_CompiledScript]:
        if len(source) > MAX_SCRIPT_SIZE:
            logger.error(
                f"Lua script too large ({len(source)} bytes, max {MAX_SCRIPT_SIZE})"
            )
            return None

        try:
            lua = create_sandboxed_lua()
            lua.execute(_CTX_HELPERS)
            lua.execute(source)
        except Exception as e:
            logger.error(f"Lua compilation error: {e}")
            return None

        hooks = parse_hooks(lua)
        return _CompiledScript(lua=lua, hooks=hooks)

    @staticmethod
    def _call_hook(
        compiled: _CompiledScript,
        hook_name: str,
        request: Optional[dict],
        response: Optional[dict],
        unified: Optional[dict],
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

    @staticmethod
    def _call_transform_hook(
        compiled: _CompiledScript,
        hook_name: str,
        request: Optional[dict],
        response: Optional[dict],
        unified: Optional[dict],
        provider_name: str,
        model: str,
        client_protocol: str,
        provider_protocol: str,
        extract: str,
    ) -> Optional[dict]:
        """Execute a protocol transform hook.

        *extract* determines which ctx field to read after the hook:
        "unified", "request", or "response".
        """
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

            # Set extra fields not covered by _create_ctx
            if unified is not None:
                ctx["_unified"] = _dict_to_lua_table(lua, unified)
            ctx["_client_protocol"] = client_protocol
            ctx["_provider_protocol"] = provider_protocol

            func(ctx)

            field_map = {
                "unified": "_unified",
                "request": "_request",
                "response": "_response",
            }
            result = ctx[field_map[extract]]

            if result is None:
                return None

            return _lua_table_to_dict(result)

        except Exception as e:
            raise LuaScriptError(
                f"Lua {hook_name} error for {provider_name}: {e}"
            ) from e


def _dict_to_lua_table(lua: LuaRuntime, d: dict) -> Any:
    """Recursively convert a Python dict to a Lua table."""
    return _deep_to_lua(lua, d)


def _deep_to_lua(lua: LuaRuntime, obj: Any) -> Any:
    """Deeply convert a Python object to nested Lua tables.

    lupa's table_from only converts one level, so we must recurse and
    call table_from at every nesting level to produce real Lua tables.
    """
    if isinstance(obj, dict):
        return lua.table_from({k: _deep_to_lua(lua, v) for k, v in obj.items()})
    if isinstance(obj, (list, tuple)):
        return lua.table_from({i + 1: _deep_to_lua(lua, v) for i, v in enumerate(obj)})
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
