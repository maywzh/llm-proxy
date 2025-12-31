"""Main API router"""
from fastapi import APIRouter

from app.api import completions, models, metrics

api_router = APIRouter(prefix='/v1')

api_router.include_router(completions.router, tags=['completions'])
api_router.include_router(models.router, tags=['models'])

metrics_router = APIRouter()
metrics_router.include_router(metrics.router, tags=['metrics'])