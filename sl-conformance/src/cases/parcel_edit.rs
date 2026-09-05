//! Save the About Land form and read the parcel back, then do the same to its
//! ban list — the parcel half of `test-fake-grid-edit-surfaces`.
//!
//! The About Land floater is the one edit surface with a shape all its own. An
//! object has two records travelling in two messages and a change to either is
//! pushed on its own; a parcel has **one** record, and a
//! `ParcelPropertiesUpdate` ([`Command::UpdateParcel`]) carries the whole of it
//! — a viewer starts from what it last read
//! ([`ParcelInfo::to_update`]), sets the one field the resident touched, and
//! sends every other field back exactly as it found them. So the round trip
//! this case measures is not "did the field I changed arrive" but "did the
//! record survive being sent back": a simulator that dropped or rewrote any
//! part of it would show up as the *unchanged* fields coming back different.
//!
//! The flow leaves the parcel exactly as it found it:
//!
//! 1. Wait for the region, then read the parcel under the agent with a
//!    [`Command::RequestParcelProperties`] carrying a distinctive sequence id,
//!    so the reply is this query's answer and not the unsolicited one an
//!    arrival pushes.
//! 2. Save an About Land form built from that record with the name, description
//!    and category changed, and re-read the parcel **by its region-local id**
//!    ([`Command::RequestParcelPropertiesById`]) — the refetch that proves the
//!    write reached the region rather than only the floater.
//! 3. Read the ban list ([`Command::RequestParcelAccessList`]), add an entry,
//!    re-read it, then restore it to exactly what it held. An empty list
//!    travels as a single nil-agent placeholder, which the client drops on
//!    decode, so an empty list reads as zero entries.
//! 4. Save the original form back and confirm the name returned to what it was.
//!
//! `1av`, `[both]`, and **offline**. Editing land needs land rights, so a live
//! run has to be the **estate-owner** avatar (`--avatar estate-owner`), the same
//! requirement [`super::estate_info`] carries and for the same reason; the fake
//! grid enforces no permissions, so its primary avatar suffices. A grid that
//! answers the first query with no data — the agent is standing on land nobody
//! has parcelled — is recorded `partial` rather than failed.

use std::time::Instant;

use sl_client_tokio::{
    Command, Event, ParcelAccessEntry, ParcelAccessFlags, ParcelAccessScope, ParcelCategory,
    ParcelInfo, RegionLocalParcelId, ScopedParcelId, Uuid,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check, check_eq, secs_metric};

/// The western/southern edge of the queried square, in region metres — a 4×4 m
/// square centred on the region centre, so the reply describes the parcel in
/// the middle of the region.
const SQUARE_WEST_SOUTH: f32 = 124.0;

/// The eastern/northern edge of the queried square, in region metres.
const SQUARE_EAST_NORTH: f32 = 128.0;

/// A distinctive sequence id for the first query, echoed back in the reply so
/// the awaited record is this query's answer and not the unsolicited push an
/// arrival produces. Distinct from every other case's ids so the two never
/// alias.
const SEQUENCE_ID: i32 = 5361;

/// The name the edit gives the parcel.
const NEW_NAME: &str = "SLClientParcelEditTest";

/// The description the edit gives the parcel.
const NEW_DESCRIPTION: &str = "edited by the parcel-edit conformance case";

/// The agent id the ban-list edit adds. A synthetic id rather than a real
/// avatar: a ban list is a list of ids and a simulator does not resolve them,
/// so a fixture avatar would add a dependency the assertion does not need.
const BANNED_AGENT: Uuid = Uuid::from_u128(0x0000_0000_0000_4000_8000_0000_5ec0_1dea);

/// Saves the About Land form and its ban list, reading each back and restoring.
#[derive(Debug)]
pub struct ParcelEdit;

impl GridTest for ParcelEdit {
    fn name(&self) -> &'static str {
        "parcel-edit"
    }

    fn description(&self) -> &'static str {
        "Save a parcel's About Land form and ban list, refetch each, and restore"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::Fake]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let circuit = session.circuit_id().ok_or_else(|| {
                TestFailure::Assertion("login established no root circuit id".to_owned())
            })?;

            // 1. The parcel as it stands.
            session
                .send(Command::RequestParcelProperties {
                    west: SQUARE_WEST_SOUTH,
                    south: SQUARE_WEST_SOUTH,
                    east: SQUARE_EAST_NORTH,
                    north: SQUARE_EAST_NORTH,
                    sequence_id: SEQUENCE_ID,
                })
                .await?;
            let original: ParcelInfo = session
                .wait_for(LONG_TIMEOUT, |event| match event {
                    Event::ParcelProperties(parcel) if parcel.sequence_id == SEQUENCE_ID => {
                        Some((**parcel).clone())
                    }
                    _ => None,
                })
                .await?;
            if !original.request_result.has_data() {
                ctx.mark_partial("the agent is standing on land the region has no parcel for");
                return Ok(());
            }
            let local_id = original.local_id;
            let scoped = ScopedParcelId::new(circuit, local_id);

            // 2. The edit: the record as read, with three fields changed. Every
            //    other field goes back exactly as it came, which is what makes
            //    the refetch below a test of the record and not of one field.
            let edit_started = Instant::now();
            let mut edited = original.to_update();
            NEW_NAME.clone_into(&mut edited.name);
            NEW_DESCRIPTION.clone_into(&mut edited.description);
            edited.category = ParcelCategory::Residential;
            session.send(Command::UpdateParcel(edited)).await?;
            let saved = refetch(session, scoped, SEQUENCE_ID + 1).await?;
            let edit_rtt = edit_started.elapsed();
            check_eq("edited parcel name", &saved.name, &NEW_NAME.to_owned())?;
            check_eq(
                "edited parcel description",
                &saved.description,
                &NEW_DESCRIPTION.to_owned(),
            )?;
            check_eq(
                "edited parcel category",
                &saved.category,
                &ParcelCategory::Residential,
            )?;
            // The fields the form re-asserted rather than changed have to come
            // back as they went out: a simulator that rewrote one of them would
            // be quietly editing land nobody asked it to.
            check_eq(
                "re-asserted landing type",
                &saved.landing_type,
                &original.landing_type,
            )?;
            check_eq(
                "re-asserted parcel flags",
                &saved.raw_parcel_flags,
                &original.raw_parcel_flags,
            )?;

            // 3. The ban list, which is the one parcel record that does not
            //    travel in the properties reply.
            let ban_started = Instant::now();
            let initial_ban = read_access_list(session, scoped, local_id).await?;
            check(
                !initial_ban.iter().any(|entry| entry.id == BANNED_AGENT),
                "the test's synthetic agent was already on the ban list",
            )?;
            let mut with_ban = initial_ban.clone();
            with_ban.push(ParcelAccessEntry {
                id: BANNED_AGENT,
                // Never expires.
                time: 0,
                // Just the list scope; no experience sub-flags.
                flags: ParcelAccessFlags::NONE,
            });
            session
                .send(Command::UpdateParcelAccessList {
                    local_id: scoped,
                    scope: ParcelAccessScope::Ban,
                    entries: with_ban,
                })
                .await?;
            let after_ban = read_access_list(session, scoped, local_id).await?;
            let ban_rtt = ban_started.elapsed();
            check(
                after_ban.iter().any(|entry| entry.id == BANNED_AGENT),
                "the banned agent was not on the ban list after the update",
            )?;

            // 4. Put both back the way they were found.
            session
                .send(Command::UpdateParcelAccessList {
                    local_id: scoped,
                    scope: ParcelAccessScope::Ban,
                    entries: initial_ban.clone(),
                })
                .await?;
            let restored_ban = read_access_list(session, scoped, local_id).await?;
            check(
                !restored_ban.iter().any(|entry| entry.id == BANNED_AGENT),
                "the banned agent was still on the ban list after the restore",
            )?;

            session
                .send(Command::UpdateParcel(original.to_update()))
                .await?;
            let restored = refetch(session, scoped, SEQUENCE_ID + 2).await?;
            check_eq("restored parcel name", &restored.name, &original.name)?;

            let metrics = ctx.metrics();
            metrics.set("parcel_local_id", i64::from(local_id.0));
            metrics.set("parcel_name", original.name.clone());
            metrics.set(
                "ban_list_entries_before",
                i64::try_from(initial_ban.len()).unwrap_or(-1),
            );
            metrics.set_timing(&secs_metric("about_land_rtt"), edit_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("ban_list_rtt"), ban_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// Re-reads the parcel by its region-local id under `sequence_id`, waiting for
/// the reply that echoes it — the refetch that says the write reached the
/// region rather than only the floater.
async fn refetch(
    session: &mut Session,
    local_id: ScopedParcelId,
    sequence_id: i32,
) -> Result<ParcelInfo, TestFailure> {
    session
        .send(Command::RequestParcelPropertiesById {
            local_id,
            sequence_id,
        })
        .await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ParcelProperties(parcel) if parcel.sequence_id == sequence_id => {
                Some((**parcel).clone())
            }
            _ => None,
        })
        .await
}

/// Reads the parcel's ban list.
async fn read_access_list(
    session: &mut Session,
    scoped: ScopedParcelId,
    local_id: RegionLocalParcelId,
) -> Result<Vec<ParcelAccessEntry>, TestFailure> {
    session
        .send(Command::RequestParcelAccessList {
            local_id: scoped,
            scope: ParcelAccessScope::Ban,
        })
        .await?;
    session
        .wait_for(LONG_TIMEOUT, |event| match event {
            Event::ParcelAccessList {
                local_id: replied,
                scope: ParcelAccessScope::Ban,
                entries,
            } if replied.id() == local_id => Some(entries.clone()),
            _ => None,
        })
        .await
}
