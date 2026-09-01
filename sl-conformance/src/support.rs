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

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, CreateGroupParams, Event, GroupKey, LindenAmount};

use crate::context::{Session, TestContext, TestFailure};
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

/// One round of the group-departure confirmation poll: how long to wait for
/// either the `AgentDropGroup` or a refreshed membership list before
/// re-requesting agent data (the overall wait is bounded by [`REPLY_TIMEOUT`]).
const GROUP_DROP_POLL: Duration = Duration::from_secs(5);

/// How long each group-creation attempt waits for a `CreateGroupReply` before
/// re-sending with a fresh per-attempt name suffix.
const GROUP_CREATE_ATTEMPT_WINDOW: Duration = Duration::from_secs(15);

/// How many creation attempts before concluding the grid genuinely refuses.
const GROUP_CREATE_ATTEMPTS: u32 = 3;

/// After a *retried* group creation answered, how long to keep watching for a
/// second `CreateGroupReply` — the late answer to an earlier attempt, which
/// means that attempt did create a group after all and nothing else will ever
/// use it.
///
/// Only entered when a retry actually happened, because the wait discards the
/// events that arrive during it (see [`dispose_of_orphan_groups`]).
const GROUP_CREATE_ORPHAN_WINDOW: Duration = Duration::from_secs(10);

/// Confirm `session`'s agent is no longer a member of `group_id`.
///
/// The membership-list confirmation differs per grid: OpenSim pushes an
/// `AgentDropGroup` ([`Event::DroppedFromGroup`])
/// after a leave or ejection, while Second Life sends no drop message for
/// either — the reference viewer re-requests agent data
/// (`sendAgentDataUpdateRequest`) and trusts the refreshed membership list.
/// Accept whichever arrives first: watch for the drop while re-requesting
/// ([`Command::RequestAgentDataUpdate`]) until the membership list no longer
/// contains the group.
///
/// # Errors
///
/// Propagates send/wait failures; times out with [`TestFailure::Timeout`]
/// when neither signal arrives within [`REPLY_TIMEOUT`].
pub async fn confirm_group_departure(
    session: &mut Session,
    group_id: sl_client_tokio::GroupKey,
) -> Result<(), TestFailure> {
    let started = Instant::now();
    loop {
        session.send(Command::RequestAgentDataUpdate).await?;
        match session
            .wait_for(GROUP_DROP_POLL, |event| match event {
                Event::DroppedFromGroup { group_id: dropped } if *dropped == group_id => Some(()),
                Event::GroupMemberships(groups)
                    if !groups.iter().any(|entry| entry.group_id == group_id) =>
                {
                    Some(())
                }
                _ => None,
            })
            .await
        {
            Ok(()) => return Ok(()),
            Err(TestFailure::Timeout(_)) if started.elapsed() < REPLY_TIMEOUT => {}
            Err(other) => return Err(other),
        }
    }
}

/// Where the group a membership/messaging case operates on came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupSource {
    /// A throwaway group created fresh for this run (the OpenSim default: free
    /// and disposable).
    Created,
    /// A pre-made group configured via [`crate::fixtures`] and reused across runs
    /// (the Second Life path: avoids the per-run L$100 group-creation fee and the
    /// founder group-slot churn).
    Premade,
}

impl GroupSource {
    /// The metric label recorded for this source.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Premade => "premade",
        }
    }
}

/// The group a membership/messaging case will operate on, plus where it came
/// from and (for a freshly created group) how long creation took.
#[derive(Clone, Copy, Debug)]
pub struct MembershipGroup {
    /// The group to drive the case against.
    pub group_id: GroupKey,
    /// Whether it was created for this run or reused from fixtures.
    pub source: GroupSource,
    /// The create round-trip time, present only when [`source`](Self::source) is
    /// [`GroupSource::Created`].
    pub create_rtt: Option<Duration>,
}

/// Resolve the `index`-th group a group case should operate on.
///
/// Prefers the [pre-made group](crate::fixtures) configured at `index` for the
/// grid — reusing stable groups avoids Second Life's per-run L$100
/// group-creation fee and the founder group-slot churn (an emptied SL group
/// purges only ~48 h after dropping below two members). When none is configured
/// at that position (the norm on the throwaway OpenSim grid), it creates a fresh
/// open-enrollment group with the given `name` and `charter`, leaving the primary
/// as founder/owner.
///
/// `index` lets a case that needs more than one distinct group take them by
/// position: the membership/messaging cases use `0`, while
/// [`super::cases::chat_invite_accept_decline`] uses `0` and `1`.
///
/// The returned group is one the **primary** owns or belongs to, so the primary
/// can drive group traffic on it; a secondary then joins it.
///
/// Keep `name` **between 4 and 35 characters** (the reference viewer's
/// `DB_GROUP_NAME_MIN_LEN`/`DB_GROUP_NAME_STR_LEN`): Second Life's server
/// silently discards a `CreateGroupRequest` whose name is over the limit — no
/// `CreateGroupReply` at all, observed live on aditi — while OpenSim accepts
/// any length. The cases use short `"slc <tag> <millis>"` names for this.
///
/// # Errors
///
/// Returns [`TestFailure`] if creating the group fails (channel closed, timeout,
/// disconnect, or the grid reporting failure).
pub async fn membership_group(
    ctx: &mut TestContext,
    index: usize,
    name: &str,
    charter: &str,
) -> Result<MembershipGroup, TestFailure> {
    if let Some(group_id) = ctx.premade_group(index) {
        return Ok(MembershipGroup {
            group_id,
            source: GroupSource::Premade,
            create_rtt: None,
        });
    }

    let session = ctx.primary();
    let created_at = Instant::now();
    // Second Life silently drops a `CreateGroupRequest` that arrives too soon
    // after another create by the same agent (observed live: a case needing
    // two groups back-to-back got only one `CreateGroupReply`), so retry with
    // a per-attempt name suffix. The wait accepts whichever attempt's reply
    // arrives first; a retry after a merely-slow first reply can still leave an
    // orphan single-member group, which the disposal below names and leaves.
    let mut attempt: u32 = 0;
    let (group_id, create_ok, create_message) = loop {
        attempt = attempt.saturating_add(1);
        let attempt_name = if attempt == 1 {
            name.to_owned()
        } else {
            format!("{name} a{attempt}")
        };
        session
            .send(Command::CreateGroup(CreateGroupParams {
                name: attempt_name,
                charter: charter.to_owned(),
                show_in_list: false,
                insignia_id: None,
                membership_fee: LindenAmount(0),
                open_enrollment: true,
                allow_publish: false,
                mature_publish: false,
            }))
            .await?;
        match session
            .wait_for(GROUP_CREATE_ATTEMPT_WINDOW, |event| match event {
                Event::CreateGroupResult {
                    group_id,
                    success,
                    message,
                } => Some((*group_id, *success, message.clone())),
                _ => None,
            })
            .await
        {
            Ok(reply) => break reply,
            Err(TestFailure::Timeout(_)) if attempt < GROUP_CREATE_ATTEMPTS => {}
            Err(other) => return Err(other),
        }
    };
    let create_rtt = created_at.elapsed();
    check(
        create_ok,
        &format!("group creation failed: {create_message}"),
    )?;

    // A retry that raced a merely-slow first reply leaves an orphan group behind
    // — on Second Life that is L$100 and a founder group slot per orphan. Give
    // the late reply a moment to arrive so the orphan is at least named, and ask
    // to leave it (a group its founder has left drops to zero members and the
    // grid purges it).
    if attempt > 1 {
        let orphans = dispose_of_orphan_groups(ctx.primary(), group_id).await?;
        if !orphans.is_empty() {
            let listed = orphans
                .iter()
                .map(|orphan| orphan.uuid().to_string())
                .collect::<Vec<_>>()
                .join(",");
            tracing::warn!(
                "group creation retried {attempt} times and left {} orphan group(s): {listed}",
                orphans.len()
            );
            let metrics = ctx.metrics();
            metrics.set(
                &count_metric("orphan_group"),
                i64::try_from(orphans.len()).unwrap_or(i64::MAX),
            );
            metrics.set("orphan_groups", listed);
        }
    }

    Ok(MembershipGroup {
        group_id,
        source: GroupSource::Created,
        create_rtt: Some(create_rtt),
    })
}

/// Collect the groups an earlier creation attempt created after all — every
/// `CreateGroupReply` other than `kept`'s that still arrives within
/// [`GROUP_CREATE_ORPHAN_WINDOW`] — and ask to leave each one.
///
/// The departure is issued but not awaited: the point is to stop owning the
/// orphan, and a second wait here would discard yet more of the caller's events.
/// The ids are returned so the caller can name them in the log and the record;
/// on a grid that refuses to let a lone owner leave, that log line is the only
/// trace an operator has to clean up by hand.
///
/// Note this *does* consume events: [`Session::wait_for`] drops what does not
/// match, so this runs only on the retry path, where an orphan is possible.
///
/// # Errors
///
/// Propagates a [`Session::send`] failure; a timeout is the expected, quiet
/// outcome (no late reply, hence no orphan).
async fn dispose_of_orphan_groups(
    session: &mut Session,
    kept: GroupKey,
) -> Result<Vec<GroupKey>, TestFailure> {
    let mut orphans: Vec<GroupKey> = Vec::new();
    let started = Instant::now();
    loop {
        let remaining = GROUP_CREATE_ORPHAN_WINDOW.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        match session
            .wait_for(remaining, |event| match event {
                Event::CreateGroupResult {
                    group_id,
                    success,
                    message: _,
                } if *success && *group_id != kept => Some(*group_id),
                _ => None,
            })
            .await
        {
            Ok(orphan) => {
                if !orphans.contains(&orphan) {
                    orphans.push(orphan);
                }
            }
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    for orphan in &orphans {
        session.send(Command::LeaveGroup(*orphan)).await?;
    }
    Ok(orphans)
}

/// Well-known ids and labels reused across cases.
pub mod fixtures {
    use sl_client_tokio::{AgentKey, TextureKey, Uuid};

    use crate::context::TestFailure;

    /// The standard SL/OpenSim "plywood" default texture, present on any stock
    /// grid; used by `asset-decode` as a guaranteed-fetchable asset. Taken from
    /// the protocol crate rather than restated, so a case fetches the id the
    /// renderer falls back to.
    pub const PLYWOOD_TEXTURE: Uuid = sl_client_tokio::DEFAULT_PRIM_TEXTURE;

    /// The local OpenSim "Default Region" UUID, from this workspace's
    /// `Regions/Regions.ini` (the region at grid location 1000,1000).
    ///
    /// OpenSim-only and specific to the local test grid; Second Life regions
    /// have their own ids.
    pub const OPENSIM_DEFAULT_REGION: &str = "11111111-2222-3333-4444-555555555555";

    /// The conventional credentials-file label for the estate-owner avatar that
    /// estate/land-edit cases log in as (`--avatar estate-owner`).
    pub const ESTATE_OWNER_LABEL: &str = "estate-owner";

    /// The local OpenSim secondary test avatar (`Friend Tester`), created with a
    /// fixed UUID on this workspace's grid. The `avatar-properties` case reads
    /// *this* avatar's profile as a known "other avatar" on OpenSim — the account
    /// exists (so the profile service answers) and need not be logged in. Second
    /// Life has no such built-in second avatar, so the aditi run reads the
    /// `other_avatar` configured in `fixtures.aditi.toml` instead.
    pub const OPENSIM_SECONDARY_AVATAR: &str = "bbbbbbbb-aaaa-cccc-dddd-000000000001";

    /// The OpenSim secondary test avatar as a typed [`AgentKey`].
    ///
    /// # Errors
    ///
    /// Returns [`TestFailure::Assertion`] if the constant is malformed.
    pub fn opensim_secondary_avatar() -> Result<AgentKey, TestFailure> {
        Ok(AgentKey::from(uuid(OPENSIM_SECONDARY_AVATAR)?))
    }

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
    #[must_use]
    pub fn plywood_texture() -> TextureKey {
        TextureKey::from(PLYWOOD_TEXTURE)
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
        let _region = fixtures::uuid(fixtures::OPENSIM_DEFAULT_REGION)?;
        assert!(matches!(fixtures::uuid("not-a-uuid"), Err(_failure)));
        assert_eq!(
            fixtures::plywood_texture().uuid(),
            fixtures::PLYWOOD_TEXTURE
        );
        Ok(())
    }
}
