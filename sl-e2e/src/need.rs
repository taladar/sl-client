//! What a test needs of the grid it runs on, and the reason it is skipped
//! when the chosen grid cannot provide it.
//!
//! A test states its needs on the [`StageBuilder`](crate::StageBuilder):
//! explicitly with [`needs`](crate::StageBuilder::needs), and implicitly by
//! dictating the grid ([`region`](crate::StageBuilder::region),
//! [`configure_grid`](crate::StageBuilder::configure_grid),
//! [`start_position`](crate::StageBuilder::start_position)), which only a
//! grid the stage starts can obey. Every viewer is an account, so a live grid
//! must have as many as the stage has viewers.

use core::fmt;

use crate::grid::Grid;

/// Something a test needs that not every grid provides.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Need {
    /// The grid-control handle: [`Stage::grid`](crate::Stage::grid),
    /// [`agent`](crate::Stage::agent), [`mark`](crate::Stage::mark) and
    /// [`wait_marker`](crate::Stage::wait_marker).
    GridControl,
    /// The grid the stage dictates: the regions it names, their
    /// configuration, where its viewers start. Stated by the builder calls
    /// that dictate it, named here by the first one.
    DictatedGrid(&'static str),
    /// Content only the stage's own scene has, described.
    Content(&'static str),
    /// What only a live grid does yet, described — two viewers seeing each
    /// other's avatars, an IM from one reaching the other: the fake grid's
    /// side of it is a server task still to do.
    LiveGrid(&'static str),
    /// What only OpenSim does, described: a simulator feature Second Life
    /// leaves out, such as the viewer-side bake upload.
    OpenSim(&'static str),
}

impl Need {
    /// Whether `grid` provides it.
    #[must_use]
    pub const fn met_by(&self, grid: Grid) -> bool {
        match self {
            Self::GridControl | Self::DictatedGrid(_) | Self::Content(_) => !grid.is_live(),
            Self::LiveGrid(_) => grid.is_live(),
            Self::OpenSim(_) => matches!(grid, Grid::OpenSim),
        }
    }
}

impl fmt::Display for Need {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GridControl => f.write_str("grid control (the fake grid's handle)"),
            Self::DictatedGrid(how) => write!(f, "a grid it configures itself ({how})"),
            Self::Content(what) => write!(f, "content only its own scene has: {what}"),
            Self::LiveGrid(what) => write!(f, "a live grid: {what}"),
            Self::OpenSim(what) => write!(f, "OpenSim: {what}"),
        }
    }
}

/// Why a stage with `needs` and `viewers` viewers cannot run on `grid`
/// with `accounts` accounts available (`None` for the fake grid, which makes
/// one per viewer), or `None` when it can.
#[must_use]
pub fn unmet(
    needs: &[Need],
    viewers: usize,
    grid: Grid,
    accounts: Option<usize>,
) -> Option<String> {
    let mut reasons: Vec<String> = needs
        .iter()
        .filter(|need| !need.met_by(grid))
        .map(ToString::to_string)
        .collect();
    reasons.dedup();
    if let Some(accounts) = accounts
        && accounts < viewers
    {
        reasons.push(format!(
            "{viewers} accounts, and the credentials file has {accounts}"
        ));
    }
    (!reasons.is_empty()).then(|| format!("it needs {}", reasons.join("; ")))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Need, unmet};
    use crate::grid::Grid;

    /// The fake grid provides every need; a live grid none of them, and the
    /// skip names each one.
    #[test]
    fn a_live_grid_cannot_give_grid_control_and_says_so() {
        let needs = [Need::GridControl, Need::Content("a box to rez on")];
        assert_eq!(unmet(&needs, 2, Grid::Fake, None), None);
        assert_eq!(
            unmet(&needs, 1, Grid::OpenSim, Some(3)).as_deref(),
            Some(
                "it needs grid control (the fake grid's handle); content only its own scene has: \
                 a box to rez on"
            )
        );
    }

    /// What only a live grid does skips the fake grid, and what only OpenSim
    /// does skips the other two.
    #[test]
    fn a_live_only_need_skips_the_fake_grid_and_an_opensim_one_aditi() {
        let live = [Need::LiveGrid("an IM relayed between two viewers")];
        assert_eq!(
            unmet(&live, 2, Grid::Fake, None).as_deref(),
            Some("it needs a live grid: an IM relayed between two viewers")
        );
        assert_eq!(unmet(&live, 2, Grid::Aditi, Some(3)), None);
        let opensim = [Need::OpenSim("the viewer's own bake upload")];
        assert_eq!(unmet(&opensim, 1, Grid::OpenSim, Some(3)), None);
        assert_eq!(
            unmet(&opensim, 1, Grid::Aditi, Some(3)).as_deref(),
            Some("it needs OpenSim: the viewer's own bake upload")
        );
        assert!(unmet(&opensim, 1, Grid::Fake, None).is_some());
    }

    /// A test with no needs runs on a live grid while it has an account per
    /// viewer, and skips when it has too few.
    #[test]
    fn a_live_grid_needs_an_account_per_viewer() {
        assert_eq!(unmet(&[], 2, Grid::Aditi, Some(2)), None);
        assert_eq!(
            unmet(&[], 3, Grid::OpenSim, Some(2)).as_deref(),
            Some("it needs 3 accounts, and the credentials file has 2")
        );
    }
}
