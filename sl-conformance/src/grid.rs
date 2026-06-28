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
}

impl Grid {
    /// Every grid, in declaration order — used by the reporter to lay out
    /// columns deterministically.
    pub const ALL: [Self; 2] = [Self::Opensim, Self::Aditi];

    /// The on-disk directory name (under `records/`) holding this grid's
    /// records.
    #[must_use]
    pub const fn dir_name(self) -> &'static str {
        match self {
            Self::Opensim => "opensim",
            Self::Aditi => "aditi",
        }
    }

    /// Whether logins to this grid are rate-limited enough to warrant the
    /// per-avatar cooldown guard (and, in practice, require MFA).
    #[must_use]
    pub const fn needs_cooldown(self) -> bool {
        matches!(self, Self::Aditi)
    }

    /// The default XML-RPC login URI used when the credentials entry for the
    /// chosen avatar does not specify one.
    #[must_use]
    pub const fn default_login_uri(self) -> &'static str {
        match self {
            Self::Opensim => "http://127.0.0.1:9000/",
            // Second Life Beta (aditi).
            Self::Aditi => "https://login.aditi.lindenlab.com/cgi-bin/login.cgi",
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
        assert!(!Grid::Opensim.needs_cooldown());
        assert!(Grid::Aditi.needs_cooldown());
        assert_eq!(format!("{}", Grid::Aditi), "aditi");
    }
}
