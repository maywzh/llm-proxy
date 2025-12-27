//! Configuration management for the LLM proxy server.
//!
//! This module handles loading and parsing configuration from YAML files,
//! with support for environment variable expansion.

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

/// Main application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// List of LLM provider configurations
    pub providers: Vec<ProviderConfig>,

    /// Server configuration (host, port)
    #[serde(default)]
    pub server: ServerConfig,

    /// Whether to verify SSL certificates for upstream requests
    #[serde(default = "default_verify_ssl")]
    pub verify_ssl: bool,

    /// Request timeout in seconds for upstream providers
    #[serde(default = "default_request_timeout")]
    pub request_timeout_secs: u64,

    /// List of master keys with optional rate limiting
    #[serde(default)]
    pub master_keys: Vec<MasterKeyConfig>,
}

/// Configuration for a master API key with optional rate limiting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterKeyConfig {
    /// The actual API key
    pub key: String,

    /// Human-readable name for the key
    pub name: String,

    /// Optional description
    #[serde(default)]
    pub description: Option<String>,

    /// Optional rate limiting configuration
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,

    /// Whether this key is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

/// Rate limiting configuration for a master key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per second
    pub requests_per_second: u32,

    /// Maximum burst size (allows temporary spikes)
    #[serde(default = "default_burst")]
    pub burst_size: u32,
}

fn default_enabled() -> bool {
    true
}

fn default_burst() -> u32 {
    10
}

/// Configuration for a single LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider name (for logging and metrics)
    pub name: String,

    /// Base URL for the provider's API
    pub api_base: String,

    /// API key for authentication
    pub api_key: String,

    /// Weight for round-robin selection (higher = more likely to be selected)
    #[serde(default = "default_weight")]
    pub weight: u32,

    /// Model name mappings (client model -> provider model)
    #[serde(default)]
    pub model_mapping: HashMap<String, String>,
}

/// Server-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to
    #[serde(default = "default_host")]
    pub host: String,

    /// Port to bind to
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    18000
}

fn default_weight() -> u32 {
    1
}

fn default_verify_ssl() -> bool {
    true
}

fn default_request_timeout() -> u64 {
    300
}

impl AppConfig {
    /// Load configuration from a YAML file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the configuration file
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use llm_proxy_rust::core::config::AppConfig;
    ///
    /// let config = AppConfig::load("config.yaml").expect("Failed to load config");
    /// ```
    pub fn load(path: &str) -> Result<Self> {
        // Load .env file if it exists (skip in tests to avoid interference)
        #[cfg(not(test))]
        dotenv::dotenv().ok();

        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path))?;

        // Expand environment variables
        let expanded = expand_env_vars(&content);

        let mut config: AppConfig = serde_yaml::from_str(&expanded)
            .with_context(|| format!("Failed to parse config file: {}", path))?;

        // Override with environment variables (env vars take precedence)

        // Server host override
        if let Ok(host) = std::env::var("HOST") {
            config.server.host = host;
        }

        // Server port override
        if let Ok(port_str) = std::env::var("PORT") {
            if let Ok(port) = port_str.parse::<u16>() {
                config.server.port = port;
            }
        }

        // SSL verification override
        if let Ok(verify_ssl_str) = std::env::var("VERIFY_SSL") {
            config.verify_ssl = str_to_bool(&verify_ssl_str);
        }

        // Request timeout override
        if let Ok(timeout_str) = std::env::var("REQUEST_TIMEOUT_SECS") {
            if let Ok(timeout) = timeout_str.parse::<u64>() {
                config.request_timeout_secs = timeout;
            }
        }

        Ok(config)
    }
}

/// Expand environment variables in configuration content.
///
/// Supports patterns: ${VAR}, ${VAR:-default}, ${VAR:default}
///
/// This function also handles numeric values by removing quotes around
/// expanded values that look like numbers or booleans.
fn expand_env_vars(content: &str) -> String {
    let re = Regex::new(r#"["']?\$\{([^}:]+)(?::?-?([^}]*))?\}["']?"#).unwrap();

    re.replace_all(content, |caps: &regex::Captures| {
        let var_name = &caps[1];
        let default_value = caps.get(2).map(|m| m.as_str()).unwrap_or("");

        let expanded = std::env::var(var_name).unwrap_or_else(|_| default_value.to_string());

        // If the expanded value looks like a number or boolean, don't quote it
        // This allows YAML to parse it as the correct type
        if is_numeric_or_bool(&expanded) {
            expanded
        } else {
            // For string values, preserve them as-is
            expanded
        }
    })
    .to_string()
}

/// Check if a string represents a numeric value or boolean.
fn is_numeric_or_bool(s: &str) -> bool {
    // Check if it's a number (integer or float)
    if s.parse::<f64>().is_ok() {
        return true;
    }

    // Check if it's a boolean
    matches!(s.to_lowercase().as_str(), "true" | "false")
}

/// Convert string to boolean.
///
/// Accepts: "true", "1", "yes", "on" (case-insensitive)
fn str_to_bool(value: &str) -> bool {
    matches!(value.to_lowercase().as_str(), "true" | "1" | "yes" | "on")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_is_numeric_or_bool() {
        assert!(is_numeric_or_bool("123"));
        assert!(is_numeric_or_bool("18000"));
        assert!(is_numeric_or_bool("3.14"));
        assert!(is_numeric_or_bool("0"));
        assert!(is_numeric_or_bool("true"));
        assert!(is_numeric_or_bool("false"));
        assert!(is_numeric_or_bool("True"));
        assert!(is_numeric_or_bool("FALSE"));
        assert!(!is_numeric_or_bool("test_value"));
        assert!(!is_numeric_or_bool("http://example.com"));
        assert!(!is_numeric_or_bool(""));
    }

    #[test]
    fn test_expand_env_vars() {
        unsafe {
            std::env::set_var("TEST_VAR", "test_value");
        }
        let input = "api_key: ${TEST_VAR}";
        let output = expand_env_vars(input);
        assert_eq!(output, "api_key: test_value");
        unsafe {
            std::env::remove_var("TEST_VAR");
        }
    }

    #[test]
    fn test_expand_env_vars_numeric() {
        unsafe {
            std::env::set_var("TEST_NUMERIC_PORT", "18000");
        }
        let input = "port: ${TEST_NUMERIC_PORT}";
        let output = expand_env_vars(input);
        assert_eq!(output, "port: 18000");
        unsafe {
            std::env::remove_var("TEST_NUMERIC_PORT");
        }
    }

    #[test]
    fn test_expand_env_vars_boolean() {
        unsafe {
            std::env::set_var("TEST_BOOL", "true");
        }
        let input = "verify_ssl: ${TEST_BOOL}";
        let output = expand_env_vars(input);
        assert_eq!(output, "verify_ssl: true");
        unsafe {
            std::env::remove_var("TEST_BOOL");
        }
    }

    #[test]
    fn test_expand_env_vars_with_default() {
        unsafe {
            std::env::remove_var("MISSING_VAR");
        }
        let input = "api_key: ${MISSING_VAR:-default_value}";
        let output = expand_env_vars(input);
        assert_eq!(output, "api_key: default_value");
    }

    #[test]
    fn test_expand_env_vars_with_colon_default() {
        unsafe {
            std::env::remove_var("MISSING_VAR2");
        }
        let input = "api_key: ${MISSING_VAR2:default_value}";
        let output = expand_env_vars(input);
        assert_eq!(output, "api_key: default_value");
    }

    #[test]
    fn test_expand_env_vars_multiple() {
        unsafe {
            std::env::set_var("VAR1", "value1");
            std::env::set_var("VAR2", "value2");
        }
        let input = "key1: ${VAR1}, key2: ${VAR2}";
        let output = expand_env_vars(input);
        assert_eq!(output, "key1: value1, key2: value2");
        unsafe {
            std::env::remove_var("VAR1");
            std::env::remove_var("VAR2");
        }
    }

    #[test]
    fn test_expand_env_vars_empty_default() {
        unsafe {
            std::env::remove_var("EMPTY_VAR");
        }
        let input = "api_key: ${EMPTY_VAR:-}";
        let output = expand_env_vars(input);
        assert_eq!(output, "api_key: ");
    }

    #[test]
    fn test_str_to_bool() {
        assert!(str_to_bool("true"));
        assert!(str_to_bool("True"));
        assert!(str_to_bool("TRUE"));
        assert!(str_to_bool("1"));
        assert!(str_to_bool("yes"));
        assert!(str_to_bool("Yes"));
        assert!(str_to_bool("YES"));
        assert!(str_to_bool("on"));
        assert!(str_to_bool("On"));
        assert!(str_to_bool("ON"));
        assert!(!str_to_bool("false"));
        assert!(!str_to_bool("False"));
        assert!(!str_to_bool("0"));
        assert!(!str_to_bool("no"));
        assert!(!str_to_bool("off"));
        assert!(!str_to_bool(""));
        assert!(!str_to_bool("invalid"));
    }

    #[test]
    fn test_default_values() {
        let config = ServerConfig::default();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 18000);
    }

    #[test]
    fn test_default_weight() {
        assert_eq!(default_weight(), 1);
    }

    #[test]
    fn test_default_verify_ssl() {
        assert!(default_verify_ssl());
    }

    #[test]
    #[serial]
    fn test_load_config_from_file() {
        // Clear environment variables that might interfere
        unsafe {
            std::env::remove_var("HOST");
            std::env::remove_var("PORT");
            std::env::remove_var("VERIFY_SSL");
        }

        let mut temp_file = NamedTempFile::new().unwrap();
        let config_content = r#"
providers:
  - name: TestProvider
    api_base: http://localhost:8000
    api_key: test_key
    weight: 2
    model_mapping:
      gpt-4: test-model-4

server:
  host: 127.0.0.1
  port: 8080

verify_ssl: false
"#;
        temp_file.write_all(config_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = AppConfig::load(temp_file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].name, "TestProvider");
        assert_eq!(config.providers[0].api_base, "http://localhost:8000");
        assert_eq!(config.providers[0].api_key, "test_key");
        assert_eq!(config.providers[0].weight, 2);
        assert_eq!(
            config.providers[0].model_mapping.get("gpt-4").unwrap(),
            "test-model-4"
        );

        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 8080);

        assert!(!config.verify_ssl);
    }

    #[test]
    fn test_load_config_with_env_vars() {
        unsafe {
            std::env::set_var("TEST_API_KEY", "env_api_key");
            std::env::set_var("TEST_PORT", "9000");
        }

        let mut temp_file = NamedTempFile::new().unwrap();
        let config_content = r#"
providers:
  - name: TestProvider
    api_base: http://localhost:8000
    api_key: ${TEST_API_KEY}
    weight: 1

server:
  host: 0.0.0.0
  port: ${TEST_PORT}

verify_ssl: true
"#;
        temp_file.write_all(config_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = AppConfig::load(temp_file.path().to_str().unwrap()).unwrap();

        assert_eq!(config.providers[0].api_key, "env_api_key");

        unsafe {
            std::env::remove_var("TEST_API_KEY");
            std::env::remove_var("TEST_PORT");
        }
    }

    #[test]
    fn test_load_config_missing_file() {
        let result = AppConfig::load("nonexistent_file.yaml");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_config_invalid_yaml() {
        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file.write_all(b"invalid: yaml: content:").unwrap();
        temp_file.flush().unwrap();

        let result = AppConfig::load(temp_file.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn test_env_var_overrides() {
        // Test that environment variables override config file values
        unsafe {
            std::env::set_var("HOST", "192.168.1.1");
            std::env::set_var("PORT", "9999");
            std::env::set_var("VERIFY_SSL", "false");
        }

        let mut temp_file = NamedTempFile::new().unwrap();
        let config_content = r#"
providers:
  - name: TestProvider
    api_base: http://localhost:8000
    api_key: test_key

server:
  host: 127.0.0.1
  port: 8080

verify_ssl: true
"#;
        temp_file.write_all(config_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = AppConfig::load(temp_file.path().to_str().unwrap()).unwrap();

        // Environment variables should override config file
        assert_eq!(config.server.host, "192.168.1.1");
        assert_eq!(config.server.port, 9999);
        assert!(!config.verify_ssl);

        unsafe {
            std::env::remove_var("HOST");
            std::env::remove_var("PORT");
            std::env::remove_var("VERIFY_SSL");
        }
    }

    #[test]
    fn test_config_with_default_weight() {
        let mut temp_file = NamedTempFile::new().unwrap();
        let config_content = r#"
providers:
  - name: TestProvider
    api_base: http://localhost:8000
    api_key: test_key

verify_ssl: true
"#;
        temp_file.write_all(config_content.as_bytes()).unwrap();
        temp_file.flush().unwrap();

        let config = AppConfig::load(temp_file.path().to_str().unwrap()).unwrap();
        assert_eq!(config.providers[0].weight, 1);
    }

    #[test]
    fn test_config_serialization() {
        let config = AppConfig {
            providers: vec![ProviderConfig {
                name: "Test".to_string(),
                api_base: "http://test".to_string(),
                api_key: "key".to_string(),
                weight: 1,
                model_mapping: HashMap::new(),
            }],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            master_keys: vec![],
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("Test"));
        assert!(yaml.contains("http://test"));
    }
}

