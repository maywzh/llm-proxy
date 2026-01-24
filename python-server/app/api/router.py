"""Main API router"""

from fastapi import APIRouter

from app.api import completions, models, metrics, admin, health, claude

api_router = APIRouter(prefix="/v1")

api_router.include_router(completions.router, tags=["completions"])
api_router.include_router(models.router, tags=["models"])

# Claude API router (note: claude.router already has /v1 prefix)
claude_router = APIRouter()
claude_router.include_router(claude.router, tags=["claude"])

metrics_router = APIRouter()
metrics_router.include_router(metrics.router, tags=["metrics"])

admin_router = APIRouter()
admin_router.include_router(admin.router, tags=["admin"])
admin_router.include_router(health.router, tags=["health"])
admin_router.include_router(health.provider_health_router, tags=["health"])
