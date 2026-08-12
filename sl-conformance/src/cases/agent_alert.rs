//! Provoke and decode the two simulator notice channels: the agent-addressed
//! `AgentAlertMessage` and the broadcast-style `AlertMessage`.
//!
//! Two deterministic provocations, one per channel:
//!
//! 1. **Set-Home** (`SetStartLocationRequest`, [`Command::SetStartLocation`]
//!    with the Home slot): every outcome is answered with an
//!    `AgentAlertMessage` — "Home position set." when the parcel allows it,
//!    or the not-allowed refusal otherwise — so the reply is deterministic
//!    regardless of land ownership. The case accepts either notice channel
//!    (a grid may answer with a keyed `AlertMessage` instead) and asserts
//!    the decoded notice is non-empty.
//! 2. **Estate map regeneration** (`EstateOwnerMessage` /
//!    `refreshmapvisibility`, hand-built and sent via [`Command::Send`] —
//!    there is deliberately no typed command for this admin nudge): with
//!    estate rights every branch of OpenSim's handler answers a plain
//!    `AlertMessage` ("Terrain map generated", the 2-minute cool-down
//!    notice, or a generator-unavailable notice), so any reply exercises
//!    the broadcast channel. Without estate rights the request is silently
//!    refused — like the other estate cases the OpenSim run must therefore
//!    use the **estate-owner** avatar (`--avatar estate-owner`); on Second
//!    Life the test avatar holds no estate power, so this half records a
//!    partial instead.

use std::time::Instant;

use sl_client_tokio::{
    AnyMessage, Command, Event, RegionCoordinates, Reliability, StartLocationSlot, Uuid, Vector,
};
use sl_wire::messages::{
    EstateOwnerMessage, EstateOwnerMessageAgentDataBlock, EstateOwnerMessageMethodDataBlock,
    EstateOwnerMessageParamListBlock,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, count_metric, is_opensim, secs_metric};

/// A decoded notice from either alert channel.
struct Notice {
    /// Which channel carried it (`"agent_alert"` / `"alert"`).
    kind: &'static str,
    /// The human-readable message, or the first keyed alert id when the
    /// message text is empty.
    message: String,
    /// `AgentAlertMessage` only: whether the notice was addressed to the
    /// logged-in agent.
    addressed_to_self: Option<bool>,
    /// `AgentAlertMessage` only: the modal flag.
    modal: Option<bool>,
    /// `AlertMessage` only: how many structured `AlertInfo` entries
    /// accompanied the text.
    alert_info_count: Option<usize>,
}

/// Sets Home and pokes the estate map generator, asserting each notice
/// channel decodes.
#[derive(Debug)]
pub struct AgentAlert;

impl GridTest for AgentAlert {
    fn name(&self) -> &'static str {
        "agent-alert"
    }

    fn description(&self) -> &'static str {
        "Provoke AgentAlertMessage (Set-Home) and AlertMessage (estate map regen) and decode both"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;

            let agent_id = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            let session_id = session
                .session_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;

            // Half 1 — Set-Home. Every outcome (stored or refused) is answered
            // with a notice; accept either channel and keep whichever arrives.
            let started_at = Instant::now();
            session
                .send(Command::SetStartLocation {
                    slot: StartLocationSlot::Home,
                    position: RegionCoordinates::new(128.0, 128.0, 30.0),
                    look_at: Vector {
                        x: 1.0,
                        y: 0.0,
                        z: 0.0,
                    },
                })
                .await?;
            let home_notice = match session
                .wait_for(REPLY_TIMEOUT, |event| notice_from(event, agent_id))
                .await
            {
                Ok(notice) => Some(notice),
                Err(TestFailure::Timeout(_)) => None,
                Err(other) => return Err(other),
            };
            let home_elapsed = started_at.elapsed();

            // Half 2 — estate map regeneration. Every branch of the handler
            // replies with a plain AlertMessage — but only with estate rights;
            // without them the request is silently dropped.
            let started_at = Instant::now();
            let poke = AnyMessage::EstateOwnerMessage(EstateOwnerMessage {
                agent_data: EstateOwnerMessageAgentDataBlock {
                    agent_id: agent_id.uuid(),
                    session_id,
                    transaction_id: Uuid::nil(),
                },
                method_data: EstateOwnerMessageMethodDataBlock {
                    method: b"refreshmapvisibility\0".to_vec(),
                    invoice: Uuid::nil(),
                },
                // The method takes no parameters; mirror the session's own
                // encoder, which always sends one (empty) block.
                param_list: vec![EstateOwnerMessageParamListBlock {
                    parameter: Vec::new(),
                }],
            });
            session
                .send(Command::Send {
                    message: Box::new(poke),
                    reliability: Reliability::Reliable,
                })
                .await?;
            let map_notice = match session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::AlertMessage { .. } => notice_from(event, agent_id),
                    _ => None,
                })
                .await
            {
                Ok(notice) => Some(notice),
                Err(TestFailure::Timeout(_)) => None,
                Err(other) => return Err(other),
            };
            let map_elapsed = started_at.elapsed();

            // Record and assert. Each observed notice must decode to a
            // non-empty message; a missing notice is a failure on OpenSim
            // (both provocations are deterministic there, given the
            // estate-owner avatar) and an honestly-recorded gap on a grid
            // where the avatar lacks the standing to provoke it.
            let metrics = ctx.metrics();
            metrics.set("home_notice_seen", home_notice.is_some());
            metrics.set("map_notice_seen", map_notice.is_some());
            if let Some(notice) = &home_notice {
                metrics.set("home_notice_kind", notice.kind);
                metrics.set("home_notice_message", notice.message.clone());
                if let Some(modal) = notice.modal {
                    metrics.set("home_notice_modal", modal);
                }
                if let Some(addressed) = notice.addressed_to_self {
                    metrics.set("home_notice_addressed_to_self", addressed);
                }
                metrics.set_timing(&secs_metric("home_notice"), home_elapsed.as_secs_f64());
            }
            if let Some(notice) = &map_notice {
                metrics.set("map_notice_message", notice.message.clone());
                if let Some(info_count) = notice.alert_info_count {
                    metrics.set(
                        &count_metric("map_notice_alert_info"),
                        i64::try_from(info_count).unwrap_or(-1),
                    );
                }
                metrics.set_timing(&secs_metric("map_notice"), map_elapsed.as_secs_f64());
            }
            for notice in home_notice.iter().chain(map_notice.iter()) {
                check(
                    !notice.message.trim().is_empty(),
                    "expected the notice to carry a non-empty message (or a keyed alert id)",
                )?;
            }
            match (&home_notice, &map_notice) {
                (Some(_), Some(_)) => {}
                (None, _) if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "Set-Home drew no AgentAlertMessage/AlertMessage on OpenSim".to_owned(),
                    ));
                }
                (_, None) if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "estate map regeneration drew no AlertMessage on OpenSim (run as the \
                         estate-owner avatar)"
                            .to_owned(),
                    ));
                }
                (None, None) => {
                    ctx.mark_partial(
                        "neither provocation drew a notice on this grid (Set-Home unanswered; \
                         no estate rights for the map-regeneration AlertMessage)",
                    );
                }
                (Some(notice), None) => {
                    ctx.mark_partial(&format!(
                        "Set-Home notice exercised (as {}); the estate map-regeneration \
                         AlertMessage needs estate rights the test avatar lacks on this grid",
                        notice.kind
                    ));
                }
                (None, Some(_)) => {
                    ctx.mark_partial(
                        "AlertMessage exercised via the estate map regeneration; Set-Home drew \
                         no notice on this grid",
                    );
                }
            }
            Ok(())
        })
    }
}

/// Decodes an alert event of either channel into a [`Notice`], or `None` for
/// any other event.
fn notice_from(event: &Event, self_id: sl_client_tokio::AgentKey) -> Option<Notice> {
    match event {
        Event::AgentAlertMessage {
            agent_id,
            modal,
            message,
        } => Some(Notice {
            kind: "agent_alert",
            message: message.clone(),
            addressed_to_self: Some(*agent_id == self_id),
            modal: Some(*modal),
            alert_info_count: None,
        }),
        Event::AlertMessage {
            message,
            alert_info,
            ..
        } => {
            // A keyed-only alert has an empty plain message; surface the first
            // structured alert id instead so the record shows what arrived.
            let text = if message.trim().is_empty() {
                alert_info
                    .first()
                    .map(|info| info.message.clone())
                    .unwrap_or_default()
            } else {
                message.clone()
            };
            Some(Notice {
                kind: "alert",
                message: text,
                addressed_to_self: None,
                modal: None,
                alert_info_count: Some(alert_info.len()),
            })
        }
        _ => None,
    }
}
