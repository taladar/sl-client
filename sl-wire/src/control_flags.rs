//! The `AgentControlFlags` bitfield carried in `AgentUpdate.ControlFlags`, which
//! the simulator uses to move and steer the agent. Bit meanings match the
//! viewer's `indra/llcommon/indra_constants.h` (`AGENT_CONTROL_*`).
//!
//! Like the parcel/region flags, these live here rather than on the generated
//! message struct because the generated code is regenerated on every build and
//! cannot carry hand-written constants.

use core::ops::{BitAnd, BitOr, BitOrAssign, Not};

/// The agent movement/control bitfield sent in `AgentUpdate.ControlFlags`.
/// Combine bits with `|`; the simulator drives the avatar accordingly (e.g.
/// [`ControlFlags::AT_POS`] walks forward in the direction of the body rotation,
/// `| `[`ControlFlags::FLY`] flies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControlFlags {
    /// The raw flags value.
    bits: u32,
}

impl ControlFlags {
    /// No controls pressed.
    pub const NONE: Self = Self { bits: 0 };
    /// Move forward (walk/run in the body-rotation direction).
    pub const AT_POS: Self = Self { bits: 0x0000_0001 };
    /// Move backward.
    pub const AT_NEG: Self = Self { bits: 0x0000_0002 };
    /// Strafe left.
    pub const LEFT_POS: Self = Self { bits: 0x0000_0004 };
    /// Strafe right.
    pub const LEFT_NEG: Self = Self { bits: 0x0000_0008 };
    /// Move up (jump, or ascend while flying).
    pub const UP_POS: Self = Self { bits: 0x0000_0010 };
    /// Move down (crouch, or descend while flying).
    pub const UP_NEG: Self = Self { bits: 0x0000_0020 };
    /// Pitch up.
    pub const PITCH_POS: Self = Self { bits: 0x0000_0040 };
    /// Pitch down.
    pub const PITCH_NEG: Self = Self { bits: 0x0000_0080 };
    /// Yaw left (turn left).
    pub const YAW_POS: Self = Self { bits: 0x0000_0100 };
    /// Yaw right (turn right).
    pub const YAW_NEG: Self = Self { bits: 0x0000_0200 };
    /// Move forward fast (run).
    pub const FAST_AT: Self = Self { bits: 0x0000_0400 };
    /// Strafe fast.
    pub const FAST_LEFT: Self = Self { bits: 0x0000_0800 };
    /// Move up fast.
    pub const FAST_UP: Self = Self { bits: 0x0000_1000 };
    /// Fly.
    pub const FLY: Self = Self { bits: 0x0000_2000 };
    /// Stop (brake).
    pub const STOP: Self = Self { bits: 0x0000_4000 };
    /// Finish the current animation.
    pub const FINISH_ANIM: Self = Self { bits: 0x0000_8000 };
    /// Stand up (from sitting).
    pub const STAND_UP: Self = Self { bits: 0x0001_0000 };
    /// Sit on the ground where standing.
    pub const SIT_ON_GROUND: Self = Self { bits: 0x0002_0000 };
    /// Enter mouselook (first-person).
    pub const MOUSELOOK: Self = Self { bits: 0x0004_0000 };
    /// A single forward nudge (one step).
    pub const NUDGE_AT_POS: Self = Self { bits: 0x0008_0000 };
    /// A single backward nudge.
    pub const NUDGE_AT_NEG: Self = Self { bits: 0x0010_0000 };
    /// A single left-strafe nudge.
    pub const NUDGE_LEFT_POS: Self = Self { bits: 0x0020_0000 };
    /// A single right-strafe nudge.
    pub const NUDGE_LEFT_NEG: Self = Self { bits: 0x0040_0000 };
    /// A single up nudge.
    pub const NUDGE_UP_POS: Self = Self { bits: 0x0080_0000 };
    /// A single down nudge.
    pub const NUDGE_UP_NEG: Self = Self { bits: 0x0100_0000 };
    /// Turn left (avatar body turns, distinct from camera yaw).
    pub const TURN_LEFT: Self = Self { bits: 0x0200_0000 };
    /// Turn right.
    pub const TURN_RIGHT: Self = Self { bits: 0x0400_0000 };
    /// Mark the agent as away.
    pub const AWAY: Self = Self { bits: 0x0800_0000 };
    /// Left mouse button down.
    pub const LBUTTON_DOWN: Self = Self { bits: 0x1000_0000 };
    /// Left mouse button up.
    pub const LBUTTON_UP: Self = Self { bits: 0x2000_0000 };
    /// Left mouse button down in mouselook.
    pub const ML_LBUTTON_DOWN: Self = Self { bits: 0x4000_0000 };
    /// Left mouse button up in mouselook.
    pub const ML_LBUTTON_UP: Self = Self { bits: 0x8000_0000 };

    /// No controls pressed (the empty set).
    #[must_use]
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

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

impl BitOr for ControlFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl BitOrAssign for ControlFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.bits |= rhs.bits;
    }
}

impl BitAnd for ControlFlags {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self {
            bits: self.bits & rhs.bits,
        }
    }
}

impl Not for ControlFlags {
    type Output = Self;
    fn not(self) -> Self {
        Self { bits: !self.bits }
    }
}
