//! Codec boundary for Second Life region-name wire fields.
//!
//! The wire carries region (simulator) names as raw, NUL-padded strings. A
//! conforming Second Life grid only ever sends names that satisfy the
//! [`RegionName`](sl_types::map::RegionName) grammar (2–35 characters, the limit
//! the SL wiki documents), and an *empty* value is the "region unknown / not
//! set" sentinel. These helpers wrap that convention so the typed surface can
//! hold an [`Option<RegionName>`] (`None` for the empty sentinel) while a
//! non-empty but invalid name is rejected rather than silently coerced.

use sl_types::map::RegionName;

use crate::error::WireError;

/// Decode a raw wire region-name string into an [`Option<RegionName>`].
///
/// An empty (or whitespace-only) value is the "unknown region" sentinel and
/// decodes to `None`. A non-empty value is validated through
/// [`RegionName::try_new`]; a value that fails the region-name grammar (its
/// trimmed length falls outside 2–35 characters — possible on OpenSim, never on
/// a conforming Second Life grid) is rejected with
/// [`WireError::InvalidRegionName`] rather than masked, so a malformed message
/// is dropped (and surfaced as a diagnostic) instead of masquerading as a valid
/// name. The inverse on encode is [`region_name_to_wire`].
///
/// # Errors
///
/// Returns [`WireError::InvalidRegionName`] when `raw` is non-empty but does not
/// satisfy the [`RegionName`] grammar.
pub fn region_name_from_wire(
    field: &'static str,
    raw: &str,
) -> Result<Option<RegionName>, WireError> {
    if raw.trim().is_empty() {
        Ok(None)
    } else {
        match RegionName::try_new(raw) {
            Ok(name) => Ok(Some(name)),
            Err(_invalid) => Err(WireError::InvalidRegionName {
                field,
                value: raw.to_owned(),
            }),
        }
    }
}

/// Encode an [`Option<RegionName>`] back into its raw wire region-name string:
/// the (trimmed) name when `Some`, or the empty "unknown region" sentinel when
/// `None`. The inverse of [`region_name_from_wire`].
#[must_use]
pub fn region_name_to_wire(name: Option<&RegionName>) -> String {
    match name {
        Some(region_name) => region_name.to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod test {
    use super::{region_name_from_wire, region_name_to_wire};
    use pretty_assertions::assert_eq;

    #[test]
    fn empty_decodes_to_none_and_round_trips() -> Result<(), crate::error::WireError> {
        assert_eq!(region_name_from_wire("SimName", "")?, None);
        assert_eq!(region_name_from_wire("SimName", "   ")?, None);
        assert_eq!(region_name_to_wire(None), "");
        Ok(())
    }

    #[test]
    fn valid_name_round_trips_bit_identically() -> Result<(), crate::error::WireError> {
        let decoded = region_name_from_wire("SimName", "Beach Valley")?;
        assert!(decoded.is_some());
        assert_eq!(region_name_to_wire(decoded.as_ref()), "Beach Valley");
        Ok(())
    }

    #[test]
    fn non_empty_invalid_name_is_rejected() {
        // A single character is below the 2-character minimum.
        assert_eq!(
            region_name_from_wire("SimName", "x"),
            Err(crate::error::WireError::InvalidRegionName {
                field: "SimName",
                value: "x".to_owned(),
            })
        );
        // 36 characters exceeds the 35-character maximum.
        let too_long = "a".repeat(36);
        assert_eq!(
            region_name_from_wire("SimName", &too_long),
            Err(crate::error::WireError::InvalidRegionName {
                field: "SimName",
                value: too_long,
            })
        );
    }
}
