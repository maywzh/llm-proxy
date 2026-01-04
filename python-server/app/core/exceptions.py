"""Custom exceptions for the application"""


class TTFTTimeoutError(Exception):
    """Raised when the first token is not received within the configured timeout"""

    def __init__(self, timeout_secs: int, provider_name: str):
        self.timeout_secs = timeout_secs
        self.provider_name = provider_name
        super().__init__(
            f"TTFT timeout: first token not received within {timeout_secs} seconds from provider {provider_name}"
        )
