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
                Err(_) => Err(AppError::RateLimitExceeded(
                    "Rate limit exceeded for key".to_string(),
                )),
            }
        } else {
            // No rate limit configured for this key
            Ok(())
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
}
