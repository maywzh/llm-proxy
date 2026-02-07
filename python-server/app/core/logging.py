"""Global loguru logging configuration"""

import sys
import logging
from pathlib import Path
from contextvars import ContextVar
from loguru import logger

# Context variable to store current provider name
current_provider: ContextVar[str] = ContextVar("current_provider", default="")

# Context variable to store current API key name for metrics
current_api_key_name: ContextVar[str] = ContextVar(
    "current_api_key_name", default="anonymous"
)


class InterceptHandler(logging.Handler):
    """Intercept standard logging messages and redirect to loguru"""

    def emit(self, record: logging.LogRecord) -> None:
        """Emit a log record to loguru"""
        # Get corresponding Loguru level if it exists
        try:
            level = logger.level(record.levelname).name
        except ValueError:
            level = record.levelno

        # Find caller from where originated the logged message
        frame, depth = sys._getframe(6), 6
        while frame and frame.f_code.co_filename == logging.__file__:
            frame = frame.f_back
            depth += 1

        # Add provider prefix for httpx logs
        message = record.getMessage()
        if record.name.startswith("httpx") and "HTTP Request:" in message:
            provider_name = current_provider.get()
            if provider_name:
                message = f"[Provider: {provider_name}] {message}"

        logger.opt(depth=depth, exception=record.exc_info).log(level, message)


def setup_logging(log_level: str = "INFO", log_file: str = "logs/app.log") -> None:
    """Configure loguru logging with console and file handlers

    Args:
        log_level: Logging level (DEBUG, INFO, WARNING, ERROR, CRITICAL)
        log_file: Path to log file
    """
    # Remove default handler
    logger.remove()

    # Add console handler with colored output
    logger.add(
        sys.stdout,
        format="<green>{time:YYYY-MM-DD HH:mm:ss}</green> | <level>{level: <8}</level> | <cyan>{name}</cyan>:<cyan>{function}</cyan>:<cyan>{line}</cyan> - <level>{message}</level>",
        level=log_level,
        colorize=True,
    )

    # Add file handler with rotation
    log_path = Path(log_file)
    log_path.parent.mkdir(parents=True, exist_ok=True)

    logger.add(
        log_file,
        format="{time:YYYY-MM-DD HH:mm:ss} | {level: <8} | {name}:{function}:{line} - {message}",
        level="DEBUG",  # File logs everything
        rotation="500 MB",
        retention="10 days",
        compression="zip",
        encoding="utf-8",
    )

    # Intercept standard logging (uvicorn, fastapi, etc.)
    logging.basicConfig(handlers=[InterceptHandler()], level=0, force=True)

    # Configure specific loggers
    for logger_name in [
        "uvicorn",
        "uvicorn.access",
        "uvicorn.error",
        "fastapi",
        "httpx",
    ]:
        logging_logger = logging.getLogger(logger_name)
        logging_logger.handlers = [InterceptHandler()]
        logging_logger.propagate = False

    logger.info(f"Logging initialized: level={log_level}, file={log_file}")


def set_provider_context(provider_name: str) -> None:
    """Set the current provider name in context for logging

    Args:
        provider_name: Name of the provider making the request
    """
    current_provider.set(provider_name)


def clear_provider_context() -> None:
    """Clear the provider context"""
    current_provider.set("")


def set_api_key_context(api_key_name: str) -> None:
    """Set the current API key name in context for metrics

    Args:
        api_key_name: Name of the API key making the request
    """
    current_api_key_name.set(api_key_name)


def get_api_key_name() -> str:
    """Get the current API key name from context

    Returns:
        The API key name, or 'anonymous' if not set
    """
    return current_api_key_name.get()


def clear_api_key_context() -> None:
    """Clear the API key context"""
    current_api_key_name.set("anonymous")


def get_logger():
    """Get the configured logger instance

    Returns:
        Configured loguru logger
    """
    return logger
