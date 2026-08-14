//! Response parsers for the SLM DirectDelivery JSON routes.
//!
//! Every JSON route answers with the same envelope,
//! `{"listings": [...]}`; only the merchant probe (see
//! [`crate::merchant`]) is different. `DELETE /listing/<id>` replies
//! carry thinner elements (only the `id` field is guaranteed), so
//! deletions get their own lenient parser.

use serde::Deserialize;

use crate::error::{ApiError, ApiErrorKind, parse_error_body};
use crate::listing::{Listing, ListingId};

/// The `{"listings": [...]}` envelope of every JSON-route reply.
#[derive(Deserialize)]
struct ListingsEnvelope<T> {
    /// The listing records; tolerate a missing key (the service may
    /// answer an empty envelope, which the reference viewer logs and
    /// treats as zero listings).
    #[serde(default = "Vec::new")]
    listings: Vec<T>,
}

/// A deletion-reply element: only the listing id is guaranteed.
#[derive(Deserialize)]
struct DeletedListing {
    /// The id of the deleted (archived) listing.
    id: ListingId,
}

/// Decode a reply envelope of `T` from a 2xx body, mapping JSON
/// decode failures to [`ApiErrorKind::Decode`] and non-2xx statuses
/// to [`parse_error_body`].
fn parse_envelope<T: for<'de> Deserialize<'de>>(
    status: u16,
    body: &str,
) -> Result<Vec<T>, ApiError> {
    if !(200..=299).contains(&status) {
        return Err(parse_error_body(status, body));
    }
    match serde_json::from_str::<ListingsEnvelope<T>>(body) {
        Ok(envelope) => Ok(envelope.listings),
        Err(e) => Err(ApiError {
            status: Some(status),
            kind: ApiErrorKind::Decode(e.to_string()),
        }),
    }
}

/// Parse the `{"listings": [...]}` reply of the listing routes
/// (`GET`/`POST /listings`, `GET`/`PUT /listing/<id>`,
/// `PUT /associate_inventory/<id>`).
///
/// # Errors
///
/// Returns the typed [`ApiError`] for non-2xx statuses and for 2xx
/// bodies that do not decode as the envelope.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root as `sl_marketplace::parse_listings_response`, where it reads clearly"
)]
pub fn parse_listings_response(status: u16, body: &str) -> Result<Vec<Listing>, ApiError> {
    parse_envelope(status, body)
}

/// Parse a `DELETE /listing/<id>` reply into the deleted listing ids
/// (deletion-reply elements only guarantee the `id` field —
/// reference-viewer parity).
///
/// # Errors
///
/// Returns the typed [`ApiError`] for non-2xx statuses and for 2xx
/// bodies that do not decode as the envelope.
pub fn parse_deleted_ids(status: u16, body: &str) -> Result<Vec<ListingId>, ApiError> {
    Ok(parse_envelope::<DeletedListing>(status, body)?
        .into_iter()
        .map(|deleted| deleted.id)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listing::InventoryInfo;
    use pretty_assertions::assert_eq;
    use sl_types::key::InventoryFolderKey;
    use uuid::Uuid;

    /// Boxed error so tests can use `?` on fallible parsers.
    type TestError = Box<dyn core::error::Error>;

    /// The canonical single-listing reply body used by several tests.
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
                },
                "name": "ignored extra field"
            }
        ]
    }"#;

    #[test]
    fn parses_the_canonical_listing_envelope() -> Result<(), TestError> {
        let listings = parse_listings_response(200, LISTING_BODY)
            .map_err(|e| format!("unexpected error: {e}"))?;
        assert_eq!(
            listings,
            vec![Listing {
                id: ListingId(12345),
                is_listed: true,
                edit_url: "https://marketplace.secondlife.com/p/x/edit".to_owned(),
                inventory_info: InventoryInfo {
                    listing_folder_id: InventoryFolderKey::from(Uuid::from_u128(
                        0x1111_2222_3333_4444_5555_6666_7777_8888
                    )),
                    version_folder_id: InventoryFolderKey::from(Uuid::nil()),
                    count_on_hand: 3,
                },
            }]
        );
        Ok(())
    }

    #[test]
    fn tolerates_a_missing_edit_url_and_a_missing_listings_key() -> Result<(), TestError> {
        let body = r#"{
            "listings": [
                {
                    "id": 1,
                    "is_listed": false,
                    "inventory_info": {
                        "listing_folder_id": "11112222-3333-4444-5555-666677778888",
                        "version_folder_id": "00000000-0000-0000-0000-000000000000",
                        "count_on_hand": -1
                    }
                }
            ]
        }"#;
        let listings =
            parse_listings_response(200, body).map_err(|e| format!("unexpected error: {e}"))?;
        assert_eq!(listings.first().map(|l| l.edit_url.as_str()), Some(""));
        assert_eq!(
            listings.first().map(|l| l.inventory_info.count_on_hand),
            Some(-1)
        );

        let empty =
            parse_listings_response(200, "{}").map_err(|e| format!("unexpected error: {e}"))?;
        assert_eq!(empty, Vec::new());
        Ok(())
    }

    #[test]
    fn non_success_statuses_become_typed_errors() {
        let error = match parse_listings_response(404, r#""no such listing""#) {
            Err(error) => error,
            Ok(listings) => {
                assert_eq!(listings, Vec::new(), "expected an error, got listings");
                return;
            }
        };
        assert_eq!(error.status, Some(404));
        assert_eq!(
            error.kind,
            ApiErrorKind::Api {
                error_code: None,
                error_description: None,
                messages: vec!["no such listing".to_owned()],
            }
        );
    }

    #[test]
    fn undecodable_success_bodies_become_decode_errors() {
        match parse_listings_response(200, "<html>not json</html>") {
            Err(ApiError {
                status: Some(200),
                kind: ApiErrorKind::Decode(_),
            }) => {}
            other => {
                assert_eq!(format!("{other:?}"), "a Decode error", "unexpected result");
            }
        }
    }

    #[test]
    fn deletion_replies_only_need_the_id_field() -> Result<(), TestError> {
        let body = r#"{"listings": [{"id": 111}, {"id": 222}]}"#;
        let ids = parse_deleted_ids(200, body).map_err(|e| format!("unexpected error: {e}"))?;
        assert_eq!(ids, vec![ListingId(111), ListingId(222)]);
        Ok(())
    }
}
