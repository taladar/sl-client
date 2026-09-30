//! A live grid's accounts: which avatar of its credentials file each stage
//! viewer logs in as, where, and at which login URI.
//!
//! - `SL_E2E_CREDENTIALS` names the credentials file; unset, it is the
//!   workspace's `credentials.toml` (OpenSim) or `credentials.aditi.toml`
//!   (aditi) — the files `sl-conformance` and the viewer read.
//! - `SL_E2E_AVATARS` lists the avatar keys (`[avatars.<key>]`) in the order
//!   the stage's viewers take them, comma-separated; unset, `primary`,
//!   `secondary` and `tertiary` come first and every other key follows in
//!   sorted order.
//! - `SL_E2E_START` is the start location every viewer logs in at (`last`,
//!   `home` or `uri:Region&x&y&z`); unset, the grid's
//!   ([`Grid::default_start`]): one spot for every viewer on OpenSim, `last`
//!   on aditi.

use std::path::{Path, PathBuf};

use sl_client_bevy::StartLocation;
use sl_repl::{Avatar, Credentials};

use crate::error::StageError;
use crate::grid::Grid;

/// The variable naming a live grid's credentials file.
pub const CREDENTIALS_VARIABLE: &str = "SL_E2E_CREDENTIALS";

/// The variable ordering the avatar keys the viewers take.
pub const AVATARS_VARIABLE: &str = "SL_E2E_AVATARS";

/// The variable naming the start location on a live grid.
pub const START_VARIABLE: &str = "SL_E2E_START";

/// The keys the viewers take first, in this order, when `SL_E2E_AVATARS`
/// is unset — the keys the credentials files and `sl-conformance` use.
const PREFERRED: [&str; 3] = ["primary", "secondary", "tertiary"];

/// One account of a live grid.
#[derive(Debug, Clone)]
pub(crate) struct LiveAccount {
    /// Its key in the credentials file.
    pub(crate) key: String,
    /// Its credentials.
    pub(crate) avatar: Avatar,
    /// The login URI it logs in at.
    pub(crate) login_uri: String,
}

impl LiveAccount {
    /// Its `First Last` name: its cooldown stamp, and what the grid calls it.
    pub(crate) fn name(&self) -> String {
        format!("{} {}", self.avatar.first(), self.avatar.last())
    }
}

/// A live grid's accounts, in the order the viewers take them, and where
/// they start.
#[derive(Debug, Clone)]
pub(crate) struct LiveAccounts {
    /// The credentials file, which a process viewer is handed.
    pub(crate) file: PathBuf,
    /// The accounts, in order.
    pub(crate) accounts: Vec<LiveAccount>,
    /// Where every viewer starts.
    pub(crate) start: StartLocation,
}

impl LiveAccounts {
    /// `grid`'s accounts, as the environment configures them.
    ///
    /// # Errors
    ///
    /// [`StageError::Credentials`] when the file cannot be read, a key named
    /// in `SL_E2E_AVATARS` is not in it, an avatar's login URI cannot be
    /// resolved, or `SL_E2E_START` is not a start location.
    pub(crate) fn from_env(grid: Grid) -> Result<Self, StageError> {
        let file = match std::env::var_os(CREDENTIALS_VARIABLE) {
            Some(file) => PathBuf::from(file),
            None => {
                let name = grid.credentials_file().ok_or_else(|| {
                    StageError::Credentials(format!("the {grid} grid has no credentials file"))
                })?;
                workspace_root().join(name)
            }
        };
        let order = std::env::var(AVATARS_VARIABLE).ok();
        let start = std::env::var(START_VARIABLE).ok();
        Self::load(grid, &file, order.as_deref(), start.as_deref())
    }

    /// `grid`'s accounts from the credentials `file`, taken in `order` (a
    /// comma-separated key list) and starting at `start`, each defaulted as
    /// the module says.
    ///
    /// # Errors
    ///
    /// As [`from_env`](Self::from_env).
    pub(crate) fn load(
        grid: Grid,
        file: &Path,
        order: Option<&str>,
        start: Option<&str>,
    ) -> Result<Self, StageError> {
        let credentials = Credentials::load(file)
            .map_err(|error| StageError::Credentials(format!("{}: {error}", file.display())))?;
        let accounts = avatar_order(&credentials, order)
            .into_iter()
            .map(|key| {
                let avatar = credentials
                    .select(Some(&key))
                    .map_err(|error| {
                        StageError::Credentials(format!("{}: {error}", file.display()))
                    })?
                    .clone();
                let login_uri = login_uri(grid, &avatar).ok_or_else(|| {
                    StageError::Credentials(format!(
                        "avatar {key} of {} names a grid with no known login URI",
                        file.display()
                    ))
                })?;
                Ok(LiveAccount {
                    key,
                    avatar,
                    login_uri,
                })
            })
            .collect::<Result<Vec<_>, StageError>>()?;
        let start = start
            .map(str::trim)
            .filter(|start| !start.is_empty())
            .unwrap_or_else(|| grid.default_start())
            .parse()
            .map_err(|error| StageError::Credentials(format!("{START_VARIABLE}: {error}")))?;
        // Absolute: a process viewer runs in its own directory.
        let file = std::path::absolute(file)
            .map_err(|error| StageError::Credentials(format!("{}: {error}", file.display())))?;
        Ok(Self {
            file,
            accounts,
            start,
        })
    }
}

/// The workspace root this crate was built in, where the credentials files
/// are.
fn workspace_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap_or(manifest).to_path_buf()
}

/// The avatar keys in the order the viewers take them: `order`'s, or the
/// preferred keys first and the rest sorted. A key `order` names twice is
/// taken once.
fn avatar_order(credentials: &Credentials, order: Option<&str>) -> Vec<String> {
    let names = credentials.avatar_names();
    let mut keys: Vec<String> = match order {
        Some(order) => order
            .split(',')
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(str::to_owned)
            .collect(),
        None => PREFERRED
            .iter()
            .copied()
            .filter(|key| names.contains(key))
            .chain(names.iter().copied().filter(|key| !PREFERRED.contains(key)))
            .map(str::to_owned)
            .collect(),
    };
    let mut seen = Vec::new();
    keys.retain(|key| {
        let first = !seen.contains(key);
        if first {
            seen.push(key.clone());
        }
        first
    });
    keys
}

/// The login URI of `avatar` on `grid`: its own, else its grid nickname's,
/// else the grid's.
fn login_uri(grid: Grid, avatar: &Avatar) -> Option<String> {
    if let Some(uri) = avatar.login_uri() {
        return Some(uri.to_owned());
    }
    avatar
        .grid()
        .or_else(|| grid.nickname())
        .and_then(sl_client_bevy_viewer::grid_login_uri)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{LiveAccounts, avatar_order};
    use crate::error::StageError;
    use crate::grid::Grid;

    /// A credentials file with the conventional keys and one more, none with
    /// a login URI of its own but one.
    const CREDENTIALS: &str = "\
[avatars.estate-owner]
first = \"Estate\"
last = \"Owner\"
password = \"x\"

[avatars.secondary]
first = \"Second\"
last = \"Tester\"
password = \"x\"

[avatars.primary]
first = \"First\"
last = \"Tester\"
password = \"x\"
login_uri = \"http://127.0.0.1:9100/\"
";

    /// Write [`CREDENTIALS`] to a scratch file.
    fn credentials_file(name: &str) -> Result<std::path::PathBuf, std::io::Error> {
        let file = std::env::temp_dir().join(format!(
            "sl-e2e-credentials-{name}-{}.toml",
            std::process::id()
        ));
        fs_err::write(&file, CREDENTIALS)?;
        Ok(file)
    }

    /// Unset, the conventional keys come first and the rest follow sorted;
    /// set, the list is taken as given, once each.
    #[test]
    fn the_viewers_take_the_avatars_in_order() -> Result<(), String> {
        let credentials =
            sl_repl::Credentials::from_toml_str(CREDENTIALS).map_err(|error| error.to_string())?;
        assert_eq!(
            avatar_order(&credentials, None),
            ["primary", "secondary", "estate-owner"]
        );
        assert_eq!(
            avatar_order(&credentials, Some("secondary, primary,secondary")),
            ["secondary", "primary"]
        );
        Ok(())
    }

    /// Each account logs in at its own URI, else its grid's; the start is
    /// the grid's unless one is given.
    #[test]
    fn an_account_logs_in_at_its_own_uri_else_the_grids() -> Result<(), Box<dyn core::error::Error>>
    {
        let file = credentials_file("uri")?;
        let live = LiveAccounts::load(Grid::OpenSim, &file, None, None)?;
        let uris: Vec<&str> = live
            .accounts
            .iter()
            .map(|account| account.login_uri.as_str())
            .collect();
        assert_eq!(
            uris,
            [
                "http://127.0.0.1:9100/",
                "http://127.0.0.1:9000/",
                "http://127.0.0.1:9000/"
            ]
        );
        assert_eq!(live.start.to_wire_string(), "uri:Default Region&128&128&30");
        let aditi = LiveAccounts::load(
            Grid::Aditi,
            &file,
            Some("secondary"),
            Some("uri:Somewhere&10&20&30"),
        )?;
        assert_eq!(
            aditi
                .accounts
                .iter()
                .map(|account| account.login_uri.as_str())
                .collect::<Vec<_>>(),
            ["https://login.aditi.lindenlab.com/cgi-bin/login.cgi"]
        );
        assert_eq!(aditi.start.to_wire_string(), "uri:Somewhere&10&20&30");
        fs_err::remove_file(&file)?;
        Ok(())
    }

    /// A key the credentials file does not have is an error, not a skip:
    /// the run was configured wrongly.
    #[test]
    fn an_unknown_avatar_key_is_refused() -> Result<(), std::io::Error> {
        let file = credentials_file("unknown")?;
        assert!(matches!(
            LiveAccounts::load(Grid::OpenSim, &file, Some("quaternary"), None),
            Err(StageError::Credentials(message)) if message.contains("quaternary")
        ));
        fs_err::remove_file(&file)?;
        Ok(())
    }
}
