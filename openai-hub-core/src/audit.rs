use crate::config::{AuditBackendType, AuditConfig};
use base64::engine::general_purpose;
use base64::Engine;
use chrono::serde::ts_milliseconds;
use serde::{Serialize, Serializer};
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
}

#[derive(Default, Debug, Serialize)]
pub struct AccessLog {
    #[serde(with = "ts_milliseconds")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
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
        Self {
            timestamp: chrono::Utc::now(),
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
}

#[async_trait::async_trait]
impl BackendEngine for Pool<Sqlite> {
    async fn init(&self) -> Result<(), BackendCreationError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp INTEGER,
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
        Ok(())
    }
    async fn log_access(&self, log: AccessLog) {
        let body = log.body_as_string();
        let response_body = log.response_body_as_string();
        let result = sqlx::query(r#"INSERT INTO audit_log (timestamp, user, method, uri, headers, body, response_status, response_headers, response_body)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#)
            .bind(log.timestamp.timestamp_millis())
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
}

#[async_trait::async_trait]
impl BackendEngine for Pool<MySql> {
    async fn init(&self) -> Result<(), BackendCreationError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTO_INCREMENT,
    timestamp TIMESTAMP,
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
        Ok(())
    }

    async fn log_access(&self, log: AccessLog) {
        let body = log.body_as_string();
        let response_body = log.response_body_as_string();
        let result = sqlx::query(r#"INSERT INTO audit_log (timestamp, user, method, uri, headers, body, response_status, response_headers, response_body)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"#)
            .bind(log.timestamp.timestamp_millis())
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
}

#[async_trait::async_trait]
impl BackendEngine for Pool<Postgres> {
    async fn init(&self) -> Result<(), BackendCreationError> {
        sqlx::query(
            r#"CREATE TABLE IF NOT EXISTS audit_log (
    id SERIAL PRIMARY KEY,
    timestamp TIMESTAMP,
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
        Ok(())
    }

    async fn log_access(&self, log: AccessLog) {
        let body = log.body_as_string();
        let response_body = log.response_body_as_string();
        let result = sqlx::query(r#"INSERT INTO audit_log (timestamp, user, method, uri, headers, body, response_status, response_headers, response_body)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#)
            .bind(log.timestamp.timestamp_millis())
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
