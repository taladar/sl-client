//! Add an agent to an estate access list, verify it landed, then restore it.
//!
//! An estate keeps four per-id access lists — allowed agents, allowed groups,
//! banned agents and estate managers — that a viewer mutates one entry at a time
//! with an `EstateOwnerMessage`/`estateaccessdelta`
//! ([`Command::UpdateEstateAccess`]): a [`EstateAccessDelta`] selects the list and
//! the add/remove direction, and the target id is the agent (or group) to change.
//! The simulator applies the change and answers with the affected list(s) as
//! `setaccess` [`Event::EstateAccessList`] events (the same events `getinfo`
//! trails, keyed by [`EstateAccessKind`]).
//!
//! Editing an estate list needs estate-owner (or god) rights — OpenSim rechecks
//! `IsEstateManager`/`CanIssueEstateCommand` on every delta — so this case runs as
//! the **estate-owner** avatar (`--avatar estate-owner`), who owns the region on
//! the local grid. It is a read-modify-verify-restore cycle over *two* lists that
//! leaves the estate exactly as it found it:
//!
//! 1. Wait for the region to become active.
//! 2. Read the estate's current lists (`getinfo`) and record the allowed-agents
//!    and banned-agents membership; assert the target avatar is on neither yet.
//! 3. Add the target to the allowed-agents list, re-read, assert it is present.
//! 4. Remove it again, assert it is gone and the list is back to its start size.
//! 5. Repeat the add/remove round-trip against the banned-agents list.
//!
//! The target is a known *other* avatar, never the estate owner: OpenSim refuses
//! to place the estate owner on any list (`if (_user == EstateOwner) return;`), so
//! the case asserts the target differs from the logged-in owner up front. The
//! target need not be online — the lists are pure id sets — and the ban round-trip
//! has no eject side effect because the target is not present in the region.
//!
//! Two wire subtleties shape the drain: OpenSim defers the `setaccess` replies,
//! flushing them only once its delta queue drains (a ~500 ms batch), and an
//! allowed-agents or banned-agents change replies with *both* the allowed list and
//! the ban list together. So after each delta the case drains every
//! [`Event::EstateAccessList`] to a quiet gap and takes the latest membership per
//! [`EstateAccessKind`], rather than matching the first event of a kind (which
//! could be a stale reply left over from the previous step).
//!
//! `1av` (estate owner), `[both]`. On OpenSim's Default Region the estate starts
//! with empty allowed/banned lists, so the added avatar is each list's only member
//! and the restore returns it to empty. Second Life enforces the same estate-owner
//! gating and `estateaccessdelta` flow.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, Command, EstateAccessDelta, EstateAccessKind, EstateInfo, Event, OwnerKey, Uuid,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, fixtures, is_opensim, secs_metric,
};

/// The quiet gap (no further `setaccess`) that marks a `getinfo`/`estateaccessdelta`
/// reply's trailing access lists fully drained. OpenSim batches the deltas with a
/// ~500 ms poll, so this stays comfortably above that flush cadence.
const ACCESS_DRAIN_QUIET: Duration = Duration::from_secs(3);

/// Adds an agent to two estate access lists, verifying and restoring each.
#[derive(Debug)]
pub struct EstateAccess;

impl GridTest for EstateAccess {
    fn name(&self) -> &'static str {
        "estate-access"
    }

    fn description(&self) -> &'static str {
        "Add an agent to an estate access list, verify it, then restore it"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Resolve the "other avatar" to place on the lists. A configured
            // fixture wins (the Second Life path); OpenSim falls back to the local
            // secondary test avatar. With neither, the dataset is legitimately
            // incomplete (an aditi run with no fixture) → partial.
            let grid = ctx.grid();
            let target: AgentKey = match ctx.other_avatar() {
                Some(other) => other,
                None if is_opensim(grid) => fixtures::opensim_secondary_avatar()?,
                None => {
                    ctx.mark_partial(
                        "no other-avatar fixture configured for this grid \
                         (set `other_avatar` in fixtures.<grid>.toml)",
                    );
                    return Ok(());
                }
            };

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

            // The estate owner can never appear on any list (OpenSim short-circuits
            // `_user == EstateOwner`), so the target must be a different avatar.
            check(
                target.uuid() != agent.uuid(),
                "the estate-list target must differ from the estate owner",
            )?;

            // 2. Read the estate's current lists and confirm our rights (a reply at
            //    all proves estate-owner access) plus the clean starting state.
            let read_start = Instant::now();
            let (info, lists) = read_estate_lists(session).await?;
            let read_secs = read_start.elapsed().as_secs_f64();
            check_eq(
                "estate owner is the logged-in (estate-owner) avatar",
                &info.estate_owner,
                &agent.uuid(),
            )?;
            let initial_allowed = latest(&lists, EstateAccessKind::AllowedAgents).to_vec();
            let initial_banned = latest(&lists, EstateAccessKind::BannedAgents).to_vec();
            check(
                !initial_allowed.contains(&target.uuid()),
                "the target avatar is already on the allowed-agents list before the test added it",
            )?;
            check(
                !initial_banned.contains(&target.uuid()),
                "the target avatar is already on the banned-agents list before the test added it",
            )?;

            // 3–4. Allowed-agents round-trip: add, verify present, remove, verify
            //      gone and the list size restored.
            let allowed_start = Instant::now();
            let after_allowed_add = apply_delta(
                session,
                EstateAccessDelta::AllowedAgentAdd,
                OwnerKey::Agent(target),
                EstateAccessKind::AllowedAgents,
            )
            .await?;
            check(
                after_allowed_add.contains(&target.uuid()),
                "the target avatar was not on the allowed-agents list after the add",
            )?;
            let after_allowed_remove = apply_delta(
                session,
                EstateAccessDelta::AllowedAgentRemove,
                OwnerKey::Agent(target),
                EstateAccessKind::AllowedAgents,
            )
            .await?;
            let allowed_secs = allowed_start.elapsed().as_secs_f64();
            check(
                !after_allowed_remove.contains(&target.uuid()),
                "the target avatar was still on the allowed-agents list after the remove",
            )?;
            check_eq(
                "allowed-agents list size restored to its original",
                &after_allowed_remove.len(),
                &initial_allowed.len(),
            )?;

            // 5. Banned-agents round-trip: same add/verify/remove/restore cycle.
            let banned_start = Instant::now();
            let after_banned_add = apply_delta(
                session,
                EstateAccessDelta::BannedAgentAdd,
                OwnerKey::Agent(target),
                EstateAccessKind::BannedAgents,
            )
            .await?;
            check(
                after_banned_add.contains(&target.uuid()),
                "the target avatar was not on the banned-agents list after the add",
            )?;
            let after_banned_remove = apply_delta(
                session,
                EstateAccessDelta::BannedAgentRemove,
                OwnerKey::Agent(target),
                EstateAccessKind::BannedAgents,
            )
            .await?;
            let banned_secs = banned_start.elapsed().as_secs_f64();
            check(
                !after_banned_remove.contains(&target.uuid()),
                "the target avatar was still on the banned-agents list after the remove",
            )?;
            check_eq(
                "banned-agents list size restored to its original",
                &after_banned_remove.len(),
                &initial_banned.len(),
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("estate_lists_read"), read_secs);
            metrics.set_timing(&secs_metric("allowed_update"), allowed_secs);
            metrics.set_timing(&secs_metric("banned_update"), banned_secs);
            metrics.set("estate_name", info.estate_name.clone());
            metrics.set("estate_id", info.estate_id);
            metrics.set("target_id", target.uuid().to_string());
            metrics.set(
                "initial_allowed_count",
                i64::try_from(initial_allowed.len()).unwrap_or(-1),
            );
            metrics.set(
                "initial_banned_count",
                i64::try_from(initial_banned.len()).unwrap_or(-1),
            );
            metrics.set(
                "after_allowed_add_count",
                i64::try_from(after_allowed_add.len()).unwrap_or(-1),
            );
            metrics.set(
                "after_banned_add_count",
                i64::try_from(after_banned_add.len()).unwrap_or(-1),
            );
            Ok(())
        })
    }
}

/// Requests the estate configuration (`getinfo`) and returns it together with the
/// four trailing access lists, each as its [`EstateAccessKind`] and members.
///
/// # Errors
///
/// Propagates the send / [`Session::wait_for`] failures, or times out if the
/// simulator never answers (a non-owner gets no reply at all).
async fn read_estate_lists(
    session: &mut Session,
) -> Result<(EstateInfo, Vec<(EstateAccessKind, Vec<Uuid>)>), TestFailure> {
    session.send(Command::RequestEstateInfo).await?;
    let info: EstateInfo = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::EstateInfo(info) => Some((**info).clone()),
            _ => None,
        })
        .await?;
    let lists = drain_access_lists(session, ACCESS_DRAIN_QUIET).await?;
    Ok((info, lists))
}

/// Sends an `estateaccessdelta` and returns the resulting membership of
/// `kind_of_interest` once the (batched, possibly multi-list) `setaccess` replies
/// have drained to a quiet gap.
///
/// The latest reply for the kind wins, so a stale list left in the queue by an
/// earlier delta cannot mask this delta's result.
///
/// # Errors
///
/// Propagates the send / [`Session::wait_for`] failures, or times out if no
/// `setaccess` reply arrives.
async fn apply_delta(
    session: &mut Session,
    delta: EstateAccessDelta,
    target: OwnerKey,
    kind_of_interest: EstateAccessKind,
) -> Result<Vec<Uuid>, TestFailure> {
    session
        .send(Command::UpdateEstateAccess { delta, target })
        .await?;
    let lists = drain_access_lists(session, ACCESS_DRAIN_QUIET).await?;
    Ok(latest(&lists, kind_of_interest).to_vec())
}

/// Drains the `setaccess` [`Event::EstateAccessList`] events that trail a
/// `getinfo` or `estateaccessdelta` reply until none arrives for `quiet`,
/// returning each chunk's kind and members in arrival order.
///
/// The first event is awaited with the full [`REPLY_TIMEOUT`] (OpenSim defers the
/// delta flush ~500 ms); subsequent events only need the shorter `quiet` gap.
///
/// # Errors
///
/// Propagates a [`Session::wait_for`] disconnect, or times out if not even the
/// first list arrives.
async fn drain_access_lists(
    session: &mut Session,
    quiet: Duration,
) -> Result<Vec<(EstateAccessKind, Vec<Uuid>)>, TestFailure> {
    let mut lists = Vec::new();
    // The first reply may lag behind the delta flush, so give it the full window;
    // once one list is in hand, further ones follow closely or not at all.
    let first_timeout = REPLY_TIMEOUT;
    loop {
        let timeout = if lists.is_empty() {
            first_timeout
        } else {
            quiet
        };
        match session
            .wait_for(timeout, |event| match event {
                Event::EstateAccessList { kind, members, .. } => Some((*kind, members.clone())),
                _ => None,
            })
            .await
        {
            Ok(chunk) => lists.push(chunk),
            Err(TestFailure::Timeout(_)) => return Ok(lists),
            Err(other) => return Err(other),
        }
    }
}

/// The most recent membership reported for `kind` in a drained list batch, or an
/// empty slice if no chunk of that kind arrived.
fn latest(lists: &[(EstateAccessKind, Vec<Uuid>)], kind: EstateAccessKind) -> &[Uuid] {
    lists
        .iter()
        .rev()
        .find(|(chunk_kind, _)| *chunk_kind == kind)
        .map_or(&[], |(_, members)| members.as_slice())
}
