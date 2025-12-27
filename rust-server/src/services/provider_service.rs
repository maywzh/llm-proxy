//! Provider selection and management service.
//!
//! This module implements weighted round-robin selection of LLM providers
//! with thread-safe state management.

use crate::api::models::Provider;
use crate::core::config::AppConfig;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;

/// Service for managing and selecting LLM providers.
///
/// Uses weighted random selection to distribute requests across providers
/// based on their configured weights.
#[derive(Clone)]
pub struct ProviderService {
    providers: Arc<Vec<Provider>>,
    weights: Arc<Vec<u32>>,
    weighted_index: Arc<WeightedIndex<u32>>,
}

impl ProviderService {
    /// Create a new provider service from configuration.
    ///
    /// # Arguments
    ///
    /// * `config` - Application configuration containing provider definitions
    ///
    /// # Panics
    ///
    /// Panics if the weighted index cannot be created (e.g., all weights are zero)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use llm_proxy_rust::core::config::AppConfig;
    /// use llm_proxy_rust::services::ProviderService;
    ///
    /// let config = AppConfig::load("config.yaml").unwrap();
    /// let service = ProviderService::new(config);
    /// ```
    pub fn new(config: AppConfig) -> Self {
        let providers: Vec<Provider> = config
            .providers
            .into_iter()
            .map(|p| Provider {
                name: p.name,
                api_base: p.api_base,
                api_key: p.api_key,
                weight: p.weight,
                model_mapping: p.model_mapping,
            })
            .collect();

        let weights: Vec<u32> = providers.iter().map(|p| p.weight).collect();
        let weighted_index = WeightedIndex::new(&weights).expect("Failed to create weighted index");

        Self {
            providers: Arc::new(providers),
            weights: Arc::new(weights),
            weighted_index: Arc::new(weighted_index),
        }
    }

    /// Get the next provider using weighted random selection.
    ///
    /// This method is thread-safe and can be called concurrently.
    /// Returns a clone of the provider to avoid holding locks.
    ///
    /// # Arguments
    ///
    /// * `model` - Optional model name to filter providers that support it
    ///
    /// # Returns
    ///
    /// Selected provider
    ///
    /// # Errors
    ///
    /// Returns error if model is specified but no provider supports it
    pub fn get_next_provider(&self, model: Option<&str>) -> Result<Provider, String> {
        let mut rng = thread_rng();

        // If no model specified, use all providers with original weights
        if model.is_none() {
            let index = self.weighted_index.sample(&mut rng);
            return Ok(self.providers[index].clone());
        }

        let model_name = model.unwrap();

        // Filter providers that have the requested model
        let mut available_providers = Vec::new();
        let mut available_weights = Vec::new();

        for (provider, &weight) in self.providers.iter().zip(self.weights.iter()) {
            if provider.model_mapping.contains_key(model_name) {
                available_providers.push(provider.clone());
                available_weights.push(weight);
            }
        }

        // If no provider has the model, return error
        if available_providers.is_empty() {
            return Err(format!("No provider supports model: {}", model_name));
        }

        // Create weighted index for available providers
        let weighted_index = WeightedIndex::new(&available_weights)
            .map_err(|e| format!("Failed to create weighted index: {}", e))?;

        let index = weighted_index.sample(&mut rng);
        Ok(available_providers[index].clone())
    }

    /// Get all configured providers.
    ///
    /// Returns a vector of provider clones.
    pub fn get_all_providers(&self) -> Vec<Provider> {
        (*self.providers).clone()
    }

    /// Get provider weights.
    ///
    /// Returns a vector of weights corresponding to each provider.
    pub fn get_provider_weights(&self) -> Vec<u32> {
        (*self.weights).clone()
    }

    /// Get all unique model names across all providers.
    ///
    /// Returns a set of model identifiers that can be requested.
    pub fn get_all_models(&self) -> HashSet<String> {
        let mut models = HashSet::new();
        for provider in self.providers.iter() {
            for model in provider.model_mapping.keys() {
                models.insert(model.clone());
            }
        }
        models
    }

    /// Log provider configuration at startup.
    ///
    /// Outputs provider names, weights, and selection probabilities.
    pub fn log_providers(&self) {
        let total_weight: u32 = self.weights.iter().sum();
        tracing::info!(
            "Starting LLM API Proxy with {} providers",
            self.providers.len()
        );
        for (i, provider) in self.providers.iter().enumerate() {
            let weight = self.weights[i];
            let probability = (weight as f64 / total_weight as f64) * 100.0;
            tracing::info!(
                "  - {}: weight={} ({:.1}%)",
                provider.name,
                weight,
                probability
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::{ProviderConfig, ServerConfig};
    use std::collections::HashMap;

    fn create_test_config() -> AppConfig {
        AppConfig {
            providers: vec![
                ProviderConfig {
                    name: "Provider1".to_string(),
                    api_base: "http://localhost:8000".to_string(),
                    api_key: "key1".to_string(),
                    weight: 2,
                    model_mapping: {
                        let mut map = HashMap::new();
                        map.insert("model1".to_string(), "provider1-model1".to_string());
                        map
                    },
                },
                ProviderConfig {
                    name: "Provider2".to_string(),
                    api_base: "http://localhost:8001".to_string(),
                    api_key: "key2".to_string(),
                    weight: 1,
                    model_mapping: {
                        let mut map = HashMap::new();
                        map.insert("model2".to_string(), "provider2-model2".to_string());
                        map
                    },
                },
            ],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            master_keys: vec![],
        }
    }

    fn create_single_provider_config() -> AppConfig {
        AppConfig {
            providers: vec![ProviderConfig {
                name: "OnlyProvider".to_string(),
                api_base: "http://localhost:8000".to_string(),
                api_key: "key".to_string(),
                weight: 1,
                model_mapping: HashMap::new(),
            }],
            server: ServerConfig::default(),
            verify_ssl: true,
            request_timeout_secs: 300,
            master_keys: vec![],
        }
    }

    #[test]
    fn test_provider_service_creation() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        assert_eq!(service.get_all_providers().len(), 2);
        assert_eq!(service.get_provider_weights(), vec![2, 1]);
    }

    #[test]
    fn test_weighted_selection() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        // Test that selection works
        let provider = service.get_next_provider(None).unwrap();
        assert!(provider.name == "Provider1" || provider.name == "Provider2");
    }

    #[test]
    fn test_weighted_selection_distribution() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let mut provider1_count = 0;
        let mut provider2_count = 0;

        // Sample 1000 times to check distribution
        for _ in 0..1000 {
            let provider = service.get_next_provider(None).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }

        // Provider1 has weight 2, Provider2 has weight 1
        // So Provider1 should be selected roughly 2x as often
        // Allow for some variance (between 1.5x and 3x)
        let ratio = provider1_count as f64 / provider2_count as f64;
        assert!(ratio > 1.5 && ratio < 3.0, "Ratio was {}", ratio);
    }

    #[test]
    fn test_single_provider_always_selected() {
        let config = create_single_provider_config();
        let service = ProviderService::new(config);

        for _ in 0..100 {
            let provider = service.get_next_provider(None).unwrap();
            assert_eq!(provider.name, "OnlyProvider");
        }
    }

    #[test]
    fn test_get_all_models() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let models = service.get_all_models();
        assert_eq!(models.len(), 2);
        assert!(models.contains("model1"));
        assert!(models.contains("model2"));
    }

    #[test]
    fn test_get_all_models_empty() {
        let config = create_single_provider_config();
        let service = ProviderService::new(config);

        let models = service.get_all_models();
        assert_eq!(models.len(), 0);
    }

    #[test]
    fn test_get_all_models_with_duplicates() {
        let mut config = create_test_config();
        // Add same model to both providers
        config.providers[0]
            .model_mapping
            .insert("shared-model".to_string(), "provider1-shared".to_string());
        config.providers[1]
            .model_mapping
            .insert("shared-model".to_string(), "provider2-shared".to_string());

        let service = ProviderService::new(config);
        let models = service.get_all_models();

        // Should only have 3 unique models (model1, model2, shared-model)
        assert_eq!(models.len(), 3);
        assert!(models.contains("shared-model"));
    }

    #[test]
    fn test_provider_cloning() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let provider1 = service.get_next_provider(None).unwrap();
        let provider2 = service.get_next_provider(None).unwrap();

        // Providers should be independent clones
        assert!(provider1.name == "Provider1" || provider1.name == "Provider2");
        assert!(provider2.name == "Provider1" || provider2.name == "Provider2");
    }

    #[test]
    fn test_provider_fields() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let providers = service.get_all_providers();

        for provider in providers {
            assert!(!provider.name.is_empty());
            assert!(!provider.api_base.is_empty());
            assert!(!provider.api_key.is_empty());
            assert!(provider.weight > 0);
        }
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        let config = create_test_config();
        let service = Arc::new(ProviderService::new(config));

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let service = Arc::clone(&service);
                thread::spawn(move || {
                    let _provider = service.get_next_provider(None).unwrap();
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let config = create_test_config();
        let service = Arc::new(ProviderService::new(config));

        let handles: Vec<_> = (0..100)
            .map(|_| {
                let service = Arc::clone(&service);
                thread::spawn(move || {
                    for _ in 0..10 {
                        let provider = service.get_next_provider(None).unwrap();
                        assert!(provider.name == "Provider1" || provider.name == "Provider2");
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }

    #[test]
    fn test_equal_weights() {
        let mut config = create_test_config();
        config.providers[0].weight = 1;
        config.providers[1].weight = 1;

        let service = ProviderService::new(config);

        let mut provider1_count = 0;
        let mut provider2_count = 0;

        for _ in 0..1000 {
            let provider = service.get_next_provider(None).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }

        // With equal weights, distribution should be roughly 50/50
        let ratio = provider1_count as f64 / provider2_count as f64;
        assert!(ratio > 0.8 && ratio < 1.2, "Ratio was {}", ratio);
    }

    #[test]
    fn test_high_weight_difference() {
        let mut config = create_test_config();
        config.providers[0].weight = 10;
        config.providers[1].weight = 1;

        let service = ProviderService::new(config);

        let mut provider1_count = 0;

        for _ in 0..1000 {
            let provider = service.get_next_provider(None).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            }
        }

        // Provider1 should be selected roughly 90% of the time
        let percentage = (provider1_count as f64 / 1000.0) * 100.0;
        assert!(
            percentage > 80.0 && percentage < 95.0,
            "Percentage was {}",
            percentage
        );
    }

    #[test]
    fn test_model_mapping_preserved() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let providers = service.get_all_providers();

        let provider1 = providers.iter().find(|p| p.name == "Provider1").unwrap();
        assert_eq!(
            provider1.model_mapping.get("model1").unwrap(),
            "provider1-model1"
        );

        let provider2 = providers.iter().find(|p| p.name == "Provider2").unwrap();
        assert_eq!(
            provider2.model_mapping.get("model2").unwrap(),
            "provider2-model2"
        );
    }

    #[test]
    fn test_get_next_provider_with_model() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        // Request model1 which only Provider1 has
        for _ in 0..10 {
            let provider = service.get_next_provider(Some("model1")).unwrap();
            assert_eq!(provider.name, "Provider1");
            assert!(provider.model_mapping.contains_key("model1"));
        }
    }

    #[test]
    fn test_get_next_provider_with_nonexistent_model() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        // Request a model that no provider has
        let result = service.get_next_provider(Some("nonexistent-model"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No provider supports model"));
    }

    #[test]
    fn test_get_next_provider_without_model_uses_all_providers() {
        let config = create_test_config();
        let service = ProviderService::new(config);

        let mut provider1_count = 0;
        let mut provider2_count = 0;

        for _ in 0..1000 {
            let provider = service.get_next_provider(None).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }

        // Both providers should be selected
        assert!(provider1_count > 0);
        assert!(provider2_count > 0);

        // Should follow weight distribution (2:1 ratio)
        let ratio = provider1_count as f64 / provider2_count as f64;
        assert!(ratio > 1.5 && ratio < 2.5, "Ratio was {}", ratio);
    }

    #[test]
    fn test_get_next_provider_with_shared_model() {
        // Create config where both providers have the same model
        let mut config = create_test_config();
        config.providers[0]
            .model_mapping
            .insert("shared-model".to_string(), "provider1-shared".to_string());
        config.providers[1]
            .model_mapping
            .insert("shared-model".to_string(), "provider2-shared".to_string());

        let service = ProviderService::new(config);

        let mut provider1_count = 0;
        let mut provider2_count = 0;

        // Request the shared model many times
        for _ in 0..1000 {
            let provider = service.get_next_provider(Some("shared-model")).unwrap();
            if provider.name == "Provider1" {
                provider1_count += 1;
            } else {
                provider2_count += 1;
            }
        }

        // Both providers should be selected
        assert!(provider1_count > 0);
        assert!(provider2_count > 0);

        // Should follow weight distribution (2:1 ratio)
        let ratio = provider1_count as f64 / provider2_count as f64;
        assert!(ratio > 1.5 && ratio < 2.5, "Ratio was {}", ratio);
    }
}
