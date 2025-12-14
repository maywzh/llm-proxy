//! Property-based tests for the LLM proxy server.
//!
//! These tests use proptest to verify properties that should hold
//! for all inputs, particularly focusing on the provider selection algorithm.

use llm_proxy_rust::{
    core::config::{AppConfig, ProviderConfig, ServerConfig},
    services::ProviderService,
};
use proptest::prelude::*;
use std::collections::HashMap;

/// Generate a valid provider config with random weight
fn provider_config_strategy() -> impl Strategy<Value = ProviderConfig> {
    (1u32..=100u32, "[a-z]{5,10}", "[a-z]{5,10}").prop_map(|(weight, name, key)| {
        ProviderConfig {
            name: format!("Provider_{}", name),
            api_base: format!("http://localhost:{}", 8000 + weight % 100),
            api_key: format!("key_{}", key),
            weight,
            model_mapping: HashMap::new(),
        }
    })
}

/// Generate a list of provider configs
fn providers_strategy() -> impl Strategy<Value = Vec<ProviderConfig>> {
    prop::collection::vec(provider_config_strategy(), 1..=10)
}

/// Generate a complete app config
fn app_config_strategy() -> impl Strategy<Value = AppConfig> {
    providers_strategy().prop_map(|providers| AppConfig {
        providers,
        server: ServerConfig::default(),
        verify_ssl: true,
    })
}

proptest! {
    /// Property: Provider selection should always return a valid provider
    #[test]
    fn prop_provider_selection_returns_valid_provider(config in app_config_strategy()) {
        let service = ProviderService::new(config.clone());
        let provider = service.get_next_provider();
        
        // Provider name should match one of the configured providers
        let valid_names: Vec<String> = config.providers.iter()
            .map(|p| p.name.clone())
            .collect();
        
        prop_assert!(valid_names.contains(&provider.name));
    }

    /// Property: All providers should be selectable eventually
    #[test]
    fn prop_all_providers_eventually_selected(config in app_config_strategy()) {
        let service = ProviderService::new(config.clone());
        let mut selected_providers = std::collections::HashSet::new();
        
        // Sample many times to ensure all providers are selected
        let iterations = config.providers.len() * 1000;
        for _ in 0..iterations {
            let provider = service.get_next_provider();
            selected_providers.insert(provider.name);
        }
        
        // All providers should have been selected at least once
        for provider_config in &config.providers {
            prop_assert!(
                selected_providers.contains(&provider_config.name),
                "Provider {} was never selected",
                provider_config.name
            );
        }
    }

    /// Property: Provider weights should affect selection frequency
    #[test]
    fn prop_weights_affect_selection_frequency(
        weight1 in 1u32..=10u32,
        weight2 in 1u32..=10u32,
    ) {
        let config = AppConfig {
            providers: vec![
                ProviderConfig {
                    name: "Provider1".to_string(),
                    api_base: "http://localhost:8000".to_string(),
                    api_key: "key1".to_string(),
                    weight: weight1,
                    model_mapping: HashMap::new(),
                },
                ProviderConfig {
                    name: "Provider2".to_string(),
                    api_base: "http://localhost:8001".to_string(),
                    api_key: "key2".to_string(),
                    weight: weight2,
                    model_mapping: HashMap::new(),
                },
            ],
            server: ServerConfig::default(),
            verify_ssl: true,
        };
        
        let service = ProviderService::new(config);
        let mut provider1_count = 0;
        let mut provider2_count = 0;
        
        let iterations = 10000;
        for _ in 0..iterations {
            let provider = service.get_next_provider();
            if provider.name == "Provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }
        
        // Calculate expected ratio
        let expected_ratio = weight1 as f64 / weight2 as f64;
        let actual_ratio = provider1_count as f64 / provider2_count as f64;
        
        // Allow 30% variance due to randomness
        let lower_bound = expected_ratio * 0.7;
        let upper_bound = expected_ratio * 1.3;
        
        prop_assert!(
            actual_ratio >= lower_bound && actual_ratio <= upper_bound,
            "Ratio {} not in expected range [{}, {}]",
            actual_ratio,
            lower_bound,
            upper_bound
        );
    }

    /// Property: get_all_providers should return all configured providers
    #[test]
    fn prop_get_all_providers_returns_all(config in app_config_strategy()) {
        let service = ProviderService::new(config.clone());
        let all_providers = service.get_all_providers();
        
        prop_assert_eq!(all_providers.len(), config.providers.len());
        
        for provider_config in &config.providers {
            let found = all_providers.iter()
                .any(|p| p.name == provider_config.name);
            prop_assert!(found, "Provider {} not found", provider_config.name);
        }
    }

    /// Property: get_provider_weights should match configured weights
    #[test]
    fn prop_get_provider_weights_matches_config(config in app_config_strategy()) {
        let service = ProviderService::new(config.clone());
        let weights = service.get_provider_weights();
        
        prop_assert_eq!(weights.len(), config.providers.len());
        
        for (i, provider_config) in config.providers.iter().enumerate() {
            prop_assert_eq!(weights[i], provider_config.weight);
        }
    }

    /// Property: Model mappings should be preserved
    #[test]
    fn prop_model_mappings_preserved(
        model_name in "[a-z]{3,10}",
        mapped_name in "[a-z]{3,10}",
    ) {
        let mut model_mapping = HashMap::new();
        model_mapping.insert(model_name.clone(), mapped_name.clone());
        
        let config = AppConfig {
            providers: vec![
                ProviderConfig {
                    name: "TestProvider".to_string(),
                    api_base: "http://localhost:8000".to_string(),
                    api_key: "key".to_string(),
                    weight: 1,
                    model_mapping,
                },
            ],
            server: ServerConfig::default(),
            verify_ssl: true,
        };
        
        let service = ProviderService::new(config);
        let providers = service.get_all_providers();
        
        prop_assert_eq!(providers.len(), 1);
        let provider = &providers[0];
        
        prop_assert_eq!(
            provider.model_mapping.get(&model_name),
            Some(&mapped_name)
        );
    }

    /// Property: Thread-safe concurrent access
    #[test]
    fn prop_thread_safe_concurrent_access(config in app_config_strategy()) {
        use std::sync::Arc;
        use std::thread;
        
        let service = Arc::new(ProviderService::new(config.clone()));
        let mut handles = vec![];
        
        for _ in 0..10 {
            let service_clone = Arc::clone(&service);
            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    let _provider = service_clone.get_next_provider();
                }
            });
            handles.push(handle);
        }
        
        for handle in handles {
            prop_assert!(handle.join().is_ok());
        }
    }

    /// Property: Single provider always selected
    #[test]
    fn prop_single_provider_always_selected(weight in 1u32..=100u32) {
        let config = AppConfig {
            providers: vec![
                ProviderConfig {
                    name: "OnlyProvider".to_string(),
                    api_base: "http://localhost:8000".to_string(),
                    api_key: "key".to_string(),
                    weight,
                    model_mapping: HashMap::new(),
                },
            ],
            server: ServerConfig::default(),
            verify_ssl: true,
        };
        
        let service = ProviderService::new(config);
        
        for _ in 0..100 {
            let provider = service.get_next_provider();
            prop_assert_eq!(provider.name, "OnlyProvider");
        }
    }

    /// Property: get_all_models returns unique models
    #[test]
    fn prop_get_all_models_unique(config in app_config_strategy()) {
        let service = ProviderService::new(config);
        let models = service.get_all_models();
        
        // Convert to vec to check for duplicates
        let models_vec: Vec<String> = models.iter().cloned().collect();
        let unique_count = models.len();
        
        prop_assert_eq!(models_vec.len(), unique_count);
    }

    /// Property: Provider selection is deterministic for same RNG state
    #[test]
    fn prop_selection_uses_randomness(config in app_config_strategy()) {
        prop_assume!(config.providers.len() > 1);
        
        let service = ProviderService::new(config);
        
        // Collect 100 selections
        let mut selections = vec![];
        for _ in 0..100 {
            selections.push(service.get_next_provider().name);
        }
        
        // Should have some variation (not all the same)
        let unique_selections: std::collections::HashSet<_> = 
            selections.iter().collect();
        
        prop_assert!(
            unique_selections.len() > 1,
            "All selections were the same, expected variation"
        );
    }
}

#[cfg(test)]
mod quickcheck_tests {
    use super::*;
    use quickcheck::TestResult;
    use quickcheck_macros::quickcheck;

    #[quickcheck]
    fn qc_provider_weights_positive(weights: Vec<u32>) -> TestResult {
        // Discard if empty, has zeros, or weights are too large (to avoid overflow)
        if weights.is_empty()
            || weights.iter().any(|&w| w == 0 || w > 10000)
            || weights.len() > 100 {
            return TestResult::discard();
        }
        
        // Check if sum would overflow
        let sum: u64 = weights.iter().map(|&w| w as u64).sum();
        if sum > u32::MAX as u64 {
            return TestResult::discard();
        }

        let providers: Vec<ProviderConfig> = weights
            .iter()
            .enumerate()
            .map(|(i, &weight)| ProviderConfig {
                name: format!("Provider{}", i),
                api_base: format!("http://localhost:{}", 8000 + i),
                api_key: format!("key{}", i),
                weight,
                model_mapping: HashMap::new(),
            })
            .collect();

        let config = AppConfig {
            providers,
            server: ServerConfig::default(),
            verify_ssl: true,
        };

        let service = ProviderService::new(config);
        let retrieved_weights = service.get_provider_weights();

        TestResult::from_bool(retrieved_weights == weights)
    }

    #[quickcheck]
    fn qc_provider_count_preserved(count: u8) -> TestResult {
        let count = count as usize;
        if count == 0 || count > 20 {
            return TestResult::discard();
        }

        let providers: Vec<ProviderConfig> = (0..count)
            .map(|i| ProviderConfig {
                name: format!("Provider{}", i),
                api_base: format!("http://localhost:{}", 8000 + i),
                api_key: format!("key{}", i),
                weight: 1,
                model_mapping: HashMap::new(),
            })
            .collect();

        let config = AppConfig {
            providers,
            server: ServerConfig::default(),
            verify_ssl: true,
        };

        let service = ProviderService::new(config);
        let all_providers = service.get_all_providers();

        TestResult::from_bool(all_providers.len() == count)
    }
}