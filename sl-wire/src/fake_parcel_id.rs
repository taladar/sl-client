//! OpenSim's "fake parcel id": a region handle and a region-local position
//! packed into a UUID (`Util.BuildFakeParcelID` / `ParseFakeParcelID`).
//!
//! OpenSim uses it wherever the protocol carries a UUID but the server wants
//! to name a *place*: the lure id of a `RequestTeleport` IM (the accepting
//! client echoes it in `TeleportLureRequest`, and `LureModule` decodes the
//! destination from it), the parcel ids of search results and
//! `RemoteParcelRequest` replies. Second Life's ids for the same fields are
//! opaque, so the parser validates the layout and declines a real UUID.
//!
//! Layout (little-endian): bytes 0–7 the region handle, 8–9 `x`, 10–11 `z`
//! (zero in the two-coordinate form), 12–13 `y`, 14–15 zero.

use uuid::Uuid;

use crate::endian::{u16_from_le, u16_to_le, u64_from_le, u64_to_le};
use crate::region_handle::RegionHandle;

/// A place packed into a fake parcel id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakeParcelId {
    /// The region handle.
    pub region_handle: RegionHandle,
    /// The region-local X, in whole metres.
    pub x: u16,
    /// The region-local Y, in whole metres.
    pub y: u16,
    /// The region-local Z, in whole metres (zero in the two-coordinate form).
    pub z: u16,
}

impl FakeParcelId {
    /// Packs the place into a UUID (the four-coordinate `BuildFakeParcelID`).
    #[must_use]
    pub const fn to_uuid(self) -> Uuid {
        let [h0, h1, h2, h3, h4, h5, h6, h7] = u64_to_le(self.region_handle.0);
        let [x0, x1] = u16_to_le(self.x);
        let [z0, z1] = u16_to_le(self.z);
        let [y0, y1] = u16_to_le(self.y);
        Uuid::from_bytes([h0, h1, h2, h3, h4, h5, h6, h7, x0, x1, z0, z1, y0, y1, 0, 0])
    }

    /// Unpacks a fake parcel id, or `None` when the bytes do not have the
    /// layout (OpenSim's own plausibility checks: the handle's low bytes are
    /// zero since region origins are multiples of 256, the positions are
    /// below 16 km, and the tail bytes are zero) — a real UUID fails them.
    #[must_use]
    pub const fn parse(id: Uuid) -> Option<Self> {
        let [
            h0,
            h1,
            h2,
            h3,
            h4,
            h5,
            h6,
            h7,
            x0,
            x1,
            z0,
            z1,
            y0,
            y1,
            t0,
            t1,
        ] = *id.as_bytes();
        let plausible = h0 == 0 && h4 == 0 && x1 < 64 && y1 < 64 && t0 == 0 && t1 == 0;
        if !plausible || id.is_nil() {
            return None;
        }
        Some(Self {
            region_handle: RegionHandle(u64_from_le([h0, h1, h2, h3, h4, h5, h6, h7])),
            x: u16_from_le([x0, x1]),
            z: u16_from_le([z0, z1]),
            y: u16_from_le([y0, y1]),
        })
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn round_trips_and_matches_the_reference_layout() {
        let place = FakeParcelId {
            region_handle: RegionHandle::from_grid(1000, 1001),
            x: 64,
            y: 200,
            z: 23,
        };
        let id = place.to_uuid();
        // The reference byte order: handle LE, x, z, y, zero tail.
        let mut expected = Vec::new();
        expected.extend_from_slice(&u64_to_le(((1000_u64 * 256) << 32) | (1001 * 256)));
        expected.extend_from_slice(&u16_to_le(64));
        expected.extend_from_slice(&u16_to_le(23));
        expected.extend_from_slice(&u16_to_le(200));
        expected.extend_from_slice(&[0, 0]);
        assert_eq!(id.as_bytes().as_slice(), expected.as_slice());
        assert_eq!(FakeParcelId::parse(id), Some(place));
    }

    #[test]
    fn a_real_uuid_and_nil_are_refused() {
        assert_eq!(FakeParcelId::parse(Uuid::nil()), None);
        assert_eq!(
            FakeParcelId::parse(Uuid::from_u128(0x3b6b_7c62_8f8f_4e34_9c1a_79c2_e2ba_0fd1)),
            None
        );
    }
}
