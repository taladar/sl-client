//! The **Content tab** of the Build Tools floater and the standalone
//! **Object Contents** floater (`viewer-prim-inventory-editing`): the list of
//! items inside a prim's task inventory, and the actions that add / remove /
//! rename them.
//!
//! # The two surfaces
//!
//! Both windows show the *same* thing — a prim's task inventory
//! ([`TaskInventoryItem`]s) as a virtualized list ([`crate::virtual_list`]) — so
//! the row pool, the model cache, and the fetch driver are shared and each
//! surface is just a [`ContentsSurface`] tag on its viewport:
//!
//! - [`ContentsSurface::BuildTab`] — docked into the Build floater's Content
//!   tab page ([`crate::edit_tool::BuildTabPages::content`]). It follows the
//!   build selection: the currently-selected prim, which in *edit linked parts*
//!   mode ([`EditToolState::edit_linked`]) is an individual linkset member and
//!   otherwise the linkset root. It offers the editing actions (new script,
//!   rename, remove, drag-in add) gated by permission.
//! - [`ContentsSurface::OpenFloater`] — a separate window opened by the object
//!   pie's **Open** action (the reference viewer's `LLFloaterOpenObject`). It
//!   lists a picked object's contents and offers **Copy To Inventory** /
//!   **Copy And Wear**, which move the contents into the agent's inventory.
//!
//! # The cache — why cycling linkset parts stays instant
//!
//! Fetching a prim's inventory is a round trip (a `RequestTaskInventory`, then a
//! legacy `Xfer` download of the listing — see [`Command::FetchTaskInventory`]),
//! so it can be slow. [`TaskInventoryCache`] keeps every fetched listing keyed by
//! the prim's [`ObjectKey`] for the whole session, so stepping the ◀ ▶ linked-part
//! nav through a linkset re-shows an already-loaded prim's contents with no wire
//! traffic. A prim is fetched once on first view (or on an explicit **Refresh**);
//! the simulator's [`serial`](TaskInventoryReply::serial) is stored so a future
//! staleness check is possible.
//!
//! # Permissions — object vs. content
//!
//! The reference distinguishes the **object's** permissions (`permModify` /
//! `permYouOwner` / `flagAllowInventoryAdd`) from each **content item's** own
//! permission masks, and gates each action on the right one
//! ([`ContentsPerms`]):
//!
//! - **Add** (drag-in / new script): the object is modifiable **or** flagged
//!   "allow anyone to add inventory" ([`ObjectState::agent_allows_inventory_drop`]),
//!   the one documented exception to needing object modify.
//! - **Remove**: offered when you can modify **or own** the object
//!   ([`ContentsPerms::can_remove_menu`]), but only actually applied with object
//!   **modify** — an owner-without-modify gets the reference's
//!   "can't modify content in a no-modify object" notice.
//! - **Rename**: offered when you can modify **or own** the object, but only
//!   applied when the **item** itself is modifiable (its owner mask carries
//!   `MODIFY`) — the reference's two-level `renameItem` check.
//!
//! Reference (Firestorm, read-only): `llpanelcontents`,
//! `llpanelobjectinventory`, `llfloateropenobject`.

use std::collections::HashMap;

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    Command, FolderType, InventoryKey, InventoryType, ObjectKey, Permissions, RestoreItem,
    RezScriptParams, ScopedObjectId, SlCommand, SlEvent, SlIdentity, SlSessionEvent,
    TaskInventoryItem, TaskInventoryKey, TaskInventoryReply, Uuid,
};

use crate::edit_tool::{BuildTabPages, LABEL_CLASS, TOOL_FONT_SIZE, VALUE_CLASS};
use crate::floater::{FloaterCaps, FloaterHandle, FloaterSpec, spawn_floater};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::input_context::InputContext;
use crate::inventory::{InventoryModel, item_icon};
use crate::objects::ObjectState;
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_font::UiFont;
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::virtual_list::{VirtualList, VirtualRow, VirtualViewport};
use crate::world_api::EditToolState;
use crate::world_api::LocalChatNotice;
use crate::world_api::SelectionSet;

/// The uniform height of a contents row, in logical pixels (matches the
/// inventory list's row metric so the two read the same).
const ROW_HEIGHT: f32 = 22.0;

/// The contents row font size, in logical pixels.
const ROW_FONT_SIZE: f32 = 13.0;

/// The minimum icon-column width, in logical pixels.
const ICON_COL_WIDTH: f32 = 18.0;

/// The base tab-index for the Content-tab controls (after the transform fields,
/// whose block starts at 21 in [`crate::edit_tool`]).
const CONTENTS_TAB_INDEX: i32 = 60;

// ---------------------------------------------------------------------------
// Which surface a viewport / view belongs to
// ---------------------------------------------------------------------------

/// Which of the two contents windows a viewport, view, or selection belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentsSurface {
    /// The Build floater's Content tab (editing, follows the build selection).
    BuildTab,
    /// The standalone Object Contents floater (the reference's
    /// `LLFloaterOpenObject`; copy-to-inventory).
    OpenFloater,
}

// ---------------------------------------------------------------------------
// The per-object task-inventory cache
// ---------------------------------------------------------------------------

/// One prim's cached task inventory.
#[derive(Debug, Default)]
struct ContentsEntry {
    /// The simulator's contents serial for the cached listing, if a reply has
    /// been seen — it increments on every change, so a future staleness check
    /// can compare it.
    serial: Option<i16>,
    /// The parsed items (empty when the prim's inventory is empty).
    items: Vec<TaskInventoryItem>,
    /// Whether a fetch is in flight, so the fetch driver does not re-request.
    fetching: bool,
    /// Whether a listing has actually arrived (distinguishes "known empty" from
    /// "not fetched yet").
    loaded: bool,
}

/// The session-lifetime cache of every fetched prim task inventory, keyed by the
/// prim's grid-wide [`ObjectKey`]. See the [module docs](self#the-cache).
#[derive(Resource, Debug, Default)]
pub(crate) struct TaskInventoryCache {
    /// The per-object entries.
    entries: HashMap<ObjectKey, ContentsEntry>,
}

impl TaskInventoryCache {
    /// The cached entry for `task`, if any.
    fn get(&self, task: &ObjectKey) -> Option<&ContentsEntry> {
        self.entries.get(task)
    }

    /// Mark `task` as having a fetch in flight, creating its entry if new.
    /// Returns whether a fetch should actually be sent (`false` if one is
    /// already in flight).
    fn begin_fetch(&mut self, task: ObjectKey) -> bool {
        let entry = self.entries.entry(task).or_default();
        if entry.fetching {
            return false;
        }
        entry.fetching = true;
        true
    }

    /// Store a freshly parsed listing for `task`.
    fn store(&mut self, task: ObjectKey, serial: i16, items: Vec<TaskInventoryItem>) {
        let entry = self.entries.entry(task).or_default();
        entry.serial = Some(serial);
        entry.items = items;
        entry.fetching = false;
        entry.loaded = true;
    }

    /// Record an empty task inventory (a reply whose Xfer filename was empty, so
    /// no `TaskInventoryContents` follows).
    fn store_empty(&mut self, task: ObjectKey, serial: i16) {
        let entry = self.entries.entry(task).or_default();
        entry.serial = Some(serial);
        entry.items.clear();
        entry.fetching = false;
        entry.loaded = true;
    }

    /// Mark a cached entry stale after we sent a mutation, so it is re-fetched
    /// and reconciled against the **server's** authoritative listing — *without*
    /// clearing the current items, so the list keeps showing the last-known-good
    /// contents (no "loading" flash) until the reconcile reply lands.
    ///
    /// We deliberately do **not** mutate the cached items optimistically: the
    /// cache stays a faithful mirror of the last listing the simulator sent, so a
    /// mutation that the simulator rejects (a permission the client misjudged, a
    /// checksum SL refuses) simply leaves the re-fetched listing unchanged — the
    /// item visibly stays / keeps its old name instead of the cache silently
    /// drifting away from the server. A `fetching` entry that is still `loaded`
    /// is surfaced as "updating…" so the user can see a reconcile is in flight.
    fn mark_stale(&mut self, task: &ObjectKey) {
        if let Some(entry) = self.entries.get_mut(task) {
            entry.fetching = true;
        }
    }
}

// ---------------------------------------------------------------------------
// Permissions: object vs. content
// ---------------------------------------------------------------------------

/// The object-level permissions that gate the contents actions (see the
/// [module docs](self#permissions--object-vs-content)).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ContentsPerms {
    /// Whether this agent may modify the holding object (`permModify`).
    can_modify: bool,
    /// Whether this agent owns the holding object (`permYouOwner`).
    owns: bool,
    /// Whether the object lets anyone add inventory (`flagAllowInventoryAdd`).
    allows_drop: bool,
}

impl ContentsPerms {
    /// Resolve the object-level permissions of `scoped` from the object state.
    fn resolve(objects: &ObjectState, scoped: &ScopedObjectId) -> Self {
        Self {
            can_modify: objects.agent_can_modify(scoped),
            owns: objects.agent_owns(scoped),
            allows_drop: objects.agent_allows_inventory_drop(scoped),
        }
    }

    /// Whether an item may be **added** (drag-in / new script): object modify, or
    /// the "allow anyone to add inventory" flag.
    pub(crate) const fn can_add(self) -> bool {
        self.can_modify || self.allows_drop
    }

    /// Whether the **Remove** action should be offered (enabled) — object modify
    /// or ownership. The actual removal still needs [`can_modify`](Self::can_modify).
    const fn can_remove_menu(self) -> bool {
        self.can_modify || self.owns
    }

    /// Whether the **Rename** action should be offered (enabled) — object modify
    /// or ownership. The actual rename still needs the item's own modify bit.
    const fn can_rename_menu(self) -> bool {
        self.can_modify || self.owns
    }
}

/// Whether a content **item** is itself modifiable — its owner permission mask
/// carries `MODIFY`, the reference's `allowOperation(PERM_MODIFY, item perms)`
/// for the common owned-content case. The rename commit checks this.
const fn item_modifiable(item: &TaskInventoryItem) -> bool {
    item.permissions.owner.contains(Permissions::MODIFY)
}

// ---------------------------------------------------------------------------
// The per-surface flattened view
// ---------------------------------------------------------------------------

/// The per-item pending state of a display row while a mutation the client sent
/// awaits the server's authoritative confirmation (see [`PendingMutations`]). A
/// pending row is greyed, shows its state as a suffix, and cannot be re-edited
/// until the server reconciles it — so unrelated items can still be edited
/// (batched) without racing a change already in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowState {
    /// Confirmed / unchanged — editable.
    Normal,
    /// A rename is in flight (shows the new name + "…refreshing").
    Refreshing,
    /// A removal is in flight (still shown + "…deleting").
    Deleting,
    /// An add is in flight — a phantom row for an item not yet in the server's
    /// listing (shows the name + "…adding").
    Adding,
}

impl RowState {
    /// Whether the row is awaiting confirmation (greyed, un-editable).
    const fn is_pending(self) -> bool {
        !matches!(self, Self::Normal)
    }

    /// The Fluent key of this state's suffix, or `None` for a normal row.
    const fn suffix_key(self) -> Option<&'static str> {
        match self {
            Self::Normal => None,
            Self::Refreshing => Some("build-content-state-refreshing"),
            Self::Deleting => Some("build-content-state-deleting"),
            Self::Adding => Some("build-content-state-adding"),
        }
    }
}

/// One display row of a contents list.
#[derive(Debug, Clone)]
struct ContentsRow {
    /// The task inventory item id (addresses remove / rename).
    item_id: InventoryKey,
    /// The item name (the pending new name for a row being renamed).
    name: String,
    /// The type-icon glyph.
    icon: &'static str,
    /// The row's pending-mutation state (normal unless a client edit is in
    /// flight).
    state: RowState,
}

/// The flattened, ready-to-bind view of one surface's current target.
#[derive(Debug, Default)]
struct ContentsSurfaceView {
    /// The target prim (region-scoped id + grid-wide key), or `None` when the
    /// surface shows nothing.
    target: Option<(ScopedObjectId, ObjectKey)>,
    /// The target object's name (for the floater title / count line).
    name: String,
    /// The display rows.
    rows: Vec<ContentsRow>,
    /// The resolved object-level permissions of the target.
    perms: ContentsPerms,
    /// Whether the target's listing has never loaded and is still being fetched
    /// (the list shows a "loading" placeholder). A **reconcile** re-fetch over an
    /// already-loaded listing does not set this — its pending items carry their
    /// own per-row [`RowState`] instead, so the last-good contents stay shown.
    loading: bool,
}

/// Both surfaces' flattened views, rebuilt from the selection / open-target and
/// the cache each time either changes.
#[derive(Resource, Debug, Default)]
struct ContentsViews {
    /// The Build-tab surface's view.
    build: ContentsSurfaceView,
    /// The Open-floater surface's view.
    open: ContentsSurfaceView,
}

impl ContentsViews {
    /// The view for `surface`.
    const fn view(&self, surface: ContentsSurface) -> &ContentsSurfaceView {
        match surface {
            ContentsSurface::BuildTab => &self.build,
            ContentsSurface::OpenFloater => &self.open,
        }
    }
}

/// Which item is selected in each surface's list (for rename / remove).
#[derive(Resource, Debug, Default)]
struct ContentsSelection {
    /// The Build-tab surface's selected item.
    build: Option<InventoryKey>,
    /// The Open-floater surface's selected item.
    open: Option<InventoryKey>,
}

impl ContentsSelection {
    /// The selected item for `surface`.
    const fn get(&self, surface: ContentsSurface) -> Option<InventoryKey> {
        match surface {
            ContentsSurface::BuildTab => self.build,
            ContentsSurface::OpenFloater => self.open,
        }
    }

    /// Set the selected item for `surface`.
    const fn set(&mut self, surface: ContentsSurface, item: Option<InventoryKey>) {
        match surface {
            ContentsSurface::BuildTab => self.build = item,
            ContentsSurface::OpenFloater => self.open = item,
        }
    }
}

/// How long after a click a second click on the **same** item still counts as a
/// double-click (the reference's open gesture), in seconds.
const DOUBLE_CLICK_SECONDS: f32 = 0.4;

/// The most recent contents-row click, so a second click on the same item
/// within [`DOUBLE_CLICK_SECONDS`] is recognised as a double-click Open.
#[derive(Resource, Debug, Default)]
struct ContentsLastClick {
    /// The item clicked, and the time (seconds since startup) it was clicked.
    last: Option<(InventoryKey, f32)>,
}

// ---------------------------------------------------------------------------
// Pending mutations (per-item optimistic-but-tracked state)
// ---------------------------------------------------------------------------

/// One item's in-flight mutation, awaiting the server's authoritative listing.
#[derive(Debug, Clone)]
enum PendingKind {
    /// A rename is in flight; carries the intended new name (shown until the
    /// server confirms — or reverts — it).
    Renaming(String),
    /// A removal is in flight (the item is still shown, flagged, until the
    /// server drops it).
    Deleting,
    /// An add is in flight; carries the item's name + icon, so a phantom row can
    /// stand in until the added item appears in the server's listing.
    Adding {
        /// The added item's display name.
        name: String,
        /// The added item's type-icon glyph.
        icon: &'static str,
    },
}

/// The per-object, per-item mutations the client has sent but the server has not
/// yet confirmed — the batching / conflict-avoidance model the user asked for.
///
/// A mutation records a pending entry keyed by `(object, item)`; that item is
/// then shown greyed with a "…refreshing" / "…deleting" / "…adding" suffix and
/// **cannot be re-edited** until it clears, so a second edit never races the
/// first. Items the user has **not** touched stay fully editable, so several
/// independent renames / removes can be queued at once.
///
/// Every pending entry for an object is cleared the moment a fresh authoritative
/// listing for it arrives ([`ingest_task_inventory`]): that listing *is* the
/// truth, so the overlay is dropped and the real state (whether the edit landed
/// or was rejected) shows through — the cache never silently drifts from the
/// server.
#[derive(Resource, Debug, Default)]
struct PendingMutations {
    /// The pending mutations, grouped by object then item.
    by_object: HashMap<ObjectKey, HashMap<InventoryKey, PendingKind>>,
}

impl PendingMutations {
    /// The pending mutation for `(object, item)`, if any.
    fn get(&self, object: &ObjectKey, item: &InventoryKey) -> Option<&PendingKind> {
        self.by_object.get(object).and_then(|items| items.get(item))
    }

    /// All pending mutations for `object`.
    fn for_object(&self, object: &ObjectKey) -> Option<&HashMap<InventoryKey, PendingKind>> {
        self.by_object.get(object)
    }

    /// Whether `(object, item)` has a mutation in flight (un-editable).
    fn is_pending(&self, object: &ObjectKey, item: &InventoryKey) -> bool {
        self.get(object, item).is_some()
    }

    /// Record a pending mutation for `(object, item)`.
    fn set(&mut self, object: ObjectKey, item: InventoryKey, kind: PendingKind) {
        self.by_object.entry(object).or_default().insert(item, kind);
    }

    /// Drop every pending mutation for `object` (an authoritative listing
    /// arrived, so the overlay is no longer needed).
    fn clear_object(&mut self, object: &ObjectKey) {
        self.by_object.remove(object);
    }
}

// ---------------------------------------------------------------------------
// UI handles
// ---------------------------------------------------------------------------

/// The Content-tab widget entities.
#[derive(Resource, Debug)]
struct ContentsTabUi {
    /// The virtualized list viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The "N items" summary line.
    count_text: Entity,
    /// The action buttons, so their disabled state can be gated by permission.
    new_script: Entity,
    /// The Rename action button.
    rename: Entity,
    /// The Remove action button.
    remove: Entity,
}

/// The standalone Object Contents floater's entities.
#[derive(Resource, Debug)]
pub(crate) struct OpenObjectFloaterUi {
    /// The floater root (carries `UiPanelShown`).
    panel: Entity,
    /// The virtualized list viewport.
    viewport: Entity,
    /// The object-name line.
    name_text: Entity,
}

/// The Open-floater's current target object, set by the object pie's **Open**
/// action.
#[derive(Resource, Debug, Default)]
struct OpenObjectFloaterState {
    /// The picked object (region-scoped id + grid-wide key), or `None`.
    target: Option<(ScopedObjectId, ObjectKey)>,
}

/// A request to open the Object Contents floater against a picked object, sent
/// by the object menu's **Open** action ([`crate::object_menu`]).
#[derive(Message, Debug, Clone, Copy)]
pub(crate) struct OpenObjectContents {
    /// The region-scoped id of the picked object.
    pub(crate) scoped: ScopedObjectId,
    /// The grid-wide key of the picked object.
    pub(crate) full: ObjectKey,
}

/// A description of an item added to a prim's contents, for the pending-add
/// phantom row shown until the server confirms it.
#[derive(Debug, Clone)]
pub(crate) struct PendingAdd {
    /// The source item's id (the phantom row's key until reconcile).
    pub(crate) item_id: InventoryKey,
    /// The added item's display name.
    pub(crate) name: String,
    /// The added item's type-icon glyph.
    pub(crate) icon: &'static str,
}

/// A signal that an object's task inventory was mutated from outside this module
/// (a drag-in add resolved by [`crate::inventory_drag`]), so its cached listing
/// must be reconciled against the server — the same round trip the in-module
/// mutations do inline. `added` carries the dropped items so a "…adding" phantom
/// row can stand in until the server's listing includes them.
#[derive(Message, Debug, Clone)]
pub(crate) struct ContentsMutated {
    /// The region-scoped id of the mutated object.
    pub(crate) scoped: ScopedObjectId,
    /// The grid-wide key of the mutated object.
    pub(crate) full: ObjectKey,
    /// The items added by this mutation (empty for a pure reconcile).
    pub(crate) added: Vec<PendingAdd>,
}

/// Marks a viewport as a contents list, carrying which surface it is, so the
/// shared row-pool systems can resolve a row's surface from its parent.
#[derive(Component, Debug, Clone, Copy)]
struct ContentsViewport(ContentsSurface);

/// The persistent inner parts of a pooled contents row.
#[derive(Component, Debug)]
struct ContentsRowParts {
    /// The type-icon glyph node.
    icon: Entity,
    /// The item-name label node.
    label: Entity,
}

/// The contents action a header button fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContentsAction {
    /// Re-fetch the current prim's inventory (invalidate + fetch).
    Refresh,
    /// Open the selected item in its editor (double-click) — the reference's
    /// `LLTaskInvFVBridge::openItem`. Only notecards have an editor today.
    Open,
    /// Create a fresh default script in the prim.
    NewScript,
    /// Rename the selected item.
    Rename,
    /// Remove the selected item.
    Remove,
    /// Copy all contents into the agent's inventory (Open floater).
    CopyToInventory,
    /// Copy all contents into the agent's inventory and wear them (Open floater).
    CopyAndWear,
}

// ---------------------------------------------------------------------------
// The plugin
// ---------------------------------------------------------------------------

/// The plugin wiring the contents tab + floater into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EditContentsPlugin;

impl Plugin for EditContentsPlugin {
    /// Register the cache / views / selection resources, spawn both surfaces,
    /// and run the ingest / fetch / rebuild / bind / action systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<TaskInventoryCache>()
            .init_resource::<ContentsViews>()
            .init_resource::<ContentsSelection>()
            .init_resource::<ContentsLastClick>()
            .init_resource::<PendingMutations>()
            .init_resource::<OpenObjectFloaterState>()
            .init_resource::<ContentsRename>()
            .add_message::<OpenObjectContents>()
            .add_message::<ContentsMutated>()
            .add_message::<ContentsActionRequest>()
            .add_systems(
                Startup,
                spawn_open_object_floater.after(UiScaffoldSystems::SpawnRoot),
            )
            // Fills the Build floater's Content page once its lazily-built
            // content publishes the pages (`BuildTabPages` appears on first
            // open); ordered after the general parameter tabs, as before.
            .add_systems(
                Update,
                spawn_contents_tab
                    .run_if(resource_added::<BuildTabPages>)
                    .after(crate::edit_params::spawn_param_tabs),
            )
            .add_systems(
                Update,
                (
                    ingest_task_inventory,
                    open_object_contents_requests,
                    handle_contents_mutated,
                    rebuild_contents_views,
                    drive_contents_fetch,
                    sync_contents_drop_targets,
                    populate_new_contents_rows,
                    bind_contents_rows,
                    paint_contents_selection,
                    gate_contents_buttons,
                    contents_hotkeys,
                    run_contents_actions,
                    start_contents_rename,
                    drive_contents_rename,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------------
// Spawning the two surfaces
// ---------------------------------------------------------------------------

/// Spawn the Content-tab UI into the Build floater's Content page.
fn spawn_contents_tab(mut commands: Commands, pages: Option<Res<BuildTabPages>>) {
    let Some(pages) = pages else {
        return;
    };
    let page = pages.content;

    // Summary line: how many items (and a loading hint).
    let count_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(TOOL_FONT_SIZE),
            TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
            ClassList::new_with_classes([LABEL_CLASS]),
            Name::new("contents:count"),
            ChildOf(page),
        ))
        .id();

    // Action-button row.
    let button_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(4.0))
            },
            ChildOf(page),
        ))
        .id();
    let new_script = spawn_contents_button(
        &mut commands,
        button_row,
        "build-content-new-script",
        ContentsSurface::BuildTab,
        ContentsAction::NewScript,
        CONTENTS_TAB_INDEX,
    );
    let rename = spawn_contents_button(
        &mut commands,
        button_row,
        "build-content-rename",
        ContentsSurface::BuildTab,
        ContentsAction::Rename,
        CONTENTS_TAB_INDEX + 1,
    );
    let remove = spawn_contents_button(
        &mut commands,
        button_row,
        "build-content-remove",
        ContentsSurface::BuildTab,
        ContentsAction::Remove,
        CONTENTS_TAB_INDEX + 2,
    );
    spawn_contents_button(
        &mut commands,
        button_row,
        "build-content-refresh",
        ContentsSurface::BuildTab,
        ContentsAction::Refresh,
        CONTENTS_TAB_INDEX + 3,
    );

    let viewport = spawn_contents_viewport(
        &mut commands,
        page,
        ContentsSurface::BuildTab,
        CONTENTS_TAB_INDEX + 4,
    );

    commands.insert_resource(ContentsTabUi {
        viewport,
        count_text,
        new_script,
        rename,
        remove,
    });
}

/// Spawn the standalone Object Contents floater (the Open floater).
fn spawn_open_object_floater(mut commands: Commands, root: Option<Res<UiRoot>>) {
    let Some(root) = root.map(|root| root.0) else {
        return;
    };
    let handle: FloaterHandle = spawn_floater(
        &mut commands,
        root,
        FloaterSpec {
            id: "object-contents",
            title: String::from("Object Contents"),
            position: Vec2::new(120.0, 120.0),
            default_size: Some(Vec2::new(320.0, 420.0)),
            min_size: Some(Vec2::new(260.0, 260.0)),
            dock_host: None,
            caps: FloaterCaps {
                resizable: true,
                minimizable: true,
                closable: true,
                dockable: false,
            },
        },
    );
    commands
        .entity(handle.title_text)
        .insert(Translated::new("object-contents-floater-title"));

    let content = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..column(Val::Px(6.0))
            },
            ChildOf(handle.content),
        ))
        .id();

    // The object-name line (the reference's `object_name` text box).
    let name_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(TOOL_FONT_SIZE),
            TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
            ClassList::new_with_classes([LABEL_CLASS]),
            Name::new("object-contents:name"),
            ChildOf(content),
        ))
        .id();

    // Copy actions.
    let button_row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(4.0),
                ..row(Val::Px(4.0))
            },
            ChildOf(content),
        ))
        .id();
    spawn_contents_button(
        &mut commands,
        button_row,
        "object-contents-copy",
        ContentsSurface::OpenFloater,
        ContentsAction::CopyToInventory,
        10,
    );
    spawn_contents_button(
        &mut commands,
        button_row,
        "object-contents-copy-wear",
        ContentsSurface::OpenFloater,
        ContentsAction::CopyAndWear,
        11,
    );

    let viewport =
        spawn_contents_viewport(&mut commands, content, ContentsSurface::OpenFloater, 12);

    commands.insert_resource(OpenObjectFloaterUi {
        panel: handle.root,
        viewport,
        name_text,
    });
}

/// Spawn a contents-list viewport (the clipped, focusable virtual-list host)
/// under `parent` for `surface`, and return it.
fn spawn_contents_viewport(
    commands: &mut Commands,
    parent: Entity,
    surface: ContentsSurface,
    tab_index: i32,
) -> Entity {
    commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(80.0),
                overflow: Overflow::clip(),
                position_type: PositionType::Relative,
                ..Default::default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            VirtualList::new(ROW_HEIGHT),
            VirtualViewport,
            ContentsViewport(surface),
            crate::inventory_drag::ContentsDropTarget::default(),
            Pickable::default(),
            TabIndex(tab_index),
            Name::new(match surface {
                ContentsSurface::BuildTab => "contents-tab-viewport",
                ContentsSurface::OpenFloater => "object-contents-viewport",
            }),
            ChildOf(parent),
        ))
        .id()
}

/// Spawn one contents action button (a focusable, clickable labelled box).
fn spawn_contents_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    surface: ContentsSurface,
    action: ContentsAction,
    tab_index: i32,
) -> Entity {
    let button = commands
        .spawn((
            bevy::ui_widgets::Button,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..Default::default()
            },
            BorderColor::all(Color::srgba(0.4, 0.4, 0.4, 1.0)),
            BackgroundColor(Color::srgba(0.2, 0.2, 0.2, 1.0)),
            Pickable::default(),
            Name::new(format!("contents-button:{label_key}")),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Text::default(),
        Translated::new(label_key),
        UiFont::Sans.at(TOOL_FONT_SIZE),
        TextColor(Color::WHITE),
        ClassList::new_with_classes([VALUE_CLASS]),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.entity(button).observe(
        move |press: On<Pointer<Press>>, mut requests: MessageWriter<ContentsActionRequest>| {
            if press.button == PointerButton::Primary {
                requests.write(ContentsActionRequest { surface, action });
            }
        },
    );
    button
}

// ---------------------------------------------------------------------------
// Event ingest + fetch driver
// ---------------------------------------------------------------------------

/// Ingest the session's task-inventory events into the cache: a parsed contents
/// listing ([`SlSessionEvent::TaskInventoryContents`]) stores its items, and a
/// reply naming an empty Xfer file ([`SlSessionEvent::TaskInventoryReply`] with
/// no filename) records a known-empty inventory (no contents event follows).
fn ingest_task_inventory(
    mut events: MessageReader<SlEvent>,
    mut cache: ResMut<TaskInventoryCache>,
    mut pending: ResMut<PendingMutations>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::TaskInventoryContents {
                task,
                serial,
                items,
            } => {
                cache.store(*task, *serial, items.clone());
                // The authoritative listing supersedes every optimistic overlay.
                pending.clear_object(task);
            }
            SlSessionEvent::TaskInventoryReply(TaskInventoryReply {
                task,
                serial,
                filename,
            }) if filename.is_empty() => {
                cache.store_empty(*task, *serial);
                pending.clear_object(task);
            }
            _other => {}
        }
    }
}

/// React to an object pie **Open** request: point the Object Contents floater at
/// the picked object and show it.
fn open_object_contents_requests(
    mut requests: MessageReader<OpenObjectContents>,
    ui: Option<Res<OpenObjectFloaterUi>>,
    mut state: ResMut<OpenObjectFloaterState>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let Some(ui) = ui else {
        return;
    };
    for request in requests.read() {
        state.target = Some((request.scoped, request.full));
        if let Ok(mut shown) = panels.get_mut(ui.panel) {
            shown.0 = true;
        }
    }
}

/// Reconcile an object's cached listing after an out-of-module mutation (a
/// drag-in add from [`crate::inventory_drag`]): re-fetch it against the server,
/// keeping the last-good contents visible until the reply lands.
fn handle_contents_mutated(
    mut events: MessageReader<ContentsMutated>,
    mut cache: ResMut<TaskInventoryCache>,
    mut pending: ResMut<PendingMutations>,
    mut commands: MessageWriter<SlCommand>,
) {
    for event in events.read() {
        for add in &event.added {
            pending.set(
                event.full,
                add.item_id,
                PendingKind::Adding {
                    name: add.name.clone(),
                    icon: add.icon,
                },
            );
        }
        reconcile_after_mutation(&mut cache, &mut commands, event.scoped, event.full);
    }
}

/// Send a `FetchTaskInventory` for any surface's target that is not yet cached
/// (and not already being fetched). The cache's `fetching` flag debounces
/// repeat requests while the round trip is in flight.
fn drive_contents_fetch(
    views: Res<ContentsViews>,
    mut cache: ResMut<TaskInventoryCache>,
    mut commands: MessageWriter<SlCommand>,
) {
    for surface in [ContentsSurface::BuildTab, ContentsSurface::OpenFloater] {
        let Some((scoped, full)) = views.view(surface).target else {
            continue;
        };
        let needs_fetch = cache
            .get(&full)
            .is_none_or(|entry| !entry.loaded && !entry.fetching);
        if needs_fetch && cache.begin_fetch(full) {
            commands.write(SlCommand(Command::FetchTaskInventory { target: scoped }));
        }
    }
}

// ---------------------------------------------------------------------------
// Rebuild the flattened views
// ---------------------------------------------------------------------------

/// Recompute both surfaces' views from the current build selection / open target
/// and the cache, and set each list's item count. Cheap when nothing changed:
/// the whole body only runs on a change to the selection, the tool state, the
/// open target, or the cache.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool + \
              selection + open-target state, the object state, the cache, the two UI handles, \
              the views output, the selection reset, and the list-count queries"
)]
fn rebuild_contents_views(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    open_state: Res<OpenObjectFloaterState>,
    objects: Res<ObjectState>,
    cache: Res<TaskInventoryCache>,
    pending: Res<PendingMutations>,
    tab_ui: Option<Res<ContentsTabUi>>,
    open_ui: Option<Res<OpenObjectFloaterUi>>,
    mut views: ResMut<ContentsViews>,
    mut contents_selection: ResMut<ContentsSelection>,
    mut lists: Query<&mut VirtualList>,
) {
    if !(selection.is_changed()
        || tool.is_changed()
        || open_state.is_changed()
        || cache.is_changed()
        || pending.is_changed())
    {
        return;
    }

    // The Build tab follows the primary selection while the tool is open.
    let build_target = if tool.active {
        selection.primary().map(|node| (node.scoped, node.full))
    } else {
        None
    };
    rebuild_one_view(
        &mut views.build,
        build_target,
        &objects,
        &cache,
        &pending,
        &selection,
    );
    rebuild_one_view(
        &mut views.open,
        open_state.target,
        &objects,
        &cache,
        &pending,
        &selection,
    );

    // Keep each surface's selection valid: drop it if its item is gone.
    for (surface, view) in [
        (ContentsSurface::BuildTab, &views.build),
        (ContentsSurface::OpenFloater, &views.open),
    ] {
        if let Some(selected) = contents_selection.get(surface)
            && !view.rows.iter().any(|row| row.item_id == selected)
        {
            contents_selection.set(surface, None);
        }
    }

    if let Some(ui) = tab_ui
        && let Ok(mut list) = lists.get_mut(ui.viewport)
    {
        let count = views.build.rows.len();
        if list.item_count != count {
            list.item_count = count;
            list.scroll_to_top();
        }
    }
    if let Some(ui) = open_ui
        && let Ok(mut list) = lists.get_mut(ui.viewport)
    {
        let count = views.open.rows.len();
        if list.item_count != count {
            list.item_count = count;
            list.scroll_to_top();
        }
    }
}

/// Keep each surface's viewport [`ContentsDropTarget`] pointed at the object its
/// view shows — but only when the agent may **add** to it — so an inventory drag
/// dropped on the list adds to that prim's contents (and is refused otherwise).
///
/// [`ContentsDropTarget`]: crate::inventory_drag::ContentsDropTarget
fn sync_contents_drop_targets(
    views: Res<ContentsViews>,
    tab_ui: Option<Res<ContentsTabUi>>,
    open_ui: Option<Res<OpenObjectFloaterUi>>,
    mut drop_targets: Query<&mut crate::inventory_drag::ContentsDropTarget>,
) {
    if !views.is_changed() {
        return;
    }
    let mut apply = |viewport: Entity, view: &ContentsSurfaceView| {
        if let Ok(mut target) = drop_targets.get_mut(viewport) {
            let resolved = view.target.filter(|_target| view.perms.can_add());
            if target.target != resolved {
                target.target = resolved;
            }
        }
    };
    if let Some(ui) = tab_ui {
        apply(ui.viewport, &views.build);
    }
    if let Some(ui) = open_ui {
        apply(ui.viewport, &views.open);
    }
}

/// Rebuild one surface's view from its target, the cache, and the pending
/// mutation overlay: the server's listing forms the base rows, each item's
/// pending mutation (if any) recolours / relabels its row, and pending **adds**
/// (not yet in the listing) append phantom rows.
fn rebuild_one_view(
    view: &mut ContentsSurfaceView,
    target: Option<(ScopedObjectId, ObjectKey)>,
    objects: &ObjectState,
    cache: &TaskInventoryCache,
    pending: &PendingMutations,
    selection: &SelectionSet,
) {
    view.target = target;
    view.rows.clear();
    view.name.clear();
    view.perms = ContentsPerms::default();
    view.loading = false;
    let Some((scoped, full)) = target else {
        return;
    };
    view.perms = ContentsPerms::resolve(objects, &scoped);
    // The primary selection's `ObjectProperties` name, when it is this target.
    if let Some(primary) = selection.primary()
        && primary.full == full
        && let Some(properties) = primary.properties.as_ref()
    {
        properties.name.clone_into(&mut view.name);
    }
    let Some(entry) = cache.get(&full).filter(|entry| entry.loaded) else {
        view.loading = true;
        return;
    };
    view.rows = merge_contents_rows(&entry.items, pending.for_object(&full));
}

/// Merge a prim's authoritative task-inventory `items` with its pending-mutation
/// overlay into display rows — the pure heart of the pending model, so it is
/// where the tests are.
///
/// Each listed item's row carries any pending rename (its new name, "…refreshing")
/// or delete ("…deleting"); pending **adds** whose id is not yet in the listing
/// append phantom "…adding" rows. The listing is always the base, so a mutation
/// that never confirms simply vanishes from the overlay when it clears.
fn merge_contents_rows(
    items: &[TaskInventoryItem],
    pending: Option<&HashMap<InventoryKey, PendingKind>>,
) -> Vec<ContentsRow> {
    let mut rows: Vec<ContentsRow> = items
        .iter()
        .map(|item| {
            let (name, state) = match pending.and_then(|items| items.get(&item.item_id)) {
                Some(PendingKind::Renaming(new_name)) => (new_name.clone(), RowState::Refreshing),
                Some(PendingKind::Deleting) => (item.name.clone(), RowState::Deleting),
                // An `Adding` keyed by an existing item id is unexpected; treat as
                // normal (the listing already has it).
                Some(PendingKind::Adding { .. }) | None => (item.name.clone(), RowState::Normal),
            };
            ContentsRow {
                item_id: item.item_id,
                name,
                icon: item_icon(item.inv_type),
                state,
            }
        })
        .collect();
    // Pending adds: phantom rows for items not yet in the listing.
    if let Some(pending_items) = pending {
        for (item_id, kind) in pending_items {
            if let PendingKind::Adding { name, icon } = kind
                && !items.iter().any(|item| item.item_id == *item_id)
            {
                rows.push(ContentsRow {
                    item_id: *item_id,
                    name: name.clone(),
                    icon,
                    state: RowState::Adding,
                });
            }
        }
    }
    rows
}

// ---------------------------------------------------------------------------
// Row pool: populate + bind
// ---------------------------------------------------------------------------

/// Build the inner structure of a freshly-pooled contents row (once) and wire
/// its click-to-select observer.
fn populate_new_contents_rows(
    mut commands: Commands,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
    viewports: Query<&ContentsViewport>,
) {
    for (row_entity, child_of) in &new_rows {
        // Only rows under a contents viewport; ignore the inventory / people pools.
        let Ok(&ContentsViewport(surface)) = viewports.get(child_of.parent()) else {
            continue;
        };
        commands.entity(row_entity).insert((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(ROW_HEIGHT),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..Default::default()
            },
            Pickable::default(),
            BackgroundColor(Color::NONE),
        ));
        let icon = commands
            .spawn((
                Text::new(""),
                UiFont::Sans.at(ROW_FONT_SIZE),
                TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
                Node {
                    min_width: Val::Px(ICON_COL_WIDTH),
                    ..Default::default()
                },
                Pickable::IGNORE,
                ChildOf(row_entity),
            ))
            .id();
        let label = commands
            .spawn((
                Text::new(""),
                UiFont::Sans.at(ROW_FONT_SIZE),
                TextColor(Color::srgba(0.85, 0.85, 0.85, 1.0)),
                Pickable::IGNORE,
                ChildOf(row_entity),
            ))
            .id();
        commands
            .entity(row_entity)
            .insert(ContentsRowParts { icon, label })
            .observe(
                move |press: On<Pointer<Press>>,
                      rows: Query<&VirtualRow>,
                      views: Res<ContentsViews>,
                      time: Res<Time>,
                      mut selection: ResMut<ContentsSelection>,
                      mut last_click: ResMut<ContentsLastClick>,
                      mut requests: MessageWriter<ContentsActionRequest>,
                      mut focus: ResMut<InputFocus>| {
                    if press.button != PointerButton::Primary {
                        return;
                    }
                    if let Ok(row) = rows.get(press.entity)
                        && let Some(index) = row.index
                        && let Some(display) = views.view(surface).rows.get(index)
                    {
                        let item = display.item_id;
                        selection.set(surface, Some(item));
                        // Clicking the list focuses it so the wheel scrolls it.
                        focus.set(press.entity, FocusCause::Navigated);
                        // A second primary click on the same item within the
                        // double-click window opens it (the reference's openItem).
                        let now = time.elapsed_secs();
                        let double = last_click.last.is_some_and(|(prev, at)| {
                            prev == item && now - at < DOUBLE_CLICK_SECONDS
                        });
                        if double {
                            last_click.last = None;
                            requests.write(ContentsActionRequest {
                                surface,
                                action: ContentsAction::Open,
                            });
                        } else {
                            last_click.last = Some((item, now));
                        }
                    }
                },
            );
    }
}

/// Bind each pooled contents row to the item it now points at, appending its
/// pending-state suffix ("…adding" / "…deleting" / "…refreshing") and greying it
/// while a mutation is in flight.
fn bind_contents_rows(
    views: Res<ContentsViews>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &ContentsRowParts)>,
    viewports: Query<&ContentsViewport>,
    translator: Translator,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut nodes: Query<&mut Node>,
) {
    let rebuild_all = views.is_changed();
    for (row, child_of, parts) in &rows {
        let Ok(&ContentsViewport(surface)) = viewports.get(child_of.parent()) else {
            continue;
        };
        if !rebuild_all && !row.is_changed() {
            continue;
        }
        let view = views.view(surface);
        let bound = row.index.and_then(|index| view.rows.get(index));
        // Show the label only when the row is bound to an item.
        if let Ok(mut label_node) = nodes.get_mut(parts.label) {
            let display = if bound.is_some() {
                Display::Flex
            } else {
                Display::None
            };
            if label_node.display != display {
                label_node.display = display;
            }
        }
        let Some(display) = bound else {
            if let Ok((mut text, _color)) = texts.get_mut(parts.icon) {
                set_row_text(&mut text, "");
            }
            continue;
        };
        // A pending (in-flight) item draws dimmer until the server confirms it.
        let color = if display.state.is_pending() {
            TextColor(Color::srgba(0.55, 0.55, 0.55, 1.0))
        } else {
            TextColor(Color::srgba(0.92, 0.92, 0.92, 1.0))
        };
        if let Ok((mut text, mut text_color)) = texts.get_mut(parts.icon) {
            set_row_text(&mut text, display.icon);
            *text_color = color;
        }
        if let Ok((mut text, mut text_color)) = texts.get_mut(parts.label) {
            // Append the pending-state suffix, e.g. "Read me   …deleting".
            let label = match display.state.suffix_key() {
                Some(key) => format!("{}   {}", display.name, translator.get(key)),
                None => display.name.clone(),
            };
            set_row_text(&mut text, &label);
            *text_color = color;
        }
    }
}

/// Paint each pooled row's selection background.
fn paint_contents_selection(
    selection: Res<ContentsSelection>,
    views: Res<ContentsViews>,
    viewports: Query<&ContentsViewport>,
    mut rows: Query<(&VirtualRow, &ChildOf, &mut BackgroundColor)>,
) {
    for (row, child_of, mut background) in &mut rows {
        let Ok(&ContentsViewport(surface)) = viewports.get(child_of.parent()) else {
            continue;
        };
        let selected = row
            .index
            .and_then(|index| views.view(surface).rows.get(index))
            .is_some_and(|display| selection.get(surface) == Some(display.item_id));
        let want = if selected {
            Color::srgba(0.25, 0.4, 0.65, 0.6)
        } else {
            Color::NONE
        };
        if background.0 != want {
            background.0 = want;
        }
    }
}

/// Write `value` into a row text node only when it changed (avoids re-measuring).
fn set_row_text(text: &mut Text, value: &str) {
    if text.0 != value {
        value.clone_into(&mut text.0);
    }
}

// ---------------------------------------------------------------------------
// Count line + button gating
// ---------------------------------------------------------------------------

/// Keep the Content-tab count line current and gate the action buttons by the
/// resolved permissions / selection.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the views + selection to \
              read state from, the two UI handles, the translator, the text query, the \
              transition guard, and the command channel that toggles the buttons"
)]
fn gate_contents_buttons(
    views: Res<ContentsViews>,
    selection: Res<ContentsSelection>,
    tab_ui: Option<Res<ContentsTabUi>>,
    open_ui: Option<Res<OpenObjectFloaterUi>>,
    translator: Translator,
    mut texts: Query<&mut Text>,
    mut last_enabled: Local<Option<[bool; 3]>>,
    mut commands: Commands,
) {
    if let Some(ui) = &tab_ui {
        let view = &views.build;
        if let Ok(mut text) = texts.get_mut(ui.count_text) {
            let want = contents_summary(view, &translator);
            if text.0 != want {
                text.0 = want;
            }
        }
        let has_target = view.target.is_some();
        // A pending (in-flight) selection cannot be re-edited until it clears.
        let selected = selection.build.is_some()
            && !selection_is_pending(ContentsSurface::BuildTab, &views, &selection);
        // Only touch the buttons' interaction state on a transition, so a stable
        // selection does not churn archetypes every frame.
        let enabled = [
            has_target && view.perms.can_add(),
            has_target && selected && view.perms.can_rename_menu(),
            has_target && selected && view.perms.can_remove_menu(),
        ];
        if *last_enabled != Some(enabled) {
            *last_enabled = Some(enabled);
            let [add, rename, remove] = enabled;
            set_button_enabled(&mut commands, ui.new_script, add);
            set_button_enabled(&mut commands, ui.rename, rename);
            set_button_enabled(&mut commands, ui.remove, remove);
        }
    }
    if let Some(ui) = &open_ui {
        let view = &views.open;
        if let Ok(mut text) = texts.get_mut(ui.name_text) {
            let want = if view.target.is_none() {
                translator.get("object-contents-none")
            } else if view.name.is_empty() {
                contents_summary(view, &translator)
            } else {
                format!("{} — {}", view.name, contents_summary(view, &translator))
            };
            if text.0 != want {
                text.0 = want;
            }
        }
    }
}

/// The "N items" (or loading / empty) summary for a view.
fn contents_summary(view: &ContentsSurfaceView, translator: &Translator) -> String {
    if view.target.is_none() {
        return translator.get("build-content-no-target");
    }
    if view.loading {
        return translator.get("build-content-loading");
    }
    let count = i64::try_from(view.rows.len()).unwrap_or(i64::MAX);
    translator.format("build-content-count", &TransArgs::new().int("count", count))
}

/// Whether `surface`'s currently-selected item has a mutation in flight (so
/// rename / remove must be blocked to avoid racing the change already sent).
fn selection_is_pending(
    surface: ContentsSurface,
    views: &ContentsViews,
    selection: &ContentsSelection,
) -> bool {
    let Some(item) = selection.get(surface) else {
        return false;
    };
    views
        .view(surface)
        .rows
        .iter()
        .find(|row| row.item_id == item)
        .is_some_and(|row| row.state.is_pending())
}

/// Enable or disable a button (its interaction + a disabled visual class).
fn set_button_enabled(commands: &mut Commands, button: Entity, enabled: bool) {
    if enabled {
        commands
            .entity(button)
            .remove::<bevy::ui::InteractionDisabled>()
            .insert(Pickable::default());
    } else {
        commands
            .entity(button)
            .insert((bevy::ui::InteractionDisabled, Pickable::IGNORE));
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// Whether `focused` is `viewport` or a descendant of it — the focus gate a
/// list's keyboard shortcuts use so only the list actually clicked-into responds
/// (its rows focus themselves, so a plain `focus == viewport` test would miss).
pub(crate) fn focus_within(focused: Entity, viewport: Entity, child_of: &Query<&ChildOf>) -> bool {
    let mut node = focused;
    loop {
        if node == viewport {
            return true;
        }
        match child_of.get(node) {
            Ok(parent) => node = parent.parent(),
            Err(_root) => return false,
        }
    }
}

/// **F2** renames and **Delete / Backspace** removes the selected Content-tab
/// item — but only while the contents list is the focused widget, so the same
/// keys over the inventory list or the world hit their own handlers instead. The
/// work itself reuses the Rename / Remove actions (with their permission gates).
fn contents_hotkeys(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    focus: Res<InputFocus>,
    tab_ui: Option<Res<ContentsTabUi>>,
    selection: Res<ContentsSelection>,
    child_of: Query<&ChildOf>,
    mut requests: MessageWriter<ContentsActionRequest>,
) {
    if *context == InputContext::TextEntry {
        return;
    }
    let Some(ui) = tab_ui else {
        return;
    };
    let focused_here = focus
        .get()
        .is_some_and(|focused| focus_within(focused, ui.viewport, &child_of));
    if !focused_here || selection.build.is_none() {
        return;
    }
    if keyboard.just_pressed(KeyCode::F2) {
        requests.write(ContentsActionRequest {
            surface: ContentsSurface::BuildTab,
            action: ContentsAction::Rename,
        });
    }
    if keyboard.just_pressed(KeyCode::Delete) || keyboard.just_pressed(KeyCode::Backspace) {
        requests.write(ContentsActionRequest {
            surface: ContentsSurface::BuildTab,
            action: ContentsAction::Remove,
        });
    }
}

/// A contents action fired by a header button.
#[derive(Message, Debug, Clone, Copy)]
struct ContentsActionRequest {
    /// The surface the action acts on.
    surface: ContentsSurface,
    /// Which action.
    action: ContentsAction,
}

/// Dispatch a fired contents action to its command(s).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the action stream, the \
              views + cache + selection + identity + inventory model to resolve the target, the \
              rename state, and the command / notice channels"
)]
fn run_contents_actions(
    mut requests: MessageReader<ContentsActionRequest>,
    views: Res<ContentsViews>,
    mut cache: ResMut<TaskInventoryCache>,
    mut pending: ResMut<PendingMutations>,
    selection: Res<ContentsSelection>,
    identity: Res<SlIdentity>,
    inventory: Res<InventoryModel>,
    mut rename: ResMut<ContentsRename>,
    translator: Translator,
    mut commands: MessageWriter<SlCommand>,
    mut notices: MessageWriter<LocalChatNotice>,
    mut notecard_opens: MessageWriter<crate::edit_notecard::OpenNotecard>,
    mut script_opens: MessageWriter<crate::edit_script::OpenScript>,
) {
    for request in requests.read() {
        let view = views.view(request.surface);
        let Some((scoped, full)) = view.target else {
            continue;
        };
        match request.action {
            ContentsAction::Refresh => {
                reconcile_after_mutation(&mut cache, &mut commands, scoped, full);
            }
            ContentsAction::Open => {
                let Some(item_id) = selection.get(request.surface) else {
                    continue;
                };
                let Some(item) = cache
                    .get(&full)
                    .and_then(|entry| entry.items.iter().find(|it| it.item_id == item_id))
                else {
                    continue;
                };
                // Notecards and scripts have an editor today; other types no-op
                // (the reference opens each type in its own preview/editor).
                if !matches!(
                    item.inv_type,
                    InventoryType::Notecard | InventoryType::Script
                ) {
                    continue;
                }
                // A redacted (nil) asset id means the grid withheld it — the item
                // cannot be fetched, so there is nothing to open.
                let Some(asset_id) = item.asset_id else {
                    notices.write(LocalChatNotice::new(
                        translator.get("build-content-no-modify"),
                    ));
                    continue;
                };
                // Editable needs BOTH the object's modify and the item's own
                // modify bit (the reference's two-level rule); otherwise the
                // item opens read-only.
                let editable = view.perms.can_modify && item_modifiable(item);
                if item.inv_type == InventoryType::Script {
                    script_opens.write(crate::edit_script::OpenScript {
                        name: item.name.clone(),
                        asset_id: asset_id.uuid(),
                        editable,
                        source: crate::edit_script::ScriptSource::Task {
                            task_id: full,
                            item_id,
                        },
                        target: crate::edit_script::target_for(
                            sl_client_bevy::ScriptLanguage::from_item_flags(item.flags),
                        ),
                    });
                } else {
                    notecard_opens.write(crate::edit_notecard::OpenNotecard {
                        name: item.name.clone(),
                        asset_id: asset_id.uuid(),
                        editable,
                        source: crate::edit_notecard::NotecardSource::Task {
                            task_id: full,
                            item_id,
                        },
                    });
                }
            }
            ContentsAction::NewScript => {
                if !view.perms.can_add() {
                    notices.write(LocalChatNotice::new(
                        translator.get("build-content-no-modify"),
                    ));
                    continue;
                }
                let Some(creator) = identity.agent_id else {
                    continue;
                };
                let name = translator.get("build-content-new-script-name");
                // A fresh id keys the "…adding" phantom until the simulator (which
                // assigns the real task id) confirms the new script in the listing.
                let phantom_id = InventoryKey::from(Uuid::new_v4());
                let item = RestoreItem::new_script(creator, full, &name, Uuid::new_v4());
                commands.write(SlCommand(Command::RezScript {
                    target: scoped,
                    params: Box::new(RezScriptParams {
                        group_id: None,
                        enabled: false,
                        item,
                    }),
                }));
                pending.set(
                    full,
                    phantom_id,
                    PendingKind::Adding {
                        name,
                        icon: item_icon(InventoryType::Script),
                    },
                );
                reconcile_after_mutation(&mut cache, &mut commands, scoped, full);
            }
            ContentsAction::Rename => {
                if let Some(item) = selection.get(request.surface)
                    && !pending.is_pending(&full, &item)
                {
                    rename.pending = Some(item);
                }
            }
            ContentsAction::Remove => {
                let Some(item_id) = selection.get(request.surface) else {
                    continue;
                };
                // Already in flight — do not race the change already sent.
                if pending.is_pending(&full, &item_id) {
                    continue;
                }
                if !view.perms.can_remove_menu() {
                    continue;
                }
                // The reference offers Remove to an owner but only *applies* it
                // with object modify — an owner-without-modify gets the notice.
                if !view.perms.can_modify {
                    notices.write(LocalChatNotice::new(
                        translator.get("build-content-no-modify"),
                    ));
                    continue;
                }
                commands.write(SlCommand(Command::RemoveTaskInventory {
                    target: scoped,
                    item_id,
                }));
                pending.set(full, item_id, PendingKind::Deleting);
                reconcile_after_mutation(&mut cache, &mut commands, scoped, full);
            }
            ContentsAction::CopyToInventory | ContentsAction::CopyAndWear => {
                copy_contents_out(
                    &full,
                    scoped,
                    &cache,
                    &inventory,
                    request.action == ContentsAction::CopyAndWear,
                    &mut commands,
                    &translator,
                    &mut notices,
                );
                // Moving a no-copy item out empties it from the prim, so reconcile
                // the prim's own cache against what actually left.
                reconcile_after_mutation(&mut cache, &mut commands, scoped, full);
            }
        }
    }
}

/// After sending a task-inventory mutation, re-fetch the prim's listing so the
/// cache reconciles against the **server's** authoritative state (see
/// [`TaskInventoryCache::mark_stale`]). The old listing stays visible until the
/// reply lands, so there is no "loading" flash and a rejected mutation simply
/// leaves the contents unchanged rather than drifting.
fn reconcile_after_mutation(
    cache: &mut TaskInventoryCache,
    commands: &mut MessageWriter<SlCommand>,
    scoped: ScopedObjectId,
    full: ObjectKey,
) {
    cache.mark_stale(&full);
    commands.write(SlCommand(Command::FetchTaskInventory { target: scoped }));
}

/// Copy an object's whole contents into the agent's inventory (the Open
/// floater's Copy actions) by moving each item into the system Objects folder.
/// A no-copy item is moved out of the prim; a copyable one is copied — the
/// simulator arbitrates per item's permissions.
#[expect(
    clippy::too_many_arguments,
    reason = "the target keys, the cache + inventory model to resolve the folder + items, the \
              wear flag, and the command / translator / notice channels"
)]
fn copy_contents_out(
    full: &ObjectKey,
    scoped: ScopedObjectId,
    cache: &TaskInventoryCache,
    inventory: &InventoryModel,
    wear: bool,
    commands: &mut MessageWriter<SlCommand>,
    translator: &Translator,
    notices: &mut MessageWriter<LocalChatNotice>,
) {
    let Some(folder) = inventory.folder_by_type(FolderType::Object) else {
        notices.write(LocalChatNotice::new(
            translator.get("object-contents-no-folder"),
        ));
        return;
    };
    let Some(entry) = cache.get(full) else {
        return;
    };
    for item in &entry.items {
        commands.write(SlCommand(Command::MoveTaskInventory {
            target: scoped,
            folder_id: folder,
            item_id: item.item_id,
        }));
    }
    // "Copy And Wear" wearing is deferred: the moved items arrive asynchronously
    // as inventory updates, so a follow-up wear needs the new agent-inventory
    // ids — surfaced to the user for now rather than silently dropped.
    if wear {
        notices.write(LocalChatNotice::new(
            translator.get("object-contents-wear-note"),
        ));
    }
}

// ---------------------------------------------------------------------------
// Inline rename
// ---------------------------------------------------------------------------

/// The pending / active Content-tab rename (the reference's task-item rename).
#[derive(Resource, Debug, Default)]
struct ContentsRename {
    /// An item id whose rename is requested but whose row is not yet placed.
    pending: Option<InventoryKey>,
    /// The active inline editor, if one is open.
    active: Option<ActiveContentsRename>,
}

/// An open inline rename editor over a contents row.
#[derive(Debug, Clone, Copy)]
struct ActiveContentsRename {
    /// The item being renamed.
    item_id: InventoryKey,
    /// The row index it sits at.
    index: usize,
    /// The pooled row entity hosting the editor.
    row: Entity,
    /// The spawned text-input field.
    field: Entity,
}

/// Begin a pending Content-tab rename once its row is on screen.
fn start_contents_rename(
    mut rename: ResMut<ContentsRename>,
    views: Res<ContentsViews>,
    tab_ui: Option<Res<ContentsTabUi>>,
    rows: Query<(Entity, &VirtualRow, &ContentsRowParts, &ChildOf)>,
    mut nodes: Query<&mut Node>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if rename.active.is_some() {
        return;
    }
    let Some(item_id) = rename.pending else {
        return;
    };
    let Some(ui) = tab_ui else {
        return;
    };
    let view = &views.build;
    let Some(index) = view.rows.iter().position(|row| row.item_id == item_id) else {
        return;
    };
    let Some(name) = view.rows.get(index).map(|row| row.name.clone()) else {
        return;
    };
    for (entity, row, parts, child_of) in &rows {
        if child_of.parent() != ui.viewport || row.index != Some(index) {
            continue;
        }
        if let Ok(mut label) = nodes.get_mut(parts.label) {
            label.display = Display::None;
        }
        let field = spawn_text_input(
            &mut commands,
            entity,
            &TextInputSpec {
                initial: name,
                font_size: ROW_FONT_SIZE,
                width_glyphs: 20.0,
                ..TextInputSpec::new("contents-rename", TextInputKind::Line)
            },
        );
        focus.set(field, FocusCause::Navigated);
        rename.pending = None;
        rename.active = Some(ActiveContentsRename {
            item_id,
            index,
            row: entity,
            field,
        });
        return;
    }
}

/// Drive the open Content-tab rename: `Enter` commits (an `UpdateTaskInventory`
/// re-sending the item with its new name), `Escape` cancels, and the row
/// scrolling away / rebinding cancels.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the rename state, the \
              keyboard, the views + cache to resolve the item, the field to read, the label to \
              restore, and the command / notice channels"
)]
fn drive_contents_rename(
    keyboard: Res<ButtonInput<KeyCode>>,
    views: Res<ContentsViews>,
    mut cache: ResMut<TaskInventoryCache>,
    mut pending: ResMut<PendingMutations>,
    rows: Query<(&VirtualRow, &ContentsRowParts)>,
    fields: Query<&EditableText>,
    mut nodes: Query<&mut Node>,
    mut rename: ResMut<ContentsRename>,
    translator: Translator,
    mut commands_bevy: Commands,
    mut commands: MessageWriter<SlCommand>,
    mut notices: MessageWriter<LocalChatNotice>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        rename.pending = None;
        if let Some(active) = rename.active.take() {
            end_contents_rename(&active, &rows, &mut nodes, &mut commands_bevy);
        }
        return;
    }
    let Some(active) = rename.active else {
        return;
    };
    let view = &views.build;
    // Cancel when the hosting row no longer shows the renamed item.
    let still_bound = rows
        .get(active.row)
        .is_ok_and(|(row, _parts)| row.index == Some(active.index))
        && view
            .rows
            .get(active.index)
            .is_some_and(|display| display.item_id == active.item_id);
    if !still_bound {
        rename.active = None;
        end_contents_rename(&active, &rows, &mut nodes, &mut commands_bevy);
        return;
    }
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let new_name = fields
        .get(active.field)
        .map(|field| field.value().to_string().trim().to_owned())
        .unwrap_or_default();
    let sent = commit_contents_rename(
        active.item_id,
        &new_name,
        view,
        &cache,
        &translator,
        &mut commands,
        &mut notices,
    );
    // Reconcile the prim's cache against the server after a rename we sent, so a
    // rejected rename reverts to the real name instead of drifting; show the new
    // name flagged "…refreshing" meanwhile, and block a second rename of it.
    if sent && let Some((scoped, full)) = view.target {
        pending.set(full, active.item_id, PendingKind::Renaming(new_name));
        reconcile_after_mutation(&mut cache, &mut commands, scoped, full);
    }
    rename.active = None;
    end_contents_rename(&active, &rows, &mut nodes, &mut commands_bevy);
}

/// Send the rename `UpdateTaskInventory` for `item_id`, gated on the item's own
/// modify permission (the reference's item-level `renameItem` check). Returns
/// whether a command was actually sent, so the caller can reconcile the cache.
fn commit_contents_rename(
    item_id: InventoryKey,
    new_name: &str,
    view: &ContentsSurfaceView,
    cache: &TaskInventoryCache,
    translator: &Translator,
    commands: &mut MessageWriter<SlCommand>,
    notices: &mut MessageWriter<LocalChatNotice>,
) -> bool {
    let Some((scoped, full)) = view.target else {
        return false;
    };
    if new_name.is_empty() {
        return false;
    }
    let Some(item) = cache
        .get(&full)
        .and_then(|entry| entry.items.iter().find(|item| item.item_id == item_id))
    else {
        return false;
    };
    if new_name == item.name {
        return false;
    }
    // The rename is only *applied* when the item itself is modifiable.
    if !item_modifiable(item) {
        notices.write(LocalChatNotice::new(
            translator.get("build-content-item-no-modify"),
        ));
        return false;
    }
    let restore = match RestoreItem::from_task_item(item, Some(new_name), None, Uuid::new_v4()) {
        Ok(restore) => restore,
        Err(error) => {
            warn!("contents rename: could not encode item: {error}");
            return false;
        }
    };
    commands.write(SlCommand(Command::UpdateTaskInventory {
        target: scoped,
        key: TaskInventoryKey::Item,
        item: Box::new(restore),
    }));
    true
}

/// Tear the inline rename editor down: despawn the field and restore the label.
fn end_contents_rename(
    active: &ActiveContentsRename,
    rows: &Query<(&VirtualRow, &ContentsRowParts)>,
    nodes: &mut Query<&mut Node>,
    commands: &mut Commands,
) {
    commands.entity(active.field).despawn();
    if let Ok((_row, parts)) = rows.get(active.row)
        && let Ok(mut label) = nodes.get_mut(parts.label)
    {
        label.display = Display::Flex;
    }
}

/// Add the dropped inventory item to `object`'s task inventory — the drag-in
/// path, called from [`crate::inventory_drag`] when a drag ends over a contents
/// list. Returns the command to send, or `None` when the source is a folder
/// (task inventory takes single items).
pub(crate) fn contents_drop_command(
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

#[cfg(test)]
mod tests {
    use super::{ContentsPerms, item_modifiable};
    use pretty_assertions::assert_eq;

    /// `can_add` is object-modify OR the allow-drop flag; `can_remove_menu` /
    /// `can_rename_menu` are object-modify OR ownership — the reference's split.
    #[test]
    fn perm_gates_match_reference() {
        // A modifiable object: every affordance is offered.
        let modifiable = ContentsPerms {
            can_modify: true,
            owns: true,
            allows_drop: false,
        };
        assert!(modifiable.can_add());
        assert!(modifiable.can_remove_menu());
        assert!(modifiable.can_rename_menu());

        // Owned but not modifiable: rename / remove are *offered* (the reference
        // enables them for an owner), but add is not.
        let owned_no_mod = ContentsPerms {
            can_modify: false,
            owns: true,
            allows_drop: false,
        };
        assert!(!owned_no_mod.can_add());
        assert!(owned_no_mod.can_remove_menu());
        assert!(owned_no_mod.can_rename_menu());

        // Not owned, not modifiable, but "allow anyone to add inventory": the one
        // exception that lets a non-owner drop items in.
        let drop_flag = ContentsPerms {
            can_modify: false,
            owns: false,
            allows_drop: true,
        };
        assert!(drop_flag.can_add());
        assert!(!drop_flag.can_remove_menu());
        assert!(!drop_flag.can_rename_menu());
    }

    /// Item-level modify reads the item's own owner mask (content permission),
    /// independent of the object's permissions.
    #[test]
    fn item_modify_reads_the_item_owner_mask() {
        use sl_client_bevy::{Permissions, Permissions5};
        let mut item = super::TaskInventoryItem {
            item_id: sl_client_bevy::InventoryKey::from(sl_client_bevy::Uuid::nil()),
            parent_task: sl_client_bevy::ObjectKey::from(sl_client_bevy::Uuid::nil()),
            permissions: Permissions5 {
                base: Permissions::ALL,
                owner: Permissions::ALL,
                group: Permissions::empty(),
                everyone: Permissions::empty(),
                next_owner: Permissions::ALL,
            },
            creator_id: sl_client_bevy::AgentKey::from(sl_client_bevy::Uuid::nil()),
            last_owner_id: sl_client_bevy::AgentKey::from(sl_client_bevy::Uuid::nil()),
            owner: sl_client_bevy::OwnerKey::Agent(sl_client_bevy::AgentKey::from(
                sl_client_bevy::Uuid::nil(),
            )),
            group: None,
            group_owned: false,
            asset_id: None,
            asset_type: sl_client_bevy::AssetType::Notecard,
            inv_type: sl_client_bevy::InventoryType::Notecard,
            flags: 0,
            sale_type: sl_client_bevy::SaleType::NotForSale,
            sale_price: sl_client_bevy::LindenAmount(0),
            name: String::from("x"),
            description: String::new(),
            creation_date: 0,
        };
        assert!(item_modifiable(&item));
        item.permissions.owner = Permissions::empty();
        assert_eq!(item_modifiable(&item), false);
    }

    /// A task item with the given id and name; the rest is inert filler.
    fn task_item(id: u128, name: &str) -> super::TaskInventoryItem {
        use sl_client_bevy::{Permissions, Permissions5};
        super::TaskInventoryItem {
            item_id: sl_client_bevy::InventoryKey::from(sl_client_bevy::Uuid::from_u128(id)),
            parent_task: sl_client_bevy::ObjectKey::from(sl_client_bevy::Uuid::nil()),
            permissions: Permissions5 {
                base: Permissions::ALL,
                owner: Permissions::ALL,
                group: Permissions::empty(),
                everyone: Permissions::empty(),
                next_owner: Permissions::ALL,
            },
            creator_id: sl_client_bevy::AgentKey::from(sl_client_bevy::Uuid::nil()),
            last_owner_id: sl_client_bevy::AgentKey::from(sl_client_bevy::Uuid::nil()),
            owner: sl_client_bevy::OwnerKey::Agent(sl_client_bevy::AgentKey::from(
                sl_client_bevy::Uuid::nil(),
            )),
            group: None,
            group_owned: false,
            asset_id: Some(sl_client_bevy::AssetKey::from(
                sl_client_bevy::Uuid::from_u128(id),
            )),
            asset_type: sl_client_bevy::AssetType::Notecard,
            inv_type: sl_client_bevy::InventoryType::Notecard,
            flags: 0,
            sale_type: sl_client_bevy::SaleType::NotForSale,
            sale_price: sl_client_bevy::LindenAmount(0),
            name: String::from(name),
            description: String::new(),
            creation_date: 0,
        }
    }

    /// The pending overlay merge: the server listing is the base; a pending
    /// rename relabels its row "…refreshing", a pending delete keeps its row
    /// "…deleting", and a pending add appends a phantom "…adding" row. The base
    /// listing (not the overlay) is the source of truth, so nothing drifts.
    #[test]
    fn pending_overlay_merges_over_the_listing() -> Result<(), Box<dyn core::error::Error>> {
        use super::{PendingKind, RowState};
        use std::collections::HashMap;

        let keep = task_item(1, "Keep");
        let renamed = task_item(2, "Renamed");
        let gone = task_item(3, "Gone");
        let items = vec![keep.clone(), renamed.clone(), gone.clone()];
        let phantom = sl_client_bevy::InventoryKey::from(sl_client_bevy::Uuid::from_u128(0x99));
        let mut pending: HashMap<sl_client_bevy::InventoryKey, PendingKind> = HashMap::new();
        pending.insert(
            renamed.item_id,
            PendingKind::Renaming(String::from("New name")),
        );
        pending.insert(gone.item_id, PendingKind::Deleting);
        pending.insert(
            phantom,
            PendingKind::Adding {
                name: String::from("Being added"),
                icon: "?",
            },
        );

        let rows = super::merge_contents_rows(&items, Some(&pending));
        let row_for = |id| rows.iter().find(|row| row.item_id == id);
        // Three listed items + one phantom add.
        assert_eq!(rows.len(), 4);
        // Untouched item stays normal.
        let keep_row = row_for(keep.item_id).ok_or("keep row")?;
        assert_eq!(keep_row.name, "Keep");
        assert_eq!(keep_row.state, RowState::Normal);
        assert!(!keep_row.state.is_pending());
        // Renamed item shows the new name, flagged refreshing.
        let renamed_row = row_for(renamed.item_id).ok_or("renamed row")?;
        assert_eq!(renamed_row.name, "New name");
        assert_eq!(renamed_row.state, RowState::Refreshing);
        // Deleted item is still listed (its own name), flagged deleting.
        let gone_row = row_for(gone.item_id).ok_or("gone row")?;
        assert_eq!(gone_row.name, "Gone");
        assert_eq!(gone_row.state, RowState::Deleting);
        // The phantom add is appended for an id not in the listing.
        let phantom_row = row_for(phantom).ok_or("phantom add row")?;
        assert_eq!(phantom_row.name, "Being added");
        assert_eq!(phantom_row.state, RowState::Adding);

        // With no overlay every row is normal — the plain listing.
        let plain = super::merge_contents_rows(&items, None);
        assert_eq!(plain.len(), 3);
        assert!(plain.iter().all(|row| row.state == RowState::Normal));
        Ok(())
    }
}
