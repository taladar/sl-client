//! A [`MeshFetcher`](sl_mesh::MeshFetcher) backed by async `reqwest`, for driving
//! an [`sl_mesh::MeshStore`] from the tokio client.
//!
//! It fetches byte ranges of a mesh's `GetMesh2` / `GetMesh` asset. The cap URL
//! is held in an [`ArcSwapOption`] so [`Client::run`](crate::Client::run) can
//! refresh it at startup and on every region change without rebuilding the store
//! (prefer `GetMesh2` when the region offers it, else `GetMesh`).

use arc_swap::ArcSwapOption;
use async_trait::async_trait;
use bytes::Bytes;
use reqwest::Client as ReqwestClient;
use reqwest::StatusCode as ReqwestStatusCode;
use sl_mesh::{AssetFetcher, FetchChunk, FetchError};
use sl_proto::MeshKey;

/// The `Accept` MIME type for a mesh asset (the viewer's
/// `HTTP_CONTENT_VND_LL_MESH`).
const MESH_ACCEPT: &str = "application/vnd.ll.mesh";

/// A `GetMesh2` / `GetMesh` asset fetcher over a shared async `reqwest` client.
#[derive(Debug)]
pub struct ReqwestMeshFetcher {
    /// The shared HTTP client.
    http: ReqwestClient,
    /// The current `GetMesh2` / `GetMesh` capability URL, or `None` before caps
    /// arrive.
    cap_url: ArcSwapOption<String>,
}

impl ReqwestMeshFetcher {
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

    /// Updates (or clears) the mesh capability URL. Called by the run loop when
    /// the region's capabilities are (re)fetched (prefer `GetMesh2`, else
    /// `GetMesh`).
    pub fn set_cap_url(&self, url: Option<String>) {
        self.cap_url.store(url.map(std::sync::Arc::new));
    }
}

#[async_trait]
impl AssetFetcher<MeshKey> for ReqwestMeshFetcher {
    async fn fetch_range(
        &self,
        id: MeshKey,
        start: usize,
        end: usize,
    ) -> Result<FetchChunk, FetchError> {
        let cap = self
            .cap_url
            .load_full()
            .ok_or_else(|| FetchError::Transport("mesh capability not available".to_owned()))?;
        let url = format!("{cap}/?mesh_id={id}");
        let response = self
            .http
            .get(&url)
            .header("Accept", MESH_ACCEPT)
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
