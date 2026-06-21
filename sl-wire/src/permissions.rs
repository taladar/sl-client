//! The Second Life object/inventory permission bitfield (`PERM_*`) and the
//! five-mask permission block (`Permissions5`) that every owned object or
//! inventory item carries.
//!
//! Bit meanings match the viewer's `indra/llinventory/llpermissionsflags.h`.
//! Like the parcel/region/control flags, these live here rather than on the
//! generated message structs because the generated code is regenerated on every
//! build and cannot carry hand-written constants, yet the bit meanings are a
//! fixed part of the wire protocol.

use core::ops::{BitAnd, BitOr, BitOrAssign, Not};

/// A Second Life permission mask — the `PERM_*` bitfield applied per
/// permission-holder (base / owner / group / everyone / next-owner). Combine
/// bits with `|`; query with [`Permissions::contains`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Permissions {
    /// The raw permission bits.
    bits: u32,
}

impl Permissions {
    /// No permissions (`PERM_NONE`).
    pub const NONE: Self = Self { bits: 0 };
    /// The object/item may be transferred to another owner (`PERM_TRANSFER`).
    pub const TRANSFER: Self = Self { bits: 1 << 13 };
    /// The object/item may be modified (`PERM_MODIFY`).
    pub const MODIFY: Self = Self { bits: 1 << 14 };
    /// The object/item may be copied (`PERM_COPY`).
    pub const COPY: Self = Self { bits: 1 << 15 };
    /// The object/item may be exported from the grid (`PERM_EXPORT`).
    pub const EXPORT: Self = Self { bits: 1 << 16 };
    /// The object may be moved (`PERM_MOVE`).
    pub const MOVE: Self = Self { bits: 1 << 19 };
    /// Combat damage may be applied (`PERM_DAMAGE`).
    pub const DAMAGE: Self = Self { bits: 1 << 20 };
    /// The reserved high bit (`PERM_RESERVED`).
    pub const RESERVED: Self = Self { bits: 1 << 31 };
    /// All permissions (`PERM_ALL`).
    pub const ALL: Self = Self { bits: 0x7fff_ffff };
    /// The unrestricted-item shorthand (`PERM_ITEM_UNRESTRICTED` =
    /// modify | copy | transfer).
    pub const ITEM_UNRESTRICTED: Self = Self {
        bits: (1 << 14) | (1 << 15) | (1 << 13),
    };

    /// The empty permission set.
    #[must_use]
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    /// Builds permissions from a raw value.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Returns the raw permission bits.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.bits
    }

    /// Returns `true` if every bit in `other` is set in `self`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.bits & other.bits == other.bits
    }

    /// Returns `true` if no bits are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    /// Returns `self` with the bits in `other` set.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }

    /// Returns `self` with the bits in `other` cleared.
    #[must_use]
    pub const fn difference(self, other: Self) -> Self {
        Self {
            bits: self.bits & !other.bits,
        }
    }
}

impl BitOr for Permissions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitOrAssign for Permissions {
    fn bitor_assign(&mut self, rhs: Self) {
        self.bits |= rhs.bits;
    }
}

impl BitAnd for Permissions {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self {
            bits: self.bits & rhs.bits,
        }
    }
}

impl Not for Permissions {
    type Output = Self;
    fn not(self) -> Self {
        Self { bits: !self.bits }
    }
}

/// The complete five-mask permission block an owned object or inventory item
/// carries on the wire (`LLPermissions`): the base mask plus the masks granted
/// to the owner, the group, everyone, and the next owner. Grouping the five into
/// one named struct keeps them from being scattered as five same-typed fields a
/// caller could transpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Permissions5 {
    /// The base permission mask — the ceiling the other masks are clamped to.
    pub base: Permissions,
    /// The permissions granted to the current owner.
    pub owner: Permissions,
    /// The permissions granted to the object/item's group.
    pub group: Permissions,
    /// The permissions granted to everyone.
    pub everyone: Permissions,
    /// The permissions the next owner will receive on transfer.
    pub next_owner: Permissions,
}

impl Permissions5 {
    /// An all-zero permission block.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            base: Permissions::NONE,
            owner: Permissions::NONE,
            group: Permissions::NONE,
            everyone: Permissions::NONE,
            next_owner: Permissions::NONE,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Permissions, Permissions5};
    use pretty_assertions::assert_eq;

    #[test]
    fn named_bits_match_the_viewer_constants() {
        // The raw values from `indra/llinventory/llpermissionsflags.h`.
        assert_eq!(Permissions::TRANSFER.bits(), 0x0000_2000);
        assert_eq!(Permissions::MODIFY.bits(), 0x0000_4000);
        assert_eq!(Permissions::COPY.bits(), 0x0000_8000);
        assert_eq!(Permissions::EXPORT.bits(), 0x0001_0000);
        assert_eq!(Permissions::MOVE.bits(), 0x0008_0000);
        assert_eq!(Permissions::DAMAGE.bits(), 0x0010_0000);
        assert_eq!(Permissions::RESERVED.bits(), 0x8000_0000);
        assert_eq!(Permissions::ALL.bits(), 0x7fff_ffff);
        assert_eq!(
            Permissions::ITEM_UNRESTRICTED,
            Permissions::MODIFY | Permissions::COPY | Permissions::TRANSFER
        );
    }

    #[test]
    fn round_trips_every_raw_value_bit_identically() {
        // A spread of values, including the typical "copy+modify+transfer for the
        // next owner" mask the viewer sends and the all-bits-set base mask.
        for raw in [0u32, 0x0008_2000, 0x0008_e000, 0x7fff_ffff, 0xffff_ffff] {
            assert_eq!(Permissions::from_bits(raw).bits(), raw);
        }
    }

    #[test]
    fn contains_and_combinators_behave() {
        let perms = Permissions::MODIFY | Permissions::COPY;
        assert!(perms.contains(Permissions::MODIFY));
        assert!(perms.contains(Permissions::COPY));
        assert!(!perms.contains(Permissions::TRANSFER));
        assert!(perms.difference(Permissions::COPY) == Permissions::MODIFY);
        assert!(Permissions::empty().is_empty());
        assert!(!perms.is_empty());
    }

    #[test]
    fn permissions5_groups_the_five_masks() {
        let block = Permissions5 {
            base: Permissions::ALL,
            owner: Permissions::MOVE,
            group: Permissions::NONE,
            everyone: Permissions::COPY,
            next_owner: Permissions::ITEM_UNRESTRICTED,
        };
        // The five masks are independently addressable and survive a bit round
        // trip through the wire representation.
        assert_eq!(block.base.bits(), 0x7fff_ffff);
        assert_eq!(block.owner.bits(), 0x0008_0000);
        assert_eq!(block.everyone, Permissions::COPY);
        assert_eq!(Permissions5::empty().base, Permissions::NONE);
    }
}
