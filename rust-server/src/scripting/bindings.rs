//! Lua-to-Rust bindings for the transform context.

use mlua::{IntoLua, Lua, LuaSerdeExt, Result as LuaResult, UserData, UserDataMethods};
use std::collections::HashMap;

/// UserData wrapper exposing request/response payload to Lua scripts.
///
/// Lua scripts interact with this object inside hook functions:
///
/// ```lua
/// function on_request(ctx)
///   local req = ctx:get_request()
///   req.temperature = 0.7
///   ctx:set_request(req)
/// end
/// ```
pub struct LuaTransformContext {
    pub request: Option<serde_json::Value>,
    pub response: Option<serde_json::Value>,
    pub provider_name: String,
    pub model: String,
    pub meta: HashMap<String, serde_json::Value>,
}

impl LuaTransformContext {
    pub fn for_request(request: serde_json::Value, provider_name: &str, model: &str) -> Self {
        Self {
            request: Some(request),
            response: None,
            provider_name: provider_name.to_string(),
            model: model.to_string(),
            meta: HashMap::new(),
        }
    }

    pub fn for_response(response: serde_json::Value, provider_name: &str, model: &str) -> Self {
        Self {
            request: None,
            response: Some(response),
            provider_name: provider_name.to_string(),
            model: model.to_string(),
            meta: HashMap::new(),
        }
    }
}

/// Convert serde_json::Value to mlua::Value.
fn json_to_lua(lua: &Lua, value: &serde_json::Value) -> LuaResult<mlua::Value> {
    lua.to_value(value)
}

/// Convert mlua::Value to serde_json::Value.
fn lua_to_json(lua: &Lua, value: mlua::Value) -> LuaResult<serde_json::Value> {
    lua.from_value(value)
}

impl UserData for LuaTransformContext {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get_request", |lua, this, ()| match &this.request {
            Some(req) => json_to_lua(lua, req),
            None => Ok(mlua::Value::Nil),
        });

        methods.add_method_mut("set_request", |lua, this, value: mlua::Value| {
            this.request = Some(lua_to_json(lua, value)?);
            Ok(())
        });

        methods.add_method("get_response", |lua, this, ()| match &this.response {
            Some(resp) => json_to_lua(lua, resp),
            None => Ok(mlua::Value::Nil),
        });

        methods.add_method_mut("set_response", |lua, this, value: mlua::Value| {
            this.response = Some(lua_to_json(lua, value)?);
            Ok(())
        });

        methods.add_method("get_provider", |lua, this, ()| {
            this.provider_name.clone().into_lua(lua)
        });

        methods.add_method("get_model", |lua, this, ()| {
            this.model.clone().into_lua(lua)
        });

        methods.add_method("get_meta", |lua, this, key: String| {
            match this.meta.get(&key) {
                Some(v) => json_to_lua(lua, v),
                None => Ok(mlua::Value::Nil),
            }
        });

        methods.add_method_mut(
            "set_meta",
            |lua, this, (key, value): (String, mlua::Value)| {
                let json_val = lua_to_json(lua, value)?;
                this.meta.insert(key, json_val);
                Ok(())
            },
        );
    }
}
