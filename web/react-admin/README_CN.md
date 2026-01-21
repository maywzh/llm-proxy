# LLM Proxy Admin - React

[![React 18](https://img.shields.io/badge/React-18-blue.svg)](https://react.dev/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5-blue.svg)](https://www.typescriptlang.org/)
[![Vite](https://img.shields.io/badge/Vite-5-purple.svg)](https://vitejs.dev/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

中文文档 | [English](README.md)

基于 React 构建的管理界面，用于管理 LLM Proxy 配置。

> 完整项目概览请查看[主 README](../../README_CN.md)

## 快速开始

### 前置要求
- Node.js 18+
- bun (推荐)

### 安装与运行

```bash
cd web/react-admin
bun install
bun run dev
```

在浏览器中打开 [http://localhost:5173](http://localhost:5173)

## 配置

### 环境变量

复制 `.env.example` 到 `.env.local` 并配置:

```bash
# 可选: 默认 API 基础 URL
VITE_PUBLIC_API_BASE_URL=http://127.0.0.1:17999

# 可选: Grafana 公共仪表盘 URL (用于仪表盘页面)
# 在 Grafana 中创建公共仪表盘并粘贴 URL 到这里
# 参见: https://grafana.com/docs/grafana/latest/dashboards/dashboard-public/
PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=
```

### 登录凭证

- **API 基础 URL**: 您的 LLM Proxy 服务器 URL (例如 `http://127.0.0.1:17999`)
- **Admin API Key**: 服务器配置中的 `ADMIN_KEY`

## 功能特性

- **提供商管理**: 创建、编辑、删除和切换 LLM 提供商
- **凭证管理**: 管理 API 密钥，支持速率限制和模型限制
- **聊天界面**: 使用流式响应与 LLM 模型进行交互式对话
  - 从所有可用提供商中选择模型
  - 实时流式响应
  - Markdown 消息渲染（清理后的 HTML）
  - 可配置参数（max_tokens）
  - 白名单视觉模型的图片上传功能（例如 grok-4/grok-3）
  - 停止生成和清空对话控制
- **认证**: 使用 admin API 密钥的安全登录
- **配置**: 实时配置版本显示和重新加载
- **仪表盘**: 嵌入式 Grafana 仪表盘用于监控（需要公共仪表盘 URL）

## 技术栈

- React 18 + TypeScript
- Vite (构建工具)
- React Router (路由)
- Tailwind CSS (样式)
- Lucide React (图标)

## 可用脚本

```bash
bun run dev      # 启动开发服务器
bun run build    # 生产环境构建
bun run preview  # 预览生产构建
bun run lint     # 运行 ESLint
```

## 聊天功能

聊天页面允许您通过代理与 LLM 模型进行交互:

### 使用方法

1. **选择模型**: 从已启用提供商的可用模型中选择
2. **配置参数**（可选）:
   - **Max Tokens**: 最大响应长度（100 - 8000，默认: 2000）
3. **设置凭证密钥**: 打开设置并设置凭证密钥
4. **开始聊天**: 输入您的消息并按 Enter
5. **流式响应**: 响应实时流式传输
6. **控制**:
   - **停止**: 随时中断生成
   - **清空**: 重置对话历史

### 键盘快捷键

- `Enter`: 发送消息
- `Shift + Enter`: 插入新行

### 注意事项

- 聊天使用启用了流式传输的 `/v1/chat/completions` API
- 需要在系统中配置有效的凭证密钥
- 聊天页面需要输入凭证密钥（与 admin 密钥分开）
- 模型从 `/v1/models` 加载，使用凭证密钥（遵守 allowed_models）
- 图片上传仅对白名单模型启用，通过 `VITE_CHAT_VISION_MODEL_ALLOWLIST` 配置

## 故障排除

### 连接问题

1. 验证 LLM Proxy 服务器正在运行
2. 检查 API 基础 URL 是否正确
3. 确保服务器上配置了 `ADMIN_KEY`
4. 检查浏览器控制台错误

### 聊天问题

1. 确保至少配置了一个提供商和凭证
2. 检查提供商是否已启用
3. 验证凭证密钥有效
4. 检查浏览器开发工具中的网络请求

### 构建问题

```bash
# 清除依赖并重新安装
rm -rf node_modules bun.lockb
bun install
```

## 项目结构

```
src/
├── api/client.ts         # API 客户端
├── components/Layout.tsx # 主布局
├── contexts/             # React contexts
├── hooks/                # 自定义 hooks
├── pages/                # 页面组件
│   ├── Providers.tsx     # 提供商管理
│   ├── Credentials.tsx   # 凭证管理
│   ├── Chat.tsx          # 聊天界面
│   ├── Dashboard.tsx     # Grafana 仪表盘
│   └── Login.tsx         # 登录页面
└── types/                # TypeScript 类型
```

## Grafana 集成

仪表盘页面通过 iframe 嵌入 Grafana 公共仪表盘。启用方法:

1. **在 Grafana 中启用公共仪表盘**:
   - 在 Grafana 配置中设置 `GF_FEATURE_TOGGLES_ENABLE=publicDashboards`
   - 设置 `GF_SECURITY_ALLOW_EMBEDDING=true` 以支持 iframe

2. **创建公共仪表盘**:
   - 在 Grafana 中打开您的仪表盘
   - 点击分享 → 公共仪表盘
   - 启用并复制 URL

3. **配置 URL**:
   - 在 `.env.local` 中设置 `PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL`

## 📄 许可证

MIT License

---

Last Updated: 2025-01-15 10:49 (Asia/Shanghai)
