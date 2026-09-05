//! Where an upload's bytes go, and which item ends up naming them.
//!
//! Every save a viewer makes ends in one of three places, and until this module
//! existed all three ended nowhere: [`ServerEvent::CapsAssetUploaded`] and
//! [`ServerEvent::AssetUploaded`] went past the driver's flush unread, and
//! [`crate::assets::GridAssets`] was written once from the region fixtures and
//! never again. The grid answered "complete" and forgot the bytes.
//!
//! That is not a gap a test can shrug at, because **a save is only observable
//! as a re-fetch**. A viewer that trusts its own in-memory copy after a save —
//! which is the bug the round trip exists to catch — behaves identically
//! against a grid that stored the bytes and a grid that dropped them. So does
//! the editor's Save button. Folding the upload in is what makes the two
//! distinguishable below a live grid.
//!
//! # The three paths
//!
//! - **The two-stage CAPS uploader** ([`ServerEvent::CapsAssetUploaded`]) —
//!   `NewFileAgentInventory`, `UploadBakedTexture` and every
//!   `Update{Gesture,Notecard,Script,Settings,Material}{Agent,Task}Inventory`.
//!   The metadata says which of the three it is, so one arm covers the agent
//!   inventory, an object's task inventory and a bake.
//! - **The legacy UDP transaction upload** ([`ServerEvent::AssetUploaded`]) —
//!   how a *wearable* save reaches a grid, there being no capability for one.
//!   The bytes arrive under an id derived from the transaction, and the item
//!   they belong to is named separately, which is the next path.
//! - **`UpdateInventoryItem`** ([`ServerEvent::UpdateAgentInventoryItems`]) —
//!   the second half of that wearable save. Nothing else correlates the two:
//!   a simulator that stores the bytes and ignores this message leaves the item
//!   pointing at the asset it had before, and the viewer's next fetch of its
//!   own wearable answers with what it saved over.
//!
//! # Ordering
//!
//! The two halves of a transaction save arrive in the order the client sent
//! them (`AssetUploadRequest` then `UpdateInventoryItem`, one circuit, ordered),
//! so the bytes are in the store before the item names them. The reverse order
//! would still be correct — the item's `asset_id` is set from the *derived* id
//! either way, and the store is consulted at fetch time, not at bind time.
//!
//! # Locking
//!
//! Runs inside the driver's flush, under the session lock, and takes the region
//! world lock for a task-inventory write — the same session → region order
//! everything else in the crate takes.

use std::time::Instant;

use sl_proto::{
    AssetKey, AssetType, CapsUploadMetadata, InventoryItem, InventoryKey, InventoryType, OwnerKey,
    Permissions5, ServerEvent, SimSession, TransactionId,
};
use sl_types::key::ObjectKey;

use crate::assets::GridAssets;
use crate::world::RegionWorld;

/// Folds one drained [`ServerEvent`] into the grid's asset store and the
/// inventory that names it, and tells the client what changed.
///
/// Returns `true` when the event was an upload this module owns, so the caller
/// can see at a glance which events are still unclaimed.
pub(crate) fn answer_upload(
    assets: &GridAssets,
    world: &RegionWorld,
    sim: &mut SimSession,
    event: &ServerEvent,
    now: Instant,
) -> bool {
    match event {
        ServerEvent::CapsAssetUploaded {
            metadata,
            new_asset,
            new_inventory_item,
            data,
        } => {
            store(assets, *new_asset, data.clone());
            apply_caps_upload(world, sim, metadata, *new_asset, *new_inventory_item, now);
            true
        }
        ServerEvent::AssetUploaded {
            asset_id,
            asset_type,
            data,
            ..
        } => {
            // The bytes only. Which item they belong to arrives separately, as
            // an `UpdateInventoryItem` carrying the same transaction id — see
            // the module docs.
            tracing::debug!(
                "storing a {asset_type:?} transaction upload as {asset_id} ({} bytes)",
                data.len()
            );
            store(assets, *asset_id, data.clone());
            true
        }
        ServerEvent::UpdateAgentInventoryItems {
            items,
            transaction_id,
        } => {
            apply_item_updates(sim, items, *transaction_id, now);
            true
        }
        _other => false,
    }
}

/// Puts `data` in the grid-wide store under `key`, replacing whatever was there.
fn store(assets: &GridAssets, key: AssetKey, data: Vec<u8>) {
    let _previous = assets.write().insert(key, data);
}

/// Points the item a completed CAPS upload named at the asset it just stored.
///
/// A `NewFileAgentInventory` creates the item; every `Update*` family replaces
/// an existing item's asset id in place (in the agent's tree, or in an object's
/// task inventory); a baked texture names no item at all.
fn apply_caps_upload(
    world: &RegionWorld,
    sim: &mut SimSession,
    metadata: &CapsUploadMetadata,
    new_asset: AssetKey,
    new_inventory_item: Option<InventoryKey>,
    now: Instant,
) {
    match metadata {
        CapsUploadMetadata::BakedTexture => {}
        CapsUploadMetadata::NewFileInventory(request) => {
            let Some(item_id) = new_inventory_item else {
                tracing::warn!("a NewFileAgentInventory upload minted no inventory item");
                return;
            };
            let item = created_item(sim, request, item_id, new_asset);
            sim.agent_inventory_mut().insert_item(item.clone());
            announce(sim, &item, now);
        }
        CapsUploadMetadata::UpdateAgentItem { item_id, .. } => {
            repoint_agent_item(sim, *item_id, new_asset, now);
        }
        CapsUploadMetadata::UpdateScriptAgent(request) => {
            repoint_agent_item(sim, request.item_id, new_asset, now);
        }
        CapsUploadMetadata::UpdateTaskItem {
            task_id, item_id, ..
        } => {
            repoint_task_item(world, *task_id, *item_id, new_asset);
        }
        CapsUploadMetadata::UpdateScriptTask(request) => {
            repoint_task_item(world, request.task_id, request.item_id, new_asset);
        }
        // `CapsUploadMetadata` is `#[non_exhaustive]`: a family added later
        // stores its bytes (that happened above) but binds no item until
        // somebody teaches this match what it names.
        other => tracing::warn!("no item-binding rule for the upload metadata {other:?}"),
    }
}

/// The agent-inventory item a `NewFileAgentInventory` upload creates.
///
/// Everything but the ids comes from the client's own metadata, which is what a
/// grid has: the upload states the folder, the name, the description, the two
/// short class names and the permission masks it wants, and nothing else knows
/// them. The **owner and creator are the session's agent**, not the client's
/// claim — those are the two fields a client does not get to assert.
fn created_item(
    sim: &SimSession,
    request: &sl_wire::NewFileAgentInventoryRequest,
    item_id: InventoryKey,
    new_asset: AssetKey,
) -> InventoryItem {
    let agent = sim
        .agent_id()
        .unwrap_or_else(|| sl_proto::AgentKey::from(uuid::Uuid::nil()));
    InventoryItem {
        item_id,
        folder_id: request.folder_id,
        name: request.name.clone(),
        description: request.description.clone(),
        asset_id: new_asset.uuid(),
        item_type: narrow(AssetType::from_type_name(&request.asset_type).to_code()),
        inv_type: narrow(InventoryType::from_type_name(&request.inventory_type).to_code()),
        flags: 0,
        sale_type: sl_proto::SaleType::NotForSale.to_code(),
        sale_price: None,
        creation_date: 0,
        owner: OwnerKey::Agent(agent),
        last_owner_id: uuid::Uuid::nil(),
        creator_id: agent,
        group: None,
        permissions: Permissions5 {
            base: sl_proto::Permissions::from_bits(request.next_owner_mask),
            owner: sl_proto::Permissions::from_bits(request.next_owner_mask),
            group: sl_proto::Permissions::from_bits(request.group_mask),
            everyone: sl_proto::Permissions::from_bits(request.everyone_mask),
            next_owner: sl_proto::Permissions::from_bits(request.next_owner_mask),
        },
    }
}

/// Sets an agent-inventory item's asset id and hands the client the rewritten
/// item, so its own copy stops naming the asset the save replaced.
fn repoint_agent_item(
    sim: &mut SimSession,
    item_id: InventoryKey,
    new_asset: AssetKey,
    now: Instant,
) {
    let Some(mut item) = sim.agent_inventory().item(item_id).cloned() else {
        // A save onto an item the grid does not hold. The bytes are stored (the
        // client can still fetch them by the id it was given), but no item names
        // them, which is exactly what a real grid's inventory service would say.
        tracing::debug!("an upload named agent item {item_id}, which this agent does not hold");
        return;
    };
    item.asset_id = new_asset.uuid();
    sim.agent_inventory_mut().insert_item(item.clone());
    announce(sim, &item, now);
}

/// Sets a task-inventory item's asset id and advances the holding object's
/// contents serial.
///
/// The serial is the point: a changed asset is a changed listing, and a viewer
/// that sees the same serial twice keeps the listing it already downloaded —
/// so a save that left the serial alone would leave every *other* viewer's
/// cached contents naming the asset that was replaced.
fn repoint_task_item(
    world: &RegionWorld,
    task_id: ObjectKey,
    item_id: InventoryKey,
    new_asset: AssetKey,
) {
    let mut world = world.lock();
    let Some(local_id) = world.local_id_of(task_id) else {
        tracing::debug!("an upload named object {task_id}, which this region does not have");
        return;
    };
    let Some(contents) = world.task_inventories.get_mut(&local_id) else {
        tracing::debug!("an upload named an item in {task_id}, which holds nothing");
        return;
    };
    let Some(mut item) = contents
        .items
        .iter()
        .find(|held| held.item_id == item_id)
        .cloned()
    else {
        tracing::debug!("an upload named task item {item_id}, which {task_id} does not hold");
        return;
    };
    item.asset_id = Some(new_asset);
    // Through `write`, not by mutating in place: `write` is what advances the
    // serial, and a listing whose bytes changed under an unchanged serial is
    // one no viewer will re-read (see [`TaskInventory`]).
    contents.write(item);
    drop(world);
}

/// Applies the client's `UpdateInventoryItem` blocks: the metadata it sent, and
/// — where the block carried a transaction — the asset that transaction's
/// upload stored.
fn apply_item_updates(
    sim: &mut SimSession,
    items: &[sl_proto::UpdatedInventoryItem],
    transaction_id: TransactionId,
    now: Instant,
) {
    let mut written = Vec::new();
    for update in items {
        let Some(mut item) = sim.agent_inventory().item(update.item.item_id).cloned() else {
            tracing::debug!(
                "an item update named {}, which this agent does not hold",
                update.item.item_id
            );
            continue;
        };
        // The client's fields, except the ones it does not own: the item's id,
        // its owner and its creator stay the grid's. A rename that could also
        // reassign creatorship is a rename a viewer could use to launder an
        // asset's provenance.
        item.folder_id = update.item.folder_id;
        item.name.clone_from(&update.item.name);
        item.description.clone_from(&update.item.description);
        item.permissions = update.item.permissions;
        item.item_type = update.item.asset_type;
        item.inv_type = update.item.inv_type;
        item.flags = update.item.flags;
        item.sale_type = update.item.sale_type.to_code();
        item.sale_price.clone_from(&update.item.sale_price);
        if let Some(bound) = update.bound_asset {
            item.asset_id = bound.uuid();
        }
        sim.agent_inventory_mut().insert_item(item.clone());
        written.push((item, update.callback_id));
    }
    if written.is_empty() {
        return;
    }
    if let Err(error) = sim.send_inventory_items_created(&written, transaction_id, true, now) {
        tracing::warn!("confirming an inventory item update failed: {error}");
    }
}

/// Hands the client one server-side item creation / rewrite.
fn announce(sim: &mut SimSession, item: &InventoryItem, now: Instant) {
    if let Err(error) = sim.send_inventory_item_created(
        std::slice::from_ref(item),
        TransactionId::from(uuid::Uuid::nil()),
        true,
        now,
    ) {
        tracing::warn!("handing over an uploaded item failed: {error}");
    }
}

/// An `LLAssetType` / `LLInventoryType` code narrowed to the `i8` an inventory
/// item's wire block carries, falling back to LL's own "none" sentinel.
fn narrow(code: i32) -> i8 {
    i8::try_from(code).unwrap_or(-1)
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use sl_proto::{AssetSource as _, RegionLocalObjectId, TaskInventoryItem};

    use crate::world::TaskInventory;

    use super::*;

    /// What a test here returns when a lookup it depends on came back empty.
    type TestError = Box<dyn core::error::Error>;

    /// A stored asset is readable back from the store under the id the upload
    /// reported — the whole point of the module.
    #[test]
    fn a_caps_upload_lands_in_the_grid_store() {
        let assets = GridAssets::default();
        let key = AssetKey::from(uuid::Uuid::from_u128(0xA55E7));
        store(&assets, key, b"notecard body".to_vec());
        assert_eq!(assets.read().get(key), Some(b"notecard body".as_slice()));
    }

    /// A task-item save repoints the item **and** advances the contents serial,
    /// so a viewer holding the old listing re-reads it.
    #[test]
    fn a_task_item_save_advances_the_contents_serial() -> Result<(), TestError> {
        let mut fixtures = crate::world::SceneFixtures::new();
        let task = ObjectKey::from(uuid::Uuid::from_u128(0x0B7));
        let item_id = InventoryKey::from(uuid::Uuid::from_u128(0x17E));
        let local_id = RegionLocalObjectId(7);
        fixtures.objects.push(crate::world::box_prim(
            local_id,
            task,
            sl_proto::AgentKey::from(uuid::Uuid::from_u128(1)),
            sl_types::lsl::Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            sl_types::lsl::Vector {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        ));
        let item = TaskInventoryItem {
            item_id,
            parent_task: task,
            asset_id: Some(AssetKey::from(uuid::Uuid::from_u128(0x0117))),
            ..crate::scenario::stock_script_item()
        };
        let _previous = fixtures
            .task_inventories
            .insert(local_id, TaskInventory::stated(3, vec![item]));
        let world: RegionWorld = std::sync::Arc::new(parking_lot::Mutex::new(fixtures));

        let new_asset = AssetKey::from(uuid::Uuid::from_u128(0x0222));
        repoint_task_item(&world, task, item_id, new_asset);

        let guard = world.lock();
        let contents = guard
            .task_inventories
            .get(&local_id)
            .ok_or("the object still has contents")?;
        assert_eq!(contents.serial, 4, "the contents serial did not advance");
        assert_eq!(
            contents.items.first().and_then(|held| held.asset_id),
            Some(new_asset),
            "the task item still names the asset the save replaced"
        );
        drop(guard);
        Ok(())
    }

    /// A save onto an object this region does not have changes nothing and does
    /// not panic — the bytes are still stored, but no listing claims them.
    #[test]
    fn a_task_item_save_for_an_unknown_object_is_dropped() {
        let world: RegionWorld =
            std::sync::Arc::new(parking_lot::Mutex::new(crate::world::SceneFixtures::new()));
        repoint_task_item(
            &world,
            ObjectKey::from(uuid::Uuid::from_u128(0xDEAD)),
            InventoryKey::from(uuid::Uuid::from_u128(0xBEEF)),
            AssetKey::from(uuid::Uuid::from_u128(1)),
        );
        assert!(world.lock().task_inventories.is_empty());
    }
}
