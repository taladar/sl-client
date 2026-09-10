//! Where the viewer's own `assets/` tree — icons, locales, skins — lives, for
//! whichever layout the running binary was started from.
//!
//! Bevy resolves its asset root from `BEVY_ASSET_ROOT`, else the **runtime**
//! `CARGO_MANIFEST_DIR` (which only `cargo run` sets), else the executable's
//! own directory. A viewer started as `./target/release/sl-client-bevy-viewer`
//! therefore looks in `target/release/assets/`, where nothing is: it opens its
//! window, logs in and renders the world with no skin, no toolbar icons and no
//! translated labels, scattering `bevy_asset` "Path not found" lines that read
//! like ordinary noise. A UI check can be run and believed against that build.
//!
//! So the binaries resolve their own root rather than inheriting that default,
//! in the order a viewer is actually run:
//!
//! 1. `BEVY_ASSET_ROOT` when set — the existing override, honoured exactly as
//!    Bevy honours it (the base directory the asset folder sits *in*), so every
//!    harness that already passes it keeps working.
//! 2. an `assets/` beside the executable — the **installed** layout, and what
//!    Bevy would have assumed anyway.
//! 3. the crate's own `assets/`, baked in at compile time — the **development**
//!    layout, which makes a bare `target/…` binary behave like `cargo run`.
//!
//! An installed build has no crate directory to fall back on, which is why step
//! 2 comes first and why step 3 is guarded by the directory still existing.

use std::path::{Path, PathBuf};

use bevy::asset::AssetPlugin;
use tracing::{error, info};

/// The asset folder's name, under whichever base directory is chosen — the
/// name Bevy's own default (`AssetPlugin::file_path`) uses.
const ASSETS_DIR: &str = "assets";

/// The subdirectories of the asset root the viewer's UI cannot do without: the
/// skin stylesheets that give every widget its box and colour, the Fluent
/// bundles every label is looked up in, and the toolbar / parcel icons. None of
/// them is cosmetic, and a run missing any of them is not worth judging
/// appearance from — so their absence is reported once, plainly.
const REQUIRED_ENTRIES: [&str; 3] = ["skins", "locales", "icons"];

/// Which of the layouts in the module docs an [`AssetRoot`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// `BEVY_ASSET_ROOT` was set and named the base directory.
    Environment,
    /// An `assets/` beside the executable — the installed layout.
    BesideExecutable,
    /// The crate's `assets/`, baked in at compile time — the development
    /// layout, and how a bare `target/…` binary finds the tree.
    CrateDirectory,
    /// Nothing was found: the path is where the installed layout would put it,
    /// so the error names the directory a shipped build ought to have.
    Missing,
}

impl Source {
    /// A short phrase for the start-up line, naming the layout rather than
    /// repeating the path that follows it.
    const fn describe(self) -> &'static str {
        match self {
            Self::Environment => "BEVY_ASSET_ROOT",
            Self::BesideExecutable => "beside the executable",
            Self::CrateDirectory => "the viewer crate's source tree",
            Self::Missing => "nowhere — no candidate directory exists",
        }
    }
}

/// A resolved asset root: the absolute directory Bevy should read assets from,
/// and which layout it was found in.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AssetRoot {
    /// The absolute `assets/` directory itself, not the base it sits in — an
    /// absolute [`AssetPlugin::file_path`] wins over Bevy's own base path,
    /// since joining an absolute path replaces the base entirely.
    path: PathBuf,
    /// Where [`choose`] found it.
    source: Source,
}

/// The [`AssetPlugin`] the viewer's binaries run with: Bevy's default with the
/// asset root pinned to [`resolve`]'s answer, and the skin-watching override
/// passed through (`--watch-skins`, and the gallery, which always watches).
///
/// Logs what it resolved — once per process, since each binary builds its
/// `App` once — and shouts if the tree is not there.
pub(crate) fn asset_plugin(watch_for_changes_override: Option<bool>) -> AssetPlugin {
    let root = resolve();
    report(&root, &missing_entries(&root, &|path| path.is_dir()));
    AssetPlugin {
        file_path: root.path.display().to_string(),
        watch_for_changes_override,
        ..AssetPlugin::default()
    }
}

/// [`choose`] over this process: the real environment, the real executable, and
/// the crate directory this binary was compiled in.
fn resolve() -> AssetRoot {
    choose(
        std::env::var_os("BEVY_ASSET_ROOT").map(PathBuf::from),
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &|path| path.is_dir(),
    )
}

/// The resolution order in the module docs, over explicit inputs so it can be
/// unit-tested without an installed viewer to hand.
///
/// `is_dir` decides whether a candidate exists; `environment` is the value of
/// `BEVY_ASSET_ROOT` (the *base* directory, as Bevy reads it), `executable_dir`
/// the directory the running binary sits in, and `crate_dir` the compile-time
/// `CARGO_MANIFEST_DIR`.
fn choose(
    environment: Option<PathBuf>,
    executable_dir: Option<PathBuf>,
    crate_dir: &Path,
    is_dir: &dyn Fn(&Path) -> bool,
) -> AssetRoot {
    // An explicit override is obeyed whether or not it exists: a wrong one must
    // fail loudly at the path the operator named, not fall through to a tree
    // that happens to be there and quietly render a different skin.
    if let Some(base) = environment {
        return AssetRoot {
            path: base.join(ASSETS_DIR),
            source: Source::Environment,
        };
    }
    let beside_executable = executable_dir.map(|dir| dir.join(ASSETS_DIR));
    if let Some(path) = beside_executable.clone().filter(|path| is_dir(path)) {
        return AssetRoot {
            path,
            source: Source::BesideExecutable,
        };
    }
    let in_crate = crate_dir.join(ASSETS_DIR);
    if is_dir(&in_crate) {
        return AssetRoot {
            path: in_crate,
            source: Source::CrateDirectory,
        };
    }
    AssetRoot {
        // The installed layout's path when there is one to name: an operator
        // reading the error is looking at a shipped tree, not at these sources.
        path: beside_executable.unwrap_or(in_crate),
        source: Source::Missing,
    }
}

/// Which of [`REQUIRED_ENTRIES`] the resolved root does not hold.
fn missing_entries(root: &AssetRoot, is_dir: &dyn Fn(&Path) -> bool) -> Vec<&'static str> {
    REQUIRED_ENTRIES
        .into_iter()
        .filter(|entry| !is_dir(&root.path.join(entry)))
        .collect()
}

/// Says once what the root is, and — when the tree is not all there — says so
/// as a single error naming the directory and the override, rather than leaving
/// the per-file `bevy_asset` failures to speak for it.
fn report(root: &AssetRoot, missing: &[&str]) {
    if missing.is_empty() {
        info!(
            path = %root.path.display(),
            source = root.source.describe(),
            "viewer assets"
        );
        return;
    }
    error!(
        path = %root.path.display(),
        source = root.source.describe(),
        missing = missing.join(", "),
        "the viewer's asset tree is incomplete: the UI will come up without its \
         skin, icons or translated labels. Point BEVY_ASSET_ROOT at the \
         directory holding `assets/` (the viewer crate directory in a source \
         checkout), or install `assets/` beside the executable."
    );
}

#[cfg(test)]
mod test {
    use std::path::{Path, PathBuf};

    use pretty_assertions::assert_eq;

    use super::{AssetRoot, Source, choose, missing_entries};

    /// An `is_dir` over a fixed list of directories, so the resolution order
    /// can be exercised without laying out four trees on disk.
    fn only(existing: &[&str]) -> impl Fn(&Path) -> bool {
        let existing: Vec<PathBuf> = existing.iter().map(PathBuf::from).collect();
        move |path: &Path| existing.iter().any(|dir| dir == path)
    }

    /// `BEVY_ASSET_ROOT` wins over both layouts, and is joined the way Bevy
    /// joins it: the variable names the directory `assets/` sits in.
    #[test]
    fn the_environment_override_wins() {
        assert_eq!(
            choose(
                Some(PathBuf::from("/opt/override")),
                Some(PathBuf::from("/opt/viewer/bin")),
                Path::new("/src/sl-client-bevy-viewer"),
                &only(&[
                    "/opt/viewer/bin/assets",
                    "/src/sl-client-bevy-viewer/assets"
                ]),
            ),
            AssetRoot {
                path: PathBuf::from("/opt/override/assets"),
                source: Source::Environment,
            }
        );
    }

    /// An override is taken at its word even when it names nothing: a run given
    /// a wrong path must fail at that path rather than silently rendering some
    /// other tree's skin.
    #[test]
    fn a_wrong_override_is_not_second_guessed() {
        assert_eq!(
            choose(
                Some(PathBuf::from("/opt/typo")),
                Some(PathBuf::from("/opt/viewer/bin")),
                Path::new("/src/sl-client-bevy-viewer"),
                &only(&["/opt/viewer/bin/assets"]),
            ),
            AssetRoot {
                path: PathBuf::from("/opt/typo/assets"),
                source: Source::Environment,
            }
        );
    }

    /// The installed layout: an `assets/` beside the executable is preferred
    /// over the crate directory, which an installed build would not have.
    #[test]
    fn assets_beside_the_executable_win_over_the_crate() {
        assert_eq!(
            choose(
                None,
                Some(PathBuf::from("/opt/viewer/bin")),
                Path::new("/src/sl-client-bevy-viewer"),
                &only(&[
                    "/opt/viewer/bin/assets",
                    "/src/sl-client-bevy-viewer/assets"
                ]),
            ),
            AssetRoot {
                path: PathBuf::from("/opt/viewer/bin/assets"),
                source: Source::BesideExecutable,
            }
        );
    }

    /// The case this task was filed for: a binary run straight out of
    /// `target/release`, where nothing sits beside it, finds the crate's tree.
    #[test]
    fn a_target_directory_binary_finds_the_crate_tree() {
        assert_eq!(
            choose(
                None,
                Some(PathBuf::from("/src/target/release")),
                Path::new("/src/sl-client-bevy-viewer"),
                &only(&["/src/sl-client-bevy-viewer/assets"]),
            ),
            AssetRoot {
                path: PathBuf::from("/src/sl-client-bevy-viewer/assets"),
                source: Source::CrateDirectory,
            }
        );
    }

    /// With neither layout present the answer names where an installed build
    /// would keep its tree — the directory the error tells the operator about.
    #[test]
    fn nothing_found_names_the_installed_layout() {
        assert_eq!(
            choose(
                None,
                Some(PathBuf::from("/opt/viewer/bin")),
                Path::new("/src/sl-client-bevy-viewer"),
                &only(&[]),
            ),
            AssetRoot {
                path: PathBuf::from("/opt/viewer/bin/assets"),
                source: Source::Missing,
            }
        );
    }

    /// With no executable path to be had at all, the crate tree is still named.
    #[test]
    fn without_an_executable_path_the_crate_tree_is_named() {
        assert_eq!(
            choose(
                None,
                None,
                Path::new("/src/sl-client-bevy-viewer"),
                &only(&[]),
            ),
            AssetRoot {
                path: PathBuf::from("/src/sl-client-bevy-viewer/assets"),
                source: Source::Missing,
            }
        );
    }

    /// A root holding every required subdirectory reports nothing missing.
    #[test]
    fn a_complete_tree_reports_nothing_missing() {
        let root = AssetRoot {
            path: PathBuf::from("/src/sl-client-bevy-viewer/assets"),
            source: Source::CrateDirectory,
        };
        let missing: Vec<&str> = missing_entries(
            &root,
            &only(&[
                "/src/sl-client-bevy-viewer/assets/skins",
                "/src/sl-client-bevy-viewer/assets/locales",
                "/src/sl-client-bevy-viewer/assets/icons",
            ]),
        );
        assert_eq!(missing, Vec::<&str>::new());
    }

    /// The `target/release/assets` case: nothing is there, so every required
    /// entry is named in the one error rather than one line per missing file.
    #[test]
    fn an_empty_root_names_every_required_entry() {
        let root = AssetRoot {
            path: PathBuf::from("/src/target/release/assets"),
            source: Source::Missing,
        };
        assert_eq!(
            missing_entries(&root, &only(&[])),
            vec!["skins", "locales", "icons"]
        );
    }

    /// A half-populated tree names only what it lacks.
    #[test]
    fn a_partial_tree_names_only_what_it_lacks() {
        let root = AssetRoot {
            path: PathBuf::from("/opt/viewer/bin/assets"),
            source: Source::BesideExecutable,
        };
        assert_eq!(
            missing_entries(
                &root,
                &only(&[
                    "/opt/viewer/bin/assets/skins",
                    "/opt/viewer/bin/assets/icons"
                ]),
            ),
            vec!["locales"]
        );
    }

    /// The real build's own crate tree is complete — the compile-time fallback
    /// this module leans on actually holds what it promises.
    #[test]
    fn the_shipped_crate_tree_is_complete() {
        let root = AssetRoot {
            path: Path::new(env!("CARGO_MANIFEST_DIR")).join("assets"),
            source: Source::CrateDirectory,
        };
        assert_eq!(
            missing_entries(&root, &|path| path.is_dir()),
            Vec::<&str>::new()
        );
    }
}
