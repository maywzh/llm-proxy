//! Shared authentication module.
//!
//! This module provides unified authentication logic for all API endpoints,
//! supporting both OpenAI-style (Authorization: Bearer) and Claude-style (x-api-key) headers.

use axum::http::HeaderMap;
use sha2::{Digest, Sha256};

use crate::core::config::CredentialConfig;
use crate::core::error::Result;
use crate::core::AppError;
use crate::AppState;

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
/// let credential = verify_auth(&headers, &state, AuthFormat::BearerOnly)?;
///
/// // For Claude-compatible endpoints
/// let credential = verify_auth(&headers, &state, AuthFormat::MultiFormat)?;
/// ```
pub fn verify_auth(
    headers: &HeaderMap,
    state: &AppState,
    format: AuthFormat,
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
            // Check rate limit for this credential using the hash
            state
                .rate_limiter
                .check_rate_limit(&credential_config.credential_key)?;

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
}
