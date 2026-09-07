//! The version handshake — the first thing a scripted attachment asks.
//!
//! An RLV object opens with `@version=<channel>` and refuses to work if the
//! answer does not look like a viewer it knows. The reply is two versions in
//! one string: the **specification** the viewer implements, and the RLVa
//! **implementation** that implements it. This crate answers for the
//! specification the state machine implements — RLV 3.4.3, with a 2.9.28
//! compatibility floor for objects written against the older spec.
//!
//! Reference (Firestorm, read-only): `rlvdefines.h:31-46` (the version
//! constants) and `rlvcommon.cpp:431-457` (`RlvStrings::getVersion`).

/// The RLV specification version this crate implements, as
/// `(major, minor, patch, build)`.
pub const RLV_VERSION: (u32, u32, u32, u32) = (3, 4, 3, 0);

/// The older RLV specification version reported to an object in compatibility
/// mode, as `(major, minor, patch, build)`.
///
/// Some long-lived scripted objects refuse to run against a version they were
/// not written for; reporting the floor keeps them working.
pub const RLV_VERSION_COMPAT: (u32, u32, u32, u32) = (2, 9, 28, 0);

/// The RLVa implementation version, as `(major, minor, patch)`.
pub const RLVA_VERSION: (u32, u32, u32) = (2, 4, 2);

/// The RLVa implementation id, reported by `@versionnum:impl`.
pub const RLVA_IMPL_ID: u32 = 13;

/// The specification version to report, honouring compatibility mode.
const fn spec(compatibility: bool) -> (u32, u32, u32, u32) {
    if compatibility {
        RLV_VERSION_COMPAT
    } else {
        RLV_VERSION
    }
}

/// The `@version` / `@versionnew` reply.
///
/// `@version` is the legacy spelling and answers `RestrainedLife`;
/// `@versionnew` answers `RestrainedLove`. The distinction is historical and
/// scripts match on it, so both spellings have to survive.
///
/// ```
/// # use sl_rlv::version_reply;
/// assert_eq!(
///     version_reply(true, false),
///     "RestrainedLife viewer v3.4.3 (RLVa 2.4.2)"
/// );
/// assert_eq!(
///     version_reply(false, true),
///     "RestrainedLove viewer v2.9.28 (RLVa 2.4.2)"
/// );
/// ```
#[must_use]
pub fn version_reply(legacy: bool, compatibility: bool) -> String {
    let name = if legacy {
        "RestrainedLife"
    } else {
        "RestrainedLove"
    };
    let (major, minor, patch, _build) = spec(compatibility);
    let (impl_major, impl_minor, impl_patch) = RLVA_VERSION;
    format!(
        "{name} viewer v{major}.{minor}.{patch} \
         (RLVa {impl_major}.{impl_minor}.{impl_patch})"
    )
}

/// The `@versionnum` reply — the specification version packed as
/// `<major><minor><patch><build>` with two digits each.
///
/// ```
/// # use sl_rlv::version_num_reply;
/// assert_eq!(version_num_reply(false), "3040300");
/// assert_eq!(version_num_reply(true), "2092800");
/// ```
#[must_use]
pub fn version_num_reply(compatibility: bool) -> String {
    let (major, minor, patch, build) = spec(compatibility);
    format!("{major}{minor:02}{patch:02}{build:02}")
}

/// The `@versionnum:impl` reply — the RLVa implementation version packed the
/// same way, with the implementation id in the build position.
///
/// ```
/// # use sl_rlv::version_impl_num_reply;
/// assert_eq!(version_impl_num_reply(), "2040213");
/// ```
#[must_use]
pub fn version_impl_num_reply() -> String {
    let (major, minor, patch) = RLVA_VERSION;
    let id = RLVA_IMPL_ID;
    format!("{major}{minor:02}{patch:02}{id:02}")
}
