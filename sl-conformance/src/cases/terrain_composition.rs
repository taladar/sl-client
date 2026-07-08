//! Read the current region's terrain-compositing parameters from its
//! `RegionHandshake` and probe whether the four ground ("detail") assets are
//! legacy J2C textures or modern PBR GLTF materials.
//!
//! This underpins the viewer's terrain texture-splatting (`VIEWER_ROADMAP.md`
//! R15): a region ships four ground texture ids plus per-corner elevation bands,
//! and the viewer height-blends the four textures across the ground. On OpenSim
//! (and legacy Second Life) the four ids are ordinary J2C textures that fetch and
//! decode through the `GetTexture` capability. On modern Second Life the region
//! may instead leave the four legacy `TerrainDetail` ids **nil** and drive the
//! ground appearance a different way (PBR terrain materials) — so the legacy
//! splat path, which expects diffuse textures, has nothing to fetch and the
//! ground renders flat (the R15 symptom).
//!
//! To tell a genuine grid difference from a misaligned parse, the case records
//! `RegionInfo` fields that sit *after* the terrain block in the message
//! (`RegionID`, `ProductName`, `ProductSKU`): if those parse correctly while the
//! terrain ids are nil, the terrain block was read at the right offsets and the
//! ids are genuinely nil rather than misaligned. It then attempts a
//! full-resolution fetch+decode of each declared id through the same
//! `TextureStore` the viewer uses, reporting how many decoded.

use std::sync::Arc;

use sl_client_tokio::{
    CacheLimits, Command, DiscardLevel, Event, RegionIdentity, RemoteTextureSource,
    ReqwestTextureFetcher, TextureKey, TextureStore, Throttle,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check};

/// The number of ground ("detail") assets a region blends between.
const DETAIL_COUNT: usize = 4;

/// Reads the region's terrain detail assets and elevation bands, then probes
/// legacy-texture versus PBR-material terrain by attempting to decode each asset.
#[derive(Debug)]
pub struct TerrainComposition;

impl GridTest for TerrainComposition {
    fn name(&self) -> &'static str {
        "terrain-composition"
    }

    fn description(&self) -> &'static str {
        "Read the region's terrain detail assets and elevation bands, and probe \
         legacy-texture vs PBR-material terrain"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();

            // The terrain-compositing parameters ride the `RegionHandshake`,
            // surfaced as `RegionInfoHandshake`. On the root circuit it is pushed
            // immediately before `RegionHandshakeComplete`, so wait for it
            // *directly* as the first action — a prior `wait_for_region` would
            // consume (and discard) this root event, leaving only a neighbour
            // child-circuit handshake to catch, which may not arrive.
            let identity: RegionIdentity = session
                .wait_for(REGION_TIMEOUT, |event| match event {
                    Event::RegionInfoHandshake(identity) => Some((**identity).clone()),
                    _ => None,
                })
                .await?;
            let terrain = identity.terrain;

            // The per-corner elevation bands must at least parse into finite
            // values (Firestorm's `LLVLComposition`).
            check(
                terrain.start_heights.iter().all(|h| h.is_finite())
                    && terrain.height_ranges.iter().all(|h| h.is_finite()),
                "terrain elevation bands did not parse to finite values",
            )?;

            let declared = terrain
                .detail_textures
                .iter()
                .filter(|id| !id.is_nil())
                .count();
            // The ids the viewer will actually request: nil slots substituted with
            // the default Linden terrain textures (the R15 fix).
            let effective = terrain.detail_textures_or_default();

            // Build a store over the live `GetTexture` capability — the exact
            // path the viewer's terrain splat uses to fetch its detail textures.
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;
            let cap = session
                .cap("GetTexture")
                .ok_or_else(|| TestFailure::Assertion("no GetTexture capability".to_owned()))?;
            let fetcher = Arc::new(ReqwestTextureFetcher::with_default_client());
            fetcher.set_cap_url(Some(cap));
            let dir = std::env::temp_dir().join(format!(
                "sl-conformance-terraincache-{}",
                std::process::id()
            ));
            let _removed = fs_err::remove_dir_all(&dir);
            let store = TextureStore::new(fetcher, Some(dir.clone()), CacheLimits::default())
                .map_err(|error| TestFailure::Assertion(format!("open texture store: {error}")))?;

            // Attempt a full-resolution fetch+decode of each effective detail id —
            // the region's own id, or the default fallback for a nil slot. A
            // renderable region decodes all four; a PBR-material region (non-nil
            // ids that are GLTF materials, not J2C textures) decodes none.
            let mut decoded_ok = 0_usize;
            for (index, (id, raw)) in effective
                .iter()
                .zip(terrain.detail_textures.iter())
                .enumerate()
            {
                ctx.metrics()
                    .set(&format!("detail{index}_id"), raw.to_string());
                ctx.metrics()
                    .set(&format!("detail{index}_substituted"), raw.is_nil());
                let outcome = store
                    .get(
                        TextureKey::from(*id),
                        DiscardLevel::FULL,
                        RemoteTextureSource::Default,
                    )
                    .await;
                let ok = matches!(&outcome, Ok(entry)
                    if entry.image().is_some_and(|image| image.width > 0 && image.height > 0));
                if ok {
                    decoded_ok = decoded_ok.saturating_add(1);
                }
                ctx.metrics().set(&format!("detail{index}_decoded"), ok);
            }

            // Record the elevation bands, the fields that sit *after* the terrain
            // block in the message (to prove alignment), and the verdict.
            let metrics = ctx.metrics();
            for (index, (start, range)) in terrain
                .start_heights
                .iter()
                .zip(terrain.height_ranges.iter())
                .enumerate()
            {
                metrics.set(&format!("start_height{index}"), f64::from(*start));
                metrics.set(&format!("height_range{index}"), f64::from(*range));
            }
            metrics.set(
                "sim_name",
                identity
                    .sim_name
                    .as_ref()
                    .map_or_else(String::new, ToString::to_string),
            );
            metrics.set("region_id", identity.region_id.to_string());
            metrics.set("product_name", identity.product_name.clone());
            metrics.set("product_sku", identity.product_sku.clone());
            let substituted = DETAIL_COUNT.saturating_sub(declared);
            let declared_i64 = i64::try_from(declared).unwrap_or(-1);
            let decoded_i64 = i64::try_from(decoded_ok).unwrap_or(-1);
            metrics.set("detail_textures_declared", declared_i64);
            metrics.set(
                "detail_textures_substituted",
                i64::try_from(substituted).unwrap_or(-1),
            );
            metrics.set("detail_textures_decoded", decoded_i64);
            let mode = if decoded_ok < DETAIL_COUNT {
                "pbr-material-or-unfetchable"
            } else if declared == DETAIL_COUNT {
                "legacy-texture"
            } else {
                "default-substituted"
            };
            metrics.set("terrain_mode", mode);

            let _removed = fs_err::remove_dir_all(&dir);

            // With the nil-slot fallback the ground is renderable whenever all four
            // effective detail textures decode — including the modern Second Life
            // mainland case where the handshake sends nil ids and the defaults are
            // substituted. A region is only unrenderable by the legacy splat when a
            // non-nil id fails to decode as J2C (a PBR GLTF material), which awaits
            // GLTF material support (Phase 27); mark that partial.
            if decoded_ok < DETAIL_COUNT {
                let count = DETAIL_COUNT.saturating_sub(decoded_ok);
                ctx.mark_partial(&format!(
                    "{count} of {DETAIL_COUNT} terrain detail assets did not decode as J2C \
                     textures (likely PBR GLTF material terrain — needs Phase 27)"
                ));
            }
            Ok(())
        })
    }
}
