//! Fetch and cache a worn wearable asset through the higher-level
//! [`AssetStore`]: pull a body-part (or clothing)
//! asset over the live `ViewerAsset` capability, confirm a second request is
//! served from the in-memory cache (the same shared entry, no re-fetch), and
//! confirm a fresh store over the same on-disk cache directory serves it *from
//! disk* — proven by giving that second store a fetcher with no capability URL,
//! so any network fetch would fail.
//!
//! `ViewerAsset` is the modern generic-asset fetch that both Second Life and
//! OpenSim expose; it replaced the legacy UDP `TransferRequest` path (which
//! modern Second Life no longer serves). Textures and meshes are deliberately
//! excluded — they have their own `GetTexture` / `GetMesh` capabilities and
//! decoding stores. Every avatar wears at least its body parts, so the case
//! discovers a worn wearable from the agent's `AgentWearablesUpdate` and pulls
//! its asset; a stripped avatar with no known worn asset id is recorded
//! `partial`.

use std::sync::Arc;
use std::time::Instant;

use sl_client_tokio::{
    AssetCacheLimits, AssetError, AssetFetchError, AssetKey, AssetStore, AssetType, Command, Event,
    ReqwestAssetFetcher, Throttle, Uuid, Wearable,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check};

/// Drives a worn wearable asset through the caching `AssetStore`.
#[derive(Debug)]
pub struct AssetFetchHttp;

impl GridTest for AssetFetchHttp {
    fn name(&self) -> &'static str {
        "asset-fetch-http"
    }

    fn description(&self) -> &'static str {
        "Fetch and cache a wearable asset through the ViewerAsset AssetStore"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // The generic-asset fetch capability, offered by both grids.
            let cap = session.cap("ViewerAsset").ok_or_else(|| {
                TestFailure::Assertion("no ViewerAsset capability offered".to_owned())
            })?;

            // Discover a worn wearable with a known asset id to fetch by id.
            let Some((asset_id, asset_type)) = discover_wearable(session).await? else {
                ctx.mark_partial("no worn wearable with a known asset id to fetch");
                return Ok(());
            };
            let key = AssetKey::from(asset_id);

            // Build a store over the live ViewerAsset capability, backed by its
            // own on-disk cache in a throwaway directory.
            let dir = std::env::temp_dir()
                .join(format!("sl-conformance-assetcache-{}", std::process::id()));
            let _removed = fs_err::remove_dir_all(&dir);
            let fetcher = Arc::new(ReqwestAssetFetcher::with_default_client());
            fetcher.set_cap_url(Some(cap));
            let store = AssetStore::new(fetcher, Some(dir.clone()), AssetCacheLimits::default())
                .map_err(|error| TestFailure::Assertion(format!("open asset store: {error}")))?;

            // Fetch over the network.
            let start = Instant::now();
            let entry = match store.get(key, asset_type).await {
                Ok(entry) => entry,
                Err(error) => {
                    // Log what the server actually said (the fetch error carries
                    // the response status + body snippet) so a failure is
                    // diagnosable from the run output, not just "it failed".
                    tracing::warn!(%error, "asset-fetch-http: ViewerAsset fetch failed");
                    // Aditi's ViewerAsset CDN can persistently answer `503`
                    // (its Akamai edge fails to reach the asset origin for any
                    // asset not already edge-cached); after the fetcher exhausts
                    // its poll retries this is a grid condition, not a client
                    // fault, so record it partial (with the server's response)
                    // rather than failing.
                    if let AssetError::Fetch(AssetFetchError::Unavailable(detail)) = &error {
                        ctx.mark_partial(&format!(
                            "grid ViewerAsset service unavailable — {detail}"
                        ));
                        return Ok(());
                    }
                    return Err(TestFailure::Assertion(format!("get asset: {error}")));
                }
            };
            let fetch_secs = start.elapsed().as_secs_f64();

            let data = entry
                .data()
                .ok_or_else(|| TestFailure::Assertion("asset fetched no bytes".to_owned()))?;
            check(!data.is_empty(), "fetched asset has no data")?;
            check(
                entry.asset_type() == asset_type,
                "entry asset class differs from the requested one",
            )?;
            let byte_len = data.len();

            // A second request for the same held asset returns the same shared
            // entry from memory (no re-fetch).
            let again = store
                .get(key, asset_type)
                .await
                .map_err(|error| TestFailure::Assertion(format!("second get: {error}")))?;
            check(
                Arc::ptr_eq(&entry, &again),
                "second get did not return the cached entry",
            )?;

            // A fresh store over the same directory serves the asset from the
            // on-disk cache. Its fetcher has no capability URL, so a network
            // fetch would fail — success proves the disk cache was used.
            let offline_fetcher = Arc::new(ReqwestAssetFetcher::with_default_client());
            offline_fetcher.set_cap_url(None);
            let disk_store = AssetStore::new(
                offline_fetcher,
                Some(dir.clone()),
                AssetCacheLimits::default(),
            )
            .map_err(|error| TestFailure::Assertion(format!("open disk-cache store: {error}")))?;
            let from_disk = disk_store.get(key, asset_type).await.map_err(|error| {
                TestFailure::Assertion(format!("disk-cache get (no network): {error}"))
            })?;
            let disk_data = from_disk.data().ok_or_else(|| {
                TestFailure::Assertion("disk-cache entry has no bytes".to_owned())
            })?;
            check(
                disk_data.len() == byte_len,
                "disk-cached asset length differs from the fetched one",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing("asset_fetch_secs", fetch_secs);
            metrics.set("asset_bytes", i64::try_from(byte_len).unwrap_or(-1));
            metrics.set("asset_type", format!("{asset_type:?}"));

            let _removed = fs_err::remove_dir_all(&dir);
            Ok(())
        })
    }
}

/// Requests the agent's current wearables and returns the `(asset id, class)` of
/// a worn wearable that carries a known asset id, preferring a body part (always
/// worn) over a clothing layer. Returns `None` when no worn wearable names a
/// (non-nil) asset id.
async fn discover_wearable(
    session: &mut Session,
) -> Result<Option<(Uuid, AssetType)>, TestFailure> {
    session.send(Command::RequestWearables).await?;
    let wearables = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::AgentWearables { wearables, .. } => Some(wearables.clone()),
            _other => None,
        })
        .await?;
    Ok(pick_wearable(&wearables))
}

/// Picks a worn wearable with a known, non-nil asset id, preferring a body part
/// (shape / skin / hair / eyes — reliably worn) over any other layer. Maps its
/// slot to the asset class the fetch needs.
fn pick_wearable(wearables: &[Wearable]) -> Option<(Uuid, AssetType)> {
    let fetchable = |wearable: &Wearable| {
        wearable
            .asset_id
            .filter(|asset_id| !asset_id.is_nil())
            .map(|asset_id| (asset_id, asset_class(wearable)))
    };
    wearables
        .iter()
        .filter(|wearable| wearable.wearable_type.is_body_part())
        .find_map(fetchable)
        .or_else(|| wearables.iter().find_map(fetchable))
}

/// The asset class a wearable's bytes are stored as: body parts are
/// [`AssetType::Bodypart`], every other slot is a [`AssetType::Clothing`] layer.
const fn asset_class(wearable: &Wearable) -> AssetType {
    if wearable.wearable_type.is_body_part() {
        AssetType::Bodypart
    } else {
        AssetType::Clothing
    }
}
