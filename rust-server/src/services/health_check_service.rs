//! Health check service for testing provider availability.
//!
//! This module implements health checking by making actual API calls
//! to providers with minimal token usage to verify their availability.

use crate::api::health::{HealthStatus, ModelHealthStatus, ProviderHealthStatus};
use crate::core::database::{Database, ProviderEntity};
use chrono::Utc;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tokio::time::{timeout, Duration};

/// Service for checking provider health
pub struct HealthCheckService {
    client: Client,
    timeout_secs: u64,
}

impl HealthCheckService {
    /// Create a new health check service
    pub fn new(timeout_secs: u64) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            timeout_secs,
        }
    }

    /// Check health of a single provider
    ///
    /// # Arguments
    ///
    /// * `provider` - Provider entity to check
    /// * `models` - Optional list of models to test (None = use default test models)
    ///
    /// # Returns
    ///
    /// Provider health status with test results
    pub async fn check_provider_health(
        &self,
        provider: &ProviderEntity,
        models: Option<Vec<String>>,
    ) -> ProviderHealthStatus {
        // Check if provider is disabled
        if !provider.is_enabled {
            return ProviderHealthStatus {
                provider_id: provider.id,
                provider_key: provider.provider_key.clone(),
                status: HealthStatus::Disabled,
                models: vec![],
                avg_response_time_ms: None,
                checked_at: Utc::now().to_rfc3339(),
            };
        }

        // Determine which models to test
        let test_models = models.unwrap_or_else(|| self.get_default_test_models(provider));

        // Test each model sequentially (not concurrently) to avoid overwhelming the provider
        let mut model_statuses = Vec::new();
        for model in test_models {
            let status = Self::test_model(&self.client, provider, &model, self.timeout_secs).await;
            model_statuses.push(status);
        }

        // Determine overall provider status
        let provider_status = self.determine_provider_status(&model_statuses);

        // Calculate average response time
        let avg_response_time = self.calculate_avg_response_time(&model_statuses);

        ProviderHealthStatus {
            provider_id: provider.id,
            provider_key: provider.provider_key.clone(),
            status: provider_status,
            models: model_statuses,
            avg_response_time_ms: avg_response_time,
            checked_at: Utc::now().to_rfc3339(),
        }
    }

    /// Test a single model on a provider
    ///
    /// # Arguments
    ///
    /// * `client` - HTTP client
    /// * `provider` - Provider to test
    /// * `model` - Model name to test
    /// * `timeout_secs` - Timeout in seconds
    ///
    /// # Returns
    ///
    /// Model health status with test result
    async fn test_model(
        client: &Client,
        provider: &ProviderEntity,
        model: &str,
        timeout_secs: u64,
    ) -> ModelHealthStatus {
        // Get the actual model name from mapping
        let actual_model = provider
            .model_mapping
            .0
            .get(model)
            .map(|s| s.as_str())
            .unwrap_or(model);

        // Prepare test request (minimal tokens to reduce cost)
        let test_payload = json!({
            "model": actual_model,
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 5,
            "temperature": 0,
            "stream": false,
        });

        let start_time = Instant::now();

        // Make the request with timeout
        let result = timeout(
            Duration::from_secs(timeout_secs),
            client
                .post(format!("{}/chat/completions", provider.api_base))
                .json(&test_payload)
                .header("Authorization", format!("Bearer {}", provider.api_key))
                .header("Content-Type", "application/json")
                .send(),
        )
        .await;

        let elapsed_ms = start_time.elapsed().as_millis() as i32;

        match result {
            Ok(Ok(response)) => {
                let status_code = response.status();
                if status_code.is_success() {
                    ModelHealthStatus {
                        model: model.to_string(),
                        status: HealthStatus::Healthy,
                        response_time_ms: Some(elapsed_ms),
                        error: None,
                    }
                } else {
                    let error_msg = match response.json::<serde_json::Value>().await {
                        Ok(json) => {
                            if let Some(error) = json.get("error") {
                                if let Some(message) = error.get("message") {
                                    message.as_str().unwrap_or("Unknown error").to_string()
                                } else {
                                    format!("HTTP {}", status_code.as_u16())
                                }
                            } else {
                                format!("HTTP {}", status_code.as_u16())
                            }
                        }
                        Err(_) => format!("HTTP {}", status_code.as_u16()),
                    };

                    ModelHealthStatus {
                        model: model.to_string(),
                        status: HealthStatus::Unhealthy,
                        response_time_ms: Some(elapsed_ms),
                        error: Some(error_msg),
                    }
                }
            }
            Ok(Err(e)) => {
                let error_msg = e.to_string();
                // Don't expose sensitive information
                let safe_error = if error_msg.to_lowercase().contains("api")
                    && error_msg.to_lowercase().contains("key")
                {
                    "Authentication error".to_string()
                } else {
                    error_msg
                };

                ModelHealthStatus {
                    model: model.to_string(),
                    status: HealthStatus::Unhealthy,
                    response_time_ms: Some(elapsed_ms),
                    error: Some(safe_error),
                }
            }
            Err(_) => ModelHealthStatus {
                model: model.to_string(),
                status: HealthStatus::Unhealthy,
                response_time_ms: Some(elapsed_ms),
                error: Some(format!("Timeout after {}s", timeout_secs)),
            },
        }
    }

    /// Get default models to test for a provider
    ///
    /// # Arguments
    ///
    /// * `provider` - Provider to get test models for
    ///
    /// # Returns
    ///
    /// List of model names to test
    fn get_default_test_models(&self, provider: &ProviderEntity) -> Vec<String> {
        let model_mapping = &provider.model_mapping.0;

        // If provider has model mapping, test all configured models
        if !model_mapping.is_empty() {
            return model_mapping.keys().cloned().collect();
        }

        // Otherwise, use common model names based on provider type
        let provider_type = provider.provider_type.to_lowercase();

        if provider_type.contains("openai") {
            vec!["gpt-3.5-turbo".to_string()]
        } else if provider_type.contains("anthropic") {
            vec!["claude-3-haiku-20240307".to_string()]
        } else if provider_type.contains("azure") {
            vec!["gpt-35-turbo".to_string()]
        } else {
            // Generic fallback
            vec!["gpt-3.5-turbo".to_string()]
        }
    }

    /// Determine overall provider status from model statuses
    ///
    /// # Arguments
    ///
    /// * `model_statuses` - List of model health statuses
    ///
    /// # Returns
    ///
    /// Overall provider health status
    fn determine_provider_status(&self, model_statuses: &[ModelHealthStatus]) -> HealthStatus {
        if model_statuses.is_empty() {
            return HealthStatus::Unknown;
        }

        // If all models are healthy, provider is healthy
        if model_statuses
            .iter()
            .all(|m| m.status == HealthStatus::Healthy)
        {
            return HealthStatus::Healthy;
        }

        // If any model is healthy, provider is partially healthy (still mark as healthy)
        if model_statuses
            .iter()
            .any(|m| m.status == HealthStatus::Healthy)
        {
            return HealthStatus::Healthy;
        }

        // Otherwise, provider is unhealthy
        HealthStatus::Unhealthy
    }

    /// Calculate average response time from model statuses
    ///
    /// # Arguments
    ///
    /// * `model_statuses` - List of model health statuses
    ///
    /// # Returns
    ///
    /// Average response time in milliseconds, or None if no valid times
    fn calculate_avg_response_time(&self, model_statuses: &[ModelHealthStatus]) -> Option<i32> {
        let valid_times: Vec<i32> = model_statuses
            .iter()
            .filter_map(|m| m.response_time_ms)
            .collect();

        if valid_times.is_empty() {
            return None;
        }

        let sum: i32 = valid_times.iter().sum();
        Some(sum / valid_times.len() as i32)
    }
}

/// Check health of multiple providers with controlled concurrency
///
/// # Arguments
///
/// * `db` - Database instance
/// * `provider_ids` - Optional list of provider IDs to check (None = all enabled providers)
/// * `models` - Optional list of models to test (None = default test models)
/// * `timeout_secs` - Timeout for each model test
/// * `max_concurrent` - Maximum number of providers to check concurrently (default: 2)
///
/// # Returns
///
/// List of provider health statuses
pub async fn check_providers_health(
    db: &Arc<Database>,
    provider_ids: Option<Vec<i32>>,
    models: Option<Vec<String>>,
    timeout_secs: u64,
    max_concurrent: usize,
) -> Vec<ProviderHealthStatus> {
    // Get providers to check
    let providers = if let Some(ids) = provider_ids {
        let mut result = Vec::new();
        for id in ids {
            if let Ok(Some(provider)) = db.get_provider(id).await {
                result.push(provider);
            }
        }
        result
    } else {
        // Get all providers (including disabled for status reporting)
        db.load_all_providers().await.unwrap_or_default()
    };

    // Use semaphore to limit concurrent provider checks
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
    
    // Check all providers with controlled concurrency
    let mut tasks = Vec::new();
    for provider in providers {
        let service_clone = HealthCheckService::new(timeout_secs);
        let models_clone = models.clone();
        let sem = semaphore.clone();
        
        tasks.push(tokio::spawn(async move {
            // Acquire semaphore permit before checking
            let _permit = sem.acquire().await.unwrap();
            service_clone
                .check_provider_health(&provider, models_clone)
                .await
        }));
    }

    // Wait for all tasks to complete
    let results = futures::future::join_all(tasks).await;

    // Filter out errors and collect results
    let mut health_statuses = Vec::new();
    for result in results {
        match result {
            Ok(status) => health_statuses.push(status),
            Err(e) => {
                tracing::error!(error = %e, "Error checking provider health");
            }
        }
    }

    health_statuses
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn create_test_provider() -> ProviderEntity {
        ProviderEntity {
            id: 1,
            provider_key: "test-provider".to_string(),
            provider_type: "openai".to_string(),
            api_base: "http://localhost:8000".to_string(),
            api_key: "test-key".to_string(),
            model_mapping: sqlx::types::Json(HashMap::new()),
            weight: 1,
            is_enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_get_default_test_models_with_mapping() {
        let service = HealthCheckService::new(10);
        let mut provider = create_test_provider();
        let mut mapping = HashMap::new();
        mapping.insert("model1".to_string(), "provider-model1".to_string());
        mapping.insert("model2".to_string(), "provider-model2".to_string());
        mapping.insert("model3".to_string(), "provider-model3".to_string());
        mapping.insert("model4".to_string(), "provider-model4".to_string());
        provider.model_mapping = sqlx::types::Json(mapping);

        let models = service.get_default_test_models(&provider);
        assert_eq!(models.len(), 4);
        let model_set: std::collections::HashSet<_> = models.into_iter().collect();
        assert_eq!(
            model_set,
            ["model1", "model2", "model3", "model4"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        );
    }

    #[test]
    fn test_get_default_test_models_openai() {
        let service = HealthCheckService::new(10);
        let provider = create_test_provider();

        let models = service.get_default_test_models(&provider);
        assert_eq!(models, vec!["gpt-3.5-turbo"]);
    }

    #[test]
    fn test_get_default_test_models_anthropic() {
        let service = HealthCheckService::new(10);
        let mut provider = create_test_provider();
        provider.provider_type = "anthropic".to_string();

        let models = service.get_default_test_models(&provider);
        assert_eq!(models, vec!["claude-3-haiku-20240307"]);
    }

    #[test]
    fn test_get_default_test_models_azure() {
        let service = HealthCheckService::new(10);
        let mut provider = create_test_provider();
        provider.provider_type = "azure".to_string();

        let models = service.get_default_test_models(&provider);
        assert_eq!(models, vec!["gpt-35-turbo"]);
    }

    #[test]
    fn test_determine_provider_status_all_healthy() {
        let service = HealthCheckService::new(10);
        let statuses = vec![
            ModelHealthStatus {
                model: "model1".to_string(),
                status: HealthStatus::Healthy,
                response_time_ms: Some(100),
                error: None,
            },
            ModelHealthStatus {
                model: "model2".to_string(),
                status: HealthStatus::Healthy,
                response_time_ms: Some(200),
                error: None,
            },
        ];

        assert_eq!(
            service.determine_provider_status(&statuses),
            HealthStatus::Healthy
        );
    }

    #[test]
    fn test_determine_provider_status_partially_healthy() {
        let service = HealthCheckService::new(10);
        let statuses = vec![
            ModelHealthStatus {
                model: "model1".to_string(),
                status: HealthStatus::Healthy,
                response_time_ms: Some(100),
                error: None,
            },
            ModelHealthStatus {
                model: "model2".to_string(),
                status: HealthStatus::Unhealthy,
                response_time_ms: Some(200),
                error: Some("Error".to_string()),
            },
        ];

        assert_eq!(
            service.determine_provider_status(&statuses),
            HealthStatus::Healthy
        );
    }

    #[test]
    fn test_determine_provider_status_all_unhealthy() {
        let service = HealthCheckService::new(10);
        let statuses = vec![
            ModelHealthStatus {
                model: "model1".to_string(),
                status: HealthStatus::Unhealthy,
                response_time_ms: Some(100),
                error: Some("Error".to_string()),
            },
            ModelHealthStatus {
                model: "model2".to_string(),
                status: HealthStatus::Unhealthy,
                response_time_ms: Some(200),
                error: Some("Error".to_string()),
            },
        ];

        assert_eq!(
            service.determine_provider_status(&statuses),
            HealthStatus::Unhealthy
        );
    }

    #[test]
    fn test_determine_provider_status_empty() {
        let service = HealthCheckService::new(10);
        let statuses = vec![];

        assert_eq!(
            service.determine_provider_status(&statuses),
            HealthStatus::Unknown
        );
    }

    #[test]
    fn test_calculate_avg_response_time() {
        let service = HealthCheckService::new(10);
        let statuses = vec![
            ModelHealthStatus {
                model: "model1".to_string(),
                status: HealthStatus::Healthy,
                response_time_ms: Some(100),
                error: None,
            },
            ModelHealthStatus {
                model: "model2".to_string(),
                status: HealthStatus::Healthy,
                response_time_ms: Some(200),
                error: None,
            },
            ModelHealthStatus {
                model: "model3".to_string(),
                status: HealthStatus::Healthy,
                response_time_ms: Some(300),
                error: None,
            },
        ];

        assert_eq!(service.calculate_avg_response_time(&statuses), Some(200));
    }

    #[test]
    fn test_calculate_avg_response_time_with_none() {
        let service = HealthCheckService::new(10);
        let statuses = vec![
            ModelHealthStatus {
                model: "model1".to_string(),
                status: HealthStatus::Healthy,
                response_time_ms: Some(100),
                error: None,
            },
            ModelHealthStatus {
                model: "model2".to_string(),
                status: HealthStatus::Unhealthy,
                response_time_ms: None,
                error: Some("Error".to_string()),
            },
        ];

        assert_eq!(service.calculate_avg_response_time(&statuses), Some(100));
    }

    #[test]
    fn test_calculate_avg_response_time_empty() {
        let service = HealthCheckService::new(10);
        let statuses = vec![];

        assert_eq!(service.calculate_avg_response_time(&statuses), None);
    }

    #[tokio::test]
    async fn test_check_provider_health_disabled() {
        let service = HealthCheckService::new(10);
        let mut provider = create_test_provider();
        provider.is_enabled = false;

        let result = service.check_provider_health(&provider, None).await;

        assert_eq!(result.status, HealthStatus::Disabled);
        assert_eq!(result.models.len(), 0);
        assert_eq!(result.avg_response_time_ms, None);
    }
}