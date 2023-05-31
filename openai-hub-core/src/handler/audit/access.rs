use crate::audit::{AccessLog, Backend, BackendEngine};
use crate::config::AuditConfig;
use crate::error::ErrorResponse;
use crate::handler::helpers::{stream_read_req_body, stream_read_response_body};
use crate::handler::jwt::AUTHED_HEADER;
use crate::helpers::HeaderMapExt;
use crate::short_circuit_if;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;
use tokio::spawn;

pub const RAY_ID_HEADER: &str = "X-Ray-Id";

pub async fn audit_access_layer(
    State(state): State<Option<(Arc<AuditConfig>, Backend)>>,
    req: Request,
    next: Next,
) -> Result<Response, ErrorResponse> {
    short_circuit_if!(req, next, state.is_none());

    let (config, backend) = state.unwrap();
    short_circuit_if!(req, next, !config.filters.access.enable);

    let (mut parts, body) = req.into_parts();
    let mut log = AccessLog::now();
    parts.headers.remove(RAY_ID_HEADER);
    parts
        .headers
        .insert(RAY_ID_HEADER, log.ray_id.as_str().parse().unwrap());

    if let Some(user) = parts.headers.get(AUTHED_HEADER) {
        log.user = Some(user.to_str().unwrap().to_string());
    }
    if config.filters.access.method {
        log.method = Some(parts.method.as_str().to_string());
    }
    if config.filters.access.uri {
        log.uri = Some(parts.uri.path().to_string());
    }
    if config.filters.access.headers {
        log.headers = Some(parts.headers.as_btree_map());
    }
    let req = Request::from_parts(parts, body);
    let response = if config.filters.access.body {
        let (response, mut body_recv) = stream_read_req_body(req, next).await;
        log.body = body_recv.recv().await.flatten();
        response
    } else {
        next.run(req).await
    };

    let response = if config.filters.access.response {
        let status = response.status();
        let headers = response.headers().clone();

        let (response, mut body_rx) = stream_read_response_body(response);
        spawn(async move {
            log.response_status = Some(status.as_u16());
            log.response_headers = Some(headers.as_btree_map());
            log.response_body = body_rx.recv().await.flatten();
            backend.log_access(log).await;
        });

        response
    } else {
        spawn(async move {
            backend.log_access(log).await;
        });
        response
    };

    Ok(response)
}
