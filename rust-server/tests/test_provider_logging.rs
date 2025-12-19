//! Integration tests for Provider logging functionality
//!
//! This test suite verifies that the provider context is properly set
//! and that HTTP request logs include the provider name.

use llm_proxy_rust::core::logging::{
    clear_provider_context, get_provider_context, set_provider_context, PROVIDER_CONTEXT,
};
use std::cell::RefCell;

#[tokio::test]
async fn test_provider_context_basic() {
    // Test basic set and get
    PROVIDER_CONTEXT
        .scope(RefCell::new(String::new()), async {
            set_provider_context("TestProvider");
            let context = get_provider_context();
            assert_eq!(context, "TestProvider");
        })
        .await;
}

#[tokio::test]
async fn test_provider_context_clear() {
    // Test clearing context
    PROVIDER_CONTEXT
        .scope(RefCell::new(String::new()), async {
            set_provider_context("TestProvider");
            assert_eq!(get_provider_context(), "TestProvider");

            clear_provider_context();
            assert_eq!(get_provider_context(), "");
        })
        .await;
}

#[tokio::test]
async fn test_provider_context_isolation() {
    // Test that contexts are isolated between tasks
    let task1 = tokio::spawn(async {
        PROVIDER_CONTEXT
            .scope(RefCell::new(String::new()), async {
                set_provider_context("Provider1");
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                get_provider_context()
            })
            .await
    });

    let task2 = tokio::spawn(async {
        PROVIDER_CONTEXT
            .scope(RefCell::new(String::new()), async {
                set_provider_context("Provider2");
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
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
async fn test_provider_context_nested_scopes() {
    // Test nested scopes
    PROVIDER_CONTEXT
        .scope(RefCell::new(String::new()), async {
            set_provider_context("OuterProvider");
            assert_eq!(get_provider_context(), "OuterProvider");

            PROVIDER_CONTEXT
                .scope(RefCell::new(String::new()), async {
                    set_provider_context("InnerProvider");
                    assert_eq!(get_provider_context(), "InnerProvider");
                })
                .await;

            // After inner scope, outer context should still be active
            assert_eq!(get_provider_context(), "OuterProvider");
        })
        .await;
}

#[tokio::test]
async fn test_provider_logging_simulation() {
    // Simulate the actual usage pattern in handlers
    PROVIDER_CONTEXT
        .scope(RefCell::new(String::new()), async {
            let provider_name = "OpenAI";
            set_provider_context(provider_name);

            // Simulate HTTP request logging
            let context = get_provider_context();
            assert_eq!(context, provider_name);

            // Simulate cleanup
            clear_provider_context();
            assert_eq!(get_provider_context(), "");
        })
        .await;
}

#[tokio::test]
async fn test_concurrent_provider_requests() {
    // Simulate multiple concurrent requests to different providers
    let providers = vec!["OpenAI", "Anthropic", "Google", "Azure"];
    
    let tasks: Vec<_> = providers
        .into_iter()
        .map(|provider| {
            tokio::spawn(async move {
                PROVIDER_CONTEXT
                    .scope(RefCell::new(String::new()), async move {
                        set_provider_context(provider);
                        
                        // Simulate some work
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                        
                        let context = get_provider_context();
                        assert_eq!(context, provider);
                        clear_provider_context();
                        
                        provider
                    })
                    .await
            })
        })
        .collect();

    let results: Vec<_> = futures::future::join_all(tasks)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(results.len(), 4);
}

#[tokio::test]
async fn test_provider_context_default() {
    // Test that context returns empty string when not set
    PROVIDER_CONTEXT
        .scope(RefCell::new(String::new()), async {
            assert_eq!(get_provider_context(), "");
        })
        .await;
}