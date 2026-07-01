//! Inventory structure and region-handle coordinate helpers.

use sl_types::key::{
    AgentKey, GroupKey, InventoryFolderKey, InventoryItemOrFolderKey, InventoryKey, OwnerKey,
};
use sl_types::money::LindenAmount;
use sl_wire::{Permissions5, RegionHandle};
use uuid::Uuid;

use crate::{AssetType, FolderState, InventoryType, SaleType, WearableType};

/// An inventory folder (category): from the login skeleton
/// ([`Event::InventorySkeleton`](crate::Event::InventorySkeleton)) or an `InventoryDescendents` sub-folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryFolder {
    /// The folder's id.
    pub folder_id: InventoryFolderKey,
    /// The parent folder's id (`None` for the root).
    pub parent_id: Option<InventoryFolderKey>,
    /// The folder name.
    pub name: String,
    /// The folder's default asset/folder type (`FolderType`; `-1` for none).
    pub folder_type: i8,
    /// The folder version, or `0` when not provided (sub-folders of a descendents
    /// reply do not carry their own version).
    pub version: i32,
}

/// An inventory item, from an `InventoryDescendents` item entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItem {
    /// The item's id.
    pub item_id: InventoryKey,
    /// The containing folder's id.
    pub folder_id: InventoryFolderKey,
    /// The item name.
    pub name: String,
    /// The item description.
    pub description: String,
    /// The underlying asset id.
    pub asset_id: Uuid,
    /// The asset type (`AssetType`).
    pub item_type: i8,
    /// The inventory type (`InventoryType`).
    pub inv_type: i8,
    /// The item flags bitfield.
    pub flags: u32,
    /// The sale type (not for sale / original / copy / contents).
    pub sale_type: u8,
    /// The asking price in L$ when the item is for sale, or `None` when it is
    /// not (`sale_type == SALE_TYPE_NOT`). A for-sale item may still be free
    /// (`Some(LindenAmount(0))`).
    pub sale_price: Option<LindenAmount>,
    /// The creation date (Unix seconds).
    pub creation_date: i32,
    /// The current owner — an agent, or a group when the item is group-owned
    /// (signalled on the wire by the `GroupOwned` flag, with the group carried in
    /// `GroupID`).
    pub owner: OwnerKey,
    /// The previous owner's id. Only the CAPS/AIS inventory path carries this;
    /// it is nil for items fetched over the legacy UDP path (and is part of the
    /// item's permissions checksum, the `CRC` of `UpdateInventoryItem`).
    pub last_owner_id: Uuid,
    /// The creator's id.
    pub creator_id: AgentKey,
    /// The group the item is set to, or `None` when no group is set (a
    /// group-*owned* item reports its group via [`owner`](Self::owner)).
    pub group: Option<GroupKey>,
    /// The base / owner / group / everyone / next-owner permission masks.
    pub permissions: Permissions5,
}

/// Parameters for creating a new inventory item via
/// [`Session::create_inventory_item`](crate::Session::create_inventory_item)
/// (`CreateInventoryItem`). The simulator allocates the item's id and replies
/// with an [`Event::InventoryItemCreated`](crate::Event::InventoryItemCreated).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInventoryItem {
    /// The folder the new item is created in.
    pub folder_id: InventoryFolderKey,
    /// The transaction id associating a freshly uploaded asset with the item
    /// (nil for an item with no backing asset, e.g. a fresh notecard the sim
    /// fills in).
    pub transaction_id: Uuid,
    /// The next-owner permissions mask for the new item.
    pub next_owner_mask: u32,
    /// The asset class of the new item ([`AssetType`]).
    pub asset_type: AssetType,
    /// The inventory class of the new item ([`InventoryType`]).
    pub inv_type: InventoryType,
    /// The wearable slot ([`WearableType`]) — only meaningful for
    /// clothing/body-part items (use [`WearableType::Shape`] otherwise).
    pub wearable_type: WearableType,
    /// The item name.
    pub name: String,
    /// The item description.
    pub description: String,
}

impl Default for NewInventoryItem {
    /// All-nil/zero defaults; [`folder_id`](Self::folder_id) is the nil inventory
    /// folder key ([`InventoryFolderKey`] is not itself [`Default`]) and the typed
    /// classes default to their `0`-code variants ([`AssetType::Texture`] /
    /// [`InventoryType::Texture`] / [`WearableType::Shape`]).
    fn default() -> Self {
        Self {
            folder_id: InventoryFolderKey::from(Uuid::nil()),
            transaction_id: Uuid::nil(),
            next_owner_mask: 0,
            asset_type: AssetType::Texture,
            inv_type: InventoryType::Texture,
            wearable_type: WearableType::Shape,
            name: String::new(),
            description: String::new(),
        }
    }
}

/// Parameters for creating an inventory **link** via
/// [`Session::link_inventory_item`](crate::Session::link_inventory_item)
/// (`LinkInventoryItem`). A link is a lightweight pointer to an existing item or
/// folder ([`linked_id`](Self::linked_id)) filed in
/// [`folder_id`](Self::folder_id); removing the link leaves its target intact.
/// The simulator allocates the link item's id and echoes the request's async
/// callback id in its
/// [`Event::InventoryItemCreated`](crate::Event::InventoryItemCreated) reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewInventoryLink {
    /// The folder the new link is created in.
    pub folder_id: InventoryFolderKey,
    /// The item or folder the link points to (the wire `OldItemID`).
    pub linked_id: InventoryItemOrFolderKey,
    /// The link asset class ([`AssetType`]): [`AssetType::Other(24)`](AssetType::Other)
    /// (`AT_LINK`) for an item link, [`AssetType::Other(25)`](AssetType::Other)
    /// (`AT_LINK_FOLDER`) for a folder link.
    pub link_type: AssetType,
    /// The inventory class ([`InventoryType`]) — the viewer mirrors the linked
    /// object's type ([`InventoryType::Category`] for a folder link).
    pub inv_type: InventoryType,
    /// The link's name (the viewer copies the linked object's name).
    pub name: String,
    /// The link's description.
    pub description: String,
}

/// A single item relocation from a `MoveInventoryItem`: the simulator tells the
/// client to re-parent `item` into `folder`, optionally renaming it. A client
/// mirroring inventory should move (and rename) the item locally to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItemMove {
    /// The item being moved.
    pub item: InventoryKey,
    /// The destination folder the item now lives in.
    pub folder: InventoryFolderKey,
    /// The item's new name, or `None` when the move does not rename it (the wire
    /// `NewName` field was empty).
    pub new_name: Option<String>,
}

/// A gesture to activate via
/// [`Session::activate_gestures`](crate::Session::activate_gestures)
/// (`ActivateGestures`), pairing the gesture's inventory item id with the asset
/// id of its gesture asset. Deactivation
/// ([`Session::deactivate_gestures`](crate::Session::deactivate_gestures)) only
/// needs the item id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GestureActivation {
    /// The gesture's inventory item id.
    pub item_id: InventoryKey,
    /// The gesture asset id backing that item.
    pub asset_id: Uuid,
}

/// The Second Life inventory **folder** preferred type (`LLFolderType::EType` /
/// `FT_*`), the system role a folder plays — e.g. the Trash, Lost-And-Found, the
/// Current Outfit, the Marketplace listings. Distinct from [`AssetType`]: the
/// two number spaces overlap but disagree (`AT_CATEGORY` is `8` while
/// `FT_ROOT_INVENTORY` is also `8`), so a folder's type must be resolved through
/// this enum, never [`AssetType`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FolderType {
    /// No preferred type (`FT_NONE`, wire `-1`) — an ordinary user folder.
    None,
    /// Default textures folder (`FT_TEXTURE`).
    Texture,
    /// Default sounds folder (`FT_SOUND`).
    Sound,
    /// Calling cards folder (`FT_CALLINGCARD`).
    CallingCard,
    /// Landmarks folder (`FT_LANDMARK`).
    Landmark,
    /// Clothing folder (`FT_CLOTHING`).
    Clothing,
    /// Objects folder (`FT_OBJECT`).
    Object,
    /// Notecards folder (`FT_NOTECARD`).
    Notecard,
    /// The inventory root ("My Inventory") (`FT_ROOT_INVENTORY`, wire `8` —
    /// **not** [`AssetType::Folder`], which shares that code).
    RootInventory,
    /// Scripts folder (`FT_LSL_TEXT`).
    ScriptText,
    /// Body parts folder (`FT_BODYPART`).
    Bodypart,
    /// The Trash (`FT_TRASH`).
    Trash,
    /// The Photo Album / snapshots folder (`FT_SNAPSHOT_CATEGORY`).
    SnapshotCategory,
    /// The Lost And Found (`FT_LOST_AND_FOUND`).
    LostAndFound,
    /// Animations folder (`FT_ANIMATION`).
    Animation,
    /// Gestures folder (`FT_GESTURE`).
    Gesture,
    /// The Favorites bar folder (`FT_FAVORITE`).
    Favorite,
    /// The Current Outfit folder (`FT_CURRENT_OUTFIT`).
    CurrentOutfit,
    /// An outfit folder (`FT_OUTFIT`).
    Outfit,
    /// The My Outfits folder (`FT_MY_OUTFITS`).
    MyOutfits,
    /// The default meshes folder (`FT_MESH`).
    Mesh,
    /// The Received Items / marketplace inbox (`FT_INBOX`).
    Inbox,
    /// The marketplace outbox (`FT_OUTBOX`).
    Outbox,
    /// The basic root for a sub-account (`FT_BASIC_ROOT`).
    BasicRoot,
    /// The Marketplace Listings folder (`FT_MARKETPLACE_LISTINGS`).
    MarketplaceListings,
    /// A marketplace stock folder (`FT_MARKETPLACE_STOCK`).
    MarketplaceStock,
    /// A marketplace version folder (`FT_MARKETPLACE_VERSION`).
    MarketplaceVersion,
    /// The Settings folder (`FT_SETTINGS`).
    Settings,
    /// The Materials folder (`FT_MATERIAL`).
    Material,
    /// Any other / unrecognised folder type, carrying the raw `FT_*` code.
    Other(i8),
}

impl FolderType {
    /// The `LLFolderType::EType` wire byte for this folder type.
    #[must_use]
    pub const fn to_code(self) -> i8 {
        match self {
            Self::None => -1,
            Self::Texture => 0,
            Self::Sound => 1,
            Self::CallingCard => 2,
            Self::Landmark => 3,
            Self::Clothing => 5,
            Self::Object => 6,
            Self::Notecard => 7,
            Self::RootInventory => 8,
            Self::ScriptText => 10,
            Self::Bodypart => 13,
            Self::Trash => 14,
            Self::SnapshotCategory => 15,
            Self::LostAndFound => 16,
            Self::Animation => 20,
            Self::Gesture => 21,
            Self::Favorite => 23,
            Self::CurrentOutfit => 46,
            Self::Outfit => 47,
            Self::MyOutfits => 48,
            Self::Mesh => 49,
            Self::Inbox => 50,
            Self::Outbox => 51,
            Self::BasicRoot => 52,
            Self::MarketplaceListings => 53,
            Self::MarketplaceStock => 54,
            Self::MarketplaceVersion => 55,
            Self::Settings => 56,
            Self::Material => 57,
            Self::Other(code) => code,
        }
    }

    /// Classifies an `LLFolderType::EType` wire byte (unknown codes — other than
    /// `-1`, which is [`None`](Self::None) — become [`Other`](Self::Other)).
    #[must_use]
    pub const fn from_code(code: i8) -> Self {
        match code {
            -1 => Self::None,
            0 => Self::Texture,
            1 => Self::Sound,
            2 => Self::CallingCard,
            3 => Self::Landmark,
            5 => Self::Clothing,
            6 => Self::Object,
            7 => Self::Notecard,
            8 => Self::RootInventory,
            10 => Self::ScriptText,
            13 => Self::Bodypart,
            14 => Self::Trash,
            15 => Self::SnapshotCategory,
            16 => Self::LostAndFound,
            20 => Self::Animation,
            21 => Self::Gesture,
            23 => Self::Favorite,
            46 => Self::CurrentOutfit,
            47 => Self::Outfit,
            48 => Self::MyOutfits,
            49 => Self::Mesh,
            50 => Self::Inbox,
            51 => Self::Outbox,
            52 => Self::BasicRoot,
            53 => Self::MarketplaceListings,
            54 => Self::MarketplaceStock,
            55 => Self::MarketplaceVersion,
            56 => Self::Settings,
            57 => Self::Material,
            other => Self::Other(other),
        }
    }
}

/// One immediate child of an inventory folder, borrowed from the held model: a
/// sub-folder or an item. Yielded by
/// [`Session::inventory_children`](crate::Session::inventory_children) for a
/// zero-copy tree walk (the borrowed counterpart of the owning
/// [`FolderInfo`] / [`ItemInfo`] snapshots returned by
/// [`Session::inventory_folder_page`](crate::Session::inventory_folder_page)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Child<'a> {
    /// A sub-folder filed directly under the parent.
    Folder(&'a InventoryFolder),
    /// An item filed directly under the parent.
    Item(&'a InventoryItem),
}

/// An owning snapshot of an inventory folder for the paginated read API
/// ([`Session::inventory_folder_page`](crate::Session::inventory_folder_page))
/// and the [`Command`](crate::Command)/[`Event`](crate::Event) pull-bridge:
/// typed keys and a resolved [`FolderType`] / [`FolderState`] instead of the raw
/// `i8` / loose fields of [`InventoryFolder`]. Cheap to clone and `Arc`-share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInfo {
    /// The folder's id.
    pub folder_id: InventoryFolderKey,
    /// The parent folder's id (`None` for a root).
    pub parent_id: Option<InventoryFolderKey>,
    /// The folder name.
    pub name: String,
    /// The folder's preferred type, resolved from the raw wire byte.
    pub folder_type: FolderType,
    /// The folder version.
    pub version: i32,
    /// The fetch state of this folder's contents.
    pub state: FolderState,
}

impl FolderInfo {
    /// Builds a snapshot from a held [`InventoryFolder`] and its tracked
    /// [`FolderState`].
    pub(crate) fn from_folder(folder: &InventoryFolder, state: FolderState) -> Self {
        Self {
            folder_id: folder.folder_id,
            parent_id: folder.parent_id,
            name: folder.name.clone(),
            folder_type: FolderType::from_code(folder.folder_type),
            version: folder.version,
            state,
        }
    }
}

/// An owning snapshot of an inventory item for the paginated read API
/// ([`Session::inventory_folder_page`](crate::Session::inventory_folder_page))
/// and the [`Command`](crate::Command)/[`Event`](crate::Event) pull-bridge:
/// typed keys and resolved [`AssetType`] / [`InventoryType`] / [`SaleType`]
/// enums instead of the raw `i8` / `u8` of [`InventoryItem`]. Cheap to clone and
/// `Arc`-share.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemInfo {
    /// The item's id.
    pub item_id: InventoryKey,
    /// The containing folder's id.
    pub folder_id: InventoryFolderKey,
    /// The item name.
    pub name: String,
    /// The item description.
    pub description: String,
    /// The underlying asset id.
    pub asset_id: Uuid,
    /// The asset class, resolved from the raw wire byte.
    pub asset_type: AssetType,
    /// The inventory class, resolved from the raw wire byte.
    pub inv_type: InventoryType,
    /// The item flags bitfield.
    pub flags: u32,
    /// The sale type paired with the asking price in L$ when the item is for
    /// sale, or `None` when it is not. A for-sale item may still be free
    /// (`Some((_, LindenAmount(0)))`).
    pub sale: Option<(SaleType, LindenAmount)>,
    /// The creation date (Unix seconds).
    pub creation_date: i32,
    /// The current owner (an agent, or a group for a group-owned item).
    pub owner: OwnerKey,
    /// The previous owner's id (nil for items fetched over the legacy UDP path).
    pub last_owner_id: Uuid,
    /// The creator's id.
    pub creator_id: AgentKey,
    /// The group the item is set to, or `None` when no group is set.
    pub group: Option<GroupKey>,
    /// The base / owner / group / everyone / next-owner permission masks.
    pub permissions: Permissions5,
}

impl ItemInfo {
    /// Builds a snapshot from a held [`InventoryItem`], resolving its raw type
    /// bytes and pairing the sale type with the price (`None` when not for sale).
    pub(crate) fn from_item(item: &InventoryItem) -> Self {
        Self {
            item_id: item.item_id,
            folder_id: item.folder_id,
            name: item.name.clone(),
            description: item.description.clone(),
            asset_id: item.asset_id,
            asset_type: AssetType::from_code(i32::from(item.item_type)),
            inv_type: InventoryType::from_code(i32::from(item.inv_type)),
            flags: item.flags,
            sale: item
                .sale_price
                .clone()
                .map(|price| (SaleType::from_code(item.sale_type), price)),
            creation_date: item.creation_date,
            owner: item.owner,
            last_owner_id: item.last_owner_id,
            creator_id: item.creator_id,
            group: item.group,
            permissions: item.permissions,
        }
    }
}

/// An opaque page token for
/// [`Session::inventory_folder_page`](crate::Session::inventory_folder_page) — a
/// cursor returned by one page is fed back as the `before` argument of the next
/// to walk the rest of a large folder's combined child sequence (its sub-folders
/// first, then its items, in parent→children-index order). The consumed count is
/// exposed (rather than fully opaque) so the
/// [`Command`](crate::Command)/[`Event`](crate::Event) pull-bridge can carry it
/// across the channel boundary; ordinary in-memory consumers need not interpret
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InventoryCursor(usize);

impl InventoryCursor {
    /// Wraps a "children already consumed from the start" count as a cursor.
    pub(crate) const fn new(consumed: usize) -> Self {
        Self(consumed)
    }

    /// The number of children (from the start of the combined sequence) this
    /// cursor skips past.
    pub(crate) const fn consumed(self) -> usize {
        self.0
    }

    /// Builds a cursor from a children-consumed count — the constructor the
    /// pull-bridge uses to resume paging across the channel boundary.
    #[must_use]
    pub const fn from_consumed(consumed: usize) -> Self {
        Self(consumed)
    }

    /// This cursor's children-consumed count, for the pull-bridge to round-trip.
    #[must_use]
    pub const fn consumed_count(self) -> usize {
        self.0
    }
}

/// Splits a region handle into its global south-west corner in metres,
/// `(global_x, global_y)`. Thin `u64` wrapper around
/// [`RegionHandle::global_coordinates`] for raw-wire contexts.
#[must_use]
pub fn handle_to_global(handle: u64) -> (u32, u32) {
    RegionHandle(handle).global_coordinates()
}

/// Splits a region handle into its grid coordinates (region indices), i.e. the
/// global south-west corner in metres divided by 256. Thin `u64` wrapper around
/// [`RegionHandle::grid_coordinates`] for raw-wire contexts.
#[must_use]
pub fn handle_to_grid(handle: u64) -> (u32, u32) {
    RegionHandle(handle).grid_coordinates()
}

/// Builds a region handle from its global south-west corner in metres,
/// `(global_x, global_y)` — the inverse of [`handle_to_global`]. Unlike
/// [`grid_to_handle`], the inputs are already in metres (not region indices),
/// e.g. the `region_x` / `region_y` fields of the login response. Thin `u64`
/// wrapper around [`RegionHandle::from_global`].
#[must_use]
pub fn global_to_handle(global_x: u32, global_y: u32) -> u64 {
    RegionHandle::from_global(global_x, global_y).0
}

/// Builds a region handle from grid coordinates (region indices). Thin `u64`
/// wrapper around [`RegionHandle::from_grid`].
#[must_use]
pub fn grid_to_handle(grid_x: u32, grid_y: u32) -> u64 {
    RegionHandle::from_grid(grid_x, grid_y).0
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{InventoryFolder, InventoryFolderKey, InventoryKey};

    /// [`InventoryKey`] and [`InventoryFolderKey`] are transparent wrappers over
    /// their [`Uuid`]: wrapping a raw id and unwrapping it again yields the
    /// identical bytes, so the on-wire representation is unchanged by the
    /// newtypes — and an item key and a folder key built from the same raw id are
    /// distinct types (the mix-up guard) yet carry the same underlying uuid.
    #[test]
    fn inventory_keys_round_trip_uuid_bit_identically() {
        for raw in [
            Uuid::nil(),
            Uuid::from_u128(1),
            Uuid::from_u128(0xdead_beef_dead_beef_dead_beef_dead_beef),
        ] {
            assert_eq!(InventoryKey::from(raw).uuid(), raw);
            assert_eq!(InventoryFolderKey::from(raw).uuid(), raw);
            // Same raw id, distinct typed views, same uuid underneath.
            assert_eq!(
                InventoryKey::from(raw).uuid(),
                InventoryFolderKey::from(raw).uuid()
            );
        }
    }

    /// The folder/parent ids on an [`InventoryFolder`] survive a wrap/unwrap
    /// round trip unchanged — the typed keys hold exactly the wire uuids, and the
    /// nil-parent root convention is preserved.
    #[test]
    fn inventory_folder_ids_survive_round_trip() {
        let folder_raw = Uuid::from_u128(0xb22);

        let folder = InventoryFolder {
            folder_id: InventoryFolderKey::from(folder_raw),
            parent_id: None,
            name: "Objects".to_owned(),
            folder_type: -1,
            version: 1,
        };
        assert_eq!(folder.folder_id.uuid(), folder_raw);
        assert!(folder.parent_id.is_none());
    }
}
