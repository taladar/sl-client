//! Link a set of prims into one linkset, then delink it — the two halves of the
//! build-tool link operation, each confirmed by the re-parenting of the child
//! prims on the region's object-update stream.
//!
//! "Link and delink a set" needs a set of rezzed prims under one owner. Rather
//! than depend on pre-existing rezzable objects, this case manufactures a fresh
//! set of three throwaway cubes (a genuine *set*: one root plus two children)
//! and exercises both operations against them, each leg observed on the same
//! interest-list stream [`super::object_update_decode`] decodes:
//!
//! 1. **Create** three throwaway primitives with [`Command::RezObject`]
//!    (`ObjectAdd`), spaced a little apart above a reference primitive already in
//!    the region. Each arrival is an [`Event::ObjectAdded`] with a region-local
//!    id not seen during the initial scene settle; the first is the linkset
//!    root, the other two its children-to-be.
//! 2. **Link** the three into one linkset with [`Command::LinkObjects`] (the
//!    first id is the root, `ObjectLink`). The simulator re-broadcasts the whole
//!    linkset, so each child arrives as an [`Event::ObjectUpdated`] whose
//!    [`Object::parent_id`] now points at the root — the observable proof of the
//!    link.
//! 3. **Delink** the set with [`Command::DelinkObjects`] (`ObjectDelink`). Each
//!    former child becomes a solo object again, arriving as an
//!    [`Event::ObjectUpdated`] whose parent is back to zero (no parent) — the
//!    observable proof of the delink.
//! 4. **Clean up**: derez all three to the Trash
//!    ([`Command::DerezObjects`] / [`DeRezDestination::Trash`]), each confirmed
//!    by an [`Event::ObjectRemoved`] (`KillObject`), leaving the scene as found.
//!
//! `1av`, `[both]`. The `ObjectLink` handler links only same-owner prims and
//! needs no prior selection, so the self-manufactured set links cleanly on the
//! local grid. On OpenSim the avatar is forced into the "Default Region", which
//! holds this workspace's rezzed test object as the placement reference, so a
//! primitive is guaranteed and its absence fails the case. On Second Life the
//! landing region's contents are uncontrolled; a region that streams no
//! primitive to place against within the window is recorded `partial` rather
//! than failed. The Trash cleanup leaves three items per run — inventory residue
//! bounded and acceptable on a throwaway grid. The aditi run is deferred with
//! the rest of the Aditi batch (no aditi record this session).

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, DeRezDestination, Event, FolderType, InventoryFolder, InventoryFolderKey, Object,
    PrimShape, RegionLocalObjectId, ScopedObjectId, TransactionId, Uuid, Vector, pcode,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object lives and serves as the rez placement
/// reference. On Second Life the avatar keeps `"last"` (a named OpenSim region
/// is meaningless there), and whatever region it lands in supplies the
/// reference primitive.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling the initial scene: collecting the region's
/// pre-existing object ids (so a freshly rezzed object is recognised as new) and
/// a reference primitive to place the throwaway set against.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle: once no new [`Event::ObjectAdded`] has
/// arrived for this long the initial scene is considered fully streamed
/// (generous enough to span the `ObjectUpdateCached` cache-miss round trip a
/// digest triggers).
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for each step's confirming event(s): a rezzed object
/// appearing, a child re-parenting on link or delink, an object being removed.
/// Kept generous for Aditi network jitter.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// The number of prims the set is built from: one root plus two children — the
/// smallest set that exercises multi-child linking (as opposed to a two-prim
/// pair).
const SET_SIZE: usize = 3;

/// How far above the reference primitive to rez the set, in metres — clear of
/// the reference so nothing coincides, well inside the same parcel so the rez
/// permission check passes.
const REZ_LIFT_M: f32 = 1.0;

/// The horizontal spacing between the cubes of the set, in metres — enough that
/// the 0.5 m default cubes never perfectly overlap, well within one parcel and
/// the linker's reach.
const REZ_SPACING_M: f32 = 0.6;

/// Links a manufactured set of prims into one linkset and delinks it, verifying
/// both operations by the re-parenting of the child prims.
#[derive(Debug)]
pub struct ObjectLinkDelink;

impl GridTest for ObjectLinkDelink {
    fn name(&self) -> &'static str {
        "object-link-delink"
    }

    fn description(&self) -> &'static str {
        "Link a set of prims into one linkset, then delink it"
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

    #[expect(
        clippy::too_many_lines,
        reason = "one linear flow: settle, create the set, link, delink, clean up"
    )]
    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            // The Trash folder (cleanup destination) comes from the login
            // inventory skeleton, emitted before the region is ready — capture it
            // first, before `wait_for_region` would discard it.
            let trash_folder = {
                let session = ctx.primary();
                let folders = session
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::InventorySkeleton(folders) => Some(folders.clone()),
                        _ => None,
                    })
                    .await?;
                // Fall back to the inventory root if the Trash folder is absent:
                // OpenSim resolves the caller's own Trash for a Delete/Trash derez
                // regardless of the destination id.
                folder_of_type(&folders, FolderType::Trash)
                    .or_else(|| agent_root(&folders))
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "inventory skeleton had neither a Trash nor a root folder".to_owned(),
                        )
                    })?
            };

            // Settle the initial scene: record every region-local id already
            // present (so a rezzed cube is recognisable as new) and keep the first
            // primitive as the placement reference.
            let (mut seen, reference) = {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                settle_scene(session).await?
            };

            let reference = match reference {
                Some(reference) => reference,
                None if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "no primitive appeared in the Default Region object stream".to_owned(),
                    ));
                }
                None => {
                    ctx.mark_partial(
                        "landing region streamed no primitive to place against within the window",
                    );
                    return Ok(());
                }
            };

            let base = reference.motion.position.clone();
            let session = ctx.primary();

            // 1. Create the set of throwaway cubes, one at a time so each new id
            //    is captured unambiguously. The first cube is the linkset root.
            let create_started = Instant::now();
            let mut set: Vec<ScopedObjectId> = Vec::with_capacity(SET_SIZE);
            let mut created: Vec<Object> = Vec::with_capacity(SET_SIZE);
            for index in 0..SET_SIZE {
                let position = set_position(&base, index);
                session
                    .send(Command::RezObject {
                        shape: PrimShape::cube(position),
                        group_id: None,
                    })
                    .await?;
                let object = wait_for_new_object(session, &seen).await?.ok_or_else(|| {
                    TestFailure::Assertion(format!(
                        "no new object appeared after RezObject #{index} (ObjectAdd)"
                    ))
                })?;
                let id = object.scoped_id();
                seen.insert(id);
                set.push(id);
                created.push(object);
            }
            let create_rtt = create_started.elapsed();
            let (root, children) = set.split_first().ok_or_else(|| {
                TestFailure::Assertion("the created set was unexpectedly empty".to_owned())
            })?;
            let root = *root;
            let children: HashSet<ScopedObjectId> = children.iter().copied().collect();
            check(
                children.len() == SET_SIZE - 1,
                "the created set did not have distinct child ids",
            )?;

            // 2. Link the set (`ObjectLink`, root first). Each child re-broadcasts
            //    with its parent set to the root — the proof of the link.
            let link_started = Instant::now();
            session
                .send(Command::LinkObjects {
                    local_ids: set.clone(),
                })
                .await?;
            confirm_parents(session, children.clone(), Some(root)).await?;
            let link_rtt = link_started.elapsed();

            // 3. Delink the set (`ObjectDelink`). Each former child re-broadcasts
            //    with its parent back to zero — the proof of the delink.
            let delink_started = Instant::now();
            session
                .send(Command::DelinkObjects {
                    local_ids: set.clone(),
                })
                .await?;
            confirm_parents(session, children.clone(), None).await?;
            let delink_rtt = delink_started.elapsed();

            // 4. Clean up: derez the whole set to Trash, each confirmed by its
            //    `KillObject`, leaving the scene as found.
            session
                .send(Command::DerezObjects {
                    local_ids: set.clone(),
                    destination: DeRezDestination::Trash(trash_folder),
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                    group_id: None,
                })
                .await?;
            let removed = wait_for_removed(session, set.iter().copied().collect()).await?;
            check(
                removed == SET_SIZE,
                "not every object in the set was removed on cleanup",
            )?;

            let root_object = created
                .first()
                .ok_or_else(|| {
                    TestFailure::Assertion("the created set was unexpectedly empty".to_owned())
                })?
                .full_id
                .to_string();
            let child_objects = created
                .iter()
                .skip(1)
                .map(|object| object.full_id.to_string())
                .collect::<Vec<_>>()
                .join(",");

            let metrics = ctx.metrics();
            metrics.set("set_size", SET_SIZE.to_string());
            metrics.set("root_object", root_object);
            metrics.set("child_objects", child_objects);
            metrics.set_timing(&secs_metric("create_rtt"), create_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("link_rtt"), link_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("delink_rtt"), delink_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// The agent inventory root folder id from a login skeleton — the folder with no
/// parent (`None` if the skeleton is empty or rootless).
fn agent_root(folders: &[InventoryFolder]) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.parent_id.is_none())
        .map(|folder| folder.folder_id)
}

/// The id of the first folder of the given well-known type in a login skeleton
/// (`None` if absent).
fn folder_of_type(
    folders: &[InventoryFolder],
    folder_type: FolderType,
) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.folder_type == folder_type.to_code())
        .map(|folder| folder.folder_id)
}

/// The rez position of the `index`-th cube of the set: lifted [`REZ_LIFT_M`]
/// above the reference primitive and stepped [`REZ_SPACING_M`] along X so the
/// cubes never coincide.
fn set_position(base: &Vector, index: usize) -> Vector {
    // `index` is bounded by `SET_SIZE`, a tiny constant, so the widening cast to
    // f32 is exact and cannot truncate.
    let step = f32::from(u8::try_from(index).unwrap_or(u8::MAX));
    Vector {
        x: base.x + step * REZ_SPACING_M,
        y: base.y,
        z: base.z + REZ_LIFT_M,
    }
}

/// Drains the region's initial object-update burst, returning the set of every
/// region-local id sighted and the first primitive seen (the placement
/// reference, or `None` if the region streamed no primitive). The drain ends
/// once no new [`Event::ObjectAdded`] has arrived for [`SETTLE_IDLE`], or the
/// overall [`SETTLE_WINDOW`] elapses.
async fn settle_scene(
    session: &mut Session,
) -> Result<(HashSet<ScopedObjectId>, Option<Object>), TestFailure> {
    let mut seen = HashSet::new();
    let mut reference: Option<Object> = None;
    let started = Instant::now();
    loop {
        let remaining = SETTLE_WINDOW.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let cap = remaining.min(SETTLE_IDLE);
        match session
            .wait_for(cap, |event| match event {
                Event::ObjectAdded(object) => Some((**object).clone()),
                _ => None,
            })
            .await
        {
            Ok(object) => {
                if reference.is_none() && object.pcode == pcode::PRIMITIVE {
                    reference = Some(object.clone());
                }
                seen.insert(object.scoped_id());
            }
            // An idle gap (no new object for `cap`) means the scene has settled.
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    Ok((seen, reference))
}

/// Waits for the next [`Event::ObjectAdded`] whose region-local id is not in
/// `seen` — a freshly rezzed cube. Returns `None` if none appears within
/// [`STEP_TIMEOUT`] (a per-attempt timeout that consumes the whole window).
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

/// Waits until every id in `pending` has arrived as an [`Event::ObjectUpdated`]
/// with the expected parent: the given root (a link) when `expected_root` is
/// `Some`, or the zero/root sentinel (a delink) when it is `None`. Object updates
/// that do not match (motion-only terse updates, unrelated objects, or a wrong
/// parent) are skipped. Fails if the whole set has not re-parented within
/// [`STEP_TIMEOUT`].
async fn confirm_parents(
    session: &mut Session,
    mut pending: HashSet<ScopedObjectId>,
    expected_root: Option<ScopedObjectId>,
) -> Result<(), TestFailure> {
    let started = Instant::now();
    while !pending.is_empty() {
        let remaining = STEP_TIMEOUT.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(TestFailure::Assertion(format!(
                "{} object(s) never reached the expected parent state",
                pending.len()
            )));
        }
        match session
            .wait_for(remaining, |event| match event {
                Event::ObjectUpdated(object) => Some((**object).clone()),
                _ => None,
            })
            .await
        {
            Ok(object) => {
                let id = object.scoped_id();
                if !pending.contains(&id) {
                    continue;
                }
                let matched = match expected_root {
                    Some(root) => {
                        object.parent_id != RegionLocalObjectId(0)
                            && object.scoped_parent_id() == root
                    }
                    None => object.parent_id == RegionLocalObjectId(0),
                };
                if matched {
                    pending.remove(&id);
                }
            }
            // No update at all within the remaining window: loop so the
            // top-of-loop budget check turns it into a clear assertion.
            Err(TestFailure::Timeout(_)) => {}
            Err(other) => return Err(other),
        }
    }
    Ok(())
}

/// Waits until every id in `pending` has arrived as an [`Event::ObjectRemoved`]
/// (`KillObject`), returning how many were removed. Fails if the whole set has
/// not been removed within [`STEP_TIMEOUT`].
async fn wait_for_removed(
    session: &mut Session,
    mut pending: HashSet<ScopedObjectId>,
) -> Result<usize, TestFailure> {
    let total = pending.len();
    let started = Instant::now();
    while !pending.is_empty() {
        let remaining = STEP_TIMEOUT.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(TestFailure::Assertion(format!(
                "{} object(s) were never removed on cleanup",
                pending.len()
            )));
        }
        match session
            .wait_for(remaining, |event| match event {
                Event::ObjectRemoved { local_id, .. } => Some(*local_id),
                _ => None,
            })
            .await
        {
            Ok(local_id) => {
                pending.remove(&local_id);
            }
            Err(TestFailure::Timeout(_)) => {}
            Err(other) => return Err(other),
        }
    }
    Ok(total)
}
