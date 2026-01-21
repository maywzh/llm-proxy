# LLM Proxy - Rust 服务

[![Rust 1.85+](https://img.shields.io/badge/rust-1.85+-orange.svg)](https://www.rust-lang.org/)
[![Axum](https://img.shields.io/badge/Axum-0.7-blue.svg)](https://github.com/tokio-rs/axum)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[English](README.md) | 中文文档

基于 Rust + Axum + Tokio 的高性能 LLM API 代理实现,提供卓越的性能和资源效率。

> 完整项目概述请参阅 [主 README](../README_CN.md)

## 📋 目录

- [核心特性](#-核心特性)
- [技术栈](#-技术栈)
- [性能优势](#-性能优势)
- [快速开始](#-快速开始)
- [开发指南](#️-开发指南)

## ✨ 核心特性

**完整的 Python 版本功能对等:**

- ✅ **YAML 配置加载** - 支持环境变量展开
- ✅ **多提供商支持** - 加权轮询负载均衡
- ✅ **OpenAI 兼容 API** - 完整支持 `/v1/chat/completions`、`/v1/completions`、`/v1/models`
- ✅ **流式响应** - Server-Sent Events (SSE) 支持
- ✅ **健康检查** - 基础和详细的健康检查端点
- ✅ **Prometheus 监控** - 完整的 `/metrics` 指标导出
- ✅ **API 密钥认证** - Master Key 认证机制
- ✅ **CORS 支持** - 跨域资源共享配置
- ✅ **请求日志** - 使用 tracing 的结构化日志
- ✅ **错误处理和重试** - 健壮的错误处理机制
- ✅ **Docker 支持** - 多阶段构建优化
- ✅ **动态配置** - PostgreSQL 数据库配置存储
- ✅ **热重载** - 运行时配置更新(无需重启)
- ✅ **Admin API** - RESTful 配置管理接口
- ✅ **速率限制** - 可选的 Master Key 速率限制

## 🔧 技术栈

### 核心框架
- **Web 框架**: Axum 0.7 - 基于 Tokio 的模块化 Web 框架
- **异步运行时**: Tokio 1.x - Rust 最流行的异步运行时
- **路由**: Tower + Tower-HTTP - 中间件和服务抽象

### HTTP 与网络
- **HTTP 客户端**: reqwest 0.11 - 功能丰富的异步 HTTP 客户端
- **流式处理**: async-stream - 异步流处理
- **字节处理**: bytes 1.5 - 高效的字节缓冲区

### 数据处理
- **序列化**: serde + serde_json - 零开销序列化/反序列化
- **配置管理**: config 0.14 + dotenvy 0.15 - 配置加载和环境变量
- **数据库**: SQLx 0.8 - 编译时检查的 SQL 客户端
- **热重载**: arc-swap 1.7 - 无锁原子配置更新

### 监控与日志
- **监控**: prometheus 0.13 - Prometheus 官方 Rust 客户端
- **日志**: tracing + tracing-subscriber - 结构化日志和追踪
- **Token 计数**: tiktoken-rs 0.5 - Rust 版 token 计数库

### 安全与限流
- **速率限制**: governor 0.7 - 高性能速率限制器
- **并发控制**: DashMap 6.1 - 并发哈希映射
- **密钥哈希**: sha2 + hex - 安全的密钥存储

### 错误处理
- **错误类型**: thiserror 1.0 - 派生宏简化错误定义
- **错误传播**: anyhow 1.0 - 灵活的错误处理

### 开发工具
- **测试框架**: tokio-test + mockito + wiremock
- **属性测试**: proptest + quickcheck
- **断言增强**: assert_matches + pretty_assertions

## 🚀 性能优势

Rust 实现相比 Python 实现的性能提升:

| 指标 | Python (FastAPI) | Rust (Axum) | 提升幅度 |
|------|------------------|-------------|---------|
| **内存占用** | ~50-100 MB | ~10-20 MB | **↓ 5x** |
| **启动时间** | ~1-2 秒 | ~100 毫秒 | **↑ 10-20x** |
| **吞吐量 (RPS)** | 基准 | 2-3x 基准 | **↑ 2-3x** |
| **P99 延迟** | 基准 | ~50% 基准 | **↓ 50%** |
| **并发性能** | 良好 (asyncio) | 优秀 (原生 async) | **显著提升** |
| **CPU 效率** | 中等 (解释型) | 极高 (编译优化) | **5-10x** |

**关键优势:**
- 🚀 **极低延迟** - 原生编译,零运行时开销
- 💪 **高并发** - Tokio 运行时提供卓越的并发性能
- 💾 **内存效率** - 精确的内存管理,无 GC 暂停
- 🔥 **高吞吐** - 零拷贝和优化的 I/O 处理
- 📦 **小体积** - 独立二进制文件,无依赖

**适用场景:**
- ✅ 生产环境高负载场景
- ✅ 资源受限环境(容器、边缘计算)
- ✅ 对延迟敏感的应用
- ✅ 需要极致性能的场景

## 🚀 快速开始

### 前置要求

- Rust 1.85+ (使用 rustup 安装)
- PostgreSQL 数据库
- Cargo (Rust 包管理器)

### 1. 安装 Rust

```bash
# 安装 Rust (如果还没有安装)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 更新到最新版本
rustup update
```

### 2. 构建项目

```bash
# 开发构建
cargo build

# Release 构建(生产环境)
cargo build --release
```

### 3. 配置环境变量

创建 `.env` 文件或设置环境变量:

```bash
# 必需: 数据库连接
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# 必需: Admin API 认证密钥
export ADMIN_KEY='your-admin-key'

# 可选: 服务端口(默认 18000)
export PORT=18000

# 可选: 配置文件路径
export CONFIG_PATH=config.yaml

# 可选: 模型名称前缀
export PROVIDER_SUFFIX='Proxy'
```

### 4. 运行数据库迁移

```bash
# 安装 golang-migrate
brew install golang-migrate

# 运行迁移
../scripts/db_migrate.sh up
```

### 5. 启动服务

**方式一: 本地运行**
```bash
# 开发模式(带调试信息)
RUST_LOG=debug cargo run

# Release 模式
cargo run --release

# 使用已构建的二进制
./target/release/llm-proxy-rust
```

**方式二: Docker 运行**
```bash
# 构建 Docker 镜像
docker build -t llm-proxy-rust:latest .

# 运行容器
docker run -p 18000:18000 \
  -e DB_URL='postgresql://user:pass@host.docker.internal:5432/llm_proxy' \
  -e ADMIN_KEY='your-admin-key' \
  -e PORT=18000 \
  llm-proxy-rust:latest
```

**服务访问地址:**
- LLM Proxy: <http://localhost:18000>
- 健康检查: <http://localhost:18000/health>
- 指标监控: <http://localhost:18000/metrics>
- Swagger UI: <http://localhost:18000/swagger-ui/>

## ⚙️ 配置说明

详细配置文档请参阅 [主 README](../README_CN.md#-配置说明) 或 [CONFIGURATION.md](CONFIGURATION.md)。

## 🛠️ 开发指南

### 运行测试

```bash
# 运行所有测试
cargo test

# 运行测试并显示输出
cargo test -- --nocapture

# 运行特定测试
cargo test test_name
```

### 代码质量

```bash
# 代码检查
cargo clippy

# 代码格式化
cargo fmt

# 检查代码(不构建)
cargo check
```

### 构建

```bash
# 调试构建
cargo build

# Release 构建(优化)
cargo build --release

# 为特定目标构建
cargo build --release --target x86_64-unknown-linux-gnu
```

### Docker 开发

```bash
# 构建 Docker 镜像
docker build -t llm-proxy-rust:dev .

# 使用 Docker 运行
docker run -p 18000:18000 llm-proxy-rust:dev
```

更多详情请参阅:
- [主 README](../README_CN.md) - 完整项目文档
- [CONFIGURATION.md](CONFIGURATION.md) - 详细配置指南

## 📁 项目结构

```text
rust-server/
├── Cargo.toml              # 依赖和项目元数据
├── Dockerfile              # 多阶段 Docker 构建
├── .dockerignore           # Docker 忽略模式
├── README.md               # 英文文档
├── README_CN.md            # 本文件
├── CONFIGURATION.md        # 配置详细文档
└── src/
    ├── main.rs             # 应用程序入口点
    ├── lib.rs              # 库入口
    ├── api/                # API 层
    │   ├── mod.rs          # API 模块定义
    │   ├── handlers.rs     # 请求处理器
    │   ├── health.rs       # 健康检查端点
    │   ├── models.rs       # API 数据模型
    │   ├── streaming.rs    # SSE 流式响应
    │   └── admin.rs        # Admin API 端点
    ├── core/               # 核心功能
    │   ├── mod.rs          # 核心模块定义
    │   ├── config.rs       # 配置加载和解析
    │   ├── database.rs     # 数据库操作
    │   ├── error.rs        # 错误类型和处理
    │   ├── metrics.rs      # Prometheus 指标
    │   ├── middleware.rs   # 请求中间件
    │   ├── logging.rs      # 日志配置
    │   └── rate_limiter.rs # 速率限制器
    └── services/           # 业务逻辑
        ├── mod.rs          # 服务模块定义
        ├── provider_service.rs      # 提供商服务
        └── health_check_service.rs  # 健康检查服务
```

## 📄 许可证

MIT 许可证

---

## 构建

### 本地构建

```bash
cd rust-server
cargo build --release
```

### Docker 构建

```bash
cd rust-server
docker build -t llm-proxy-rust:latest .
```

## 运行

### 本地运行

```bash
# 设置环境变量或创建 .env 文件
export CONFIG_PATH=config.yaml

# 运行二进制文件
cargo run --release
```

### Docker 运行

```bash
docker run -p 18000:18000 \
  -v $(pwd)/config.yaml:/app/config.yaml \
  -e CONFIG_PATH=/app/config.yaml \
  llm-proxy-rust:latest
```

## 配置

服务器支持通过环境变量和 YAML 文件进行灵活配置,并可选择使用数据库支持的动态配置模式。

## 动态配置模式

LLM Proxy 支持两种配置模式:

### YAML 模式(默认)

- 不设置 `DB_URL` 环境变量
- 使用 `config.yaml` 文件进行配置
- 适用于开发和简单部署
- 配置更改需要重启服务器

### 数据库模式

- 设置 `DB_URL` 和 `ADMIN_KEY` 环境变量
- 配置存储在 PostgreSQL 数据库中
- 支持运行时热重载,无需重启
- 适用于生产环境
- 通过 Admin API 管理配置

### 动态配置环境变量

| 变量 | 描述 | 是否必需 |
|----------|-------------|----------|
| `DB_URL` | PostgreSQL 连接字符串 | 数据库模式必需 |
| `ADMIN_KEY` | Admin API 认证密钥 | 数据库模式必需 |
| `PORT` | 服务端口 | 否(默认: 18000) |
| `PROVIDER_SUFFIX` | 可选的模型名称前缀。设置后,形如 `{PROVIDER_SUFFIX}/{model}` 的模型名称将被视为 `{model}` | 否 |

### 数据库迁移

```bash
# 安装 golang-migrate
brew install golang-migrate

# 设置数据库 URL
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# 运行迁移
./scripts/db_migrate.sh up

# 检查迁移版本
./scripts/db_migrate.sh version

# 回滚一个迁移
./scripts/db_migrate.sh down
```

### 将现有 YAML 配置迁移到数据库

```bash
# 设置环境变量
export DB_URL='postgresql://user:pass@localhost:5432/llm_proxy'

# 运行迁移脚本
./scripts/migrate_config.sh config.yaml
```

### Admin API 示例

```bash
# 设置您的 admin key
export ADMIN_KEY='your-admin-key'

# 创建提供商
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

# 列出所有提供商
curl http://localhost:18000/admin/v1/providers \
  -H "Authorization: Bearer $ADMIN_KEY"

# 获取特定提供商
curl http://localhost:18000/admin/v1/providers/openai-main \
  -H "Authorization: Bearer $ADMIN_KEY"

# 更新提供商
curl -X PUT http://localhost:18000/admin/v1/providers/openai-main \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-new-key",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "is_enabled": true
  }'

# 删除提供商
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

# 重新加载配置(热重载)
curl -X POST http://localhost:18000/admin/v1/config/reload \
  -H "Authorization: Bearer $ADMIN_KEY"

# 获取当前配置版本
curl http://localhost:18000/admin/v1/config/version \
  -H "Authorization: Bearer $ADMIN_KEY"
```

---

## YAML 配置

### 快速开始

1. **复制示例文件:**

   ```bash
   cp .env.example .env
   cp config.example.yaml config.yaml
   ```

2. **编辑 `.env` 文件:**

   ```bash
   # 编辑 API 密钥和敏感数据
   nano .env
   ```

3. **运行服务器:**

   ```bash
   cargo run
   # 或使用特定配置文件
   CONFIG_PATH=config.prod.yaml cargo run
   ```

### 配置方法

服务器支持三种配置方法,优先级从高到低:

1. **直接环境变量** - 在 shell 或系统中设置
2. **`.env` 文件** - 如果存在则自动加载
3. **YAML 配置** - `config.yaml` 中的结构化配置

### 关键环境变量

| 变量 | 描述 | 默认值 | 示例 |
|----------|-------------|---------|---------|
| `CONFIG_PATH` | YAML 配置路径 | `config.yaml` | `config.prod.yaml` |
| `HOST` | 服务器绑定地址 | `0.0.0.0` | `127.0.0.1` |
| `PORT` | 服务器端口 | `18000` | `8080` |
| `VERIFY_SSL` | 验证 SSL 证书 | `true` | `false` |
| `PROVIDER_SUFFIX` | 模型名称前缀 | 无 | `Proxy` |

### 配置示例

**`.env` 文件:**

```bash
API_KEY_1=your-api-key-1
API_KEY_2=your-api-key-2
API_BASE_URL=https://api.example.com
MASTER_KEY_1=sk-your-master-key
VERIFY_SSL=false
```

**`config.yaml` 文件:**

```yaml
providers:
  - name: "Provider-1"
    api_base: "${API_BASE_URL}"
    api_key: "${API_KEY_1}"
    weight: 2
    model_mapping:
      "claude-4.5-sonnet": "actual-model-name"

  - name: "Provider-2"
    api_base: "${API_BASE_URL}"
    api_key: "${API_KEY_2}"
    weight: 1
    model_mapping:
      "claude-4.5-sonnet": "actual-model-name"

# 带可选速率限制的 Master Key
master_keys:
  # 带速率限制的密钥
  - name: "Production Key"
    key: "${MASTER_KEY_1}"
    rate_limit:
      requests_per_second: 100
      burst_size: 150
  
  # 无速率限制的密钥(无限请求)
  - name: "Unlimited Key"
    key: "${MASTER_KEY_UNLIMITED}"
    # 无 rate_limit 字段 = 无速率限制

server:
  host: "${HOST:-0.0.0.0}"
  port: ${PORT:-18000}

verify_ssl: true
```

### 环境特定配置

为不同环境使用不同的配置文件:

```bash
# 开发环境
CONFIG_PATH=config.dev.yaml cargo run

# 预发布环境
CONFIG_PATH=config.staging.yaml cargo run

# 生产环境
CONFIG_PATH=config.prod.yaml cargo run
```

### 覆盖配置

无需更改文件即可覆盖特定设置:

```bash
# 覆盖端口和主机
PORT=8080 HOST=127.0.0.1 cargo run

# 禁用 SSL 验证
VERIFY_SSL=false cargo run
```

📖 **详细配置文档请参阅 [CONFIGURATION.md](CONFIGURATION.md)**

## API 端点

### 聊天补全

```bash
POST /v1/chat/completions
Authorization: Bearer <master_key>
Content-Type: application/json

{
  "model": "claude-4.5-sonnet",
  "messages": [{"role": "user", "content": "Hello"}],
  "stream": false
}
```

### 模型名称前缀功能

当设置 `PROVIDER_SUFFIX` 环境变量时,可以使用带前缀的模型名称:

```bash
# 设置提供商后缀
export PROVIDER_SUFFIX=Proxy

# 以下两个请求是等效的:
# 1. 使用带前缀的模型名称
curl -X POST http://localhost:18000/v1/chat/completions \
  -H "Authorization: Bearer <master_key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "Proxy/gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'

# 2. 使用原始模型名称
curl -X POST http://localhost:18000/v1/chat/completions \
  -H "Authorization: Bearer <master_key>" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

#### 前缀行为

- 如果未设置 `PROVIDER_SUFFIX`,模型名称按原样使用
- 如果设置了 `PROVIDER_SUFFIX`(例如 "Proxy"):
  - `Proxy/gpt-4` → `gpt-4`(前缀被移除)
  - `gpt-4` → `gpt-4`(保持不变)
  - `Other/gpt-4` → `Other/gpt-4`(保持不变,不同前缀)

此功能对于标准化模型名称格式非常有用,特别是在不同代理服务之间切换时。

### 列出模型

```bash
GET /v1/models
Authorization: Bearer <master_key>
```

### 健康检查

```bash
GET /health
```

### 详细健康检查

```bash
GET /health/detailed
```

### 指标监控

```bash
GET /metrics
```

## Master Key 速率限制

系统支持可选的按密钥速率限制。每个 Master Key 可以有独立的速率限制,或完全不限制。

### 速率限制配置

**启用速率限制:**

```yaml
master_keys:
  - name: "Limited Key"
    key: "sk-limited-key"
    rate_limit:
      requests_per_second: 100  # 每秒最多 100 个请求
      burst_size: 150           # 允许 150 个请求的突发
```

**禁用速率限制(无限制):**

```yaml
master_keys:
  - name: "Unlimited Key"
    key: "sk-unlimited-key"
    # 无 rate_limit 字段 = 无速率限制
```

### 行为说明

| 配置 | 行为 |
|--------------|----------|
| `rate_limit: {requests_per_second: 100, burst_size: 150}` | 启用速率限制: 100 请求/秒,150 突发 |
| `rate_limit: {requests_per_second: 0, burst_size: 0}` | 启用速率限制: 阻止所有请求 |
| 无 `rate_limit` 字段 | 禁用速率限制: 无限请求 |

### 使用场景

- **生产密钥**: 设置合理的速率限制以防止滥用
- **开发/测试密钥**: 省略 rate_limit 以便于开发
- **特殊用途密钥**: 根据实际需求灵活配置

## 性能对比

Rust 实现相比 Python 版本提供了显著的性能提升:

- **更低的内存使用**: ~10-20MB vs ~50-100MB (Python)
- **更快的启动**: ~100ms vs ~1-2s (Python)
- **更高的吞吐量**: 每秒请求数提高 2-3 倍
- **更低的延迟**: P99 延迟降低约 50%
- **更好的并发**: 使用 Tokio 运行时的原生 async/await

## Prometheus 指标

Prometheus 指标可在 `/metrics` 端点获取:

- `llm_proxy_requests_total` - 请求总数
- `llm_proxy_request_duration_seconds` - 请求持续时间直方图
- `llm_proxy_active_requests` - 活跃请求数
- `llm_proxy_tokens_total` - 使用的 token 总数(提示/补全/总计)
- `llm_proxy_provider_health` - 提供商健康状态
- `llm_proxy_provider_latency_seconds` - 提供商延迟直方图

## 开发

### 运行测试

```bash
cargo test
```

### 使用调试日志运行

```bash
RUST_LOG=debug cargo run
```

### 格式化代码

```bash
cargo fmt
```

### 代码检查

```bash
cargo clippy
```

## 依赖

主要依赖:

- `axum` - Web 框架
- `tokio` - 异步运行时
- `reqwest` - HTTP 客户端
- `serde` - 序列化
- `prometheus` - 指标监控
- `tracing` - 日志

完整列表请参阅 `Cargo.toml`。

## 许可证

与父项目相同。
