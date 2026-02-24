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
///
/// For protocol transform hooks, the `unified` field carries the UIF
/// (Unified Internal Format) as a JSON table:
///
/// ```lua
/// function on_transform_request_out(ctx)
///   local raw = ctx:get_request()
///   ctx:set_unified({ model = raw.model, messages = {}, parameters = {} })
/// end
/// ```
pub struct LuaTransformContext {
    pub request: Option<serde_json::Value>,
    pub response: Option<serde_json::Value>,
    /// UIF (Unified Internal Format) payload for protocol transform hooks.
    pub unified: Option<serde_json::Value>,
    pub provider_name: String,
    pub model: String,
    pub client_protocol: String,
    pub provider_protocol: String,
    pub meta: HashMap<String, serde_json::Value>,
}

impl LuaTransformContext {
    pub fn for_request(request: serde_json::Value, provider_name: &str, model: &str) -> Self {
        Self {
            request: Some(request),
            response: None,
            unified: None,
            provider_name: provider_name.to_string(),
            model: model.to_string(),
            client_protocol: String::new(),
            provider_protocol: String::new(),
            meta: HashMap::new(),
        }
    }

    pub fn for_response(response: serde_json::Value, provider_name: &str, model: &str) -> Self {
        Self {
            request: None,
            response: Some(response),
            unified: None,
            provider_name: provider_name.to_string(),
            model: model.to_string(),
            client_protocol: String::new(),
            provider_protocol: String::new(),
            meta: HashMap::new(),
        }
    }

    /// Create context for protocol transform hooks that need UIF access.
    pub fn for_transform(
        request: Option<serde_json::Value>,
        response: Option<serde_json::Value>,
        unified: Option<serde_json::Value>,
        provider_name: &str,
        model: &str,
        client_protocol: &str,
        provider_protocol: &str,
    ) -> Self {
        Self {
            request,
            response,
            unified,
            provider_name: provider_name.to_string(),
            model: model.to_string(),
            client_protocol: client_protocol.to_string(),
            provider_protocol: provider_protocol.to_string(),
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

        methods.add_method("get_unified", |lua, this, ()| match &this.unified {
            Some(u) => json_to_lua(lua, u),
            None => Ok(mlua::Value::Nil),
        });

        methods.add_method_mut("set_unified", |lua, this, value: mlua::Value| {
            this.unified = Some(lua_to_json(lua, value)?);
            Ok(())
        });

        methods.add_method("get_client_protocol", |lua, this, ()| {
            this.client_protocol.clone().into_lua(lua)
        });

        methods.add_method("get_provider_protocol", |lua, this, ()| {
            this.provider_protocol.clone().into_lua(lua)
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
