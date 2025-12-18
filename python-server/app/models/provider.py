"""Provider runtime model"""
from dataclasses import dataclass
from typing import Dict


@dataclass
class Provider:
    """Runtime provider instance"""
    name: str
    api_base: str
    api_key: str
    weight: int
    model_mapping: Dict[str, str]