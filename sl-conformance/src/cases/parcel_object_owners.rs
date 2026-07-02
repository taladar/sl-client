//! Request a parcel's per-owner object tally, then return objects to their owner.
//!
//! A land owner's "Objects" land-panel has two halves this case exercises against
//! the region-centre parcel, run as the **estate-owner** avatar
//! (`--avatar estate-owner`) who owns the region-wide parcel on the local grid
//! (both the object-owners request and the return need land rights):
//!
//! - **Request object owners** — [`Command::RequestParcelObjectOwners`]
//!   (`ParcelObjectOwnersRequest`, keyed on a [`ScopedParcelId`]) asks the
//!   simulator for one row per avatar/group with objects sitting on the parcel;
//!   the reply arrives as [`Event::ParcelObjectOwners`] (a `ParcelObjectOwnersReply`
//!   over UDP), each row a [`ParcelObjectOwner`] carrying the owner, a prim count,
//!   and an online flag. This is the data behind the panel's "Returnable objects"
//!   owner list.
//! - **Return objects** — [`Command::ReturnParcelObjects`] (`ParcelReturnObjects`)
//!   returns every object on the parcel owned by the listed owners to their owner's
//!   inventory. Using [`ParcelReturnType::LIST`] scoped to a single owner id mirrors
//!   the viewer's "Return objects owned by \<selected owner\>" button (the viewer
//!   sends the owner ids in the `OwnerIDs` block; the reference simulator
//!   `LandObject.ReturnLandObjects` matches `primsOverMe` by owner id).
//!
//! Neither the request-reply nor the return alters anything permanently that the
//! case does not restore, so it runs as a self-contained rez-tally-return-tally
//! cycle that leaves the region as found:
//!
//! 1. Wait for the region, learn the region-centre parcel's region-local id and
//!    owner from a `ParcelPropertiesRequest` reply (as in
//!    [`parcel_properties`](super::parcel_properties)),
//!    and confirm we own it.
//! 2. Request the object owners as a **baseline** and assert we own no objects on
//!    the parcel yet — the return below returns objects *by owner*, so a clean
//!    owner baseline guarantees the cycle touches only the throwaway object this
//!    case rezzes and nothing pre-existing.
//! 3. Rez a throwaway cube ([`Command::RezObject`], `ObjectAdd`) at the region
//!    centre; its arrival is the first [`Event::ObjectAdded`] with an id not seen
//!    while the initial scene settled.
//! 4. Request the object owners again and assert our owner now appears with a prim
//!    count one higher than the baseline — the tally reflects the new object.
//! 5. Return our objects on the parcel ([`Command::ReturnParcelObjects`],
//!    `ParcelReturnType::LIST` scoped to our owner id), confirmed by the
//!    [`Event::ObjectRemoved`] (`KillObject`) for the rezzed object's id.
//! 6. Request the object owners a final time and assert our owner is back to the
//!    baseline (no objects), leaving the parcel as found.
//!
//! `1av`, `[both]`. On OpenSim the avatar is forced to the "Default Region" centre
//! so the rez lands within terrain/parcel range; the single region-wide parcel is
//! owned by the estate owner, who starts with no objects on it, so the baseline is
//! empty, the cube tallies as one prim, and the return removes exactly that cube
//! (returning it to the estate owner's Lost and Found — inventory residue bounded
//! to one item per run, acceptable on a throwaway grid). Second Life enforces the
//! same message flow. The aditi run is deferred with the batch — it needs a
//! **full owned region** (the rez assumes we own the region centre), like
//! [`parcel_divide_join`](super::parcel_divide_join).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, Event, Object, OwnerKey, ParcelInfo, ParcelObjectOwner, ParcelReturnType, PrimShape,
    ScopedObjectId, ScopedParcelId, Uuid, Vector,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, is_opensim, secs_metric,
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

            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            let circuit = session.circuit_id().ok_or_else(|| {
                TestFailure::Assertion("login established no root circuit id".to_owned())
            })?;

            // 1. Learn the region-centre parcel's local id and owner; confirm we
            //    own it (the object-owners request and the return need land rights).
            let parcel = query_parcel(session, SEQ_CENTRE).await?;
            let local_id = parcel.local_id;
            let scoped_parcel = ScopedParcelId::new(circuit, local_id);
            check_eq(
                "parcel owner is the logged-in (estate-owner) avatar",
                &parcel.owner.uuid(),
                &agent.uuid(),
            )?;

            // 2. Baseline object owners. The return below returns objects by owner,
            //    so require that we own nothing on the parcel yet — then the cycle
            //    touches only the cube this case rezzes.
            let baseline = request_object_owners(session, scoped_parcel).await?;
            let owner_before = owner_count(&baseline, agent.uuid());
            check_eq(
                "the estate owner starts with no objects on the parcel",
                &owner_before,
                &0,
            )?;

            // Settle the initial scene: record every region-local id already
            // present so the object we rez is recognisable as new.
            let mut seen = settle_scene(session).await?;

            // 3. Rez a throwaway cube (`ObjectAdd`) at the region centre.
            let rez_started = Instant::now();
            session
                .send(Command::RezObject {
                    shape: PrimShape::cube(REZ_POSITION),
                    group_id: None,
                })
                .await?;
            let created = wait_for_new_object(session, &seen).await?.ok_or_else(|| {
                TestFailure::Assertion(
                    "no new object appeared after RezObject (ObjectAdd)".to_owned(),
                )
            })?;
            let rez_rtt = rez_started.elapsed();
            let created_id = created.scoped_id();
            seen.insert(created_id);

            // 4. Re-request the object owners: our owner should now tally one prim.
            tokio::time::sleep(TALLY_SETTLE).await;
            let after_rez = request_object_owners(session, scoped_parcel).await?;
            let owner_after_rez = owner_count(&after_rez, agent.uuid());
            let expected_after_rez = owner_before.checked_add(1).ok_or_else(|| {
                TestFailure::Assertion("baseline owner count overflowed i32".to_owned())
            })?;
            check_eq(
                "the object-owners tally reflects the rezzed cube (one prim)",
                &owner_after_rez,
                &expected_after_rez,
            )?;

            // 5. Return our objects on the parcel to their owner (the estate owner),
            //    scoped to our owner id, and watch for the cube's removal.
            let return_started = Instant::now();
            session
                .send(Command::ReturnParcelObjects {
                    local_id: scoped_parcel,
                    return_type: ParcelReturnType::LIST,
                    owner_ids: vec![OwnerKey::Agent(agent)],
                    task_ids: Vec::new(),
                })
                .await?;
            let removed = session
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

            // 6. Final object owners: our owner is back to the baseline (no objects).
            tokio::time::sleep(TALLY_SETTLE).await;
            let after_return = request_object_owners(session, scoped_parcel).await?;
            let owner_after_return = owner_count(&after_return, agent.uuid());
            check_eq(
                "the estate owner has no objects on the parcel again after the return",
                &owner_after_return,
                &owner_before,
            )?;

            let metrics = ctx.metrics();
            metrics.set("parcel_local_id", i64::from(local_id.0));
            metrics.set("owner_id", agent.uuid().to_string());
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
            Event::ParcelObjectOwners { owners } => Some(owners.clone()),
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

/// Waits for the next [`Event::ObjectAdded`] whose region-local id is not in
/// `seen` — the freshly rezzed object. Returns `None` if none appears within
/// [`STEP_TIMEOUT`].
async fn wait_for_new_object(
    session: &mut Session,
    seen: &HashSet<ScopedObjectId>,
) -> Result<Option<Object>, TestFailure> {
    match session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) if !seen.contains(&object.scoped_id()) => {
                Some((**object).clone())
            }
            _ => None,
        })
        .await
    {
        Ok(object) => Ok(Some(object)),
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}
