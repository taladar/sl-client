//! Read a parcel's access (allow) list, add an entry, then restore it.
//!
//! A parcel's *access list* (the AL_ACCESS "allow" list) and its *ban list*
//! (AL_BAN) are the two per-avatar lists a land owner keeps to gate entry. The
//! viewer reads either with a UDP `ParcelAccessListRequest`
//! ([`Command::RequestParcelAccessList`]) — selecting the list by
//! [`ParcelAccessScope`] — answered by a `ParcelAccessListReply`
//! ([`Event::ParcelAccessList`]); and replaces a whole list with a
//! `ParcelAccessListUpdate` ([`Command::UpdateParcelAccessList`]), where an empty
//! entry set clears it.
//!
//! Updating a parcel's lists needs land-edit rights, so this case runs as the
//! **estate-owner** avatar (`--avatar estate-owner`), who owns the region-wide
//! parcel on the local grid. The flow is a read-modify-verify-restore cycle that
//! leaves the parcel exactly as it found it:
//!
//! 1. Wait for the region to become active.
//! 2. Learn the region-centre parcel's *region-local* id (and confirm we own it)
//!    from a `ParcelPropertiesRequest` reply.
//! 3. Read the current allow list *and* ban list (the two [`ParcelAccessScope`]s)
//!    and record their sizes.
//! 4. Add one entry (a known other avatar) to the allow list and re-read it,
//!    asserting the entry is now present.
//! 5. Restore the allow list to exactly its original entries and re-read it,
//!    asserting the added entry is gone again.
//!
//! Each `ParcelAccessListUpdate` *replaces* the whole list for its scope, so the
//! runtime mints a fresh transaction id per update; without that the reference
//! simulator would append to the list instead of clearing it first (the runtime
//! handling of [`Command::UpdateParcelAccessList`]), and neither the add nor the
//! restore would round-trip cleanly.
//!
//! A simulator represents an *empty* list as a single nil-agent placeholder
//! block, which the client drops on decode (as the reference viewer does), so an
//! empty list surfaces as zero entries here.
//!
//! `1av`, `[both]`. On OpenSim's Default Region the single region-wide parcel is
//! owned by the estate owner and starts with empty allow/ban lists, so the added
//! entry is the list's only member and the restore clears it back to empty.
//! Second Life enforces the same message flow.

use std::time::Instant;

use sl_client_tokio::{
    AgentKey, Command, Event, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelInfo,
    RegionLocalParcelId, ScopedParcelId,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    LONG_TIMEOUT, REGION_TIMEOUT, REPLY_TIMEOUT, check, check_eq, fixtures, is_opensim, secs_metric,
};

/// The western/southern edge of the queried square, in region metres — a 4×4 m
/// square centred on the region centre (128, 128), so the reply describes the
/// parcel at the middle of the region.
const SQUARE_WEST_SOUTH: f32 = 124.0;

/// The eastern/northern edge of the queried square, in region metres (see
/// [`SQUARE_WEST_SOUTH`]).
const SQUARE_EAST_NORTH: f32 = 128.0;

/// A distinctive sequence id, echoed back in the `ParcelProperties` reply so the
/// awaited reply is *our* query's answer and not an unsolicited on-entry one.
/// Distinct from the other Phase 10 cases' ids so the three never alias.
const SEQUENCE_ID: i32 = 5152;

/// Reads and updates a parcel's access (allow) list, restoring it afterwards.
#[derive(Debug)]
pub struct ParcelAccessList;

impl GridTest for ParcelAccessList {
    fn name(&self) -> &'static str {
        "parcel-access-list"
    }

    fn description(&self) -> &'static str {
        "Read a parcel's access list, add an entry, then restore it"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // Resolve the "other avatar" to place on the list. A configured
            // fixture wins (the Second Life path); OpenSim falls back to the
            // local secondary test avatar. With neither, the dataset is
            // legitimately incomplete (an aditi run with no fixture) → partial.
            let grid = ctx.grid();
            let entry_agent: AgentKey = match ctx.other_avatar() {
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

            let circuit = session.circuit_id().ok_or_else(|| {
                TestFailure::Assertion("login established no root circuit id".to_owned())
            })?;
            let agent = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

            // 1. Learn the parcel's region-local id and owner from a
            //    ParcelProperties reply (the access-list requests are keyed on the
            //    region-local id, and the update needs us to own the parcel).
            session
                .send(Command::RequestParcelProperties {
                    west: SQUARE_WEST_SOUTH,
                    south: SQUARE_WEST_SOUTH,
                    east: SQUARE_EAST_NORTH,
                    north: SQUARE_EAST_NORTH,
                    sequence_id: SEQUENCE_ID,
                })
                .await?;
            let parcel: ParcelInfo = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::ParcelProperties(parcel) if parcel.sequence_id == SEQUENCE_ID => {
                        Some((**parcel).clone())
                    }
                    _ => None,
                })
                .await?;
            check(
                parcel.request_result.has_data(),
                &format!(
                    "parcel query returned no data (request_result: {:?})",
                    parcel.request_result
                ),
            )?;
            let local_id = parcel.local_id;
            let scoped = ScopedParcelId::new(circuit, local_id);
            check_eq(
                "parcel owner is the logged-in (estate-owner) avatar",
                &parcel.owner.uuid(),
                &agent.uuid(),
            )?;

            // 2. Read the current allow and ban lists.
            let allow_start = Instant::now();
            let initial_allow =
                read_access_list(session, scoped, local_id, ParcelAccessScope::Access).await?;
            let allow_read_elapsed = allow_start.elapsed().as_secs_f64();
            let initial_ban =
                read_access_list(session, scoped, local_id, ParcelAccessScope::Ban).await?;
            check(
                !initial_allow
                    .iter()
                    .any(|entry| entry.id == entry_agent.uuid()),
                "the other avatar is already on the allow list before the test added it",
            )?;

            // 3. Add the other avatar to the allow list, then re-read it and
            //    assert the entry landed.
            let added_entry = ParcelAccessEntry {
                id: entry_agent.uuid(),
                // Never expires.
                time: 0,
                // Just the list scope; no experience sub-flags.
                flags: ParcelAccessFlags::NONE,
            };
            let update_start = Instant::now();
            session
                .send(Command::UpdateParcelAccessList {
                    local_id: scoped,
                    scope: ParcelAccessScope::Access,
                    entries: vec![added_entry],
                })
                .await?;
            let after_add =
                read_access_list(session, scoped, local_id, ParcelAccessScope::Access).await?;
            let update_elapsed = update_start.elapsed().as_secs_f64();
            check(
                after_add.iter().any(|entry| entry.id == entry_agent.uuid()),
                "the added avatar was not on the allow list after the update",
            )?;

            // 4. Restore the allow list to exactly its original entries (an empty
            //    original clears it) and confirm the added entry is gone.
            session
                .send(Command::UpdateParcelAccessList {
                    local_id: scoped,
                    scope: ParcelAccessScope::Access,
                    entries: initial_allow.clone(),
                })
                .await?;
            let after_restore =
                read_access_list(session, scoped, local_id, ParcelAccessScope::Access).await?;
            check(
                !after_restore
                    .iter()
                    .any(|entry| entry.id == entry_agent.uuid()),
                "the added avatar was still on the allow list after the restore",
            )?;
            check_eq(
                "allow list size restored to its original",
                &after_restore.len(),
                &initial_allow.len(),
            )?;

            let metrics = ctx.metrics();
            metrics.set_timing(&secs_metric("access_list_read"), allow_read_elapsed);
            metrics.set_timing(&secs_metric("access_list_update"), update_elapsed);
            metrics.set("local_id", i64::from(local_id.0));
            metrics.set("owner_id", parcel.owner.uuid().to_string());
            metrics.set(
                "initial_allow_count",
                i64::try_from(initial_allow.len()).unwrap_or(-1),
            );
            metrics.set(
                "initial_ban_count",
                i64::try_from(initial_ban.len()).unwrap_or(-1),
            );
            metrics.set(
                "after_add_count",
                i64::try_from(after_add.len()).unwrap_or(-1),
            );
            metrics.set(
                "after_restore_count",
                i64::try_from(after_restore.len()).unwrap_or(-1),
            );
            Ok(())
        })
    }
}

/// Requests a parcel's `scope` (allow or ban) list and returns its entries.
///
/// # Errors
///
/// Propagates the send / [`Session::wait_for`] failures, or times out if the
/// simulator never answers with a matching [`Event::ParcelAccessList`].
async fn read_access_list(
    session: &mut Session,
    scoped: ScopedParcelId,
    local_id: RegionLocalParcelId,
    scope: ParcelAccessScope,
) -> Result<Vec<ParcelAccessEntry>, TestFailure> {
    session
        .send(Command::RequestParcelAccessList {
            local_id: scoped,
            scope,
        })
        .await?;
    session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ParcelAccessList {
                local_id: reply_id,
                scope: reply_scope,
                entries,
            } if reply_id.id() == local_id && *reply_scope == scope => Some(entries.clone()),
            _ => None,
        })
        .await
}
