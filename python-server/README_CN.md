# LLM Proxy - Python 服务

[![Python 3.12+](https://img.shields.io/badge/python-3.12+-blue.svg)](https://www.python.org/downloads/)
[![FastAPI](https://img.shields.io/badge/FastAPI-0.110+-green.svg)](https://fastapi.tiangolo.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

中文文档 | [English](README.md)

基于 FastAPI 的高性能 LLM API 代理服务，支持加权负载均衡、完整的 Prometheus 监控和 Grafana 可视化。

> 完整项目概述请参阅[主 README](../README_CN.md)

## 📋 目录

- [核心特性](#-核心特性)
- [技术栈](#-技术栈)
- [快速开始](#-快速开始)
- [配置](#️-配置)
- [使用方法](#-使用方法)
- [Admin API](#-admin-api)
- [速率限制](#️-速率限制)
- [监控](#-监控)
- [项目结构](#-项目结构)
- [开发指南](#️-开发指南)
- [许可证](#-许可证)

## ✨ 核心特性

- ✅ **加权负载均衡** - 智能的加权轮询算法，支持按权重分配请求
- ✅ **流式响应** - 完整的 SSE 流式响应支持
- ✅ **OpenAI 兼容** - 100% 兼容 OpenAI API 格式
- ✅ **跨协议转换** - 接受任意协议格式（OpenAI、Anthropic、Response API）并路由到任意提供商
- ✅ **V2 API 端点** - 新增支持完整跨协议转换的端点
- ✅ **模型映射** - 灵活的模型名称转换和路由
- ✅ **Prometheus 监控** - 完整的指标收集和导出
- ✅ **Grafana 可视化** - 预配置的仪表盘和告警
- ✅ **Token 统计** - 精确的 token 使用量追踪（使用 tiktoken）
- ✅ **延迟追踪** - P50/P95/P99 延迟百分位数监控
- ✅ **健康检查** - Provider 健康状态实时监控
- ✅ **模块化架构** - 清晰的分层架构设计
- ✅ **类型安全** - Pydantic 2.0+ 数据验证
- ✅ **速率限制** - 可选的按 Key 速率限制
- ✅ **动态配置** - 基于 PostgreSQL 的热重载配置
- ✅ **异步处理** - FastAPI + httpx 全异步架构
- ✅ **Langfuse 集成** - 可选的 LLM 可观测性和追踪
- ✅ **JSONL 日志** - 可选的异步 JSONL 文件日志用于调试

## 🔧 技术栈

### 核心框架
- **Web 框架**: FastAPI 0.110+ - 高性能异步 Python Web 框架
- **ASGI 服务器**: Uvicorn - 生产级 ASGI 服务器
- **Python 版本**: Python 3.12+

### 数据处理
- **数据验证**: Pydantic 2.0+ - 类型安全的数据模型和验证
- **数据库 ORM**: SQLAlchemy 2.0+ - 异步 ORM
- **数据库驱动**: asyncpg - 高性能异步 PostgreSQL 驱动

### HTTP 与网络
- **HTTP 客户端**: httpx - 异步 HTTP 客户端
- **流式处理**: SSE (Server-Sent Events)

### 监控与日志
- **指标收集**: prometheus-client - Prometheus 官方 Python 客户端
- **日志系统**: loguru - 现代化的 Python 日志库
- **Token 计数**: tiktoken - OpenAI 官方 token 计数库

### 安全与限流
- **速率限制**: limits 3.10+ - 令牌桶算法实现
- **认证**: Bearer Token 认证

### 开发工具
- **包管理**: uv - 极速的 Python 包管理器
- **测试框架**: pytest + pytest-asyncio + pytest-cov
- **测试工具**: hypothesis（属性测试）+ respx（HTTP mock）

## 🚀 快速开始

### 前置要求

- Python 3.12+
- PostgreSQL 数据库
- uv（Python 包管理器）

### 1. 安装依赖

```bash
# 安装 uv（如果还没有安装）
curl -LsSf https://astral.sh/uv/install.sh | sh

# 同步依赖
uv sync
```

### 2. 配置环境变量

创建 `.env` 文件或设置环境变量：

```bash
# 必需：数据库连接
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# 必需：Admin API 认证密钥
export ADMIN_KEY='your-admin-key'

# 可选：服务端口（默认 18000）
export PORT=18000

# 可选：模型名称前缀（用于统一模型名称格式）
export PROVIDER_SUFFIX='Proxy'
```

### 3. 运行数据库迁移

```bash
# 安装 golang-migrate
brew install golang-migrate

# 设置数据库 URL
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# 运行迁移
../scripts/db_migrate.sh up

# 查看迁移版本
../scripts/db_migrate.sh version

# 回滚一个迁移
../scripts/db_migrate.sh down
```

### 4. 启动服务

**方式一：直接运行**
```bash
# 使用快速启动脚本
./run.sh

# 或使用 uv
uv run python main.py
```

**方式二：Docker Compose（推荐，包含监控）**
```bash
# 启动所有服务（LLM Proxy + Prometheus + Grafana）
docker-compose up -d

# 查看日志
docker-compose logs -f llm-proxy

# 停止服务
docker-compose down
```

**服务访问地址：**
- LLM Proxy: <http://localhost:18000>
- Prometheus: <http://localhost:9090>
- Grafana: <http://localhost:3000>（admin/admin）
- API 文档: <http://localhost:18000/docs>

## ⚙️ 配置

详细配置文档请参阅[主 README](../README_CN.md#-配置)。

## 📖 使用方法

代理服务启动后，可以像调用 OpenAI API 一样使用：

### Chat Completions

```bash
curl http://localhost:18000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### 流式响应

```bash
curl http://localhost:18000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "gpt-3.5-turbo",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

### 模型名称前缀功能

当设置了 `PROVIDER_SUFFIX` 环境变量时，可以使用带前缀的模型名称：

```bash
# 设置前缀
export PROVIDER_SUFFIX=Proxy

# 以下两种请求是等价的：
# 1. 使用带前缀的模型名
curl http://localhost:18000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Proxy/gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# 2. 使用原始模型名
curl http://localhost:18000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

#### 前缀功能行为说明

- 如果未设置 `PROVIDER_SUFFIX`，模型名称保持原样
- 如果设置了 `PROVIDER_SUFFIX`（例如 "Proxy"）：
  - `Proxy/gpt-4` → `gpt-4`（去除前缀）
  - `gpt-4` → `gpt-4`（保持不变）
  - `Other/gpt-4` → `Other/gpt-4`（不同前缀，保持不变）

这个功能适用于需要统一模型名称格式的场景，特别是在多个代理服务之间切换时。

### 健康检查

```bash
# 基础健康检查
curl http://localhost:18000/health

# 详细健康检查（测试所有 provider）
curl http://localhost:18000/health/detailed
```

### 支持的端点

**V1 API（传统）**
- `/v1/chat/completions` - Chat 接口
- `/v1/completions` - Completions 接口
- `/v1/models` - 列出所有可用模型

**V2 API（跨协议支持）**
- `/v2/chat/completions` - OpenAI 兼容，支持跨协议转换
- `/v2/messages` - Anthropic 兼容，支持跨协议转换
- `/v2/responses` - Response API，支持跨协议转换

**其他端点**
- `/health` - 基础健康检查
- `/health/detailed` - 详细健康检查（测试所有 provider）
- `/metrics` - Prometheus 指标端点
- `/docs` - OpenAPI 文档

### V2 API：跨协议转换

V2 API 端点支持跨协议转换，允许您以任何支持的格式发送请求并路由到任何提供商：

```bash
# 向 Anthropic 提供商发送 OpenAI 请求
curl http://localhost:18000/v2/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "claude-3-opus",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# 向 OpenAI 提供商发送 Anthropic 请求
curl http://localhost:18000/v2/messages \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "gpt-4",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# 向任意提供商发送 Response API 请求
curl http://localhost:18000/v2/responses \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_CREDENTIAL_KEY" \
  -d '{
    "model": "gpt-4",
    "instructions": "You are a helpful assistant.",
    "input": "Hello!",
    "max_output_tokens": 1000
  }'
```

**工作原理：**
1. 代理从请求格式检测客户端协议
2. 将请求转换为提供商的原生协议
3. 将请求转发到选定的提供商
4. 将响应转换回客户端期望的格式

**性能：**
- 同协议请求使用旁路优化（最小开销）
- 跨协议请求使用完整转换管道
- 指标跟踪旁路与跨协议使用情况

## 🔑 Admin API

### Provider 管理

```bash
# 设置 Admin Key
export ADMIN_KEY='your-admin-key'

# 创建 Provider
curl -X POST http://localhost:18000/admin/v1/providers \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "provider_key": "openai-main",
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
curl http://localhost:18000/admin/v1/providers/1 \
  -H "Authorization: Bearer $ADMIN_KEY"

# 更新 Provider
curl -X PUT http://localhost:18000/admin/v1/providers/1 \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-new-key",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "is_enabled": true
  }'

# 删除 Provider
curl -X DELETE http://localhost:18000/admin/v1/providers/1 \
  -H "Authorization: Bearer $ADMIN_KEY"
```

### Credential 管理

```bash
# 创建 Credential
curl -X POST http://localhost:18000/admin/v1/credentials \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "key": "sk-my-secret-key",
    "name": "Default Key",
    "allowed_models": ["*"],
    "is_enabled": true
  }'

# 列出所有 Credential
curl http://localhost:18000/admin/v1/credentials \
  -H "Authorization: Bearer $ADMIN_KEY"

# 重新加载配置（热更新）
curl -X POST http://localhost:18000/admin/v1/config/reload \
  -H "Authorization: Bearer $ADMIN_KEY"

# 获取当前配置版本
curl http://localhost:18000/admin/v1/config/version \
  -H "Authorization: Bearer $ADMIN_KEY"
```

## ⏱️ 速率限制

系统支持为每个 Credential Key 配置独立的速率限制，也可以完全禁用速率限制。

### 配置方式

通过 Admin API 创建 Credential 时配置速率限制：

```bash
# 创建带速率限制的 Key
curl -X POST http://localhost:18000/admin/v1/credentials \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "key": "sk-limited",
    "name": "Limited Key",
    "rate_limit": 100,
    "is_enabled": true
  }'

# 创建无速率限制的 Key（rate_limit 设为 null 或不设置）
curl -X POST http://localhost:18000/admin/v1/credentials \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "key": "sk-unlimited",
    "name": "Unlimited Key",
    "is_enabled": true
  }'
```

### 行为说明

| 配置 | 行为 |
|------|------|
| `rate_limit: 100` | 启用速率限制：每秒 100 个请求 |
| `rate_limit: 0` | 启用速率限制：阻止所有请求 |
| 不设置 `rate_limit` 字段 | 禁用速率限制：允许无限请求 |

### 使用场景

- **生产环境 Key**：设置合理的速率限制，防止滥用
- **开发/测试 Key**：可以不设置速率限制，方便开发调试
- **特殊用途 Key**：根据实际需求灵活配置

## 🔍 Langfuse 集成

LLM Proxy 支持可选的 [Langfuse](https://langfuse.com) 集成，用于 LLM 可观测性和追踪。

### 功能特性

- **请求追踪**：捕获提供商信息、请求/响应数据、token 使用量
- **TTFT 追踪**：流式请求的首 token 时间指标
- **采样支持**：可配置的采样率，适用于高流量场景
- **后台批处理**：异步批处理，最小化延迟影响

### 配置

设置以下环境变量以启用 Langfuse：

| 变量 | 描述 | 默认值 |
|------|------|--------|
| `LANGFUSE_ENABLED` | 启用 Langfuse 追踪 | `false` |
| `LANGFUSE_PUBLIC_KEY` | Langfuse 公钥（启用时必需） | - |
| `LANGFUSE_SECRET_KEY` | Langfuse 密钥（启用时必需） | - |
| `LANGFUSE_HOST` | Langfuse 服务器 URL | `https://cloud.langfuse.com` |
| `LANGFUSE_SAMPLE_RATE` | 采样率（0.0-1.0） | `1.0` |
| `LANGFUSE_FLUSH_INTERVAL` | 刷新间隔（秒） | `5` |
| `LANGFUSE_DEBUG` | 启用调试日志 | `false` |

### 启用 Langfuse

1. 在 [Langfuse](https://langfuse.com) 注册并创建项目
2. 从项目设置中获取公钥和密钥
3. 设置环境变量：

```bash
export LANGFUSE_ENABLED=true
export LANGFUSE_PUBLIC_KEY=pk-lf-...
export LANGFUSE_SECRET_KEY=sk-lf-...
```

4. 重启服务器

追踪将出现在您的 Langfuse 仪表板中，包含：
- 提供商信息（哪个提供商处理了请求）
- 模型映射（原始模型名 vs 映射模型名）
- Token 使用量（提示、完成、总计）
- 时间指标（持续时间、流式的 TTFT）
- 错误详情（如果请求失败）

## 📝 JSONL 日志

LLM Proxy 支持可选的 JSONL 文件日志，用于调试和分析。请求和响应记录为单独的 JSONL 行，通过 `request_id` 关联。

### 功能特性

- **异步日志**：非阻塞的缓冲写入，定期刷新
- **独立记录**：客户端请求、提供商请求和响应分别记录
- **流式支持**：捕获流式响应的完整块序列
- **关联记录**：所有记录共享相同的 `request_id` 以便关联

### 配置

设置以下环境变量以启用 JSONL 日志：

| 变量 | 描述 | 默认值 |
|------|------|--------|
| `JSONL_LOG_ENABLED` | 启用 JSONL 日志 | `false` |
| `JSONL_LOG_PATH` | JSONL 日志文件路径 | `./logs/requests.jsonl` |
| `JSONL_LOG_BUFFER_SIZE` | 队列缓冲区大小 | `1000` |

### 启用 JSONL 日志

```bash
export JSONL_LOG_ENABLED=true
export JSONL_LOG_PATH=./logs/requests.jsonl
export JSONL_LOG_BUFFER_SIZE=1000
```

### 日志记录类型

每个 JSONL 行包含以下记录类型之一：

1. **`request`** - 收到客户端请求
   ```json
   {
     "type": "request",
     "timestamp": "2026-01-23T19:30:00.000Z",
     "request_id": "req-123",
     "endpoint": "/v2/chat/completions",
     "provider": "openai-main",
     "payload": {...}
   }
   ```

2. **`provider_request`** - 发送到提供商的请求
   ```json
   {
     "type": "provider_request",
     "timestamp": "2026-01-23T19:30:00.100Z",
     "request_id": "req-123",
     "provider": "openai-main",
     "api_base": "https://api.openai.com/v1",
     "endpoint": "/chat/completions",
     "payload": {...}
   }
   ```

3. **`provider_response`** - 来自提供商的响应
   ```json
   {
     "type": "provider_response",
     "timestamp": "2026-01-23T19:30:01.000Z",
     "request_id": "req-123",
     "provider": "openai-main",
     "status_code": 200,
     "body": {...}
   }
   ```

4. **`response`** - 发送给客户端的响应
   ```json
   {
     "type": "response",
     "timestamp": "2026-01-23T19:30:01.100Z",
     "request_id": "req-123",
     "status_code": 200,
     "body": {...}
   }
   ```

对于流式响应，`body` 被替换为包含所有 SSE 块的 `chunk_sequence`。

## 📊 监控

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

## 📁 项目结构

```
app/
├── api/          # API 路由
├── core/         # 核心功能（配置、安全、监控）
├── models/       # Pydantic 数据模型
├── services/     # 业务逻辑层
└── utils/        # 工具函数

grafana/          # Grafana 配置和 Dashboard
prometheus/       # Prometheus 配置
```

详细架构说明见 [REFACTORING.md](REFACTORING.md)

## 🛠️ 开发指南

### 运行测试

```bash
# 运行所有测试
make test

# 生成覆盖率报告
make coverage

# 运行特定测试文件
pytest tests/test_specific.py -v
```

### 代码质量

```bash
# 格式化代码
make format

# 代码检查
make lint

# 类型检查
mypy app
```

### Docker 开发

```bash
# 构建 Docker 镜像
docker build -t llm-proxy:dev .

# 使用 Docker Compose 运行
docker-compose up -d

# 查看日志
docker-compose logs -f
```

## 工作原理

1. 代理从数据库读取多个 API 提供商配置
2. 使用加权随机算法选择提供商
3. 将请求转发到选中的提供商
4. 返回提供商的响应给客户端

根据配置的权重，请求会按比例分配到不同的提供商，实现负载均衡。

## 注意事项

- 确保所有提供商使用相同的 API 格式（默认 OpenAI 格式）
- API key 需要有效且有足够的配额
- 建议在生产环境中配置 Grafana 告警规则

## 相关文档

- [主 README](../README_CN.md) - 完整项目文档
- [REFACTORING.md](REFACTORING.md) - 架构设计说明
- [MONITORING.md](MONITORING.md) - 监控系统文档
- [DOCKER_USAGE.md](DOCKER_USAGE.md) - Docker 使用指南

## 📄 许可证

MIT License
