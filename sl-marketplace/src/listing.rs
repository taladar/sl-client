//! Typed records for SLM marketplace listings.
//!
//! Field names and JSON types mirror the wire format the reference
//! viewer speaks (`llmarketplacefunctions.cpp`): listing ids and stock
//! counts are JSON integers, the listed flag is a JSON boolean, and
//! inventory folder keys travel as hyphenated UUID strings (the null
//! UUID is the all-zeros string).

use serde::{Deserialize, Serialize};
use sl_types::key::InventoryFolderKey;

/// The numeric id of a marketplace listing.
///
/// Appears as a decimal path segment in routes (`/listing/<id>`,
/// `/associate_inventory/<id>`) and as a JSON integer in request and
/// response bodies.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::ListingId`, where it reads clearly"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ListingId(pub u32);

impl std::fmt::Display for ListingId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The inventory side of a listing: which folders hold it and how much
/// stock it has.
///
/// This is the three-field form used by `GET`/`POST /listings` and
/// `PUT /listing/<id>`; [`AssociateInventoryInfo`] is the two-field
/// form used by `PUT /associate_inventory/<id>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryInfo {
    /// The marketplace listing folder (a child of the Marketplace
    /// Listings special folder).
    pub listing_folder_id: InventoryFolderKey,
    /// The active version folder inside the listing folder; the null
    /// key when no version folder has been picked yet.
    pub version_folder_id: InventoryFolderKey,
    /// Units in stock; `-1` when the stock count is not known.
    pub count_on_hand: i32,
}

/// The two-field inventory info sent by `PUT /associate_inventory/<id>`
/// (no stock count — the service recomputes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociateInventoryInfo {
    /// The marketplace listing folder to associate with the listing.
    pub listing_folder_id: InventoryFolderKey,
    /// The active version folder inside the listing folder; the null
    /// key when no version folder has been picked yet.
    pub version_folder_id: InventoryFolderKey,
}

/// One listing record as returned inside the `{"listings": [...]}`
/// response envelope of every JSON route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    /// The numeric listing id.
    pub id: ListingId,
    /// Whether the listing is currently listed (visible on the
    /// marketplace website).
    pub is_listed: bool,
    /// URL of the listing's edit page on the marketplace website;
    /// empty when the service did not send one.
    #[serde(default)]
    pub edit_url: String,
    /// The folders and stock count backing the listing.
    pub inventory_info: InventoryInfo,
}
