//! Upload a RAW heightmap to the region over the legacy `Xfer` path.
//!
//! The "Upload RAW…" button on a viewer's Region/Estate → Terrain panel drives
//! `EstateOwnerMessage`/`terrain` with the `["upload filename", <name>]`
//! parameters ([`Command::RequestRegionTerrainUpload`]). The simulator answers
//! with a `RequestXfer` naming that file and picking the transfer id; the
//! session follows it to stream the RAW heightmap up one `SendXferPacket` at a
//! time, each released by the previous packet's `ConfirmXferPacket` (mirroring
//! the reference viewer's `LLXferManager`). When the simulator confirms the
//! final packet the upload surfaces as [`Event::XferUploaded`]; the simulator
//! then loads the heightmap and re-broadcasts the changed terrain as
//! [`Event::TerrainPatch`]es.
//!
//! This is the case that pins the `Xfer` **upload** direction — its only
//! consumer once the legacy UDP asset upload is dropped. The command is
//! region-owner/god gated (Firestorm only enables the button for `owner_or_god`),
//! so it runs as the **estate-owner** avatar (`--avatar estate-owner`), who owns
//! the local Default Region — a non-owner gets no `RequestXfer` at all. There is
//! no capability for this on either grid, so it always rides `Xfer`.
//!
//! `[opensim]` only: we own no region on Second Life (same constraint as the
//! other estate-owner cases). The estate-owner avatar is forced to the Default
//! Region so it is within the region whose terrain it owns.
//!
//! The flow leaves the region as found:
//!
//! 1. Download the current region RAW (the [`terrain-raw-transfer-download`]
//!    path) — a valid, region-sized heightmap to start from.
//! 2. Ask the simulator to stream terrain and drain the login/download terrain
//!    flood, so a later patch is a genuine post-upload re-broadcast.
//! 3. Build a modified copy that raises a band of points to a distinctive height
//!    and upload it. OpenSim only re-broadcasts patches whose height actually
//!    changed (`TerrainData` taints a patch only when a cell's value differs), so
//!    the modification guarantees a detectable change; assert the upload
//!    completes ([`Event::XferUploaded`]) and that a land terrain patch
//!    re-broadcasts.
//! 4. Re-upload the original RAW to restore the region to its downloaded
//!    baseline, leaving it clean.
//!
//! [`terrain-raw-transfer-download`]: super::terrain_raw_download

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, Throttle};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, is_opensim, secs_metric};

/// The bytes-per-terrain-point stride of the LL RAW heightmap format. OpenSim's
/// `LLRAW` writer emits exactly this many bytes per point (the first two are the
/// height index; the rest is padding the loader skips), so a valid file is a
/// whole multiple of it.
const LL_RAW_STRIDE: usize = 13;

/// The edge length in points of the smallest region whose terrain we handle (a
/// classic `256 × 256` region). A megaregion may report a larger square; a
/// smaller one would be malformed.
const MIN_EDGE: usize = 256;

/// The `start` location forcing the estate-owner avatar to the OpenSim Default
/// Region centre, so it owns and is within the region whose terrain it uploads.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The viewer-side filename the transfer is labelled with (the reference viewer's
/// picked-file path); echoed back by the simulator's `RequestXfer`, otherwise
/// opaque.
const VIEWER_FILENAME: &str = "terrain.raw";

/// How long to wait for a whole RAW file to stream in either direction. A
/// `256 × 256` region's LL RAW heightmap is ~832 KB, and `Xfer` is reliable and
/// strictly one-packet-at-a-time (the next ~1 KB chunk only ships once the
/// previous is confirmed), so a full transfer runs to a minute or two.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(240);

/// How long to wait for a terrain patch to re-broadcast after the upload is
/// applied.
const TERRAIN_TIMEOUT: Duration = Duration::from_secs(30);

/// The quiet gap (no terrain patch) that marks the login/download terrain flood
/// drained, so a later patch is a genuine post-upload re-broadcast.
const DRAIN_QUIET: Duration = Duration::from_secs(3);

/// How many leading rows of the heightmap to raise. Sixteen rows span a full band
/// of `16 × 16` patches, so at least one land patch is guaranteed to change and
/// re-broadcast regardless of the region's current relief.
const MODIFIED_ROWS: usize = 16;

/// The `red` (low index) byte written into each modified point. With
/// [`MODIFIED_GREEN`] it encodes a height of `173 × (131 / 128) ≈ 177 m` — far
/// above the local Default Region's relief, so every modified point's height
/// changes and taints its patch. See OpenSim `LLRAW`: `height = red × green/128`.
const MODIFIED_RED: u8 = 173;

/// The `green` (high index / scale) byte written into each modified point (see
/// [`MODIFIED_RED`]).
const MODIFIED_GREEN: u8 = 131;

/// Uploads a RAW heightmap over `Xfer`, asserts the terrain changes, then
/// restores the region.
#[derive(Debug)]
pub struct TerrainRawUpload;

impl GridTest for TerrainRawUpload {
    fn name(&self) -> &'static str {
        "terrain-raw-transfer-upload"
    }

    fn description(&self) -> &'static str {
        "Upload a RAW heightmap to the region over the Xfer path"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // Ask the simulator to stream terrain so the post-upload patch
            // re-broadcast reaches us.
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // 1. Download the current region RAW: a valid, region-sized heightmap
            //    to upload back (and to restore from). OpenSim only answers a
            //    region owner/god, so a reply also confirms our estate rights.
            session
                .send(Command::RequestRegionTerrainDownload {
                    viewer_filename: VIEWER_FILENAME.to_owned(),
                })
                .await?;
            let original = session
                .wait_for(TRANSFER_TIMEOUT, |event| match event {
                    Event::ServerFileDownloaded { data, .. } => Some(data.clone()),
                    _ => None,
                })
                .await?;

            check(
                !original.is_empty() && original.len() % LL_RAW_STRIDE == 0,
                &format!(
                    "expected a non-empty RAW file whose length ({}) is a whole multiple \
                     of the {LL_RAW_STRIDE}-byte LL RAW point stride",
                    original.len()
                ),
            )?;
            let points = original.len() / LL_RAW_STRIDE;
            let edge = points.isqrt();
            check(
                edge.checked_mul(edge) == Some(points) && edge >= MIN_EDGE,
                &format!(
                    "expected at least a {MIN_EDGE}×{MIN_EDGE} square heightmap; got {points} \
                     points ({} bytes)",
                    original.len()
                ),
            )?;

            // 2. Drain the login/download terrain flood so the next patch we see
            //    is genuinely post-upload.
            drain_terrain(session, DRAIN_QUIET).await?;

            // 3. Raise a band of points to a distinctive height and upload the
            //    result. The changed patches taint server-side and re-broadcast.
            let modified = raise_band(&original, edge);
            let start = Instant::now();
            session
                .send(Command::RequestRegionTerrainUpload {
                    viewer_filename: VIEWER_FILENAME.to_owned(),
                    data: modified.clone(),
                })
                .await?;
            let uploaded_bytes = wait_upload(session, VIEWER_FILENAME).await?;
            let elapsed = start.elapsed().as_secs_f64();
            check(
                uploaded_bytes == modified.len(),
                &format!(
                    "expected the upload to report {} bytes, got {uploaded_bytes}",
                    modified.len()
                ),
            )?;

            // The simulator loads the heightmap and re-broadcasts the changed
            // terrain — the confirmation that the upload actually took effect.
            wait_land_patch(session, TERRAIN_TIMEOUT).await?;

            // 4. Restore the region to its downloaded baseline.
            session
                .send(Command::RequestRegionTerrainUpload {
                    viewer_filename: VIEWER_FILENAME.to_owned(),
                    data: original.clone(),
                })
                .await?;
            let restored_bytes = wait_upload(session, VIEWER_FILENAME).await?;
            check(
                restored_bytes == original.len(),
                &format!(
                    "expected the restore upload to report {} bytes, got {restored_bytes}",
                    original.len()
                ),
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("terrain_raw_upload"), elapsed);
            metrics.set("raw_bytes", i64::try_from(modified.len()).unwrap_or(-1));
            metrics.set("terrain_edge", i64::try_from(edge).unwrap_or(-1));
            Ok(())
        })
    }
}

/// Returns a copy of `raw` with the first [`MODIFIED_ROWS`] rows of points raised
/// to a distinctive height, so uploading it is guaranteed to change (and taint)
/// at least one terrain patch. Only the two height-index bytes of each point are
/// rewritten; the loader ignores the rest.
fn raise_band(raw: &[u8], edge: usize) -> Vec<u8> {
    let mut modified = raw.to_vec();
    let band = edge.saturating_mul(MODIFIED_ROWS);
    for point in modified.chunks_mut(LL_RAW_STRIDE).take(band) {
        if let [red, green, ..] = point {
            *red = MODIFIED_RED;
            *green = MODIFIED_GREEN;
        }
    }
    modified
}

/// Awaits the completion of the outbound `Xfer` upload named `filename`,
/// returning the byte count the simulator confirmed.
///
/// # Errors
///
/// Returns [`TestFailure::Timeout`] if no completion arrives within
/// [`TRANSFER_TIMEOUT`], or propagates a [`Session::wait_for`] disconnect.
async fn wait_upload(session: &mut Session, filename: &str) -> Result<usize, TestFailure> {
    session
        .wait_for(TRANSFER_TIMEOUT, |event| match event {
            Event::XferUploaded {
                viewer_filename,
                byte_count,
                ..
            } if viewer_filename == filename => Some(*byte_count),
            _ => None,
        })
        .await
}

/// Awaits the next re-broadcast of any land terrain patch, confirming the region
/// terrain changed after the upload was applied.
///
/// # Errors
///
/// Returns [`TestFailure::Timeout`] if no land patch arrives within `timeout`, or
/// propagates a [`Session::wait_for`] disconnect.
async fn wait_land_patch(session: &mut Session, timeout: Duration) -> Result<(), TestFailure> {
    session
        .wait_for(timeout, |event| match event {
            Event::TerrainPatch(patch) if patch.layer.is_land() => Some(()),
            _ => None,
        })
        .await
}

/// Drains queued terrain patches until none arrives for `quiet`, so a later
/// [`wait_land_patch`] sees only genuinely post-upload re-broadcasts.
///
/// # Errors
///
/// Propagates a [`Session::wait_for`] disconnect.
async fn drain_terrain(session: &mut Session, quiet: Duration) -> Result<(), TestFailure> {
    loop {
        match session
            .wait_for(quiet, |event| {
                matches!(event, Event::TerrainPatch(_)).then_some(())
            })
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(()),
            Err(other) => return Err(other),
        }
    }
}
