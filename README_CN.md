# llm-proxy

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.12+](https://img.shields.io/badge/python-3.12+-blue.svg)](https://www.python.org/downloads/)
[![Rust 1.85+](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)

[English](README.md) | 中文文档

高性能、OpenAI 兼容的 LLM API 代理服务,支持加权负载均衡、流式响应和内置可观测性。本仓库包含两个功能对等的一流实现:

- **Python 服务** ([python-server/](python-server/)) - 基于 FastAPI + Uvicorn,快速开发和部署
- **Rust 服务** ([rust-server/](rust-server/)) - 基于 Axum + Tokio,极致性能和资源效率

两个实现都提供相同的端点 (`/v1/chat/completions`, `/v1/completions`, `/v1/models`, `/health`, `/health/detailed`, `/metrics`) 并共享相同的配置概念(提供商、加权路由、模型映射、Master Key、SSL 验证开关)。

## 📋 目录

- [核心特性](#-核心特性)
- [技术架构](#️-技术架构)
- [快速开始](#-快速开始)
- [配置说明](#️-配置说明)
- [动态配置模式](#️-动态配置模式)
- [监控系统](#-监控系统)
- [性能对比](#-性能对比)
- [项目结构](#-项目结构)
- [开发指南](#️-开发指南)
- [许可证](#-许可证)

## ✨ 核心特性

### 负载均衡与路由
- ⚖️ **加权轮询** - 跨多个 API 提供商的智能负载分配
- 🔄 **模型映射** - 灵活的模型名称转换和路由
- 🎯 **智能选择** - 基于提供商权重和健康状态的自动选择

### API 兼容性
- 🔌 **OpenAI 兼容** - 完全兼容 OpenAI API 格式
- 📡 **流式支持** - SSE 实时流式响应
- 🔄 **非流式支持** - 标准 JSON 响应模式

### 安全与认证
- 🔐 **Master Key 认证** - 统一的 API 密钥管理
- 🚦 **速率限制** - 可选的每键速率限制(支持突发流量)
- 🔓 **灵活认证** - 支持无限制密钥用于开发环境

### 可观测性
- 📊 **Prometheus 指标** - 完整的指标收集和导出
- 📈 **Grafana 仪表盘** - 预配置的可视化面板
- 💊 **健康检查** - 基础和详细的健康检查端点
- 📝 **请求追踪** - 详细的请求/响应日志

### 配置管理
- 🗄️ **动态配置** - 基于 PostgreSQL 的运行时配置
- 🔥 **热重载** - 无需重启的配置更新
- 📝 **YAML 模式** - 简单的文件配置模式
- 🔧 **Admin API** - RESTful 配置管理接口

### 部署选项
- 🐳 **Docker 支持** - 完整的容器化解决方案
- ☸️ **Kubernetes 清单** - 开发环境部署示例
- 📦 **Docker Compose** - 一键启动完整监控栈
- 🚀 **二进制部署** - 独立可执行文件(Rust 版本)

## 🏗️ 技术架构

### Python 实现 (FastAPI)

**核心技术栈:**
- **Web 框架**: FastAPI 0.110+ (高性能异步框架)
- **ASGI 服务器**: Uvicorn (生产级异步服务器)
- **HTTP 客户端**: httpx (异步 HTTP 客户端)
- **数据验证**: Pydantic 2.0+ (类型安全的数据模型)
- **数据库**: PostgreSQL + SQLAlchemy 2.0 + asyncpg (异步 ORM)
- **监控**: prometheus-client (指标收集)
- **日志**: loguru (结构化日志)
- **速率限制**: limits 3.10+ (令牌桶算法)
- **Token 计数**: tiktoken (精确的 token 使用统计)

**项目结构:**
```
python-server/
├── app/
│   ├── api/          # API 路由和端点
│   │   ├── admin.py      # Admin API (动态配置管理)
│   │   ├── chat.py       # Chat completions 端点
│   │   ├── completions.py # Text completions 端点
│   │   ├── health.py     # 健康检查端点
│   │   ├── models.py     # 模型列表端点
│   │   └── metrics.py    # Prometheus 指标端点
│   ├── core/         # 核心功能
│   │   ├── config.py     # 配置加载和管理
│   │   ├── database.py   # 数据库连接和操作
│   │   ├── security.py   # 认证和授权
│   │   ├── rate_limiter.py # 速率限制器
│   │   ├── metrics.py    # Prometheus 指标定义
│   │   ├── logging.py    # 日志配置
│   │   ├── http_client.py # HTTP 客户端封装
│   │   ├── middleware.py # 中间件
│   │   └── exceptions.py # 自定义异常
│   ├── models/       # 数据模型
│   │   ├── config.py     # 配置相关模型
│   │   ├── provider.py   # 提供商模型
│   │   └── health.py     # 健康检查模型
│   ├── services/     # 业务逻辑
│   │   ├── provider_service.py # 提供商选择和管理
│   │   └── health_check_service.py # 健康检查服务
│   └── utils/        # 工具函数
│       └── streaming.py  # SSE 流式响应处理
├── tests/           # 测试套件
├── grafana/         # Grafana 配置和仪表盘
└── prometheus/      # Prometheus 配置
```

### Rust 实现 (Axum)

**核心技术栈:**
- **Web 框架**: Axum 0.7 (基于 Tokio 的高性能框架)
- **异步运行时**: Tokio 1.x (Rust 生态最流行的异步运行时)
- **HTTP 客户端**: reqwest 0.11 (异步 HTTP 客户端)
- **序列化**: serde + serde_json (零开销序列化)
- **数据库**: SQLx 0.8 (编译时检查的 SQL 客户端)
- **监控**: prometheus 0.13 (官方 Rust 客户端)
- **日志**: tracing + tracing-subscriber (结构化追踪)
- **速率限制**: governor 0.7 (高效的速率限制器)
- **Token 计数**: tiktoken-rs (Rust 移植版本)
- **热重载**: arc-swap 1.7 (无锁配置更新)

**项目结构:**
```
rust-server/
└── src/
    ├── main.rs       # 应用程序入口
    ├── lib.rs        # 库入口
    ├── api/          # API 层
    │   ├── handlers.rs   # 请求处理器
    │   ├── health.rs     # 健康检查
    │   ├── models.rs     # API 数据模型
    │   ├── streaming.rs  # SSE 流式响应
    │   └── admin.rs      # Admin API
    ├── core/         # 核心功能
    │   ├── config.rs     # 配置加载
    │   ├── database.rs   # 数据库操作
    │   ├── error.rs      # 错误处理
    │   ├── metrics.rs    # Prometheus 指标
    │   ├── middleware.rs # 中间件
    │   ├── logging.rs    # 日志配置
    │   └── rate_limiter.rs # 速率限制
    └── services/     # 业务逻辑
        ├── provider_service.rs # 提供商服务
        └── health_check_service.rs # 健康检查服务
```

## 🚀 快速开始

### 方式 1: Python 服务 (推荐用于快速开发)

**1. 安装依赖:**
```bash
cd python-server
# 安装 uv (如果还没有安装)
curl -LsSf https://astral.sh/uv/install.sh | sh
# 同步依赖
uv sync
```

**2. 设置数据库和环境变量:**
```bash
# 设置环境变量
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'
export ADMIN_KEY='your-admin-key'
export PORT=18000

# 运行数据库迁移
./scripts/db_migrate.sh up
```

**3. 启动服务:**
```bash
# 使用快速启动脚本
./run.sh

# 或使用 uv
uv run python main.py
```

**4. 使用 Docker Compose (包含 Prometheus + Grafana):**
```bash
docker-compose up -d
# LLM Proxy: http://localhost:18000
# Prometheus: http://localhost:9090
# Grafana: http://localhost:3000 (admin/admin)
```

**5. 运行测试:**
```bash
make test       # 运行所有测试
make coverage   # 生成覆盖率报告
```

更多细节: [python-server/README.md](python-server/README.md)

### 方式 2: Rust 服务 (推荐用于生产环境)

**1. 构建项目:**
```bash
cd rust-server
cargo build --release
```

**2. 设置环境变量:**
```bash
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'
export ADMIN_KEY='your-admin-key'
export PORT=18000

# 运行数据库迁移
./scripts/db_migrate.sh up
```

**3. 启动服务:**
```bash
# 直接运行
CONFIG_PATH=config.yaml cargo run --release

# 使用已构建的二进制
./target/release/llm-proxy-rust
```

**4. 使用 Docker:**
```bash
# 构建镜像
docker build -t llm-proxy-rust:latest .

# 运行容器
docker run -p 18000:18000 \
  -v $(pwd)/config.yaml:/app/config.yaml \
  -e CONFIG_PATH=/app/config.yaml \
  -e DB_URL='postgresql://user:pass@localhost:5432/llm_proxy' \
  -e ADMIN_KEY='your-admin-key' \
  llm-proxy-rust:latest
```

**5. 运行测试:**
```bash
cargo test
cargo clippy  # 代码检查
cargo fmt     # 代码格式化
```

更多细节: [rust-server/README.md](rust-server/README.md)

## ⚙️ 配置说明

系统支持两种配置模式,两种实现共享相同的配置格式:

### 环境变量优先级

```
环境变量 > .env 文件 > YAML 配置文件
```

### 核心配置字段

```yaml
# 提供商配置
providers:
  - name: Provider-1
    api_base: "${API_BASE_URL}"
    api_key: "${API_KEY_1}"
    weight: 2  # 权重值,越大分配的请求越多
    model_mapping:
      # 精确匹配
      "claude-4.5-sonnet": "actual-provider-model"
      # 支持通配符/正则表达式模式:
      "claude-opus-4-5-.*": "claude-opus-mapped"  # 正则表达式模式
      "gemini-*": "gemini-pro"                     # 简单通配符 (* -> .*)

# Master Key 配置
master_keys:
  # 带速率限制的密钥
  - name: "Production Key"
    key: "sk-prod-key"
    rate_limit:
      requests_per_second: 100  # 每秒请求数
      burst_size: 150            # 突发请求数

  # 无速率限制的密钥(用于开发)
  - name: "Unlimited Key"
    key: "sk-dev-key"
    # 不设置 rate_limit 字段 = 无限制

# 服务器配置
server:
  host: 0.0.0.0
  port: 18000

# SSL 验证(调用提供商时)
verify_ssl: false
```

### 环境变量说明

| 变量 | 说明 | 必需 | 默认值 |
|------|------|------|--------|
| `DB_URL` | PostgreSQL 连接字符串 | 是 | - |
| `ADMIN_KEY` | Admin API 认证密钥 | 是 | - |
| `PORT` | 服务端口 | 否 | 18000 |
| `PROVIDER_SUFFIX` | 模型名称前缀过滤 | 否 | - |
| `VERIFY_SSL` | 验证提供商 SSL 证书 | 否 | true |

### 模型名称前缀功能

当设置 `PROVIDER_SUFFIX=Proxy` 时:
- `Proxy/gpt-4` → 自动转换为 `gpt-4`
- `gpt-4` → 保持不变
- `Other/gpt-4` → 保持不变(不同前缀)

此功能用于在多个代理服务之间切换时统一模型名称格式。

## 🗄️ 动态配置模式

系统支持两种配置模式:

### YAML 模式 (简单部署)

- **不设置** `DB_URL` 环境变量
- 使用 `config.yaml` 文件进行配置
- 适用于开发环境和简单部署
- 配置变更需要重启服务

### 数据库模式 (生产推荐)

- **设置** `DB_URL` 和 `ADMIN_KEY` 环境变量
- 配置存储在 PostgreSQL 数据库
- 支持运行时热重载,无需重启
- 适用于生产环境
- 通过 Admin API 管理配置

### 数据库迁移

```bash
# 安装 golang-migrate
brew install golang-migrate

# 设置数据库 URL
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# 运行迁移
./scripts/db_migrate.sh up

# 查看迁移版本
./scripts/db_migrate.sh version
```

### 迁移现有 YAML 配置到数据库

```bash
# 迁移配置文件到数据库
./scripts/migrate_config.sh config.yaml
```

### Admin API 使用示例

```bash
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

# 热重载配置(无需重启)
curl -X POST http://localhost:18000/admin/v1/config/reload \
  -H "Authorization: Bearer $ADMIN_KEY"
```

完整 Admin API 文档: [rust-server/README.md](rust-server/README.md) 或 [python-server/README.md](python-server/README.md)

## 📊 监控系统

### Prometheus 指标

系统自动暴露 `/metrics` 端点,通过 [python-server/prometheus/prometheus.yml](python-server/prometheus/prometheus.yml) 收集,包含以下指标:

**请求指标:**
- `llm_proxy_requests_total` - 总请求数(按 method、endpoint、model、provider、status_code 标签)
- `llm_proxy_request_duration_seconds` - 请求延迟直方图(P50/P95/P99)
- `llm_proxy_active_requests` - 当前活跃请求数

**Token 使用指标:**
- `llm_proxy_tokens_total` - Token 使用总量(按 model、provider、token_type 标签)
  - `token_type`: prompt_tokens, completion_tokens, total_tokens

**Provider 健康指标:**
- `llm_proxy_provider_health` - Provider 健康状态(1=健康, 0=不健康)
- `llm_proxy_provider_latency_seconds` - Provider 响应延迟直方图

### Grafana 仪表盘

预配置的仪表盘位于 [python-server/grafana/dashboards/](python-server/grafana/dashboards/),通过 [python-server/grafana/provisioning/](python-server/grafana/provisioning/) 自动加载,包含:

- 📈 **请求速率趋势** - 每秒请求数(RPS)时序图
- ⏱️ **延迟分析** - P50/P95/P99 延迟百分位数
- 🎫 **Token 使用统计** - 按模型和提供商的 token 消耗
- 📊 **状态码分布** - HTTP 状态码饼图
- ⚖️ **Provider 负载** - 各提供商请求分布
- 🏥 **健康状态** - Provider 健康检查和可用性
- 🔥 **实时活跃请求** - 当前并发请求数

### 启动监控栈

```bash
cd python-server
docker-compose up -d

# 访问地址:
# - Grafana: http://localhost:3000 (admin/admin)
# - Prometheus: http://localhost:9090
```

## 🚄 性能对比

Rust 实现相比 Python 实现的性能优势:

| 指标 | Python (FastAPI) | Rust (Axum) | 改善 |
|------|------------------|-------------|------|
| **内存占用** | ~50-100 MB | ~10-20 MB | **5x 更少** |
| **启动时间** | ~1-2 秒 | ~100 毫秒 | **10x 更快** |
| **吞吐量** | 基准 | 2-3x 基准 | **2-3x 更高** |
| **P99 延迟** | 基准 | ~50% 基准 | **50% 更低** |
| **并发能力** | 良好 | 优秀 | **原生异步** |

**选择建议:**
- **Python**: 快速开发、原型验证、团队熟悉 Python
- **Rust**: 生产环境、高性能需求、资源受限环境

## 📁 项目结构

```
llm-proxy/
├── python-server/          # Python FastAPI 实现
│   ├── app/               # 应用程序代码
│   │   ├── api/          # API 路由层
│   │   ├── core/         # 核心功能(配置、数据库、安全)
│   │   ├── models/       # Pydantic 数据模型
│   │   ├── services/     # 业务逻辑层
│   │   └── utils/        # 工具函数
│   ├── tests/            # 测试套件
│   ├── grafana/          # Grafana 配置和仪表盘
│   ├── prometheus/       # Prometheus 配置
│   ├── Makefile          # 开发命令
│   ├── pyproject.toml    # Python 依赖
│   └── README.md         # Python 服务文档
│
├── rust-server/           # Rust Axum 实现
│   ├── src/              # 源代码
│   │   ├── api/         # API 层
│   │   ├── core/        # 核心功能
│   │   └── services/    # 业务逻辑
│   ├── Cargo.toml       # Rust 依赖
│   └── README.md        # Rust 服务文档
│
├── migrations/           # 数据库迁移脚本
├── scripts/             # 运维脚本
│   ├── db_migrate.sh   # 数据库迁移
│   └── migrate_config.sh # 配置迁移
├── k8s/                 # Kubernetes 部署清单
│   └── dev/            # 开发环境示例
├── web/                 # 管理界面(可选)
└── README.md           # 本文件
```

## 🛠️ 开发指南

### Python 开发

```bash
cd python-server

# 安装开发依赖
uv sync

# 运行测试
make test

# 生成覆盖率报告
make coverage

# 代码格式化
make format

# 代码检查
make lint
```

### Rust 开发

```bash
cd rust-server

# 运行测试
cargo test

# 代码检查
cargo clippy

# 代码格式化
cargo fmt

# 构建 release 版本
cargo build --release
```

### Kubernetes 部署

开发环境示例部署:

```bash
cd k8s/dev

# 应用配置
./deploy.sh

# 或手动应用
kubectl apply -f k8s.yaml
```

部署清单: [k8s/dev/k8s.yaml](k8s/dev/k8s.yaml)

## 📚 相关文档

### Python 实现
- [python-server/README.md](python-server/README.md) - 详细使用文档
- [python-server/REFACTORING.md](python-server/REFACTORING.md) - 架构设计说明

### Rust 实现
- [rust-server/README.md](rust-server/README.md) - 详细使用文档
- [rust-server/CONFIGURATION.md](rust-server/CONFIGURATION.md) - 配置指南

## 📄 许可证

MIT License - 详见项目文件
