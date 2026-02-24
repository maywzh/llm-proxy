//! LuaFeatureTransformer — bridges the Lua engine into the FeatureTransformer trait.

use std::sync::Arc;

use crate::core::error::Result;
use crate::transformer::features::FeatureTransformer;
use crate::transformer::unified::{UnifiedRequest, UnifiedResponse, UnifiedStreamChunk};

use super::engine::LuaEngine;

/// A FeatureTransformer that delegates to the Lua scripting engine.
///
/// This transformer is injected into the TransformPipeline when a provider
/// has a Lua script configured. It calls the corresponding Lua hooks
/// (`on_request`, `on_response`, `on_stream_chunk`) on the raw JSON payload.
pub struct LuaFeatureTransformer {
    engine: Arc<LuaEngine>,
    provider_name: String,
}

impl LuaFeatureTransformer {
    pub fn new(engine: Arc<LuaEngine>, provider_name: String) -> Self {
        Self {
            engine,
            provider_name,
        }
    }

    /// Check if this provider has a Lua script.
    pub fn is_active(&self) -> bool {
        self.engine.has_script(&self.provider_name)
    }
}

impl FeatureTransformer for LuaFeatureTransformer {
    fn transform_request(&self, _request: &mut UnifiedRequest) -> Result<()> {
        // Lua scripts operate on raw JSON, not on UIF.
        // The actual Lua hook is called in the pipeline layer where we have
        // access to the raw JSON payload (before/after protocol transformation).
        // This is intentionally a no-op in the FeatureTransformer trait —
        // the pipeline calls LuaEngine directly.
        Ok(())
    }

    fn transform_response(&self, _response: &mut UnifiedResponse) -> Result<()> {
        // Same as transform_request — Lua hooks run on raw JSON in the pipeline.
        Ok(())
    }

    fn transform_stream_chunk(&self, _chunk: &mut UnifiedStreamChunk) -> Result<()> {
        // Stream chunk transformation via Lua is handled at the pipeline level.
        Ok(())
    }

    fn name(&self) -> &'static str {
        "lua"
    }
}
