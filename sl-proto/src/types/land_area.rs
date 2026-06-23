//! Land area as a count of square metres.

/// A Second Life land area, in **square metres** — the unit SL measures parcels
/// and land-tier accounting in (a member's group land contribution, a parcel's
/// actual/billable area, an avatar's land credit/commitment, …).
///
/// This is deliberately **not** an L$ amount: the wire carries land areas in the
/// same signed-32-bit integer slots prices use, and the two were trivially
/// confusable as raw `i32`s. Wrapping area in its own newtype makes "passed a
/// land area where an L$ price was expected" (and vice-versa) a compile error.
///
/// A land area is non-negative by construction (a `u32`), so an illegal negative
/// area is unrepresentable; the codec boundary rejects a negative wire value
/// rather than masking it to `0`.
///
/// This newtype is kept **client-local** to this workspace for now (it lives in
/// `sl-proto`, not the shared `sl-types`); it — along with the other value types
/// adopted in this hardening pass — is slated to move to `sl-types` in a single
/// later update, to avoid churning the shared crate's version.
#[derive(
    Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct LandArea(pub u32);

impl LandArea {
    /// A zero land area.
    pub const ZERO: Self = Self(0);

    /// The wrapped count of square metres.
    #[must_use]
    pub const fn get(&self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for LandArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self(value) = self;
        write!(f, "{value} m²")
    }
}

impl std::ops::Add for LandArea {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        let Self(lhs) = self;
        let Self(rhs) = rhs;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "the same overflow behaviour as the underlying integer addition, which is what a caller summing areas expects"
        )]
        Self(lhs + rhs)
    }
}

impl std::ops::Sub for LandArea {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        let Self(lhs) = self;
        let Self(rhs) = rhs;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "the same underflow behaviour as the underlying integer subtraction, which is what a caller differencing areas expects"
        )]
        Self(lhs - rhs)
    }
}
