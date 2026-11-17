//! The grids the login screen offers: the built-in ones, which the viewer
//! knows, and the user's own, which its grid manager adds and keeps in
//! `grids.toml` beside the global settings
//! ([`ViewerPaths::grids_file`](crate::assembly::ViewerPaths::grids_file)).
//!
//! The built-ins are the local OpenSim test grid (the default), Second Life
//! and the Second Life beta grid, and they cannot be removed — Firestorm
//! protects its system grids the same way. A user grid is a display name and
//! a login URI, the name filled in from the grid's own `get_grid_info` when it
//! is added (`crate::login_screen`).
//!
//! Reference (Firestorm, read-only): `llviewernetwork.cpp` (`LLGridManager`),
//! `fsgridhandler.cpp`.

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use url::Url;

/// A grid the viewer knows without being told.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinGrid {
    /// The names `--grid` and a credentials file's `grid` accept for it, the
    /// first of them its own.
    pub nicks: &'static [&'static str],
    /// What the login screen calls it.
    pub name: &'static str,
    /// Its login URI.
    pub login_uri: &'static str,
}

impl BuiltinGrid {
    /// Whether `nick` names this grid, in any case.
    fn answers_to(&self, nick: &str) -> bool {
        self.nicks
            .iter()
            .any(|known| known.eq_ignore_ascii_case(nick))
    }
}

/// The local OpenSim test grid's login URI: the default grid, and the one a
/// credentials entry naming none logs in to.
pub const DEFAULT_LOGIN_URI: &str = "http://127.0.0.1:9000/";

/// The built-in grids, the default first.
pub const BUILTIN_GRIDS: &[BuiltinGrid] = &[
    BuiltinGrid {
        nicks: &["localhost", "local", "opensim"],
        name: "Local OpenSim",
        login_uri: DEFAULT_LOGIN_URI,
    },
    BuiltinGrid {
        nicks: &["agni", "secondlife", "sl"],
        name: "Second Life",
        login_uri: "https://login.agni.lindenlab.com/cgi-bin/login.cgi",
    },
    BuiltinGrid {
        nicks: &["aditi", "beta"],
        name: "Second Life Beta",
        login_uri: "https://login.aditi.lindenlab.com/cgi-bin/login.cgi",
    },
];

/// The login URI of the built-in grid `nick` names, or `None`.
#[must_use]
pub fn builtin_login_uri(nick: &str) -> Option<&'static str> {
    BUILTIN_GRIDS
        .iter()
        .find(|grid| grid.answers_to(nick))
        .map(|grid| grid.login_uri)
}

/// One grid the login screen offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grid {
    /// What the login screen calls it.
    pub name: String,
    /// Its login URI.
    pub login_uri: Url,
    /// Whether the viewer knows it of itself, and so will not remove it.
    pub builtin: bool,
}

/// Why the grid list refused a change.
#[derive(Debug, thiserror::Error)]
pub enum GridListError {
    /// A built-in grid cannot be removed.
    #[error("{0} is built in and cannot be removed")]
    Builtin(String),
    /// No grid is at that place in the list.
    #[error("there is no grid at {0} in the list")]
    NoSuchGrid(usize),
    /// The grids file could not be written.
    #[error("could not write the grids file {path}")]
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// The user grids could not be written as TOML.
    #[error("could not write the user grids as TOML")]
    Serialize(#[from] toml::ser::Error),
}

/// The grids file on disk: the user's grids, in order.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct GridsFile {
    /// One `[[grid]]` table per grid.
    #[serde(default, rename = "grid")]
    grids: Vec<GridsFileEntry>,
}

/// One user grid in the grids file.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct GridsFileEntry {
    /// Its display name.
    name: String,
    /// Its login URI.
    login_uri: String,
}

/// The grids the login screen offers, built-ins first, and where the user's
/// own are kept.
#[derive(Resource, Debug, Clone)]
pub struct GridList {
    /// Every grid, built-ins first.
    grids: Vec<Grid>,
    /// The file the user's grids are kept in, or `None` to keep them only for
    /// the run.
    file: Option<PathBuf>,
}

impl GridList {
    /// The built-in grids and the user's grids from `file`. A missing file is
    /// no user grids; one that cannot be read, or an entry whose login URI is
    /// no URL, is logged and left out, so the built-ins are always offered.
    #[must_use]
    pub fn load(file: Option<PathBuf>) -> Self {
        let mut grids: Vec<Grid> = BUILTIN_GRIDS
            .iter()
            .filter_map(|grid| match grid.login_uri.parse() {
                Ok(login_uri) => Some(Grid {
                    name: grid.name.to_owned(),
                    login_uri,
                    builtin: true,
                }),
                Err(error) => {
                    error!("the built-in grid {} has no login URI: {error}", grid.name);
                    None
                }
            })
            .collect();
        if let Some(path) = &file {
            for entry in read_grids_file(path) {
                match entry.login_uri.parse::<Url>() {
                    Ok(login_uri) if !grids.iter().any(|grid| grid.login_uri == login_uri) => {
                        grids.push(Grid {
                            name: entry.name,
                            login_uri,
                            builtin: false,
                        });
                    }
                    Ok(_duplicate) => {}
                    Err(error) => warn!(
                        "the grids file {} lists {} at {:?}, which is no login URI: {error}",
                        path.display(),
                        entry.name,
                        entry.login_uri
                    ),
                }
            }
        }
        Self { grids, file }
    }

    /// Every grid, built-ins first.
    #[must_use]
    pub fn grids(&self) -> &[Grid] {
        &self.grids
    }

    /// Where the grid with `login_uri` is in the list.
    #[must_use]
    pub fn position(&self, login_uri: &Url) -> Option<usize> {
        self.grids
            .iter()
            .position(|grid| &grid.login_uri == login_uri)
    }

    /// Add a user grid, or rename the user grid already at its login URI, and
    /// keep the list on disk; answers where the grid now is. A built-in grid's
    /// URI is not added twice: its place is answered as it is.
    ///
    /// # Errors
    ///
    /// When the list could not be written; the grid is in the list for this
    /// run regardless.
    pub fn add(&mut self, name: String, login_uri: Url) -> Result<usize, GridListError> {
        let index = match self.position(&login_uri) {
            Some(index) => {
                if let Some(grid) = self.grids.get_mut(index)
                    && !grid.builtin
                {
                    grid.name = name;
                }
                index
            }
            None => {
                self.grids.push(Grid {
                    name,
                    login_uri,
                    builtin: false,
                });
                self.grids.len().saturating_sub(1)
            }
        };
        self.save()?;
        Ok(index)
    }

    /// Remove the user grid at `index` and keep the list on disk.
    ///
    /// # Errors
    ///
    /// [`GridListError::Builtin`] for a built-in grid,
    /// [`GridListError::NoSuchGrid`] past the end, and when the list could not
    /// be written.
    pub fn remove(&mut self, index: usize) -> Result<Grid, GridListError> {
        let grid = self
            .grids
            .get(index)
            .ok_or(GridListError::NoSuchGrid(index))?;
        if grid.builtin {
            return Err(GridListError::Builtin(grid.name.clone()));
        }
        let removed = self.grids.remove(index);
        self.save()?;
        Ok(removed)
    }

    /// Write the user's grids to the grids file, where there is one.
    fn save(&self) -> Result<(), GridListError> {
        let Some(path) = &self.file else {
            return Ok(());
        };
        let file = GridsFile {
            grids: self
                .grids
                .iter()
                .filter(|grid| !grid.builtin)
                .map(|grid| GridsFileEntry {
                    name: grid.name.clone(),
                    login_uri: grid.login_uri.to_string(),
                })
                .collect(),
        };
        let text = toml::to_string_pretty(&file)?;
        let write = |path: &Path| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                fs_err::create_dir_all(parent)?;
            }
            fs_err::write(path, text.as_bytes())
        };
        write(path).map_err(|source| GridListError::Write {
            path: path.clone(),
            source,
        })
    }
}

/// The user grids in the grids file at `path`: none when there is no file,
/// and none (logged) when it cannot be read.
fn read_grids_file(path: &Path) -> Vec<GridsFileEntry> {
    let text = match fs_err::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            warn!("could not read the grids file: {error}");
            return Vec::new();
        }
    };
    match toml::from_str::<GridsFile>(&text) {
        Ok(file) => file.grids,
        Err(error) => {
            warn!(
                "the grids file {} is not a grid list, so only the built-in grids are offered: \
                 {error}",
                path.display()
            );
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BUILTIN_GRIDS, GridList, GridListError, builtin_login_uri};
    use pretty_assertions::assert_eq;

    /// The test's error type.
    type TestError = Box<dyn core::error::Error>;

    /// Every nick `--grid` took before the grid list existed still answers.
    #[test]
    fn the_builtin_nicks_answer_as_they_always_did() {
        for (nick, expected) in [
            ("agni", "https://login.agni.lindenlab.com/cgi-bin/login.cgi"),
            (
                "SecondLife",
                "https://login.agni.lindenlab.com/cgi-bin/login.cgi",
            ),
            ("sl", "https://login.agni.lindenlab.com/cgi-bin/login.cgi"),
            (
                "aditi",
                "https://login.aditi.lindenlab.com/cgi-bin/login.cgi",
            ),
            (
                "beta",
                "https://login.aditi.lindenlab.com/cgi-bin/login.cgi",
            ),
            ("localhost", "http://127.0.0.1:9000/"),
            ("local", "http://127.0.0.1:9000/"),
            ("opensim", "http://127.0.0.1:9000/"),
        ] {
            assert_eq!(builtin_login_uri(nick), Some(expected), "{nick}");
        }
        assert_eq!(builtin_login_uri("osgrid"), None);
    }

    /// A user grid survives a reload, a second add at its URI renames it
    /// rather than listing it twice, and only a user grid can be removed.
    #[test]
    fn user_grids_are_kept_renamed_and_removed() -> Result<(), TestError> {
        let dir = std::env::temp_dir().join(format!("sl-grids-{}", std::process::id()));
        let file = dir.join("grids.toml");
        let mut list = GridList::load(Some(file.clone()));
        assert_eq!(
            list.grids().len(),
            BUILTIN_GRIDS.len(),
            "only the built-ins at first"
        );

        let uri: url::Url = "http://grid.example.org:8002/".parse()?;
        let index = list.add("Example".to_owned(), uri.clone())?;
        assert_eq!(list.add("Example Grid".to_owned(), uri.clone())?, index);
        let reloaded = GridList::load(Some(file.clone()));
        let grid = reloaded.grids().get(index).ok_or("the grid was not kept")?;
        assert_eq!(grid.name, "Example Grid", "a second add renames");
        assert!(!grid.builtin);

        assert!(matches!(list.remove(0), Err(GridListError::Builtin(_))));
        let removed = list.remove(index)?;
        assert_eq!(removed.login_uri, uri);
        assert_eq!(
            GridList::load(Some(file)).grids().len(),
            BUILTIN_GRIDS.len(),
            "a removal is kept too"
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }

    /// A grids file that is not a grid list leaves the built-ins offered.
    #[test]
    fn a_broken_grids_file_leaves_the_builtins() -> Result<(), TestError> {
        let dir = std::env::temp_dir().join(format!("sl-grids-broken-{}", std::process::id()));
        fs_err::create_dir_all(&dir)?;
        let file = dir.join("grids.toml");
        fs_err::write(&file, "grid = 7")?;
        assert_eq!(
            GridList::load(Some(file)).grids().len(),
            BUILTIN_GRIDS.len()
        );
        fs_err::remove_dir_all(&dir)?;
        Ok(())
    }
}
