use crate::config::JwtAuthConfig;
use crate::error::ErrorResponse;
use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use jwt::{RegisteredClaims, VerifyWithKey};
use std::sync::Arc;
use tracing::{event, instrument, Level};

const AUTHED_HEADER: &str = "X-AUTHED-SUB";

#[instrument(skip_all)]
pub async fn jwt_auth_layer(
    State(jwt_config): State<Arc<JwtAuthConfig>>,
    req: Request,
    next: Next,
) -> Result<Response, ErrorResponse> {
    jwt_auth_layer_inner(jwt_config, req, next)
        .await
        .map_err(|_| {
            event!(Level::ERROR, "Failed to authenticate request");
            ErrorResponse::new(StatusCode::FORBIDDEN, "invalid authorization header")
        })
}

async fn jwt_auth_layer_inner(
    jwt_config: Arc<JwtAuthConfig>,
    req: Request,
    next: Next,
) -> Result<Response, ()> {
    let (mut parts, body) = req.into_parts();

    parts.headers.remove(AUTHED_HEADER);

    let token = parts
        .headers
        .get(header::AUTHORIZATION)
        .ok_or_else(|| {
            event!(Level::ERROR, "Missing authorization header");
        })?
        .to_str()
        .map_err(|_| {
            event!(Level::ERROR, "Invalid string in authorization header");
        })?
        .strip_prefix("Bearer ")
        .ok_or_else(|| {
            event!(Level::ERROR, "Not start with 'Bearer '");
        })?;

    event!(Level::DEBUG, "Token: {}", token);

    let claims: RegisteredClaims =
        VerifyWithKey::verify_with_key(token, &jwt_config.key).map_err(|e| {
            event!(Level::ERROR, "Failed to verify token: {}", e);
        })?;

    let now = chrono::Utc::now().timestamp() as u64;

    if let Some(nbf) = claims.not_before {
        if nbf > now {
            event!(Level::ERROR, "claims not valid before now: {:?}", claims);
            return Err(());
        }
    }
    if let Some(exp) = claims.expiration {
        if exp < now {
            event!(Level::ERROR, "expired claims: {:?}", claims);
            return Err(());
        }
    }

    event!(Level::INFO, "verified claims: {:?}", claims);
    match claims.subject {
        Some(sub) => {
            event!(Level::INFO, "authed subject: {}", sub);
            parts
                .headers
                .insert(AUTHED_HEADER, sub.parse().map_err(|_| ())?);
        }
        None => {
            event!(Level::INFO, "anonymous claims");
            parts
                .headers
                .insert(AUTHED_HEADER, "anonymous".parse().unwrap());
        }
    }

    let req = Request::from_parts(parts, body);
    Ok(next.run(req).await)
}
