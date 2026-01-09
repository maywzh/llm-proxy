# LLM Proxy Admin - Svelte

A Svelte-based admin interface for managing LLM Proxy configurations.

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
# Create a public dashboard in Grafana and paste the URL here
# See: https://grafana.com/docs/grafana/latest/dashboards/dashboard-public/
PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL=
```

### Login Credentials

- **API Base URL**: Your LLM Proxy server URL (e.g., `http://127.0.0.1:17999`)
- **Admin API Key**: The `ADMIN_KEY` from your server configuration

## Features

- **Provider Management**: Create, edit, delete, and toggle LLM providers
- **Master Key Management**: Manage API keys with rate limiting and model restrictions
- **Authentication**: Secure login with admin API key
- **Configuration**: Real-time config version display and reload
- **Dashboard**: Embedded Grafana dashboard for monitoring (requires Public Dashboard URL)

## Tech Stack

- Svelte 5 + SvelteKit + TypeScript
- Vite (build tool)
- Tailwind CSS (styling)
- pnpm (package manager)

## Available Scripts

```bash
pnpm run dev         # Start development server
pnpm run build       # Build for production
pnpm run preview     # Preview production build
pnpm run check       # Run Svelte check
pnpm run check:watch # Run Svelte check in watch mode
```

## Troubleshooting

### Connection Issues

1. Verify LLM Proxy server is running
2. Check API Base URL is correct
3. Ensure `ADMIN_KEY` is configured on server
4. Check browser console for errors

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
   - Enable and copy the URL

3. **Configure the URL**:
   - Set `PUBLIC_GRAFANA_PUBLIC_DASHBOARD_URL` in your `.env.local`
