//! The **`RemoteParcelRequest`** capability: resolve a parcel id from a region
//! location.
//!
//! Older land/search code identifies a parcel by its region-local id, but the
//! places/search panels need a grid-wide *parcel id* to fetch a parcel's listing
//! by `ParcelInfoRequest`. The viewer obtains that id by POSTing a region
//! location to the `RemoteParcelRequest` capability and reading the `parcel_id`
//! out of the LLSD reply.
//!
//! This module builds that request body and decodes the reply (client side), and
//! parses the request and builds the reply (server side). The body keys
//! (`location`, `region_id`, `region_handle`, `parcel_id`) are cross-checked
//! against the Firestorm viewer's `indra/newview/llremoteparcelrequest.cpp` and
//! OpenSim's `LandManagementModule` cap handler.
//!
//! The capability is a single POST:
//!
//! - `RemoteParcelRequest` — POST `{ location: [x, y, z], region_id?: <uuid>,
//!   region_handle?: <u64> }` → `{ parcel_id: <uuid> }`. The viewer sends the
//!   region id when it knows it, otherwise the 256 m region handle; the grid
//!   resolves either to the parcel covering `location`.

use std::collections::HashMap;

use uuid::Uuid;

use crate::WireError;
use crate::endian::{u64_from_be, u64_to_be};
use crate::llsd::Llsd;
use crate::region_handle::RegionHandle;
use sl_types::key::ParcelKey;

/// A decoded `RemoteParcelRequest` body: the region location to resolve, plus the
/// region identity the grid uses to find it. Exactly one of
/// [`region_id`](Self::region_id) / [`region_handle`](Self::region_handle) is
/// meaningful — the viewer sends the id when known and the handle otherwise — so
/// the absent one is nil / zero.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RemoteParcelRequest {
    /// The region-relative position `[x, y, z]` whose parcel to resolve.
    pub location: [f64; 3],
    /// The region's grid-wide id (nil when the viewer only knew the handle).
    pub region_id: Uuid,
    /// The 256 m region handle (zero when the viewer sent a `region_id` instead).
    pub region_handle: RegionHandle,
}

// ---------------------------------------------------------------------------
// Client side — the request builder and reply parser.
// ---------------------------------------------------------------------------

/// Builds the LLSD body for a `RemoteParcelRequest` POST. `location` is the
/// region-relative position whose parcel to resolve; a non-nil `region_id` is
/// sent verbatim, otherwise a non-zero `region_handle` is sent as the 8-byte
/// big-endian binary the grid expects (mirroring the viewer, which prefers the id
/// when it knows the region and falls back to the handle). Built on
/// [`Llsd::to_llsd_xml`], so it round-trips through
/// [`parse_remote_parcel_request`].
#[must_use]
pub fn build_remote_parcel_request(
    location: [f64; 3],
    region_id: Uuid,
    region_handle: RegionHandle,
) -> String {
    let mut map: HashMap<String, Llsd> = HashMap::new();
    let _previous = map.insert(
        "location".to_owned(),
        Llsd::Array(location.iter().map(|coord| Llsd::Real(*coord)).collect()),
    );
    if region_id.is_nil() {
        let _previous = map.insert(
            "region_handle".to_owned(),
            Llsd::Binary(u64_to_be(region_handle.0).to_vec()),
        );
    } else {
        let _previous = map.insert("region_id".to_owned(), Llsd::Uuid(region_id));
    }
    Llsd::Map(map).to_llsd_xml()
}

/// Decodes a `RemoteParcelRequest` reply (`{ parcel_id }`) into the resolved
/// parcel id, or [`None`] when the body lacks a `parcel_id` (the grid could not
/// resolve the location).
///
/// # Errors
/// Returns [`WireError::MalformedField`] if `parcel_id` is present but of the
/// wrong LLSD kind.
pub fn parse_remote_parcel_reply(body: &Llsd) -> Result<Option<ParcelKey>, WireError> {
    Ok(body
        .field_uuid("parcel_id", "parcel_id")?
        .map(ParcelKey::from))
}

// ---------------------------------------------------------------------------
// Server side — the inverse: the request parser and reply builder.
// ---------------------------------------------------------------------------

/// Parses a `RemoteParcelRequest` POST body — the inverse of
/// [`build_remote_parcel_request`]. A missing `location` defaults to the origin;
/// an absent `region_id` / `region_handle` decodes to nil / zero. The
/// `region_handle` is read from the 8-byte big-endian binary the viewer sends.
///
/// # Errors
/// Returns [`WireError::MalformedField`] if a decoded LLSD field is present but
/// of the wrong kind.
pub fn parse_remote_parcel_request(body: &Llsd) -> Result<RemoteParcelRequest, WireError> {
    let location = match body.field_array("location", "location")? {
        None => [0.0, 0.0, 0.0],
        Some(array) => {
            let coord = |index: usize| -> Result<f64, WireError> {
                match array.get(index) {
                    None | Some(Llsd::Undef) => Ok(0.0),
                    Some(v) => v.as_f64().ok_or_else(|| WireError::MalformedField {
                        field: "location",
                        value: v.kind().to_owned(),
                    }),
                }
            };
            [coord(0)?, coord(1)?, coord(2)?]
        }
    };
    let region_id = body
        .field_uuid("region_id", "region_id")?
        .unwrap_or_else(Uuid::nil);
    let region_handle = body
        .field_binary("region_handle", "region_handle")?
        .and_then(|bytes| bytes.get(0..8))
        .and_then(|head| <[u8; 8]>::try_from(head).ok())
        .map(|head| RegionHandle(u64_from_be(head)))
        .unwrap_or_default();
    Ok(RemoteParcelRequest {
        location,
        region_id,
        region_handle,
    })
}

/// Builds a `RemoteParcelRequest` reply (`{ parcel_id }`) from a resolved parcel
/// id — the inverse of [`parse_remote_parcel_reply`]. Built on
/// [`Llsd::to_llsd_xml`], so it round-trips through
/// [`parse_llsd_xml`](crate::parse_llsd_xml).
#[must_use]
pub fn build_remote_parcel_response(parcel_id: ParcelKey) -> String {
    Llsd::Map(HashMap::from([(
        "parcel_id".to_owned(),
        Llsd::Uuid(parcel_id.uuid()),
    )]))
    .to_llsd_xml()
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use uuid::Uuid;

    use super::{
        ParcelKey, build_remote_parcel_request, build_remote_parcel_response,
        parse_remote_parcel_reply, parse_remote_parcel_request,
    };
    use crate::llsd::parse_llsd_xml;
    use crate::region_handle::RegionHandle;

    /// Parses a UUID in a test, surfacing a `String` error for the `?` operator.
    fn uuid(text: &str) -> Result<Uuid, String> {
        Uuid::parse_str(text).map_err(|error| error.to_string())
    }

    /// A request built with a `region_id` round-trips through the server parser,
    /// preserving the location and the id (and leaving the handle zero).
    #[test]
    fn request_with_region_id_round_trips() -> Result<(), String> {
        let region = uuid("11111111-1111-1111-1111-111111111111")?;
        let body = build_remote_parcel_request([128.0, 64.5, 22.0], region, RegionHandle(0));
        let parsed =
            parse_remote_parcel_request(&parse_llsd_xml(&body).map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            parsed.location.map(f64::to_bits),
            [128.0_f64, 64.5, 22.0].map(f64::to_bits)
        );
        assert_eq!(parsed.region_id, region);
        assert_eq!(parsed.region_handle, RegionHandle(0));
        Ok(())
    }

    /// With a nil `region_id` the builder sends the `region_handle` as 8-byte
    /// big-endian binary, which the parser reads back exactly.
    #[test]
    fn request_with_region_handle_round_trips() -> Result<(), String> {
        let handle = RegionHandle(0x0003_F480_0003_F480_u64);
        let body = build_remote_parcel_request([1.0, 2.0, 3.0], Uuid::nil(), handle);
        let parsed =
            parse_remote_parcel_request(&parse_llsd_xml(&body).map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?;
        assert_eq!(parsed.region_id, Uuid::nil());
        assert_eq!(parsed.region_handle, handle);
        Ok(())
    }

    /// The reply builder round-trips through the client parser.
    #[test]
    fn reply_round_trips() -> Result<(), String> {
        let parcel = ParcelKey::from(uuid("22222222-2222-2222-2222-222222222222")?);
        let xml = build_remote_parcel_response(parcel);
        let parsed =
            parse_remote_parcel_reply(&parse_llsd_xml(&xml).map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?;
        assert_eq!(parsed, Some(parcel));
        Ok(())
    }
}
