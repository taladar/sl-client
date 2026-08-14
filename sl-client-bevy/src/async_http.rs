//! The **async asset-fetch HTTP** layer: a shared non-blocking `reqwest::Client`
//! and a single range-request helper the texture / mesh / generic-asset fetchers
//! call, driven on the crate's [shared tokio runtime](crate::async_runtime).
//!
//! Every asset fetcher used to perform a **blocking** `reqwest` request inside an
//! `async fn` body: the future never yielded, so it monopolised a whole
//! `IoTaskPool` thread for its entire round-trip. Bevy's `IoTaskPool` is a real
//! async executor, but it caps at `max_threads: 4`, so at most ~4 downloads ran
//! at once no matter how many were queued — the store-side admission gates
//! (texture 16, mesh, …) and the CPU decode permits all sat *downstream* of that
//! 4-wide funnel.
//!
//! Now a fetcher's `IoTaskPool` task hands its request to the shared runtime with
//! [`run_on_shared_runtime`](crate::async_runtime::run_on_shared_runtime) and
//! `.await`s the result: the socket IO is non-blocking, so the executor
//! interleaves every admitted fetch and the store gates become the real
//! concurrency governor. The F3 pipeline overlay's `dl` / `gate in_flight`
//! figures then reflect real in-flight work instead of a backlog stuck behind the
//! funnel.
//!
//! It **falls back gracefully**: if the shared client cannot be built (or the
//! runtime is absent), the fetchers revert to the blocking `reqwest` client each
//! still holds, so a failure here never keeps the viewer from loading assets.

use std::time::Duration;

use bytes::Bytes;
use reqwest::StatusCode as ReqwestStatusCode;
use sl_asset::{FetchChunk, FetchError};

use crate::retry::{MAX_TRANSIENT_RETRIES, is_transient_status, transient_backoff};

/// The per-request timeout for the shared async client, matching the 60 s each
/// fetcher's blocking client used.
const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

/// The shared async `reqwest::Client` every asset fetch reuses, so connections to
/// the `GetTexture` / `GetMesh` / `ViewerAsset` hosts pool across all asset
/// pipelines. `None` if the client could not be built (the fetchers then fall
/// back to their own blocking client). Built lazily on first fetch.
static FETCH_CLIENT: std::sync::LazyLock<Option<reqwest::Client>> =
    std::sync::LazyLock::new(|| {
        crate::http_proxy::async_client_builder()
            .timeout(FETCH_TIMEOUT)
            .build()
            .ok()
    });

/// A clone of the shared async client, or `None` if it could not be built. A
/// `reqwest::Client` is a cheap `Arc`-style handle, so cloning it just shares the
/// underlying connection pool. `None` signals the caller to take its blocking
/// fallback.
pub(crate) fn shared_async_client() -> Option<reqwest::Client> {
    FETCH_CLIENT.as_ref().cloned()
}

/// Summarizes a failed async HTTP response as a one-line `HTTP status; body: …`
/// string (body whitespace-collapsed and truncated), so a fetch error carries
/// what the server actually said. Consumes the response to read its body.
async fn describe_failure(response: reqwest::Response) -> String {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let snippet: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let snippet: String = snippet.chars().take(300).collect();
    format!("HTTP {status}; body: {snippet}")
}

/// Perform one asset range request over the shared async client, with the same
/// 404 / range-not-satisfiable / transient-503-retry handling the blocking
/// fetchers use — but yielding at each `.await` so the executor can interleave
/// other fetches. `range` is an inclusive-lower / exclusive-upper byte span, or
/// `None` for the whole asset (no `Range` header). `accept` is the fetcher's
/// `Accept` header. Owns its `url` / `client` so the returned future is
/// `Send + 'static` and can be `spawn`ed on the runtime.
pub(crate) async fn fetch_range_async(
    client: reqwest::Client,
    url: String,
    accept: &'static str,
    range: Option<(usize, usize)>,
) -> Result<FetchChunk, FetchError> {
    let mut attempt = 0_u32;
    loop {
        let mut request = client.get(&url).header("Accept", accept);
        if let Some((start, end)) = range {
            request = request.header("Range", format!("bytes={start}-{}", end.saturating_sub(1)));
        }
        let response = request
            .send()
            .await
            .map_err(|error| FetchError::Transport(error.to_string()))?;
        let status = response.status();
        if status == ReqwestStatusCode::NOT_FOUND {
            return Err(FetchError::NotFound);
        }
        if status == ReqwestStatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(FetchChunk {
                bytes: Bytes::new(),
                whole: false,
            });
        }
        // The GetTexture / GetMesh / ViewerAsset services answer `503` while they
        // queue the asset; retry with exponential backoff (async sleep on the
        // runtime, not a thread park) rather than failing the fetch.
        if is_transient_status(status) {
            if attempt < MAX_TRANSIENT_RETRIES {
                tokio::time::sleep(transient_backoff(attempt)).await;
                attempt = attempt.saturating_add(1);
                continue;
            }
            return Err(FetchError::Unavailable(describe_failure(response).await));
        }
        let whole = status == ReqwestStatusCode::OK;
        if !status.is_success() {
            return Err(FetchError::Transport(describe_failure(response).await));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| FetchError::Transport(error.to_string()))?;
        return Ok(FetchChunk { bytes, whole });
    }
}

#[cfg(test)]
mod tests {
    use super::{fetch_range_async, shared_async_client};
    use crate::async_runtime::run_on_shared_runtime;
    use bevy::tasks::block_on;
    use sl_asset::FetchError;

    /// The shared async client builds, so the fast path is available (and the
    /// fetchers do not silently run everything on the blocking fallback).
    #[test]
    fn shared_client_builds() {
        assert!(shared_async_client().is_some());
    }

    /// A request to a syntactically invalid URL fails fast as a
    /// [`FetchError::Transport`], driven all the way through the shared runtime
    /// offload — exercising the full async request path and its error mapping
    /// without any network (reqwest rejects the URL before connecting).
    #[test]
    fn invalid_url_maps_to_transport_error() {
        let Some(client) = shared_async_client() else {
            // No client — the blocking fallback covers this environment; nothing
            // to assert about the async path.
            return;
        };
        let result = block_on(run_on_shared_runtime(fetch_range_async(
            client,
            "not-a-valid-url".to_owned(),
            "application/octet-stream",
            None,
        )));
        assert!(matches!(result, Some(Err(FetchError::Transport(_)))));
    }
}
