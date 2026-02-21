# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.1] - 2026-02-20

### Bug Fixes

- Improve SSE chunk rewriting and response handling in streaming utilities
- Add error handling for get_next_provider in benchmarks
- **react-admin**: Upgrade React to 19.2.3 to fix CVE-2025-55182
- Update environment variable name and improve iframe loading logic in Dashboard component
- **core**: Use database weight field instead of hardcoded value
- **chat**: Improve markdown rendering styles and list markers
- **rust-server**: Fix stale provider data on reload and optimize with caching
- Update image tags for admin UI deployments and improve prepare script error handling
- Chat width issue
- Chat page 500 internal error issue
- **health-check**: Respect VERIFY_SSL env var in health check service
- **streaming**: Establish downstream connection immediately before first token
- Langfuse trace issue
- Fix token counting issue
- Build scripts
- Credential model auth bug
- Fix svelte bugs
- Prevent SSE chunk truncation with line buffering
- Gemini3 handling and tests
- Handle Anthropic API cross-provider compatibility issues
- Always fetch fresh provider list in health check
- Handle Anthropic tool_result and blank text in cross-protocol conversion
- Persist response_body in error logs and add 5xx logging
- Reorder tool_result before text in Anthropic→OpenAI message split
- Propagate is_error in legacy Anthropic→OpenAI tool_result conversion
- Resolve active_requests gauge leak on client disconnect with RAII Drop guard
- **ratelimit**: Set burst_size equal to rps instead of rps*2
- **ratelimit**: Include key_name in 429 response for proper logging
- **streaming**: Extract usage metrics for Response API same-protocol path
- **ui**: Improve modal scroll behavior with flexbox layout
- **gemini**: Sanitize tool schema for Gemini API compatibility
- **gemini**: Use fileData for URL-type media instead of inlineData

### Documentation

- Update README for improved clarity and configuration examples
- Enhance README with dynamic configuration mode details and Admin API usage
- Update README with formatted service access links and Grafana dashboard details

### Features

- Add Prometheus metrics and logging for LLM API Proxy
- Add configuration files and environment variable support for Rust server
- Enhance detailed health check to test models serially and return model-level status
- Implement detailed health check with concurrent provider testing and serial model evaluation
- Update provider selection to support model filtering and enhance tests
- Add .gitignore to exclude analysis and debug files
- Add Kubernetes deployment and service configuration files
- Add Docker Compose configuration and Grafana dashboards for Rust LLM Proxy
- Update .dockerignore and .gitignore, add build script for Docker image management
- Add comprehensive README with setup instructions and configuration details for Python and Rust implementations
- Enhance error handling for provider responses in API and tests
- Add tvs-llm-proxy deployment, service, and ingress configurations for both Python and Rust
- Add token counting functionality for fallback calculations
- Add configurable request timeout for upstream providers
- Add example configuration and tests for request timeout with environment variable support
- Enhance error handling and logging for proxy requests, including client disconnects and network errors
- Enhance request logging to conditionally include model and provider for specific endpoint
- Add performance metrics for Time to First Token (TTFT) and Tokens Per Second (TPS) in streaming requests
- Enhance streaming error handling and response metadata attachment
- Implement shared HTTP client for improved request handling and resource management
- Optimize SSE stream processing by reducing unnecessary JSON parsing and improving token counting logic
- Update Grafana dashboard metrics and improve API key context handling
- Dynamic configuration mode
- Admin-ui
- Admin-key validation api
- TTFT timeout
- Add PROVIDER_SUFFIX support for model name prefix stripping
- Dark mode
- **chat**: Add copy actions panel component and persist selected model
- Implement code hightlightjs markdown rendering
- **chat**: Add share/regenerate actions, system prompt, and persistent settings
- **chat**: Render streaming thinking/reasoning in admin chat
- **web**: Switch provider model_mapping to JSON editor (React/Svelte)
- **admin-ui**: Add collapsible sidebar + edge toggle button, persist state, improve button visibility, add global button cursor, update docs
- **web-admin**: Move model/settings into chat composer; settings uses modal
- **web**: Extract ChatComposer component for chat input
- **health-check**: Add provider/model health check for admin UI
- **health**: Add per-provider health check API with concurrent model testing
- **langfuse**: Add Langfuse observability integration for LLM tracing
- Support claude api
- Extend Claude support, model mapping, and Langfuse hooks; expand tests and docs
- Use tiktoken for accurate token counting in count_tokens endpoint
- **rust-server**: Add tiered logging with DEBUG summary and TRACE full payload
- Add Gemini 3 thought_signature debugging support
- Add V2 cross-protocol transformation API with transformer pipeline
- **proxy**: Forward anthropic-beta and anthropic-version headers for Claude Code compatibility
- Add token calculation support
- Add image and tool token calculation support
- Implement v2 token count
- Add client label to metrics from User-Agent header
- Implement model info api
- Unify token calculation with OutboundTokenCounter
- **web**: Add toast notifications, skeleton loading, and UX improvements
- LiteLLM API compatibility
- Add client disconnect detection for streaming requests
- Add GCP Vertex AI proxy support with provider_params
- Redesign health check page UI with provider brand icons
- Add async error logging for provider 4xx errors
- Add anthropic-beta header policy, rectifier module, and tool_use pairing
- Rebrand web admin from LLM Proxy to HEN
- Improve chat UI with auto-resize textarea and refined layout
- Add mermaid diagram rendering, custom fonts, and Dockerfile migration
- Add adaptive provider routing with circuit breaker and unified upstream module
- Add clipboard paste support for chat images with multi-image management
- **ratelimit**: Exempt token counting endpoints from rate limiting
- Add response_api as provider_type with full streaming support
- **health**: Add provider_type to health check API and fix response_api payload
- Add request logs audit UI with full-stack implementation
- **health**: Add show disabled providers toggle and filter health checks
- **gcp-vertex**: Support configurable action verbs for Gemini models
- **gemini**: Add Gemini transformer with litellm-compatible thinking support

### Refactoring

- Optimize Dockerfile by removing dummy source code and adjusting CMD/ENTRYPOINT
- Rename all instances of 'tvs-llm-proxy' to 'llm-proxy' in documentation and configuration files
- Db migrate
- **web/react-admin**: Replace nginx with serve for static file serving
- Improve code quality based on architecture review
- **rust-server**: Apply Clippy lints and code quality improvements
- Unify stream metrics recording for all APIs
- Sync claude & gemini with litellm
- **ratelimit**: Synchronize rate limiting logic across Python/Rust servers

### Testing

- Add cross-language shared fixtures for Anthropic→OpenAI conversion

