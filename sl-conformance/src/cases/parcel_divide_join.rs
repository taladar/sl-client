//! Subdivide a parcel into two, then join the pieces back into one.
//!
//! A land owner reshapes a region's parcels with two complementary UDP messages:
//! `ParcelDivide` ([`Command::DivideParcel`]) chops a metre rectangle — which must
//! be a strict subsection of exactly one parcel — out into a brand-new parcel; and
//! `ParcelJoin` ([`Command::JoinParcels`]) merges every owned parcel within a metre
//! rectangle back into one (the largest becomes the survivor, the rest are
//! absorbed). Both take a region-local `west/south/east/north` rectangle and need
//! land-divide/join rights, so this case runs as the **estate-owner** avatar
//! (`--avatar estate-owner`), who owns the region-wide parcel on the local grid.
//!
//! Neither message has a direct reply: the confirmation is the simulator's
//! reshaped parcel layout, which the case reads back with `ParcelPropertiesRequest`
//! queries (as in [`parcel_properties`](super::parcel_properties)). The flow is a
//! divide-verify-join-verify cycle that leaves the region with exactly the single
//! parcel it started with:
//!
//! 1. Wait for the region, and learn the region-centre parcel's region-local id,
//!    owner, and total area `A0` from a `ParcelPropertiesRequest` reply. Confirm we
//!    own it (divide/join need land rights).
//! 2. Divide the south-west corner rectangle out into a new parcel. Re-query:
//!    - a point inside the chopped corner now resolves to a **new** parcel (a
//!      different region-local id) whose area is the corner's area; and
//!    - the region centre still resolves to the **original** parcel id, now with a
//!      reduced area — and the two areas sum back to `A0`.
//! 3. Join the whole region back into one parcel. Re-query:
//!    - the region centre resolves to the original parcel id again, with area `A0`
//!      fully restored; and
//!    - the chopped corner now resolves to that **same** parcel id — the region is
//!      a single parcel once more.
//!
//! `1av`, `[both]`. On OpenSim's Default Region the single region-wide parcel
//! (area 65536 m²) is owned by the estate owner; the 64×64 m corner divides out as
//! a 4096 m² parcel leaving 61440 m², and the join restores the full 65536 m² under
//! the original local id. Second Life enforces the same message flow. The aditi run
//! is deferred with the batch.

use std::time::Duration;

use sl_client_tokio::{Command, Event, ParcelInfo};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, check_eq};

/// How long to let the simulator apply a `ParcelDivide` / `ParcelJoin` edit before
/// reading the reshaped layout back. Neither message has a reply, so we settle
/// briefly rather than racing the readback against the edit.
const EDIT_SETTLE: Duration = Duration::from_secs(2);

/// A region-local metre rectangle `(west, south, east, north)`, used both for the
/// divide/join edits and (as a small centred query square) for reading a parcel
/// back.
#[derive(Clone, Copy)]
struct Rect {
    /// The western edge, in region metres.
    west: f32,
    /// The southern edge, in region metres.
    south: f32,
    /// The eastern edge, in region metres.
    east: f32,
    /// The northern edge, in region metres.
    north: f32,
}

/// The corner rectangle chopped out by the divide: the south-west 64×64 m square,
/// well clear of the region centre so the region-centre parcel remains the larger
/// (surviving) piece. Its area (4096 m²) is a strict subsection of the region-wide
/// parcel.
const CHOP: Rect = Rect {
    west: 0.0,
    south: 0.0,
    east: 64.0,
    north: 64.0,
};

/// A 4×4 m query square inside the [`CHOP`] corner, used to read back the parcel
/// that owns that corner after the divide (and after the join).
const CORNER_SQUARE: Rect = Rect {
    west: 28.0,
    south: 28.0,
    east: 32.0,
    north: 32.0,
};

/// A 4×4 m query square centred on the region centre (128, 128), used to read back
/// the region-centre parcel — the piece that survives the divide.
const CENTRE_SQUARE: Rect = Rect {
    west: 124.0,
    south: 124.0,
    east: 128.0,
    north: 128.0,
};

/// The whole-region rectangle used for the join (the divide's two pieces both lie
/// within it).
const WHOLE_REGION: Rect = Rect {
    west: 0.0,
    south: 0.0,
    east: 256.0,
    north: 256.0,
};

/// The expected area of the chopped-out corner parcel, in square metres
/// (64 × 64 = 4096). A parcel bitmap is exact, so the new parcel covers precisely
/// the [`CHOP`] rectangle.
const CHOP_AREA: u32 = 4096;

/// Distinctive `ParcelProperties` sequence ids, echoed back so each awaited reply
/// is the answer to *that* query and not an unsolicited on-edit one. Distinct from
/// the other Phase 10 cases' ids so the cases never alias.
const SEQ_INITIAL: i32 = 5154;
/// Sequence id for the post-divide read of the chopped corner (see [`SEQ_INITIAL`]).
const SEQ_CHOPPED: i32 = 5155;
/// Sequence id for the post-divide read of the region-centre remainder.
const SEQ_REMAINDER: i32 = 5156;
/// Sequence id for the post-join read of the region centre.
const SEQ_JOINED_CENTRE: i32 = 5157;
/// Sequence id for the post-join read of the (formerly chopped) corner.
const SEQ_JOINED_CORNER: i32 = 5158;

/// Subdivides a parcel then joins the pieces back, leaving the region as found.
#[derive(Debug)]
pub struct ParcelDivideJoin;

impl GridTest for ParcelDivideJoin {
    fn name(&self) -> &'static str {
        "parcel-divide-join"
    }

    fn description(&self) -> &'static str {
        "Subdivide a parcel into two, then join the pieces back into one"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

            // 0. Defensively join the whole region into a single parcel first, so
            //    the divide/join cycle starts from a known single-parcel baseline
            //    (this also heals any parcels a prior interrupted run left behind).
            //    A join of an already-single parcel is a no-op on the simulator.
            session
                .send(Command::JoinParcels {
                    west: WHOLE_REGION.west,
                    south: WHOLE_REGION.south,
                    east: WHOLE_REGION.east,
                    north: WHOLE_REGION.north,
                })
                .await?;
            tokio::time::sleep(EDIT_SETTLE).await;

            // 1. Learn the region-centre parcel's local id, owner, and total area.
            //    Confirm we own it (divide/join need land-divide/join rights).
            let initial = query_parcel(session, CENTRE_SQUARE, SEQ_INITIAL).await?;
            let original_id = initial.local_id;
            let a0 = initial.area.0;
            check_eq(
                "parcel owner is the logged-in (estate-owner) avatar",
                &initial.owner.uuid(),
                &agent.uuid(),
            )?;
            check(
                a0 > CHOP_AREA,
                &format!(
                    "region-centre parcel area {a0} m² is not larger than the {CHOP_AREA} m² chop; \
                 the region is not a single divisible parcel"
                ),
            )?;

            // 2. Divide the corner out into a new parcel, then read both pieces.
            session
                .send(Command::DivideParcel {
                    west: CHOP.west,
                    south: CHOP.south,
                    east: CHOP.east,
                    north: CHOP.north,
                })
                .await?;
            tokio::time::sleep(EDIT_SETTLE).await;
            let chopped = query_parcel(session, CORNER_SQUARE, SEQ_CHOPPED).await?;
            let remainder = query_parcel(session, CENTRE_SQUARE, SEQ_REMAINDER).await?;

            check(
                chopped.local_id != original_id,
                &format!(
                    "divide did not create a new parcel: the chopped corner still reports the \
                     original local id {}",
                    original_id.0
                ),
            )?;
            check_eq(
                "region-centre remainder keeps the original parcel local id",
                &remainder.local_id,
                &original_id,
            )?;
            check_eq(
                "chopped-out corner parcel covers exactly the divided rectangle",
                &chopped.area.0,
                &CHOP_AREA,
            )?;
            let combined_area = chopped
                .area
                .0
                .checked_add(remainder.area.0)
                .ok_or_else(|| {
                    TestFailure::Assertion("chopped + remainder area overflowed u32".to_owned())
                })?;
            check_eq(
                "chopped + remainder areas sum back to the original parcel area",
                &combined_area,
                &a0,
            )?;

            // 3. Join the whole region back into one parcel, then read it back.
            session
                .send(Command::JoinParcels {
                    west: WHOLE_REGION.west,
                    south: WHOLE_REGION.south,
                    east: WHOLE_REGION.east,
                    north: WHOLE_REGION.north,
                })
                .await?;
            tokio::time::sleep(EDIT_SETTLE).await;
            let joined_centre = query_parcel(session, CENTRE_SQUARE, SEQ_JOINED_CENTRE).await?;
            let joined_corner = query_parcel(session, CORNER_SQUARE, SEQ_JOINED_CORNER).await?;

            check_eq(
                "joined parcel keeps the original region-centre local id",
                &joined_centre.local_id,
                &original_id,
            )?;
            check_eq(
                "joined parcel area is fully restored to the original",
                &joined_centre.area.0,
                &a0,
            )?;
            check_eq(
                "the formerly chopped corner is now part of the same single parcel",
                &joined_corner.local_id,
                &joined_centre.local_id,
            )?;

            let metrics = ctx.metrics();
            metrics.set("original_local_id", i64::from(original_id.0));
            metrics.set("owner_id", initial.owner.uuid().to_string());
            metrics.set("initial_area", i64::from(a0));
            metrics.set("chopped_local_id", i64::from(chopped.local_id.0));
            metrics.set("chopped_area", i64::from(chopped.area.0));
            metrics.set("remainder_area", i64::from(remainder.area.0));
            metrics.set("joined_area", i64::from(joined_centre.area.0));
            Ok(())
        })
    }
}

/// Sends a `ParcelPropertiesRequest` for the `square` metre rectangle with the
/// given echoed `sequence_id`, and returns the matching parcel's [`ParcelInfo`].
///
/// # Errors
///
/// Propagates the send / [`Session::wait_for`] failures, times out if no matching
/// [`Event::ParcelProperties`] arrives, or returns [`TestFailure::Assertion`] if
/// the reply carries no parcel data.
async fn query_parcel(
    session: &mut Session,
    square: Rect,
    sequence_id: i32,
) -> Result<ParcelInfo, TestFailure> {
    session
        .send(Command::RequestParcelProperties {
            west: square.west,
            south: square.south,
            east: square.east,
            north: square.north,
            sequence_id,
        })
        .await?;
    let parcel: ParcelInfo = session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ParcelProperties(parcel) if parcel.sequence_id == sequence_id => {
                Some((**parcel).clone())
            }
            _ => None,
        })
        .await?;
    check(
        parcel.request_result.has_data(),
        &format!(
            "parcel query (seq {sequence_id}) returned no data (request_result: {:?})",
            parcel.request_result
        ),
    )?;
    Ok(parcel)
}
