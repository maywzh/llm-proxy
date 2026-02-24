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
            provider_type: "openai".to_string(),
            provider_params: HashMap::new(),
            lua_script: None,
        }],
        server: ServerConfig::default(),
        verify_ssl: true,
        request_timeout_secs: 300,
        ttft_timeout_secs: None,
        credentials,
        provider_suffix: None,
        min_tokens_limit: 100,
        max_tokens_limit: 4096,
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
            provider_type: "openai".to_string(),
            provider_params: HashMap::new(),
            lua_script: None,
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
        min_tokens_limit: 100,
        max_tokens_limit: 4096,
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
            provider_type: "openai".to_string(),
            provider_params: HashMap::new(),
            lua_script: None,
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
        min_tokens_limit: 100,
        max_tokens_limit: 4096,
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
            provider_type: "openai".to_string(),
            provider_params: HashMap::new(),
            lua_script: None,
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
        min_tokens_limit: 100,
        max_tokens_limit: 4096,
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

// ============================================================================
// Hot-reload (sync_from_credentials) integration tests
// ============================================================================

fn make_credential(key: &str, rps: Option<u32>, enabled: bool) -> CredentialConfig {
    CredentialConfig {
        credential_key: key.to_string(),
        name: format!("cred-{}", key),
        description: None,
        rate_limit: rps.map(|r| RateLimitConfig {
            requests_per_second: r,
            burst_size: r.saturating_mul(2),
        }),
        enabled,
        allowed_models: vec![],
    }
}

#[test]
fn test_sync_hot_reload_add_new_credential() {
    let rate_limiter = Arc::new(RateLimiter::new());

    // Initial: one credential with rate limit
    let creds_v1 = vec![make_credential("key-a", Some(5), true)];
    rate_limiter.sync_from_credentials(&creds_v1);

    // key-a should be limited at burst=10 (rps=5, burst=5*2)
    for _ in 0..10 {
        assert!(rate_limiter.check_rate_limit("key-a").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("key-a").is_err());

    // Hot reload: add key-b
    let creds_v2 = vec![
        make_credential("key-a", Some(5), true),
        make_credential("key-b", Some(3), true),
    ];
    rate_limiter.sync_from_credentials(&creds_v2);

    // key-b should now be limited at burst=6 (rps=3, burst=3*2)
    for _ in 0..6 {
        assert!(rate_limiter.check_rate_limit("key-b").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("key-b").is_err());
}

#[test]
fn test_sync_hot_reload_remove_credential() {
    let rate_limiter = Arc::new(RateLimiter::new());

    // Initial: two credentials
    let creds_v1 = vec![
        make_credential("key-a", Some(5), true),
        make_credential("key-b", Some(5), true),
    ];
    rate_limiter.sync_from_credentials(&creds_v1);

    // Both should be limited
    for _ in 0..10 {
        assert!(rate_limiter.check_rate_limit("key-a").is_ok());
        assert!(rate_limiter.check_rate_limit("key-b").is_ok());
    }

    // Hot reload: remove key-b
    let creds_v2 = vec![make_credential("key-a", Some(5), true)];
    rate_limiter.sync_from_credentials(&creds_v2);

    // key-b should now be unlimited (removed from limiter)
    for _ in 0..100 {
        assert!(rate_limiter.check_rate_limit("key-b").is_ok());
    }
}

#[test]
fn test_sync_hot_reload_update_rate_limit() {
    let rate_limiter = Arc::new(RateLimiter::new());

    // Initial: key-a with rps=3 (burst=6)
    let creds_v1 = vec![make_credential("key-a", Some(3), true)];
    rate_limiter.sync_from_credentials(&creds_v1);

    for _ in 0..6 {
        assert!(rate_limiter.check_rate_limit("key-a").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("key-a").is_err());

    // Hot reload: increase to rps=10 (burst=20)
    let creds_v2 = vec![make_credential("key-a", Some(10), true)];
    rate_limiter.sync_from_credentials(&creds_v2);

    // Should now allow 20 requests (new burst)
    for _ in 0..20 {
        assert!(rate_limiter.check_rate_limit("key-a").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("key-a").is_err());
}

#[test]
fn test_sync_hot_reload_disable_credential() {
    let rate_limiter = Arc::new(RateLimiter::new());

    // Initial: key-a enabled with rate limit
    let creds_v1 = vec![make_credential("key-a", Some(5), true)];
    rate_limiter.sync_from_credentials(&creds_v1);

    // Hot reload: disable key-a
    let creds_v2 = vec![make_credential("key-a", Some(5), false)];
    rate_limiter.sync_from_credentials(&creds_v2);

    // Disabled credential's rate limit should be removed
    for _ in 0..100 {
        assert!(rate_limiter.check_rate_limit("key-a").is_ok());
    }
}

#[test]
fn test_sync_hot_reload_remove_rate_limit_from_credential() {
    let rate_limiter = Arc::new(RateLimiter::new());

    // Initial: key-a with rate limit
    let creds_v1 = vec![make_credential("key-a", Some(5), true)];
    rate_limiter.sync_from_credentials(&creds_v1);

    // Hot reload: remove rate limit (set to None)
    let creds_v2 = vec![make_credential("key-a", None, true)];
    rate_limiter.sync_from_credentials(&creds_v2);

    // Should now be unlimited
    for _ in 0..100 {
        assert!(rate_limiter.check_rate_limit("key-a").is_ok());
    }
}

#[test]
fn test_sync_hot_reload_add_rate_limit_to_existing_credential() {
    let rate_limiter = Arc::new(RateLimiter::new());

    // Initial: key-a without rate limit
    let creds_v1 = vec![make_credential("key-a", None, true)];
    rate_limiter.sync_from_credentials(&creds_v1);

    // Should be unlimited
    for _ in 0..100 {
        assert!(rate_limiter.check_rate_limit("key-a").is_ok());
    }

    // Hot reload: add rate limit
    let creds_v2 = vec![make_credential("key-a", Some(3), true)];
    rate_limiter.sync_from_credentials(&creds_v2);

    // Should now be limited at burst=6
    for _ in 0..6 {
        assert!(rate_limiter.check_rate_limit("key-a").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("key-a").is_err());
}

#[test]
fn test_sync_via_app_state_rebuild() {
    // Verify that sync_from_credentials is accessible through the rate_limiter
    // in the same way AppState.rebuild_cache would use it
    let rate_limiter = Arc::new(RateLimiter::new());

    // Simulate initial registration (as main.rs does at startup)
    let initial_creds = vec![make_credential("hash-key-1", Some(5), true)];
    for cred in &initial_creds {
        if cred.enabled {
            if let Some(ref rl) = cred.rate_limit {
                rate_limiter.register_key(&cred.credential_key, rl);
            }
        }
    }

    // Verify initial limit works
    for _ in 0..10 {
        assert!(rate_limiter.check_rate_limit("hash-key-1").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("hash-key-1").is_err());

    // Simulate rebuild_cache calling sync_from_credentials with updated creds
    let updated_creds = vec![
        make_credential("hash-key-1", Some(20), true), // increased limit
        make_credential("hash-key-2", Some(3), true),  // new key
    ];
    rate_limiter.sync_from_credentials(&updated_creds);

    // hash-key-1: should have new burst=40
    for _ in 0..40 {
        assert!(rate_limiter.check_rate_limit("hash-key-1").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("hash-key-1").is_err());

    // hash-key-2: should be limited at burst=6
    for _ in 0..6 {
        assert!(rate_limiter.check_rate_limit("hash-key-2").is_ok());
    }
    assert!(rate_limiter.check_rate_limit("hash-key-2").is_err());
}

// ============================================================================
// Rate Limit Exempt Paths Tests
// ============================================================================

#[test]
fn test_rate_limit_exempt_paths_constant() {
    // Verify the exempt paths constant is defined correctly
    use llm_proxy_rust::api::auth::RATE_LIMIT_EXEMPT_PATHS;

    assert!(RATE_LIMIT_EXEMPT_PATHS.contains(&"/v1/messages/count_tokens"));
    assert!(RATE_LIMIT_EXEMPT_PATHS.contains(&"/v2/messages/count_tokens"));
    assert_eq!(RATE_LIMIT_EXEMPT_PATHS.len(), 2);
}
