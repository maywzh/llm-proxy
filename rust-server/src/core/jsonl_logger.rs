//! JSONL file logging for request/response pairs.
//!
//! This module provides async JSONL logging for debugging and analysis.
//! Requests and responses are logged as separate JSONL lines, linked by request_id.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Mutex};

/// JSONL logger configuration
#[derive(Debug, Clone)]
pub struct JsonlLoggerConfig {
    /// Path to the JSONL log file
    pub log_path: PathBuf,
    /// Whether logging is enabled
    pub enabled: bool,
    /// Buffer size for the write channel
    pub buffer_size: usize,
}

impl Default for JsonlLoggerConfig {
    fn default() -> Self {
        Self {
            log_path: PathBuf::from("./logs/requests.jsonl"),
            enabled: false,
            buffer_size: 1000,
        }
    }
}

impl JsonlLoggerConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Self {
        let enabled = std::env::var("JSONL_LOG_ENABLED")
            .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
            .unwrap_or(false);

        let log_path = std::env::var("JSONL_LOG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./logs/requests.jsonl"));

        let buffer_size = std::env::var("JSONL_LOG_BUFFER_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000);

        Self {
            log_path,
            enabled,
            buffer_size,
        }
    }
}

// ============================================================================
// Record Types
// ============================================================================

/// Request record - logged immediately when request is received
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestRecord {
    /// Record type identifier
    #[serde(rename = "type")]
    pub record_type: String,
    /// Timestamp of the request
    pub timestamp: DateTime<Utc>,
    /// Unique request ID (links request and response)
    pub request_id: String,
    /// API endpoint
    pub endpoint: String,
    /// Provider name
    pub provider: String,
    /// Request payload
    pub payload: Value,
}

impl RequestRecord {
    /// Create a new request record
    pub fn new(request_id: String, endpoint: String, provider: String, payload: Value) -> Self {
        Self {
            record_type: "request".to_string(),
            timestamp: Utc::now(),
            request_id,
            endpoint,
            provider,
            payload,
        }
    }
}

/// Response record - logged when response is completed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseRecord {
    /// Record type identifier
    #[serde(rename = "type")]
    pub record_type: String,
    /// Timestamp of the response
    pub timestamp: DateTime<Utc>,
    /// Unique request ID (links request and response)
    pub request_id: String,
    /// HTTP status code
    pub status_code: u16,
    /// Error message if request failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_msg: Option<String>,
    /// Response body (for non-streaming responses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    /// Chunk sequence (for streaming responses) - raw SSE data strings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_sequence: Option<Vec<String>>,
}

impl ResponseRecord {
    /// Create a new non-streaming response record
    pub fn new_non_streaming(
        request_id: String,
        status_code: u16,
        error_msg: Option<String>,
        body: Value,
    ) -> Self {
        Self {
            record_type: "response".to_string(),
            timestamp: Utc::now(),
            request_id,
            status_code,
            error_msg,
            body: Some(body),
            chunk_sequence: None,
        }
    }

    /// Create a new streaming response record
    pub fn new_streaming(
        request_id: String,
        status_code: u16,
        error_msg: Option<String>,
        chunk_sequence: Vec<String>,
    ) -> Self {
        Self {
            record_type: "response".to_string(),
            timestamp: Utc::now(),
            request_id,
            status_code,
            error_msg,
            body: None,
            chunk_sequence: Some(chunk_sequence),
        }
    }
}

/// Provider request record - logged when request is sent to provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequestRecord {
    /// Record type identifier
    #[serde(rename = "type")]
    pub record_type: String,
    /// Timestamp of the request
    pub timestamp: DateTime<Utc>,
    /// Unique request ID (links all records for a request)
    pub request_id: String,
    /// Provider name
    pub provider: String,
    /// Provider API base URL
    pub api_base: String,
    /// Provider endpoint path
    pub endpoint: String,
    /// Request payload sent to provider
    pub payload: Value,
}

impl ProviderRequestRecord {
    /// Create a new provider request record
    pub fn new(
        request_id: String,
        provider: String,
        api_base: String,
        endpoint: String,
        payload: Value,
    ) -> Self {
        Self {
            record_type: "provider_request".to_string(),
            timestamp: Utc::now(),
            request_id,
            provider,
            api_base,
            endpoint,
            payload,
        }
    }
}

/// Provider response record - logged when response is received from provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponseRecord {
    /// Record type identifier
    #[serde(rename = "type")]
    pub record_type: String,
    /// Timestamp of the response
    pub timestamp: DateTime<Utc>,
    /// Unique request ID (links all records for a request)
    pub request_id: String,
    /// Provider name
    pub provider: String,
    /// HTTP status code
    pub status_code: u16,
    /// Error message if request failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_msg: Option<String>,
    /// Response body (for non-streaming responses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    /// Chunk sequence (for streaming responses) - raw SSE data strings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_sequence: Option<Vec<String>>,
}

impl ProviderResponseRecord {
    /// Create a new non-streaming provider response record
    pub fn new_non_streaming(
        request_id: String,
        provider: String,
        status_code: u16,
        error_msg: Option<String>,
        body: Value,
    ) -> Self {
        Self {
            record_type: "provider_response".to_string(),
            timestamp: Utc::now(),
            request_id,
            provider,
            status_code,
            error_msg,
            body: Some(body),
            chunk_sequence: None,
        }
    }

    /// Create a new streaming provider response record
    pub fn new_streaming(
        request_id: String,
        provider: String,
        status_code: u16,
        error_msg: Option<String>,
        chunk_sequence: Vec<String>,
    ) -> Self {
        Self {
            record_type: "provider_response".to_string(),
            timestamp: Utc::now(),
            request_id,
            provider,
            status_code,
            error_msg,
            body: None,
            chunk_sequence: Some(chunk_sequence),
        }
    }
}

/// Union type for log records
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LogRecord {
    Request(RequestRecord),
    Response(ResponseRecord),
    ProviderRequest(ProviderRequestRecord),
    ProviderResponse(ProviderResponseRecord),
}

// ============================================================================
// Logger Implementation
// ============================================================================

/// Async JSONL logger with buffered writes
pub struct JsonlLogger {
    /// Sender for log records
    sender: mpsc::Sender<LogRecord>,
    /// Configuration
    config: JsonlLoggerConfig,
}

impl JsonlLogger {
    /// Create a new JSONL logger
    ///
    /// Returns None if logging is disabled
    pub async fn new(config: JsonlLoggerConfig) -> Option<Arc<Self>> {
        if !config.enabled {
            tracing::info!("JSONL logging is disabled");
            return None;
        }

        // Ensure parent directory exists
        if let Some(parent) = config.log_path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                tracing::error!("Failed to create JSONL log directory: {}", e);
                return None;
            }
        }

        // Create channel for async writes
        let (sender, receiver) = mpsc::channel::<LogRecord>(config.buffer_size);

        let log_path = config.log_path.clone();

        // Spawn background writer task
        tokio::spawn(async move {
            Self::writer_task(receiver, log_path).await;
        });

        tracing::info!(
            "JSONL logging enabled, writing to: {}",
            config.log_path.display()
        );

        Some(Arc::new(Self { sender, config }))
    }

    /// Background task that writes records to the file
    async fn writer_task(mut receiver: mpsc::Receiver<LogRecord>, log_path: PathBuf) {
        // Open file in append mode
        let file = match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
        {
            Ok(f) => f,
            Err(e) => {
                tracing::error!("Failed to open JSONL log file: {}", e);
                return;
            }
        };

        let file = Arc::new(Mutex::new(file));
        let mut buffer = Vec::with_capacity(100);
        let mut flush_interval = tokio::time::interval(std::time::Duration::from_secs(1));

        loop {
            tokio::select! {
                // Receive records
                Some(record) = receiver.recv() => {
                    buffer.push(record);

                    // Flush if buffer is full
                    if buffer.len() >= 100 {
                        Self::flush_buffer(&file, &mut buffer).await;
                    }
                }
                // Periodic flush
                _ = flush_interval.tick() => {
                    if !buffer.is_empty() {
                        Self::flush_buffer(&file, &mut buffer).await;
                    }
                }
                else => {
                    // Channel closed, flush remaining and exit
                    if !buffer.is_empty() {
                        Self::flush_buffer(&file, &mut buffer).await;
                    }
                    break;
                }
            }
        }

        tracing::info!("JSONL logger writer task stopped");
    }

    /// Flush buffered records to file
    async fn flush_buffer(file: &Arc<Mutex<File>>, buffer: &mut Vec<LogRecord>) {
        if buffer.is_empty() {
            return;
        }

        let mut output = String::new();
        for record in buffer.drain(..) {
            match serde_json::to_string(&record) {
                Ok(json) => {
                    output.push_str(&json);
                    output.push('\n');
                }
                Err(e) => {
                    tracing::error!("Failed to serialize JSONL record: {}", e);
                }
            }
        }

        if !output.is_empty() {
            let mut file = file.lock().await;
            if let Err(e) = file.write_all(output.as_bytes()).await {
                tracing::error!("Failed to write to JSONL log file: {}", e);
            }
            if let Err(e) = file.flush().await {
                tracing::error!("Failed to flush JSONL log file: {}", e);
            }
        }
    }

    /// Log a record (non-blocking)
    fn log(&self, record: LogRecord) {
        if let Err(e) = self.sender.try_send(record) {
            tracing::warn!("Failed to send JSONL record: {}", e);
        }
    }

    /// Log a request immediately when received
    pub fn log_request(
        &self,
        request_id: String,
        endpoint: String,
        provider: String,
        payload: Value,
    ) {
        let record = RequestRecord::new(request_id, endpoint, provider, payload);
        self.log(LogRecord::Request(record));
    }

    /// Log a non-streaming response when completed
    pub fn log_response(
        &self,
        request_id: String,
        status_code: u16,
        error_msg: Option<String>,
        body: Value,
    ) {
        let record = ResponseRecord::new_non_streaming(request_id, status_code, error_msg, body);
        self.log(LogRecord::Response(record));
    }

    /// Log a streaming response when stream completes
    pub fn log_streaming_response(
        &self,
        request_id: String,
        status_code: u16,
        error_msg: Option<String>,
        chunk_sequence: Vec<String>,
    ) {
        let record =
            ResponseRecord::new_streaming(request_id, status_code, error_msg, chunk_sequence);
        self.log(LogRecord::Response(record));
    }

    /// Log a provider request when sent to upstream
    pub fn log_provider_request(
        &self,
        request_id: String,
        provider: String,
        api_base: String,
        endpoint: String,
        payload: Value,
    ) {
        let record = ProviderRequestRecord::new(request_id, provider, api_base, endpoint, payload);
        self.log(LogRecord::ProviderRequest(record));
    }

    /// Log a non-streaming provider response when received
    pub fn log_provider_response(
        &self,
        request_id: String,
        provider: String,
        status_code: u16,
        error_msg: Option<String>,
        body: Value,
    ) {
        let record = ProviderResponseRecord::new_non_streaming(
            request_id,
            provider,
            status_code,
            error_msg,
            body,
        );
        self.log(LogRecord::ProviderResponse(record));
    }

    /// Log a streaming provider response when stream completes
    pub fn log_provider_streaming_response(
        &self,
        request_id: String,
        provider: String,
        status_code: u16,
        error_msg: Option<String>,
        chunk_sequence: Vec<String>,
    ) {
        let record = ProviderResponseRecord::new_streaming(
            request_id,
            provider,
            status_code,
            error_msg,
            chunk_sequence,
        );
        self.log(LogRecord::ProviderResponse(record));
    }

    /// Check if logging is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Get the log file path
    pub fn log_path(&self) -> &PathBuf {
        &self.config.log_path
    }
}

// ============================================================================
// Global Logger Instance
// ============================================================================

/// Global JSONL logger instance
static JSONL_LOGGER: std::sync::OnceLock<Option<Arc<JsonlLogger>>> = std::sync::OnceLock::new();

/// Initialize the global JSONL logger
pub async fn init_jsonl_logger() {
    let config = JsonlLoggerConfig::from_env();
    let logger = JsonlLogger::new(config).await;
    let _ = JSONL_LOGGER.set(logger);
}

/// Get the global JSONL logger
pub fn get_jsonl_logger() -> Option<Arc<JsonlLogger>> {
    JSONL_LOGGER.get().and_then(|opt| opt.clone())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Log a request immediately when received
pub fn log_request(request_id: &str, endpoint: &str, provider: &str, payload: &Value) {
    if let Some(logger) = get_jsonl_logger() {
        logger.log_request(
            request_id.to_string(),
            endpoint.to_string(),
            provider.to_string(),
            payload.clone(),
        );
    }
}

/// Log a non-streaming response when completed
pub fn log_response(request_id: &str, status_code: u16, error_msg: Option<&str>, body: &Value) {
    if let Some(logger) = get_jsonl_logger() {
        logger.log_response(
            request_id.to_string(),
            status_code,
            error_msg.map(|s| s.to_string()),
            body.clone(),
        );
    }
}

/// Log a streaming response when stream completes
pub fn log_streaming_response(
    request_id: &str,
    status_code: u16,
    error_msg: Option<&str>,
    chunk_sequence: Vec<String>,
) {
    if let Some(logger) = get_jsonl_logger() {
        logger.log_streaming_response(
            request_id.to_string(),
            status_code,
            error_msg.map(|s| s.to_string()),
            chunk_sequence,
        );
    }
}

/// Log a provider request when sent to upstream
pub fn log_provider_request(
    request_id: &str,
    provider: &str,
    api_base: &str,
    endpoint: &str,
    payload: &Value,
) {
    if let Some(logger) = get_jsonl_logger() {
        logger.log_provider_request(
            request_id.to_string(),
            provider.to_string(),
            api_base.to_string(),
            endpoint.to_string(),
            payload.clone(),
        );
    }
}

/// Log a non-streaming provider response when received
pub fn log_provider_response(
    request_id: &str,
    provider: &str,
    status_code: u16,
    error_msg: Option<&str>,
    body: &Value,
) {
    if let Some(logger) = get_jsonl_logger() {
        logger.log_provider_response(
            request_id.to_string(),
            provider.to_string(),
            status_code,
            error_msg.map(|s| s.to_string()),
            body.clone(),
        );
    }
}

/// Log a streaming provider response when stream completes
pub fn log_provider_streaming_response(
    request_id: &str,
    provider: &str,
    status_code: u16,
    error_msg: Option<&str>,
    chunk_sequence: Vec<String>,
) {
    if let Some(logger) = get_jsonl_logger() {
        logger.log_provider_streaming_response(
            request_id.to_string(),
            provider.to_string(),
            status_code,
            error_msg.map(|s| s.to_string()),
            chunk_sequence,
        );
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_record() {
        let record = RequestRecord::new(
            "req-123".to_string(),
            "/v1/chat/completions".to_string(),
            "openai-main".to_string(),
            json!({"model": "gpt-4", "messages": []}),
        );

        assert_eq!(record.record_type, "request");
        assert_eq!(record.request_id, "req-123");
        assert_eq!(record.endpoint, "/v1/chat/completions");
        assert_eq!(record.provider, "openai-main");
    }

    #[test]
    fn test_response_record_non_streaming() {
        let record = ResponseRecord::new_non_streaming(
            "req-123".to_string(),
            200,
            None,
            json!({"id": "resp-123", "choices": []}),
        );

        assert_eq!(record.record_type, "response");
        assert_eq!(record.request_id, "req-123");
        assert_eq!(record.status_code, 200);
        assert!(record.error_msg.is_none());
        assert!(record.body.is_some());
        assert!(record.chunk_sequence.is_none());
    }

    #[test]
    fn test_response_record_streaming() {
        let chunks = vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}".to_string(),
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}".to_string(),
            "data: [DONE]".to_string(),
        ];

        let record = ResponseRecord::new_streaming("req-456".to_string(), 200, None, chunks);

        assert_eq!(record.record_type, "response");
        assert_eq!(record.request_id, "req-456");
        assert_eq!(record.status_code, 200);
        assert!(record.body.is_none());
        assert!(record.chunk_sequence.is_some());
        assert_eq!(record.chunk_sequence.as_ref().unwrap().len(), 3);
    }

    #[test]
    fn test_response_record_error() {
        let record = ResponseRecord::new_non_streaming(
            "req-789".to_string(),
            429,
            Some("Rate limit exceeded".to_string()),
            json!({"error": {"message": "Rate limit exceeded"}}),
        );

        assert_eq!(record.status_code, 429);
        assert_eq!(record.error_msg, Some("Rate limit exceeded".to_string()));
    }

    #[test]
    fn test_request_record_serialization() {
        let record = RequestRecord::new(
            "req-123".to_string(),
            "/v1/chat/completions".to_string(),
            "openai-main".to_string(),
            json!({"model": "gpt-4"}),
        );

        let json_str = serde_json::to_string(&record).unwrap();
        assert!(json_str.contains("\"type\":\"request\""));
        assert!(json_str.contains("\"request_id\":\"req-123\""));
        assert!(json_str.contains("\"endpoint\":\"/v1/chat/completions\""));
        assert!(json_str.contains("\"provider\":\"openai-main\""));
    }

    #[test]
    fn test_response_record_serialization() {
        let record = ResponseRecord::new_non_streaming(
            "req-123".to_string(),
            200,
            None,
            json!({"id": "resp-123"}),
        );

        let json_str = serde_json::to_string(&record).unwrap();
        assert!(json_str.contains("\"type\":\"response\""));
        assert!(json_str.contains("\"request_id\":\"req-123\""));
        assert!(json_str.contains("\"status_code\":200"));
        // error_msg should be skipped when None
        assert!(!json_str.contains("\"error_msg\""));
        // chunk_sequence should be skipped when None
        assert!(!json_str.contains("\"chunk_sequence\""));
    }

    #[test]
    fn test_streaming_response_serialization() {
        let chunks = vec!["data: {\"test\":1}".to_string(), "data: [DONE]".to_string()];

        let record = ResponseRecord::new_streaming("req-456".to_string(), 200, None, chunks);

        let json_str = serde_json::to_string(&record).unwrap();
        assert!(json_str.contains("\"type\":\"response\""));
        assert!(json_str.contains("\"chunk_sequence\""));
        // body should be skipped when None
        assert!(!json_str.contains("\"body\""));
    }

    #[test]
    fn test_config_from_env_defaults() {
        // Clear env vars
        std::env::remove_var("JSONL_LOG_ENABLED");
        std::env::remove_var("JSONL_LOG_PATH");
        std::env::remove_var("JSONL_LOG_BUFFER_SIZE");

        let config = JsonlLoggerConfig::from_env();
        assert!(!config.enabled);
        assert_eq!(config.log_path, PathBuf::from("./logs/requests.jsonl"));
        assert_eq!(config.buffer_size, 1000);
    }
}
