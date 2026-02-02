use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Entry representing a provider in cooldown state
#[derive(Debug, Clone)]
pub struct CooldownEntry {
    pub provider_name: String,
    pub status_code: u16,
    pub exception_type: String,
    pub started_at: Instant,
    pub cooldown_duration: Duration,
    pub error_message: Option<String>,
}

impl CooldownEntry {
    /// Check if this cooldown entry has expired
    #[inline]
    pub fn is_expired(&self) -> bool {
        self.started_at.elapsed() >= self.cooldown_duration
    }

    /// Get remaining cooldown time in seconds
    pub fn remaining_secs(&self) -> u64 {
        let elapsed = self.started_at.elapsed();
        if elapsed >= self.cooldown_duration {
            0
        } else {
            (self.cooldown_duration - elapsed).as_secs()
        }
    }
}

/// Configuration for cooldown behavior
#[derive(Debug, Clone)]
pub struct CooldownConfig {
    pub enabled: bool,
    pub default_cooldown_secs: u64,
    pub max_cooldown_secs: u64,
    /// Status codes that trigger cooldown: only 429 (rate limit) and 5xx (server errors)
    pub cooldown_status_codes: Vec<u16>,
    /// Per-status-code cooldown durations
    pub cooldown_durations: HashMap<u16, u64>,
}

impl Default for CooldownConfig {
    fn default() -> Self {
        let mut cooldown_durations = HashMap::new();
        cooldown_durations.insert(429, 60); // Rate limit - longer cooldown
        cooldown_durations.insert(500, 30);
        cooldown_durations.insert(501, 30);
        cooldown_durations.insert(502, 30);
        cooldown_durations.insert(503, 60); // Service unavailable - longer
        cooldown_durations.insert(504, 30);

        Self {
            enabled: std::env::var("COOLDOWN_ENABLED")
                .map(|v| v.to_lowercase() == "true" || v == "1")
                .unwrap_or(true),
            default_cooldown_secs: std::env::var("COOLDOWN_DEFAULT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            max_cooldown_secs: std::env::var("COOLDOWN_MAX_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300),
            cooldown_status_codes: vec![429, 500, 501, 502, 503, 504],
            cooldown_durations,
        }
    }
}

impl CooldownConfig {
    /// Check if a status code should trigger cooldown
    #[inline]
    pub fn should_trigger_cooldown(&self, status_code: u16) -> bool {
        if !self.enabled {
            return false;
        }
        // Only 429 and 5xx trigger cooldown
        status_code == 429 || (500..600).contains(&status_code)
    }

    /// Get cooldown duration for a specific status code
    pub fn get_cooldown_duration(&self, status_code: u16) -> Duration {
        let secs = self
            .cooldown_durations
            .get(&status_code)
            .copied()
            .unwrap_or(self.default_cooldown_secs)
            .min(self.max_cooldown_secs);
        Duration::from_secs(secs)
    }

    /// Get exception type for a status code
    pub fn get_exception_type(status_code: u16) -> &'static str {
        if status_code == 429 {
            "rate_limit"
        } else {
            "server_error"
        }
    }
}

/// Thread-safe cooldown service using DashMap for lock-free concurrent access
#[derive(Debug, Clone)]
pub struct CooldownService {
    cooldowns: Arc<DashMap<String, CooldownEntry>>,
    config: CooldownConfig,
}

impl CooldownService {
    pub fn new(config: CooldownConfig) -> Self {
        Self {
            cooldowns: Arc::new(DashMap::new()),
            config,
        }
    }

    /// Check if cooldown is enabled
    #[inline]
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the configuration
    pub fn config(&self) -> &CooldownConfig {
        &self.config
    }

    /// Add a provider to cooldown
    pub fn add_cooldown(
        &self,
        provider_name: &str,
        status_code: u16,
        error_message: Option<String>,
    ) {
        if !self.config.should_trigger_cooldown(status_code) {
            return;
        }

        let entry = CooldownEntry {
            provider_name: provider_name.to_string(),
            status_code,
            exception_type: CooldownConfig::get_exception_type(status_code).to_string(),
            started_at: Instant::now(),
            cooldown_duration: self.config.get_cooldown_duration(status_code),
            error_message,
        };

        tracing::warn!(
            provider = %provider_name,
            status_code = %status_code,
            cooldown_secs = %entry.cooldown_duration.as_secs(),
            "Provider added to cooldown"
        );

        self.cooldowns.insert(provider_name.to_string(), entry);
    }

    /// Check if a provider is currently in cooldown
    #[inline]
    pub fn is_in_cooldown(&self, provider_name: &str) -> bool {
        if !self.config.enabled {
            return false;
        }

        if let Some(entry) = self.cooldowns.get(provider_name) {
            if entry.is_expired() {
                drop(entry); // Release the reference before removing
                self.cooldowns.remove(provider_name);
                false
            } else {
                true
            }
        } else {
            false
        }
    }

    /// Filter available providers, removing those in cooldown
    /// Returns (available_providers, cooled_down_count)
    ///
    /// Performance: Single iteration with lazy expiration cleanup
    pub fn filter_available_providers<'a>(
        &self,
        providers: &'a [String],
    ) -> (Vec<&'a String>, usize) {
        if !self.config.enabled {
            return (providers.iter().collect(), 0);
        }

        let mut available = Vec::with_capacity(providers.len());
        let mut cooled_down = 0;
        let mut expired_keys = Vec::new();

        for provider in providers {
            if let Some(entry) = self.cooldowns.get(provider) {
                if entry.is_expired() {
                    expired_keys.push(provider.clone());
                    available.push(provider);
                } else {
                    cooled_down += 1;
                }
            } else {
                available.push(provider);
            }
        }

        // Cleanup expired entries
        for key in expired_keys {
            self.cooldowns.remove(&key);
        }

        (available, cooled_down)
    }

    /// Get cooldown entry for a specific provider
    pub fn get_cooldown(&self, provider_name: &str) -> Option<CooldownEntry> {
        self.cooldowns.get(provider_name).map(|r| r.clone())
    }

    /// Remove a provider from cooldown manually
    pub fn remove_cooldown(&self, provider_name: &str) -> bool {
        self.cooldowns.remove(provider_name).is_some()
    }

    /// Get all current cooldowns (excluding expired)
    pub fn get_all_cooldowns(&self) -> Vec<CooldownEntry> {
        let mut result = Vec::new();
        let mut expired_keys = Vec::new();

        for entry in self.cooldowns.iter() {
            if entry.is_expired() {
                expired_keys.push(entry.key().clone());
            } else {
                result.push(entry.clone());
            }
        }

        // Cleanup expired entries
        for key in expired_keys {
            self.cooldowns.remove(&key);
        }

        result
    }

    /// Clear all cooldowns
    pub fn clear_all(&self) {
        self.cooldowns.clear();
    }

    /// Get count of active cooldowns
    pub fn active_count(&self) -> usize {
        self.cleanup_expired();
        self.cooldowns.len()
    }

    /// Cleanup expired entries
    fn cleanup_expired(&self) {
        self.cooldowns.retain(|_, entry| !entry.is_expired());
    }
}

impl Default for CooldownService {
    fn default() -> Self {
        Self::new(CooldownConfig::default())
    }
}

// Global singleton using once_cell
use once_cell::sync::Lazy;

static COOLDOWN_SERVICE: Lazy<CooldownService> = Lazy::new(CooldownService::default);

/// Get reference to the global cooldown service
/// Performance: No clone, no lock - just a static reference
#[inline]
pub fn get_cooldown_service() -> &'static CooldownService {
    &COOLDOWN_SERVICE
}

/// Reset the global cooldown service (for testing)
/// Note: This clears the DashMap but doesn't recreate the service
#[cfg(test)]
pub fn reset_cooldown_service() {
    COOLDOWN_SERVICE.clear_all();
}

/// Trigger cooldown if the status code warrants it
/// This is the main entry point for triggering cooldowns from error handlers
#[inline]
pub fn trigger_cooldown_if_needed(
    provider_name: &str,
    status_code: u16,
    error_message: Option<String>,
) {
    get_cooldown_service().add_cooldown(provider_name, status_code, error_message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_cooldown_config_default() {
        let config = CooldownConfig::default();
        assert!(config.enabled);
        assert_eq!(config.default_cooldown_secs, 30);
        assert_eq!(config.max_cooldown_secs, 300);
    }

    #[test]
    fn test_should_trigger_cooldown() {
        let config = CooldownConfig::default();

        // Should trigger
        assert!(config.should_trigger_cooldown(429));
        assert!(config.should_trigger_cooldown(500));
        assert!(config.should_trigger_cooldown(502));
        assert!(config.should_trigger_cooldown(503));

        // Should NOT trigger (4xx client errors)
        assert!(!config.should_trigger_cooldown(400));
        assert!(!config.should_trigger_cooldown(401));
        assert!(!config.should_trigger_cooldown(403));
        assert!(!config.should_trigger_cooldown(404));
        assert!(!config.should_trigger_cooldown(422));
    }

    #[test]
    fn test_add_and_check_cooldown() {
        let service = CooldownService::default();

        assert!(!service.is_in_cooldown("test-provider"));

        service.add_cooldown("test-provider", 429, Some("Rate limited".to_string()));

        assert!(service.is_in_cooldown("test-provider"));
    }

    #[test]
    fn test_cooldown_expiration() {
        let mut config = CooldownConfig::default();
        config.cooldown_durations.insert(429, 1); // 1 second cooldown
        let service = CooldownService::new(config);

        service.add_cooldown("test-provider", 429, None);
        assert!(service.is_in_cooldown("test-provider"));

        sleep(Duration::from_millis(1100));
        assert!(!service.is_in_cooldown("test-provider"));
    }

    #[test]
    fn test_filter_available_providers() {
        let service = CooldownService::default();

        let providers = vec![
            "provider-a".to_string(),
            "provider-b".to_string(),
            "provider-c".to_string(),
        ];

        service.add_cooldown("provider-b", 500, None);

        let (available, cooled_down) = service.filter_available_providers(&providers);

        assert_eq!(available.len(), 2);
        assert_eq!(cooled_down, 1);
        assert!(available.contains(&&"provider-a".to_string()));
        assert!(available.contains(&&"provider-c".to_string()));
        assert!(!available.contains(&&"provider-b".to_string()));
    }

    #[test]
    fn test_remove_cooldown() {
        let service = CooldownService::default();

        service.add_cooldown("test-provider", 429, None);
        assert!(service.is_in_cooldown("test-provider"));

        assert!(service.remove_cooldown("test-provider"));
        assert!(!service.is_in_cooldown("test-provider"));

        assert!(!service.remove_cooldown("non-existent"));
    }

    #[test]
    fn test_clear_all() {
        let service = CooldownService::default();

        service.add_cooldown("provider-a", 429, None);
        service.add_cooldown("provider-b", 500, None);

        assert_eq!(service.active_count(), 2);

        service.clear_all();

        assert_eq!(service.active_count(), 0);
    }

    #[test]
    fn test_4xx_does_not_trigger_cooldown() {
        let service = CooldownService::default();

        // These should NOT trigger cooldown
        service.add_cooldown("provider-400", 400, None);
        service.add_cooldown("provider-401", 401, None);
        service.add_cooldown("provider-403", 403, None);
        service.add_cooldown("provider-404", 404, None);
        service.add_cooldown("provider-422", 422, None);

        assert!(!service.is_in_cooldown("provider-400"));
        assert!(!service.is_in_cooldown("provider-401"));
        assert!(!service.is_in_cooldown("provider-403"));
        assert!(!service.is_in_cooldown("provider-404"));
        assert!(!service.is_in_cooldown("provider-422"));
    }

    #[test]
    fn test_get_all_cooldowns() {
        let service = CooldownService::default();

        service.add_cooldown("provider-a", 429, Some("Rate limit".to_string()));
        service.add_cooldown("provider-b", 500, Some("Server error".to_string()));

        let cooldowns = service.get_all_cooldowns();

        assert_eq!(cooldowns.len(), 2);
    }

    #[test]
    fn test_exception_type() {
        assert_eq!(CooldownConfig::get_exception_type(429), "rate_limit");
        assert_eq!(CooldownConfig::get_exception_type(500), "server_error");
        assert_eq!(CooldownConfig::get_exception_type(503), "server_error");
    }

    #[test]
    fn test_remaining_secs() {
        let entry = CooldownEntry {
            provider_name: "test".to_string(),
            status_code: 429,
            exception_type: "rate_limit".to_string(),
            started_at: Instant::now(),
            cooldown_duration: Duration::from_secs(60),
            error_message: None,
        };

        assert!(entry.remaining_secs() > 55);
        assert!(entry.remaining_secs() <= 60);
    }
}
