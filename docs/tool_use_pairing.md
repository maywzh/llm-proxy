# tool_use/tool_result 补全规则

## 行为

- 在发送到上游之前，检查消息中是否存在孤立的 `tool_use` / `tool_call`。
- 对缺失的结果插入占位 `tool_result`，避免 Bedrock Converse 因顺序校验返回 400。

## Thinking 签名整流（无条件）

- 在发送到上游之前，无条件执行请求清洗：
  - 删除 `messages[*].content` 中的 `thinking` / `redacted_thinking` block。
  - 删除非 thinking block 上的 `signature` 字段。
  - 将空白 `text` 替换为 `.`。
  - 若 assistant 消息清洗后内容为空，回填占位文本 block。
  - 若顶层 `thinking.type=enabled` 且最后一条 assistant 工具链不再以 thinking 前缀开头，则删除顶层 `thinking`。
- 该策略用于 round-robin provider 场景，避免跨 provider 重放历史 thinking 签名导致校验失败。

## 相关实现

- V1 Claude 请求补全入口：[rust-server/src/api/claude.rs](../rust-server/src/api/claude.rs#L389)
- 补全逻辑实现：[rust-server/src/api/proxy.rs](../rust-server/src/api/proxy.rs#L698)
- Rust 请求清洗实现：[rust-server/src/api/rectifier.rs](../rust-server/src/api/rectifier.rs#L6)
- Rust GCP Vertex 入口复用清洗：[rust-server/src/api/gcp_vertex.rs](../rust-server/src/api/gcp_vertex.rs#L152)
- Python 请求清洗实现：[python-server/app/transformer/rectifier.py](../python-server/app/transformer/rectifier.py#L6)
- Python GCP Vertex 入口复用清洗：[python-server/app/api/gcp_vertex.py](../python-server/app/api/gcp_vertex.py#L459)

**Last Updated**: 2026-02-09
