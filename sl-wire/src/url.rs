//! Codec boundary for Second Life URL wire fields.
//!
//! Many wire and LLSD/capability fields carry URLs as raw strings (the grid's
//! map/search endpoints, a parcel's media/music stream, an object's media URL, a
//! capability seed URL, …). A conforming peer either sends a parseable URL or the
//! *empty* string, which is the "absent / not set" sentinel. These helpers wrap
//! that convention so the typed surface can hold a [`url::Url`] (or
//! [`Option<url::Url>`], `None` for the empty sentinel) while a non-empty but
//! unparsable value is rejected rather than silently kept as an invalid string —
//! the same non-masking stance as [`region_name_from_wire`](crate::region_name)
//! and the UUID/scalar boundary helpers.

use url::Url;

use crate::error::WireError;

/// Decode a raw wire string into a required [`Url`].
///
/// The value is parsed through [`Url::parse`]; an empty value or one that fails
/// to parse is rejected with [`WireError::InvalidUrl`] rather than masked, so a
/// malformed message is dropped (and surfaced as a diagnostic) instead of
/// masquerading as a valid URL. Use [`optional_url_from_wire`] for fields whose
/// empty value is a legitimate "absent" sentinel. The inverse on encode is
/// [`url_to_wire`].
///
/// # Errors
///
/// Returns [`WireError::InvalidUrl`] when `raw` is empty or does not parse as a
/// URL.
pub fn url_from_wire(field: &'static str, raw: &str) -> Result<Url, WireError> {
    Url::parse(raw).map_err(|_invalid| WireError::InvalidUrl {
        field,
        value: raw.to_owned(),
    })
}

/// Decode a raw wire string into an [`Option<Url>`].
///
/// An empty (or whitespace-only) value is the "absent" sentinel and decodes to
/// `None`. A non-empty value is parsed through [`Url::parse`]; a value that fails
/// to parse is rejected with [`WireError::InvalidUrl`] rather than masked. The
/// inverse on encode is [`optional_url_to_wire`].
///
/// # Errors
///
/// Returns [`WireError::InvalidUrl`] when `raw` is non-empty but does not parse
/// as a URL.
pub fn optional_url_from_wire(field: &'static str, raw: &str) -> Result<Option<Url>, WireError> {
    if raw.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(url_from_wire(field, raw)?))
    }
}

/// Encode a [`Url`] back into its raw wire string (its canonical serialization).
/// The inverse of [`url_from_wire`].
#[must_use]
pub fn url_to_wire(url: &Url) -> String {
    url.as_str().to_owned()
}

/// Encode an [`Option<Url>`] back into its raw wire string: the URL when `Some`,
/// or the empty "absent" sentinel when `None`. The inverse of
/// [`optional_url_from_wire`].
#[must_use]
pub fn optional_url_to_wire(url: Option<&Url>) -> String {
    match url {
        Some(value) => value.as_str().to_owned(),
        None => String::new(),
    }
}

#[cfg(test)]
mod test {
    use super::{optional_url_from_wire, optional_url_to_wire, url_from_wire, url_to_wire};
    use pretty_assertions::assert_eq;

    #[test]
    fn empty_decodes_to_none_and_round_trips() -> Result<(), crate::error::WireError> {
        assert_eq!(optional_url_from_wire("MediaURL", "")?, None);
        assert_eq!(optional_url_from_wire("MediaURL", "   ")?, None);
        assert_eq!(optional_url_to_wire(None), "");
        Ok(())
    }

    #[test]
    fn valid_url_round_trips_bit_identically() -> Result<(), crate::error::WireError> {
        // A canonical URL survives a wire round-trip byte-for-byte.
        let raw = "http://stream.example.com:8000/live";
        let decoded = url_from_wire("MusicURL", raw)?;
        assert_eq!(url_to_wire(&decoded), raw);

        let optional = optional_url_from_wire("MusicURL", raw)?;
        assert!(optional.is_some());
        assert_eq!(optional_url_to_wire(optional.as_ref()), raw);
        Ok(())
    }

    #[test]
    fn slurl_scheme_round_trips() -> Result<(), crate::error::WireError> {
        // A SLURL uses the `secondlife` scheme, which `url` parses fine.
        let raw = "secondlife:///app/agent/00000000-0000-0000-0000-000000000000/about";
        let decoded = url_from_wire("slurl", raw)?;
        assert_eq!(url_to_wire(&decoded), raw);
        Ok(())
    }

    #[test]
    fn non_empty_invalid_url_is_rejected() {
        assert_eq!(
            url_from_wire("MediaURL", "not a url"),
            Err(crate::error::WireError::InvalidUrl {
                field: "MediaURL",
                value: "not a url".to_owned(),
            })
        );
        // Empty is a hard error for the required form, the "absent" sentinel for
        // the optional one.
        assert_eq!(
            url_from_wire("MediaURL", ""),
            Err(crate::error::WireError::InvalidUrl {
                field: "MediaURL",
                value: String::new(),
            })
        );
        assert_eq!(
            optional_url_from_wire("MediaURL", "http://["),
            Err(crate::error::WireError::InvalidUrl {
                field: "MediaURL",
                value: "http://[".to_owned(),
            })
        );
    }
}
