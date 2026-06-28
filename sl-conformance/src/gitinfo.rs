//! Behaviour-aware git describe.
//!
//! A conformance record stamps the commit at which a feature was last verified.
//! A plain `git describe --dirty` would flag the tree dirty whenever *any* file
//! changed — including the record files this harness writes and the project
//! documentation, neither of which changes runtime behaviour. [`behavior_describe`]
//! therefore computes the base describe itself and applies a `-dirty` suffix only
//! when a **behaviour-relevant** path differs (see [`path_is_behavioural`]).

use std::path::Path;

/// A computed describe plus whether the behaviour-relevant tree is dirty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BehaviorDescribe {
    /// The base describe (`git describe --tags --always --long`), without any
    /// dirty marker.
    pub base: String,
    /// Whether a behaviour-relevant path is modified, staged, or untracked.
    pub dirty: bool,
}

impl BehaviorDescribe {
    /// The describe string to store in a record: the base, with `-dirty`
    /// appended when [`dirty`](Self::dirty) is set.
    #[must_use]
    pub fn describe_string(&self) -> String {
        if self.dirty {
            format!("{}-dirty", self.base)
        } else {
            self.base.clone()
        }
    }
}

/// Whether a changed path counts as a behaviour change (and so makes the tree
/// dirty for conformance purposes).
///
/// Records, documentation, the mdBook, and changelog files are explicitly *not*
/// behavioural; everything else (notably `*.rs`, `Cargo.toml`/`Cargo.lock`, the
/// message template, build scripts) is.
#[must_use]
pub fn path_is_behavioural(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with("records/") {
        return false;
    }
    if trimmed.starts_with("book/") {
        return false;
    }
    let path_obj = Path::new(trimmed);
    if path_obj
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
    {
        return false;
    }
    let file = path_obj
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if file.eq_ignore_ascii_case("changelog") {
        return false;
    }
    if file.starts_with("CHANGELOG") {
        return false;
    }
    true
}

/// Parse the NUL-separated output of `git status --porcelain=v1 -z` into the
/// list of changed paths.
///
/// Each entry is `XY PATH`; for a rename or copy (`R`/`C` in either status
/// column) the following NUL field is the origin path, which is included too so
/// that either side can mark the tree dirty.
fn parse_porcelain_z(raw: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut fields = raw.split('\0').filter(|field| !field.is_empty());
    while let Some(entry) = fields.next() {
        let mut status = entry.chars();
        let index_status = status.next();
        let worktree_status = status.next();
        let path = entry.get(3..).unwrap_or("");
        if !path.is_empty() {
            paths.push(path.to_owned());
        }
        if (matches!(index_status, Some('R' | 'C')) || matches!(worktree_status, Some('R' | 'C')))
            && let Some(origin) = fields.next()
            && !origin.is_empty()
        {
            paths.push(origin.to_owned());
        }
    }
    paths
}

/// Run `git` with the given arguments in `repo_root`, returning trimmed stdout.
///
/// # Errors
///
/// Returns [`GitError::Spawn`] if git cannot be executed, [`GitError::Command`]
/// if it exits non-zero, or [`GitError::NonUtf8`] if its output is not UTF-8.
fn run_git(repo_root: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_root)
        .output()
        .map_err(|error| GitError::Spawn(error.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(GitError::Command {
            args: args.join(" "),
            stderr,
        });
    }
    String::from_utf8(output.stdout).map_err(|_error| GitError::NonUtf8)
}

/// The absolute path of the repository working-tree root containing `start`.
///
/// # Errors
///
/// Returns a [`GitError`] if git is unavailable or `start` is not in a
/// repository.
pub fn repo_root(start: &Path) -> Result<std::path::PathBuf, GitError> {
    let top = run_git(start, &["rev-parse", "--show-toplevel"])?;
    Ok(std::path::PathBuf::from(top.trim()))
}

/// How many commits the current `HEAD` is ahead of the commit named by a
/// recorded describe string (its `-dirty` suffix is ignored).
///
/// Returns `None` when the commit is not in the current history (e.g. it was
/// rebased away) or git cannot answer, so callers can fall back to showing the
/// raw describe.
#[must_use]
pub fn commits_behind(repo_root: &Path, recorded_describe: &str) -> Option<u32> {
    let base = recorded_describe
        .strip_suffix("-dirty")
        .unwrap_or(recorded_describe);
    // `git describe --long` yields `<tag>-<n>-g<hash>`; `--always` with no tag
    // yields the bare `<hash>`. Take whatever follows the last `-g`, else all.
    let hash = base.rsplit_once("-g").map_or(base, |(_prefix, hash)| hash);
    let range = format!("{hash}..HEAD");
    let output = run_git(repo_root, &["rev-list", "--count", &range]).ok()?;
    output.trim().parse().ok()
}

/// Compute the behaviour-aware describe for the repository rooted at
/// `repo_root`.
///
/// # Errors
///
/// Returns a [`GitError`] if git is unavailable, the path is not a repository,
/// or git's output cannot be read.
pub fn behavior_describe(repo_root: &Path) -> Result<BehaviorDescribe, GitError> {
    let base = run_git(repo_root, &["describe", "--tags", "--always", "--long"])?;
    let base = base.trim().to_owned();
    let status = run_git(
        repo_root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
    )?;
    let dirty = parse_porcelain_z(&status)
        .iter()
        .any(|path| path_is_behavioural(path));
    Ok(BehaviorDescribe { base, dirty })
}

/// An error computing the behaviour-aware describe.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitError {
    /// Git could not be spawned (not installed, or not on `PATH`).
    #[error("could not run git: {0}")]
    Spawn(String),
    /// A git invocation exited non-zero (e.g. not a repository).
    #[error("git {args} failed: {stderr}")]
    Command {
        /// The git arguments that failed.
        args: String,
        /// The trimmed stderr of the failing invocation.
        stderr: String,
    },
    /// Git produced output that was not valid UTF-8.
    #[error("git produced non-UTF-8 output")]
    NonUtf8,
}

#[cfg(test)]
mod tests {
    use super::{BehaviorDescribe, parse_porcelain_z, path_is_behavioural};
    use pretty_assertions::assert_eq;

    /// Records, docs, the book, and changelogs are not behavioural; source is.
    #[test]
    fn classification() {
        assert!(path_is_behavioural("sl-conformance/src/grid.rs"));
        assert!(path_is_behavioural("Cargo.toml"));
        assert!(path_is_behavioural("Cargo.lock"));
        assert!(path_is_behavioural("sl-wire/message_template.msg"));
        assert!(path_is_behavioural("sl-conformance/clippy.toml"));

        assert!(!path_is_behavioural("records/opensim/inventory-fetch.toml"));
        assert!(!path_is_behavioural("book/src/conformance/records.md"));
        assert!(!path_is_behavioural("ROADMAP.md"));
        assert!(!path_is_behavioural("README.md"));
        assert!(!path_is_behavioural("sl-survey/changelog"));
        assert!(!path_is_behavioural("CHANGELOG.md"));
        assert!(!path_is_behavioural(""));
    }

    /// A modified source file and an untracked doc parse to two paths.
    #[test]
    fn parse_modified_and_untracked() {
        let raw = "M  sl-conformance/src/grid.rs\0?? notes.md\0";
        assert_eq!(
            parse_porcelain_z(raw),
            vec![
                "sl-conformance/src/grid.rs".to_owned(),
                "notes.md".to_owned(),
            ]
        );
    }

    /// A rename yields both the new and the origin path.
    #[test]
    fn parse_rename_includes_origin() {
        let raw = "R  new.rs\0old.rs\0";
        assert_eq!(
            parse_porcelain_z(raw),
            vec!["new.rs".to_owned(), "old.rs".to_owned()]
        );
    }

    /// The dirty suffix is only appended when dirty.
    #[test]
    fn describe_string_suffix() {
        let clean = BehaviorDescribe {
            base: "v0.1.0-3-gabcdef".to_owned(),
            dirty: false,
        };
        assert_eq!(clean.describe_string(), "v0.1.0-3-gabcdef");
        let dirty = BehaviorDescribe {
            base: "v0.1.0-3-gabcdef".to_owned(),
            dirty: true,
        };
        assert_eq!(dirty.describe_string(), "v0.1.0-3-gabcdef-dirty");
    }
}
