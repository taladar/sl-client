//! An [`AssetFetcher`] backed by async `reqwest`, for driving an
//! [`sl_asset::AssetStore`] from the tokio client.
//!
//! It fetches a generic asset whole over the `ViewerAsset` capability (a
//! `GET ?<class>_id=<uuid>`, the class picked from the [`AssetRef`]'s
//! [`AssetType`](sl_proto::AssetType)). The cap URL is held in an
//! [`ArcSwapOption`] so [`Client::run`](crate::Client::run) can refresh it at
//! startup and on every region change without rebuilding the store.

use std::time::Duration;

use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use bytes::Bytes;
use reqwest::Client as ReqwestClient;
use reqwest::StatusCode as ReqwestStatusCode;
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
async fn describe_failure(response: reqwest::Response) -> String {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let snippet: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let snippet: String = snippet.chars().take(300).collect();
    format!("HTTP {status}; body: {snippet}")
}

/// A `ViewerAsset` generic-asset fetcher over a shared async `reqwest` client.
#[derive(Debug)]
pub struct ReqwestAssetFetcher {
    /// The shared HTTP client.
    http: ReqwestClient,
    /// The current `ViewerAsset` capability URL, or `None` before caps arrive.
    cap_url: ArcSwapOption<String>,
}

impl ReqwestAssetFetcher {
    /// A fetcher over `http` with no capability URL yet (set it with
    /// [`Self::set_cap_url`] once the region's caps are known).
    #[must_use]
    pub fn new(http: ReqwestClient) -> Self {
        Self {
            http,
            cap_url: ArcSwapOption::empty(),
        }
    }

    /// A fetcher over a freshly built `reqwest` client (rustls, 60 s timeout),
    /// for callers that do not already have one to share.
    #[must_use]
    pub fn with_default_client() -> Self {
        let http = ReqwestClient::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_error| ReqwestClient::new());
        Self::new(http)
    }

    /// Updates (or clears) the `ViewerAsset` capability URL. Called by the run
    /// loop when the region's capabilities are (re)fetched.
    pub fn set_cap_url(&self, url: Option<String>) {
        self.cap_url.store(url.map(std::sync::Arc::new));
    }
}

#[async_trait]
impl AssetFetcher<AssetRef> for ReqwestAssetFetcher {
    async fn fetch_range(
        &self,
        id: AssetRef,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        let cap = self.cap_url.load_full().ok_or_else(|| {
            FetchError::Transport("ViewerAsset capability not available".to_owned())
        })?;
        // The `ViewerAsset` fetch selects the asset by a class-specific query
        // parameter; a class with no such parameter cannot be fetched this way.
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
            // Any narrower span issues a byte-range request.
            if !(start == 0 && end == usize::MAX) {
                request =
                    request.header("Range", format!("bytes={start}-{}", end.saturating_sub(1)));
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
            // The service can return 503 transiently while it queues the asset;
            // retry a bounded number of times, as the viewer's HTTP layer does.
            if is_transient(status) {
                if attempt < MAX_POLL_RETRIES {
                    attempt = attempt.saturating_add(1);
                    tokio::time::sleep(POLL_RETRY_BACKOFF).await;
                    continue;
                }
                return Err(FetchError::Unavailable(describe_failure(response).await));
            }
            if !status.is_success() {
                return Err(FetchError::Transport(describe_failure(response).await));
            }
            // 200 = the whole asset (either an unranged request or a server that
            // ignored the range); 206 = exactly the requested range.
            let whole = status == ReqwestStatusCode::OK;
            let bytes = response
                .bytes()
                .await
                .map_err(|error| FetchError::Transport(error.to_string()))?;
            return Ok(FetchChunk { bytes, whole });
        }
    }
}
