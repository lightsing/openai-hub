use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::response::Response;

use crate::acl::AclError;
use serde_json::json;

#[derive(Debug)]
pub struct ErrorResponse {
    status_code: StatusCode,
    message: String,
}

impl ErrorResponse {
    pub fn new<S: AsRef<str>>(status_code: StatusCode, message: S) -> Self {
        Self {
            status_code,
            message: message.as_ref().to_string(),
        }
    }

    pub fn body(&self) -> Body {
        let buf = json!({
            "error": {
                "message": &self.message,
            }
        });
        Body::from(serde_json::to_string(&buf).unwrap())
    }
}

impl IntoResponse for ErrorResponse {
    fn into_response(self) -> Response {
        Response::builder()
            .status(self.status_code)
            .header(header::CONTENT_TYPE, "application/json")
            .body(self.body())
            .unwrap()
    }
}

impl From<AclError> for ErrorResponse {
    fn from(err: AclError) -> Self {
        ErrorResponse {
            status_code: err.status_code(),
            message: err.to_string(),
        }
    }
}
