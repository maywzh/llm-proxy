//! Logging utilities with provider context support.
//!
//! This module provides context-aware logging that can include provider names
//! and request IDs in HTTP request logs, similar to Python's contextvars implementation.

tokio::task_local! {
    /// Task-local storage for the current provider name.
    ///
    /// This allows HTTP request logs to include the provider name
    /// without passing it through every function call.
    pub static PROVIDER_CONTEXT: String;
}

tokio::task_local! {
    /// Task-local storage for the current request ID.
    ///
    /// This allows HTTP request logs to include a unique request ID
    /// for tracking all logs related to a single request.
    pub static REQUEST_ID: String;
}

tokio::task_local! {
    /// Task-local storage for the current API key name.
    ///
    /// This allows metrics and logging to include the API key name
    /// without passing it through every function call.
    pub static API_KEY_NAME: String;
}

/// Get the current provider name from context, if set.
///
/// Returns an empty string if no provider context is set.
pub fn get_provider_context() -> String {
    PROVIDER_CONTEXT
        .try_with(|ctx| ctx.clone())
        .unwrap_or_default()
}

/// Get the current request ID from context, if set.
///
/// Returns an empty string if no request ID is set.
pub fn get_request_id() -> String {
    REQUEST_ID.try_with(|id| id.clone()).unwrap_or_default()
}

/// Get the current API key name from context, if set.
///
/// Returns "anonymous" if no API key name is set.
pub fn get_api_key_name() -> String {
    API_KEY_NAME
        .try_with(|name| name.clone())
        .unwrap_or_else(|_| "anonymous".to_string())
}

/// Generate a new unique request ID using UUID v4.
///
/// Returns a string representation of the UUID.
pub fn generate_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Create a tracing span with provider context.
///
/// This is a helper function to create spans that include provider information.
///
/// # Arguments
///
/// * `name` - Span name
/// * `provider` - Provider name
///
/// # Returns
///
/// A tracing span with provider field
#[macro_export]
macro_rules! provider_span {
    ($name:expr, $provider:expr) => {
        tracing::info_span!($name, provider = %$provider)
    };
}

/// Execute an async block with request context (request_id, api_key_name, provider).
///
/// This macro simplifies the nested scope pattern used in handlers.
///
/// # Arguments
///
/// * `request_id` - The request ID string
/// * `api_key_name` - The API key name string
/// * `provider_name` - The provider name string
/// * `body` - The async block to execute
///
/// # Example
///
/// ```ignore
/// with_request_context!(request_id, api_key_name, provider_name, async {
///     // handler logic here
/// })
/// ```
#[macro_export]
macro_rules! with_request_context {
    ($request_id:expr, $api_key_name:expr, $provider_name:expr, $body:expr) => {
        $crate::core::logging::REQUEST_ID
            .scope($request_id, async {
                $crate::core::logging::API_KEY_NAME
                    .scope($api_key_name, async {
                        $crate::core::logging::PROVIDER_CONTEXT
                            .scope($provider_name, $body)
                            .await
                    })
                    .await
            })
            .await
    };
    // Version without provider context
    ($request_id:expr, $api_key_name:expr, $body:expr) => {
        $crate::core::logging::REQUEST_ID
            .scope($request_id, async {
                $crate::core::logging::API_KEY_NAME
                    .scope($api_key_name, $body)
                    .await
            })
            .await
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_provider_context_get() {
        PROVIDER_CONTEXT
            .scope("TestProvider".to_string(), async {
                assert_eq!(get_provider_context(), "TestProvider");
            })
            .await;
    }

    #[tokio::test]
    async fn test_provider_context_isolation() {
        // Test that contexts are isolated between tasks
        let task1 = tokio::spawn(async {
            PROVIDER_CONTEXT
                .scope("Provider1".to_string(), async {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    get_provider_context()
                })
                .await
        });

        let task2 = tokio::spawn(async {
            PROVIDER_CONTEXT
                .scope("Provider2".to_string(), async {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    get_provider_context()
                })
                .await
        });

        let result1 = task1.await.unwrap();
        let result2 = task2.await.unwrap();

        assert_eq!(result1, "Provider1");
        assert_eq!(result2, "Provider2");
    }

    #[tokio::test]
    async fn test_provider_context_default() {
        // Test that context returns empty string when not set
        assert_eq!(get_provider_context(), "");
    }

    #[tokio::test]
    async fn test_request_id_get() {
        let request_id = "test-request-123".to_string();
        REQUEST_ID
            .scope(request_id.clone(), async {
                assert_eq!(get_request_id(), "test-request-123");
            })
            .await;
    }

    #[tokio::test]
    async fn test_request_id_isolation() {
        // Test that request IDs are isolated between tasks
        let task1 = tokio::spawn(async {
            REQUEST_ID
                .scope("request-1".to_string(), async {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    get_request_id()
                })
                .await
        });

        let task2 = tokio::spawn(async {
            REQUEST_ID
                .scope("request-2".to_string(), async {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    get_request_id()
                })
                .await
        });

        let result1 = task1.await.unwrap();
        let result2 = task2.await.unwrap();

        assert_eq!(result1, "request-1");
        assert_eq!(result2, "request-2");
    }

    #[tokio::test]
    async fn test_request_id_default() {
        // Test that request ID returns empty string when not set
        assert_eq!(get_request_id(), "");
    }

    #[tokio::test]
    async fn test_generate_request_id() {
        // Test that generate_request_id creates valid UUIDs
        let id1 = generate_request_id();
        let id2 = generate_request_id();

        // UUIDs should be 36 characters (including hyphens)
        assert_eq!(id1.len(), 36);
        assert_eq!(id2.len(), 36);

        // Each generated ID should be unique
        assert_ne!(id1, id2);

        // Should be valid UUID format (8-4-4-4-12)
        let parts: Vec<&str> = id1.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0].len(), 8);
        assert_eq!(parts[1].len(), 4);
        assert_eq!(parts[2].len(), 4);
        assert_eq!(parts[3].len(), 4);
        assert_eq!(parts[4].len(), 12);
    }

    #[tokio::test]
    async fn test_nested_contexts() {
        // Test that both provider and request ID contexts work together
        let request_id = "test-request-456".to_string();
        let provider = "TestProvider".to_string();

        REQUEST_ID
            .scope(request_id.clone(), async {
                PROVIDER_CONTEXT
                    .scope(provider.clone(), async {
                        assert_eq!(get_request_id(), "test-request-456");
                        assert_eq!(get_provider_context(), "TestProvider");
                    })
                    .await
            })
            .await;
    }

    #[tokio::test]
    async fn test_api_key_name_get() {
        API_KEY_NAME
            .scope("test-key".to_string(), async {
                assert_eq!(get_api_key_name(), "test-key");
            })
            .await;
    }

    #[tokio::test]
    async fn test_api_key_name_default() {
        // Test that API key name returns "anonymous" when not set
        assert_eq!(get_api_key_name(), "anonymous");
    }

    #[tokio::test]
    async fn test_api_key_name_isolation() {
        // Test that API key names are isolated between tasks
        let task1 = tokio::spawn(async {
            API_KEY_NAME
                .scope("key-1".to_string(), async {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    get_api_key_name()
                })
                .await
        });

        let task2 = tokio::spawn(async {
            API_KEY_NAME
                .scope("key-2".to_string(), async {
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                    get_api_key_name()
                })
                .await
        });

        let result1 = task1.await.unwrap();
        let result2 = task2.await.unwrap();

        assert_eq!(result1, "key-1");
        assert_eq!(result2, "key-2");
    }

    #[tokio::test]
    async fn test_all_contexts_together() {
        // Test that all three contexts work together
        let request_id = "test-request-789".to_string();
        let provider = "TestProvider".to_string();
        let api_key = "test-api-key".to_string();

        REQUEST_ID
            .scope(request_id.clone(), async {
                PROVIDER_CONTEXT
                    .scope(provider.clone(), async {
                        API_KEY_NAME
                            .scope(api_key.clone(), async {
                                assert_eq!(get_request_id(), "test-request-789");
                                assert_eq!(get_provider_context(), "TestProvider");
                                assert_eq!(get_api_key_name(), "test-api-key");
                            })
                            .await
                    })
                    .await
            })
            .await;
    }
}
