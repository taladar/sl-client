//! Download the region's RAW heightmap over the legacy `Xfer` path.
//!
//! The "Download RAW…" button on a viewer's Region/Estate → Terrain panel drives
//! `EstateOwnerMessage`/`terrain` with the `["download filename", <name>]`
//! parameters ([`Command::RequestRegionTerrainDownload`]). The simulator
//! serialises the region heightmap to an LL **RAW** file, then offers it back
//! over `Xfer`: it first sends an `InitiateDownload` naming the server-side file,
//! which the session follows automatically (mirroring the reference viewer's
//! `process_initiate_download`), streaming the bytes back one `SendXferPacket` at
//! a time. The assembled file surfaces as [`Event::ServerFileDownloaded`].
//!
//! This is the case that pins the `Xfer` **download** direction for a
//! non-mute-list, non-task-inventory consumer. The command is
//! region-owner/god gated (Firestorm only enables the button for
//! `owner_or_god`), so it runs as the **estate-owner** avatar
//! (`--avatar estate-owner`), who owns the local Default Region — a non-owner
//! gets no reply at all. There is no capability for this on either grid, so it
//! always rides `Xfer`.
//!
//! `[opensim]` only: we own no region on Second Life (same constraint as the
//! other estate-owner cases). The estate-owner avatar is forced to the Default
//! Region so it is within the region whose terrain it owns.
//!
//! The RAW file is asserted to be a plausible LL heightmap: non-empty, an exact
//! multiple of the 13-byte-per-point LL RAW stride, and a perfect square of at
//! least a 256 m region's `256 × 256` points. The derived edge length and byte
//! size are recorded as metrics.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, is_opensim, secs_metric};

/// The bytes-per-terrain-point stride of the LL RAW heightmap format (RGB + 10
/// alpha/derived channels). OpenSim's `LLRAW` writer emits exactly this many
/// bytes per point, so a valid file is a whole multiple of it.
const LL_RAW_STRIDE: usize = 13;

/// The edge length in points of the smallest region whose terrain we download (a
/// classic `256 × 256` region). A megaregion may report a larger square; a
/// smaller one would be malformed.
const MIN_EDGE: usize = 256;

/// The `start` location forcing the estate-owner avatar to the OpenSim Default
/// Region centre, so it owns and is within the region whose terrain it requests.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The viewer-side filename the download is labelled with (the reference viewer's
/// save-dialog default); echoed back on the completion event, otherwise opaque.
const VIEWER_FILENAME: &str = "terrain.raw";

/// How long to wait for the whole RAW file to arrive. A `256 × 256` region's LL
/// RAW heightmap is ~832 KB, and `Xfer` is reliable and strictly
/// one-packet-at-a-time (the next ~1 KB chunk only ships once the previous is
/// confirmed), so a full download runs to a couple of minutes — far longer than
/// the ordinary reply timeout.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(240);

/// Downloads the region's RAW heightmap over `Xfer` and validates its shape.
#[derive(Debug)]
pub struct TerrainRawDownload;

impl GridTest for TerrainRawDownload {
    fn name(&self) -> &'static str {
        "terrain-raw-transfer-download"
    }

    fn description(&self) -> &'static str {
        "Download the region's RAW heightmap over the Xfer path"
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

            // Ask the simulator to serialise and send the region terrain RAW.
            // OpenSim only answers a region owner/god, so a reply also confirms
            // our estate-owner rights.
            let start = Instant::now();
            session
                .send(Command::RequestRegionTerrainDownload {
                    viewer_filename: VIEWER_FILENAME.to_owned(),
                })
                .await?;

            // The bytes arrive over `Xfer` after an `InitiateDownload` the
            // session follows automatically.
            let (viewer_filename, data) = session
                .wait_for(DOWNLOAD_TIMEOUT, |event| match event {
                    Event::ServerFileDownloaded {
                        viewer_filename,
                        data,
                    } => Some((viewer_filename.clone(), data.clone())),
                    _ => None,
                })
                .await?;
            let elapsed = start.elapsed().as_secs_f64();

            check(
                !data.is_empty(),
                "expected a non-empty RAW terrain file to arrive",
            )?;
            check(
                data.len() % LL_RAW_STRIDE == 0,
                &format!(
                    "expected the RAW file length ({}) to be a whole multiple of the \
                     {LL_RAW_STRIDE}-byte LL RAW point stride",
                    data.len()
                ),
            )?;
            let points = data.len() / LL_RAW_STRIDE;
            let edge = points.isqrt();
            check(
                edge.checked_mul(edge) == Some(points),
                &format!(
                    "expected the RAW file to hold a square heightmap; got {points} points \
                     ({} bytes), not a perfect square",
                    data.len()
                ),
            )?;
            check(
                edge >= MIN_EDGE,
                &format!(
                    "expected at least a {MIN_EDGE}x{MIN_EDGE} region heightmap; got {edge}x{edge}"
                ),
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("terrain_raw_download"), elapsed);
            metrics.set("raw_bytes", i64::try_from(data.len()).unwrap_or(-1));
            metrics.set("terrain_edge", i64::try_from(edge).unwrap_or(-1));
            metrics.set("viewer_filename", viewer_filename);
            Ok(())
        })
    }
}
