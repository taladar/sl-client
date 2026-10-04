//! The grids a conformance test can target.

/// A grid the conformance harness can run a test against.
///
/// The fake grid is **two** of them, because it can be either live grid where
/// the two disagree ([`sl_fake_grid::ImitatedGrid`]). A case that behaves
/// differently on the two — `asset-round-trip` can only read a taken object's
/// asset back on the OpenSim-flavoured one — says so in
/// [`GridTest::grids`](crate::registry::GridTest::grids) the same way it says it
/// is meaningless on aditi, and a survey that finds something different on each
/// declares both and is run twice.
///
/// One flavour is *not* enough, and naming only the odd one out would be worse:
/// a grid called plainly "fake" reads as though it were flavourless, which is
/// how the fake grid ended up announcing `platform: OpenSim` while withholding
/// object assets like Second Life. Both are named.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, clap::ValueEnum, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
#[clap(rename_all = "kebab-case")]
pub enum Grid {
    /// The local OpenSim standalone grid (default login URI
    /// `http://127.0.0.1:9000/`).
    Opensim,
    /// Second Life Beta, the "aditi" grid (requires MFA; rate-limited).
    Aditi,
    /// The offline [`sl-fake-grid`](sl_fake_grid) started inside this process
    /// **imitating Second Life** — the flavour this workspace targets, and the
    /// fake grid's own default.
    ///
    /// Unlike the two live grids it needs no credentials file, no network and
    /// no cooldown: [`crate::fake::FakeGridHarness`] stands it up, synthesises
    /// the accounts and hands out the login URI it bound. That is what lets the
    /// cases in [`crate::fake::OFFLINE_CASES`] run as plain `cargo test`.
    FakeSl,
    /// The same offline grid **imitating OpenSim**: a taken object's asset is
    /// served rather than withheld, and a login response carries every field
    /// whatever the request's `options` asked for.
    FakeOpensim,
}

impl Grid {
    /// The columns the reporter lays out when it is not told which grid to
    /// show: each live grid **followed by the fake flavour imitating it**, so a
    /// case's live answer and the fake grid's sit side by side.
    ///
    /// The fake columns read records the runner writes on
    /// `sl-conformance run --grid fake-sl` (or `run-offline`); the offline
    /// `cargo test` writes none, since there the assertion is the record.
    pub const REPORTED: [Self; 4] = [Self::Opensim, Self::FakeOpensim, Self::Aditi, Self::FakeSl];

    /// The fake flavour imitating this live grid, or `None` for a fake grid —
    /// which is the imitation, not the thing imitated.
    #[must_use]
    pub const fn fake_twin(self) -> Option<Self> {
        match self {
            Self::Opensim => Some(Self::FakeOpensim),
            Self::Aditi => Some(Self::FakeSl),
            Self::FakeSl | Self::FakeOpensim => None,
        }
    }

    /// The on-disk directory name (under `records/`) holding this grid's
    /// records.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Opensim => "opensim",
            Self::Aditi => "aditi",
            Self::FakeSl => "fake-sl",
            Self::FakeOpensim => "fake-opensim",
        }
    }

    /// Which live grid this one *is*, for a fake grid the harness has to start,
    /// or `None` for a live grid — which behaves however it behaves and is not
    /// this workspace's to configure.
    #[must_use]
    pub const fn imitates(self) -> Option<sl_fake_grid::ImitatedGrid> {
        match self {
            Self::Opensim | Self::Aditi => None,
            Self::FakeSl => Some(sl_fake_grid::ImitatedGrid::SecondLife),
            Self::FakeOpensim => Some(sl_fake_grid::ImitatedGrid::OpenSim),
        }
    }

    /// Which of the two live grids this one **behaves like** where they
    /// disagree: itself, for a live grid, and the grid it imitates for a fake
    /// one.
    ///
    /// This is the question a case asks when it is surveying a divergence
    /// rather than the harness — "does this grid send `OpenSimExtras`", not "is
    /// this grid offline". [`imitates`](Self::imitates) answers the other one,
    /// and answers `None` for the live grids, which is exactly wrong for this:
    /// aditi is not a grid with no flavour, it is the flavour.
    #[must_use]
    pub const fn behaves_like(self) -> sl_fake_grid::ImitatedGrid {
        match self {
            Self::Opensim | Self::FakeOpensim => sl_fake_grid::ImitatedGrid::OpenSim,
            Self::Aditi | Self::FakeSl => sl_fake_grid::ImitatedGrid::SecondLife,
        }
    }

    /// Whether this is the offline fake grid, whichever grid it is imitating.
    ///
    /// The question almost every case asks — "is this a grid whose content and
    /// policies this workspace wrote" — is about the harness, not the flavour.
    #[must_use]
    pub const fn is_fake(self) -> bool {
        self.imitates().is_some()
    }

    /// Whether logins to this grid are rate-limited enough to warrant the
    /// per-avatar cooldown guard (and, in practice, require MFA).
    #[must_use]
    pub const fn needs_cooldown(self) -> bool {
        matches!(self, Self::Aditi)
    }

    /// The default XML-RPC login URI used when the credentials entry for the
    /// chosen avatar does not specify one, or `None` for a grid that has no
    /// fixed address.
    ///
    /// Both fake flavours are the `None`: each binds an ephemeral port at
    /// start-up, so its address is only known to whoever started it — which is
    /// why [`crate::fake::FakeGridHarness`] writes the URI it bound into the
    /// credentials it synthesises.
    #[must_use]
    pub const fn default_login_uri(self) -> Option<&'static str> {
        match self {
            Self::Opensim => Some("http://127.0.0.1:9000/"),
            // Second Life Beta (aditi).
            Self::Aditi => Some("https://login.aditi.lindenlab.com/cgi-bin/login.cgi"),
            Self::FakeSl | Self::FakeOpensim => None,
        }
    }
}

impl core::fmt::Display for Grid {
    /// Render the grid as its lowercase directory name.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.dir_name())
    }
}

#[cfg(test)]
mod tests {
    use super::Grid;
    use pretty_assertions::assert_eq;

    /// Directory names and cooldown gating are stable per grid.
    #[test]
    fn grid_properties() {
        assert_eq!(Grid::Opensim.dir_name(), "opensim");
        assert_eq!(Grid::Aditi.dir_name(), "aditi");
        assert_eq!(Grid::FakeSl.dir_name(), "fake-sl");
        assert_eq!(Grid::FakeOpensim.dir_name(), "fake-opensim");
        assert!(!Grid::Opensim.needs_cooldown());
        assert!(Grid::Aditi.needs_cooldown());
        assert!(!Grid::FakeSl.needs_cooldown());
        assert_eq!(format!("{}", Grid::Aditi), "aditi");
    }

    /// Only a fake grid has no fixed address.
    #[test]
    fn only_the_fake_grids_are_addressless() {
        assert!(Grid::Opensim.default_login_uri().is_some());
        assert!(Grid::Aditi.default_login_uri().is_some());
        for fake in [Grid::FakeSl, Grid::FakeOpensim] {
            assert_eq!(fake.default_login_uri(), None);
        }
    }

    /// Each live grid's twin is the fake flavour that behaves like it, and the
    /// report lays every live grid out immediately followed by its twin.
    #[test]
    fn each_live_grid_is_reported_beside_its_fake_twin() {
        for live in [Grid::Opensim, Grid::Aditi] {
            let twin = live.fake_twin();
            assert!(twin.is_some_and(Grid::is_fake), "{live} has no fake twin");
            assert_eq!(twin.map(Grid::behaves_like), Some(live.behaves_like()));
            let position = Grid::REPORTED.iter().position(|grid| *grid == live);
            let twin_position = Grid::REPORTED.iter().position(|grid| Some(*grid) == twin);
            assert_eq!(twin_position, position.map(|index| index.saturating_add(1)));
        }
        assert_eq!(Grid::FakeSl.fake_twin(), None);
        assert_eq!(Grid::FakeOpensim.fake_twin(), None);
    }

    /// A fake grid is exactly one that names the live grid it imitates, and the
    /// two flavours name different ones — which is what stops the harness from
    /// starting two grids that are secretly the same.
    #[test]
    fn a_fake_grid_is_one_that_imitates_something() {
        assert_eq!(Grid::Opensim.imitates(), None);
        assert_eq!(Grid::Aditi.imitates(), None);
        assert!(!Grid::Opensim.is_fake());
        assert!(!Grid::Aditi.is_fake());
        assert!(Grid::FakeSl.is_fake());
        assert!(Grid::FakeOpensim.is_fake());
        assert_eq!(
            Grid::FakeSl.imitates(),
            Some(sl_fake_grid::ImitatedGrid::SecondLife)
        );
        assert_eq!(
            Grid::FakeOpensim.imitates(),
            Some(sl_fake_grid::ImitatedGrid::OpenSim)
        );
        // The default flavour is the one the plain fake grid already was.
        assert_eq!(
            Grid::FakeSl.imitates(),
            Some(sl_fake_grid::ImitatedGrid::default())
        );
    }
}
