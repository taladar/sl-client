//! Save the About Land form of a parcel the agent has no rights over, and
//! record what the grid answers — the refusal half of
//! `gridspec-parcel-management`.
//!
//! [`super::parcel_edit`] needs land the agent may edit, which on Second Life
//! means owned land the test avatars do not have. A refused edit needs nothing
//! but a parcel someone else owns, so this case runs as the **primary** avatar
//! on both grids, and it is the one measurement of the `ParcelPropertiesUpdate`
//! capability aditi can give: whether a refusal is an HTTP error, an alert, or
//! nothing at all.
//!
//! 1. Read the parcel under the agent (a distinctive sequence id, as in
//!    `parcel-edit`), then ask for it by its region-local id and record whether
//!    the grid answers (`by_id_answered`) — the one place aditi can measure
//!    that, since `parcel-edit` cannot run there.
//! 2. Save a form with only the name changed, then watch for
//!    ten seconds for the alerts a refusal may push, and for the parcel
//!    echo an accepted edit gets. The watch is a `wait_for` that never
//!    matches: the point is what it sees on the way.
//! 3. Re-read the parcel by the same square query (OpenSim ignores the by-id
//!    form) and check the name is unchanged. A grid that took the edit means the avatar *can* edit this
//!    land; the case puts the name back and records `partial`, since it then
//!    measured nothing about a refusal.
//!
//! What the grid answered goes into the record: `edit_transport` (capability
//! or UDP), `capability_rejected` (whether the POST came back as an HTTP
//! error, which the client reports as an `ExpectedReplyMissing` diagnostic),
//! `alerts` (every alert text that arrived in the window) and
//! `echo_sequence_id` (the sequence id of the parcel the region pushed back
//! anyway, or `none`).
//!
//! `1av`, `[both]`, live only: the fake grid enforces no land rights.

use std::time::Duration;

use sl_client_tokio::ScopedParcelId;

use sl_client_tokio::{Command, Diagnostic, Event, ParcelInfo, RegionLocalParcelId};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::registry::{GridTest, TestFuture};
use crate::support::{LONG_TIMEOUT, REGION_TIMEOUT, check_eq};

/// The western/southern edge of the queried square, in region metres — the
/// same 4×4 m square at the region centre `parcel-edit` reads.
const SQUARE_WEST_SOUTH: f32 = 124.0;

/// The eastern/northern edge of the queried square, in region metres.
const SQUARE_EAST_NORTH: f32 = 128.0;

/// A distinctive sequence id for the first query; the refetch uses the next
/// one. Distinct from every other case's ids so the replies never alias.
const SEQUENCE_ID: i32 = 5371;

/// The name the refused edit tries to give the parcel.
const NEW_NAME: &str = "SLClientParcelEditRefusedTest";

/// How long the by-id probe waits for an answer before recording that the grid
/// ignored it.
const BY_ID_WINDOW: Duration = Duration::from_secs(5);

/// How long to watch for the alerts a refusal may push after the edit.
const WATCH_WINDOW: Duration = Duration::from_secs(10);

/// What a grid answers to a refused edit: whether the capability POST failed,
/// and the sequence id of the parcel pushed back, if any. Neither grid says
/// no: Second Life answers the POST, alerts nothing and pushes the unchanged
/// parcel back under the reference viewer's `SELECTED_PARCEL_SEQ_ID`; OpenSim
/// answers the POST and sends nothing at all.
const REFUSAL: Measured<(bool, Option<i32>)> = Measured {
    second_life: (false, Some(-10_000)),
    opensim: (false, None),
    source: "parcel-edit-refused on aditi and OpenSim (2026-10-05, book/src/gridspec/land.md)",
};

/// The capability whose refused POST the client reports as a diagnostic.
const CAPABILITY: &str = "ParcelPropertiesUpdate";

/// Saves the About Land form of land the agent does not own, recording the
/// refusal.
#[derive(Debug)]
pub struct ParcelEditRefused;

impl GridTest for ParcelEditRefused {
    fn name(&self) -> &'static str {
        "parcel-edit-refused"
    }

    fn description(&self) -> &'static str {
        "Save the About Land form of a parcel the agent cannot edit and record the refusal"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            // 1. The parcel as it stands.
            let original = read_parcel(session, SEQUENCE_ID).await?;
            if !original.request_result.has_data() {
                ctx.mark_partial("the agent is standing on land the region has no parcel for");
                return Ok(());
            }
            let circuit = session.circuit_id().ok_or_else(|| {
                TestFailure::Assertion("login established no root circuit id".to_owned())
            })?;
            session
                .send(Command::RequestParcelPropertiesById {
                    local_id: ScopedParcelId::new(circuit, original.local_id),
                    sequence_id: SEQUENCE_ID + 2,
                })
                .await?;
            let by_id_answered = match session
                .wait_for(BY_ID_WINDOW, |event| match event {
                    Event::ParcelProperties(parcel) if parcel.sequence_id == SEQUENCE_ID + 2 => {
                        Some(())
                    }
                    _ => None,
                })
                .await
            {
                Ok(()) => true,
                Err(TestFailure::Timeout(_)) => false,
                Err(other) => return Err(other),
            };
            super::parcel_edit::BY_ID_ANSWERED.check(
                "by-id parcel request answered",
                grid,
                &by_id_answered,
            )?;

            // 2. The edit, and what the grid pushes after it.
            let transport = if session.cap(CAPABILITY).is_some() {
                "capability"
            } else {
                "udp"
            };
            let mut edited = original.to_update();
            NEW_NAME.clone_into(&mut edited.name);
            session
                .send(Command::UpdateParcel(Box::new(edited)))
                .await?;
            let (alerts, echoed) = watch(session, original.local_id).await?;
            let capability_rejected = session.diagnostics().iter().any(|diagnostic| {
                matches!(
                    diagnostic,
                    Diagnostic::ExpectedReplyMissing { request, .. } if request == CAPABILITY
                )
            });

            // 3. Did the name change?
            let after = read_parcel(session, SEQUENCE_ID + 1).await?;
            let metrics = ctx.metrics();
            metrics.set("edit_transport", transport);
            metrics.set("capability_rejected", capability_rejected);
            metrics.set("alerts", alerts.join(" | "));
            metrics.set("by_id_answered", by_id_answered);
            metrics.set(
                "echo_sequence_id",
                echoed.map_or_else(|| "none".to_owned(), |sequence| sequence.to_string()),
            );
            if after.name == original.name {
                REFUSAL.check(
                    "refused edit (POST failed, echo sequence id)",
                    grid,
                    &(capability_rejected, echoed),
                )?;
            } else {
                let session = ctx.primary();
                session
                    .send(Command::UpdateParcel(Box::new(original.to_update())))
                    .await?;
                let restored = super::parcel_edit::await_echo(session, original.local_id).await?;
                check_eq("restored parcel name", &restored.name, &original.name)?;
                ctx.mark_partial("the grid took the edit: the avatar can edit this land");
            }
            Ok(())
        })
    }
}

/// Collects the text of every alert that arrives in [`WATCH_WINDOW`], and the
/// sequence id of the first push of the parcel `local_id` in it. The predicate
/// never matches, so the wait always ends in its own timeout, which is the
/// expected outcome here.
async fn watch(
    session: &mut Session,
    local_id: RegionLocalParcelId,
) -> Result<(Vec<String>, Option<i32>), TestFailure> {
    let mut alerts = Vec::new();
    let mut echoed = None;
    let outcome = session
        .wait_for(WATCH_WINDOW, |event| {
            match event {
                Event::ParcelProperties(parcel) if parcel.local_id == local_id => {
                    echoed = echoed.or(Some(parcel.sequence_id));
                }
                Event::AlertMessage {
                    message,
                    alert_info,
                    ..
                } => {
                    // A keyed-only alert has an empty plain message; its first
                    // structured id says what arrived.
                    let text = if message.trim().is_empty() {
                        alert_info
                            .first()
                            .map(|info| info.message.clone())
                            .unwrap_or_default()
                    } else {
                        message.clone()
                    };
                    alerts.push(text);
                }
                Event::AgentAlertMessage { message, .. } => alerts.push(message.clone()),
                _ => {}
            }
            None::<()>
        })
        .await;
    match outcome {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok((alerts, echoed)),
        Err(other) => Err(other),
    }
}

/// Reads the parcel at the region centre under `sequence_id`, waiting for the
/// reply that echoes it.
async fn read_parcel(session: &mut Session, sequence_id: i32) -> Result<ParcelInfo, TestFailure> {
    session
        .send(Command::RequestParcelProperties {
            west: SQUARE_WEST_SOUTH,
            south: SQUARE_WEST_SOUTH,
            east: SQUARE_EAST_NORTH,
            north: SQUARE_EAST_NORTH,
            sequence_id,
            snap_selection: false,
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
