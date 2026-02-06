# GCP Vertex Claude anthropic-beta 处理

## 行为

- Anthropic 与 GCP Vertex 默认不转发客户端的 `anthropic-beta`，避免未知 beta 值导致 400。
- 可通过 `provider_params` 配置 `anthropic_beta_policy` 控制转发行为：
  - `drop`（默认）：不转发。
  - `allowlist`：仅转发 `anthropic_beta_allowlist` 中允许的值（数组或逗号分隔字符串）。
  - `passthrough`：原样透传。
- 记录 provider_request_headers 时，仅包含策略过滤后的 `anthropic-beta`。

## 相关实现

- 统一策略实现（Rust）：[rust-server/src/core/header_policy.rs](../rust-server/src/core/header_policy.rs#L1)
- V2 代理请求构建（Rust）：[rust-server/src/api/proxy.rs](../rust-server/src/api/proxy.rs#L429)
- Claude 兼容请求构建（Rust）：[rust-server/src/api/claude.rs](../rust-server/src/api/claude.rs#L420)
- 统一策略实现（Python）：[python-server/app/core/header_policy.py](../python-server/app/core/header_policy.py#L1)
- GCP Vertex 请求构建（Python）：[python-server/app/api/gcp_vertex.py](../python-server/app/api/gcp_vertex.py#L166)

**Last Updated**: 2026-02-08
