# Gemini 3 thought_signature 对齐说明

## 目标

- 按 LiteLLM 逻辑识别 Gemini 3：基于 model 字符串包含 `gemini-3`。
- 请求侧：为 Gemini 3 的 tool_calls 写入 `provider_specific_fields.thought_signature`，缺失时注入 dummy signature；非 Gemini 模型会移除 tool_call_id 中的签名后缀。
- 响应侧：将 `extra_content.google.thought_signature` 映射到 `provider_specific_fields`，并把签名嵌入 tool_call_id，保证 OpenAI 客户端也能保留签名。
- Streaming：逐 chunk 做同样的 Gemini 3 归一化处理。
- reasoning_effort：自动映射到 `thinking_level`（Gemini 3 Flash 支持 `minimal/medium`，其他 Gemini 3 模型按 LiteLLM 规则回退）。
- temperature：Gemini 3 未提供时默认补齐为 `1.0`。

## Python 路径

- 识别与归一化逻辑：[python-server/app/utils/gemini3.py](../python-server/app/utils/gemini3.py#L27)
- v1 请求/响应挂载点：[python-server/app/api/completions.py](../python-server/app/api/completions.py#L819)
- Streaming 归一化入口：[python-server/app/utils/streaming.py](../python-server/app/utils/streaming.py#L488)

## Rust 路径

- 识别与归一化逻辑：[rust-server/src/api/gemini3.rs](../rust-server/src/api/gemini3.rs#L7)
- v1 请求/响应挂载点：[rust-server/src/api/handlers.rs](../rust-server/src/api/handlers.rs#L1105)
- Streaming 归一化入口：[rust-server/src/api/streaming.rs](../rust-server/src/api/streaming.rs#L1157)

## 兼容说明

- 仍保留 `extra_content` 字段，不做删除；但面向客户端会优先补齐 `provider_specific_fields`。
- dummy signature 采用 `base64("skip_thought_signature_validator")`（Google 推荐方式）。

**Last Updated**: 2026-01-31
