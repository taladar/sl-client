//! The embedded inventory item a notecard carries — the landmark, object or
//! other item a resident drops into the body — modelled as the inventory item
//! it is (id, asset id, type, name, permissions).

use crate::types::{AssetType, InventoryType, PermissionMask, SaleType};
use sl_types::key::Key;
use uuid::Uuid;

/// The XOR pad the simulator uses to lightly obfuscate an asset id it writes as
/// a `shadow_id` (`LLInventoryItem`'s `MAGIC_ID`).
const MAGIC_ID: Uuid = uuid::uuid!("3c115e51-04f4-523c-9fa6-98aff1034730");

/// Apply the `MAGIC_ID` XOR pad to a key. The cipher is symmetric, so this is
/// both the obfuscation (asset id → `shadow_id`) and its inverse.
#[must_use]
pub(crate) fn xor_magic(key: Key) -> Key {
    let source = key.0.as_bytes();
    let pad = MAGIC_ID.as_bytes();
    let mut out = [0u8; 16];
    for (slot, (byte, mask)) in out.iter_mut().zip(source.iter().zip(pad.iter())) {
        *slot = byte ^ mask;
    }
    Key(Uuid::from_bytes(out))
}

/// The five permission scopes and ownership ids of an embedded item
/// (`LLPermissions`). The masks are kept as their raw bit values for faithful
/// round-tripping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    /// The base mask, the ceiling all other masks are clamped to.
    pub base_mask: PermissionMask,
    /// The permissions the current owner has.
    pub owner_mask: PermissionMask,
    /// The permissions the owning group has.
    pub group_mask: PermissionMask,
    /// The permissions everyone has.
    pub everyone_mask: PermissionMask,
    /// The permissions the next owner will receive.
    pub next_owner_mask: PermissionMask,
    /// The creator of the item.
    pub creator_id: Key,
    /// The current owner of the item.
    pub owner_id: Key,
    /// The previous owner of the item.
    pub last_owner_id: Key,
    /// The group the item is shared with.
    pub group_id: Key,
    /// Whether the group (rather than an individual) owns the item.
    pub group_owned: bool,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            base_mask: PermissionMask(0),
            owner_mask: PermissionMask(0),
            group_mask: PermissionMask(0),
            everyone_mask: PermissionMask(0),
            next_owner_mask: PermissionMask(0),
            creator_id: sl_types::key::NULL_KEY,
            owner_id: sl_types::key::NULL_KEY,
            last_owner_id: sl_types::key::NULL_KEY,
            group_id: sl_types::key::NULL_KEY,
            group_owned: false,
        }
    }
}

/// The sale terms of an embedded item (`LLSaleInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaleInfo {
    /// How the item may be sold.
    pub sale_type: SaleType,
    /// The asking price in L$.
    pub sale_price: i32,
}

impl Default for SaleInfo {
    fn default() -> Self {
        Self {
            sale_type: SaleType::NotForSale,
            sale_price: 0,
        }
    }
}

/// How the asset id was stored in the stream, so it re-emits in the same form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetIdEncoding {
    /// Written in the clear as `asset_id`.
    Plain,
    /// Written XOR-obfuscated as `shadow_id`.
    Shadow,
}

/// A single inventory item embedded in a notecard, decoded from its legacy
/// stream chunk. Unknown keyword lines are preserved in
/// [`unknown_fields`](InventoryItem::unknown_fields) so a future field is never
/// silently dropped — this is somebody's inventory.
#[expect(
    clippy::module_name_repetitions,
    reason = "the type is used across the crate as sl_notecard::InventoryItem"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InventoryItem {
    /// The item's own id.
    pub item_id: Key,
    /// The id of the folder the item came from.
    pub parent_id: Key,
    /// The permission scopes and ownership.
    pub permissions: Permissions,
    /// The metadata blob (thumbnail / favourite LLSD), preserved verbatim as
    /// the raw value written after the `metadata` keyword (without the trailing
    /// `|` separator) when present. Rare on embedded items.
    pub metadata: Option<String>,
    /// The asset the item points at (always the real id; obfuscation is undone
    /// on decode and re-applied on encode per
    /// [`asset_id_encoding`](InventoryItem::asset_id_encoding)).
    pub asset_id: Key,
    /// Whether the asset id was stored in the clear or XOR-obfuscated.
    pub asset_id_encoding: AssetIdEncoding,
    /// The asset class.
    pub asset_type: AssetType,
    /// The inventory classification.
    pub inventory_type: InventoryType,
    /// The item flags bitfield.
    pub flags: u32,
    /// The sale terms.
    pub sale_info: SaleInfo,
    /// The item's display name.
    pub name: String,
    /// The item's description.
    pub description: String,
    /// The item's creation date (Unix seconds).
    pub creation_date: i64,
    /// Raw keyword lines this decoder did not recognise, preserved verbatim
    /// (leading whitespace trimmed) and re-emitted so unknown or future fields
    /// survive a round-trip.
    pub unknown_fields: Vec<String>,
}

impl InventoryItem {
    /// The asset id in the obfuscated (`shadow_id`) form the stream stores it in
    /// when [`asset_id_encoding`](Self::asset_id_encoding) is
    /// [`Shadow`](AssetIdEncoding::Shadow).
    #[must_use]
    pub fn shadow_id(&self) -> Key {
        xor_magic(self.asset_id)
    }
}

/// An embedded item together with the character index the notecard text refers
/// to it by (the `index` in `FIRST_EMBEDDED_CHAR + index`).
#[expect(
    clippy::module_name_repetitions,
    reason = "the type is used across the crate as sl_notecard::EmbeddedItem"
)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedItem {
    /// The index the text references this item by.
    pub char_index: u32,
    /// The embedded inventory item.
    pub item: InventoryItem,
}
