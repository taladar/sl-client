//! What each live grid was measured to answer, and the check that holds every
//! grid — live or fake — to it.
//!
//! The `gridspec-*` work measures a behaviour on Second Life (on aditi) and on
//! OpenSim, writes the answer into the book's *Grid Behaviour* part, and makes
//! the fake grid give the same answer per flavour. A case states what it
//! learned as a [`Measured`] constant beside the code that observes it:
//!
//! ```
//! use sl_conformance::measured::Measured;
//!
//! /// Whether a region names its voice backend in `SimulatorFeatures`.
//! const NAMES_VOICE_SERVER: Measured<bool> = Measured {
//!     second_life: true,
//!     opensim: false,
//!     source: "book/src/gridspec/region.md (simulator-features, 2026-10-04)",
//! };
//! ```
//!
//! and checks the observation with [`Measured::check`], which picks the answer
//! by [`Grid::behaves_like`]. That is the whole pairing:
//!
//! - a [`Grid::FakeSl`] run is held to the **aditi** answer and a
//!   [`Grid::FakeOpensim`] run to the **OpenSim** one, so the fake grid cannot
//!   drift from the measurement it was built to;
//! - a live run is held to its own grid's answer, so a grid that changes its
//!   behaviour — or a book table that was wrong — fails the case that measured
//!   it rather than leaving a stale table behind.
//!
//! The `source` names where the answer is written down (the book chapter, the
//! case and the date it was measured), and every failure quotes it, so the
//! person reading one knows which table to re-check.

use crate::context::TestFailure;
use crate::grid::Grid;

/// One behaviour's measured answer on each live grid.
///
/// `T` is whatever the case observes — a flag, a count, a field's text — and
/// the two answers are often equal: a behaviour the grids agree on is worth
/// holding the fake grid to as much as one they disagree on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measured<T> {
    /// What Second Life answered (measured on aditi).
    pub second_life: T,
    /// What the local OpenSim answered.
    pub opensim: T,
    /// Where the measurement is written down: the book chapter, and the case
    /// and date it was measured with.
    pub source: &'static str,
}

impl<T> Measured<T> {
    /// The answer `grid` is held to: its own for a live grid, the one it
    /// imitates for a fake one.
    #[must_use]
    pub const fn on(&self, grid: Grid) -> &T {
        match grid.behaves_like() {
            sl_fake_grid::ImitatedGrid::SecondLife => &self.second_life,
            sl_fake_grid::ImitatedGrid::OpenSim => &self.opensim,
        }
    }

    /// Hold the observation `actual` of `what` on `grid` to the measured
    /// answer.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Assertion`] naming the grid, the grid it behaves
    /// like, both values and [`Measured::source`] when they differ.
    pub fn check(&self, what: &str, grid: Grid, actual: &T) -> Result<(), TestFailure>
    where
        T: PartialEq + core::fmt::Debug,
    {
        let expected = self.on(grid);
        if actual == expected {
            return Ok(());
        }
        let held_to = match grid.behaves_like() {
            sl_fake_grid::ImitatedGrid::SecondLife => "Second Life",
            sl_fake_grid::ImitatedGrid::OpenSim => "OpenSim",
        };
        // A live grid disagreeing is news about the grid (or about the table); a
        // fake one disagreeing is a fake grid that drifted from it. The message
        // says which, since the fix is in a different place.
        let verdict = if grid.is_fake() {
            "the fake grid no longer imitates"
        } else {
            "the grid no longer matches"
        };
        Err(TestFailure::Assertion(format!(
            "{what} on {grid}: got {actual:?}, but {held_to} was measured answering \
             {expected:?} — {verdict} {}",
            self.source
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::Measured;
    use crate::grid::Grid;
    use pretty_assertions::assert_eq;

    /// A behaviour the two grids disagree on.
    const DIVERGENT: Measured<&str> = Measured {
        second_life: "bulk",
        opensim: "legacy",
        source: "a test table",
    };

    /// Each grid is held to the answer of the grid it behaves like: the fake
    /// flavours to the live grid they imitate.
    #[test]
    fn each_grid_is_held_to_the_grid_it_behaves_like() {
        assert_eq!(*DIVERGENT.on(Grid::Aditi), "bulk");
        assert_eq!(*DIVERGENT.on(Grid::FakeSl), "bulk");
        assert_eq!(*DIVERGENT.on(Grid::Opensim), "legacy");
        assert_eq!(*DIVERGENT.on(Grid::FakeOpensim), "legacy");
    }

    /// A match passes on every grid.
    #[test]
    fn a_matching_observation_passes() {
        for (grid, observed) in [
            (Grid::Aditi, "bulk"),
            (Grid::FakeSl, "bulk"),
            (Grid::Opensim, "legacy"),
            (Grid::FakeOpensim, "legacy"),
        ] {
            assert!(
                DIVERGENT.check("the announcement", grid, &observed).is_ok(),
                "{grid} answering {observed} should match"
            );
        }
    }

    /// A mismatch names the grid, both answers and the source, and says
    /// whether it is the grid or the fake grid that moved.
    #[test]
    fn a_mismatch_names_both_answers_and_the_source() -> Result<(), String> {
        let Err(fake) = DIVERGENT
            .check("the announcement", Grid::FakeSl, &"legacy")
            .map_err(|failure| failure.to_string())
        else {
            return Err("a Second-Life-flavoured grid answering legacy should fail".to_owned());
        };
        for needle in [
            "fake-sl",
            "\"legacy\"",
            "\"bulk\"",
            "Second Life",
            "a test table",
            "fake grid",
        ] {
            assert!(fake.contains(needle), "{fake:?} lacks {needle:?}");
        }
        let Err(live) = DIVERGENT
            .check("the announcement", Grid::Opensim, &"bulk")
            .map_err(|failure| failure.to_string())
        else {
            return Err("OpenSim answering bulk should fail".to_owned());
        };
        assert!(live.contains("the grid no longer matches"), "{live:?}");
        Ok(())
    }
}
