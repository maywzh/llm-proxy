//! Sandboxed Lua runtime creation.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use mlua::{HookTriggers, Lua, Result as LuaResult, VmState};

const MEMORY_LIMIT: usize = 16 * 1024 * 1024; // 16 MB
const MAX_INSTRUCTIONS: u32 = 1_000_000;
const HOOK_GRANULARITY: u32 = 10_000;

/// Maximum script source size in bytes (1 MB).
pub const MAX_SCRIPT_SIZE: usize = 1024 * 1024;

/// Create a sandboxed Lua runtime with dangerous globals removed, memory
/// limit, and an instruction-count hook to prevent infinite loops.
///
/// Returns `(lua, counter)` where `counter` should be reset to 0 before
/// each hook invocation so the script gets a fresh instruction budget.
pub fn create_sandboxed_lua() -> LuaResult<(Lua, Arc<AtomicU32>)> {
    let lua = unsafe { Lua::unsafe_new() };
    lua.set_memory_limit(MEMORY_LIMIT)?;

    // Disable JIT so instruction-count hooks fire reliably (LuaJIT skips
    // hooks for JIT-compiled code paths).
    lua.load("if jit then jit.off() end").exec()?;

    // Remove dangerous globals
    let globals = lua.globals();
    for name in &["io", "os", "debug", "loadfile", "dofile", "require", "jit"] {
        globals.set(*name, mlua::Value::Nil)?;
    }

    // Instruction-count hook
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = counter.clone();
    lua.set_hook(
        HookTriggers::new().every_nth_instruction(HOOK_GRANULARITY),
        move |_lua, _debug| {
            let prev = counter_clone.fetch_add(HOOK_GRANULARITY, Ordering::Relaxed);
            if prev + HOOK_GRANULARITY > MAX_INSTRUCTIONS {
                return Err(mlua::Error::runtime(
                    "script exceeded instruction limit (possible infinite loop)",
                ));
            }
            Ok(VmState::Continue)
        },
    );

    Ok((lua, counter))
}

/// Inspect a compiled Lua environment and return which hooks are defined.
pub fn parse_hooks(lua: &Lua) -> (bool, bool, bool) {
    let globals = lua.globals();
    let has_on_request = globals.get::<mlua::Function>("on_request").is_ok();
    let has_on_response = globals.get::<mlua::Function>("on_response").is_ok();
    let has_on_stream_chunk = globals.get::<mlua::Function>("on_stream_chunk").is_ok();
    (has_on_request, has_on_response, has_on_stream_chunk)
}

/// Validate a Lua script by compiling it in a sandbox.
///
/// Returns `Ok(())` if valid, or `Err(message)` on failure.
pub fn validate_script(source: &str) -> std::result::Result<(), String> {
    if source.len() > MAX_SCRIPT_SIZE {
        return Err(format!(
            "Script size ({} bytes) exceeds maximum ({MAX_SCRIPT_SIZE} bytes)",
            source.len(),
        ));
    }

    let (lua, _counter) =
        create_sandboxed_lua().map_err(|e| format!("Failed to create Lua runtime: {e}"))?;

    lua.load(source)
        .exec()
        .map_err(|e| format!("Lua compilation error: {e}"))?;

    let (has_req, has_resp, has_chunk) = parse_hooks(&lua);
    if !has_req && !has_resp && !has_chunk {
        return Err(
            "Script must define at least one hook: on_request, on_response, or on_stream_chunk"
                .to_string(),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_sandboxed_lua() {
        let (lua, _) = create_sandboxed_lua().unwrap();
        let globals = lua.globals();

        assert!(globals.get::<mlua::Value>("io").unwrap().is_nil());
        assert!(globals.get::<mlua::Value>("os").unwrap().is_nil());
        assert!(globals.get::<mlua::Value>("debug").unwrap().is_nil());

        assert!(!globals.get::<mlua::Value>("string").unwrap().is_nil());
        assert!(!globals.get::<mlua::Value>("table").unwrap().is_nil());
        assert!(!globals.get::<mlua::Value>("math").unwrap().is_nil());
    }

    #[test]
    fn test_validate_script_valid() {
        let result = validate_script("function on_request(ctx) end");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_script_no_hooks() {
        let result = validate_script("local x = 1");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("must define at least one hook"));
    }

    #[test]
    fn test_validate_script_syntax_error() {
        let result = validate_script("function on_request(");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Lua compilation error"));
    }

    #[test]
    fn test_validate_script_too_large() {
        let source = "a".repeat(MAX_SCRIPT_SIZE + 1);
        let result = validate_script(&source);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds maximum"));
    }

    #[test]
    fn test_instruction_limit() {
        let result = validate_script(
            r#"
            function on_request(ctx)
                while true do end
            end
            on_request(nil)
            "#,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("instruction limit"),
            "Expected instruction limit error, got: {err}"
        );
    }
}
