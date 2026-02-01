# Gemini 3 thought_signature 对齐说明

## 目标

- 按 LiteLLM 逻辑识别 Gemini 3：基于 model 字符串包含 `gemini-3`。
- 请求侧：为 Gemini 3 的 tool_calls 写入 `provider_specific_fields.thought_signature`，缺失时注入 dummy signature；非 Gemini 模型会移除 tool_call_id 中的签名后缀。
- 请求侧：同步 `tool_calls.function.provider_specific_fields.thought_signature`，便于上游在 functionCall parts 校验 thought_signature。
- 请求侧：同步 `tool_calls.extra_content.google.thought_signature`，满足 Gemini functionCall parts 的签名校验。
- 请求侧：/v1/messages（OpenAI provider 路径）与 /v2 代理端点在发往 OpenAI provider 前应用同样归一化/剥离。
- 请求侧：补齐 `thinkingConfig`（从 `reasoning_effort` / `thinking_level` 映射），并为非 image 的 Gemini 3 模型补默认 `thinkingLevel`（Flash= `minimal`，其他= `low`）。
- 请求侧：转发到 OpenAI 兼容 Gemini 3 provider 前，会剥离 `thinkingConfig` / `thinking_level` / `thinking_config`，避免上游 schema 校验失败。
- 请求侧：Gemini 3 不支持 `frequency_penalty/presence_penalty`，会被剥离。
- 响应侧：解析 Gemini `parts` 中的 `thoughtSignature` 与 `thought: true`，映射到 `provider_specific_fields.thought_signatures`，并生成 `thinking_blocks` / `reasoning_content`。
- 响应侧：将 `extra_content.google.thought_signature` 映射到 `provider_specific_fields`，并把签名嵌入 tool_call_id，保证 OpenAI 客户端也能保留签名。
- Streaming：逐 chunk 做同样的 Gemini 3 归一化处理（含 parts/thinking 解析）。
- reasoning_effort：自动映射到 `thinking_level`（Gemini 3 Flash 支持 `minimal/medium`，其他 Gemini 3 模型按 LiteLLM 规则回退）。
- temperature：Gemini 3 未提供时默认补齐为 `1.0`。

## Python 路径

- 识别与归一化逻辑：[python-server/app/utils/gemini3.py](../python-server/app/utils/gemini3.py#L28)
- v1 请求/响应挂载点：[python-server/app/api/completions.py](../python-server/app/api/completions.py#L805)
- v1 messages（OpenAI provider 路径）：[python-server/app/api/claude.py](../python-server/app/api/claude.py#L265)
- v2 代理端点：[python-server/app/api/proxy.py](../python-server/app/api/proxy.py#L884)
- Streaming 归一化入口：[python-server/app/utils/streaming.py](../python-server/app/utils/streaming.py#L608)

## Rust 路径

- 识别与归一化逻辑：[rust-server/src/api/gemini3.rs](../rust-server/src/api/gemini3.rs#L10)
- v1 请求/响应挂载点：[rust-server/src/api/handlers.rs](../rust-server/src/api/handlers.rs#L1019)
- v1 messages（OpenAI provider 路径）：[rust-server/src/api/claude.rs](../rust-server/src/api/claude.rs#L297)
- v2 代理端点：[rust-server/src/api/proxy.rs](../rust-server/src/api/proxy.rs#L293)
- Streaming 归一化入口：[rust-server/src/api/streaming.rs](../rust-server/src/api/streaming.rs#L1222)

## 兼容说明

- 仍保留 `extra_content` 字段，不做删除；但面向客户端会优先补齐 `provider_specific_fields`。
- dummy signature 采用 `base64("skip_thought_signature_validator")`（Google 推荐方式）。

**Last Updated**: 2026-02-02
