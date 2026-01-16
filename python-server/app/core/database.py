"""Database abstraction layer for configuration persistence (PostgreSQL only)"""

import asyncio
import hashlib
import os
import re
import uuid
from contextlib import asynccontextmanager
from dataclasses import dataclass
from datetime import datetime
from typing import AsyncGenerator, Optional
from urllib.parse import quote

from loguru import logger
from sqlalchemy import (
    Boolean,
    DateTime,
    Integer,
    String,
    Text,
    func,
    select,
    text,
    update,
    delete,
)
from sqlalchemy.dialects.postgresql import JSONB
from sqlalchemy.ext.asyncio import (
    AsyncEngine,
    AsyncSession,
    async_sessionmaker,
    create_async_engine,
)
from sqlalchemy.orm import DeclarativeBase, Mapped, mapped_column


class Base(DeclarativeBase):
    """SQLAlchemy declarative base"""

    pass


class ProviderModel(Base):
    """Provider database model"""

    __tablename__ = "providers"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    provider_key: Mapped[str] = mapped_column(
        String(255), nullable=False, unique=True, index=True
    )
    provider_type: Mapped[str] = mapped_column(String(50), nullable=False)
    api_base: Mapped[str] = mapped_column(String(500), nullable=False)
    api_key: Mapped[str] = mapped_column(String(500), nullable=False)
    model_mapping: Mapped[dict] = mapped_column(JSONB, nullable=False, default={})
    weight: Mapped[int] = mapped_column(Integer, nullable=False, default=1)
    is_enabled: Mapped[bool] = mapped_column(Boolean, nullable=False, default=True)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), nullable=False, server_default=func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        nullable=False,
        server_default=func.now(),
        onupdate=func.now(),
    )

    def get_model_mapping(self) -> dict[str, str]:
        """Get model_mapping dict"""
        return self.model_mapping or {}


class CredentialModel(Base):
    """Credential database model (renamed from master_keys)"""

    __tablename__ = "credentials"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, autoincrement=True)
    credential_key: Mapped[str] = mapped_column(
        String(255), nullable=False, unique=True, index=True
    )
    name: Mapped[str] = mapped_column(String(255), nullable=False)
    allowed_models: Mapped[list] = mapped_column(JSONB, nullable=False, default=[])
    rate_limit: Mapped[Optional[int]] = mapped_column(Integer, nullable=True)
    is_enabled: Mapped[bool] = mapped_column(Boolean, nullable=False, default=True)
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), nullable=False, server_default=func.now()
    )
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        nullable=False,
        server_default=func.now(),
        onupdate=func.now(),
    )


class ConfigVersionModel(Base):
    """Config version database model (singleton)"""

    __tablename__ = "config_version"

    id: Mapped[int] = mapped_column(Integer, primary_key=True, default=1)
    version: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), nullable=False, server_default=func.now()
    )


class RequestLogModel(Base):
    """Request log database model for audit and analytics"""

    __tablename__ = "request_logs"

    id: Mapped[str] = mapped_column(
        String(36), primary_key=True, default=lambda: str(uuid.uuid4())
    )
    created_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), nullable=False, server_default=func.now()
    )
    credential_id: Mapped[Optional[int]] = mapped_column(Integer, nullable=True)
    credential_name: Mapped[Optional[str]] = mapped_column(String(255), nullable=True)
    provider_id: Mapped[Optional[int]] = mapped_column(Integer, nullable=True)
    provider_name: Mapped[Optional[str]] = mapped_column(String(255), nullable=True)
    endpoint: Mapped[str] = mapped_column(String(100), nullable=False)
    method: Mapped[str] = mapped_column(String(10), nullable=False)
    model: Mapped[Optional[str]] = mapped_column(String(100), nullable=True)
    is_streaming: Mapped[bool] = mapped_column(Boolean, nullable=False, default=False)
    status_code: Mapped[Optional[int]] = mapped_column(Integer, nullable=True)
    duration_ms: Mapped[Optional[int]] = mapped_column(Integer, nullable=True)
    ttft_ms: Mapped[Optional[int]] = mapped_column(Integer, nullable=True)
    prompt_tokens: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    completion_tokens: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    total_tokens: Mapped[int] = mapped_column(Integer, nullable=False, default=0)
    request_body: Mapped[Optional[dict]] = mapped_column(JSONB, nullable=True)
    response_body: Mapped[Optional[dict]] = mapped_column(JSONB, nullable=True)
    error_message: Mapped[Optional[str]] = mapped_column(Text, nullable=True)
    client_ip: Mapped[Optional[str]] = mapped_column(String(45), nullable=True)
    user_agent: Mapped[Optional[str]] = mapped_column(Text, nullable=True)


def hash_key(key: str) -> str:
    """Hash a key for secure storage using SHA-256"""
    return hashlib.sha256(key.encode()).hexdigest()


def create_key_preview(key: str) -> str:
    """Create a preview of the key (e.g., 'sk-***abc')"""
    if len(key) <= 6:
        return "***"
    return f"{key[:3]}***{key[-3:]}"


class DatabaseConfig:
    """Database configuration (PostgreSQL only)"""

    def __init__(
        self,
        url: Optional[str] = None,
        pool_size: int = 10,
        max_overflow: int = 20,
        pool_timeout: int = 30,
        echo: bool = False,
    ):
        raw_url = url or os.environ.get("DB_URL")
        if raw_url:
            self.url = self._convert_url(raw_url)
        else:
            self.url = None
        self.pool_size = pool_size
        self.max_overflow = max_overflow
        self.pool_timeout = pool_timeout
        self.echo = echo

    @staticmethod
    def _convert_url(url: str) -> str:
        """Convert DB_URL to SQLAlchemy async URL format with password encoding.

        Also removes sslmode parameter as asyncpg doesn't support it directly.
        """
        encoded_url = _encode_password_in_url(url)
        encoded_url = _remove_sslmode_param(encoded_url)
        if encoded_url.startswith("postgresql://"):
            return encoded_url.replace("postgresql://", "postgresql+asyncpg://")
        elif encoded_url.startswith("postgres://"):
            return encoded_url.replace("postgres://", "postgresql+asyncpg://")
        elif encoded_url.startswith("postgresql+asyncpg://"):
            return encoded_url
        else:
            raise ValueError(
                f"Unsupported database URL: {url}. Only PostgreSQL is supported."
            )

    @property
    def is_configured(self) -> bool:
        """Check if database is configured"""
        return self.url is not None


def _remove_sslmode_param(url: str) -> str:
    """Remove sslmode parameter from URL as asyncpg doesn't support it directly.

    asyncpg uses ssl=True/False instead of sslmode parameter.
    """
    if "?" not in url:
        return url

    base_url, query_string = url.split("?", 1)
    params = query_string.split("&")
    filtered_params = [p for p in params if not p.startswith("sslmode=")]

    if not filtered_params:
        return base_url
    return f"{base_url}?{'&'.join(filtered_params)}"


def _encode_password_in_url(url: str) -> str:
    """
    Encode special characters in the password part of a database URL.
    Handles URLs in the format: postgresql://user:password@host:port/database
    Uses rfind to find the last @ which separates userinfo from host.
    """
    scheme_end = url.find("://")
    if scheme_end == -1:
        return url

    scheme = url[: scheme_end + 3]
    after_scheme = url[scheme_end + 3 :]

    at_pos = after_scheme.rfind("@")
    if at_pos == -1:
        return url

    userinfo = after_scheme[:at_pos]
    host_and_rest = after_scheme[at_pos + 1 :]

    colon_pos = userinfo.find(":")
    if colon_pos == -1:
        return url

    username = userinfo[:colon_pos]
    password = userinfo[colon_pos + 1 :]

    if not password:
        return url

    encoded_password = quote(password, safe="")

    return f"{scheme}{username}:{encoded_password}@{host_and_rest}"


class Database:
    """Database connection manager (PostgreSQL only)"""

    def __init__(self, config: Optional[DatabaseConfig] = None):
        self.config = config or DatabaseConfig()
        self._engine: Optional[AsyncEngine] = None
        self._session_factory: Optional[async_sessionmaker[AsyncSession]] = None

    async def connect(self) -> None:
        """Create database connection"""
        if self._engine is not None:
            return

        if not self.config.is_configured:
            raise RuntimeError(
                "Database URL not configured. Set DB_URL environment variable."
            )

        self._engine = create_async_engine(
            self.config.url,
            pool_size=self.config.pool_size,
            max_overflow=self.config.max_overflow,
            pool_timeout=self.config.pool_timeout,
            echo=self.config.echo,
        )

        self._session_factory = async_sessionmaker(
            self._engine,
            class_=AsyncSession,
            expire_on_commit=False,
        )

    async def disconnect(self) -> None:
        """Close database connection"""
        if self._engine is not None:
            await self._engine.dispose()
            self._engine = None
            self._session_factory = None

    @asynccontextmanager
    async def session(self) -> AsyncGenerator[AsyncSession, None]:
        """Get database session context manager"""
        if self._session_factory is None:
            raise RuntimeError("Database not connected")

        async with self._session_factory() as session:
            try:
                yield session
                await session.commit()
            except Exception:
                await session.rollback()
                raise

    async def check_migrations(self) -> bool:
        """Check if migrations have been applied (by golang-migrate)"""
        if self._engine is None:
            raise RuntimeError("Database not connected")

        async with self._engine.begin() as conn:
            result = await conn.execute(
                text(
                    "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_name='schema_migrations')"
                )
            )
            row = result.fetchone()
            return bool(row and row[0])

    async def is_empty(self) -> bool:
        """Check if database has no providers configured"""
        if self._engine is None:
            raise RuntimeError("Database not connected")

        async with self._engine.begin() as conn:
            result = await conn.execute(text("SELECT COUNT(*) FROM providers"))
            row = result.fetchone()
            return row[0] == 0 if row else True

    async def get_config_version(self) -> int:
        """Get current config version from database"""
        if self._engine is None:
            raise RuntimeError("Database not connected")

        async with self._engine.begin() as conn:
            result = await conn.execute(
                text("SELECT version FROM config_version WHERE id = 1")
            )
            row = result.fetchone()
            return row[0] if row else 0


@dataclass
class InitResult:
    """Initialization result"""

    providers: list = None
    credentials: list = None
    version: int = 0

    def __post_init__(self):
        if self.providers is None:
            self.providers = []
        if self.credentials is None:
            self.credentials = []


@dataclass
class VersionedConfig:
    """Versioned configuration wrapper"""

    version: int
    timestamp: datetime
    providers: list
    credentials: list


class DynamicConfig:
    """Dynamic configuration manager with thread-safe updates using asyncio.Lock"""

    def __init__(self, db: "Database"):
        self.db = db
        self._config: Optional[VersionedConfig] = None
        self._lock = asyncio.Lock()

    @property
    def config(self) -> Optional[VersionedConfig]:
        """Get current configuration (read is lock-free)"""
        return self._config

    @property
    def providers(self) -> list:
        """Get current providers"""
        return self._config.providers if self._config else []

    @property
    def credentials(self) -> list:
        """Get current credentials"""
        return self._config.credentials if self._config else []

    @property
    def version(self) -> int:
        """Get current config version"""
        return self._config.version if self._config else 0

    async def load(self) -> VersionedConfig:
        """Load configuration from database"""
        async with self._lock:
            return await self._load_from_db()

    async def reload(self) -> VersionedConfig:
        """Reload configuration from database and update AppConfig and ProviderService"""
        async with self._lock:
            config = await self._load_from_db()
            self._sync_app_config(config)
            logger.info(f"Configuration reloaded, version={config.version}")
            return config

    def _sync_app_config(self, versioned_config: VersionedConfig) -> None:
        """Sync AppConfig and ProviderService with the loaded configuration"""
        from app.core.config import set_config, get_env_config, clear_config_cache
        from app.models.config import (
            AppConfig,
            ProviderConfig,
            CredentialConfig,
            RateLimitConfig,
            ServerConfig,
        )
        from app.services.provider_service import get_provider_service

        env_config = get_env_config()

        providers = [
            ProviderConfig(
                name=p.provider_key,
                api_base=p.api_base,
                api_key=p.api_key,
                weight=p.weight,
                model_mapping=p.get_model_mapping(),
            )
            for p in versioned_config.providers
        ]

        credentials = [
            CredentialConfig(
                credential_key=cred.credential_key,
                name=cred.name,
                rate_limit=(
                    RateLimitConfig(
                        requests_per_second=cred.rate_limit,
                        burst_size=cred.rate_limit,
                    )
                    if cred.rate_limit
                    else None
                ),
                enabled=cred.is_enabled,
                allowed_models=cred.allowed_models or [],
            )
            for cred in versioned_config.credentials
        ]

        new_config = AppConfig(
            providers=providers,
            credentials=credentials,
            server=ServerConfig(host=env_config.host, port=env_config.port),
            verify_ssl=env_config.verify_ssl,
            request_timeout_secs=env_config.request_timeout_secs,
        )

        clear_config_cache()
        set_config(new_config)

        provider_svc = get_provider_service()
        provider_svc.reinitialize()

        logger.info(
            f"AppConfig and ProviderService synced: {len(providers)} providers, {len(credentials)} credentials"
        )

    async def _load_from_db(self) -> VersionedConfig:
        """Internal method to load config from database"""
        async with self.db.session() as session:
            provider_result = await session.execute(
                select(ProviderModel).where(ProviderModel.is_enabled == True)
            )
            providers = list(provider_result.scalars().all())

            credential_result = await session.execute(
                select(CredentialModel).where(CredentialModel.is_enabled == True)
            )
            credentials = list(credential_result.scalars().all())

        version = await self.db.get_config_version()

        self._config = VersionedConfig(
            version=version,
            timestamp=datetime.utcnow(),
            providers=providers,
            credentials=credentials,
        )
        return self._config


async def load_config_from_db(db: Database) -> InitResult:
    """Load configuration from database and return InitResult"""
    if not await db.check_migrations():
        raise RuntimeError(
            "Database migrations not applied. "
            "Please run './scripts/db_migrate.sh' first."
        )

    logger.info("Loading configuration from database")
    async with db.session() as session:
        provider_result = await session.execute(
            select(ProviderModel).where(ProviderModel.is_enabled == True)
        )
        providers = list(provider_result.scalars().all())

        credential_result = await session.execute(
            select(CredentialModel).where(CredentialModel.is_enabled == True)
        )
        credentials = list(credential_result.scalars().all())

    version = await db.get_config_version()

    return InitResult(
        providers=providers,
        credentials=credentials,
        version=version,
    )


_database: Optional[Database] = None
_dynamic_config: Optional[DynamicConfig] = None


def get_database() -> Optional[Database]:
    """Get global database instance"""
    return _database


def get_dynamic_config() -> Optional[DynamicConfig]:
    """Get global dynamic config instance"""
    return _dynamic_config


async def init_database() -> Optional[Database]:
    """Initialize and connect to database. DB_URL must be configured."""
    global _database, _dynamic_config

    config = DatabaseConfig()
    if not config.is_configured:
        raise RuntimeError("DB_URL environment variable is required")

    _database = Database(config)
    await _database.connect()
    logger.info("Database connected")

    _dynamic_config = DynamicConfig(_database)
    return _database


async def close_database() -> None:
    """Close database connection"""
    global _database, _dynamic_config
    if _database is not None:
        await _database.disconnect()
        _database = None
        _dynamic_config = None


async def list_providers(
    db: Database, enabled_only: bool = False
) -> list[ProviderModel]:
    """List all providers"""
    async with db.session() as session:
        stmt = select(ProviderModel)
        if enabled_only:
            stmt = stmt.where(ProviderModel.is_enabled == True)
        result = await session.execute(stmt)
        return list(result.scalars().all())


async def get_provider_by_id(db: Database, provider_id: int) -> Optional[ProviderModel]:
    """Get provider by ID (auto-increment integer)"""
    async with db.session() as session:
        stmt = select(ProviderModel).where(ProviderModel.id == provider_id)
        result = await session.execute(stmt)
        return result.scalar_one_or_none()


async def get_provider_by_key(
    db: Database, provider_key: str
) -> Optional[ProviderModel]:
    """Get provider by provider_key (unique string identifier)"""
    async with db.session() as session:
        stmt = select(ProviderModel).where(ProviderModel.provider_key == provider_key)
        result = await session.execute(stmt)
        return result.scalar_one_or_none()


async def create_provider(
    db: Database,
    provider_key: str,
    provider_type: str,
    api_base: str,
    api_key: str,
    model_mapping: Optional[dict] = None,
) -> ProviderModel:
    """Create a new provider"""
    async with db.session() as session:
        provider = ProviderModel(
            provider_key=provider_key,
            provider_type=provider_type,
            api_base=api_base,
            api_key=api_key,
            model_mapping=model_mapping or {},
            is_enabled=True,
        )
        session.add(provider)
        await session.flush()
        await session.refresh(provider)
        return provider


async def update_provider(
    db: Database, provider_id: int, **kwargs
) -> Optional[ProviderModel]:
    """Update provider by ID (auto-increment integer)"""
    async with db.session() as session:
        stmt = (
            update(ProviderModel)
            .where(ProviderModel.id == provider_id)
            .values(**kwargs)
            .returning(ProviderModel)
        )
        result = await session.execute(stmt)
        return result.scalar_one_or_none()


async def delete_provider(db: Database, provider_id: int) -> bool:
    """Delete provider by ID (auto-increment integer)"""
    async with db.session() as session:
        stmt = delete(ProviderModel).where(ProviderModel.id == provider_id)
        result = await session.execute(stmt)
        return result.rowcount > 0


async def list_credentials(
    db: Database, enabled_only: bool = False
) -> list[CredentialModel]:
    """List all credentials"""
    async with db.session() as session:
        stmt = select(CredentialModel)
        if enabled_only:
            stmt = stmt.where(CredentialModel.is_enabled == True)
        result = await session.execute(stmt)
        return list(result.scalars().all())


async def get_credential_by_id(
    db: Database, credential_id: int
) -> Optional[CredentialModel]:
    """Get credential by ID (auto-increment integer)"""
    async with db.session() as session:
        stmt = select(CredentialModel).where(CredentialModel.id == credential_id)
        result = await session.execute(stmt)
        return result.scalar_one_or_none()


async def get_credential_by_key(db: Database, key: str) -> Optional[CredentialModel]:
    """Get credential by credential_key (for authentication)"""
    credential_key = hash_key(key)
    async with db.session() as session:
        stmt = select(CredentialModel).where(
            CredentialModel.credential_key == credential_key
        )
        result = await session.execute(stmt)
        return result.scalar_one_or_none()


async def create_credential(
    db: Database,
    key: str,
    name: str,
    allowed_models: Optional[list] = None,
    rate_limit: Optional[int] = None,
) -> CredentialModel:
    """Create a new credential"""
    async with db.session() as session:
        credential = CredentialModel(
            credential_key=hash_key(key),
            name=name,
            allowed_models=allowed_models or [],
            rate_limit=rate_limit,
            is_enabled=True,
        )
        session.add(credential)
        await session.flush()
        await session.refresh(credential)
        return credential


async def update_credential(
    db: Database, credential_id: int, **kwargs
) -> Optional[CredentialModel]:
    """Update credential by ID (auto-increment integer)"""
    if "key" in kwargs:
        kwargs["credential_key"] = hash_key(kwargs.pop("key"))
    async with db.session() as session:
        stmt = (
            update(CredentialModel)
            .where(CredentialModel.id == credential_id)
            .values(**kwargs)
            .returning(CredentialModel)
        )
        result = await session.execute(stmt)
        return result.scalar_one_or_none()


async def delete_credential(db: Database, credential_id: int) -> bool:
    """Delete credential by ID (auto-increment integer)"""
    async with db.session() as session:
        stmt = delete(CredentialModel).where(CredentialModel.id == credential_id)
        result = await session.execute(stmt)
        return result.rowcount > 0


async def insert_log_entries(db: Database, log_data: list[dict]) -> None:
    """Batch insert log entries into the database.

    Args:
        db: Database instance
        log_data: List of dictionaries containing log entry data
    """
    if not log_data:
        return

    async with db.session() as session:
        log_models = [RequestLogModel(**data) for data in log_data]
        session.add_all(log_models)
        await session.flush()


async def cleanup_old_logs(db: Database, retention_days: int = 30) -> int:
    """Delete log entries older than retention period.

    Args:
        db: Database instance
        retention_days: Number of days to retain logs

    Returns:
        Number of deleted entries
    """
    from datetime import timedelta

    cutoff = datetime.utcnow() - timedelta(days=retention_days)
    async with db.session() as session:
        stmt = delete(RequestLogModel).where(RequestLogModel.created_at < cutoff)
        result = await session.execute(stmt)
        return result.rowcount


@dataclass
class LogQueryParams:
    """Parameters for log queries"""

    page: int = 1
    page_size: int = 20
    credential_id: Optional[int] = None
    provider_id: Optional[int] = None
    model: Optional[str] = None
    start_date: Optional[datetime] = None
    end_date: Optional[datetime] = None
    status_code: Optional[int] = None
    has_error: Optional[bool] = None


@dataclass
class LogListResult:
    """Result of log list query"""

    logs: list[RequestLogModel]
    total: int
    page: int
    page_size: int


@dataclass
class LogStats:
    """Log statistics"""

    total_requests: int
    error_count: int
    error_rate: float
    avg_duration_ms: float
    total_tokens: int
    by_model: dict
    by_provider: dict


async def get_logs(db: Database, params: LogQueryParams) -> LogListResult:
    """Get logs with filtering and pagination.

    Args:
        db: Database instance
        params: Query parameters for filtering and pagination

    Returns:
        LogListResult with logs, total count, and pagination info
    """
    async with db.session() as session:
        # Build base query
        stmt = select(RequestLogModel)
        count_stmt = select(func.count(RequestLogModel.id))

        # Apply filters
        conditions = []
        if params.credential_id is not None:
            conditions.append(RequestLogModel.credential_id == params.credential_id)
        if params.provider_id is not None:
            conditions.append(RequestLogModel.provider_id == params.provider_id)
        if params.model is not None:
            conditions.append(RequestLogModel.model == params.model)
        if params.start_date is not None:
            conditions.append(RequestLogModel.created_at >= params.start_date)
        if params.end_date is not None:
            conditions.append(RequestLogModel.created_at <= params.end_date)
        if params.status_code is not None:
            conditions.append(RequestLogModel.status_code == params.status_code)
        if params.has_error is not None:
            if params.has_error:
                conditions.append(RequestLogModel.error_message.isnot(None))
            else:
                conditions.append(RequestLogModel.error_message.is_(None))

        # Apply conditions to both queries
        for condition in conditions:
            stmt = stmt.where(condition)
            count_stmt = count_stmt.where(condition)

        # Get total count
        count_result = await session.execute(count_stmt)
        total = count_result.scalar() or 0

        # Apply ordering and pagination
        stmt = stmt.order_by(RequestLogModel.created_at.desc())
        offset = (params.page - 1) * params.page_size
        stmt = stmt.offset(offset).limit(params.page_size)

        # Execute query
        result = await session.execute(stmt)
        logs = list(result.scalars().all())

        return LogListResult(
            logs=logs,
            total=total,
            page=params.page,
            page_size=params.page_size,
        )


async def get_log_by_id(db: Database, log_id: str) -> Optional[RequestLogModel]:
    """Get a single log entry by ID.

    Args:
        db: Database instance
        log_id: UUID of the log entry

    Returns:
        RequestLogModel if found, None otherwise
    """
    async with db.session() as session:
        stmt = select(RequestLogModel).where(RequestLogModel.id == log_id)
        result = await session.execute(stmt)
        return result.scalar_one_or_none()


async def delete_logs_before(db: Database, before_date: datetime) -> int:
    """Delete logs older than specified date.

    Args:
        db: Database instance
        before_date: Delete logs created before this date

    Returns:
        Number of deleted entries
    """
    async with db.session() as session:
        stmt = delete(RequestLogModel).where(RequestLogModel.created_at < before_date)
        result = await session.execute(stmt)
        return result.rowcount


async def get_log_stats(
    db: Database,
    start_date: Optional[datetime] = None,
    end_date: Optional[datetime] = None,
) -> LogStats:
    """Get log statistics.

    Args:
        db: Database instance
        start_date: Optional start date filter
        end_date: Optional end date filter

    Returns:
        LogStats with aggregated statistics
    """
    async with db.session() as session:
        # Build base conditions
        conditions = []
        if start_date is not None:
            conditions.append(RequestLogModel.created_at >= start_date)
        if end_date is not None:
            conditions.append(RequestLogModel.created_at <= end_date)

        # Get total requests and error count
        total_stmt = select(func.count(RequestLogModel.id))
        error_stmt = select(func.count(RequestLogModel.id)).where(
            RequestLogModel.error_message.isnot(None)
        )
        avg_duration_stmt = select(func.avg(RequestLogModel.duration_ms))
        total_tokens_stmt = select(
            func.coalesce(func.sum(RequestLogModel.total_tokens), 0)
        )

        for condition in conditions:
            total_stmt = total_stmt.where(condition)
            error_stmt = error_stmt.where(condition)
            avg_duration_stmt = avg_duration_stmt.where(condition)
            total_tokens_stmt = total_tokens_stmt.where(condition)

        total_result = await session.execute(total_stmt)
        total_requests = total_result.scalar() or 0

        error_result = await session.execute(error_stmt)
        error_count = error_result.scalar() or 0

        avg_duration_result = await session.execute(avg_duration_stmt)
        avg_duration_ms = avg_duration_result.scalar() or 0.0

        total_tokens_result = await session.execute(total_tokens_stmt)
        total_tokens = total_tokens_result.scalar() or 0

        error_rate = error_count / total_requests if total_requests > 0 else 0.0

        # Get stats by model
        model_stmt = (
            select(
                RequestLogModel.model,
                func.count(RequestLogModel.id).label("count"),
                func.coalesce(func.sum(RequestLogModel.total_tokens), 0).label(
                    "tokens"
                ),
                func.avg(RequestLogModel.duration_ms).label("avg_duration"),
            )
            .where(RequestLogModel.model.isnot(None))
            .group_by(RequestLogModel.model)
        )
        for condition in conditions:
            model_stmt = model_stmt.where(condition)

        model_result = await session.execute(model_stmt)
        by_model = {
            row.model: {
                "count": row.count,
                "tokens": int(row.tokens),
                "avg_duration_ms": float(row.avg_duration) if row.avg_duration else 0.0,
            }
            for row in model_result
        }

        # Get stats by provider
        provider_stmt = (
            select(
                RequestLogModel.provider_name,
                func.count(RequestLogModel.id).label("count"),
                func.coalesce(func.sum(RequestLogModel.total_tokens), 0).label(
                    "tokens"
                ),
                func.avg(RequestLogModel.duration_ms).label("avg_duration"),
            )
            .where(RequestLogModel.provider_name.isnot(None))
            .group_by(RequestLogModel.provider_name)
        )
        for condition in conditions:
            provider_stmt = provider_stmt.where(condition)

        provider_result = await session.execute(provider_stmt)
        by_provider = {
            row.provider_name: {
                "count": row.count,
                "tokens": int(row.tokens),
                "avg_duration_ms": float(row.avg_duration) if row.avg_duration else 0.0,
            }
            for row in provider_result
        }

        return LogStats(
            total_requests=total_requests,
            error_count=error_count,
            error_rate=error_rate,
            avg_duration_ms=float(avg_duration_ms) if avg_duration_ms else 0.0,
            total_tokens=int(total_tokens),
            by_model=by_model,
            by_provider=by_provider,
        )
