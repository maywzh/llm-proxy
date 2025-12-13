"""Configuration management"""
import os
import re
from functools import lru_cache
from typing import Any

import yaml
from dotenv import load_dotenv

from app.models.config import AppConfig

load_dotenv()


def expand_env_vars(value: str) -> str:
    """Expand environment variables in string. Supports ${VAR}, ${VAR:-default}, ${VAR:default}"""
    if not isinstance(value, str):
        return value
    
    pattern = r'\$\{([^}:]+)(?::?-([^}]*))?\}'
    return re.sub(pattern, lambda m: os.environ.get(m.group(1), m.group(2) or ''), value)


def expand_config_env_vars(config: Any) -> Any:
    """Recursively expand environment variables in config"""
    if isinstance(config, dict):
        return {k: expand_config_env_vars(v) for k, v in config.items()}
    elif isinstance(config, list):
        return [expand_config_env_vars(item) for item in config]
    elif isinstance(config, str):
        return expand_env_vars(config)
    return config


def str_to_bool(value: Any) -> bool:
    """Convert string representation of boolean to actual boolean"""
    if isinstance(value, bool):
        return value
    if isinstance(value, str):
        return value.lower() in ('true', '1', 'yes', 'on')
    return bool(value)


def load_config(config_path: str = 'config.yaml') -> AppConfig:
    """Load and parse configuration from YAML file"""
    with open(config_path, 'r') as f:
        raw_config = yaml.safe_load(f)
    
    expanded_config = expand_config_env_vars(raw_config)
    expanded_config['verify_ssl'] = str_to_bool(expanded_config.get('verify_ssl', True))
    
    return AppConfig(**expanded_config)


@lru_cache
def get_config() -> AppConfig:
    """Get cached configuration instance"""
    config_path = os.environ.get('CONFIG_PATH', 'config.yaml')
    return load_config(config_path)