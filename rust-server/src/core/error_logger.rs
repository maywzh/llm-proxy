use axum::http::HeaderMap;
use serde_json::Value;
use sqlx::PgPool;
use std::fmt;
use std::sync::OnceLock;
use tokio::sync::mpsc;

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "cookie",
    "set-cookie",
    "proxy-authorization",
];

const MAX_BODY_SIZE: usize = 64 * 1024;

pub enum ErrorCategory {
    Provider4xx,
    Provider5xx,
    Timeout,
    NetworkError,
    ConnectError,
    StreamError,
    InternalError,
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorCategory::Provider4xx => write!(f, "provider_4xx"),
            ErrorCategory::Provider5xx => write!(f, "provider_5xx"),
            ErrorCategory::Timeout => write!(f, "timeout"),
            ErrorCategory::NetworkError => write!(f, "network_error"),
            ErrorCategory::ConnectError => write!(f, "connect_error"),
            ErrorCategory::StreamError => write!(f, "stream_error"),
            ErrorCategory::InternalError => write!(f, "internal_error"),
        }
    }
}

pub struct ErrorLogRecord {
    pub request_id: String,
    pub error_category: ErrorCategory,
    pub error_message: String,
    pub error_code: Option<i32>,
    pub endpoint: String,
    pub client_protocol: String,
    pub request_headers: Option<Value>,
    pub request_body: Option<Value>,
    pub provider_name: String,
    pub provider_api_base: String,
    pub provider_protocol: String,
    pub mapped_model: String,
    pub response_status_code: Option<i32>,
    pub response_body: Option<Value>,
    pub total_duration_ms: Option<i32>,
    pub credential_name: String,
    pub client: String,
    pub is_streaming: bool,
}

impl Default for ErrorLogRecord {
    fn default() -> Self {
        Self {
            request_id: String::new(),
            error_category: ErrorCategory::InternalError,
            error_message: String::new(),
            error_code: None,
            endpoint: String::new(),
            client_protocol: String::new(),
            request_headers: None,
            request_body: None,
            provider_name: String::new(),
            provider_api_base: String::new(),
            provider_protocol: String::new(),
            mapped_model: String::new(),
            response_status_code: None,
            response_body: None,
            total_duration_ms: None,
            credential_name: String::new(),
            client: String::new(),
            is_streaming: false,
        }
    }
}

pub struct ErrorLogger {
    tx: mpsc::Sender<ErrorLogRecord>,
}

impl ErrorLogger {
    pub fn new(pool: PgPool) -> Self {
        let (tx, rx) = mpsc::channel(500);
        tokio::spawn(Self::writer_task(rx, pool));
        Self { tx }
    }

    pub fn log(&self, record: ErrorLogRecord) {
        if let Err(e) = self.tx.try_send(record) {
            tracing::warn!("Error logger channel full, dropping record: {}", e);
        }
    }

    async fn writer_task(mut rx: mpsc::Receiver<ErrorLogRecord>, pool: PgPool) {
        let mut buffer: Vec<ErrorLogRecord> = Vec::with_capacity(50);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

        loop {
            tokio::select! {
                Some(record) = rx.recv() => {
                    buffer.push(record);
                    if buffer.len() >= 50 {
                        Self::flush(&pool, &mut buffer).await;
                    }
                }
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        Self::flush(&pool, &mut buffer).await;
                    }
                }
                else => {
                    if !buffer.is_empty() {
                        Self::flush(&pool, &mut buffer).await;
                    }
                    break;
                }
            }
        }

        tracing::info!("Error logger writer task stopped");
    }

    async fn flush(pool: &PgPool, buffer: &mut Vec<ErrorLogRecord>) {
        if buffer.is_empty() {
            return;
        }

        // Build batch INSERT with numbered parameters
        let count = buffer.len();
        let cols = 18;
        let mut sql = String::from(
            "INSERT INTO error_logs (\
             request_id, error_category, error_message, error_code, \
             endpoint, client_protocol, request_headers, request_body, \
             provider_name, provider_api_base, provider_protocol, mapped_model, \
             response_status_code, response_body, total_duration_ms, \
             credential_name, client, is_streaming\
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
            let request_body = truncate_body(&record.request_body);
            let response_body = truncate_body(&record.response_body);

            query = query
                .bind(record.request_id)
                .bind(record.error_category.to_string())
                .bind(record.error_message)
                .bind(record.error_code)
                .bind(record.endpoint)
                .bind(record.client_protocol)
                .bind(record.request_headers)
                .bind(request_body)
                .bind(record.provider_name)
                .bind(record.provider_api_base)
                .bind(record.provider_protocol)
                .bind(record.mapped_model)
                .bind(record.response_status_code)
                .bind(response_body)
                .bind(record.total_duration_ms)
                .bind(record.credential_name)
                .bind(record.client)
                .bind(record.is_streaming);
        }

        if let Err(e) = query.execute(pool).await {
            tracing::error!("Failed to flush error logs to database: {}", e);
        }
    }
}

pub fn mask_headers(headers: &HeaderMap) -> Value {
    let mut map = serde_json::Map::new();
    for (name, value) in headers.iter() {
        let key = name.as_str().to_lowercase();
        let val = if SENSITIVE_HEADERS.contains(&key.as_str()) {
            "***".to_string()
        } else {
            value.to_str().unwrap_or("<non-utf8>").to_string()
        };
        map.insert(key, Value::String(val));
    }
    Value::Object(map)
}

fn truncate_body(body: &Option<Value>) -> Option<Value> {
    let body = body.as_ref()?;
    let serialized = serde_json::to_string(body).ok()?;
    if serialized.len() <= MAX_BODY_SIZE {
        Some(body.clone())
    } else {
        Some(serde_json::json!({
            "_truncated": true,
            "_original_size": serialized.len(),
            "_preview": &serialized[..MAX_BODY_SIZE.min(serialized.len())]
        }))
    }
}

// Global singleton
static ERROR_LOGGER: OnceLock<ErrorLogger> = OnceLock::new();

pub fn init_error_logger(pool: PgPool) {
    let logger = ErrorLogger::new(pool);
    ERROR_LOGGER.set(logger).ok();
    tracing::info!("Error logger initialized");
}

pub fn log_error(record: ErrorLogRecord) {
    if let Some(logger) = ERROR_LOGGER.get() {
        logger.log(record);
    }
}
