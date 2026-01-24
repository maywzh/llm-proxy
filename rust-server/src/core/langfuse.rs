//! Langfuse integration for observability and tracing.
//!
//! This module provides Langfuse integration for the LLM proxy server.
//! Since there's no official Langfuse Rust SDK, we implement a lightweight
//! HTTP client for the Langfuse API.

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

// ============================================================================
// Configuration
// ============================================================================

/// Langfuse integration configuration.
#[derive(Debug, Clone)]
pub struct LangfuseConfig {
    /// Enable Langfuse tracing (default: false)
    pub enabled: bool,
    /// Langfuse public key
    pub public_key: Option<String>,
    /// Langfuse secret key
    pub secret_key: Option<String>,
    /// Langfuse server URL (default: https://cloud.langfuse.com)
    pub host: String,
    /// Sampling rate 0.0-1.0 (default: 1.0)
    pub sample_rate: f64,
    /// Flush interval in seconds (default: 5)
    pub flush_interval_secs: u64,
    /// Enable debug mode (default: false)
    pub debug: bool,
}

impl Default for LangfuseConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            public_key: None,
            secret_key: None,
            host: "https://cloud.langfuse.com".to_string(),
            sample_rate: 1.0,
            flush_interval_secs: 5,
            debug: false,
        }
    }
}

impl LangfuseConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Self {
        let enabled = std::env::var("LANGFUSE_ENABLED")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        let public_key = std::env::var("LANGFUSE_PUBLIC_KEY").ok();
        let secret_key = std::env::var("LANGFUSE_SECRET_KEY").ok();

        let host = std::env::var("LANGFUSE_HOST")
            .unwrap_or_else(|_| "https://cloud.langfuse.com".to_string());

        let sample_rate: f64 = std::env::var("LANGFUSE_SAMPLE_RATE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1.0_f64)
            .clamp(0.0, 1.0);

        let flush_interval_secs = std::env::var("LANGFUSE_FLUSH_INTERVAL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(5);

        let debug = std::env::var("LANGFUSE_DEBUG")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        Self {
            enabled,
            public_key,
            secret_key,
            host,
            sample_rate,
            flush_interval_secs,
            debug,
        }
    }

    /// Validate configuration when enabled.
    pub fn validate(&self) -> Result<(), String> {
        if self.enabled {
            if self.public_key.is_none() {
                return Err("LANGFUSE_PUBLIC_KEY is required when LANGFUSE_ENABLED=true".to_string());
            }
            if self.secret_key.is_none() {
                return Err("LANGFUSE_SECRET_KEY is required when LANGFUSE_ENABLED=true".to_string());
            }
        }
        Ok(())
    }
}

// ============================================================================
// Data Models
// ============================================================================

/// Data collected for a generation span.
#[derive(Debug, Clone, Serialize)]
pub struct GenerationData {
    /// Trace ID
    pub trace_id: String,
    /// Generation ID
    pub generation_id: String,
    /// Generation name
    pub name: String,

    // Provider info
    pub provider_key: String,
    pub provider_type: String,
    pub provider_api_base: String,

    // Model info
    pub original_model: String,
    pub mapped_model: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub model_parameters: HashMap<String, serde_json::Value>,

    // Input/Output
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub input_messages: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,

    // Token usage
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,

    // Timing
    pub start_time: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttft_time: Option<DateTime<Utc>>,

    // Status
    pub is_streaming: bool,
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,

    // Request context
    pub request_id: String,
    pub credential_name: String,
    pub endpoint: String,
}

impl Default for GenerationData {
    fn default() -> Self {
        Self {
            trace_id: String::new(),
            generation_id: Uuid::new_v4().to_string(),
            name: "chat-completion".to_string(),
            provider_key: String::new(),
            provider_type: String::new(),
            provider_api_base: String::new(),
            original_model: String::new(),
            mapped_model: String::new(),
            model_parameters: HashMap::new(),
            input_messages: Vec::new(),
            output_content: None,
            finish_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            start_time: Utc::now(),
            end_time: None,
            ttft_time: None,
            is_streaming: false,
            is_error: false,
            error_message: None,
            request_id: String::new(),
            credential_name: String::new(),
            endpoint: String::new(),
        }
    }
}

// ============================================================================
// Langfuse API Models
// ============================================================================

#[derive(Debug, Serialize)]
struct IngestionBatch {
    batch: Vec<IngestionEvent>,
}

/// Langfuse ingestion event wrapper.
/// Each event must have: id, timestamp, type, and body.
#[derive(Debug, Serialize)]
struct IngestionEvent {
    id: String,
    timestamp: DateTime<Utc>,
    #[serde(rename = "type")]
    event_type: String,
    body: serde_json::Value,
}

impl IngestionEvent {
    fn trace_create(body: TraceCreateBody) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event_type: "trace-create".to_string(),
            body: serde_json::to_value(body).unwrap_or_default(),
        }
    }

    fn generation_create(body: GenerationCreateBody) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event_type: "generation-create".to_string(),
            body: serde_json::to_value(body).unwrap_or_default(),
        }
    }

    fn trace_update(body: TraceUpdateBody) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            event_type: "trace-create".to_string(), // Langfuse uses trace-create for updates too
            body: serde_json::to_value(body).unwrap_or_default(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceCreateBody {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}

/// Body for updating trace metadata (uses same endpoint as trace-create)
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TraceUpdateBody {
    id: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationCreateBody {
    id: String,
    trace_id: String,
    name: String,
    start_time: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_time: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_start_time: Option<DateTime<Utc>>,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_parameters: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    input: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<UsageBody>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<String, serde_json::Value>,
    level: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status_message: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageBody {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct IngestionResponse {
    #[allow(dead_code)]
    successes: Vec<serde_json::Value>,
    #[allow(dead_code)]
    errors: Vec<serde_json::Value>,
}

// ============================================================================
// Langfuse Service
// ============================================================================

/// Event sent to the background worker.
enum LangfuseEvent {
    Trace {
        trace_id: String,
        request_id: String,
        credential_name: String,
        endpoint: String,
        tags: Vec<String>,
        /// Client metadata (user-agent, etc.)
        client_metadata: HashMap<String, String>,
    },
    TraceUpdate {
        trace_id: String,
        provider_key: String,
        provider_api_base: String,
        model: String,
        /// Additional tags to add (e.g., provider tag)
        tags: Vec<String>,
    },
    Generation(GenerationData),
    Flush,
    Shutdown,
}

/// Langfuse service for creating and managing traces.
pub struct LangfuseService {
    config: LangfuseConfig,
    sender: Option<mpsc::Sender<LangfuseEvent>>,
    initialized: bool,
}

impl LangfuseService {
    /// Create a new Langfuse service.
    pub fn new() -> Self {
        Self {
            config: LangfuseConfig::default(),
            sender: None,
            initialized: false,
        }
    }

    /// Initialize the Langfuse service.
    pub fn initialize(&mut self, config: Option<LangfuseConfig>) {
        let config = config.unwrap_or_else(LangfuseConfig::from_env);

        if !config.enabled {
            tracing::info!("Langfuse tracing is disabled");
            self.config = config;
            self.initialized = true;
            return;
        }

        if let Err(e) = config.validate() {
            tracing::error!("Langfuse configuration error: {}", e);
            self.config = LangfuseConfig::default();
            self.initialized = true;
            return;
        }

        // Create channel for events
        let (sender, receiver) = mpsc::channel::<LangfuseEvent>(1000);

        // Start background worker
        let worker_config = config.clone();
        tokio::spawn(async move {
            langfuse_worker(receiver, worker_config).await;
        });

        tracing::info!("Langfuse client initialized, host={}", config.host);

        self.config = config;
        self.sender = Some(sender);
        self.initialized = true;
    }

    /// Check if Langfuse is enabled and properly configured.
    pub fn enabled(&self) -> bool {
        self.initialized && self.config.enabled && self.sender.is_some()
    }

    /// Determine if this request should be sampled.
    pub fn should_sample(&self) -> bool {
        if !self.enabled() {
            return false;
        }
        if self.config.sample_rate >= 1.0 {
            return true;
        }
        rand::random::<f64>() < self.config.sample_rate
    }

    /// Create a new trace and return its ID.
    ///
    /// # Arguments
    /// * `request_id` - Internal request ID
    /// * `credential_name` - Name of the credential used
    /// * `endpoint` - API endpoint (e.g., "/v1/chat/completions")
    /// * `tags` - Optional list of tags for filtering
    /// * `client_metadata` - Client metadata (user-agent, x-forwarded-for, etc.)
    pub fn create_trace(
        &self,
        request_id: &str,
        credential_name: &str,
        endpoint: &str,
        tags: Vec<String>,
        client_metadata: HashMap<String, String>,
    ) -> Option<String> {
        if !self.enabled() || !self.should_sample() {
            return None;
        }

        let trace_id = Uuid::new_v4().to_string();

        if let Some(sender) = &self.sender {
            let event = LangfuseEvent::Trace {
                trace_id: trace_id.clone(),
                request_id: request_id.to_string(),
                credential_name: credential_name.to_string(),
                endpoint: endpoint.to_string(),
                tags,
                client_metadata,
            };

            if let Err(e) = sender.try_send(event) {
                tracing::warn!("Failed to send trace event: {}", e);
                return None;
            }

            tracing::debug!("Created Langfuse trace: {}", trace_id);
            Some(trace_id)
        } else {
            None
        }
    }

    /// Update trace with provider information.
    pub fn update_trace_provider(
        &self,
        trace_id: &str,
        provider_key: &str,
        provider_api_base: &str,
        model: &str,
    ) {
        if !self.enabled() || trace_id.is_empty() {
            return;
        }

        if let Some(sender) = &self.sender {
            // Add provider as a tag
            let tags = vec![
                format!("provider:{}", provider_key),
                format!("model:{}", model),
            ];

            let event = LangfuseEvent::TraceUpdate {
                trace_id: trace_id.to_string(),
                provider_key: provider_key.to_string(),
                provider_api_base: provider_api_base.to_string(),
                model: model.to_string(),
                tags,
            };

            if let Err(e) = sender.try_send(event) {
                tracing::warn!("Failed to send trace update event: {}", e);
            }
        }
    }

    /// Record a generation span.
    pub fn trace_generation(&self, data: GenerationData) {
        if !self.enabled() || data.trace_id.is_empty() {
            return;
        }

        if let Some(sender) = &self.sender {
            if let Err(e) = sender.try_send(LangfuseEvent::Generation(data)) {
                tracing::warn!("Failed to send generation event: {}", e);
            }
        }
    }

    /// Flush pending events.
    pub async fn flush(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(LangfuseEvent::Flush).await;
        }
    }

    /// Shutdown the service.
    pub async fn shutdown(&self) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(LangfuseEvent::Shutdown).await;
            // Give worker time to flush
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
        tracing::info!("Langfuse client shutdown complete");
    }
}

impl Default for LangfuseService {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Background Worker
// ============================================================================

async fn langfuse_worker(mut receiver: mpsc::Receiver<LangfuseEvent>, config: LangfuseConfig) {
    let client = Client::new();
    let mut batch: Vec<IngestionEvent> = Vec::new();
    let flush_interval = tokio::time::Duration::from_secs(config.flush_interval_secs);
    let mut flush_timer = tokio::time::interval(flush_interval);

    // Build auth header
    let auth = format!(
        "{}:{}",
        config.public_key.as_deref().unwrap_or(""),
        config.secret_key.as_deref().unwrap_or("")
    );
    let auth_header = format!("Basic {}", base64_encode(&auth));
    let ingestion_url = format!("{}/api/public/ingestion", config.host);

    loop {
        tokio::select! {
            Some(event) = receiver.recv() => {
                match event {
                    LangfuseEvent::Trace { trace_id, request_id, credential_name, endpoint, tags, client_metadata } => {
                        let mut metadata = HashMap::new();
                        metadata.insert("request_id".to_string(), serde_json::json!(request_id));
                        metadata.insert("credential_name".to_string(), serde_json::json!(credential_name));
                        metadata.insert("endpoint".to_string(), serde_json::json!(endpoint));
                        
                        // Add client metadata (user-agent, x-forwarded-for, etc.)
                        for (key, value) in client_metadata {
                            metadata.insert(key, serde_json::json!(value));
                        }

                        batch.push(IngestionEvent::trace_create(TraceCreateBody {
                            id: trace_id,
                            name: "llm-proxy-request".to_string(),
                            user_id: Some(credential_name),
                            metadata,
                            tags,
                        }));
                    }
                    LangfuseEvent::TraceUpdate { trace_id, provider_key, provider_api_base, model, tags } => {
                        let mut metadata = HashMap::new();
                        metadata.insert("provider_key".to_string(), serde_json::json!(provider_key));
                        metadata.insert("provider_api_base".to_string(), serde_json::json!(provider_api_base));
                        metadata.insert("model".to_string(), serde_json::json!(model));

                        batch.push(IngestionEvent::trace_update(TraceUpdateBody {
                            id: trace_id,
                            metadata,
                            tags,
                        }));
                    }
                    LangfuseEvent::Generation(data) => {
                        let mut metadata = HashMap::new();
                        metadata.insert("provider_key".to_string(), serde_json::json!(data.provider_key));
                        metadata.insert("provider_type".to_string(), serde_json::json!(data.provider_type));
                        metadata.insert("provider_api_base".to_string(), serde_json::json!(data.provider_api_base));
                        metadata.insert("mapped_model".to_string(), serde_json::json!(data.mapped_model));
                        metadata.insert("is_streaming".to_string(), serde_json::json!(data.is_streaming));
                        metadata.insert("request_id".to_string(), serde_json::json!(data.request_id));
                        if let Some(ref reason) = data.finish_reason {
                            metadata.insert("finish_reason".to_string(), serde_json::json!(reason));
                        }

                        let usage = if data.prompt_tokens > 0 || data.completion_tokens > 0 {
                            Some(UsageBody {
                                prompt_tokens: data.prompt_tokens,
                                completion_tokens: data.completion_tokens,
                                total_tokens: data.total_tokens,
                            })
                        } else {
                            None
                        };

                        let input = if !data.input_messages.is_empty() {
                            Some(serde_json::json!({ "messages": data.input_messages }))
                        } else {
                            None
                        };

                        let model_parameters = if !data.model_parameters.is_empty() {
                            Some(data.model_parameters)
                        } else {
                            None
                        };

                        batch.push(IngestionEvent::generation_create(GenerationCreateBody {
                            id: data.generation_id,
                            trace_id: data.trace_id,
                            name: data.name,
                            start_time: data.start_time,
                            end_time: data.end_time,
                            completion_start_time: data.ttft_time,
                            model: data.original_model,
                            model_parameters,
                            input,
                            output: data.output_content,
                            usage,
                            metadata,
                            level: if data.is_error { "ERROR".to_string() } else { "DEFAULT".to_string() },
                            status_message: data.error_message,
                        }));
                    }
                    LangfuseEvent::Flush => {
                        if !batch.is_empty() {
                            send_batch(&client, &ingestion_url, &auth_header, &mut batch, config.debug).await;
                        }
                    }
                    LangfuseEvent::Shutdown => {
                        if !batch.is_empty() {
                            send_batch(&client, &ingestion_url, &auth_header, &mut batch, config.debug).await;
                        }
                        break;
                    }
                }
            }
            _ = flush_timer.tick() => {
                if !batch.is_empty() {
                    send_batch(&client, &ingestion_url, &auth_header, &mut batch, config.debug).await;
                }
            }
        }
    }

    tracing::debug!("Langfuse worker shutdown");
}

async fn send_batch(
    client: &Client,
    url: &str,
    auth_header: &str,
    batch: &mut Vec<IngestionEvent>,
    debug: bool,
) {
    let events: Vec<IngestionEvent> = batch.drain(..).collect();
    let count = events.len();

    let payload = IngestionBatch { batch: events };

    if debug {
        tracing::debug!("Sending {} events to Langfuse", count);
    }

    match client
        .post(url)
        .header("Authorization", auth_header)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<IngestionResponse>().await {
                    Ok(body) => {
                        if body.errors.is_empty() {
                            if debug {
                                tracing::debug!(
                                    "Langfuse batch sent: {} successes",
                                    body.successes.len()
                                );
                            }
                        } else {
                            // Always log errors, not just in debug mode
                            tracing::warn!(
                                "Langfuse batch partially failed: {} successes, {} errors. Error details: {:?}",
                                body.successes.len(),
                                body.errors.len(),
                                body.errors
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse Langfuse response: {}", e);
                    }
                }
            } else {
                tracing::warn!(
                    "Langfuse API error: {} - {}",
                    response.status(),
                    response.text().await.unwrap_or_default()
                );
            }
        }
        Err(e) => {
            tracing::warn!("Failed to send Langfuse batch: {}", e);
        }
    }
}

fn base64_encode(input: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = base64_writer(&mut buf);
        encoder.write_all(input.as_bytes()).unwrap();
    }
    String::from_utf8(buf).unwrap()
}

fn base64_writer(output: &mut Vec<u8>) -> impl Write + '_ {
    struct Base64Writer<'a>(&'a mut Vec<u8>);

    impl<'a> Write for Base64Writer<'a> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

            for chunk in buf.chunks(3) {
                let b0 = chunk[0] as usize;
                let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
                let b2 = chunk.get(2).copied().unwrap_or(0) as usize;

                self.0.push(ALPHABET[b0 >> 2]);
                self.0.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)]);

                if chunk.len() > 1 {
                    self.0.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)]);
                } else {
                    self.0.push(b'=');
                }

                if chunk.len() > 2 {
                    self.0.push(ALPHABET[b2 & 0x3f]);
                } else {
                    self.0.push(b'=');
                }
            }

            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    Base64Writer(output)
}

// ============================================================================
// Global Service Instance
// ============================================================================

lazy_static::lazy_static! {
    static ref LANGFUSE_SERVICE: Arc<std::sync::RwLock<LangfuseService>> =
        Arc::new(std::sync::RwLock::new(LangfuseService::new()));
}

/// Get the global Langfuse service.
pub fn get_langfuse_service() -> Arc<std::sync::RwLock<LangfuseService>> {
    Arc::clone(&LANGFUSE_SERVICE)
}

/// Initialize the global Langfuse service.
pub fn init_langfuse_service(config: Option<LangfuseConfig>) {
    if let Ok(mut service) = LANGFUSE_SERVICE.write() {
        service.initialize(config);
    }
}

/// Shutdown the global Langfuse service.
pub async fn shutdown_langfuse_service() {
    if let Ok(service) = LANGFUSE_SERVICE.read() {
        service.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = LangfuseConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.host, "https://cloud.langfuse.com");
        assert_eq!(config.sample_rate, 1.0);
        assert_eq!(config.flush_interval_secs, 5);
    }

    #[test]
    fn test_config_validation() {
        let mut config = LangfuseConfig::default();
        assert!(config.validate().is_ok());

        config.enabled = true;
        assert!(config.validate().is_err());

        config.public_key = Some("pk-test".to_string());
        assert!(config.validate().is_err());

        config.secret_key = Some("sk-test".to_string());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_generation_data_default() {
        let data = GenerationData::default();
        assert!(data.trace_id.is_empty());
        assert!(!data.generation_id.is_empty());
        assert_eq!(data.name, "chat-completion");
        assert!(!data.is_error);
    }

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode("hello"), "aGVsbG8=");
        assert_eq!(base64_encode("hello world"), "aGVsbG8gd29ybGQ=");
        assert_eq!(base64_encode("pk:sk"), "cGs6c2s=");
    }

    #[test]
    fn test_service_disabled_by_default() {
        let service = LangfuseService::new();
        assert!(!service.enabled());
        assert!(!service.should_sample());
    }
}
