"""Health check API endpoints"""

from typing import Optional

from fastapi import APIRouter, Depends, HTTPException, status
from loguru import logger

from app.api.admin import verify_admin_key, get_db, ErrorResponse
from app.core.database import Database, get_provider_by_id
from app.models.health import (
    HealthCheckRequest,
    HealthCheckResponse,
    ProviderHealthStatus,
    HealthStatus,
)
from app.services.health_check_service import check_providers_health


router = APIRouter(prefix="/admin/v1/health", tags=["health"])


@router.post(
    "/check",
    response_model=HealthCheckResponse,
    summary="Check provider health",
    description="Check health status of all or specific providers by testing their models",
    responses={
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database not configured",
        },
    },
)
async def api_check_health(
    request: HealthCheckRequest = HealthCheckRequest(),
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> HealthCheckResponse:
    """Check health of providers

    This endpoint tests provider availability by making actual API calls
    with minimal token usage (max_tokens=5) to reduce costs.

    Args:
        request: Health check request with optional filters

    Returns:
        Health check response with status of each provider
    """
    logger.info(
        f"Health check requested: provider_ids={request.provider_ids}, "
        f"models={request.models}, timeout={request.timeout_secs}s, "
        f"max_concurrent={request.max_concurrent}"
    )

    # Check providers health
    health_statuses = await check_providers_health(
        db=db,
        provider_ids=request.provider_ids,
        models=request.models,
        timeout_secs=request.timeout_secs,
        max_concurrent=request.max_concurrent,
    )

    # Calculate summary statistics
    total_providers = len(health_statuses)
    healthy_providers = sum(
        1 for p in health_statuses if p.status == HealthStatus.HEALTHY
    )
    unhealthy_providers = sum(
        1 for p in health_statuses if p.status == HealthStatus.UNHEALTHY
    )

    logger.info(
        f"Health check completed: {healthy_providers}/{total_providers} healthy, "
        f"{unhealthy_providers} unhealthy"
    )

    return HealthCheckResponse(
        providers=health_statuses,
        total_providers=total_providers,
        healthy_providers=healthy_providers,
        unhealthy_providers=unhealthy_providers,
    )


@router.get(
    "/providers/{provider_id}",
    response_model=ProviderHealthStatus,
    summary="Get provider health status",
    description="Get health status of a specific provider by testing its models",
    responses={
        401: {
            "model": ErrorResponse,
            "description": "Unauthorized - Invalid or missing admin key",
        },
        404: {
            "model": ErrorResponse,
            "description": "Not found - Provider does not exist",
        },
        503: {
            "model": ErrorResponse,
            "description": "Service unavailable - Database not configured",
        },
    },
)
async def api_get_provider_health(
    provider_id: int,
    models: Optional[str] = None,
    timeout_secs: int = 10,
    _: None = Depends(verify_admin_key),
    db: Database = Depends(get_db),
) -> ProviderHealthStatus:
    """Get health status of a specific provider

    Args:
        provider_id: Provider ID to check
        models: Comma-separated list of models to test (optional)
        timeout_secs: Timeout for each model test in seconds (default: 10)

    Returns:
        Provider health status
    """
    # Get provider
    provider = await get_provider_by_id(db, provider_id)
    if not provider:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Provider with ID {provider_id} not found",
        )

    # Parse models parameter
    model_list = None
    if models:
        model_list = [m.strip() for m in models.split(",") if m.strip()]

    logger.info(
        f"Health check requested for provider {provider_id}: "
        f"models={model_list}, timeout={timeout_secs}s"
    )

    # Check provider health
    health_statuses = await check_providers_health(
        db=db,
        provider_ids=[provider_id],
        models=model_list,
        timeout_secs=timeout_secs,
    )

    if not health_statuses:
        raise HTTPException(
            status_code=status.HTTP_500_INTERNAL_SERVER_ERROR,
            detail="Failed to check provider health",
        )

    return health_statuses[0]
