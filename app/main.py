"""Main application entry point"""
import urllib3
from fastapi import FastAPI

from app.api.router import api_router, health_router, metrics_router
from app.services.provider_service import get_provider_service
from app.core.config import get_config
from app.core.middleware import MetricsMiddleware
from app.core.metrics import APP_INFO
from app.core.logging import setup_logging, get_logger

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
        logger.info(f"Master API key: {'Enabled' if config.server.master_api_key else 'Disabled'}")
        logger.info(f"Metrics endpoint: /metrics")
    
    return app


app = create_app()