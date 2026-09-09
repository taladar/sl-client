//! The **settings-asset list**: the projection two windows in this crate draw
//! the same rows from.
//!
//! [`my_environments`](crate::my_environments) is the library the user browses;
//! [`settings_picker`](crate::settings_picker) is the chooser another panel
//! summons for one field. They differ in their chrome and in what a pick does,
//! and not at all in what a row *is* — so the row, the filters, the folder
//! resolution and the ordering live here, and each window keeps only its own
//! table spec and its own actions.
//!
//! # What the rows come from
//!
//! [`SettingsIndex`], which is the reference's `FSSettingsCollector`: every
//! settings item in the whole inventory — agent tree *and* Library — outside the
//! Trash and Marketplace Listings, de-duplicated by **asset** id, name-ordered
//! with duplicate names intact. The index is rebuilt once per change of the
//! inventory mirror, so a window that opens costs a filter and a sort rather
//! than a walk.
//!
//! A row addresses the **item** the walk met — the *link*, when a link is what
//! it met — and carries the **asset** behind it. Those are two different ids and
//! both are load-bearing: an apply installs the asset, and a rename or a delete
//! acts on the item the user can see.

use sl_client_bevy::{InventoryKey, SettingsKind, Uuid};
use sl_viewer_inventory::inventory::InventoryModel;
use sl_viewer_inventory::settings_index::{SettingsAsset, SettingsIndex};

/// The three kinds in the reference's filter-row order (`chk_days`,
/// `chk_skies`, `chk_water`) — which is also the order [`SettingsListFilters`]
/// indexes its flags by.
pub const FILTER_KINDS: [SettingsKind; 3] = [
    SettingsKind::DayCycle,
    SettingsKind::Sky,
    SettingsKind::Water,
];

/// Which kinds and which name substring a settings list is showing.
///
/// The reference keeps the kind set as a bitmask over `LLSettingsType::type_e`
/// and hands it to the inventory panel's filter; here it is three flags and a
/// term, applied by [`matches()`]. The library window ticks all three and lets the
/// user untick; the picker fixes one ([`Self::only`]), as
/// `LLFloaterSettingsPicker::setSettingsFilter` does.
#[derive(bevy::prelude::Resource, Debug, Clone, PartialEq, Eq)]
pub struct SettingsListFilters {
    /// Whether each of [`FILTER_KINDS`] is shown.
    pub kinds: [bool; 3],
    /// The name filter, matched case-insensitively as a substring.
    pub search: String,
}

impl Default for SettingsListFilters {
    /// All three kinds, no name filter — the reference's initial `mTypeFilter`.
    fn default() -> Self {
        Self {
            kinds: [true; 3],
            search: String::new(),
        }
    }
}

impl SettingsListFilters {
    /// Only `kind`, no name filter — a picker's fixed filter.
    #[must_use]
    pub fn only(kind: SettingsKind) -> Self {
        let mut filters = Self {
            kinds: [false; 3],
            search: String::new(),
        };
        if let Some(index) = FILTER_KINDS.iter().position(|shown| *shown == kind)
            && let Some(slot) = filters.kinds.get_mut(index)
        {
            *slot = true;
        }
        filters
    }

    /// Widen the filters just enough to show an item of `kind` named `name`,
    /// returning whether the **search term** had to be dropped (so a caller can
    /// clear the widget that owns it).
    ///
    /// A creation is a promise that the thing made will be there to see. Both
    /// filters can break that promise independently — the kind's checkbox, and a
    /// name filter the new item's own name does not match — and a selection
    /// pointing at a row nothing draws is worse than no selection at all: the
    /// highlight shows nothing, and the actions that resolve the selection to a
    /// row find none.
    pub fn reveal(&mut self, kind: SettingsKind, name: &str) -> bool {
        if let Some(index) = FILTER_KINDS.iter().position(|shown| *shown == kind)
            && let Some(slot) = self.kinds.get_mut(index)
        {
            *slot = true;
        }
        let term = self.search.trim().to_lowercase();
        if term.is_empty() || name.to_lowercase().contains(&term) {
            return false;
        }
        self.search.clear();
        true
    }

    /// Whether `kind` is currently shown.
    #[must_use]
    pub fn shows(&self, kind: SettingsKind) -> bool {
        FILTER_KINDS
            .iter()
            .position(|shown| *shown == kind)
            .and_then(|index| self.kinds.get(index).copied())
            .unwrap_or(false)
    }
}

/// One row of a settings list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsListRow {
    /// The inventory item the row addresses — the *link*, when a link is what
    /// the walk met, so the row acts on the thing the user can see.
    pub item: InventoryKey,
    /// The settings asset behind it, which is what an apply installs.
    pub asset_id: Uuid,
    /// The item's name.
    pub name: String,
    /// Which of the three kinds it is.
    pub kind: SettingsKind,
    /// Whether it came from the read-only shared Library.
    pub library: bool,
    /// The name of the folder it lives in, or empty when the mirror has not
    /// loaded that folder's own record.
    pub folder: String,
}

/// Whether `entry` survives the filters — its kind ticked, and its name holding
/// the search term (case-insensitively, as the reference's filter substring is).
#[must_use]
pub fn matches(entry: &SettingsAsset, filters: &SettingsListFilters) -> bool {
    if !filters.shows(entry.kind) {
        return false;
    }
    let term = filters.search.trim().to_lowercase();
    term.is_empty() || entry.name.to_lowercase().contains(&term)
}

/// The **Where** cell: the folder's name, prefixed with the Library's label when
/// the item is one of the shared read-only ones.
///
/// The prefix is what replaces the tree's second root: a Library sky and one of
/// the user's own can carry the same folder name, and only one of them can be
/// saved over.
#[must_use]
pub fn location_text(library_label: &str, library: bool, folder: &str) -> String {
    match (library, folder.is_empty()) {
        (true, true) => library_label.to_owned(),
        (true, false) => format!("{library_label} / {folder}"),
        (false, _) => folder.to_owned(),
    }
}

/// The sort rank of a kind, so a Kind column orders sky, water, day cycle — the
/// `LLSettingsType::type_e` order, which is the order the rest of the viewer
/// lists them in.
#[must_use]
pub const fn kind_rank(kind: SettingsKind) -> u8 {
    kind.subtype()
}

/// The Fluent key naming `kind` — a Kind cell, and the label of the filter
/// checkbox that hides it, so a row and the checkbox that hides it read the
/// same.
#[must_use]
pub const fn kind_key(kind: SettingsKind) -> &'static str {
    match kind {
        SettingsKind::Sky => "my-environments-kind-sky",
        SettingsKind::Water => "my-environments-kind-water",
        SettingsKind::DayCycle => "my-environments-kind-day-cycle",
    }
}

/// A kind's element-id tail.
#[must_use]
pub const fn kind_slug(kind: SettingsKind) -> &'static str {
    match kind {
        SettingsKind::Sky => "sky",
        SettingsKind::Water => "water",
        SettingsKind::DayCycle => "day-cycle",
    }
}

/// Order `rows` by a table's sort keys (most significant first), falling back to
/// a case-insensitive name compare so the order is total and stable.
///
/// `keys` are `(column token, ascending)` pairs; a token no arm names orders by
/// the name, which is what a one-column list wants and what a token added to a
/// spec and forgotten here degrades to.
pub fn sort_rows(rows: &mut [SettingsListRow], keys: &[(&str, bool)]) {
    rows.sort_by(|left, right| {
        for (token, ascending) in keys {
            let ordering = match *token {
                "kind" => kind_rank(left.kind).cmp(&kind_rank(right.kind)),
                "where" => left.folder.to_lowercase().cmp(&right.folder.to_lowercase()),
                _name => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
            };
            let ordering = if *ascending {
                ordering
            } else {
                ordering.reverse()
            };
            if ordering != core::cmp::Ordering::Equal {
                return ordering;
            }
        }
        left.name.to_lowercase().cmp(&right.name.to_lowercase())
    });
}

/// Project the settings index onto a list, filtered but not yet sorted.
///
/// A free function over the two models rather than a system body, so the filter
/// and the folder resolution are testable without an `App`.
#[must_use]
pub fn project(
    index: &SettingsIndex,
    model: &InventoryModel,
    filters: &SettingsListFilters,
) -> Vec<SettingsListRow> {
    let mut rows = Vec::new();
    for kind in FILTER_KINDS {
        for entry in index.of_kind(kind) {
            if !matches(entry, filters) {
                continue;
            }
            let folder = model
                .find_item(entry.item)
                .and_then(|item| model.folder_info(item.folder_id))
                .map_or_else(String::new, |folder| folder.name.clone());
            rows.push(SettingsListRow {
                item: entry.item,
                asset_id: entry.asset_id,
                name: entry.name.clone(),
                kind: entry.kind,
                library: entry.library,
                folder,
            });
        }
    }
    rows
}

/// How many settings assets the index holds at all, over every kind — the
/// denominator of a list's count line.
#[must_use]
pub fn total(index: &SettingsIndex) -> usize {
    FILTER_KINDS
        .into_iter()
        .map(|kind| index.of_kind(kind).len())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::{
        FILTER_KINDS, SettingsListFilters, SettingsListRow, kind_key, kind_rank, location_text,
        matches, project, sort_rows,
    };
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        ASSET_CODE_LINK, AgentKey, AssetType, FolderInfo, FolderState, FolderType,
        InventoryFolderKey, InventoryKey, InventoryType, ItemInfo, OwnerKey, Permissions5,
        SaleInfo, SettingsKind, Uuid,
    };
    use sl_viewer_inventory::inventory::InventoryModel;
    use sl_viewer_inventory::settings_index::{SettingsAsset, build};

    /// A boxed error, so a test can `?` rather than reach for a `panic!` the
    /// workspace's lints forbid.
    type TestError = Box<dyn core::error::Error>;

    /// A folder of the skeleton.
    fn folder(id: u128, parent: Option<u128>, name: &str, folder_type: FolderType) -> FolderInfo {
        FolderInfo {
            folder_id: InventoryFolderKey::from(Uuid::from_u128(id)),
            parent_id: parent.map(|key| InventoryFolderKey::from(Uuid::from_u128(key))),
            name: name.to_owned(),
            folder_type,
            version: 1,
            state: FolderState::Loaded { version: 1 },
        }
    }

    /// A settings item whose kind is its `flags` low byte.
    fn item(id: u128, folder: u128, name: &str, asset: u128, flags: u32) -> ItemInfo {
        ItemInfo {
            item_id: InventoryKey::from(Uuid::from_u128(id)),
            folder_id: InventoryFolderKey::from(Uuid::from_u128(folder)),
            name: name.to_owned(),
            description: String::new(),
            asset_id: Uuid::from_u128(asset),
            asset_type: AssetType::Settings,
            inv_type: InventoryType::Settings,
            flags,
            sale: SaleInfo::default(),
            creation_date: 0,
            owner: OwnerKey::Agent(AgentKey::from(Uuid::from_u128(0))),
            last_owner_id: Uuid::from_u128(0),
            creator_id: AgentKey::from(Uuid::from_u128(0)),
            group: None,
            permissions: Permissions5::default(),
        }
    }

    /// A folder key.
    fn key(id: u128) -> InventoryFolderKey {
        InventoryFolderKey::from(Uuid::from_u128(id))
    }

    /// An agent tree with a Settings folder and one of the user's own, plus a
    /// Library holding an `Environments` folder.
    fn tree() -> InventoryModel {
        let mut model = InventoryModel::default();
        model.merge_folders(
            &[
                folder(1, None, "My Inventory", FolderType::RootInventory),
                folder(2, Some(1), "Settings", FolderType::Settings),
                folder(3, Some(1), "Skies I made", FolderType::None),
            ],
            false,
        );
        model.merge_folders(
            &[
                folder(100, None, "Library", FolderType::RootInventory),
                folder(101, Some(100), "Environments", FolderType::None),
            ],
            true,
        );
        model
    }

    /// A stand-in index entry, for the filter tests that need no model.
    fn asset(name: &str, kind: SettingsKind) -> SettingsAsset {
        SettingsAsset {
            item: InventoryKey::from(Uuid::from_u128(1)),
            asset_id: Uuid::from_u128(2),
            name: name.to_owned(),
            kind,
            library: false,
        }
    }

    /// **Each kind flag gates its own kind and no other.** The flags are
    /// positional, and a wrong index would hide the kind *next to* the one that
    /// was unticked — which reads as the list being broken rather than filtered.
    #[test]
    fn each_flag_hides_only_its_own_kind() {
        for (index, kind) in FILTER_KINDS.into_iter().enumerate() {
            let mut filters = SettingsListFilters::default();
            if let Some(slot) = filters.kinds.get_mut(index) {
                *slot = false;
            }
            for other in FILTER_KINDS {
                assert_eq!(
                    matches(&asset("anything", other), &filters),
                    other != kind,
                    "unticking {kind:?} changed {other:?}"
                );
            }
        }
    }

    /// A picker's fixed filter shows one kind and refuses the other two — the
    /// reference's `setSettingsFilter`, which is what keeps a water field from
    /// being handed a day cycle.
    #[test]
    fn a_fixed_filter_shows_exactly_one_kind() {
        for kind in FILTER_KINDS {
            let filters = SettingsListFilters::only(kind);
            for other in FILTER_KINDS {
                assert_eq!(filters.shows(other), other == kind);
            }
        }
    }

    /// **A creation is revealed by whatever was hiding its kind.** Clicking
    /// *New Sky* with Skies unticked would otherwise select a row nothing draws:
    /// no highlight, no name in the rename field, and the row-resolving actions
    /// finding nothing to act on.
    #[test]
    fn revealing_a_new_item_ticks_its_kind() {
        for kind in FILTER_KINDS {
            let mut filters = SettingsListFilters {
                kinds: [false; 3],
                search: String::new(),
            };
            let cleared = filters.reveal(kind, "New Thing");
            assert!(filters.shows(kind), "{kind:?} stayed hidden");
            assert!(!cleared, "no search term to clear");
            // Only the created kind is revealed; the other two stay as they were.
            for other in FILTER_KINDS {
                assert_eq!(filters.shows(other), other == kind);
            }
        }
    }

    /// **A name filter hides a new item just as effectively**, so it is dropped
    /// when — and only when — the new item's own name does not match it.
    #[test]
    fn revealing_drops_only_a_search_that_would_hide_it() {
        let mut hiding = SettingsListFilters {
            kinds: [true; 3],
            search: "sunset".to_owned(),
        };
        assert!(hiding.reveal(SettingsKind::Sky, "New Sky"));
        assert!(hiding.search.is_empty());

        // A term the new item matches is the user's filter doing its job, and
        // survives.
        let mut matching = SettingsListFilters {
            kinds: [true; 3],
            search: "new".to_owned(),
        };
        assert!(!matching.reveal(SettingsKind::Sky, "New Sky"));
        assert_eq!(matching.search, "new");
    }

    /// The name filter is a case-insensitive substring, as the reference's
    /// `setFilterSubString` is, and an all-spaces term filters nothing.
    #[test]
    fn the_name_filter_is_a_case_insensitive_substring() {
        let sky = asset("Bright Midday", SettingsKind::Sky);
        let searching = |term: &str| {
            matches(
                &sky,
                &SettingsListFilters {
                    search: term.to_owned(),
                    ..SettingsListFilters::default()
                },
            )
        };
        assert!(searching("midday"));
        assert!(searching("MIDD"));
        assert!(searching("  "));
        assert!(!searching("night"));
    }

    /// **A row says which folder it came from, and a Library row says so.**
    ///
    /// The flat list drops the tree's two roots and its folder nesting, so the
    /// Where cell is the whole of that context: without the Library prefix, a
    /// stock sky and one of the user's own in same-named folders read
    /// identically, and only one of them can be saved over.
    #[test]
    fn a_row_carries_its_folder_and_its_library_flag() -> Result<(), TestError> {
        let mut model = tree();
        model.set_items(key(3), &[item(10, 3, "Mine", 0xA1, 0)]);
        model.set_items(key(101), &[item(20, 101, "Theirs", 0xB1, 0)]);
        let index = build(&model);
        let rows = project(&index, &model, &SettingsListFilters::default());

        let mine = rows.iter().find(|row| row.name == "Mine").ok_or("no row")?;
        assert_eq!(mine.folder, "Skies I made");
        assert!(!mine.library);
        assert_eq!(
            location_text("Library", false, &mine.folder),
            "Skies I made"
        );

        let theirs = rows
            .iter()
            .find(|row| row.name == "Theirs")
            .ok_or("no row")?;
        assert!(theirs.library);
        assert_eq!(
            location_text("Library", true, &theirs.folder),
            "Library / Environments"
        );
        Ok(())
    }

    /// A Library entry whose folder the mirror has not named still says
    /// *Library* rather than showing an empty cell with no explanation.
    #[test]
    fn a_library_row_without_a_folder_still_says_library() {
        assert_eq!(location_text("Library", true, ""), "Library");
        assert_eq!(location_text("Library", false, ""), "");
    }

    /// **The row addresses the item, and carries the target's asset.** A link is
    /// what the walk met, so the row must act on the link the user can see while
    /// installing the target's asset — deleting the target instead would throw
    /// away something the user is not looking at.
    #[test]
    fn a_row_addresses_the_item_and_carries_the_target_asset() -> Result<(), TestError> {
        let mut model = tree();
        let mut link = item(30, 2, "shortcut", 20, 0);
        link.asset_type = AssetType::Other(ASSET_CODE_LINK);
        model.set_items(key(101), &[item(20, 101, "Library day", 0xB1, 2)]);
        model.set_items(key(2), &[link]);

        let index = build(&model);
        let rows = project(&index, &model, &SettingsListFilters::default());
        let row = rows.first().ok_or("no row")?;
        assert_eq!(row.item, InventoryKey::from(Uuid::from_u128(30)));
        assert_eq!(row.asset_id, Uuid::from_u128(0xB1));
        assert_eq!(row.kind, SettingsKind::DayCycle);
        // The link's own folder, which is where a delete would move it from.
        assert_eq!(row.folder, "Settings");
        Ok(())
    }

    /// The three sort columns each order by what they show, and the name is the
    /// tie-break under all of them — so the list never reshuffles rows a sort
    /// cannot tell apart.
    #[test]
    fn each_column_orders_by_what_it_shows() {
        let make = |name: &str, kind, folder: &str| SettingsListRow {
            item: InventoryKey::from(Uuid::from_u128(1)),
            asset_id: Uuid::from_u128(2),
            name: name.to_owned(),
            kind,
            library: false,
            folder: folder.to_owned(),
        };
        let mut rows = vec![
            make("beta", SettingsKind::DayCycle, "zulu"),
            make("alpha", SettingsKind::Water, "alpha"),
            make("gamma", SettingsKind::Sky, "mike"),
        ];

        sort_rows(&mut rows, &[("kind", true)]);
        let kinds: Vec<SettingsKind> = rows.iter().map(|row| row.kind).collect();
        assert_eq!(
            kinds,
            [
                SettingsKind::Sky,
                SettingsKind::Water,
                SettingsKind::DayCycle
            ]
        );

        sort_rows(&mut rows, &[("name", false)]);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["gamma", "beta", "alpha"]);

        sort_rows(&mut rows, &[("where", true)]);
        let folders: Vec<&str> = rows.iter().map(|row| row.folder.as_str()).collect();
        assert_eq!(folders, ["alpha", "mike", "zulu"]);

        // Equal keys fall back to the name, ascending.
        let mut tied = vec![
            make("second", SettingsKind::Sky, "same"),
            make("first", SettingsKind::Sky, "same"),
        ];
        sort_rows(&mut tied, &[("kind", true)]);
        let names: Vec<&str> = tied.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["first", "second"]);
    }

    /// The Kind column's order is the wire's `LLSettingsType::type_e`, which is
    /// the order every other settings surface in the viewer lists them in.
    #[test]
    fn the_kind_order_is_the_wires() {
        assert_eq!(kind_rank(SettingsKind::Sky), 0);
        assert_eq!(kind_rank(SettingsKind::Water), 1);
        assert_eq!(kind_rank(SettingsKind::DayCycle), 2);
    }

    /// Every kind has a label key, and the three are distinct — a shared key
    /// would make two kinds read the same in the Kind column and in the filter
    /// row, which is a silent failure in both.
    #[test]
    fn every_kind_has_its_own_label_key() {
        let keys: Vec<&str> = FILTER_KINDS.into_iter().map(kind_key).collect();
        let mut unique = keys.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(keys.len(), unique.len(), "{keys:?}");
    }
}
