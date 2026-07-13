//! Protocol-level bit and enum constants that the message template carries as
//! plain integers: parcel flags (`ParcelFlags`), region flags (`RegionFlags`),
//! and the simulator access/maturity rating (`SimAccess`).
//!
//! These live here rather than on the generated message structs because the
//! generated code is regenerated on every build and cannot carry hand-written
//! constants, yet the bit meanings are a fixed part of the wire protocol.

/// The `ParcelFlags` bitfield carried in `ParcelProperties.ParcelData.ParcelFlags`
/// (and the parcel-related update messages). The bit meanings match the viewer's
/// `indra/llinventory/llparcelflags.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParcelFlags {
    /// The raw flags value.
    bits: u32,
}

impl ParcelFlags {
    /// Anyone may fly over the parcel.
    pub const ALLOW_FLY: Self = Self { bits: 1 << 0 };
    /// Anyone's scripts may run on the parcel.
    pub const ALLOW_OTHER_SCRIPTS: Self = Self { bits: 1 << 1 };
    /// The parcel is for sale.
    pub const FOR_SALE: Self = Self { bits: 1 << 2 };
    /// Anyone may place landmarks for the parcel.
    pub const ALLOW_LANDMARK: Self = Self { bits: 1 << 3 };
    /// Anyone may terraform the parcel.
    pub const ALLOW_TERRAFORM: Self = Self { bits: 1 << 4 };
    /// Damage is enabled on the parcel.
    pub const ALLOW_DAMAGE: Self = Self { bits: 1 << 5 };
    /// Anyone may create (rez) objects on the parcel — a public rez zone.
    pub const CREATE_OBJECTS: Self = Self { bits: 1 << 6 };
    /// Access is restricted to a group.
    pub const USE_ACCESS_GROUP: Self = Self { bits: 1 << 8 };
    /// Access is restricted to an explicit allow list.
    pub const USE_ACCESS_LIST: Self = Self { bits: 1 << 9 };
    /// A ban list is in effect (banlines).
    pub const USE_BAN_LIST: Self = Self { bits: 1 << 10 };
    /// A pass (paid temporary access) list is in effect.
    pub const USE_PASS_LIST: Self = Self { bits: 1 << 11 };
    /// The parcel is listed in search.
    pub const SHOW_DIRECTORY: Self = Self { bits: 1 << 12 };
    /// Object entry from neighbouring parcels is denied to non-owners.
    pub const RESTRICT_PUSHOBJECT: Self = Self { bits: 1 << 21 };
    /// Anonymous (non-account) avatars are denied.
    pub const DENY_ANONYMOUS: Self = Self { bits: 1 << 22 };
    /// Group members may create (rez) objects on the parcel — a group rez zone.
    pub const CREATE_GROUP_OBJECTS: Self = Self { bits: 1 << 26 };
    /// Any object may enter the parcel from neighbours.
    pub const ALLOW_ALL_OBJECT_ENTRY: Self = Self { bits: 1 << 27 };
    /// Group objects may enter the parcel from neighbours.
    pub const ALLOW_GROUP_OBJECT_ENTRY: Self = Self { bits: 1 << 28 };
    /// Age-unverified avatars are denied.
    pub const DENY_AGEUNVERIFIED: Self = Self { bits: 1 << 31 };

    /// Builds flags from a raw value.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Returns the raw flags value.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.bits
    }

    /// Returns `true` if every bit in `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.bits & other.bits == other.bits
    }

    /// Returns the union (bitwise OR) of two flag sets — used to combine flags
    /// when building a parcel update.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

/// The `RegionFlags` bitfield carried in `RegionHandshake`/`RegionInfo`. Only the
/// commonly useful bits are named; use [`RegionFlags::bits`] for the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionFlags {
    /// The raw flags value.
    bits: u32,
}

impl RegionFlags {
    /// Damage (combat) is enabled region-wide.
    pub const ALLOW_DAMAGE: Self = Self { bits: 1 << 0 };
    /// Landmarks may be created in the region.
    pub const ALLOW_LANDMARK: Self = Self { bits: 1 << 1 };
    /// Avatars may set their home here.
    pub const ALLOW_SET_HOME: Self = Self { bits: 1 << 2 };
    /// Home is reset on teleport.
    pub const RESET_HOME_ON_TELEPORT: Self = Self { bits: 1 << 3 };
    /// The sun position is fixed.
    pub const SUN_FIXED: Self = Self { bits: 1 << 4 };
    /// Terraforming is blocked.
    pub const BLOCK_TERRAFORM: Self = Self { bits: 1 << 6 };
    /// Land resale is blocked.
    pub const BLOCK_LAND_RESELL: Self = Self { bits: 1 << 7 };
    /// The region is a sandbox (objects are auto-returned).
    pub const SANDBOX: Self = Self { bits: 1 << 8 };
    /// Object entry across the region edge is blocked.
    pub const SKIP_COLLISIONS: Self = Self { bits: 1 << 12 };
    /// Scripts are disabled region-wide.
    pub const SKIP_SCRIPTS: Self = Self { bits: 1 << 13 };
    /// Physics is disabled region-wide.
    pub const SKIP_PHYSICS: Self = Self { bits: 1 << 14 };
    /// Flying is blocked region-wide (the viewer's `REGION_FLAGS_BLOCK_FLY` —
    /// `LLViewerRegion::getBlockFly`, part of `LLAgent::canFly`).
    pub const BLOCK_FLY: Self = Self { bits: 1 << 19 };
    /// The region restricts access to age-verified avatars.
    pub const DENY_AGEUNVERIFIED: Self = Self { bits: 1 << 24 };
    /// Flying above the region's height cap is blocked (`REGION_FLAGS_BLOCK_FLYOVER`).
    pub const BLOCK_FLYOVER: Self = Self { bits: 1 << 27 };

    /// Builds flags from a raw value.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Returns the raw flags value.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.bits
    }

    /// Returns `true` if every bit in `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.bits & other.bits == other.bits
    }
}

/// The simulator access / maturity rating carried as the `SimAccess` byte in
/// `RegionHandshake`, `RegionInfo`, and `TeleportFinish`. Values match the
/// viewer's `indra/llcommon/indra_constants.h`.
pub mod sim_access {
    /// Unknown / not yet rated (usually treated as PG).
    pub const MIN: u8 = 0;
    /// Adult-only content prior to ratings; rarely seen.
    pub const TRIAL: u8 = 7;
    /// General ("PG") content.
    pub const PG: u8 = 13;
    /// Moderate ("Mature") content.
    pub const MATURE: u8 = 21;
    /// Adult content.
    pub const ADULT: u8 = 42;
    /// Region is down / access denied.
    pub const DOWN: u8 = 254;
}
