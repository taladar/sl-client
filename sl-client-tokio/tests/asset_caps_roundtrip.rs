//! End-to-end round-trip of the server-side asset-delivery caps against the
//! **real** client asset store, with no HTTP transport.
//!
//! The server side ([`sl_proto::AssetCaps`]) is sans-I/O: it turns a
//! [`CapsRequest`] into a [`CapsResponse`]. The client side speaks HTTP over
//! `reqwest` ([`ReqwestAssetFetcher`](sl_client_tokio::ReqwestAssetFetcher)),
//! which cannot be driven without a live server. So this test bridges the two
//! with a shim [`AssetFetcher`] that translates a store fetch into a
//! [`CapsRequest`], dispatches it against [`AssetCaps`], and maps the
//! [`CapsResponse`] back to a [`FetchChunk`] / [`FetchError`] using the *same*
//! status rules the real `ReqwestAssetFetcher` applies (`200` → whole, `206`
//! → range, `404` → `NotFound`, `416` → empty). Driving the real
//! [`sl_asset::AssetStore`] through it proves the client and server agree on
//! the wire contract.

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use async_trait::async_trait;
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_asset::{
        AssetError, AssetFetcher, AssetRef, AssetStore, BlobFetcher, CacheLimits, FetchChunk,
        FetchError,
    };
    use sl_proto::{
        AssetCaps, AssetKey, AssetType, CapsRequest, CapsResponse, InMemoryAssetSource,
    };
    use uuid::Uuid;

    /// A boxed test error.
    type TestError = Box<dyn std::error::Error>;

    /// A `BlobFetcher` that serves the `sl_asset` store from an in-memory
    /// [`AssetCaps`] + [`InMemoryAssetSource`], with no network — the same
    /// `CapsResponse` → `FetchChunk` mapping the real `ReqwestAssetFetcher`
    /// uses.
    #[derive(Debug)]
    struct ShimFetcher {
        /// The server-side asset caps surface.
        caps: AssetCaps,
        /// The bytes the caps serve.
        source: InMemoryAssetSource,
        /// The granted `ViewerAsset` cap URL path the store fetches under.
        path: String,
    }

    impl ShimFetcher {
        /// Builds the shim over a fresh `AssetCaps` (deterministic token mint)
        /// and the given source, resolving the granted `ViewerAsset` path.
        fn new(source: InMemoryAssetSource) -> Result<Self, TestError> {
            let base = "http://cdn.example/".parse()?;
            let mut next: u128 = 0;
            let caps = AssetCaps::new(base, move || {
                next = next.wrapping_add(1);
                Uuid::from_u128(next)
            });
            let granted = caps.grant(&["ViewerAsset".to_owned()]);
            let url: url::Url = granted
                .get("ViewerAsset")
                .ok_or("ViewerAsset not granted")?
                .parse()?;
            Ok(Self {
                caps,
                source,
                path: url.path().to_owned(),
            })
        }
    }

    #[async_trait]
    impl AssetFetcher<AssetRef> for ShimFetcher {
        async fn fetch_range(
            &self,
            id: AssetRef,
            start: usize,
            end: usize,
        ) -> Result<FetchChunk, FetchError> {
            let key = id
                .asset_type
                .get_asset_query_key()
                .ok_or_else(|| FetchError::Transport("class has no query key".to_owned()))?;
            let query = format!("{key}={}", id.id);
            // `0..usize::MAX` means "the whole asset": send no `Range`. Any
            // narrower span issues an inclusive byte-range request, exactly as
            // the real fetcher does.
            let range = if start == 0 && end == usize::MAX {
                None
            } else {
                Some(format!("bytes={start}-{}", end.saturating_sub(1)))
            };
            let request = CapsRequest {
                method: "GET",
                path: &self.path,
                query: Some(&query),
                range: range.as_deref(),
                body: b"",
            };
            let response: CapsResponse = self.caps.dispatch(&self.source, &request);
            match response.status {
                404 => Err(FetchError::NotFound),
                // A range past the end means "no more bytes": an empty,
                // non-whole chunk so the store stops growing.
                416 => Ok(FetchChunk {
                    bytes: Bytes::new(),
                    whole: false,
                }),
                // 200 = whole asset; 206 = exactly the requested range.
                200 => Ok(FetchChunk {
                    bytes: Bytes::from(response.body),
                    whole: true,
                }),
                206 => Ok(FetchChunk {
                    bytes: Bytes::from(response.body),
                    whole: false,
                }),
                other => Err(FetchError::Transport(format!("unexpected status {other}"))),
            }
        }
    }

    /// A store over the shim, serving `assets`, with no disk cache.
    fn store_over(assets: &[(AssetKey, Vec<u8>)]) -> Result<AssetStore, TestError> {
        let source = assets.iter().cloned().collect::<InMemoryAssetSource>();
        let fetcher: Arc<dyn BlobFetcher> = Arc::new(ShimFetcher::new(source)?);
        Ok(AssetStore::new(fetcher, None, CacheLimits::default())?)
    }

    /// A deterministic byte pattern of length `len` (cycling `0..=255`).
    fn pattern(len: usize) -> Vec<u8> {
        (0..len)
            .map(|byte| u8::try_from(byte & 0xff).unwrap_or(0))
            .collect()
    }

    /// The real `sl_asset::AssetStore` fetches a whole asset over the shimmed
    /// `ViewerAsset` cap and returns the exact stored bytes.
    #[tokio::test]
    async fn asset_store_fetches_whole_asset() -> Result<(), TestError> {
        let id = AssetKey::from(Uuid::from_u128(0x50_0d));
        let bytes = pattern(500);
        let store = store_over(&[(id, bytes.clone())])?;

        let entry = store.get(id, AssetType::Notecard).await?;
        let data = entry.data().ok_or("asset bytes present after fetch")?;
        assert_eq!(data.as_ref(), bytes.as_slice());
        Ok(())
    }

    /// A missing asset surfaces as `FetchError::NotFound` (the caps' `404`)
    /// through the real store's error path.
    #[tokio::test]
    async fn asset_store_reports_missing_asset() -> Result<(), TestError> {
        let store = store_over(&[])?;
        let missing = AssetKey::from(Uuid::from_u128(0xdead));
        match store.get(missing, AssetType::Sound).await {
            Err(AssetError::Fetch(FetchError::NotFound)) => Ok(()),
            other => Err(format!("expected NotFound, got {other:?}").into()),
        }
    }

    /// The shim's `fetch_range` — carrying the same `CapsResponse` →
    /// `FetchChunk` mapping as the real fetcher — reassembles a multi-chunk
    /// asset over a progressive `206` loop, and reports an out-of-range tail
    /// as an empty, non-whole chunk.
    #[tokio::test]
    async fn progressive_range_loop_reassembles() -> Result<(), TestError> {
        /// The byte span each progressive fetch pulls.
        const CHUNK: usize = 256;

        let id = AssetKey::from(Uuid::from_u128(0x5417));
        let total = 1000_usize;
        let bytes = pattern(total);
        let source = [(id, bytes.clone())]
            .into_iter()
            .collect::<InMemoryAssetSource>();
        let shim = ShimFetcher::new(source)?;
        let asset_ref = AssetRef::new(id, AssetType::Mesh);

        let mut reassembled: Vec<u8> = Vec::new();
        let mut start = 0_usize;
        loop {
            let chunk = shim
                .fetch_range(asset_ref, start, start.saturating_add(CHUNK))
                .await?;
            if chunk.bytes.is_empty() {
                // The out-of-range tail: an empty, non-whole chunk ends the
                // loop.
                assert!(!chunk.whole);
                break;
            }
            assert!(!chunk.whole, "a ranged response is never whole");
            reassembled.extend_from_slice(&chunk.bytes);
            start = start.saturating_add(chunk.bytes.len());
        }
        assert_eq!(reassembled, bytes);
        Ok(())
    }
}
