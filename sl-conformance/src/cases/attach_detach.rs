//! Wear an inventory object as an attachment, then detach it back into
//! inventory — the attachment lifecycle round trip.
//!
//! "Rez an attachment" needs a wearable object *item* in the agent's inventory,
//! which the test avatar is not guaranteed to have. So — like
//! [`super::object_rez_derez`] — this case manufactures one on the fly and then
//! exercises the two attachment wire messages against it, each confirmed by an
//! observable event on the region's object-update stream:
//!
//! 1. **Create** a throwaway primitive with [`Command::RezObject`] (`ObjectAdd`),
//!    placed a metre above a reference primitive already in the region, then
//!    **take** it into the agent's Objects folder with [`Command::DerezObjects`]
//!    ([`DeRezDestination::TakeIntoAgentInventory`]). The take materialises the
//!    inventory item ([`Event::InventoryItemCreated`]) that the rest of the case
//!    wears — the world object is removed in the process, leaving nothing rezzed
//!    to confuse the attach step.
//! 2. **Attach from inventory**: [`Command::RezAttachment`]
//!    (`RezSingleAttachmentFromInv`) wears that item at a chosen attachment point.
//!    Its arrival is an [`Event::ObjectAdded`] the simulator marks as an
//!    attachment — its `state` byte carries the swizzled attachment-point id
//!    ([`Object::attachment_point`]) and its `name_value` block carries an
//!    `AttachItemID` naming the very inventory item just worn
//!    ([`Object::name_value_data`]). Matching that id back to the item is the
//!    proof the *right* object was rezzed as an attachment, not merely that
//!    *some* object appeared.
//! 3. **Detach into inventory**: [`Command::DetachAttachmentIntoInventory`]
//!    (`DetachAttachmentIntoInv`) names the worn item's inventory id; the
//!    simulator resolves the worn object by its `FromItemID`, removes it from the
//!    scene and returns it to inventory. Removal is confirmed by the
//!    [`Event::ObjectRemoved`] (`KillObject`) for the attachment's region-local
//!    id, leaving the world scene as it was found.
//!
//! `1av`, `[both]`. Self-contained on the local grid: OpenSim's attachments
//! module rezzes the item from inventory, stamps the `AttachItemID` name-value on
//! the root part, and sends a `KillObject` on detach (`DeleteSceneObject` with
//! `silent = false`). On OpenSim the avatar is forced into the "Default Region",
//! which holds this workspace's rezzed test object as the placement reference, so
//! a primitive is guaranteed and its absence fails the case. On Second Life the
//! landing region's contents are uncontrolled; a region that streams no primitive
//! to place against within the window is recorded `partial` rather than failed.
//! The manufactured item is left in the Objects folder after the round trip
//! (inventory residue bounded to one item per run, acceptable on a throwaway
//! grid). The aditi run is deferred with the rest of the Aditi batch.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AttachmentMode, AttachmentPoint, Command, DeRezDestination, Event, FolderType, InventoryFolder,
    InventoryFolderKey, Object, PrimShape, RezAttachment, ScopedObjectId, TransactionId, Uuid,
    Vector, pcode,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object lives and serves as the rez placement reference.
/// On Second Life the avatar keeps `"last"` (a named OpenSim region is
/// meaningless there), and whatever region it lands in supplies the reference.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling the initial scene: collecting the region's
/// pre-existing object ids (so a freshly rezzed object is recognised as new) and
/// a reference primitive to place the throwaway against.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle: once no new [`Event::ObjectAdded`] has
/// arrived for this long the initial scene is considered fully streamed.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for each lifecycle step's confirming event (the object
/// appearing/being taken/being worn/being removed). Kept generous for Aditi
/// network jitter.
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How far above the reference primitive to rez the throwaway cube, in metres —
/// clear of the reference so the two never coincide, well inside the same parcel
/// so the rez permission check passes.
const REZ_LIFT_M: f32 = 1.0;

/// The attachment point to wear the throwaway object on. A concrete point (right
/// hand) rather than [`AttachmentPoint::Default`], so the simulator does not have
/// to fall back to a saved slot a freshly-created object never had.
const WEAR_POINT: AttachmentPoint = AttachmentPoint::RightHand;

/// The `name_value` key the simulator stamps on a rezzed attachment's root part,
/// naming the inventory item it was worn from (OpenSim's
/// `AttachItemID STRING RW SV <uuid>`).
const ATTACH_ITEM_ID: &str = "AttachItemID";

/// Wears an inventory object as an attachment and detaches it back to inventory,
/// verifying the attach → detach lifecycle by its object-update events.
#[derive(Debug)]
pub struct AttachDetach;

impl GridTest for AttachDetach {
    fn name(&self) -> &'static str {
        "attach-detach"
    }

    fn description(&self) -> &'static str {
        "Wear an inventory object as an attachment, then detach it into inventory"
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
        reason = "one linear lifecycle: settle, create, take, attach, detach"
    )]
    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            // The Objects folder (take destination) comes from the login inventory
            // skeleton, emitted before the region is ready — capture it first,
            // before `wait_for_region` would discard it.
            let objects_folder = {
                let session = ctx.primary();
                let folders = session
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::InventorySkeleton(folders) => Some(folders.clone()),
                        _ => None,
                    })
                    .await?;
                let root = agent_root(&folders);
                // Fall back to the inventory root if the Objects folder is absent:
                // a take into the root folder is still valid.
                folder_of_type(&folders, FolderType::Object)
                    .or(root)
                    .ok_or_else(|| {
                        TestFailure::Assertion(
                            "inventory skeleton had neither an Objects nor a root folder"
                                .to_owned(),
                        )
                    })?
            };

            // The agent's own id — the owner of an item worn from its inventory.
            let owner_id = ctx
                .primary()
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("no agent id after login".to_owned()))?
                .uuid();

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

            // 1a. Create a throwaway cube (`ObjectAdd`): the first `ObjectAdded`
            //     with an id not seen during the settle.
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
            let created_id = created.scoped_id();
            seen.insert(created_id);

            // 1b. Take it into the Objects folder (`DeRezObject`, take): the world
            //     object is removed and the simulator materialises the inventory
            //     item this case will wear.
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
            check(
                !item.item_id.uuid().is_nil(),
                "take produced an inventory item with a nil id",
            )?;
            let worn_item_id = item.item_id.uuid();

            // 2. Wear it as an attachment (`RezSingleAttachmentFromInv`): the
            //    rezzed attachment is an `ObjectAdded` the simulator marks with the
            //    attachment point in `state` and an `AttachItemID` name-value equal
            //    to the worn item's id.
            let attach_started = Instant::now();
            session
                .send(Command::RezAttachment(RezAttachment {
                    item_id: item.item_id,
                    owner_id,
                    attachment_point: WEAR_POINT,
                    mode: AttachmentMode::Replace,
                    name: item.name.clone(),
                    description: item.description.clone(),
                }))
                .await?;
            let attachment = wait_for_attachment(session, worn_item_id)
                .await?
                .ok_or_else(|| {
                    TestFailure::Assertion(format!(
                        "no attachment carrying AttachItemID {worn_item_id} appeared after \
                         RezSingleAttachmentFromInv"
                    ))
                })?;
            let attach_rtt = attach_started.elapsed();
            let attachment_id = attachment.scoped_id();
            let worn_point = attachment.attachment_point();
            check(
                attachment.attachment_point_id().is_some(),
                "the rezzed object did not carry an attachment point in its state byte",
            )?;

            // 3. Detach it back into inventory (`DetachAttachmentIntoInv`, by the
            //    worn item's id): confirmed by the `KillObject` for the
            //    attachment's region-local id, leaving the scene as found.
            let detach_started = Instant::now();
            session
                .send(Command::DetachAttachmentIntoInventory {
                    item_id: item.item_id,
                })
                .await?;
            let removed = session
                .wait_for(STEP_TIMEOUT, |event| match event {
                    Event::ObjectRemoved { local_id, .. } if *local_id == attachment_id => {
                        Some(*local_id)
                    }
                    _ => None,
                })
                .await?;
            let detach_rtt = detach_started.elapsed();
            check(
                removed == attachment_id,
                "the removed object id did not match the worn attachment",
            )?;

            let metrics = ctx.metrics();
            metrics.set("item_id", item.item_id.to_string());
            metrics.set("item_name", item.name.clone());
            metrics.set("attachment_object", attachment.full_id.to_string());
            metrics.set(
                "attachment_point",
                worn_point.map_or_else(|| "unnamed".to_owned(), |point| format!("{point:?}")),
            );
            metrics.set(
                "attachment_point_id",
                i64::from(attachment.attachment_point_id().unwrap_or(0)),
            );
            metrics.set_timing(&secs_metric("attach_rtt"), attach_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("detach_rtt"), detach_rtt.as_secs_f64());
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
/// reference, or `None` if the region streamed no primitive). The drain ends once
/// no new [`Event::ObjectAdded`] has arrived for [`SETTLE_IDLE`], or the overall
/// [`SETTLE_WINDOW`] elapses.
async fn settle_scene(
    session: &mut crate::context::Session,
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
/// `seen` — the freshly rezzed object. Returns `None` if none appears within
/// [`STEP_TIMEOUT`].
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

/// Waits for the next [`Event::ObjectAdded`] that is an attachment worn from
/// inventory item `item_id`: an object whose `name_value` block carries an
/// `AttachItemID` equal to `item_id`. Returns `None` if none appears within
/// [`STEP_TIMEOUT`].
async fn wait_for_attachment(
    session: &mut crate::context::Session,
    item_id: Uuid,
) -> Result<Option<Object>, TestFailure> {
    match session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectAdded(object)
                if object
                    .name_value_data(ATTACH_ITEM_ID)
                    .and_then(|value| value.trim().parse::<Uuid>().ok())
                    == Some(item_id) =>
            {
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
