//! Fetch, decode, and cache a texture through the higher-level
//! [`TextureStore`]: pull the plywood texture over
//! the live `GetTexture` capability, decode it to RGBA8, confirm a second request
//! is served from the in-memory cache (the same shared entry, no re-fetch), and
//! exercise a level-of-detail downgrade (downsample, no re-decode) and re-upgrade.

use std::sync::Arc;
use std::time::Instant;

use sl_client_tokio::{
    CacheLimits, Command, DiscardLevel, ReqwestTextureFetcher, TextureStore, Throttle,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, fixtures};

/// A coarse discard level for the downgrade leg (three halvings of each side).
const COARSE_DISCARD_LEVEL: DiscardLevel = DiscardLevel::from_clamped(3);

/// Drives the plywood texture through the decoding, LOD-aware `TextureStore`.
#[derive(Debug)]
pub struct TextureFetchHttp;

impl GridTest for TextureFetchHttp {
    fn name(&self) -> &'static str {
        "texture-fetch-http"
    }

    fn description(&self) -> &'static str {
        "Fetch, decode, and cache a texture through the LOD-aware TextureStore"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let texture_id = fixtures::plywood_texture()?;

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // Build a store over the live GetTexture capability, backed by its own
            // (LL-format) on-disk cache in a throwaway directory.
            let cap = session
                .cap("GetTexture")
                .ok_or_else(|| TestFailure::Assertion("no GetTexture capability".to_owned()))?;
            let fetcher = Arc::new(ReqwestTextureFetcher::with_default_client());
            fetcher.set_cap_url(Some(cap));
            let dir = std::env::temp_dir()
                .join(format!("sl-conformance-texcache-{}", std::process::id()));
            let _removed = fs_err::remove_dir_all(&dir);
            let store = TextureStore::new(fetcher, Some(dir.clone()), CacheLimits::default())
                .map_err(|error| TestFailure::Assertion(format!("open texture store: {error}")))?;

            // Full-resolution fetch + decode.
            let start = Instant::now();
            let entry = store
                .get(texture_id, DiscardLevel::FULL)
                .await
                .map_err(|error| TestFailure::Assertion(format!("get full texture: {error}")))?;
            let full_secs = start.elapsed().as_secs_f64();

            let image = entry
                .image()
                .ok_or_else(|| TestFailure::Assertion("texture decoded to no image".to_owned()))?;
            check(
                image.width > 0 && image.height > 0,
                "decoded image has zero size",
            )?;
            check(
                image.pixels.len() == image.expected_len(),
                "decoded RGBA buffer length does not match width*height*4",
            )?;
            let full_width = image.width;

            // A second request for the same held texture returns the same shared
            // entry from memory (no re-fetch, no re-decode).
            let again = store
                .get(texture_id, DiscardLevel::FULL)
                .await
                .map_err(|error| TestFailure::Assertion(format!("second get: {error}")))?;
            check(
                Arc::ptr_eq(&entry, &again),
                "second get did not return the cached entry",
            )?;

            // Downgrade to a coarse LOD in place (downsample, no decode), then
            // re-upgrade to full resolution.
            store
                .set_lod(&entry, COARSE_DISCARD_LEVEL)
                .await
                .map_err(|error| TestFailure::Assertion(format!("downgrade: {error}")))?;
            let coarse = entry
                .image()
                .ok_or_else(|| TestFailure::Assertion("no image after downgrade".to_owned()))?;
            check(
                coarse.width < full_width,
                "downgrade did not shrink the image",
            )?;
            check(
                coarse.discard_level == COARSE_DISCARD_LEVEL,
                "downgrade did not reach the target level",
            )?;
            let coarse_width = coarse.width;
            store
                .set_lod(&entry, DiscardLevel::FULL)
                .await
                .map_err(|error| TestFailure::Assertion(format!("re-upgrade: {error}")))?;

            let rgba_bytes = i64::try_from(image.pixels.len()).unwrap_or(-1);
            let metrics = ctx.metrics();
            metrics.set_timing("store_get_secs", full_secs);
            metrics.set("texture_width", full_width);
            metrics.set("texture_height", image.height);
            metrics.set("texture_components", i64::from(image.components));
            metrics.set("texture_rgba_bytes", rgba_bytes);
            metrics.set("texture_downgraded_width", coarse_width);

            let _removed = fs_err::remove_dir_all(&dir);
            Ok(())
        })
    }
}
