#![doc = include_str!("../README.md")]

pub mod case;
pub mod run;
pub mod verdict;

use std::path::{Path, PathBuf};

pub use case::{Case, LoadError};
pub use run::{Observed, PlayError, run};
pub use verdict::{Verdict, judge};

/// The file name a case directory's case file has.
pub const CASE_FILE: &str = "case.toml";

/// The suffix a case file outside a directory of its own has.
pub const CASE_SUFFIX: &str = ".case.toml";

/// The case files `paths` name: a file as given, and a directory's every
/// `case.toml` and `*.case.toml` below it, in path order.
///
/// # Errors
///
/// An I/O error reading a directory.
pub fn discover(paths: &[PathBuf]) -> std::io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for path in paths {
        if path.is_dir() {
            collect(path, &mut found)?;
        } else {
            found.push(path.clone());
        }
    }
    Ok(found)
}

/// Every case file below `dir`, appended to `found` in path order.
fn collect(dir: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries: Vec<PathBuf> = fs_err::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            collect(&entry, found)?;
        } else if entry
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == CASE_FILE || name.ends_with(CASE_SUFFIX))
        {
            found.push(entry);
        }
    }
    Ok(())
}

/// Plays `case` on each of its grids and judges each run.
///
/// # Errors
///
/// [`PlayError`] when a run could not be played to the end.
pub async fn check(case: &Case) -> Result<Vec<(Verdict, Observed)>, PlayError> {
    let mut verdicts = Vec::new();
    for grid in &case.grids {
        let observed = run(case, *grid).await?;
        verdicts.push((judge(case, &observed), observed));
    }
    Ok(verdicts)
}
