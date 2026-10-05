//! Request a parcel's per-owner object tally, then return objects to their owner.
//!
//! A land owner's "Objects" land-panel has two halves this case exercises against
//! the region-centre parcel. The **primary** avatar is the land owner — the
//! **estate-owner** on the local grid (`--avatar estate-owner`), who owns the
//! region-wide parcel — because both halves need land rights. The **secondary**
//! avatar is the resident whose object is tallied and returned; on the local
//! grid that is a dedicated account that owns nothing anywhere (`--secondary
//! resident` in the credentials file), since every other test avatar has
//! long-lived objects on the region-centre parcel:
//!
//! - **Request object owners** — [`Command::RequestParcelObjectOwners`]
//!   (`ParcelObjectOwnersRequest`, keyed on a [`ScopedParcelId`]) asks the
//!   simulator for one row per avatar/group with objects sitting on the parcel;
//!   the reply arrives as [`Event::ParcelObjectOwners`], each row a
//!   [`ParcelObjectOwner`] carrying the owner, a prim count, and an online flag.
//!   This is the data behind the panel's "Returnable objects" owner list.
//! - **Return objects** — [`Command::ReturnParcelObjects`] (`ParcelReturnObjects`)
//!   returns every object on the parcel owned by the listed owners to their
//!   owner's inventory. [`ParcelReturnType::LIST`] scoped to one owner id mirrors
//!   the viewer's "Return objects owned by \<selected owner\>" button (the
//!   reference simulator `LandObject.ReturnLandObjects` matches `primsOverMe` by
//!   owner id).
//!
//! Why the object is the secondary's and not the land owner's: a return by owner
//! takes **everything** that owner has on the parcel, and a land owner's parcel is
//! where its scene lives — the local grid's estate owner has a few dozen
//! long-lived test objects on it. OpenSim has no narrower return: it matches
//! by owner only and ignores the task list. A resident with nothing on the
//! parcel gives a return that touches only the cube this case rezzes.
//!
//! 1. Both avatars wait for the region; the primary learns the region-centre
//!    parcel's region-local id and owner from a `ParcelPropertiesRequest` reply
//!    and confirms it owns it.
//! 2. The primary requests the object owners as a **baseline** and asserts the
//!    secondary owns nothing on the parcel yet.
//! 3. The secondary rezzes a throwaway cube ([`Command::RezObject`], `ObjectAdd`)
//!    at the region centre; its arrival is the first of its own
//!    [`Event::ObjectAdded`]s with an id not seen while the scene settled.
//! 4. The primary requests the object owners again and asserts the secondary now
//!    tallies one prim.
//! 5. The primary returns the secondary's objects on the parcel
//!    ([`Command::ReturnParcelObjects`], `ParcelReturnType::LIST` scoped to the
//!    secondary's id), confirmed by the secondary's [`Event::ObjectRemoved`]
//!    (`KillObject`) for the cube.
//! 6. The primary requests the object owners a final time and asserts the
//!    secondary is back to none, leaving the parcel as found (the cube is in the
//!    secondary's Lost and Found — inventory residue of one item per run, fine on
//!    a throwaway grid).
//!
//! `2av`, `[both]`. On OpenSim both avatars start at the "Default Region" centre
//! so the rez lands on the parcel. The aditi run is deferred with the batch — it
//! needs land the primary owns, like
//! [`parcel_divide_join`](super::parcel_divide_join).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Event, OwnerKey, ParcelInfo, ParcelObjectOwner, ParcelReturnType, PrimShape,
    ScopedObjectId, ScopedParcelId, Uuid, Vector,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, is_opensim, secs_metric,
    wait_for_own_new_object,
};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, so the
/// avatar is within streaming range of the parcel it edits and the rez lands on
/// owned land. On Second Life the avatar keeps `"last"`.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// Where the throwaway cube is rezzed: the region centre, a few metres above the
/// ground so it clears the terrain. Well inside the region-wide parcel, so the rez
/// permission check passes for its owner.
const REZ_POSITION: Vector = Vector {
    x: 128.0,
    y: 128.0,
    z: 30.0,
};

/// A 4×4 m query square centred on the region centre (128, 128), used to read back
/// the region-centre parcel's region-local id and owner.
const CENTRE_WEST_SOUTH: f32 = 124.0;
/// The eastern/northern edge of the region-centre query square (see
/// [`CENTRE_WEST_SOUTH`]).
const CENTRE_EAST_NORTH: f32 = 128.0;

/// The overall budget for settling the initial scene — draining the region's
/// object-update burst so a freshly rezzed object is recognised as new.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle: once no new [`Event::ObjectAdded`] has
/// arrived for this long the initial scene is considered fully streamed.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to let the simulator update its per-parcel object tally after a rez or
/// return before reading the object owners back. The tally is maintained as
/// objects enter/leave the parcel; a short settle avoids racing the readback
/// against the edit.
const TALLY_SETTLE: Duration = Duration::from_secs(2);

/// How long to wait for the rezzed object to appear / be removed.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// A distinctive `ParcelProperties` sequence id, echoed back so the awaited reply
/// is the answer to this query and not an unsolicited on-entry one. Distinct from
/// the other Phase 10 cases' ids so the cases never alias.
const SEQ_CENTRE: i32 = 5171;

/// Requests a parcel's per-owner object tally, then returns the objects.
#[derive(Debug)]
pub struct ParcelObjectOwners;

impl GridTest for ParcelObjectOwners {
    fn name(&self) -> &'static str {
        "parcel-object-owners"
    }

    fn description(&self) -> &'static str {
        "Request a parcel's per-owner object tally, then return objects"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
    }

    fn rezzes_objects(&self) -> bool {
        true
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
            let resident = {
                let secondary = ctx.secondary().ok_or_else(|| {
                    TestFailure::Assertion("this case needs a secondary avatar".to_owned())
                })?;
                secondary.wait_for_region(REGION_TIMEOUT).await?;
                secondary.agent_id().ok_or_else(|| {
                    TestFailure::Assertion("the secondary's login reported no agent id".to_owned())
                })?
            };
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            let circuit = session.circuit_id().ok_or_else(|| {
                TestFailure::Assertion("login established no root circuit id".to_owned())
            })?;

            // 1. Learn the region-centre parcel's local id and owner; confirm the
            //    primary owns it (the tally and the return need land rights).
            let parcel = query_parcel(session, SEQ_CENTRE).await?;
            let local_id = parcel.local_id;
            let scoped_parcel = ScopedParcelId::new(circuit, local_id);
            check_eq(
                "parcel owner is the logged-in (estate-owner) avatar",
                &parcel.owner.uuid(),
                &agent.uuid(),
            )?;

            // 2. Baseline: the return below takes everything the resident has on
            //    the parcel, so require that to be nothing.
            let baseline = request_object_owners(session, scoped_parcel).await?;
            let owner_before = owner_count(&baseline, resident.uuid());
            check(
                owner_before == 0,
                &format!(
                    "the resident ({resident:?}) starts with objects on the parcel: {baseline:?}"
                ),
            )?;

            // 3. The resident rezzes a throwaway cube (`ObjectAdd`) at the region
            //    centre, recognised as new against the settled scene.
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("this case needs a secondary avatar".to_owned())
            })?;
            let mut seen = settle_scene(secondary).await?;
            let rez_started = Instant::now();
            secondary
                .send(Command::RezObject {
                    shape: PrimShape::cube(REZ_POSITION),
                    group_id: None,
                })
                .await?;
            let created = wait_for_own_new_object(secondary, &seen, STEP_TIMEOUT)
                .await?
                .map_err(|reason| {
                    TestFailure::Assertion(format!(
                        "no new object appeared after RezObject (ObjectAdd): {reason}"
                    ))
                })?;
            let rez_rtt = rez_started.elapsed();
            let created_id = created.scoped_id();
            seen.insert(created_id);

            // 4. The tally: the resident now has one prim on the parcel.
            tokio::time::sleep(TALLY_SETTLE).await;
            let session = ctx.primary();
            let after_rez = request_object_owners(session, scoped_parcel).await?;
            let owner_after_rez = owner_count(&after_rez, resident.uuid());
            let expected_after_rez = owner_before.checked_add(1).ok_or_else(|| {
                TestFailure::Assertion("baseline owner count overflowed i32".to_owned())
            })?;
            check_eq(
                "the object-owners tally reflects the rezzed cube (one prim)",
                &owner_after_rez,
                &expected_after_rez,
            )?;

            // 5. The land owner returns the resident's objects on the parcel, and
            //    the resident watches its cube go.
            let return_started = Instant::now();
            session
                .send(Command::ReturnParcelObjects {
                    local_id: scoped_parcel,
                    return_type: ParcelReturnType::LIST,
                    owner_ids: vec![OwnerKey::Agent(resident)],
                    task_ids: Vec::new(),
                })
                .await?;
            let secondary = ctx.secondary().ok_or_else(|| {
                TestFailure::Assertion("this case needs a secondary avatar".to_owned())
            })?;
            let removed = secondary
                .wait_for(STEP_TIMEOUT, |event| match event {
                    Event::ObjectRemoved { local_id, .. } if *local_id == created_id => {
                        Some(*local_id)
                    }
                    _ => None,
                })
                .await?;
            let return_rtt = return_started.elapsed();
            check(
                removed == created_id,
                "the removed object id did not match the returned cube",
            )?;

            // 6. Final tally: the resident is back to none.
            tokio::time::sleep(TALLY_SETTLE).await;
            let session = ctx.primary();
            let after_return = request_object_owners(session, scoped_parcel).await?;
            let owner_after_return = owner_count(&after_return, resident.uuid());
            check_eq(
                "the resident has no objects on the parcel again after the return",
                &owner_after_return,
                &owner_before,
            )?;

            let metrics = ctx.metrics();
            metrics.set("parcel_local_id", i64::from(local_id.0));
            metrics.set("rezzed_object", created.full_id.to_string());
            metrics.set("owner_count_before", i64::from(owner_before));
            metrics.set("owner_count_after_rez", i64::from(owner_after_rez));
            metrics.set("owner_count_after_return", i64::from(owner_after_return));
            metrics.set_timing(&secs_metric("rez_rtt"), rez_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("return_rtt"), return_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// Sends a `ParcelPropertiesRequest` for the region-centre query square with the
/// echoed `sequence_id`, and returns the matching parcel's [`ParcelInfo`].
///
/// # Errors
///
/// Propagates the send / [`Session::wait_for`] failures, times out if no matching
/// [`Event::ParcelProperties`] arrives, or returns [`TestFailure::Assertion`] if
/// the reply carries no parcel data.
async fn query_parcel(session: &mut Session, sequence_id: i32) -> Result<ParcelInfo, TestFailure> {
    session
        .send(Command::RequestParcelProperties {
            west: CENTRE_WEST_SOUTH,
            south: CENTRE_WEST_SOUTH,
            east: CENTRE_EAST_NORTH,
            north: CENTRE_EAST_NORTH,
            sequence_id,
            snap_selection: false,
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

/// Requests the parcel's per-owner object tally and returns the reply's rows.
///
/// # Errors
///
/// Propagates the send / [`Session::wait_for`] failures, or times out if no
/// [`Event::ParcelObjectOwners`] arrives within [`REPLY_TIMEOUT`].
async fn request_object_owners(
    session: &mut Session,
    scoped_parcel: ScopedParcelId,
) -> Result<Vec<ParcelObjectOwner>, TestFailure> {
    session
        .send(Command::RequestParcelObjectOwners {
            local_id: scoped_parcel,
        })
        .await?;
    session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ParcelObjectOwners { owners, .. } => Some(owners.clone()),
            _ => None,
        })
        .await
}

/// The object count the tally reports for `owner_uuid`, or 0 if the owner has no
/// row (a `ParcelObjectOwnersReply` omits owners with no objects).
fn owner_count(owners: &[ParcelObjectOwner], owner_uuid: Uuid) -> i32 {
    owners
        .iter()
        .find(|owner| owner.owner.uuid() == owner_uuid)
        .map_or(0, |owner| owner.count)
}

/// Drains the region's initial object-update burst, returning the set of every
/// region-local id sighted. The drain ends once no new [`Event::ObjectAdded`] has
/// arrived for [`SETTLE_IDLE`], or the overall [`SETTLE_WINDOW`] elapses.
async fn settle_scene(session: &mut Session) -> Result<HashSet<ScopedObjectId>, TestFailure> {
    let mut seen = HashSet::new();
    let started = Instant::now();
    loop {
        let remaining = SETTLE_WINDOW.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let cap = remaining.min(SETTLE_IDLE);
        match session
            .wait_for(cap, |event| match event {
                Event::ObjectAdded(object) => Some(object.scoped_id()),
                _ => None,
            })
            .await
        {
            Ok(scoped_id) => {
                seen.insert(scoped_id);
            }
            // An idle gap (no new object for `cap`) means the scene has settled.
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    Ok(seen)
}
