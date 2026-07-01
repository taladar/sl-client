//! Request an in-world object's task (prim) inventory, then update it and
//! observe the contents serial advance.
//!
//! A prim carries its own inventory — the scripts, notecards, sounds and
//! objects a build drops into it. A viewer learns its contents in two steps: a
//! [`Command::RequestTaskInventory`] returns a
//! [`Event::TaskInventoryReply`] carrying the current *contents serial* (which
//! the simulator bumps on every change) and the temporary Xfer filename to
//! download the full listing from; the serial alone is enough to tell whether a
//! cached listing is stale. Writing to the inventory — dragging an item from
//! agent inventory onto the prim — is a [`Command::UpdateTaskInventory`], after
//! which the serial advances.
//!
//! Rather than depend on a pre-populated prim, this case manufactures its own
//! self-contained fixture, each leg confirmed by an observable event (the same
//! interest-list stream [`super::object_update_decode`] decodes, plus the
//! task-inventory reply):
//!
//! 1. **Container**: rez a throwaway cube with [`Command::RezObject`]
//!    (`ObjectAdd`) a metre above a reference primitive — the prim whose task
//!    inventory this case reads and writes. Its arrival is an
//!    [`Event::ObjectAdded`] with a region-local id not seen during the settle.
//! 2. **Donor item**: rez a second cube and **take** it into the agent's
//!    Objects folder with [`Command::DerezObjects`]
//!    ([`DeRezDestination::TakeIntoAgentInventory`]); the simulator materialises
//!    the agent inventory item ([`Event::InventoryItemCreated`]) this case then
//!    drops into the container.
//! 3. **Request (empty)**: [`Command::RequestTaskInventory`] on the container
//!    returns serial `0` and an empty filename — the fresh cube's inventory is
//!    empty.
//! 4. **Update**: [`Command::UpdateTaskInventory`] with
//!    [`TaskInventoryKey::Item`] drops the taken item into the container. OpenSim
//!    resolves the item by id from the agent's own inventory and copies it in.
//! 5. **Request (populated)**: a second [`Command::RequestTaskInventory`] now
//!    returns a serial strictly greater than the baseline and a non-empty
//!    filename — the write is observable.
//! 6. **Clean up**: [`Command::DerezObjects`] the container to the Trash
//!    ([`DeRezDestination::Trash`]), confirmed by its [`Event::ObjectRemoved`]
//!    (`KillObject`), leaving the world scene as found.
//!
//! `1av`, `[both]`. No new client code — the
//! `RequestTaskInventory`/`UpdateTaskInventory` surface already existed; this
//! case only re-exports [`TaskInventoryKey`] / [`TaskInventoryReply`] from the
//! two runtime crates (as commit `d41e378` did for `ObjectPropertiesFamily`).
//! On OpenSim the avatar is forced into the "Default Region", which holds this
//! workspace's rezzed test object as the placement reference, so a primitive is
//! guaranteed and its absence fails the case. On Second Life the landing
//! region's contents are uncontrolled; a region that streams no primitive to
//! place against within the window is recorded `partial` rather than failed. The
//! take leaves the donor item in the Objects folder and the container's copy of
//! it goes to Trash with the container — bounded inventory residue, acceptable
//! on a throwaway grid. The aditi run is deferred with the rest of the Aditi
//! batch (no aditi record this session).

use std::collections::HashSet;
use std::time::Duration;

use sl_client_tokio::{
    Command, DeRezDestination, Event, FolderType, InventoryFolder, InventoryFolderKey,
    InventoryItem, Object, ObjectKey, PrimShape, RestoreItem, SaleType, ScopedObjectId,
    TaskInventoryKey, TaskInventoryReply, TransactionId, Uuid, Vector, pcode,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, count_metric, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object lives and serves as the rez placement
/// reference. On Second Life the avatar keeps `"last"` (a named OpenSim region
/// is meaningless there), and whatever region it lands in supplies the
/// reference primitive.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling the initial scene: collecting the region's
/// pre-existing object ids (so a freshly rezzed object is recognised as new) and
/// a reference primitive to place the throwaway against.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle: once no new [`Event::ObjectAdded`] has
/// arrived for this long the initial scene is considered fully streamed
/// (generous enough to span the `ObjectUpdateCached` cache-miss round trip a
/// digest triggers).
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for each step's confirming event (a rezzed object appearing,
/// the inventory item being created, the task inventory reply, the object being
/// removed). Kept generous for Aditi network jitter.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How far above the reference primitive to rez the container cube, in metres —
/// clear of the reference so the two never coincide, well inside the same parcel
/// so the rez permission check passes.
const CONTAINER_LIFT_M: f32 = 1.0;

/// How far above the reference primitive to rez the donor cube (taken into
/// inventory for the update) — a metre above the container so the two never
/// coincide.
const DONOR_LIFT_M: f32 = 2.0;

/// Requests a prim's task inventory, writes an item into it, and verifies the
/// contents serial advances.
#[derive(Debug)]
pub struct TaskInventory;

impl GridTest for TaskInventory {
    fn name(&self) -> &'static str {
        "task-inventory"
    }

    fn description(&self) -> &'static str {
        "Request and update a prim's task inventory"
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
        reason = "one linear flow: settle, rez container, rez+take donor, request/update/request, clean up"
    )]
    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            // The Objects folder (take destination) and Trash folder (cleanup
            // destination) come from the login inventory skeleton, emitted
            // before the region is ready — capture it first, before
            // `wait_for_region` would discard it.
            let (objects_folder, trash_folder) = {
                let session = ctx.primary();
                let folders = session
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::InventorySkeleton(folders) => Some(folders.clone()),
                        _ => None,
                    })
                    .await?;
                let root = agent_root(&folders);
                // Fall back to the inventory root if a well-known folder is
                // absent: OpenSim resolves its own Trash for a Delete derez
                // regardless, and a take into the root folder is still valid.
                let objects = folder_of_type(&folders, FolderType::Object)
                    .or(root)
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "inventory skeleton had neither an Objects nor a root folder"
                                .to_owned(),
                        )
                    })?;
                let trash = folder_of_type(&folders, FolderType::Trash)
                    .or(root)
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "inventory skeleton had neither a Trash nor a root folder".to_owned(),
                        )
                    })?;
                (objects, trash)
            };

            // Settle the initial scene: record every region-local id already
            // present (so an object we rez is recognisable as new) and keep the
            // first primitive as the placement reference.
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
            let container_position = lift(&base, CONTAINER_LIFT_M);
            let donor_position = lift(&base, DONOR_LIFT_M);

            let session = ctx.primary();

            // 1. Rez the container cube — the prim whose task inventory this
            //    case reads and writes.
            session
                .send(Command::RezObject {
                    shape: PrimShape::cube(container_position),
                    group_id: None,
                })
                .await?;
            let container = wait_for_new_object(session, &seen).await?.ok_or_else(|| {
                TestFailure::Assertion(
                    "no new object appeared after RezObject for the container".to_owned(),
                )
            })?;
            let container_id = container.scoped_id();
            seen.insert(container_id);

            // 2. Rez a donor cube and take it into the Objects folder — the
            //    agent inventory item this case drops into the container.
            session
                .send(Command::RezObject {
                    shape: PrimShape::cube(donor_position),
                    group_id: None,
                })
                .await?;
            let donor = wait_for_new_object(session, &seen).await?.ok_or_else(|| {
                TestFailure::Assertion(
                    "no new object appeared after RezObject for the donor".to_owned(),
                )
            })?;
            seen.insert(donor.scoped_id());
            session
                .send(Command::DerezObjects {
                    local_ids: vec![donor.scoped_id()],
                    destination: DeRezDestination::TakeIntoAgentInventory(objects_folder),
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                    group_id: None,
                })
                .await?;
            let item = session
                .wait_for(STEP_TIMEOUT, |event| match event {
                    Event::InventoryItemCreated { item, .. } => Some(item.clone()),
                    _ => None,
                })
                .await?;
            check(
                !item.item_id.uuid().is_nil(),
                "take produced an inventory item with a nil id",
            )?;

            // 3. Request the container's task inventory — a fresh cube's is
            //    empty, so serial 0 and an empty filename.
            let request_started = std::time::Instant::now();
            let before = request_task_inventory(session, container_id, container.full_id).await?;
            let request_rtt = request_started.elapsed();
            check(
                before.serial == 0,
                "a freshly rezzed cube reported a non-zero task inventory serial",
            )?;
            check(
                before.filename.is_empty(),
                "a freshly rezzed cube reported a non-empty task inventory filename",
            )?;

            // 4. Update: drop the taken item into the container's task
            //    inventory. OpenSim resolves it by id from agent inventory.
            let update_started = std::time::Instant::now();
            session
                .send(Command::UpdateTaskInventory {
                    target: container_id,
                    key: TaskInventoryKey::Item,
                    item: Box::new(task_item(&item)),
                })
                .await?;

            // 5. Request again: the serial has advanced and the filename is now
            //    non-empty — the write is observable.
            let after = request_task_inventory(session, container_id, container.full_id).await?;
            let update_rtt = update_started.elapsed();
            check(
                after.serial > before.serial,
                "the task inventory serial did not advance after UpdateTaskInventory",
            )?;
            check(
                !after.filename.is_empty(),
                "the task inventory filename stayed empty after UpdateTaskInventory",
            )?;

            // 6. Clean up: delete the container to Trash, confirmed by its
            //    KillObject, leaving the world scene as found.
            session
                .send(Command::DerezObjects {
                    local_ids: vec![container_id],
                    destination: DeRezDestination::Trash(trash_folder),
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                    group_id: None,
                })
                .await?;
            session
                .wait_for(STEP_TIMEOUT, |event| match event {
                    Event::ObjectRemoved { local_id, .. } if *local_id == container_id => {
                        Some(*local_id)
                    }
                    _ => None,
                })
                .await?;

            let metrics = ctx.metrics();
            metrics.set("container_object", container.full_id.to_string());
            metrics.set("donor_item", item.item_id.to_string());
            metrics.set("donor_item_name", item.name.clone());
            metrics.set(&count_metric("serial_before"), before.serial.to_string());
            metrics.set(&count_metric("serial_after"), after.serial.to_string());
            metrics.set_timing(&secs_metric("request_rtt"), request_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("update_rtt"), update_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// A point `lift` metres above `base`.
fn lift(base: &Vector, lift: f32) -> Vector {
    Vector {
        x: base.x,
        y: base.y,
        z: base.z + lift,
    }
}

/// Sends a [`Command::RequestTaskInventory`] for `target` and waits for the
/// matching [`Event::TaskInventoryReply`] (the one describing `task`, so a stray
/// reply for another object is skipped).
async fn request_task_inventory(
    session: &mut crate::context::Session,
    target: ScopedObjectId,
    task: ObjectKey,
) -> Result<TaskInventoryReply, TestFailure> {
    session
        .send(Command::RequestTaskInventory { target })
        .await?;
    session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::TaskInventoryReply(reply) if reply.task == task => Some(reply.clone()),
            _ => None,
        })
        .await
}

/// Builds the [`RestoreItem`] for an [`Command::UpdateTaskInventory`] that drops
/// the taken agent inventory `item` into a prim. OpenSim looks the item up by id
/// from the agent's own inventory and copies its asset in; the permission masks
/// and CRC carried here are not validated on the local grid.
fn task_item(item: &InventoryItem) -> RestoreItem {
    RestoreItem {
        item_id: item.item_id,
        folder_id: item.folder_id,
        creator_id: item.creator_id,
        owner: item.owner,
        group: item.group,
        permissions: item.permissions,
        transaction_id: Uuid::new_v4(),
        asset_type: item.item_type,
        inv_type: item.inv_type,
        flags: item.flags,
        sale_type: SaleType::from_code(item.sale_type),
        sale_price: item.sale_price.clone(),
        name: item.name.clone(),
        description: item.description.clone(),
        creation_date: item.creation_date,
        crc: 0,
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

/// Drains the region's initial object-update burst, returning the set of every
/// region-local id sighted and the first primitive seen (the placement
/// reference, or `None` if the region streamed no primitive). The drain ends
/// once no new [`Event::ObjectAdded`] has arrived for [`SETTLE_IDLE`], or the
/// overall [`SETTLE_WINDOW`] elapses.
async fn settle_scene(
    session: &mut crate::context::Session,
) -> Result<(HashSet<ScopedObjectId>, Option<Object>), TestFailure> {
    let mut seen = HashSet::new();
    let mut reference: Option<Object> = None;
    let started = std::time::Instant::now();
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
/// `seen` — the freshly rezzed object. Returns `None` if none appears within
/// [`STEP_TIMEOUT`] (a per-attempt timeout that consumes the whole window).
async fn wait_for_new_object(
    session: &mut crate::context::Session,
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
