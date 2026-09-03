//! The grids a conformance test can target.

/// A grid the conformance harness can run a test against.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, clap::ValueEnum, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[clap(rename_all = "lowercase")]
pub enum Grid {
    /// The local OpenSim standalone grid (default login URI
    /// `http://127.0.0.1:9000/`).
    Opensim,
    /// Second Life Beta, the "aditi" grid (requires MFA; rate-limited).
    Aditi,
    /// The offline [`sl-fake-grid`](sl_fake_grid) started inside this process,
    /// on ephemeral ports, serving the shared fixture catalogue.
    ///
    /// Unlike the two live grids this one needs no credentials file, no
    /// network and no cooldown: [`crate::fake::FakeGridHarness`] stands it up,
    /// synthesises the accounts and hands out the login URI it bound. That is
    /// what lets the cases in [`crate::fake::OFFLINE_CASES`] run as plain
    /// `cargo test`.
    Fake,
}

impl Grid {
    /// The grids whose runs are **recorded** under `records/`, in declaration
    /// order — the columns the reporter lays out when it is not told which grid
    /// to show.
    ///
    /// [`Fake`](Self::Fake) is deliberately absent. Its cases are asserted on
    /// every `cargo test` (see [`crate::fake`]), so a committed record of them
    /// would be a second, staler copy of an answer the test suite already
    /// gives; the reporter still renders the column on an explicit
    /// `--grid fake` if someone runs the runner against it.
    pub const RECORDED: [Self; 2] = [Self::Opensim, Self::Aditi];

    /// The on-disk directory name (under `records/`) holding this grid's
    /// records.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Opensim => "opensim",
            Self::Aditi => "aditi",
            Self::Fake => "fake",
        }
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
    /// The fake grid is the `None`: it binds an ephemeral port at start-up, so
    /// its address is only known to whoever started it — which is why
    /// [`crate::fake::FakeGridHarness`] writes the URI it bound into the
    /// credentials it synthesises.
    #[must_use]
    pub const fn default_login_uri(self) -> Option<&'static str> {
        match self {
            Self::Opensim => Some("http://127.0.0.1:9000/"),
            // Second Life Beta (aditi).
            Self::Aditi => Some("https://login.aditi.lindenlab.com/cgi-bin/login.cgi"),
            Self::Fake => None,
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
        assert_eq!(Grid::Fake.dir_name(), "fake");
        assert!(!Grid::Opensim.needs_cooldown());
        assert!(Grid::Aditi.needs_cooldown());
        assert!(!Grid::Fake.needs_cooldown());
        assert_eq!(format!("{}", Grid::Aditi), "aditi");
    }

    /// Only the fake grid has no fixed address, and it is the one grid the
    /// reporter does not lay a column out for by default.
    #[test]
    fn only_the_fake_grid_is_addressless_and_unrecorded() {
        assert!(Grid::Opensim.default_login_uri().is_some());
        assert!(Grid::Aditi.default_login_uri().is_some());
        assert_eq!(Grid::Fake.default_login_uri(), None);
        assert!(!Grid::RECORDED.contains(&Grid::Fake));
    }
}
