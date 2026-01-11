//! Tests for credential rate limiting functionality.

use llm_proxy_rust::{
    api::AppState,
    core::{
        config::{AppConfig, CredentialConfig, ProviderConfig, RateLimitConfig, ServerConfig},
        RateLimiter,
    },
    services::ProviderService,
};
use std::collections::HashMap;
use std::sync::Arc;

#[test]
fn test_rate_limiter_allows_within_limit() {
    let limiter = RateLimiter::new();
    let config = RateLimitConfig {
        requests_per_second: 10,
        burst_size: 10,
    };

    limiter.register_key("test-key", &config);

    // Should allow up to burst_size requests
    for _ in 0..10 {
        assert!(limiter.check_rate_limit("test-key").is_ok());
    }
}

#[test]
fn test_rate_limiter_blocks_over_limit() {
    let limiter = RateLimiter::new();
    let config = RateLimitConfig {
        requests_per_second: 5,
        burst_size: 5,
    };

    limiter.register_key("test-key", &config);

    // Use up all tokens
    for _ in 0..5 {
        assert!(limiter.check_rate_limit("test-key").is_ok());
    }

    // Next request should be blocked
    assert!(limiter.check_rate_limit("test-key").is_err());
}

#[test]
fn test_rate_limiter_no_limit_for_unregistered_key() {
    let limiter = RateLimiter::new();

    // Unregistered keys should not be rate limited
    for _ in 0..100 {
        assert!(limiter.check_rate_limit("unregistered-key").is_ok());
    }
}

#[test]
fn test_multiple_keys_independent_limits() {
    let limiter = RateLimiter::new();

    let config1 = RateLimitConfig {
        requests_per_second: 5,
        burst_size: 5,
    };
    let config2 = RateLimitConfig {
        requests_per_second: 10,
        burst_size: 10,
    };

    limiter.register_key("key1", &config1);
    limiter.register_key("key2", &config2);

    // key1 should be limited at 5
    for _ in 0..5 {
        assert!(limiter.check_rate_limit("key1").is_ok());
    }
    assert!(limiter.check_rate_limit("key1").is_err());

    // key2 should still work and be limited at 10
    for _ in 0..10 {
        assert!(limiter.check_rate_limit("key2").is_ok());
    }
    assert!(limiter.check_rate_limit("key2").is_err());
}

#[test]
fn test_app_state_with_rate_limiter() {
    let credentials = vec![CredentialConfig {
        credential_key: "test-key-1".to_string(),
        name: "Test Key 1".to_string(),
        description: Some("Test key with rate limit".to_string()),
        rate_limit: Some(RateLimitConfig {
            requests_per_second: 10,
            burst_size: 10,
        }),
        enabled: true,
        allowed_models: vec![],
    }];

    let config = AppConfig {
        providers: vec![ProviderConfig {
            name: "TestProvider".to_string(),
            api_base: "http://localhost:8000".to_string(),
            api_key: "test_key".to_string(),
            weight: 1,
            model_mapping: HashMap::new(),
        }],
        server: ServerConfig::default(),
        verify_ssl: true,
        request_timeout_secs: 300,
        ttft_timeout_secs: None,
        credentials,
        provider_suffix: None,
    };

    let provider_service = ProviderService::new(config.clone());
    let rate_limiter = Arc::new(RateLimiter::new());

    // Register rate limits
    for key_config in &config.credentials {
        if key_config.enabled {
            if let Some(rate_limit) = &key_config.rate_limit {
                rate_limiter.register_key(&key_config.credential_key, rate_limit);
            }
        }
    }

    // Create shared HTTP client for tests
    let http_client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!config.verify_ssl)
        .timeout(std::time::Duration::from_secs(config.request_timeout_secs))
        .pool_max_idle_per_host(20)
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("Failed to build HTTP client");

    let _state = AppState::new(
        config,
        provider_service,
        rate_limiter.clone(),
        http_client,
        None,
    );

    // Test rate limiting through the rate_limiter
    for _ in 0..10 {
        assert!(rate_limiter.check_rate_limit("test-key-1").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("test-key-1").is_err());
}

#[test]
fn test_disabled_key_not_rate_limited() {
    let config = AppConfig {
        providers: vec![ProviderConfig {
            name: "TestProvider".to_string(),
            api_base: "http://localhost:8000".to_string(),
            api_key: "test_key".to_string(),
            weight: 1,
            model_mapping: HashMap::new(),
        }],
        server: ServerConfig::default(),
        verify_ssl: true,
        request_timeout_secs: 300,
        ttft_timeout_secs: None,
        credentials: vec![CredentialConfig {
            credential_key: "disabled-key".to_string(),
            name: "Disabled Key".to_string(),
            description: None,
            rate_limit: Some(RateLimitConfig {
                requests_per_second: 5,
                burst_size: 5,
            }),
            enabled: false,
            allowed_models: vec![],
        }],
        provider_suffix: None,
    };

    let rate_limiter = RateLimiter::new();

    // Don't register disabled keys
    for key_config in &config.credentials {
        if key_config.enabled {
            if let Some(rate_limit) = &key_config.rate_limit {
                rate_limiter.register_key(&key_config.credential_key, rate_limit);
            }
        }
    }

    // Disabled key should not be rate limited
    for _ in 0..100 {
        assert!(rate_limiter.check_rate_limit("disabled-key").is_ok());
    }
}

#[test]
fn test_key_without_rate_limit_config() {
    let config = AppConfig {
        providers: vec![ProviderConfig {
            name: "TestProvider".to_string(),
            api_base: "http://localhost:8000".to_string(),
            api_key: "test_key".to_string(),
            weight: 1,
            model_mapping: HashMap::new(),
        }],
        server: ServerConfig::default(),
        verify_ssl: true,
        request_timeout_secs: 300,
        ttft_timeout_secs: None,
        credentials: vec![CredentialConfig {
            credential_key: "unlimited-key".to_string(),
            name: "Unlimited Key".to_string(),
            description: Some("No rate limit".to_string()),
            rate_limit: None,
            enabled: true,
            allowed_models: vec![],
        }],
        provider_suffix: None,
    };

    let rate_limiter = RateLimiter::new();

    // Register only keys with rate limits
    for key_config in &config.credentials {
        if key_config.enabled {
            if let Some(rate_limit) = &key_config.rate_limit {
                rate_limiter.register_key(&key_config.credential_key, rate_limit);
            }
        }
    }

    // Key without rate_limit should not be limited
    for _ in 0..100 {
        assert!(rate_limiter.check_rate_limit("unlimited-key").is_ok());
    }
}

#[test]
fn test_mixed_rate_limits() {
    // Test configuration with mixed rate limits (some None, some set)
    let config = AppConfig {
        providers: vec![ProviderConfig {
            name: "TestProvider".to_string(),
            api_base: "http://localhost:8000".to_string(),
            api_key: "test_key".to_string(),
            weight: 1,
            model_mapping: HashMap::new(),
        }],
        server: ServerConfig::default(),
        verify_ssl: true,
        request_timeout_secs: 300,
        ttft_timeout_secs: None,
        provider_suffix: None,
        credentials: vec![
            CredentialConfig {
                credential_key: "limited-key-1".to_string(),
                name: "Limited Key 1".to_string(),
                description: Some("With rate limit".to_string()),
                rate_limit: Some(RateLimitConfig {
                    requests_per_second: 5,
                    burst_size: 5,
                }),
                enabled: true,
                allowed_models: vec![],
            },
            CredentialConfig {
                credential_key: "unlimited-key-1".to_string(),
                name: "Unlimited Key 1".to_string(),
                description: Some("No rate limit".to_string()),
                rate_limit: None,
                enabled: true,
                allowed_models: vec![],
            },
            CredentialConfig {
                credential_key: "limited-key-2".to_string(),
                name: "Limited Key 2".to_string(),
                description: Some("With different rate limit".to_string()),
                rate_limit: Some(RateLimitConfig {
                    requests_per_second: 10,
                    burst_size: 10,
                }),
                enabled: true,
                allowed_models: vec![],
            },
            CredentialConfig {
                credential_key: "unlimited-key-2".to_string(),
                name: "Unlimited Key 2".to_string(),
                description: Some("No rate limit".to_string()),
                rate_limit: None,
                enabled: true,
                allowed_models: vec![],
            },
        ],
    };

    let rate_limiter = RateLimiter::new();

    // Register only keys with rate limits
    for key_config in &config.credentials {
        if key_config.enabled {
            if let Some(rate_limit) = &key_config.rate_limit {
                rate_limiter.register_key(&key_config.credential_key, rate_limit);
            }
        }
    }

    // Unlimited keys should work without limits
    for _ in 0..100 {
        assert!(rate_limiter.check_rate_limit("unlimited-key-1").is_ok());
        assert!(rate_limiter.check_rate_limit("unlimited-key-2").is_ok());
    }

    // Limited keys should be rate limited
    for _ in 0..5 {
        assert!(rate_limiter.check_rate_limit("limited-key-1").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("limited-key-1").is_err());

    for _ in 0..10 {
        assert!(rate_limiter.check_rate_limit("limited-key-2").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("limited-key-2").is_err());

    // Unlimited keys should still work
    for _ in 0..50 {
        assert!(rate_limiter.check_rate_limit("unlimited-key-1").is_ok());
        assert!(rate_limiter.check_rate_limit("unlimited-key-2").is_ok());
    }
}
