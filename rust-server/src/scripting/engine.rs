//! Lua script engine: compilation, caching, and execution.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, RwLock};

use mlua::{Function, Lua};
use tracing;

use super::bindings::LuaTransformContext;
use super::sandbox::{create_sandboxed_lua, parse_hooks, MAX_SCRIPT_SIZE};

/// Cached compiled Lua state for a single provider.
struct CompiledScript {
    lua: Lua,
    /// Reset to 0 before each hook call to give a fresh instruction budget.
    instruction_counter: Arc<AtomicU32>,
    has_on_request: bool,
    has_on_response: bool,
    has_on_stream_chunk: bool,
}

/// Manages compiled Lua scripts keyed by provider name.
pub struct LuaEngine {
    scripts: RwLock<HashMap<String, CompiledScript>>,
}

impl LuaEngine {
    pub fn new() -> Self {
        Self {
            scripts: RwLock::new(HashMap::new()),
        }
    }

    /// Reload scripts from a list of (provider_name, lua_script_source) pairs.
    ///
    /// On compilation failure the previous script for that provider is
    /// preserved. Providers absent from `sources` are removed.
    pub fn reload(&self, sources: Vec<(String, String)>) {
        let mut new_scripts = HashMap::new();
        let mut failed: Vec<String> = Vec::new();

        for (name, source) in &sources {
            match Self::compile(source) {
                Ok(compiled) => {
                    tracing::info!(provider = %name, "Lua script compiled");
                    new_scripts.insert(name.clone(), compiled);
                }
                Err(e) => {
                    tracing::error!(provider = %name, error = %e, "Failed to compile Lua script, keeping previous version");
                    failed.push(name.clone());
                }
            }
        }

        let mut scripts = self.scripts.write().unwrap();

        // Preserve old compiled scripts for providers that failed to compile
        for name in failed {
            if let Some(old) = scripts.remove(&name) {
                tracing::warn!(provider = %name, "Retaining previous Lua script version");
                new_scripts.insert(name, old);
            }
        }

        *scripts = new_scripts;
    }

    /// Check if a provider has a compiled script.
    pub fn has_script(&self, provider_name: &str) -> bool {
        self.scripts.read().unwrap().contains_key(provider_name)
    }

    /// Check if a provider's script defines the `on_stream_chunk` hook.
    pub fn has_stream_chunk_hook(&self, provider_name: &str) -> bool {
        self.scripts
            .read()
            .unwrap()
            .get(provider_name)
            .map(|s| s.has_on_stream_chunk)
            .unwrap_or(false)
    }

    /// Call `on_request` hook for the given provider.
    pub fn call_on_request(
        &self,
        provider_name: &str,
        request: serde_json::Value,
        model: &str,
    ) -> Result<Option<serde_json::Value>, String> {
        let scripts = self.scripts.read().unwrap();
        let compiled = match scripts.get(provider_name) {
            Some(c) if c.has_on_request => c,
            _ => return Ok(None),
        };

        compiled.instruction_counter.store(0, Ordering::Relaxed);
        let ctx = LuaTransformContext::for_request(request, provider_name, model);
        Self::call_hook(&compiled.lua, "on_request", ctx)
    }

    /// Call `on_response` hook for the given provider.
    pub fn call_on_response(
        &self,
        provider_name: &str,
        response: serde_json::Value,
        model: &str,
    ) -> Result<Option<serde_json::Value>, String> {
        let scripts = self.scripts.read().unwrap();
        let compiled = match scripts.get(provider_name) {
            Some(c) if c.has_on_response => c,
            _ => return Ok(None),
        };

        compiled.instruction_counter.store(0, Ordering::Relaxed);
        let ctx = LuaTransformContext::for_response(response, provider_name, model);
        Self::call_hook_response(&compiled.lua, "on_response", ctx)
    }

    /// Call `on_stream_chunk` hook for the given provider.
    pub fn call_on_stream_chunk(
        &self,
        provider_name: &str,
        chunk: serde_json::Value,
        model: &str,
    ) -> Result<Option<serde_json::Value>, String> {
        let scripts = self.scripts.read().unwrap();
        let compiled = match scripts.get(provider_name) {
            Some(c) if c.has_on_stream_chunk => c,
            _ => return Ok(None),
        };

        compiled.instruction_counter.store(0, Ordering::Relaxed);
        let ctx = LuaTransformContext::for_response(chunk, provider_name, model);
        Self::call_hook_response(&compiled.lua, "on_stream_chunk", ctx)
    }

    fn compile(source: &str) -> Result<CompiledScript, String> {
        if source.len() > MAX_SCRIPT_SIZE {
            return Err(format!(
                "Script size ({} bytes) exceeds maximum ({MAX_SCRIPT_SIZE} bytes)",
                source.len(),
            ));
        }

        let (lua, counter) =
            create_sandboxed_lua().map_err(|e| format!("Failed to create Lua runtime: {e}"))?;

        lua.load(source)
            .exec()
            .map_err(|e| format!("Lua compilation error: {e}"))?;

        // Reset counter after compilation (compilation itself consumes instructions)
        counter.store(0, Ordering::Relaxed);

        let (has_on_request, has_on_response, has_on_stream_chunk) = parse_hooks(&lua);

        Ok(CompiledScript {
            lua,
            instruction_counter: counter,
            has_on_request,
            has_on_response,
            has_on_stream_chunk,
        })
    }

    fn call_hook(
        lua: &Lua,
        hook_name: &str,
        ctx: LuaTransformContext,
    ) -> Result<Option<serde_json::Value>, String> {
        let func: Function = lua
            .globals()
            .get(hook_name)
            .map_err(|e| format!("Failed to get {hook_name}: {e}"))?;

        let ctx = lua
            .create_userdata(ctx)
            .map_err(|e| format!("Failed to create context: {e}"))?;

        func.call::<()>(ctx.clone())
            .map_err(|e| format!("Lua {hook_name} error: {e}"))?;

        let ctx_ref = ctx
            .borrow::<LuaTransformContext>()
            .map_err(|e| format!("Failed to borrow context: {e}"))?;

        Ok(ctx_ref.request.clone())
    }

    fn call_hook_response(
        lua: &Lua,
        hook_name: &str,
        ctx: LuaTransformContext,
    ) -> Result<Option<serde_json::Value>, String> {
        let func: Function = lua
            .globals()
            .get(hook_name)
            .map_err(|e| format!("Failed to get {hook_name}: {e}"))?;

        let ctx = lua
            .create_userdata(ctx)
            .map_err(|e| format!("Failed to create context: {e}"))?;

        func.call::<()>(ctx.clone())
            .map_err(|e| format!("Lua {hook_name} error: {e}"))?;

        let ctx_ref = ctx
            .borrow::<LuaTransformContext>()
            .map_err(|e| format!("Failed to borrow context: {e}"))?;

        Ok(ctx_ref.response.clone())
    }
}

impl Default for LuaEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_new() {
        let engine = LuaEngine::new();
        assert!(!engine.has_script("test"));
    }

    #[test]
    fn test_engine_reload() {
        let engine = LuaEngine::new();
        engine.reload(vec![(
            "provider-a".to_string(),
            "function on_request(ctx) end".to_string(),
        )]);
        assert!(engine.has_script("provider-a"));
        assert!(!engine.has_script("provider-b"));
    }

    #[test]
    fn test_engine_call_on_request() {
        let engine = LuaEngine::new();
        engine.reload(vec![(
            "provider-a".to_string(),
            r#"
            function on_request(ctx)
                local req = ctx:get_request()
                req.temperature = 0.5
                ctx:set_request(req)
            end
            "#
            .to_string(),
        )]);

        let request = serde_json::json!({"model": "gpt-4", "temperature": 1.0});
        let result = engine
            .call_on_request("provider-a", request, "gpt-4")
            .unwrap();

        assert!(result.is_some());
        let modified = result.unwrap();
        assert_eq!(modified["temperature"], 0.5);
    }

    #[test]
    fn test_engine_call_on_response() {
        let engine = LuaEngine::new();
        engine.reload(vec![(
            "provider-a".to_string(),
            r#"
            function on_response(ctx)
                local resp = ctx:get_response()
                resp.custom_field = "added"
                ctx:set_response(resp)
            end
            "#
            .to_string(),
        )]);

        let response = serde_json::json!({"model": "gpt-4"});
        let result = engine
            .call_on_response("provider-a", response, "gpt-4")
            .unwrap();

        assert!(result.is_some());
        let modified = result.unwrap();
        assert_eq!(modified["custom_field"], "added");
    }

    #[test]
    fn test_engine_no_script() {
        let engine = LuaEngine::new();
        let request = serde_json::json!({"model": "gpt-4"});
        let result = engine
            .call_on_request("nonexistent", request, "gpt-4")
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_engine_invalid_script() {
        let engine = LuaEngine::new();
        engine.reload(vec![(
            "bad".to_string(),
            "this is not valid lua {{{{".to_string(),
        )]);
        assert!(!engine.has_script("bad"));
    }

    #[test]
    fn test_engine_rollback_on_failure() {
        let engine = LuaEngine::new();

        // Load a valid script first
        engine.reload(vec![(
            "provider-a".to_string(),
            "function on_request(ctx) end".to_string(),
        )]);
        assert!(engine.has_script("provider-a"));

        // Reload with an invalid script for provider-a — old version should be kept
        engine.reload(vec![(
            "provider-a".to_string(),
            "invalid lua {{{{".to_string(),
        )]);
        assert!(
            engine.has_script("provider-a"),
            "Old script should be preserved on compilation failure"
        );
    }

    #[test]
    fn test_engine_instruction_limit() {
        let engine = LuaEngine::new();
        engine.reload(vec![(
            "loop".to_string(),
            "function on_request(ctx) while true do end end".to_string(),
        )]);

        let request = serde_json::json!({"model": "test"});
        let result = engine.call_on_request("loop", request, "test");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("instruction limit"),
            "Expected instruction limit error, got: {err}"
        );
    }
}
