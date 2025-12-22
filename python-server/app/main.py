"""Main application entry point"""
import urllib3
from fastapi import FastAPI

from app.api.router import api_router, health_router, metrics_router
from app.services.provider_service import get_provider_service
from app.core.config import get_config
from app.core.middleware import MetricsMiddleware
from app.core.metrics import APP_INFO
from app.core.logging import setup_logging, get_logger
from app.core.security import init_rate_limiter

urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)

logger = get_logger()


def create_app() -> FastAPI:
    """Create and configure FastAPI application"""
    config = get_config()
    
    app = FastAPI(
        title="LLM API Proxy",
        description="Weighted load balancer for LLM API requests",
        version="1.0.0"
    )
    
    # Add metrics middleware
    app.add_middleware(MetricsMiddleware)
    
    # Include routers
    app.include_router(api_router)
    app.include_router(health_router)
    app.include_router(metrics_router)
    
    @app.on_event("startup")
    async def startup_event():
        """Initialize services on startup"""
        # Initialize logging
        setup_logging(log_level="INFO")
        
        # Set application info for Prometheus
        APP_INFO.info({
            'version': '1.0.0',
            'title': 'LLM API Proxy'
        })
        
        # Initialize rate limiter with master keys
        init_rate_limiter()
        
        provider_svc = get_provider_service()
        provider_svc.initialize()
        
        providers = provider_svc.get_all_providers()
        weights = provider_svc.get_provider_weights()
        total_weight = sum(weights)
        
        logger.info(f"Starting LLM API Proxy with {len(providers)} providers")
        for i, provider in enumerate(providers):
            weight = weights[i]
            probability = (weight / total_weight) * 100
            logger.info(f"  - {provider.name}: weight={weight} ({probability:.1f}%)")
        
        # Log authentication configuration
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
        
        logger.info(f"Metrics endpoint: /metrics")
    
    return app


app = create_app()