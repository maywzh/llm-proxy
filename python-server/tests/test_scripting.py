"""Tests for the Lua scripting engine."""

import pytest

from app.scripting.engine import LuaEngine, LuaScriptError
from app.scripting.sandbox import MAX_SCRIPT_SIZE, validate_script


class TestValidateScript:
    def test_valid_on_request(self):
        assert validate_script("function on_request(ctx) end") is None

    def test_valid_on_response(self):
        assert validate_script("function on_response(ctx) end") is None

    def test_valid_on_stream_chunk(self):
        assert validate_script("function on_stream_chunk(ctx) end") is None

    def test_no_hooks(self):
        err = validate_script("local x = 1")
        assert err is not None
        assert "must define at least one hook" in err

    def test_syntax_error(self):
        err = validate_script("function on_request(")
        assert err is not None
        assert "Syntax error" in err

    def test_too_large(self):
        err = validate_script("a" * (MAX_SCRIPT_SIZE + 1))
        assert err is not None
        assert "exceeds maximum" in err


class TestLuaEngine:
    def test_new_engine_has_no_scripts(self):
        engine = LuaEngine()
        assert not engine.has_script("test")

    def test_reload_and_has_script(self):
        engine = LuaEngine()
        engine.reload([("prov-a", "function on_request(ctx) end")])
        assert engine.has_script("prov-a")
        assert not engine.has_script("prov-b")

    def test_call_on_request(self):
        engine = LuaEngine()
        engine.reload(
            [
                (
                    "prov-a",
                    """
                function on_request(ctx)
                    local req = ctx:get_request()
                    req.temperature = 0.5
                    ctx:set_request(req)
                end
                """,
                )
            ]
        )

        result = engine.call_on_request(
            "prov-a", {"model": "gpt-4", "temperature": 1.0}, "gpt-4"
        )
        assert result is not None
        assert result["temperature"] == 0.5

    def test_call_on_response(self):
        engine = LuaEngine()
        engine.reload(
            [
                (
                    "prov-a",
                    """
                function on_response(ctx)
                    local resp = ctx:get_response()
                    resp.custom = "added"
                    ctx:set_response(resp)
                end
                """,
                )
            ]
        )

        result = engine.call_on_response("prov-a", {"model": "gpt-4"}, "gpt-4")
        assert result is not None
        assert result["custom"] == "added"

    def test_call_on_stream_chunk(self):
        engine = LuaEngine()
        engine.reload(
            [
                (
                    "prov-a",
                    """
                function on_stream_chunk(ctx)
                    local resp = ctx:get_response()
                    resp.modified = true
                    ctx:set_response(resp)
                end
                """,
                )
            ]
        )

        assert engine.has_stream_chunk_hook("prov-a")
        result = engine.call_on_stream_chunk("prov-a", {"data": "chunk"}, "gpt-4")
        assert result is not None
        assert result["modified"] is True

    def test_no_script_returns_none(self):
        engine = LuaEngine()
        result = engine.call_on_request("nonexistent", {"model": "gpt-4"}, "gpt-4")
        assert result is None

    def test_invalid_script_not_loaded(self):
        engine = LuaEngine()
        engine.reload([("bad", "this is not valid lua {{{{")])
        assert not engine.has_script("bad")

    def test_rollback_on_failure(self):
        engine = LuaEngine()
        engine.reload([("prov-a", "function on_request(ctx) end")])
        assert engine.has_script("prov-a")

        # Reload with invalid script — old version should be preserved
        engine.reload([("prov-a", "invalid lua {{{{")])
        assert engine.has_script("prov-a"), "Old script should be preserved"

    def test_script_error_raises(self):
        engine = LuaEngine()
        engine.reload(
            [
                (
                    "prov-err",
                    """
                function on_request(ctx)
                    error("intentional error")
                end
                """,
                )
            ]
        )

        with pytest.raises(LuaScriptError):
            engine.call_on_request("prov-err", {"model": "gpt-4"}, "gpt-4")
