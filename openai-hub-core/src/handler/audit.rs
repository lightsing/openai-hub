use crate::audit::{AccessLog, Backend, BackendEngine};
use crate::config::{AuditConfig};
use crate::error::ErrorResponse;
use crate::helpers::{tee, HeaderMapExt};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use futures::TryStreamExt;

use std::io;
use std::sync::Arc;
use sync_wrapper::SyncStream;
use tokio::io::AsyncReadExt;
use tokio::spawn;
use tokio::sync::mpsc;
use tokio_util::io::StreamReader;
use tracing::{event, Level};

pub async fn audit_access_layer(
    State(state): State<Option<(Arc<AuditConfig>, Backend)>>,
    req: Request,
    next: Next,
) -> Result<Response, ErrorResponse> {
    if state.is_none() {
        return Ok(next.run(req).await);
    }

    let (config, backend) = state.unwrap();
    if !config.filters.access.enable {
        return Ok(next.run(req).await);
    }

    let (parts, body) = req.into_parts();
    let mut log = AccessLog::now();

    if config.filters.access.method {
        log.method = Some(parts.method.as_str().to_string());
    }
    if config.filters.access.uri {
        log.uri = Some(parts.uri.path().to_string());
    }
    if config.filters.access.headers {
        log.headers = Some(parts.headers.as_btree_map());
    }
    let mut response = if config.filters.access.body {
        let (dup_body, body_) = tee(SyncStream::new(body));

        let (tx, mut rx) = mpsc::channel(1);
        spawn(async move {
            let mut body_reader = StreamReader::new(
                dup_body.map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string())),
            );
            let mut body_buffer = vec![];
            if let Err(_) = body_reader.read_to_end(&mut body_buffer).await {
                tx.send(None).await.ok();
            }
            tx.send(Some(body_buffer)).await.ok()
        });
        let response = next
            .run(Request::from_parts(parts, Body::from_stream(body_)))
            .await;

        let body = rx.recv().await.flatten().ok_or_else(|| {
            ErrorResponse::new(StatusCode::BAD_REQUEST, "fail to read request body")
        })?;
        log.body = Some(body);

        response
    } else {
        next.run(Request::from_parts(parts, body)).await
    };

    if config.filters.access.response {
        let (parts, body) = response.into_parts();
        let status = parts.status;
        let headers = parts.headers.clone();

        let (dup_body, body) = tee(SyncStream::new(body));
        spawn(async move {
            let mut body_reader = StreamReader::new(
                dup_body.map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string())),
            );
            let mut body_buffer = vec![];
            if let Err(e) = body_reader.read_to_end(&mut body_buffer).await {
                event!(Level::ERROR, "failed to read response body: {}", e);
            }
            log.response_status = Some(status.as_u16());
            log.response_headers = Some(headers.as_btree_map());
            log.response_body = Some(body_buffer);
            backend.log_access(log).await;
        });

        response = Response::from_parts(parts, Body::from_stream(body));
    } else {
        spawn(async move {
            backend.log_access(log).await;
        });
    }

    Ok(response)
}
