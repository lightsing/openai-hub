use crate::config::{AccessLogBackend, AccessLogConfig};
use sqlx::{MySql, Pool, Postgres, Sqlite};

pub enum Backend {
    Text(TextBackend),
    Database(DatabaseBackend),
}

#[derive(Debug, thiserror::Error)]
pub enum BackendCreationError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    DatabaseError(#[from] sqlx::Error),
}

impl Backend {
    pub async fn create_with(config: AccessLogConfig) -> Result<Self, BackendCreationError> {
        Ok(match config.backend {
            AccessLogBackend::File => Self::Text(TextBackend::create_with(config).await?),
            _ => Self::Database(DatabaseBackend::create_with(config).await?),
        })
    }
}

pub struct TextBackend {
    writer: tokio::fs::File,
}

impl TextBackend {
    async fn create_with(config: AccessLogConfig) -> Result<Self, BackendCreationError> {
        let config = config.file_backend.unwrap_or_default();
        let writer = tokio::fs::File::create(config.path).await?;
        Ok(Self { writer })
    }
}

#[derive(Clone)]
pub enum DatabaseBackend {
    Sqlite(Pool<Sqlite>),
    MySql(Pool<MySql>),
    Postgres(Pool<Postgres>),
}

impl DatabaseBackend {
    async fn create_with(config: AccessLogConfig) -> Result<Self, BackendCreationError> {
        Ok(match config.backend {
            AccessLogBackend::Sqlite => Self::Sqlite(
                Pool::connect_with(config.sqlite_backend.unwrap_or_default().into()).await?,
            ),
            AccessLogBackend::Mysql => Self::MySql(
                Pool::connect_with(config.mysql_backend.unwrap_or_default().into()).await?,
            ),
            AccessLogBackend::Postgres => Self::Postgres(
                Pool::connect_with(config.postgres_backend.unwrap_or_default().into()).await?,
            ),
            _ => unreachable!(),
        })
    }
}
