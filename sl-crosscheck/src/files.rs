//! The two small TOML files both viewers read: the credentials and the grid.
//!
//! They are written into the run directory rather than taken from the operator's
//! own `credentials.toml` for three reasons, in increasing order of how much
//! trouble they save:
//!
//! - a run must never put a real avatar's name or password on a fake grid;
//! - the fake grid's account is minted per run, so there is nothing to keep;
//! - Firestorm reads this same format (`FSTestConfig::parseTomlSubset`), and a
//!   file written here is one this crate knows stays inside the subset that
//!   parser accepts.
//!
//! That last point is the constraint on everything here: Firestorm's parser is a
//! deliberately strict *subset* of TOML — flat keys, string / integer / boolean
//! values, `[table.sub]` headers — and anything outside it (arrays, inline
//! tables, dotted keys, multi-line strings) is a hard error there while being
//! perfectly ordinary TOML here. So these files are built as text in that
//! subset, and the tests read them back with a real TOML parser to prove the two
//! readings agree.

use std::path::{Path, PathBuf};

use crate::plan::RunPlan;

/// The `[avatars.<key>]` name a run's credentials file uses.
///
/// One fixed key rather than the avatar's name: both viewers are told which
/// entry to use with `--avatar`, and a fixed key keeps that flag the same
/// whatever the account is called.
pub const AVATAR_KEY: &str = "crosscheck";

/// Where a run's configuration files live, and what they are called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The credentials file both viewers log in from.
    pub credentials: PathBuf,
    /// The grid description file Firestorm reads.
    pub grid: PathBuf,
}

/// Write both files into `dir`, creating it if needed.
///
/// # Errors
///
/// Returns the underlying I/O error, named after the file it happened to.
pub fn write(dir: &Path, plan: &RunPlan) -> Result<Paths, std::io::Error> {
    fs_err::create_dir_all(dir)?;
    let files = Paths {
        credentials: dir.join("credentials.toml"),
        grid: dir.join("grid.toml"),
    };
    fs_err::write(&files.credentials, credentials_toml(plan))?;
    fs_err::write(&files.grid, grid_toml(plan))?;
    Ok(files)
}

/// The credentials file's text: one avatar, with the login URI baked in so this
/// viewer needs no `--grid` and no nickname table.
#[must_use]
pub fn credentials_toml(plan: &RunPlan) -> String {
    format!(
        "# Written by sl-crosscheck for one run against a fake grid.\n\
         # The account exists only in that grid's memory; nothing here is a real\n\
         # avatar or a real password.\n\
         default_avatar = \"{AVATAR_KEY}\"\n\
         \n\
         [avatars.{AVATAR_KEY}]\n\
         first = \"{first}\"\n\
         last = \"{last}\"\n\
         password = \"{password}\"\n\
         login_uri = \"{login_uri}\"\n",
        first = plan.first_name,
        last = plan.last_name,
        password = plan.password,
        login_uri = plan.login_uri,
    )
}

/// The grid file's text: the login URI and a name, which is what Firestorm's
/// `--gridfile` wants.
#[must_use]
pub fn grid_toml(plan: &RunPlan) -> String {
    format!(
        "# Written by sl-crosscheck for one run against a fake grid.\n\
         login_uri = \"{login_uri}\"\n\
         gridname = \"Fake Grid ({scenario})\"\n\
         gridnick = \"fakegrid\"\n",
        login_uri = plan.login_uri,
        scenario = plan.scenario,
    )
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{AVATAR_KEY, credentials_toml, grid_toml};
    use crate::plan::{CaptureSpec, RunPlan};

    /// The boxed error every test in this module reports through.
    type TestError = Box<dyn core::error::Error>;

    /// A plan whose account is the fake grid's stock one.
    fn plan() -> Result<RunPlan, TestError> {
        Ok(RunPlan {
            scenario: "catalogue".to_owned(),
            login_uri: "http://127.0.0.1:9100/".parse()?,
            first_name: "Test".to_owned(),
            last_name: "User".to_owned(),
            password: "password".to_owned(),
            capture: CaptureSpec::default(),
            camera: None,
        })
    }

    /// A real TOML parser reads the file as the fixed avatar key, the account
    /// and the login URI — the three things `--avatar` and the login path need.
    #[test]
    fn the_credentials_file_carries_the_account_and_its_grid() -> Result<(), TestError> {
        let value: toml::Table = toml::from_str(&credentials_toml(&plan()?))?;
        assert_eq!(
            value.get("default_avatar").and_then(toml::Value::as_str),
            Some(AVATAR_KEY)
        );
        let avatar = value
            .get("avatars")
            .and_then(|avatars| avatars.get(AVATAR_KEY))
            .ok_or("the file has an [avatars.crosscheck] table")?;
        assert_eq!(
            avatar.get("first").and_then(toml::Value::as_str),
            Some("Test")
        );
        assert_eq!(
            avatar.get("last").and_then(toml::Value::as_str),
            Some("User")
        );
        assert_eq!(
            avatar.get("login_uri").and_then(toml::Value::as_str),
            Some("http://127.0.0.1:9100/")
        );
        Ok(())
    }

    /// Every line is one of the four shapes Firestorm's strict subset parser
    /// accepts: blank, a comment, a `[table]` header, or `key = "value"`. A
    /// file that leaves the subset is a hard parse error there and a perfectly
    /// good file here, which is exactly the mistake worth a test.
    #[test]
    fn both_files_stay_inside_firestorms_toml_subset() -> Result<(), TestError> {
        let plan = plan()?;
        for text in [credentials_toml(&plan), grid_toml(&plan)] {
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if line.starts_with('[') && line.ends_with(']') {
                    continue;
                }
                let (key, value) = line
                    .split_once(" = ")
                    .ok_or_else(|| format!("{line:?} is not `key = value`"))?;
                assert!(
                    !key.contains('.'),
                    "{key:?} is a dotted key, which the subset rejects"
                );
                assert!(
                    value.starts_with('"') && value.ends_with('"'),
                    "{value:?} is not a plain quoted string"
                );
            }
        }
        Ok(())
    }

    /// The grid file names the scene, so a grid left in Firestorm's grid manager
    /// after a run says which run put it there.
    #[test]
    fn the_grid_file_names_the_scene() -> Result<(), TestError> {
        let value: toml::Table = toml::from_str(&grid_toml(&plan()?))?;
        assert_eq!(
            value.get("gridname").and_then(toml::Value::as_str),
            Some("Fake Grid (catalogue)")
        );
        Ok(())
    }
}
