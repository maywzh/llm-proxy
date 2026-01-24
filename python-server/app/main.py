"""Main application entry point.

Configuration is loaded from the database via DynamicConfig.
Environment variables control server settings (host, port, etc.).
"""

import urllib3
from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware
from fastapi.openapi.utils import get_openapi

from app.api.router import api_router, metrics_router, admin_router
from app.services.provider_service import get_provider_service
from app.services.langfuse_service import (
    init_langfuse_service,
    shutdown_langfuse_service,
)
from app.core.config import get_config, get_env_config
from app.core.http_client import get_http_client, close_http_client
from app.core.middleware import MetricsMiddleware
from app.core.metrics import APP_INFO
from app.core.logging import setup_logging, get_logger
from app.core.security import init_rate_limiter
from app.core.database import init_database, close_database, get_dynamic_config

urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)

logger = get_logger()


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan manager"""
    setup_logging(log_level="INFO")
    logger.info("Starting LLM API Proxy...")

    APP_INFO.info({"version": "1.0.0", "title": "LLM API Proxy"})

    get_http_client()
    logger.info("Shared HTTP client initialized")

    env_config = get_env_config()

    if not env_config.db_url:
        raise RuntimeError("DB_URL environment variable is required")

    logger.info("Connecting to database...")
    db = await init_database()
    if db is None:
        raise RuntimeError("Failed to initialize database")

    dynamic_config = get_dynamic_config()
    if dynamic_config is None:
        raise RuntimeError("Failed to initialize dynamic config")

    versioned_config = await dynamic_config.reload()

    logger.info(
        f"Configuration loaded from database, version={versioned_config.version}"
    )
    logger.info(
        f"Loaded {len(versioned_config.providers)} providers and {len(versioned_config.credentials)} credentials"
    )

    if not versioned_config.providers:
        logger.warning("No providers configured. Add providers via Admin API.")
    if not versioned_config.credentials:
        logger.warning("No credentials configured. API authentication is disabled.")

    _log_provider_info()

    # Initialize Langfuse service (optional, fails gracefully if not configured)
    init_langfuse_service()

    yield

    logger.info("Shutting down LLM API Proxy")

    # Shutdown Langfuse service (flushes pending events)
    shutdown_langfuse_service()

    await close_http_client()
    await close_database()
    logger.info("Cleanup completed")


def _log_provider_info() -> None:
    """Log provider and credential information"""
    init_rate_limiter()

    provider_svc = get_provider_service()
    providers = provider_svc.get_all_providers()
    weights = provider_svc.get_provider_weights()
    total_weight = sum(weights) if weights else 1

    logger.info(f"Starting LLM API Proxy with {len(providers)} providers")
    for i, provider in enumerate(providers):
        weight = weights[i] if i < len(weights) else 1
        probability = (weight / total_weight) * 100
        logger.info(f"  - {provider.name}: weight={weight} ({probability:.1f}%)")

    config = get_config()
    if config.credentials:
        logger.info(f"Credentials: {len(config.credentials)} configured")
        for credential_config in config.credentials:
            if credential_config.rate_limit:
                logger.info(
                    f"  - Key ending in ...{credential_config.credential_key[-8:]}: "
                    f"{credential_config.rate_limit.requests_per_second} req/s, "
                    f"burst={credential_config.rate_limit.burst_size}"
                )
            else:
                logger.info(
                    f"  - Key ending in ...{credential_config.credential_key[-8:]}: unlimited"
                )
    else:
        logger.info("Credential API key: Disabled")

    logger.info("Metrics endpoint: /metrics")


def create_app() -> FastAPI:
    """Create and configure FastAPI application"""
    app = FastAPI(
        title="LLM Proxy API",
        description="LLM Proxy API with OpenAI-compatible endpoints and Admin API for configuration management.",
        version="1.0.0",
        lifespan=lifespan,
        docs_url="/swagger-ui",
        redoc_url="/redoc",
        openapi_url="/api-docs/openapi.json",
        openapi_tags=[
            {
                "name": "completions",
                "description": "OpenAI-compatible completion endpoints",
            },
            {
                "name": "models",
                "description": "OpenAI-compatible model listing endpoints",
            },
            {"name": "providers", "description": "Provider management endpoints"},
            {"name": "credentials", "description": "Credential management endpoints"},
            {"name": "config", "description": "Configuration management endpoints"},
            {"name": "health", "description": "Health check endpoints"},
        ],
    )

    def custom_openapi():
        if app.openapi_schema:
            return app.openapi_schema
        openapi_schema = get_openapi(
            title="LLM Proxy API",
            version="1.0.0",
            description="LLM Proxy API with OpenAI-compatible endpoints and Admin API for configuration management.",
            routes=app.routes,
            tags=[
                {
                    "name": "completions",
                    "description": "OpenAI-compatible completion endpoints",
                },
                {
                    "name": "models",
                    "description": "OpenAI-compatible model listing endpoints",
                },
                {"name": "providers", "description": "Provider management endpoints"},
                {
                    "name": "credentials",
                    "description": "Credential management endpoints",
                },
                {"name": "config", "description": "Configuration management endpoints"},
                {"name": "health", "description": "Health check endpoints"},
            ],
        )
        openapi_schema["servers"] = [
            {
                "url": "http://127.0.0.1:18000",
                "description": "Local development server",
            },
            {
                "url": "http://localhost:18000",
                "description": "Local development server (localhost)",
            },
        ]
        openapi_schema["components"]["securitySchemes"] = {
            "bearer_auth": {
                "type": "http",
                "scheme": "bearer",
                "bearerFormat": "JWT",
            }
        }
        openapi_schema["security"] = [{"bearer_auth": []}]
        app.openapi_schema = openapi_schema
        return app.openapi_schema

    app.openapi = custom_openapi

    app.add_middleware(
        CORSMiddleware,
        allow_origins=["http://localhost:5173", "http://127.0.0.1:5173"],
        allow_credentials=True,
        allow_methods=["*"],
        allow_headers=["*"],
    )

    app.add_middleware(MetricsMiddleware)

    app.include_router(api_router)
    app.include_router(metrics_router)
    app.include_router(admin_router)

    @app.get("/health")
    async def health_check():
        """Health check endpoint"""
        config = get_config()
        return {
            "status": "ok",
            "providers_count": len(config.providers),
            "credentials_count": len(config.credentials),
        }

    return app


app = create_app()
