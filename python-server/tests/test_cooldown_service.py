"""Tests for cooldown service"""

import time
from unittest.mock import patch

import pytest

from app.services.cooldown_service import (
    CooldownConfig,
    CooldownEntry,
    CooldownService,
    get_cooldown_service,
    init_cooldown_service,
    trigger_cooldown_if_needed,
    _get_exception_type,
)


@pytest.fixture
def cooldown_service():
    """Create a fresh cooldown service for testing."""
    return CooldownService()


@pytest.fixture
def cooldown_service_with_config():
    """Create cooldown service with custom config."""
    config = CooldownConfig(
        enabled=True,
        default_cooldown_secs=30,
        max_cooldown_secs=120,
        cooldown_status_codes={429, 500, 503},
        cooldown_durations={429: 60, 500: 30, 503: 90},
    )
    return CooldownService(config)


@pytest.mark.unit
class TestCooldownEntry:
    """Test CooldownEntry dataclass"""

    def test_entry_properties(self):
        """Test CooldownEntry properties"""
        now = time.time()
        entry = CooldownEntry(
            provider_key="provider1",
            exception_type="rate_limit",
            status_code=429,
            timestamp=now,
            cooldown_time=60,
            error_message="Rate limit exceeded",
        )

        assert entry.provider_key == "provider1"
        assert entry.exception_type == "rate_limit"
        assert entry.status_code == 429
        assert entry.cooldown_time == 60
        assert entry.error_message == "Rate limit exceeded"

        # Test expires_at
        assert entry.expires_at == now + 60

        # Test is_expired (should not be expired immediately)
        assert entry.is_expired is False

        # Test remaining_seconds
        assert 59 <= entry.remaining_seconds <= 60

    def test_entry_expired(self):
        """Test expired entry"""
        past_time = time.time() - 120  # 2 minutes ago
        entry = CooldownEntry(
            provider_key="provider1",
            exception_type="rate_limit",
            status_code=429,
            timestamp=past_time,
            cooldown_time=60,
        )

        assert entry.is_expired is True
        assert entry.remaining_seconds == 0

    def test_entry_iso_timestamps(self):
        """Test ISO timestamp formatting"""
        now = time.time()
        entry = CooldownEntry(
            provider_key="provider1",
            exception_type="server_error",
            status_code=500,
            timestamp=now,
            cooldown_time=30,
        )

        # Should be valid ISO format strings
        assert "T" in entry.started_at_iso
        assert "T" in entry.expires_at_iso
        assert entry.started_at_iso.endswith("+00:00")


@pytest.mark.unit
class TestCooldownConfig:
    """Test CooldownConfig dataclass"""

    def test_default_config(self):
        """Test default configuration values"""
        config = CooldownConfig()

        assert config.enabled is True
        assert config.default_cooldown_secs == 60
        assert config.max_cooldown_secs == 600
        # Only 429 and 5xx trigger cooldown
        assert 429 in config.cooldown_status_codes
        assert 500 in config.cooldown_status_codes
        # 4xx errors (including auth) do NOT trigger cooldown
        assert 400 in config.non_cooldown_status_codes
        assert 401 in config.non_cooldown_status_codes
        assert 403 in config.non_cooldown_status_codes
        assert 404 in config.non_cooldown_status_codes
        assert 422 in config.non_cooldown_status_codes

    def test_custom_config(self):
        """Test custom configuration"""
        config = CooldownConfig(
            enabled=False,
            default_cooldown_secs=120,
            max_cooldown_secs=300,
        )

        assert config.enabled is False
        assert config.default_cooldown_secs == 120
        assert config.max_cooldown_secs == 300


@pytest.mark.unit
class TestGetExceptionType:
    """Test _get_exception_type helper"""

    def test_rate_limit(self):
        assert _get_exception_type(429) == "rate_limit"

    def test_server_errors(self):
        assert _get_exception_type(500) == "server_error"
        assert _get_exception_type(502) == "server_error"
        assert _get_exception_type(503) == "server_error"
        assert _get_exception_type(504) == "server_error"

    def test_unknown(self):
        # 4xx errors (except 429) are unknown since they don't trigger cooldown
        assert _get_exception_type(401) == "unknown_error"
        assert _get_exception_type(403) == "unknown_error"
        assert _get_exception_type(404) == "unknown_error"
        assert _get_exception_type(408) == "unknown_error"
        assert _get_exception_type(418) == "unknown_error"


@pytest.mark.unit
class TestCooldownService:
    """Test CooldownService class"""

    def test_should_trigger_cooldown_429(self, cooldown_service):
        """Test 429 triggers cooldown"""
        assert cooldown_service.should_trigger_cooldown(429) is True

    def test_should_trigger_cooldown_5xx(self, cooldown_service):
        """Test 5xx errors trigger cooldown"""
        assert cooldown_service.should_trigger_cooldown(500) is True
        assert cooldown_service.should_trigger_cooldown(502) is True
        assert cooldown_service.should_trigger_cooldown(503) is True
        assert cooldown_service.should_trigger_cooldown(504) is True

    def test_should_not_trigger_cooldown_400(self, cooldown_service):
        """Test 400 does not trigger cooldown"""
        assert cooldown_service.should_trigger_cooldown(400) is False

    def test_should_not_trigger_cooldown_401(self, cooldown_service):
        """Test 401 does not trigger cooldown (auth error is client issue)"""
        assert cooldown_service.should_trigger_cooldown(401) is False

    def test_should_not_trigger_cooldown_403(self, cooldown_service):
        """Test 403 does not trigger cooldown"""
        assert cooldown_service.should_trigger_cooldown(403) is False

    def test_should_not_trigger_cooldown_404(self, cooldown_service):
        """Test 404 does not trigger cooldown"""
        assert cooldown_service.should_trigger_cooldown(404) is False

    def test_should_not_trigger_cooldown_422(self, cooldown_service):
        """Test 422 does not trigger cooldown"""
        assert cooldown_service.should_trigger_cooldown(422) is False

    def test_should_not_trigger_when_disabled(self):
        """Test cooldown disabled"""
        config = CooldownConfig(enabled=False)
        service = CooldownService(config)

        assert service.should_trigger_cooldown(429) is False
        assert service.should_trigger_cooldown(500) is False

    def test_add_cooldown(self, cooldown_service):
        """Test adding a provider to cooldown"""
        entry = cooldown_service.add_cooldown(
            provider_key="provider1",
            status_code=429,
            error_message="Rate limit exceeded",
        )

        assert entry.provider_key == "provider1"
        assert entry.status_code == 429
        assert entry.exception_type == "rate_limit"
        assert entry.cooldown_time == 60  # Default for 429

    def test_add_cooldown_with_custom_duration(self, cooldown_service):
        """Test adding cooldown with custom duration"""
        entry = cooldown_service.add_cooldown(
            provider_key="provider1",
            status_code=429,
            cooldown_time=120,
        )

        assert entry.cooldown_time == 120

    def test_add_cooldown_caps_duration(self, cooldown_service):
        """Test cooldown duration is capped"""
        entry = cooldown_service.add_cooldown(
            provider_key="provider1",
            status_code=429,
            cooldown_time=9999,  # Exceeds max
        )

        assert entry.cooldown_time == 600  # Max cooldown

    def test_is_in_cooldown(self, cooldown_service):
        """Test checking if provider is in cooldown"""
        cooldown_service.add_cooldown(
            provider_key="provider1",
            status_code=429,
        )

        assert cooldown_service.is_in_cooldown("provider1") is True
        assert cooldown_service.is_in_cooldown("provider2") is False

    def test_is_in_cooldown_expired(self, cooldown_service):
        """Test expired cooldown returns False"""
        # Add with very short cooldown
        entry = cooldown_service.add_cooldown(
            provider_key="provider1",
            status_code=429,
            cooldown_time=0,  # Immediate expiration
        )

        # Should be expired
        assert cooldown_service.is_in_cooldown("provider1") is False

    def test_get_cooldown(self, cooldown_service):
        """Test getting cooldown entry"""
        cooldown_service.add_cooldown(
            provider_key="provider1",
            status_code=429,
        )

        entry = cooldown_service.get_cooldown("provider1")
        assert entry is not None
        assert entry.provider_key == "provider1"

        # Non-existent provider
        assert cooldown_service.get_cooldown("provider2") is None

    def test_remove_cooldown(self, cooldown_service):
        """Test removing cooldown"""
        cooldown_service.add_cooldown(
            provider_key="provider1",
            status_code=429,
        )

        assert cooldown_service.remove_cooldown("provider1") is True
        assert cooldown_service.is_in_cooldown("provider1") is False

        # Remove non-existent
        assert cooldown_service.remove_cooldown("provider2") is False

    def test_get_all_cooldowns(self, cooldown_service):
        """Test getting all cooldowns"""
        cooldown_service.add_cooldown("provider1", 429)
        cooldown_service.add_cooldown("provider2", 500)

        all_cooldowns = cooldown_service.get_all_cooldowns()

        assert len(all_cooldowns) == 2
        assert "provider1" in all_cooldowns
        assert "provider2" in all_cooldowns

    def test_get_all_cooldowns_filters_expired(self, cooldown_service):
        """Test expired cooldowns are filtered"""
        cooldown_service.add_cooldown("provider1", 429, cooldown_time=0)
        cooldown_service.add_cooldown("provider2", 500, cooldown_time=60)

        all_cooldowns = cooldown_service.get_all_cooldowns()

        assert len(all_cooldowns) == 1
        assert "provider2" in all_cooldowns
        assert "provider1" not in all_cooldowns

    def test_clear_all_cooldowns(self, cooldown_service):
        """Test clearing all cooldowns"""
        cooldown_service.add_cooldown("provider1", 429)
        cooldown_service.add_cooldown("provider2", 500)

        count = cooldown_service.clear_all_cooldowns()

        assert count == 2
        assert len(cooldown_service.get_all_cooldowns()) == 0

    def test_filter_available_providers(self, cooldown_service):
        """Test filtering providers in cooldown"""

        class MockProvider:
            def __init__(self, name):
                self.name = name

        providers = [MockProvider("p1"), MockProvider("p2"), MockProvider("p3")]
        weights = [1, 2, 3]

        # Put p2 in cooldown
        cooldown_service.add_cooldown("p2", 429)

        filtered_providers, filtered_weights = cooldown_service.filter_available_providers(
            providers, weights
        )

        assert len(filtered_providers) == 2
        assert filtered_providers[0].name == "p1"
        assert filtered_providers[1].name == "p3"
        assert filtered_weights == [1, 3]

    def test_filter_available_providers_disabled(self):
        """Test filtering when cooldown is disabled"""
        config = CooldownConfig(enabled=False)
        service = CooldownService(config)

        class MockProvider:
            def __init__(self, name):
                self.name = name

        providers = [MockProvider("p1"), MockProvider("p2")]
        weights = [1, 2]

        # Add cooldown (should be ignored when disabled)
        service.add_cooldown("p2", 429)

        filtered_providers, filtered_weights = service.filter_available_providers(
            providers, weights
        )

        # All providers should be returned when disabled
        assert len(filtered_providers) == 2
        assert filtered_weights == [1, 2]


@pytest.mark.unit
class TestCooldownServiceThreadSafety:
    """Test thread safety of CooldownService"""

    def test_concurrent_add_cooldown(self, cooldown_service):
        """Test concurrent cooldown additions"""
        import concurrent.futures

        def add_cooldown(i):
            cooldown_service.add_cooldown(f"provider{i}", 429)
            return f"provider{i}"

        with concurrent.futures.ThreadPoolExecutor(max_workers=10) as executor:
            futures = [executor.submit(add_cooldown, i) for i in range(100)]
            results = [f.result() for f in futures]

        assert len(results) == 100
        all_cooldowns = cooldown_service.get_all_cooldowns()
        assert len(all_cooldowns) == 100

    def test_concurrent_read_write(self, cooldown_service):
        """Test concurrent reads and writes"""
        import concurrent.futures

        def mixed_operation(i):
            if i % 2 == 0:
                cooldown_service.add_cooldown(f"provider{i}", 429)
            else:
                cooldown_service.is_in_cooldown(f"provider{i-1}")
            return True

        with concurrent.futures.ThreadPoolExecutor(max_workers=20) as executor:
            futures = [executor.submit(mixed_operation, i) for i in range(200)]
            results = [f.result() for f in futures]

        assert all(results)


@pytest.mark.unit
class TestGlobalCooldownService:
    """Test global cooldown service functions"""

    def test_get_cooldown_service_singleton(self):
        """Test singleton pattern"""
        from app.services import cooldown_service as cs_module

        # Reset singleton
        cs_module._cooldown_service = None

        svc1 = get_cooldown_service()
        svc2 = get_cooldown_service()

        assert svc1 is svc2

    def test_init_cooldown_service(self):
        """Test initializing with custom config"""
        from app.services import cooldown_service as cs_module

        config = CooldownConfig(default_cooldown_secs=120)
        service = init_cooldown_service(config)

        assert service.config.default_cooldown_secs == 120
        assert get_cooldown_service() is service

        # Clean up
        cs_module._cooldown_service = None

    def test_trigger_cooldown_if_needed_429(self):
        """Test trigger helper with 429"""
        from app.services import cooldown_service as cs_module

        cs_module._cooldown_service = None
        init_cooldown_service()

        entry = trigger_cooldown_if_needed(
            provider_key="test-provider",
            status_code=429,
            error_message="Rate limit",
        )

        assert entry is not None
        assert entry.provider_key == "test-provider"
        assert entry.status_code == 429

        # Clean up
        cs_module._cooldown_service = None

    def test_trigger_cooldown_if_needed_400(self):
        """Test trigger helper with non-triggering status"""
        from app.services import cooldown_service as cs_module

        cs_module._cooldown_service = None
        init_cooldown_service()

        entry = trigger_cooldown_if_needed(
            provider_key="test-provider",
            status_code=400,
            error_message="Bad request",
        )

        assert entry is None

        # Clean up
        cs_module._cooldown_service = None


@pytest.mark.unit
class TestCooldownWithProviderService:
    """Test cooldown integration with provider service"""

    def test_provider_selection_with_cooldown(self, monkeypatch, clear_config_cache):
        """Test that cooled down providers are skipped"""
        from app.models.config import AppConfig, ProviderConfig
        from app.services import provider_service as ps_module
        from app.services import cooldown_service as cs_module

        # Reset cooldown service
        cs_module._cooldown_service = None
        init_cooldown_service()

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="provider1",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=1,
                    model_mapping={"gpt-4": "gpt-4"},
                ),
                ProviderConfig(
                    name="provider2",
                    api_base="https://api2.com",
                    api_key="key2",
                    weight=1,
                    model_mapping={"gpt-4": "gpt-4"},
                ),
            ],
            verify_ssl=True,
        )

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
        ps_module._provider_service = None

        from app.services.provider_service import ProviderService

        service = ProviderService()
        service.initialize()

        # Put provider1 in cooldown
        cooldown_svc = get_cooldown_service()
        cooldown_svc.add_cooldown("provider1", 429, cooldown_time=300)

        # All selections should be provider2
        for _ in range(20):
            provider = service.get_next_provider(model="gpt-4")
            assert provider.name == "provider2"

        # Clean up
        cs_module._cooldown_service = None

    def test_all_providers_in_cooldown_error(self, monkeypatch, clear_config_cache):
        """Test error when all providers are in cooldown"""
        from app.models.config import AppConfig, ProviderConfig
        from app.services import provider_service as ps_module
        from app.services import cooldown_service as cs_module

        # Reset cooldown service
        cs_module._cooldown_service = None
        init_cooldown_service()

        config = AppConfig(
            providers=[
                ProviderConfig(
                    name="provider1",
                    api_base="https://api1.com",
                    api_key="key1",
                    weight=1,
                    model_mapping={"gpt-4": "gpt-4"},
                ),
                ProviderConfig(
                    name="provider2",
                    api_base="https://api2.com",
                    api_key="key2",
                    weight=1,
                    model_mapping={"gpt-4": "gpt-4"},
                ),
            ],
            verify_ssl=True,
        )

        monkeypatch.setattr(ps_module, "get_config", lambda: config)
        ps_module._provider_service = None

        from app.services.provider_service import ProviderService

        service = ProviderService()
        service.initialize()

        # Put both providers in cooldown
        cooldown_svc = get_cooldown_service()
        cooldown_svc.add_cooldown("provider1", 429, cooldown_time=300)
        cooldown_svc.add_cooldown("provider2", 500, cooldown_time=300)

        # Should raise error
        with pytest.raises(ValueError, match="All providers.*are in cooldown"):
            service.get_next_provider(model="gpt-4")

        # Clean up
        cs_module._cooldown_service = None
