//! Bevy integration for the generic-asset [`AssetStore`](sl_asset::AssetStore):
//! a blocking-HTTP [`BlobFetcher`](sl_asset::BlobFetcher) so a Bevy app (which
//! has no async runtime of its own) can build and drive an asset store.
//!
//! A generic asset is an opaque blob, so — unlike the texture and mesh
//! integrations — there is nothing to bridge into a Bevy render asset here; the
//! app receives the raw bytes from the store and interprets them itself. Because
//! the store's `get` is `async`, a Bevy app drives it by `block_on`-ing on a
//! task/thread (this fetcher's HTTP is blocking, matching that use).

use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use bytes::Bytes;
use reqwest::StatusCode as ReqwestStatusCode;
use reqwest::blocking::Client as ReqwestBlockingClient;
use sl_asset::{AssetFetcher, AssetRef, FetchChunk, FetchError};

use crate::async_http::{fetch_range_async, shared_async_client};
use crate::async_runtime::run_on_shared_runtime;
use crate::retry::{MAX_TRANSIENT_RETRIES, is_transient_status, transient_backoff};

/// The `Accept` header a generic-asset fetch sends.
const ASSET_ACCEPT: &str = "application/octet-stream";

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
        let http = crate::http_proxy::blocking_client_builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            // Proxy-less fallback: the builder only fails on TLS backend
            // initialization (the proxy value was validated at startup).
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

    /// The `ViewerAsset` URL a fetch of `id` targets: the capability queried by
    /// the asset class's query key. Errors if the capability is not set yet or the
    /// asset class has no fetch query key. Shared by the async and blocking paths.
    fn resolve_url(&self, id: AssetRef) -> Result<String, FetchError> {
        let cap = self.cap_url.load_full().ok_or_else(|| {
            FetchError::Transport("ViewerAsset capability not available".to_owned())
        })?;
        let key = id.asset_type.get_asset_query_key().ok_or_else(|| {
            FetchError::Transport(format!(
                "asset class {:?} has no fetch query key",
                id.asset_type
            ))
        })?;
        Ok(format!("{cap}/?{key}={}", id.id))
    }

    /// Performs the blocking request, returning the chunk — the fallback path when
    /// the shared async runtime / client is unavailable.
    fn fetch_blocking(
        &self,
        id: AssetRef,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        let url = self.resolve_url(id)?;
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
            // The poll service returns 503 while it queues the asset; retry with
            // exponential backoff rather than failing the fetch.
            if is_transient_status(status) {
                if attempt < MAX_TRANSIENT_RETRIES {
                    std::thread::sleep(transient_backoff(attempt));
                    attempt = attempt.saturating_add(1);
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
        // Prefer the shared async runtime: the non-blocking request yields at each
        // `.await`, so this fetch does not monopolise its `IoTaskPool` thread and
        // the store's admission gate governs real concurrency. Fall back to the
        // blocking client only if the shared client / runtime is unavailable.
        if let Some(client) = shared_async_client() {
            let url = self.resolve_url(id)?;
            // `0..usize::MAX` means "the whole asset": send no `Range` header.
            let range = if start == 0 && end == usize::MAX {
                None
            } else {
                Some((start, end))
            };
            if let Some(result) =
                run_on_shared_runtime(fetch_range_async(client, url, ASSET_ACCEPT, range)).await
            {
                return result;
            }
        }
        self.fetch_blocking(id, start, end)
    }
}
