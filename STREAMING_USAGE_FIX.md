# V1 API Streaming Usage 修复方案（简化版）

## TL;DR

只修改 **v1 API**，不涉及 v2 transformer API。通过在 streaming 开始前预计算 input tokens，解决 Claude Code `/context` 显示 0/200000 的问题。

---

## 问题现状

### 测试命令

```bash
# Test /v1/chat/completions
curl -X POST 'http://127.0.0.1:17999/v1/chat/completions' \
  -H 'Content-Type: application/json' \
  -H 'Authorization: Bearer sk-your-api-key-here' \
  -d '{
    "model": "claude-sonnet-4-5",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'

# Test /v1/messages
curl -X POST 'http://127.0.0.1:17999/v1/messages' \
  -H 'Content-Type: application/json' \
  -H 'x-api-key: sk-your-api-key-here' \
  -d '{
    "model": "claude-sonnet-4-5",
    "max_tokens": 1024,
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'
```

### 当前问题

**Streaming response 中 `usage` 为 `null` 或 `0`**:
```json
// /v1/chat/completions streaming
data: {"choices":[...],"usage":null}  // ❌

// /v1/messages streaming
event: message_delta
data: {"usage":{"output_tokens":0}}   // ❌
```

---

## 修复方案

### 核心思路

1. **在接收完整请求时**，使用现有的 `calculate_message_tokens` 函数计算 input tokens
2. **将 input_tokens 传递给 streaming handler**
3. **在 streaming 处理中**：
   - ✅ **如果 Provider 返回 usage**：直接使用真实值（bypass，不使用 fallback）
   - ❌ **如果 Provider 不返回 usage**：使用预计算的 input_tokens + 累计的 output_tokens

**关键优化**: 利用现有的 `usage_found` 标志，只在必要时才使用 fallback，避免不必要的计算。

### 决策流程

```
接收请求
   ↓
预计算 input_tokens (tiktoken)
   ↓
开始 streaming
   ↓
每个 chunk:
   ├─ 检查是否有 usage?
   │     ├─ YES → 使用 Provider usage (bypass fallback) ✅
   │     │        标记 usage_found = true
   │     │
   │     └─ NO → 使用 fallback
   │              ├─ input_tokens: 使用预计算值
   │              └─ output_tokens: 累计 tiktoken 计算
   ↓
返回最终 usage
```

**关键点**:
- ✅ **Provider 有 usage**: 完全 bypass，不进行任何 fallback 计算
- ❌ **Provider 无 usage**: 使用 fallback（input: 预计算，output: 实时累计）

### 优势

- ✅ 利用现有函数 `calculate_message_tokens`（已测试）
- ✅ 最小化代码改动
- ✅ 不影响 v2 API
- ✅ 性能开销 <1ms

---

## 实现步骤

### Step 1: 修改 `/v1/chat/completions` Handler

**文件**: `rust-server/src/api/handlers.rs`

**找到 `chat_completions_handler` 函数**（大约在文件中部），在调用 streaming 处理前添加：

```rust
// 找到类似这样的代码：
if request.stream {
    // ⭐ 在这里添加 input tokens 计算
    let input_tokens = Some(calculate_message_tokens(
        &openai_request.get("messages")
            .and_then(|m| m.as_array())
            .unwrap_or(&vec![]),
        &model_label
    ));

    return create_sse_stream(
        response,
        original_model,
        provider.name,
        input_tokens,  // ⭐ 确保传递这个参数（现有代码可能是 None）
        ttft_timeout_secs,
        generation_data,
        Some(request_id.clone()),
        Some("/v1/chat/completions"),
        Some(serde_json::to_value(&request)?),
    ).await;
}
```

**完整改动**（找到对应位置替换）:

```rust
// 查找：create_sse_stream(...) 的调用
// 替换为：

// Calculate input tokens for streaming (fallback if provider doesn't return usage)
let input_tokens = if request.stream {
    let messages = openai_request
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|arr| arr.as_slice())
        .unwrap_or(&[]);
    Some(calculate_message_tokens(messages, &model_label))
} else {
    None
};

// ... 保持现有的 response 发送逻辑 ...

if request.stream {
    return create_sse_stream(
        response,
        original_model,
        provider.name,
        input_tokens,  // ⭐ 使用计算的值（而不是 None）
        ttft_timeout_secs,
        generation_data,
        Some(request_id.clone()),
        Some("/v1/chat/completions"),
        Some(serde_json::to_value(&request)?),
    ).await;
}
```

### Step 2: 修改 `/v1/messages` Handler

**文件**: `rust-server/src/api/claude.rs`

**在 `create_message` 函数中**，找到 streaming 分支，添加：

```rust
// 找到类似这样的代码：
if claude_request.stream {
    // ⭐ 在这里添加 input tokens 计算
    let input_tokens = Some(calculate_claude_input_tokens(&claude_request));

    return handle_streaming_response(
        response,
        claude_request.model.clone(),
        model_label,
        provider.name.clone(),
        api_key_name,
        generation_data,
        trace_id,
        request_id.clone(),
        serde_json::to_value(&claude_request).unwrap_or_default(),
        input_tokens,  // ⭐ 新增参数
    ).await;
}
```

**需要添加辅助函数**（放在 claude.rs 文件末尾）:

```rust
/// Calculate input tokens for Claude request using tiktoken.
fn calculate_claude_input_tokens(request: &ClaudeMessagesRequest) -> usize {
    use crate::api::streaming::count_tokens;

    let model = &request.model;
    let mut total = 0;

    // System prompt
    if let Some(ref system) = request.system {
        match system {
            ClaudeSystemPrompt::Text(text) => {
                total += count_tokens(text, model);
            }
            ClaudeSystemPrompt::Blocks(blocks) => {
                for block in blocks {
                    total += count_tokens(&block.text, model);
                }
            }
        }
    }

    // Messages
    for msg in &request.messages {
        total += count_tokens(&msg.role, model);

        match &msg.content {
            ClaudeMessageContent::Text(text) => {
                total += count_tokens(text, model);
            }
            ClaudeMessageContent::Blocks(blocks) => {
                for block in blocks {
                    if let ClaudeContentBlock::Text(text_block) = block {
                        total += count_tokens(&text_block.text, model);
                    }
                }
            }
        }

        total += 4;  // Per-message overhead
    }

    total += 2;  // Conversation overhead

    // Tools (if any)
    if let Some(tools) = &request.tools {
        let tools_str = serde_json::to_string(tools).unwrap_or_default();
        total += count_tokens(&tools_str, model);
    }

    total
}
```

**修改 `handle_streaming_response` 函数签名**:

```rust
// 查找：async fn handle_streaming_response(
// 在参数列表末尾添加：
async fn handle_streaming_response(
    response: reqwest::Response,
    original_model: String,
    model_label: String,
    provider_name: String,
    api_key_name: String,
    generation_data: GenerationData,
    trace_id: Option<String>,
    request_id: String,
    request_payload: serde_json::Value,
    input_tokens: Option<usize>,  // ⭐ 新增参数
) -> Result<Response> {
    // ... 函数体保持不变，但需要传递给 convert_openai_streaming_to_claude
    let claude_stream = convert_openai_streaming_to_claude(
        Box::pin(stream),
        original_model,
        input_tokens,  // ⭐ 传递参数
    );
    // ... rest of the code
}
```

### Step 3: 修改 Claude Streaming 转换器

**文件**: `rust-server/src/services/claude_converter.rs`

**修改函数签名**:

```rust
// 查找：pub fn convert_openai_streaming_to_claude(
// 修改为：
pub fn convert_openai_streaming_to_claude(
    openai_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    original_model: String,
    fallback_input_tokens: Option<usize>,  // ⭐ 新增参数
) -> Pin<Box<dyn Stream<Item = String> + Send>> {
    let message_id = format!("msg_{}", &Uuid::new_v4().simple().to_string()[..24]);

    let state = StreamingState {
        message_id: message_id.clone(),
        original_model: original_model.clone(),
        // ... 其他字段 ...
        usage_data: ClaudeUsage {
            input_tokens: fallback_input_tokens.unwrap_or(0) as i32,  // ⭐ 使用 fallback
            output_tokens: 0,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        },
        // ... 其他字段 ...
    };

    // ... rest of the code
}
```

**在 unfold 循环中**（查找 `if let Some(usage) = chunk_json.get("usage")`）:

```rust
// 查找类似这样的代码，添加 usage_found 标志：
let mut usage_found = false;

if let Some(usage) = chunk_json.get("usage") {
    let extracted = extract_usage_data(usage);
    // ⭐ Provider 返回了 usage，直接使用（bypass fallback）
    state.usage_data = extracted;
    usage_found = true;  // 标记已找到 usage
}

// ⭐ 只在 Provider 没有返回 usage 时才使用 fallback
if !usage_found {
    // 保留 fallback input_tokens（已在初始化时设置）
    // 累计 output tokens
    let content = extract_stream_text(&chunk_json);
    for text in &content {
        state.usage_data.output_tokens += count_tokens(text, &state.original_model) as i32;
    }
}
```

**关键**: 使用 `usage_found` 标志确保：
- ✅ Provider 有 usage → 完全使用 Provider 值（不累计 fallback output tokens）
- ❌ Provider 无 usage → 使用 fallback input_tokens + 累计 output tokens

---

## 测试验证

### 验证步骤

1. **启动 llm-proxy**:
   ```bash
   cd rust-server
   cargo build --release
   ./target/release/llm-proxy
   ```

2. **测试 /v1/chat/completions**:
   ```bash
   curl -X POST 'http://127.0.0.1:17999/v1/chat/completions' \
     -H 'Content-Type: application/json' \
     -H 'Authorization: Bearer sk-your-api-key-here' \
     -d '{
       "model": "claude-sonnet-4-5",
       "messages": [{"role": "user", "content": "Count from 1 to 5"}],
       "stream": true
     }' | grep -E "usage|data:"
   ```

   **预期输出**:
   ```
   data: {"choices":[...],"usage":{"prompt_tokens":12,"completion_tokens":25,"total_tokens":37}}
   ```
   ✅ `prompt_tokens` 不为 0

3. **测试 /v1/messages**:
   ```bash
   curl -X POST 'http://127.0.0.1:17999/v1/messages' \
     -H 'Content-Type: application/json' \
     -H 'x-api-key: sk-your-api-key-here' \
     -d '{
       "model": "claude-sonnet-4-5",
       "max_tokens": 1024,
       "messages": [{"role": "user", "content": "Count from 1 to 5"}],
       "stream": true
     }' | grep -E "event:|usage"
   ```

   **预期输出**:
   ```
   event: message_start
   data: {"message":{"usage":{"input_tokens":12,"output_tokens":0}}}

   event: message_delta
   data: {"usage":{"output_tokens":25},"delta":{...}}
   ```
   ✅ `input_tokens` 不为 0
   ✅ `output_tokens` 不为 0

4. **验证 Claude Code**:
   ```bash
   claude
   > /context
   ```

   **预期**:
   ```
   ╭─────────────────────────────────────────────────╮
   │              Context Usage                      │
   │   claude-sonnet-4-5 • 37k/200k tokens (18%)    │
   ╰─────────────────────────────────────────────────╯
   ```
   ✅ 不再显示 0/200000

---

## 文件改动清单

### 需要修改的文件

1. **rust-server/src/api/handlers.rs**
   - 在 `chat_completions_handler` 中添加 `input_tokens` 计算

2. **rust-server/src/api/claude.rs**
   - 在 `create_message` 中添加 `input_tokens` 计算
   - 添加 `calculate_claude_input_tokens` 辅助函数
   - 修改 `handle_streaming_response` 签名

3. **rust-server/src/services/claude_converter.rs**
   - 修改 `convert_openai_streaming_to_claude` 签名
   - 在 usage 提取逻辑中添加 fallback

### 不需要修改的文件

- ❌ v2 transformer API (`rust-server/src/transformer/*`)
- ❌ Response API (`rust-server/src/api/response_api.rs` 等)
- ❌ `streaming.rs` 的核心逻辑（已经有 fallback）

---

## 实现检查清单

- [ ] 修改 `handlers.rs` - chat_completions_handler
- [ ] 修改 `claude.rs` - create_message
- [ ] 添加 `calculate_claude_input_tokens` 函数
- [ ] 修改 `handle_streaming_response` 签名
- [ ] 修改 `claude_converter.rs` - convert_openai_streaming_to_claude
- [ ] 编译验证：`cargo build`
- [ ] 测试 /v1/chat/completions streaming
- [ ] 测试 /v1/messages streaming
- [ ] 验证 Claude Code `/context` 命令

---

## 风险评估

| 风险 | 等级 | 缓解措施 |
|------|------|----------|
| Tiktoken 偏差 (5-15%) | 🟡 低 | 文档说明是估算值，优先级：可用性 > 准确性 |
| 函数签名变更 | 🟢 极低 | 只影响内部调用，不涉及公开 API |
| 性能影响 | 🟢 极低 | <1ms 开销，可忽略 |
| v2 API 兼容性 | 🟢 无 | 不修改 v2 transformer |

---

## 参考资料

- 现有函数: `calculate_message_tokens` ([streaming.rs#L116](../rust-server/src/api/streaming.rs#L116))
- 现有函数: `count_tokens` ([streaming.rs#L161](../rust-server/src/api/streaming.rs#L161))
- Claude usage 转换: `extract_usage_data` ([claude_converter.rs](../rust-server/src/services/claude_converter.rs))
