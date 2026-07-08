//! Bevy integration for the generic-asset [`AssetStore`](sl_asset::AssetStore):
//! a blocking-HTTP [`BlobFetcher`](sl_asset::BlobFetcher) so a Bevy app (which
//! has no async runtime of its own) can build and drive an asset store.
//!
//! A generic asset is an opaque blob, so — unlike the texture and mesh
//! integrations — there is nothing to bridge into a Bevy render asset here; the
//! app receives the raw bytes from the store and interprets them itself. Because
//! the store's `get` is `async`, a Bevy app drives it by `block_on`-ing on a
//! task/thread (this fetcher's HTTP is blocking, matching that use).

use std::time::Duration;

use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use bytes::Bytes;
use reqwest::StatusCode as ReqwestStatusCode;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_asset::{AssetFetcher, AssetRef, FetchChunk, FetchError};

/// How many times to retry the `ViewerAsset` poll service on a transient "not
/// ready" response before giving up.
const MAX_POLL_RETRIES: u32 = 12;

/// The delay between `ViewerAsset` poll retries.
const POLL_RETRY_BACKOFF: Duration = Duration::from_millis(500);

/// Whether `status` is a transient poll-service response the viewer's HTTP layer
/// retries: the `ViewerAsset` service answers `503` (and, behind a proxy, `502`
/// / `504`) while it queues the asset from the backing store, then serves the
/// bytes once ready.
fn is_transient(status: ReqwestStatusCode) -> bool {
    matches!(
        status,
        ReqwestStatusCode::SERVICE_UNAVAILABLE
            | ReqwestStatusCode::BAD_GATEWAY
            | ReqwestStatusCode::GATEWAY_TIMEOUT
    )
}

/// Summarizes a failed HTTP response as a one-line `status; body: …` string
/// (body whitespace-collapsed and truncated), so a fetch error carries what the
/// server actually said. Consumes the response to read its body.
fn describe_failure(response: reqwest::blocking::Response) -> String {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    let snippet: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let snippet: String = snippet.chars().take(300).collect();
    format!("HTTP {status}; body: {snippet}")
}

/// A [`BlobFetcher`](sl_asset::BlobFetcher) over blocking `reqwest`, for a Bevy
/// app with no async runtime. It fetches a generic asset whole over the
/// `ViewerAsset` capability; the capability URL is held in an [`ArcSwapOption`]
/// so it can be refreshed on a region change.
#[derive(Debug)]
pub struct BevyAssetFetcher {
    /// The shared blocking HTTP client.
    http: ReqwestBlockingClient,
    /// The current `ViewerAsset` capability URL, or `None` before caps arrive.
    cap_url: ArcSwapOption<String>,
}

impl BevyAssetFetcher {
    /// A fetcher with a freshly built blocking client and no capability URL yet.
    #[must_use]
    pub fn new() -> Self {
        let http = ReqwestBlockingClient::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_error| ReqwestBlockingClient::new());
        Self {
            http,
            cap_url: ArcSwapOption::empty(),
        }
    }

    /// Updates (or clears) the `ViewerAsset` capability URL.
    pub fn set_cap_url(&self, url: Option<String>) {
        self.cap_url.store(url.map(std::sync::Arc::new));
    }

    /// Whether the `ViewerAsset` capability URL is currently set, i.e. a fetch can
    /// succeed. A consumer that might request an asset before the seed caps have
    /// arrived uses this to defer the request rather than fail it permanently.
    #[must_use]
    pub fn has_cap_url(&self) -> bool {
        self.cap_url.load().is_some()
    }

    /// Performs the blocking request, returning the chunk.
    fn fetch_blocking(
        &self,
        id: AssetRef,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        let cap = self.cap_url.load_full().ok_or_else(|| {
            FetchError::Transport("ViewerAsset capability not available".to_owned())
        })?;
        let key = id.asset_type.get_asset_query_key().ok_or_else(|| {
            FetchError::Transport(format!(
                "asset class {:?} has no fetch query key",
                id.asset_type
            ))
        })?;
        let url = format!("{cap}/?{key}={}", id.id);
        let mut attempt = 0_u32;
        loop {
            let mut request = self
                .http
                .get(&url)
                .header("Accept", "application/octet-stream");
            // `0..usize::MAX` means "the whole asset": send no `Range` header.
            if !(start == 0 && end == usize::MAX) {
                request =
                    request.header("Range", format!("bytes={start}-{}", end.saturating_sub(1)));
            }
            let response = request
                .send()
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
            // The poll service returns 503 while it queues the asset; retry a
            // bounded number of times, as the viewer's HTTP layer does.
            if is_transient(status) {
                if attempt < MAX_POLL_RETRIES {
                    attempt = attempt.saturating_add(1);
                    std::thread::sleep(POLL_RETRY_BACKOFF);
                    continue;
                }
                return Err(FetchError::Unavailable(describe_failure(response)));
            }
            if !status.is_success() {
                return Err(FetchError::Transport(describe_failure(response)));
            }
            let whole = status == ReqwestStatusCode::OK;
            let bytes = response
                .bytes()
                .map_err(|error| FetchError::Transport(error.to_string()))?;
            return Ok(FetchChunk { bytes, whole });
        }
    }
}

impl Default for BevyAssetFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AssetFetcher<AssetRef> for BevyAssetFetcher {
    async fn fetch_range(
        &self,
        id: AssetRef,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        // The blocking request runs on whatever thread `block_on`s this future
        // (a Bevy task/thread dedicated to the fetch), which is the intended use.
        self.fetch_blocking(id, start, end)
    }
}
