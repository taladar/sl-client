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

    /// Maps an on-screen pixel area (from
    /// [`ScreenMetrics::pixel_area`](crate::ScreenMetrics::pixel_area)) to a
    /// scheduling priority, mirroring the reference viewer where a texture's
    /// decode priority is its maximum on-screen virtual size
    /// (`LLViewerFetchedTexture::calcDecodePriority` returns `mMaxVirtualSize`,
    /// the largest pixel area any visible face using the texture covered this
    /// frame). The area is clamped to `[0, FULL_RESOLUTION_PIXEL_AREA]` — a
    /// non-finite or negative area maps to [`IDLE`](Self::IDLE) — and rounded to
    /// the nearest whole pixel, so a larger / closer object outranks a smaller /
    /// farther one, and everything above a full-resolution image ties at the top.
    #[must_use]
    pub fn from_pixel_area(pixel_area: f32) -> Self {
        Self(round_pixel_area(pixel_area))
    }
}

/// Rounds a clamped, non-negative pixel area to the nearest `u32` priority value.
/// A non-finite or non-positive area is [`Priority::IDLE`] (`0`); anything above
/// [`FULL_RESOLUTION_PIXEL_AREA`] saturates there. There is no `f32 → u32`
/// conversion without a cast, and the clamped input (`0..=4_194_304`) fits a
/// `u32` exactly, so the cast lints are expected here.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the area is clamped to [0, FULL_RESOLUTION_PIXEL_AREA] (~4.19M) and rounded, so it is non-negative and fits a u32 exactly"
)]
fn round_pixel_area(pixel_area: f32) -> u32 {
    if !pixel_area.is_finite() || pixel_area <= 0.0 {
        return 0;
    }
    pixel_area.clamp(0.0, FULL_RESOLUTION_PIXEL_AREA).round() as u32
}

/// The on-screen pixel area of a full 2048×2048 texture (`2048 * 2048`).
///
/// The reference viewer forces any texture boosted to `BOOST_HIGH` or above to
/// fetch at full resolution by pinning its decode priority to this pixel area
/// (`LLViewerTexture::addTextureStats`), and it is the largest pixel area worth
/// distinguishing — [`Priority::from_pixel_area`] saturates here. A caller that
/// wants to force a boosted asset (the own avatar, an attachment, the UI) to the
/// front of the queue maps this area through [`Priority::from_pixel_area`].
pub const FULL_RESOLUTION_PIXEL_AREA: f32 = 2048.0 * 2048.0;

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
    use super::{FULL_RESOLUTION_PIXEL_AREA, Priority, popularity_boost};
    use pretty_assertions::assert_eq;

    #[test]
    fn from_pixel_area_is_monotonic_and_saturates() {
        // A degenerate area is the lowest priority; a larger area outranks a
        // smaller one; anything at or above a full-resolution image ties at the
        // top (the reference viewer's `mMaxVirtualSize` cap).
        assert_eq!(Priority::from_pixel_area(0.0), Priority::IDLE);
        assert_eq!(Priority::from_pixel_area(-5.0), Priority::IDLE);
        assert_eq!(Priority::from_pixel_area(f32::NAN), Priority::IDLE);
        assert!(Priority::from_pixel_area(100.0) < Priority::from_pixel_area(101.0));
        assert!(Priority::from_pixel_area(100.0) > Priority::from_pixel_area(1.0));
        // Rounds to the nearest whole pixel.
        assert_eq!(Priority::from_pixel_area(41.4).get(), 41);
        assert_eq!(Priority::from_pixel_area(41.6).get(), 42);
        // Saturates at (and never exceeds) the full-resolution area.
        let full = Priority::from_pixel_area(FULL_RESOLUTION_PIXEL_AREA);
        assert_eq!(full.get(), 2048 * 2048);
        assert_eq!(
            Priority::from_pixel_area(FULL_RESOLUTION_PIXEL_AREA * 4.0),
            full
        );
        assert_eq!(Priority::from_pixel_area(f32::INFINITY), Priority::IDLE);
    }

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
