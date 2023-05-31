use crate::helpers::{tee, ResultStream};
use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;
use futures::TryStreamExt;
use std::io;
use sync_wrapper::SyncStream;
use tokio::io::AsyncReadExt;
use tokio::spawn;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio_util::io::StreamReader;

#[macro_export]
macro_rules! short_circuit_if {
    ($req:ident, $next:ident, $cond:expr) => {
        if $cond {
            return Ok($next.run($req).await);
        }
    };
}

pub async fn stream_read_req_body(
    req: Request,
    next: Next,
) -> (Response, Receiver<Option<Vec<u8>>>) {
    let (parts, body) = req.into_parts();
    let (dup_body, body_) = tee(SyncStream::new(body));

    let rx = stream_read_body(dup_body);
    let response = next
        .run(Request::from_parts(parts, Body::from_stream(body_)))
        .await;

    (response, rx)
}

pub fn stream_read_response_body(response: Response) -> (Response, Receiver<Option<Vec<u8>>>) {
    let (parts, body) = response.into_parts();
    let (dup_body, body) = tee(SyncStream::new(body));
    let rx = stream_read_body(dup_body);
    let response = Response::from_parts(parts, Body::from_stream(body));
    (response, rx)
}

fn stream_read_body(body: ResultStream<Bytes, axum::Error>) -> Receiver<Option<Vec<u8>>> {
    let (tx, rx) = mpsc::channel(1);
    spawn(async move {
        let mut body_reader = StreamReader::new(
            body.map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string())),
        );
        let mut body_buffer = vec![];
        if body_reader.read_to_end(&mut body_buffer).await.is_err() {
            tx.send(None).await.ok();
        }
        tx.send(Some(body_buffer)).await.ok()
    });
    rx
}
