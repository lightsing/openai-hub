use axum::body::Body;
use std::collections::BTreeMap;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::Response;
use base64::Engine;
use futures::{Stream, StreamExt};
use tokio::sync::mpsc;

use tokio_stream::wrappers::ReceiverStream;

use crate::error::ErrorResponse;
use crate::key::KeyGuard;
use tracing::{event, instrument, Level};

#[cfg(feature = "acl")]
mod regex_helpers {
    use once_cell::sync::Lazy;
    use regex::Regex;
    use tracing::{event, instrument, Level};

    static SPECIAL_CHARS_EXCEPT_START_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"([.+?^$()\[\]{}|\\])").unwrap());
    static SPECIAL_CHARS_EXCEPT_GROUP_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"([.+?*^$()\[\]|\\])").unwrap());
    static PATH_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"/\{(?P<name>[^/{}]+)}").unwrap());

    #[instrument(skip_all)]
    pub fn wildcards_to_regex<S: AsRef<str>, I: Iterator<Item = S>>(
        wildcards: I,
    ) -> Result<Regex, regex::Error> {
        let mut candidates = vec![];
        for wildcard in wildcards {
            let wildcard = wildcard.as_ref();
            let wildcard = SPECIAL_CHARS_EXCEPT_START_REGEX.replace(wildcard, "\\$1");
            // short circuit if the wildcard is allowing anything
            if wildcard == "*" {
                event!(Level::DEBUG, "found *, skip remaining");
                return Ok(Regex::new("^.*$").unwrap());
            }
            let mut wildcard = wildcard.replace('*', ".*");
            // group with non-capturing group
            wildcard.insert_str(0, "(?:");
            wildcard.push(')');
            event!(Level::DEBUG, "transformed wildcard to {}", wildcard);
            candidates.push(wildcard);
        }
        let mut regex = candidates.join("|");
        regex.insert_str(0, "^(?:");
        regex.push_str(")$");
        event!(Level::DEBUG, "transformed wildcards to regex {}", regex);
        Regex::new(&regex)
    }

    #[instrument(skip_all)]
    pub fn endpoints_to_regex<S: AsRef<str>, I: Iterator<Item = S>>(
        endpoints: I,
    ) -> Result<Regex, regex::Error> {
        let mut candidates = vec![];
        for endpoint in endpoints {
            let endpoint = endpoint.as_ref();
            let endpoint = SPECIAL_CHARS_EXCEPT_GROUP_REGEX.replace(endpoint, "\\$1");
            let endpoint = PATH_REGEX.replace(&endpoint, "/(?:[^/]+)");
            event!(Level::DEBUG, "transformed regex rule: {}", endpoint);
            // group with non-capturing group
            candidates.push(format!("(?:{endpoint})"));
        }
        let mut regex = candidates.join("|");
        regex.insert_str(0, "^(?:");
        regex.push_str(")$");
        event!(Level::DEBUG, "transformed wildcards to regex {}", regex);
        Regex::new(&regex)
    }
}

#[cfg(feature = "acl")]
pub use regex_helpers::*;

pub fn request_error_into_response(e: reqwest::Error) -> ErrorResponse {
    if e.is_timeout() {
        return ErrorResponse::new(StatusCode::GATEWAY_TIMEOUT, "openai timeout");
    }
    ErrorResponse::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

#[pin_project::pin_project]
pub struct StreamWithKey<S> {
    #[pin]
    stream: S,
    key: KeyGuard,
}

impl<S> StreamWithKey<S> {
    pub fn new(stream: S, key: KeyGuard) -> Self {
        Self { stream, key }
    }
}

impl<S: futures::Stream> futures::Stream for StreamWithKey<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        this.stream.as_mut().poll_next(cx)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.stream.size_hint()
    }
}

#[instrument(skip(client, key, body))]
pub async fn proxy_request<U, B>(
    client: reqwest::Client,
    method: Method,
    uri: U,
    key: KeyGuard,
    headers: HeaderMap,
    body: B,
) -> Result<Response, ErrorResponse>
where
    U: reqwest::IntoUrl + std::fmt::Debug,
    B: Into<reqwest::Body>,
{
    let mut request = client
        .request(method, uri)
        .header(header::AUTHORIZATION, format!("Bearer {}", key.as_str()))
        .body(body);
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(accept) = headers.get(header::ACCEPT) {
        request = request.header(header::ACCEPT, accept);
    }
    let result = request.send().await.map_err(request_error_into_response)?;
    let status = result.status();
    let headers = result.headers().clone();
    event!(Level::DEBUG, "openai returns status: {}", status);
    let body = StreamWithKey::new(result.bytes_stream(), key);
    let mut builder = Response::builder().status(status);
    for (k, v) in headers.iter() {
        builder = builder.header(k, v);
    }
    Ok(builder.body(Body::from_stream(body)).unwrap())
}

pub type ResultStream<T, E> = ReceiverStream<Result<T, E>>;

pub fn tee<S, T, E>(stream: S) -> (ResultStream<T, E>, ResultStream<T, E>)
where
    S: Stream<Item = Result<T, E>> + Send + Sync + Unpin + 'static,
    T: Clone + Send + Sync + 'static,
    E: Send + Sync + 'static,
{
    let (tx1, rx1) = mpsc::channel(1);
    let (tx2, rx2) = mpsc::channel(1);

    tokio::spawn(async move {
        let mut stream = stream;

        while let Some(value) = stream.next().await {
            match value {
                Ok(t) => {
                    let clone_t = t.clone();
                    let tx1 = tx1.clone();
                    tokio::spawn(async move {
                        tx1.send(Ok(clone_t)).await.ok();
                    });
                    let tx2 = tx2.clone();
                    tokio::spawn(async move {
                        tx2.send(Ok(t)).await.ok();
                    });
                }
                Err(e) => {
                    tx2.send(Err(e)).await.ok();
                }
            }
        }
    });

    (ReceiverStream::new(rx1), ReceiverStream::new(rx2))
}

pub trait HeaderMapExt {
    fn as_btree_map(&self) -> BTreeMap<String, String>;
}

impl HeaderMapExt for HeaderMap {
    fn as_btree_map(&self) -> BTreeMap<String, String> {
        self.iter()
            .map(|(k, v)| match v.to_str() {
                Ok(s) => (k.to_string(), s.to_string()),
                Err(_) => (
                    k.to_string(),
                    if cfg!(feature = "base64-serialize") {
                        base64::engine::general_purpose::STANDARD.encode(v.as_bytes())
                    } else {
                        "<ERROR: non-ASCII HeaderValue>".to_string()
                    },
                ),
            })
            .collect()
    }
}
