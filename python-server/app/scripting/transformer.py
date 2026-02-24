"""LuaFeatureTransformer — bridges the Lua engine into the FeatureTransformer ABC."""

from app.scripting.engine import LuaEngine
from app.transformer.pipeline import FeatureTransformer
from app.transformer.unified import UnifiedRequest, UnifiedResponse, UnifiedStreamChunk


class LuaFeatureTransformer(FeatureTransformer):
    """A FeatureTransformer that delegates to the Lua scripting engine.

    Lua scripts operate on raw JSON payloads, so the actual hook calls
    happen at the pipeline/proxy layer where raw dicts are available.
    The UIF-level methods here are intentional no-ops.
    """

    def __init__(self, engine: LuaEngine, provider_name: str) -> None:
        self._engine = engine
        self._provider_name = provider_name

    @property
    def name(self) -> str:
        return "lua"

    def is_active(self) -> bool:
        return self._engine.has_script(self._provider_name)

    def transform_request(self, request: UnifiedRequest) -> None:
        pass  # Lua hooks run on raw JSON at the proxy layer

    def transform_response(self, response: UnifiedResponse) -> None:
        pass  # Lua hooks run on raw JSON at the proxy layer

    def transform_stream_chunk(self, chunk: UnifiedStreamChunk) -> None:
        pass  # Lua hooks run on raw JSON at the proxy layer

    @property
    def engine(self) -> LuaEngine:
        return self._engine

    @property
    def provider_name(self) -> str:
        return self._provider_name
