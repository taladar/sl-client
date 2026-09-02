//! The hyper HTTP service: one loopback listener serving the login endpoint
//! at `/` (also the XML-RPC `get_grid_info` method), `GET /get_grid_info`,
//! the economy helper scripts (`/currency.php`, `/landtool.php`), the
//! world-map tiles (`/map-<zoom>-<x>-<y>-objects.jpg`), and every session's
//! CAPS surface under `/sim/<seq>/…`.
//!
//! Connections are served one spawned task each, so a held `EventQueueGet`
//! long-poll never starves other requests (the client's HTTP stack pools
//! connections and opens new ones as needed). Each connection task holds a
//! semaphore permit (a bound on concurrent connections), is dropped if it
//! does not send its request head in time, and shuts down with the grid.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use http::header::CONTENT_TYPE;
use http::status::StatusCode as HttpStatusCode;
use http_body_util::{BodyExt as _, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};
use sl_proto::{AssetKey, AssetSource as _, CapsResponse};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

use crate::caps_endpoint::dispatch_caps;
use crate::economy_endpoint::{handle_helper, is_helper_path};
use crate::http_answer::HttpAnswer;
use crate::login_endpoint::handle_login;
use crate::runtime::GridCore;

/// The XML-RPC content type.
const XML_RPC_CONTENT_TYPE: &str = "text/xml";

/// The largest request body accepted, uploads included.
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// The most connections served at once; a peer beyond this is dropped rather
/// than allowed to pin an unbounded number of tasks and file descriptors.
const MAX_CONNECTIONS: usize = 256;

/// How long the accept loop waits after a failed `accept` before trying
/// again, so an exhausted descriptor table cannot spin the task.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(50);

/// How long a connection may take to send its request head before it is
/// dropped — a peer that connects and then says nothing never pins a task.
const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(15);

/// How long a connection may take to finish its in-flight request after the
/// grid asked it to shut down, before it is dropped outright.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(1);

/// The accept loop: serves connections until shutdown.
pub(crate) async fn run_http(core: Arc<GridCore>, listener: TcpListener) {
    let mut shutdown_rx = core.shutdown_tx.subscribe();
    let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _peer) = match accepted {
                    Ok(pair) => pair,
                    Err(error) => {
                        tracing::warn!("accepting an HTTP connection failed: {error}");
                        tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                        continue;
                    }
                };
                let Ok(permit) = Arc::clone(&connections).try_acquire_owned() else {
                    tracing::warn!(
                        "refusing an HTTP connection: {MAX_CONNECTIONS} already open"
                    );
                    drop(stream);
                    continue;
                };
                tokio::spawn(serve_connection(
                    Arc::clone(&core),
                    stream,
                    core.shutdown_tx.subscribe(),
                    permit,
                ));
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() {
                    break;
                }
            }
        }
    }
}

/// Serves one accepted connection, holding `permit` for its lifetime: the
/// request head must arrive within [`HEADER_READ_TIMEOUT`], and a shutdown
/// ends the connection gracefully — or, if it does not finish within
/// [`SHUTDOWN_GRACE`], by dropping it.
async fn serve_connection(
    core: Arc<GridCore>,
    stream: TcpStream,
    mut shutdown_rx: watch::Receiver<bool>,
    permit: OwnedSemaphorePermit,
) {
    let service = hyper::service::service_fn(move |request| {
        let core = Arc::clone(&core);
        async move { handle_request(core, request).await }
    });
    let mut builder = hyper::server::conn::http1::Builder::new();
    // The header-read timeout is inert without a timer.
    builder
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT);
    let connection = builder.serve_connection(hyper_util::rt::TokioIo::new(stream), service);
    let mut connection = std::pin::pin!(connection);
    tokio::select! {
        result = connection.as_mut() => {
            if let Err(error) = result {
                tracing::debug!("HTTP connection ended with an error: {error}");
            }
        }
        _ = shutdown_rx.changed() => {
            connection.as_mut().graceful_shutdown();
            match tokio::time::timeout(SHUTDOWN_GRACE, connection).await {
                Ok(Err(error)) => {
                    tracing::debug!("HTTP connection ended with an error: {error}");
                }
                Ok(Ok(())) => {}
                Err(_) => tracing::debug!(
                    "dropping an HTTP connection still busy after the shutdown grace"
                ),
            }
        }
    }
    drop(permit);
}

/// Routes one request: `POST /` → login (or the XML-RPC `get_grid_info`
/// method), `GET /get_grid_info` → the grid-info XML, the helper scripts →
/// the economy endpoint, tile paths → the map-tile store, `/sim/<seq>/…` →
/// that session's CAPS surface, anything else → 404.
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
        // Real login hosts serve `get_grid_info` as an XML-RPC method on the
        // login URL too; anything else (including LLSD) is a login.
        if std::str::from_utf8(&body)
            .ok()
            .and_then(sl_wire::xmlrpc::method_name)
            .is_some_and(|name| name == sl_wire::GRID_INFO_METHOD)
        {
            return Ok(answer_response(HttpAnswer::ok(
                XML_RPC_CONTENT_TYPE,
                sl_wire::build_grid_info_xmlrpc_response(&core.grid_info),
            )));
        }
        let answer = handle_login(&core, content_type.as_deref().unwrap_or(""), &body).await;
        return Ok(answer_response(answer));
    }

    if path.strip_prefix('/') == Some(sl_wire::GRID_INFO_PATH) {
        if method != "GET" {
            return Ok(plain_status(HttpStatusCode::METHOD_NOT_ALLOWED));
        }
        return Ok(answer_response(HttpAnswer::ok(
            XML_RPC_CONTENT_TYPE,
            sl_wire::build_grid_info_xml(&core.grid_info),
        )));
    }

    if is_helper_path(&path) {
        if method != "POST" {
            return Ok(plain_status(HttpStatusCode::METHOD_NOT_ALLOWED));
        }
        return Ok(answer_response(handle_helper(&core, &path, &body)));
    }

    if let Some(answer) = core.map_tiles.answer(&method, &path) {
        return Ok(answer_response(answer));
    }

    if let Some(seq) = session_seq(&path) {
        let Some(shared) = core.session(seq).await else {
            return Ok(plain_status(HttpStatusCode::NOT_FOUND));
        };
        if let Some(texture) = appearance_texture_id(&path) {
            if method != "GET" {
                return Ok(plain_status(HttpStatusCode::METHOD_NOT_ALLOWED));
            }
            let answer = {
                let guard = shared.state.lock().await;
                guard
                    .assets
                    .get(AssetKey::from(texture))
                    .map(|bytes| ranged_asset(bytes, range.as_deref(), J2C_CONTENT_TYPE))
            };
            return Ok(match answer {
                Some(answer) => answer_response(answer),
                None => plain_status(HttpStatusCode::NOT_FOUND),
            });
        }
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

/// The `Content-Type` a baked avatar texture is served with, the same
/// JPEG2000 codestream media type the `GetTexture` cap uses.
const J2C_CONTENT_TYPE: &str = "image/x-j2c";

/// Serves stored asset bytes, honouring a `Range` header the way the asset
/// caps do: no range → `200` whole; a satisfiable range → `206` with the
/// slice and a `Content-Range`; a start past the end → `416`.
///
/// The viewer's texture fetcher opens a texture with `Range: bytes=0-599` and
/// only asks for the rest once it has read the codestream header, so a route
/// that serves textures has to answer ranges — and has to advertise that it
/// does, or the fetcher cannot tell a short whole body from a truncated one.
fn ranged_asset(bytes: &[u8], range: Option<&str>, content_type: &'static str) -> HttpAnswer {
    let whole = || {
        HttpAnswer::ok(content_type, bytes.to_vec())
            .header("accept-ranges", "bytes")
            .header("content-length", bytes.len().to_string())
    };
    let Some((start, end)) = range.and_then(parse_byte_range) else {
        return whole();
    };
    if start >= bytes.len() {
        return HttpAnswer::status(HttpStatusCode::RANGE_NOT_SATISFIABLE.as_u16())
            .header("content-range", format!("bytes */{}", bytes.len()));
    }
    // A range covering the whole asset is answered as the whole asset: the
    // fetcher's opening `bytes=0-599` on a texture shorter than 600 bytes is
    // the common case, and a 206 claiming a range wider than the body is not.
    let end = end
        .unwrap_or_else(|| bytes.len().saturating_sub(1))
        .min(bytes.len().saturating_sub(1));
    if start == 0 && end == bytes.len().saturating_sub(1) {
        return whole();
    }
    let slice = bytes.get(start..=end).unwrap_or_default().to_vec();
    let len = slice.len();
    HttpAnswer::with_status(
        HttpStatusCode::PARTIAL_CONTENT.as_u16(),
        content_type,
        slice,
    )
    .header("accept-ranges", "bytes")
    .header("content-length", len.to_string())
    .header(
        "content-range",
        format!("bytes {start}-{end}/{}", bytes.len()),
    )
}

/// Parses a single-range `Range: bytes=<start>-[<end>]` header into its
/// offsets. Multi-range and suffix (`bytes=-N`) forms are not used by the
/// viewer and are treated as absent.
fn parse_byte_range(header: &str) -> Option<(usize, Option<usize>)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start, end) = spec.split_once('-')?;
    let start = start.trim().parse::<usize>().ok()?;
    let end = end.trim();
    let end = if end.is_empty() {
        None
    } else {
        Some(end.parse::<usize>().ok()?)
    };
    Some((start, end))
}

/// Parses an **appearance-service** texture URL into the baked texture id it
/// names: `/sim/<seq>/appearance/texture/<agent>/<slot>/<uuid>`.
///
/// This is the grid's avatar-baking service, the one the login response names
/// in `agent_appearance_service`. The reference viewer builds the URL as
/// `<service>texture/<agent id>/<slot name>/<texture id>`
/// (`LLVOAvatar::getImageURL`) and will not fetch a baked slot any other way
/// once it decides the avatar is server-baked — so a grid that hands out bake
/// ids but names no service leaves every avatar, the agent's own included,
/// permanently a cloud with no failing request to point at: `getImageURL`
/// returns an empty URL and nothing is ever requested.
///
/// The fake grid runs no real bake service: the bakes are fabricated per
/// session ([`crate::world`]) and live in that session's asset store, so the
/// route is mounted **under the session** rather than at the grid root a real
/// deployment would use. The agent id and slot name are accepted and ignored
/// — the store is keyed by texture id alone, and the fake grid has one
/// appearance per session to serve.
fn appearance_texture_id(path: &str) -> Option<uuid::Uuid> {
    let rest = path.strip_prefix("/sim/")?;
    let rest = rest.split_once('/')?.1;
    let rest = rest.strip_prefix("appearance/texture/")?;
    let mut parts = rest.split('/');
    let _agent = parts.next()?;
    let _slot = parts.next()?;
    let texture = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    uuid::Uuid::parse_str(texture).ok()
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

/// Builds the hyper response for a non-CAPS endpoint answer.
fn answer_response(answer: HttpAnswer) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(answer.body));
    *response.status_mut() =
        HttpStatusCode::from_u16(answer.status).unwrap_or(HttpStatusCode::INTERNAL_SERVER_ERROR);
    if !answer.content_type.is_empty() {
        set_content_type(&mut response, answer.content_type);
    }
    for (name, value) in answer.headers {
        if let (Ok(name), Ok(value)) = (
            http::header::HeaderName::from_bytes(name.as_bytes()),
            http::header::HeaderValue::from_str(&value),
        ) {
            response.headers_mut().insert(name, value);
        }
    }
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
