//! A [`TextureFetcher`](sl_texture::TextureFetcher) backed by async `reqwest`,
//! for driving an [`sl_texture::TextureStore`] from the tokio client.
//!
//! It fetches byte ranges of a texture's `GetTexture` codestream. The cap URL is
//! held in an [`ArcSwapOption`] so [`Client::run`](crate::Client::run) can refresh
//! it at startup and on every region change without rebuilding the store.

use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use bytes::Bytes;
use reqwest::Client as ReqwestClient;
use reqwest::StatusCode as ReqwestStatusCode;
use sl_proto::TextureKey;
use sl_texture::{AssetFetcher, FetchChunk, FetchError};

/// A `GetTexture` codestream fetcher over a shared async `reqwest` client.
#[derive(Debug)]
pub struct ReqwestTextureFetcher {
    /// The shared HTTP client.
    http: ReqwestClient,
    /// The current `GetTexture` capability URL, or `None` before caps arrive.
    cap_url: ArcSwapOption<String>,
}

impl ReqwestTextureFetcher {
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

    /// Updates (or clears) the `GetTexture` capability URL. Called by the run
    /// loop when the region's capabilities are (re)fetched.
    pub fn set_cap_url(&self, url: Option<String>) {
        self.cap_url.store(url.map(std::sync::Arc::new));
    }
}

#[async_trait]
impl AssetFetcher<TextureKey> for ReqwestTextureFetcher {
    async fn fetch_range(
        &self,
        id: TextureKey,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        let cap = self.cap_url.load_full().ok_or_else(|| {
            FetchError::Transport("GetTexture capability not available".to_owned())
        })?;
        let url = format!("{cap}/?texture_id={id}");
        let response = self
            .http
            .get(&url)
            .header("Accept", "image/x-j2c")
            .header("Range", format!("bytes={start}-{}", end.saturating_sub(1)))
            .send()
            .await
            .map_err(|error| FetchError::Transport(error.to_string()))?;
        let status = response.status();
        if status == ReqwestStatusCode::NOT_FOUND {
            return Err(FetchError::NotFound);
        }
        // A range past the end of the asset means "no more bytes": report an empty
        // chunk so the store stops growing and decodes what it has.
        if status == ReqwestStatusCode::RANGE_NOT_SATISFIABLE {
            return Ok(FetchChunk {
                bytes: Bytes::new(),
                whole: false,
            });
        }
        // 200 = the server ignored the range and returned the whole asset.
        let whole = status == ReqwestStatusCode::OK;
        if !status.is_success() {
            return Err(FetchError::Transport(format!("unexpected status {status}")));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| FetchError::Transport(error.to_string()))?;
        Ok(FetchChunk { bytes, whole })
    }
}
