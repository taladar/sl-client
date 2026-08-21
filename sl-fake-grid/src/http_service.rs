//! The hyper HTTP service: one loopback listener serving the login endpoint
//! at `/` and every session's CAPS surface under `/sim/<seq>/…`.
//!
//! Connections are served one spawned task each, so a held `EventQueueGet`
//! long-poll never starves other requests (the client's HTTP stack pools
//! connections and opens new ones as needed).

use std::convert::Infallible;
use std::sync::Arc;

use bytes::Bytes;
use http::header::CONTENT_TYPE;
use http::status::StatusCode as HttpStatusCode;
use http_body_util::{BodyExt as _, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};
use sl_proto::CapsResponse;
use tokio::net::TcpListener;

use crate::caps_endpoint::dispatch_caps;
use crate::login_endpoint::handle_login;
use crate::runtime::GridCore;

/// The largest request body accepted, uploads included.
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// The accept loop: serves connections until shutdown.
pub(crate) async fn run_http(core: Arc<GridCore>, listener: TcpListener) {
    let mut shutdown_rx = core.shutdown_tx.subscribe();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _peer) = match accepted {
                    Ok(pair) => pair,
                    Err(error) => {
                        tracing::warn!("accepting an HTTP connection failed: {error}");
                        continue;
                    }
                };
                let core = Arc::clone(&core);
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |request| {
                        let core = Arc::clone(&core);
                        async move { handle_request(core, request).await }
                    });
                    let connection = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service);
                    if let Err(error) = connection.await {
                        tracing::debug!("HTTP connection ended with an error: {error}");
                    }
                });
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
        }
    }
}

/// Routes one request: `POST /` → login, `/sim/<seq>/…` → that session's
/// CAPS surface, anything else → 404.
async fn handle_request(
    core: Arc<GridCore>,
    request: Request<Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let method = request.method().as_str().to_owned();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().map(str::to_owned);
    let content_type = header_string(&request, CONTENT_TYPE);
    let range = header_string(&request, http::header::RANGE);

    let body = match collect_body(request).await {
        Ok(body) => body,
        Err(status) => return Ok(plain_status(status)),
    };

    if path == "/" {
        if method != "POST" {
            return Ok(plain_status(HttpStatusCode::METHOD_NOT_ALLOWED));
        }
        let answer = handle_login(&core, content_type.as_deref().unwrap_or(""), &body).await;
        return Ok(login_response(&answer));
    }

    if let Some(seq) = session_seq(&path) {
        let Some(shared) = core.session(seq).await else {
            return Ok(plain_status(HttpStatusCode::NOT_FOUND));
        };
        let caps_response = dispatch_caps(
            &shared,
            core.eq_hold,
            &method,
            &path,
            query.as_deref(),
            range.as_deref(),
            &body,
        )
        .await;
        return Ok(caps_http_response(caps_response));
    }

    Ok(plain_status(HttpStatusCode::NOT_FOUND))
}

/// Reads one header as a string, if present and valid UTF-8.
fn header_string(request: &Request<Incoming>, name: http::header::HeaderName) -> Option<String> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
}

/// Collects the request body up to [`MAX_BODY_BYTES`]; a larger or
/// unreadable body yields the status to answer with (413).
async fn collect_body(request: Request<Incoming>) -> Result<Bytes, HttpStatusCode> {
    let body = request.into_body();
    match http_body_util::Limited::new(body, MAX_BODY_BYTES)
        .collect()
        .await
    {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(error) => {
            tracing::debug!("collecting a request body failed: {error}");
            Err(HttpStatusCode::PAYLOAD_TOO_LARGE)
        }
    }
}

/// Parses `/sim/<seq>/…` into the session sequence number.
fn session_seq(path: &str) -> Option<u64> {
    let rest = path.strip_prefix("/sim/")?;
    let seq_text = rest.split('/').next()?;
    seq_text.parse().ok()
}

/// An empty response with the given status.
fn plain_status(status: HttpStatusCode) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::new()));
    *response.status_mut() = status;
    response
}

/// Builds the hyper response for a login answer.
fn login_response(answer: &crate::login_endpoint::LoginHttpAnswer) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(answer.body.clone())));
    *response.status_mut() =
        HttpStatusCode::from_u16(answer.status).unwrap_or(HttpStatusCode::INTERNAL_SERVER_ERROR);
    set_content_type(&mut response, answer.content_type);
    response
}

/// Builds the hyper response for a CAPS dispatch outcome.
fn caps_http_response(caps: CapsResponse) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from(caps.body)));
    *response.status_mut() =
        HttpStatusCode::from_u16(caps.status).unwrap_or(HttpStatusCode::INTERNAL_SERVER_ERROR);
    set_content_type(&mut response, caps.content_type);
    if let Some(content_range) = caps.content_range
        && let Ok(value) = http::header::HeaderValue::from_str(&content_range)
    {
        response
            .headers_mut()
            .insert(http::header::CONTENT_RANGE, value);
    }
    response
}

/// Sets the `Content-Type` header when the value is a valid header string.
fn set_content_type(response: &mut Response<Full<Bytes>>, content_type: &str) {
    if let Ok(value) = http::header::HeaderValue::from_str(content_type) {
        response.headers_mut().insert(CONTENT_TYPE, value);
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::session_seq;

    #[test]
    fn session_paths_parse_and_junk_does_not() {
        assert_eq!(session_seq("/sim/1/cap/abc"), Some(1));
        assert_eq!(session_seq("/sim/42"), Some(42));
        assert_eq!(session_seq("/sim/x/cap/abc"), None);
        assert_eq!(session_seq("/other/1"), None);
        assert_eq!(session_seq("/"), None);
    }
}
