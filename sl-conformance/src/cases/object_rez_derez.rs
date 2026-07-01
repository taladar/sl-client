//! Rez an object from inventory, then derez/delete it — the full object
//! lifecycle round trip.
//!
//! "Rez from inventory" needs a rezzable object *item* in the agent's
//! inventory, which the test avatar is not guaranteed to have. So this case
//! manufactures one on the fly and exercises every leg of the lifecycle against
//! it, each confirmed by an observable event on the region's object-update
//! stream (the same interest-list stream [`super::object_update_decode`]
//! decodes):
//!
//! 1. **Create** a throwaway primitive with [`Command::RezObject`] (`ObjectAdd`,
//!    the build-tool "new prim" path), placed a metre above a reference
//!    primitive already in the region. Its arrival is the first
//!    [`Event::ObjectAdded`] carrying a region-local id not seen during the
//!    initial scene settle.
//! 2. **Take** it into the agent's Objects folder with [`Command::DerezObjects`]
//!    ([`DeRezDestination::TakeIntoAgentInventory`]). The object leaves the
//!    world and the simulator materialises the inventory item, delivered as an
//!    [`Event::InventoryItemCreated`] — the item this case then rezzes.
//! 3. **Rez from inventory**: [`Command::RezObjectFromInventory`] with the taken
//!    item's full payload rezzes it back into the world as a fresh object — a
//!    second [`Event::ObjectAdded`] with a new region-local id, the operation
//!    the roadmap item names.
//! 4. **Delete**: [`Command::DerezObjects`] to the Trash
//!    ([`DeRezDestination::Trash`]) removes it, confirmed by the
//!    [`Event::ObjectRemoved`] (`KillObject`) for that id, leaving the world
//!    scene as it was found.
//!
//! `1av`, `[both]`. Force-deleting with `ObjectDelete`
//! ([`Command::DeleteObjects`]) is a no-op on stock OpenSim, so the portable
//! delete is the derez-to-Trash above; OpenSim resolves the caller's own Trash
//! folder for a `Delete` derez regardless of the destination id, and looks the
//! source item up by id alone for the rez (the permission masks and CRC in the
//! rez payload are not validated), so this round trip is self-contained on the
//! local grid. On OpenSim the avatar is forced into the "Default Region", which
//! holds this workspace's rezzed test object as the placement reference, so a
//! primitive is guaranteed and its absence fails the case. On Second Life the
//! landing region's contents are uncontrolled; a region that streams no
//! primitive to place against within the window is recorded `partial` rather
//! than failed. The take leaves the created item in the Objects folder and the
//! final delete leaves a copy in Trash — inventory residue bounded to two items
//! per run, acceptable on a throwaway grid. The aditi run is deferred with the
//! rest of the Aditi batch (no aditi record this session).

use std::collections::HashSet;
use std::time::Duration;

use sl_client_tokio::{
    Command, DeRezDestination, Event, FolderType, InventoryFolder, InventoryFolderKey,
    InventoryItem, Object, PrimShape, RestoreItem, RezObjectParams, SaleType, ScopedObjectId,
    TransactionId, Uuid, Vector, pcode,
};

use crate::context::{TestContext, TestFailure};
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
/// a reference primitive to place the throwaway against.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle: once no new [`Event::ObjectAdded`] has
/// arrived for this long the initial scene is considered fully streamed
/// (generous enough to span the `ObjectUpdateCached` cache-miss round trip a
/// digest triggers).
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for each lifecycle step's confirming event (the rezzed
/// object appearing, the inventory item being created, the object being
/// removed). Kept generous for Aditi network jitter.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How far above the reference primitive to rez the throwaway cube, in metres —
/// clear of the reference so the two never coincide, well inside the same
/// parcel so the rez permission check passes.
const REZ_LIFT_M: f32 = 1.0;

/// Rezzes an object from inventory and derezzes/deletes it, verifying the full
/// create → take → rez → delete lifecycle by its object-update events.
#[derive(Debug)]
pub struct ObjectRezDerez;

impl GridTest for ObjectRezDerez {
    fn name(&self) -> &'static str {
        "object-rez-derez"
    }

    fn description(&self) -> &'static str {
        "Rez an object from inventory, then derez/delete it"
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
        reason = "one linear lifecycle: settle, create, take, rez-from-inventory, delete"
    )]
    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            // The Objects folder (take destination) and Trash folder (delete
            // destination) come from the login inventory skeleton, which is
            // emitted before the region is ready — capture it first, before
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
            // present (so the object we rez is recognisable as new) and keep the
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

            // Rez position: a metre above the reference primitive.
            let base = reference.motion.position.clone();
            let rez_position = Vector {
                x: base.x,
                y: base.y,
                z: base.z + REZ_LIFT_M,
            };

            let session = ctx.primary();

            // 1. Create a throwaway cube (`ObjectAdd`). Its arrival is the first
            //    `ObjectAdded` with an id not seen during the settle.
            let create_started = std::time::Instant::now();
            session
                .send(Command::RezObject {
                    shape: PrimShape::cube(rez_position.clone()),
                    group_id: None,
                })
                .await?;
            let created = wait_for_new_object(session, &seen).await?.ok_or_else(|| {
                TestFailure::Assertion(
                    "no new object appeared after RezObject (ObjectAdd)".to_owned(),
                )
            })?;
            let create_rtt = create_started.elapsed();
            let created_id = created.scoped_id();
            seen.insert(created_id);

            // 2. Take it into the Objects folder (`DeRezObject`, take): the world
            //    object is removed and the simulator materialises the inventory
            //    item this case will rez.
            let take_started = std::time::Instant::now();
            session
                .send(Command::DerezObjects {
                    local_ids: vec![created_id],
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
            let take_rtt = take_started.elapsed();
            check(
                !item.item_id.uuid().is_nil(),
                "take produced an inventory item with a nil id",
            )?;

            // 3. Rez that item back into the world (`RezObject` from inventory):
            //    a fresh object with a new region-local id.
            let rez_started = std::time::Instant::now();
            session
                .send(Command::RezObjectFromInventory {
                    params: Box::new(rez_params(&item, &rez_position)),
                })
                .await?;
            let rezzed = wait_for_new_object(session, &seen).await?.ok_or_else(|| {
                TestFailure::Assertion(
                    "no new object appeared after RezObjectFromInventory".to_owned(),
                )
            })?;
            let rez_rtt = rez_started.elapsed();
            let rezzed_id = rezzed.scoped_id();
            check(
                rezzed_id != created_id,
                "rez-from-inventory reused the created object's id — no fresh rez",
            )?;
            seen.insert(rezzed_id);

            // 4. Delete it to Trash (`DeRezObject`, delete): confirmed by the
            //    `KillObject` for that id, leaving the scene as found.
            let delete_started = std::time::Instant::now();
            session
                .send(Command::DerezObjects {
                    local_ids: vec![rezzed_id],
                    destination: DeRezDestination::Trash(trash_folder),
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                    group_id: None,
                })
                .await?;
            let removed = session
                .wait_for(STEP_TIMEOUT, |event| match event {
                    Event::ObjectRemoved { local_id, .. } if *local_id == rezzed_id => {
                        Some(*local_id)
                    }
                    _ => None,
                })
                .await?;
            let delete_rtt = delete_started.elapsed();
            check(
                removed == rezzed_id,
                "the removed object id did not match the rezzed object",
            )?;

            let metrics = ctx.metrics();
            metrics.set("item_id", item.item_id.to_string());
            metrics.set("item_name", item.name.clone());
            metrics.set("created_object", created.full_id.to_string());
            metrics.set("rezzed_object", rezzed.full_id.to_string());
            metrics.set_timing(&secs_metric("create_rtt"), create_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("take_rtt"), take_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("rez_rtt"), rez_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("delete_rtt"), delete_rtt.as_secs_f64());
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

/// Builds the [`RezObjectParams`] to rez `item` back into the world at
/// `position`, carrying the item's own permission masks and full payload. The
/// ray is bypassed so the object rezzes exactly at `position` (the headless rez
/// path), and the source item is left in inventory. The CRC is left `0`: OpenSim
/// looks the item up by id and does not validate it (a real CRC would be needed
/// only on Second Life).
fn rez_params(item: &InventoryItem, position: &Vector) -> RezObjectParams {
    RezObjectParams {
        group_id: None,
        from_task_id: None,
        bypass_raycast: true,
        ray_start: position.clone(),
        ray_end: position.clone(),
        ray_target_id: None,
        ray_end_is_intersection: false,
        rez_selected: false,
        remove_item: false,
        item_flags: item.flags,
        group_mask: item.permissions.group.bits(),
        everyone_mask: item.permissions.everyone.bits(),
        next_owner_mask: item.permissions.next_owner.bits(),
        item: RestoreItem {
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
        },
    }
}
