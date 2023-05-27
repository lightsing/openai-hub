mod acl;

use crate::config::OpenAIConfig;
use crate::error::ErrorResponse;
use crate::helpers::proxy_request;
use crate::key::KeyPool;
use axum::extract::Request;
use axum::handler::Handler;
use axum::response::{IntoResponse, Response};
use futures::TryStreamExt;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use sync_wrapper::SyncStream;

pub use acl::global_acl_layer;

#[derive(Clone)]
pub struct RequestHandler {
    pub key_pool: Arc<KeyPool>,
    pub client: reqwest::Client,
    pub config: Arc<OpenAIConfig>,
}

impl Handler<Result<Response, ErrorResponse>, ()> for RequestHandler {
    type Future = Pin<Box<dyn Future<Output = Response> + Send>>;

    fn call(self, req: Request, _state: ()) -> Self::Future {
        Box::pin(async move { self.handle_request(req).await.into_response() })
    }
}

impl RequestHandler {
    async fn handle_request(self, req: Request) -> Result<Response, ErrorResponse> {
        let (parts, body) = req.into_parts();
        let body = body.map_err(|e| io::Error::new(io::ErrorKind::Other, e));

        proxy_request(
            self.client,
            parts.method,
            format!("{}{}", self.config.api_base, parts.uri.path()),
            self.key_pool.get().await,
            parts.headers,
            reqwest::Body::wrap_stream(SyncStream::new(body)),
        )
        .await
    }
}
