//! Configuration management for the LLM proxy server.
//!
//! This module handles configuration loading from environment variables
//! and provides data structures for runtime configuration.
//! Dynamic configuration is loaded from the database.

use anyhow::Result;
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

/// Extended model mapping entry with metadata.
///
/// Supports rich model information including token limits, costs, and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize, Default, ToSchema)]
pub struct ModelMappingEntry {
    /// The actual model name to use for the provider
    pub mapped_model: String,

    /// Maximum context window (input + output)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// Maximum input tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u32>,

    /// Maximum output tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,

    /// Cost per 1K input tokens in USD
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_cost_per_1k_tokens: Option<f64>,

    /// Cost per 1K output tokens in USD
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_cost_per_1k_tokens: Option<f64>,

    /// Whether model supports image input
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,

    /// Whether model supports function/tool calling
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_function_calling: Option<bool>,

    /// Whether model supports streaming responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_streaming: Option<bool>,

    /// Whether model supports JSON schema responses
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_response_schema: Option<bool>,

    /// Whether model supports extended thinking/reasoning
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_reasoning: Option<bool>,

    /// Whether model supports computer use
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_computer_use: Option<bool>,

    /// Whether model supports PDF input
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_pdf_input: Option<bool>,

    /// Model operation mode (chat, completion, embedding, image_generation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// Union type for backward-compatible model mapping.
///
/// Supports both simple string format (e.g., "gpt-4-turbo") and
/// extended object format with metadata.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(untagged)]
pub enum ModelMappingValue {
    /// Simple string mapping (backward compatible)
    Simple(String),
    /// Extended mapping with metadata
    Extended(ModelMappingEntry),
}

impl<'de> Deserialize<'de> for ModelMappingValue {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(ModelMappingValue::Simple(s)),
            serde_json::Value::Object(_) => {
                let entry: ModelMappingEntry =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(ModelMappingValue::Extended(entry))
            }
            _ => Err(serde::de::Error::custom(
                "expected string or object for model mapping value",
            )),
        }
    }
}

impl ModelMappingValue {
    /// Get the mapped model name from either format.
    pub fn mapped_model(&self) -> &str {
        match self {
            ModelMappingValue::Simple(s) => s,
            ModelMappingValue::Extended(e) => &e.mapped_model,
        }
    }

    /// Get metadata if available (None for simple string format).
    pub fn metadata(&self) -> Option<&ModelMappingEntry> {
        match self {
            ModelMappingValue::Simple(_) => None,
            ModelMappingValue::Extended(e) => Some(e),
        }
    }
}

impl From<String> for ModelMappingValue {
    fn from(s: String) -> Self {
        ModelMappingValue::Simple(s)
    }
}

impl From<&str> for ModelMappingValue {
    fn from(s: &str) -> Self {
        ModelMappingValue::Simple(s.to_string())
    }
}

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

    /// TTFT (Time To First Token) timeout in seconds
    /// If set, returns an error if the first token doesn't arrive within this timeout
    #[serde(default)]
    pub ttft_timeout_secs: Option<u64>,

    /// List of credentials with optional rate limiting
    #[serde(default)]
    pub credentials: Vec<CredentialConfig>,

    /// Optional provider suffix for model name prefixing
    /// If set (e.g., "Proxy"), then "Proxy/gpt-4" is equivalent to "gpt-4"
    #[serde(default)]
    pub provider_suffix: Option<String>,

    /// Minimum tokens limit for max_tokens clamping
    #[serde(default = "default_min_tokens_limit")]
    pub min_tokens_limit: u32,

    /// Maximum tokens limit for max_tokens clamping
    #[serde(default = "default_max_tokens_limit")]
    pub max_tokens_limit: u32,
}

/// Configuration for a credential with optional rate limiting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialConfig {
    /// The actual credential key (or key hash when loaded from database)
    pub credential_key: String,

    /// Human-readable name for the credential
    pub name: String,

    /// Optional description
    #[serde(default)]
    pub description: Option<String>,

    /// Optional rate limiting configuration
    #[serde(default)]
    pub rate_limit: Option<RateLimitConfig>,

    /// Whether this credential is enabled
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// List of models this credential can access (empty = all models allowed)
    #[serde(default)]
    pub allowed_models: Vec<String>,
}

/// Rate limiting configuration for a credential.
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
    1
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

    /// Model name mappings (client model -> provider model or extended entry)
    #[serde(default)]
    pub model_mapping: HashMap<String, ModelMappingValue>,

    /// Provider type (e.g., "openai", "azure", "anthropic", "gcp-vertex")
    #[serde(default = "default_provider_type")]
    pub provider_type: String,

    /// Provider-specific parameters (e.g., GCP project, location, publisher)
    #[serde(default)]
    pub provider_params: HashMap<String, serde_json::Value>,

    /// Optional Lua script for request/response transformation
    #[serde(default)]
    pub lua_script: Option<String>,
}

fn default_provider_type() -> String {
    "openai".to_string()
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

fn default_min_tokens_limit() -> u32 {
    100
}

fn default_max_tokens_limit() -> u32 {
    4096
}

impl AppConfig {
    /// Load server configuration from environment variables.
    /// Providers and master keys are loaded from the database.
    pub fn from_env() -> Result<Self> {
        // Load .env file if it exists
        #[cfg(not(test))]
        dotenvy::dotenv().ok();

        let host = std::env::var("HOST").unwrap_or_else(|_| default_host());
        let port = std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or_else(default_port);
        let verify_ssl = std::env::var("VERIFY_SSL")
            .map(|v| str_to_bool(&v))
            .unwrap_or_else(|_| default_verify_ssl());
        let request_timeout_secs = std::env::var("REQUEST_TIMEOUT_SECS")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or_else(default_request_timeout);
        let ttft_timeout_secs = std::env::var("TTFT_TIMEOUT_SECS")
            .ok()
            .and_then(|t| t.parse().ok());
        let provider_suffix = std::env::var("PROVIDER_SUFFIX").ok();
        let min_tokens_limit = std::env::var("MIN_TOKENS_LIMIT")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or_else(default_min_tokens_limit);
        let max_tokens_limit = std::env::var("MAX_TOKENS_LIMIT")
            .ok()
            .and_then(|t| t.parse().ok())
            .unwrap_or_else(default_max_tokens_limit);

        Ok(Self {
            providers: vec![],
            server: ServerConfig { host, port },
            verify_ssl,
            request_timeout_secs,
            ttft_timeout_secs,
            credentials: vec![],
            provider_suffix,
            min_tokens_limit,
            max_tokens_limit,
        })
    }
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
    fn test_from_env_defaults() {
        unsafe {
            std::env::remove_var("HOST");
            std::env::remove_var("PORT");
            std::env::remove_var("VERIFY_SSL");
            std::env::remove_var("REQUEST_TIMEOUT_SECS");
            std::env::remove_var("TTFT_TIMEOUT_SECS");
            std::env::remove_var("PROVIDER_SUFFIX");
        }

        let config = AppConfig::from_env().unwrap();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 18000);
        assert!(config.verify_ssl);
        assert_eq!(config.request_timeout_secs, 300);
        assert!(config.ttft_timeout_secs.is_none());
        assert!(config.providers.is_empty());
        assert!(config.credentials.is_empty());
        assert!(config.provider_suffix.is_none());
    }

    #[test]
    #[serial]
    fn test_from_env_with_overrides() {
        unsafe {
            std::env::set_var("HOST", "127.0.0.1");
            std::env::set_var("PORT", "9000");
            std::env::set_var("VERIFY_SSL", "false");
            std::env::set_var("REQUEST_TIMEOUT_SECS", "60");
            std::env::set_var("TTFT_TIMEOUT_SECS", "30");
        }

        let config = AppConfig::from_env().unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.port, 9000);
        assert!(!config.verify_ssl);
        assert_eq!(config.request_timeout_secs, 60);
        assert_eq!(config.ttft_timeout_secs, Some(30));

        unsafe {
            std::env::remove_var("HOST");
            std::env::remove_var("PORT");
            std::env::remove_var("VERIFY_SSL");
            std::env::remove_var("REQUEST_TIMEOUT_SECS");
            std::env::remove_var("TTFT_TIMEOUT_SECS");
        }
    }

    #[test]
    #[serial]
    fn test_provider_suffix_from_env() {
        unsafe {
            std::env::set_var("PROVIDER_SUFFIX", "Proxy");
        }

        let config = AppConfig::from_env().unwrap();
        assert_eq!(config.provider_suffix, Some("Proxy".to_string()));

        unsafe {
            std::env::remove_var("PROVIDER_SUFFIX");
        }
    }

    #[test]
    #[serial]
    fn test_provider_suffix_empty_string() {
        // Empty string should still be Some("")
        unsafe {
            std::env::set_var("PROVIDER_SUFFIX", "");
        }

        let config = AppConfig::from_env().unwrap();
        assert_eq!(config.provider_suffix, Some("".to_string()));

        unsafe {
            std::env::remove_var("PROVIDER_SUFFIX");
        }
    }
}
