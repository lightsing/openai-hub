use crate::config::{AuditBackendType, AuditConfig};
use base64::engine::general_purpose;
use base64::Engine;
use chrono::serde::ts_milliseconds;
use rand::distributions::{Alphanumeric, DistString};
use rand::thread_rng;
use serde::{Deserialize, Serialize, Serializer};
use sqlx::{MySql, Pool, Postgres, Sqlite};
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tracing::{event, Level};

#[async_trait::async_trait]
pub trait BackendEngine {
    async fn init(&self) -> Result<(), BackendCreationError> {
        Ok(())
    }
    async fn log_access(&self, access: AccessLog);
    async fn log_tokens(&self, tokens: TokenUsageLog);
}

#[derive(Default, Debug, Serialize)]
pub struct AccessLog {
    #[serde(with = "ts_milliseconds")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    pub ray_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "might_as_base64_option"
    )]
    pub body: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_headers: Option<BTreeMap<String, String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "might_as_base64_option"
    )]
    pub response_body: Option<Vec<u8>>,
}

impl AccessLog {
    pub fn now() -> Self {
        let ray_id = Alphanumeric.sample_string(&mut thread_rng(), 16);
        Self {
            timestamp: chrono::Utc::now(),
            ray_id,
            ..Default::default()
        }
    }

    fn body_as_string(&self) -> Option<String> {
        self.body.as_ref().map(|b| {
            String::from_utf8(b.clone()).unwrap_or_else(|_| general_purpose::STANDARD.encode(b))
        })
    }

    fn response_body_as_string(&self) -> Option<String> {
        self.response_body.as_ref().map(|b| {
            String::from_utf8(b.clone()).unwrap_or_else(|_| general_purpose::STANDARD.encode(b))
        })
    }
}

#[derive(Debug, Serialize)]
pub struct TokenUsageLog {
    #[serde(with = "ts_milliseconds")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    pub ray_id: String,
    pub model: String,
    pub usage: TokenUsage,
    pub is_estimated: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum BackendCreationError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    DatabaseError(#[from] sqlx::Error),
}

#[derive(Clone)]
pub enum Backend {
    Text(TextBackend),
    Database(DatabaseBackend),
}

impl Backend {
    pub async fn create_with(config: &AuditConfig) -> Result<Self, BackendCreationError> {
        let this = match config.backend {
            AuditBackendType::File => Self::Text(TextBackend::create_with(config).await?),
            _ => Self::Database(DatabaseBackend::create_with(config).await?),
        };
        this.init().await?;
        Ok(this)
    }
}

#[async_trait::async_trait]
impl BackendEngine for Backend {
    async fn init(&self) -> Result<(), BackendCreationError> {
        match self {
            Self::Text(backend) => backend.init().await,
            Self::Database(backend) => backend.init().await,
        }
    }

    async fn log_access(&self, access: AccessLog) {
        match self {
            Backend::Text(backend) => backend.log_access(access).await,
            Backend::Database(backend) => backend.log_access(access).await,
        }
    }

    async fn log_tokens(&self, tokens: TokenUsageLog) {
        match self {
            Backend::Text(backend) => backend.log_tokens(tokens).await,
            Backend::Database(backend) => backend.log_tokens(tokens).await,
        }
    }
}

#[derive(Clone)]
pub struct TextBackend {
    writer: Arc<Mutex<tokio::fs::File>>,
}

impl TextBackend {
    async fn create_with(config: &AuditConfig) -> Result<Self, BackendCreationError> {
        let writer = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&config.backends.file_backend.filename)
            .await?;
        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
        })
    }
}

#[async_trait::async_trait]
impl BackendEngine for TextBackend {
    async fn log_access(&self, access: AccessLog) {
        let mut writer = self.writer.lock().await;
        let mut vec = serde_json::to_vec(&access).unwrap();
        vec.push(b'\n');
        if let Err(e) = writer.write_all(&vec).await {
            event!(
                Level::ERROR,
                error = ?e,
                "Failed to write access log to file"
            );
        }
    }

    async fn log_tokens(&self, tokens: TokenUsageLog) {
        let mut writer = self.writer.lock().await;
        let mut vec = serde_json::to_vec(&tokens).unwrap();
        vec.push(b'\n');
        if let Err(e) = writer.write_all(&vec).await {
            event!(
                Level::ERROR,
                error = ?e,
                "Failed to write tokens log to file"
            );
        }
    }
}

#[derive(Clone)]
pub enum DatabaseBackend {
    Sqlite(Pool<Sqlite>),
    MySql(Pool<MySql>),
    Postgres(Pool<Postgres>),
}

impl DatabaseBackend {
    async fn create_with(config: &AuditConfig) -> Result<Self, BackendCreationError> {
        Ok(match config.backend {
            AuditBackendType::Sqlite => {
                Self::Sqlite(Pool::connect_with((&config.backends.sqlite_backend).into()).await?)
            }
            AuditBackendType::Mysql => {
                Self::MySql(Pool::connect_with((&config.backends.mysql_backend).into()).await?)
            }
            AuditBackendType::Postgres => Self::Postgres(
                Pool::connect_with((&config.backends.postgres_backend).into()).await?,
            ),
            _ => unreachable!(),
        })
    }
}

#[async_trait::async_trait]
impl BackendEngine for DatabaseBackend {
    async fn init(&self) -> Result<(), BackendCreationError> {
        match self {
            Self::Sqlite(pool) => pool.init().await,
            Self::MySql(pool) => pool.init().await,
            Self::Postgres(pool) => pool.init().await,
        }
    }

    async fn log_access(&self, access: AccessLog) {
        match self {
            DatabaseBackend::Sqlite(pool) => pool.log_access(access).await,
            DatabaseBackend::MySql(pool) => pool.log_access(access).await,
            DatabaseBackend::Postgres(pool) => pool.log_access(access).await,
        }
    }

    async fn log_tokens(&self, tokens: TokenUsageLog) {
        match self {
            DatabaseBackend::Sqlite(pool) => pool.log_tokens(tokens).await,
            DatabaseBackend::MySql(pool) => pool.log_tokens(tokens).await,
            DatabaseBackend::Postgres(pool) => pool.log_tokens(tokens).await,
        }
    }
}

#[async_trait::async_trait]
impl BackendEngine for Pool<Sqlite> {
    async fn init(&self) -> Result<(), BackendCreationError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp DATETIME NOT NULL,
    ray_id TEXT NOT NULL,
    user TEXT,
    method TEXT,
    uri TEXT,
    headers TEXT,
    body TEXT,
    response_status INTEGER,
    response_headers TEXT,
    response_body TEXT
)"#,
        )
        .execute(self)
        .await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS tokens_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp DATETIME,
    ray_id TEXT NOT NULL,
    user TEXT,
    model TEXT NOT NULL,
    is_estimated BOOLEAN NOT NULL,
    prompt_tokens INTEGER NOT NULL,
    completion_tokens INTEGER NOT NULL,
    total_tokens INTEGER NOT NULL
)"#,
        )
        .execute(self)
        .await?;
        Ok(())
    }
    async fn log_access(&self, log: AccessLog) {
        let body = log.body_as_string();
        let response_body = log.response_body_as_string();
        let result = sqlx::query(r#"INSERT INTO audit_log (timestamp, ray_id, user, method, uri, headers, body, response_status, response_headers, response_body)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#)
            .bind(log.timestamp)
            .bind(log.ray_id)
            .bind(log.user)
            .bind(log.method)
            .bind(log.uri)
            .bind(serde_json::to_string(&log.headers).unwrap())
            .bind(body)
            .bind(log.response_status)
            .bind(serde_json::to_string(&log.response_headers).unwrap())
            .bind(response_body)
            .execute(self)
            .await;
        if let Err(e) = result {
            event!(
                Level::ERROR,
                error = ?e,
                "Failed to write access log to sqlite"
            );
        }
    }

    async fn log_tokens(&self, tokens: TokenUsageLog) {
        let result = sqlx::query(r#"INSERT INTO tokens_log (timestamp, ray_id, user, model, is_estimated, prompt_tokens, completion_tokens, total_tokens)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#)
            .bind(tokens.timestamp)
            .bind(tokens.ray_id)
            .bind(tokens.user)
            .bind(tokens.model)
            .bind(tokens.is_estimated)
            .bind(tokens.usage.prompt_tokens as u32)
            .bind(tokens.usage.completion_tokens as u32)
            .bind(tokens.usage.total_tokens as u32)
            .execute(self)
            .await;
        if let Err(e) = result {
            event!(
                Level::ERROR,
                error = ?e,
                "Failed to write tokens log to sqlite"
            );
        }
    }
}

#[async_trait::async_trait]
impl BackendEngine for Pool<MySql> {
    async fn init(&self) -> Result<(), BackendCreationError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTO_INCREMENT,
    timestamp TIMESTAMP NOT NULL,
    ray_id VARCHAR(16) NOT NULL,
    user VARCHAR(255),
    method VARCHAR(10),
    uri VARCHAR(255),
    headers TEXT,
    body TEXT,
    response_status SMALLINT UNSIGNED,
    response_headers TEXT,
    response_body TEXT
    )"#,
        )
        .execute(self)
        .await?;
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS tokens_log (
    id INTEGER PRIMARY KEY AUTO_INCREMENT,
    timestamp TIMESTAMP NOT NULL,
    ray_id VARCHAR(16) NOT NULL,
    user VARCHAR(255),
    model VARCHAR(255) NOT NULL,
    is_estimated BOOLEAN NOT NULL,
    prompt_tokens BIGINT UNSIGNED NOT NULL,
    completion_tokens BIGINT UNSIGNED NOT NULL,
    total_tokens BIGINT UNSIGNED NOT NULL
    )"#,
        )
        .execute(self)
        .await?;
        Ok(())
    }

    async fn log_access(&self, log: AccessLog) {
        let body = log.body_as_string();
        let response_body = log.response_body_as_string();
        let result = sqlx::query(r#"INSERT INTO audit_log (timestamp, ray_id, user, method, uri, headers, body, response_status, response_headers, response_body)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#)
            .bind(log.timestamp)
            .bind(log.ray_id)
            .bind(log.user)
            .bind(log.method)
            .bind(log.uri)
            .bind(serde_json::to_string(&log.headers).unwrap())
            .bind(body)
            .bind(log.response_status)
            .bind(serde_json::to_string(&log.response_headers).unwrap())
            .bind(response_body)
            .execute(self)
            .await;
        if let Err(e) = result {
            event!(
                Level::ERROR,
                error = ?e,
                "Failed to write access log to MySql"
            );
        }
    }

    async fn log_tokens(&self, tokens: TokenUsageLog) {
        let result = sqlx::query(r#"INSERT INTO tokens_log (timestamp, ray_id, user, model, is_estimated, prompt_tokens, completion_tokens, total_tokens)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)"#)
            .bind(tokens.timestamp)
            .bind(tokens.ray_id)
            .bind(tokens.user)
            .bind(tokens.model)
            .bind(tokens.is_estimated)
            .bind(tokens.usage.prompt_tokens as u64)
            .bind(tokens.usage.completion_tokens as u64)
            .bind(tokens.usage.total_tokens as u64)
            .execute(self)
            .await;
        if let Err(e) = result {
            event!(
                Level::ERROR,
                error = ?e,
                "Failed to write tokens log to sqlite"
            );
        }
    }
}

#[async_trait::async_trait]
impl BackendEngine for Pool<Postgres> {
    async fn init(&self) -> Result<(), BackendCreationError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS audit_log (
    id SERIAL PRIMARY KEY,
    timestamp TIMESTAMPTZ NOT NULL,
    ray_id VARCHAR(16) NOT NULL,
    user VARCHAR(255),
    method VARCHAR(10),
    uri VARCHAR(255),
    headers TEXT,
    body TEXT,
    response_status SMALLINT,
    response_headers TEXT,
    response_body TEXT
    )"#,
        )
        .execute(self)
        .await?;

        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS tokens_log (
    id INTEGER PRIMARY KEY AUTO_INCREMENT,
    timestamp TIMESTAMPTZ NOT NULL,
    ray_id VARCHAR(16) NOT NULL,
    user VARCHAR(255),
    model VARCHAR(255) NOT NULL,
    is_estimated BOOL NOT NULL,
    prompt_tokens BIGINT NOT NULL,
    completion_tokens BIGINT NOT NULL,
    total_tokens BIGINT NOT NULL
    )"#,
        )
        .execute(self)
        .await?;
        Ok(())
    }

    async fn log_access(&self, log: AccessLog) {
        let body = log.body_as_string();
        let response_body = log.response_body_as_string();
        let result = sqlx::query(r#"INSERT INTO audit_log (timestamp, ray_id, user, method, uri, headers, body, response_status, response_headers, response_body)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#)
            .bind(log.timestamp)
            .bind(log.ray_id)
            .bind(log.user)
            .bind(log.method)
            .bind(log.uri)
            .bind(serde_json::to_string(&log.headers).unwrap())
            .bind(body)
            .bind(log.response_status.map(|s| s as i16))
            .bind(serde_json::to_string(&log.response_headers).unwrap())
            .bind(response_body)
            .execute(self)
            .await;
        if let Err(e) = result {
            event!(
                Level::ERROR,
                error = ?e,
                "Failed to write access log to Postgres"
            );
        }
    }

    async fn log_tokens(&self, tokens: TokenUsageLog) {
        let result = sqlx::query(r#"INSERT INTO tokens_log (timestamp, ray_id, user, model, is_estimated, prompt_tokens, completion_tokens, total_tokens)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#)
            .bind(tokens.timestamp)
            .bind(tokens.ray_id)
            .bind(tokens.user)
            .bind(tokens.model)
            .bind(tokens.is_estimated)
            .bind(tokens.usage.prompt_tokens as i64)
            .bind(tokens.usage.completion_tokens as i64)
            .bind(tokens.usage.total_tokens as i64)
            .execute(self)
            .await;
        if let Err(e) = result {
            event!(
                Level::ERROR,
                error = ?e,
                "Failed to write tokens log to sqlite"
            );
        }
    }
}

fn might_as_base64_option<T, S>(value: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Deref<Target = [u8]>,
    S: Serializer,
{
    value
        .as_ref()
        .map(|v| {
            String::from_utf8(v.deref().to_vec())
                .unwrap_or_else(|_| general_purpose::STANDARD.encode(v.deref()))
        })
        .serialize(serializer)
}
