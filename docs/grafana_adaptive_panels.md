# Grafana Adaptive Routing Panels

## TL;DR

- `Provider Effective Weight` 使用 `percentunit` 显示 0~1 因子。
- `Circuit Breaker State` 使用按 `provider` 聚合后的数值态，避免标签不匹配导致空时序。
- `Provider Ejections` 使用 `increase(...[5m])` 展示最近 5 分钟增量，避免累计值不直观。

## Dashboard 配置位置

- 面板定义文件：[k8s/dev/llm-proxy-grafana-dashboard-import.json](../k8s/dev/llm-proxy-grafana-dashboard-import.json#L2384)
- Prometheus 抓取配置：[k8s/dev/prometheus.yaml](../k8s/dev/prometheus.yaml#L18)

## Panel 查询与单位

### 1) Provider Effective Weight

- Query：

```promql
llm_proxy_provider_effective_weight{job=~".*-llm-proxy"}
```

- Unit：`percentunit`
- 目标语义：0.0~1.0 映射为 0%~100%。
- 可视化样式：对齐 `Provider Success Rate (%)`，并关闭填充（`fillOpacity=0`, `gradientMode=none`, `lineInterpolation=linear`, `thresholdsStyle=line`）。

### 2) Circuit Breaker State

- Query：

```promql
sum by (provider) (llm_proxy_provider_circuit_state{job=~".*-llm-proxy",state="closed"} * 0)
+ sum by (provider) (llm_proxy_provider_circuit_state{job=~".*-llm-proxy",state="open"} * 1)
+ sum by (provider) (llm_proxy_provider_circuit_state{job=~".*-llm-proxy",state="half_open"} * 0.5)
```

- 目标语义：`0=closed`，`0.5=half_open`，`1=open`。

### 3) Provider Ejections (5m Increase)

- Query：

```promql
sum by (provider, reason) (
  increase(llm_proxy_provider_ejections_total{job=~".*-llm-proxy"}[5m])
)
```

- 目标语义：展示最近 5 分钟剔除增量，便于识别瞬时恶化。

### 4) Top Degraded Providers

- Query：

```promql
topk(10, clamp_min(1 - max by (provider) (llm_proxy_provider_effective_weight{job=~".*-llm-proxy"}), 0))
```

- Unit：`percentunit`
- 目标语义：展示当前降权最严重的 provider（degraded factor 越高越差）。

## 指标语义说明

- 指标定义：
  - [rust-server/src/core/metrics.rs](../rust-server/src/core/metrics.rs#L134)
  - [python-server/app/core/metrics.py](../python-server/app/core/metrics.py#L87)
- 现状：`llm_proxy_provider_effective_weight` 在初始化阶段会写入静态权重，运行时多处写入恢复因子：
  - Rust 初始化写入：[rust-server/src/services/provider_service.rs](../rust-server/src/services/provider_service.rs#L205)
  - Rust 运行时更新：[rust-server/src/services/provider_service.rs](../rust-server/src/services/provider_service.rs#L311)
  - Python 初始化写入：[python-server/app/services/provider_service.py](../python-server/app/services/provider_service.py#L150)
  - Python 运行时更新：[python-server/app/services/provider_service.py](../python-server/app/services/provider_service.py#L210)

**Last Updated**: 2026-02-11
