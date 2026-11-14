//! Inventory one avatar gives another: where the gift is filed and when, what
//! the offer says, what an answer does, and what becomes of a gift to an
//! avatar that is away.
//!
//! A viewer gives with an `ImprovedInstantMessage` of dialog
//! `InventoryOffered`, its bucket the asset type and the item's (or folder's)
//! id, its id the viewer's transaction; it answers with `InventoryAccepted` or
//! `InventoryDeclined` to the giver, the offer's id as its id and a folder in
//! its bucket. The IM responder (`crate::messages`) hands each of those here.
//! What each live grid does was measured by the `inventory-offers`
//! conformance case on aditi and the local OpenSim (2026-10-22,
//! `book/src/gridspec/inventory.md` § Giving):
//!
//! | | Second Life | OpenSim |
//! | --- | --- | --- |
//! | when the gift is filed | when the offer is made, in the recipient's folder of its type (a folder under the root) | the same |
//! | the offer's bucket | the type and the recipient's copy's id | the same |
//! | the offer's id | the giver's transaction | the copy's id |
//! | what else the recipient is sent | a folder's copy over the event queue, without what it holds; nothing for an item | the copy in a UDP `BulkUpdateInventory`, a folder's without what it holds |
//! | an accept | nothing | passed on to the giver |
//! | a decline | nothing: the copy stays where it was filed | passed on to the giver; the copy moved to the Trash and announced |
//! | an avatar that is away | the copy filed at once; nothing said or kept | the copy filed at once; the giver alerted; the offer kept |
//!
//! On both the giver keeps its original, one whose next owner may not copy it
//! too, and the copy takes the next owner's masks. An original the giver
//! itself may not copy leaves its inventory without a word (measured on
//! OpenSim by `inventory-mutations`, § Changing inventory). A gift of an object the grid took carries
//! the taken linkset along, with the same masks and the recipient as its
//! owner, so the recipient can rez it.
//!
//! The fake grid keeps no inventory between sessions: a session's inventory is
//! rebuilt at each login. A gift to an avatar that is away is therefore held
//! by the grid (`Gifts`) and filed when its next session is prepared.
//!
//! An object's gift (`TaskInventoryOffered`, `llGiveInventory` and
//! `llGiveInventoryList` to an avatar) is offered by the script engine
//! (`crate::scripts`), which hands the copy here (`object_gift`,
//! `object_offered`); the answer (`TaskInventoryAccepted`,
//! `TaskInventoryDeclined`) comes through the IM responder
//! (`object_answered`):
//!
//! | | Second Life | OpenSim |
//! | --- | --- | --- |
//! | when an item is filed | on the accept, in the folder it names, announced (`UpdateCreateInventoryItem`) | at the offer, in the folder of its type, announced |
//! | when a folder is filed | on the accept, under the root, announced over the event queue without what it holds | at the offer, under the root, unannounced; on the accept, a `BulkUpdateInventory` naming it and what it holds |
//! | a decline | nothing filed | the copy moved to the Trash and announced |

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use sl_proto::{
    AssetType, FolderType, InstantMessage, InventoryFolder, InventoryItem, KeptMessage, SimSession,
    TransactionId,
};
use sl_types::key::{AgentKey, InventoryFolderKey, InventoryKey, OwnerKey};
use sl_wire::{Permissions, Permissions5};
use uuid::Uuid;

use crate::driver::SharedSim;
use crate::runtime::GridCore;
use crate::social::{Whereabouts, warn_on};

/// What OpenSim alerts the giver of a gift to an avatar that is away with.
pub const OPENSIM_OFFLINE_GIFT_TEXT: &str = "User not online. Inventory has been saved";

/// How a grid hands over what one avatar gives another
/// ([`ImitatedGrid::gift_policy`](crate::ImitatedGrid::gift_policy)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GiftPolicy {
    /// What the offer's id is.
    pub offer_id: OfferId,
    /// How the recipient is told of the copy before the offer.
    pub announcement: GiftAnnouncement,
    /// What an accept or a decline does.
    pub answers: GiftAnswers,
    /// What a gift to an avatar that is away brings besides the copy.
    pub away: AwayGift,
    /// When an object's gift is filed.
    pub object_filing: ObjectGiftFiling,
}

/// When an object's gift is filed ([`GiftPolicy::object_filing`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectGiftFiling {
    /// On the accept, in the folder the accept names; nothing on a decline:
    /// Second Life.
    OnAccept,
    /// When the offer is made, in the folder of its type; a decline moves it
    /// to the Trash: OpenSim.
    AtOffer,
}

impl Default for GiftPolicy {
    /// Second Life's, as the grid's own default flavour is.
    fn default() -> Self {
        crate::ImitatedGrid::SecondLife.gift_policy()
    }
}

/// What a gift's offer's id is ([`GiftPolicy::offer_id`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferId {
    /// The giver's transaction, as the viewer sent it: Second Life.
    Transaction,
    /// The recipient's copy's id: OpenSim.
    Copy,
}

/// How the recipient is told of the copy ([`GiftPolicy::announcement`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiftAnnouncement {
    /// A folder's copy in an event-queue `BulkUpdateInventory`, without what it
    /// holds; nothing for an item, whose offer is the only word of it: Second
    /// Life.
    FoldersOverEventQueue,
    /// Every copy in a UDP `BulkUpdateInventory`, a folder's without what it
    /// holds: OpenSim.
    EveryCopyOverUdp,
}

/// What an answer to an offer does ([`GiftPolicy::answers`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GiftAnswers {
    /// Nothing; nobody is sent anything: Second Life.
    Ignored,
    /// Passed on to the giver; a decline moves the copy to the Trash and
    /// announces it: OpenSim.
    Relayed,
}

/// What a gift to an avatar that is away brings besides the copy
/// ([`GiftPolicy::away`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwayGift {
    /// Nothing: the giver is told nothing and no offer is kept: Second Life.
    Silent,
    /// The giver is alerted ([`OPENSIM_OFFLINE_GIFT_TEXT`]) and the offer is
    /// kept for the recipient to ask for: OpenSim.
    AlertedAndKept,
}

/// A copy to be filed for its recipient.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Gift {
    /// The type of the folder the copy is filed in; [`None`] for the root.
    folder_type: Option<i8>,
    /// The copied folders: a given folder's copy first, its parent named when
    /// it is filed, and every folder under it after.
    folders: Vec<InventoryFolder>,
    /// The copied items: a given item's copy, its folder named when it is
    /// filed; or every item under a given folder.
    items: Vec<InventoryItem>,
    /// The giver's items the copies were made of.
    originals: Vec<InventoryKey>,
}

impl Gift {
    /// The id of the copy the offer names.
    pub(crate) fn copy_id(&self) -> Option<Uuid> {
        self.folders
            .first()
            .map(|folder| folder.folder_id.uuid())
            .or_else(|| self.items.first().map(|item| item.item_id.uuid()))
    }
}

/// The gifts the grid holds for avatars that were away when they were given,
/// and the objects' gifts waiting for an answer.
#[derive(Debug, Default)]
pub(crate) struct Gifts {
    /// What is held for each account, in the order given.
    held: HashMap<AgentKey, Vec<Gift>>,
    /// The objects' gifts offered and not yet answered, by the offer's id.
    object_offers: HashMap<Uuid, ObjectOffer>,
}

/// An object's gift waiting for its recipient's answer.
#[derive(Debug, Clone)]
struct ObjectOffer {
    /// The recipient.
    to: AgentKey,
    /// The copy, filed already on a grid that files it at the offer.
    gift: Gift,
}

/// Files in `sim` every gift held for `agent`, whose session it is: the
/// gifts given while it was away.
pub(crate) fn file_held(core: &GridCore, agent: AgentKey, sim: &mut SimSession) {
    let held = core.gifts.lock().held.remove(&agent).unwrap_or_default();
    for gift in held {
        let _filed = file(sim, gift);
    }
}

/// `permissions` as the next owner holds them: every mask cut to the next
/// owner's.
fn handed_over(permissions: Permissions5) -> Permissions5 {
    let next = permissions.next_owner;
    Permissions5 {
        base: permissions.base & next,
        owner: permissions.owner & next,
        group: permissions.group & next,
        everyone: permissions.everyone & next,
        next_owner: next & permissions.base,
    }
}

/// `item` as `to`'s copy of `from`'s, under `id`.
fn item_copy(item: &InventoryItem, id: Uuid, from: AgentKey, to: AgentKey) -> InventoryItem {
    InventoryItem {
        item_id: InventoryKey::from(id),
        owner: OwnerKey::Agent(to),
        last_owner_id: from.uuid(),
        group: None,
        permissions: handed_over(item.permissions),
        ..item.clone()
    }
}

/// The copy for `to` of what `from`'s offer names in `from`'s `sim`: the asset
/// type and the original's id in its bucket. [`None`] for an offer that names
/// nothing `from` holds.
fn gift_of(
    core: &GridCore,
    sim: &SimSession,
    bucket: &[u8],
    from: AgentKey,
    to: AgentKey,
) -> Option<Gift> {
    let (&kind, id) = bucket.split_first()?;
    let original = Uuid::from_bytes(<[u8; 16]>::try_from(id.get(..16)?).ok()?);
    let inventory = sim.agent_inventory();
    if i32::from(kind) == AssetType::Folder.to_code() {
        // A folder: it and everything under it, copied under new ids.
        let top = inventory.folder(InventoryFolderKey::from(original))?;
        let mut renamed: HashMap<InventoryFolderKey, InventoryFolderKey> = HashMap::new();
        let mut folders = vec![top.clone()];
        let mut index = 0;
        while let Some(folder) = folders.get(index).cloned() {
            folders.extend(
                inventory
                    .folders()
                    .filter(|child| child.parent_id == Some(folder.folder_id))
                    .cloned(),
            );
            index = index.saturating_add(1);
        }
        for folder in &folders {
            renamed.insert(
                folder.folder_id,
                InventoryFolderKey::from(core.minter.uuid()),
            );
        }
        let (originals, items) = inventory
            .items()
            .filter_map(|item| {
                let folder = renamed.get(&item.folder_id)?;
                Some((
                    item.item_id,
                    InventoryItem {
                        folder_id: *folder,
                        ..item_copy(item, core.minter.uuid(), from, to)
                    },
                ))
            })
            .unzip();
        let folders = folders
            .into_iter()
            .map(|folder| InventoryFolder {
                folder_id: renamed
                    .get(&folder.folder_id)
                    .copied()
                    .unwrap_or(folder.folder_id),
                parent_id: folder
                    .parent_id
                    .map(|parent| renamed.get(&parent).copied().unwrap_or(parent)),
                version: 1,
                ..folder
            })
            .collect();
        return Some(Gift {
            folder_type: None,
            folders,
            items,
            originals,
        });
    }
    let item = inventory.item(InventoryKey::from(original))?;
    let copy = item_copy(item, core.minter.uuid(), from, to);
    if let Some(mut linkset) = core.assets.taken_linkset(item.item_id) {
        let objects = handed_over_linkset(linkset.objects(), from, to);
        for (prim, object) in linkset.prims.iter_mut().zip(objects) {
            prim.object = object;
        }
        core.assets.insert_taken_linkset(copy.item_id, linkset);
    }
    Some(Gift {
        folder_type: Some(item.item_type),
        folders: Vec::new(),
        items: vec![copy],
        originals: vec![item.item_id],
    })
}

/// Takes out of the giver's `sim` every original of `gift` the giver may not
/// copy: the gift is that item itself, which leaves without a word (OpenSim,
/// `book/src/gridspec/inventory.md` § Changing inventory; Second Life's
/// measurement was the test account's).
fn give_up_no_copy(sim: &mut SimSession, gift: &Gift) {
    let originals: Vec<InventoryKey> = gift
        .originals
        .iter()
        .copied()
        .filter(|original| {
            sim.agent_inventory()
                .item(*original)
                .is_some_and(|item| !item.permissions.owner.contains(Permissions::COPY))
        })
        .collect();
    let inventory = sim.agent_inventory_mut();
    for original in originals {
        if let Some(item) = inventory.take_item(original) {
            inventory.step_version(item.folder_id);
        }
    }
}

/// A taken linkset as its next owner `to` holds it: owned by `to`, last owned
/// by `from`, every prim's masks cut to the next owner's.
fn handed_over_linkset(
    linkset: Vec<sl_proto::Object>,
    from: AgentKey,
    to: AgentKey,
) -> Vec<sl_proto::Object> {
    linkset
        .into_iter()
        .map(|mut object| {
            let mut properties = object
                .properties
                .take()
                .unwrap_or_else(|| crate::world::default_object_properties(&object));
            properties.owner = OwnerKey::Agent(to);
            properties.last_owner_id = from.uuid();
            properties.group = None;
            properties.permissions = handed_over(properties.permissions);
            object.owner_id = to.uuid();
            object.properties = Some(properties);
            object
        })
        .collect()
}

/// Files `gift` in `sim`'s agent inventory: the copy in the folder of its
/// type, or the root where there is none. The gift as filed.
fn file(sim: &mut SimSession, gift: Gift) -> Gift {
    file_in(sim, gift, None)
}

/// Files `gift` in `sim`'s agent inventory: the copy in `named`, where that is
/// a folder of the agent's, else in the folder of its type, or the root where
/// there is none. The gift as filed.
fn file_in(sim: &mut SimSession, mut gift: Gift, named: Option<InventoryFolderKey>) -> Gift {
    let root = sim
        .agent_inventory()
        .folders()
        .find(|folder| folder.parent_id.is_none())
        .map(|folder| folder.folder_id);
    let home = named
        .filter(|folder| sim.agent_inventory().folder(*folder).is_some())
        .or_else(|| {
            gift.folder_type
                .and_then(|folder_type| crate::social::folder_of_type(sim, folder_type))
        })
        .or(root);
    let Some(home) = home else {
        return gift;
    };
    if let Some(top) = gift.folders.first_mut() {
        top.parent_id = Some(home);
    } else if let Some(item) = gift.items.first_mut() {
        item.folder_id = home;
    }
    let inventory = sim.agent_inventory_mut();
    for folder in &gift.folders {
        inventory.insert_folder(folder.clone());
    }
    for item in &gift.items {
        inventory.insert_item(item.clone());
    }
    inventory.step_version(home);
    gift
}

/// An avatar gave something: the copy is filed for the recipient at once, and
/// the recipient offered it — or, where it is away, the copy held for it and
/// the giver told as the grid tells it.
pub(crate) async fn offered(core: &Arc<GridCore>, shared: &SharedSim, im: &InstantMessage) {
    let to = im.to_agent_id;
    if to.uuid().is_nil() || core.account_by_agent(to).is_none() {
        return;
    }
    let policy = core.gift_policy;
    let (from, gift) = {
        let mut state = shared.state.lock().await;
        let from = Whereabouts::of(&state);
        let gift = gift_of(core, &state.sim, &im.binary_bucket, from.agent, to);
        if let Some(gift) = gift.as_ref() {
            give_up_no_copy(&mut state.sim, gift);
        }
        drop(state);
        (from, gift)
    };
    let Some(gift) = gift else {
        tracing::debug!(
            "{} offered {to} something it does not hold",
            from.agent.uuid()
        );
        return;
    };
    let Some(copy) = gift.copy_id() else {
        return;
    };
    let offer = offer_of(core, &from, im, copy);
    let is_folder = !gift.folders.is_empty();
    let here = {
        let offer = offer.clone();
        let gift = gift.clone();
        crate::social::tell(core, to, move |state, now| {
            let filed = file(&mut state.sim, gift);
            announce(policy.announcement, &mut state.sim, &filed, is_folder, now);
            warn_on(
                state.sim.send_instant_message(&offer, now),
                "an inventory offer",
            );
        })
        .await
    };
    if here {
        return;
    }
    core.gifts.lock().held.entry(to).or_default().push(gift);
    if policy.away == AwayGift::AlertedAndKept {
        warn_on(
            shared
                .with_sim(|sim| {
                    sim.send_alert_message(OPENSIM_OFFLINE_GIFT_TEXT, &[], &[], shared.now())
                })
                .await,
            "a gift to an avatar that is away",
        );
        core.social.lock().keep(
            to,
            KeptMessage {
                message: offer.clone(),
                record: offer,
            },
        );
    }
}

/// The offer of `copy` the recipient of `im` is sent: the giver's message as
/// the grid passes it on, under the id the grid's policy names, its bucket
/// the type and the copy's id.
fn offer_of(
    core: &GridCore,
    from: &Whereabouts,
    im: &InstantMessage,
    copy: Uuid,
) -> InstantMessage {
    let mut offer = core
        .im_policy
        .passed_on(from, im, core.wall.unix_seconds_u32());
    offer.id = match core.gift_policy.offer_id {
        OfferId::Transaction => im.id,
        OfferId::Copy => copy,
    };
    let mut bucket = im.binary_bucket.get(..1).unwrap_or_default().to_vec();
    bucket.extend_from_slice(copy.as_bytes());
    offer.binary_bucket = bucket;
    offer
}

/// Tells the recipient of `filed` of it before its offer, as `announcement`
/// says.
fn announce(
    announcement: GiftAnnouncement,
    sim: &mut SimSession,
    filed: &Gift,
    is_folder: bool,
    now: Instant,
) {
    let nil = TransactionId::from(Uuid::nil());
    let top: Vec<InventoryFolder> = filed.folders.first().cloned().into_iter().collect();
    match announcement {
        GiftAnnouncement::FoldersOverEventQueue if is_folder => warn_on(
            sim.enqueue_bulk_update_inventory(nil, &top, &[]),
            "a given folder",
        ),
        GiftAnnouncement::FoldersOverEventQueue => {}
        GiftAnnouncement::EveryCopyOverUdp => {
            let items = if is_folder {
                Vec::new()
            } else {
                filed.items.clone()
            };
            warn_on(
                sim.send_bulk_update_inventory(nil, &top, &items, now),
                "a gift",
            );
        }
    }
}

/// The copy for `to` of what a prim owned by `from` gives: each of `held`, its
/// next owner's masks, under a new id — in a new folder called `folder`, under
/// the root, for `llGiveInventoryList`. An object carries the linkset the prim
/// holds for it along, as an avatar's gift does.
pub(crate) fn object_gift(
    core: &GridCore,
    held: &[sl_proto::TaskInventoryItem],
    folder: Option<&str>,
    from: AgentKey,
    to: AgentKey,
) -> Gift {
    let mint = || core.minter.uuid();
    let top = folder.map(|name| InventoryFolder {
        folder_id: InventoryFolderKey::from(mint()),
        parent_id: None,
        name: name.to_owned(),
        folder_type: -1,
        version: 1,
    });
    let placeholder = InventoryFolderKey::from(Uuid::nil());
    let items: Vec<InventoryItem> = held
        .iter()
        .map(|held| {
            let original = crate::task_contents::agent_item_from(held, placeholder, &mint);
            let copy = InventoryItem {
                folder_id: top.as_ref().map_or(placeholder, |top| top.folder_id),
                ..item_copy(&original, mint(), from, to)
            };
            if let Some(mut linkset) = core.assets.taken_linkset(held.item_id) {
                let objects = handed_over_linkset(linkset.objects(), from, to);
                for (prim, object) in linkset.prims.iter_mut().zip(objects) {
                    prim.object = object;
                }
                core.assets.insert_taken_linkset(copy.item_id, linkset);
            }
            copy
        })
        .collect();
    Gift {
        folder_type: if top.is_some() {
            None
        } else {
            items.first().map(|item| item.item_type)
        },
        folders: top.into_iter().collect(),
        items,
        originals: Vec::new(),
    }
}

/// Hands the recipient of `offer`, whose session `sim` is, an object's
/// `gift` after its offer: on a grid that files it at the offer, filed in the
/// folder of its type and an item announced. Remembered for the answer either
/// way.
pub(crate) fn object_offered(
    core: &GridCore,
    sim: &mut SimSession,
    offer: &InstantMessage,
    gift: Gift,
    now: Instant,
) {
    let gift = if core.gift_policy.object_filing == ObjectGiftFiling::AtOffer {
        let filed = file(sim, gift);
        if filed.folders.is_empty() {
            warn_on(
                sim.send_inventory_item_created(
                    &filed.items,
                    TransactionId::from(Uuid::nil()),
                    true,
                    now,
                ),
                "an object's gift",
            );
        }
        filed
    } else {
        gift
    };
    core.gifts.lock().object_offers.insert(
        offer.id,
        ObjectOffer {
            to: offer.to_agent_id,
            gift,
        },
    );
}

/// An avatar answered an object's offer (`TaskInventoryAccepted`, the folder
/// in the bucket, or `TaskInventoryDeclined`), the offer's id as `im`'s id: an
/// accept files the copy where the grid files it on the accept and announces
/// it; a decline moves a copy filed at the offer to the Trash.
pub(crate) async fn object_answered(core: &Arc<GridCore>, shared: &SharedSim, im: &InstantMessage) {
    let by = shared.state.lock().await.avatar.agent_id;
    let pending = {
        let mut gifts = core.gifts.lock();
        match gifts.object_offers.get(&im.id) {
            Some(offer) if offer.to == by => gifts.object_offers.remove(&im.id),
            _ => None,
        }
    };
    let Some(ObjectOffer { gift, .. }) = pending else {
        return;
    };
    let filed_at_offer = core.gift_policy.object_filing == ObjectGiftFiling::AtOffer;
    if im.dialog == sl_proto::ImDialog::TaskInventoryDeclined {
        if filed_at_offer {
            discard(shared, im).await;
        }
        return;
    }
    let named = im
        .binary_bucket
        .get(..16)
        .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
        .map(|bytes| InventoryFolderKey::from(Uuid::from_bytes(bytes)));
    shared
        .with_state(|state| {
            let now = shared.now();
            let sim = &mut state.sim;
            let nil = TransactionId::from(Uuid::nil());
            if filed_at_offer {
                if !gift.folders.is_empty() {
                    warn_on(
                        sim.send_bulk_update_inventory(nil, &gift.folders, &gift.items, now),
                        "an accepted folder",
                    );
                }
                return;
            }
            let is_folder = !gift.folders.is_empty();
            let filed = file_in(sim, gift, named.filter(|_named| !is_folder));
            if is_folder {
                let top: Vec<InventoryFolder> =
                    filed.folders.first().cloned().into_iter().collect();
                warn_on(
                    sim.enqueue_bulk_update_inventory(nil, &top, &[]),
                    "an accepted folder",
                );
            } else {
                warn_on(
                    sim.send_inventory_item_created(&filed.items, nil, true, now),
                    "an accepted gift",
                );
            }
        })
        .await;
}

/// An avatar declined the gift `im` names, on a grid that moves a declined
/// copy: the copy is moved to the folder the decline names, or the Trash,
/// and the move announced.
pub(crate) async fn discard(shared: &SharedSim, im: &InstantMessage) {
    let named = im
        .binary_bucket
        .get(..16)
        .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
        .map(|bytes| InventoryFolderKey::from(Uuid::from_bytes(bytes)));
    let copy = im.id;
    shared
        .with_state(|state| {
            let now = shared.now();
            let sim = &mut state.sim;
            let trash = named
                .filter(|folder| sim.agent_inventory().folder(*folder).is_some())
                .or_else(|| crate::social::folder_of_type(sim, FolderType::Trash.to_code()));
            let Some(trash) = trash else {
                return;
            };
            let inventory = sim.agent_inventory_mut();
            if let Some(mut item) = inventory.take_item(InventoryKey::from(copy)) {
                let left = item.folder_id;
                item.folder_id = trash;
                inventory.insert_item(item.clone());
                inventory.step_version(left);
                inventory.step_version(trash);
                warn_on(
                    sim.send_inventory_item_created(
                        &[item],
                        TransactionId::from(Uuid::nil()),
                        true,
                        now,
                    ),
                    "a declined gift",
                );
            } else if let Some(folder) = inventory.folder(InventoryFolderKey::from(copy)).cloned() {
                let moved = InventoryFolder {
                    parent_id: Some(trash),
                    ..folder.clone()
                };
                inventory.insert_folder(moved.clone());
                if let Some(left) = folder.parent_id {
                    inventory.step_version(left);
                }
                inventory.step_version(trash);
                warn_on(
                    sim.send_bulk_update_inventory(
                        TransactionId::from(Uuid::nil()),
                        &[moved],
                        &[],
                        now,
                    ),
                    "a declined folder",
                );
            }
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::handed_over;
    use pretty_assertions::assert_eq;
    use sl_wire::{Permissions, Permissions5};

    /// Whether `permissions` let their owner copy.
    fn owner_copies(permissions: Permissions5) -> bool {
        permissions.owner.contains(Permissions::COPY)
    }

    /// A full-permission item whose next owner may move, modify and transfer.
    fn no_copy_next() -> Permissions5 {
        Permissions5 {
            base: Permissions::ALL,
            owner: Permissions::ALL,
            group: Permissions::NONE,
            everyone: Permissions::NONE,
            next_owner: Permissions::from_bits(0x0008_6000),
        }
    }

    #[test]
    fn the_copy_takes_the_next_owners_masks() {
        let handed = handed_over(no_copy_next());
        assert!(!owner_copies(handed));
        assert_eq!(handed.owner, Permissions::from_bits(0x0008_6000));
        assert_eq!(handed.base, Permissions::from_bits(0x0008_6000));
        let full = Permissions5 {
            next_owner: Permissions::from_bits(0x0008_e000),
            ..no_copy_next()
        };
        assert!(owner_copies(handed_over(full)));
    }
}
