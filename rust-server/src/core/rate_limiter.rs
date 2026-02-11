//! Rate limiting service for master API keys.
//!
//! This module provides per-key rate limiting using the token bucket algorithm
//! via the governor crate. Each master key can have independent rate limits.

use dashmap::DashMap;
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter as GovernorRateLimiter,
};
use nonzero_ext::nonzero;
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::core::config::RateLimitConfig;
use crate::core::error::AppError;

/// Type alias for the rate limiter instance
type RateLimiterInstance = Arc<GovernorRateLimiter<NotKeyed, InMemoryState, DefaultClock>>;

/// Rate limiter for managing per-key request limits.
pub struct RateLimiter {
    /// Map of key -> rate limiter instance
    limiters: Arc<DashMap<String, RateLimiterInstance>>,
}

impl RateLimiter {
    /// Create a new rate limiter instance.
    pub fn new() -> Self {
        Self {
            limiters: Arc::new(DashMap::new()),
        }
    }

    /// Register a new key with rate limiting.
    ///
    /// # Arguments
    ///
    /// * `key` - The API key to register
    /// * `config` - Rate limit configuration
    pub fn register_key(&self, key: &str, config: &RateLimitConfig) {
        let quota = Quota::per_second(
            NonZeroU32::new(config.requests_per_second).unwrap_or(nonzero!(1u32)),
        )
        .allow_burst(NonZeroU32::new(config.burst_size).unwrap_or(nonzero!(10u32)));

        let limiter = Arc::new(GovernorRateLimiter::direct(quota));
        self.limiters.insert(key.to_string(), limiter);
    }

    /// Check if a request is allowed for the given key.
    ///
    /// # Arguments
    ///
    /// * `key` - The API key to check
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the request is allowed
    /// * `Err(AppError::RateLimitExceeded)` if the rate limit is exceeded
    pub fn check_rate_limit(&self, key: &str) -> Result<(), AppError> {
        if let Some(limiter) = self.limiters.get(key) {
            match limiter.check() {
                Ok(_) => Ok(()),
                Err(_) => {
                    tracing::warn!(
                        credential_key_prefix = &key[..key.len().min(8)],
                        "Rate limit exceeded"
                    );
                    Err(AppError::RateLimitExceeded(
                        "Rate limit exceeded for key".to_string(),
                    ))
                }
            }
        } else {
            // No rate limit configured for this key
            Ok(())
        }
    }

    /// Synchronize rate limits from a list of credential configs.
    ///
    /// This performs a full diff: adds new keys, updates changed limits,
    /// and removes keys that are no longer present.
    pub fn sync_from_credentials(&self, credentials: &[crate::core::config::CredentialConfig]) {
        use std::collections::HashSet;

        let mut desired_keys: HashSet<String> = HashSet::new();

        for cred in credentials {
            if !cred.enabled {
                continue;
            }
            if let Some(ref rl) = cred.rate_limit {
                desired_keys.insert(cred.credential_key.clone());
                // Re-register unconditionally — governor limiter has no way to
                // inspect its current quota, so we just replace it.
                self.register_key(&cred.credential_key, rl);
            }
        }

        // Remove keys that are no longer in the desired set
        let stale_keys: Vec<String> = self
            .limiters
            .iter()
            .filter(|entry| !desired_keys.contains(entry.key().as_str()))
            .map(|entry| entry.key().clone())
            .collect();

        for key in &stale_keys {
            self.limiters.remove(key);
        }

        if !stale_keys.is_empty() {
            tracing::info!(
                removed_count = stale_keys.len(),
                "Removed stale rate limit entries"
            );
        }
    }

    /// Remove a key from rate limiting.
    ///
    /// # Arguments
    ///
    /// * `key` - The API key to remove
    pub fn remove_key(&self, key: &str) {
        self.limiters.remove(key);
    }

    /// Clear all rate limiters.
    pub fn clear(&self) {
        self.limiters.clear();
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::CredentialConfig;

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
    fn test_rate_limiter_remove_key() {
        let limiter = RateLimiter::new();
        let config = RateLimitConfig {
            requests_per_second: 5,
            burst_size: 5,
        };

        limiter.register_key("test-key", &config);

        // Use up tokens
        for _ in 0..5 {
            assert!(limiter.check_rate_limit("test-key").is_ok());
        }
        assert!(limiter.check_rate_limit("test-key").is_err());

        // Remove key
        limiter.remove_key("test-key");

        // Should now be unlimited
        assert!(limiter.check_rate_limit("test-key").is_ok());
    }

    #[test]
    fn test_rate_limiter_clear() {
        let limiter = RateLimiter::new();
        let config = RateLimitConfig {
            requests_per_second: 5,
            burst_size: 5,
        };

        limiter.register_key("key1", &config);
        limiter.register_key("key2", &config);

        limiter.clear();

        // Both keys should now be unlimited
        for _ in 0..100 {
            assert!(limiter.check_rate_limit("key1").is_ok());
            assert!(limiter.check_rate_limit("key2").is_ok());
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

    fn make_credential(key: &str, rps: Option<u32>, enabled: bool) -> CredentialConfig {
        CredentialConfig {
            credential_key: key.to_string(),
            name: format!("cred-{}", key),
            description: None,
            rate_limit: rps.map(|r| RateLimitConfig {
                requests_per_second: r,
                burst_size: r,
            }),
            enabled,
            allowed_models: vec![],
        }
    }

    #[test]
    fn test_sync_adds_new_keys() {
        let limiter = RateLimiter::new();
        let creds = vec![
            make_credential("key-a", Some(5), true),
            make_credential("key-b", Some(10), true),
        ];

        limiter.sync_from_credentials(&creds);

        // key-a: burst=5
        for _ in 0..5 {
            assert!(limiter.check_rate_limit("key-a").is_ok());
        }
        assert!(limiter.check_rate_limit("key-a").is_err());

        // key-b: burst=10
        for _ in 0..10 {
            assert!(limiter.check_rate_limit("key-b").is_ok());
        }
        assert!(limiter.check_rate_limit("key-b").is_err());
    }

    #[test]
    fn test_sync_removes_stale_keys() {
        let limiter = RateLimiter::new();

        // Initial: register key-a and key-b
        limiter.register_key(
            "key-a",
            &RateLimitConfig {
                requests_per_second: 5,
                burst_size: 5,
            },
        );
        limiter.register_key(
            "key-b",
            &RateLimitConfig {
                requests_per_second: 5,
                burst_size: 5,
            },
        );

        // Sync with only key-a — key-b should be removed
        let creds = vec![make_credential("key-a", Some(5), true)];
        limiter.sync_from_credentials(&creds);

        // key-b should now be unlimited (removed from limiter)
        for _ in 0..100 {
            assert!(limiter.check_rate_limit("key-b").is_ok());
        }

        // key-a should still be limited
        for _ in 0..5 {
            assert!(limiter.check_rate_limit("key-a").is_ok());
        }
        assert!(limiter.check_rate_limit("key-a").is_err());
    }

    #[test]
    fn test_sync_updates_existing_key_limit() {
        let limiter = RateLimiter::new();

        // Register with burst=3
        limiter.register_key(
            "key-a",
            &RateLimitConfig {
                requests_per_second: 3,
                burst_size: 3,
            },
        );

        // Sync with burst=10 — should reset the limiter
        let creds = vec![make_credential("key-a", Some(10), true)];
        limiter.sync_from_credentials(&creds);

        // Should now allow 10 requests
        for _ in 0..10 {
            assert!(limiter.check_rate_limit("key-a").is_ok());
        }
        assert!(limiter.check_rate_limit("key-a").is_err());
    }

    #[test]
    fn test_sync_skips_disabled_credentials() {
        let limiter = RateLimiter::new();

        let creds = vec![
            make_credential("enabled-key", Some(5), true),
            make_credential("disabled-key", Some(5), false),
        ];

        limiter.sync_from_credentials(&creds);

        // enabled-key should be limited
        for _ in 0..5 {
            assert!(limiter.check_rate_limit("enabled-key").is_ok());
        }
        assert!(limiter.check_rate_limit("enabled-key").is_err());

        // disabled-key should be unlimited (not registered)
        for _ in 0..100 {
            assert!(limiter.check_rate_limit("disabled-key").is_ok());
        }
    }

    #[test]
    fn test_sync_skips_credentials_without_rate_limit() {
        let limiter = RateLimiter::new();

        let creds = vec![
            make_credential("limited-key", Some(5), true),
            make_credential("unlimited-key", None, true),
        ];

        limiter.sync_from_credentials(&creds);

        // limited-key should be limited
        for _ in 0..5 {
            assert!(limiter.check_rate_limit("limited-key").is_ok());
        }
        assert!(limiter.check_rate_limit("limited-key").is_err());

        // unlimited-key should be unlimited
        for _ in 0..100 {
            assert!(limiter.check_rate_limit("unlimited-key").is_ok());
        }
    }

    #[test]
    fn test_sync_removes_key_when_rate_limit_cleared() {
        let limiter = RateLimiter::new();

        // Register with rate limit
        limiter.register_key(
            "key-a",
            &RateLimitConfig {
                requests_per_second: 5,
                burst_size: 5,
            },
        );

        // Sync with same key but no rate limit — should remove from limiter
        let creds = vec![make_credential("key-a", None, true)];
        limiter.sync_from_credentials(&creds);

        // key-a should now be unlimited
        for _ in 0..100 {
            assert!(limiter.check_rate_limit("key-a").is_ok());
        }
    }

    #[test]
    fn test_sync_empty_credentials_clears_all() {
        let limiter = RateLimiter::new();

        limiter.register_key(
            "key-a",
            &RateLimitConfig {
                requests_per_second: 5,
                burst_size: 5,
            },
        );
        limiter.register_key(
            "key-b",
            &RateLimitConfig {
                requests_per_second: 5,
                burst_size: 5,
            },
        );

        limiter.sync_from_credentials(&[]);

        // All keys should be unlimited
        for _ in 0..100 {
            assert!(limiter.check_rate_limit("key-a").is_ok());
            assert!(limiter.check_rate_limit("key-b").is_ok());
        }
    }
}
