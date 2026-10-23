//! Notecards and gestures as each live grid keeps them: what a copy of an
//! item embedded in a notecard does, where a gesture's active state is kept,
//! and what a new gesture starts as.
//!
//! Measured by the `notecards-gestures` census on aditi and the local
//! OpenSim (2026-10-23, `book/src/gridspec/inventory.md` § Notecards and
//! gestures):
//!
//! | | Second Life | OpenSim |
//! | --- | --- | --- |
//! | `CopyInventoryFromNotecard` of an item the notecard holds | `200` and an undefined body; the copy filed in the folder asked for (or of its type), announced with a `BulkUpdateInventory` over the event queue | `200` and no body; the same filing and announcement — and an item of the agent's under the same id is copied as itself |
//! | of an item it does not hold | `200` and an undefined body; nothing | `400` and no body; an alert "Failed to retrieve item" |
//! | `ActivateGestures` / `DeactivateGestures` | nothing on the item | the item's flags bit 1 set / cleared |
//! | the next login's `gestures` | the active gestures, each with its asset; only when asked for | the same, whether asked for or not (an empty list for none) |
//! | a new gesture's body | the "/hey" default gesture | one zero byte |
//!
//! Neither grid restates the active set in the session, and an active
//! gesture that is removed is dropped from it. The fake grid keeps no
//! inventory between logins, so the set is the grid's, per account
//! ([`ActiveGestures`]), like the outfit folder's version.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use sl_proto::{CopyFromNotecardAnswer, InventoryDialect, InventoryItem, ServerEvent, SimSession};
use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, ObjectKey, OwnerKey};
use sl_wire::{GestureEntry, Permissions, Permissions5};
use uuid::Uuid;

/// The body Second Life gives a new gesture: its "/hey" default — a wave and
/// a sound, the words "Hey!" — as aditi served it.
pub(crate) const SECOND_LIFE_NEW_GESTURE: &[u8] = b"2\n255\n0\n/hey\nHey!\n2\n0\nWave\r\n\
9b29cd61-c45b-5689-ded2-91756b8d76a9\n0\n1\nhey_male.wav\n\
3a3fa800-81a0-6b88-d671-6faf0b457a72\n0\n\0";

/// The body OpenSim gives every item it makes empty: one zero byte
/// (`Scene.CreateAsset`'s `new byte[1]`).
pub(crate) const OPENSIM_EMPTY_BODY: &[u8] = &[0];

/// What OpenSim alerts a refused copy out of a notecard with.
pub(crate) const OPENSIM_COPY_REFUSAL: &str = "Failed to retrieve item";

/// The item flag OpenSim sets on an active gesture (`GesturesModule`).
const GESTURE_ACTIVE_FLAG: u32 = 1;

/// Each account's active gestures, item to asset: shared with every session,
/// which files its `ActivateGestures` / `DeactivateGestures` here, and read at
/// each login for the `gestures` array.
#[derive(Debug, Clone, Default)]
pub(crate) struct ActiveGestures(
    Arc<parking_lot::Mutex<HashMap<AgentKey, BTreeMap<InventoryKey, Uuid>>>>,
);

impl ActiveGestures {
    /// `agent`'s active gestures, as a login answer names them.
    pub(crate) fn entries(&self, agent: AgentKey) -> Vec<GestureEntry> {
        self.0
            .lock()
            .get(&agent)
            .map(|active| {
                active
                    .iter()
                    .map(|(item_id, asset_id)| GestureEntry {
                        item_id: *item_id,
                        asset_id: *asset_id,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Applies `change` to `agent`'s set.
    fn change(&self, agent: AgentKey, change: impl FnOnce(&mut BTreeMap<InventoryKey, Uuid>)) {
        change(self.0.lock().entry(agent).or_default());
    }
}

/// Files a gesture's activation, deactivation or removal for `agent` in
/// `gestures`, and on OpenSim flips the item's flag. Anything else is not this
/// module's, and `false` says so.
pub(crate) fn answer(
    gestures: (&ActiveGestures, AgentKey),
    sim: &mut SimSession,
    event: &ServerEvent,
) -> bool {
    let (active, agent) = gestures;
    let flags = sim.inventory_dialect() == InventoryDialect::OpenSim;
    match event {
        ServerEvent::ActivateGestures { gestures } => {
            active.change(agent, |set| {
                for gesture in gestures {
                    let _previous = set.insert(gesture.item_id, gesture.asset_id);
                }
            });
            if flags {
                for gesture in gestures {
                    set_flag(sim, gesture.item_id, true);
                }
            }
            true
        }
        ServerEvent::DeactivateGestures { item_ids } => {
            active.change(agent, |set| {
                for item in item_ids {
                    let _removed = set.remove(&InventoryKey::from(*item));
                }
            });
            if flags {
                for item in item_ids {
                    set_flag(sim, InventoryKey::from(*item), false);
                }
            }
            true
        }
        ServerEvent::InventoryItemRemoved { item_id } => {
            active.change(agent, |set| {
                let _removed = set.remove(item_id);
            });
            false
        }
        ServerEvent::InventoryCategoryRemoved { removed_items, .. }
        | ServerEvent::InventoryCategoryPurged { removed_items, .. } => {
            active.change(agent, |set| {
                for item in removed_items {
                    let _removed = set.remove(item);
                }
            });
            false
        }
        _other => false,
    }
}

/// Sets or clears the active flag on the agent's item `item`.
fn set_flag(sim: &mut SimSession, item: InventoryKey, on: bool) {
    let Some(mut held) = sim.agent_inventory().item(item).cloned() else {
        return;
    };
    if on {
        held.flags |= GESTURE_ACTIVE_FLAG;
    } else {
        held.flags &= !GESTURE_ACTIVE_FLAG;
    }
    sim.agent_inventory_mut().insert_item(held);
}

/// One `CopyInventoryFromNotecard` to answer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CopyRequest {
    /// The notecard holding the embedded item.
    pub(crate) notecard: InventoryKey,
    /// The object holding the notecard, `None` for the agent's own.
    pub(crate) object: Option<ObjectKey>,
    /// The embedded item to copy.
    pub(crate) item: InventoryKey,
    /// Where the copy is to go, `None` for the grid's pick.
    pub(crate) folder: Option<InventoryFolderKey>,
}

/// Copies the item `request` names out of its notecard into `sim`'s agent
/// inventory as the imitated grid does, announces it, and says how to answer
/// (see the module docs). A notecard in an object's contents is not read:
/// the fake grid answers it as it answers a notecard it finds nothing in.
pub(crate) fn copy_from_notecard(
    sim: &mut SimSession,
    assets: &crate::assets::GridAssets,
    request: CopyRequest,
    mint: &dyn Fn() -> Uuid,
    now: Instant,
) -> CopyFromNotecardAnswer {
    let dialect = sim.inventory_dialect();
    let copied = copy_of(sim, assets, request, dialect).map(|mut copy| {
        copy.item_id = InventoryKey::from(mint());
        copy.folder_id = destination(sim, request.folder, copy.item_type);
        copy
    });
    match (copied, dialect) {
        (Some(copy), _) => {
            let _update = sim.agent_inventory_mut().add_item(copy.clone());
            if let Err(error) = sim.enqueue_bulk_update_inventory(
                sl_proto::TransactionId::from(Uuid::nil()),
                &[],
                &[copy],
            ) {
                tracing::warn!("announcing a copy out of a notecard failed: {error}");
            }
            match dialect {
                InventoryDialect::SecondLife => CopyFromNotecardAnswer::Undefined,
                InventoryDialect::OpenSim => CopyFromNotecardAnswer::Empty,
            }
        }
        (None, InventoryDialect::SecondLife) => CopyFromNotecardAnswer::Undefined,
        (None, InventoryDialect::OpenSim) => {
            if let Err(error) = sim.send_alert_message(OPENSIM_COPY_REFUSAL, &[], &[], now) {
                tracing::warn!("alerting a refused copy out of a notecard failed: {error}");
            }
            CopyFromNotecardAnswer::Refused
        }
    }
}

/// The item a copy is made of: on OpenSim an item of the agent's under the
/// asked-for id, as itself; otherwise the item the notecard embeds under it.
fn copy_of(
    sim: &SimSession,
    assets: &crate::assets::GridAssets,
    request: CopyRequest,
    dialect: InventoryDialect,
) -> Option<InventoryItem> {
    if request.object.is_some() {
        return None;
    }
    if dialect == InventoryDialect::OpenSim
        && let Some(local) = sim.agent_inventory().item(request.item)
    {
        return Some(local.clone());
    }
    let asset = sim.agent_inventory().item(request.notecard)?.asset_id;
    let body =
        sl_proto::AssetSource::get(&*assets.read(), sl_proto::AssetKey::from(asset))?.to_vec();
    let notecard = sl_notecard::Notecard::decode(&body).ok()?;
    let embedded = notecard
        .items
        .iter()
        .find(|item| item.item_id.0 == request.item.uuid())?;
    // An item whose asset the grid has not got is not copied: OpenSim refuses
    // it ("Failed to find the notecard item asset"), and Second Life's
    // verifier never let one be saved.
    sl_proto::AssetSource::get(
        &*assets.read(),
        sl_proto::AssetKey::from(embedded.asset_id.0),
    )?;
    let agent = sim
        .agent_id()
        .unwrap_or_else(|| AgentKey::from(Uuid::nil()));
    Some(item_of(embedded, agent, dialect))
}

/// An embedded item as the agent's own, with the masks the grid gives it:
/// Second Life keeps the embedded ones, OpenSim takes the group's away and
/// limits everyone's to the next owner's (`BunchOfCaps.CopyInventoryFromNotecard`
/// for an item its copier owns).
fn item_of(
    embedded: &sl_notecard::InventoryItem,
    agent: AgentKey,
    dialect: InventoryDialect,
) -> InventoryItem {
    let masks = &embedded.permissions;
    let next = masks.next_owner_mask.0;
    let (group, everyone) = match dialect {
        InventoryDialect::SecondLife => (masks.group_mask.0, masks.everyone_mask.0),
        InventoryDialect::OpenSim => (0, masks.everyone_mask.0 & next),
    };
    InventoryItem {
        item_id: InventoryKey::from(embedded.item_id.0),
        folder_id: InventoryFolderKey::from(Uuid::nil()),
        name: embedded.name.clone(),
        description: embedded.description.clone(),
        asset_id: embedded.asset_id.0,
        item_type: i8::try_from(
            sl_proto::AssetType::from_type_name(embedded.asset_type.type_name()).to_code(),
        )
        .unwrap_or(-1),
        inv_type: embedded.inventory_type.type_name().map_or(-1, |name| {
            i8::try_from(sl_proto::InventoryType::from_type_name(name).to_code()).unwrap_or(-1)
        }),
        flags: embedded.flags,
        sale_type: match embedded.sale_info.sale_type.type_name() {
            "orig" => 1,
            "copy" => 2,
            "cntn" => 3,
            _ => 0,
        },
        sale_price: None,
        creation_date: i32::try_from(embedded.creation_date).unwrap_or_default(),
        owner: OwnerKey::Agent(agent),
        last_owner_id: embedded.permissions.last_owner_id.0,
        creator_id: AgentKey::from(embedded.permissions.creator_id.0),
        group: None,
        permissions: Permissions5 {
            base: Permissions::from_bits(masks.base_mask.0),
            owner: Permissions::from_bits(masks.owner_mask.0),
            group: Permissions::from_bits(group),
            everyone: Permissions::from_bits(everyone),
            next_owner: Permissions::from_bits(next),
        },
    }
}

/// Where a copy of an item of asset type `item_type` goes: the folder asked
/// for where the agent has it, else the folder of the item's type, else the
/// root.
fn destination(
    sim: &SimSession,
    asked: Option<InventoryFolderKey>,
    item_type: i8,
) -> InventoryFolderKey {
    let inventory = sim.agent_inventory();
    asked
        .filter(|folder| inventory.folder(*folder).is_some())
        .or_else(|| crate::social::folder_of_type(sim, item_type))
        .or_else(|| {
            inventory
                .folders()
                .find(|folder| folder.parent_id.is_none())
                .map(|folder| folder.folder_id)
        })
        .unwrap_or_else(|| InventoryFolderKey::from(Uuid::nil()))
}

/// What a new notecard or gesture starts as ([`new_item_body`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NewBody {
    /// No asset at all: Second Life's notecard, until its first save.
    NoAsset,
    /// These bytes.
    Bytes(&'static [u8]),
}

/// What the imitated grid makes a new item of `asset_type` start as, where it
/// is a notecard or a gesture; `None` for any other type.
pub(crate) const fn new_item_body(
    dialect: InventoryDialect,
    asset_type: sl_proto::AssetType,
) -> Option<NewBody> {
    match (asset_type, dialect) {
        (sl_proto::AssetType::Notecard, InventoryDialect::SecondLife) => Some(NewBody::NoAsset),
        (sl_proto::AssetType::Gesture, InventoryDialect::SecondLife) => {
            Some(NewBody::Bytes(SECOND_LIFE_NEW_GESTURE))
        }
        (
            sl_proto::AssetType::Notecard | sl_proto::AssetType::Gesture,
            InventoryDialect::OpenSim,
        ) => Some(NewBody::Bytes(OPENSIM_EMPTY_BODY)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::SECOND_LIFE_NEW_GESTURE;
    use pretty_assertions::assert_eq;

    /// The default gesture is the bytes aditi served: 122 of them, the
    /// "/hey" trigger, ending in the NUL the reference ends a gesture with.
    #[test]
    fn the_second_life_default_gesture_is_aditis() {
        assert_eq!(SECOND_LIFE_NEW_GESTURE.len(), 122);
        assert!(SECOND_LIFE_NEW_GESTURE.starts_with(b"2\n255\n0\n/hey\n"));
        assert_eq!(SECOND_LIFE_NEW_GESTURE.last(), Some(&0));
    }
}
