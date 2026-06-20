//! Inventory structure and region-handle coordinate helpers.

use uuid::Uuid;

/// An inventory folder (category): from the login skeleton
/// ([`Event::InventorySkeleton`](crate::Event::InventorySkeleton)) or an `InventoryDescendents` sub-folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryFolder {
    /// The folder's id.
    pub folder_id: Uuid,
    /// The parent folder's id (nil for the root).
    pub parent_id: Uuid,
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
    pub item_id: Uuid,
    /// The containing folder's id.
    pub folder_id: Uuid,
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
    /// The sale price, in L$.
    pub sale_price: i32,
    /// The creation date (Unix seconds).
    pub creation_date: i32,
    /// The current owner's id.
    pub owner_id: Uuid,
    /// The previous owner's id. Only the CAPS/AIS inventory path carries this;
    /// it is nil for items fetched over the legacy UDP path (and is part of the
    /// item's permissions checksum, the `CRC` of `UpdateInventoryItem`).
    pub last_owner_id: Uuid,
    /// The creator's id.
    pub creator_id: Uuid,
    /// The group associated with the item.
    pub group_id: Uuid,
    /// Whether the item is group-owned.
    pub group_owned: bool,
    /// The base permissions mask.
    pub base_mask: u32,
    /// The owner permissions mask.
    pub owner_mask: u32,
    /// The group permissions mask.
    pub group_mask: u32,
    /// The everyone permissions mask.
    pub everyone_mask: u32,
    /// The next-owner permissions mask.
    pub next_owner_mask: u32,
}

/// Parameters for creating a new inventory item via
/// [`Session::create_inventory_item`](crate::Session::create_inventory_item)
/// (`CreateInventoryItem`). The simulator allocates the item's id and replies
/// with an [`Event::InventoryItemCreated`](crate::Event::InventoryItemCreated).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NewInventoryItem {
    /// The folder the new item is created in.
    pub folder_id: Uuid,
    /// The transaction id associating a freshly uploaded asset with the item
    /// (nil for an item with no backing asset, e.g. a fresh notecard the sim
    /// fills in).
    pub transaction_id: Uuid,
    /// The next-owner permissions mask for the new item.
    pub next_owner_mask: u32,
    /// The asset type (`AssetType`).
    pub asset_type: i8,
    /// The inventory type (`InventoryType`).
    pub inv_type: i8,
    /// The wearable type (only meaningful for clothing/body-part items).
    pub wearable_type: u8,
    /// The item name.
    pub name: String,
    /// The item description.
    pub description: String,
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
    pub item_id: Uuid,
    /// The gesture asset id backing that item.
    pub asset_id: Uuid,
}

/// Splits a region handle into its global south-west corner in metres,
/// `(global_x, global_y)`.
#[must_use]
pub fn handle_to_global(handle: u64) -> (u32, u32) {
    let high = handle.checked_shr(32).unwrap_or(0);
    let low = handle & 0xFFFF_FFFF;
    (
        u32::try_from(high).unwrap_or(u32::MAX),
        u32::try_from(low).unwrap_or(u32::MAX),
    )
}

/// Splits a region handle into its grid coordinates (region indices), i.e. the
/// global south-west corner in metres divided by 256.
#[must_use]
pub fn handle_to_grid(handle: u64) -> (u32, u32) {
    let (global_x, global_y) = handle_to_global(handle);
    (
        global_x.checked_div(256).unwrap_or(0),
        global_y.checked_div(256).unwrap_or(0),
    )
}

/// Builds a region handle from its global south-west corner in metres,
/// `(global_x, global_y)` — the inverse of [`handle_to_global`]. Unlike
/// [`grid_to_handle`], the inputs are already in metres (not region indices),
/// e.g. the `region_x` / `region_y` fields of the login response.
#[must_use]
pub fn global_to_handle(global_x: u32, global_y: u32) -> u64 {
    u64::from(global_x).checked_shl(32).unwrap_or(0) | u64::from(global_y)
}

/// Builds a region handle from grid coordinates (region indices).
#[must_use]
pub fn grid_to_handle(grid_x: u32, grid_y: u32) -> u64 {
    let global_x = u64::from(grid_x).checked_mul(256).unwrap_or(0);
    let global_y = u64::from(grid_y).checked_mul(256).unwrap_or(0);
    global_x.checked_shl(32).unwrap_or(0) | global_y
}
