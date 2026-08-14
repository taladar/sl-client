//! Build-time viewer identity: name, crate version, and the `git describe`
//! string `build.rs` embeds — the single source for the version the viewer
//! reports to the grid at login and shows in the About floater.

/// The viewer's user-facing application name (also the default login channel).
pub(crate) const VIEWER_NAME: &str = "sl-client-bevy-viewer";

/// The crate version from `Cargo.toml`.
pub(crate) const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The `git describe --tags --always --dirty` string embedded by `build.rs`,
/// or `None` when the viewer was built outside a git checkout (e.g. from a
/// source tarball).
pub(crate) const GIT_DESCRIBE: Option<&str> = option_env!("SL_VIEWER_GIT_DESCRIBE");

/// The Bevy version locked in `Cargo.lock`, embedded by `build.rs`.
pub(crate) const BEVY_VERSION: Option<&str> = option_env!("SL_VIEWER_BEVY_VERSION");

/// The wgpu version locked in `Cargo.lock`, embedded by `build.rs`.
pub(crate) const WGPU_VERSION: Option<&str> = option_env!("SL_VIEWER_WGPU_VERSION");

/// The build profile the viewer was compiled with (`debug` / `release`).
pub(crate) const BUILD_PROFILE: &str = if cfg!(debug_assertions) {
    "debug"
} else {
    "release"
};

/// The full viewer version: the crate version, extended with the build-time
/// git describe as semver-style build metadata (`0.1.0+ed81459-dirty`) when
/// one was embedded and it adds information beyond the crate version itself.
pub(crate) fn full_version() -> String {
    version_with_describe(CRATE_VERSION, GIT_DESCRIBE)
}

/// [`full_version`] over explicit inputs, for unit testing: appends
/// `+<describe>` unless the describe output is absent or already equal to the
/// crate version (a build exactly on a release tag).
fn version_with_describe(crate_version: &str, describe: Option<&str>) -> String {
    match describe {
        Some(describe) if describe != crate_version => format!("{crate_version}+{describe}"),
        _ => crate_version.to_owned(),
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::version_with_describe;

    /// A plain checkout build appends the describe output as build metadata.
    #[test]
    fn version_appends_describe() {
        assert_eq!(
            version_with_describe("0.1.0", Some("ed81459-dirty")),
            "0.1.0+ed81459-dirty"
        );
    }

    /// A tarball build (no git metadata) reports the crate version alone.
    #[test]
    fn version_without_describe_is_crate_version() {
        assert_eq!(version_with_describe("0.1.0", None), "0.1.0");
    }

    /// A build exactly on the release tag does not duplicate the version.
    #[test]
    fn version_on_tag_is_not_duplicated() {
        assert_eq!(version_with_describe("0.1.0", Some("0.1.0")), "0.1.0");
    }
}
