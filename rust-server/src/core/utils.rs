//! Utility functions shared across the LLM proxy server.
//!
//! This module contains common utility functions used by multiple API handlers.

use crate::core::config::CredentialConfig;

/// Get the name of the API key from credential config.
///
/// Returns "anonymous" if no credential is provided.
///
/// # Examples
///
/// ```
/// use llm_proxy_rust::core::utils::get_key_name;
/// use llm_proxy_rust::core::config::CredentialConfig;
///
/// let config = Some(CredentialConfig {
///     credential_key: "hash".to_string(),
///     name: "my-key".to_string(),
///     description: None,
///     rate_limit: None,
///     enabled: true,
///     allowed_models: vec![],
/// });
/// assert_eq!(get_key_name(&config), "my-key");
///
/// let no_config: Option<CredentialConfig> = None;
/// assert_eq!(get_key_name(&no_config), "anonymous");
/// ```
pub fn get_key_name(key_config: &Option<CredentialConfig>) -> String {
    key_config
        .as_ref()
        .map(|k| k.name.clone())
        .unwrap_or_else(|| "anonymous".to_string())
}

/// Strip provider suffix from model name.
///
/// When PROVIDER_SUFFIX is set (e.g., "openrouter"), model names like
/// "openrouter/gpt-4" are treated as "gpt-4".
///
/// # Arguments
/// * `model` - The model name to process
/// * `provider_suffix` - Optional provider suffix to strip
///
/// # Returns
/// The model name with the provider suffix stripped if it matches.
///
/// # Examples
/// ```
/// use llm_proxy_rust::core::utils::strip_provider_suffix;
///
/// // With matching prefix
/// assert_eq!(strip_provider_suffix("Proxy/gpt-4", Some("Proxy")), "gpt-4");
///
/// // Without prefix
/// assert_eq!(strip_provider_suffix("gpt-4", Some("Proxy")), "gpt-4");
///
/// // Different prefix (unchanged)
/// assert_eq!(strip_provider_suffix("Other/gpt-4", Some("Proxy")), "Other/gpt-4");
///
/// // No suffix configured
/// assert_eq!(strip_provider_suffix("Proxy/gpt-4", None), "Proxy/gpt-4");
/// ```
pub fn strip_provider_suffix(model: &str, provider_suffix: Option<&str>) -> String {
    let Some(suffix) = provider_suffix.filter(|s| !s.is_empty()) else {
        return model.to_string();
    };

    model
        .strip_prefix(suffix)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(model)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_provider_suffix_with_matching_prefix() {
        // When provider_suffix is "Proxy", "Proxy/gpt-4" should become "gpt-4"
        assert_eq!(strip_provider_suffix("Proxy/gpt-4", Some("Proxy")), "gpt-4");
    }

    #[test]
    fn test_strip_provider_suffix_without_prefix() {
        // When model doesn't have the prefix, it should remain unchanged
        assert_eq!(strip_provider_suffix("gpt-4", Some("Proxy")), "gpt-4");
    }

    #[test]
    fn test_strip_provider_suffix_different_prefix() {
        // When model has a different prefix, it should remain unchanged
        assert_eq!(
            strip_provider_suffix("Other/gpt-4", Some("Proxy")),
            "Other/gpt-4"
        );
    }

    #[test]
    fn test_strip_provider_suffix_no_suffix_configured() {
        // When no provider_suffix is configured, model should remain unchanged
        assert_eq!(strip_provider_suffix("Proxy/gpt-4", None), "Proxy/gpt-4");
    }

    #[test]
    fn test_strip_provider_suffix_empty_suffix() {
        // When provider_suffix is empty string, model should remain unchanged
        assert_eq!(
            strip_provider_suffix("Proxy/gpt-4", Some("")),
            "Proxy/gpt-4"
        );
    }

    #[test]
    fn test_strip_provider_suffix_complex_model_name() {
        // Test with more complex model names
        assert_eq!(
            strip_provider_suffix("Proxy/gpt-4-turbo-preview", Some("Proxy")),
            "gpt-4-turbo-preview"
        );
        assert_eq!(
            strip_provider_suffix("Proxy/claude-3-opus-20240229", Some("Proxy")),
            "claude-3-opus-20240229"
        );
    }

    #[test]
    fn test_strip_provider_suffix_nested_slashes() {
        // Test with model names that have slashes in them
        assert_eq!(
            strip_provider_suffix("Proxy/org/model-name", Some("Proxy")),
            "org/model-name"
        );
    }

    #[test]
    fn test_strip_provider_suffix_case_sensitive() {
        // Prefix matching should be case-sensitive
        assert_eq!(
            strip_provider_suffix("proxy/gpt-4", Some("Proxy")),
            "proxy/gpt-4"
        );
        assert_eq!(
            strip_provider_suffix("PROXY/gpt-4", Some("Proxy")),
            "PROXY/gpt-4"
        );
    }

    #[test]
    fn test_strip_provider_suffix_partial_match() {
        // Should not strip if it's only a partial match (no slash)
        assert_eq!(
            strip_provider_suffix("Proxygpt-4", Some("Proxy")),
            "Proxygpt-4"
        );
    }

    // ========================================================================
    // get_key_name Tests
    // ========================================================================

    #[test]
    fn test_get_key_name_with_credential() {
        let config = Some(CredentialConfig {
            credential_key: "hash".to_string(),
            name: "test-key".to_string(),
            description: None,
            rate_limit: None,
            enabled: true,
            allowed_models: vec![],
        });
        assert_eq!(get_key_name(&config), "test-key");
    }

    #[test]
    fn test_get_key_name_without_credential() {
        let config: Option<CredentialConfig> = None;
        assert_eq!(get_key_name(&config), "anonymous");
    }

    #[test]
    fn test_get_key_name_with_empty_name() {
        let config = Some(CredentialConfig {
            credential_key: "hash".to_string(),
            name: "".to_string(),
            description: None,
            rate_limit: None,
            enabled: true,
            allowed_models: vec![],
        });
        assert_eq!(get_key_name(&config), "");
    }
}
