use crate::error::ErrorResponse;
use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;

pub async fn access_log_layer(
    State(config): State<Option<()>>,
    req: Request,
    next: Next,
) -> Result<Response, ErrorResponse> {
    Ok(next.run(req).await)
}
