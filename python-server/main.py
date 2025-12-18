#!/usr/bin/env python3
"""CLI entry point for LLM API Proxy"""
import argparse
import os

import uvicorn

from app.core.config import load_config
from app.core.logging import setup_logging, get_logger


def main():
    """Main entry point"""
    # Initialize logging early
    setup_logging(log_level="INFO")
    logger = get_logger()

    parser = argparse.ArgumentParser(description="LLM API Proxy Server")
    parser.add_argument(
        "--config",
        type=str,
        default="config.yaml",
        help="Path to configuration file (default: config.yaml)",
    )
    args = parser.parse_args()

    config = load_config(args.config)

    host = os.environ.get("HOST", config.server.host)
    port = int(os.environ.get("PORT", config.server.port))

    logger.info(f"Using config file: {args.config}")
    logger.info(f"Listening on {host}:{port}")

    # Configure uvicorn to use loguru
    uvicorn.run(
        "app.main:app",
        host=host,
        port=port,
        log_config=None,  # Disable uvicorn's default logging config
        access_log=True,  # Enable access logs (will be intercepted by loguru)
    )


if __name__ == "__main__":
    main()
