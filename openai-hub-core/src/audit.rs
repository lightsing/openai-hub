use crate::config::{AuditBackendType, AuditConfig};
use base64::engine::general_purpose;
use base64::Engine;
use chrono::serde::ts_milliseconds;
use serde::{Serialize, Serializer};
use sqlx::{Database, MySql, Pool, Postgres, Sqlite};
use std::collections::BTreeMap;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{event, Level};

#[async_trait::async_trait]
pub trait BackendEngine {
    async fn log_access(&self, access: AccessLog);
}

#[derive(Default, Debug, Serialize)]
pub struct AccessLog {
    #[serde(with = "ts_milliseconds")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "as_base64_option"
    )]
    pub body: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_headers: Option<BTreeMap<String, String>>,
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "as_base64_option"
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
        Ok(match config.backend {
            AuditBackendType::File => Self::Text(TextBackend::create_with(config).await?),
            _ => Self::Database(DatabaseBackend::create_with(config).await?),
        })
    }
}

#[async_trait::async_trait]
impl BackendEngine for Backend {
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
        let writer = tokio::fs::File::create(&config.backends.file_backend.filename).await?;
        Ok(Self {
            writer: Arc::new(Mutex::new(writer)),
        })
    }
}

#[async_trait::async_trait]
impl BackendEngine for TextBackend {
    async fn log_access(&self, access: AccessLog) {
        let _writer = self.writer.lock().await;
        event!(Level::DEBUG, "{:?}", access);
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
    async fn log_access(&self, access: AccessLog) {
        match self {
            DatabaseBackend::Sqlite(pool) => pool.log_access(access).await,
            DatabaseBackend::MySql(pool) => pool.log_access(access).await,
            DatabaseBackend::Postgres(pool) => pool.log_access(access).await,
        }
    }
}

#[async_trait::async_trait]
impl<DB: Database> BackendEngine for Pool<DB> {
    async fn log_access(&self, _access: AccessLog) {
        // self.execute()
    }
}

fn as_base64_option<T, S>(key: &Option<T>, serializer: S) -> Result<S::Ok, S::Error>
where
    T: Deref<Target = [u8]>,
    S: Serializer,
{
    key.as_ref()
        .map(|k| general_purpose::STANDARD.encode(k.deref()))
        .serialize(serializer)
}
