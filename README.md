# LLM API Round-Robin Proxy

一个简单的 LLM API 代理服务，支持多个提供商的轮询（round-robin）请求分发。

## 功能特性

- ✅ 支持多个 API 提供商配置
- ✅ Round-robin 轮询算法
- ✅ 支持流式和非流式响应
- ✅ 兼容 OpenAI API 格式
- ✅ YAML 配置文件
- ✅ 线程安全

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

server:
  host: "0.0.0.0"
  port: 8000
```

### 3. 启动代理服务

使用 uv 运行：

```bash
uv run proxy.py
```

或直接运行：

```bash
python proxy.py
```

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
- `/health` - 健康检查

## 注意事项

- 确保所有提供商使用相同的 API 格式（默认 OpenAI 格式）
- API key 需要有效且有足够的配额
- 代理不会自动处理失败重试，如果某个提供商失败，会直接返回错误

## License

MIT
