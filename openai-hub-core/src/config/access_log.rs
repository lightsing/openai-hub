use serde::Deserialize;
use sqlx::mysql::MySqlConnectOptions;
use sqlx::postgres::PgConnectOptions;
use sqlx::sqlite::SqliteConnectOptions;

#[derive(Clone, Deserialize)]
pub struct AccessLogConfig {
    pub backend: AccessLogBackend,
    #[serde(default)]
    pub file_backend: Option<FileAccessLogConfig>,
    #[serde(default)]
    pub sqlite_backend: Option<SqliteAccessLogConfig>,
    #[serde(default)]
    pub mysql_backend: Option<MysqlAccessLogConfig>,
    #[serde(default)]
    pub postgres_backend: Option<PostgresAccessLogConfig>,
}

#[derive(Copy, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccessLogBackend {
    File,
    Sqlite,
    Mysql,
    Postgres,
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct FileAccessLogConfig {
    pub path: String,
}

impl Default for FileAccessLogConfig {
    fn default() -> Self {
        Self {
            path: "access.log".to_string(),
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct SqliteAccessLogConfig {
    pub filename: String,
    pub create_if_missing: bool,
}

impl Default for SqliteAccessLogConfig {
    fn default() -> Self {
        Self {
            filename: "access-log.sqlite".to_string(),
            create_if_missing: true,
        }
    }
}

impl From<SqliteAccessLogConfig> for SqliteConnectOptions {
    fn from(config: SqliteAccessLogConfig) -> Self {
        (&config).into()
    }
}

impl From<&SqliteAccessLogConfig> for SqliteConnectOptions {
    fn from(config: &SqliteAccessLogConfig) -> Self {
        SqliteConnectOptions::new()
            .filename(&config.filename)
            .create_if_missing(config.create_if_missing)
    }
}

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct MysqlAccessLogConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub socket: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub database: String,
}

impl Default for MysqlAccessLogConfig {
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

impl From<MysqlAccessLogConfig> for MySqlConnectOptions {
    fn from(config: MysqlAccessLogConfig) -> Self {
        (&config).into()
    }
}

impl From<&MysqlAccessLogConfig> for MySqlConnectOptions {
    fn from(config: &MysqlAccessLogConfig) -> Self {
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

#[derive(Clone, Deserialize)]
#[serde(default)]
pub struct PostgresAccessLogConfig {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub socket: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub database: String,
}

impl Default for PostgresAccessLogConfig {
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

impl From<PostgresAccessLogConfig> for PgConnectOptions {
    fn from(config: PostgresAccessLogConfig) -> Self {
        (&config).into()
    }
}

impl From<&PostgresAccessLogConfig> for PgConnectOptions {
    fn from(config: &PostgresAccessLogConfig) -> Self {
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
