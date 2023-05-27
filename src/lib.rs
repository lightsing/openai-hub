#![deny(unsafe_code)]
//! This is the main module for the OpenAI Hub server.
//! It handles server configuration, request handling, access control, and the server's API key pool.

#[cfg(feature = "acl")]
/// Access Control List (ACL) module
mod acl;
/// Configuration
pub mod config;
/// Error handling
mod error;
/// Request handlers
mod handler;
/// Helpers
mod helpers;
/// API Key Pool
mod key;

#[cfg(feature = "acl")]
pub use acl::ApiAcl;

use crate::handler::RequestHandler;
use crate::key::KeyPool;
use axum::handler::HandlerWithoutStateExt;
use config::ServerConfig;
use std::io;
use std::sync::Arc;
use tokio::net::TcpListener;

#[cfg(any(feature = "acl"))]
use axum::handler::Handler;
#[cfg(any(feature = "acl"))]
use axum::middleware::from_fn_with_state;

#[cfg(feature = "acl")]
use crate::handler::global_acl_layer;

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Holds the server's configuration and API key pool.
pub struct Server {
    config: Arc<ServerConfig>,
    api_key_pool: Arc<KeyPool>,
}

/// Server Error
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
    /// Create a new Server from a given configuration.
    pub fn from_config(config: ServerConfig) -> Self {
        let api_key_pool = Arc::new(KeyPool::new(config.api_keys.clone()));
        Self {
            config: Arc::new(config),
            api_key_pool,
        }
    }

    /// Start the server and listen for incoming connections.
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

        #[cfg(feature = "acl")]
        let handler = if let Some(ref acl) = self.config.global_api_acl {
            handler.layer(from_fn_with_state(Arc::new(acl.clone()), global_acl_layer))
        } else {
            handler
        };

        axum::serve(listener, handler.into_service()).await?;
        Ok(())
    }
}
