//! The Second Life Marketplace (SLM) DirectDelivery reply mapping.
//!
//! The SLM API (see [`CAP_DIRECT_DELIVERY`](crate::CAP_DIRECT_DELIVERY))
//! is plain JSON, so its replies do not ride the LLSD
//! `handle_caps_event` path. Instead each runtime performs the HTTP
//! request built by the `sl-marketplace` crate and feeds the status
//! code plus raw body text through the pure mapping functions here,
//! sending the resulting [`Event`] directly to the application (the
//! same pattern the experience-capability fetchers use). Keeping the
//! mapping in one sans-I/O place means both runtimes agree on it and
//! it is unit-testable without sockets.

use crate::types::Event;

pub use sl_marketplace::{
    ApiError as MarketplaceApiError, ApiErrorKind as MarketplaceApiErrorKind, AssociateInventory,
    AssociateInventoryInfo as MarketplaceAssociateInventoryInfo,
    BuildRequestError as MarketplaceBuildRequestError, CreateListing,
    InventoryInfo as MarketplaceInventoryInfo, Listing, ListingId, MerchantStatus,
    Method as MarketplaceMethod, Operation as MarketplaceOperation, Request as MarketplaceRequest,
    UpdateListing, associate_inventory_request, create_listing_request, delete_listing_request,
    listing_request, listings_request, merchant_status_request, parse_deleted_ids,
    parse_listings_response, parse_merchant_status, update_listing_request,
};

/// Map an SLM reply (the HTTP status code and raw body text of a
/// completed request) to the [`Event`] a runtime should surface.
///
/// The merchant probe maps by status alone; `GET /listing/<id>`
/// answering 404 becomes [`Event::MarketplaceListingGone`] (the
/// listing was deleted server-side — reference-viewer semantics, not
/// an error); deletion replies are parsed leniently (only the `id`
/// field of each element is guaranteed); every other error becomes
/// [`Event::MarketplaceError`].
#[must_use]
pub fn marketplace_reply_event(operation: MarketplaceOperation, status: u16, body: &str) -> Event {
    match operation {
        MarketplaceOperation::MerchantStatus => {
            Event::MarketplaceMerchantStatus(parse_merchant_status(status, body))
        }
        MarketplaceOperation::GetListings => match parse_listings_response(status, body) {
            Ok(listings) => Event::MarketplaceListings(listings),
            Err(error) => Event::MarketplaceError { operation, error },
        },
        MarketplaceOperation::GetListing(id) => {
            if status == 404 {
                return Event::MarketplaceListingGone(id);
            }
            match parse_listings_response(status, body) {
                Ok(listings) => Event::MarketplaceListing(listings),
                Err(error) => Event::MarketplaceError { operation, error },
            }
        }
        MarketplaceOperation::CreateListing => match parse_listings_response(status, body) {
            Ok(listings) => Event::MarketplaceListingCreated(listings),
            Err(error) => Event::MarketplaceError { operation, error },
        },
        MarketplaceOperation::UpdateListing(_) => match parse_listings_response(status, body) {
            Ok(listings) => Event::MarketplaceListingUpdated(listings),
            Err(error) => Event::MarketplaceError { operation, error },
        },
        MarketplaceOperation::AssociateInventory(_) => {
            match parse_listings_response(status, body) {
                Ok(listings) => Event::MarketplaceInventoryAssociated(listings),
                Err(error) => Event::MarketplaceError { operation, error },
            }
        }
        MarketplaceOperation::DeleteListing(_) => match parse_deleted_ids(status, body) {
            Ok(ids) => Event::MarketplaceListingDeleted(ids),
            Err(error) => Event::MarketplaceError { operation, error },
        },
    }
}

/// Map an SLM request that never produced an HTTP reply (connection
/// failure, missing `DirectDelivery` capability — the OpenSim case,
/// or an unbuildable request body) to the [`Event`] a runtime should
/// surface.
///
/// The merchant probe reports
/// [`MerchantStatus::ConnectionFailure`] (mirroring the reference
/// viewer's empty-capability-URL path); every other operation reports
/// a [`MarketplaceApiErrorKind::Transport`] error.
#[must_use]
pub const fn marketplace_failure_event(operation: MarketplaceOperation, reason: String) -> Event {
    match operation {
        MarketplaceOperation::MerchantStatus => {
            Event::MarketplaceMerchantStatus(MerchantStatus::ConnectionFailure {
                status: None,
                reason,
            })
        }
        _ => Event::MarketplaceError {
            operation,
            error: MarketplaceApiError {
                status: None,
                kind: MarketplaceApiErrorKind::Transport(reason),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// The canonical single-listing reply body used by the mapping
    /// tests.
    const LISTING_BODY: &str = r#"{
        "listings": [
            {
                "id": 12345,
                "is_listed": true,
                "edit_url": "https://marketplace.secondlife.com/p/x/edit",
                "inventory_info": {
                    "listing_folder_id": "11112222-3333-4444-5555-666677778888",
                    "version_folder_id": "00000000-0000-0000-0000-000000000000",
                    "count_on_hand": 3
                }
            }
        ]
    }"#;

    #[test]
    fn merchant_probe_maps_by_status_alone() {
        assert_eq!(
            marketplace_reply_event(MarketplaceOperation::MerchantStatus, 200, ""),
            Event::MarketplaceMerchantStatus(MerchantStatus::Merchant)
        );
        assert_eq!(
            marketplace_reply_event(MarketplaceOperation::MerchantStatus, 404, ""),
            Event::MarketplaceMerchantStatus(MerchantStatus::NotMerchant)
        );
        assert_eq!(
            marketplace_reply_event(MarketplaceOperation::MerchantStatus, 503, ""),
            Event::MarketplaceMerchantStatus(MerchantStatus::NotMigratedMerchant)
        );
    }

    #[test]
    fn listing_routes_map_success_bodies_to_their_events() {
        /// An [`Event`] constructor taking the parsed listings.
        type ListingsEventFn = fn(Vec<Listing>) -> Event;
        let cases: Vec<(MarketplaceOperation, ListingsEventFn)> = vec![
            (
                MarketplaceOperation::GetListings,
                Event::MarketplaceListings,
            ),
            (
                MarketplaceOperation::GetListing(ListingId(12345)),
                Event::MarketplaceListing,
            ),
            (
                MarketplaceOperation::CreateListing,
                Event::MarketplaceListingCreated,
            ),
            (
                MarketplaceOperation::UpdateListing(ListingId(12345)),
                Event::MarketplaceListingUpdated,
            ),
            (
                MarketplaceOperation::AssociateInventory(ListingId(12345)),
                Event::MarketplaceInventoryAssociated,
            ),
        ];
        for (operation, wrap) in cases {
            let event = marketplace_reply_event(operation, 200, LISTING_BODY);
            match event {
                Event::MarketplaceListings(ref listings)
                | Event::MarketplaceListing(ref listings)
                | Event::MarketplaceListingCreated(ref listings)
                | Event::MarketplaceListingUpdated(ref listings)
                | Event::MarketplaceInventoryAssociated(ref listings) => {
                    assert_eq!(event, wrap(listings.clone()), "wrong event for {operation}");
                    assert_eq!(
                        listings.first().map(|l| l.id),
                        Some(ListingId(12345)),
                        "wrong listing for {operation}"
                    );
                }
                other => {
                    assert_eq!(
                        format!("{other:?}"),
                        "a listings event",
                        "unexpected event for {operation}"
                    );
                }
            }
        }
    }

    #[test]
    fn single_listing_404_means_gone_not_error() {
        assert_eq!(
            marketplace_reply_event(
                MarketplaceOperation::GetListing(ListingId(7)),
                404,
                r#""not found""#
            ),
            Event::MarketplaceListingGone(ListingId(7))
        );
        // ... but 404 on any other route stays a typed error.
        let event = marketplace_reply_event(MarketplaceOperation::GetListings, 404, "");
        match event {
            Event::MarketplaceError {
                operation: MarketplaceOperation::GetListings,
                error,
            } => assert_eq!(error.status, Some(404)),
            other => assert_eq!(format!("{other:?}"), "a MarketplaceError"),
        }
    }

    #[test]
    fn deletion_replies_map_to_deleted_ids() {
        assert_eq!(
            marketplace_reply_event(
                MarketplaceOperation::DeleteListing(ListingId(111)),
                200,
                r#"{"listings": [{"id": 111}]}"#
            ),
            Event::MarketplaceListingDeleted(vec![ListingId(111)])
        );
    }

    #[test]
    fn transport_failures_map_per_operation() {
        assert_eq!(
            marketplace_failure_event(
                MarketplaceOperation::MerchantStatus,
                "no DirectDelivery capability".to_owned()
            ),
            Event::MarketplaceMerchantStatus(MerchantStatus::ConnectionFailure {
                status: None,
                reason: "no DirectDelivery capability".to_owned(),
            })
        );
        assert_eq!(
            marketplace_failure_event(
                MarketplaceOperation::GetListings,
                "connection refused".to_owned()
            ),
            Event::MarketplaceError {
                operation: MarketplaceOperation::GetListings,
                error: MarketplaceApiError {
                    status: None,
                    kind: MarketplaceApiErrorKind::Transport("connection refused".to_owned()),
                },
            }
        );
    }
}
