//! Resolve agent ids to their **display names** over the `GetDisplayNames`
//! capability.
//!
//! A *display name* is the mutable, user-chosen name an avatar shows in world,
//! layered over the immutable *legacy* `First Last` identity the UDP
//! `UUIDNameRequest` path resolves. Display names live behind an HTTP capability
//! rather than UDP: a viewer batches a set of agent ids into one
//! `GetDisplayNames` GET and decodes the `{ agents, bad_ids }` LLSD reply. This
//! case drives that lookup with [`Command::RequestDisplayNames`], batching the
//! agent's own id together with a second known avatar into a single request, and
//! asserts the reply ([`Event::DisplayNames`]) resolves the agent's own id to a
//! real record — the observable protocol effect of the capability.
//!
//! **This case only ever *reads*.** The client has no "set display name" command
//! at all — the sole display-name commands are the `GetDisplayNames` lookup and
//! observing the CAPS-pushed `DisplayNameUpdate` / `SetDisplayNameReply`. That
//! matters because Second Life rate-limits *changing* a display name (a multi-day
//! per-avatar cooldown); a lookup carries no such limit, so the case is safe to
//! re-run freely and never touches the cooldown.
//!
//! The agent's own id is guaranteed resolvable, so it anchors the assertions: the
//! reply must contain a non-[`missing`](sl_client_tokio::DisplayName::missing)
//! record for it, with a non-empty username, legacy name, and display name. A
//! second avatar is added to the batch to exercise multi-id resolution — the
//! `other_avatar` fixture on Second Life, or the local secondary test avatar
//! (`Friend Tester`, a fixed-UUID account) on OpenSim — but its resolution is
//! best-effort (recorded, not asserted), since a grid returns an unknown id in
//! `bad_ids` (or, on stock OpenSim, silently omits it) rather than failing.
//!
//! `1av`. The capability is Second-Life-centric — the legend lists Display Names
//! under SL-only — but stock OpenSim *does* serve `GetDisplayNames` whenever its
//! user-management component is present (`BunchOfCaps.cs`), returning the legacy
//! name as a default (`is_display_name_default = true`) display name. So the read
//! round-trip is assertable on both grids; where a grid omits the capability
//! entirely the command is a silent no-op and the case records `partial` on the
//! resulting timeout rather than failing.

use std::time::Instant;

use sl_client_tokio::{Command, Event};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, count_metric, fixtures, is_opensim, secs_metric,
};

/// Resolves agent ids to display names over the `GetDisplayNames` capability.
#[derive(Debug)]
pub struct DisplayNames;

impl GridTest for DisplayNames {
    fn name(&self) -> &'static str {
        "display-names"
    }

    fn description(&self) -> &'static str {
        "Resolve agent ids to display names over the GetDisplayNames capability"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Resolve a *second* avatar to batch alongside the agent's own id, so
            // the request exercises multi-id resolution. A configured fixture wins
            // (the Second Life path); otherwise OpenSim falls back to the local
            // secondary test avatar. With neither (an aditi run without the
            // fixture) the batch is just the agent's own id — still a valid lookup.
            let grid = ctx.grid();
            let other = match ctx.other_avatar() {
                Some(other) => Some(other),
                None if is_opensim(grid) => Some(fixtures::opensim_secondary_avatar()?),
                None => None,
            };

            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let own = session.agent_id().ok_or_else(|| {
                TestFailure::Assertion("primary login reported no agent id".to_owned())
            })?;

            // Batch the agent's own id with the second avatar when it differs.
            let other_id = other.filter(|id| *id != own);
            let other_requested = other_id.is_some();
            let mut ids = vec![own];
            if let Some(id) = other_id {
                ids.push(id);
            }
            let requested = ids.len();

            let started = Instant::now();
            session.send(Command::RequestDisplayNames(ids)).await?;
            let names = match session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::DisplayNames(names) => Some(names.clone()),
                    _ => None,
                })
                .await
            {
                Ok(names) => names,
                Err(TestFailure::Timeout(_)) => {
                    // The grid does not serve GetDisplayNames (the command is a
                    // silent no-op when the region seed omits the capability), so
                    // there is no reply to assert against — record partial.
                    ctx.mark_partial(
                        "grid did not answer the GetDisplayNames lookup (capability \
                         absent from the region seed)",
                    );
                    return Ok(());
                }
                Err(other) => return Err(other),
            };
            let rtt = started.elapsed();

            // The agent's own id is always a known account, so the reply must
            // resolve it to a real (non-missing) record — the observable effect of
            // the capability.
            let Some(own_record) = names.iter().find(|record| record.id == own) else {
                return Err(TestFailure::Assertion(format!(
                    "GetDisplayNames reply ({} record(s)) did not resolve the \
                     agent's own id {own}",
                    names.len(),
                )));
            };
            check(
                !own_record.missing,
                "the agent's own id came back as a missing (bad_ids) placeholder",
            )?;
            check_eq("own record id", &own_record.id, &own)?;
            check(
                !own_record.username.is_empty(),
                "the resolved own record has an empty username",
            )?;
            check(
                !own_record.legacy_name().is_empty(),
                "the resolved own record has an empty legacy name",
            )?;
            check(
                !own_record.display_name.is_empty(),
                "the resolved own record has an empty display name",
            )?;

            // The second avatar's resolution is best-effort: a grid returns an
            // unknown id in bad_ids (or omits it), so record the outcome without
            // asserting it.
            let other_resolved = other_id.is_some_and(|id| {
                names
                    .iter()
                    .any(|record| record.id == id && !record.missing)
            });
            let resolved = names.iter().filter(|record| !record.missing).count();

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("lookup_rtt"), rtt.as_secs_f64());
            metrics.set(
                &count_metric("requested"),
                i64::try_from(requested).unwrap_or(-1),
            );
            metrics.set(
                &count_metric("resolved"),
                i64::try_from(resolved).unwrap_or(-1),
            );
            metrics.set("own_display_name", own_record.display_name.clone());
            metrics.set("own_username", own_record.username.clone());
            metrics.set("own_legacy_name", own_record.legacy_name());
            metrics.set(
                "own_is_display_name_default",
                own_record.is_display_name_default,
            );
            metrics.set("other_requested", other_requested);
            metrics.set("other_resolved", other_resolved);
            Ok(())
        })
    }
}
