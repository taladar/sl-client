//! Request builders for every SLM DirectDelivery route.
//!
//! The final URL is always the region's `DirectDelivery` capability
//! URL with the built [`Request::path`] appended verbatim (the
//! reference viewer's `getSLMConnectURL`). Every route except the
//! merchant probe sends `Accept: application/json` and
//! `Content-Type: application/json`; the probe sends neither.

use serde::{Deserialize, Serialize};

use crate::error::BuildRequestError;
use crate::listing::{AssociateInventoryInfo, InventoryInfo, ListingId};

/// The HTTP method of an SLM request (deliberately a tiny local
/// vocabulary so the sans-I/O crate needs no HTTP dependency).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// HTTP GET.
    Get,
    /// HTTP POST.
    Post,
    /// HTTP PUT.
    Put,
    /// HTTP DELETE.
    Delete,
}

/// A fully-built SLM request, ready for a runtime to pair with the
/// `DirectDelivery` capability URL and an HTTP client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// The HTTP method.
    pub method: Method,
    /// The path to append verbatim to the `DirectDelivery` capability
    /// URL (starts with `/`).
    pub path: String,
    /// The pre-serialized JSON body, when the route takes one.
    pub body: Option<String>,
    /// Whether to send the `Accept: application/json` and
    /// `Content-Type: application/json` headers (every route except
    /// the merchant probe, which sends neither — reference-viewer
    /// parity).
    pub json_headers: bool,
}

/// Which SLM route a request (and its eventual reply) belongs to.
///
/// Carried alongside the request through the runtime so the reply can
/// be mapped back to the operation that caused it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// `GET /merchant` — the merchant-status probe.
    MerchantStatus,
    /// `GET /listings` — fetch all listings.
    GetListings,
    /// `GET /listing/<id>` — fetch one listing.
    GetListing(ListingId),
    /// `POST /listings` — create a listing.
    CreateListing,
    /// `PUT /listing/<id>` — update a listing (list / unlist, switch
    /// version folder, stock count).
    UpdateListing(ListingId),
    /// `PUT /associate_inventory/<id>` — associate a listing folder
    /// with an existing listing id.
    AssociateInventory(ListingId),
    /// `DELETE /listing/<id>` — delete (archive) a listing.
    DeleteListing(ListingId),
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MerchantStatus => write!(f, "GET /merchant"),
            Self::GetListings => write!(f, "GET /listings"),
            Self::GetListing(id) => write!(f, "GET /listing/{id}"),
            Self::CreateListing => write!(f, "POST /listings"),
            Self::UpdateListing(id) => write!(f, "PUT /listing/{id}"),
            Self::AssociateInventory(id) => {
                write!(f, "PUT /associate_inventory/{id}")
            }
            Self::DeleteListing(id) => write!(f, "DELETE /listing/{id}"),
        }
    }
}

/// The payload of `POST /listings`: create a listing from a listing
/// folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateListing {
    /// The listing name (the reference viewer sends the listing
    /// folder's name).
    pub name: String,
    /// The backing folders and stock count; `version_folder_id` may be
    /// the null key when no unique version subfolder exists yet.
    pub inventory_info: InventoryInfo,
}

/// The payload of `PUT /listing/<id>`: list / unlist, switch the
/// version folder, or update the stock count.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateListing {
    /// The listing id being updated (also appears in the URL).
    pub id: ListingId,
    /// The desired listed state; the reference viewer forces `false`
    /// when clearing the version folder.
    pub is_listed: bool,
    /// The (possibly changed) backing folders and stock count.
    pub inventory_info: InventoryInfo,
}

/// The payload of `PUT /associate_inventory/<id>`: point an existing
/// listing id at a (new) listing folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociateInventory {
    /// The listing id being associated (also appears in the URL).
    pub id: ListingId,
    /// The folders to associate (no stock count in this form).
    pub inventory_info: AssociateInventoryInfo,
}

/// The `{"listing": ...}` envelope every request body is wrapped in.
#[derive(Serialize)]
struct ListingEnvelope<T: Serialize> {
    /// The wrapped payload.
    listing: T,
}

/// Serialize a payload into the `{"listing": ...}` request envelope.
fn envelope_body<T: Serialize>(payload: &T) -> Result<String, BuildRequestError> {
    Ok(serde_json::to_string(&ListingEnvelope {
        listing: payload,
    })?)
}

/// Build `GET /merchant` (the merchant-status probe; sends no JSON
/// headers — reference-viewer parity).
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::merchant_status_request`, where it reads clearly"
)]
#[must_use]
pub fn merchant_status_request() -> Request {
    Request {
        method: Method::Get,
        path: "/merchant".to_owned(),
        body: None,
        json_headers: false,
    }
}

/// Build `GET /listings` (fetch all listings).
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::listings_request`, where it reads clearly"
)]
#[must_use]
pub fn listings_request() -> Request {
    Request {
        method: Method::Get,
        path: "/listings".to_owned(),
        body: None,
        json_headers: true,
    }
}

/// Build `GET /listing/<id>` (fetch one listing).
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::listing_request`, where it reads clearly"
)]
#[must_use]
pub fn listing_request(id: ListingId) -> Request {
    Request {
        method: Method::Get,
        path: format!("/listing/{id}"),
        body: None,
        json_headers: true,
    }
}

/// Build `POST /listings` (create a listing).
///
/// # Errors
///
/// Returns an error if the payload cannot be serialized to JSON.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::create_listing_request`, where it reads clearly"
)]
pub fn create_listing_request(payload: &CreateListing) -> Result<Request, BuildRequestError> {
    Ok(Request {
        method: Method::Post,
        path: "/listings".to_owned(),
        body: Some(envelope_body(payload)?),
        json_headers: true,
    })
}

/// Build `PUT /listing/<id>` (update a listing).
///
/// # Errors
///
/// Returns an error if the payload cannot be serialized to JSON.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::update_listing_request`, where it reads clearly"
)]
pub fn update_listing_request(payload: &UpdateListing) -> Result<Request, BuildRequestError> {
    Ok(Request {
        method: Method::Put,
        path: format!("/listing/{}", payload.id),
        body: Some(envelope_body(payload)?),
        json_headers: true,
    })
}

/// Build `PUT /associate_inventory/<id>` (associate a listing folder
/// with an existing listing id).
///
/// # Errors
///
/// Returns an error if the payload cannot be serialized to JSON.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::associate_inventory_request`, where it reads clearly"
)]
pub fn associate_inventory_request(
    payload: &AssociateInventory,
) -> Result<Request, BuildRequestError> {
    Ok(Request {
        method: Method::Put,
        path: format!("/associate_inventory/{}", payload.id),
        body: Some(envelope_body(payload)?),
        json_headers: true,
    })
}

/// Build `DELETE /listing/<id>` (delete / archive a listing; no body).
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::delete_listing_request`, where it reads clearly"
)]
#[must_use]
pub fn delete_listing_request(id: ListingId) -> Request {
    Request {
        method: Method::Delete,
        path: format!("/listing/{id}"),
        body: None,
        json_headers: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::Value as JsonValue;
    use sl_types::key::InventoryFolderKey;
    use uuid::Uuid;

    /// Boxed error so tests can use `?` on fallible builders.
    type TestError = Box<dyn core::error::Error>;

    /// The example listing folder key used by the body tests.
    fn listing_folder() -> InventoryFolderKey {
        InventoryFolderKey::from(Uuid::from_u128(0x1111_2222_3333_4444_5555_6666_7777_8888))
    }

    /// The example version folder key used by the body tests.
    fn version_folder() -> InventoryFolderKey {
        InventoryFolderKey::from(Uuid::from_u128(0x9999_aaaa_bbbb_cccc_dddd_eeee_ffff_0000))
    }

    /// Parse a built body back into a JSON value for order-insensitive
    /// comparison.
    fn body_json(request: &Request) -> JsonValue {
        let body = request.body.as_deref().unwrap_or("null");
        serde_json::from_str(body).unwrap_or(JsonValue::Null)
    }

    #[test]
    fn merchant_probe_has_no_body_and_no_json_headers() {
        let request = merchant_status_request();
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.path, "/merchant");
        assert_eq!(request.body, None);
        assert!(!request.json_headers);
    }

    #[test]
    fn get_routes_have_json_headers_and_no_body() {
        let all = listings_request();
        assert_eq!(all.method, Method::Get);
        assert_eq!(all.path, "/listings");
        assert_eq!(all.body, None);
        assert!(all.json_headers);

        let one = listing_request(ListingId(12345));
        assert_eq!(one.method, Method::Get);
        assert_eq!(one.path, "/listing/12345");
        assert_eq!(one.body, None);
    }

    #[test]
    fn create_listing_body_matches_the_wire_shape() -> Result<(), TestError> {
        let request = create_listing_request(&CreateListing {
            name: "My Product".to_owned(),
            inventory_info: InventoryInfo {
                listing_folder_id: listing_folder(),
                version_folder_id: InventoryFolderKey::from(Uuid::nil()),
                count_on_hand: 0,
            },
        })?;
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.path, "/listings");
        assert_eq!(
            body_json(&request),
            serde_json::json!({
                "listing": {
                    "name": "My Product",
                    "inventory_info": {
                        "listing_folder_id": "11112222-3333-4444-5555-666677778888",
                        "version_folder_id": "00000000-0000-0000-0000-000000000000",
                        "count_on_hand": 0,
                    },
                },
            })
        );
        Ok(())
    }

    #[test]
    fn update_listing_body_matches_the_wire_shape() -> Result<(), TestError> {
        let request = update_listing_request(&UpdateListing {
            id: ListingId(12345),
            is_listed: true,
            inventory_info: InventoryInfo {
                listing_folder_id: listing_folder(),
                version_folder_id: version_folder(),
                count_on_hand: 7,
            },
        })?;
        assert_eq!(request.method, Method::Put);
        assert_eq!(request.path, "/listing/12345");
        assert_eq!(
            body_json(&request),
            serde_json::json!({
                "listing": {
                    "id": 12345,
                    "is_listed": true,
                    "inventory_info": {
                        "listing_folder_id": "11112222-3333-4444-5555-666677778888",
                        "version_folder_id": "9999aaaa-bbbb-cccc-dddd-eeeeffff0000",
                        "count_on_hand": 7,
                    },
                },
            })
        );
        Ok(())
    }

    #[test]
    fn associate_inventory_body_has_no_count_and_no_is_listed() -> Result<(), TestError> {
        let request = associate_inventory_request(&AssociateInventory {
            id: ListingId(12345),
            inventory_info: AssociateInventoryInfo {
                listing_folder_id: listing_folder(),
                version_folder_id: version_folder(),
            },
        })?;
        assert_eq!(request.method, Method::Put);
        assert_eq!(request.path, "/associate_inventory/12345");
        assert_eq!(
            body_json(&request),
            serde_json::json!({
                "listing": {
                    "id": 12345,
                    "inventory_info": {
                        "listing_folder_id": "11112222-3333-4444-5555-666677778888",
                        "version_folder_id": "9999aaaa-bbbb-cccc-dddd-eeeeffff0000",
                    },
                },
            })
        );
        Ok(())
    }

    #[test]
    fn delete_listing_has_no_body() {
        let request = delete_listing_request(ListingId(12345));
        assert_eq!(request.method, Method::Delete);
        assert_eq!(request.path, "/listing/12345");
        assert_eq!(request.body, None);
        assert!(request.json_headers);
    }
}
