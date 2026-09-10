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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    /// The block a client must derive before *asserting* a permission set it
    /// assembled itself — the reference's `LLPermissions::fixFairUse()` followed
    /// by `LLPermissions::fix()`, which every `initMasks` call runs. Two rules
    /// the caller cannot skip:
    ///
    /// - **fair use**: you can never take away the right to move something you
    ///   own (`base |= MOVE`; a non-empty `next_owner |= MOVE`), nor the right
    ///   to transfer something you cannot otherwise copy (a `base` without
    ///   `COPY` gains `TRANSFER`);
    /// - **clamping**: `owner` to `base`, `group` to `owner`, `next_owner` to
    ///   `base` (not to `owner` — a locked object may still be sellable), and
    ///   `everyone` to `owner` *minus* `MODIFY`. A no-transfer, non-group-owned
    ///   item additionally loses `COPY` for the group and for everyone;
    ///   `next_owner` is deliberately left alone there, because an over-tight
    ///   next-owner mask survives a rez-time ownership transfer as the item's
    ///   permanent permissions.
    ///
    /// `group_owned` is the item's ownership flag (`mIsGroupOwned`), which the
    /// mask block itself does not carry.
    #[must_use]
    pub fn fair_use_fixed(self, group_owned: bool) -> Self {
        let mut base = self.base | Permissions::MOVE;
        if !base.contains(Permissions::COPY) {
            base |= Permissions::TRANSFER;
        }
        let mut next_owner = self.next_owner;
        if !next_owner.is_empty() {
            next_owner |= Permissions::MOVE;
        }
        let owner = self.owner & base;
        let mut group = self.group & owner;
        let mut everyone = (self.everyone & owner).difference(Permissions::MODIFY);
        next_owner = next_owner & base;
        if !base.contains(Permissions::TRANSFER) && !group_owned {
            group = group.difference(Permissions::COPY);
            everyone = everyone.difference(Permissions::COPY);
        }
        Self {
            base,
            owner,
            group,
            everyone,
            next_owner,
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

    #[test]
    fn fair_use_grants_move_and_clamps_everyone() {
        // The block a fresh upload asserts: everything for the owner, a
        // next-owner mask of copy|modify|transfer, nothing for anyone else.
        let fixed = Permissions5 {
            base: Permissions::ALL,
            owner: Permissions::ALL,
            group: Permissions::NONE,
            everyone: Permissions::NONE,
            next_owner: Permissions::ITEM_UNRESTRICTED,
        }
        .fair_use_fixed(false);
        // A next owner who gets anything at all gets the right to move it.
        assert_eq!(
            fixed.next_owner,
            Permissions::ITEM_UNRESTRICTED | Permissions::MOVE
        );
        assert_eq!(fixed.base, Permissions::ALL);
        assert_eq!(fixed.owner, Permissions::ALL);
        // Everyone never holds modify, however it was asked for.
        let everyone = Permissions5 {
            base: Permissions::ALL,
            owner: Permissions::ALL,
            group: Permissions::NONE,
            everyone: Permissions::MODIFY | Permissions::COPY,
            next_owner: Permissions::NONE,
        }
        .fair_use_fixed(false);
        assert_eq!(everyone.everyone, Permissions::COPY);
        // An empty next-owner mask stays empty (it is how "no transfer" is said).
        assert_eq!(everyone.next_owner, Permissions::NONE);
    }

    #[test]
    fn fair_use_forces_transfer_on_a_no_copy_base() {
        // "You can never take away the right to transfer something you cannot
        // otherwise copy."
        let fixed = Permissions5 {
            base: Permissions::MODIFY,
            owner: Permissions::MODIFY | Permissions::COPY,
            group: Permissions::NONE,
            everyone: Permissions::NONE,
            next_owner: Permissions::NONE,
        }
        .fair_use_fixed(false);
        assert!(fixed.base.contains(Permissions::TRANSFER));
        assert!(fixed.base.contains(Permissions::MOVE));
        // The owner mask is clamped to the (now transfer-bearing) base, which
        // still has no copy bit.
        assert!(!fixed.owner.contains(Permissions::COPY));
    }

    #[test]
    fn a_no_transfer_item_shares_no_copy_with_group_or_everyone() {
        // A copyable base without transfer keeps its own bits, but nobody else
        // gets to copy it away — unless the item is group owned, where the
        // group's copy right is the ownership itself.
        let block = Permissions5 {
            base: Permissions::COPY | Permissions::MODIFY,
            owner: Permissions::COPY | Permissions::MODIFY,
            group: Permissions::COPY,
            everyone: Permissions::COPY,
            next_owner: Permissions::NONE,
        };
        let personal = block.fair_use_fixed(false);
        assert!(!personal.base.contains(Permissions::TRANSFER));
        assert_eq!(personal.group, Permissions::NONE);
        assert_eq!(personal.everyone, Permissions::NONE);
        let group_owned = block.fair_use_fixed(true);
        assert_eq!(group_owned.group, Permissions::COPY);
        assert_eq!(group_owned.everyone, Permissions::COPY);
    }
}
