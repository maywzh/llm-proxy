"""Main application entry point"""
import urllib3
from fastapi import FastAPI

from app.api.router import api_router, health_router
from app.services.provider_service import get_provider_service
from app.core.config import get_config

urllib3.disable_warnings(urllib3.exceptions.InsecureRequestWarning)


def create_app() -> FastAPI:
    """Create and configure FastAPI application"""
    config = get_config()
    
    app = FastAPI(
        title="LLM API Proxy",
        description="Weighted load balancer for LLM API requests",
        version="1.0.0"
    )
    
    app.include_router(api_router)
    app.include_router(health_router)
    
    @app.on_event("startup")
    async def startup_event():
        """Initialize services on startup"""
        provider_svc = get_provider_service()
        provider_svc.initialize()
        
        providers = provider_svc.get_all_providers()
        weights = provider_svc.get_provider_weights()
        total_weight = sum(weights)
        
        print(f"Starting LLM API Proxy with {len(providers)} providers")
        for i, provider in enumerate(providers):
            weight = weights[i]
            probability = (weight / total_weight) * 100
            print(f"  - {provider.name}: weight={weight} ({probability:.1f}%)")
        print(f"Master API key: {'Enabled' if config.server.master_api_key else 'Disabled'}")
    
    return app


app = create_app()