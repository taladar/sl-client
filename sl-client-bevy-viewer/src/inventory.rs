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
    AssetType, Command, FolderInfo, FolderState, FolderType, InventoryFolder, InventoryFolderKey,
    InventoryKey, InventoryType, ItemInfo, Permissions, SlCommand, SlEvent, SlSessionEvent,
    Wearable, WearableType,
};

use crate::floater::{FloaterCaps, FloaterSpec, spawn_floater};
use crate::i18n::Translated;
use crate::menu::{MenuCommand, MenuConditions, MenuDef, MenuItemDef};
use crate::ui::{UiPanelShown, UiRoot, UiScaffoldSystems, column, row};
use crate::ui_element::{ElementCx, UiAction};
use crate::ui_font::UiFont;
use crate::ui_tab::{DEFAULT_ELLIPSIS, TabPlacement, TabSpec, TabStrip, spawn_tab_strip};
use crate::virtual_list::{
    VirtualList, VirtualRow, VirtualViewport, index_to_f32, layout_virtual_lists,
};

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

/// A row's trailing suffix colour (the permission / worn decorations) — dimmer
/// than the label, so the name stays the thing the eye reads first.
const SUFFIX_COLOR: Color = Color::srgb(0.62, 0.66, 0.74);

/// An inactive toolbar button background.
const BUTTON_BACKGROUND: Color = Color::srgb(0.13, 0.15, 0.20);

/// A button's border.
const BUTTON_BORDER: Color = Color::srgb(0.34, 0.40, 0.52);

/// The chrome font size, in logical pixels.
const CHROME_FONT_SIZE: f32 = 14.0;

/// A tree row's font size, in logical pixels.
const ROW_FONT_SIZE: f32 = 14.0;

/// A selected row's background.
const SELECTED_ROW_BACKGROUND: Color = Color::srgba(0.24, 0.34, 0.52, 0.55);

/// Two clicks on the same row within this window are a double-click (which
/// toggles a folder), in seconds.
const DOUBLE_CLICK_SECS: f64 = 0.35;

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
            .init_resource::<InventorySelection>()
            .init_resource::<InlineRename>()
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
                    update_gear_conditions,
                    apply_ui_actions,
                    read_search_field,
                    rebuild_view,
                )
                    .chain()
                    .before(layout_virtual_lists),
            )
            .add_systems(
                Update,
                (
                    populate_new_rows,
                    bind_rows,
                    paint_selection,
                    start_inline_rename,
                    drive_inline_rename,
                )
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

    /// The held info of a folder, if known.
    pub(crate) fn folder_info(&self, folder: InventoryFolderKey) -> Option<&FolderInfo> {
        self.folders.get(&folder)
    }

    /// Find a loaded item anywhere in the tree by its key. A linear scan over
    /// the fetched folders — run on a click, not per frame, where the largest
    /// real inventory is cheap.
    pub(crate) fn find_item(&self, item: InventoryKey) -> Option<&ItemInfo> {
        self.items
            .values()
            .flat_map(|items| items.iter())
            .find(|info| info.item_id == item)
    }

    /// Whether a folder belongs to the read-only shared Library tree.
    pub(crate) fn is_library(&self, folder: InventoryFolderKey) -> bool {
        self.library_folders.contains(&folder)
    }

    /// The first folder of a given system type in the **agent's** tree (the
    /// Library may carry same-typed folders; those never win), e.g. the Trash
    /// or Lost And Found.
    pub(crate) fn folder_by_type(&self, folder_type: FolderType) -> Option<InventoryFolderKey> {
        self.folders
            .values()
            .filter(|info| !self.library_folders.contains(&info.folder_id))
            .find(|info| info.folder_type == folder_type)
            .map(|info| info.folder_id)
    }

    /// Whether `folder` is `ancestor` or sits anywhere below it — the check that
    /// stops a folder being moved into its own subtree, and that classifies a
    /// row as "inside the Trash". Walks the parent chain upward, bounded against
    /// a (server-side impossible) parent cycle.
    pub(crate) fn is_within(
        &self,
        folder: InventoryFolderKey,
        ancestor: InventoryFolderKey,
    ) -> bool {
        let mut current = Some(folder);
        for _step in 0..64 {
            let Some(key) = current else {
                return false;
            };
            if key == ancestor {
                return true;
            }
            current = self.folders.get(&key).and_then(|info| info.parent_id);
        }
        false
    }

    /// The agent's own root folder ("My Inventory") — the first non-Library
    /// root, the target of a right-click on the window's empty background.
    pub(crate) fn agent_root(&self) -> Option<InventoryFolderKey> {
        self.roots
            .iter()
            .copied()
            .find(|root| !self.library_folders.contains(root))
    }

    /// The legacy worn-wearables set, for the wear / take-off wiring.
    pub(crate) fn worn_wearables(&self) -> &[Wearable] {
        &self.wearables
    }

    /// The Current Outfit Folder's fetched contents (empty when unknown or not
    /// yet fetched) — the modern worn set, whose links mark items as worn.
    pub(crate) fn cof_items(&self) -> &[ItemInfo] {
        self.cof.map_or(&[], |cof| self.items_of(cof))
    }

    /// Whether a folder is currently expanded in the Everything tab.
    pub(crate) fn is_expanded(&self, folder: InventoryFolderKey) -> bool {
        self.expanded.contains(&folder)
    }

    /// Every **loaded** item in `folder`'s subtree (an unfetched folder's
    /// contents are unknown and simply absent). Drives the outfit-folder
    /// conditions and actions.
    pub(crate) fn subtree_items(&self, folder: InventoryFolderKey) -> Vec<&ItemInfo> {
        let mut out = Vec::new();
        self.collect_subtree_items(folder, &mut out);
        out
    }

    /// The recursive half of [`subtree_items`](Self::subtree_items).
    fn collect_subtree_items<'model>(
        &'model self,
        folder: InventoryFolderKey,
        out: &mut Vec<&'model ItemInfo>,
    ) {
        for &child in self.children_of(folder) {
            self.collect_subtree_items(child, out);
        }
        out.extend(self.items_of(folder));
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

    /// Flatten the model into the linear row list for a tab and a view spec.
    ///
    /// The pure heart of the window: given the tab, the (possibly empty)
    /// query, the sort order, the advanced filter and the expand state,
    /// produce exactly the rows to draw, in order, each resolved to its
    /// label, icon, decorations and arrow. Tested directly; the Bevy side
    /// only renders the result.
    pub(crate) fn build_rows(&self, tab: InventoryTab, spec: &ViewSpec<'_>) -> Vec<DisplayRow> {
        let needle = spec.query.trim().to_lowercase();
        let worn = self.worn_set(spec.tracked_attachments);
        let filter_active = spec.filter.is_active();
        let passes = |item: &ItemInfo| {
            spec.filter.passes(
                item,
                worn.contains(&item.item_id),
                spec.now_unix,
                spec.login_unix,
            )
        };
        match tab {
            InventoryTab::Everything if needle.is_empty() && !filter_active => {
                self.tree_rows(&worn, spec.sort)
            }
            // A text search and an active filter narrow the tree the same
            // way: matching items shown inside their expanded ancestors.
            InventoryTab::Everything => {
                self.filtered_rows(&needle, &worn, spec.sort, filter_active, &passes)
            }
            InventoryTab::Recent => self.recent_rows(&needle, &worn, filter_active, &passes),
            InventoryTab::Worn => self.worn_rows(&needle, &worn, spec.sort, &passes),
        }
    }

    /// Every currently worn item key — the COF links' targets, the legacy
    /// wearables set, and the viewer-tracked attachments — the set the row
    /// decorations (bold + `(worn)`) read.
    pub(crate) fn worn_set(&self, tracked: &HashSet<InventoryKey>) -> HashSet<InventoryKey> {
        let mut set: HashSet<InventoryKey> = self
            .worn_item_keys()
            .into_iter()
            .map(|(key, _name, _ty)| key)
            .collect();
        set.extend(tracked.iter().copied());
        set
    }

    /// The Everything tab's tree, depth-first from the roots.
    fn tree_rows(&self, worn: &HashSet<InventoryKey>, sort: SortSpec) -> Vec<DisplayRow> {
        let mut rows = Vec::new();
        for &root in &self.roots {
            self.emit_folder(root, 0, worn, sort, &mut rows);
        }
        rows
    }

    /// A folder's child folders in display order: name order from the index,
    /// with the system folders stably lifted to the top when the sort asks
    /// for it (the reference's `SO_SYSTEM_FOLDERS_TO_TOP`).
    fn ordered_children(
        &self,
        folder: InventoryFolderKey,
        sort: SortSpec,
    ) -> Vec<InventoryFolderKey> {
        let mut children: Vec<InventoryFolderKey> = self.children_of(folder).to_vec();
        if sort.system_folders_to_top {
            children.sort_by_key(|key| {
                self.folders
                    .get(key)
                    .is_none_or(|info| info.folder_type == FolderType::None)
            });
        }
        children
    }

    /// A folder's items in display order: the held name order, or newest
    /// first under the date sort (the reference's `SO_DATE`, ties by name).
    fn ordered_items(&self, folder: InventoryFolderKey, sort: SortSpec) -> Vec<&ItemInfo> {
        let mut items: Vec<&ItemInfo> = self.items_of(folder).iter().collect();
        if sort.by_date {
            items.sort_by(|a, b| {
                b.creation_date
                    .cmp(&a.creation_date)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
        }
        items
    }

    /// Emit `folder`'s row, then — if expanded — its child folders and items,
    /// indented one level deeper.
    fn emit_folder(
        &self,
        folder: InventoryFolderKey,
        depth: usize,
        worn: &HashSet<InventoryKey>,
        sort: SortSpec,
        rows: &mut Vec<DisplayRow>,
    ) {
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
        for child in self.ordered_children(folder, sort) {
            self.emit_folder(child, child_depth, worn, sort, rows);
        }
        for item in self.ordered_items(folder, sort) {
            rows.push(decorated_item_row(item, child_depth, worn));
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
            suffix: String::new(),
            bold: false,
            italic: false,
        }
    }

    /// The Everything tab under an active search **or filter**: the folder
    /// **hierarchy** narrowed to the branches that lead to a match — every
    /// ancestor folder of a matching item (or, for a pure text search, a
    /// matching folder name) is kept and shown expanded — the way the
    /// reference viewer filters its tree. Only loaded folders' items are
    /// searchable (an unfetched folder's contents are not held).
    fn filtered_rows(
        &self,
        needle: &str,
        worn: &HashSet<InventoryKey>,
        sort: SortSpec,
        filter_active: bool,
        passes: &dyn Fn(&ItemInfo) -> bool,
    ) -> Vec<DisplayRow> {
        // A folder-name hit keeps its branch only under a pure text search:
        // with a filter active the reference shows only folders that still
        // hold passing content (the non-empty-folders rule).
        let folder_names_match = !needle.is_empty() && !filter_active;
        let mut keep = HashSet::new();
        for &root in &self.roots {
            self.mark_matching_subtree(root, needle, folder_names_match, passes, &mut keep);
        }
        let mut rows = Vec::new();
        for &root in &self.roots {
            if keep.contains(&root) {
                self.emit_filtered_folder(root, 0, needle, &keep, worn, sort, passes, &mut rows);
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

    /// Mark `folder` in `keep` if it, or anything in its subtree, matches,
    /// and return whether it did — so an ancestor of a match is retained.
    fn mark_matching_subtree(
        &self,
        folder: InventoryFolderKey,
        needle: &str,
        folder_names_match: bool,
        passes: &dyn Fn(&ItemInfo) -> bool,
        keep: &mut HashSet<InventoryFolderKey>,
    ) -> bool {
        let mut any = folder_names_match && self.folder_name_matches(folder, needle);
        for &child in self.children_of(folder) {
            if self.mark_matching_subtree(child, needle, folder_names_match, passes, keep) {
                any = true;
            }
        }
        for item in self.items_of(folder) {
            if item_matches(item, needle, passes) {
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
    #[expect(
        clippy::too_many_arguments,
        reason = "a recursive tree emitter threading the query, filter, sort, kept-folder and \
                  worn sets through each level"
    )]
    fn emit_filtered_folder(
        &self,
        folder: InventoryFolderKey,
        depth: usize,
        needle: &str,
        keep: &HashSet<InventoryFolderKey>,
        worn: &HashSet<InventoryKey>,
        sort: SortSpec,
        passes: &dyn Fn(&ItemInfo) -> bool,
        rows: &mut Vec<DisplayRow>,
    ) {
        rows.push(self.folder_row(folder, depth, RowArrow::Expanded));
        let child_depth = depth.saturating_add(1);
        for child in self.ordered_children(folder, sort) {
            if keep.contains(&child) {
                self.emit_filtered_folder(
                    child,
                    child_depth,
                    needle,
                    keep,
                    worn,
                    sort,
                    passes,
                    rows,
                );
            }
        }
        for item in self.ordered_items(folder, sort) {
            if item_matches(item, needle, passes) {
                rows.push(decorated_item_row(item, child_depth, worn));
            }
        }
    }

    /// The Recent tab: the received-since-login list, narrowed by the query
    /// and — where the item is held — the filter. A recent entry whose item
    /// is held in a loaded folder is decorated like a tree row; one not yet
    /// loaded draws plain (and is hidden by an active filter, whose
    /// dimensions it cannot answer).
    fn recent_rows(
        &self,
        needle: &str,
        worn: &HashSet<InventoryKey>,
        filter_active: bool,
        passes: &dyn Fn(&ItemInfo) -> bool,
    ) -> Vec<DisplayRow> {
        self.recent
            .iter()
            .filter(|item| needle.is_empty() || item.name.to_lowercase().contains(needle))
            .filter_map(|item| match self.find_item(item.key) {
                Some(info) => passes(info).then(|| decorated_item_row(info, 0, worn)),
                None => (!filter_active).then(|| item_row(item.key, &item.name, item.inv_type, 0)),
            })
            .collect()
    }

    /// The worn set: each worn item's key with a display-name / type hint,
    /// merged from the Current Outfit Folder's links (the modern worn set — a
    /// link's asset id names the original item) and the legacy
    /// `AgentWearables` set. Order: COF first, then wearables not already
    /// present.
    fn worn_item_keys(&self) -> Vec<(InventoryKey, String, InventoryType)> {
        let mut keys: Vec<(InventoryKey, String, InventoryType)> = Vec::new();
        let mut seen: HashSet<InventoryKey> = HashSet::new();
        if let Some(cof) = self.cof {
            for entry in self.items_of(cof) {
                // A COF entry is normally a link (`AT_LINK`, asset id = the
                // linked item); take the target so the row names the original.
                let target = if entry.asset_type == AssetType::Other(24) {
                    InventoryKey::from(entry.asset_id)
                } else {
                    entry.item_id
                };
                if seen.insert(target) {
                    keys.push((target, entry.name.clone(), entry.inv_type));
                }
            }
        }
        for worn in &self.wearables {
            if seen.insert(worn.item_id) {
                keys.push((
                    worn.item_id,
                    wearable_label(worn.wearable_type).to_owned(),
                    InventoryType::Wearable,
                ));
            }
        }
        keys
    }

    /// Mark in `keep` every folder whose subtree holds a worn item, so the Worn
    /// tab can show each worn item **inside its folder hierarchy**; returns
    /// whether this subtree held one.
    fn mark_worn_subtree(
        &self,
        folder: InventoryFolderKey,
        worn: &HashSet<InventoryKey>,
        keep: &mut HashSet<InventoryFolderKey>,
    ) -> bool {
        let mut any = false;
        for &child in self.children_of(folder) {
            if self.mark_worn_subtree(child, worn, keep) {
                any = true;
            }
        }
        if self
            .items_of(folder)
            .iter()
            .any(|item| worn.contains(&item.item_id))
        {
            any = true;
        }
        if any {
            keep.insert(folder);
        }
        any
    }

    /// Emit a kept folder of the Worn tab (shown expanded) and, recursively,
    /// its kept child folders and its worn items, narrowed by the query and
    /// the filter.
    #[expect(
        clippy::too_many_arguments,
        reason = "a recursive tree emitter threading the worn set, the kept-folder set, the \
                  placed-item record, the sort, the filter and the query through each level"
    )]
    fn emit_worn_folder(
        &self,
        folder: InventoryFolderKey,
        depth: usize,
        worn: &HashSet<InventoryKey>,
        keep: &HashSet<InventoryFolderKey>,
        placed: &mut HashSet<InventoryKey>,
        needle: &str,
        sort: SortSpec,
        passes: &dyn Fn(&ItemInfo) -> bool,
        rows: &mut Vec<DisplayRow>,
    ) {
        rows.push(self.folder_row(folder, depth, RowArrow::Expanded));
        let child_depth = depth.saturating_add(1);
        for child in self.ordered_children(folder, sort) {
            if keep.contains(&child) {
                self.emit_worn_folder(
                    child,
                    child_depth,
                    worn,
                    keep,
                    placed,
                    needle,
                    sort,
                    passes,
                    rows,
                );
            }
        }
        for item in self.ordered_items(folder, sort) {
            if worn.contains(&item.item_id) && item_matches(item, needle, passes) {
                placed.insert(item.item_id);
                rows.push(decorated_item_row(item, child_depth, worn));
            }
        }
    }

    /// The Worn tab: the worn set (COF links falling back to the legacy
    /// `AgentWearables`), each item shown **inside its folder hierarchy** —
    /// the ancestor folders that lead to it, expanded, the way the Everything
    /// search narrows the tree. A worn item whose containing folder is not yet
    /// fetched cannot be placed and is appended as a flat row instead, so
    /// nothing worn is ever hidden. Narrowed by the query and the filter.
    fn worn_rows(
        &self,
        needle: &str,
        worn: &HashSet<InventoryKey>,
        sort: SortSpec,
        passes: &dyn Fn(&ItemInfo) -> bool,
    ) -> Vec<DisplayRow> {
        let matches = |name: &str| needle.is_empty() || name.to_lowercase().contains(needle);
        let worn_keys = self.worn_item_keys();
        // The hierarchy half: folders on the path to a loaded worn item.
        let mut keep = HashSet::new();
        for &root in &self.roots {
            self.mark_worn_subtree(root, worn, &mut keep);
        }
        let mut rows = Vec::new();
        let mut placed: HashSet<InventoryKey> = HashSet::new();
        for &root in &self.roots {
            if keep.contains(&root) {
                self.emit_worn_folder(
                    root,
                    0,
                    worn,
                    &keep,
                    &mut placed,
                    needle,
                    sort,
                    passes,
                    &mut rows,
                );
            }
        }
        // The flat tail: worn items whose place in the tree is not loaded yet.
        for (key, name, inv_type) in worn_keys {
            if !placed.contains(&key) && matches(&name) {
                rows.push(item_row(key, &name, inv_type, 0));
            }
        }
        rows
    }
}

/// The item sort order the gear menu drives — the reference's
/// `LLInventoryFilter::ESortOrder` bits this viewer supports. Folders are
/// always name-sorted (the wire carries no folder dates), so the reference's
/// `SO_FOLDERS_BY_NAME` is permanently on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SortSpec {
    /// Items newest-first (`SO_DATE`) instead of by name.
    pub(crate) by_date: bool,
    /// System folders stably lifted above user folders
    /// (`SO_SYSTEM_FOLDERS_TO_TOP`).
    pub(crate) system_folders_to_top: bool,
}

impl Default for SortSpec {
    /// The reference default (`InventorySortOrder` = 7): most-recent items
    /// first, folders by name, system folders on top.
    fn default() -> Self {
        Self {
            by_date: true,
            system_folders_to_top: true,
        }
    }
}

/// Everything the row flattening reads beyond the model itself — the query,
/// the tracked-attachment worn source, the sort order, the advanced filter
/// and the two timestamps its date dimensions compare against.
pub(crate) struct ViewSpec<'a> {
    /// The search query (empty for none).
    pub(crate) query: &'a str,
    /// The viewer-tracked worn attachments
    /// ([`crate::inventory_actions::WornAttachments`]).
    pub(crate) tracked_attachments: &'a HashSet<InventoryKey>,
    /// The sort order.
    pub(crate) sort: SortSpec,
    /// The advanced filter ([`crate::inventory_filters`]).
    pub(crate) filter: &'a crate::inventory_filters::ItemFilter,
    /// The current unix time, for the filter's hours/days cutoff.
    pub(crate) now_unix: i64,
    /// The session's login unix time, for the filter's since-login switch.
    pub(crate) login_unix: i64,
}

/// Whether an item matches the query **and** the filter predicate.
fn item_matches(item: &ItemInfo, needle: &str, passes: &dyn Fn(&ItemInfo) -> bool) -> bool {
    (needle.is_empty() || item.name.to_lowercase().contains(needle)) && passes(item)
}

/// Build an item's display row at a given depth, undecorated (used where no
/// [`ItemInfo`] is held — a Recent entry or an unplaced worn item).
fn item_row(key: InventoryKey, name: &str, inv_type: InventoryType, depth: usize) -> DisplayRow {
    DisplayRow {
        key: RowKey::Item(key),
        depth,
        name: name.to_owned(),
        icon: item_icon(inv_type),
        arrow: RowArrow::Leaf,
        suffix: String::new(),
        bold: false,
        italic: false,
    }
}

/// Build a loaded item's display row, decorated with the permission / link
/// suffixes and the worn emphasis (`viewer-inventory-row-decorations`).
fn decorated_item_row(item: &ItemInfo, depth: usize, worn: &HashSet<InventoryKey>) -> DisplayRow {
    let is_worn = worn.contains(&item.item_id);
    DisplayRow {
        key: RowKey::Item(item.item_id),
        depth,
        name: item.name.clone(),
        icon: item_icon(item.inv_type),
        arrow: RowArrow::Leaf,
        suffix: item_suffix(item, is_worn),
        bold: is_worn,
        // The reference draws an unworn link italic (worn wins over link).
        italic: !is_worn && is_link_asset(item.asset_type),
    }
}

/// Whether an asset type is one of the two inventory-link types.
const fn is_link_asset(asset_type: AssetType) -> bool {
    matches!(
        asset_type,
        AssetType::Other(ASSET_TYPE_LINK | ASSET_TYPE_LINK_FOLDER)
    )
}

/// The `AT_LINK` asset-type wire code: an inventory link to an item.
const ASSET_TYPE_LINK: i32 = 24;

/// The `AT_LINK_FOLDER` asset-type wire code: an inventory link to a folder.
const ASSET_TYPE_LINK_FOLDER: i32 = 25;

/// The trailing suffix the reference viewer draws after an item's label
/// (`LLItemBridge::getLabelSuffix`): a link is marked `(link)` (its
/// permissions are the target's, not its own); otherwise each withheld owner
/// permission is spelled out; and a worn item is marked `(worn)`.
pub(crate) fn item_suffix(item: &ItemInfo, worn: bool) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if is_link_asset(item.asset_type) {
        parts.push("(link)");
    } else {
        let owner = item.permissions.owner;
        if !owner.contains(Permissions::COPY) {
            parts.push("(no copy)");
        }
        if !owner.contains(Permissions::MODIFY) {
            parts.push("(no modify)");
        }
        if !owner.contains(Permissions::TRANSFER) {
            parts.push("(no transfer)");
        }
    }
    if worn {
        parts.push("(worn)");
    }
    parts.join(" ")
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
    /// The gear menu's sort order.
    sort: SortSpec,
}

/// The flattened rows the window is currently drawing — recomputed from the
/// model whenever it, the tab or the query changes, and read by the row binder
/// (and by the context-menu / drag modules, which resolve a pointer hit back to
/// the row it landed on).
#[derive(Resource, Default)]
pub(crate) struct InventoryView {
    /// The rows, top to bottom.
    rows: Vec<DisplayRow>,
}

impl InventoryView {
    /// The rows currently shown, top to bottom.
    pub(crate) fn rows(&self) -> &[DisplayRow] {
        &self.rows
    }
}

/// The list's selection — the usual list semantics: click selects one,
/// `Ctrl`-click toggles, `Shift`-click extends from the anchor.
///
/// Keyed by [`RowKey`] (not row index), so the selection survives a view
/// rebuild (expand / collapse, a fetched page); the anchor is an index because
/// a `Shift` range is a *visual* span of the current view.
#[derive(Resource, Debug, Default)]
pub(crate) struct InventorySelection {
    /// The selected row keys.
    selected: HashSet<RowKey>,
    /// The range anchor: the index (in the current view) of the last plain or
    /// `Ctrl` click.
    anchor: Option<usize>,
}

impl InventorySelection {
    /// Whether `key` is selected.
    pub(crate) fn contains(&self, key: RowKey) -> bool {
        self.selected.contains(&key)
    }

    /// Select exactly `key` (a plain click), anchoring at `index`.
    pub(crate) fn select_single(&mut self, key: RowKey, index: usize) {
        self.selected.clear();
        self.selected.insert(key);
        self.anchor = Some(index);
    }

    /// Toggle `key` (a `Ctrl`-click), anchoring at `index`.
    fn toggle(&mut self, key: RowKey, index: usize) {
        if !self.selected.remove(&key) {
            self.selected.insert(key);
        }
        self.anchor = Some(index);
    }

    /// Select the visual range between the anchor and `index` (a `Shift`-click)
    /// over the current `rows`, replacing the selection. Without an anchor it
    /// falls back to a single select.
    fn select_range(&mut self, rows: &[DisplayRow], index: usize) {
        let Some(anchor) = self.anchor else {
            if let Some(row) = rows.get(index) {
                self.select_single(row.key(), index);
            }
            return;
        };
        let (first, last) = if anchor <= index {
            (anchor, index)
        } else {
            (index, anchor)
        };
        self.selected.clear();
        for row in rows
            .iter()
            .skip(first)
            .take(last.saturating_sub(first).saturating_add(1))
        {
            self.selected.insert(row.key());
        }
    }

    /// Clear the selection (a click on the list background).
    pub(crate) fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    /// The selection when it is exactly **one** row — the toolbar + menu's
    /// create destination (an ambiguous multi-selection falls back to the
    /// root instead of guessing).
    pub(crate) fn single(&self) -> Option<RowKey> {
        if self.selected.len() == 1 {
            self.selected.iter().copied().next()
        } else {
            None
        }
    }
}

/// The **inline rename** state: the reference edits a row's label in place,
/// and so does this — while active, the row's label node is hidden and a text
/// field sits in the row; `Enter` commits, `Escape` cancels, and scrolling the
/// row away cancels (the pooled row is about to be recycled).
#[derive(Resource, Debug, Default)]
pub(crate) struct InlineRename {
    /// A rename asked for but not yet started — the row may not be on screen
    /// (or, for a freshly created folder, not in the model) yet.
    pub(crate) pending: Option<RowKey>,
    /// The live in-row editor.
    active: Option<ActiveRename>,
}

/// The live inline-rename editor's entities.
#[derive(Debug, Clone, Copy)]
struct ActiveRename {
    /// The row key being renamed.
    key: RowKey,
    /// The key's index in the view when the editor opened.
    index: usize,
    /// The pooled row entity hosting the editor.
    row: Entity,
    /// The text field.
    field: Entity,
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

    /// The scrolling viewport entity (carries [`VirtualList`]) — the drag-and-
    /// drop module maps pointer positions to rows against it.
    pub(crate) const fn viewport(&self) -> Entity {
        self.viewport
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
    /// The trailing decoration text — the permission / link / worn suffixes
    /// the reference viewer draws after the label ("(no copy) (worn)", …).
    /// Empty for an undecorated row.
    suffix: String,
    /// Whether the label is drawn **bold** — a currently worn item, so the
    /// outfit stands out in the tree (the reference's worn emphasis,
    /// `LLItemBridge::getLabelStyle`).
    bold: bool,
    /// Whether the label is drawn **italic** — an inventory link that is not
    /// worn (the reference's link emphasis; worn wins over link).
    italic: bool,
}

impl DisplayRow {
    /// What this row refers to.
    pub(crate) const fn key(&self) -> RowKey {
        self.key
    }

    /// The row's label text.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The row's type-icon glyph.
    pub(crate) const fn icon(&self) -> &'static str {
        self.icon
    }
}

/// What a [`DisplayRow`] points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RowKey {
    /// A folder, which a click expands or collapses.
    Folder(InventoryFolderKey),
    /// An item (context actions and drag-and-drop resolve it through the
    /// model).
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
#[derive(Message, Debug, Clone, Copy)]
pub(crate) enum InventoryUiAction {
    /// Switch to a tab.
    SelectTab(InventoryTab),
    /// Expand every folder.
    ExpandAll,
    /// Collapse every folder.
    CollapseAll,
    /// Toggle a folder's expand state (from a row click).
    ToggleFolder(InventoryFolderKey),
    /// Expand one folder (idempotent) — the drag-hover auto-expand
    /// ([`crate::inventory_drag`]), which must never *collapse* the folder a
    /// drag lingers over.
    ExpandFolder(InventoryFolderKey),
}

/// The `element` the inventory gear menu attributes its picks to, so
/// [`route_gear_menu`] routes its own menu and no other.
const INVENTORY_GEAR_ELEMENT: &str = "inventory-gear";

/// The condition key held while items sort by name.
const GEAR_SORT_NAME: &str = "gear-sort-by-name";

/// The condition key held while items sort newest-first.
const GEAR_SORT_DATE: &str = "gear-sort-by-date";

/// The condition key held while system folders sort to the top.
const GEAR_SYSTEM_TOP: &str = "gear-system-folders-top";

/// The condition key for "Sort Folders Always by Name" — always held, since
/// folders are permanently name-sorted (no folder dates on the wire); the
/// entry itself stays greyed.
const GEAR_FOLDERS_BY_NAME: &str = "gear-folders-by-name";

/// The condition key held while the filters floater is open.
const GEAR_FILTERS_OPEN: &str = "gear-filters-open";

/// The inventory window's gear (options) menu — the reference's
/// `menu_inventory_gear_default` set (minus the Firestorm-only extras, the
/// way the context menus omit the marketplace block), on [`crate::menu`]'s
/// reusable widget. Its label is the gear glyph (U+2699). Entries whose
/// feature does not exist yet keep their reference place greyed on
/// [`UNIMPLEMENTED`](crate::avatar_menu::UNIMPLEMENTED).
static INVENTORY_GEAR_MENU: MenuDef = MenuDef {
    label: "\u{2699}",
    items: &[
        MenuItemDef::Command(
            MenuCommand::new("New Inventory Window", "new-window")
                .enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Sort by Name", "sort-by-name").checked_when(GEAR_SORT_NAME),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Sort by Most Recent", "sort-by-recent").checked_when(GEAR_SORT_DATE),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Sort Folders Always by Name", "sort-folders-by-name")
                .checked_when(GEAR_FOLDERS_BY_NAME)
                .enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Sort System Folders to Top", "sort-system-folders-to-top")
                .checked_when(GEAR_SYSTEM_TOP),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Show Filters...", "show-filters").checked_when(GEAR_FILTERS_OPEN),
        ),
        MenuItemDef::Command(MenuCommand::new("Reset Filters", "reset-filters")),
        MenuItemDef::Command(MenuCommand::new("Expand All Folders", "expand-all")),
        MenuItemDef::Command(MenuCommand::new("Collapse All Folders", "collapse-all")),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new(
            "Empty Lost And Found",
            "empty-lost-and-found",
        )),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Save Texture As", "save-texture")
                .enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Share", "share").enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Find Original", "find-original")
                .enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Find All Links", "find-links")
                .enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Replace Links", "replace-links")
                .enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(
            MenuCommand::new("Show Links", "filter-show-links")
                .enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Show Only Links", "filter-only-links")
                .enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Command(
            MenuCommand::new("Hide Links", "filter-hide-links")
                .enabled_when(crate::avatar_menu::UNIMPLEMENTED),
        ),
        MenuItemDef::Separator,
        MenuItemDef::Command(MenuCommand::new("Empty Trash", "empty-trash")),
    ],
};

/// The marker on the gear button's host entity, whose [`MenuConditions`] the
/// sort / filter check marks read.
#[derive(Component)]
struct InventoryGearHost;

/// Keep the gear host's [`MenuConditions`] current: which sort mode is
/// active, and whether the filters floater is open.
fn update_gear_conditions(
    state: Res<InventoryState>,
    filters_ui: Option<Res<crate::inventory_filters::InventoryFiltersUi>>,
    panels: Query<&UiPanelShown>,
    mut hosts: Query<&mut MenuConditions, With<InventoryGearHost>>,
) {
    let filters_open = filters_ui
        .and_then(|ui| panels.get(ui.panel()).ok().map(|shown| shown.0))
        .unwrap_or(false);
    let mut wanted: Vec<&'static str> = Vec::new();
    wanted.push(if state.sort.by_date {
        GEAR_SORT_DATE
    } else {
        GEAR_SORT_NAME
    });
    if state.sort.system_folders_to_top {
        wanted.push(GEAR_SYSTEM_TOP);
    }
    wanted.push(GEAR_FOLDERS_BY_NAME);
    if filters_open {
        wanted.push(GEAR_FILTERS_OPEN);
    }
    for mut conditions in &mut hosts {
        if conditions.0 != wanted {
            conditions.0.clone_from(&wanted);
        }
    }
}

/// Route the gear menu's picks (a [`UiAction`]): the expand / collapse and
/// sort toggles act on the window state, Show / Reset Filters drive the
/// filters floater ([`crate::inventory_filters`]), and the two emptiers issue
/// the same purge the context menu does.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the pick stream, the \
              window / filter state, the filters floater, the model for the emptiers, and the \
              command / action channels"
)]
fn route_gear_menu(
    mut picks: MessageReader<UiAction>,
    mut actions: MessageWriter<InventoryUiAction>,
    mut state: ResMut<InventoryState>,
    mut filter_state: ResMut<crate::inventory_filters::InventoryFilterState>,
    filters_ui: Option<Res<crate::inventory_filters::InventoryFiltersUi>>,
    model: Res<InventoryModel>,
    mut panels: Query<&mut UiPanelShown>,
    mut fields: Query<&mut EditableText>,
    mut commands: MessageWriter<SlCommand>,
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
            "sort-by-name" => {
                state.sort.by_date = false;
            }
            "sort-by-recent" => {
                state.sort.by_date = true;
            }
            "sort-system-folders-to-top" => {
                state.sort.system_folders_to_top = !state.sort.system_folders_to_top;
            }
            "show-filters" => {
                if let Some(ui) = &filters_ui
                    && let Ok(mut shown) = panels.get_mut(ui.panel())
                {
                    shown.0 = !shown.0;
                }
            }
            "reset-filters" => {
                crate::inventory_filters::apply_reset(
                    &mut filter_state,
                    filters_ui.as_deref(),
                    &mut fields,
                );
            }
            "empty-trash" => {
                if let Some(trash) = model.folder_by_type(FolderType::Trash) {
                    commands.write(SlCommand(Command::PurgeInventoryDescendents(trash)));
                    commands.write(SlCommand(Command::QueryInventoryFolders));
                    query_folder_page(trash, &mut commands);
                }
            }
            "empty-lost-and-found" => {
                if let Some(lost) = model.folder_by_type(FolderType::LostAndFound) {
                    commands.write(SlCommand(Command::PurgeInventoryDescendents(lost)));
                    commands.write(SlCommand(Command::QueryInventoryFolders));
                    query_folder_page(lost, &mut commands);
                }
            }
            _other => {}
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
    state: Res<InventoryState>,
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
        // The context menu's wear / take-off gating reads the legacy worn set;
        // keep it fresh whenever the window opens.
        commands.write(SlCommand(Command::RequestWearables));
        // Re-opening onto the Worn tab needs the hierarchy sources too.
        if state.tab == InventoryTab::Worn {
            request_all_agent_folders(&mut model, &mut commands);
        }
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
                    // A bulk update is how a paste-copy, a give, or a created
                    // item lands; re-query its folder (if we hold it) so the
                    // tree shows the new item without a manual refresh.
                    if model.requested.contains(&item.folder_id) {
                        query_folder_page(item.folder_id, &mut commands);
                    }
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

/// Request the contents of every not-yet-fetched folder of the **agent** tree
/// (the Library stays lazy) — the Worn tab's way of locating each worn item's
/// place in the hierarchy. Each folder is requested at most once and the
/// session's fetcher serves repeats from its disk cache, so this converges.
fn request_all_agent_folders(model: &mut InventoryModel, commands: &mut MessageWriter<SlCommand>) {
    let wanted: Vec<InventoryFolderKey> = model
        .folders
        .keys()
        .copied()
        .filter(|key| !model.library_folders.contains(key) && model.needs_fetch(*key))
        .collect();
    for folder in wanted {
        request_folder(model, folder, commands);
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

/// Send the page query for a folder. Also the **refresh** path after a
/// mutation: the session's cache applies moves / renames / removals
/// optimistically, so re-querying a page immediately reflects them in the
/// model (used by [`crate::inventory_actions`] and [`crate::inventory_drag`]).
pub(crate) fn query_folder_page(
    folder: InventoryFolderKey,
    commands: &mut MessageWriter<SlCommand>,
) {
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
                    // Placing each worn item in its folder hierarchy needs the
                    // folder that holds it — unknowable until fetched — so the
                    // Worn tab kicks off the background fetch of every agent
                    // folder (the session's fetcher dedupes and disk-caches).
                    request_all_agent_folders(&mut model, &mut commands);
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
            InventoryUiAction::ExpandFolder(folder) => {
                if model.expanded.insert(folder) && model.needs_fetch(folder) {
                    request_folder(&mut model, folder, &mut commands);
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
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the model, the window / \
              filter / worn state, the login clock, the UI handles and the list"
)]
fn rebuild_view(
    model: Res<InventoryModel>,
    state: Res<InventoryState>,
    worn: Res<crate::inventory_actions::WornAttachments>,
    filters: Res<crate::inventory_filters::InventoryFilterState>,
    login_time: Res<crate::inventory_filters::SessionLoginTime>,
    ui: Option<Res<InventoryUi>>,
    mut view: ResMut<InventoryView>,
    mut lists: Query<&mut VirtualList>,
) {
    if !model.is_changed() && !state.is_changed() && !worn.is_changed() && !filters.is_changed() {
        return;
    }
    let Some(ui) = ui else {
        return;
    };
    // Reset the scroll only when the *content* changes wholesale — a tab switch,
    // a new search, a filter edit, or an open. Expanding or collapsing a folder
    // changes only the model, so the position is kept (the reference viewer
    // keeps it too); the generic list clamps it if the list got shorter.
    let reset_scroll = state.is_changed() || filters.is_changed();
    let now_unix = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs()),
    )
    .unwrap_or(0);
    view.rows = model.build_rows(
        state.tab,
        &ViewSpec {
            query: &state.query,
            tracked_attachments: &worn.items,
            sort: state.sort,
            filter: &filters.filter,
            now_unix,
            login_unix: login_time.0,
        },
    );
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
    /// The trailing decoration text (permissions / link / worn), dimmer than
    /// the label.
    suffix: Entity,
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
            // Transparent until the drag-and-drop hover paints it as the drop
            // target ([`crate::inventory_drag`]).
            BackgroundColor(Color::NONE),
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
        let suffix = commands
            .spawn((
                Text::new(""),
                UiFont::Sans.at(ROW_FONT_SIZE),
                TextColor(SUFFIX_COLOR),
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
                suffix,
            })
            .observe(on_row_press)
            .observe(crate::inventory_actions::on_row_context)
            .observe(crate::inventory_drag::on_row_drag_start)
            .observe(crate::inventory_drag::on_row_drag_end);
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
    mut fonts: Query<&mut TextFont>,
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
        // A worn item's label draws bold, an unworn link italic (the
        // reference's `getLabelStyle`); write-guarded so an unchanged style
        // does not re-measure the text.
        if let Ok(mut font) = fonts.get_mut(parts.label) {
            let weight = if display.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            };
            if font.weight != weight {
                font.weight = weight;
            }
            let style = if display.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            };
            if font.style != style {
                font.style = style;
            }
        }
        if let Ok((mut text, _color)) = texts.get_mut(parts.suffix) {
            set_text(&mut text, &display.suffix);
        }
    }
}

/// A primary press anywhere in the viewport: focus the list (so the wheel
/// scrolls it), and — when the press landed on the empty background below the
/// last row, not on a row — clear the selection, the usual list semantics.
///
/// A press on a row also bubbles here (rows do not consume the primary press,
/// so an open menu's click-away dismissal still sees it); the row-vs-background
/// distinction is therefore made geometrically, not by propagation.
fn on_viewport_press(
    press: On<Pointer<Press>>,
    ui: Res<InventoryUi>,
    view: Res<InventoryView>,
    lists: Query<&VirtualList>,
    geometry: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut selection: ResMut<InventorySelection>,
    mut focus: ResMut<InputFocus>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.viewport, FocusCause::Navigated);
    let Ok(list) = lists.get(ui.viewport) else {
        return;
    };
    let Ok((computed, transform)) = geometry.get(ui.viewport) else {
        return;
    };
    // The row index the press falls on, in list space (top of the viewport plus
    // the scroll offset). Component-wise f32 maths, per the workspace
    // `arithmetic_side_effects` convention on `glam` operators.
    let scale = computed.inverse_scale_factor();
    let top = transform.translation.y * scale - computed.size().y * scale / 2.0;
    let offset = press.pointer_location.position.y - top + list.scroll_offset();
    if list.row_height > 0.0 && offset >= 0.0 {
        let past_end = offset / list.row_height >= index_to_f32(view.rows.len());
        if past_end {
            selection.clear();
        }
    }
}

/// Whether a logical-pixel point sits inside a UI node's rectangle.
/// Component-wise f32 maths, per the workspace `arithmetic_side_effects`
/// convention on `glam` operators.
fn point_in_node(point: Vec2, computed: &ComputedNode, transform: &UiGlobalTransform) -> bool {
    let scale = computed.inverse_scale_factor();
    let size = computed.size();
    let half_width = size.x * scale / 2.0;
    let half_height = size.y * scale / 2.0;
    let centre_x = transform.translation.x * scale;
    let centre_y = transform.translation.y * scale;
    (point.x - centre_x).abs() <= half_width && (point.y - centre_y).abs() <= half_height
}

/// A row was clicked: focus the window (so the wheel scrolls the list, not the
/// camera), update the **selection** with the usual list semantics — click
/// selects one, `Ctrl`-click toggles, `Shift`-click extends from the anchor —
/// and toggle a folder on its **arrow** or on a **double-click** (the
/// reference's `llfolderview` behaviour; a plain click no longer toggles, it
/// selects).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy observer's parameters are its injected resources: the row pool and its \
              parts, the arrow geometry for the hit test, the view, the modifier keys and \
              clock for the click semantics, and the selection / focus / action outputs"
)]
fn on_row_press(
    press: On<Pointer<Press>>,
    rows: Query<(&VirtualRow, &RowParts)>,
    arrows: Query<(&ComputedNode, &UiGlobalTransform)>,
    view: Res<InventoryView>,
    ui: Res<InventoryUi>,
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut last_click: Local<Option<(f64, usize)>>,
    mut selection: ResMut<InventorySelection>,
    mut focus: ResMut<InputFocus>,
    mut actions: MessageWriter<InventoryUiAction>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    focus.set(ui.viewport, FocusCause::Navigated);
    let Ok((row, parts)) = rows.get(press.entity) else {
        return;
    };
    let Some(index) = row.index else {
        return;
    };
    let Some(display) = view.rows.get(index) else {
        return;
    };
    let key = display.key();

    // A folder toggles on its expand arrow, or on a double-click anywhere.
    let now = time.elapsed_secs_f64();
    let double = last_click
        .is_some_and(|(at, last_index)| last_index == index && now - at <= DOUBLE_CLICK_SECS);
    *last_click = Some((now, index));
    if let RowKey::Folder(folder) = key {
        let on_arrow = arrows.get(parts.arrow).is_ok_and(|(computed, transform)| {
            point_in_node(press.pointer_location.position, computed, transform)
        });
        if on_arrow || double {
            actions.write(InventoryUiAction::ToggleFolder(folder));
        }
    }

    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if shift {
        selection.select_range(&view.rows, index);
    } else if ctrl {
        selection.toggle(key, index);
    } else {
        selection.select_single(key, index);
    }
}

/// Paint each pooled row's selection background. Skipped mid-drag, when the
/// drop-target highlight ([`crate::inventory_drag`]) owns the row backgrounds;
/// runs write-guarded otherwise, so a still list costs comparisons only.
fn paint_selection(
    ui: Option<Res<InventoryUi>>,
    selection: Res<InventorySelection>,
    view: Res<InventoryView>,
    drag: Res<crate::inventory_drag::InventoryDragState>,
    mut rows: Query<(&VirtualRow, &ChildOf, &mut BackgroundColor)>,
) {
    let Some(ui) = ui else {
        return;
    };
    if drag.is_active() {
        return;
    }
    for (row, child_of, mut background) in &mut rows {
        if child_of.parent() != ui.viewport {
            continue;
        }
        let selected = row
            .index
            .and_then(|index| view.rows.get(index))
            .is_some_and(|display| selection.contains(display.key()));
        let wanted = if selected {
            SELECTED_ROW_BACKGROUND
        } else {
            Color::NONE
        };
        if background.0 != wanted {
            background.0 = wanted;
        }
    }
}

/// Begin a pending inline rename once its row is on screen: hide the row's
/// label, put a pre-filled text field in its place, and focus it.
fn start_inline_rename(
    mut rename: ResMut<InlineRename>,
    view: Res<InventoryView>,
    ui: Option<Res<InventoryUi>>,
    rows: Query<(Entity, &VirtualRow, &RowParts, &ChildOf)>,
    mut nodes: Query<&mut Node>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if rename.active.is_some() {
        return;
    }
    let Some(key) = rename.pending else {
        return;
    };
    let Some(ui) = ui else {
        return;
    };
    // The row may not be in the view yet (a freshly created folder's skeleton
    // refresh is still in flight); keep the rename pending until it appears.
    let Some(index) = view.rows.iter().position(|row| row.key() == key) else {
        return;
    };
    let Some(name) = view.rows.get(index).map(DisplayRow::name) else {
        return;
    };
    for (entity, row, parts, child_of) in &rows {
        if child_of.parent() != ui.viewport || row.index != Some(index) {
            continue;
        }
        if let Ok(mut label) = nodes.get_mut(parts.label) {
            label.display = Display::None;
        }
        let field = crate::ui_text_input::spawn_text_input(
            &mut commands,
            entity,
            &crate::ui_text_input::TextInputSpec {
                initial: name.to_owned(),
                font_size: ROW_FONT_SIZE,
                width_glyphs: 22.0,
                ..crate::ui_text_input::TextInputSpec::new(
                    "inventory-rename",
                    crate::ui_text_input::TextInputKind::Line,
                )
            },
        );
        focus.set(field, FocusCause::Navigated);
        rename.pending = None;
        rename.active = Some(ActiveRename {
            key,
            index,
            row: entity,
            field,
        });
        return;
    }
}

/// Drive the open inline rename: `Enter` commits (an item renames through a
/// same-folder `MoveInventoryItem`, a folder through `UpdateInventoryFolder`),
/// `Escape` cancels, and the row scrolling away / rebinding cancels (its
/// pooled entity is about to show something else).
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources: the rename state, the \
              keyboard, the view / model to resolve the renamed row, the field to read, the \
              label to restore, and the command channels"
)]
fn drive_inline_rename(
    keyboard: Res<ButtonInput<KeyCode>>,
    view: Res<InventoryView>,
    model: Res<InventoryModel>,
    rows: Query<(&VirtualRow, &RowParts)>,
    fields: Query<&EditableText>,
    mut nodes: Query<&mut Node>,
    mut rename: ResMut<InlineRename>,
    mut commands_bevy: Commands,
    mut commands: MessageWriter<SlCommand>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        rename.pending = None;
        if let Some(active) = rename.active.take() {
            end_inline_rename(&active, &rows, &mut nodes, &mut commands_bevy);
        }
        return;
    }
    let Some(active) = rename.active else {
        return;
    };
    // Cancel when the hosting row no longer shows the renamed key (scrolled
    // away and recycled, or the view rebuilt underneath it).
    let still_bound = rows
        .get(active.row)
        .is_ok_and(|(row, _parts)| row.index == Some(active.index))
        && view
            .rows
            .get(active.index)
            .is_some_and(|display| display.key() == active.key);
    if !still_bound {
        rename.active = None;
        end_inline_rename(&active, &rows, &mut nodes, &mut commands_bevy);
        return;
    }
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }
    let new_name = fields
        .get(active.field)
        .map(|field| field.value().to_string().trim().to_owned())
        .unwrap_or_default();
    if !new_name.is_empty() {
        match active.key {
            RowKey::Item(item_key) => {
                if let Some(item) = model.find_item(item_key)
                    && new_name != item.name
                {
                    commands.write(SlCommand(Command::MoveInventoryItem {
                        item_id: item.item_id,
                        folder_id: item.folder_id,
                        new_name,
                    }));
                    query_folder_page(item.folder_id, &mut commands);
                }
            }
            RowKey::Folder(folder_key) => {
                if let Some(info) = model.folder_info(folder_key)
                    && new_name != info.name
                    && let Some(parent) = info.parent_id
                {
                    commands.write(SlCommand(Command::UpdateInventoryFolder {
                        folder_id: info.folder_id,
                        parent_id: parent,
                        folder_type: info.folder_type,
                        name: new_name,
                    }));
                    commands.write(SlCommand(Command::QueryInventoryFolders));
                }
            }
        }
    }
    rename.active = None;
    end_inline_rename(&active, &rows, &mut nodes, &mut commands_bevy);
}

/// Tear the inline editor down: despawn the field and restore the row's label.
fn end_inline_rename(
    active: &ActiveRename,
    rows: &Query<(&VirtualRow, &RowParts)>,
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
            // Uses the shared top-trailing dock host.
            dock_host: None,
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
    let gear_host = crate::menu::spawn_menu_button(
        &mut commands,
        expand_row,
        ElementCx::new(),
        &INVENTORY_GEAR_MENU,
        INVENTORY_GEAR_ELEMENT,
    );
    // The gear entries' check marks (sort mode, filters open) read live
    // conditions off the host, the menu-bar pattern.
    commands
        .entity(gear_host)
        .insert((MenuConditions::default(), InventoryGearHost));
    // The reference's **+** (create) menu sits beside the gear: the Upload /
    // New-item entries (`menu_inventory_add.xml`), targeting the selected
    // folder. Its defs and routing live in [`crate::inventory_actions`].
    crate::menu::spawn_menu_button(
        &mut commands,
        expand_row,
        ElementCx::new(),
        &crate::inventory_actions::INVENTORY_ADD_MENU,
        crate::inventory_actions::INVENTORY_ADD_ELEMENT,
    );

    // Search field — the reusable search-field widget (`crate::ui_search`), the
    // same box the menu bar uses. It owns the border, the `×` clear button, the
    // placeholder and clear-on-`Escape`; the inventory owns only what the term
    // *means* (`read_search_field` narrows the shown rows to it). `bevy_ui_widgets`
    // focuses the field on click, so no press observer is needed here.
    let search = crate::ui_search::spawn_search_field(
        &mut commands,
        content,
        &crate::ui_search::SearchFieldSpec {
            tab_index: 4,
            font_size: CHROME_FONT_SIZE,
            placeholder: "Search inventory".to_owned(),
            search_glyph: true,
            ..crate::ui_search::SearchFieldSpec::new("inventory")
        },
    )
    .field;

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
        .observe(on_viewport_press)
        // A right-click on the list's empty background (below the last row)
        // targets the agent root — the reference's "New Folder / Paste at top
        // level" background menu. A row's own context observer consumes its
        // press first, so only true background clicks reach this.
        .observe(crate::inventory_actions::on_viewport_context)
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
        "",
    );
    spawn_sample_row(
        commands,
        list,
        cx,
        1,
        RowArrow::Leaf,
        item_icon(InventoryType::Wearable),
        "A shirt",
        "(no copy) (worn)",
    );
    list
}

/// Spawn one static sample row (indent, arrow, icon, label, suffix) for the
/// registry sample.
#[expect(
    clippy::too_many_arguments,
    reason = "a static sample spawner mirroring the live row's parts, each an argument"
)]
fn spawn_sample_row(
    commands: &mut Commands,
    parent: Entity,
    cx: crate::ui_element::ElementCx,
    depth: usize,
    arrow: RowArrow,
    icon: &str,
    label: &str,
    suffix: &str,
) {
    let row_entity = commands
        .spawn((
            Node {
                // A *minimum*, not the live rows' fixed height: the harness
                // sweeps the sample across large fonts and long pseudolocale
                // strings, where the wrapped label + suffix must be allowed to
                // grow the row instead of escaping a fixed box.
                min_height: Val::Px(ROW_HEIGHT),
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
    if !suffix.is_empty() {
        commands.spawn((
            Text::new(cx.text(suffix)),
            cx.font(UiFont::Sans),
            TextColor(SUFFIX_COLOR),
            ChildOf(row_entity),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DisplayRow, InventoryModel, InventoryTab, InventoryType, ItemInfo, RowArrow, RowKey,
        depth_indent, folder_icon, item_icon, item_suffix,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{FolderInfo, FolderState, FolderType, Permissions};
    use std::collections::HashSet;

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

    /// An empty tracked-attachment set, shared by the spec helpers.
    static EMPTY_TRACKED: std::sync::LazyLock<HashSet<sl_client_bevy::InventoryKey>> =
        std::sync::LazyLock::new(HashSet::new);

    /// Flatten with a plain spec: the given query / tracked set, name sort
    /// (so the fixtures' alphabetical expectations hold), no filter.
    fn build(
        model: &InventoryModel,
        tab: InventoryTab,
        query: &str,
        tracked: &HashSet<sl_client_bevy::InventoryKey>,
    ) -> Vec<DisplayRow> {
        let filter = crate::inventory_filters::ItemFilter::default();
        model.build_rows(
            tab,
            &super::ViewSpec {
                query,
                tracked_attachments: tracked,
                sort: super::SortSpec {
                    by_date: false,
                    system_folders_to_top: false,
                },
                filter: &filter,
                now_unix: 0,
                login_unix: 0,
            },
        )
    }

    /// A collapsed tree shows only the root.
    #[test]
    fn collapsed_tree_shows_only_roots() {
        let model = sample_model();
        let rows = build(&model, InventoryTab::Everything, "", &HashSet::new());
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
        let rows = build(&model, InventoryTab::Everything, "", &HashSet::new());
        assert_eq!(names(&rows), vec!["My Inventory", "Clothing", "Objects"]);

        model.expanded.insert(clothing);
        let rows = build(&model, InventoryTab::Everything, "", &HashSet::new());
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
        let rows = build(&model, InventoryTab::Everything, "shirt", &HashSet::new());
        assert_eq!(names(&rows), vec!["My Inventory", "Clothing", "Blue shirt"]);
        // A folder-name match keeps the folder and its ancestor, but not the
        // sibling folder or the non-matching item.
        let rows = build(&model, InventoryTab::Everything, "cloth", &HashSet::new());
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
        let rows = build(&model, InventoryTab::Everything, "", &HashSet::new());
        // "My Inventory" (agent root) comes before "Library", even though L < M.
        assert_eq!(names(&rows), vec!["My Inventory", "Library"]);
        let library_key =
            sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(0x50));
        assert!(model.library_folders.contains(&library_key));
    }

    /// The Worn tab shows each worn item inside its folder hierarchy, and a
    /// worn item whose folder is not loaded falls back to a flat row — nothing
    /// worn is hidden.
    #[test]
    fn worn_tab_shows_the_folder_hierarchy_with_a_flat_fallback() {
        let mut model = sample_model();
        // The "Blue shirt" (item 10, in Clothing) is worn, plus a second worn
        // item (0x99) that no loaded folder holds.
        model.wearables = vec![
            sl_client_bevy::Wearable {
                item_id: sl_client_bevy::InventoryKey::from(sl_client_bevy::Uuid::from_u128(10)),
                asset_id: None,
                wearable_type: sl_client_bevy::WearableType::Shirt,
            },
            sl_client_bevy::Wearable {
                item_id: sl_client_bevy::InventoryKey::from(sl_client_bevy::Uuid::from_u128(0x99)),
                asset_id: None,
                wearable_type: sl_client_bevy::WearableType::Pants,
            },
        ];
        let rows = build(&model, InventoryTab::Worn, "", &HashSet::new());
        // Hierarchy first — the ancestors of the placed shirt, expanded — then
        // the flat fallback for the unplaced pants.
        assert_eq!(
            names(&rows),
            vec!["My Inventory", "Clothing", "Blue shirt", "Pants"]
        );
        let shirt = rows.iter().find(|row| row.name == "Blue shirt");
        assert_eq!(shirt.map(|row| row.depth), Some(2));
        let pants = rows.iter().find(|row| row.name == "Pants");
        assert_eq!(pants.map(|row| row.depth), Some(0));
        // The sibling "Objects" folder holds nothing worn and is not shown.
        assert!(!names(&rows).contains(&"Objects"));
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
        let rows = build(&model, InventoryTab::Recent, "", &HashSet::new());
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

    /// **The gear menu's entry table, pinned** (the menu-address convention:
    /// moving an entry must be a deliberate edit here).
    #[test]
    fn gear_menu_keeps_every_entry() {
        let mut entries = Vec::new();
        for item in super::INVENTORY_GEAR_MENU.items {
            if let crate::menu::MenuItemDef::Command(command) = item {
                entries.push((command.label, command.action));
            }
        }
        let expected = vec![
            ("New Inventory Window", "new-window"),
            ("Sort by Name", "sort-by-name"),
            ("Sort by Most Recent", "sort-by-recent"),
            ("Sort Folders Always by Name", "sort-folders-by-name"),
            ("Sort System Folders to Top", "sort-system-folders-to-top"),
            ("Show Filters...", "show-filters"),
            ("Reset Filters", "reset-filters"),
            ("Expand All Folders", "expand-all"),
            ("Collapse All Folders", "collapse-all"),
            ("Empty Lost And Found", "empty-lost-and-found"),
            ("Save Texture As", "save-texture"),
            ("Share", "share"),
            ("Find Original", "find-original"),
            ("Find All Links", "find-links"),
            ("Replace Links", "replace-links"),
            ("Show Links", "filter-show-links"),
            ("Show Only Links", "filter-only-links"),
            ("Hide Links", "filter-hide-links"),
            ("Empty Trash", "empty-trash"),
        ];
        assert_eq!(
            entries, expected,
            "a gear-menu entry moved — if intended, bless it by editing this table"
        );
    }

    /// The date sort puts newer items first (ties by name), and the
    /// system-folders-to-top sort lifts typed folders above user folders
    /// while keeping name order within each group.
    #[test]
    fn sorting_orders_items_and_folders() {
        let mut model = InventoryModel::default();
        model.merge_folders(
            &[
                folder(1, None, "My Inventory", FolderType::RootInventory),
                folder(2, Some(1), "Aardvark", FolderType::None),
                folder(3, Some(1), "Clothing", FolderType::Clothing),
            ],
            false,
        );
        let root = sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(1));
        let mut old_item = item(10, 1, "Old thing", InventoryType::Object);
        old_item.creation_date = 100;
        let mut new_item = item(11, 1, "New thing", InventoryType::Object);
        new_item.creation_date = 200;
        model.set_items(root, &[old_item, new_item]);
        model.expanded.insert(root);

        // Name sort, no system-top: alphabetical folders, alphabetical items.
        let filter = crate::inventory_filters::ItemFilter::default();
        let spec = |sort: super::SortSpec| super::ViewSpec {
            query: "",
            tracked_attachments: &EMPTY_TRACKED,
            sort,
            filter: &filter,
            now_unix: 0,
            login_unix: 0,
        };
        let rows = model.build_rows(
            InventoryTab::Everything,
            &spec(super::SortSpec {
                by_date: false,
                system_folders_to_top: false,
            }),
        );
        assert_eq!(
            names(&rows),
            vec![
                "My Inventory",
                "Aardvark",
                "Clothing",
                "New thing",
                "Old thing"
            ]
        );
        // Date sort + system folders on top: Clothing lifts above Aardvark,
        // the newer item leads.
        let rows = model.build_rows(
            InventoryTab::Everything,
            &spec(super::SortSpec {
                by_date: true,
                system_folders_to_top: true,
            }),
        );
        assert_eq!(
            names(&rows),
            vec![
                "My Inventory",
                "Clothing",
                "Aardvark",
                "New thing",
                "Old thing"
            ]
        );
    }

    /// An active type filter narrows the tree like a search: passing items
    /// keep their expanded ancestor chain, everything else is hidden.
    #[test]
    fn type_filter_narrows_the_tree() {
        let mut model = sample_model();
        let objects = sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(3));
        model.set_items(objects, &[item(11, 3, "A box", InventoryType::Object)]);
        // Nothing expanded: the filter still finds the loaded items.
        let mut only_objects = crate::inventory_filters::TypeFilterSet::none();
        only_objects.toggle(crate::inventory_filters::TypeFilter::Object);
        let filter = crate::inventory_filters::ItemFilter {
            types: only_objects,
            ..crate::inventory_filters::ItemFilter::default()
        };
        let spec = super::ViewSpec {
            query: "",
            tracked_attachments: &EMPTY_TRACKED,
            sort: super::SortSpec {
                by_date: false,
                system_folders_to_top: false,
            },
            filter: &filter,
            now_unix: 0,
            login_unix: 0,
        };
        let rows = model.build_rows(InventoryTab::Everything, &spec);
        // The shirt (a wearable) and its Clothing branch are gone; the box
        // and its ancestors remain.
        assert_eq!(names(&rows), vec!["My Inventory", "Objects", "A box"]);
    }

    /// The permission suffixes spell out each withheld owner permission, a
    /// link is marked `(link)` instead (its permissions are the target's),
    /// and `(worn)` trails everything.
    #[test]
    fn suffixes_follow_permissions_link_and_worn() {
        // All permissions withheld: every suffix, in reference order.
        let locked = item(1, 2, "Locked", InventoryType::Object);
        assert_eq!(
            item_suffix(&locked, false),
            "(no copy) (no modify) (no transfer)"
        );
        assert_eq!(
            item_suffix(&locked, true),
            "(no copy) (no modify) (no transfer) (worn)"
        );
        // Full permissions: nothing but the worn marker.
        let mut open = item(2, 2, "Open", InventoryType::Object);
        open.permissions.owner = Permissions::COPY | Permissions::MODIFY | Permissions::TRANSFER;
        assert_eq!(item_suffix(&open, false), "");
        assert_eq!(item_suffix(&open, true), "(worn)");
        // A link shows `(link)` in place of the permission suffixes.
        let mut link = item(3, 2, "Linked", InventoryType::Wearable);
        link.asset_type = sl_client_bevy::AssetType::Other(24);
        assert_eq!(item_suffix(&link, false), "(link)");
    }

    /// A worn item's tree row carries the bold emphasis and the `(worn)`
    /// suffix; an unworn sibling does not.
    #[test]
    fn worn_rows_are_bold_and_suffixed() {
        let mut model = sample_model();
        let root = sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(1));
        let clothing = sl_client_bevy::InventoryFolderKey::from(sl_client_bevy::Uuid::from_u128(2));
        model.expanded.insert(root);
        model.expanded.insert(clothing);
        // The shirt (item 10) is tracked worn (e.g. an attachment we attached).
        let mut tracked = HashSet::new();
        tracked.insert(sl_client_bevy::InventoryKey::from(
            sl_client_bevy::Uuid::from_u128(10),
        ));
        let rows = build(&model, InventoryTab::Everything, "", &tracked);
        let shirt = rows.iter().find(|row| row.name == "Blue shirt");
        assert_eq!(shirt.map(|row| row.bold), Some(true));
        assert_eq!(
            shirt.map(|row| row.suffix.as_str()),
            Some("(no copy) (no modify) (no transfer) (worn)")
        );
        // Folders stay undecorated.
        let folder = rows.iter().find(|row| row.name == "Clothing");
        assert_eq!(folder.map(|row| row.bold), Some(false));
        assert_eq!(folder.map(|row| row.suffix.as_str()), Some(""));
    }
}
