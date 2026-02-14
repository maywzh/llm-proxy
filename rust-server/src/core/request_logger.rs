//! Async request logger that batches ALL request records into the database.
//!
//! Mirrors the error_logger.rs pattern: MPSC channel → batch INSERT.
//! Controlled by `REQUEST_LOG_ENABLED` (default true) and
//! `REQUEST_LOG_BODY_ENABLED` (default false) environment variables.

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::sync::{Mutex, OnceLock};
use tokio::sync::{mpsc, oneshot};

pub struct RequestLogRecord {
    pub request_id: String,
    pub endpoint: Option<String>,
    pub credential_name: Option<String>,
    pub model_requested: Option<String>,
    pub model_mapped: Option<String>,
    pub provider_name: Option<String>,
    pub provider_type: Option<String>,
    pub client_protocol: Option<String>,
    pub provider_protocol: Option<String>,
    pub is_streaming: bool,
    pub status_code: Option<i32>,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub total_tokens: i32,
    pub total_duration_ms: Option<i32>,
    pub ttft_ms: Option<i32>,
    pub error_category: Option<String>,
    pub error_message: Option<String>,
    pub request_headers: Option<String>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub timestamp: DateTime<Utc>,
}

impl Default for RequestLogRecord {
    fn default() -> Self {
        Self {
            request_id: String::new(),
            endpoint: None,
            credential_name: None,
            model_requested: None,
            model_mapped: None,
            provider_name: None,
            provider_type: None,
            client_protocol: None,
            provider_protocol: None,
            is_streaming: false,
            status_code: None,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            total_duration_ms: None,
            ttft_ms: None,
            error_category: None,
            error_message: None,
            request_headers: None,
            request_body: None,
            response_body: None,
            timestamp: Utc::now(),
        }
    }
}

pub struct RequestLogger {
    tx: mpsc::Sender<RequestLogRecord>,
    done_rx: Mutex<Option<oneshot::Receiver<()>>>,
}

impl RequestLogger {
    pub fn new(pool: PgPool, body_enabled: bool) -> Self {
        let (tx, rx) = mpsc::channel(1000);
        let (done_tx, done_rx) = oneshot::channel();
        tokio::spawn(Self::writer_task(rx, pool, body_enabled, done_tx));
        Self {
            tx,
            done_rx: Mutex::new(Some(done_rx)),
        }
    }

    pub fn log(&self, record: RequestLogRecord) {
        if let Err(e) = self.tx.try_send(record) {
            tracing::warn!("Request log channel full, dropping record: {}", e);
        }
    }

    async fn writer_task(
        mut rx: mpsc::Receiver<RequestLogRecord>,
        pool: PgPool,
        body_enabled: bool,
        done_tx: oneshot::Sender<()>,
    ) {
        let mut buffer: Vec<RequestLogRecord> = Vec::with_capacity(50);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

        loop {
            tokio::select! {
                Some(record) = rx.recv() => {
                    buffer.push(record);
                    if buffer.len() >= 50 {
                        Self::flush(&pool, &mut buffer, body_enabled).await;
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        Self::flush(&pool, &mut buffer, body_enabled).await;
                    }
                }
                else => {
                    // Channel closed — flush remaining records
                    if !buffer.is_empty() {
                        Self::flush(&pool, &mut buffer, body_enabled).await;
                    }
                    break;
                }
            }
        }

        tracing::info!("Request logger writer task stopped");
        let _ = done_tx.send(());
    }

    async fn flush(pool: &PgPool, buffer: &mut Vec<RequestLogRecord>, body_enabled: bool) {
        if buffer.is_empty() {
            return;
        }

        let count = buffer.len();
        let cols = 22;
        let mut sql = String::from(
            "INSERT INTO request_logs (\
             timestamp, request_id, endpoint, credential_name, \
             model_requested, model_mapped, provider_name, provider_type, \
             client_protocol, provider_protocol, is_streaming, status_code, \
             input_tokens, output_tokens, total_tokens, \
             total_duration_ms, ttft_ms, \
             error_category, error_message, \
             request_headers, request_body, response_body\
             ) VALUES ",
        );

        for i in 0..count {
            if i > 0 {
                sql.push_str(", ");
            }
            let base = i * cols + 1;
            sql.push('(');
            for j in 0..cols {
                if j > 0 {
                    sql.push_str(", ");
                }
                sql.push('$');
                sql.push_str(&(base + j).to_string());
            }
            sql.push(')');
        }

        let mut query = sqlx::query(&sql);

        for record in buffer.drain(..) {
            let request_body = if body_enabled {
                truncate_string(&record.request_body, 10000)
            } else {
                None
            };
            let response_body = if body_enabled {
                truncate_string(&record.response_body, 10000)
            } else {
                None
            };

            query = query
                .bind(record.timestamp)
                .bind(record.request_id)
                .bind(record.endpoint)
                .bind(record.credential_name)
                .bind(record.model_requested)
                .bind(record.model_mapped)
                .bind(record.provider_name)
                .bind(record.provider_type)
                .bind(record.client_protocol)
                .bind(record.provider_protocol)
                .bind(record.is_streaming)
                .bind(record.status_code)
                .bind(record.input_tokens)
                .bind(record.output_tokens)
                .bind(record.total_tokens)
                .bind(record.total_duration_ms)
                .bind(record.ttft_ms)
                .bind(record.error_category)
                .bind(record.error_message)
                .bind(record.request_headers)
                .bind(request_body)
                .bind(response_body);
        }

        if let Err(e) = query.execute(pool).await {
            tracing::error!("Failed to flush request logs to database: {}", e);
        }
    }
}

fn truncate_string(s: &Option<String>, max_len: usize) -> Option<String> {
    s.as_ref().map(|val| {
        if val.len() <= max_len {
            val.clone()
        } else {
            let mut end = max_len;
            while end > 0 && !val.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}... [truncated, {} bytes total]", &val[..end], val.len())
        }
    })
}

// Mutex<Option<…>> allows shutdown to take (drop) the sender, triggering writer flush
static REQUEST_LOGGER: OnceLock<Mutex<Option<RequestLogger>>> = OnceLock::new();

pub fn init_request_logger(pool: PgPool) {
    let enabled = std::env::var("REQUEST_LOG_ENABLED")
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(true);

    if !enabled {
        tracing::info!("Request logging is disabled");
        return;
    }

    let body_enabled = std::env::var("REQUEST_LOG_BODY_ENABLED")
        .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
        .unwrap_or(false);

    let logger = RequestLogger::new(pool, body_enabled);
    REQUEST_LOGGER.get_or_init(|| Mutex::new(Some(logger)));
    tracing::info!(
        "Request logger initialized (body_logging={})",
        if body_enabled { "enabled" } else { "disabled" }
    );
}

pub fn log_request_record(record: RequestLogRecord) {
    if let Some(mutex) = REQUEST_LOGGER.get() {
        if let Ok(guard) = mutex.lock() {
            if let Some(ref logger) = *guard {
                logger.log(record);
            }
        }
    }
}

/// Graceful shutdown: drops the sender so the writer task flushes remaining buffer and exits.
/// Waits for the writer task to complete via oneshot channel.
pub async fn shutdown_request_logger() {
    if let Some(mutex) = REQUEST_LOGGER.get() {
        let (taken, done_rx) = {
            let mut guard = mutex.lock().unwrap_or_else(|e| e.into_inner());
            let logger = guard.take();
            let rx = logger
                .as_ref()
                .and_then(|l| l.done_rx.lock().ok().and_then(|mut r| r.take()));
            (logger, rx)
        };
        if taken.is_some() {
            drop(taken);
            if let Some(rx) = done_rx {
                let _ = tokio::time::timeout(std::time::Duration::from_secs(5), rx).await;
            }
            tracing::info!("Request logger shut down");
        }
    }
}
