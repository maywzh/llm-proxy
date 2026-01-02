"""Main application entry point"""
import urllib3
from contextlib import asynccontextmanager

from fastapi import FastAPI
from fastapi.openapi.utils import get_openapi

from app.api.router import api_router, metrics_router, admin_router
from app.services.provider_service import get_provider_service
from app.core.config import (
    get_config,
    get_env_config,
    set_config,
)
from app.core.http_client import get_http_client, close_http_client
from app.core.middleware import MetricsMiddleware
from app.core.metrics import APP_INFO
from app.core.logging import setup_logging, get_logger
from app.core.security import init_rate_limiter
from app.core.database import (
    init_database,
    close_database,
    get_dynamic_config,
    load_config_from_db,
)
from app.models.config import AppConfig, ProviderConfig, MasterKeyConfig, RateLimitConfig, ServerConfig

urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)

logger = get_logger()


@asynccontextmanager
async def lifespan(app: FastAPI):
    """Application lifespan manager"""
    setup_logging(log_level="INFO")
    logger.info("Starting LLM API Proxy...")

    APP_INFO.info({
        'version': '1.0.0',
        'title': 'LLM API Proxy'
    })

    get_http_client()
    logger.info("Shared HTTP client initialized")

    env_config = get_env_config()

    if not env_config.db_url:
        raise RuntimeError("DB_URL environment variable is required. Only database mode is supported.")

    logger.info("Connecting to database...")
    db = await init_database()
    if db is None:
        raise RuntimeError("Failed to initialize database")

    init_result = await load_config_from_db(db)

    providers = [
        ProviderConfig(
            name=p.id,
            api_base=p.api_base,
            api_key=p.api_key,
            weight=1,
            model_mapping=p.get_model_mapping(),
        )
        for p in init_result.providers
    ]

    master_keys = [
        MasterKeyConfig(
            key=mk.key_hash,
            name=mk.name,
            rate_limit=RateLimitConfig(
                requests_per_second=mk.rate_limit,
                burst_size=mk.rate_limit,
            ) if mk.rate_limit else None,
            enabled=mk.is_enabled,
        )
        for mk in init_result.master_keys
    ]

    config = AppConfig(
        providers=providers,
        master_keys=master_keys,
        server=ServerConfig(host=env_config.host, port=env_config.port),
        verify_ssl=env_config.verify_ssl,
        request_timeout_secs=env_config.request_timeout_secs,
    )
    set_config(config)

    dynamic_config = get_dynamic_config()
    if dynamic_config:
        await dynamic_config.load()

    logger.info(f"Configuration loaded from database, version={init_result.version}")
    logger.info(f"Loaded {len(providers)} providers and {len(master_keys)} master keys")

    if not providers:
        logger.warning("No providers configured. Add providers via Admin API.")
    if not master_keys:
        logger.warning("No master keys configured. API authentication is disabled.")

    _initialize_services(config)

    yield

    logger.info("Shutting down LLM API Proxy")
    await close_http_client()
    await close_database()
    logger.info("Cleanup completed")


def _initialize_services(config: AppConfig) -> None:
    """Initialize services with the given configuration"""
    init_rate_limiter()

    provider_svc = get_provider_service()
    provider_svc.initialize()

    providers = provider_svc.get_all_providers()
    weights = provider_svc.get_provider_weights()
    total_weight = sum(weights) if weights else 1

    logger.info(f"Starting LLM API Proxy with {len(providers)} providers")
    for i, provider in enumerate(providers):
        weight = weights[i] if i < len(weights) else 1
        probability = (weight / total_weight) * 100
        logger.info(f"  - {provider.name}: weight={weight} ({probability:.1f}%)")

    if config.master_keys:
        logger.info(f"Master keys: {len(config.master_keys)} configured")
        for key_config in config.master_keys:
            if key_config.rate_limit:
                logger.info(f"  - Key ending in ...{key_config.key[-8:]}: "
                          f"{key_config.rate_limit.requests_per_second} req/s, "
                          f"burst={key_config.rate_limit.burst_size}")
            else:
                logger.info(f"  - Key ending in ...{key_config.key[-8:]}: unlimited")
    else:
        logger.info("Master API key: Disabled")

    logger.info("Metrics endpoint: /metrics")


def create_app() -> FastAPI:
    """Create and configure FastAPI application"""
    app = FastAPI(
        title="LLM Proxy Admin API",
        description="Admin API for managing LLM Proxy configuration including providers and master keys.",
        version="1.0.0",
        lifespan=lifespan,
        docs_url="/swagger-ui",
        redoc_url="/redoc",
        openapi_url="/api-docs/openapi.json",
        openapi_tags=[
            {"name": "providers", "description": "Provider management endpoints"},
            {"name": "master-keys", "description": "Master key management endpoints"},
            {"name": "config", "description": "Configuration management endpoints"},
            {"name": "health", "description": "Health check endpoints"},
        ],
    )

    def custom_openapi():
        if app.openapi_schema:
            return app.openapi_schema
        openapi_schema = get_openapi(
            title="LLM Proxy Admin API",
            version="1.0.0",
            description="Admin API for managing LLM Proxy configuration including providers and master keys.",
            routes=app.routes,
            tags=[
                {"name": "providers", "description": "Provider management endpoints"},
                {"name": "master-keys", "description": "Master key management endpoints"},
                {"name": "config", "description": "Configuration management endpoints"},
                {"name": "health", "description": "Health check endpoints"},
            ],
        )
        openapi_schema["servers"] = [
            {"url": "http://127.0.0.1:18000", "description": "Local development server"},
            {"url": "http://localhost:18000", "description": "Local development server (localhost)"},
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
            "master_keys_count": len(config.master_keys),
        }

    return app


app = create_app()