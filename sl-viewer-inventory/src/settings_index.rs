//! Every **settings asset** the inventory mirror holds, grouped by kind and
//! addressable by name.
//!
//! Three surfaces want the same lookup and none of them can walk the tree for
//! itself: `@setenv_preset:<name>` / `@setenv_daycycle:<name>` resolve a name
//! against the Library's `Environments` folder, the quick-preferences sky /
//! water / day-cycle combos list every settings asset in inventory, and the
//! settings picker the environment panels summon lists the same thing. Walking
//! the whole tree per query would mean walking it on every combo open, so the
//! walk happens once per change of the mirror and the result is held here.
//!
//! # Two different collectors, one index
//!
//! The reference has two, and they disagree on purpose:
//!
//! - `FSSettingsCollector` (`quickprefs.cpp`) is the **list**: every settings
//!   item in the whole inventory — agent tree *and* Library — outside the Trash
//!   and outside Marketplace Listings, de-duplicated by **asset** id rather
//!   than item id, kept in name order with duplicate names intact (it is a
//!   `std::multimap`, and two skies really can share a name).
//! - `RlvIsOfSettingsType` (`rlvenvironment.cpp`) is the **name lookup**: only
//!   the Library `Environments` folder, only items that are *actually* settings
//!   (a link is not), matched case-insensitively, first match winning.
//!
//! So the list resolves links and the name lookup refuses them. That is not an
//! oversight on either side: the list is showing the user what they have, and a
//! link is a thing they have; the lookup is answering a script, and a link's own
//! flags are not the target's.
//!
//! # Links
//!
//! `LLViewerInventoryItem` resolves a link for *every* property the collectors
//! read — `getType`, `getAssetUUID`, `getFlags` and `getName` all follow the
//! link to its target. Our mirror does not: an `ItemInfo` for a link carries
//! `AssetType::Other(ASSET_CODE_LINK)`, its own flags, and an `asset_id` that is
//! the target **item**'s id. `resolve_link` closes that gap by following the
//! link in the mirror, so a link contributes its target's name, kind and asset —
//! and is then usually dropped by the de-duplication, because the target is in
//! the walk too.
//!
//! Reference (Firestorm, read-only): `indra/newview/quickprefs.cpp`
//! (`FSSettingsCollector`, `FloaterQuickPrefs::loadPresets`),
//! `indra/newview/rlvenvironment.cpp` (`rlvGetLibraryEnvironmentsFolder`,
//! `RlvIsOfSettingsType`), `indra/llinventory/llinventorysettings.cpp`
//! (`LLSettingsType::fromInventoryFlags`).

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use sl_client_bevy::{
    ASSET_CODE_LINK, AssetType, FolderType, InventoryFolderKey, InventoryKey, ItemInfo,
    SettingsKind, SlCommand, Uuid,
};

use crate::inventory::{InventoryModel, request_folder};
use crate::world_api::rlv::{RlvEnvironmentSlot, RlvLibraryEnvironments};

/// The name of the Library folder `@setenv_preset` and `@setenv_daycycle`
/// search, matched exactly and case-sensitively as `LLNameCategoryCollector`
/// does.
const LIBRARY_ENVIRONMENTS_FOLDER: &str = "Environments";

/// One settings asset, as a list of them needs it: enough to show a row and to
/// apply it, and nothing that would go stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsAsset {
    /// The inventory item this entry was found through — the *link*, when a
    /// link is what the walk met, so a UI can address the thing the user sees.
    pub item: InventoryKey,
    /// The settings asset itself, which is what an environment change is given.
    pub asset_id: Uuid,
    /// The item's name, resolved through a link the way `getName` is.
    pub name: String,
    /// Which of the three kinds it is, from the item's flag low byte.
    pub kind: SettingsKind,
    /// Whether it came from the read-only shared Library rather than the
    /// agent's own tree.
    pub library: bool,
}

/// Every settings asset in the inventory mirror, by kind, plus the Library
/// `Environments` folder indexed by name.
///
/// Rebuilt whole on every change of [`InventoryModel`] rather than maintained
/// incrementally: a removal has to drop out, and the mirror publishes no
/// per-item delta a subtraction could be driven from.
#[derive(Resource, Debug, Default, Clone, PartialEq, Eq)]
pub struct SettingsIndex {
    /// Sky assets, name-ordered, duplicate names kept.
    sky: Vec<SettingsAsset>,
    /// Water assets, name-ordered, duplicate names kept.
    water: Vec<SettingsAsset>,
    /// Day-cycle assets, name-ordered, duplicate names kept.
    day_cycle: Vec<SettingsAsset>,
    /// The Library `Environments` folder, once the skeleton names one.
    environments_folder: Option<InventoryFolderKey>,
    /// That folder's contents by kind and lower-cased name — the projection
    /// `@setenv_preset` / `@setenv_daycycle` resolve against.
    library_named: RlvLibraryEnvironments,
}

impl SettingsIndex {
    /// The assets of one kind, name-ordered.
    #[must_use]
    pub fn of_kind(&self, kind: SettingsKind) -> &[SettingsAsset] {
        match kind {
            SettingsKind::Sky => &self.sky,
            SettingsKind::Water => &self.water,
            SettingsKind::DayCycle => &self.day_cycle,
        }
    }

    /// The Library `Environments` folder, or `None` on a grid whose library has
    /// none — which is a resolution failure, never a fallback to the agent's own
    /// tree.
    #[must_use]
    pub const fn environments_folder(&self) -> Option<InventoryFolderKey> {
        self.environments_folder
    }

    /// The Library `Environments` folder by name, for the RLV commands that
    /// search it.
    #[must_use]
    pub const fn library_named(&self) -> &RlvLibraryEnvironments {
        &self.library_named
    }

    /// Whether nothing at all is indexed — every kind empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.sky.is_empty() && self.water.is_empty() && self.day_cycle.is_empty()
    }
}

/// Follow a link to the item it points at, so the caller reads the *target*'s
/// name, kind and asset — `LLViewerInventoryItem`'s own behaviour for all four
/// of the properties the collectors touch.
///
/// A non-link resolves to itself. A link whose target the mirror has not loaded
/// resolves to nothing: the reference's `getLinkedItem()` returns null there
/// too, and its collector then reads `AT_LINK` and rejects the item.
///
/// `by_id` rather than [`InventoryModel::find_item`], which is a linear scan of
/// every fetched folder: the Current Outfit Folder is a folder of nothing *but*
/// links, and resolving each of them by scan would make one rebuild quadratic in
/// the size of the inventory.
#[must_use]
fn resolve_link<'model>(
    by_id: &HashMap<InventoryKey, &'model ItemInfo>,
    item: &'model ItemInfo,
) -> Option<&'model ItemInfo> {
    if item.asset_type == AssetType::Other(ASSET_CODE_LINK) {
        by_id.get(&InventoryKey::from(item.asset_id)).copied()
    } else {
        Some(item)
    }
}

/// Rebuild the index from the mirror.
///
/// Deliberately a free function over `&InventoryModel` rather than a system
/// body: the walk is the part worth testing, and a test should not need an
/// `App`.
#[must_use]
pub fn build(model: &InventoryModel) -> SettingsIndex {
    let mut index = SettingsIndex::default();

    // The two subtrees `FSSettingsCollector` excludes. Both are agent-tree
    // folders, and `folder_by_type` never answers with a Library one, so a
    // Library folder that happens to be typed `Trash` cannot hide the library.
    let trash = model.folder_by_type(FolderType::Trash);
    let marketplace = model.folder_by_type(FolderType::MarketplaceListings);

    // Every loaded item by id, so a link costs a lookup rather than a scan. A
    // link's target may live anywhere, the Trash and Marketplace included — it
    // is the *link*'s folder that decides whether an entry is collected, and
    // the target's contents that fill it in.
    let by_id: HashMap<InventoryKey, &ItemInfo> = model
        .all_loaded_items()
        .map(|item| (item.item_id, item))
        .collect();

    let mut seen: HashSet<Uuid> = HashSet::new();
    for root in model.roots().iter().copied() {
        let library = model.is_library(root);
        for folder in model.subtree_folders(root) {
            if trash.is_some_and(|key| model.is_within(folder, key))
                || marketplace.is_some_and(|key| model.is_within(folder, key))
            {
                continue;
            }
            for item in model.loaded_items_of(folder) {
                let Some(target) = resolve_link(&by_id, item) else {
                    continue;
                };
                if target.asset_type != AssetType::Settings {
                    continue;
                }
                let Some(kind) = SettingsKind::from_item_flags(target.flags) else {
                    // The reference logs the item and drops it; a settings asset
                    // whose subtype byte names no kind is not one we can offer.
                    continue;
                };
                if !seen.insert(target.asset_id) {
                    continue;
                }
                let entry = SettingsAsset {
                    item: item.item_id,
                    asset_id: target.asset_id,
                    name: target.name.clone(),
                    kind,
                    library,
                };
                match kind {
                    SettingsKind::Sky => index.sky.push(entry),
                    SettingsKind::Water => index.water.push(entry),
                    SettingsKind::DayCycle => index.day_cycle.push(entry),
                }
            }
        }
    }

    // `std::multimap<std::string, LLUUID>`: name order, and equal names in
    // insertion order — a *stable* sort over the walk order, not a plain one.
    for list in [&mut index.sky, &mut index.water, &mut index.day_cycle] {
        list.sort_by(|left, right| left.name.cmp(&right.name));
    }

    index.environments_folder = model
        .library_root()
        .and_then(|root| model.folder_by_name_under(root, LIBRARY_ENVIRONMENTS_FOLDER));
    if let Some(environments) = index.environments_folder {
        // `RlvIsOfSettingsType` reads `getActualType()`, so a link is not a
        // settings item here however it resolves — the one place the two
        // collectors part company.
        for folder in model.subtree_folders(environments) {
            for item in model.loaded_items_of(folder) {
                if item.asset_type != AssetType::Settings {
                    continue;
                }
                if let Some(kind) = SettingsKind::from_item_flags(item.flags) {
                    index.library_named.insert(kind, &item.name, item.asset_id);
                }
            }
        }
    }

    index
}

/// Rebuild the index whenever the mirror changes.
fn rebuild_settings_index(model: Res<InventoryModel>, mut index: ResMut<SettingsIndex>) {
    if !model.is_changed() {
        return;
    }
    let rebuilt = build(&model);
    if rebuilt != *index {
        *index = rebuilt;
    }
}

/// Ask for the contents of the Library `Environments` folder and everything
/// under it.
///
/// The Library is fetched lazily — `request_all_agent_folders` deliberately
/// skips it, because a user who never opens the Library should not pay for it —
/// but a script's `@setenv_preset:<name>` cannot wait for the user to expand a
/// folder. This is the same targeted eager fetch the Current Outfit Folder gets,
/// scoped to one subtree: each folder is requested at most once, and a page
/// arriving adds its child folders to the next pass.
fn prefetch_library_environments(
    index: Res<SettingsIndex>,
    mut model: ResMut<InventoryModel>,
    mut commands: MessageWriter<SlCommand>,
) {
    if !index.is_changed() && !model.is_changed() {
        return;
    }
    let Some(environments) = index.environments_folder else {
        return;
    };
    let wanted: Vec<InventoryFolderKey> = model
        .subtree_folders(environments)
        .into_iter()
        .filter(|folder| model.needs_fetch(*folder))
        .collect();
    for folder in wanted {
        request_folder(&mut model, folder, &mut commands);
    }
}

/// Publish the library name lookup into the RLV environment slot, which is
/// where `@setenv_preset` / `@setenv_daycycle` reach it from inside the command
/// parser.
fn publish_rlv_library_environments(
    index: Res<SettingsIndex>,
    mut slot: ResMut<RlvEnvironmentSlot>,
) {
    if !index.is_changed() {
        return;
    }
    // The slot is compared for equality by the scene each frame; writing an
    // unchanged map would make every mirror change look like an environment
    // change.
    if slot.library_environments != *index.library_named() {
        slot.library_environments = index.library_named().clone();
    }
}

/// The settings index: the resource, its rebuild, the Library prefetch that
/// gives it something to index, and the RLV projection.
#[derive(Debug)]
pub struct SettingsIndexPlugin;

impl Plugin for SettingsIndexPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsIndex>().add_systems(
            Update,
            (
                rebuild_settings_index,
                prefetch_library_environments,
                publish_rlv_library_environments,
            )
                .chain(),
        );
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_client_bevy::{
        AgentKey, FolderInfo, FolderState, InventoryType, OwnerKey, Permissions5, SaleInfo,
    };

    use super::{
        ASSET_CODE_LINK, AssetType, FolderType, InventoryFolderKey, InventoryKey, InventoryModel,
        ItemInfo, SettingsKind, Uuid, build,
    };

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

    /// An item of a fetched page.
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

    /// A link item pointing at the item with id `target`.
    fn link(id: u128, folder: u128, name: &str, target: u128) -> ItemInfo {
        let mut link = item(id, folder, name, target, 0);
        link.asset_type = AssetType::Other(ASSET_CODE_LINK);
        link
    }

    /// Agent tree with Trash and Marketplace Listings, plus a Library holding an
    /// `Environments` folder.
    fn tree() -> InventoryModel {
        let mut model = InventoryModel::default();
        model.merge_folders(
            &[
                folder(1, None, "My Inventory", FolderType::RootInventory),
                folder(2, Some(1), "Settings", FolderType::Settings),
                folder(3, Some(1), "Trash", FolderType::Trash),
                folder(
                    4,
                    Some(1),
                    "Marketplace Listings",
                    FolderType::MarketplaceListings,
                ),
                folder(5, Some(4), "A listing", FolderType::Other(0)),
            ],
            false,
        );
        model.merge_folders(
            &[
                folder(100, None, "Library", FolderType::RootInventory),
                folder(101, Some(100), "Environments", FolderType::Other(0)),
                folder(102, Some(101), "Skies", FolderType::Other(0)),
            ],
            true,
        );
        model
    }

    /// Shorthand for a folder key.
    fn key(id: u128) -> InventoryFolderKey {
        InventoryFolderKey::from(Uuid::from_u128(id))
    }

    /// The names of one kind's entries.
    fn names(index: &super::SettingsIndex, kind: SettingsKind) -> Vec<&str> {
        index
            .of_kind(kind)
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    /// **The subtype byte, not the folder, decides the kind.**
    ///
    /// Three items in one folder, one of each kind — the flag low byte is the
    /// only thing telling them apart, and reading it wrong would put every
    /// settings item in the sky list (byte 0 is `ST_SKY`).
    #[test]
    fn each_kind_lands_in_its_own_list() {
        let mut model = tree();
        model.set_items(
            key(2),
            &[
                item(10, 2, "Clear sky", 0xA1, 0),
                item(11, 2, "Deep water", 0xA2, 1),
                item(12, 2, "A whole day", 0xA3, 2),
                // A byte no kind claims: dropped, not filed under sky.
                item(13, 2, "Nonsense", 0xA4, 9),
            ],
        );
        let index = build(&model);
        assert_eq!(names(&index, SettingsKind::Sky), ["Clear sky"]);
        assert_eq!(names(&index, SettingsKind::Water), ["Deep water"]);
        assert_eq!(names(&index, SettingsKind::DayCycle), ["A whole day"]);
    }

    /// **The Trash and Marketplace Listings are excluded, at any depth.**
    #[test]
    fn trash_and_marketplace_subtrees_are_excluded() {
        let mut model = tree();
        model.set_items(key(2), &[item(10, 2, "Kept", 0xA1, 0)]);
        model.set_items(key(3), &[item(11, 3, "Thrown away", 0xA2, 0)]);
        // Nested one level below Marketplace Listings, not directly in it.
        model.set_items(key(5), &[item(12, 5, "For sale", 0xA3, 0)]);
        let index = build(&model);
        assert_eq!(names(&index, SettingsKind::Sky), ["Kept"]);
    }

    /// **De-duplication is by asset id, not item id.**
    ///
    /// Two separate items of the same asset — a copy, the ordinary way an
    /// inventory ends up with two — are one entry, and the first met wins.
    #[test]
    fn two_items_of_one_asset_are_one_entry() {
        let mut model = tree();
        model.set_items(
            key(2),
            &[
                item(10, 2, "A copy", 0xA1, 0),
                item(11, 2, "Another copy", 0xA1, 0),
                item(12, 2, "A different sky", 0xA2, 0),
            ],
        );
        let index = build(&model);
        assert_eq!(
            names(&index, SettingsKind::Sky),
            ["A copy", "A different sky"]
        );
    }

    /// **Two assets that share a name are both kept.**
    ///
    /// The reference's `std::multimap` keeps them; a map keyed by name would
    /// silently lose one, and two skies really can be called the same thing.
    #[test]
    fn duplicate_names_of_distinct_assets_are_both_kept() {
        let mut model = tree();
        model.set_items(
            key(2),
            &[
                item(10, 2, "Sunrise", 0xA1, 0),
                item(11, 2, "Sunrise", 0xA2, 0),
            ],
        );
        let index = build(&model);
        assert_eq!(names(&index, SettingsKind::Sky), ["Sunrise", "Sunrise"]);
        let assets: Vec<Uuid> = index
            .of_kind(SettingsKind::Sky)
            .iter()
            .map(|entry| entry.asset_id)
            .collect();
        assert_eq!(assets, [Uuid::from_u128(0xA1), Uuid::from_u128(0xA2)]);
    }

    /// **A link reads as its target — name, kind and asset all followed.**
    ///
    /// The link's own name and flags are wrong for all three
    /// (`LLViewerInventoryItem` overrides `getName`, `getFlags`, `getType` and
    /// `getAssetUUID` to follow the link), and its `asset_id` is an item id.
    #[test]
    fn a_link_contributes_its_targets_name_kind_and_asset() {
        let mut model = tree();
        model.set_items(key(102), &[item(20, 102, "Library day", 0xB1, 2)]);
        model.set_items(key(2), &[link(10, 2, "shortcut", 20)]);
        let index = build(&model);
        // Filed by the *target*'s kind (day cycle), under the target's name.
        assert_eq!(names(&index, SettingsKind::DayCycle), ["Library day"]);
        assert!(names(&index, SettingsKind::Sky).is_empty());
        let entry = index.of_kind(SettingsKind::DayCycle).first();
        assert_eq!(
            entry.map(|found| (found.asset_id, found.item)),
            // The asset is the target's; the row addresses the link the user
            // can see, not the target.
            Some((
                Uuid::from_u128(0xB1),
                InventoryKey::from(Uuid::from_u128(10))
            ))
        );
    }

    /// **A link whose target is not loaded contributes nothing.**
    #[test]
    fn a_dangling_link_is_dropped() {
        let mut model = tree();
        model.set_items(key(2), &[link(10, 2, "shortcut", 999)]);
        assert!(build(&model).is_empty());
    }

    /// **A link to something that is not settings contributes nothing** — which
    /// is every link the Current Outfit Folder holds, the folder that makes
    /// link resolution worth doing efficiently at all.
    #[test]
    fn a_link_to_a_non_settings_item_is_dropped() {
        let mut model = tree();
        let mut shirt = item(20, 2, "Blue shirt", 0xB1, 0);
        shirt.asset_type = AssetType::Clothing;
        shirt.inv_type = InventoryType::Wearable;
        model.set_items(key(2), &[shirt, link(10, 2, "Blue shirt", 20)]);
        assert!(build(&model).is_empty());
    }

    /// **The link's own folder decides whether it is collected, and the
    /// target's contents fill the entry in** — so a link outside the Trash to a
    /// settings item inside it does surface that item.
    ///
    /// The reference lands here too: `EXCLUDE_TRASH` is a flag on the
    /// *traversal*, which never reaches the target, and the collector's own
    /// Marketplace test is against the link item's uuid.
    #[test]
    fn a_link_reaches_a_target_the_walk_itself_excludes() {
        let mut model = tree();
        model.set_items(key(3), &[item(20, 3, "Thrown away", 0xB1, 0)]);
        model.set_items(key(2), &[link(10, 2, "shortcut", 20)]);
        assert_eq!(names(&build(&model), SettingsKind::Sky), ["Thrown away"]);
    }

    /// **The Library `Environments` folder is found by name, under the library
    /// root only.**
    ///
    /// A same-named folder in the agent's own tree must not answer: that is the
    /// scope `rlvGetLibraryEnvironmentsFolder` searches, and the whole point of
    /// searching it is that the user cannot forge one.
    #[test]
    fn the_environments_folder_belongs_to_the_library() {
        let mut model = tree();
        model.merge_folders(
            &[folder(6, Some(1), "Environments", FolderType::Other(0))],
            false,
        );
        assert_eq!(build(&model).environments_folder(), Some(key(101)));
    }

    /// **The name lookup is case-insensitive, kind-exact, and library-scoped.**
    #[test]
    fn the_library_name_lookup_matches_the_reference_rules() {
        let mut model = tree();
        model.set_items(
            key(102),
            &[
                item(20, 102, "Sunrise", 0xB1, 0),
                item(21, 102, "Long Day", 0xB2, 2),
            ],
        );
        // Same name, same kind, in the agent's own tree: not searched.
        model.set_items(key(2), &[item(10, 2, "Sunrise", 0xC1, 0)]);
        let index = build(&model);
        let named = index.library_named();
        assert_eq!(
            named.resolve(SettingsKind::Sky, "sUnRiSe"),
            Some(Uuid::from_u128(0xB1))
        );
        // `@setenv_preset` searches ST_SKY only, so a day cycle of that name is
        // not an answer to it.
        assert_eq!(named.resolve(SettingsKind::DayCycle, "Sunrise"), None);
        assert_eq!(
            named.resolve(SettingsKind::DayCycle, "long day"),
            Some(Uuid::from_u128(0xB2))
        );
        assert_eq!(named.resolve(SettingsKind::Sky, "Nothing"), None);
    }

    /// **The name lookup refuses a link, where the list resolves one.**
    ///
    /// `RlvIsOfSettingsType` reads `getActualType()` and `FSSettingsCollector`
    /// reads `getType()`; the two collectors really do disagree here, and a
    /// script that could reach a preset through a link the user placed in the
    /// Library would be reaching past the scope the reference gives it.
    #[test]
    fn the_library_name_lookup_ignores_a_link_the_list_resolves() {
        let mut model = tree();
        model.set_items(key(2), &[item(10, 2, "Owned sky", 0xC1, 0)]);
        model.set_items(key(102), &[link(20, 102, "Owned sky", 10)]);
        let index = build(&model);
        // The list met the agent's copy first, so the link de-duplicated away.
        assert_eq!(names(&index, SettingsKind::Sky), ["Owned sky"]);
        assert_eq!(
            index
                .library_named()
                .resolve(SettingsKind::Sky, "Owned sky"),
            None
        );
    }

    /// **A repeated library name keeps the first met**, as `items.front()` does.
    #[test]
    fn a_repeated_library_name_keeps_the_first() {
        let mut model = tree();
        model.set_items(key(101), &[item(20, 101, "Sunrise", 0xB1, 0)]);
        model.set_items(key(102), &[item(21, 102, "Sunrise", 0xB2, 0)]);
        assert_eq!(
            build(&model)
                .library_named()
                .resolve(SettingsKind::Sky, "Sunrise"),
            Some(Uuid::from_u128(0xB1))
        );
    }

    /// **A library with no `Environments` folder resolves nothing** — it never
    /// falls back to the agent's tree.
    #[test]
    fn no_environments_folder_resolves_nothing() {
        let mut model = InventoryModel::default();
        model.merge_folders(
            &[folder(1, None, "My Inventory", FolderType::RootInventory)],
            false,
        );
        model.merge_folders(
            &[folder(100, None, "Library", FolderType::RootInventory)],
            true,
        );
        model.set_items(key(1), &[item(10, 1, "Sunrise", 0xA1, 0)]);
        let index = build(&model);
        assert_eq!(index.environments_folder(), None);
        assert!(index.library_named().is_empty());
        // …while the *list* still holds the agent's own item.
        assert_eq!(names(&index, SettingsKind::Sky), ["Sunrise"]);
    }

    /// **A library entry is marked as one**, so a picker can say where a preset
    /// came from and a save path can refuse to write over it.
    #[test]
    fn library_entries_are_flagged() {
        let mut model = tree();
        model.set_items(key(2), &[item(10, 2, "Mine", 0xA1, 0)]);
        model.set_items(key(102), &[item(20, 102, "Theirs", 0xB1, 0)]);
        let index = build(&model);
        let flags: Vec<(&str, bool)> = index
            .of_kind(SettingsKind::Sky)
            .iter()
            .map(|entry| (entry.name.as_str(), entry.library))
            .collect();
        assert_eq!(flags, [("Mine", false), ("Theirs", true)]);
    }

    /// A settings item under a folder no page has arrived for is simply not
    /// there — the index reports what is loaded, and says so by being empty
    /// rather than by guessing.
    #[test]
    fn an_unfetched_folder_contributes_nothing() {
        let model = tree();
        assert!(build(&model).is_empty());
    }
}
