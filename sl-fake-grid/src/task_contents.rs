//! Writes into a prim's contents, and who is told of each.
//!
//! A prim's contents are written five ways — an item dropped in, a script
//! made or dropped in, an item removed, an item moved out into the agent's
//! inventory, and an item's asset saved over a capability — and the two live
//! grids answer each differently enough that a viewer written against one
//! shows a stale Contents tab against the other. The differences are
//! [`ContentsPolicy`](crate::imitates::ContentsPolicy)'s, measured by the
//! `task-inventory` conformance case (`book/src/gridspec/building.md`
//! § Contents):
//!
//! - **Second Life sends the prim again, in full, to every viewer in sight
//!   after every write**, and the prim's record — the only thing that
//!   carries the contents serial — to every session holding it selected.
//!   OpenSim sends the record to the writer alone, selected or not, and the
//!   prim again only when a script lands or something is removed.
//! - **A script that starts moves Second Life's serial a second time**, and
//!   that second step is told with a `ReplyTaskInventory` nobody asked for.
//!   A script asked not to run is left stopped there and run anyway on
//!   OpenSim.
//! - **A remove on OpenSim files a copy of the item in the agent's Trash.**
//! - **A move leaves a copyable item where it was** on both, and says
//!   nothing: the serial does not move.
//!
//! What a write does to the listing is one store's business
//! ([`TaskInventory`](crate::world::TaskInventory)); this module decides what
//! is said about it, to the session that wrote and — as the
//! [`RegionChange`]s it returns — to the region's others.

use std::time::Instant;

use sl_proto::{
    AssetKey, AssetType, FolderType, InventoryItem, InventoryType, RegionLocalObjectId,
    RestoreItem, SaleType, ServerEvent, SimSession, TaskInventoryItem, TransactionId, prim_flags,
};
use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, ObjectKey, OwnerKey};
use sl_types::money::LindenAmount;

use crate::assets::GridAssets;
use crate::imitates::{
    ContentsAudience, PrimResent, RemovedItem, SaveTold, ScriptStart, StoppedScript,
};
use crate::inventory::InventoryAnnouncement;
use crate::object_edits::Selection;
use crate::world::{RegionChange, SceneFixtures, announce_created_items, send_objects};

/// The kinds of write a grid tells of differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Write {
    /// An item dropped in.
    Drop,
    /// A script made or dropped in.
    Script,
    /// An item removed.
    Remove,
    /// An item's asset saved over a capability.
    Save,
}

/// Everything a contents write is answered from and into.
pub(crate) struct ContentsStores<'a> {
    /// The region's world.
    pub(crate) world: &'a mut SceneFixtures,
    /// The grid's assets, which a new script's body goes into.
    pub(crate) assets: &'a GridAssets,
    /// What this session holds selected.
    pub(crate) selection: &'a Selection,
    /// The session's own machine.
    pub(crate) sim: &'a mut SimSession,
}

/// Answers one drained [`ServerEvent`] if it is a write into a prim's
/// contents, or a question about a script in one. `None` for anything else.
pub(crate) fn answer_contents_write(
    stores: ContentsStores<'_>,
    announcement: InventoryAnnouncement,
    mint: &dyn Fn() -> uuid::Uuid,
    event: &ServerEvent,
    now: Instant,
) -> Option<Vec<RegionChange>> {
    let ContentsStores {
        world,
        assets,
        selection,
        sim,
    } = stores;
    let policy = world.contents;
    match event {
        // An agent inventory item dropped into a prim. The simulator resolves
        // the item by id from the agent's *own* inventory rather than trusting
        // the copy the client sent, and mints a fresh task item id for it: a
        // task copy is a new item, not the same one in two places.
        ServerEvent::UpdateTaskInventory { local_id, item, .. } => {
            let object = world.object_by_local_id(*local_id)?;
            let Some(source) = sim.agent_inventory().item(item.item_id).cloned() else {
                tracing::debug!(
                    "a task inventory write named item {}, which this agent does not hold",
                    item.item_id
                );
                if let Some(alert) = policy.unheld_drop_alert {
                    crate::object_edits::send_alert(sim, alert, &[], now);
                }
                return Some(Vec::new());
            };
            let contents = world.task_inventories.entry(*local_id).or_default();
            let mut dropped = task_item_from(&source, object.full_id, mint);
            dropped.name = contents.free_name(&dropped.name);
            contents.write(dropped);
            Some(tell(world, selection, sim, *local_id, Write::Drop, now))
        }
        // A script: made new, when the item names no id, or the agent's own
        // dropped in.
        ServerEvent::RezScript { local_id, params } => {
            let object = world.object_by_local_id(*local_id)?;
            let agent = sim.agent_id()?;
            let mut script = if params.item.item_id.uuid().is_nil() {
                new_script(&params.item, object.full_id, agent, mint)
            } else {
                // A script the agent does not hold is not said anything
                // about, on either grid.
                let source = sim.agent_inventory().item(params.item.item_id).cloned()?;
                task_item_from(&source, object.full_id, mint)
            };
            // A script with no body of its own — a new one, and on Second
            // Life one made in the inventory and never saved — gets the
            // grid's.
            if script.asset_id.is_none() {
                let body = AssetKey::from(mint());
                let _previous = assets
                    .write()
                    .insert(body, policy.new_script_body.as_bytes().to_vec());
                script.asset_id = Some(body);
            }
            let runs = params.enabled || policy.stopped_script == StoppedScript::RunAnyway;
            let item_id = script.item_id;
            let contents = world.task_inventories.entry(*local_id).or_default();
            script.name = contents.free_name(&script.name);
            contents.write(script);
            let mut changes = tell(world, selection, sim, *local_id, Write::Script, now);
            if runs {
                changes.extend(start_script(world, selection, sim, *local_id, item_id, now));
            }
            Some(changes)
        }
        ServerEvent::RemoveTaskInventory { local_id, item_id } => {
            let _object = world.object_by_local_id(*local_id)?;
            let contents = world.task_inventories.get_mut(local_id)?;
            let was_running = contents.running.contains(item_id);
            // An item the prim does not hold: nothing is said, on either grid.
            let removed = contents.remove(*item_id)?;
            if was_running && policy.script_start == ScriptStart::MovesSerial {
                contents.touch();
            }
            if policy.removed_item == RemovedItem::FiledInTrash
                && let Some(trash) = folder_of_type(sim, FolderType::Trash)
            {
                let filed = agent_item_from(&removed, trash, mint);
                sim.agent_inventory_mut().insert_item(filed.clone());
                announce_created_items(announcement, sim, &[filed], nil_transaction(), now);
            }
            Some(tell(world, selection, sim, *local_id, Write::Remove, now))
        }
        // A copy into the agent's inventory. An item the agent may copy stays
        // in the prim, the serial stays where it was, and nobody is told
        // anything but the new item — on both grids.
        ServerEvent::MoveTaskInventory {
            local_id,
            folder_id,
            item_id,
        } => {
            let _object = world.object_by_local_id(*local_id)?;
            let contents = world.task_inventories.get_mut(local_id)?;
            let held = contents
                .items
                .iter()
                .find(|held| held.item_id == *item_id)
                .cloned()?;
            let moved = agent_item_from(&held, *folder_id, mint);
            sim.agent_inventory_mut().insert_item(moved.clone());
            announce_created_items(announcement, sim, &[moved], nil_transaction(), now);
            if held.permissions.owner.contains(sl_wire::Permissions::COPY) {
                return Some(Vec::new());
            }
            // The one copy there is leaves the prim with it.
            let _taken = contents.remove(*item_id);
            Some(tell(world, selection, sim, *local_id, Write::Remove, now))
        }
        // A save over an `Update*TaskInventory` capability. The asset and the
        // serial are the upload's own business ([`crate::uploads`], which ran
        // first); who hears of it is this module's.
        ServerEvent::CapsAssetUploaded { metadata, .. } => {
            let (task, item_type) = match &**metadata {
                sl_proto::CapsUploadMetadata::UpdateTaskItem { task_id, cap, .. } => {
                    (*task_id, saved_type(cap))
                }
                sl_proto::CapsUploadMetadata::UpdateScriptTask(request) => (request.task_id, None),
                _other => return None,
            };
            let local_id = world.local_id_of(task)?;
            if let Some(alert) = item_type.and_then(|item_type| policy.save_alert(item_type))
                && let Err(error) = sim.send_alert_message(alert, &[], &[], now)
            {
                tracing::warn!("telling of a saved task item failed: {error}");
            }
            Some(tell(world, selection, sim, local_id, Write::Save, now))
        }
        ServerEvent::RequestScriptRunning { object_id, item_id } => {
            let local_id = world.local_id_of(*object_id)?;
            let contents = world.task_inventories.get(&local_id)?;
            let item = InventoryKey::from(*item_id);
            // A script the prim does not hold is not answered.
            let _held = contents.items.iter().find(|held| held.item_id == item)?;
            let running = contents.running.contains(&item);
            if let Err(error) = sim.send_script_running_reply(*object_id, *item_id, running, now) {
                tracing::warn!("answering a script's run state failed: {error}");
            }
            Some(Vec::new())
        }
        ServerEvent::SetScriptRunning {
            object_id,
            item_id,
            running,
        } => {
            let local_id = world.local_id_of(*object_id)?;
            let contents = world.task_inventories.get_mut(&local_id)?;
            let item = InventoryKey::from(*item_id);
            let _held = contents.items.iter().find(|held| held.item_id == item)?;
            if *running {
                let _was = contents.running.insert(item);
            } else {
                let _was = contents.running.remove(&item);
            }
            Some(Vec::new())
        }
        _other => None,
    }
}

/// The item type an `Update*TaskInventory` capability saves, by its name.
fn saved_type(cap: &str) -> Option<InventoryType> {
    match cap {
        sl_proto::CAP_UPDATE_NOTECARD_TASK_INVENTORY => Some(InventoryType::Notecard),
        sl_proto::CAP_UPDATE_GESTURE_TASK_INVENTORY => Some(InventoryType::Gesture),
        _other => None,
    }
}

/// A transaction id that names no transaction.
fn nil_transaction() -> TransactionId {
    TransactionId::from(uuid::Uuid::nil())
}

/// Starts the script `item_id` of the prim `local_id`: it is running from
/// here on, and where the grid's serial counts that, the sessions holding the
/// prim selected are sent the listing's new serial in a `ReplyTaskInventory`
/// none of them asked for.
fn start_script(
    world: &mut SceneFixtures,
    selection: &Selection,
    sim: &mut SimSession,
    local_id: RegionLocalObjectId,
    item_id: InventoryKey,
    now: Instant,
) -> Vec<RegionChange> {
    let policy = world.contents;
    let Some(task) = world
        .object_by_local_id(local_id)
        .map(|object| object.full_id)
    else {
        return Vec::new();
    };
    let Some(contents) = world.task_inventories.get_mut(&local_id) else {
        return Vec::new();
    };
    let _was = contents.running.insert(item_id);
    if policy.script_start != ScriptStart::MovesSerial {
        return Vec::new();
    }
    contents.touch();
    if selection.contains(&local_id)
        && let Err(error) = sim.serve_task_inventory(task, contents.serial, &contents.items, now)
    {
        tracing::warn!("telling of a started script failed: {error}");
    }
    vec![RegionChange::ContentsListed { local_id }]
}

/// Says what a grid says of a write into the prim `local_id`: to the session
/// that wrote, here, and to the region's others as the changes returned.
fn tell(
    world: &mut SceneFixtures,
    selection: &Selection,
    sim: &mut SimSession,
    local_id: RegionLocalObjectId,
    write: Write,
    now: Instant,
) -> Vec<RegionChange> {
    let policy = world.contents;
    let mut changes = Vec::new();
    // The prim's flags say whether it holds a script, which is why a grid
    // that sends a prim again at all sends it when one lands or leaves.
    let scripted = world.task_inventories.get(&local_id).is_some_and(|held| {
        held.items
            .iter()
            .any(|item| item.inv_type == InventoryType::Script)
    });
    if let Some(object) = world.object_mut(local_id) {
        if scripted {
            object.update_flags |= prim_flags::SCRIPTED;
        } else {
            object.update_flags &= !prim_flags::SCRIPTED;
        }
    }
    let resent =
        policy.resends == PrimResent::EveryWrite || matches!(write, Write::Script | Write::Remove);
    if resent && let Some(object) = world.object_by_local_id(local_id) {
        if let Err(error) = send_objects(sim, std::slice::from_ref(&object), now) {
            tracing::warn!("sending a written prim again failed: {error}");
        }
        changes.push(RegionChange::Updated(Box::new(object)));
    }
    // The record, with its advanced contents serial. OpenSim's save tells the
    // saver with an alert and sends no record.
    if write == Write::Save && policy.save_told == SaveTold::Alert {
        return changes;
    }
    let Some(properties) = world.record_of(local_id) else {
        return changes;
    };
    let (to_writer, to_selectors) = match world.properties.contents_change_told_to {
        ContentsAudience::Selectors => (selection.contains(&local_id), true),
        ContentsAudience::Writer => (true, false),
    };
    if to_writer && let Err(error) = sim.send_object_properties(&properties, now) {
        tracing::warn!("pushing a written prim's contents serial failed: {error}");
    }
    if to_selectors {
        changes.push(RegionChange::PropertiesChanged {
            local_id,
            properties: Box::new(properties),
        });
    }
    changes
}

/// The folder of the agent's inventory that is of `folder_type`.
fn folder_of_type(sim: &SimSession, folder_type: FolderType) -> Option<InventoryFolderKey> {
    let code = folder_type.to_code();
    sim.agent_inventory()
        .folders()
        .find(|folder| folder.folder_type == code)
        .map(|folder| folder.folder_id)
}

/// The script a `RezScript` naming no item makes: the name, the description
/// and the masks the client sent, the agent as its creator and owner, and no
/// body yet.
fn new_script(
    asked: &RestoreItem,
    task: ObjectKey,
    agent: AgentKey,
    mint: &dyn Fn() -> uuid::Uuid,
) -> TaskInventoryItem {
    TaskInventoryItem {
        item_id: InventoryKey::from(mint()),
        parent_task: task,
        permissions: asked.permissions,
        creator_id: agent,
        last_owner_id: agent,
        owner: OwnerKey::Agent(agent),
        group: None,
        group_owned: false,
        asset_id: None,
        asset_type: AssetType::ScriptText,
        inv_type: InventoryType::Script,
        flags: 0,
        sale_type: SaleType::NotForSale,
        sale_price: LindenAmount(0),
        name: asked.name.clone(),
        description: asked.description.clone(),
        creation_date: asked.creation_date,
    }
}

/// The task-inventory entry an agent inventory item becomes when it is dropped
/// into a prim.
///
/// The item id is **fresh**: a task copy is a new item that happens to name the
/// same asset. Everything else is carried across from the item the simulator
/// holds, not from the copy the client sent. An item with no asset yet — a
/// script made in a Second Life inventory and never saved — has none here
/// either.
pub(crate) fn task_item_from(
    source: &InventoryItem,
    task: ObjectKey,
    mint: &dyn Fn() -> uuid::Uuid,
) -> TaskInventoryItem {
    TaskInventoryItem {
        item_id: InventoryKey::from(mint()),
        parent_task: task,
        permissions: source.permissions,
        creator_id: source.creator_id,
        last_owner_id: AgentKey::from(source.last_owner_id),
        owner: source.owner,
        group: source.group,
        group_owned: source.owner.is_group(),
        asset_id: (!source.asset_id.is_nil()).then(|| AssetKey::from(source.asset_id)),
        asset_type: AssetType::from_code(i32::from(source.item_type)),
        inv_type: InventoryType::from_code(i32::from(source.inv_type)),
        flags: source.flags,
        sale_type: SaleType::from_code(source.sale_type),
        sale_price: source.sale_price.clone().unwrap_or(LindenAmount(0)),
        name: source.name.clone(),
        description: source.description.clone(),
        creation_date: source.creation_date,
    }
}

/// The agent inventory item a prim's `held` item becomes in `folder`: a new
/// item under a new id, naming the same asset and keeping the name it had in
/// the prim.
fn agent_item_from(
    held: &TaskInventoryItem,
    folder: InventoryFolderKey,
    mint: &dyn Fn() -> uuid::Uuid,
) -> InventoryItem {
    let narrow = |code: i32| i8::try_from(code).unwrap_or(-1);
    InventoryItem {
        item_id: InventoryKey::from(mint()),
        folder_id: folder,
        name: held.name.clone(),
        description: held.description.clone(),
        asset_id: held
            .asset_id
            .as_ref()
            .map_or_else(uuid::Uuid::nil, AssetKey::uuid),
        item_type: narrow(held.asset_type.to_code()),
        inv_type: narrow(held.inv_type.to_code()),
        flags: held.flags,
        sale_type: held.sale_type.to_code(),
        sale_price: (held.sale_type != SaleType::NotForSale).then(|| held.sale_price.clone()),
        creation_date: held.creation_date,
        owner: held.owner,
        last_owner_id: held.last_owner_id.uuid(),
        creator_id: held.creator_id,
        group: held.group,
        permissions: held.permissions,
    }
}
