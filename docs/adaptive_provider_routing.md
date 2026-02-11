# Provider 429/5xx 自适应降权（Rust MVP）

## 行为

- 通过环境变量 `ADAPTIVE_ROUTING_ENABLED=true` 启用运行时自适应选路；默认关闭，保持原有静态权重行为。
- 启用后，选路使用 `effective_weight`：在静态权重基础上叠加错误惩罚、熔断状态和慢启动恢复。
- 对 `429`：优先读取 `Retry-After`，进入 provider 冷却期；连续 `429` 达阈值后进入 `open`。
- 对 `5xx` 与网络错误：连续失败触发 `open`，并按指数退避延长 `open` 时长。
- `open` 到期自动转 **`half_open`**；探测成功后回到 `closed`，并进入慢启动爬升。

## 状态机

- `closed`：正常选路。
- `open`：权重视为不可用（仅在所有 provider 都不可用时作为兜底探测）。
- `half_open`：低权重探测（默认约 20% 因子）。

## 指标

- `llm_proxy_provider_effective_weight{provider}`：当前生效权重。
- `llm_proxy_provider_circuit_state{provider,state}`：熔断状态 one-hot 指标。
- `llm_proxy_provider_ejections_total{provider,reason}`：按原因统计剔除次数。

## Grafana 面板建议

- 参考面板文档：[docs/grafana_adaptive_panels.md](../docs/grafana_adaptive_panels.md#L1)
- `Provider Effective Weight` 建议使用 `percentunit`（0~1 映射到 0%~100%）。
- `Circuit Breaker State` 建议先按 `provider` 聚合后编码成 `0/0.5/1`，避免标签匹配导致空时序。
- `Provider Ejections` 建议使用 `increase(...[5m])` 展示窗口增量。

## 相关实现

- 自适应权重与熔断核心：[rust-server/src/services/provider_service.rs](../rust-server/src/services/provider_service.rs#L408)
- 指标定义：[rust-server/src/core/metrics.rs](../rust-server/src/core/metrics.rs#L135)
- 统一错误类型常量：[rust-server/src/core/error_types.rs](../rust-server/src/core/error_types.rs#L5)
- 统一错误分类与剔除原因（常量+枚举）：[rust-server/src/core/error_types.rs](../rust-server/src/core/error_types.rs#L59)
- 统一认证入口（Bearer/MultiFormat）：[rust-server/src/api/auth.rs](../rust-server/src/api/auth.rs#L90)
- 统一上游请求构造（Auth/Header/JSON）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L37)
- 统一 GCP Vertex URL 构造：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L68)
- 统一成功 JSON 响应构造（状态码 + 扩展）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L251)
- 统一后端错误解析与兜底体构造：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L276)
- 统一 transport 错误响应构造（分类 + 协议错误体）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L357)
- 统一 transport 错误日志与错误响应编排：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L378)
- 统一上游 JSON 解析失败编排：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L429)
- 统一上游 JSON 解析失败日志编排：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L450)
- 统一上游 JSON 解析失败日志分流（仅返回错误响应）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L477)
- 统一状态码归一化与上游错误解析编排：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L508)
- 统一 status 错误响应编排（protocol/passthrough）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L526)
- 统一 status 错误分流（成功/错误分支）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L580)
- 统一 status 错误分流 + 结构化日志编排：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L625)
- 统一 status 错误分流（仅返回错误响应）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L686)
- 统一 status 分流意外成功兜底响应：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L720)
- 统一协议错误响应构造：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L327)
- 统一上游执行与事件回写：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L744)
- 统一上游执行 + transport 失败分流：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L770)
- 统一上游执行失败分流（仅返回错误响应）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L799)
- 统一 token usage 指标记录：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L831)
- 统一非流式成功响应收尾（Langfuse + JSONL + JSON）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L857)
- 统一响应扩展注入（Model/Provider/ApiKey）：[rust-server/src/api/upstream.rs](../rust-server/src/api/upstream.rs#L231)
- 统一 Langfuse 采样上报入口：[rust-server/src/core/langfuse.rs](../rust-server/src/core/langfuse.rs#L217)
- 统一 Langfuse 成功/失败收尾 helper：[rust-server/src/core/langfuse.rs](../rust-server/src/core/langfuse.rs#L226)
- V1 Chat/Completions 调用入口：[rust-server/src/api/handlers.rs](../rust-server/src/api/handlers.rs#L831)
- Claude 路由调用入口：[rust-server/src/api/claude.rs](../rust-server/src/api/claude.rs#L85)
- GCP Vertex 路由调用入口：[rust-server/src/api/gcp_vertex.rs](../rust-server/src/api/gcp_vertex.rs#L88)
- V2 代理路由调用入口：[rust-server/src/api/proxy.rs](../rust-server/src/api/proxy.rs#L119)

**Last Updated**: 2026-02-11
