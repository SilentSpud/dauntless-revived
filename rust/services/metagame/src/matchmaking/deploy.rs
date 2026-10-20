//! The deploy server, over plain HTTP/1.1 (`fetch` with `redirect: 'error'` in Node): one request
//! per connection, the whole exchange under one deadline.

use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use serde_json::Value;

/// A reply: its status, its `Retry-After`, and its body when it was read.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub retry_after: Option<String>,
    pub body: Option<Value>,
}

/// `METHOD http://<authority><path>` with an optional JSON body; the body of a reply with one of
/// `read_body_for` is parsed as JSON (a parse failure is an error, as `reply.json()` threw).
pub async fn request(
    authority: &str,
    method: Method,
    path: &str,
    body: Option<String>,
    read_body_for: &[u16],
    timeout_ms: u64,
) -> Result<Reply, String> {
    let work = exchange(authority, method, path, body, read_body_for);
    match tokio::time::timeout(Duration::from_millis(timeout_ms), work).await {
        Ok(result) => result,
        Err(_) => Err(format!("Deploy request exceeded {timeout_ms} ms")),
    }
}

async fn exchange(
    authority: &str,
    method: Method,
    path: &str,
    body: Option<String>,
    read_body_for: &[u16],
) -> Result<Reply, String> {
    let address = if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    };
    let stream = tokio::net::TcpStream::connect(&address)
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let mut builder = Request::builder().method(method).uri(path).header("host", authority);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let request = builder
        .body(Full::new(Bytes::from(body.unwrap_or_default())))
        .map_err(|e| format!("fetch failed: {e}"))?;
    let response = sender
        .send_request(request)
        .await
        .map_err(|e| format!("fetch failed: {e}"))?;
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if !read_body_for.contains(&status) {
        return Ok(Reply {
            status,
            retry_after,
            body: None,
        });
    }
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|e| format!("fetch failed: {e}"))?
        .to_bytes();
    let text = std::str::from_utf8(&bytes).map_err(|e| format!("SyntaxError: {e}"))?;
    let body = service_kit::jsjson::parse(text).map_err(|e| format!("SyntaxError: {e}"))?;
    Ok(Reply {
        status,
        retry_after,
        body: Some(body),
    })
}
