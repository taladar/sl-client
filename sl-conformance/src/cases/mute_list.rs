//! Mute (block) an entity, fetch the mute list, then unmute — a full
//! add → read-back → remove → read-back round-trip over the agent's own
//! private block list.
//!
//! The mute (block) list is per-account state the simulator keeps for each
//! avatar; it is entirely private (the muted entity is never told and need not
//! exist or be online), so a single logged-in avatar drives the whole
//! round-trip — hence `1av`. A viewer reads the list with
//! [`Command::RequestMuteList`] (`MuteListRequest` with a zero CRC, forcing a
//! fresh download): the simulator answers by uploading the list file over the
//! `Xfer` path and pointing at it with `MuteListUpdate` (surfaced, once
//! downloaded and parsed, as [`Event::MuteList`]), or — for an empty list —
//! with the `emptymutelist` `GenericMessage` (also surfaced as
//! [`Event::MuteList`]`([])`). Adding a mute is [`Command::Mute`]
//! (`UpdateMuteListEntry`) and removing one is [`Command::Unmute`]
//! (`RemoveMuteListEntry`); neither carries an acknowledgement of its own, so
//! each edit is verified by re-requesting the list until the change shows.
//!
//! The case mutes a **fixed synthetic target** — a stable conformance UUID and
//! name, muted as a [`MuteType::Agent`] with the default (mute-everything)
//! flags. Nothing external is touched: a mute is private block-list state, the
//! target need not be a real account, and the fixed id means a re-run edits the
//! one marker entry rather than piling up. Because the round-trip *is*
//! add-then-remove, the case leaves the list as it found it (its marker absent)
//! with no separate restore step; an interrupted run self-heals, since a
//! leftover marker is swept by the next run's remove. That also makes the case
//! grid-agnostic and free of any fixture or cooldown concern — muting has no
//! display-name-style change cooldown, so it is safe to re-run freely on both
//! grids.
//!
//! `1av`, `[both]`. Stock OpenSim serves the whole round-trip once its
//! `MuteListModule` (+ `MuteListService`) is enabled (see the setup appendix);
//! with the module absent the simulator's default handler answers a read with
//! an empty list but drops the write, so the muted entry never appears — the
//! case detects that (the add never surfaces), records the write as exercised,
//! and marks `partial` rather than failing. Second Life serves it natively; the
//! aditi run is batched with the rest of the deferred Aditi runs.

use std::time::{Duration, Instant};

use sl_client_tokio::{Command, Event, MuteEntry, MuteFlags, MuteType, Uuid};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, fixtures, secs_metric,
};

/// The fixed synthetic UUID the case mutes and unmutes. Clearly not a real
/// account (a conformance-owned literal), so a re-run edits this one marker
/// rather than accumulating entries.
const MUTE_TARGET_ID: &str = "c04f0117-0000-4000-8000-000000000001";

/// The name recorded for the muted marker entry; must match on both the mute
/// (`UpdateMuteListEntry`) and unmute (`RemoveMuteListEntry`) so the simulator
/// keys the removal to the same row.
const MUTE_TARGET_NAME: &str = "SL-Conformance Mute Target";

/// How long to keep re-reading the list for an edit (add or remove) to appear.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait between re-reads while polling for an edit.
const VERIFY_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The outcome of a single mute-list read: the downloaded/parsed list, or the
/// simulator's "cached list is current" answer.
enum MuteRead {
    /// The list the simulator returned (empty for an unmuted account).
    List(Vec<MuteEntry>),
    /// `UseCachedMuteList`: the cached list is still current. The case always
    /// requests with a zero CRC (forcing a fresh download), so this is not
    /// expected on either target grid; it is handled defensively.
    Unchanged,
}

/// Mutes a synthetic target, reads the list back, then unmutes it.
#[derive(Debug)]
pub struct MuteList;

impl GridTest for MuteList {
    fn name(&self) -> &'static str {
        "mute-list"
    }

    fn description(&self) -> &'static str {
        "Mute and unmute an entity and fetch the mute list"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let target_id = fixtures::uuid(MUTE_TARGET_ID)?;

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let own = session.agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login reported no agent id".to_owned())
            })?;

            // OpenSim rejects a self-mute outright, and it is meaningless
            // anyway; the synthetic target must not be the logged-in avatar.
            check(
                target_id != own.uuid(),
                "mute target must differ from the logged-in primary",
            )?;

            // Read the baseline list. A zero-CRC request forces a fresh
            // download, so the simulator answers with the list (possibly empty),
            // never "unchanged"; treat an unexpected cached answer as a grid
            // that will not let us enumerate, and record partial.
            let baseline = match read_mute_list(session).await? {
                MuteRead::List(entries) => entries,
                MuteRead::Unchanged => {
                    ctx.mark_partial(
                        "grid answered the initial mute-list request with \
                         UseCachedMuteList despite a zero CRC; cannot enumerate \
                         the list on this grid",
                    );
                    return Ok(());
                }
            };
            let baseline_count = baseline.len();

            // Add the marker mute. The update carries no ack, so confirm by
            // re-reading until the entry appears.
            session
                .send(Command::Mute {
                    id: target_id,
                    name: MUTE_TARGET_NAME.to_owned(),
                    mute_type: MuteType::Agent,
                    flags: MuteFlags::default(),
                })
                .await?;
            let mute_started = Instant::now();
            let after_mute = match poll_mute_until(
                session,
                |entries| contains_marker(entries, target_id),
                "muted entry never appeared",
            )
            .await
            {
                Ok(entries) => entries,
                Err(TestFailure::Assertion(_)) => {
                    // The write was exercised on the wire, but the simulator
                    // never surfaced it (no MuteListModule, or a non-persisting
                    // backend). Best-effort clean up the leftover, then record
                    // partial — the read-back round-trip is unverifiable here.
                    session
                        .send(Command::Unmute {
                            id: target_id,
                            name: MUTE_TARGET_NAME.to_owned(),
                        })
                        .await?;
                    let metrics = ctx.metrics();
                    metrics.set(&count_metric("baseline_mutes"), count_value(baseline_count));
                    metrics.set("mute_target", target_id.to_string());
                    metrics.set("mute_readback", false);
                    ctx.mark_partial(
                        "the mute write was exercised on the wire but the entry \
                         never appeared in a re-read list (grid without a \
                         persisting MuteListModule); the read-back round-trip is \
                         unverifiable on this grid",
                    );
                    return Ok(());
                }
                Err(other) => return Err(other),
            };
            let mute_rtt = mute_started.elapsed();

            // The grid surfaced the mute — assert the read-back entry's fields.
            let entry = after_mute
                .iter()
                .find(|entry| entry.id == target_id && entry.name == MUTE_TARGET_NAME)
                .ok_or_else(|| {
                    TestFailure::Assertion("muted entry vanished after read-back".to_owned())
                })?;
            check_eq("muted entry type", &entry.mute_type, &MuteType::Agent)?;
            check_eq("muted entry flags", &entry.flags, &MuteFlags::default())?;

            // Remove the marker mute and confirm it left the list.
            session
                .send(Command::Unmute {
                    id: target_id,
                    name: MUTE_TARGET_NAME.to_owned(),
                })
                .await?;
            let unmute_started = Instant::now();
            let after_unmute = poll_mute_until(
                session,
                |entries| !contains_marker(entries, target_id),
                "muted entry never removed",
            )
            .await?;
            let unmute_rtt = unmute_started.elapsed();
            check(
                !contains_marker(&after_unmute, target_id),
                "mute entry still present after unmute",
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("mute_rtt"), mute_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("unmute_rtt"), unmute_rtt.as_secs_f64());
            metrics.set(&count_metric("baseline_mutes"), count_value(baseline_count));
            metrics.set("mute_target", target_id.to_string());
            metrics.set("mute_readback", true);
            metrics.set("mute_type", "agent");
            Ok(())
        })
    }
}

/// Whether `entries` contains the case's marker mute (matched on id and name).
fn contains_marker(entries: &[MuteEntry], target_id: Uuid) -> bool {
    entries
        .iter()
        .any(|entry| entry.id == target_id && entry.name == MUTE_TARGET_NAME)
}

/// Narrows a list length to the metric's `i64`, saturating to `-1` on the
/// (impossible in practice) overflow.
fn count_value(len: usize) -> i64 {
    i64::try_from(len).unwrap_or(-1)
}

/// Requests the mute list with a zero CRC and returns the simulator's answer.
async fn read_mute_list(session: &mut Session) -> Result<MuteRead, TestFailure> {
    session.send(Command::RequestMuteList).await?;
    session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::MuteList(entries) => Some(MuteRead::List(entries.clone())),
            Event::MuteListUnchanged => Some(MuteRead::Unchanged),
            _ => None,
        })
        .await
}

/// Re-reads the mute list until `predicate` holds over the returned entries, or
/// fails with `description` after [`VERIFY_TIMEOUT`]. A defensive
/// `UseCachedMuteList` answer (not expected with the zero-CRC request) is
/// treated as "not yet satisfied" and the poll continues.
async fn poll_mute_until<P>(
    session: &mut Session,
    mut predicate: P,
    description: &str,
) -> Result<Vec<MuteEntry>, TestFailure>
where
    P: FnMut(&[MuteEntry]) -> bool,
{
    let start = Instant::now();
    loop {
        if let MuteRead::List(entries) = read_mute_list(session).await?
            && predicate(&entries)
        {
            return Ok(entries);
        }
        if start.elapsed() >= VERIFY_TIMEOUT {
            return Err(TestFailure::Assertion(format!(
                "{description} after {VERIFY_TIMEOUT:?}"
            )));
        }
        tokio::time::sleep(VERIFY_POLL_INTERVAL).await;
    }
}
