//! Inventory structure and region-handle coordinate helpers.

use sl_types::key::{AgentKey, GroupKey, InventoryFolderKey, InventoryKey, OwnerKey};
use sl_wire::{Permissions5, RegionHandle};
use uuid::Uuid;

/// An inventory folder (category): from the login skeleton
/// ([`Event::InventorySkeleton`](crate::Event::InventorySkeleton)) or an `InventoryDescendents` sub-folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryFolder {
    /// The folder's id.
    pub folder_id: InventoryFolderKey,
    /// The parent folder's id (nil for the root).
    pub parent_id: InventoryFolderKey,
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
    /// The sale price, in L$.
    pub sale_price: i32,
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

impl Default for NewInventoryItem {
    /// All-nil/zero defaults; [`folder_id`](Self::folder_id) is the nil inventory
    /// folder key ([`InventoryFolderKey`] is not itself [`Default`]).
    fn default() -> Self {
        Self {
            folder_id: InventoryFolderKey::from(Uuid::nil()),
            transaction_id: Uuid::nil(),
            next_owner_mask: 0,
            asset_type: 0,
            inv_type: 0,
            wearable_type: 0,
            name: String::new(),
            description: String::new(),
        }
    }
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
            parent_id: InventoryFolderKey::from(Uuid::nil()),
            name: "Objects".to_owned(),
            folder_type: -1,
            version: 1,
        };
        assert_eq!(folder.folder_id.uuid(), folder_raw);
        assert!(folder.parent_id.uuid().is_nil());
    }
}
