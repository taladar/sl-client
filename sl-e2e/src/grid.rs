//! Which grid a stage's viewers log into: a fresh fake grid in this process,
//! or a live one — chosen by the environment, never by rewriting a test.

use core::fmt;

use crate::error::StageError;

/// The environment variable that picks the grid a stage runs on.
pub const GRID_VARIABLE: &str = "SL_E2E_GRID";

/// The grid a stage runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grid {
    /// A fresh `sl-fake-grid` per backend, started in this process: the
    /// stage has the grid-control handle, and makes the accounts.
    Fake,
    /// The local OpenSim grid; accounts from its credentials file.
    OpenSim,
    /// Second Life Beta; accounts from its credentials file, each held to the
    /// shared login cooldown.
    Aditi,
}

impl Grid {
    /// The grid's name: in `SL_E2E_GRID`, in the artifact directory, in
    /// skips and errors.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Fake => "fake",
            Self::OpenSim => "opensim",
            Self::Aditi => "aditi",
        }
    }

    /// The grid `value` (an `SL_E2E_GRID` value) names; unset is the fake
    /// grid, so the pre-commit suite never logs into a live one.
    ///
    /// # Errors
    ///
    /// [`StageError::GridVariable`] for anything but `fake`, `opensim` or
    /// `aditi`.
    pub fn selected(value: Option<&str>) -> Result<Self, StageError> {
        match value.map(str::trim) {
            None | Some("fake" | "") => Ok(Self::Fake),
            Some("opensim") => Ok(Self::OpenSim),
            Some("aditi") => Ok(Self::Aditi),
            Some(other) => Err(StageError::GridVariable(other.to_owned())),
        }
    }

    /// The grid the environment selects.
    ///
    /// # Errors
    ///
    /// As [`selected`](Self::selected).
    pub fn from_env() -> Result<Self, StageError> {
        Self::selected(std::env::var(GRID_VARIABLE).ok().as_deref())
    }

    /// Whether this is a live grid: one this process neither starts nor
    /// controls.
    #[must_use]
    pub const fn is_live(self) -> bool {
        !matches!(self, Self::Fake)
    }

    /// Whether logins to it are held to the shared per-avatar cooldown.
    #[must_use]
    pub const fn needs_cooldown(self) -> bool {
        matches!(self, Self::Aditi)
    }

    /// The grid nickname its login URI resolves through when an avatar names
    /// none (`sl_client_bevy_viewer::grid_login_uri`), for a live grid.
    #[must_use]
    pub const fn nickname(self) -> Option<&'static str> {
        match self {
            Self::Fake => None,
            Self::OpenSim => Some("opensim"),
            Self::Aditi => Some("aditi"),
        }
    }

    /// Where a live grid's viewers start when `SL_E2E_START` names nowhere:
    /// on OpenSim the centre of the local grid's `Default Region`, where
    /// `sl-conformance` also starts, so every viewer is within chat range of
    /// the others; on aditi wherever each avatar last logged out.
    #[must_use]
    pub const fn default_start(self) -> &'static str {
        match self {
            Self::OpenSim => "uri:Default Region&128&128&30",
            Self::Fake | Self::Aditi => "last",
        }
    }

    /// The credentials file a live grid's accounts come from when
    /// `SL_E2E_CREDENTIALS` names none, relative to the workspace root —
    /// the files `sl-conformance` and the viewer read.
    #[must_use]
    pub const fn credentials_file(self) -> Option<&'static str> {
        match self {
            Self::Fake => None,
            Self::OpenSim => Some("credentials.toml"),
            Self::Aditi => Some("credentials.aditi.toml"),
        }
    }
}

impl fmt::Display for Grid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::Grid;
    use crate::error::StageError;

    /// Unset is the fake grid; each name picks its grid; anything else is
    /// refused rather than silently running on the fake grid.
    #[test]
    fn the_variable_selects_the_grid() -> Result<(), StageError> {
        assert_eq!(Grid::selected(None)?, Grid::Fake);
        assert_eq!(Grid::selected(Some("fake"))?, Grid::Fake);
        assert_eq!(Grid::selected(Some(" opensim "))?, Grid::OpenSim);
        assert_eq!(Grid::selected(Some("aditi"))?, Grid::Aditi);
        assert!(matches!(
            Grid::selected(Some("agni")),
            Err(StageError::GridVariable(value)) if value == "agni"
        ));
        Ok(())
    }

    /// Only aditi is held to the cooldown; only the live grids have a
    /// credentials file.
    #[test]
    fn a_live_grid_has_credentials_and_only_aditi_a_cooldown() {
        assert!(!Grid::Fake.is_live());
        assert!(Grid::OpenSim.is_live() && Grid::Aditi.is_live());
        assert!(Grid::Aditi.needs_cooldown());
        assert!(!Grid::OpenSim.needs_cooldown() && !Grid::Fake.needs_cooldown());
        assert_eq!(Grid::Fake.credentials_file(), None);
        assert_eq!(Grid::OpenSim.credentials_file(), Some("credentials.toml"));
    }
}
