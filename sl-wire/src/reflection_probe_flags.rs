//! The PBR reflection-probe flag byte (`LLReflectionProbeParams::EFlags`) an
//! object carries in its `ExtraParams` block.
//!
//! Bit meanings match the viewer's `indra/llprimitive/llprimitive.h`. Like the
//! parcel/region/control/permission flags, these live here rather than being
//! reconstructed into a handful of `bool`s so the named bits are a single typed
//! value and any future bit is preserved on the wire rather than silently
//! dropped.

use core::ops::{BitAnd, BitOr, BitOrAssign, Not};

/// A Second Life reflection-probe flag set — the `FLAG_*` bitfield in the
/// `LLReflectionProbeParams` `ExtraParams` entry. Combine bits with `|`; query
/// with [`ReflectionProbeFlags::contains`].
///
/// The wire field is a single `u8`. Storing the raw byte (rather than three
/// reconstructed booleans) keeps the value byte-identical across a decode/encode
/// round trip even if the simulator sets bits the viewer does not yet name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ReflectionProbeFlags {
    /// The raw flag bits.
    bits: u8,
}

impl ReflectionProbeFlags {
    /// The influence volume is a box rather than a sphere (`FLAG_BOX_VOLUME`).
    pub const BOX_VOLUME: Self = Self { bits: 0x01 };
    /// Dynamic objects (e.g. avatars) are rendered into the probe
    /// (`FLAG_DYNAMIC`).
    pub const DYNAMIC: Self = Self { bits: 0x02 };
    /// The probe drives a realtime mirror (`FLAG_MIRROR`).
    pub const MIRROR: Self = Self { bits: 0x04 };

    /// The empty flag set.
    #[must_use]
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    /// Builds a flag set from a raw value.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self { bits }
    }

    /// Returns the raw flag bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
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

impl BitOr for ReflectionProbeFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitOrAssign for ReflectionProbeFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.bits |= rhs.bits;
    }
}

impl BitAnd for ReflectionProbeFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self {
            bits: self.bits & rhs.bits,
        }
    }
}

impl Not for ReflectionProbeFlags {
    type Output = Self;
    fn not(self) -> Self {
        Self { bits: !self.bits }
    }
}

#[cfg(test)]
mod tests {
    use super::ReflectionProbeFlags;
    use pretty_assertions::assert_eq;

    #[test]
    fn named_bits_match_the_viewer_constants() {
        // The raw values from `indra/llprimitive/llprimitive.h`'s
        // `LLReflectionProbeParams::EFlags`.
        assert_eq!(ReflectionProbeFlags::BOX_VOLUME.bits(), 0x01);
        assert_eq!(ReflectionProbeFlags::DYNAMIC.bits(), 0x02);
        assert_eq!(ReflectionProbeFlags::MIRROR.bits(), 0x04);
    }

    #[test]
    fn round_trips_every_raw_value_bit_identically() {
        // Including bits the viewer does not name (0x08, 0xff), which the byte
        // form must still preserve.
        for raw in [0u8, 0x01, 0x03, 0x07, 0x08, 0xff] {
            assert_eq!(ReflectionProbeFlags::from_bits(raw).bits(), raw);
        }
    }

    #[test]
    fn contains_and_combinators_behave() {
        let flags = ReflectionProbeFlags::BOX_VOLUME | ReflectionProbeFlags::MIRROR;
        assert!(flags.contains(ReflectionProbeFlags::BOX_VOLUME));
        assert!(flags.contains(ReflectionProbeFlags::MIRROR));
        assert!(!flags.contains(ReflectionProbeFlags::DYNAMIC));
        assert!(flags.difference(ReflectionProbeFlags::MIRROR) == ReflectionProbeFlags::BOX_VOLUME);
        assert!(ReflectionProbeFlags::empty().is_empty());
        assert!(!flags.is_empty());
    }
}
