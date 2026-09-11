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
use crate::llsd::{Llsd, LlsdError};
use crate::region_handle::RegionHandle;
use sl_types::key::ParcelKey;
use sl_types::map::RegionCoordinates;

/// A decoded `RemoteParcelRequest` body: the region location to resolve, plus the
/// region identity the grid uses to find it. Exactly one of
/// [`region_id`](Self::region_id) / [`region_handle`](Self::region_handle) is
/// meaningful — the viewer sends the id when known and the handle otherwise — so
/// the absent one is nil / zero.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RemoteParcelRequest {
    /// The region-relative position whose parcel to resolve.
    pub location: RegionCoordinates,
    /// The region's grid-wide id (nil when the viewer only knew the handle).
    pub region_id: Uuid,
    /// The 256 m region handle (zero when the viewer sent a `region_id` instead).
    pub region_handle: RegionHandle,
}

impl Default for RemoteParcelRequest {
    fn default() -> Self {
        Self {
            location: RegionCoordinates::new(0.0, 0.0, 0.0),
            region_id: Uuid::nil(),
            region_handle: RegionHandle::default(),
        }
    }
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
    location: RegionCoordinates,
    region_id: Uuid,
    region_handle: RegionHandle,
) -> String {
    let mut map: HashMap<String, Llsd> = HashMap::new();
    let _previous = map.insert(
        "location".to_owned(),
        Llsd::Array(vec![
            Llsd::Real(f64::from(location.x())),
            Llsd::Real(f64::from(location.y())),
            Llsd::Real(f64::from(location.z())),
        ]),
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
/// Returns [`LlsdError::MalformedField`] if `parcel_id` is present but of the
/// wrong LLSD kind.
pub fn parse_remote_parcel_reply(body: &Llsd) -> Result<Option<ParcelKey>, WireError> {
    Ok(body
        .field_uuid("parcel_id", "parcel_id")?
        .map(ParcelKey::from))
}

/// A `RemoteParcelRequest` answer as the client sees it: the resolved parcel id
/// **and the question it answers**.
///
/// The grid's reply is a bare `{ parcel_id }` — it names neither the location
/// nor the region asked about, so two resolves in flight cannot be told apart by
/// content, and handing a window the wrong id is worse than handing it none: it
/// would fill with a different parcel's name, owner and traffic, all of it
/// plausible. The capability is a per-request POST, though, so the runtime
/// *holds* the question while the answer arrives, and
/// [`stamp_remote_parcel_request`] writes it into the reply map before the reply
/// leaves the runtime. This is the same trick `AvatarPickerSearch` plays with
/// its `query-id`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RemoteParcelAnswer {
    /// The grid-wide parcel id covering [`request.location`](RemoteParcelRequest::location).
    pub parcel_id: ParcelKey,
    /// The question this answers, as the client asked it.
    pub request: RemoteParcelRequest,
}

/// Writes the question a `RemoteParcelRequest` POST asked into its reply map, so
/// the answer can be matched to the window that asked.
///
/// Called by the runtimes, which hold the request across the POST; the grid
/// never sends these keys. They are the request body's own keys
/// (`location` / `region_id` / `region_handle`), so
/// [`parse_remote_parcel_request`] reads the echo straight back — one vocabulary
/// for the question whichever direction it travels. Both region fields are
/// written even when only one was sent, so the echo round-trips the request
/// exactly (the unsent one is nil / zero, which is what it was).
///
/// A `reply` that is not a map (a malformed or empty answer) becomes a map
/// holding only the echo: the parcel id is then absent, which
/// [`parse_remote_parcel_answer`] reports as an unresolved location rather than
/// as a correlation failure.
#[must_use]
pub fn stamp_remote_parcel_request(reply: Llsd, request: &RemoteParcelRequest) -> Llsd {
    let mut map = match reply {
        Llsd::Map(map) => map,
        _other => HashMap::new(),
    };
    let _previous = map.insert(
        "location".to_owned(),
        Llsd::Array(vec![
            Llsd::Real(f64::from(request.location.x())),
            Llsd::Real(f64::from(request.location.y())),
            Llsd::Real(f64::from(request.location.z())),
        ]),
    );
    let _previous = map.insert("region_id".to_owned(), Llsd::Uuid(request.region_id));
    let _previous = map.insert(
        "region_handle".to_owned(),
        Llsd::Binary(u64_to_be(request.region_handle.0).to_vec()),
    );
    Llsd::Map(map)
}

/// Decodes a stamped `RemoteParcelRequest` reply into the parcel id **and** the
/// question it answers, or [`None`] when the body lacks a `parcel_id` (the grid
/// could not resolve the location — the question is still known, but there is no
/// answer to correlate).
///
/// # Errors
/// Returns [`LlsdError::MissingField`] when the reply carries no `location`,
/// which means it was never stamped: a correlation-free answer is not something
/// to guess at, since every window waiting would match a defaulted origin
/// equally well. Returns [`LlsdError::MalformedField`] if a present field is of
/// the wrong LLSD kind.
pub fn parse_remote_parcel_answer(body: &Llsd) -> Result<Option<RemoteParcelAnswer>, WireError> {
    if body.field_array("location", "location")?.is_none() {
        return Err(LlsdError::MissingField { field: "location" }.into());
    }
    let request = parse_remote_parcel_request(body)?;
    Ok(parse_remote_parcel_reply(body)?.map(|parcel_id| RemoteParcelAnswer { parcel_id, request }))
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
/// Returns [`LlsdError::MalformedField`] if a decoded LLSD field is present but
/// of the wrong kind.
pub fn parse_remote_parcel_request(body: &Llsd) -> Result<RemoteParcelRequest, WireError> {
    let location = match body.field_array("location", "location")? {
        None => RegionCoordinates::new(0.0, 0.0, 0.0),
        Some(array) => {
            let coord = |index: usize| -> Result<f32, WireError> {
                match array.get(index) {
                    None | Some(Llsd::Undef) => Ok(0.0),
                    Some(v) => v
                        .as_f64()
                        .map(crate::geometry::narrow)
                        .ok_or_else(|| LlsdError::MalformedField {
                            field: "location",
                            value: v.kind().to_owned(),
                        })
                        .map_err(WireError::from),
                }
            };
            RegionCoordinates::new(coord(0)?, coord(1)?, coord(2)?)
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
        ParcelKey, RemoteParcelRequest, build_remote_parcel_request, build_remote_parcel_response,
        parse_remote_parcel_answer, parse_remote_parcel_reply, parse_remote_parcel_request,
        stamp_remote_parcel_request,
    };
    use crate::WireError;
    use crate::llsd::{Llsd, LlsdError, parse_llsd_xml};
    use crate::region_handle::RegionHandle;
    use sl_types::map::RegionCoordinates;

    /// Parses a UUID in a test, surfacing a `String` error for the `?` operator.
    fn uuid(text: &str) -> Result<Uuid, String> {
        Uuid::parse_str(text).map_err(|error| error.to_string())
    }

    /// A request built with a `region_id` round-trips through the server parser,
    /// preserving the location and the id (and leaving the handle zero).
    #[test]
    fn request_with_region_id_round_trips() -> Result<(), String> {
        let region = uuid("11111111-1111-1111-1111-111111111111")?;
        let body = build_remote_parcel_request(
            RegionCoordinates::new(128.0, 64.5, 22.0),
            region,
            RegionHandle(0),
        );
        let parsed =
            parse_remote_parcel_request(&parse_llsd_xml(&body).map_err(|e| format!("{e:?}"))?)
                .map_err(|e| format!("{e:?}"))?;
        assert_eq!(parsed.location, RegionCoordinates::new(128.0, 64.5, 22.0));
        assert_eq!(parsed.region_id, region);
        assert_eq!(parsed.region_handle, RegionHandle(0));
        Ok(())
    }

    /// With a nil `region_id` the builder sends the `region_handle` as 8-byte
    /// big-endian binary, which the parser reads back exactly.
    #[test]
    fn request_with_region_handle_round_trips() -> Result<(), String> {
        let handle = RegionHandle(0x0003_F480_0003_F480_u64);
        let body =
            build_remote_parcel_request(RegionCoordinates::new(1.0, 2.0, 3.0), Uuid::nil(), handle);
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

    /// The stamp puts the question into the grid's bare answer, and the answer
    /// parser reads both back — which is the whole point: two resolves in flight
    /// are told apart by the location and region they name, not by arrival
    /// order.
    #[test]
    fn a_stamped_reply_carries_its_question_back() -> Result<(), String> {
        let parcel = ParcelKey::from(uuid("33333333-3333-3333-3333-333333333333")?);
        let request = RemoteParcelRequest {
            location: RegionCoordinates::new(200.0, 12.25, 30.5),
            region_id: uuid("44444444-4444-4444-4444-444444444444")?,
            region_handle: RegionHandle(0),
        };
        let reply =
            parse_llsd_xml(&build_remote_parcel_response(parcel)).map_err(|e| format!("{e:?}"))?;
        let stamped = stamp_remote_parcel_request(reply, &request);
        let answer = parse_remote_parcel_answer(&stamped)
            .map_err(|e| format!("{e:?}"))?
            .ok_or("expected a resolved parcel id")?;
        assert_eq!(answer.parcel_id, parcel);
        assert_eq!(answer.request, request);
        Ok(())
    }

    /// A request that named its region by handle is echoed as one: the stamp
    /// writes both region fields, so the unsent one comes back nil / zero rather
    /// than as a different question.
    #[test]
    fn the_stamp_round_trips_a_handle_request() -> Result<(), String> {
        let parcel = ParcelKey::from(uuid("55555555-5555-5555-5555-555555555555")?);
        let request = RemoteParcelRequest {
            location: RegionCoordinates::new(0.0, 0.0, 0.0),
            region_id: Uuid::nil(),
            region_handle: RegionHandle(0x0003_F480_0003_F480_u64),
        };
        let reply =
            parse_llsd_xml(&build_remote_parcel_response(parcel)).map_err(|e| format!("{e:?}"))?;
        let answer = parse_remote_parcel_answer(&stamp_remote_parcel_request(reply, &request))
            .map_err(|e| format!("{e:?}"))?
            .ok_or("expected a resolved parcel id")?;
        assert_eq!(answer.request, request);
        Ok(())
    }

    /// An unresolved location still answers `{}`. Stamped, that is a known
    /// question with no answer — `None`, not an error, and not a correlation
    /// failure.
    #[test]
    fn an_unresolved_stamped_reply_is_none() -> Result<(), String> {
        let request = RemoteParcelRequest::default();
        let stamped =
            stamp_remote_parcel_request(Llsd::Map(std::collections::HashMap::new()), &request);
        let answer = parse_remote_parcel_answer(&stamped).map_err(|e| format!("{e:?}"))?;
        assert_eq!(answer, None);
        Ok(())
    }

    /// An *unstamped* reply is rejected rather than defaulted. Every window
    /// waiting would match a defaulted origin equally well, so guessing here is
    /// how the wrong parcel's name, owner and traffic end up in a window — the
    /// failure this whole correlation exists to prevent.
    #[test]
    fn an_unstamped_reply_is_an_error() -> Result<(), String> {
        let parcel = ParcelKey::from(uuid("66666666-6666-6666-6666-666666666666")?);
        let reply =
            parse_llsd_xml(&build_remote_parcel_response(parcel)).map_err(|e| format!("{e:?}"))?;
        match parse_remote_parcel_answer(&reply) {
            Err(WireError::Llsd(LlsdError::MissingField { field })) => {
                assert_eq!(field, "location");
                Ok(())
            }
            other => Err(format!("expected a missing-location error, got {other:?}")),
        }
    }
}
