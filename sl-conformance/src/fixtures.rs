//! Optional, environment-specific test fixtures loaded from a gitignored
//! `fixtures.<grid>.toml` next to the credentials file.
//!
//! Some cases need a *stable, pre-existing* grid resource rather than one
//! created fresh on every run. On the throwaway local OpenSim grid, creating per
//! run is free and disposable, so no fixtures file is needed (the default is
//! empty). On Second Life, by contrast, creating a group costs **L$100**, the
//! grid has no on-demand group delete (an emptied group self-purges only after
//! ~48 h once it drops below two members), and the founder keeps a group slot
//! for every group they create until that purge — so a case that creates a group
//! per run both spends L$ and marches the founder toward Second Life's ~42-group
//! cap. To avoid that, such a case can be pointed at a **pre-made** group via
//! this file and reuse it across runs.
//!
//! The file is per grid (`fixtures.toml` for OpenSim, `fixtures.aditi.toml` for
//! aditi), mirroring the credentials-file naming, and is gitignored because the
//! ids in it are specific to one operator's avatars. Every field is optional; an
//! absent file is equivalent to an empty one.
//!
//! ```toml
//! # Pre-made open-enrollment groups the primary avatar owns, reused by the
//! # group cases instead of creating throwaway groups per run. A case takes the
//! # group(s) it needs by position: the membership/messaging cases use the
//! # first, `chat-invite-accept-decline` uses the first two (it needs two
//! # distinct pending sessions).
//! premade_groups = [
//!     "00000000-0000-0000-0000-000000000000",
//!     "11111111-1111-1111-1111-111111111111",
//! ]
//!
//! # A second, stable avatar whose profile the `avatar-properties` case reads.
//! # On OpenSim the case falls back to the local secondary test avatar, so this
//! # is only needed on Second Life (where there is no built-in second avatar).
//! other_avatar = "22222222-2222-2222-2222-222222222222"
//!
//! # A stable, fetchable mesh asset the `mesh-fetch-http` case pulls and decodes.
//! # Optional: absent it, the case scans the region's object stream for a
//! # mesh-shaped prim and records `partial` if it finds none.
//! mesh_asset = "33333333-3333-3333-3333-333333333333"
//! ```

use std::path::{Path, PathBuf};

use sl_client_tokio::{AgentKey, GroupKey, MeshKey, Uuid};

use crate::grid::Grid;

/// Environment-specific fixtures for one grid.
#[derive(Clone, Debug, Default)]
pub struct Fixtures {
    /// Pre-made open-enrollment groups owned by the primary avatar, reused by the
    /// group cases (by position) instead of creating throwaway groups per run. An
    /// empty list (the default, and the norm on OpenSim) means "create per run".
    premade_groups: Vec<GroupKey>,
    /// A second, stable avatar whose profile the `avatar-properties` case reads.
    /// Needed only on Second Life, which has no built-in second avatar; on
    /// OpenSim the case falls back to the local secondary test avatar.
    other_avatar: Option<AgentKey>,
    /// A stable, fetchable mesh asset id the `mesh-fetch-http` case pulls and
    /// decodes. Optional: when absent the case instead scans the region's object
    /// stream for a mesh-shaped prim, and records `partial` if it finds none.
    mesh_asset: Option<MeshKey>,
}

/// The raw TOML shape, before ids are parsed into typed keys.
#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFixtures {
    /// The pre-made group ids as UUID strings, parsed into
    /// [`Fixtures::premade_groups`].
    #[serde(default)]
    premade_groups: Vec<String>,
    /// The second-avatar id as a UUID string, parsed into
    /// [`Fixtures::other_avatar`].
    #[serde(default)]
    other_avatar: Option<String>,
    /// The mesh asset id as a UUID string, parsed into
    /// [`Fixtures::mesh_asset`].
    #[serde(default)]
    mesh_asset: Option<String>,
}

/// Why a fixtures file could not be turned into [`Fixtures`].
#[expect(
    clippy::module_name_repetitions,
    reason = "matches the crate's `<Module>Error` convention (cf. gitinfo::GitError)"
)]
#[derive(Debug, thiserror::Error)]
pub enum FixturesError {
    /// The file could not be read.
    #[error("could not read fixtures file {path}: {source}")]
    Read {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The TOML did not parse or did not match the expected shape.
    #[error("could not parse fixtures file {path}: {message}")]
    Parse {
        /// The path that failed to parse.
        path: PathBuf,
        /// The parser's message.
        message: String,
    },
    /// A field that should hold a UUID did not.
    #[error("fixtures field `{field}` is not a valid UUID: {value}")]
    BadUuid {
        /// The offending field name.
        field: &'static str,
        /// The value that failed to parse.
        value: String,
    },
}

impl Fixtures {
    /// The default fixtures-file path for a grid (alongside the credentials
    /// file): `fixtures.toml` for OpenSim, `fixtures.<grid>.toml` otherwise.
    #[must_use]
    pub fn default_path(grid: Grid) -> PathBuf {
        match grid {
            Grid::Opensim => PathBuf::from("fixtures.toml"),
            Grid::Aditi => PathBuf::from("fixtures.aditi.toml"),
        }
    }

    /// Load fixtures for `grid`.
    ///
    /// When `path` is `Some`, the file is required and a missing file is an
    /// error (the operator explicitly asked for it). When `path` is `None`, the
    /// grid's [`default_path`](Self::default_path) is consulted but a missing
    /// file is *not* an error — it yields the empty default, since most setups
    /// (and every OpenSim setup) need no fixtures.
    ///
    /// # Errors
    ///
    /// Returns a [`FixturesError`] if a required file is missing, an existing
    /// file cannot be read or parsed, or a field holds a malformed id.
    pub fn load(grid: Grid, path: Option<&Path>) -> Result<Self, FixturesError> {
        match path {
            Some(explicit) => Self::load_file(explicit),
            None => {
                let default = Self::default_path(grid);
                if default.exists() {
                    Self::load_file(&default)
                } else {
                    Ok(Self::default())
                }
            }
        }
    }

    /// Read and parse a fixtures file that is known to be wanted.
    fn load_file(path: &Path) -> Result<Self, FixturesError> {
        let text = fs_err::read_to_string(path).map_err(|source| FixturesError::Read {
            path: path.to_owned(),
            source,
        })?;
        let raw: RawFixtures = toml::from_str(&text).map_err(|error| FixturesError::Parse {
            path: path.to_owned(),
            message: error.to_string(),
        })?;
        Self::from_raw(raw)
    }

    /// Convert the parsed TOML into typed fixtures, validating ids.
    fn from_raw(raw: RawFixtures) -> Result<Self, FixturesError> {
        let premade_groups = raw
            .premade_groups
            .into_iter()
            .map(|value| {
                Uuid::parse_str(&value)
                    .map(GroupKey::from)
                    .map_err(|_invalid| FixturesError::BadUuid {
                        field: "premade_groups",
                        value,
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let other_avatar = raw
            .other_avatar
            .map(|value| {
                Uuid::parse_str(&value)
                    .map(AgentKey::from)
                    .map_err(|_invalid| FixturesError::BadUuid {
                        field: "other_avatar",
                        value,
                    })
            })
            .transpose()?;
        let mesh_asset = raw
            .mesh_asset
            .map(|value| {
                Uuid::parse_str(&value)
                    .map(MeshKey::from)
                    .map_err(|_invalid| FixturesError::BadUuid {
                        field: "mesh_asset",
                        value,
                    })
            })
            .transpose()?;
        Ok(Self {
            premade_groups,
            other_avatar,
            mesh_asset,
        })
    }

    /// The `index`-th pre-made group, if one is configured at that position for
    /// this grid. Cases take the group(s) they need by position (the
    /// membership/messaging cases use `0`; `chat-invite-accept-decline` uses `0`
    /// and `1`).
    #[must_use]
    pub fn premade_group(&self, index: usize) -> Option<GroupKey> {
        self.premade_groups.get(index).copied()
    }

    /// The configured second avatar whose profile the `avatar-properties` case
    /// reads, if any. Needed only on Second Life; OpenSim falls back to the local
    /// secondary test avatar.
    #[must_use]
    pub const fn other_avatar(&self) -> Option<AgentKey> {
        self.other_avatar
    }

    /// The configured fetchable mesh asset the `mesh-fetch-http` case pulls, if
    /// any. When absent the case scans the region's object stream for a
    /// mesh-shaped prim instead.
    #[must_use]
    pub const fn mesh_asset(&self) -> Option<MeshKey> {
        self.mesh_asset
    }
}

#[cfg(test)]
mod tests {
    use super::{Fixtures, RawFixtures};
    use crate::grid::Grid;
    use pretty_assertions::{assert_eq, assert_ne};

    /// The default fixtures path mirrors the credentials-file naming per grid.
    #[test]
    fn default_path_per_grid() {
        assert_eq!(
            Fixtures::default_path(Grid::Opensim).to_string_lossy(),
            "fixtures.toml"
        );
        assert_eq!(
            Fixtures::default_path(Grid::Aditi).to_string_lossy(),
            "fixtures.aditi.toml"
        );
    }

    /// An explicit, non-existent fixtures path is a read error, not silently
    /// empty — an operator who names a file means to use it.
    #[test]
    fn explicit_missing_is_error() {
        let path = std::path::Path::new("does-not-exist.fixtures.toml");
        assert!(matches!(
            Fixtures::load(Grid::Opensim, Some(path)),
            Err(super::FixturesError::Read { .. })
        ));
    }

    /// The empty default carries no pre-made group at any index.
    #[test]
    fn empty_default_has_no_group() {
        assert_eq!(Fixtures::default().premade_group(0), None);
    }

    /// Well-formed `premade_groups` parse into typed keys, addressable by index.
    #[test]
    fn parses_premade_groups() -> Result<(), super::FixturesError> {
        let raw = RawFixtures {
            premade_groups: vec![
                "11111111-2222-3333-4444-555555555555".to_owned(),
                "22222222-3333-4444-5555-666666666666".to_owned(),
            ],
            other_avatar: None,
            mesh_asset: None,
        };
        let fixtures = Fixtures::from_raw(raw)?;
        assert!(fixtures.premade_group(0).is_some());
        assert!(fixtures.premade_group(1).is_some());
        // Distinct ids parsed in order.
        assert_ne!(fixtures.premade_group(0), fixtures.premade_group(1));
        // No third group configured.
        assert_eq!(fixtures.premade_group(2), None);
        // No other-avatar configured here.
        assert_eq!(fixtures.other_avatar(), None);
        Ok(())
    }

    /// A well-formed `other_avatar` parses into a typed key; an absent one is
    /// `None`.
    #[test]
    fn parses_other_avatar() -> Result<(), super::FixturesError> {
        let raw = RawFixtures {
            premade_groups: Vec::new(),
            other_avatar: Some("33333333-4444-5555-6666-777777777777".to_owned()),
            mesh_asset: None,
        };
        let fixtures = Fixtures::from_raw(raw)?;
        assert!(fixtures.other_avatar().is_some());
        Ok(())
    }

    /// A well-formed `mesh_asset` parses into a typed key; an absent one is
    /// `None`.
    #[test]
    fn parses_mesh_asset() -> Result<(), super::FixturesError> {
        let raw = RawFixtures {
            premade_groups: Vec::new(),
            other_avatar: None,
            mesh_asset: Some("44444444-5555-6666-7777-888888888888".to_owned()),
        };
        let fixtures = Fixtures::from_raw(raw)?;
        assert!(fixtures.mesh_asset().is_some());
        assert_eq!(Fixtures::default().mesh_asset(), None);
        Ok(())
    }

    /// A malformed `mesh_asset` is a `BadUuid` error, not a silent drop.
    #[test]
    fn rejects_bad_mesh_asset() {
        let raw = RawFixtures {
            premade_groups: Vec::new(),
            other_avatar: None,
            mesh_asset: Some("not-a-uuid".to_owned()),
        };
        assert!(matches!(
            Fixtures::from_raw(raw),
            Err(super::FixturesError::BadUuid {
                field: "mesh_asset",
                ..
            })
        ));
    }

    /// A malformed `other_avatar` is a `BadUuid` error, not a silent drop.
    #[test]
    fn rejects_bad_other_avatar() {
        let raw = RawFixtures {
            premade_groups: Vec::new(),
            other_avatar: Some("not-a-uuid".to_owned()),
            mesh_asset: None,
        };
        assert!(matches!(
            Fixtures::from_raw(raw),
            Err(super::FixturesError::BadUuid {
                field: "other_avatar",
                ..
            })
        ));
    }

    /// A malformed entry in `premade_groups` is a `BadUuid` error, not a silent
    /// drop.
    #[test]
    fn rejects_bad_premade_group() {
        let raw = RawFixtures {
            premade_groups: vec!["not-a-uuid".to_owned()],
            other_avatar: None,
            mesh_asset: None,
        };
        assert!(matches!(
            Fixtures::from_raw(raw),
            Err(super::FixturesError::BadUuid {
                field: "premade_groups",
                ..
            })
        ));
    }
}
