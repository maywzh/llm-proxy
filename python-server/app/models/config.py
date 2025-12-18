"""Configuration models"""
from typing import Dict, Optional
from pydantic import BaseModel, Field


class ProviderConfig(BaseModel):
    """Provider configuration"""
    name: str
    api_base: str
    api_key: str
    weight: int = Field(default=1, ge=1)
    model_mapping: Dict[str, str] = Field(default_factory=dict)


class ServerConfig(BaseModel):
    """Server configuration"""
    host: str = "0.0.0.0"
    port: int = 18000
    master_api_key: Optional[str] = None


class AppConfig(BaseModel):
    """Application configuration"""
    providers: list[ProviderConfig]
    server: ServerConfig = Field(default_factory=ServerConfig)
    verify_ssl: bool = True