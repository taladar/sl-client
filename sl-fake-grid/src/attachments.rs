//! The agent's attachments: wearing an inventory item, taking it off, an
//! object worn from the world, and what is worn travelling with its wearer
//! from region to region.
//!
//! A worn object is the region's state (`seating`): its root carries
//! [`AttachedTo`] — the wearer, the point, and what it is worn as
//! ([`WornItem`]) — and the wire states the wearer's avatar as its parent,
//! the point in its state and the item in its `AttachItemID` name-value.
//! What the two live grids answer every way of wearing and taking one off
//! was measured by the `attachments` census (2026-10-20,
//! `book/src/gridspec/appearance.md` § Attachments); where they differ is
//! [`AttachmentPolicy`]. Both grids:
//!
//! - wear an item's object under ids the region mints, and an object from the
//!   world under the ids it had, filing a new item for it, announced over
//!   UDP;
//! - ignore an item already worn, and refuse the one past the limit of 38
//!   without a word;
//! - kill what comes off for every viewer and restate its item — one item for
//!   a message that takes off several — but restate nothing for a take-off by
//!   the item (`DetachAttachmentIntoInv`) and nothing for a temporary
//!   attachment, which is gone;
//! - put a dropped object back under the same ids with no name-values, and
//!   remove its item;
//! - state a worn and a dropped object's owner as nil
//!   ([`sl_proto::UpdateOwner`]), and show a HUD to its wearer alone;
//! - carry what is worn to the region a teleport goes to, under the same full
//!   ids and new local ids, and put it back on at the next login.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Instant;

use sl_proto::{
    AlertInfo, AttachmentMode, AvatarAttachment, InventoryItem, Object, RegionLocalObjectId,
    RezAttachment, SimSession,
};
use sl_types::key::{AgentKey, InventoryKey, ObjectKey};
use sl_types::lsl::Vector;

use crate::scene_store::{AttachedTo, WornItem};
use crate::world::{AvatarIdentity, RegionChange, SceneFixtures, TaskInventory};

/// The name-value key a worn object names what it is worn as under.
pub const ATTACH_ITEM_ID: &str = "AttachItemID";

/// The most objects an avatar wears at once on both grids: the login's
/// `attachment_limit` on Second Life, `MaxAgentAttachments` on both.
pub const ATTACHMENT_LIMIT: usize = 38;

/// The alert Second Life answers a wear of an item nobody has with.
const REZ_FAILED_TEXT: &str = "Attempt to rez an object failed.";

/// The key of [`REZ_FAILED_TEXT`].
const REZ_FAILED_KEY: &str = "RezAttemptFailed";

/// How a grid wears and takes off attachments, where the live grids disagree
/// ([`ImitatedGrid::attachment_policy`](crate::ImitatedGrid::attachment_policy)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is one measured yes-or-no difference between the two grids"
)]
pub struct AttachmentPolicy {
    /// What a wear that *replaces* takes off the point.
    pub replace: Replace,
    /// Whether a wear of an item nobody has is answered with an alert
    /// (`RezAttemptFailed`). Second Life says so; OpenSim says nothing.
    pub alerts_unknown_item: bool,
    /// How an item that comes off is restated: on the event queue
    /// (`BulkUpdateInventory`, Second Life) or over UDP
    /// (`UpdateCreateInventoryItem`, OpenSim).
    pub restated: crate::inventory::InventoryAnnouncement,
    /// Whether the agent's own `AvatarAppearance` lists what it wears, and is
    /// sent again after a wear of one item from the inventory. Second Life's
    /// does; OpenSim's lists nothing and is not sent again.
    pub appearance_lists_worn: bool,
    /// Whether a viewer that is not the wearer is told the owner may modify a
    /// worn object (`OBJECT_OWNER_MODIFY`). Second Life tells it; OpenSim
    /// does not.
    pub others_told_owner_modifies: bool,
    /// Whether a HUD taken off is killed for the viewers that were never sent
    /// it. Second Life kills it; OpenSim does not.
    pub hud_off_killed_for_others: bool,
    /// Whether the region a teleport leaves kills the avatar — for the agent
    /// itself before it arrives, and for the viewers left behind — and never
    /// what it wears. OpenSim's kills nothing.
    pub teleport_kills_avatar: bool,
    /// Whether a worn object brought back into a region it left takes the
    /// local id it had there. OpenSim's does; Second Life mints a new one.
    pub returning_keeps_local_id: bool,
    /// Which owner an object update states ([`sl_proto::UpdateOwner`]).
    pub update_owner: sl_proto::UpdateOwner,
    /// The script memory the `AttachmentResources` report states for each
    /// script a worn object holds — 65,536 bytes, a Mono script's, on Second
    /// Life — or [`None`] where it states none: OpenSim never names `memory`
    /// and sums it as 0 (`avatar-render-info`, 2026-10-21).
    pub script_memory: Option<i32>,
}

impl Default for AttachmentPolicy {
    /// Second Life's.
    fn default() -> Self {
        crate::ImitatedGrid::SecondLife.attachment_policy()
    }
}

/// What a wear that replaces takes off the point ([`AttachmentPolicy::replace`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replace {
    /// The first object worn there: Second Life.
    First,
    /// Everything worn there: OpenSim, under `[Attachments] WearReplacesAll`,
    /// true by default.
    All,
}

/// Which of the items a take-off names are restated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Restate {
    /// Each one: a wear that replaces, a script's take-off.
    Each,
    /// The last one alone: a viewer's `ObjectDetach`, on both grids.
    Last,
    /// None: a take-off by the item (`DetachAttachmentIntoInv`).
    None,
}

/// The name-values a worn object carries: its `AttachItemID`, the item it is
/// worn as or its own id for a temporary attachment, or none while a script's
/// wear is still to be filed.
#[must_use]
pub(crate) fn name_values(item: WornItem, own: ObjectKey) -> String {
    let id = match item {
        WornItem::Item(item) => item.uuid(),
        WornItem::Temporary => own.uuid(),
        WornItem::Unfiled => return String::new(),
    };
    format!("{ATTACH_ITEM_ID} STRING RW SV {id}")
}

/// What `agent` wears, as its `AvatarAppearance` lists it: each worn object's
/// own id and its point, in the store's order.
#[must_use]
pub(crate) fn worn_list(world: &SceneFixtures, agent: AgentKey) -> Vec<AvatarAttachment> {
    world
        .objects
        .worn_by(agent)
        .into_iter()
        .filter_map(|root| {
            let point = world.objects.attached_to(root)?.point;
            let object = world.objects.get(root)?;
            Some(AvatarAttachment {
                id: object.full_id,
                attachment_point: point,
            })
        })
        .collect()
}

/// The items `agent` wears and their points, which the next login puts back
/// on ([`WornOutfits`]): temporary attachments and unfiled ones left out.
#[must_use]
pub(crate) fn worn_items(world: &SceneFixtures, agent: AgentKey) -> Vec<WornEntry> {
    world
        .objects
        .worn_by(agent)
        .into_iter()
        .filter_map(|root| {
            let attached = world.objects.attached_to(root)?;
            match attached.item {
                WornItem::Item(item) => Some((item, attached.point)),
                WornItem::Unfiled | WornItem::Temporary => None,
            }
        })
        .collect()
}

/// The root `agent` wears as `item`, if any.
fn worn_as(
    world: &SceneFixtures,
    agent: AgentKey,
    item: InventoryKey,
) -> Option<RegionLocalObjectId> {
    world.objects.worn_by(agent).into_iter().find(|root| {
        world
            .objects
            .attached_to(*root)
            .is_some_and(|attached| attached.item == WornItem::Item(item))
    })
}

/// What every account wears, as its last session left it: what a login puts
/// back on, as both grids do. Grid-wide, since an outfit outlives the session.
#[derive(Debug, Clone, Default)]
pub(crate) struct WornOutfits(Arc<parking_lot::Mutex<HashMap<AgentKey, Vec<WornEntry>>>>);

/// One worn item and the point it is worn on.
pub(crate) type WornEntry = (InventoryKey, u8);

impl WornOutfits {
    /// What `agent` was left wearing.
    pub(crate) fn get(&self, agent: AgentKey) -> Vec<WornEntry> {
        self.0.lock().get(&agent).cloned().unwrap_or_default()
    }

    /// Records what `agent` wears in `world` now.
    pub(crate) fn keep(&self, world: &SceneFixtures, agent: AgentKey) {
        let _before = self.0.lock().insert(agent, worn_items(world, agent));
    }
}

/// Who wears, and the two things a wear from the inventory reads besides.
pub(crate) struct Wearer<'a> {
    /// The agent.
    pub(crate) identity: &'a AvatarIdentity,
    /// Where it stands in the region.
    pub(crate) at: Vector,
    /// The region's handle, for what is rezzed.
    pub(crate) region_handle: sl_wire::RegionHandle,
}

/// Wears each of `rezzes`, the items of a `RezSingleAttachmentFromInv` or a
/// `RezMultipleAttachmentsFromInv`, in order: what the region's other
/// sessions are to be told, and whether anything was put on.
///
/// An item the agent does not hold is refused, with Second Life's alert; an
/// item already worn, and one past the limit, are ignored. A wear that
/// replaces takes the point's objects off first ([`Replace`]), restating
/// their items.
pub(crate) fn wear_items(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    (wearer, mint): (&Wearer<'_>, &dyn Fn() -> uuid::Uuid),
    sim: &mut SimSession,
    rezzes: &[RezAttachment],
    now: Instant,
) -> (Vec<RegionChange>, bool) {
    let policy = world.attachments;
    let agent = wearer.identity.agent_id;
    let mut changes = Vec::new();
    let mut wore = false;
    for rez in rezzes {
        let Some(item) = sim.agent_inventory().item(rez.item_id).cloned() else {
            if policy.alerts_unknown_item {
                let named = AlertInfo {
                    message: REZ_FAILED_KEY.to_owned(),
                    extra_params: String::new(),
                };
                if let Err(error) = sim.send_alert_message(REZ_FAILED_TEXT, &[named], &[], now) {
                    tracing::warn!("refusing a wear of an unknown item failed: {error}");
                }
            }
            continue;
        };
        if worn_as(world, agent, item.item_id).is_some() {
            continue;
        }
        let point = rez.attachment_point.to_code();
        if rez.mode == AttachmentMode::Replace {
            let on_point: Vec<RegionLocalObjectId> = world
                .objects
                .worn_by(agent)
                .into_iter()
                .filter(|root| {
                    world
                        .objects
                        .attached_to(*root)
                        .is_some_and(|attached| attached.point == point)
                })
                .collect();
            let off: Vec<RegionLocalObjectId> = match policy.replace {
                Replace::First => on_point.into_iter().take(1).collect(),
                Replace::All => on_point,
            };
            changes.extend(detach(
                world,
                assets,
                wearer.identity,
                sim,
                (&off, Restate::Each),
                now,
            ));
        }
        if world.objects.worn_by(agent).len() >= ATTACHMENT_LIMIT {
            continue;
        }
        if let Some(objects) = wear_item(world, assets, (wearer, mint), &item, point, now) {
            if let Err(error) = crate::world::send_objects(sim, &objects, now) {
                tracing::warn!("wearing an item failed: {error}");
            }
            changes.push(RegionChange::Worn { agent, objects });
            wore = true;
        }
    }
    (changes, wore)
}

/// Rezzes `item`'s object and puts it on the wearer's `point`, worn as the
/// item: the records, the root first, or [`None`] for an item whose object
/// nothing holds.
pub(crate) fn wear_item(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    (wearer, mint): (&Wearer<'_>, &dyn Fn() -> uuid::Uuid),
    item: &InventoryItem,
    point: u8,
    now: Instant,
) -> Option<Vec<Object>> {
    let rezzed = crate::world::rez_item(
        world,
        assets,
        (wearer.region_handle, mint),
        item,
        Some(&wearer.at),
        now,
    )?;
    let root = rezzed.first()?.local_id;
    crate::seating::wear(
        world,
        (wearer.identity.agent_id, &wearer.at),
        root,
        (point, None),
        WornItem::Item(item.item_id),
    )
}

/// Files the worn root `root` of the agent's as a new item in its Objects
/// folder — a wear from the world, by the viewer or a script — and announces
/// it over UDP, as both grids do: the records worn as it, the root first, or
/// [`None`] for a root the agent does not wear, or wears as an item its
/// inventory already holds. A script's wear names the item's id already
/// ([`RegionChange::FileWorn`]); the viewer's is minted here.
pub(crate) fn file_worn(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    (identity, mint): (&AvatarIdentity, &dyn Fn() -> uuid::Uuid),
    object_assets: crate::assets::ObjectAssetPolicy,
    sim: &mut SimSession,
    root: RegionLocalObjectId,
    now: Instant,
) -> Option<Vec<Object>> {
    let attached = world
        .objects
        .attached_to(root)
        .filter(|attached| attached.agent == identity.agent_id)
        .cloned()?;
    let named = match attached.item {
        WornItem::Unfiled => None,
        WornItem::Item(item) if sim.agent_inventory().item(item).is_none() => Some(item),
        WornItem::Item(_) | WornItem::Temporary => return None,
    };
    let folder = crate::world::folder_of_type(sim, sl_proto::FolderType::Object)?;
    let linkset = linkset_of(world, root);
    let contents = contents_of(world, &linkset);
    let scripts = linkset
        .iter()
        .map(|member| world.script_instances.save_prim(member.full_id, false))
        .collect();
    let mut item = crate::world::taken_item(
        &linkset,
        folder,
        identity.agent_id,
        mint,
        (
            object_assets,
            world.rez.taken_item_masks,
            world.rez.undescribed_take,
        ),
    );
    if let Some(named) = named {
        item.item_id = named;
    }
    crate::world::store_taken_asset(assets, &item, &linkset, (&contents, scripts), now);
    crate::world::announce_created_items(
        crate::inventory::InventoryAnnouncement::Legacy,
        sim,
        core::slice::from_ref(&item),
        sl_proto::TransactionId::from(uuid::Uuid::nil()),
        now,
    );
    let item_id = item.item_id;
    sim.agent_inventory_mut().insert_item(item);
    let filed = AttachedTo {
        item: WornItem::Item(item_id),
        ..attached
    };
    let _worn = world.objects.wear(root, filed);
    if let Some(mut object) = world.object_mut(root) {
        object.name_value = name_values(WornItem::Item(item_id), object.full_id);
    }
    Some(linkset_of(world, root))
}

/// Takes the agent's worn `roots` off into its inventory: each killed for
/// every viewer (a HUD for the others only where the grid kills it), what it
/// is now saved into its item, and the items `restate` names restated as the
/// grid restates one. A temporary attachment is gone, its item none. What the
/// region's other sessions are to be told.
pub(crate) fn detach(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    identity: &AvatarIdentity,
    sim: &mut SimSession,
    (roots, restate): (&[RegionLocalObjectId], Restate),
    now: Instant,
) -> Vec<RegionChange> {
    let policy = world.attachments;
    let mut changes = Vec::new();
    let mut killed = Vec::new();
    let mut restated = Vec::new();
    for root in roots {
        let Some(attached) = world
            .objects
            .attached_to(*root)
            .filter(|attached| attached.agent == identity.agent_id)
            .cloned()
        else {
            continue;
        };
        let item = match attached.item {
            WornItem::Item(item_id) => sim.agent_inventory().item(item_id).cloned(),
            WornItem::Unfiled | WornItem::Temporary => None,
        };
        if let Some(item) = &item {
            restated.push(item.clone());
        }
        let _off = world.objects.take_off(*root);
        // Until it leaves it stands where it was worn, as nobody's child: what
        // its scripts say is heard from there.
        if let Some(mut object) = world.object_mut(*root) {
            object.parent_id = RegionLocalObjectId(0);
            object.state = 0;
            object.motion.position = attached.at.clone();
        }
        // Its scripts hear it come off — `attach(NULL_KEY)` — and have the
        // region's next tick to answer before it leaves; one with none leaves
        // at once.
        crate::scripts::raise_attach(world, *root, None);
        let leaving = Leaving { root: *root, item };
        if crate::scripts::linkset_scripts(world, *root).is_empty() {
            leaving.leave(world, assets, now);
        } else {
            world.leaving.push(leaving);
        }
        killed.push(*root);
        let hud = sl_proto::AttachmentPoint::from_code(attached.point).is_hud();
        if !hud || policy.hud_off_killed_for_others {
            changes.push(RegionChange::Killed(*root));
        }
    }
    if !killed.is_empty()
        && let Err(error) = sim.send_kill_object(&killed, now)
    {
        tracing::warn!("killing what came off failed: {error}");
    }
    let restated: Vec<InventoryItem> = match restate {
        Restate::Each => restated,
        Restate::Last => restated.pop().into_iter().collect(),
        Restate::None => Vec::new(),
    };
    if !restated.is_empty() {
        crate::world::announce_created_items(
            policy.restated,
            sim,
            &restated,
            sl_proto::TransactionId::from(uuid::Uuid::nil()),
            now,
        );
    }
    changes
}

/// A worn object taken off, waiting out the tick its scripts have to answer
/// `attach(NULL_KEY)` before it leaves the region ([`finish_leaving`]): killed
/// for every viewer already, and shown to no newcomer.
#[derive(Debug, Clone)]
pub(crate) struct Leaving {
    /// Its root.
    root: RegionLocalObjectId,
    /// The item it goes back into, or [`None`] for a temporary attachment,
    /// which is gone.
    item: Option<InventoryItem>,
}

impl Leaving {
    /// Takes it out of the region: what it is now, scripts and all, saved
    /// into its item.
    fn leave(self, world: &mut SceneFixtures, assets: &crate::assets::GridAssets, now: Instant) {
        let linkset = linkset_of(world, self.root);
        let contents = contents_of(world, &linkset);
        let scripts: Vec<_> = linkset
            .iter()
            .map(|member| world.script_instances.save_prim(member.full_id, true))
            .collect();
        if let Some(item) = &self.item {
            crate::world::store_taken_asset(assets, item, &linkset, (&contents, scripts), now);
        }
        for member in &linkset {
            let _removed = world.remove_object(member.local_id);
        }
    }
}

/// Takes out of the region what came off before the tick that just ran
/// ([`Leaving`]): the region's script tick calls it once its scripts have
/// had their turn.
pub(crate) fn finish_leaving(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    now: Instant,
) {
    for leaving in core::mem::take(&mut world.leaving) {
        leaving.leave(world, assets, now);
    }
}

/// Whether `object` is part of something taken off and still waiting to
/// leave ([`Leaving`]): what no viewer is shown again.
#[must_use]
pub(crate) fn is_leaving(world: &SceneFixtures, object: &Object) -> bool {
    let root = world.root_of(object.local_id);
    world.leaving.iter().any(|leaving| leaving.root == root)
}

/// Takes off what the agent wears as `item`, restating nothing
/// (`DetachAttachmentIntoInv`).
pub(crate) fn detach_item(
    world: &mut SceneFixtures,
    assets: &crate::assets::GridAssets,
    identity: &AvatarIdentity,
    sim: &mut SimSession,
    item: InventoryKey,
    now: Instant,
) -> Vec<RegionChange> {
    let Some(root) = worn_as(world, identity.agent_id, item) else {
        return Vec::new();
    };
    detach(world, assets, identity, sim, (&[root], Restate::None), now)
}

/// The linkset whose root is `root`, the root first, each prim with its
/// record as the region holds it.
fn linkset_of(world: &SceneFixtures, root: RegionLocalObjectId) -> Vec<Object> {
    std::iter::once(root)
        .chain(crate::link_sets::children_in_order(world, root))
        .filter_map(|prim| {
            let mut object = world.objects.get(prim)?.clone();
            if let Some(record) = world.properties_of(prim) {
                object.properties = Some(record);
            }
            Some(object)
        })
        .collect()
}

/// What each prim of `linkset` contains.
fn contents_of(world: &SceneFixtures, linkset: &[Object]) -> Vec<TaskInventory> {
    linkset
        .iter()
        .map(|member| {
            world
                .objects
                .contents(member.local_id)
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

/// One worn object on its way to another region with its wearer: the
/// linkset as it stood, what each prim held and its scripts as they stood.
#[derive(Debug, Clone)]
pub(crate) struct Carried {
    /// The linkset, the root first, each with its record.
    linkset: Vec<Object>,
    /// What each prim contains.
    contents: Vec<TaskInventory>,
    /// Each prim's scripts, saved as they left.
    scripts: Vec<BTreeMap<InventoryKey, sl_lsl_runtime::vm::SavedScript>>,
    /// The point it is worn on.
    point: u8,
    /// What it is worn as.
    item: WornItem,
    /// When it left.
    left_at: Instant,
    /// What its scripts are raised as it arrives: `changed` with these bits,
    /// or nothing for 0 — a move that was put back.
    change: i32,
}

/// Takes everything `agent` wears out of the region it is leaving, kill
/// nobody is sent: both live grids carry it, and the region left never kills
/// it. Each prim's id is kept, for a grid on which an object brought back
/// takes it again. On OpenSim's YEngine the scripts first hear their own
/// `changed(32768)`, as they do leaving.
pub(crate) fn pack(
    world: &mut SceneFixtures,
    agent: AgentKey,
    change: i32,
    now: Instant,
) -> Vec<Carried> {
    let mut carried = Vec::new();
    for root in world.objects.worn_by(agent) {
        let Some(attached) = world.objects.attached_to(root).cloned() else {
            continue;
        };
        if world.script_instances.flavour() == sl_lsl_runtime::vm::EngineFlavour::YEngine {
            crate::scripts::raise_changed(
                world,
                root,
                sl_lsl_runtime::vm::OPENSIM_CHANGED_POSITION,
            );
        }
        let linkset = linkset_of(world, root);
        let contents = contents_of(world, &linkset);
        let scripts = linkset
            .iter()
            .map(|member| world.script_instances.save_prim(member.full_id, true))
            .collect();
        let _off = world.objects.take_off(root);
        for member in &linkset {
            let _kept = world.departed_ids.insert(member.full_id, member.local_id);
            let _removed = world.remove_object(member.local_id);
        }
        carried.push(Carried {
            linkset,
            contents,
            scripts,
            point: attached.point,
            item: attached.item,
            left_at: now,
            change,
        });
    }
    carried
}

/// Puts what `agent` brought along back on its avatar in this region, under
/// the same full ids and local ids the region gives them — the ones they had
/// here before, on a grid that keeps them — its scripts brought back as
/// their engine brings a travelling script back, with the `changed` each was
/// packed with. The records, each linkset's root first.
pub(crate) fn unpack(
    world: &mut SceneFixtures,
    (agent, at): (AgentKey, &Vector),
    carried: Vec<Carried>,
    region_handle: sl_wire::RegionHandle,
    now: Instant,
) -> Vec<Object> {
    let keeps = world.attachments.returning_keeps_local_id;
    let avatar = world.avatar_id(agent);
    let mut records = Vec::new();
    for worn in carried {
        let mut ids = Vec::with_capacity(worn.linkset.len());
        for member in &worn.linkset {
            let kept = keeps
                .then(|| world.departed_ids.get(&member.full_id).copied())
                .flatten()
                .filter(|id| world.object_by_local_id(*id).is_none());
            ids.push(kept.unwrap_or_else(|| world.mint_local_id()));
        }
        let Some(root_id) = ids.first().copied() else {
            continue;
        };
        let mut linkset = worn.linkset;
        for (member, id) in linkset.iter_mut().zip(&ids) {
            member.local_id = *id;
            member.region_handle = region_handle;
            member.parent_id = if *id == root_id { avatar } else { root_id };
        }
        world.objects.extend(linkset.iter().cloned());
        let away = now.saturating_duration_since(worn.left_at);
        for ((member, mut contents), mut saved) in
            linkset.iter().zip(worn.contents).zip(worn.scripts)
        {
            let scripts: Vec<InventoryKey> = contents
                .items
                .iter()
                .filter(|item| item.inv_type == sl_proto::InventoryType::Script)
                .map(|item| item.item_id)
                .collect();
            for item in &mut contents.items {
                item.parent_task = member.full_id;
            }
            for item in scripts {
                world.script_instances.travel(
                    member.full_id,
                    item,
                    saved.remove(&item),
                    away,
                    worn.change,
                );
            }
            if world
                .objects
                .set_contents(member.local_id, contents)
                .is_err()
            {
                tracing::error!(
                    local_id = member.local_id.0,
                    "a carried prim is not in the region"
                );
            }
        }
        let _worn = world.objects.wear(
            root_id,
            AttachedTo {
                agent,
                point: worn.point,
                at: at.clone(),
                item: worn.item,
            },
        );
        tracing::debug!(
            ?agent,
            local_ids = ?ids,
            ?avatar,
            "a carried attachment is back on its wearer"
        );
        records.extend(linkset);
    }
    records
}

/// Packs what `agent` wears out of `source`'s region and leaves it waiting
/// in the region at `destination` (the grid's region index) for the arrival
/// there to put back on ([`pack`]); a teleport or a crossing calls it just
/// before it hands the client the destination. Nothing for a region the grid
/// does not have.
///
/// `teleport` says how it goes: the scripts hear `CHANGED_REGION`, with
/// `CHANGED_TELEPORT` for a teleport, as they arrive.
pub(crate) async fn carry(
    core: &crate::runtime::GridCore,
    source: &crate::driver::SharedSim,
    (agent, teleport): (AgentKey, bool),
    destination: usize,
) {
    let Some(region) = core.region(destination) else {
        return;
    };
    let now = source.now();
    let change = if teleport {
        sl_lsl_runtime::vm::CHANGED_REGION | sl_lsl_runtime::vm::CHANGED_TELEPORT
    } else {
        sl_lsl_runtime::vm::CHANGED_REGION
    };
    let carried = source
        .with_state(|state| pack(&mut state.world.lock(), agent, change, now))
        .await;
    if !carried.is_empty() {
        let _left = region.world.lock().luggage.insert(agent, carried);
    }
}

/// Puts back on in `source`'s region what [`carry`] left waiting at
/// `destination` for an arrival that never came: a move that timed out
/// leaves the agent where it was, wearing what it wore.
pub(crate) async fn bring_back(
    core: &crate::runtime::GridCore,
    source: &crate::driver::SharedSim,
    agent: AgentKey,
    destination: usize,
) {
    let Some(region) = core.region(destination) else {
        return;
    };
    let Some(mut carried) = region.world.lock().luggage.remove(&agent) else {
        return;
    };
    // Back where they were: no region was changed.
    for worn in &mut carried {
        worn.change = 0;
    }
    source
        .with_state(|state| {
            let now = source.now();
            let at = crate::chat::agent_position(&state.sim);
            let handle = state.sim.region_handle();
            let mut world = state.world.lock();
            let records = unpack(&mut world, (agent, &at), carried, handle, now);
            drop(world);
            if let Err(error) = crate::world::send_objects(&mut state.sim, &records, now) {
                tracing::warn!("putting back what a failed move carried failed: {error}");
            }
        })
        .await;
}

/// The LSL constant each attachment point is named by in an
/// `AttachmentResources` report, by its code: `ATTACH_RIGHT_PEC` for 30, as
/// both grids name one (`avatar-render-info`, 2026-10-21). Index 0 is no
/// point.
const POINT_NAMES: [&str; 56] = [
    "",
    "ATTACH_CHEST",
    "ATTACH_HEAD",
    "ATTACH_LSHOULDER",
    "ATTACH_RSHOULDER",
    "ATTACH_LHAND",
    "ATTACH_RHAND",
    "ATTACH_LFOOT",
    "ATTACH_RFOOT",
    "ATTACH_BACK",
    "ATTACH_PELVIS",
    "ATTACH_MOUTH",
    "ATTACH_CHIN",
    "ATTACH_LEAR",
    "ATTACH_REAR",
    "ATTACH_LEYE",
    "ATTACH_REYE",
    "ATTACH_NOSE",
    "ATTACH_RUARM",
    "ATTACH_RLARM",
    "ATTACH_LUARM",
    "ATTACH_LLARM",
    "ATTACH_RHIP",
    "ATTACH_RULEG",
    "ATTACH_RLLEG",
    "ATTACH_LHIP",
    "ATTACH_LULEG",
    "ATTACH_LLLEG",
    "ATTACH_BELLY",
    "ATTACH_LEFT_PEC",
    "ATTACH_RIGHT_PEC",
    "ATTACH_HUD_CENTER_2",
    "ATTACH_HUD_TOP_RIGHT",
    "ATTACH_HUD_TOP_CENTER",
    "ATTACH_HUD_TOP_LEFT",
    "ATTACH_HUD_CENTER_1",
    "ATTACH_HUD_BOTTOM_LEFT",
    "ATTACH_HUD_BOTTOM",
    "ATTACH_HUD_BOTTOM_RIGHT",
    "ATTACH_NECK",
    "ATTACH_AVATAR_CENTER",
    "ATTACH_LHAND_RING1",
    "ATTACH_RHAND_RING1",
    "ATTACH_TAIL_BASE",
    "ATTACH_TAIL_TIP",
    "ATTACH_LWING",
    "ATTACH_RWING",
    "ATTACH_FACE_JAW",
    "ATTACH_FACE_LEAR",
    "ATTACH_FACE_REAR",
    "ATTACH_FACE_LEYE",
    "ATTACH_FACE_REYE",
    "ATTACH_FACE_TONGUE",
    "ATTACH_GROIN",
    "ATTACH_HIND_LFOOT",
    "ATTACH_HIND_RFOOT",
];

/// What the `AttachmentResources` capability reports of what `agent` wears in
/// `world`: each worn object that holds a script, grouped by the point it is
/// on — HUDs included — with its id, where it stands, its name, its owner and
/// its resources: on Second Life the memory of its scripts
/// ([`AttachmentPolicy::script_memory`]), and the URLs it holds — none, until
/// a script can hold one (`server-lsl-lib-http-url`). The summary states
/// unlimited memory and 38 URLs available and the sums used, as both grids
/// do (`avatar-render-info`, 2026-10-21, `book/src/gridspec/avatars.md`
/// § Attachment resources).
#[must_use]
pub(crate) fn resources_report(
    world: &SceneFixtures,
    agent: AgentKey,
) -> sl_wire::AttachmentResourcesReport {
    let memory_each = world.attachments.script_memory;
    let mut report = sl_wire::AttachmentResourcesReport::nothing_scripted();
    let mut memory_used = 0_i32;
    let mut locations: BTreeMap<u8, Vec<sl_wire::ScriptedObjectInfo>> = BTreeMap::new();
    for root in world.objects.worn_by(agent) {
        let scripts = crate::scripts::linkset_scripts(world, root).len();
        let (Some(attached), Some(object)) =
            (world.objects.attached_to(root), world.objects.get(root))
        else {
            continue;
        };
        if scripts == 0 {
            continue;
        }
        let memory =
            memory_each.map(|each| each.saturating_mul(i32::try_from(scripts).unwrap_or(i32::MAX)));
        memory_used = memory_used.saturating_add(memory.unwrap_or(0));
        let name = world
            .properties_of(root)
            .map(|properties| properties.name)
            .unwrap_or_default();
        locations
            .entry(attached.point)
            .or_default()
            .push(sl_wire::ScriptedObjectInfo {
                id: object.full_id.uuid(),
                location: sl_types::map::RegionCoordinates::new(
                    attached.at.x,
                    attached.at.y,
                    attached.at.z,
                ),
                name,
                owner: sl_types::key::OwnerKey::Agent(agent),
                resources: sl_wire::ScriptedObjectResources { memory, urls: None },
            });
    }
    report.attachments = locations
        .into_iter()
        .map(|(point, objects)| sl_wire::AttachmentLocation {
            location: POINT_NAMES
                .get(usize::from(point))
                .map_or_else(|| point.to_string(), |name| (*name).to_owned()),
            objects,
        })
        .collect();
    for used in &mut report.summary.used {
        if used.resource_type == "memory" {
            used.amount = memory_used;
        }
    }
    report
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::POINT_NAMES;

    /// Every point's name is the LSL constant of that value.
    #[test]
    fn each_point_is_named_by_its_lsl_constant() -> Result<(), String> {
        for (code, name) in POINT_NAMES.iter().enumerate().skip(1) {
            let constant = sl_lsl_runtime::library::constant(name)
                .ok_or_else(|| format!("{name} is no LSL constant"))?;
            let value = i32::try_from(code).map_err(|error| error.to_string())?;
            assert_eq!(
                constant.value,
                sl_lsl_runtime::library::ConstantValue::Integer(value),
                "{name}"
            );
        }
        Ok(())
    }
}
