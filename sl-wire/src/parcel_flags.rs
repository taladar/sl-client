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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// In-world sounds (gestures, object/collision sounds) are restricted to the
    /// parcel — they do not play outside it, and outside sounds do not play in
    /// (`PF_SOUND_LOCAL`).
    pub const SOUND_LOCAL: Self = Self { bits: 1 << 15 };
    /// The parcel's search listing is flagged as moderate ("mature") content
    /// (`PF_MATURE_PUBLISH`).
    pub const MATURE_PUBLISH: Self = Self { bits: 1 << 18 };
    /// Group members' scripts may run on the parcel (`PF_ALLOW_GROUP_SCRIPTS`).
    pub const ALLOW_GROUP_SCRIPTS: Self = Self { bits: 1 << 25 };
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
    /// Residents may use voice chat on the parcel (`PF_ALLOW_VOICE_CHAT`).
    pub const ALLOW_VOICE: Self = Self { bits: 1 << 29 };
    /// The parcel routes voice through the estate-wide channel rather than its
    /// own (`PF_USE_ESTATE_VOICE_CHAN`).
    pub const USE_ESTATE_VOICE_CHAN: Self = Self { bits: 1 << 30 };
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

    /// Returns this set with `other`'s flags cleared — the inverse of
    /// [`union`](Self::union), for the edits that take a flag *off* a parcel
    /// (a sale that completes clears `FOR_SALE`).
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self {
            bits: self.bits & !other.bits,
        }
    }
}

/// The region flag space (`REGION_FLAGS_*`, the reference viewer's
/// `indra/llmessage/llregionflags.h`), 64 bits wide as the reference keeps it.
///
/// One space serves the region and the estate: `RegionHandshake` and
/// `RegionInfo` carry it as a legacy 32-bit `RegionFlags` field plus, where the
/// grid sends the block, the full `RegionFlagsExtended`; an estate's
/// `estateupdateinfo` reply and `estatechangeinfo` request carry the estate's
/// bits of the same space ([`RegionFlags::ESTATE_MASK`] and the access,
/// voice, teleport and environment toggles) as a decimal string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RegionFlags {
    /// The raw flags value.
    bits: u64,
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
    /// Parcel owners may set stricter access than the estate (an estate bit).
    pub const ALLOW_ACCESS_OVERRIDE: Self = Self { bits: 1 << 5 };
    /// Terraforming is blocked.
    pub const BLOCK_TERRAFORM: Self = Self { bits: 1 << 6 };
    /// Land resale is blocked.
    pub const BLOCK_LAND_RESELL: Self = Self { bits: 1 << 7 };
    /// The region is a sandbox (objects are auto-returned).
    pub const SANDBOX: Self = Self { bits: 1 << 8 };
    /// Parcel owners may publish their own environment (EEP) over the
    /// region's — the estate half of the environment panel's "Parcel Owners
    /// May Override" checkbox. With it clear, a parcel's `ExtEnvironment`
    /// entry is ignored and every parcel renders the region's environment.
    pub const ALLOW_ENVIRONMENT_OVERRIDE: Self = Self { bits: 1 << 9 };
    /// Every non-agent rigid body is pinned (collisions disabled region-wide).
    pub const SKIP_COLLISIONS: Self = Self { bits: 1 << 12 };
    /// Scripts are disabled region-wide.
    pub const SKIP_SCRIPTS: Self = Self { bits: 1 << 13 };
    /// Physics is disabled region-wide.
    pub const SKIP_PHYSICS: Self = Self { bits: 1 << 14 };
    /// The estate is publicly visible: anyone may visit (an estate bit).
    pub const EXTERNALLY_VISIBLE: Self = Self { bits: 1 << 15 };
    /// Parcel owners may return objects encroaching on their land.
    pub const ALLOW_RETURN_ENCROACHING_OBJECT: Self = Self { bits: 1 << 16 };
    /// Estate managers may return objects encroaching on the estate.
    pub const ALLOW_RETURN_ENCROACHING_ESTATE_OBJECT: Self = Self { bits: 1 << 17 };
    /// Traffic (dwell) is not counted.
    pub const BLOCK_DWELL: Self = Self { bits: 1 << 18 };
    /// Flying is blocked region-wide (the viewer's `REGION_FLAGS_BLOCK_FLY` —
    /// `LLViewerRegion::getBlockFly`, part of `LLAgent::canFly`).
    pub const BLOCK_FLY: Self = Self { bits: 1 << 19 };
    /// Direct teleport (rather than routing to a telehub) is allowed (an
    /// estate bit).
    pub const ALLOW_DIRECT_TELEPORT: Self = Self { bits: 1 << 20 };
    /// Scripts owned by non-estate-managers are disabled region-wide
    /// (`REGION_FLAGS_ESTATE_SKIP_SCRIPTS`, part of the reference viewer's
    /// `LLViewerParcelMgr::allowAgentScripts`).
    pub const ESTATE_SKIP_SCRIPTS: Self = Self { bits: 1 << 21 };
    /// Pushing (`llPushObject`) against avatars is restricted region-wide
    /// (`REGION_FLAGS_RESTRICT_PUSHOBJECT` / `LLViewerRegion::getRestrictPushObject`,
    /// part of the reference viewer's `LLViewerParcelMgr::allowAgentPush`).
    pub const RESTRICT_PUSHOBJECT: Self = Self { bits: 1 << 22 };
    /// Avatars without payment information on file are denied (an estate
    /// bit).
    pub const DENY_ANONYMOUS: Self = Self { bits: 1 << 23 };
    /// Parcels may be joined or subdivided (`REGION_FLAGS_ALLOW_PARCEL_CHANGES`).
    /// Drives the About Land covenant tab's "Subdivide" clause.
    pub const ALLOW_PARCEL_CHANGES: Self = Self { bits: 1 << 26 };
    /// Flying above the region's height cap is blocked (`REGION_FLAGS_BLOCK_FLYOVER`).
    pub const BLOCK_FLYOVER: Self = Self { bits: 1 << 27 };
    /// Voice chat is allowed (an estate bit).
    pub const ALLOW_VOICE: Self = Self { bits: 1 << 28 };
    /// The region's parcels are kept out of search
    /// (`REGION_FLAGS_BLOCK_PARCEL_SEARCH`).
    pub const BLOCK_PARCEL_SEARCH: Self = Self { bits: 1 << 29 };
    /// Age-unverified avatars are denied (an estate bit).
    pub const DENY_AGEUNVERIFIED: Self = Self { bits: 1 << 30 };
    /// Scripted agents (bots) are denied (an estate bit).
    pub const DENY_BOTS: Self = Self { bits: 1 << 31 };

    /// The bits a region takes from its estate (`REGION_FLAGS_ESTATE_MASK`).
    pub const ESTATE_MASK: Self = Self {
        bits: Self::EXTERNALLY_VISIBLE.bits
            | Self::SUN_FIXED.bits
            | Self::DENY_ANONYMOUS.bits
            | Self::DENY_AGEUNVERIFIED.bits,
    };

    /// Builds flags from a raw value.
    #[must_use]
    pub const fn from_bits(bits: u64) -> Self {
        Self { bits }
    }

    /// Builds flags from a legacy 32-bit field (the low half of the space).
    #[must_use]
    pub fn from_legacy(bits: u32) -> Self {
        Self {
            bits: u64::from(bits),
        }
    }

    /// Returns the raw flags value.
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.bits
    }

    /// The low 32 bits, as a legacy 32-bit `RegionFlags` field carries them —
    /// the reference viewer truncates the same way.
    #[must_use]
    pub fn legacy_bits(self) -> u32 {
        u32::try_from(self.bits & 0xffff_ffff).unwrap_or(u32::MAX)
    }

    /// Returns `true` if every bit in `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.bits & other.bits == other.bits
    }

    /// A copy with the bits of `flag` set when `on`, cleared otherwise.
    #[must_use]
    pub const fn with(self, flag: Self, on: bool) -> Self {
        if on {
            Self {
                bits: self.bits | flag.bits,
            }
        } else {
            Self {
                bits: self.bits & !flag.bits,
            }
        }
    }
}

impl core::fmt::LowerHex for RegionFlags {
    /// The raw value in hex, as a log line or a census writes flags.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::LowerHex::fmt(&self.bits, f)
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
