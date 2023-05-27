pub mod acl;
pub mod config;
mod error;
mod handler;
mod helpers;
pub mod key;

use crate::handler::{global_acl_layer, RequestHandler};
use crate::key::KeyPool;
use axum::handler::{Handler, HandlerWithoutStateExt};
use axum::middleware::from_fn_with_state;
use config::ServerConfig;
use std::io;
use std::sync::Arc;
use tokio::net::TcpListener;

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub struct Server {
    config: Arc<ServerConfig>,
    api_key_pool: Arc<KeyPool>,
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
        let handler = RequestHandler {
            key_pool: self.api_key_pool.clone(),
            client,
            config: Arc::new(self.config.openai.clone()),
        };
        let handler = handler.layer(from_fn_with_state(
            Arc::new(self.config.global_api_acl.clone()),
            global_acl_layer,
        ));
        axum::serve(listener, handler.into_service()).await?;
        Ok(())
    }
}
