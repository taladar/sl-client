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
//! The reply also carries what only the event-queue form has room for: the
//! `MediaData`, `MediaLinkSharing` and `ParcelExtendedFlags` blocks and the
//! avatar-visibility booleans. Which of them each grid sends is held to the
//! measurement (`BLOCKS`), because a client must send them back on an edit.
//!
//! `1av`, `[both, fake]`. The query rectangle is region-relative and
//! independent of the avatar's exact position, so no fixed start location is
//! needed — the reply describes whichever parcel occupies the region centre of
//! the avatar's current region (on OpenSim's Default Region and on the fake
//! grid's catalogue region alike, that is the single region-wide parcel).

use sl_client_tokio::{Command, Event, ParcelInfo, ParcelRequestResult};

use crate::context::TestContext;
use crate::grid::Grid;
use crate::measured::Measured;
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

/// Which of the event-queue-only parts the reply for a parcel without media
/// carries: `MediaData`, `MediaLinkSharing`, `ParcelExtendedFlags`, and the
/// avatar-visibility booleans.
const BLOCKS: Measured<[bool; 4]> = Measured {
    second_life: [true, false, true, true],
    opensim: [true, false, false, true],
    source: "parcel-properties on aditi and OpenSim (2026-10-05, book/src/gridspec/land.md)",
};

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
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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
                    snap_selection: false,
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

            BLOCKS.check(
                "event-queue-only parts (media, link sharing, extended flags, visibility)",
                ctx.grid(),
                &[
                    parcel.media_data.is_some(),
                    parcel.media_sharing.is_some(),
                    parcel.obscure_moap.is_some(),
                    parcel.see_avs.is_some(),
                ],
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
            // The blocks only the event-queue form carries, and what each grid
            // puts in them: a client must send them back on an edit, so which
            // a grid sends is what a `ParcelPropertiesUpdate` can preserve.
            metrics.set(
                "media_data",
                parcel
                    .media_data
                    .as_ref()
                    .map_or_else(|| "absent".to_owned(), |media| format!("{media:?}")),
            );
            metrics.set(
                "media_sharing",
                parcel
                    .media_sharing
                    .as_ref()
                    .map_or_else(|| "absent".to_owned(), |sharing| format!("{sharing:?}")),
            );
            metrics.set("obscure_moap", format!("{:?}", parcel.obscure_moap));
            metrics.set(
                "avatar_visibility",
                format!(
                    "see={:?} any_sounds={:?} group_sounds={:?}",
                    parcel.see_avs, parcel.any_av_sounds, parcel.group_av_sounds
                ),
            );
            // The whole decoded record, the bitmap aside (512 bytes of it): the
            // raw event, every key and its wire type, is in the trace log
            // (`sl_client_tokio::caps=trace`).
            let mut shown = parcel.clone();
            shown.bitmap = Vec::new();
            metrics.set("record", format!("{shown:?}"));
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
