use serde::Deserialize;
use sqlx::mysql::MySqlConnectOptions;
use sqlx::postgres::PgConnectOptions;
use sqlx::sqlite::SqliteConnectOptions;
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize)]
pub struct AuditConfig {
    pub backend: AuditBackendType,
    #[serde(default)]
    pub backends: AuditBackendConfig,
    #[serde(default)]
    pub filters: AuditFiltersConfig,
}

#[derive(Copy, Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditBackendType {
    File,
    Sqlite,
    Mysql,
    Postgres,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct AuditBackendConfig {
    pub file_backend: FileBackendConfig,
    pub sqlite_backend: SqliteBackendConfig,
    pub mysql_backend: MySqlBackendConfig,
    pub postgres_backend: PostgresBackendConfig,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct AuditFiltersConfig {
    pub access: AuditAccessFilterConfig,
    pub tokens: AuditTokensFilterConfig,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AuditAccessFilterConfig {
    pub enable: bool,
    pub method: bool,
    pub uri: bool,
    pub headers: bool,
    pub body: bool,
    pub response: bool,
}

impl Default for AuditAccessFilterConfig {
    fn default() -> Self {
        Self {
            enable: true,
            method: true,
            uri: true,
            headers: false,
            body: false,
            response: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct AuditTokensFilterConfig {
    pub enable: bool,
    pub endpoints: HashSet<String>,
    pub stream_tokens: StreamTokensPolicy,
}

impl Default for AuditTokensFilterConfig {
    fn default() -> Self {
        Self {
            enable: true,
            endpoints: HashSet::from_iter([
                "/completions".to_string(),
                "/chat/completions".to_string(),
                "/edits".to_string(),
                "/embeddings".to_string(),
            ]),
            stream_tokens: StreamTokensPolicy::default(),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamTokensPolicy {
    Skip,
    Reject,
    Estimate,
}

impl Default for StreamTokensPolicy {
    fn default() -> Self {
        Self::Estimate
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct FileBackendConfig {
    pub filename: String,
}

impl Default for FileBackendConfig {
    fn default() -> Self {
        Self {
            filename: "access.log".to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct SqliteBackendConfig {
    pub filename: String,
    pub create_if_missing: bool,
}

impl Default for SqliteBackendConfig {
    fn default() -> Self {
        Self {
            filename: "access-log.sqlite".to_string(),
            create_if_missing: true,
        }
    }
}

impl From<&SqliteBackendConfig> for SqliteConnectOptions {
    fn from(config: &SqliteBackendConfig) -> Self {
        SqliteConnectOptions::new()
            .filename(&config.filename)
            .create_if_missing(config.create_if_missing)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct MySqlBackendConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub socket: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub database: String,
}

impl Default for MySqlBackendConfig {
    fn default() -> Self {
        Self {
            host: None,
            port: None,
            socket: None,
            username: None,
            password: None,
            database: "access_log".to_string(),
        }
    }
}

impl From<&MySqlBackendConfig> for MySqlConnectOptions {
    fn from(config: &MySqlBackendConfig) -> Self {
        let mut options = MySqlConnectOptions::new();
        if let Some(ref host) = config.host {
            options = options.host(host);
        }
        if let Some(port) = config.port {
            options = options.port(port);
        }
        if let Some(ref socket) = config.socket {
            options = options.socket(socket);
        }
        if let Some(ref username) = config.username {
            options = options.username(username);
        }
        if let Some(ref password) = config.password {
            options = options.password(password);
        }
        options = options.database(&config.database);
        options
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct PostgresBackendConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub socket: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub database: String,
}

impl Default for PostgresBackendConfig {
    fn default() -> Self {
        Self {
            host: None,
            port: None,
            socket: None,
            username: None,
            password: None,
            database: "access_log".to_string(),
        }
    }
}

impl From<&PostgresBackendConfig> for PgConnectOptions {
    fn from(config: &PostgresBackendConfig) -> Self {
        let mut options = PgConnectOptions::new();
        if let Some(ref host) = config.host {
            options = options.host(host);
        }
        if let Some(port) = config.port {
            options = options.port(port);
        }
        if let Some(ref socket) = config.socket {
            options = options.socket(socket);
        }
        if let Some(ref username) = config.username {
            options = options.username(username);
        }
        if let Some(ref password) = config.password {
            options = options.password(password);
        }
        options = options.database(&config.database);
        options
    }
}
