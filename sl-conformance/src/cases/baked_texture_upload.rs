//! Upload a client-baked avatar texture over the `UploadBakedTexture` capability
//! and confirm the two-step CAPS flow stores a *temporary* texture asset with no
//! inventory item — the outcome the legacy (client-side-bake) appearance path
//! relies on.
//!
//! The uploader is the same two-step CAPS POST as `NewFileAgentInventory`
//! ([`asset-upload`](super::asset_upload)) but for a baked avatar texture: an
//! (empty) LLSD metadata body goes to the `UploadBakedTexture` capability, which
//! answers with an `uploader` URL; the raw JPEG-2000 codestream goes there, and
//! the completion carries the new (temporary) asset UUID. Unlike
//! `NewFileAgentInventory`, no inventory item is created — a baked texture is a
//! throwaway asset the viewer references from `AgentSetAppearance`, so the
//! completion's `new_inventory_item` is nil (surfaced as
//! [`Event::AssetUploaded`] with `new_inventory_item = None`).
//!
//! The bytes uploaded are a real JPEG-2000 codestream: the case first fetches the
//! plywood texture's `GetTexture` codestream (the same asset `texture-fetch-http`
//! drives, present on both grids) and re-uploads those bytes as the bake. That
//! keeps the payload a valid J2C on Second Life — which validates the bake —
//! without a client-side JPEG-2000 encoder (the decode-only `sl-texture` crate
//! has none), while OpenSim caches the bytes verbatim as a temporary texture.
//!
//! **Grid divergence:** both grids offer the capability and the legacy appearance
//! path uploads over it, so the store-a-temporary-asset flow runs on each. If a
//! grid does not offer `UploadBakedTexture` (or declines the bake), the case
//! records `partial` with the reason — the client formed and POSTed the request
//! correctly — mirroring `asset-upload`'s aditi handling.

use std::time::Instant;

use sl_client_tokio::{AssetFetcher as _, Command, Event, ReqwestTextureFetcher, Throttle};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, fixtures, is_aditi};

/// A generous upper bound on the plywood codestream fetch — larger than any baked
/// texture (OpenSim caps a bake at 6 MB), so one range request pulls it whole.
const MAX_BAKE_BYTES: usize = 8 * 1024 * 1024;

/// The JPEG-2000 codestream start-of-codestream (`SOC`) marker: every raw `.j2c`
/// begins `FF 4F`, so a fetched bake that starts otherwise is not a codestream.
const J2C_SOC_MARKER: [u8; 2] = [0xFF, 0x4F];

/// Uploads a baked avatar texture over the `UploadBakedTexture` capability.
#[derive(Debug)]
pub struct BakedTextureUpload;

impl GridTest for BakedTextureUpload {
    fn name(&self) -> &'static str {
        "baked-texture-upload"
    }

    fn description(&self) -> &'static str {
        "Upload a baked avatar texture over the UploadBakedTexture CAPS uploader"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let texture_id = fixtures::plywood_texture()?;

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // The two-step CAPS uploader the legacy appearance path uses; without
            // it the bake cannot be uploaded (there is no UDP fallback).
            if session.cap("UploadBakedTexture").is_none() {
                ctx.mark_partial("no UploadBakedTexture capability offered");
                return Ok(());
            }

            // A real J2C to bake: fetch the plywood codestream over `GetTexture`
            // (present on both grids) and re-upload it, so the payload is a valid
            // codestream on Second Life without a client-side encoder.
            let cap = session
                .cap("GetTexture")
                .ok_or_else(|| TestFailure::Assertion("no GetTexture capability".to_owned()))?;
            let fetcher = ReqwestTextureFetcher::with_default_client();
            fetcher.set_cap_url(Some(cap));
            let chunk = fetcher
                .fetch_range(texture_id, 0, MAX_BAKE_BYTES)
                .await
                .map_err(|error| {
                    TestFailure::Assertion(format!("fetch plywood codestream: {error}"))
                })?;
            let bake = chunk.bytes;
            check(!bake.is_empty(), "fetched an empty codestream to bake")?;
            check(
                bake.starts_with(&J2C_SOC_MARKER),
                "fetched bytes are not a JPEG-2000 codestream (no SOC marker)",
            )?;
            let byte_len = bake.len();

            let start = Instant::now();
            session
                .send(Command::UploadBakedTexture {
                    data: bake.to_vec(),
                })
                .await?;

            // The two-step uploader completes as `AssetUploaded` (stored temporary
            // asset, no inventory item) or `AssetUploadFailed` (a grid error).
            let outcome = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::AssetUploaded {
                        new_asset,
                        new_inventory_item,
                    } => Some(Ok((*new_asset, *new_inventory_item))),
                    Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
                    _other => None,
                })
                .await?;
            let upload_secs = start.elapsed().as_secs_f64();

            let (new_asset, new_item) = match outcome {
                Ok(pair) => pair,
                Err(reason) => {
                    // A grid that declines the bake is a grid-behaviour difference,
                    // not a client fault (the request was formed and POSTed
                    // correctly), so record it partial with the server's reason, as
                    // `asset-upload` does for aditi.
                    if is_aditi(grid) {
                        ctx.mark_partial(&format!(
                            "grid declined the baked-texture upload — {reason}"
                        ));
                        return Ok(());
                    }
                    return Err(TestFailure::Assertion(format!(
                        "UploadBakedTexture upload failed: {reason}"
                    )));
                }
            };
            check(!new_asset.is_nil(), "upload stored a nil asset id")?;
            // A baked texture is a temporary asset with no inventory item; the
            // completion must not carry a (non-nil) item id.
            check(
                new_item.is_none(),
                "baked-texture upload unexpectedly created an inventory item",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing("upload_secs", upload_secs);
            metrics.set("bake_bytes", i64::try_from(byte_len).unwrap_or(-1));
            metrics.set("new_asset", new_asset.to_string());

            Ok(())
        })
    }
}
