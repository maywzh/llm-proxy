# Contributing to llm-proxy

Thank you for your interest in contributing!

## Branch Strategy

We use a simplified Gitflow:

```
main       ← stable releases only (tagged vX.Y.Z)
develop    ← integration branch, all PRs target here
```

**Branch naming:**

| Type | Pattern | Example |
|------|---------|---------|
| Feature | `feature/<scope>-<desc>` | `feature/gemini-thinking-support` |
| Bug fix | `fix/<scope>-<desc>` | `fix/ratelimit-burst-size` |
| Refactor | `refactor/<scope>-<desc>` | `refactor/streaming-state-machine` |
| Hotfix | `hotfix/<desc>` | `hotfix/auth-bypass` |

## Commit Messages

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <subject>
```

Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`

Examples:
- `feat(gemini): add thinking support`
- `fix(ratelimit): correct burst size calculation`
- `docs: update API reference`

A `commit-msg` hook enforces this format automatically.

## Pull Request Process

1. Fork the repo and create your branch from `develop`
2. Follow the branch naming convention above
3. Write tests for new functionality
4. Ensure CI passes (lint, format, secret scan)
5. Open a PR targeting `develop` (or `main` for hotfixes)
6. Fill in the PR template
7. Wait for code review

## Development Setup

### Python Server

```bash
cd python-server
uv sync
uv run pytest
```

### Rust Server

```bash
cd rust-server
cargo build
cargo test
```

### Web Admin

```bash
cd web/svelte-admin
pnpm install
pnpm dev
```

## Pre-commit Hooks

Install hooks before committing:

```bash
git config core.hooksPath .githooks
```

Hooks run automatically on `git commit`:
- Secret scanning (gitleaks)
- Rust: fmt + clippy
- Python: ruff format + lint
- React/Svelte: prettier + eslint

## Code of Conduct

Please read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before contributing.
