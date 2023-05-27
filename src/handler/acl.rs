use crate::acl::ApiAcl;
use crate::error::ErrorResponse;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use futures::TryStreamExt;
use std::io;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio_util::io::StreamReader;
use tracing::{event, Level};

pub async fn global_acl_layer(
    State(acl): State<Arc<ApiAcl>>,
    req: Request,
    next: Next,
) -> Result<Response, ErrorResponse> {
    let (parts, mut body) = req.into_parts();
    event!(Level::DEBUG, "{} {}", parts.method, parts.uri.path());

    let may_validate_model = acl
        .validate(&parts.method, parts.uri.path())
        .map_err(ErrorResponse::from)?;

    if let Some(validator) = may_validate_model {
        validator
            .validate_path(parts.uri.path())
            .map_err(ErrorResponse::from)?;
        if !parts.method.is_safe() {
            let content_type = parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| {
                    ErrorResponse::new(StatusCode::BAD_REQUEST, "missing content-type header")
                })?;
            event!(Level::DEBUG, "Content-Type: {}", content_type);
            if content_type == "application/json" {
                let mut buf = vec![];
                StreamReader::new(body.map_err(|e| io::Error::new(io::ErrorKind::Other, e)))
                    .read_to_end(&mut buf)
                    .await
                    .map_err(|_| {
                        ErrorResponse::new(StatusCode::BAD_REQUEST, "failed to read body")
                    })?;
                let json: serde_json::Value = serde_json::from_slice(&buf)
                    .map_err(|e| ErrorResponse::new(StatusCode::BAD_REQUEST, e.to_string()))?;
                event!(Level::DEBUG, "json: {:?}", json);
                validator
                    .validate_body(&json)
                    .map_err(ErrorResponse::from)?;

                body = Body::from(serde_json::to_string(&json).unwrap());
            }
        }
    }

    let req = Request::from_parts(parts, body);
    Ok(next.run(req).await)
}
