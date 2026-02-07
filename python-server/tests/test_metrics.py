"""Tests for Prometheus metrics"""

import pytest

from app.core.metrics import (
    REQUEST_COUNT,
    REQUEST_DURATION,
    ACTIVE_REQUESTS,
    TOKEN_USAGE,
    PROVIDER_HEALTH,
    PROVIDER_LATENCY,
    APP_INFO,
)


@pytest.mark.unit
class TestMetricsDefinitions:
    """Test that metrics are properly defined"""

    def test_request_count_metric_exists(self):
        """Test REQUEST_COUNT metric is defined"""
        assert REQUEST_COUNT is not None
        # prometheus_client Counter._name doesn't include _total suffix
        assert REQUEST_COUNT._name == "llm_proxy_requests"
        assert REQUEST_COUNT._type == "counter"

    def test_request_count_labels(self):
        """Test REQUEST_COUNT has correct labels"""
        # Increment with all labels
        REQUEST_COUNT.labels(
            method="POST",
            endpoint="/chat/completions",
            model="gpt-4",
            provider="test",
            status_code="200",
            api_key_name="test-key",
            client="test-client",
        ).inc()

        # Should not raise error
        assert True

    def test_request_duration_metric_exists(self):
        """Test REQUEST_DURATION metric is defined"""
        assert REQUEST_DURATION is not None
        assert REQUEST_DURATION._name == "llm_proxy_request_duration_seconds"
        assert REQUEST_DURATION._type == "histogram"

    def test_request_duration_labels(self):
        """Test REQUEST_DURATION has correct labels"""
        REQUEST_DURATION.labels(
            method="POST",
            endpoint="/chat/completions",
            model="gpt-4",
            provider="test",
            api_key_name="test-key",
            client="test-client",
        ).observe(1.5)

        assert True

    def test_request_duration_buckets(self):
        """Test REQUEST_DURATION has appropriate buckets"""
        # Check that buckets are defined via _upper_bounds attribute
        assert hasattr(REQUEST_DURATION, "_upper_bounds")
        # Should have buckets for various durations (prometheus_client returns list)
        expected_buckets = [
            0.1,
            0.5,
            1.0,
            2.0,
            5.0,
            10.0,
            30.0,
            60.0,
            120.0,
            float("inf"),
        ]
        assert list(REQUEST_DURATION._upper_bounds) == expected_buckets

    def test_active_requests_metric_exists(self):
        """Test ACTIVE_REQUESTS metric is defined"""
        assert ACTIVE_REQUESTS is not None
        assert ACTIVE_REQUESTS._name == "llm_proxy_active_requests"
        assert ACTIVE_REQUESTS._type == "gauge"

    def test_active_requests_labels(self):
        """Test ACTIVE_REQUESTS has correct labels"""
        ACTIVE_REQUESTS.labels(endpoint="/chat/completions").inc()
        ACTIVE_REQUESTS.labels(endpoint="/chat/completions").dec()

        assert True

    def test_token_usage_metric_exists(self):
        """Test TOKEN_USAGE metric is defined"""
        assert TOKEN_USAGE is not None
        # prometheus_client Counter._name doesn't include _total suffix
        assert TOKEN_USAGE._name == "llm_proxy_tokens"
        assert TOKEN_USAGE._type == "counter"

    def test_token_usage_labels(self):
        """Test TOKEN_USAGE has correct labels"""
        TOKEN_USAGE.labels(
            model="gpt-4",
            provider="test",
            token_type="prompt",
            api_key_name="test-key",
            client="test-client",
        ).inc(10)

        TOKEN_USAGE.labels(
            model="gpt-4",
            provider="test",
            token_type="completion",
            api_key_name="test-key",
            client="test-client",
        ).inc(20)

        TOKEN_USAGE.labels(
            model="gpt-4",
            provider="test",
            token_type="total",
            api_key_name="test-key",
            client="test-client",
        ).inc(30)

        assert True

    def test_provider_health_metric_exists(self):
        """Test PROVIDER_HEALTH metric is defined"""
        assert PROVIDER_HEALTH is not None
        assert PROVIDER_HEALTH._name == "llm_proxy_provider_health"
        assert PROVIDER_HEALTH._type == "gauge"

    def test_provider_health_labels(self):
        """Test PROVIDER_HEALTH has correct labels"""
        PROVIDER_HEALTH.labels(provider="test").set(1)
        PROVIDER_HEALTH.labels(provider="test").set(0)

        assert True

    def test_provider_latency_metric_exists(self):
        """Test PROVIDER_LATENCY metric is defined"""
        assert PROVIDER_LATENCY is not None
        assert PROVIDER_LATENCY._name == "llm_proxy_provider_latency_seconds"
        assert PROVIDER_LATENCY._type == "histogram"

    def test_provider_latency_labels(self):
        """Test PROVIDER_LATENCY has correct labels"""
        PROVIDER_LATENCY.labels(provider="test").observe(0.5)

        assert True

    def test_app_info_metric_exists(self):
        """Test APP_INFO metric is defined"""
        assert APP_INFO is not None
        assert APP_INFO._name == "llm_proxy_app"
        assert APP_INFO._type == "info"


@pytest.mark.unit
class TestMetricsUsage:
    """Test metrics usage patterns"""

    def test_increment_request_count(self):
        """Test incrementing request count"""
        initial_value = REQUEST_COUNT.labels(
            method="GET",
            endpoint="/health",
            model="none",
            provider="none",
            status_code="200",
            api_key_name="test-key",
            client="test-client",
        )._value.get()

        REQUEST_COUNT.labels(
            method="GET",
            endpoint="/health",
            model="none",
            provider="none",
            status_code="200",
            api_key_name="test-key",
            client="test-client",
        ).inc()

        final_value = REQUEST_COUNT.labels(
            method="GET",
            endpoint="/health",
            model="none",
            provider="none",
            status_code="200",
            api_key_name="test-key",
            client="test-client",
        )._value.get()

        assert final_value > initial_value

    def test_observe_request_duration(self):
        """Test observing request duration"""
        REQUEST_DURATION.labels(
            method="POST",
            endpoint="/chat/completions",
            model="gpt-4",
            provider="test",
            api_key_name="test-key",
            client="test-client",
        ).observe(2.5)

        # Check that observation was recorded
        metric = REQUEST_DURATION.labels(
            method="POST",
            endpoint="/chat/completions",
            model="gpt-4",
            provider="test",
            api_key_name="test-key",
            client="test-client",
        )

        assert metric._sum.get() > 0

    def test_gauge_increment_decrement(self):
        """Test gauge increment and decrement"""
        gauge = ACTIVE_REQUESTS.labels(endpoint="/test")

        initial = gauge._value.get()
        gauge.inc()
        after_inc = gauge._value.get()
        gauge.dec()
        after_dec = gauge._value.get()

        assert after_inc == initial + 1
        assert after_dec == initial

    def test_token_usage_tracking(self):
        """Test tracking token usage"""
        TOKEN_USAGE.labels(
            model="gpt-4",
            provider="test-provider",
            token_type="prompt",
            api_key_name="test-key",
            client="test-client",
        ).inc(100)

        TOKEN_USAGE.labels(
            model="gpt-4",
            provider="test-provider",
            token_type="completion",
            api_key_name="test-key",
            client="test-client",
        ).inc(200)

        TOKEN_USAGE.labels(
            model="gpt-4",
            provider="test-provider",
            token_type="total",
            api_key_name="test-key",
            client="test-client",
        ).inc(300)

        # Verify values were recorded
        prompt_metric = TOKEN_USAGE.labels(
            model="gpt-4",
            provider="test-provider",
            token_type="prompt",
            api_key_name="test-key",
            client="test-client",
        )
        assert prompt_metric._value.get() >= 100

    def test_provider_health_status(self):
        """Test setting provider health status"""
        PROVIDER_HEALTH.labels(provider="healthy-provider").set(1)
        PROVIDER_HEALTH.labels(provider="unhealthy-provider").set(0)

        healthy = PROVIDER_HEALTH.labels(provider="healthy-provider")._value.get()
        unhealthy = PROVIDER_HEALTH.labels(provider="unhealthy-provider")._value.get()

        assert healthy == 1
        assert unhealthy == 0

    def test_provider_latency_observation(self):
        """Test observing provider latency"""
        PROVIDER_LATENCY.labels(provider="fast-provider").observe(0.1)
        PROVIDER_LATENCY.labels(provider="slow-provider").observe(5.0)

        fast_metric = PROVIDER_LATENCY.labels(provider="fast-provider")
        slow_metric = PROVIDER_LATENCY.labels(provider="slow-provider")

        assert fast_metric._sum.get() > 0
        assert slow_metric._sum.get() > 0


@pytest.mark.unit
class TestMetricsLabels:
    """Test metrics with different label combinations"""

    def test_multiple_models(self):
        """Test tracking metrics for multiple models"""
        models = ["gpt-4", "gpt-3.5-turbo", "claude-3"]

        for model in models:
            REQUEST_COUNT.labels(
                method="POST",
                endpoint="/chat/completions",
                model=model,
                provider="test",
                status_code="200",
                api_key_name="test-key",
                client="test-client",
            ).inc()

        # All models should have separate counters
        for model in models:
            metric = REQUEST_COUNT.labels(
                method="POST",
                endpoint="/chat/completions",
                model=model,
                provider="test",
                status_code="200",
                api_key_name="test-key",
                client="test-client",
            )
            assert metric._value.get() > 0

    def test_multiple_providers(self):
        """Test tracking metrics for multiple providers"""
        providers = ["provider1", "provider2", "provider3"]

        for provider in providers:
            TOKEN_USAGE.labels(
                model="gpt-4",
                provider=provider,
                token_type="total",
                api_key_name="test-key",
                client="test-client",
            ).inc(100)

        # All providers should have separate counters
        for provider in providers:
            metric = TOKEN_USAGE.labels(
                model="gpt-4",
                provider=provider,
                token_type="total",
                api_key_name="test-key",
                client="test-client",
            )
            assert metric._value.get() >= 100

    def test_multiple_status_codes(self):
        """Test tracking different status codes"""
        status_codes = ["200", "400", "500"]

        for code in status_codes:
            REQUEST_COUNT.labels(
                method="POST",
                endpoint="/chat/completions",
                model="gpt-4",
                provider="test",
                status_code=code,
                api_key_name="test-key",
                client="test-client",
            ).inc()

        # Each status code should have its own counter
        for code in status_codes:
            metric = REQUEST_COUNT.labels(
                method="POST",
                endpoint="/chat/completions",
                model="gpt-4",
                provider="test",
                status_code=code,
                api_key_name="test-key",
                client="test-client",
            )
            assert metric._value.get() > 0

    def test_token_types(self):
        """Test tracking different token types"""
        token_types = ["prompt", "completion", "total"]

        for token_type in token_types:
            TOKEN_USAGE.labels(
                model="gpt-4",
                provider="test",
                token_type=token_type,
                api_key_name="test-key",
                client="test-client",
            ).inc(50)

        # Each token type should have its own counter
        for token_type in token_types:
            metric = TOKEN_USAGE.labels(
                model="gpt-4",
                provider="test",
                token_type=token_type,
                api_key_name="test-key",
                client="test-client",
            )
            assert metric._value.get() >= 50


@pytest.mark.unit
class TestMetricsEdgeCases:
    """Test edge cases in metrics"""

    def test_zero_duration(self):
        """Test observing zero duration"""
        REQUEST_DURATION.labels(
            method="GET",
            endpoint="/health",
            model="none",
            provider="none",
            api_key_name="test-key",
            client="test-client",
        ).observe(0.0)

        # Should not raise error
        assert True

    def test_very_large_duration(self):
        """Test observing very large duration"""
        REQUEST_DURATION.labels(
            method="POST",
            endpoint="/chat/completions",
            model="gpt-4",
            provider="test",
            api_key_name="test-key",
            client="test-client",
        ).observe(1000.0)

        # Should be recorded in the +Inf bucket
        assert True

    def test_negative_gauge_value(self):
        """Test gauge can go negative"""
        gauge = ACTIVE_REQUESTS.labels(endpoint="/test-negative")
        gauge.set(0)
        gauge.dec()

        value = gauge._value.get()
        assert value == -1

    def test_large_token_count(self):
        """Test tracking large token counts"""
        TOKEN_USAGE.labels(
            model="gpt-4",
            provider="test",
            token_type="total",
            api_key_name="test-key",
            client="test-client",
        ).inc(1000000)

        metric = TOKEN_USAGE.labels(
            model="gpt-4",
            provider="test",
            token_type="total",
            api_key_name="test-key",
            client="test-client",
        )
        assert metric._value.get() >= 1000000

    def test_special_characters_in_labels(self):
        """Test labels with special characters"""
        # Prometheus labels should handle various characters
        REQUEST_COUNT.labels(
            method="POST",
            endpoint="/v1/chat/completions",
            model="gpt-4-0613",
            provider="provider-1",
            status_code="200",
            api_key_name="test-key",
            client="test-client",
        ).inc()

        assert True


@pytest.mark.unit
class TestMetricsRegistry:
    """Test metrics registry integration"""

    def test_metrics_registered(self):
        """Test that all metrics are registered"""
        # Import metrics to ensure they are registered
        from app.core.metrics import (
            REQUEST_COUNT,
            REQUEST_DURATION,
            ACTIVE_REQUESTS,
            TOKEN_USAGE,
            PROVIDER_HEALTH,
            PROVIDER_LATENCY,
        )

        # Verify metrics exist and have correct names
        assert REQUEST_COUNT._name == "llm_proxy_requests"
        assert REQUEST_DURATION._name == "llm_proxy_request_duration_seconds"
        assert ACTIVE_REQUESTS._name == "llm_proxy_active_requests"
        assert TOKEN_USAGE._name == "llm_proxy_tokens"
        assert PROVIDER_HEALTH._name == "llm_proxy_provider_health"
        assert PROVIDER_LATENCY._name == "llm_proxy_provider_latency_seconds"

    def test_metrics_can_be_collected(self):
        """Test that metrics can be collected for export"""
        from prometheus_client import generate_latest, CollectorRegistry
        from prometheus_client import Counter

        # Use a fresh registry for this test
        test_registry = CollectorRegistry()

        # Create test metrics in the test registry
        test_counter = Counter(
            "test_llm_proxy_requests",
            "Test counter",
            ["method"],
            registry=test_registry,
        )
        test_counter.labels(method="POST").inc()

        # Generate metrics output from test registry
        output = generate_latest(test_registry)

        # Verify the test metric is in output
        assert b"test_llm_proxy_requests" in output
