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

const MAX_STRING_LEN: usize = 200;

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
    pub provider_request_body: Option<Value>,
    pub provider_request_headers: Option<Value>,
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
            provider_request_body: None,
            provider_request_headers: None,
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
        let cols = 20;
        let mut sql = String::from(
            "INSERT INTO error_logs (\
             request_id, error_category, error_message, error_code, \
             endpoint, client_protocol, request_headers, request_body, \
             provider_name, provider_api_base, provider_protocol, mapped_model, \
             response_status_code, response_body, total_duration_ms, \
             credential_name, client, is_streaming, \
             provider_request_body, provider_request_headers\
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
            let provider_request_body = truncate_body(&record.provider_request_body);
            let provider_request_headers = truncate_body(&record.provider_request_headers);

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
                .bind(record.is_streaming)
                .bind(provider_request_body)
                .bind(provider_request_headers);
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

const MAX_TRUNCATE_DEPTH: usize = 10;

fn truncate_body(body: &Option<Value>) -> Option<Value> {
    let body = body.as_ref()?;
    Some(truncate_json_strings_inner(body, MAX_TRUNCATE_DEPTH))
}

/// Recursively truncate long string values in a JSON structure while preserving
/// the complete JSON shape. If a string value is itself valid JSON, parse and
/// recurse into it, then serialize back. Plain strings longer than MAX_STRING_LEN
/// are shortened; keys, numbers, booleans, nulls are kept intact.
#[cfg(test)]
fn truncate_json_strings(value: &Value) -> Value {
    truncate_json_strings_inner(value, MAX_TRUNCATE_DEPTH)
}

fn truncate_json_strings_inner(value: &Value, depth: usize) -> Value {
    match value {
        Value::String(s) => {
            if depth > 0 && (s.starts_with('{') || s.starts_with('[')) {
                if let Ok(parsed) = serde_json::from_str::<Value>(s) {
                    let truncated = truncate_json_strings_inner(&parsed, depth - 1);
                    return Value::String(
                        serde_json::to_string(&truncated).unwrap_or_else(|_| s.clone()),
                    );
                }
            }
            if s.len() <= MAX_STRING_LEN {
                value.clone()
            } else {
                let mut end = MAX_STRING_LEN;
                while end > 0 && !s.is_char_boundary(end) {
                    end -= 1;
                }
                Value::String(format!(
                    "{}... [truncated, {} bytes total]",
                    &s[..end],
                    s.len()
                ))
            }
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| truncate_json_strings_inner(v, depth))
                .collect(),
        ),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), truncate_json_strings_inner(v, depth)))
                .collect(),
        ),
        _ => value.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn long_string(len: usize) -> String {
        "a".repeat(len)
    }

    #[test]
    fn test_short_string_unchanged() {
        let val = json!("hello");
        let result = truncate_json_strings(&val);
        assert_eq!(result, json!("hello"));
    }

    #[test]
    fn test_exact_limit_string_unchanged() {
        let s = long_string(MAX_STRING_LEN);
        let val = json!(s);
        let result = truncate_json_strings(&val);
        assert_eq!(result, json!(s));
    }

    #[test]
    fn test_long_string_truncated() {
        let s = long_string(MAX_STRING_LEN + 50);
        let val = json!(s);
        let result = truncate_json_strings(&val);
        let result_str = result.as_str().unwrap();
        assert!(result_str.contains("... [truncated,"));
        assert!(result_str.contains(&format!("{} bytes total]", MAX_STRING_LEN + 50)));
        assert!(result_str.starts_with(&"a".repeat(MAX_STRING_LEN)));
    }

    #[test]
    fn test_number_bool_null_unchanged() {
        assert_eq!(truncate_json_strings(&json!(42)), json!(42));
        assert_eq!(truncate_json_strings(&json!(1.23)), json!(1.23));
        assert_eq!(truncate_json_strings(&json!(true)), json!(true));
        assert_eq!(truncate_json_strings(&json!(false)), json!(false));
        assert_eq!(truncate_json_strings(&json!(null)), json!(null));
    }

    #[test]
    fn test_object_structure_preserved() {
        let long = long_string(300);
        let val = json!({
            "short": "ok",
            "long": long,
            "num": 123
        });
        let result = truncate_json_strings(&val);
        let obj = result.as_object().unwrap();
        assert_eq!(obj["short"], json!("ok"));
        assert_eq!(obj["num"], json!(123));
        assert!(obj["long"].as_str().unwrap().contains("[truncated,"));
    }

    #[test]
    fn test_nested_object_structure_preserved() {
        let long = long_string(300);
        let val = json!({
            "level1": {
                "level2": {
                    "data": long,
                    "flag": true
                },
                "name": "short"
            }
        });
        let result = truncate_json_strings(&val);
        let l1 = result["level1"].as_object().unwrap();
        let l2 = l1["level2"].as_object().unwrap();
        assert!(l2["data"].as_str().unwrap().contains("[truncated,"));
        assert_eq!(l2["flag"], json!(true));
        assert_eq!(l1["name"], json!("short"));
    }

    #[test]
    fn test_array_elements_truncated() {
        let long = long_string(300);
        let val = json!(["short", long, 42]);
        let result = truncate_json_strings(&val);
        let arr = result.as_array().unwrap();
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0], json!("short"));
        assert!(arr[1].as_str().unwrap().contains("[truncated,"));
        assert_eq!(arr[2], json!(42));
    }

    #[test]
    fn test_embedded_json_string_recursed() {
        let inner_long = long_string(500);
        let inner_json = json!({"key": inner_long, "x": 1});
        let embedded = serde_json::to_string(&inner_json).unwrap();
        let val = json!({"payload": embedded});

        let result = truncate_json_strings(&val);
        // payload should still be a string, but the inner JSON should be truncated
        let payload_str = result["payload"].as_str().unwrap();
        let parsed_back: Value = serde_json::from_str(payload_str).unwrap();
        assert!(parsed_back["key"].as_str().unwrap().contains("[truncated,"));
        assert_eq!(parsed_back["x"], json!(1));
    }

    #[test]
    fn test_embedded_json_array_string_recursed() {
        let inner_long = long_string(500);
        let inner_json = json!([inner_long, "short"]);
        let embedded = serde_json::to_string(&inner_json).unwrap();
        let val = json!(embedded);

        let result = truncate_json_strings(&val);
        let result_str = result.as_str().unwrap();
        let parsed_back: Value = serde_json::from_str(result_str).unwrap();
        let arr = parsed_back.as_array().unwrap();
        assert!(arr[0].as_str().unwrap().contains("[truncated,"));
        assert_eq!(arr[1], json!("short"));
    }

    #[test]
    fn test_non_json_string_starting_with_brace_not_parsed() {
        let val = json!("{this is not valid json at all");
        let result = truncate_json_strings(&val);
        assert_eq!(result, json!("{this is not valid json at all"));
    }

    #[test]
    fn test_deeply_nested_embedded_json() {
        let inner_long = long_string(400);
        let deep = json!({"deep": inner_long});
        let mid = json!({"nested": serde_json::to_string(&deep).unwrap()});
        let outer = json!({"outer": serde_json::to_string(&mid).unwrap()});

        let result = truncate_json_strings(&outer);
        // Parse outer embedded string
        let outer_parsed: Value = serde_json::from_str(result["outer"].as_str().unwrap()).unwrap();
        // Parse mid embedded string
        let mid_parsed: Value =
            serde_json::from_str(outer_parsed["nested"].as_str().unwrap()).unwrap();
        assert!(mid_parsed["deep"].as_str().unwrap().contains("[truncated,"));
    }

    #[test]
    fn test_truncate_body_none() {
        assert_eq!(truncate_body(&None), None);
    }

    #[test]
    fn test_truncate_body_some() {
        let long = long_string(300);
        let body = Some(json!({"msg": long, "id": 1}));
        let result = truncate_body(&body).unwrap();
        let obj = result.as_object().unwrap();
        assert!(obj["msg"].as_str().unwrap().contains("[truncated,"));
        assert_eq!(obj["id"], json!(1));
    }

    #[test]
    fn test_empty_object_and_array() {
        assert_eq!(truncate_json_strings(&json!({})), json!({}));
        assert_eq!(truncate_json_strings(&json!([])), json!([]));
    }

    #[test]
    fn test_realistic_anthropic_payload() {
        let long_content = long_string(5000);
        let val = json!({
            "model": "claude-3-5-sonnet",
            "max_tokens": 4096,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {"type": "text", "text": long_content},
                        {"type": "tool_result", "tool_use_id": "tooluse_abc123", "content": "short result"}
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "id": "tooluse_abc123", "name": "get_weather", "input": {"city": "Shanghai"}},
                        {"type": "text", "text": "Here is the weather."}
                    ]
                }
            ],
            "stream": true
        });
        let result = truncate_json_strings(&val);

        // Structure fully preserved
        assert_eq!(result["model"], json!("claude-3-5-sonnet"));
        assert_eq!(result["max_tokens"], json!(4096));
        assert_eq!(result["stream"], json!(true));

        let msgs = result["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 2);

        // Long text truncated
        let user_content = msgs[0]["content"].as_array().unwrap();
        assert!(user_content[0]["text"]
            .as_str()
            .unwrap()
            .contains("[truncated,"));
        assert_eq!(user_content[0]["type"], json!("text"));

        // Short fields preserved
        assert_eq!(user_content[1]["tool_use_id"], json!("tooluse_abc123"));
        assert_eq!(user_content[1]["content"], json!("short result"));

        // tool_use block structure preserved
        let asst_content = msgs[1]["content"].as_array().unwrap();
        assert_eq!(asst_content[0]["type"], json!("tool_use"));
        assert_eq!(asst_content[0]["id"], json!("tooluse_abc123"));
        assert_eq!(asst_content[0]["name"], json!("get_weather"));
        assert_eq!(asst_content[0]["input"]["city"], json!("Shanghai"));
        assert_eq!(asst_content[1]["text"], json!("Here is the weather."));
    }
}
