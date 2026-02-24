//! Lua scripting engine for per-provider request/response transformation.
//!
//! Providers can define Lua scripts with hook functions:
//! - `on_request(ctx)`: Transform request before sending to provider
//! - `on_response(ctx)`: Transform response before returning to client
//! - `on_stream_chunk(ctx)`: Transform each streaming chunk (optional)

pub mod bindings;
pub mod engine;
pub mod lua_feature;
pub mod sandbox;

pub use engine::LuaEngine;
pub use lua_feature::LuaFeatureTransformer;
