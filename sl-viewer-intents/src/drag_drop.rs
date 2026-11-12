//! Drag, drop and open-editor vocabulary.
//!
//! The inventory drags items onto things other features own -- an object's
//! contents, a notecard's body, whatever the cursor is over in the world -- and
//! opens editors it does not implement. The markers, messages and the one
//! command builder that describe those hand-offs live here so neither side has
//! to name the other.

use std::collections::VecDeque;

use bevy::prelude::*;
use sl_client_bevy::{
    AgentKey, AssetType, AssetUpdateLocation, Command, ExperienceKey, ImSessionId,
    InventoryFolderKey, InventoryKey, InventoryType, ItemInfo, NewInventoryItem, ObjectKey,
    Permissions, RestoreItem, ScopedObjectId, ScriptLanguage, ScriptTarget, ScriptUploadLocation,
    SettingsKind, TaskInventoryKey, TransactionId, Uuid, WearableType,
};

/// A description of an item added to a prim's contents, for the pending-add
/// phantom row shown until the server confirms it.
#[derive(Debug, Clone)]
pub struct PendingAdd {
    /// The source item's id (the phantom row's key until reconcile).
    pub item_id: InventoryKey,
    /// The added item's display name.
    pub name: String,
    /// The added item's type-icon glyph.
    pub icon: &'static str,
}

/// A signal that an object's task inventory was mutated from outside this module
/// (a drag-in add resolved by `crate::inventory_drag`), so its cached listing
/// must be reconciled against the server — the same round trip the in-module
/// mutations do inline. `added` carries the dropped items so a "…adding" phantom
/// row can stand in until the server's listing includes them.
#[derive(Message, Debug, Clone)]
pub struct ContentsMutated {
    /// The region-scoped id of the mutated object.
    pub scoped: ScopedObjectId,
    /// The grid-wide key of the mutated object.
    pub full: ObjectKey,
    /// The items added by this mutation (empty for a pure reconcile).
    pub added: Vec<PendingAdd>,
}

/// Add the dropped inventory item to `object`'s task inventory — the drag-in
/// path, called from `crate::inventory_drag` when a drag ends over a contents
/// list. Returns the command to send, or `None` when the source is a folder
/// (task inventory takes single items).
#[must_use]
pub fn contents_drop_command(
    item: &sl_client_bevy::ItemInfo,
    scoped: ScopedObjectId,
    object: ObjectKey,
) -> Option<Command> {
    let inventory_item = item.to_item();
    let restore = RestoreItem::for_task_drop(&inventory_item, object, Uuid::new_v4()).ok()?;
    Some(Command::UpdateTaskInventory {
        target: scoped,
        key: TaskInventoryKey::Item,
        item: Box::new(restore),
    })
}

/// Where the notecard being edited lives — the agent's own inventory, or an
/// in-world object's task inventory. Carried through the editor so Save writes
/// back to the right place (the reference's "opened-from-task" provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotecardSource {
    /// A notecard in the agent's own inventory.
    Agent {
        /// The agent-inventory item.
        item_id: InventoryKey,
    },
    /// A notecard inside an in-world object's task inventory.
    Task {
        /// The object (task) holding the notecard.
        task_id: ObjectKey,
        /// The notecard item within that object's inventory.
        item_id: InventoryKey,
    },
}

impl NotecardSource {
    /// The notecard item's own id, whichever inventory it lives in — the
    /// `notecard-id` a `CopyInventoryFromNotecard` copy names.
    #[must_use]
    pub const fn item_id(self) -> InventoryKey {
        match self {
            Self::Agent { item_id } | Self::Task { item_id, .. } => item_id,
        }
    }

    /// The asset-update location this source saves back to.
    #[must_use]
    pub const fn location(self) -> AssetUpdateLocation {
        match self {
            Self::Agent { item_id } => AssetUpdateLocation::AgentInventory { item_id },
            Self::Task { task_id, item_id } => {
                AssetUpdateLocation::TaskInventory { task_id, item_id }
            }
        }
    }

    /// The prim holding the notecard when it lives in a task inventory, or
    /// `None` for an agent-inventory notecard — the `object-id` a
    /// `CopyInventoryFromNotecard` copy of an embedded item names.
    #[must_use]
    pub const fn object_id(self) -> Option<ObjectKey> {
        match self {
            Self::Agent { .. } => None,
            Self::Task { task_id, .. } => Some(task_id),
        }
    }
}

/// Open the notecard editor on a notecard. Written by the inventory **Open**
/// action (routed here from `crate::inventory_properties`) and by the Object
/// Contents floater's Open (`crate::edit_contents`) for a task-inventory
/// notecard.
#[derive(Message, Debug, Clone)]
pub struct OpenNotecard {
    /// The notecard's name, shown as the floater title.
    pub name: String,
    /// The notecard asset to fetch and show.
    pub asset_id: Uuid,
    /// Whether the notecard is editable (the caller applies the right
    /// permission rule: an agent item's own modify bit, or an object's modify
    /// **and** the item's modify bit for a task notecard).
    pub editable: bool,
    /// Where the notecard lives, so Save writes back to the right place.
    pub source: NotecardSource,
}

/// The wearable creations whose item has not arrived yet, by the transaction
/// each was made with.
///
/// A new wearable, or a wearable's Save As, is made the reference's way
/// ([`new_wearable_command`]): its bytes go up over `AssetUploadRequest` under a
/// fresh transaction and a `CreateInventoryItem` naming that transaction asks
/// for the item, which the simulator stamps with the wearable slot itself. The
/// item's `UpdateCreateInventoryItem` echoes the transaction, and so does the
/// upload's completion, so each answer names the creation it belongs to — no
/// arrival-order guessing, and nobody else's item can be taken for it.
///
/// One registry for the whole viewer rather than one per feature: the
/// producers (the inventory's New Clothes / New Body Parts creators and the
/// appearance editor's Save As) only enqueue, and the inventory owns the single
/// consumer, which also tells the resident when an upload is refused — the
/// reference's `CannotSaveToAssetStore`, raised for both.
#[derive(Resource, Debug, Default)]
pub struct PendingItemCreations {
    /// The in-flight creations, oldest first.
    pending: Vec<PendingItemCreation>,
}

/// One in-flight wearable creation.
#[derive(Debug, Clone)]
pub struct PendingItemCreation {
    /// The transaction its upload and its create both name.
    pub transaction: TransactionId,
    /// The name the item is made under, for the notice a refusal raises.
    pub name: String,
}

/// What became of one wearable creation, published by the single consumer of
/// [`PendingItemCreations`] so whoever started it can say so.
///
/// Without this an upload that mints an item has no outcome at all on the
/// surface that asked for it: the appearance editor's Save As used to announce
/// "Saved a copy to inventory." the instant the bytes were queued, which is a
/// claim about something that had not happened yet and might not.
#[derive(Message, Debug, Clone, Copy)]
pub struct ItemCreationFinished {
    /// The transaction of the creation this answers.
    pub transaction: TransactionId,
    /// The fresh item, or `None` when the upload failed.
    pub item: Option<InventoryKey>,
}

impl PendingItemCreations {
    /// Register a creation made with `transaction`, so the item that names it
    /// (or the refusal of its upload) is matched to it.
    pub fn enqueue(&mut self, transaction: TransactionId, name: &str) {
        self.pending.push(PendingItemCreation {
            transaction,
            name: name.to_owned(),
        });
    }

    /// Take the creation made with `transaction`, if one is in flight.
    pub fn take(&mut self, transaction: TransactionId) -> Option<PendingItemCreation> {
        let index = self
            .pending
            .iter()
            .position(|creation| creation.transaction == transaction)?;
        Some(self.pending.remove(index))
    }
}

/// The command that makes a new wearable item of `slot` named `name` in
/// `folder`, holding the wearable asset `data`, under the freshly minted
/// `transaction` — the reference's `LLAgentWearables::createWearable`, which
/// stores the asset under the wearable's transaction (`saveNewAsset`) and
/// creates the item naming it (`create_inventory_wearable`).
///
/// Not `NewFileAgentInventory`: Second Life takes clothing and body parts
/// there only at a price (L$ 10, refused as `Upload_UploadPriceDiffers` when a
/// client states 0; `book/src/gridspec/assets.md` § Uploads), and no viewer
/// charges for a new shirt. The slot rides the create's subtype byte, which the
/// simulator stamps into the item's flags.
///
/// A function because two surfaces make wearables — the inventory's create
/// menus and the appearance editor's Save As — and two copies would be two
/// places for the asset class or the permission mask to drift.
#[must_use]
pub fn new_wearable_command(
    folder: InventoryFolderKey,
    slot: WearableType,
    name: &str,
    data: Vec<u8>,
    transaction: TransactionId,
) -> Command {
    Command::CreateInventoryItemWithAsset {
        item: Box::new(NewInventoryItem {
            folder_id: folder,
            transaction_id: transaction.get(),
            next_owner_mask: Permissions::DEFAULT_NEXT_OWNER,
            asset_type: if slot.is_body_part() {
                AssetType::Bodypart
            } else {
                AssetType::Clothing
            },
            inv_type: InventoryType::Wearable,
            wearable_type: slot,
            name: name.to_owned(),
            description: String::new(),
        }),
        data,
    }
}

/// The **settings** items asked of the simulator and not yet seen back, oldest
/// first.
///
/// A settings item is minted by `CreateInventoryItem` rather than by an upload —
/// the simulator authors the default asset for the kind and stamps the subtype
/// byte — and its `UpdateCreateInventoryItem` reply names the item but not who
/// asked for it — a create with no upload behind it carries a nil transaction,
/// so unlike a wearable's ([`PendingItemCreations`]) it has nothing to be
/// matched by. So this is the viewer's **one** queue for those replies: two
/// queues watching the same untagged reply stream would each pop on
/// the other's creation, and a Save As would write its body onto somebody else's
/// fresh item.
///
/// One ordered queue is also what makes each *consumer's* own bookkeeping sound.
/// The library window counts the creations it asked for and claims that many
/// [`SettingsItemCreated`]s; so does the editor. Because every creation passes
/// through here in order, "the next one is mine" is a true statement for both.
#[derive(Resource, Debug, Default)]
pub struct PendingSettingsCreations {
    /// The in-flight creations, oldest first.
    queue: VecDeque<PendingSettingsCreation>,
}

/// One in-flight settings creation: which kind, and what to do with the item
/// when it arrives.
#[derive(Debug, Clone)]
pub struct PendingSettingsCreation {
    /// The kind asked for — carried rather than read back off the reply,
    /// because it is what the user chose whatever the simulator stamped.
    pub kind: SettingsKind,
    /// The encoded asset to write onto the fresh item, for a creator that has a
    /// body to store (the editors' **Save As**).
    ///
    /// `None` for New Sky / New Water, where the *point* is the default asset
    /// the simulator authors — `LLSettingsVOBase::onInventoryItemCreated` says
    /// so outright when it is called with no settings: "no need to upload
    /// asset".
    pub body: Option<Vec<u8>>,
}

impl PendingSettingsCreations {
    /// Enqueue a creation, so the reply that names its item is matched to it.
    pub fn enqueue(&mut self, kind: SettingsKind, body: Option<Vec<u8>>) {
        self.queue.push_back(PendingSettingsCreation { kind, body });
    }

    /// Take the oldest in-flight creation — the reply that just landed is its
    /// own, since the simulator answers in the order it was asked.
    pub fn take_next(&mut self) -> Option<PendingSettingsCreation> {
        self.queue.pop_front()
    }
}

/// A settings item the simulator has created for us, published once its reply
/// has been matched to the request that asked for it.
///
/// Consumers read this rather than the raw
/// `SlSessionEvent::InventoryItemCreated`, so that "was this one mine?" is
/// answered once, in queue order, instead of separately (and racily) by each
/// window.
#[derive(Message, Debug, Clone)]
pub struct SettingsItemCreated {
    /// The fresh item.
    pub item: InventoryKey,
    /// The folder it landed in.
    pub folder: InventoryFolderKey,
    /// The kind that was asked for.
    pub kind: SettingsKind,
    /// Whether a body was written onto it (a **Save As**) rather than the
    /// simulator's default asset being kept (a New Sky / New Water).
    pub authored: bool,
}

/// Open the settings editor on an EEP **settings** inventory item — the sky
/// editor or the water editor, chosen by the item's own
/// [`SettingsKind`] flag. Written by the
/// inventory's Open / Edit actions.
///
/// Flat fields rather than an inventory `ItemInfo` because the editor lives
/// below the inventory in the crate graph, and because a *freshly created*
/// settings item is opened straight from its upload reply, which carries the
/// ids and nothing else.
#[derive(Message, Debug, Clone)]
pub struct OpenSettingsEditor {
    /// The item's name, shown in the editor's name field and saved with the
    /// asset.
    pub name: String,
    /// The settings asset to fetch and edit.
    pub asset_id: Uuid,
    /// The inventory item the asset belongs to, so Save writes back onto it.
    pub item_id: InventoryKey,
    /// The folder it lives in, where a Save As puts the copy.
    pub folder_id: InventoryFolderKey,
    /// Which editor this is — a sky item opens the sky editor, a water item the
    /// water editor.
    pub kind: SettingsKind,
    /// Whether the item may be saved back onto (its owner modify bit).
    pub editable: bool,
}

/// Open the **settings picker** on one field: a chooser over the settings assets
/// in inventory of one [`SettingsKind`], answering with [`SettingsPicked`].
///
/// The reference's `LLFloaterSettingsPicker`, which the region / parcel
/// environment panel and the day-cycle editor summon. The kind is fixed by the
/// opener (`setSettingsFilter`), because a water field being handed a day cycle
/// is not a choice the user should be able to make.
#[derive(Message, Debug, Clone)]
pub struct OpenSettingsPicker {
    /// The widget (or panel) the reply is tagged back to.
    pub requester: Entity,
    /// Which field is being picked for — shown in the window's subtitle, so two
    /// consecutive picks say which one is being answered.
    ///
    /// Owned rather than `&'static str` for the same reason
    /// [`crate::OpenTexturePicker::field`] is: a table-driven panel names its controls
    /// at spawn time.
    pub field: Box<str>,
    /// Which kind of settings asset may be chosen.
    pub kind: SettingsKind,
    /// The settings **asset** the field currently holds, opened on and restored
    /// by Cancel; `None` for a field holding nothing yet.
    pub current: Option<Uuid>,
}

/// One settings asset a picker can answer with — the item the user sees and the
/// asset behind it, which are two different ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickedSettings {
    /// The inventory item chosen (a link, when a link is what the list held).
    pub item: InventoryKey,
    /// The settings asset behind it — what a panel publishes or applies.
    pub asset_id: Uuid,
    /// Its name, which the reference carries alongside the asset id in every
    /// environment update so a panel can show what it is holding.
    pub name: String,
}

/// The settings asset a picker returned, tagged back to the
/// [`requester`](Self::requester).
///
/// Emitted **non-final** on each selection so a consumer can live-preview it,
/// once on **OK** with [`final_pick`](Self::final_pick) true, and on **Cancel**
/// as the asset the picker opened on (a revert) — the same protocol
/// [`crate::TexturePicked`] follows.
#[derive(Message, Debug, Clone)]
pub struct SettingsPicked {
    /// The widget that opened the picker.
    pub requester: Entity,
    /// The chosen asset, or `None` when the picker opened on nothing and a
    /// Cancel put that back.
    pub chosen: Option<PickedSettings>,
    /// Whether this is the committed choice (**OK**) rather than a live-preview
    /// or revert update.
    pub final_pick: bool,
}

/// Marks the notecard editor floater as an **inventory drop target**: dropping
/// an inventory item on it while [`editable`](Self::editable) adds the item as
/// an embedded item. `crate::inventory_drag` walks up from the hovered node to
/// find it; `open_notecard` keeps [`editable`](Self::editable) in step with
/// the notecard currently shown (a no-modify notecard rejects drops).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct NotecardDropTarget {
    /// Whether the notecard currently shown accepts an added embedded item.
    pub editable: bool,
}

/// Marks a UI node as taking **one** dropped inventory item of the
/// [`accepts`](Self::accepts) type while [`enabled`](Self::enabled) — the
/// reference's `ACCEPT_YES_COPY_SINGLE` panels, such as the estate covenant's
/// notecard drop. `crate::inventory_drag` walks up from the hovered node to find
/// it and answers with an [`ItemDropped`] naming the node; a drag of several
/// items, of another type, or onto a disabled target is swallowed, as the
/// reference's `ACCEPT_NO` is.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemDropTarget {
    /// The one inventory type the target takes.
    pub accepts: InventoryType,
    /// Whether it takes one now (the agent's rights, say).
    pub enabled: bool,
}

impl ItemDropTarget {
    /// The item of `dragged` this target takes, if it takes the drag at all:
    /// exactly one item, of the accepted type, while enabled.
    #[must_use]
    pub fn accepts_drag<'a>(&self, dragged: &[&'a ItemInfo]) -> Option<&'a ItemInfo> {
        match dragged {
            [item] if self.enabled && item.inv_type == self.accepts => Some(item),
            _ => None,
        }
    }
}

/// An inventory item dropped onto an [`ItemDropTarget`] that took it.
#[derive(Message, Debug, Clone)]
pub struct ItemDropped {
    /// The node carrying the [`ItemDropTarget`].
    pub target: Entity,
    /// The dropped item (a link already resolved to what it points at).
    pub item: ItemInfo,
}

/// Where the script being edited lives — the agent's own inventory, or an
/// in-world object's task inventory. Carried through the editor so Save writes
/// back to the right capability (the reference's "opened-from-task" provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptSource {
    /// A script in the agent's own inventory (`UpdateScriptAgent`).
    Agent {
        /// The agent-inventory item.
        item_id: InventoryKey,
    },
    /// A script inside an in-world object's task inventory (`UpdateScriptTask`).
    Task {
        /// The object (task) holding the script.
        task_id: ObjectKey,
        /// The script item within that object's inventory.
        item_id: InventoryKey,
    },
}

impl ScriptSource {
    /// Whether this source is an in-world object's task inventory (which carries
    /// a run state the save must preserve).
    #[must_use]
    pub const fn is_task(self) -> bool {
        matches!(self, Self::Task { .. })
    }

    /// The upload location this source saves back to, carrying `running` for a
    /// task script (`is_script_running`) and the `experience` it was compiled
    /// under. A save that names no experience takes the script out of the one
    /// it had, so the editor passes back what the grid said the script is
    /// under. An agent-inventory script has neither.
    #[must_use]
    pub const fn location(
        self,
        running: bool,
        experience: Option<ExperienceKey>,
    ) -> ScriptUploadLocation {
        match self {
            Self::Agent { item_id } => ScriptUploadLocation::AgentInventory { item_id },
            Self::Task { task_id, item_id } => ScriptUploadLocation::TaskInventory {
                task_id,
                item_id,
                running,
                experience,
            },
        }
    }
}

/// Open the script editor on a script. Written by the inventory **Open** action
/// (routed here from `crate::inventory_properties`) and by the Object Contents
/// The compile backend to request for a script's [`ScriptLanguage`]. Second Life
/// honours the token (Mono is its LSL default); OpenSim ignores it and reads the
/// language from a source-header comment, so an unknown backend does no harm.
#[must_use]
pub const fn target_for(language: Option<ScriptLanguage>) -> ScriptTarget {
    match language {
        Some(ScriptLanguage::Luau) => ScriptTarget::Luau,
        // LSL, or an item whose subtype byte we do not recognise.
        _ => ScriptTarget::Mono,
    }
}

/// floater's Open (`crate::edit_contents`) for a task-inventory script.
#[derive(Message, Debug, Clone)]
pub struct OpenScript {
    /// The script's name, shown as the floater title.
    pub name: String,
    /// The script source asset to fetch and show.
    pub asset_id: Uuid,
    /// Whether the script is editable (the caller applies the right permission
    /// rule: an agent item's own modify bit, or an object's modify **and** the
    /// item's modify bit for a task script).
    pub editable: bool,
    /// Where the script lives, so Save writes back to the right place.
    pub source: ScriptSource,
    /// The compile backend to request, derived from the item's language.
    pub target: ScriptTarget,
}

/// The in-world object an inventory drag is currently hovering, if it accepts the
/// drop — set by `crate::inventory_drag` each frame while a drag is active and
/// consumed by `apply_drag_hover_highlight` to draw the accept / foreign
/// outline (the reference's `highlightObjectAndFamily` during a drag).
#[derive(Resource, Debug, Default)]
pub struct DragHoverHighlight {
    /// The hovered object's root render entity and whether it is foreign (not
    /// owned, so the outline is red), or `None` when nothing droppable is hovered.
    pub hover: Option<DragHover>,
}

/// One drag-hover target: the object's root render entity and its ownership tint.
#[derive(Debug, Clone, Copy)]
pub struct DragHover {
    /// The hovered object's root render entity (a `SceneObject`).
    pub root: Entity,
    /// Whether the object is **foreign** (not owned / not modifiable but accepts
    /// the drop) — drawn red rather than the green accept colour.
    pub foreign: bool,
}

/// Whether an inventory drag is in flight, so the world half should keep a GPU
/// pick alive under the cursor — set by `crate::inventory_drag` as a drag
/// starts and ends, read by `crate::gpu_pick`'s drag driver.
///
/// The flag exists because the two halves of a drag-to-world drop sit on
/// opposite sides of the dependency graph: the panel that starts the drag is a
/// feature crate, the picker that resolves what is under the cursor is the
/// world tier, and neither may depend on the other. So the request and the
/// answer ([`DragWorldPick`]) both live here, in the vocabulary layer beneath
/// both — the same seam [`DragHoverHighlight`] already uses.
#[derive(Resource, Debug, Default)]
pub struct DragPickActive {
    /// Whether a drag is running right now.
    pub active: bool,
}

/// What the latest GPU pick found under the cursor during an active drag —
/// the world half of an inventory drop's resolution. Filled by
/// `crate::gpu_pick`'s drag driver while [`DragPickActive`] is set, read by
/// `crate::inventory_drag` at `DragEnd` and by the hover outline every frame.
#[derive(Resource, Debug, Default)]
pub struct DragWorldPick {
    /// The latest resolved world hit, `None` when the pick missed (sky) or no
    /// drag is active.
    pub hit: Option<DragPickHit>,
}

/// One resolved drag-time world hit.
#[derive(Debug, Clone, Copy)]
pub enum DragPickHit {
    /// An avatar's drawn pixels (a worn rigged submesh drops onto its wearer
    /// too, matching the old body pick).
    Avatar(AgentKey),
    /// An object face: the face's mesh entity (for the linkset walk) and the
    /// struck world point (the rez ray's end).
    Object {
        /// The face's mesh entity.
        entity: Entity,
        /// The struck world point.
        world_point: Vec3,
    },
    /// Bare terrain — or water, which the old first-hit ray also treated as a
    /// rez surface.
    Ground {
        /// The struck world point.
        world_point: Vec3,
    },
}

/// A request to start an **ad-hoc conference** with several residents, or to
/// invite more people into one that is already open — the reference's
/// `LLAvatarActions::startConference` (`llavataractions.cpp:423`), and the one
/// verb every multi-selection of avatars in this viewer routes to: the radar's
/// multi-row *IM*, the People panel's Friends list, and the inventory's
/// *Start Conference Chat* on calling cards.
///
/// The list is taken as the user picked it — the handler drops our own agent
/// and any repeats, and a list that leaves **one** resident opens a plain
/// one-to-one IM instead (what the reference's `Avatar.IM` does by count), so a
/// caller never has to branch on how many rows are selected.
#[derive(Message, Debug, Clone)]
pub struct StartConference {
    /// The residents to invite.
    pub agents: Vec<AgentKey>,
    /// The conference to invite them **into**, or `None` to start a new one.
    /// Inviting into an open conference is the same wire request with the same
    /// session id (the reference's "Add participants" on an IM floater).
    pub into: Option<ImSessionId>,
}

impl StartConference {
    /// Start a fresh conference with `agents`.
    #[must_use]
    pub const fn with(agents: Vec<AgentKey>) -> Self {
        Self { agents, into: None }
    }

    /// Invite `agents` into the already-open conference `session`.
    #[must_use]
    pub const fn adding(session: ImSessionId, agents: Vec<AgentKey>) -> Self {
        Self {
            agents,
            into: Some(session),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::target_for;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{ScriptLanguage, ScriptTarget};

    /// The compile backend follows the item's recorded language, defaulting to
    /// Mono (SL's LSL default) for LSL or an unrecognised subtype.
    #[test]
    fn target_follows_language() {
        assert_eq!(target_for(Some(ScriptLanguage::Luau)), ScriptTarget::Luau);
        assert_eq!(target_for(Some(ScriptLanguage::Lsl)), ScriptTarget::Mono);
        assert_eq!(target_for(None), ScriptTarget::Mono);
    }
}
