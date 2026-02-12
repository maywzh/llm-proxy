//! Shared authentication module.
//!
//! This module provides unified authentication logic for all API endpoints,
//! supporting both OpenAI-style (Authorization: Bearer) and Claude-style (x-api-key) headers.

use axum::http::HeaderMap;
use sha2::{Digest, Sha256};

use crate::api::models::{compile_pattern, is_pattern};
use crate::core::config::CredentialConfig;
use crate::core::error::Result;
use crate::core::AppError;
use crate::AppState;

// ============================================================================
// Rate Limit Exemptions
// ============================================================================

/// Paths that should be exempt from rate limiting.
/// These endpoints perform local computation and don't consume upstream LLM resources.
pub const RATE_LIMIT_EXEMPT_PATHS: &[&str] =
    &["/v1/messages/count_tokens", "/v2/messages/count_tokens"];

// ============================================================================
// Authentication Format
// ============================================================================

/// Authentication header format to support.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthFormat {
    /// Only support Authorization: Bearer header (OpenAI style)
    BearerOnly,
    /// Support both x-api-key and Authorization: Bearer headers (multi-format)
    MultiFormat,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Hash an API key using SHA-256.
pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract API key from headers based on the specified format.
fn extract_api_key(headers: &HeaderMap, format: AuthFormat) -> Option<&str> {
    match format {
        AuthFormat::BearerOnly => extract_bearer(headers),
        AuthFormat::MultiFormat => {
            // x-api-key takes priority
            headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .or_else(|| extract_bearer(headers))
        }
    }
}

/// Extract Bearer token from Authorization header.
fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
}

// ============================================================================
// Main Authentication Function
// ============================================================================

/// Verify authentication and return the matched credential configuration.
///
/// # Arguments
///
/// * `headers` - Request headers containing authentication info
/// * `state` - Application state containing credentials and rate limiter
/// * `format` - Which authentication header format to accept
/// * `request_path` - The request path (used to skip rate limiting for exempt endpoints)
///
/// # Returns
///
/// * `Ok(Some(credential))` - Authentication successful, returns matched credential
/// * `Ok(None)` - No authentication required (empty credentials list)
/// * `Err(AppError::Unauthorized)` - Authentication failed
///
/// # Example
///
/// ```ignore
/// use llm_proxy_rust::api::auth::{verify_auth, AuthFormat};
///
/// // For OpenAI-compatible endpoints
/// let credential = verify_auth(&headers, &state, AuthFormat::BearerOnly, Some("/v1/chat/completions"))?;
///
/// // For Claude-compatible endpoints
/// let credential = verify_auth(&headers, &state, AuthFormat::MultiFormat, Some("/v1/messages"))?;
/// ```
pub fn verify_auth(
    headers: &HeaderMap,
    state: &AppState,
    format: AuthFormat,
    request_path: Option<&str>,
) -> Result<Option<CredentialConfig>> {
    // Extract the provided key from headers
    let provided_key = extract_api_key(headers, format);

    // Get credentials from DynamicConfig if available
    let credentials = state.get_credentials();

    // Check if any authentication is required
    if credentials.is_empty() {
        return Ok(None);
    }

    // If credentials are configured, a key is required
    let provided_key = provided_key.ok_or(AppError::Unauthorized)?;

    // Hash the provided key for comparison with stored hashes
    let provided_key_hash = hash_key(provided_key);

    // Check against credentials configuration
    for credential_config in credentials {
        if credential_config.enabled && credential_config.credential_key == provided_key_hash {
            // Check if request path is exempt from rate limiting
            let is_exempt = request_path
                .map(|path| RATE_LIMIT_EXEMPT_PATHS.contains(&path))
                .unwrap_or(false);

            // Only check rate limit if path is not exempt
            if !is_exempt {
                // Check rate limit for this credential using the hash
                // Wrap error with key_name context at auth layer
                if let Err(AppError::RateLimitExceeded { message, .. }) = state
                    .rate_limiter
                    .check_rate_limit(&credential_config.credential_key)
                {
                    return Err(AppError::RateLimitExceeded {
                        message,
                        key_name: Some(credential_config.name.clone()),
                    });
                }
            }

            tracing::debug!(
                credential_name = %credential_config.name,
                "Authentication successful"
            );

            return Ok(Some(credential_config));
        }
    }

    // No matching credential found
    Err(AppError::Unauthorized)
}

// ============================================================================
// Model Permission Check
// ============================================================================

/// Check if a model matches any entry in allowed_models list.
///
/// Supports:
/// - Exact match: "gpt-4" matches "gpt-4"
/// - Simple wildcard: "gpt-*" matches "gpt-4", "gpt-4o"
/// - Regex pattern: "claude-opus-4-5-.*" matches "claude-opus-4-5-20240620"
pub fn model_matches_allowed_list(model: &str, allowed_models: &[String]) -> bool {
    for pattern in allowed_models {
        if is_pattern(pattern) {
            if let Some(regex) = compile_pattern(pattern) {
                if regex.is_match(model) {
                    return true;
                }
            }
        } else if model == pattern {
            return true;
        }
    }
    false
}

/// Check if the model is allowed for the given credential.
///
/// Returns Ok(()) if:
/// - model is None
/// - credential_config is None (no auth required)
/// - allowed_models is empty (all models allowed)
/// - model matches any entry in allowed_models (exact or pattern)
///
/// Returns Err(AppError::Forbidden) if model is not allowed.
pub fn check_model_permission(
    model: Option<&str>,
    credential_config: &Option<CredentialConfig>,
) -> Result<()> {
    let Some(model) = model else {
        return Ok(());
    };

    let Some(config) = credential_config else {
        return Ok(());
    };

    if config.allowed_models.is_empty() {
        return Ok(());
    }

    if model_matches_allowed_list(model, &config.allowed_models) {
        return Ok(());
    }

    tracing::warn!(
        model = %model,
        credential_name = %config.name,
        allowed_models = ?config.allowed_models,
        "Model not allowed for this credential"
    );

    Err(AppError::Forbidden(format!(
        "Model '{}' is not allowed for this credential. Allowed models: {:?}",
        model, config.allowed_models
    )))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_key() {
        let key = "sk-test-key-123";
        let hash = hash_key(key);
        // Hash should be 64 hex characters (256 bits)
        assert_eq!(hash.len(), 64);
        // Same key should produce same hash
        assert_eq!(hash, hash_key(key));
        // Different key should produce different hash
        assert_ne!(hash, hash_key("different-key"));
    }

    #[test]
    fn test_extract_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-test-key".parse().unwrap());
        assert_eq!(extract_bearer(&headers), Some("sk-test-key"));
    }

    #[test]
    fn test_extract_bearer_missing() {
        let headers = HeaderMap::new();
        assert_eq!(extract_bearer(&headers), None);
    }

    #[test]
    fn test_extract_bearer_wrong_format() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Basic dXNlcjpwYXNz".parse().unwrap());
        assert_eq!(extract_bearer(&headers), None);
    }

    #[test]
    fn test_extract_api_key_bearer_only() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-test-key".parse().unwrap());
        headers.insert("x-api-key", "sk-claude-key".parse().unwrap());

        // BearerOnly should only use Bearer header
        assert_eq!(
            extract_api_key(&headers, AuthFormat::BearerOnly),
            Some("sk-test-key")
        );
    }

    #[test]
    fn test_extract_api_key_multi_format_prefers_x_api_key() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-test-key".parse().unwrap());
        headers.insert("x-api-key", "sk-claude-key".parse().unwrap());

        // MultiFormat should prefer x-api-key
        assert_eq!(
            extract_api_key(&headers, AuthFormat::MultiFormat),
            Some("sk-claude-key")
        );
    }

    #[test]
    fn test_extract_api_key_multi_format_falls_back_to_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer sk-test-key".parse().unwrap());

        // MultiFormat should fall back to Bearer if x-api-key is missing
        assert_eq!(
            extract_api_key(&headers, AuthFormat::MultiFormat),
            Some("sk-test-key")
        );
    }

    // ========================================================================
    // Model Permission Tests
    // ========================================================================

    #[test]
    fn test_model_matches_allowed_list_exact_match() {
        let allowed = vec![
            "gpt-4".to_string(),
            "gpt-3.5-turbo".to_string(),
            "claude-3-opus".to_string(),
        ];

        assert!(model_matches_allowed_list("gpt-4", &allowed));
        assert!(model_matches_allowed_list("gpt-3.5-turbo", &allowed));
        assert!(model_matches_allowed_list("claude-3-opus", &allowed));
        assert!(!model_matches_allowed_list("gpt-4o", &allowed));
        assert!(!model_matches_allowed_list("unknown", &allowed));
    }

    #[test]
    fn test_model_matches_allowed_list_simple_wildcard() {
        let allowed = vec!["gpt-*".to_string(), "claude-3-*".to_string()];

        assert!(model_matches_allowed_list("gpt-4", &allowed));
        assert!(model_matches_allowed_list("gpt-4o", &allowed));
        assert!(model_matches_allowed_list("gpt-3.5-turbo", &allowed));
        assert!(model_matches_allowed_list("claude-3-opus", &allowed));
        assert!(model_matches_allowed_list("claude-3-sonnet", &allowed));
        assert!(!model_matches_allowed_list("claude-2", &allowed));
        assert!(!model_matches_allowed_list("llama-2", &allowed));
    }

    #[test]
    fn test_model_matches_allowed_list_regex_pattern() {
        let allowed = vec!["claude-opus-4-5-.*".to_string(), "gpt-4-.*".to_string()];

        assert!(model_matches_allowed_list(
            "claude-opus-4-5-20240620",
            &allowed
        ));
        assert!(model_matches_allowed_list(
            "claude-opus-4-5-latest",
            &allowed
        ));
        assert!(model_matches_allowed_list("gpt-4-turbo", &allowed));
        assert!(model_matches_allowed_list("gpt-4-0125-preview", &allowed));
        // These should NOT match because .* requires at least one character after the prefix
        assert!(!model_matches_allowed_list("claude-opus-4-5", &allowed));
        assert!(!model_matches_allowed_list("gpt-4", &allowed));
        assert!(!model_matches_allowed_list("gpt-3.5-turbo", &allowed));
    }

    #[test]
    fn test_model_matches_allowed_list_empty() {
        let allowed: Vec<String> = vec![];
        assert!(!model_matches_allowed_list("any-model", &allowed));
    }

    #[test]
    fn test_check_model_permission_no_model() {
        let config = Some(CredentialConfig {
            credential_key: "test".to_string(),
            name: "test".to_string(),
            description: None,
            rate_limit: None,
            enabled: true,
            allowed_models: vec!["gpt-4".to_string()],
        });
        assert!(check_model_permission(None, &config).is_ok());
    }

    #[test]
    fn test_check_model_permission_no_credential() {
        assert!(check_model_permission(Some("gpt-4"), &None).is_ok());
    }

    #[test]
    fn test_check_model_permission_empty_allowed_models() {
        let config = Some(CredentialConfig {
            credential_key: "test".to_string(),
            name: "test".to_string(),
            description: None,
            rate_limit: None,
            enabled: true,
            allowed_models: vec![],
        });
        assert!(check_model_permission(Some("any-model"), &config).is_ok());
    }

    #[test]
    fn test_check_model_permission_allowed_model() {
        let config = Some(CredentialConfig {
            credential_key: "test".to_string(),
            name: "test".to_string(),
            description: None,
            rate_limit: None,
            enabled: true,
            allowed_models: vec!["gpt-4".to_string(), "gpt-3.5-turbo".to_string()],
        });
        assert!(check_model_permission(Some("gpt-4"), &config).is_ok());
        assert!(check_model_permission(Some("gpt-3.5-turbo"), &config).is_ok());
    }

    #[test]
    fn test_check_model_permission_disallowed_model() {
        let config = Some(CredentialConfig {
            credential_key: "test".to_string(),
            name: "test".to_string(),
            description: None,
            rate_limit: None,
            enabled: true,
            allowed_models: vec!["gpt-4".to_string()],
        });
        let result = check_model_permission(Some("gpt-3.5-turbo"), &config);
        assert!(result.is_err());
        if let Err(AppError::Forbidden(msg)) = result {
            assert!(msg.contains("gpt-3.5-turbo"));
            assert!(msg.contains("not allowed"));
        } else {
            panic!("Expected Forbidden error");
        }
    }

    #[test]
    fn test_check_model_permission_wildcard_allowed() {
        let config = Some(CredentialConfig {
            credential_key: "test".to_string(),
            name: "test".to_string(),
            description: None,
            rate_limit: None,
            enabled: true,
            allowed_models: vec!["claude-opus-4-5-.*".to_string()],
        });
        assert!(check_model_permission(Some("claude-opus-4-5-20240620"), &config).is_ok());
        assert!(check_model_permission(Some("claude-opus-4-5-latest"), &config).is_ok());
    }

    #[test]
    fn test_check_model_permission_wildcard_disallowed() {
        let config = Some(CredentialConfig {
            credential_key: "test".to_string(),
            name: "test".to_string(),
            description: None,
            rate_limit: None,
            enabled: true,
            allowed_models: vec!["claude-opus-4-5-.*".to_string()],
        });
        assert!(check_model_permission(Some("claude-3-opus"), &config).is_err());
    }

    #[test]
    fn test_check_model_permission_simple_wildcard() {
        let config = Some(CredentialConfig {
            credential_key: "test".to_string(),
            name: "test".to_string(),
            description: None,
            rate_limit: None,
            enabled: true,
            allowed_models: vec!["gpt-*".to_string()],
        });
        assert!(check_model_permission(Some("gpt-4"), &config).is_ok());
        assert!(check_model_permission(Some("gpt-4o"), &config).is_ok());
        assert!(check_model_permission(Some("gpt-3.5-turbo"), &config).is_ok());
        assert!(check_model_permission(Some("claude-3-opus"), &config).is_err());
    }
}
