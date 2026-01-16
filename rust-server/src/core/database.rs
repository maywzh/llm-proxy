//! Database abstraction layer for configuration persistence.
//!
//! PostgreSQL only - optimized for production use.
//! Migrations are managed externally by golang-migrate.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use std::collections::HashMap;
use std::sync::Arc;
use utoipa::ToSchema;

/// Database configuration
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout_secs: u64,
    pub idle_timeout_secs: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "postgresql://localhost/llm_proxy".to_string(),
            max_connections: 10,
            min_connections: 2,
            connect_timeout_secs: 30,
            idle_timeout_secs: 600,
        }
    }
}

impl DatabaseConfig {
    pub fn from_env() -> Result<Self, std::env::VarError> {
        let url = std::env::var("DB_URL")?;
        Ok(Self::from_url(&url))
    }

    pub fn from_url(url: &str) -> Self {
        let url = encode_password_in_url(url);

        Self {
            url,
            ..Default::default()
        }
    }
}

/// Encode special characters in the password part of a database URL.
/// Handles URLs in the format: postgresql://user:password@host:port/database
fn encode_password_in_url(url: &str) -> String {
    let url = if url.starts_with("postgres://") {
        url.replace("postgres://", "postgresql://")
    } else {
        url.to_string()
    };

    let Some(scheme_end) = url.find("://") else {
        return url;
    };

    let after_scheme = &url[scheme_end + 3..];

    let Some(at_pos) = after_scheme.rfind('@') else {
        return url;
    };

    let userinfo = &after_scheme[..at_pos];
    let host_and_rest = &after_scheme[at_pos + 1..];

    let Some(colon_pos) = userinfo.find(':') else {
        return url;
    };

    let username = &userinfo[..colon_pos];
    let password = &userinfo[colon_pos + 1..];

    if password.is_empty() {
        return url;
    }

    let encoded_password = encode_password(password);

    format!(
        "{}://{}:{}@{}",
        &url[..scheme_end],
        username,
        encoded_password,
        host_and_rest
    )
}

/// URL-encode special characters in a password string.
/// Only encodes characters that are problematic in URLs.
fn encode_password(password: &str) -> String {
    let mut encoded = String::with_capacity(password.len() * 3);
    for c in password.chars() {
        match c {
            '$' => encoded.push_str("%24"),
            '^' => encoded.push_str("%5E"),
            '@' => encoded.push_str("%40"),
            '#' => encoded.push_str("%23"),
            '&' => encoded.push_str("%26"),
            '=' => encoded.push_str("%3D"),
            '+' => encoded.push_str("%2B"),
            '/' => encoded.push_str("%2F"),
            '?' => encoded.push_str("%3F"),
            '%' => encoded.push_str("%25"),
            ':' => encoded.push_str("%3A"),
            ' ' => encoded.push_str("%20"),
            _ => encoded.push(c),
        }
    }
    encoded
}

/// Database connection manager
#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, sqlx::Error> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(std::time::Duration::from_secs(config.connect_timeout_secs))
            .idle_timeout(std::time::Duration::from_secs(config.idle_timeout_secs))
            .connect(&config.url)
            .await?;

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Check if migrations have been applied (by golang-migrate)
    pub async fn check_migrations(&self) -> Result<bool, sqlx::Error> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_name='providers')",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(exists)
    }

    /// Check if database is empty (no providers configured)
    pub async fn is_empty(&self) -> Result<bool, sqlx::Error> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM providers")
            .fetch_one(&self.pool)
            .await?;
        Ok(count == 0)
    }

    /// Get current config version
    pub async fn get_config_version(&self) -> Result<i64, sqlx::Error> {
        let version: i64 = sqlx::query_scalar("SELECT version FROM config_version WHERE id = 1")
            .fetch_one(&self.pool)
            .await?;
        Ok(version)
    }

    /// Load all enabled providers from database
    pub async fn load_providers(&self) -> Result<Vec<ProviderEntity>, sqlx::Error> {
        let providers = sqlx::query_as::<_, ProviderEntity>(
            r#"
            SELECT id, provider_key, provider_type, api_base, api_key, model_mapping, weight, is_enabled, created_at, updated_at
            FROM providers
            WHERE is_enabled = true
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(providers)
    }

    /// Load all providers (including disabled)
    pub async fn load_all_providers(&self) -> Result<Vec<ProviderEntity>, sqlx::Error> {
        let providers = sqlx::query_as::<_, ProviderEntity>(
            r#"
            SELECT id, provider_key, provider_type, api_base, api_key, model_mapping, weight, is_enabled, created_at, updated_at
            FROM providers
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(providers)
    }

    /// Get provider by ID (auto-increment integer)
    pub async fn get_provider(&self, id: i32) -> Result<Option<ProviderEntity>, sqlx::Error> {
        let provider = sqlx::query_as::<_, ProviderEntity>(
            r#"
            SELECT id, provider_key, provider_type, api_base, api_key, model_mapping, weight, is_enabled, created_at, updated_at
            FROM providers
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(provider)
    }

    /// Get provider by provider_key (unique string identifier)
    pub async fn get_provider_by_key(&self, provider_key: &str) -> Result<Option<ProviderEntity>, sqlx::Error> {
        let provider = sqlx::query_as::<_, ProviderEntity>(
            r#"
            SELECT id, provider_key, provider_type, api_base, api_key, model_mapping, weight, is_enabled, created_at, updated_at
            FROM providers
            WHERE provider_key = $1
            "#,
        )
        .bind(provider_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(provider)
    }

    /// Create a new provider
    pub async fn create_provider(&self, provider: &CreateProvider) -> Result<ProviderEntity, sqlx::Error> {
        let entity = sqlx::query_as::<_, ProviderEntity>(
            r#"
            INSERT INTO providers (provider_key, provider_type, api_base, api_key, model_mapping, weight, is_enabled)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, provider_key, provider_type, api_base, api_key, model_mapping, weight, is_enabled, created_at, updated_at
            "#,
        )
        .bind(&provider.provider_key)
        .bind(&provider.provider_type)
        .bind(&provider.api_base)
        .bind(&provider.api_key)
        .bind(sqlx::types::Json(&provider.model_mapping))
        .bind(provider.weight)
        .bind(provider.is_enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(entity)
    }

    /// Update an existing provider
    pub async fn update_provider(&self, id: i32, update: &UpdateProvider) -> Result<Option<ProviderEntity>, sqlx::Error> {
        let entity = sqlx::query_as::<_, ProviderEntity>(
            r#"
            UPDATE providers
            SET provider_type = COALESCE($2, provider_type),
                api_base = COALESCE($3, api_base),
                api_key = COALESCE($4, api_key),
                model_mapping = COALESCE($5, model_mapping),
                weight = COALESCE($6, weight),
                is_enabled = COALESCE($7, is_enabled),
                updated_at = NOW()
            WHERE id = $1
            RETURNING id, provider_key, provider_type, api_base, api_key, model_mapping, weight, is_enabled, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(&update.provider_type)
        .bind(&update.api_base)
        .bind(&update.api_key)
        .bind(update.model_mapping.as_ref().map(sqlx::types::Json))
        .bind(update.weight)
        .bind(update.is_enabled)
        .fetch_optional(&self.pool)
        .await?;
        Ok(entity)
    }

    /// Delete a provider
    pub async fn delete_provider(&self, id: i32) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM providers WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Load all enabled credentials from database
    pub async fn load_credentials(&self) -> Result<Vec<CredentialEntity>, sqlx::Error> {
        let credentials = sqlx::query_as::<_, CredentialEntity>(
            r#"
            SELECT id, credential_key, name, allowed_models, rate_limit, is_enabled, created_at, updated_at
            FROM credentials
            WHERE is_enabled = true
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(credentials)
    }

    /// Load all credentials (including disabled)
    pub async fn load_all_credentials(&self) -> Result<Vec<CredentialEntity>, sqlx::Error> {
        let credentials = sqlx::query_as::<_, CredentialEntity>(
            r#"
            SELECT id, credential_key, name, allowed_models, rate_limit, is_enabled, created_at, updated_at
            FROM credentials
            ORDER BY id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(credentials)
    }

    /// Get credential by ID (auto-increment integer)
    pub async fn get_credential(&self, id: i32) -> Result<Option<CredentialEntity>, sqlx::Error> {
        let credential = sqlx::query_as::<_, CredentialEntity>(
            r#"
            SELECT id, credential_key, name, allowed_models, rate_limit, is_enabled, created_at, updated_at
            FROM credentials
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(credential)
    }

    /// Get credential by credential_key hash (for authentication)
    pub async fn get_credential_by_key(&self, credential_key: &str) -> Result<Option<CredentialEntity>, sqlx::Error> {
        let credential = sqlx::query_as::<_, CredentialEntity>(
            r#"
            SELECT id, credential_key, name, allowed_models, rate_limit, is_enabled, created_at, updated_at
            FROM credentials
            WHERE credential_key = $1 AND is_enabled = true
            "#,
        )
        .bind(credential_key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(credential)
    }

    /// Create a new credential
    pub async fn create_credential(&self, credential: &CreateCredential) -> Result<CredentialEntity, sqlx::Error> {
        let credential_key = hash_key(&credential.key);
        let entity = sqlx::query_as::<_, CredentialEntity>(
            r#"
            INSERT INTO credentials (credential_key, name, allowed_models, rate_limit, is_enabled)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, credential_key, name, allowed_models, rate_limit, is_enabled, created_at, updated_at
            "#,
        )
        .bind(&credential_key)
        .bind(&credential.name)
        .bind(sqlx::types::Json(&credential.allowed_models))
        .bind(credential.rate_limit)
        .bind(credential.is_enabled)
        .fetch_one(&self.pool)
        .await?;
        Ok(entity)
    }

    /// Update an existing credential
    pub async fn update_credential(&self, id: i32, update: &UpdateCredential) -> Result<Option<CredentialEntity>, sqlx::Error> {
        let credential_key = update.key.as_ref().map(|k| hash_key(k));
        let entity = sqlx::query_as::<_, CredentialEntity>(
            r#"
            UPDATE credentials
            SET credential_key = COALESCE($2, credential_key),
                name = COALESCE($3, name),
                allowed_models = COALESCE($4, allowed_models),
                rate_limit = COALESCE($5, rate_limit),
                is_enabled = COALESCE($6, is_enabled),
                updated_at = NOW()
            WHERE id = $1
            RETURNING id, credential_key, name, allowed_models, rate_limit, is_enabled, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(credential_key)
        .bind(&update.name)
        .bind(update.allowed_models.as_ref().map(sqlx::types::Json))
        .bind(update.rate_limit)
        .bind(update.is_enabled)
        .fetch_optional(&self.pool)
        .await?;
        Ok(entity)
    }

    /// Delete a credential
    pub async fn delete_credential(&self, id: i32) -> Result<bool, sqlx::Error> {
        let result = sqlx::query("DELETE FROM credentials WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}

/// Provider entity from database
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "id": 1,
    "provider_key": "openai-1",
    "provider_type": "openai",
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-***",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "weight": 1,
    "is_enabled": true,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z"
}))]
pub struct ProviderEntity {
    /// Auto-increment provider ID
    pub id: i32,
    /// Unique provider key identifier
    pub provider_key: String,
    /// Provider type (e.g., "openai", "azure", "anthropic")
    pub provider_type: String,
    /// Base URL for the provider API
    pub api_base: String,
    /// API key for authentication (stored encrypted)
    #[schema(value_type = String)]
    pub api_key: String,
    /// Model name mapping (request model -> provider model)
    #[schema(value_type = HashMap<String, String>)]
    pub model_mapping: sqlx::types::Json<HashMap<String, String>>,
    /// Weight for load balancing (higher = more traffic)
    pub weight: i32,
    /// Whether this provider is enabled
    pub is_enabled: bool,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

/// Create provider request
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "provider_key": "openai-1",
    "provider_type": "openai",
    "api_base": "https://api.openai.com/v1",
    "api_key": "sk-your-api-key",
    "model_mapping": {"gpt-4": "gpt-4-turbo"},
    "weight": 1,
    "is_enabled": true
}))]
pub struct CreateProvider {
    /// Unique provider key identifier
    pub provider_key: String,
    /// Provider type (e.g., "openai", "azure", "anthropic")
    pub provider_type: String,
    /// Base URL for the provider API
    pub api_base: String,
    /// API key for authentication
    pub api_key: String,
    /// Model name mapping (request model -> provider model)
    #[serde(default)]
    pub model_mapping: HashMap<String, String>,
    /// Weight for load balancing (default: 1, higher = more traffic)
    #[serde(default = "default_weight")]
    pub weight: i32,
    /// Whether this provider is enabled (default: true)
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}

/// Update provider request
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "api_base": "https://api.openai.com/v1",
    "weight": 2,
    "is_enabled": false
}))]
pub struct UpdateProvider {
    /// Provider type (e.g., "openai", "azure", "anthropic")
    pub provider_type: Option<String>,
    /// Base URL for the provider API
    pub api_base: Option<String>,
    /// API key for authentication
    pub api_key: Option<String>,
    /// Model name mapping (request model -> provider model)
    pub model_mapping: Option<HashMap<String, String>>,
    /// Weight for load balancing (higher = more traffic)
    pub weight: Option<i32>,
    /// Whether this provider is enabled
    pub is_enabled: Option<bool>,
}

/// Credential entity from database
#[derive(Debug, Clone, FromRow, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "id": 1,
    "credential_key": "abc123...",
    "name": "Production Credential",
    "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
    "rate_limit": 100,
    "is_enabled": true,
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z"
}))]
pub struct CredentialEntity {
    /// Auto-increment credential ID
    pub id: i32,
    /// SHA-256 hash of the credential key
    pub credential_key: String,
    /// Human-readable name for the credential
    pub name: String,
    /// List of models this credential can access (empty = all models)
    #[sqlx(json)]
    pub allowed_models: Vec<String>,
    /// Rate limit in requests per second (null = unlimited)
    pub rate_limit: Option<i32>,
    /// Whether this credential is enabled
    pub is_enabled: bool,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last update timestamp
    pub updated_at: DateTime<Utc>,
}

/// Create credential request
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "key": "sk-your-credential-key",
    "name": "Production Credential",
    "allowed_models": ["gpt-4", "gpt-3.5-turbo"],
    "rate_limit": 100,
    "is_enabled": true
}))]
pub struct CreateCredential {
    /// The actual key value (will be hashed for storage)
    pub key: String,
    /// Human-readable name for the credential
    pub name: String,
    /// List of models this credential can access (empty = all models)
    #[serde(default)]
    pub allowed_models: Vec<String>,
    /// Rate limit in requests per second (null = unlimited)
    pub rate_limit: Option<i32>,
    /// Whether this credential is enabled (default: true)
    #[serde(default = "default_true")]
    pub is_enabled: bool,
}

/// Update credential request
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[schema(example = json!({
    "name": "Updated Credential Name",
    "rate_limit": 200,
    "is_enabled": false
}))]
pub struct UpdateCredential {
    /// New key value (will be hashed for storage)
    pub key: Option<String>,
    /// Human-readable name for the credential
    pub name: Option<String>,
    /// List of models this credential can access (empty = all models)
    pub allowed_models: Option<Vec<String>>,
    /// Rate limit in requests per second (null = unlimited)
    pub rate_limit: Option<i32>,
    /// Whether this credential is enabled
    pub is_enabled: Option<bool>,
}

fn default_true() -> bool {
    true
}

fn default_weight() -> i32 {
    1
}

/// Hash a key for secure storage using SHA-256
pub fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Create a preview of the key (e.g., "sk-***abc")
pub fn create_key_preview(key: &str) -> String {
    if key.len() <= 6 {
        return "***".to_string();
    }
    let prefix = &key[..3];
    let suffix = &key[key.len() - 3..];
    format!("{}***{}", prefix, suffix)
}

/// Runtime configuration loaded from database
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub providers: Vec<ProviderEntity>,
    pub credentials: Vec<CredentialEntity>,
    pub version: i64,
    pub loaded_at: DateTime<Utc>,
}

impl RuntimeConfig {
    pub async fn load_from_db(db: &Database) -> Result<Self, sqlx::Error> {
        let providers = db.load_providers().await?;
        let credentials = db.load_credentials().await?;
        let version = db.get_config_version().await?;

        Ok(Self {
            providers,
            credentials,
            version,
            loaded_at: Utc::now(),
        })
    }
}

/// Dynamic configuration manager using ArcSwap for hot reload
pub struct DynamicConfig {
    config: arc_swap::ArcSwap<RuntimeConfig>,
    db: Arc<Database>,
}

impl DynamicConfig {
    pub fn new(config: RuntimeConfig, db: Arc<Database>) -> Self {
        Self {
            config: arc_swap::ArcSwap::from_pointee(config),
            db,
        }
    }

    /// Get current configuration (zero-cost read)
    pub fn get(&self) -> arc_swap::Guard<Arc<RuntimeConfig>> {
        self.config.load()
    }

    /// Get full Arc to current configuration
    pub fn get_full(&self) -> Arc<RuntimeConfig> {
        self.config.load_full()
    }

    /// Reload configuration from database
    pub async fn reload(&self) -> Result<i64, sqlx::Error> {
        let new_config = RuntimeConfig::load_from_db(&self.db).await?;
        let version = new_config.version;
        self.config.store(Arc::new(new_config));
        tracing::info!(version = version, "Configuration reloaded from database");
        Ok(version)
    }

    /// Get database reference
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }
}

/// Insert log entries in batch
pub async fn insert_log_entries(
    db: &Database,
    entries: &[crate::services::log_service::LogEntry],
) -> Result<usize, sqlx::Error> {
    if entries.is_empty() {
        return Ok(0);
    }

    // Build batch insert query
    let mut query_builder = String::from(
        r#"INSERT INTO request_logs (
            id, created_at, credential_id, credential_name, provider_id, provider_name,
            endpoint, method, model, is_streaming, status_code, duration_ms, ttft_ms,
            prompt_tokens, completion_tokens, total_tokens, request_body, response_body,
            error_message, client_ip, user_agent
        ) VALUES "#,
    );

    let mut param_idx = 1;

    for (i, _) in entries.iter().enumerate() {
        if i > 0 {
            query_builder.push_str(", ");
        }
        let placeholders: Vec<String> = (0..21)
            .map(|j| format!("${}", param_idx + j))
            .collect();
        query_builder.push_str(&format!("({})", placeholders.join(", ")));
        param_idx += 21;
    }

    // Execute with all parameters bound
    let mut query = sqlx::query(&query_builder);

    for entry in entries {
        let request_body_json = entry
            .request_body
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        let response_body_json = entry
            .response_body
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());

        query = query
            .bind(entry.request_id)
            .bind(entry.timestamp)
            .bind(entry.credential_id)
            .bind(&entry.credential_name)
            .bind(entry.provider_id)
            .bind(&entry.provider_name)
            .bind(&entry.endpoint)
            .bind(&entry.method)
            .bind(&entry.model)
            .bind(entry.is_streaming)
            .bind(entry.status_code)
            .bind(entry.duration_ms)
            .bind(entry.ttft_ms)
            .bind(entry.prompt_tokens)
            .bind(entry.completion_tokens)
            .bind(entry.total_tokens)
            .bind(request_body_json.as_ref().map(|s| sqlx::types::Json(serde_json::from_str::<serde_json::Value>(s).unwrap_or_default())))
            .bind(response_body_json.as_ref().map(|s| sqlx::types::Json(serde_json::from_str::<serde_json::Value>(s).unwrap_or_default())))
            .bind(&entry.error_message)
            .bind(&entry.client_ip)
            .bind(&entry.user_agent);
    }

    let result = query.execute(db.pool()).await?;
    Ok(result.rows_affected() as usize)
}

/// Cleanup old log entries based on retention days
pub async fn cleanup_old_logs(db: &Database, retention_days: i32) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        r#"DELETE FROM request_logs WHERE created_at < NOW() - INTERVAL '1 day' * $1"#,
    )
    .bind(retention_days)
    .execute(db.pool())
    .await?;

    Ok(result.rows_affected())
}

/// Log entry entity from database
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub credential_id: Option<i32>,
    pub credential_name: Option<String>,
    pub provider_id: Option<i32>,
    pub provider_name: Option<String>,
    pub endpoint: String,
    pub method: String,
    pub model: Option<String>,
    pub is_streaming: bool,
    pub status_code: Option<i32>,
    pub duration_ms: Option<i32>,
    pub ttft_ms: Option<i32>,
    pub prompt_tokens: Option<i32>,
    pub completion_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub request_body: Option<sqlx::types::Json<serde_json::Value>>,
    pub response_body: Option<sqlx::types::Json<serde_json::Value>>,
    pub error_message: Option<String>,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
}

/// Parameters for log queries
#[derive(Debug, Clone, Default)]
pub struct LogQueryParams {
    pub page: i64,
    pub page_size: i64,
    pub credential_id: Option<i32>,
    pub provider_id: Option<i32>,
    pub model: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub status_code: Option<i32>,
    pub has_error: Option<bool>,
}

/// Result of log list query
#[derive(Debug, Clone)]
pub struct LogListResult {
    pub logs: Vec<LogEntry>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

/// Log statistics
#[derive(Debug, Clone, Serialize)]
pub struct LogStats {
    pub total_requests: i64,
    pub error_count: i64,
    pub error_rate: f64,
    pub avg_duration_ms: f64,
    pub total_tokens: i64,
    pub by_model: HashMap<String, ModelStats>,
    pub by_provider: HashMap<String, ProviderStats>,
}

/// Statistics per model
#[derive(Debug, Clone, Serialize)]
pub struct ModelStats {
    pub count: i64,
    pub tokens: i64,
    pub avg_duration_ms: f64,
}

/// Statistics per provider
#[derive(Debug, Clone, Serialize)]
pub struct ProviderStats {
    pub count: i64,
    pub tokens: i64,
    pub avg_duration_ms: f64,
}

impl Database {
    /// Get logs with filtering and pagination
    pub async fn get_logs(&self, params: &LogQueryParams) -> Result<LogListResult, sqlx::Error> {
        let page = if params.page < 1 { 1 } else { params.page };
        let page_size = if params.page_size < 1 {
            20
        } else if params.page_size > 100 {
            100
        } else {
            params.page_size
        };
        let offset = (page - 1) * page_size;

        // Build dynamic query for logs
        let mut query = String::from(
            r#"SELECT id, created_at, credential_id, credential_name, provider_id, provider_name,
               endpoint, method, model, is_streaming, status_code, duration_ms, ttft_ms,
               prompt_tokens, completion_tokens, total_tokens, request_body, response_body,
               error_message, client_ip, user_agent
               FROM request_logs WHERE 1=1"#,
        );

        let mut count_query = String::from("SELECT COUNT(*) FROM request_logs WHERE 1=1");

        // Build WHERE conditions
        let mut conditions = Vec::new();
        let mut param_idx = 1;

        if params.credential_id.is_some() {
            conditions.push(format!(" AND credential_id = ${}", param_idx));
            param_idx += 1;
        }
        if params.provider_id.is_some() {
            conditions.push(format!(" AND provider_id = ${}", param_idx));
            param_idx += 1;
        }
        if params.model.is_some() {
            conditions.push(format!(" AND model = ${}", param_idx));
            param_idx += 1;
        }
        if params.start_date.is_some() {
            conditions.push(format!(" AND created_at >= ${}", param_idx));
            param_idx += 1;
        }
        if params.end_date.is_some() {
            conditions.push(format!(" AND created_at <= ${}", param_idx));
            param_idx += 1;
        }
        if params.status_code.is_some() {
            conditions.push(format!(" AND status_code = ${}", param_idx));
            param_idx += 1;
        }
        if let Some(has_error) = params.has_error {
            if has_error {
                conditions.push(" AND error_message IS NOT NULL".to_string());
            } else {
                conditions.push(" AND error_message IS NULL".to_string());
            }
        }

        for condition in &conditions {
            query.push_str(condition);
            count_query.push_str(condition);
        }

        query.push_str(&format!(
            " ORDER BY created_at DESC LIMIT ${} OFFSET ${}",
            param_idx,
            param_idx + 1
        ));

        // Execute count query
        let mut count_q = sqlx::query_scalar::<_, i64>(&count_query);
        if let Some(cred_id) = params.credential_id {
            count_q = count_q.bind(cred_id);
        }
        if let Some(prov_id) = params.provider_id {
            count_q = count_q.bind(prov_id);
        }
        if let Some(ref model) = params.model {
            count_q = count_q.bind(model);
        }
        if let Some(start) = params.start_date {
            count_q = count_q.bind(start);
        }
        if let Some(end) = params.end_date {
            count_q = count_q.bind(end);
        }
        if let Some(status) = params.status_code {
            count_q = count_q.bind(status);
        }

        let total = count_q.fetch_one(&self.pool).await?;

        // Execute main query
        let mut main_q = sqlx::query_as::<_, LogEntry>(&query);
        if let Some(cred_id) = params.credential_id {
            main_q = main_q.bind(cred_id);
        }
        if let Some(prov_id) = params.provider_id {
            main_q = main_q.bind(prov_id);
        }
        if let Some(ref model) = params.model {
            main_q = main_q.bind(model);
        }
        if let Some(start) = params.start_date {
            main_q = main_q.bind(start);
        }
        if let Some(end) = params.end_date {
            main_q = main_q.bind(end);
        }
        if let Some(status) = params.status_code {
            main_q = main_q.bind(status);
        }
        main_q = main_q.bind(page_size).bind(offset);

        let logs = main_q.fetch_all(&self.pool).await?;

        Ok(LogListResult {
            logs,
            total,
            page,
            page_size,
        })
    }

    /// Get a single log entry by ID
    pub async fn get_log_by_id(&self, log_id: &str) -> Result<Option<LogEntry>, sqlx::Error> {
        let log = sqlx::query_as::<_, LogEntry>(
            r#"SELECT id, created_at, credential_id, credential_name, provider_id, provider_name,
               endpoint, method, model, is_streaming, status_code, duration_ms, ttft_ms,
               prompt_tokens, completion_tokens, total_tokens, request_body, response_body,
               error_message, client_ip, user_agent
               FROM request_logs WHERE id = $1"#,
        )
        .bind(log_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(log)
    }

    /// Delete logs older than specified date
    pub async fn delete_logs_before(&self, before_date: DateTime<Utc>) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("DELETE FROM request_logs WHERE created_at < $1")
            .bind(before_date)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected())
    }

    /// Get log statistics
    pub async fn get_log_stats(
        &self,
        start_date: Option<DateTime<Utc>>,
        end_date: Option<DateTime<Utc>>,
    ) -> Result<LogStats, sqlx::Error> {
        // Build date filter
        let mut date_filter = String::new();
        let mut param_idx = 1;

        if start_date.is_some() {
            date_filter.push_str(&format!(" AND created_at >= ${}", param_idx));
            param_idx += 1;
        }
        if end_date.is_some() {
            date_filter.push_str(&format!(" AND created_at <= ${}", param_idx));
        }

        // Get total requests
        let total_query = format!(
            "SELECT COUNT(*) FROM request_logs WHERE 1=1{}",
            date_filter
        );
        let mut total_q = sqlx::query_scalar::<_, i64>(&total_query);
        if let Some(start) = start_date {
            total_q = total_q.bind(start);
        }
        if let Some(end) = end_date {
            total_q = total_q.bind(end);
        }
        let total_requests = total_q.fetch_one(&self.pool).await?;

        // Get error count
        let error_query = format!(
            "SELECT COUNT(*) FROM request_logs WHERE error_message IS NOT NULL{}",
            date_filter
        );
        let mut error_q = sqlx::query_scalar::<_, i64>(&error_query);
        if let Some(start) = start_date {
            error_q = error_q.bind(start);
        }
        if let Some(end) = end_date {
            error_q = error_q.bind(end);
        }
        let error_count = error_q.fetch_one(&self.pool).await?;

        // Get average duration
        let avg_query = format!(
            "SELECT COALESCE(AVG(duration_ms), 0) FROM request_logs WHERE 1=1{}",
            date_filter
        );
        let mut avg_q = sqlx::query_scalar::<_, f64>(&avg_query);
        if let Some(start) = start_date {
            avg_q = avg_q.bind(start);
        }
        if let Some(end) = end_date {
            avg_q = avg_q.bind(end);
        }
        let avg_duration_ms = avg_q.fetch_one(&self.pool).await?;

        // Get total tokens
        let tokens_query = format!(
            "SELECT COALESCE(SUM(total_tokens), 0) FROM request_logs WHERE 1=1{}",
            date_filter
        );
        let mut tokens_q = sqlx::query_scalar::<_, i64>(&tokens_query);
        if let Some(start) = start_date {
            tokens_q = tokens_q.bind(start);
        }
        if let Some(end) = end_date {
            tokens_q = tokens_q.bind(end);
        }
        let total_tokens = tokens_q.fetch_one(&self.pool).await?;

        let error_rate = if total_requests > 0 {
            error_count as f64 / total_requests as f64
        } else {
            0.0
        };

        // Get stats by model
        let model_query = format!(
            r#"SELECT model, COUNT(*) as count, COALESCE(SUM(total_tokens), 0) as tokens,
               COALESCE(AVG(duration_ms), 0) as avg_duration
               FROM request_logs WHERE model IS NOT NULL{}
               GROUP BY model"#,
            date_filter
        );

        #[derive(FromRow)]
        struct ModelRow {
            model: String,
            count: i64,
            tokens: i64,
            avg_duration: f64,
        }

        let mut model_q = sqlx::query_as::<_, ModelRow>(&model_query);
        if let Some(start) = start_date {
            model_q = model_q.bind(start);
        }
        if let Some(end) = end_date {
            model_q = model_q.bind(end);
        }
        let model_rows = model_q.fetch_all(&self.pool).await?;

        let by_model: HashMap<String, ModelStats> = model_rows
            .into_iter()
            .map(|row| {
                (
                    row.model,
                    ModelStats {
                        count: row.count,
                        tokens: row.tokens,
                        avg_duration_ms: row.avg_duration,
                    },
                )
            })
            .collect();

        // Get stats by provider
        let provider_query = format!(
            r#"SELECT provider_name, COUNT(*) as count, COALESCE(SUM(total_tokens), 0) as tokens,
               COALESCE(AVG(duration_ms), 0) as avg_duration
               FROM request_logs WHERE provider_name IS NOT NULL{}
               GROUP BY provider_name"#,
            date_filter
        );

        #[derive(FromRow)]
        struct ProviderRow {
            provider_name: String,
            count: i64,
            tokens: i64,
            avg_duration: f64,
        }

        let mut provider_q = sqlx::query_as::<_, ProviderRow>(&provider_query);
        if let Some(start) = start_date {
            provider_q = provider_q.bind(start);
        }
        if let Some(end) = end_date {
            provider_q = provider_q.bind(end);
        }
        let provider_rows = provider_q.fetch_all(&self.pool).await?;

        let by_provider: HashMap<String, ProviderStats> = provider_rows
            .into_iter()
            .map(|row| {
                (
                    row.provider_name,
                    ProviderStats {
                        count: row.count,
                        tokens: row.tokens,
                        avg_duration_ms: row.avg_duration,
                    },
                )
            })
            .collect();

        Ok(LogStats {
            total_requests,
            error_count,
            error_rate,
            avg_duration_ms,
            total_tokens,
            by_model,
            by_provider,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_key() {
        let key = "sk-test-key-12345";
        let hash = hash_key(key);
        assert_eq!(hash.len(), 64);
        
        let hash2 = hash_key(key);
        assert_eq!(hash, hash2);
        
        let different_hash = hash_key("different-key");
        assert_ne!(hash, different_hash);
    }

    #[test]
    fn test_create_key_preview() {
        assert_eq!(create_key_preview("sk-test-key-12345"), "sk-***345");
        assert_eq!(create_key_preview("short"), "***");
        assert_eq!(create_key_preview("123456"), "***");
        assert_eq!(create_key_preview("1234567"), "123***567");
    }

    #[test]
    fn test_database_config_from_url() {
        let config = DatabaseConfig::from_url("postgres://user:pass@localhost/db");
        assert_eq!(config.url, "postgresql://user:pass@localhost/db");

        let config2 = DatabaseConfig::from_url("postgresql://user:pass@localhost/db");
        assert_eq!(config2.url, "postgresql://user:pass@localhost/db");
    }

    #[test]
    fn test_encode_password_in_url() {
        assert_eq!(
            encode_password_in_url("postgresql://user:EPVr$mtFHghus^Qx@localhost:5432/llm_proxy"),
            "postgresql://user:EPVr%24mtFHghus%5EQx@localhost:5432/llm_proxy"
        );

        assert_eq!(
            encode_password_in_url("postgres://user:pass@localhost/db"),
            "postgresql://user:pass@localhost/db"
        );

        assert_eq!(
            encode_password_in_url("postgresql://user:p@ss#word&test=1@localhost/db"),
            "postgresql://user:p%40ss%23word%26test%3D1@localhost/db"
        );

        assert_eq!(
            encode_password_in_url("postgresql://user@localhost/db"),
            "postgresql://user@localhost/db"
        );

        assert_eq!(
            encode_password_in_url("postgresql://user:@localhost/db"),
            "postgresql://user:@localhost/db"
        );

        assert_eq!(
            encode_password_in_url("postgresql://localhost/db"),
            "postgresql://localhost/db"
        );

        assert_eq!(
            encode_password_in_url("postgresql://user:100%done@localhost/db"),
            "postgresql://user:100%25done@localhost/db"
        );
    }

    #[test]
    fn test_encode_password() {
        assert_eq!(encode_password("simple"), "simple");
        assert_eq!(encode_password("EPVr$mtFHghus^Qx"), "EPVr%24mtFHghus%5EQx");
        assert_eq!(encode_password("p@ss#word"), "p%40ss%23word");
        assert_eq!(encode_password("a&b=c+d/e?f%g"), "a%26b%3Dc%2Bd%2Fe%3Ff%25g");
        assert_eq!(encode_password("with space"), "with%20space");
        assert_eq!(encode_password("user:pass"), "user%3Apass");
    }
}