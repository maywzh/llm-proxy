//! Log service for async background logging of request/response data.
//!
//! This module provides a non-blocking logging system that queues log entries
//! and writes them to PostgreSQL in batches to minimize latency impact.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::core::database::Database;

/// Sensitive fields that should be masked in request/response bodies
const SENSITIVE_FIELDS: &[&str] = &[
    "api_key",
    "authorization",
    "x-api-key",
    "password",
    "secret",
    "token",
];

/// Maximum body size in bytes before truncation
const MAX_BODY_SIZE: usize = 65536;

/// Batch size for database writes
const BATCH_SIZE: usize = 100;

/// Flush interval in seconds
const FLUSH_INTERVAL_SECS: u64 = 5;

/// Log entry data structure matching the database schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub request_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub credential_id: Option<i32>,
    pub credential_name: String,
    pub provider_id: Option<i32>,
    pub provider_name: String,
    pub endpoint: String,
    pub method: String,
    pub model: Option<String>,
    pub is_streaming: bool,
    pub status_code: i32,
    pub duration_ms: i32,
    pub ttft_ms: Option<i32>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    pub request_body: Option<Value>,
    pub response_body: Option<Value>,
    pub error_message: Option<String>,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
}

impl LogEntry {
    /// Create a new log entry with required fields
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: Uuid,
        credential_name: String,
        provider_name: String,
        endpoint: String,
        method: String,
        model: Option<String>,
        is_streaming: bool,
        status_code: i32,
        duration_ms: i32,
    ) -> Self {
        Self {
            request_id,
            timestamp: Utc::now(),
            credential_id: None,
            credential_name,
            provider_id: None,
            provider_name,
            endpoint,
            method,
            model,
            is_streaming,
            status_code,
            duration_ms,
            ttft_ms: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            request_body: None,
            response_body: None,
            error_message: None,
            client_ip: None,
            user_agent: None,
        }
    }

    /// Set credential information
    pub fn with_credential(mut self, id: Option<i32>, name: String) -> Self {
        self.credential_id = id;
        self.credential_name = name;
        self
    }

    /// Set provider information
    pub fn with_provider(mut self, id: Option<i32>, name: String) -> Self {
        self.provider_id = id;
        self.provider_name = name;
        self
    }

    /// Set token usage
    pub fn with_tokens(mut self, prompt: i32, completion: i32) -> Self {
        self.prompt_tokens = prompt;
        self.completion_tokens = completion;
        self.total_tokens = prompt + completion;
        self
    }

    /// Set TTFT (time to first token)
    pub fn with_ttft(mut self, ttft_ms: Option<i32>) -> Self {
        self.ttft_ms = ttft_ms;
        self
    }

    /// Set request body
    pub fn with_request_body(mut self, body: Option<Value>) -> Self {
        self.request_body = body;
        self
    }

    /// Set response body
    pub fn with_response_body(mut self, body: Option<Value>) -> Self {
        self.response_body = body;
        self
    }

    /// Set error message
    pub fn with_error(mut self, error: Option<String>) -> Self {
        self.error_message = error;
        self
    }

    /// Set client information
    pub fn with_client_info(mut self, ip: Option<String>, user_agent: Option<String>) -> Self {
        self.client_ip = ip;
        self.user_agent = user_agent;
        self
    }
}

/// Log collector that preprocesses entries before queuing
pub struct LogCollector {
    sender: mpsc::Sender<LogEntry>,
    log_request_bodies: bool,
    sensitive_fields: HashSet<String>,
}

impl LogCollector {
    /// Create a new log collector
    pub fn new(sender: mpsc::Sender<LogEntry>, log_request_bodies: bool) -> Self {
        let sensitive_fields: HashSet<String> = SENSITIVE_FIELDS
            .iter()
            .map(|s| s.to_lowercase())
            .collect();

        Self {
            sender,
            log_request_bodies,
            sensitive_fields,
        }
    }

    /// Queue a log entry for async writing
    pub async fn log(&self, mut entry: LogEntry) {
        // Apply privacy filters
        entry = self.apply_privacy_filters(entry);

        // Truncate large bodies
        entry = self.truncate_bodies(entry);

        // Send to buffer (non-blocking)
        if let Err(e) = self.sender.try_send(entry) {
            tracing::warn!("Failed to queue log entry: {}", e);
        }
    }

    /// Apply privacy filters to mask sensitive fields
    fn apply_privacy_filters(&self, mut entry: LogEntry) -> LogEntry {
        if let Some(body) = entry.request_body.take() {
            entry.request_body = Some(self.mask_sensitive_fields(body));
        }
        if let Some(body) = entry.response_body.take() {
            entry.response_body = Some(self.mask_sensitive_fields(body));
        }
        entry
    }

    /// Recursively mask sensitive fields in a JSON value
    fn mask_sensitive_fields(&self, value: Value) -> Value {
        match value {
            Value::Object(mut map) => {
                for (key, val) in map.clone().iter() {
                    if self.sensitive_fields.contains(&key.to_lowercase()) {
                        map.insert(key.clone(), Value::String("***MASKED***".to_string()));
                    } else {
                        map.insert(key.clone(), self.mask_sensitive_fields(val.clone()));
                    }
                }
                Value::Object(map)
            }
            Value::Array(arr) => {
                Value::Array(arr.into_iter().map(|v| self.mask_sensitive_fields(v)).collect())
            }
            other => other,
        }
    }

    /// Truncate large request/response bodies
    fn truncate_bodies(&self, mut entry: LogEntry) -> LogEntry {
        if !self.log_request_bodies {
            entry.request_body = None;
            entry.response_body = None;
            return entry;
        }

        if let Some(body) = &entry.request_body {
            let body_str = serde_json::to_string(body).unwrap_or_default();
            if body_str.len() > MAX_BODY_SIZE {
                entry.request_body = Some(serde_json::json!({
                    "_truncated": true,
                    "_size": body_str.len(),
                    "_max_size": MAX_BODY_SIZE
                }));
            }
        }

        if let Some(body) = &entry.response_body {
            let body_str = serde_json::to_string(body).unwrap_or_default();
            if body_str.len() > MAX_BODY_SIZE {
                entry.response_body = Some(serde_json::json!({
                    "_truncated": true,
                    "_size": body_str.len(),
                    "_max_size": MAX_BODY_SIZE
                }));
            }
        }

        entry
    }
}

/// Log buffer that collects entries and flushes in batches
pub struct LogBuffer {
    receiver: mpsc::Receiver<LogEntry>,
}

impl LogBuffer {
    /// Create a new log buffer
    pub fn new(receiver: mpsc::Receiver<LogEntry>) -> Self {
        Self { receiver }
    }

    /// Start processing the buffer, writing batches to the database
    pub async fn start(mut self, writer: LogWriter) {
        let mut batch: Vec<LogEntry> = Vec::with_capacity(BATCH_SIZE);
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(FLUSH_INTERVAL_SECS));

        loop {
            tokio::select! {
                // Receive new entries
                entry = self.receiver.recv() => {
                    match entry {
                        Some(e) => {
                            batch.push(e);
                            if batch.len() >= BATCH_SIZE {
                                writer.write_batch(&batch).await;
                                batch.clear();
                            }
                        }
                        None => {
                            // Channel closed, flush remaining and exit
                            if !batch.is_empty() {
                                writer.write_batch(&batch).await;
                            }
                            tracing::info!("Log buffer shutting down");
                            break;
                        }
                    }
                }
                // Periodic flush
                _ = interval.tick() => {
                    if !batch.is_empty() {
                        writer.write_batch(&batch).await;
                        batch.clear();
                    }
                }
            }
        }
    }
}

/// Log writer that writes batches to PostgreSQL
pub struct LogWriter {
    db: Arc<Database>,
}

impl LogWriter {
    /// Create a new log writer
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Write a batch of log entries to the database
    pub async fn write_batch(&self, entries: &[LogEntry]) {
        if entries.is_empty() {
            return;
        }

        match crate::core::database::insert_log_entries(&self.db, entries).await {
            Ok(count) => {
                tracing::debug!("Wrote {} log entries to database", count);
            }
            Err(e) => {
                tracing::error!("Failed to write log batch: {}", e);
            }
        }
    }
}

/// Log service managing the entire logging pipeline
pub struct LogService {
    sender: mpsc::Sender<LogEntry>,
    collector: Arc<LogCollector>,
    _shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl LogService {
    /// Initialize the log service with background writer
    pub fn new(db: Arc<Database>, log_request_bodies: bool) -> Self {
        // Create channel with buffer
        let (sender, receiver) = mpsc::channel::<LogEntry>(1000);

        // Create components
        let collector = Arc::new(LogCollector::new(sender.clone(), log_request_bodies));
        let buffer = LogBuffer::new(receiver);
        let writer = LogWriter::new(db);

        // Spawn background writer task
        tokio::spawn(async move {
            buffer.start(writer).await;
        });

        tracing::info!("Log service started");

        Self {
            sender,
            collector,
            _shutdown_tx: None,
        }
    }

    /// Get the log collector for queuing entries
    pub fn collector(&self) -> Arc<LogCollector> {
        Arc::clone(&self.collector)
    }

    /// Log a request asynchronously (convenience method)
    pub async fn log_request(&self, entry: LogEntry) {
        self.collector.log(entry).await;
    }

    /// Check if the service is running
    pub fn is_running(&self) -> bool {
        !self.sender.is_closed()
    }
}

impl Drop for LogService {
    fn drop(&mut self) {
        tracing::info!("Log service shutting down");
    }
}

/// Extract client IP from X-Forwarded-For header or connection info
pub fn extract_client_ip(headers: &axum::http::HeaderMap) -> Option<String> {
    // Try X-Forwarded-For first
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(xff_str) = xff.to_str() {
            // Take the first IP in the chain
            if let Some(first_ip) = xff_str.split(',').next() {
                return Some(first_ip.trim().to_string());
            }
        }
    }

    // Try X-Real-IP
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(ip_str) = real_ip.to_str() {
            return Some(ip_str.to_string());
        }
    }

    None
}

/// Extract user agent from headers
pub fn extract_user_agent(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("user-agent")
        .and_then(|ua| ua.to_str().ok())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_builder() {
        let entry = LogEntry::new(
            Uuid::new_v4(),
            "test-key".to_string(),
            "test-provider".to_string(),
            "/v1/chat/completions".to_string(),
            "POST".to_string(),
            Some("gpt-4".to_string()),
            false,
            200,
            100,
        )
        .with_tokens(50, 100)
        .with_ttft(Some(150))
        .with_client_info(Some("127.0.0.1".to_string()), Some("test-agent".to_string()));

        assert_eq!(entry.credential_name, "test-key");
        assert_eq!(entry.provider_name, "test-provider");
        assert_eq!(entry.prompt_tokens, 50);
        assert_eq!(entry.completion_tokens, 100);
        assert_eq!(entry.total_tokens, 150);
        assert_eq!(entry.ttft_ms, Some(150));
        assert_eq!(entry.client_ip, Some("127.0.0.1".to_string()));
    }

    #[test]
    fn test_mask_sensitive_fields() {
        let (sender, _) = mpsc::channel(10);
        let collector = LogCollector::new(sender, true);

        let input = serde_json::json!({
            "model": "gpt-4",
            "api_key": "sk-secret-key",
            "messages": [
                {"role": "user", "content": "Hello"}
            ],
            "nested": {
                "password": "secret123",
                "data": "visible"
            }
        });

        let masked = collector.mask_sensitive_fields(input);

        assert_eq!(masked["model"], "gpt-4");
        assert_eq!(masked["api_key"], "***MASKED***");
        assert_eq!(masked["nested"]["password"], "***MASKED***");
        assert_eq!(masked["nested"]["data"], "visible");
    }

    #[test]
    fn test_truncate_large_body() {
        let (sender, _) = mpsc::channel(10);
        let collector = LogCollector::new(sender, true);

        // Create a large body
        let large_content = "x".repeat(MAX_BODY_SIZE + 1000);
        let entry = LogEntry::new(
            Uuid::new_v4(),
            "test".to_string(),
            "test".to_string(),
            "/test".to_string(),
            "POST".to_string(),
            None,
            false,
            200,
            100,
        )
        .with_request_body(Some(serde_json::json!({ "content": large_content })));

        let truncated = collector.truncate_bodies(entry);

        assert!(truncated.request_body.is_some());
        let body = truncated.request_body.unwrap();
        assert_eq!(body["_truncated"], true);
    }

    #[test]
    fn test_disable_body_logging() {
        let (sender, _) = mpsc::channel(10);
        let collector = LogCollector::new(sender, false);

        let entry = LogEntry::new(
            Uuid::new_v4(),
            "test".to_string(),
            "test".to_string(),
            "/test".to_string(),
            "POST".to_string(),
            None,
            false,
            200,
            100,
        )
        .with_request_body(Some(serde_json::json!({ "content": "test" })))
        .with_response_body(Some(serde_json::json!({ "result": "ok" })));

        let processed = collector.truncate_bodies(entry);

        assert!(processed.request_body.is_none());
        assert!(processed.response_body.is_none());
    }

    #[test]
    fn test_extract_client_ip_xff() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "203.0.113.195, 70.41.3.18, 150.172.238.178".parse().unwrap(),
        );

        let ip = extract_client_ip(&headers);
        assert_eq!(ip, Some("203.0.113.195".to_string()));
    }

    #[test]
    fn test_extract_client_ip_real_ip() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-real-ip", "192.168.1.1".parse().unwrap());

        let ip = extract_client_ip(&headers);
        assert_eq!(ip, Some("192.168.1.1".to_string()));
    }

    #[test]
    fn test_extract_user_agent() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "user-agent",
            "Mozilla/5.0 (compatible; TestBot/1.0)".parse().unwrap(),
        );

        let ua = extract_user_agent(&headers);
        assert_eq!(ua, Some("Mozilla/5.0 (compatible; TestBot/1.0)".to_string()));
    }
}
