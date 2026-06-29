//! Shared scaffolding so the concrete cases stay short and consistent.
//!
//! This is the "Phase 0" helper layer of the test roadmap (`TEST_ROADMAP.md`):
//!
//! - standard [timeout constants](self#constants) tuned for live grids,
//! - a [`send_then_wait`] send-then-await-matching-event combinator,
//! - [grid-gating helpers](is_opensim) for per-grid conditionals,
//! - [`check`] / [`check_eq`] assertion helpers that wrap
//!   [`TestFailure::Assertion`] with a clear message,
//! - [metric-name helpers](secs_metric) for the conventional `_secs` / `_count`
//!   suffixes,
//! - a [`fixtures`] module of well-known ids.

use std::time::Duration;

use sl_client_tokio::{Command, Event};

use crate::context::{Session, TestFailure};
use crate::grid::Grid;

/// Generous timeout for the initial region handshake; covers an aditi login,
/// MFA, and a slow region cross.
pub const REGION_TIMEOUT: Duration = Duration::from_secs(60);

/// Default timeout for a single request/reply round-trip over the circuit.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// Longer timeout for replies that stream, page, or arrive over a CAPS/HTTP
/// path rather than a single UDP packet.
pub const LONG_TIMEOUT: Duration = Duration::from_secs(60);

/// Send `command`, then await the first event for which `predicate` returns
/// `Some`, up to `timeout`.
///
/// The common shape of almost every case: issue one command and wait for its
/// reply. Wraps [`Session::send`] + [`Session::wait_for`].
///
/// # Errors
///
/// Propagates [`Session::send`] and [`Session::wait_for`] errors (a closed
/// channel, a timeout, or an intervening disconnect).
pub async fn send_then_wait<T, P>(
    session: &mut Session,
    command: Command,
    timeout: Duration,
    predicate: P,
) -> Result<T, TestFailure>
where
    P: FnMut(&Event) -> Option<T>,
{
    session.send(command).await?;
    session.wait_for(timeout, predicate).await
}

/// Whether the test is running on the local OpenSim grid.
///
/// Cases that branch on grid (e.g. asserting an OpenSim-only field, or marking
/// partial on aditi) read more clearly with these than with a bare `match`.
#[must_use]
pub const fn is_opensim(grid: Grid) -> bool {
    matches!(grid, Grid::Opensim)
}

/// Whether the test is running on the Second Life beta (aditi) grid.
#[must_use]
pub const fn is_aditi(grid: Grid) -> bool {
    matches!(grid, Grid::Aditi)
}

/// Assert `condition`, failing the test with `message` as a
/// [`TestFailure::Assertion`] when it does not hold.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] when `condition` is false.
pub fn check(condition: bool, message: &str) -> Result<(), TestFailure> {
    if condition {
        Ok(())
    } else {
        Err(TestFailure::Assertion(message.to_owned()))
    }
}

/// Assert that `actual` equals `expected`, failing with a formatted
/// `field: expected … got …` message naming the field under test.
///
/// Prefer this over [`check`] when comparing an observed protocol field to a
/// known value, so the failure record says what was wrong, not just that
/// something was.
///
/// # Errors
///
/// Returns [`TestFailure::Assertion`] when `actual != expected`.
pub fn check_eq<T>(field: &str, actual: &T, expected: &T) -> Result<(), TestFailure>
where
    T: PartialEq + core::fmt::Debug,
{
    if actual == expected {
        Ok(())
    } else {
        Err(TestFailure::Assertion(format!(
            "{field}: expected {expected:?}, got {actual:?}"
        )))
    }
}

/// The conventional name for a timing metric: `<base>_secs`, which the reporter
/// renders as "lower is better".
#[must_use]
pub fn secs_metric(base: &str) -> String {
    format!("{base}_secs")
}

/// The conventional name for a count metric: `<base>_count`.
#[must_use]
pub fn count_metric(base: &str) -> String {
    format!("{base}_count")
}

/// Well-known ids and labels reused across cases.
pub mod fixtures {
    use sl_client_tokio::{TextureKey, Uuid};

    use crate::context::TestFailure;

    /// The standard SL/OpenSim "plywood" default texture, present on any stock
    /// grid; used by `asset-decode` as a guaranteed-fetchable asset.
    pub const PLYWOOD_TEXTURE: &str = "89556747-24cb-43ed-920b-47caed15465f";

    /// The local OpenSim "Default Region" UUID, from this workspace's
    /// `Regions/Regions.ini` (the region at grid location 1000,1000).
    ///
    /// OpenSim-only and specific to the local test grid; Second Life regions
    /// have their own ids.
    pub const OPENSIM_DEFAULT_REGION: &str = "11111111-2222-3333-4444-555555555555";

    /// The conventional credentials-file label for the estate-owner avatar that
    /// estate/land-edit cases log in as (`--avatar estate-owner`).
    pub const ESTATE_OWNER_LABEL: &str = "estate-owner";

    /// Parse a well-known UUID literal, failing the test on a malformed value.
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Assertion`] if `literal` is not a valid UUID.
    pub fn uuid(literal: &str) -> Result<Uuid, TestFailure> {
        literal
            .parse()
            .map_err(|_invalid| TestFailure::Assertion(format!("bad fixture uuid: {literal}")))
    }

    /// The plywood default texture as a typed [`TextureKey`].
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Assertion`] if the constant is malformed.
    pub fn plywood_texture() -> Result<TextureKey, TestFailure> {
        Ok(TextureKey::from(uuid(PLYWOOD_TEXTURE)?))
    }
}

#[cfg(test)]
mod tests {
    use super::{check, check_eq, count_metric, fixtures, is_aditi, is_opensim, secs_metric};
    use crate::context::TestFailure;
    use crate::grid::Grid;
    use pretty_assertions::assert_eq;

    /// `check` passes a true condition and fails a false one with its message.
    #[test]
    fn check_reports_message() {
        assert!(matches!(check(true, "ok"), Ok(())));
        assert!(matches!(
            check(false, "boom"),
            Err(TestFailure::Assertion(message)) if message == "boom"
        ));
    }

    /// `check_eq` formats field, expected, and actual on mismatch.
    #[test]
    fn check_eq_formats_mismatch() {
        assert!(matches!(check_eq("n", &3_i32, &3_i32), Ok(())));
        assert!(matches!(
            check_eq("max_agents", &10_i32, &40_i32),
            Err(TestFailure::Assertion(message))
                if message == "max_agents: expected 40, got 10"
        ));
    }

    /// Metric-name helpers apply the conventional suffixes.
    #[test]
    fn metric_name_suffixes() {
        assert_eq!(secs_metric("region_info"), "region_info_secs");
        assert_eq!(count_metric("folders"), "folders_count");
    }

    /// Grid-gating predicates are mutually exclusive.
    #[test]
    fn grid_gating() {
        assert!(is_opensim(Grid::Opensim));
        assert!(!is_aditi(Grid::Opensim));
        assert!(is_aditi(Grid::Aditi));
        assert!(!is_opensim(Grid::Aditi));
    }

    /// The fixture UUID constants parse, and the typed accessor matches.
    #[test]
    fn fixtures_parse() -> Result<(), crate::context::TestFailure> {
        let plywood = fixtures::uuid(fixtures::PLYWOOD_TEXTURE)?;
        let _region = fixtures::uuid(fixtures::OPENSIM_DEFAULT_REGION)?;
        assert!(matches!(fixtures::uuid("not-a-uuid"), Err(_failure)));
        let texture = fixtures::plywood_texture()?;
        assert_eq!(texture.uuid(), plywood);
        Ok(())
    }
}
