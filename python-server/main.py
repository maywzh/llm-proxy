#!/usr/bin/env python3
"""CLI entry point for LLM API Proxy (database mode only)"""
import os

import uvicorn

from app.core.config import get_env_config
from app.core.logging import setup_logging, get_logger


def main():
    """Main entry point"""
    setup_logging(log_level="INFO")
    logger = get_logger()

    env_config = get_env_config()

    if not env_config.db_url:
        logger.error("DB_URL environment variable is required")
        logger.info("Set DB_URL to your PostgreSQL connection string")
        raise SystemExit(1)

    host = env_config.host
    port = env_config.port

    logger.info(f"Starting LLM API Proxy (database mode)")
    logger.info(f"Listening on {host}:{port}")

    uvicorn.run(
        "app.main:app",
        host=host,
        port=port,
        log_config=None,
        access_log=True,
    )


if __name__ == "__main__":
    main()
