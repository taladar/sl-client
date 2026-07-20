//! The **inventory window** — the folder tree, the Everything / Recent / Worn
//! tabs, and the search bar (`viewer-inventory-folder-tree`,
//! `viewer-inventory-outfit-tab`, `viewer-inventory-search-filter`).
//!
//! A floating panel, toggled with `Ctrl+I`, that presents the agent's inventory
//! the way the reference viewer (Firestorm) does:
//!
//! - **Everything** — the folder **tree**: expand / collapse folders (per row,
//!   or all at once with the toolbar buttons), each item drawn with an **icon by
//!   its inventory type**. Folders come from the login-seeded skeleton, so the
//!   whole tree structure is known immediately; a folder's **items** are fetched
//!   lazily the first time it is expanded.
//! - **Recent** — items received since login (an inventory offer accepted, a
//!   copy, a give), newest first.
//! - **Worn** — the Current Outfit Folder's contents (the modern authoritative
//!   worn set on Second Life), falling back to the legacy `AgentWearables` set
//!   on a grid that does not populate the COF.
//! - **Search** — a text field that narrows the shown rows to items (and
//!   folders) whose name matches. Type-and-date filters are a follow-up
//!   ([`viewer-inventory-search-filter`]); this is the name filter.
//!
//! # It talks to the model, never the wire
//!
//! The whole window sits on the **high-level inventory bridge** in
//! `sl-client-bevy`: it sends [`Command::QueryInventoryFolders`] /
//! [`Command::QueryInventoryFolder`] and reads
//! [`SlSessionEvent::InventoryFolders`] / [`SlSessionEvent::InventoryFolderPage`]
//! and the push events. `QueryInventoryFolder` **auto-schedules a fetch through
//! the session's own background fetcher / disk cache** when a folder is not yet
//! loaded — so this module never touches a UDP message or a CAPS request itself,
//! and reuses the caching the session already does.
//!
//! # Virtualized, so size does not matter
//!
//! The tree is flattened to a linear list of visible rows and drawn through the
//! recycling [`crate::virtual_list`], so a 10 000-item inventory scrolls at the
//! cost of the viewport, not the item count. The flattening
//! ([`InventoryModel::build_rows`]) is a pure function tested in isolation; the
//! Bevy half only turns its output into pooled row entities.

use std::collections::{HashMap, HashSet};

use bevy::input_focus::tab_navigation::TabIndex;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use sl_client_bevy::{
    Command, FolderInfo, FolderState, FolderType, InventoryFolder, InventoryFolderKey,
    InventoryKey, InventoryType, ItemInfo, SlCommand, SlEvent, SlSessionEvent, Wearable,
    WearableType,
};

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::menu::{MenuCommand, MenuDef, MenuItemDef};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::ui_tab::{DEFAULT_ELLIPSIS, TabPlacement, TabSpec, TabStrip, spawn_tab_strip};
use crate::virtual_list::{VirtualList, VirtualRow, VirtualViewport, layout_virtual_lists};

/// The uniform height of a tree row, in logical pixels. Drives the virtualized
/// list's windowing.
const ROW_HEIGHT: f32 = 22.0;

/// How far each tree depth level indents a row, in logical pixels.
const INDENT_PER_DEPTH: f32 = 16.0;

/// The minimum width of a row's expand-arrow column, in logical pixels. A
/// `min_width` (not a fixed width) so a folder's arrow and a leaf's blank align
/// the icons below them in the common case, while a wider glyph at a large font
/// grows the column rather than being clipped.
const ARROW_COL_WIDTH: f32 = 12.0;

/// The minimum width of a row's type-icon column, in logical pixels — a
/// `min_width`, so labels line up in the common case but a wide emoji glyph
/// grows the column instead of clipping.
const ICON_COL_WIDTH: f32 = 20.0;

/// The window's bounded width, in logical pixels — a bound, not a fixed size.
const PANEL_WIDTH: f32 = 340.0;

/// The scrolling viewport's height, in logical pixels. A **definite** height is
/// correct here — a scroll viewport is exactly the case the content-sizing
/// convention carves out, the way a text editor bounds its `visible_lines`.
const VIEWPORT_HEIGHT: f32 = 420.0;

/// The narrowest the floater's content area may be resized to, in logical pixels
/// — enough for the Everything / Recent / Worn tab row and the expand / collapse
/// toolbar to sit without being clipped.
const INVENTORY_MIN_WIDTH: f32 = 260.0;

/// The shortest the floater's content area may be resized to, in logical pixels —
/// enough for the tabs, toolbar and search plus a few list rows.
const INVENTORY_MIN_HEIGHT: f32 = 200.0;

/// The most recent-tab entries to keep. Firestorm's Recent tab is likewise a
/// bounded running list of what arrived this session.
const RECENT_LIMIT: usize = 200;

/// The default page size requested for a folder's contents — large enough that a
/// normal folder arrives in one page (pagination past this is a follow-up).
const FOLDER_PAGE_LIMIT: usize = 4096;

/// The window title / toolbar text colour.
const CHROME_COLOR: Color = Color::srgb(0.86, 0.89, 0.95);

/// A row label's colour.
const LABEL_COLOR: Color = Color::srgb(0.90, 0.92, 0.96);

/// A folder row label's colour — a touch warmer than an item, so the tree's
/// structure reads at a glance.
const FOLDER_LABEL_COLOR: Color = Color::srgb(0.98, 0.86, 0.55);

/// An inactive toolbar button background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// A button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The chrome font size, in logical pixels.
const CHROME_FONT_SIZE: f32 = 14.0;

/// A tree row's font size, in logical pixels.
const ROW_FONT_SIZE: f32 = 14.0;

/// The plugin that owns the inventory window.
pub(crate) struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    /// Wire up the window: its resources, its spawn (after the scaffold root
    /// exists), and the systems that fold events into the model, react to the
    /// toolbar / search, rebuild the flattened view, and bind the pooled rows.
    fn build(&self, app: &mut App) {
        app.init_resource::<InventoryModel>()
            .init_resource::<InventoryState>()
            .init_resource::<InventoryView>()
            .add_message::<InventoryUiAction>()
            .add_systems(
                Startup,
                spawn_inventory_panel.after(UiScaffoldSystems::SpawnRoot),
            )
            // The model half runs before the generic list recycles its pool (so a
            // rebuilt view / new item count is in place first); the row populate
            // and bind run after (so freshly-recycled rows exist to fill this
            // frame, no flicker).
            .add_systems(
                Update,
                (
                    toggle_inventory,
                    refresh_inventory_on_show,
                    ingest_inventory,
                    bridge_tab_selection,
                    route_gear_menu,
                    apply_ui_actions,
                    read_search_field,
                    rebuild_view,
                )
                    .chain()
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (populate_new_rows, bind_rows)
                    .chain()
                    .after(layout_virtual_lists),
            );
    }
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// The held inventory read-model that backs the window: the folder tree (from
/// the login skeleton), each expanded folder's fetched items, the recent and
/// worn sets, and the expand state.
///
/// Everything here is fed by the high-level bridge events; nothing here reaches
/// for the wire.
#[derive(Resource, Default)]
pub(crate) struct InventoryModel {
    /// Every known folder, by key.
    folders: HashMap<InventoryFolderKey, FolderInfo>,
    /// Child folder keys per parent, kept sorted by name.
    child_folders: HashMap<InventoryFolderKey, Vec<InventoryFolderKey>>,
    /// The tree roots (folders with no parent): the agent's "My Inventory" first,
    /// then the read-only "Library".
    roots: Vec<InventoryFolderKey>,
    /// Folders belonging to the read-only shared Library tree, distinct from the
    /// agent's own — the read-only flag a future context menu must honour (there
    /// are no mutation actions yet, so nothing is gated on it today).
    library_folders: HashSet<InventoryFolderKey>,
    /// The fetched items of each folder, by folder key.
    items: HashMap<InventoryFolderKey, Vec<ItemInfo>>,
    /// Folders whose contents have already been requested, so a re-expand does
    /// not re-request.
    requested: HashSet<InventoryFolderKey>,
    /// The currently expanded folders (Everything tab).
    expanded: HashSet<InventoryFolderKey>,
    /// Items received since login (Recent tab), newest first.
    recent: Vec<RecentItem>,
    /// Keys of items already in [`recent`](Self::recent), to dedupe re-pushes.
    recent_seen: HashSet<InventoryKey>,
    /// The Current Outfit Folder, once located in the skeleton.
    cof: Option<InventoryFolderKey>,
    /// The legacy worn-wearables set, a fallback for the Worn tab on grids that
    /// do not populate the COF.
    wearables: Vec<Wearable>,
    /// Whether the folder skeleton snapshot has arrived at least once.
    folders_loaded: bool,
}

/// A Recent-tab entry: enough to draw the row, resolved from the pushed item.
#[derive(Debug, Clone)]
struct RecentItem {
    /// The item key, for dedupe.
    key: InventoryKey,
    /// The item name.
    name: String,
    /// The inventory type, for the icon.
    inv_type: InventoryType,
}

impl InventoryModel {
    /// The child folders of `parent`, or an empty slice when it has none.
    fn children_of(&self, parent: InventoryFolderKey) -> &[InventoryFolderKey] {
        self.child_folders.get(&parent).map_or(&[], Vec::as_slice)
    }

    /// The fetched items of `folder`, or an empty slice when none are loaded.
    fn items_of(&self, folder: InventoryFolderKey) -> &[ItemInfo] {
        self.items.get(&folder).map_or(&[], Vec::as_slice)
    }

    /// Merge a batch of folders into the tree, then rebuild the index.
    ///
    /// Merging rather than replacing is what lets the agent tree (from
    /// [`SlSessionEvent::InventoryFolders`]) and the Library tree (from
    /// [`SlSessionEvent::LibraryInventory`]) — which arrive from different
    /// events — coexist, and lets a fetched page fill in a Library subtree the
    /// login skeleton did not carry. `library` tags the batch as read-only shared
    /// Library folders. Also locates the Current Outfit Folder.
    fn merge_folders(&mut self, infos: &[FolderInfo], library: bool) {
        for info in infos {
            self.folders.insert(info.folder_id, info.clone());
            if library {
                self.library_folders.insert(info.folder_id);
            }
            if info.folder_type == FolderType::CurrentOutfit {
                self.cof = Some(info.folder_id);
            }
        }
        self.reindex();
    }

    /// Merge a raw-wire folder batch (the Library login skeleton), resolving each
    /// folder's type from its wire byte, as read-only Library folders.
    fn merge_library_folders(&mut self, folders: &[InventoryFolder]) {
        let infos: Vec<FolderInfo> = folders
            .iter()
            .map(|folder| FolderInfo {
                folder_id: folder.folder_id,
                parent_id: folder.parent_id,
                name: folder.name.clone(),
                folder_type: FolderType::from_code(folder.folder_type),
                version: folder.version,
                state: FolderState::Unknown,
            })
            .collect();
        self.merge_folders(&infos, true);
    }

    /// Rebuild the parent→children index and the sorted root list from the folder
    /// map. Roots order agent-tree first (so "My Inventory" sits above the
    /// read-only "Library"), then by name; each child list is sorted by name.
    fn reindex(&mut self) {
        self.child_folders.clear();
        let mut roots: Vec<InventoryFolderKey> = Vec::new();
        for info in self.folders.values() {
            match info.parent_id {
                Some(parent) => self
                    .child_folders
                    .entry(parent)
                    .or_default()
                    .push(info.folder_id),
                None => roots.push(info.folder_id),
            }
        }
        let names = &self.folders;
        let library = &self.library_folders;
        roots.sort_by_key(|key| {
            (
                library.contains(key),
                names
                    .get(key)
                    .map(|info| info.name.to_lowercase())
                    .unwrap_or_default(),
            )
        });
        self.roots = roots;
        for list in self.child_folders.values_mut() {
            list.sort_by_key(|key| {
                names
                    .get(key)
                    .map(|info| info.name.to_lowercase())
                    .unwrap_or_default()
            });
        }
    }

    /// Store a fetched page of a folder's items (replacing any earlier page),
    /// sorted by name.
    fn set_items(&mut self, folder: InventoryFolderKey, items: &[ItemInfo]) {
        let mut owned: Vec<ItemInfo> = items.to_vec();
        owned.sort_by_key(|item| item.name.to_lowercase());
        self.items.insert(folder, owned);
    }

    /// Record an item as recently received, newest first, deduped and bounded.
    fn push_recent(&mut self, key: InventoryKey, name: String, inv_type: InventoryType) {
        if self.recent_seen.contains(&key) {
            return;
        }
        self.recent_seen.insert(key);
        self.recent.insert(
            0,
            RecentItem {
                key,
                name,
                inv_type,
            },
        );
        self.recent.truncate(RECENT_LIMIT);
    }

    /// Whether a folder's contents still need requesting (never asked, and not
    /// already held).
    fn needs_fetch(&self, folder: InventoryFolderKey) -> bool {
        !self.requested.contains(&folder) && !self.items.contains_key(&folder)
    }

    /// Flatten the model into the linear row list for a tab and a search query.
    ///
    /// The pure heart of the window: given the tab, the (possibly empty) query,
    /// and the expand state, produce exactly the rows to draw, in order, each
    /// resolved to its label, icon and arrow. Tested directly; the Bevy side only
    /// renders the result.
    pub(crate) fn build_rows(&self, tab: InventoryTab, query: &str) -> Vec<DisplayRow> {
        let needle = query.trim().to_lowercase();
        match tab {
            InventoryTab::Everything if needle.is_empty() => self.tree_rows(),
            InventoryTab::Everything => self.search_rows(&needle),
            InventoryTab::Recent => self.recent_rows(&needle),
            InventoryTab::Worn => self.worn_rows(&needle),
        }
    }

    /// The Everything tab's tree, depth-first from the roots.
    fn tree_rows(&self) -> Vec<DisplayRow> {
        let mut rows = Vec::new();
        for &root in &self.roots {
            self.emit_folder(root, 0, &mut rows);
        }
        rows
    }

    /// Emit `folder`'s row, then — if expanded — its child folders and items,
    /// indented one level deeper.
    fn emit_folder(&self, folder: InventoryFolderKey, depth: usize, rows: &mut Vec<DisplayRow>) {
        let expanded = self.expanded.contains(&folder);
        let arrow = if expanded {
            RowArrow::Expanded
        } else {
            RowArrow::Collapsed
        };
        rows.push(self.folder_row(folder, depth, arrow));
        if !expanded {
            return;
        }
        let child_depth = depth.saturating_add(1);
        for &child in self.children_of(folder) {
            self.emit_folder(child, child_depth, rows);
        }
        for item in self.items_of(folder) {
            rows.push(item_row(
                item.item_id,
                &item.name,
                item.inv_type,
                child_depth,
            ));
        }
    }

    /// Build a folder's display row.
    fn folder_row(&self, folder: InventoryFolderKey, depth: usize, arrow: RowArrow) -> DisplayRow {
        let info = self.folders.get(&folder);
        let name = info.map_or_else(|| "(folder)".to_owned(), |info| info.name.clone());
        let folder_type = info.map_or(FolderType::None, |info| info.folder_type);
        DisplayRow {
            key: RowKey::Folder(folder),
            depth,
            name,
            icon: folder_icon(folder_type, !matches!(arrow, RowArrow::Collapsed)),
            arrow,
        }
    }

    /// The Everything tab under an active search: the folder **hierarchy**
    /// narrowed to the branches that lead to a match — every ancestor folder of a
    /// matching item or folder is kept and shown expanded — the way the reference
    /// viewer filters its tree. Only loaded folders' items are searchable (an
    /// unfetched folder's contents are not held).
    fn search_rows(&self, needle: &str) -> Vec<DisplayRow> {
        let mut keep = HashSet::new();
        for &root in &self.roots {
            self.mark_matching_subtree(root, needle, &mut keep);
        }
        let mut rows = Vec::new();
        for &root in &self.roots {
            if keep.contains(&root) {
                self.emit_search_folder(root, 0, needle, &keep, &mut rows);
            }
        }
        rows
    }

    /// Whether `folder`'s name matches the query.
    fn folder_name_matches(&self, folder: InventoryFolderKey, needle: &str) -> bool {
        self.folders
            .get(&folder)
            .is_some_and(|info| info.name.to_lowercase().contains(needle))
    }

    /// Mark `folder` in `keep` if it, or anything in its subtree, matches the
    /// query, and return whether it did — so an ancestor of a match is retained.
    fn mark_matching_subtree(
        &self,
        folder: InventoryFolderKey,
        needle: &str,
        keep: &mut HashSet<InventoryFolderKey>,
    ) -> bool {
        let mut any = self.folder_name_matches(folder, needle);
        for &child in self.children_of(folder) {
            if self.mark_matching_subtree(child, needle, keep) {
                any = true;
            }
        }
        for item in self.items_of(folder) {
            if item.name.to_lowercase().contains(needle) {
                any = true;
            }
        }
        if any {
            keep.insert(folder);
        }
        any
    }

    /// Emit a kept folder (shown expanded) and, recursively, its kept child
    /// folders and its matching items, indented one level deeper.
    fn emit_search_folder(
        &self,
        folder: InventoryFolderKey,
        depth: usize,
        needle: &str,
        keep: &HashSet<InventoryFolderKey>,
        rows: &mut Vec<DisplayRow>,
    ) {
        rows.push(self.folder_row(folder, depth, RowArrow::Expanded));
        let child_depth = depth.saturating_add(1);
        for &child in self.children_of(folder) {
            if keep.contains(&child) {
                self.emit_search_folder(child, child_depth, needle, keep, rows);
            }
        }
        for item in self.items_of(folder) {
            if item.name.to_lowercase().contains(needle) {
                rows.push(item_row(
                    item.item_id,
                    &item.name,
                    item.inv_type,
                    child_depth,
                ));
            }
        }
    }

    /// The Recent tab: the received-since-login list, filtered by the query.
    fn recent_rows(&self, needle: &str) -> Vec<DisplayRow> {
        self.recent
            .iter()
            .filter(|item| needle.is_empty() || item.name.to_lowercase().contains(needle))
            .map(|item| item_row(item.key, &item.name, item.inv_type, 0))
            .collect()
    }

    /// The Worn tab: the COF's contents if it holds any, else the legacy
    /// `AgentWearables` set mapped to type labels. Filtered by the query.
    fn worn_rows(&self, needle: &str) -> Vec<DisplayRow> {
        let matches = |name: &str| needle.is_empty() || name.to_lowercase().contains(needle);
        if let Some(cof) = self.cof {
            let items = self.items_of(cof);
            if !items.is_empty() {
                return items
                    .iter()
                    .filter(|item| matches(&item.name))
                    .map(|item| item_row(item.item_id, &item.name, item.inv_type, 0))
                    .collect();
            }
        }
        self.wearables
            .iter()
            .filter_map(|worn| {
                let name = wearable_label(worn.wearable_type);
                matches(name).then(|| DisplayRow {
                    key: RowKey::Item(worn.item_id),
                    depth: 0,
                    name: name.to_owned(),
                    icon: item_icon(InventoryType::Wearable),
                    arrow: RowArrow::Leaf,
                })
            })
            .collect()
    }
}

/// Build an item's display row at a given depth.
fn item_row(key: InventoryKey, name: &str, inv_type: InventoryType, depth: usize) -> DisplayRow {
    DisplayRow {
        key: RowKey::Item(key),
        depth,
        name: name.to_owned(),
        icon: item_icon(inv_type),
        arrow: RowArrow::Leaf,
    }
}

// ---------------------------------------------------------------------------
// View + UI state
// ---------------------------------------------------------------------------

/// Which tab the window is showing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum InventoryTab {
    /// The full folder tree.
    #[default]
    Everything,
    /// Items received since login.
    Recent,
    /// The current outfit.
    Worn,
}

impl InventoryTab {
    /// This tab's button label as a Fluent key (the strip is spawned with
    /// `translate_labels`, so `crate::i18n` resolves it per locale).
    const fn label_key(self) -> &'static str {
        match self {
            Self::Everything => "inventory-tab-everything",
            Self::Recent => "inventory-tab-recent",
            Self::Worn => "inventory-tab-worn",
        }
    }
}

/// The window's transient UI state: which tab and the search query.
///
/// Open / closed is **not** tracked here — the floater's
/// [`UiPanelShown`] is the single source of truth, so restoring the window open
/// from saved settings ([`crate::floater_persist`]) and toggling it with `Ctrl+I`
/// go through the same flag and can never drift apart.
#[derive(Resource, Default)]
struct InventoryState {
    /// The active tab.
    tab: InventoryTab,
    /// The current search query.
    query: String,
}

/// The flattened rows the window is currently drawing — recomputed from the
/// model whenever it, the tab or the query changes, and read by the row binder.
#[derive(Resource, Default)]
struct InventoryView {
    /// The rows, top to bottom.
    rows: Vec<DisplayRow>,
}

/// Entity handles for the window's parts, so the systems can find them without
/// re-querying by marker every frame.
#[derive(Resource)]
pub(crate) struct InventoryUi {
    /// The panel root (carries [`UiPanelShown`]).
    panel: Entity,
    /// The scrolling viewport (carries [`VirtualList`]).
    viewport: Entity,
    /// The search text field.
    search: Entity,
    /// The reusable tab strip ([`crate::ui_tab`]) whose active index selects the
    /// list — mapped through [`TAB_ORDER`] by [`bridge_tab_selection`].
    tab_strip: Entity,
}

impl InventoryUi {
    /// The window's panel-root entity — carries [`UiPanelShown`], so the top
    /// menu bar ([`crate::menu_bar`]) can toggle the window and read whether it
    /// is open without reaching into this module's private fields.
    pub(crate) const fn panel(&self) -> Entity {
        self.panel
    }
}

/// The inventory list each tab of the strip selects, in the strip's button
/// order. The one place the widget's index and the domain enum are tied
/// together — [`bridge_tab_selection`] reads it, and the labels below are spawned
/// in the same order.
const TAB_ORDER: [InventoryTab; 3] = [
    InventoryTab::Everything,
    InventoryTab::Recent,
    InventoryTab::Worn,
];

/// One row of the flattened inventory view, fully resolved for drawing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DisplayRow {
    /// What this row refers to (for a click).
    key: RowKey,
    /// The tree depth, driving the indent.
    depth: usize,
    /// The label text.
    name: String,
    /// The type icon glyph.
    icon: &'static str,
    /// The expand-arrow state.
    arrow: RowArrow,
}

/// What a [`DisplayRow`] points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowKey {
    /// A folder, which a click expands or collapses.
    Folder(InventoryFolderKey),
    /// An item, inert to a click for now (selection / context actions are a
    /// follow-up).
    Item(InventoryKey),
}

/// A row's expand-arrow state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowArrow {
    /// A collapsed folder.
    Collapsed,
    /// An expanded folder.
    Expanded,
    /// A leaf (item, or a folder shown flat in search) — no arrow.
    Leaf,
}

impl RowArrow {
    /// The glyph for this arrow state. Empty for a leaf — the arrow sits in a
    /// min-width column ([`ARROW_COL_WIDTH`]), so an item's blank arrow still
    /// lines its icon up under a folder's.
    const fn glyph(self) -> &'static str {
        match self {
            Self::Collapsed => "\u{25b8}",
            Self::Expanded => "\u{25be}",
            Self::Leaf => "",
        }
    }
}

/// A toolbar action, emitted by a tab or expand/collapse button and applied by
/// [`apply_ui_actions`].
#[expect(
    variant_size_differences,
    reason = "the folder-key variant is one Uuid; boxing it to even out a transient, \
              short-lived message enum is not worth the indirection"
)]
#[derive(Message, Debug, Clone, Copy)]
enum InventoryUiAction {
    /// Switch to a tab.
    SelectTab(InventoryTab),
    /// Expand every folder.
    ExpandAll,
    /// Collapse every folder.
    CollapseAll,
    /// Toggle a folder's expand state (from a row click).
    ToggleFolder(InventoryFolderKey),
}

/// The `element` the inventory gear menu attributes its picks to, so
/// [`route_gear_menu`] routes its own menu and no other.
const INVENTORY_GEAR_ELEMENT: &str = "inventory-gear";

/// The inventory window's gear (options) menu — the reference's
/// `menu_inventory_gear_default`, on [`crate::menu`]'s reusable widget. Its
/// label is the gear glyph (U+2699). Only the entries with a live action today
/// are wired; the rest are a placeholder for future inventory tasks.
static INVENTORY_GEAR_MENU: MenuDef = MenuDef {
    label: "\u{2699}",
    items: &[
        MenuItemDef::Command(MenuCommand::new("Expand All Folders", "expand-all")),
        MenuItemDef::Command(MenuCommand::new("Collapse All Folders", "collapse-all")),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("(more options soon)", "noop").enabled_when("never")),
    ],
};

/// Route the gear menu's picks (a [`UiAction`]) to the window's own
/// [`InventoryUiAction`]s — the live wiring the reusable widget leaves to its
/// host, exactly as the top menu bar wires its own picks
/// ([`crate::menu_bar`]).
fn route_gear_menu(
    mut picks: MessageReader<UiAction>,
    mut actions: MessageWriter<InventoryUiAction>,
) {
    for pick in picks.read() {
        if pick.element != INVENTORY_GEAR_ELEMENT {
            continue;
        }
        match pick.action {
            "expand-all" => {
                actions.write(InventoryUiAction::ExpandAll);
            }
            "collapse-all" => {
                actions.write(InventoryUiAction::CollapseAll);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

/// The emoji glyph for an inventory item's type — the viewer's font stack paints
/// colour emoji, so a glyph stands in for the reference viewer's icon textures
/// (which are not among the assets we ship).
pub(crate) const fn item_icon(inv_type: InventoryType) -> &'static str {
    match inv_type {
        InventoryType::Texture => "\u{1f5bc}\u{fe0f}",
        InventoryType::Sound => "\u{1f50a}",
        InventoryType::CallingCard => "\u{1f4c7}",
        InventoryType::Landmark => "\u{1f4cd}",
        InventoryType::Object | InventoryType::Attachment => "\u{1f4e6}",
        InventoryType::Notecard => "\u{1f4c4}",
        InventoryType::Category => "\u{1f4c1}",
        InventoryType::Script => "\u{1f4dc}",
        InventoryType::Snapshot => "\u{1f4f7}",
        InventoryType::Wearable => "\u{1f455}",
        InventoryType::Animation => "\u{1f3c3}",
        InventoryType::Gesture => "\u{1f44b}",
        InventoryType::Mesh => "\u{1f4d0}",
        InventoryType::Settings => "\u{2699}\u{fe0f}",
        InventoryType::Material => "\u{1f3a8}",
        _other => "\u{2753}",
    }
}

/// The emoji glyph for a folder, keyed on its type (with a distinct open glyph
/// when expanded) — trash, current outfit, favourites and the rest read at a
/// glance; a plain folder is the default.
pub(crate) const fn folder_icon(folder_type: FolderType, expanded: bool) -> &'static str {
    match folder_type {
        FolderType::Trash => "\u{1f5d1}\u{fe0f}",
        FolderType::CurrentOutfit | FolderType::Outfit => "\u{1f454}",
        FolderType::MyOutfits => "\u{1f45a}",
        FolderType::Favorite => "\u{2b50}",
        FolderType::LostAndFound => "\u{1f4ca}",
        FolderType::Inbox | FolderType::Outbox => "\u{1f4ec}",
        FolderType::MarketplaceListings | FolderType::MarketplaceStock => "\u{1f6d2}",
        _other => {
            if expanded {
                "\u{1f4c2}"
            } else {
                "\u{1f4c1}"
            }
        }
    }
}

/// A short label for a legacy worn wearable's type, for the Worn tab's fallback.
const fn wearable_label(wearable_type: WearableType) -> &'static str {
    match wearable_type {
        WearableType::Shape => "Shape",
        WearableType::Skin => "Skin",
        WearableType::Hair => "Hair",
        WearableType::Eyes => "Eyes",
        WearableType::Shirt => "Shirt",
        WearableType::Pants => "Pants",
        WearableType::Shoes => "Shoes",
        WearableType::Socks => "Socks",
        WearableType::Jacket => "Jacket",
        WearableType::Gloves => "Gloves",
        WearableType::Undershirt => "Undershirt",
        WearableType::Underpants => "Underpants",
        WearableType::Skirt => "Skirt",
        WearableType::Alpha => "Alpha",
        WearableType::Tattoo => "Tattoo",
        WearableType::Physics => "Physics",
        WearableType::Universal => "Universal",
        _other => "Wearable",
    }
}

// ---------------------------------------------------------------------------
// Toggle
// ---------------------------------------------------------------------------

/// The hosting floater's [`crate::floater::FloaterSpec::id`] — it also keys the
/// window's remembered geometry in the settings store
/// ([`crate::floater_persist`]).
const INVENTORY_FLOATER_ID: &str = "inventory";

/// `Ctrl+I` opens / closes the window, matching the reference viewer's shortcut.
/// Ungated by the input-context (like the `F`-key overlay toggles) so it always
/// works; the `Ctrl` modifier keeps it from firing while a bare `i` is typed.
fn toggle_inventory(
    keyboard: Res<ButtonInput<KeyCode>>,
    ui: Option<Res<InventoryUi>>,
    mut panels: Query<&mut UiPanelShown>,
) {
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if !(ctrl && keyboard.just_pressed(KeyCode::KeyI)) {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    // Flip the floater's own shown flag; the refresh below (and any geometry
    // persistence) reacts to the change, so there is no separate open-state to
    // keep in step.
    if let Ok(mut shown) = panels.get_mut(ui.panel) {
        shown.0 = !shown.0;
    }
}

/// Refresh the inventory whenever the window becomes visible — whether opened by
/// `Ctrl+I` or **restored open** from saved settings ([`crate::floater_persist`]),
/// since both just flip the floater's [`UiPanelShown`].
///
/// A cheap local snapshot each time: the login skeleton may have arrived after a
/// previous open, and folders can be created during the session.
fn refresh_inventory_on_show(
    ui: Option<Res<InventoryUi>>,
    shown: Query<&UiPanelShown, Changed<UiPanelShown>>,
    mut model: ResMut<InventoryModel>,
    mut commands: MessageWriter<SlCommand>,
) {
    let Some(ui) = ui else {
        return;
    };
    // `get` on a `Changed` query yields the panel only on the frame its
    // visibility flips; ignore the close transition.
    let Ok(shown) = shown.get(ui.panel) else {
        return;
    };
    if shown.0 {
        commands.write(SlCommand(Command::QueryInventoryFolders));
        // The Worn tab wants the COF contents; harmless if already held.
        request_worn_source(&mut model, &mut commands);
    }
}

// ---------------------------------------------------------------------------
// Event ingestion
// ---------------------------------------------------------------------------

/// Fold the high-level inventory events into [`InventoryModel`], marking the view
/// dirty (via `InventoryModel`'s change tick) whenever something it draws moved.
fn ingest_inventory(
    mut events: MessageReader<SlEvent>,
    mut model: ResMut<InventoryModel>,
    mut commands: MessageWriter<SlCommand>,
) {
    for event in events.read() {
        match &event.0 {
            SlSessionEvent::InventoryFolders(folders) => {
                let first_load = !model.folders_loaded;
                model.merge_folders(folders, false);
                model.folders_loaded = true;
                if first_load {
                    // Show the top level: expand the agent roots (not the huge
                    // read-only Library) and pull their items.
                    let roots: Vec<InventoryFolderKey> = model
                        .roots
                        .iter()
                        .copied()
                        .filter(|root| !model.library_folders.contains(root))
                        .collect();
                    for root in roots {
                        model.expanded.insert(root);
                        request_folder(&mut model, root, &mut commands);
                    }
                }
            }
            SlSessionEvent::LibraryInventory(folders) => {
                model.merge_library_folders(folders);
            }
            SlSessionEvent::InventoryFolderPage {
                folder,
                folders,
                items,
                ..
            } => {
                model.requested.insert(*folder);
                // A page carries the folder's sub-folders too; merging them fills
                // in the Library subtree (whose structure the login skeleton does
                // not fully carry) as it is browsed.
                let library = model.library_folders.contains(folder);
                model.merge_folders(folders, library);
                model.set_items(*folder, items);
            }
            SlSessionEvent::InventoryDescendents { folder_id, .. } => {
                // The async fetch of a folder finished; re-query to get the page
                // in resolved (`ItemInfo`) form from the now-loaded model.
                if model.requested.contains(folder_id) {
                    query_folder_page(*folder_id, &mut commands);
                }
            }
            SlSessionEvent::InventoryBulkUpdate { items, .. } => {
                for item in items {
                    model.push_recent(
                        item.item_id,
                        item.name.clone(),
                        InventoryType::from_code(i32::from(item.inv_type)),
                    );
                }
            }
            SlSessionEvent::InventoryItemCreated { item, .. } => {
                model.push_recent(
                    item.item_id,
                    item.name.clone(),
                    InventoryType::from_code(i32::from(item.inv_type)),
                );
            }
            SlSessionEvent::AgentWearables { wearables, .. } => {
                model.wearables.clone_from(wearables);
            }
            _other => {}
        }
    }
}

/// Ensure the Worn tab has a source: request the COF's contents if it is known
/// and not yet held.
fn request_worn_source(model: &mut InventoryModel, commands: &mut MessageWriter<SlCommand>) {
    if let Some(cof) = model.cof
        && model.needs_fetch(cof)
    {
        request_folder(model, cof, commands);
    }
}

/// Mark a folder requested and query its page (which auto-schedules the session's
/// own fetch when the folder is not yet loaded).
fn request_folder(
    model: &mut InventoryModel,
    folder: InventoryFolderKey,
    commands: &mut MessageWriter<SlCommand>,
) {
    model.requested.insert(folder);
    query_folder_page(folder, commands);
}

/// Send the page query for a folder.
fn query_folder_page(folder: InventoryFolderKey, commands: &mut MessageWriter<SlCommand>) {
    commands.write(SlCommand(Command::QueryInventoryFolder {
        folder,
        before: None,
        limit: FOLDER_PAGE_LIMIT,
    }));
}

// ---------------------------------------------------------------------------
// Toolbar + search
// ---------------------------------------------------------------------------

/// Apply the toolbar actions: switch tab, expand / collapse all, or toggle one
/// folder — fetching contents lazily as folders open.
fn apply_ui_actions(
    mut actions: MessageReader<InventoryUiAction>,
    mut state: ResMut<InventoryState>,
    mut model: ResMut<InventoryModel>,
    mut commands: MessageWriter<SlCommand>,
) {
    for action in actions.read() {
        match *action {
            InventoryUiAction::SelectTab(tab) => {
                if state.tab != tab {
                    state.tab = tab;
                }
                if tab == InventoryTab::Worn {
                    request_worn_source(&mut model, &mut commands);
                }
            }
            InventoryUiAction::ExpandAll => {
                let keys: Vec<InventoryFolderKey> = model.folders.keys().copied().collect();
                for key in keys {
                    model.expanded.insert(key);
                    if model.needs_fetch(key) {
                        request_folder(&mut model, key, &mut commands);
                    }
                }
            }
            InventoryUiAction::CollapseAll => {
                model.expanded.clear();
            }
            InventoryUiAction::ToggleFolder(folder) => {
                if model.expanded.remove(&folder) {
                    // just collapsed
                } else {
                    model.expanded.insert(folder);
                    if model.needs_fetch(folder) {
                        request_folder(&mut model, folder, &mut commands);
                    }
                }
            }
        }
    }
}

/// Read the search field's live text into [`InventoryState::query`] when it
/// changes.
fn read_search_field(
    ui: Option<Res<InventoryUi>>,
    fields: Query<&EditableText>,
    mut state: ResMut<InventoryState>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(field) = fields.get(ui.search) else {
        return;
    };
    let text = field.value().to_string();
    if text != state.query {
        state.query = text;
    }
}

/// Recompute the flattened view whenever the model, tab or query changed, keep
/// the list's item count in step, and reset the scroll so a shorter new list is
/// not left scrolled past its end.
fn rebuild_view(
    model: Res<InventoryModel>,
    state: Res<InventoryState>,
    ui: Option<Res<InventoryUi>>,
    mut view: ResMut<InventoryView>,
    mut lists: Query<&mut VirtualList>,
) {
    if !model.is_changed() && !state.is_changed() {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    // Reset the scroll only when the *content* changes wholesale — a tab switch,
    // a new search, or an open. Expanding or collapsing a folder changes only the
    // model, so the position is kept (the reference viewer keeps it too); the
    // generic list clamps it if the list got shorter.
    let reset_scroll = state.is_changed();
    view.rows = model.build_rows(state.tab, &state.query);
    if let Ok(mut list) = lists.get_mut(ui.viewport) {
        list.item_count = view.rows.len();
        if reset_scroll {
            list.scroll_to_top();
        }
    }
}

/// Bridge the reusable tab strip's selection to the window: when the strip's
/// active tab changes ([`TabStrip::active`]), turn it into a
/// [`InventoryUiAction::SelectTab`] on the list it names via [`TAB_ORDER`].
///
/// The strip owns "exactly one active", the highlight and the keyboard, so this
/// is the whole of the wiring — the widget is not coupled to the inventory, it
/// just exposes its selection and we react. Runs on the strip's own change
/// detection (the query is filtered `Changed<TabStrip>`), so an unchanged tab
/// costs nothing; the first frame it fires with the default tab, which
/// [`apply_ui_actions`] treats as the no-op it is.
fn bridge_tab_selection(
    ui: Option<Res<InventoryUi>>,
    strips: Query<&TabStrip, Changed<TabStrip>>,
    mut actions: MessageWriter<InventoryUiAction>,
) {
    let Some(ui) = ui else {
        return;
    };
    let Ok(strip) = strips.get(ui.tab_strip) else {
        return;
    };
    if let Some(tab) = TAB_ORDER.get(strip.active) {
        actions.write(InventoryUiAction::SelectTab(*tab));
    }
}

// ---------------------------------------------------------------------------
// Row pool: populate + bind
// ---------------------------------------------------------------------------

/// The persistent inner parts of a pooled row, so binding updates them in place
/// rather than respawning (which would re-measure text every scroll frame).
#[derive(Component)]
struct RowParts {
    /// The leading indent spacer, whose width encodes the depth.
    indent: Entity,
    /// The expand-arrow glyph.
    arrow: Entity,
    /// The type-icon glyph.
    icon: Entity,
    /// The label text.
    label: Entity,
}

/// Build the inner structure of a freshly-pooled row (once), and wire its click.
///
/// The generic list spawns bare positioned row containers; this fills each with
/// the indent / arrow / icon / label it will keep for its lifetime, and installs
/// the press observer that toggles a folder and focuses the window.
fn populate_new_rows(
    mut commands: Commands,
    ui: Option<Res<InventoryUi>>,
    new_rows: Query<(Entity, &ChildOf), Added<VirtualRow>>,
) {
    let Some(ui) = ui else {
        return;
    };
    for (row_entity, child_of) in &new_rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        commands.entity(row_entity).insert((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(ROW_HEIGHT),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..default()
            },
            Pickable::default(),
        ));
        let indent = commands.spawn((Node::default(), ChildOf(row_entity))).id();
        let arrow = commands
            .spawn((
                Text::new(""),
                UiFont::Mono.at(ROW_FONT_SIZE),
                TextColor(CHROME_COLOR),
                Node {
                    min_width: Val::Px(ARROW_COL_WIDTH),
                    ..default()
                },
                ChildOf(row_entity),
            ))
            .id();
        let icon = commands
            .spawn((
                Text::new(""),
                UiFont::Sans.at(ROW_FONT_SIZE),
                TextColor(LABEL_COLOR),
                Node {
                    min_width: Val::Px(ICON_COL_WIDTH),
                    ..default()
                },
                ChildOf(row_entity),
            ))
            .id();
        let label = commands
            .spawn((
                Text::new(""),
                UiFont::Sans.at(ROW_FONT_SIZE),
                TextColor(LABEL_COLOR),
                ChildOf(row_entity),
            ))
            .id();
        commands
            .entity(row_entity)
            .insert(RowParts {
                indent,
                arrow,
                icon,
                label,
            })
            .observe(on_row_press);
    }
}

/// Bind each row's parts to the [`DisplayRow`] it now points at — on the frame
/// the view is rebuilt (all rows) or a row's index changed (that row).
fn bind_rows(
    view: Res<InventoryView>,
    ui: Option<Res<InventoryUi>>,
    rows: Query<(Ref<VirtualRow>, &ChildOf, &RowParts)>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    let rebuild_all = view.is_changed();
    for (row, child_of, parts) in &rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        if !rebuild_all && !row.is_changed() {
            continue;
        }
        let Some(index) = row.index else {
            continue;
        };
        let Some(display) = view.rows.get(index) else {
            continue;
        };
        if let Ok(mut indent) = nodes.get_mut(parts.indent) {
            indent.width = Val::Px(depth_indent(display.depth));
        }
        if let Ok((mut text, mut color)) = texts.get_mut(parts.arrow) {
            set_text(&mut text, display.arrow.glyph());
            *color = TextColor(CHROME_COLOR);
        }
        if let Ok((mut text, _color)) = texts.get_mut(parts.icon) {
            set_text(&mut text, display.icon);
        }
        if let Ok((mut text, mut color)) = texts.get_mut(parts.label) {
            set_text(&mut text, &display.name);
            *color = TextColor(match display.key {
                RowKey::Folder(_) => FOLDER_LABEL_COLOR,
                RowKey::Item(_) => LABEL_COLOR,
            });
        }
    }
}

/// A row was clicked: focus the window (so the wheel scrolls the list, not the
/// camera) and, if it is a folder, toggle it.
fn on_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&VirtualRow>,
    view: Res<InventoryView>,
    ui: Res<InventoryUi>,
    mut focus: ResMut<InputFocus>,
    mut actions: MessageWriter<InventoryUiAction>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.viewport, FocusCause::Navigated);
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    let Some(index) = row.index else {
        return;
    };
    if let Some(DisplayRow {
        key: RowKey::Folder(folder),
        ..
    }) = view.rows.get(index)
    {
        actions.write(InventoryUiAction::ToggleFolder(*folder));
    }
}

/// Set a text node's string only when it actually changed, so a re-bind of an
/// unchanged row does not needlessly re-measure it.
fn set_text(text: &mut Text, value: &str) {
    if text.0 != value {
        value.clone_into(&mut text.0);
    }
}

/// The indent width, in logical pixels, for a tree depth.
fn depth_indent(depth: usize) -> f32 {
    f32::from(u16::try_from(depth).unwrap_or(u16::MAX)) * INDENT_PER_DEPTH
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn the inventory window: a **floater** ([`crate::floater`]) whose content is
/// the tab / expand / collapse toolbar, the search field, and the virtualized
/// viewport. The floater supplies the title bar (drag), the close / minimize /
/// dock chrome, and the resize grip; this fills its content slot. Starts hidden.
fn spawn_inventory_panel(mut commands: Commands, root: Res<UiRoot>) {
    let handle = spawn_floater(
        &mut commands,
        root.0,
        FloaterSpec {
            id: INVENTORY_FLOATER_ID,
            // A fallback shown only until the bundle loads and `Translated`
            // resolves `inventory-title`; the localized title lives on the key.
            title: "Inventory".to_owned(),
            position: Vec2::new(20.0, 60.0),
            // A definite, resizable content area (the reference inventory has a
            // default rect and `can_resize`): the grip grows *and* shrinks it, and
            // the tabs / search / list below fill it.
            default_size: Some(Vec2::new(PANEL_WIDTH, VIEWPORT_HEIGHT)),
            // Don't let the grip shrink it below what the tabs, toolbar and search
            // need plus a few list rows — smaller than this the chrome would be
            // clipped by the window edge with nothing usable left.
            min_size: Some(Vec2::new(INVENTORY_MIN_WIDTH, INVENTORY_MIN_HEIGHT)),
            caps: FloaterCaps {
                resizable: true,
                minimizable: true,
                closable: true,
                dockable: true,
            },
        },
    );
    let panel = handle.root;
    let content = handle.content;
    // Localize the floater's title bar: bind its text node to the Fluent key so
    // it tracks the active locale like the rest of the window.
    commands
        .entity(handle.title_text)
        .insert(Translated::new("inventory-title"));

    // Tabs — the reusable strip widget ([`crate::ui_tab`]) in its horizontal
    // (top-edge) placement. One focus stop; the arrow keys move between the
    // Everything / Recent / Worn tabs, and the active one drives the shared list
    // via [`bridge_tab_selection`]. The labels are spawned in [`TAB_ORDER`].
    let tab_labels = TAB_ORDER.map(|tab| tab.label_key().to_owned());
    let tab_strip = spawn_tab_strip(
        &mut commands,
        content,
        &TabSpec {
            element: "inventory-tabs",
            placement: TabPlacement::BlockStart,
            labels: &tab_labels,
            active: 0,
            tab_index: 1,
            font_size: CHROME_FONT_SIZE,
            strip_width: None,
            ellipsis: DEFAULT_ELLIPSIS,
            translate_labels: true,
        },
    );

    // Expand / collapse all.
    let expand_row = commands
        .spawn((
            Node {
                ..row(Val::Px(4.0))
            },
            ChildOf(content),
        ))
        .id();
    let expand_all = spawn_toolbar_button(&mut commands, expand_row, "inventory-expand-all", 2);
    commands.entity(expand_all).observe(
        |_press: On<Pointer<Press>>, mut actions: MessageWriter<InventoryUiAction>| {
            actions.write(InventoryUiAction::ExpandAll);
        },
    );
    let collapse_all = spawn_toolbar_button(&mut commands, expand_row, "inventory-collapse-all", 3);
    commands.entity(collapse_all).observe(
        |_press: On<Pointer<Press>>, mut actions: MessageWriter<InventoryUiAction>| {
            actions.write(InventoryUiAction::CollapseAll);
        },
    );
    // The reference viewer's inventory **gear menu** — a drop-down of window
    // options anchored to a button, built on the very same line-menu widget the
    // top menu bar uses (`crate::menu`). This is the shared-widget point: the
    // main bar and the inventory's gear button are two placements of one menu, so
    // the entries a future task adds land in a `MenuDef` here, not a bespoke
    // panel. Wired today to the expand / collapse actions the window already has;
    // the rest of the reference's gear entries (sort, filters, new window) are a
    // placeholder for future tasks.
    crate::menu::spawn_menu_button(
        &mut commands,
        expand_row,
        ElementCx::new(),
        &INVENTORY_GEAR_MENU,
        INVENTORY_GEAR_ELEMENT,
    );

    // Search field.
    let mut search_field = EditableText::new("");
    search_field.allow_newlines = false;
    search_field.visible_lines = Some(1.0);
    let search = commands
        .spawn((
            search_field,
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(LABEL_COLOR),
            bevy::text::TextCursorStyle::default(),
            TabIndex(4),
            Node {
                border: UiRect::all(Val::Px(2.0)),
                padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(Color::srgb(0.10, 0.12, 0.16)),
            Name::new("inventory-search"),
            ChildOf(content),
        ))
        .observe(|press: On<Pointer<Press>>, mut focus: ResMut<InputFocus>| {
            focus.set(press.entity, FocusCause::Navigated);
        })
        .id();

    // The virtualized viewport **fills** the floater's resizable content area: its
    // width comes from the content column's stretch and its height from
    // `flex_grow` (it takes whatever the tabs / toolbar / search leave), down to
    // nothing (`min_height: 0`). So dragging the floater's resize grip grows and
    // shrinks the list with the window, and the windowing reads a real measured
    // height either way.
    let viewport = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::clip(),
                position_type: PositionType::Relative,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            VirtualList::new(ROW_HEIGHT),
            VirtualViewport,
            Pickable::default(),
            TabIndex(7),
            Name::new("inventory-viewport"),
            ChildOf(content),
        ))
        .observe(
            |press: On<Pointer<Press>>, ui: Res<InventoryUi>, mut focus: ResMut<InputFocus>| {
                if press.button == PointerButton::Primary {
                    focus.set(ui.viewport, FocusCause::Navigated);
                }
            },
        )
        .id();

    commands.insert_resource(InventoryUi {
        panel,
        viewport,
        search,
        tab_strip,
    });
}

/// Spawn one toolbar button (a focusable, clickable box with a centred label)
/// and return its entity.
fn spawn_toolbar_button(
    commands: &mut Commands,
    parent: Entity,
    label_key: &'static str,
    tab_index: i32,
) -> Entity {
    commands
        .spawn((
            Button,
            TabIndex(tab_index),
            Node {
                padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(BUTTON_BORDER),
            BackgroundColor(BUTTON_BACKGROUND),
            Pickable::default(),
            Name::new(format!("inventory-button:{label_key}")),
            ChildOf(parent),
        ))
        .with_child((
            // Empty until `crate::i18n::apply_translations` resolves the key for
            // the active locale (and re-resolves it on a locale switch).
            Text::default(),
            Translated::new(label_key),
            UiFont::Sans.at(CHROME_FONT_SIZE),
            TextColor(CHROME_COLOR),
        ))
        .id()
}

// ---------------------------------------------------------------------------
// Registry sample
// ---------------------------------------------------------------------------

/// Spawn a static sample of the inventory row layout for the UI harness /
/// gallery: an expanded folder row and an indented item row, so the row's layout
/// is swept across every script, size and direction like every other element.
///
/// Static by construction (no live model, no recycling), which is what lets the
/// registry check it — the live window is driven by resources, like the `F`-key
/// demos, and so is deliberately not registered.
pub(crate) fn spawn_inventory_row_sample(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
) -> Entity {
    let list = commands
        .spawn((
            Node {
                width: Val::Px(PANEL_WIDTH),
                ..column(Val::Px(2.0))
            },
            Name::new("inventory-row"),
            ChildOf(parent),
        ))
        .id();
    spawn_sample_row(
        commands,
        list,
        cx,
        0,
        RowArrow::Expanded,
        folder_icon(FolderType::Clothing, true),
        "Clothing",
    );
    spawn_sample_row(
        commands,
        list,
        cx,
        1,
        RowArrow::Leaf,
        item_icon(InventoryType::Wearable),
        "A shirt",
    );
    list
}

/// Spawn one static sample row (indent, arrow, icon, label) for the registry
/// sample.
fn spawn_sample_row(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
    depth: usize,
    arrow: RowArrow,
    icon: &str,
    label: &str,
) {
    let row_entity = commands
        .spawn((
            Node {
                height: Val::Px(ROW_HEIGHT),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..row(Val::Px(0.0))
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Node {
            width: Val::Px(depth_indent(depth)),
            ..default()
        },
        ChildOf(row_entity),
    ));
    // The sample sizes the arrow / icon to content (no fixed column) so it
    // survives the harness's font-size sweep, where a fixed column narrower than
    // a large emoji glyph would clip. The live rows (a fixed row font) keep the
    // min-width columns that align the tree.
    commands.spawn((
        Text::new(arrow.glyph().to_owned()),
        cx.font(UiFont::Mono),
        TextColor(CHROME_COLOR),
        ChildOf(row_entity),
    ));
    commands.spawn((
        Text::new(icon.to_owned()),
        cx.font(UiFont::Sans),
        TextColor(LABEL_COLOR),
        ChildOf(row_entity),
    ));
    commands.spawn((
        Text::new(cx.text(label)),
        cx.font(UiFont::Sans),
        TextColor(LABEL_COLOR),
        ChildOf(row_entity),
    ));
}

#[cfg(test)]
mod tests {
    use super::{
        DisplayRow, InventoryModel, InventoryTab, InventoryType, ItemInfo, RowArrow, RowKey,
        depth_indent, folder_icon, item_icon,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{FolderInfo, FolderState, FolderType};

    /// Build a folder-info skeleton entry.
    fn folder(id: u128, parent: Option<u128>, name: &str, ty: FolderType) -> FolderInfo {
        FolderInfo {
            folder_id: sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(
                id,
            )),
            parent_id: parent.map(|p| {
                sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(p))
            }),
            name: name.to_owned(),
            folder_type: ty,
            version: 1,
            state: FolderState::Loaded { version: 1 },
        }
    }

    /// Build a minimal item-info for a folder's contents.
    fn item(id: u128, folder: u128, name: &str, inv_type: InventoryType) -> ItemInfo {
        ItemInfo {
            item_id: sl_client_bevy::InventoryKey::from(sl_client_bevy::Uuid::from_u128(id)),
            folder_id: sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(
                folder,
            )),
            name: name.to_owned(),
            description: String::new(),
            asset_id: sl_client_bevy::Uuid::from_u128(0),
            asset_type: sl_client_bevy::AssetType::Object,
            inv_type,
            flags: 0,
            sale: None,
            creation_date: 0,
            owner: sl_client_bevy::OwnerKey::Agent(sl_client_bevy::AgentKey::from(
                sl_client_bevy::Uuid::from_u128(0),
            )),
            last_owner_id: sl_client_bevy::Uuid::from_u128(0),
            creator_id: sl_client_bevy::AgentKey::from(sl_client_bevy::Uuid::from_u128(0)),
            group: None,
            permissions: sl_client_bevy::Permissions5::default(),
        }
    }

    /// A model with a root, two child folders, and one item under one child.
    fn sample_model() -> InventoryModel {
        let mut model = InventoryModel::default();
        model.merge_folders(
            &[
                folder(1, None, "My Inventory", FolderType::RootInventory),
                folder(2, Some(1), "Clothing", FolderType::Clothing),
                folder(3, Some(1), "Objects", FolderType::Object),
            ],
            false,
        );
        model.set_items(
            sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(2)),
            &[item(10, 2, "Blue shirt", InventoryType::Wearable)],
        );
        model
    }

    /// The names of a row list, for concise assertions.
    fn names(rows: &[DisplayRow]) -> Vec<&str> {
        rows.iter().map(|row| row.name.as_str()).collect()
    }

    /// A collapsed tree shows only the root.
    #[test]
    fn collapsed_tree_shows_only_roots() {
        let model = sample_model();
        let rows = model.build_rows(InventoryTab::Everything, "");
        assert_eq!(names(&rows), vec!["My Inventory"]);
        assert_eq!(rows.first().map(|row| row.arrow), Some(RowArrow::Collapsed));
    }

    /// Expanding the root reveals its child folders (alphabetical), still
    /// collapsed; expanding a child reveals its item, indented one deeper.
    #[test]
    fn expanding_reveals_children_in_order_and_depth() {
        let mut model = sample_model();
        let root = sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(1));
        let clothing = sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(2));
        model.expanded.insert(root);
        let rows = model.build_rows(InventoryTab::Everything, "");
        assert_eq!(names(&rows), vec!["My Inventory", "Clothing", "Objects"]);

        model.expanded.insert(clothing);
        let rows = model.build_rows(InventoryTab::Everything, "");
        assert_eq!(
            names(&rows),
            vec!["My Inventory", "Clothing", "Blue shirt", "Objects"]
        );
        // The item sits one level deeper than its folder.
        let shirt = rows.iter().find(|row| row.name == "Blue shirt");
        let clothing_row = rows.iter().find(|row| row.name == "Clothing");
        assert_eq!(
            shirt.map(|row| row.depth),
            clothing_row.map(|row| row.depth.saturating_add(1))
        );
        assert!(matches!(shirt.map(|row| row.key), Some(RowKey::Item(_))));
    }

    /// Search keeps the hierarchy that leads to a match: an item match pulls in
    /// its ancestor folders, shown expanded, regardless of the expand state.
    #[test]
    fn search_keeps_the_hierarchy_to_a_match() {
        let model = sample_model();
        // Nothing expanded, but the loaded item match pulls in its ancestors.
        let rows = model.build_rows(InventoryTab::Everything, "shirt");
        assert_eq!(names(&rows), vec!["My Inventory", "Clothing", "Blue shirt"]);
        // A folder-name match keeps the folder and its ancestor, but not the
        // sibling folder or the non-matching item.
        let rows = model.build_rows(InventoryTab::Everything, "cloth");
        assert_eq!(names(&rows), vec!["My Inventory", "Clothing"]);
    }

    /// The read-only Library tree merges as a second root, ordered below "My
    /// Inventory", and its folders are flagged as library.
    #[test]
    fn library_is_a_second_root_below_my_inventory() {
        let mut model = sample_model();
        model.merge_library_folders(&[sl_client_bevy::InventoryFolder {
            folder_id: sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(
                0x50,
            )),
            parent_id: None,
            name: "Library".to_owned(),
            folder_type: FolderType::RootInventory.to_code(),
            version: 1,
        }]);
        let rows = model.build_rows(InventoryTab::Everything, "");
        // "My Inventory" (agent root) comes before "Library", even though L < M.
        assert_eq!(names(&rows), vec!["My Inventory", "Library"]);
        let library_key =
            sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(0x50));
        assert!(model.library_folders.contains(&library_key));
    }

    /// The Recent tab lists pushed items newest-first and dedupes re-pushes.
    #[test]
    fn recent_is_newest_first_and_deduped() {
        let mut model = InventoryModel::default();
        let a = sl_client_bevy::InventoryKey::from(sl_client_bevy::Uuid::from_u128(100));
        let b = sl_client_bevy::InventoryKey::from(sl_client_bevy::Uuid::from_u128(101));
        model.push_recent(a, "First".to_owned(), InventoryType::Object);
        model.push_recent(b, "Second".to_owned(), InventoryType::Notecard);
        model.push_recent(a, "First again".to_owned(), InventoryType::Object);
        let rows = model.build_rows(InventoryTab::Recent, "");
        assert_eq!(names(&rows), vec!["Second", "First"]);
    }

    /// Icons resolve distinct glyphs for distinct types, and a folder's open
    /// glyph differs from its closed one.
    #[test]
    fn icons_are_distinct() {
        assert!(item_icon(InventoryType::Texture) != item_icon(InventoryType::Sound));
        assert!(folder_icon(FolderType::Object, false) != folder_icon(FolderType::Object, true));
        // A special folder ignores the open/closed distinction.
        assert_eq!(
            folder_icon(FolderType::Trash, false),
            folder_icon(FolderType::Trash, true)
        );
    }

    /// The indent grows one step per depth level.
    #[expect(
        clippy::float_cmp,
        reason = "depth_indent produces exact multiples of a constant, asserted exactly"
    )]
    #[test]
    fn indent_grows_with_depth() {
        assert_eq!(depth_indent(0), 0.0);
        assert_eq!(depth_indent(2), super::INDENT_PER_DEPTH * 2.0);
    }
}
