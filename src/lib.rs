pub mod acl;
pub mod config;
mod error;
mod helpers;
pub mod key;

use crate::config::ApiType;
use crate::error::ErrorResponse;
use crate::helpers::proxy_request;
use crate::key::KeyPool;
use acl::ApiAcl;
use axum::extract::{Path, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Router;
use config::ServerConfig;
use futures::TryStreamExt;
use std::io;
use std::sync::Arc;
use sync_wrapper::SyncStream;
use tokio::io::AsyncReadExt;
use tokio::net::TcpListener;
use tokio_util::io::StreamReader;
use tracing::{event, Level};

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub struct Server {
    config: Arc<ServerConfig>,
    api_key_pool: Arc<KeyPool>,
}

#[derive(Clone)]
struct AppState {
    key_pool: Arc<KeyPool>,
    acl: Arc<ApiAcl>,
    client: reqwest::Client,
    config: Arc<ServerConfig>,
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    AddrParse(#[from] std::net::AddrParseError),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}

impl Server {
    pub fn from_config(config: ServerConfig) -> Self {
        let api_key_pool = Arc::new(KeyPool::new(config.api_keys.clone()));
        Self {
            config: Arc::new(config),
            api_key_pool,
        }
    }

    pub async fn serve(self) -> Result<(), ServerError> {
        let listener = TcpListener::bind(self.config.addr).await?;
        let client = reqwest::Client::builder()
            .user_agent(APP_USER_AGENT)
            .build()?;
        let app = Router::new()
            .route(
                if self.config.api_type == ApiType::OpenAI {
                    "/*seg"
                } else {
                    "/openai/*seg"
                },
                post(body_handler)
                    .get(no_body_handler)
                    .delete(no_body_handler),
            )
            .with_state(AppState {
                key_pool: self.api_key_pool.clone(),
                acl: Arc::new(self.config.global_api_acl.clone()),
                client,
                config: self.config.clone(),
            });
        axum::serve(listener, app).await?;
        Ok(())
    }
}

async fn no_body_handler(
    method: Method,
    headers: HeaderMap,
    Path(mut path): Path<String>,
    State(AppState {
        key_pool,
        acl,
        client,
        config,
    }): State<AppState>,
    req: Request,
) -> Result<Response, Response> {
    path.insert(0, '/');
    event!(Level::DEBUG, "{} {}", method, path);
    let may_validate_model = acl
        .validate(&method, &path)
        .map_err(ErrorResponse::from)
        .map_err(IntoResponse::into_response)?;
    if let Some(validator) = may_validate_model {
        validator
            .validate_path(&path)
            .map_err(ErrorResponse::from)
            .map_err(IntoResponse::into_response)?;
    }

    let body = req
        .into_body()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e));

    proxy_request(
        client,
        method,
        format!("{}{}", config.api_base, path),
        key_pool.get().await,
        headers,
        reqwest::Body::wrap_stream(SyncStream::new(body)),
    )
    .await
}

async fn body_handler(
    method: Method,
    headers: HeaderMap,
    Path(mut path): Path<String>,
    State(AppState {
        key_pool,
        acl,
        client,
        config,
    }): State<AppState>,
    req: Request,
) -> Result<Response, Response> {
    path.insert(0, '/');
    event!(Level::DEBUG, "{} {}", method, path);
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            ErrorResponse::new(StatusCode::BAD_REQUEST, "missing content-type header")
                .into_response()
        })?;
    event!(Level::DEBUG, "Content-Type: {}", content_type);
    let may_validate_model = acl
        .validate(&method, &path)
        .map_err(ErrorResponse::from)
        .map_err(IntoResponse::into_response)?;

    let body = req
        .into_body()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e));

    // validate json body
    if content_type == "application/json" {
        let mut buf = vec![];
        StreamReader::new(body)
            .read_to_end(&mut buf)
            .await
            .map_err(|_| {
                ErrorResponse::new(StatusCode::BAD_REQUEST, "failed to read body").into_response()
            })?;
        let json: serde_json::Value = serde_json::from_slice(&buf).map_err(|e| {
            ErrorResponse::new(StatusCode::BAD_REQUEST, e.to_string()).into_response()
        })?;
        event!(Level::DEBUG, "json: {:?}", json);
        if let Some(validator) = may_validate_model {
            validator
                .validate_body(&json)
                .map_err(ErrorResponse::from)
                .map_err(IntoResponse::into_response)?;
        }
        proxy_request(
            client,
            method,
            format!("{}{}", config.api_base, path),
            key_pool.get().await,
            headers,
            serde_json::to_string(&json).unwrap(),
        )
        .await
    } else {
        proxy_request(
            client,
            method,
            format!("{}{}", config.api_base, path),
            key_pool.get().await,
            headers,
            reqwest::Body::wrap_stream(SyncStream::new(body)),
        )
        .await
    }
}
