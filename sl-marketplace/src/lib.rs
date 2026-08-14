#![doc = include_str!("../README.md")]

pub mod error;
pub mod listing;
pub mod merchant;
pub mod request;
pub mod response;

pub use error::{ApiError, ApiErrorKind, BuildRequestError, parse_error_body};
pub use listing::{AssociateInventoryInfo, InventoryInfo, Listing, ListingId};
pub use merchant::{MerchantStatus, parse_merchant_status};
pub use request::{
    AssociateInventory, CreateListing, Method, Operation, Request, UpdateListing,
    associate_inventory_request, create_listing_request, delete_listing_request, listing_request,
    listings_request, merchant_status_request, update_listing_request,
};
pub use response::{parse_deleted_ids, parse_listings_response};
