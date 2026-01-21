# LLM Proxy Admin - Svelte

[![Svelte 5](https://img.shields.io/badge/Svelte-5-orange.svg)](https://svelte.dev/)
[![SvelteKit](https://img.shields.io/badge/SvelteKit-2-orange.svg)](https://kit.svelte.dev/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5-blue.svg)](https://www.typescriptlang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

[中文文档](README_CN.md) | English

A modern, responsive admin interface built with Svelte 5 for managing LLM Proxy configurations, monitoring, and interactive chat.

> For complete project overview, see the [main README](../../README.md)

## Quick Start

### Prerequisites
- Node.js 18+
- pnpm (recommended)

### Installation & Running

```bash
cd web/svelte-admin
pnpm install
pnpm run dev
```

Open [http://localhost:5173](http://localhost:5173) in your browser.

## Configuration

### Environment Variables

Copy `.env.example` to `.env.local` and configure:

```bash
# Optional: Default API base URL
VITE_PUBLIC_API_BASE_URL=http://127.0.0.1:17999

# Optional: Grafana Public Dashboard URL for dashboard page
# Create a public dashboard in Grafana and paste URL here
# See: https://grafana.com/docs/grafana/latest/dashboards/dashboard-public/
PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=
```

### Login Credentials

- **API Base URL**: Your LLM Proxy server URL (e.g., `http://127.0.0.1:17999`)
- **Admin API Key**: The `ADMIN_KEY` from your server configuration

## Features

- **Provider Management**: Create, edit, delete, and toggle LLM providers
- **Credential Management**: Manage API keys with rate limiting and model restrictions
- **Chat Interface**: Interactive chat with LLM models using streaming responses
  - Model selection from all available providers
  - Real-time streaming responses
  - Markdown rendering for messages (sanitized HTML)
  - Configurable parameters (max_tokens)
  - Image upload for allowlisted vision models (e.g. grok-4/grok-3)
  - Stop generation and clear conversation controls
- **Authentication**: Secure login with admin API key
- **Configuration**: Real-time config version display and reload
- **Dashboard**: Embedded Grafana dashboard for monitoring (requires Public Dashboard URL)

## Tech Stack

- Svelte 5 + SvelteKit + TypeScript
- Vite (build tool)
- Tailwind CSS (styling)
- pnpm (package manager)
- Lucide Svelte (icons)

## Available Scripts

```bash
pnpm run dev         # Start development server
pnpm run build       # Build for production
pnpm run preview     # Preview production build
pnpm run check       # Run Svelte check
pnpm run check:watch # Run Svelte check in watch mode
```

## Chat Feature

The Chat page allows you to interact with LLM models through the proxy:

### Usage

1. **Select a Model**: Choose from available models from enabled providers
2. **Configure Parameters** (optional):
   - **Max Tokens**: Maximum response length (100 - 8000, default: 2000)
3. **Set Credential Key**: Open Settings and set credential key
4. **Start Chatting**: Type your message and press Enter
5. **Streaming Responses**: Responses stream in real-time
6. **Controls**:
   - **Stop**: Interrupt generation at any time
   - **Clear**: Reset conversation history

### Keyboard Shortcuts

- `Enter`: Send message
- `Shift + Enter`: Insert new line

### Notes

- Chat uses `/v1/chat/completions` API with streaming enabled
- Requires valid credential keys configured in the system
- Chat page requires a credential key input (separate from the admin key)
- Models are loaded from `/v1/models` using the credential key (respects allowed_models)
- Image upload is enabled only for allowlisted models via `VITE_CHAT_VISION_MODEL_ALLOWLIST`

## Troubleshooting

### Connection Issues

1. Verify LLM Proxy server is running
2. Check API Base URL is correct
3. Ensure `ADMIN_KEY` is configured on server
4. Check browser console for errors

### Chat Issues

1. Ensure at least one provider and credential are configured
2. Check that providers are enabled
3. Verify credential keys are valid
4. Check network requests in browser dev tools

### Build Issues

```bash
# Clear dependencies and reinstall
rm -rf node_modules pnpm-lock.yaml
pnpm install
```

## Project Structure

```
src/
├── lib/
│   ├── api.ts           # API client
│   ├── stores.ts        # Svelte stores
│   └── types.ts         # TypeScript types
├── routes/
│   ├── +layout.svelte   # Main layout
│   ├── +page.svelte     # Login page
│   ├── providers/       # Provider management
│   ├── credentials/     # Credential management
│   ├── chat/           # Chat interface
│   └── dashboard/       # Grafana dashboard
└── app.css              # Global styles
```

## Grafana Integration

The Dashboard page embeds a Grafana Public Dashboard via iframe. To enable:

1. **Enable Public Dashboard in Grafana**:
   - Set `GF_FEATURE_TOGGLES_ENABLE=publicDashboards` in Grafana config
   - Set `GF_SECURITY_ALLOW_EMBEDDING=true` for iframe support
2. **Create a Public Dashboard**:
   - Open your dashboard in Grafana
   - Click Share → Public Dashboard
   - Enable and copy URL
3. **Configure URL**:
   - Set `PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL` in your `.env.local`

---

Last Updated: 2025-12-28 10:58 (Asia/Shanghai)
