//! Health check service for testing provider availability.
//!
//! This module implements health checking by making actual API calls
//! to providers with minimal token usage to verify their availability.

use crate::api::health::{HealthStatus, ModelHealthStatus, ProviderHealthStatus};
use crate::api::models::{CheckProviderHealthResponse, ModelHealthResult, ProviderHealthSummary};
use crate::api::upstream::{
    build_gcp_vertex_url_with_actions, build_upstream_request, extract_error_message, UpstreamAuth,
};
use crate::core::database::{Database, ProviderEntity};
use chrono::Utc;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};

/// Service for checking provider health
pub struct HealthCheckService {
    client: Client,
    timeout_secs: u64,
}

impl HealthCheckService {
    /// Create a new health check service
    pub fn new(client: Client, timeout_secs: u64) -> Self {
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
                provider_type: provider.provider_type.clone(),
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
            provider_type: provider.provider_type.clone(),
            status: provider_status,
            models: model_statuses,
            avg_response_time_ms: avg_response_time,
            checked_at: Utc::now().to_rfc3339(),
        }
    }

    /// Check health of a single provider with concurrent model testing
    ///
    /// # Arguments
    ///
    /// * `provider` - Provider entity to check
    /// * `models` - Optional list of models to test (None = use all mapped models)
    /// * `max_concurrent` - Maximum number of models to test concurrently (default: 2)
    ///
    /// # Returns
    ///
    /// CheckProviderHealthResponse with test results and summary
    pub async fn check_provider_health_concurrent(
        &self,
        provider: &ProviderEntity,
        models: Option<Vec<String>>,
        max_concurrent: usize,
    ) -> CheckProviderHealthResponse {
        // Check if provider is disabled
        if !provider.is_enabled {
            return CheckProviderHealthResponse {
                provider_id: provider.id,
                provider_key: provider.provider_key.clone(),
                provider_type: provider.provider_type.clone(),
                status: "disabled".to_string(),
                models: vec![],
                summary: ProviderHealthSummary {
                    total_models: 0,
                    healthy_models: 0,
                    unhealthy_models: 0,
                },
                avg_response_time_ms: None,
                checked_at: Utc::now().to_rfc3339(),
            };
        }

        // Determine which models to test
        let test_models = models.unwrap_or_else(|| self.get_default_test_models(provider));

        if test_models.is_empty() {
            return CheckProviderHealthResponse {
                provider_id: provider.id,
                provider_key: provider.provider_key.clone(),
                provider_type: provider.provider_type.clone(),
                status: "unknown".to_string(),
                models: vec![],
                summary: ProviderHealthSummary {
                    total_models: 0,
                    healthy_models: 0,
                    unhealthy_models: 0,
                },
                avg_response_time_ms: None,
                checked_at: Utc::now().to_rfc3339(),
            };
        }

        // Use semaphore to limit concurrent model tests
        let semaphore = Arc::new(Semaphore::new(max_concurrent));

        // Create tasks for concurrent model testing
        let mut tasks = Vec::new();
        for model in test_models.clone() {
            let sem = semaphore.clone();
            let client = self.client.clone();
            let provider_clone = provider.clone();
            let timeout_secs = self.timeout_secs;

            tasks.push(tokio::spawn(async move {
                // Acquire semaphore permit before testing
                let _permit = sem.acquire().await.unwrap();
                Self::test_model(&client, &provider_clone, &model, timeout_secs).await
            }));
        }

        // Wait for all tasks to complete
        let results = futures::future::join_all(tasks).await;

        // Process results and convert to ModelHealthResult
        let mut model_results = Vec::new();
        let mut model_statuses = Vec::new();
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(status) => {
                    model_results.push(ModelHealthResult {
                        model: status.model.clone(),
                        status: match status.status {
                            HealthStatus::Healthy => "healthy".to_string(),
                            HealthStatus::Unhealthy => "unhealthy".to_string(),
                            HealthStatus::Disabled => "disabled".to_string(),
                            HealthStatus::Unknown => "unknown".to_string(),
                        },
                        response_time_ms: status.response_time_ms,
                        error: status.error.clone(),
                    });
                    model_statuses.push(status);
                }
                Err(e) => {
                    tracing::error!(error = %e, model = %test_models[i], "Error testing model");
                    let error_status = ModelHealthStatus {
                        model: test_models[i].clone(),
                        status: HealthStatus::Unhealthy,
                        response_time_ms: None,
                        error: Some(e.to_string()),
                    };
                    model_results.push(ModelHealthResult {
                        model: test_models[i].clone(),
                        status: "unhealthy".to_string(),
                        response_time_ms: None,
                        error: Some(e.to_string()),
                    });
                    model_statuses.push(error_status);
                }
            }
        }

        // Calculate summary statistics
        let healthy_count = model_statuses
            .iter()
            .filter(|m| m.status == HealthStatus::Healthy)
            .count();
        let unhealthy_count = model_statuses.len() - healthy_count;

        let summary = ProviderHealthSummary {
            total_models: model_statuses.len(),
            healthy_models: healthy_count,
            unhealthy_models: unhealthy_count,
        };

        // Determine overall provider status
        let provider_status = self.determine_provider_status(&model_statuses);
        let status_str = match provider_status {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Unhealthy => "unhealthy",
            HealthStatus::Disabled => "disabled",
            HealthStatus::Unknown => "unknown",
        };

        // Calculate average response time
        let avg_response_time = self.calculate_avg_response_time(&model_statuses);

        CheckProviderHealthResponse {
            provider_id: provider.id,
            provider_key: provider.provider_key.clone(),
            provider_type: provider.provider_type.clone(),
            status: status_str.to_string(),
            models: model_results,
            summary,
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
            .map(|v| v.mapped_model())
            .unwrap_or(model);

        let provider_type = provider.provider_type.to_lowercase();

        let custom_headers: Option<std::collections::HashMap<String, String>> = provider
            .provider_params
            .get("custom_headers")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let payload = json!({
            "model": actual_model,
            "messages": [{"role": "user", "content": "Hi"}],
            "max_tokens": 5,
            "stream": false,
        });

        // Build URL and request based on provider type
        let request_builder = if provider_type.contains("gcp-vertex")
            || provider_type.contains("gcp_vertex")
            || provider_type == "vertex"
        {
            let gcp_project = provider
                .provider_params
                .get("gcp_project")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let gcp_location = provider
                .provider_params
                .get("gcp_location")
                .and_then(|v| v.as_str())
                .unwrap_or("us-central1");
            let gcp_publisher = provider
                .provider_params
                .get("gcp_publisher")
                .and_then(|v| v.as_str())
                .unwrap_or("anthropic");

            let actions = provider.provider_params.get("gcp_vertex_actions");
            let blocking_action = actions
                .and_then(|v| v.get("blocking"))
                .and_then(|v| v.as_str())
                .unwrap_or("rawPredict");
            let streaming_action = actions
                .and_then(|v| v.get("streaming"))
                .and_then(|v| v.as_str())
                .unwrap_or("streamRawPredict");

            let url = match build_gcp_vertex_url_with_actions(
                &provider.api_base,
                gcp_project,
                gcp_location,
                gcp_publisher,
                actual_model,
                false,
                blocking_action,
                streaming_action,
            ) {
                Ok(url) => url,
                Err(err) => {
                    return ModelHealthStatus {
                        model: model.to_string(),
                        status: HealthStatus::Unhealthy,
                        response_time_ms: None,
                        error: Some(err),
                    };
                }
            };

            build_upstream_request(
                client,
                &url,
                &payload,
                UpstreamAuth::Bearer(&provider.api_key),
                Some("vertex-2023-10-16"),
                None,
                custom_headers.as_ref(),
            )
        } else if provider_type == "gemini" || provider_type == "gcp-gemini" {
            // Gemini: uses Gemini format (contents instead of messages)
            let gcp_project = provider
                .provider_params
                .get("gcp_project")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let gcp_location = provider
                .provider_params
                .get("gcp_location")
                .and_then(|v| v.as_str())
                .unwrap_or("us-central1");
            let gcp_publisher = provider
                .provider_params
                .get("gcp_publisher")
                .and_then(|v| v.as_str())
                .unwrap_or("google");

            let url = match build_gcp_vertex_url_with_actions(
                &provider.api_base,
                gcp_project,
                gcp_location,
                gcp_publisher,
                actual_model,
                false,
                "generateContent",
                "streamGenerateContent",
            ) {
                Ok(url) => url,
                Err(err) => {
                    return ModelHealthStatus {
                        model: model.to_string(),
                        status: HealthStatus::Unhealthy,
                        response_time_ms: None,
                        error: Some(err),
                    };
                }
            };

            let gemini_payload = json!({
                "contents": [{"role": "user", "parts": [{"text": "hi"}]}],
                "generationConfig": {"maxOutputTokens": 1}
            });

            build_upstream_request(
                client,
                &url,
                &gemini_payload,
                UpstreamAuth::Bearer(&provider.api_key),
                None,
                None,
                custom_headers.as_ref(),
            )
        } else if provider_type == "anthropic" || provider_type == "claude" {
            let url = format!("{}/v1/messages", provider.api_base);
            build_upstream_request(
                client,
                &url,
                &payload,
                UpstreamAuth::XApiKey(&provider.api_key),
                Some("2023-06-01"),
                None,
                custom_headers.as_ref(),
            )
        } else if provider_type == "response_api"
            || provider_type == "response-api"
            || provider_type == "responses"
        {
            let url = format!("{}/responses", provider.api_base);
            let response_api_payload = json!({
                "model": actual_model,
                "input": [{"role": "user", "content": "Hi"}],
                "max_output_tokens": 5,
                "stream": true,
            });
            build_upstream_request(
                client,
                &url,
                &response_api_payload,
                UpstreamAuth::Bearer(&provider.api_key),
                None,
                None,
                custom_headers.as_ref(),
            )
        } else {
            let url = format!("{}/chat/completions", provider.api_base);
            build_upstream_request(
                client,
                &url,
                &payload,
                UpstreamAuth::Bearer(&provider.api_key),
                None,
                None,
                custom_headers.as_ref(),
            )
        };

        let start_time = Instant::now();

        // Make the request with timeout
        let result = timeout(Duration::from_secs(timeout_secs), request_builder.send()).await;

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
                            let detail = extract_error_message(&json)
                                .unwrap_or_else(|| format!("HTTP {}", status_code.as_u16()));
                            format!("HTTP {} - {}", status_code.as_u16(), detail)
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
        } else if provider_type == "gemini" || provider_type == "gcp-gemini" {
            vec!["gemini-2.0-flash".to_string()]
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
/// * `client` - HTTP client to use for health checks
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
    client: &Client,
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
        // Get only enabled providers
        db.load_providers().await.unwrap_or_default()
    };

    // Use semaphore to limit concurrent provider checks
    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));

    // Check all providers with controlled concurrency
    let mut tasks = Vec::new();
    for provider in providers {
        let service_clone = HealthCheckService::new(client.clone(), timeout_secs);
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
    use crate::core::config::ModelMappingValue;
    use std::collections::HashMap;

    fn simple_mapping(entries: &[(&str, &str)]) -> HashMap<String, ModelMappingValue> {
        entries
            .iter()
            .map(|(k, v)| (k.to_string(), ModelMappingValue::Simple(v.to_string())))
            .collect()
    }

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
            provider_params: sqlx::types::Json(HashMap::new()),
            lua_script: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_get_default_test_models_with_mapping() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let mut provider = create_test_provider();
        provider.model_mapping = sqlx::types::Json(simple_mapping(&[
            ("model1", "provider-model1"),
            ("model2", "provider-model2"),
            ("model3", "provider-model3"),
            ("model4", "provider-model4"),
        ]));

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
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let provider = create_test_provider();

        let models = service.get_default_test_models(&provider);
        assert_eq!(models, vec!["gpt-3.5-turbo"]);
    }

    #[test]
    fn test_get_default_test_models_anthropic() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let mut provider = create_test_provider();
        provider.provider_type = "anthropic".to_string();

        let models = service.get_default_test_models(&provider);
        assert_eq!(models, vec!["claude-3-haiku-20240307"]);
    }

    #[test]
    fn test_get_default_test_models_azure() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let mut provider = create_test_provider();
        provider.provider_type = "azure".to_string();

        let models = service.get_default_test_models(&provider);
        assert_eq!(models, vec!["gpt-35-turbo"]);
    }

    #[test]
    fn test_determine_provider_status_all_healthy() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
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
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
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
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
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
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let statuses = vec![];

        assert_eq!(
            service.determine_provider_status(&statuses),
            HealthStatus::Unknown
        );
    }

    #[test]
    fn test_calculate_avg_response_time() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
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
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
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
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let statuses = vec![];

        assert_eq!(service.calculate_avg_response_time(&statuses), None);
    }

    #[tokio::test]
    async fn test_check_provider_health_disabled() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let mut provider = create_test_provider();
        provider.is_enabled = false;

        let result = service.check_provider_health(&provider, None).await;

        assert_eq!(result.status, HealthStatus::Disabled);
        assert_eq!(result.models.len(), 0);
        assert_eq!(result.avg_response_time_ms, None);
    }

    #[tokio::test]
    async fn test_check_provider_health_concurrent_disabled() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let mut provider = create_test_provider();
        provider.is_enabled = false;

        let result = service
            .check_provider_health_concurrent(&provider, None, 2)
            .await;

        assert_eq!(result.status, "disabled");
        assert_eq!(result.models.len(), 0);
        assert_eq!(result.summary.total_models, 0);
        assert_eq!(result.summary.healthy_models, 0);
        assert_eq!(result.summary.unhealthy_models, 0);
        assert_eq!(result.avg_response_time_ms, None);
    }

    #[tokio::test]
    async fn test_check_provider_health_concurrent_no_models() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let provider = create_test_provider();

        // Provider has no model mapping, so get_default_test_models returns fallback
        // models based on provider type. These will fail to connect (no real server),
        // so the result will be "unhealthy" rather than "unknown".
        let result = service
            .check_provider_health_concurrent(&provider, None, 2)
            .await;

        assert_eq!(result.status, "unhealthy");
        assert!(!result.models.is_empty());
        assert!(result.summary.total_models > 0);
    }

    #[test]
    fn test_get_default_test_models_empty_mapping_returns_fallback() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let provider = create_test_provider();

        let models = service.get_default_test_models(&provider);
        // No model_mapping → falls back to provider-type default
        assert!(!models.is_empty());
    }

    #[test]
    fn test_get_default_test_models_with_mapping_returns_keys() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let mut provider = create_test_provider();
        provider.model_mapping = sqlx::types::Json(simple_mapping(&[
            ("gpt-4", "gpt-4-turbo"),
            ("gpt-3.5-turbo", "gpt-35-turbo"),
        ]));

        let models = service.get_default_test_models(&provider);
        assert_eq!(models.len(), 2);
        let model_set: std::collections::HashSet<_> = models.into_iter().collect();
        assert!(model_set.contains("gpt-4"));
        assert!(model_set.contains("gpt-3.5-turbo"));
    }

    #[test]
    fn test_check_provider_health_request_defaults() {
        use crate::api::models::CheckProviderHealthRequest;

        let request = CheckProviderHealthRequest::default();
        assert!(request.models.is_none());
        assert_eq!(request.max_concurrent, 2);
        assert_eq!(request.timeout_secs, 30);
    }

    #[test]
    fn test_provider_health_summary_creation() {
        let summary = ProviderHealthSummary {
            total_models: 5,
            healthy_models: 3,
            unhealthy_models: 2,
        };

        assert_eq!(summary.total_models, 5);
        assert_eq!(summary.healthy_models, 3);
        assert_eq!(summary.unhealthy_models, 2);
    }

    #[test]
    fn test_model_health_result_creation() {
        use crate::api::models::ModelHealthResult;

        let result = ModelHealthResult {
            model: "gpt-4".to_string(),
            status: "healthy".to_string(),
            response_time_ms: Some(150),
            error: None,
        };

        assert_eq!(result.model, "gpt-4");
        assert_eq!(result.status, "healthy");
        assert_eq!(result.response_time_ms, Some(150));
        assert!(result.error.is_none());
    }

    #[test]
    fn test_check_provider_health_response_creation() {
        let response = CheckProviderHealthResponse {
            provider_id: 1,
            provider_key: "test-provider".to_string(),
            provider_type: "openai".to_string(),
            status: "healthy".to_string(),
            models: vec![],
            summary: ProviderHealthSummary {
                total_models: 0,
                healthy_models: 0,
                unhealthy_models: 0,
            },
            avg_response_time_ms: None,
            checked_at: "2024-01-15T10:30:00Z".to_string(),
        };

        assert_eq!(response.provider_id, 1);
        assert_eq!(response.provider_key, "test-provider");
        assert_eq!(response.status, "healthy");
        assert!(response.models.is_empty());
    }

    // ========================================================================
    // Additional edge case tests
    // ========================================================================

    #[test]
    fn test_get_default_test_models_unknown_provider_type() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let mut provider = create_test_provider();
        provider.provider_type = "some-unknown-type".to_string();

        let models = service.get_default_test_models(&provider);
        // Fallback should return gpt-3.5-turbo
        assert_eq!(models, vec!["gpt-3.5-turbo"]);
    }

    #[test]
    fn test_get_default_test_models_case_insensitive() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);

        let mut provider = create_test_provider();
        provider.provider_type = "Anthropic".to_string();
        let models = service.get_default_test_models(&provider);
        assert_eq!(models, vec!["claude-3-haiku-20240307"]);

        provider.provider_type = "OPENAI".to_string();
        let models = service.get_default_test_models(&provider);
        assert_eq!(models, vec!["gpt-3.5-turbo"]);
    }

    #[test]
    fn test_determine_provider_status_single_healthy() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let statuses = vec![ModelHealthStatus {
            model: "model1".to_string(),
            status: HealthStatus::Healthy,
            response_time_ms: Some(100),
            error: None,
        }];

        assert_eq!(
            service.determine_provider_status(&statuses),
            HealthStatus::Healthy
        );
    }

    #[test]
    fn test_determine_provider_status_single_unhealthy() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let statuses = vec![ModelHealthStatus {
            model: "model1".to_string(),
            status: HealthStatus::Unhealthy,
            response_time_ms: None,
            error: Some("Connection refused".to_string()),
        }];

        assert_eq!(
            service.determine_provider_status(&statuses),
            HealthStatus::Unhealthy
        );
    }

    #[test]
    fn test_calculate_avg_response_time_all_none() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let statuses = vec![
            ModelHealthStatus {
                model: "model1".to_string(),
                status: HealthStatus::Unhealthy,
                response_time_ms: None,
                error: Some("Error".to_string()),
            },
            ModelHealthStatus {
                model: "model2".to_string(),
                status: HealthStatus::Unhealthy,
                response_time_ms: None,
                error: Some("Error".to_string()),
            },
        ];

        assert_eq!(service.calculate_avg_response_time(&statuses), None);
    }

    #[test]
    fn test_calculate_avg_response_time_single() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 10);
        let statuses = vec![ModelHealthStatus {
            model: "model1".to_string(),
            status: HealthStatus::Healthy,
            response_time_ms: Some(42),
            error: None,
        }];

        assert_eq!(service.calculate_avg_response_time(&statuses), Some(42));
    }

    #[tokio::test]
    async fn test_check_provider_health_uses_default_models() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 1); // 1s timeout for fast failure
        let provider = create_test_provider();

        // No model list → uses get_default_test_models
        let result = service.check_provider_health(&provider, None).await;
        assert_ne!(result.status, HealthStatus::Disabled);
        // Should have tested at least one model
        assert!(!result.models.is_empty());
    }

    #[tokio::test]
    async fn test_check_provider_health_with_explicit_models() {
        let client = Client::new();
        let service = HealthCheckService::new(client, 1);
        let provider = create_test_provider();

        // Explicit model list overrides defaults
        let result = service
            .check_provider_health(
                &provider,
                Some(vec![
                    "custom-model-a".to_string(),
                    "custom-model-b".to_string(),
                ]),
            )
            .await;

        assert_eq!(result.models.len(), 2);
        let model_names: Vec<_> = result.models.iter().map(|m| m.model.as_str()).collect();
        assert!(model_names.contains(&"custom-model-a"));
        assert!(model_names.contains(&"custom-model-b"));
    }
}
