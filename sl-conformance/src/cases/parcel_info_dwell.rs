//! Request a parcel's dwell (traffic) value and its grid-wide info listing.
//!
//! Two distinct request/reply pairs answer "tell me about this parcel", and this
//! case exercises both against the parcel at the region centre:
//!
//! 1. **Dwell** — a UDP `ParcelDwellRequest` for the parcel's *region-local* id
//!    ([`Command::RequestParcelDwell`]) answered by a UDP `ParcelDwellReply`
//!    ([`Event::ParcelDwell`]). The request takes a [`ScopedParcelId`]: the
//!    region-local id learned from a `ParcelProperties` reply, paired with the
//!    root circuit's identity (so a stale id from a previous circuit fails to
//!    resolve rather than hitting the wrong parcel).
//! 2. **Info listing** — the condensed "places/search" record a viewer shows for
//!    a parcel *id*. The id is grid-wide, not region-local, so it is first
//!    resolved from the region-centre location through the `RemoteParcelRequest`
//!    **capability** ([`Command::RequestRemoteParcelId`] →
//!    [`Event::RemoteParcelId`]); that id then feeds a UDP `ParcelInfoRequest`
//!    ([`Command::RequestParcelInfo`]) answered by a `ParcelInfoReply`
//!    ([`Event::ParcelDetails`]). The listing carries its own dwell field, so the
//!    case records both the dedicated dwell reply and the info reply's dwell.
//!
//! The flow:
//!
//! 1. Wait for the region to become active.
//! 2. Send a `ParcelPropertiesRequest` for a square at the region centre to learn
//!    the parcel's region-local id.
//! 3. Request the parcel's dwell by that region-local id (scoped to the root
//!    circuit) and await the matching `ParcelDwell`.
//! 4. Resolve the region-centre location to a grid-wide parcel id via the
//!    `RemoteParcelRequest` capability.
//! 5. Request that parcel's info listing and await the `ParcelDetails` whose
//!    echoed id matches.
//!
//! `1av`, `[both]`. On OpenSim's Default Region the single region-wide parcel
//! answers all three requests; the dwell is 0 on a fresh region (no accumulated
//! traffic) but the reply still arrives (the `DefaultDwellModule` is enabled by
//! default). Second Life tracks real dwell.

use std::time::Instant;

use sl_client_tokio::{Command, Event, ParcelInfo, RegionCoordinates, ScopedParcelId, Uuid};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, secs_metric};

/// The western/southern edge of the queried square, in region metres — a 4×4 m
/// square centred on the region centre (128, 128), so the reply describes the
/// parcel at the middle of the region.
const SQUARE_WEST_SOUTH: f32 = 124.0;

/// The eastern/northern edge of the queried square, in region metres (see
/// [`SQUARE_WEST_SOUTH`]).
const SQUARE_EAST_NORTH: f32 = 128.0;

/// The region-centre location whose parcel the `RemoteParcelRequest` resolves.
/// The z is irrelevant — the grid keys the lookup on the x/y column.
const REGION_CENTRE: f32 = 128.0;

/// A distinctive sequence id, echoed back in the `ParcelProperties` reply so the
/// awaited reply is *our* query's answer and not an unsolicited on-entry one.
/// Distinct from the `parcel-properties` case's id so the two never alias.
const SEQUENCE_ID: i32 = 5151;

/// Requests a parcel's dwell and its grid-wide info listing, recording both the
/// dwell value and the listing's geometry/owner.
#[derive(Debug)]
pub struct ParcelInfoDwell;

impl GridTest for ParcelInfoDwell {
    fn name(&self) -> &'static str {
        "parcel-info-dwell"
    }

    fn description(&self) -> &'static str {
        "Request a parcel's dwell and its grid-wide info listing"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let circuit = session.circuit_id().ok_or_else(|| {
                TestFailure::Assertion("login established no root circuit id".to_owned())
            })?;
            let region_handle = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("login reported no region handle".to_owned())
            })?;

            // 1. Learn the parcel's region-local id from a ParcelProperties reply
            //    (the dwell request is keyed on the region-local id, not the
            //    grid-wide one).
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
            check(
                parcel.request_result.has_data(),
                &format!(
                    "parcel query returned no data (request_result: {:?})",
                    parcel.request_result
                ),
            )?;
            let local_id = parcel.local_id;

            // 2. Request the parcel's dwell by its region-local id, scoped to the
            //    root circuit.
            let dwell_start = Instant::now();
            session
                .send(Command::RequestParcelDwell {
                    local_id: ScopedParcelId::new(circuit, local_id),
                })
                .await?;
            let (dwell_local_id, dwell_parcel_id, dwell) = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ParcelDwell {
                        local_id,
                        parcel_id,
                        dwell,
                    } => Some((*local_id, *parcel_id, *dwell)),
                    _ => None,
                })
                .await?;
            let dwell_elapsed = dwell_start.elapsed().as_secs_f64();
            check_eq("dwell reply local id", &dwell_local_id.id(), &local_id)?;

            // 3. Resolve the region-centre location to a grid-wide parcel id via
            //    the RemoteParcelRequest capability.
            let remote_start = Instant::now();
            session
                .send(Command::RequestRemoteParcelId {
                    location: RegionCoordinates::new(REGION_CENTRE, REGION_CENTRE, 0.0),
                    region_id: Uuid::nil(),
                    region_handle,
                })
                .await?;
            let parcel_key = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::RemoteParcelId(parcel_id) => Some(*parcel_id),
                    _ => None,
                })
                .await?;
            let remote_elapsed = remote_start.elapsed().as_secs_f64();
            check(
                !parcel_key.uuid().is_nil(),
                "RemoteParcelRequest resolved the region centre to a nil parcel id",
            )?;

            // 4. Request that parcel's info listing by its grid-wide id.
            let info_start = Instant::now();
            session
                .send(Command::RequestParcelInfo {
                    parcel_id: parcel_key,
                })
                .await?;
            let details = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::ParcelDetails(details) if details.parcel_id == parcel_key => {
                        Some(details.clone())
                    }
                    _ => None,
                })
                .await?;
            let info_elapsed = info_start.elapsed().as_secs_f64();
            check_eq("info listing parcel id", &details.parcel_id, &parcel_key)?;
            check(
                details.sim_name.is_some(),
                "parcel info listing carried no region name",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("parcel_dwell"), dwell_elapsed);
            metrics.set_timing(&secs_metric("remote_parcel_id"), remote_elapsed);
            metrics.set_timing(&secs_metric("parcel_info"), info_elapsed);
            metrics.set("local_id", i64::from(local_id.0));
            metrics.set("dwell", f64::from(dwell));
            metrics.set("dwell_parcel_id", dwell_parcel_id.to_string());
            metrics.set("parcel_id", parcel_key.to_string());
            metrics.set("info_dwell", f64::from(details.dwell));
            metrics.set("actual_area", i64::from(details.actual_area.0));
            metrics.set("billable_area", i64::from(details.billable_area.0));
            metrics.set("owner_id", details.owner_id.to_string());
            metrics.set("parcel_name", details.name);
            metrics.set(
                "region_name",
                details
                    .sim_name
                    .map(|name| name.to_string())
                    .unwrap_or_default(),
            );
            Ok(())
        })
    }
}
