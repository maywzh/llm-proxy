# llm-proxy

llm-proxy 是一个高性能的 LLM API 代理服务，支持加权负载均衡、完整的 Prometheus 监控和 Grafana 可视化。

## 功能特性

- ✅ 加权负载均衡（Weighted Load Balancing）
- ✅ 支持流式和非流式响应
- ✅ 兼容 OpenAI API 格式
- ✅ 模型名称映射
- ✅ **Prometheus 指标收集**
- ✅ **Grafana Dashboard 可视化**
- ✅ Token 使用量统计
- ✅ 请求延迟追踪
- ✅ Provider 健康监控
- ✅ 模块化架构设计
- ✅ 类型安全（Pydantic）
- ✅ **可选的 Master Key 速率限制**
- ✅ **动态配置模式（数据库存储）**

## 动态配置模式

LLM Proxy 支持两种配置模式：

### YAML 模式（默认）
- 不设置 `DB_URL` 环境变量
- 使用 `config.yaml` 文件配置
- 适合开发和简单部署
- 配置变更需要重启服务

### 数据库模式
- 设置 `DB_URL` 和 `ADMIN_KEY` 环境变量
- 配置存储在 PostgreSQL 数据库
- 支持运行时热更新，无需重启
- 适合生产环境
- 通过 Admin API 管理配置

### 动态配置环境变量

| 变量 | 说明 | 必需 |
|------|------|------|
| `DB_URL` | PostgreSQL 连接字符串 | 数据库模式必需 |
| `ADMIN_KEY` | Admin API 认证密钥 | 数据库模式必需 |
| `PORT` | 服务端口 | 否（默认 18000）|

### 数据库迁移

```bash
# 安装 golang-migrate
brew install golang-migrate

# 设置数据库 URL
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy?sslmode=disable'

# 运行迁移
./scripts/db_migrate.sh up

# 查看迁移版本
./scripts/db_migrate.sh version

# 回滚一个迁移
./scripts/db_migrate.sh down
```

### 迁移现有 YAML 配置到数据库

```bash
# 设置环境变量
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy?sslmode=disable'

# 运行迁移脚本
./scripts/migrate_config.sh config.yaml
```

### Admin API 示例

```bash
# 设置 Admin Key
export ADMIN_KEY='your-admin-key'

# 创建 Provider
curl -X POST http://localhost:18000/admin/v1/providers \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "openai-main",
    "provider_type": "openai",
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-xxx",
    "model_mapping": {},
    "is_enabled": true
  }'

# 列出所有 Provider
curl http://localhost:18000/admin/v1/providers \
  -H "Authorization: Bearer $ADMIN_KEY"

# 获取指定 Provider
curl http://localhost:18000/admin/v1/providers/openai-main \
  -H "Authorization: Bearer $ADMIN_KEY"

# 更新 Provider
curl -X PUT http://localhost:18000/admin/v1/providers/openai-main \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-new-key",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "is_enabled": true
  }'

# 删除 Provider
curl -X DELETE http://localhost:18000/admin/v1/providers/openai-main \
  -H "Authorization: Bearer $ADMIN_KEY"

# 创建 Master Key
curl -X POST http://localhost:18000/admin/v1/master-keys \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "id": "key-1",
    "key": "mk-xxx",
    "name": "Default Key",
    "allowed_models": ["*"],
    "is_enabled": true
  }'

# 列出所有 Master Key
curl http://localhost:18000/admin/v1/master-keys \
  -H "Authorization: Bearer $ADMIN_KEY"

# 重新加载配置（热更新）
curl -X POST http://localhost:18000/admin/v1/config/reload \
  -H "Authorization: Bearer $ADMIN_KEY"

# 获取当前配置版本
curl http://localhost:18000/admin/v1/config/version \
  -H "Authorization: Bearer $ADMIN_KEY"
```

---

## 快速开始

### 1. 安装依赖

使用 uv 安装依赖：

```bash
# 安装 uv（如果还没有安装）
curl -LsSf https://astral.sh/uv/install.sh | sh

# 同步依赖
uv sync
```

### 2. 配置 config.yaml

编辑 `config.yaml` 文件，添加你的 API 提供商：

```yaml
providers:
  - name: "provider1"
    api_base: "https://api.openai.com/v1"
    api_key: "sk-your-api-key-1"
    
  - name: "provider2"
    api_base: "https://api.openai.com/v1"
    api_key: "sk-your-api-key-2"

# Master API key configuration
master_keys:
  # Key with rate limiting
  - name: "Production Key"
    key: "sk-prod-key"
    rate_limit:
      requests_per_second: 100
      burst_size: 150
  
  # Key without rate limiting (unlimited requests)
  - name: "Unlimited Key"
    key: "sk-unlimited-key"
    # No rate_limit field = no rate limiting

server:
  host: "0.0.0.0"
  port: 8000
```

### 3. 启动代理服务

#### 方式一：直接运行

```bash
# 使用快速启动脚本
./run.sh

# 或使用 uv
uv run python main.py --config config.yaml
```

#### 方式二：使用 Docker Compose（推荐，包含监控）

```bash
# 启动所有服务（LLM Proxy + Prometheus + Grafana）
docker-compose up -d

# 查看日志
docker-compose logs -f llm-proxy

# 停止服务
docker-compose down
```

**服务访问地址：**
- LLM Proxy: http://localhost:18000
- Prometheus: http://localhost:9090
- Grafana: http://localhost:3000 (admin/admin)

## 使用方法

代理服务启动后，可以像调用 OpenAI API 一样使用：

### Chat Completions

```bash
curl http://localhost:8000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### 流式响应

```bash
curl http://localhost:8000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

### 健康检查

```bash
curl http://localhost:8000/health
```

## 工作原理

1. 代理从 `config.yaml` 读取多个 API 提供商配置
2. 使用 round-robin 算法循环选择提供商
3. 将请求转发到选中的提供商
4. 返回提供商的响应给客户端

每个请求会依次使用不同的提供商，实现负载均衡。

## 支持的端点

- `/v1/chat/completions` - Chat 接口
- `/v1/completions` - Completions 接口
- `/v1/models` - 列出所有可用模型
- `/health` - 基础健康检查
- `/health/detailed` - 详细健康检查（测试所有 provider）
- `/metrics` - Prometheus 指标端点
- `/docs` - OpenAPI 文档

## Master Key 速率限制

系统支持为每个 Master Key 配置独立的速率限制，也可以完全禁用速率限制。

### 配置方式

**启用速率限制：**
```yaml
master_keys:
  - name: "Limited Key"
    key: "sk-limited-key"
    rate_limit:
      requests_per_second: 100  # 每秒最多 100 个请求
      burst_size: 150           # 允许的突发请求数
```

**禁用速率限制（无限制）：**
```yaml
master_keys:
  - name: "Unlimited Key"
    key: "sk-unlimited-key"
    # 不设置 rate_limit 字段 = 无速率限制
```

### 行为说明

| 配置 | 行为 |
|------|------|
| `rate_limit: {requests_per_second: 100, burst_size: 150}` | 启用速率限制：每秒 100 个请求，允许 150 个突发请求 |
| `rate_limit: {requests_per_second: 0, burst_size: 0}` | 启用速率限制：阻止所有请求 |
| 不设置 `rate_limit` 字段 | 禁用速率限制：允许无限请求 |

### 使用场景

- **生产环境 Key**：设置合理的速率限制，防止滥用
- **开发/测试 Key**：可以不设置速率限制，方便开发调试
- **特殊用途 Key**：根据实际需求灵活配置

## 监控功能

### Prometheus 指标

系统自动收集以下指标：

- **请求指标**
  - `llm_proxy_requests_total`: 总请求数（按 method、endpoint、model、provider、status_code）
  - `llm_proxy_request_duration_seconds`: 请求延迟直方图
  - `llm_proxy_active_requests`: 当前活跃请求数

- **Token 使用指标**
  - `llm_proxy_tokens_total`: Token 使用总量（按 model、provider、token_type）

- **Provider 健康指标**
  - `llm_proxy_provider_health`: Provider 健康状态
  - `llm_proxy_provider_latency_seconds`: Provider 响应延迟

### Grafana Dashboard

预配置的 Dashboard 包含：
- 请求速率趋势
- P95/P99 延迟
- Token 使用量统计
- 状态码分布
- Provider 负载分布
- 实时活跃请求数

详细文档见 [MONITORING.md](MONITORING.md)

## 项目结构

```
app/
├── api/          # API routes
├── core/         # 核心功能（配置、安全、监控）
├── models/       # Pydantic 数据模型
├── services/     # 业务逻辑层
└── utils/        # 工具函数

grafana/          # Grafana 配置和 Dashboard
prometheus/       # Prometheus 配置
```

详细重构说明见 [REFACTORING.md](REFACTORING.md)

## 注意事项

- 确保所有提供商使用相同的 API 格式（默认 OpenAI 格式）
- API key 需要有效且有足够的配额
- 建议在生产环境中配置 Grafana 告警规则

## 相关文档

- [REFACTORING.md](REFACTORING.md) - 重构说明和架构设计
- [MONITORING.md](MONITORING.md) - 监控系统详细文档
- [DOCKER_USAGE.md](DOCKER_USAGE.md) - Docker 使用指南

## License

MIT
