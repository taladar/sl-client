//! Region-scoped, transient id newtypes the simulator assigns to objects and
//! parcels.
//!
//! Both ids identify something *within a single region's session*: they are not
//! stable across region crossings or relogins (unlike a persistent `full_id`
//! UUID), and the same raw integer can refer to a different entity in a
//! different region. Because that scoping (and the object-vs-parcel distinction)
//! is semantics the compiler can't otherwise see, the ids live here as newtypes
//! — mirroring the [`RegionHandle`](crate::RegionHandle) and `sl-types` key
//! wrappers — rather than as bare integers, so a region-local object id can't be
//! transposed with a region-local parcel id (or any other 32-bit field).
//!
//! The two ids deliberately have different signedness, matching the wire: an
//! object id is the unsigned `u32` of `LLViewerObject::mLocalID`, a parcel id is
//! the signed `i32` of `LLParcel::mLocalID`. Keeping them as distinct types is
//! what reconciles the historical `u32`/`i32` inconsistency.

/// A region-scoped, transient **object** id — the `u32` handle the simulator
/// assigns to a prim or avatar within one region's session (the reference
/// viewer's `LLViewerObject::mLocalID`).
///
/// It is the key of the per-region object cache and how commands reference an
/// object (touch, grab, select, edit, …). It is *not* stable across region
/// crossings or relogins; use the object's persistent `full_id` UUID for that.
/// A value of `0` is the conventional "no object" / "no parent" sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct RegionLocalObjectId(pub u32);

impl RegionLocalObjectId {
    /// Builds a region-local object id from its raw `u32` wire value.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Returns the raw `u32` wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl core::fmt::Display for RegionLocalObjectId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A region-scoped, transient **parcel** id — the `i32` handle the simulator
/// assigns to a parcel within one region's session (the reference viewer's
/// `LLParcel::mLocalID`).
///
/// It identifies a parcel within a region (as carried by `ParcelProperties`,
/// `ParcelDwellReply`, the parcel-management messages, …) and is *not* stable
/// across relogins or region changes; use the parcel's persistent UUID for that.
/// The wire field is signed (`S32`), so this newtype wraps an `i32` rather than
/// the `u32` of [`RegionLocalObjectId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct RegionLocalParcelId(pub i32);

impl RegionLocalParcelId {
    /// Builds a region-local parcel id from its raw `i32` wire value.
    #[must_use]
    pub const fn new(id: i32) -> Self {
        Self(id)
    }

    /// Returns the raw `i32` wire value.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
}

impl core::fmt::Display for RegionLocalParcelId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{RegionLocalObjectId, RegionLocalParcelId};
    use pretty_assertions::assert_eq;

    #[test]
    fn object_id_round_trips_raw_value() {
        let id = RegionLocalObjectId::new(123_456);
        assert_eq!(id.get(), 123_456);
        assert_eq!(RegionLocalObjectId(id.get()), id);
        assert_eq!(id.to_string(), "123456");
    }

    #[test]
    fn object_id_default_is_the_sentinel() {
        assert_eq!(RegionLocalObjectId::default(), RegionLocalObjectId(0));
    }

    #[test]
    fn parcel_id_round_trips_raw_value() {
        let id = RegionLocalParcelId::new(42);
        assert_eq!(id.get(), 42);
        assert_eq!(RegionLocalParcelId(id.get()), id);
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn parcel_id_preserves_sign() {
        // -1 is LL's "no parcel" sentinel in several parcel replies; the signed
        // newtype must round-trip it (a u32-based type could not).
        let id = RegionLocalParcelId::new(-1);
        assert_eq!(id.get(), -1);
        assert_eq!(id.to_string(), "-1");
    }
}
