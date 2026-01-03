# LLM Proxy Admin - React

A React-based admin interface for managing LLM Proxy configurations.

## Quick Start

### Prerequisites

- Node.js 18+
- pnpm (recommended)

### Installation & Running

```bash
cd web/react-admin
pnpm install
pnpm run dev
```

Open [http://localhost:5173](http://localhost:5173) in your browser.

## Configuration

### Environment Variables

Copy `.env.example` to `.env.local` and configure:

```bash
# Optional: Default API base URL
VITE_API_BASE_URL=http://127.0.0.1:17999
```

### Login Credentials

- **API Base URL**: Your LLM Proxy server URL (e.g., `http://127.0.0.1:17999`)
- **Admin API Key**: The `ADMIN_KEY` from your server configuration

## Features

- **Provider Management**: Create, edit, delete, and toggle LLM providers
- **Master Key Management**: Manage API keys with rate limiting and model restrictions
- **Authentication**: Secure login with admin API key
- **Configuration**: Real-time config version display and reload

## Tech Stack

- React 18 + TypeScript
- Vite (build tool)
- React Router (routing)
- Tailwind CSS (styling)

## Available Scripts

```bash
pnpm run dev      # Start development server
pnpm run build    # Build for production
pnpm run preview  # Preview production build
pnpm run lint     # Run ESLint
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
├── api/client.ts         # API client
├── components/Layout.tsx # Main layout
├── contexts/            # React contexts
├── hooks/              # Custom hooks
├── pages/              # Page components
└── types/              # TypeScript types
