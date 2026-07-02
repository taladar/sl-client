//! Scheduling priority and the diminishing popularity boost.
//!
//! [`Priority`] is an opaque urgency value the stores order work by; how a caller
//! derives it (expected users, on-screen, distance, size on screen) is out of
//! scope. [`popularity_boost`] is the diminishing bonus a store adds for the
//! number of distinct requesters, so an asset used by many objects outranks one
//! used by few at the same base priority.

/// An abstract scheduling priority: higher is more urgent. How a caller derives
/// it (expected users, on-screen, distance, size on screen) is out of scope —
/// a store only combines and orders by the opaque value.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Priority(u32);

impl Priority {
    /// The lowest priority (background / idle work).
    pub const IDLE: Self = Self(0);

    /// A priority from a raw urgency value.
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// The raw urgency value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// The higher of two priorities — the *base* an entry's effective priority
    /// is built from. A store's full effective priority also adds a popularity
    /// boost for the number of requesters (see [`popularity_boost`]), so an
    /// asset used by many on-screen objects outranks one used by few at the same
    /// base priority.
    #[must_use]
    pub fn combine(first: Self, second: Self) -> Self {
        Self(first.0.max(second.0))
    }
}

/// The popularity boost added per doubling of the requester count. An asset
/// requested by `n` distinct on-screen uses is boosted by
/// `floor(log2(n)) * POPULARITY_BOOST_SCALE` over its base (max) priority, so
/// the boost grows with popularity but with diminishing returns.
pub const POPULARITY_BOOST_SCALE: u32 = 4;

/// The diminishing popularity boost for `count` concurrent requesters:
/// `floor(log2(count)) * POPULARITY_BOOST_SCALE` (0 for a single requester).
#[must_use]
pub fn popularity_boost(count: usize) -> u32 {
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    if count == 0 {
        return 0;
    }
    count.ilog2().saturating_mul(POPULARITY_BOOST_SCALE)
}

#[cfg(test)]
mod tests {
    use super::{Priority, popularity_boost};
    use pretty_assertions::assert_eq;

    #[test]
    fn priority_combine_takes_the_maximum() {
        assert_eq!(
            Priority::combine(Priority::new(3), Priority::new(7)),
            Priority::new(7)
        );
        assert_eq!(Priority::combine(Priority::IDLE, Priority::new(1)).get(), 1);
    }

    #[test]
    fn popularity_boost_grows_with_diminishing_returns() {
        assert_eq!(popularity_boost(1), 0);
        assert_eq!(popularity_boost(2), 4);
        assert_eq!(popularity_boost(4), 8);
        assert_eq!(popularity_boost(8), 12);
        assert_eq!(popularity_boost(16), 16);
        // Between doublings the boost is flat (7 requesters boost as 4).
        assert_eq!(popularity_boost(7), 8);
        // A zero count contributes nothing.
        assert_eq!(popularity_boost(0), 0);
    }
}
