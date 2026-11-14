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
//! surface is just a `ContentsSurface` tag on its viewport:
//!
//! - `ContentsSurface::BuildTab` — docked into the Build floater's Content
//!   tab page (`crate::edit_tool::BuildTabPages::content`). It follows the
//!   build selection: the currently-selected prim, which in *edit linked parts*
//!   mode ([`EditToolState::edit_linked`]) is an individual linkset member and
//!   otherwise the linkset root. It offers the editing actions (new script,
//!   rename, remove, drag-in add) gated by permission.
//! - `ContentsSurface::OpenFloater` — a separate window opened by the object
//!   pie's **Open** action (the reference viewer's `LLFloaterOpenObject`). It
//!   lists a picked object's contents and offers **Copy To Inventory** /
//!   **Copy And Wear**, which move the contents into the agent's inventory.
//!
//! What a surface shows and has picked is one `ContentsPane`: the rendered
//! view, the selected row and the last click (for the double-click Open). The
//! Build tab's pane is a resource, `BuildContents` — it is a tab in one window.
//!
//! # One Object Contents window per object
//!
//! The Object Contents floater is a **keyed** floater
//! (`viewer-key-object-contents`): one window per object, keyed by the
//! object's grid-wide [`ObjectKey`] (which, unlike the region-scoped id,
//! survives the object being re-announced under a new local id). Its pane, its
//! target and its widget handles are a `ContentsWindow` component on the
//! window root, so dragging an item from one object's contents into another's
//! has both listings on screen, and a click in one window never selects, opens
//! or double-clicks in the other. Which pane a row, a viewport or a button
//! belongs to is a `PaneRef`: the Build tab, or the window it lives in.
//!
//! The reference keeps `LLFloaterOpenObject` a singleton over the current
//! selection. We key it anyway, as About Land and About Region are keyed — a
//! deliberate divergence, recorded in [[viewer-keyed-floater-audit]].
//!
//! What stays a **resource**, shared by every pane:
//!
//! - `TaskInventoryCache` — a cache of grid state keyed by object, not window
//!   state; two windows (or a window and the Build tab) on one object share its
//!   one listing and its one fetch.
//! - `PendingMutations` — keyed by object and item. Two windows on different
//!   objects cannot collide in it, and two views of one object *should* both
//!   show a change in flight, so per-window copies would only disagree.
//! - `ContentsRename` — the inline rename is the Build tab's alone; the window
//!   offers no rename.
//!
//! # The cache — why cycling linkset parts stays instant
//!
//! Fetching a prim's inventory is a round trip (a `RequestTaskInventory`, then a
//! legacy `Xfer` download of the listing — see [`Command::FetchTaskInventory`]),
//! so it can be slow. `TaskInventoryCache` keeps every fetched listing keyed by
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
//! (`ContentsPerms`):
//!
//! - **Add** (drag-in / new script): the object is modifiable **or** flagged
//!   "allow anyone to add inventory" ([`ObjectState::agent_allows_inventory_drop`]),
//!   the one documented exception to needing object modify.
//! - **Remove**: offered when you can modify **or own** the object
//!   (`ContentsPerms::can_remove_menu`), but only actually applied with object
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
use bevy::ui_widgets::Activate;
use bevy_flair::style::components::ClassList;
use sl_client_bevy::{
    CircuitId, Command, FolderType, InventoryKey, InventoryType, ObjectKey, Permissions,
    RegionLocalObjectId, RestoreItem, RezScriptParams, ScopedObjectId, SlCommand, SlEvent,
    SlIdentity, SlSessionEvent, TaskInventoryItem, TaskInventoryKey, TaskInventoryReply, Uuid,
    XferListing,
};

use crate::edit_tool::{BuildTabPages, LABEL_CLASS, TOOL_FONT_SIZE, VALUE_CLASS};
use crate::floater::{
    FloaterCaps, FloaterHandle, FloaterHost, FloaterKey, FloaterSpec, FloaterSystems,
    KeyedFloaterOpen, KeyedFloaters,
};
use crate::i18n::{TransArgs, Translated, Translator};
use crate::intents::ContentsMutated;
use crate::intents::LocalChatNotice;
use crate::inventory::{InventoryModel, item_icon, label_clip_node};
use crate::ui::focus_within;
use crate::ui::{column, row};
use crate::ui_element::{ContentMayOverflow, TextMayClip};
use crate::ui_font::UiFont;
use crate::ui_spawn::{self, ButtonKind, ButtonSpec, UiLabel};
use crate::ui_text_input::{TextInputKind, TextInputSpec, spawn_text_input};
use crate::virtual_list::{
    VirtualList, VirtualRow, VirtualViewport, amend_row_node, spawn_specimen_row,
    spawn_virtual_scrollbar,
};
use crate::world_api::EditToolState;
use crate::world_api::InputContext;
use crate::world_api::ObjectState;
use crate::world_api::SelectionSet;
use sl_viewer_ui_core::double_click::DoubleClickWindow;
use sl_viewer_ui_core::skin::{
    LIST_ROW_CLASS, LIST_SURFACE_CLASS, SELECTED_CLASS, set_role_class, set_state_class, text_role,
};
use sl_viewer_ui_core::skin_palette::SkinPalette;

/// A contents row's text, at rest — the primary role, which a list's face
/// re-roots to its field text.
const ROW_TEXT: Color = SkinPalette::FALLBACK.text_primary;

/// A contents row's text while its item has a change in flight — the greyed
/// role, until the server confirms it.
const PENDING_ROW_TEXT: Color = SkinPalette::FALLBACK.text_disabled;
use sl_viewer_ui_core::ui_ellipsis::{RevealEllipsis, spawn_ellipsis_marker};

/// The uniform height of a contents row, in logical pixels (matches the
/// inventory list's row metric so the two read the same).
const ROW_HEIGHT: f32 = 22.0;

/// The contents row font size, in logical pixels.
const ROW_FONT_SIZE: f32 = 13.0;

/// The height of a contents row whose parts are drawn at `font_size`:
/// [`ROW_HEIGHT`] at [`ROW_FONT_SIZE`], and in proportion above it, so a larger
/// font's line stays inside its fixed-height row instead of spilling into the
/// rows above and below. Never less than [`ROW_HEIGHT`].
fn row_height(font_size: f32) -> f32 {
    ROW_HEIGHT.max((font_size * ROW_HEIGHT / ROW_FONT_SIZE).ceil())
}

/// Why a row's label clip may slice the name it holds: it is the row's
/// ellipsis cell, and the `…` marker beside it says the tail is hidden.
const LABEL_CLIP_REASON: &str =
    "a contents row's name clips at the row's end behind its revealed `…` marker";

/// The row's `…` marker glyph until `i18n` sets the locale's own.
const FALLBACK_ELLIPSIS: &str = "\u{2026}";

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
    /// Whether the last listing the simulator sent failed to parse. It is not
    /// re-requested until the prim's contents serial moves (the same file
    /// would fail the same way); any earlier good listing stays shown.
    unreadable: bool,
    /// The write this viewer sent into the prim and has not yet seen the
    /// outcome of, if there is one.
    awaited: Option<AwaitedChange>,
}

/// A write into a prim whose outcome the listing has not shown yet.
///
/// A listing asked for right after a write can be answered before the write
/// is: on Second Life the write travels over UDP and the listing over the
/// `RequestTaskInventory` capability, and nothing orders the two. So a listing
/// that comes back at the serial the prim stood at before the write is not yet
/// the verdict. It is asked for once more, [`CONTENTS_RECHECK_SECONDS`] later,
/// and the answer to *that* is: a serial that still has not moved is a write
/// the grid did not take. The reference does the same
/// (`LLViewerObject::fetchInventoryFromCapCoro`, "stale inv_serial?").
#[derive(Debug, Clone, Copy, PartialEq)]
struct AwaitedChange {
    /// The serial the listing stood at when the write was sent, if a listing
    /// was held.
    from: Option<i16>,
    /// When to ask again, in seconds since startup, once a listing at the
    /// old serial has come back.
    again_at: Option<f32>,
    /// Whether the second listing has been asked for.
    asked_again: bool,
}

/// How long after an unchanged listing the prim is asked again: the
/// reference's `INVENTORY_UPDATE_WAIT_TIME_OUTDATED`.
const CONTENTS_RECHECK_SECONDS: f32 = 1.0;

/// What a listing that arrived settled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listed {
    /// The listing is the prim's contents: any optimistic overlay goes.
    Settled,
    /// The listing predates a write this viewer is waiting on the outcome of,
    /// and the prim is asked again shortly: the overlay stays.
    AskAgain,
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

    /// Store a freshly parsed listing for `task`, which arrived at `now`
    /// seconds since startup.
    fn store(
        &mut self,
        task: ObjectKey,
        serial: i16,
        items: Vec<TaskInventoryItem>,
        now: f32,
    ) -> Listed {
        let entry = self.entries.entry(task).or_default();
        entry.items = items;
        entry.loaded = true;
        entry.unreadable = false;
        entry.settle(serial, now)
    }

    /// The grid said the listing held for `task` is still its contents.
    fn store_unchanged(&mut self, task: ObjectKey, serial: i16, now: f32) -> Listed {
        self.entries.entry(task).or_default().settle(serial, now)
    }

    /// The grid never answered for `task` — or, with `None`, for a prim that
    /// could not be named: nothing in flight is still awaited. What was
    /// listed before stays listed; a prim never listed is asked about again
    /// the next time it is shown.
    fn give_up(&mut self, task: Option<&ObjectKey>) -> Vec<ObjectKey> {
        self.entries
            .iter_mut()
            .filter(|(key, entry)| entry.fetching && task.is_none_or(|task| task == *key))
            .map(|(key, entry)| {
                entry.fetching = false;
                entry.awaited = None;
                *key
            })
            .collect()
    }

    /// The prims whose second listing is due at `now`, each with the serial
    /// of the listing held. Each is returned once.
    fn take_due_rechecks(&mut self, now: f32) -> Vec<(ObjectKey, Option<i16>)> {
        self.entries
            .iter_mut()
            .filter_map(|(task, entry)| {
                let awaited = entry.awaited.as_mut()?;
                if awaited.again_at.is_none_or(|at| at > now) {
                    return None;
                }
                awaited.again_at = None;
                Some((*task, entry.serial))
            })
            .collect()
    }

    /// Record that `task`'s listing at `serial` arrived but failed to parse.
    fn store_unreadable(&mut self, task: ObjectKey, serial: i16) {
        let entry = self.entries.entry(task).or_default();
        entry.serial = Some(serial);
        entry.fetching = false;
        entry.unreadable = true;
        entry.awaited = None;
    }

    /// Record an empty task inventory (a reply whose Xfer filename was empty, so
    /// no `TaskInventoryContents` follows).
    fn store_empty(&mut self, task: ObjectKey, serial: i16, now: f32) -> Listed {
        let entry = self.entries.entry(task).or_default();
        entry.items.clear();
        entry.loaded = true;
        entry.unreadable = false;
        entry.settle(serial, now)
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

    /// [`mark_stale`](Self::mark_stale) after a write of this viewer's own:
    /// the listing that comes back is also expected to show the write, and
    /// one that does not is not taken for the answer at once
    /// ([`AwaitedChange`]).
    fn expect_change(&mut self, task: &ObjectKey) {
        if let Some(entry) = self.entries.get_mut(task) {
            entry.fetching = true;
            // A second write while the first is awaited is measured from the
            // same listing.
            if entry.awaited.is_none() {
                entry.awaited = Some(AwaitedChange {
                    from: entry.serial,
                    again_at: None,
                    asked_again: false,
                });
            }
        }
    }

    /// Whether a cached listing is out of date against a serial the simulator
    /// has just reported.
    ///
    /// A prim's contents serial is its **whole** freshness marker, and it
    /// travels only in the properties record — no message announces that a
    /// prim's inventory changed. So a listing cached against a serial the
    /// viewer never saw advance is stale with no way to notice, which is what
    /// this answers ([[viewer-floaters-never-reread-after-a-push]]).
    ///
    /// Compared for inequality, not order: the serial is an `i16` that wraps,
    /// and any disagreement means the listing is not the one the region holds.
    /// A listing that was never loaded is not stale but absent, and one already
    /// being fetched is about to be replaced anyway. One that failed to parse is
    /// stale like a loaded one: a moved serial names a new file worth reading.
    fn is_stale_against(&self, task: &ObjectKey, serial: i16) -> bool {
        self.entries.get(task).is_some_and(|entry| {
            (entry.loaded || entry.unreadable)
                && !entry.fetching
                && entry.serial.is_some_and(|cached| cached != serial)
        })
    }
}

impl ContentsEntry {
    /// Takes a listing at `serial`, which arrived at `now`, for what it
    /// settles.
    fn settle(&mut self, serial: i16, now: f32) -> Listed {
        self.serial = Some(serial);
        if let Some(awaited) = self.awaited.as_mut()
            && awaited.from == Some(serial)
            && !awaited.asked_again
        {
            // Still `fetching`: the second listing is on its way.
            awaited.asked_again = true;
            awaited.again_at = Some(now + CONTENTS_RECHECK_SECONDS);
            return Listed::AskAgain;
        }
        self.awaited = None;
        self.fetching = false;
        Listed::Settled
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
    /// Whether the target's latest listing failed to parse (any earlier good
    /// listing's rows are still shown).
    unreadable: bool,
}

/// One contents list's state: what it shows, which row is picked, and the last
/// click on it. The Build tab has one ([`BuildContents`]); each Object Contents
/// window has its own ([`ContentsWindow`]).
#[derive(Debug, Default)]
struct ContentsPane {
    /// The flattened view, rebuilt from the target and the cache.
    view: ContentsSurfaceView,
    /// The selected item (for open / rename / remove).
    selection: Option<InventoryKey>,
    /// The most recent row click — the item and the time (seconds since
    /// startup) — so a second click on the same item within the app's
    /// double-click window ([`DoubleClickWindow`]) is a double-click Open. Per pane, so a click
    /// in one list and a click in another never make a double-click.
    last_click: Option<(InventoryKey, f32)>,
}

impl ContentsPane {
    /// A primary click on `item` at `now`: select it, and answer whether it
    /// completes a double-click (a second click on the same item at most
    /// `interval_secs` after the first — the app's double-click window). A
    /// double-click is consumed, so a third click starts a new pair.
    fn click(&mut self, item: InventoryKey, now: f32, interval_secs: f64) -> bool {
        self.selection = Some(item);
        let double = self
            .last_click
            .is_some_and(|(prev, at)| prev == item && f64::from(now - at) <= interval_secs);
        self.last_click = if double { None } else { Some((item, now)) };
        double
    }

    /// Whether the selected item has a mutation in flight (so rename / remove
    /// must be blocked to avoid racing the change already sent).
    fn selection_is_pending(&self) -> bool {
        let Some(item) = self.selection else {
            return false;
        };
        self.view
            .rows
            .iter()
            .find(|row| row.item_id == item)
            .is_some_and(|row| row.state.is_pending())
    }

    /// Drop the selection when its item is no longer listed.
    fn keep_selection_valid(&mut self) {
        if let Some(selected) = self.selection
            && !self.view.rows.iter().any(|row| row.item_id == selected)
        {
            self.selection = None;
        }
    }
}

/// The Build tab's pane — a singleton, since the tab is one page of the one
/// Build window. It follows the build selection.
#[derive(Resource, Debug, Default)]
struct BuildContents(ContentsPane);

/// Which pane something belongs to: the Build tab's, or the Object Contents
/// window `.0`'s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneRef {
    /// The Build floater's Content tab.
    BuildTab,
    /// The Object Contents window with this root entity.
    Window(Entity),
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
#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct ContentsTabUi {
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

/// One Object Contents window — a **component on the window root**, since the
/// floater opens per object ([`FloaterKey`]) and everything it shows lives
/// exactly as long as its window. See the
/// [module docs](self#one-object-contents-window-per-object).
#[derive(Component, Debug)]
struct ContentsWindow {
    /// The object this window lists: its region-scoped id (what a fetch
    /// addresses, refreshed on a re-open) and its grid-wide key (the window's
    /// key).
    target: (ScopedObjectId, ObjectKey),
    /// The object's name, latched from the first properties record that names
    /// it — the build selection's, or one the grid sent unasked — so the window
    /// keeps it after the selection moves on.
    name: String,
    /// The virtualized list viewport.
    viewport: Entity,
    /// The object-name line.
    name_text: Entity,
    /// The title bar's text, rewritten with the object's name once it is known
    /// so two windows are tellable apart.
    title_text: Entity,
    /// What this window shows and has picked.
    pane: ContentsPane,
}

/// A request to open the Object Contents floater against a picked object, sent
/// by the object menu's **Open** action (`crate::object_menu`).
#[derive(Message, Debug, Clone, Copy)]
pub struct OpenObjectContents {
    /// The region-scoped id of the picked object.
    pub scoped: ScopedObjectId,
    /// The grid-wide key of the picked object.
    pub full: ObjectKey,
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
pub struct EditContentsPlugin;

impl Plugin for EditContentsPlugin {
    /// Register the cache / views / selection resources, spawn both surfaces,
    /// and run the ingest / fetch / rebuild / bind / action systems.
    fn build(&self, app: &mut App) {
        // This crate's contents actions report a refusal as a local-chat
        // notice, so the channel is registered by the crate that writes it.
        app.add_message::<LocalChatNotice>()
            .init_resource::<TaskInventoryCache>()
            .init_resource::<BuildContents>()
            .init_resource::<PendingMutations>()
            .init_resource::<ContentsRename>()
            .add_message::<OpenObjectContents>()
            .add_message::<ContentsMutated>()
            .add_message::<ContentsActionRequest>()
            // Fills the Build floater's Content page once its lazily-built
            // content publishes the pages (`BuildTabPages` appears on first
            // open); ordered after the general parameter tabs, as before.
            .add_systems(
                Update,
                spawn_contents_tab
                    .run_if(resource_added::<BuildTabPages>)
                    .after(crate::edit_params::spawn_param_tabs),
            )
            // Nothing spawns at `Startup`: an Object Contents window exists only
            // while that object's contents are open, so the open system spawns
            // each instance. It runs after the manager's command pass — see
            // `FloaterSystems`: the pie click that opens a window also raises
            // whatever it landed in, and the later raise wins.
            .add_systems(
                Update,
                open_object_contents_requests
                    .after(FloaterSystems::Commands)
                    .before(ingest_task_inventory),
            )
            .add_systems(
                Update,
                (
                    ingest_task_inventory,
                    latch_window_names,
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

/// Spawn the Content-tab UI into the Build floater's Content page once its
/// pages appear, and publish its handles.
fn spawn_contents_tab(mut commands: Commands, pages: Option<Res<BuildTabPages>>) {
    let Some(pages) = pages else {
        return;
    };
    let ui = spawn_contents_tab_into(&mut commands, pages.content, TOOL_FONT_SIZE, ROW_FONT_SIZE);
    commands.insert_resource(ui);
}

/// Spawn the Content-tab UI into `page` at `font_size`: the count line, the
/// action buttons and the (empty) contents list, whose rows are drawn at
/// `row_font_size`. Shared by the live floater and its gallery specimen.
pub(crate) fn spawn_contents_tab_into(
    commands: &mut Commands,
    page: Entity,
    font_size: f32,
    row_font_size: f32,
) -> ContentsTabUi {
    // Summary line: how many items (and a loading hint).
    let count_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(font_size),
            TextColor(SkinPalette::FALLBACK.text_primary),
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
        commands,
        button_row,
        "build-content-new-script",
        ContentsSurface::BuildTab,
        ContentsAction::NewScript,
        CONTENTS_TAB_INDEX,
        font_size,
    );
    let rename = spawn_contents_button(
        commands,
        button_row,
        "build-content-rename",
        ContentsSurface::BuildTab,
        ContentsAction::Rename,
        CONTENTS_TAB_INDEX + 1,
        font_size,
    );
    let remove = spawn_contents_button(
        commands,
        button_row,
        "build-content-remove",
        ContentsSurface::BuildTab,
        ContentsAction::Remove,
        CONTENTS_TAB_INDEX + 2,
        font_size,
    );
    spawn_contents_button(
        commands,
        button_row,
        "build-content-refresh",
        ContentsSurface::BuildTab,
        ContentsAction::Refresh,
        CONTENTS_TAB_INDEX + 3,
        font_size,
    );

    let viewport = spawn_contents_viewport(
        commands,
        page,
        ContentsSurface::BuildTab,
        CONTENTS_TAB_INDEX + 4,
        row_font_size,
    );

    ContentsTabUi {
        viewport,
        count_text,
        new_script,
        rename,
        remove,
    }
}

/// The object contents floater's [`FloaterSpec`] — shared with the `FLOATERS`
/// registry, so the swept window is the one the viewer spawns.
#[must_use]
pub fn object_contents_floater_spec() -> FloaterSpec {
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
    }
}

/// The [`FloaterKey`] of the Object Contents window listing `object`.
fn contents_window_key(object: ObjectKey) -> FloaterKey {
    FloaterKey::subject(&object)
}

/// Furnish a freshly spawned Object Contents window on `target`: its title
/// (the kind until the object's name is known), its content, and the
/// [`ContentsWindow`] every per-window system finds it by.
fn build_contents_window(
    commands: &mut Commands,
    handle: FloaterHandle,
    target: (ScopedObjectId, ObjectKey),
) {
    commands
        .entity(handle.title_text)
        .insert(Translated::new("object-contents-floater-title"));
    let parts = spawn_open_object_content(commands, handle.content, TOOL_FONT_SIZE, ROW_FONT_SIZE);
    commands.entity(handle.root).insert(ContentsWindow {
        target,
        name: String::new(),
        viewport: parts.viewport,
        name_text: parts.name_text,
        title_text: handle.title_text,
        pane: ContentsPane::default(),
    });
}

/// The Object Contents floater's content entities, what
/// [`spawn_open_object_content`] returns.
#[derive(Debug, Clone, Copy)]
struct OpenObjectParts {
    /// The virtualized list viewport.
    viewport: Entity,
    /// The object-name line.
    name_text: Entity,
}

/// Build the Object Contents floater's content into `slot` at `font_size`:
/// the object-name line, the copy buttons and the (empty) contents list, whose
/// rows are drawn at `row_font_size`. Shared by the live floater and its
/// specimen.
fn spawn_open_object_content(
    commands: &mut Commands,
    slot: Entity,
    font_size: f32,
    row_font_size: f32,
) -> OpenObjectParts {
    let content = commands
        .spawn((
            Node {
                padding: UiRect::all(Val::Px(8.0)),
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                ..column(Val::Px(6.0))
            },
            ChildOf(slot),
        ))
        .id();

    // The object-name line (the reference's `object_name` text box).
    let name_text = commands
        .spawn((
            Text::default(),
            UiFont::Sans.at(font_size),
            TextColor(SkinPalette::FALLBACK.text_primary),
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
        commands,
        button_row,
        "object-contents-copy",
        ContentsSurface::OpenFloater,
        ContentsAction::CopyToInventory,
        10,
        font_size,
    );
    spawn_contents_button(
        commands,
        button_row,
        "object-contents-copy-wear",
        ContentsSurface::OpenFloater,
        ContentsAction::CopyAndWear,
        11,
        font_size,
    );

    let viewport = spawn_contents_viewport(
        commands,
        content,
        ContentsSurface::OpenFloater,
        12,
        row_font_size,
    );

    OpenObjectParts {
        viewport,
        name_text,
    }
}

// ---------------------------------------------------------------------------
// Gallery specimen
// ---------------------------------------------------------------------------

/// The Object Contents floater's gallery / `ui_test` specimen: the live
/// content, built by the same `spawn_open_object_content` at the cell's font
/// size, holding a sample prim's listing. Each row is the list's own pooled
/// row ([`spawn_specimen_row`]), dressed by the same `dress_contents_row`
/// the row pool uses and bound by the same `bind_contents_row`, one selected
/// and one mid-delete; the name line is the same `open_floater_name_line`.
///
/// It inserts no `ContentsWindow`, so the live systems leave it alone.
pub fn spawn_object_contents_specimen(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let parts = spawn_open_object_content(commands, parent, cx.font_size, cx.font_size);
    let rows = sample_contents_rows(cx);
    let row_entities = (0..rows.len())
        .map(|index| {
            let row = spawn_specimen_row(commands, parts.viewport, index, row_height(cx.font_size));
            dress_contents_row(commands, row, ContentsSurface::OpenFloater, cx.font_size);
            row
        })
        .collect::<Vec<_>>();
    let view = ContentsSurfaceView {
        target: Some((
            ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId(1)),
            ObjectKey::from(Uuid::from_u128(0x5eed)),
        )),
        name: cx.text("Sample Door"),
        rows,
        perms: ContentsPerms::default(),
        loading: false,
        unreadable: false,
    };
    let specimen = ContentsSpecimen {
        parts,
        row_entities,
        view,
        selected: 1,
    };
    commands.queue(move |world: &mut World| {
        if let Err(error) = world.run_system_cached_with(bind_contents_specimen, specimen) {
            warn!("object contents specimen: the sample listing was not bound: {error}");
        }
    });
    parent
}

/// A sample prim's listing, the specimens' rows: a notecard, a script, a sound,
/// a texture mid-delete and an object, named through `cx`.
fn sample_contents_rows(cx: crate::ui_element::ElementCx) -> Vec<ContentsRow> {
    [
        (InventoryType::Notecard, "Read Me", RowState::Normal),
        (InventoryType::Script, "Door Script", RowState::Normal),
        (InventoryType::Sound, "Door Chime", RowState::Normal),
        (InventoryType::Texture, "Oak Planks", RowState::Deleting),
        (InventoryType::Object, "Spare Handle", RowState::Normal),
    ]
    .into_iter()
    .zip(1_u128..)
    .map(|((inv_type, name, state), id)| ContentsRow {
        item_id: InventoryKey::from(Uuid::from_u128(id)),
        name: cx.text(name),
        icon: item_icon(inv_type),
        state,
    })
    .collect()
}

/// Fill the Content tab of a Build Tools specimen with a sample prim's
/// listing, as the live systems fill it for the selected prim: each row is the
/// list's own pooled row ([`spawn_specimen_row`]) dressed by
/// [`dress_contents_row`] at `row_font_size` and bound by
/// [`bind_contents_row`], one selected; the count line is
/// [`contents_summary`]'s and the buttons are gated as a modifiable prim the
/// agent owns gates them.
pub(crate) fn fill_contents_tab_specimen(
    commands: &mut Commands,
    ui: ContentsTabUi,
    cx: crate::ui_element::ElementCx,
    row_font_size: f32,
) {
    let rows = sample_contents_rows(cx);
    let row_entities = (0..rows.len())
        .map(|index| {
            let row = spawn_specimen_row(commands, ui.viewport, index, row_height(row_font_size));
            dress_contents_row(commands, row, ContentsSurface::BuildTab, row_font_size);
            row
        })
        .collect::<Vec<_>>();
    let view = ContentsSurfaceView {
        target: Some((
            ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId(1)),
            ObjectKey::from(Uuid::from_u128(0x5eed)),
        )),
        name: cx.text("Sample Box"),
        rows,
        perms: ContentsPerms {
            can_modify: true,
            owns: true,
            allows_drop: false,
        },
        loading: false,
        unreadable: false,
    };
    let specimen = ContentsTabSpecimen {
        ui,
        row_entities,
        view,
        selected: 1,
    };
    commands.queue(move |world: &mut World| {
        if let Err(error) = world.run_system_cached_with(bind_contents_tab_specimen, specimen) {
            warn!("build tools specimen: the sample contents were not bound: {error}");
        }
    });
}

/// What the Content-tab specimen's one-shot binds: the tab, its pooled rows,
/// the sample view they show and which row is selected.
#[derive(Debug)]
struct ContentsTabSpecimen {
    /// The tab's widgets.
    ui: ContentsTabUi,
    /// The pooled rows, in item order.
    row_entities: Vec<Entity>,
    /// The sample listing.
    view: ContentsSurfaceView,
    /// The selected row's index.
    selected: usize,
}

/// The Content-tab specimen's one-shot: bind each pooled row, paint the
/// selection, size the list, write the count line and gate the buttons — what
/// the live bind, paint, rebuild and gate systems do for the selected prim.
fn bind_contents_tab_specimen(
    In(specimen): In<ContentsTabSpecimen>,
    translator: Translator,
    mut rows: Query<(&ContentsRowParts, &mut ClassList), Without<Text>>,
    mut texts: Query<RowText>,
    mut nodes: Query<&mut Node>,
    mut lists: Query<&mut VirtualList>,
    mut commands: Commands,
) {
    let ContentsTabSpecimen {
        ui,
        row_entities,
        view,
        selected,
    } = specimen;
    for (index, row) in row_entities.iter().copied().enumerate() {
        if let Ok((row_parts, mut classes)) = rows.get_mut(row) {
            bind_contents_row(
                row_parts,
                view.rows.get(index),
                &translator,
                &mut texts,
                &mut nodes,
            );
            set_state_class(&mut classes, SELECTED_CLASS, index == selected);
        }
    }
    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        list.item_count = view.rows.len();
    }
    if let Ok((mut text, _color, _classes)) = texts.get_mut(ui.count_text) {
        text.0 = contents_summary(&view, &translator);
    }
    let selection_pending = view
        .rows
        .get(selected)
        .is_some_and(|row| row.state.is_pending());
    set_button_enabled(&mut commands, ui.new_script, view.perms.can_add());
    set_button_enabled(
        &mut commands,
        ui.rename,
        !selection_pending && view.perms.can_rename_menu(),
    );
    set_button_enabled(
        &mut commands,
        ui.remove,
        !selection_pending && view.perms.can_remove_menu(),
    );
}

/// What the specimen's one-shot binds: the content, its pooled rows, the
/// sample view they show and which row is selected.
#[derive(Debug)]
struct ContentsSpecimen {
    /// The floater content.
    parts: OpenObjectParts,
    /// The pooled rows, in item order.
    row_entities: Vec<Entity>,
    /// The sample listing.
    view: ContentsSurfaceView,
    /// The selected row's index.
    selected: usize,
}

/// The specimen's one-shot: bind each pooled row, paint the selection, size
/// the list and write the name line — what the live bind, paint, rebuild and
/// gate systems do for the real view.
fn bind_contents_specimen(
    In(specimen): In<ContentsSpecimen>,
    translator: Translator,
    row_parts: Query<&ContentsRowParts>,
    mut texts: Query<RowText>,
    mut nodes: Query<&mut Node>,
    mut row_classes: Query<&mut ClassList, Without<Text>>,
    mut lists: Query<&mut VirtualList>,
) {
    let ContentsSpecimen {
        parts,
        row_entities,
        view,
        selected,
    } = specimen;
    for (index, row) in row_entities.iter().copied().enumerate() {
        if let Ok(row_parts) = row_parts.get(row) {
            bind_contents_row(
                row_parts,
                view.rows.get(index),
                &translator,
                &mut texts,
                &mut nodes,
            );
        }
        if let Ok(mut classes) = row_classes.get_mut(row) {
            set_state_class(&mut classes, SELECTED_CLASS, index == selected);
        }
    }
    if let Ok(mut list) = lists.get_mut(parts.viewport) {
        list.item_count = view.rows.len();
    }
    if let Ok((mut text, _color, _classes)) = texts.get_mut(parts.name_text) {
        text.0 = open_floater_name_line(&view, &translator);
    }
}

/// Spawn a contents-list viewport (the clipped, focusable virtual-list host)
/// under `parent` for `surface`, windowing rows drawn at `row_font_size`, and
/// return it.
fn spawn_contents_viewport(
    commands: &mut Commands,
    parent: Entity,
    surface: ContentsSurface,
    tab_index: i32,
    row_font_size: f32,
) -> Entity {
    let viewport = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(80.0),
                overflow: Overflow::clip(),
                position_type: PositionType::Relative,
                ..Default::default()
            },
            // The rows' face, from the skin (`--list-bg`): the scrim this
            // used to paint by hand, plus the field-family text roles the
            // class re-roots for a light list. See `LIST_SURFACE_CLASS`.
            ClassList::new_with_classes([LIST_SURFACE_CLASS]),
            VirtualList::new(row_height(row_font_size)),
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
        .id();
    // The list's own scrollbar: the rows stop clear of it while it shows.
    spawn_virtual_scrollbar(commands, viewport);
    viewport
}

/// Spawn one contents action button (a focusable, clickable labelled box) at
/// `font_size`.
fn spawn_contents_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    surface: ContentsSurface,
    action: ContentsAction,
    tab_index: i32,
    font_size: f32,
) -> Entity {
    let button = ui_spawn::spawn_button(
        commands,
        parent,
        ButtonSpec::bordered(
            UiLabel::key(label_key),
            format!("contents-button:{label_key}"),
        )
        .kind(ButtonKind::Headless)
        .tab_index(tab_index)
        // The colour `.sk-build-value` paints — see [`VALUE_CLASS`].
        .label_color(SkinPalette::FALLBACK.text_primary)
        .label_class(VALUE_CLASS)
        .font_size(font_size),
    )
    .button;
    commands.entity(button).observe(
        // `Activate`, not `Pointer<Press>`: the headless button raises it for a
        // primary click and for `Enter` / `Space` on the focused button alike.
        move |activate: On<Activate>,
              host: FloaterHost,
              requests: Option<MessageWriter<ContentsActionRequest>>| {
            // Absent only in the gallery, whose specimen has no contents to act on.
            let Some(mut requests) = requests else {
                return;
            };
            // A window's button acts on *its* window's object.
            let pane = match surface {
                ContentsSurface::BuildTab => PaneRef::BuildTab,
                ContentsSurface::OpenFloater => match host.of(activate.entity) {
                    Some(window) => PaneRef::Window(window),
                    None => return,
                },
            };
            requests.write(ContentsActionRequest { pane, action });
        },
    );
    button
}

/// The pane a contents viewport of `surface` belongs to: the Build tab's, or
/// the window among `windows` — `(window, its viewport)` pairs — whose list it
/// is. `None` for a viewport no live pane owns: a gallery specimen's.
fn pane_of_viewport(
    surface: ContentsSurface,
    viewport: Entity,
    mut windows: impl Iterator<Item = (Entity, Entity)>,
) -> Option<PaneRef> {
    match surface {
        ContentsSurface::BuildTab => Some(PaneRef::BuildTab),
        ContentsSurface::OpenFloater => windows
            .find_map(|(window, list)| (list == viewport).then_some(window))
            .map(PaneRef::Window),
    }
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
    objects: Res<ObjectState>,
    time: Res<Time>,
    mut commands: MessageWriter<SlCommand>,
) {
    let now = time.elapsed_secs();
    // The second listing of a prim whose first came back unchanged after a
    // write: asked with the serial held, so a grid with nothing new to say
    // says only that.
    for (task, held_serial) in cache.take_due_rechecks(now) {
        match objects.scoped_by_full_id(task.uuid()).first() {
            Some(scoped) => {
                commands.write(SlCommand(Command::FetchTaskInventory {
                    target: *scoped,
                    held_serial,
                }));
            }
            // The prim left the scene while the answer was awaited.
            None => {
                if let Some(serial) = held_serial {
                    let _settled = cache.store_unchanged(task, serial, now);
                }
                pending.clear_object(&task);
            }
        }
    }
    for event in events.read() {
        match &event.0 {
            // A properties record is the only thing that carries a prim's
            // contents serial, and one arrives unasked whenever somebody else
            // changes a prim this viewer holds selected. A serial that has
            // moved under a cached listing is the only sign that the listing is
            // stale, so it is re-fetched — the old items stay on screen until
            // the reply lands, exactly as after this viewer's own mutation.
            SlSessionEvent::ObjectProperties(properties)
                if cache.is_stale_against(&properties.object_id, properties.inventory_serial) =>
            {
                if let Some(scoped) = objects
                    .scoped_by_full_id(properties.object_id.uuid())
                    .first()
                {
                    refetch(&mut cache, &mut commands, *scoped, properties.object_id);
                }
            }
            SlSessionEvent::TaskInventoryContents {
                task,
                serial,
                items,
            } => {
                // The authoritative listing supersedes every optimistic
                // overlay — unless it predates the write the overlay is for.
                if cache.store(*task, *serial, items.clone(), now) == Listed::Settled {
                    pending.clear_object(task);
                }
            }
            SlSessionEvent::TaskInventoryUnchanged { task, serial } => {
                if cache.store_unchanged(*task, *serial, now) == Listed::Settled {
                    pending.clear_object(task);
                }
            }
            SlSessionEvent::TaskInventoryReply(TaskInventoryReply {
                task,
                serial,
                filename,
            }) if filename.is_empty() => {
                if cache.store_empty(*task, *serial, now) == Listed::Settled {
                    pending.clear_object(task);
                }
            }
            // No listing is coming. The rows stop saying a change is in
            // flight: what the grid last listed is what there is to show.
            SlSessionEvent::TaskInventoryUnanswered { task } => {
                for given_up in cache.give_up(task.as_ref()) {
                    pending.clear_object(&given_up);
                }
            }
            // A reply nobody asked for: Second Life sends one to a viewer
            // holding a prim selected when a script in it finishes compiling,
            // which moves the serial a second time. The reply to a fetch of
            // this viewer's own arrives while that fetch is in flight, and is
            // not stale against it.
            SlSessionEvent::TaskInventoryReply(TaskInventoryReply { task, serial, .. })
                if cache.is_stale_against(task, *serial) =>
            {
                if let Some(scoped) = objects.scoped_by_full_id(task.uuid()).first() {
                    refetch(&mut cache, &mut commands, *scoped, *task);
                }
            }
            // The session has already logged the parse error; here it only has
            // to stop the list waiting for a listing that is not coming. The
            // pending overlays go too: their reconcile is what failed, so they
            // would otherwise claim "refreshing" for good.
            SlSessionEvent::XferDecodeFailed {
                file: XferListing::TaskInventory { task, serial },
                ..
            } => {
                cache.store_unreadable(*task, *serial);
                pending.clear_object(task);
            }
            _other => {}
        }
    }
}

/// Open an object's contents **per object** (`viewer-key-object-contents`):
/// raise its window when it is already up, and otherwise spawn one listing it.
///
/// Every open of the frame is honoured, not just the last. A window already up
/// keeps its selection and scroll — only its region-scoped id is refreshed,
/// since that is what the key exists to outlive (an object re-announced after a
/// region change carries a new local id).
fn open_object_contents_requests(
    mut requests: MessageReader<OpenObjectContents>,
    mut floaters: KeyedFloaters,
    mut windows: Query<&mut ContentsWindow>,
    mut commands: Commands,
) {
    for request in requests.read() {
        let target = (request.scoped, request.full);
        match floaters.open(
            object_contents_floater_spec(),
            contents_window_key(request.full),
        ) {
            KeyedFloaterOpen::Spawned(handle) => {
                build_contents_window(&mut commands, handle, target);
            }
            KeyedFloaterOpen::Existing(window) => {
                if let Ok(mut window) = windows.get_mut(window)
                    && window.target != target
                {
                    window.target = target;
                }
            }
        }
    }
}

/// Latch each Object Contents window's object name from whatever names it: the
/// build selection's properties when the window's object is selected, or a
/// properties record the grid sent. Latched rather than read live, so a window
/// keeps its name once the selection has moved on to another object.
fn latch_window_names(
    mut events: MessageReader<SlEvent>,
    selection: Res<SelectionSet>,
    mut windows: Query<&mut ContentsWindow>,
) {
    let mut learn = |object: &ObjectKey, name: &str| {
        for mut window in &mut windows {
            if window.target.1 == *object && !name.is_empty() && window.name != name {
                name.clone_into(&mut window.name);
            }
        }
    };
    if let Some(primary) = selection.primary()
        && let Some(properties) = primary.properties.as_ref()
    {
        learn(&primary.full, &properties.name);
    }
    for event in events.read() {
        if let SlSessionEvent::ObjectProperties(properties) = &event.0 {
            learn(&properties.object_id, &properties.name);
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
    build: Res<BuildContents>,
    windows: Query<&ContentsWindow>,
    mut cache: ResMut<TaskInventoryCache>,
    mut commands: MessageWriter<SlCommand>,
) {
    let targets = core::iter::once(build.0.view.target)
        .chain(windows.iter().map(|window| window.pane.view.target));
    for (scoped, full) in targets.flatten() {
        let needs_fetch = cache
            .get(&full)
            .is_none_or(|entry| !entry.loaded && !entry.fetching && !entry.unreadable);
        if needs_fetch && cache.begin_fetch(full) {
            commands.write(SlCommand(Command::FetchTaskInventory {
                target: scoped,
                held_serial: None,
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// Rebuild the flattened views
// ---------------------------------------------------------------------------

/// The object-contents model, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam): the object properties every
/// permission gate reads, the server listing cache, and the optimistic
/// pending-mutation overlay each view is rendered through.
#[derive(bevy::ecs::system::SystemParam)]
struct ContentsStore<'w> {
    /// Object properties and permissions.
    objects: Res<'w, ObjectState>,
    /// The server's per-object task-inventory listing.
    cache: ResMut<'w, TaskInventoryCache>,
    /// The in-flight mutations layered over it.
    pending: ResMut<'w, PendingMutations>,
}

/// Every contents pane — the Build tab's and each Object Contents window's —
/// and the Build tab's inline rename, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct ContentsModel<'w, 's> {
    /// The Build tab's pane.
    build: ResMut<'w, BuildContents>,
    /// The Object Contents windows, each holding its own pane.
    windows: Query<'w, 's, &'static mut ContentsWindow>,
    /// The in-place rename, if one is open (the Build tab's alone).
    rename: ResMut<'w, ContentsRename>,
}

impl ContentsModel<'_, '_> {
    /// The pane `pane` names, if it still exists.
    fn pane(&self, pane: PaneRef) -> Option<&ContentsPane> {
        match pane {
            PaneRef::BuildTab => Some(&self.build.0),
            PaneRef::Window(window) => self.windows.get(window).ok().map(|window| &window.pane),
        }
    }
}

/// What a contents action sends and how its text is worded, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct ContentsOutbox<'w> {
    /// The translator every notice goes through.
    translator: Translator<'w>,
    /// The outgoing simulator commands.
    commands: MessageWriter<'w, SlCommand>,
    /// The local-chat notices a refused action writes.
    notices: MessageWriter<'w, LocalChatNotice>,
    /// Opens for a notecard picked out of contents.
    notecard_opens: MessageWriter<'w, crate::intents::OpenNotecard>,
    /// Opens for a script picked out of contents.
    script_opens: MessageWriter<'w, crate::intents::OpenScript>,
}

/// The in-place rename row's widgets, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam).
#[derive(bevy::ecs::system::SystemParam)]
struct RenameWidgets<'w, 's> {
    /// The virtual rows and their parts, to find the renamed row.
    rows: Query<'w, 's, (&'static VirtualRow, &'static ContentsRowParts)>,
    /// The rename field's text.
    fields: Query<'w, 's, &'static EditableText>,
    /// The row parts' layout, swapped between label and field.
    nodes: Query<'w, 's, &'static mut Node>,
    /// What focuses and unfocuses the field.
    commands: Commands<'w, 's>,
}

/// Recompute every pane's view from its target and the cache, and set each
/// list's item count. Cheap when nothing changed: the Build tab's pane only
/// rebuilds on a change to the selection, the tool state or the cache, and a
/// window's on those or a change to the window itself (opened, re-targeted, a
/// name latched).
fn rebuild_contents_views(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    store: ContentsStore,
    tab_ui: Option<Res<ContentsTabUi>>,
    mut model: ContentsModel,
    mut lists: Query<&mut VirtualList>,
) {
    let shared_changed = selection.is_changed()
        || tool.is_changed()
        || store.cache.is_changed()
        || store.pending.is_changed();

    if shared_changed {
        // The Build tab follows the primary selection while the tool is open.
        let build_target = if tool.active {
            selection.primary().map(|node| (node.scoped, node.full))
        } else {
            None
        };
        // The primary selection's `ObjectProperties` name — it *is* the target.
        let build_name = selection
            .primary()
            .and_then(|primary| primary.properties.as_ref())
            .map_or("", |properties| properties.name.as_str());
        let pane = &mut model.build.0;
        rebuild_one_view(
            &mut pane.view,
            build_target,
            build_name,
            &store.objects,
            &store.cache,
            &store.pending,
        );
        pane.keep_selection_valid();
        if let Some(ui) = &tab_ui {
            sync_item_count(&mut lists, ui.viewport, pane.view.rows.len());
        }
    }

    for mut window in &mut model.windows {
        if !(shared_changed || window.is_changed()) {
            continue;
        }
        let window = &mut *window;
        rebuild_one_view(
            &mut window.pane.view,
            Some(window.target),
            &window.name,
            &store.objects,
            &store.cache,
            &store.pending,
        );
        window.pane.keep_selection_valid();
        sync_item_count(&mut lists, window.viewport, window.pane.view.rows.len());
    }
}

/// Set `viewport`'s list to `count` rows, back at the top when the count moved.
fn sync_item_count(lists: &mut Query<&mut VirtualList>, viewport: Entity, count: usize) {
    if let Ok(mut list) = lists.get_mut(viewport)
        && list.item_count != count
    {
        list.item_count = count;
        list.scroll_to_top();
    }
}

/// Keep each pane's viewport [`ContentsDropTarget`] pointed at the object its
/// view shows — but only when the agent may **add** to it — so an inventory drag
/// dropped on the list adds to that prim's contents (and is refused otherwise).
///
/// [`ContentsDropTarget`]: crate::inventory_drag::ContentsDropTarget
fn sync_contents_drop_targets(
    build: Res<BuildContents>,
    tab_ui: Option<Res<ContentsTabUi>>,
    windows: Query<Ref<ContentsWindow>>,
    mut drop_targets: Query<&mut crate::inventory_drag::ContentsDropTarget>,
) {
    let mut apply = |viewport: Entity, view: &ContentsSurfaceView| {
        if let Ok(mut target) = drop_targets.get_mut(viewport) {
            let resolved = view.target.filter(|_target| view.perms.can_add());
            if target.target != resolved {
                target.target = resolved;
            }
        }
    };
    if build.is_changed()
        && let Some(ui) = tab_ui
    {
        apply(ui.viewport, &build.0.view);
    }
    for window in &windows {
        if window.is_changed() {
            apply(window.viewport, &window.pane.view);
        }
    }
}

/// Rebuild one pane's view of `target` (named `name`, when known) from the
/// cache and the pending mutation overlay: the server's listing forms the base
/// rows, each item's pending mutation (if any) recolours / relabels its row, and
/// pending **adds** (not yet in the listing) append phantom rows.
fn rebuild_one_view(
    view: &mut ContentsSurfaceView,
    target: Option<(ScopedObjectId, ObjectKey)>,
    name: &str,
    objects: &ObjectState,
    cache: &TaskInventoryCache,
    pending: &PendingMutations,
) {
    view.target = target;
    view.rows.clear();
    view.name.clear();
    view.perms = ContentsPerms::default();
    view.loading = false;
    view.unreadable = false;
    let Some((scoped, full)) = target else {
        return;
    };
    view.perms = ContentsPerms::resolve(objects, &scoped);
    name.clone_into(&mut view.name);
    view.unreadable = cache.get(&full).is_some_and(|entry| entry.unreadable);
    let Some(entry) = cache.get(&full).filter(|entry| entry.loaded) else {
        view.loading = !view.unreadable;
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
        dress_contents_row(&mut commands, row_entity, surface, ROW_FONT_SIZE);
    }
}

/// The resources a contents row's click reads and writes, bundled as one
/// [`SystemParam`](bevy::ecs::system::SystemParam) so the row observer can take
/// it as an `Option`: absent only in the gallery, whose specimen rows have no
/// contents model behind them.
#[derive(bevy::ecs::system::SystemParam)]
struct ContentsRowClick<'w, 's> {
    /// The Build tab's pane.
    build: ResMut<'w, BuildContents>,
    /// The Object Contents windows, each with its own pane.
    windows: Query<'w, 's, (Entity, &'static mut ContentsWindow)>,
    /// The clock the double-click window is measured on.
    time: Res<'w, Time>,
    /// The app's double-click window.
    double_click: DoubleClickWindow<'w>,
    /// Where a double-click's Open goes.
    requests: MessageWriter<'w, ContentsActionRequest>,
    /// Focused on a click, so the wheel scrolls the list.
    focus: ResMut<'w, InputFocus>,
}

/// Build the inner structure of one pooled contents row of `surface` at
/// `font_size` — the icon and name labels — and wire its click-to-select
/// observer. Shared by the live row pool and the specimen's rows.
fn dress_contents_row(
    commands: &mut Commands,
    row_entity: Entity,
    surface: ContentsSurface,
    font_size: f32,
) {
    // Amended, not inserted: `top` and `display` are the virtual list's.
    let height = row_height(font_size);
    amend_row_node(commands, row_entity, move |node| {
        node.position_type = PositionType::Absolute;
        node.left = Val::Px(0.0);
        node.right = Val::Px(0.0);
        node.height = Val::Px(height);
        node.align_items = AlignItems::Center;
        node.column_gap = Val::Px(4.0);
    });
    // A list row the skin paints — its resting face, its hover and, through
    // `paint_contents_selection`, its selection.
    commands.entity(row_entity).insert((
        Pickable::default(),
        BackgroundColor(Color::NONE),
        ClassList::new_with_classes([LIST_ROW_CLASS]),
    ));
    let icon = commands
        .spawn((
            Text::new(""),
            UiFont::Sans.at(font_size),
            text_role(ROW_TEXT),
            // Never given back under a long name: a shrunk column slices
            // its glyph.
            Node {
                min_width: Val::Px(ICON_COL_WIDTH),
                flex_shrink: 0.0,
                ..Default::default()
            },
            Pickable::IGNORE,
            ChildOf(row_entity),
        ))
        .id();
    // The name sits in a shrink-and-clip container with a no-wrap `Text`
    // child, so a name wider than the row draws on one line with its tail
    // hidden behind the `…` marker rather than wrapping out of its
    // fixed-height row — the inventory row's own ellipsis cell.
    let label_clip = commands
        .spawn((
            label_clip_node(),
            ContentMayOverflow {
                reason: LABEL_CLIP_REASON,
            },
            TextMayClip {
                reason: LABEL_CLIP_REASON,
            },
            Pickable::IGNORE,
            ChildOf(row_entity),
        ))
        .id();
    let label = commands
        .spawn((
            Text::new(""),
            TextLayout::no_wrap(),
            UiFont::Sans.at(font_size),
            text_role(ROW_TEXT),
            // The text keeps its full width; the clip is what shrinks.
            Node {
                flex_shrink: 0.0,
                ..Default::default()
            },
            Pickable::IGNORE,
            ChildOf(label_clip),
        ))
        .id();
    let ellipsis =
        spawn_ellipsis_marker(commands, row_entity, font_size, ROW_TEXT, FALLBACK_ELLIPSIS);
    commands
        .entity(label_clip)
        .insert(RevealEllipsis { marker: ellipsis });
    commands
        .entity(row_entity)
        .insert(ContentsRowParts { icon, label })
        .observe(
            move |press: On<Pointer<Press>>,
                  rows: Query<(&VirtualRow, &ChildOf)>,
                  click: Option<ContentsRowClick>| {
                if press.button != PointerButton::Primary {
                    return;
                }
                let Some(ContentsRowClick {
                    mut build,
                    mut windows,
                    time,
                    double_click,
                    mut requests,
                    mut focus,
                }) = click
                else {
                    return;
                };
                let Ok((row, child_of)) = rows.get(press.entity) else {
                    return;
                };
                // The row's own pane — its window's, never another window's.
                let Some(pane_ref) = pane_of_viewport(
                    surface,
                    child_of.parent(),
                    windows.iter().map(|(window, it)| (window, it.viewport)),
                ) else {
                    return;
                };
                let pane = match pane_ref {
                    PaneRef::BuildTab => &mut build.0,
                    PaneRef::Window(window) => match windows.get_mut(window) {
                        Ok((_window, it)) => &mut it.into_inner().pane,
                        Err(_) => return,
                    },
                };
                let Some(item) = row
                    .index
                    .and_then(|index| pane.view.rows.get(index))
                    .map(|display| display.item_id)
                else {
                    return;
                };
                // Clicking the list focuses it so the wheel scrolls it.
                focus.set(press.entity, FocusCause::Navigated);
                // A second primary click on the same item within the
                // double-click window opens it (the reference's openItem).
                if pane.click(item, time.elapsed_secs(), double_click.interval_secs()) {
                    requests.write(ContentsActionRequest {
                        pane: pane_ref,
                        action: ContentsAction::Open,
                    });
                }
            },
        );
}

/// Bind each pooled contents row to the item it now points at, appending its
/// pending-state suffix ("…adding" / "…deleting" / "…refreshing") and greying it
/// while a mutation is in flight.
fn bind_contents_rows(
    build: Res<BuildContents>,
    windows: Query<(Entity, Ref<ContentsWindow>)>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &ContentsRowParts)>,
    viewports: Query<&ContentsViewport>,
    translator: Translator,
    mut texts: Query<RowText>,
    mut nodes: Query<&mut Node>,
) {
    for (row, child_of, parts) in &rows {
        let Ok(&ContentsViewport(surface)) = viewports.get(child_of.parent()) else {
            continue;
        };
        // Only the panes whose view moved rebind every row.
        let (pane, pane_changed) = match surface {
            ContentsSurface::BuildTab => (&build.0, build.is_changed()),
            ContentsSurface::OpenFloater => {
                let Some(window) = windows.iter().find_map(|(_entity, window)| {
                    (window.viewport == child_of.parent()).then_some(window)
                }) else {
                    continue;
                };
                let changed = window.is_changed();
                (&window.into_inner().pane, changed)
            }
        };
        if !pane_changed && !row.is_changed() {
            continue;
        }
        let bound = row.index.and_then(|index| pane.view.rows.get(index));
        bind_contents_row(parts, bound, &translator, &mut texts, &mut nodes);
    }
}

/// Bind one pooled row's parts to `bound` (or blank it when unbound): its
/// icon, its name with any pending-state suffix, and the pending grey. The
/// per-row half of [`bind_contents_rows`], which the specimen calls too.
fn bind_contents_row(
    parts: &ContentsRowParts,
    bound: Option<&ContentsRow>,
    translator: &Translator,
    texts: &mut Query<RowText>,
    nodes: &mut Query<&mut Node>,
) {
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
        if let Ok((mut text, _color, _classes)) = texts.get_mut(parts.icon) {
            set_row_text(&mut text, "");
        }
        return;
    };
    // A pending (in-flight) item draws greyed until the server confirms it —
    // the role, so the skin decides what greyed is on a list's face.
    let color = if display.state.is_pending() {
        PENDING_ROW_TEXT
    } else {
        ROW_TEXT
    };
    if let Ok((mut text, mut text_color, classes)) = texts.get_mut(parts.icon) {
        set_row_text(&mut text, display.icon);
        set_row_role(&mut text_color, classes, color);
    }
    if let Ok((mut text, mut text_color, classes)) = texts.get_mut(parts.label) {
        // Append the pending-state suffix, e.g. "Read me   …deleting".
        let label = match display.state.suffix_key() {
            Some(key) => format!("{}   {}", display.name, translator.get(key)),
            None => display.name.clone(),
        };
        set_row_text(&mut text, &label);
        set_row_role(&mut text_color, classes, color);
    }
}

/// Mark each pooled row selected or not: [`SELECTED_CLASS`] beside its
/// [`LIST_ROW_CLASS`], so the skin paints the selection as it paints every
/// other list's.
fn paint_contents_selection(
    build: Res<BuildContents>,
    windows: Query<&ContentsWindow>,
    viewports: Query<&ContentsViewport>,
    mut rows: Query<(&VirtualRow, &ChildOf, &mut ClassList)>,
) {
    for (row, child_of, mut classes) in &mut rows {
        let Ok(&ContentsViewport(surface)) = viewports.get(child_of.parent()) else {
            continue;
        };
        let pane = match surface {
            ContentsSurface::BuildTab => &build.0,
            ContentsSurface::OpenFloater => {
                match windows
                    .iter()
                    .find(|window| window.viewport == child_of.parent())
                {
                    Some(window) => &window.pane,
                    // A specimen's rows: its own one-shot paints them.
                    None => continue,
                }
            }
        };
        let selected = row
            .index
            .and_then(|index| pane.view.rows.get(index))
            .is_some_and(|display| pane.selection == Some(display.item_id));
        set_state_class(&mut classes, SELECTED_CLASS, selected);
    }
}

/// What a contents row's icon and name are bound through: the text, its
/// fallback colour, and the class list its role is kept on (absent on the
/// count and name lines, which share the query).
type RowText = (
    &'static mut Text,
    &'static mut TextColor,
    Option<&'static mut ClassList>,
);

/// Put a row text on `color`'s role: the class the skin paints it by, and the
/// fallback colour beside it. Each written only when it moves, so a row that
/// rebinds to the same state does not wake the style engine.
fn set_row_role(
    text_color: &mut Mut<'_, TextColor>,
    classes: Option<Mut<'_, ClassList>>,
    color: Color,
) {
    if text_color.0 != color {
        text_color.0 = color;
    }
    if let Some(mut classes) = classes {
        set_role_class(&mut classes, color);
    }
}

/// Write `value` into a row text node only when it changed (avoids re-measuring).
fn set_row_text(text: &mut Mut<'_, Text>, value: &str) {
    if text.0 != value {
        value.clone_into(&mut text.0);
    }
}

// ---------------------------------------------------------------------------
// Count line + button gating
// ---------------------------------------------------------------------------

/// Keep the Content-tab count line current and gate the action buttons by the
/// resolved permissions / selection; keep each Object Contents window's name
/// line and title current.
///
/// The window offers no permission-gated buttons — its Copy actions are the
/// simulator's to arbitrate per item — so its object's permissions gate only
/// its drop target ([`sync_contents_drop_targets`]), per window.
fn gate_contents_buttons(
    tab_ui: Option<Res<ContentsTabUi>>,
    build: Res<BuildContents>,
    windows: Query<&ContentsWindow>,
    translator: Translator,
    mut texts: Query<&mut Text>,
    mut last_enabled: Local<Option<[bool; 3]>>,
    mut commands: Commands,
) {
    for window in &windows {
        if let Ok(mut text) = texts.get_mut(window.name_text) {
            let want = open_floater_name_line(&window.pane.view, &translator);
            if text.0 != want {
                text.0 = want;
            }
        }
        // The title carries the object's name once it is known (a plain
        // string, so the translator stops owning it), the way About Land's
        // carries its parcel's.
        if !window.name.is_empty()
            && let Ok(mut title) = texts.get_mut(window.title_text)
            && title.0 != window.name
        {
            window.name.clone_into(&mut title.0);
            commands.entity(window.title_text).remove::<Translated>();
        }
    }
    if let Some(ui) = &tab_ui {
        let pane = &build.0;
        let view = &pane.view;
        if let Ok(mut text) = texts.get_mut(ui.count_text) {
            let want = contents_summary(view, &translator);
            if text.0 != want {
                text.0 = want;
            }
        }
        let has_target = view.target.is_some();
        // A pending (in-flight) selection cannot be re-edited until it clears.
        let selected = pane.selection.is_some() && !pane.selection_is_pending();
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
}

/// The Object Contents floater's name line for `view`: "none" without a
/// target, else the object's name (when known) and the count summary.
fn open_floater_name_line(view: &ContentsSurfaceView, translator: &Translator) -> String {
    if view.target.is_none() {
        translator.get("object-contents-none")
    } else if view.name.is_empty() {
        contents_summary(view, translator)
    } else {
        format!("{} — {}", view.name, contents_summary(view, translator))
    }
}

/// The "N items" (or loading / empty) summary for a view.
fn contents_summary(view: &ContentsSurfaceView, translator: &Translator) -> String {
    if view.target.is_none() {
        return translator.get("build-content-no-target");
    }
    if view.unreadable {
        return translator.get("build-content-unreadable");
    }
    if view.loading {
        return translator.get("build-content-loading");
    }
    let count = i64::try_from(view.rows.len()).unwrap_or(i64::MAX);
    translator.format("build-content-count", &TransArgs::new().int("count", count))
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

/// **F2** renames and **Delete / Backspace** removes the selected Content-tab
/// item — but only while the contents list is the focused widget, so the same
/// keys over the inventory list or the world hit their own handlers instead. The
/// work itself reuses the Rename / Remove actions (with their permission gates).
fn contents_hotkeys(
    keyboard: Res<ButtonInput<KeyCode>>,
    context: Res<InputContext>,
    focus: Res<InputFocus>,
    tab_ui: Option<Res<ContentsTabUi>>,
    build: Res<BuildContents>,
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
    if !focused_here || build.0.selection.is_none() {
        return;
    }
    if keyboard.just_pressed(KeyCode::F2) {
        requests.write(ContentsActionRequest {
            pane: PaneRef::BuildTab,
            action: ContentsAction::Rename,
        });
    }
    if keyboard.just_pressed(KeyCode::Delete) || keyboard.just_pressed(KeyCode::Backspace) {
        requests.write(ContentsActionRequest {
            pane: PaneRef::BuildTab,
            action: ContentsAction::Remove,
        });
    }
}

/// A contents action fired by a header button, a hotkey or a double-click.
#[derive(Message, Debug, Clone, Copy)]
struct ContentsActionRequest {
    /// The pane the action acts on — the Build tab, or the window it was
    /// fired in.
    pane: PaneRef,
    /// Which action.
    action: ContentsAction,
}

/// Dispatch a fired contents action to its command(s).
fn run_contents_actions(
    mut requests: MessageReader<ContentsActionRequest>,
    identity: Res<SlIdentity>,
    inventory: Res<InventoryModel>,
    mut store: ContentsStore,
    mut model: ContentsModel,
    mut outbox: ContentsOutbox,
) {
    for request in requests.read() {
        // What the pane the action was fired in shows and has picked — never
        // another pane's.
        let Some((Some((scoped, full)), perms, selected)) = model
            .pane(request.pane)
            .map(|pane| (pane.view.target, pane.view.perms, pane.selection))
        else {
            continue;
        };
        match request.action {
            ContentsAction::Refresh => {
                refetch(&mut store.cache, &mut outbox.commands, scoped, full);
            }
            ContentsAction::Open => {
                let Some(item_id) = selected else {
                    continue;
                };
                let Some(item) = store
                    .cache
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
                // A redacted (nil) asset id is the grid withholding it. A
                // notecard's cannot be fetched without it; a script's is read
                // by the item (`edit_script`'s `fetch_command`), and Second
                // Life withholds every script's, even from its owner, so a
                // script opens with the nil id as the reference opens it.
                let asset_id = match item.asset_id {
                    Some(asset_id) => asset_id.uuid(),
                    None if item.inv_type == InventoryType::Script => Uuid::nil(),
                    None => {
                        outbox.notices.write(LocalChatNotice::new(
                            outbox.translator.get("build-content-no-modify"),
                        ));
                        continue;
                    }
                };
                // Editable needs BOTH the object's modify and the item's own
                // modify bit (the reference's two-level rule); otherwise the
                // item opens read-only.
                let editable = perms.can_modify && item_modifiable(item);
                if item.inv_type == InventoryType::Script {
                    outbox.script_opens.write(crate::intents::OpenScript {
                        name: item.name.clone(),
                        asset_id,
                        editable,
                        source: crate::intents::ScriptSource::Task {
                            task_id: full,
                            item_id,
                        },
                        target: crate::intents::target_for(
                            sl_client_bevy::ScriptLanguage::from_item_flags(item.flags),
                        ),
                    });
                } else {
                    outbox.notecard_opens.write(crate::intents::OpenNotecard {
                        name: item.name.clone(),
                        asset_id,
                        editable,
                        source: crate::intents::NotecardSource::Task {
                            task_id: full,
                            item_id,
                        },
                    });
                }
            }
            ContentsAction::NewScript => {
                if !perms.can_add() {
                    outbox.notices.write(LocalChatNotice::new(
                        outbox.translator.get("build-content-no-modify"),
                    ));
                    continue;
                }
                let Some(creator) = identity.agent_id else {
                    continue;
                };
                let name = outbox.translator.get("build-content-new-script-name");
                // A fresh id keys the "…adding" phantom until the simulator (which
                // assigns the real task id) confirms the new script in the listing.
                let phantom_id = InventoryKey::from(Uuid::new_v4());
                let item = RestoreItem::new_script(creator, full, &name, Uuid::new_v4());
                // Running, as the reference makes one
                // (`LLPanelContents::onClickNewScript`): Second Life leaves a
                // script made with the flag off stopped.
                outbox.commands.write(SlCommand(Command::RezScript {
                    target: scoped,
                    params: Box::new(RezScriptParams {
                        group_id: None,
                        enabled: true,
                        item,
                    }),
                }));
                store.pending.set(
                    full,
                    phantom_id,
                    PendingKind::Adding {
                        name,
                        icon: item_icon(InventoryType::Script),
                    },
                );
                reconcile_after_mutation(&mut store.cache, &mut outbox.commands, scoped, full);
            }
            ContentsAction::Rename => {
                if let Some(item) = selected
                    && !store.pending.is_pending(&full, &item)
                {
                    model.rename.pending = Some(item);
                }
            }
            ContentsAction::Remove => {
                let Some(item_id) = selected else {
                    continue;
                };
                // Already in flight — do not race the change already sent.
                if store.pending.is_pending(&full, &item_id) {
                    continue;
                }
                if !perms.can_remove_menu() {
                    continue;
                }
                // The reference offers Remove to an owner but only *applies* it
                // with object modify — an owner-without-modify gets the notice.
                if !perms.can_modify {
                    outbox.notices.write(LocalChatNotice::new(
                        outbox.translator.get("build-content-no-modify"),
                    ));
                    continue;
                }
                outbox
                    .commands
                    .write(SlCommand(Command::RemoveTaskInventory {
                        target: scoped,
                        item_id,
                    }));
                store.pending.set(full, item_id, PendingKind::Deleting);
                reconcile_after_mutation(&mut store.cache, &mut outbox.commands, scoped, full);
            }
            ContentsAction::CopyToInventory | ContentsAction::CopyAndWear => {
                copy_contents_out(
                    &full,
                    scoped,
                    &store.cache,
                    &inventory,
                    request.action == ContentsAction::CopyAndWear,
                    &mut outbox,
                );
                // Moving a no-copy item out empties it from the prim, so reconcile
                // the prim's own cache against what actually left.
                reconcile_after_mutation(&mut store.cache, &mut outbox.commands, scoped, full);
            }
        }
    }
}

/// After sending a task-inventory mutation, re-fetch the prim's listing so the
/// cache reconciles against the **server's** authoritative state (see
/// [`TaskInventoryCache::mark_stale`]). The old listing stays visible until the
/// reply lands, so there is no "loading" flash and a rejected mutation simply
/// leaves the contents unchanged rather than drifting.
///
/// The listing is also *expected* to have moved: one that comes back at the
/// serial the prim stood at is asked for once more before it is believed
/// ([`AwaitedChange`]), and until then the row keeps saying what was asked.
fn reconcile_after_mutation(
    cache: &mut TaskInventoryCache,
    commands: &mut MessageWriter<SlCommand>,
    scoped: ScopedObjectId,
    full: ObjectKey,
) {
    cache.expect_change(&full);
    commands.write(SlCommand(Command::FetchTaskInventory {
        target: scoped,
        held_serial: None,
    }));
}

/// Re-read a prim's listing because the grid said it changed, or the resident
/// asked: whatever comes back is its contents.
fn refetch(
    cache: &mut TaskInventoryCache,
    commands: &mut MessageWriter<SlCommand>,
    scoped: ScopedObjectId,
    full: ObjectKey,
) {
    cache.mark_stale(&full);
    commands.write(SlCommand(Command::FetchTaskInventory {
        target: scoped,
        held_serial: None,
    }));
}

/// Copy an object's whole contents into the agent's inventory (the Open
/// floater's Copy actions) by moving each item into the system Objects folder.
/// A no-copy item is moved out of the prim; a copyable one is copied — the
/// simulator arbitrates per item's permissions.
fn copy_contents_out(
    full: &ObjectKey,
    scoped: ScopedObjectId,
    cache: &TaskInventoryCache,
    inventory: &InventoryModel,
    wear: bool,
    outbox: &mut ContentsOutbox,
) {
    let Some(folder) = inventory.folder_by_type(FolderType::Object) else {
        outbox.notices.write(LocalChatNotice::new(
            outbox.translator.get("object-contents-no-folder"),
        ));
        return;
    };
    let Some(entry) = cache.get(full) else {
        return;
    };
    for item in &entry.items {
        outbox.commands.write(SlCommand(Command::MoveTaskInventory {
            target: scoped,
            folder_id: folder,
            item_id: item.item_id,
        }));
    }
    // "Copy And Wear" wearing is deferred: the moved items arrive asynchronously
    // as inventory updates, so a follow-up wear needs the new agent-inventory
    // ids — surfaced to the user for now rather than silently dropped.
    if wear {
        outbox.notices.write(LocalChatNotice::new(
            outbox.translator.get("object-contents-wear-note"),
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
    build: Res<BuildContents>,
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
    let view = &build.0.view;
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
fn drive_contents_rename(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut widgets: RenameWidgets,
    mut store: ContentsStore,
    mut model: ContentsModel,
    mut outbox: ContentsOutbox,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        model.rename.pending = None;
        if let Some(active) = model.rename.active.take() {
            end_contents_rename(
                &active,
                &widgets.rows,
                &mut widgets.nodes,
                &mut widgets.commands,
            );
        }
        return;
    }
    let Some(active) = model.rename.active else {
        return;
    };
    let view = &model.build.0.view;
    // Cancel when the hosting row no longer shows the renamed item.
    let still_bound = widgets
        .rows
        .get(active.row)
        .is_ok_and(|(row, _parts)| row.index == Some(active.index))
        && view
            .rows
            .get(active.index)
            .is_some_and(|display| display.item_id == active.item_id);
    if !still_bound {
        model.rename.active = None;
        end_contents_rename(
            &active,
            &widgets.rows,
            &mut widgets.nodes,
            &mut widgets.commands,
        );
        return;
    }
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let new_name = widgets
        .fields
        .get(active.field)
        .map(|field| field.value().to_string().trim().to_owned())
        .unwrap_or_default();
    let sent = commit_contents_rename(
        active.item_id,
        &new_name,
        view,
        &store.cache,
        &outbox.translator,
        &mut outbox.commands,
        &mut outbox.notices,
    );
    // Reconcile the prim's cache against the server after a rename we sent, so a
    // rejected rename reverts to the real name instead of drifting; show the new
    // name flagged "…refreshing" meanwhile, and block a second rename of it.
    if sent && let Some((scoped, full)) = view.target {
        store
            .pending
            .set(full, active.item_id, PendingKind::Renaming(new_name));
        reconcile_after_mutation(&mut store.cache, &mut outbox.commands, scoped, full);
    }
    model.rename.active = None;
    end_contents_rename(
        &active,
        &widgets.rows,
        &mut widgets.nodes,
        &mut widgets.commands,
    );
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

    /// A contents serial that moved under a cached listing is the only sign the
    /// listing is stale, and it is now read.
    ///
    /// `inventory_serial` rides only in `ObjectProperties`; nothing announces
    /// that another resident changed a prim's contents. Before this, the serial
    /// was stored and never compared, so a viewer holding a prim's contents open
    /// showed a listing the region no longer had and could not tell.
    #[test]
    fn a_moved_contents_serial_makes_a_cached_listing_stale() {
        use super::{ObjectKey, TaskInventoryCache};

        let task = ObjectKey::from(sl_client_bevy::Uuid::from_bytes([7; 16]));
        let mut cache = TaskInventoryCache::default();

        // Nothing cached: absent, not stale — the fetch driver's job, not this.
        assert!(!cache.is_stale_against(&task, 3));

        let _settled = cache.store_empty(task, 3, 0.0);
        assert!(
            !cache.is_stale_against(&task, 3),
            "the serial the listing was fetched at is not a change"
        );
        assert!(cache.is_stale_against(&task, 4));

        // The serial is an i16 and wraps, so any disagreement counts — a
        // wrapped-around serial is still a listing this viewer never saw.
        assert!(cache.is_stale_against(&task, i16::MIN));

        // Already being re-fetched: the reply is about to replace it anyway.
        cache.mark_stale(&task);
        assert!(!cache.is_stale_against(&task, 4));
    }

    /// A listing that failed to parse stops the wait without inviting a
    /// re-fetch loop: the same file would fail again, so it is read again only
    /// once the prim's contents serial moves.
    #[test]
    fn an_unreadable_listing_is_retried_only_when_its_serial_moves() {
        use super::{ObjectKey, TaskInventoryCache};

        let task = ObjectKey::from(sl_client_bevy::Uuid::from_bytes([9; 16]));
        let mut cache = TaskInventoryCache::default();
        assert!(cache.begin_fetch(task));
        cache.store_unreadable(task, 5);

        let entry = cache
            .get(&task)
            .map(|entry| (entry.loaded, entry.fetching, entry.unreadable));
        assert_eq!(entry, Some((false, false, true)));
        assert!(
            !cache.is_stale_against(&task, 5),
            "the same file fails the same way"
        );
        assert!(
            cache.is_stale_against(&task, 6),
            "a moved serial names a new file"
        );

        let _settled = cache.store(task, 6, Vec::new(), 0.0);
        assert!(
            cache
                .get(&task)
                .is_some_and(|entry| entry.loaded && !entry.unreadable)
        );
    }

    /// **A write that does not land leaves the listing as the grid has it,
    /// and is not mistaken for one that has not landed yet.**
    ///
    /// The listing asked for after a write can be answered before the write
    /// is, so the first listing at the old serial is not the verdict: the
    /// prim is asked once more, a moment later. A second answer at the old
    /// serial — the grid's `304`, or the same listing again — is.
    #[test]
    fn an_unchanged_listing_after_a_write_is_asked_for_once_more() {
        use super::{CONTENTS_RECHECK_SECONDS, Listed, ObjectKey, TaskInventoryCache};

        let task = ObjectKey::from(sl_client_bevy::Uuid::from_bytes([3; 16]));
        let mut cache = TaskInventoryCache::default();
        assert_eq!(cache.store(task, 4, Vec::new(), 0.0), Listed::Settled);

        // A write, and a listing that has not caught up with it.
        cache.expect_change(&task);
        assert_eq!(cache.store(task, 4, Vec::new(), 10.0), Listed::AskAgain);
        assert!(
            cache.get(&task).is_some_and(|entry| entry.fetching),
            "the row keeps saying a change is in flight"
        );
        assert_eq!(cache.take_due_rechecks(10.5), Vec::new());
        let due = 10.0 + CONTENTS_RECHECK_SECONDS;
        assert_eq!(cache.take_due_rechecks(due), vec![(task, Some(4))]);
        assert_eq!(
            cache.take_due_rechecks(due + 1.0),
            Vec::new(),
            "it is asked for once"
        );

        // The second answer is the verdict, whichever way it goes.
        assert_eq!(cache.store_unchanged(task, 4, due + 0.1), Listed::Settled);
        assert!(cache.get(&task).is_some_and(|entry| !entry.fetching));

        // A write that lands is settled by the first listing that shows it.
        cache.expect_change(&task);
        assert_eq!(cache.store(task, 5, Vec::new(), 20.0), Listed::Settled);
        assert_eq!(cache.take_due_rechecks(30.0), Vec::new());

        // A listing nobody wrote for is believed at once.
        cache.mark_stale(&task);
        assert_eq!(cache.store(task, 5, Vec::new(), 40.0), Listed::Settled);
    }

    /// **A contents action button acts from the keyboard as from the mouse.**
    ///
    /// The buttons used to observe `Pointer<Press>`, so `Tab` reached them and
    /// `Enter` / `Space` did nothing (`viewer-floater-buttons-ignore-keyboard-
    /// activate`). Each gesture in a fresh app, driven through the real input
    /// and focus stack, must raise the same one request.
    #[test]
    fn a_contents_button_acts_on_enter_space_and_a_click() -> Result<(), String> {
        use super::{
            ContentsAction, ContentsActionRequest, ContentsSurface, PaneRef, spawn_contents_button,
        };
        use crate::ui::{UiRoot, UiScaffoldSystems};
        use bevy::input::keyboard::Key;
        use bevy::prelude::*;
        use sl_viewer_testkit::interact::{self, InteractionTest};
        use sl_viewer_testkit::{drain, find_by_name, record, settle};

        /// The Refresh button's node name, per `spawn_contents_button`.
        const REFRESH: &str = "contents-button:build-content-refresh";

        /// The requests one gesture on the Refresh button raised.
        fn requests_after(
            gesture: fn(&mut App, Entity) -> Result<(), String>,
        ) -> Result<Vec<(PaneRef, ContentsAction)>, String> {
            let mut app = InteractionTest::new().build();
            record::<ContentsActionRequest>(&mut app);
            app.add_systems(
                Startup,
                (|mut commands: Commands, root: Res<UiRoot>| {
                    spawn_contents_button(
                        &mut commands,
                        root.0,
                        "build-content-refresh",
                        ContentsSurface::BuildTab,
                        ContentsAction::Refresh,
                        1,
                        13.0,
                    );
                })
                .after(UiScaffoldSystems::SpawnRoot),
            );
            settle(&mut app);
            let button = find_by_name(&mut app, REFRESH).ok_or("no Refresh button")?;
            let _spawned = drain::<ContentsActionRequest>(&mut app);
            gesture(&mut app, button)?;
            settle(&mut app);
            Ok(drain::<ContentsActionRequest>(&mut app)
                .into_iter()
                .map(|request| (request.pane, request.action))
                .collect())
        }

        let want = vec![(PaneRef::BuildTab, ContentsAction::Refresh)];
        assert_eq!(
            requests_after(|app, _button| interact::click_node(app, REFRESH))?,
            want,
            "a primary click"
        );
        assert_eq!(
            requests_after(|app, button| {
                interact::focus(app, button);
                interact::tap(app, KeyCode::Enter, Key::Enter);
                Ok(())
            })?,
            want,
            "Enter on the focused button"
        );
        assert_eq!(
            requests_after(|app, button| {
                interact::focus(app, button);
                interact::tap(app, KeyCode::Space, Key::Space);
                Ok(())
            })?,
            want,
            "Space on the focused button"
        );
        Ok(())
    }

    /// **One Object Contents window per object** (`viewer-key-object-contents`),
    /// run through the whole plugin and the floater manager.
    ///
    /// What the keying buys is not two windows for their own sake — it is that
    /// one object's listing, selection and actions cannot reach the other's,
    /// which the singleton floater repointed on every Open.
    mod instances {
        use super::super::{
            ContentsAction, ContentsActionRequest, ContentsPane, ContentsWindow,
            EditContentsPlugin, OpenObjectContents, PaneRef, TaskInventoryCache,
            contents_window_key,
        };
        use super::task_item;
        use crate::floater::{Floater, FloaterCommand, FloaterOp, FloaterPlugin, host_floater};
        use crate::intents::{NotecardSource, OpenNotecard, OpenScript};
        use crate::inventory::InventoryModel;
        use crate::ui::UiRoot;
        use crate::world_api::{EditToolState, InputContext, ObjectState, SelectionSet};
        use bevy::input_focus::InputFocus;
        use bevy::prelude::*;
        use bevy::ui_widgets::Activate;
        use pretty_assertions::assert_eq;
        use sl_client_bevy::{
            CircuitId, Command, InventoryKey, ObjectKey, RegionLocalObjectId, ScopedObjectId,
            SlCommand, SlEvent, SlIdentity, Uuid,
        };
        use sl_viewer_testkit::{drain, record};

        /// A boxed error so tests can use `?` rather than the disallowed
        /// `unwrap` / `expect`.
        type TestError = Box<dyn core::error::Error>;

        /// The first object's id.
        const FIRST: u32 = 0xA1;

        /// The second object's id.
        const SECOND: u32 = 0xB2;

        /// The object `id` stands for: its region-scoped id and its grid-wide
        /// key.
        fn object(id: u32) -> (ScopedObjectId, ObjectKey) {
            (
                ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId(id)),
                ObjectKey::from(Uuid::from_u128(u128::from(id))),
            )
        }

        /// The item id `id` stands for.
        fn item_key(id: u128) -> InventoryKey {
            InventoryKey::from(Uuid::from_u128(id))
        }

        /// An app with the floater manager and the whole contents plugin over
        /// default world state; strings resolve to their keys.
        fn contents_app() -> App {
            let mut app = App::new();
            app.add_message::<SlCommand>()
                .add_message::<SlEvent>()
                .add_message::<OpenNotecard>()
                .add_message::<OpenScript>()
                .init_resource::<UiScale>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<Time>()
                .init_resource::<InputFocus>()
                .init_resource::<InputContext>()
                .init_resource::<ObjectState>()
                .init_resource::<SelectionSet>()
                .init_resource::<EditToolState>()
                .init_resource::<SlIdentity>()
                .init_resource::<InventoryModel>()
                .add_plugins((FloaterPlugin, EditContentsPlugin));
            crate::i18n::install_untranslated(&mut app);
            record::<SlCommand>(&mut app);
            record::<OpenNotecard>(&mut app);
            record::<ContentsActionRequest>(&mut app);
            let root = app.world_mut().spawn(Node::default()).id();
            app.insert_resource(UiRoot(root));
            app.update();
            app
        }

        /// Open object `id`'s contents, under the region-scoped id `scoped`,
        /// the way the object pie does — by writing the message.
        fn open_as(app: &mut App, scoped: ScopedObjectId, id: u32) {
            app.world_mut().write_message(OpenObjectContents {
                scoped,
                full: object(id).1,
            });
            app.update();
        }

        /// Open object `id`'s contents.
        fn open(app: &mut App, id: u32) {
            open_as(app, object(id).0, id);
        }

        /// Every live contents window, as (window, object key) pairs.
        fn windows(app: &mut App) -> Vec<(Entity, ObjectKey)> {
            app.world_mut()
                .query::<(Entity, &ContentsWindow)>()
                .iter(app.world())
                .map(|(window, it)| (window, it.target.1))
                .collect()
        }

        /// The window listing object `id`.
        fn window_of(app: &mut App, id: u32) -> Result<Entity, TestError> {
            windows(app)
                .into_iter()
                .find_map(|(window, key)| (key == object(id).1).then_some(window))
                .ok_or_else(|| TestError::from(format!("object {id:#x} has no window")))
        }

        /// The contents window component on `window`.
        fn window(app: &App, window: Entity) -> Result<&ContentsWindow, TestError> {
            app.world()
                .get::<ContentsWindow>(window)
                .ok_or_else(|| TestError::from("the window has no contents"))
        }

        /// Have the grid list `items` — `(item id, name)` notecards — for
        /// object `id`.
        fn list(app: &mut App, id: u32, items: &[(u128, &str)]) {
            let items = items
                .iter()
                .map(|(item, name)| task_item(*item, name))
                .collect();
            let _settled = app.world_mut().resource_mut::<TaskInventoryCache>().store(
                object(id).1,
                1,
                items,
                0.0,
            );
            app.update();
        }

        /// Select item `item` in `window`'s list, as a row click does.
        fn select(app: &mut App, window: Entity, item: u128) -> Result<(), TestError> {
            app.world_mut()
                .get_mut::<ContentsWindow>(window)
                .ok_or("the window has no contents")?
                .pane
                .selection = Some(item_key(item));
            Ok(())
        }

        /// The object every `FetchTaskInventory` recorded since the last
        /// drain addressed.
        fn fetched(app: &mut App) -> Vec<ScopedObjectId> {
            // The recorder runs unordered in `Update`: one more frame lets it
            // see what the last one wrote.
            app.update();
            drain::<SlCommand>(app)
                .into_iter()
                .filter_map(|command| match command.0 {
                    Command::FetchTaskInventory { target, .. } => Some(target),
                    _other => None,
                })
                .collect()
        }

        /// Two objects are two windows, each on its own object under its own
        /// key, each with its own list.
        #[test]
        fn two_objects_open_two_windows() -> Result<(), TestError> {
            let mut app = contents_app();
            open(&mut app, FIRST);
            open(&mut app, SECOND);

            assert_eq!(
                windows(&mut app).len(),
                2,
                "the second object repointed the first window"
            );
            let first = window_of(&mut app, FIRST)?;
            let second = window_of(&mut app, SECOND)?;
            assert!(
                window(&app, first)?.viewport != window(&app, second)?.viewport,
                "both windows list into one viewport"
            );
            let key_of = |window: Entity| {
                app.world()
                    .get::<Floater>(window)
                    .and_then(Floater::key)
                    .cloned()
            };
            assert_eq!(key_of(first), Some(contents_window_key(object(FIRST).1)));
            assert_eq!(key_of(second), Some(contents_window_key(object(SECOND).1)));
            Ok(())
        }

        /// Each window lists its own object, and each object is asked for
        /// once — the cache is shared, the windows are not.
        #[test]
        fn each_window_lists_its_own_object() -> Result<(), TestError> {
            let mut app = contents_app();
            let _startup = fetched(&mut app);
            open(&mut app, FIRST);
            open(&mut app, SECOND);
            let mut asked = fetched(&mut app);
            asked.sort_by_key(|scoped| scoped.id().0);
            assert_eq!(asked, vec![object(FIRST).0, object(SECOND).0]);

            list(&mut app, FIRST, &[(1, "First Note")]);
            list(&mut app, SECOND, &[(2, "Second Note"), (3, "Another")]);
            let names = |app: &mut App, id: u32| -> Result<Vec<String>, TestError> {
                let window = window_of(app, id)?;
                Ok(window_names(app, window))
            };
            assert_eq!(names(&mut app, FIRST)?, vec!["First Note"]);
            assert_eq!(names(&mut app, SECOND)?, vec!["Second Note", "Another"]);
            Ok(())
        }

        /// The names `window` lists, in order.
        fn window_names(app: &App, window: Entity) -> Vec<String> {
            app.world()
                .get::<ContentsWindow>(window)
                .map(|it| {
                    it.pane
                        .view
                        .rows
                        .iter()
                        .map(|row| row.name.clone())
                        .collect()
                })
                .unwrap_or_default()
        }

        /// **Re-opening an object that is up raises its window and keeps its
        /// selection** — only the region-scoped id moves, which is what the
        /// grid-wide key exists to outlive.
        #[test]
        fn reopening_an_object_keeps_its_window_and_selection() -> Result<(), TestError> {
            let mut app = contents_app();
            open(&mut app, FIRST);
            list(&mut app, FIRST, &[(1, "First Note")]);
            let first = window_of(&mut app, FIRST)?;
            select(&mut app, first, 1)?;

            let moved = ScopedObjectId::new(CircuitId::new(2), RegionLocalObjectId(0x77));
            open_as(&mut app, moved, FIRST);
            assert_eq!(windows(&mut app).len(), 1, "a re-open spawned a window");
            let kept = window(&app, first)?;
            assert_eq!(kept.pane.selection, Some(item_key(1)));
            assert_eq!(kept.target, (moved, object(FIRST).1));
            Ok(())
        }

        /// Closing one window ends it and leaves the other open.
        #[test]
        fn closing_one_window_leaves_the_other() -> Result<(), TestError> {
            let mut app = contents_app();
            open(&mut app, FIRST);
            open(&mut app, SECOND);
            let first = window_of(&mut app, FIRST)?;

            app.world_mut()
                .resource_mut::<Messages<FloaterCommand>>()
                .write(FloaterCommand {
                    floater: first,
                    op: FloaterOp::Close,
                });
            app.update();

            let left: Vec<ObjectKey> = windows(&mut app).into_iter().map(|(_w, key)| key).collect();
            assert_eq!(left, vec![object(SECOND).1]);
            Ok(())
        }

        /// **An action acts on the window it was fired in**: Open in the
        /// second window opens the second window's selection out of the
        /// second object — not the first window's, which has its own.
        #[test]
        fn an_open_acts_on_its_own_windows_selection() -> Result<(), TestError> {
            let mut app = contents_app();
            open(&mut app, FIRST);
            open(&mut app, SECOND);
            list(&mut app, FIRST, &[(1, "First Note")]);
            list(&mut app, SECOND, &[(2, "Second Note")]);
            let first = window_of(&mut app, FIRST)?;
            let second = window_of(&mut app, SECOND)?;
            select(&mut app, first, 1)?;
            select(&mut app, second, 2)?;
            let _before = drain::<OpenNotecard>(&mut app);

            app.world_mut().write_message(ContentsActionRequest {
                pane: PaneRef::Window(second),
                action: ContentsAction::Open,
            });
            app.update();
            app.update();

            let opened: Vec<(String, NotecardSource)> = drain::<OpenNotecard>(&mut app)
                .into_iter()
                .map(|open| (open.name, open.source))
                .collect();
            assert_eq!(
                opened,
                vec![(
                    String::from("Second Note"),
                    NotecardSource::Task {
                        task_id: object(SECOND).1,
                        item_id: item_key(2),
                    }
                )]
            );
            Ok(())
        }

        /// **A window's button acts on its own window**: Copy To Inventory
        /// pressed in the second window asks for the second window's object.
        #[test]
        fn a_windows_button_names_its_own_window() -> Result<(), TestError> {
            let mut app = contents_app();
            open(&mut app, FIRST);
            open(&mut app, SECOND);
            let second = window_of(&mut app, SECOND)?;
            let copy = copy_button(&mut app, second)?;
            let _before = drain::<ContentsActionRequest>(&mut app);

            app.world_mut().trigger(Activate {
                entity: copy,
                button: None,
            });
            app.update();
            app.update();

            let asked: Vec<(PaneRef, ContentsAction)> = drain::<ContentsActionRequest>(&mut app)
                .into_iter()
                .map(|request| (request.pane, request.action))
                .collect();
            assert_eq!(
                asked,
                vec![(PaneRef::Window(second), ContentsAction::CopyToInventory)]
            );
            Ok(())
        }

        /// The parent links and windows `host_floater` walks up through.
        type HostLookup<'world, 'state> = (
            Query<'world, 'state, &'static ChildOf>,
            Query<'world, 'state, (Entity, &'static Floater)>,
        );

        /// The Copy To Inventory button built into `window`.
        fn copy_button(app: &mut App, window: Entity) -> Result<Entity, TestError> {
            let buttons: Vec<Entity> = app
                .world_mut()
                .query::<(Entity, &Name)>()
                .iter(app.world())
                .filter(|(_button, name)| name.as_str() == "contents-button:object-contents-copy")
                .map(|(button, _name)| button)
                .collect();
            let mut system_state: bevy::ecs::system::SystemState<HostLookup<'_, '_>> =
                bevy::ecs::system::SystemState::new(app.world_mut());
            let (parents, floaters) = system_state.get(app.world())?;
            buttons
                .into_iter()
                .find(|button| host_floater(*button, &parents, &floaters) == Some(window))
                .ok_or_else(|| TestError::from("the window has no Copy button"))
        }

        /// **A double-click is two clicks in one pane**: a click in each of two
        /// panes on the same item is no double-click, and a click in one
        /// selects only there.
        #[test]
        fn clicks_in_two_panes_never_make_a_double_click() {
            /// The double-click window the clicks are judged by.
            const WINDOW: f64 = 0.4;
            let mut first = ContentsPane::default();
            let mut second = ContentsPane::default();
            let item = item_key(1);

            assert!(
                !first.click(item, 0.0, WINDOW),
                "a first click is no double-click"
            );
            assert!(
                !second.click(item, 0.1, WINDOW),
                "a click in another pane completed the first pane's double-click"
            );
            assert_eq!(first.selection, Some(item));
            assert!(first.click(item, 0.2, WINDOW), "two clicks in one pane");
            assert!(
                !first.click(item, 0.3, WINDOW),
                "a double-click is consumed: a third click starts a new pair"
            );
            assert!(
                !second.click(item_key(2), 0.4, WINDOW),
                "a click on another item is no double-click"
            );
            assert_eq!(second.selection, Some(item_key(2)));
            assert_eq!(first.selection, Some(item));
        }
    }
}
