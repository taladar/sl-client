//! Request a parcel's full properties and record its geometry and limits.
//!
//! The viewer reads a parcel's rich data (area, prim limits, flags, media,
//! landing point, …) by sending a UDP `ParcelPropertiesRequest` for a
//! region-local metre rectangle ([`Command::RequestParcelProperties`]). The
//! *reply*, `ParcelProperties`, does **not** come back over UDP on a modern
//! region: OpenSim (whenever the region has an event queue, its default) and
//! Second Life both enqueue it on the **CAPS EventQueue**, from where the
//! runtime's event-queue task decodes it into
//! [`Event::ParcelProperties`]. The UDP request is only
//! the *trigger*; the UDP `ParcelProperties` message is deprecated. So this case
//! also exercises the CAPS decode path (`parcel_info_from_llsd`), not just a
//! plain UDP round-trip.
//!
//! The flow is a single request/reply:
//!
//! 1. Wait for the region to become active.
//! 2. Send `ParcelPropertiesRequest` for a 4×4 m square at the region centre,
//!    tagged with a distinctive sequence id.
//! 3. Await the `ParcelProperties` event whose echoed sequence id matches, and
//!    assert it carries real data (not [`ParcelRequestResult::NoData`]) with a
//!    positive area.
//!
//! `1av`, `[both]`. The query rectangle is region-relative and independent of
//! the avatar's exact position, so no fixed start location is needed — the reply
//! describes whichever parcel occupies the region centre of the avatar's current
//! region (on OpenSim's Default Region that is the single region-wide parcel).

use sl_client_tokio::{Command, Event, ParcelInfo, ParcelRequestResult};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, secs_metric};

/// The western/southern edge of the queried square, in region metres — a 4×4 m
/// square centred on the region centre (128, 128), so the reply describes the
/// parcel at the middle of the region.
const SQUARE_WEST_SOUTH: f32 = 124.0;

/// The eastern/northern edge of the queried square, in region metres (see
/// [`SQUARE_WEST_SOUTH`]).
const SQUARE_EAST_NORTH: f32 = 128.0;

/// A distinctive sequence id, echoed back in the reply so the awaited
/// `ParcelProperties` is *our* query's answer and not an unsolicited one the
/// simulator sends on region entry.
const SEQUENCE_ID: i32 = 5150;

/// Requests parcel properties for the region-centre square and records the
/// parcel's geometry and prim limits.
#[derive(Debug)]
pub struct ParcelProperties;

impl GridTest for ParcelProperties {
    fn name(&self) -> &'static str {
        "parcel-properties"
    }

    fn description(&self) -> &'static str {
        "Request a parcel's properties (over the CAPS event queue)"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let start = std::time::Instant::now();
            session
                .send(Command::RequestParcelProperties {
                    west: SQUARE_WEST_SOUTH,
                    south: SQUARE_WEST_SOUTH,
                    east: SQUARE_EAST_NORTH,
                    north: SQUARE_EAST_NORTH,
                    sequence_id: SEQUENCE_ID,
                })
                .await?;
            let parcel: ParcelInfo = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::ParcelProperties(parcel) if parcel.sequence_id == SEQUENCE_ID => {
                        Some((**parcel).clone())
                    }
                    _ => None,
                })
                .await?;
            let elapsed = start.elapsed().as_secs_f64();

            check(
                parcel.request_result.has_data(),
                &format!(
                    "parcel query returned no data (request_result: {:?})",
                    parcel.request_result
                ),
            )?;
            check(
                parcel.area.0 > 0,
                &format!("parcel area was not positive (area: {})", parcel.area.0),
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("parcel_properties"), elapsed);
            metrics.set("area", i64::from(parcel.area.0));
            metrics.set("max_prims", i64::from(parcel.max_prims));
            metrics.set("sim_wide_max_prims", i64::from(parcel.sim_wide_max_prims));
            metrics.set("local_id", parcel.local_id.to_string());
            metrics.set(
                "request_result",
                request_result_label(parcel.request_result),
            );
            metrics.set("parcel_name", parcel.name);
            Ok(())
        })
    }
}

/// A stable label for a [`ParcelRequestResult`], recorded as a metric.
fn request_result_label(result: ParcelRequestResult) -> String {
    match result {
        ParcelRequestResult::NoData => "no-data".to_owned(),
        ParcelRequestResult::Single => "single".to_owned(),
        ParcelRequestResult::Multiple => "multiple".to_owned(),
        ParcelRequestResult::Unknown(code) => format!("unknown({code})"),
        _ => "unrecognised".to_owned(),
    }
}
