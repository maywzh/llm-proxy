//! Logging utilities with provider context support.
//!
//! This module provides context-aware logging that can include provider names
//! in HTTP request logs, similar to Python's contextvars implementation.

use std::cell::RefCell;

tokio::task_local! {
    /// Task-local storage for the current provider name.
    /// 
    /// This allows HTTP request logs to include the provider name
    /// without passing it through every function call.
    pub static PROVIDER_CONTEXT: RefCell<String>;
}

/// Set the current provider name in the task-local context.
///
/// This should be called before making HTTP requests to a provider.
/// The provider name will be included in subsequent HTTP request logs.
///
/// # Arguments
///
/// * `provider_name` - Name of the provider making the request
///
/// # Examples
///
/// ```no_run
/// use llm_proxy_rust::core::logging::set_provider_context;
///
/// set_provider_context("OpenAI");
/// // HTTP requests will now be logged with [Provider: OpenAI] prefix
/// ```
pub fn set_provider_context(provider_name: &str) {
    if let Ok(context) = PROVIDER_CONTEXT.try_with(|ctx| {
        *ctx.borrow_mut() = provider_name.to_string();
    }) {
        context
    }
}

/// Clear the provider context.
///
/// This should be called after completing requests to a provider
/// to avoid leaking context to subsequent operations.
pub fn clear_provider_context() {
    if let Ok(context) = PROVIDER_CONTEXT.try_with(|ctx| {
        ctx.borrow_mut().clear();
    }) {
        context
    }
}

/// Get the current provider name from context, if set.
///
/// Returns an empty string if no provider context is set.
pub fn get_provider_context() -> String {
    PROVIDER_CONTEXT
        .try_with(|ctx| ctx.borrow().clone())
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_provider_context_set_and_get() {
        PROVIDER_CONTEXT.scope(RefCell::new(String::new()), async {
            set_provider_context("TestProvider");
            assert_eq!(get_provider_context(), "TestProvider");
        }).await;
    }

    #[tokio::test]
    async fn test_provider_context_clear() {
        PROVIDER_CONTEXT.scope(RefCell::new(String::new()), async {
            set_provider_context("TestProvider");
            assert_eq!(get_provider_context(), "TestProvider");
            
            clear_provider_context();
            assert_eq!(get_provider_context(), "");
        }).await;
    }

    #[tokio::test]
    async fn test_provider_context_isolation() {
        // Test that contexts are isolated between tasks
        let task1 = tokio::spawn(async {
            PROVIDER_CONTEXT.scope(RefCell::new(String::new()), async {
                set_provider_context("Provider1");
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                get_provider_context()
            }).await
        });

        let task2 = tokio::spawn(async {
            PROVIDER_CONTEXT.scope(RefCell::new(String::new()), async {
                set_provider_context("Provider2");
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                get_provider_context()
            }).await
        });

        let result1 = task1.await.unwrap();
        let result2 = task2.await.unwrap();

        assert_eq!(result1, "Provider1");
        assert_eq!(result2, "Provider2");
    }

    #[tokio::test]
    async fn test_provider_context_default() {
        // Test that context returns empty string when not set
        PROVIDER_CONTEXT.scope(RefCell::new(String::new()), async {
            assert_eq!(get_provider_context(), "");
        }).await;
    }
}