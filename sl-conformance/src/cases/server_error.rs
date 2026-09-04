//! Probe the deprecated UDP inventory fetch and record how the grid answers —
//! the live half of the `Error` / `FeatureDisabled` decode surface.
//!
//! Neither message can be *forced* out of every grid: stock OpenSim never
//! constructs either packet (its client stack has no sender for them at all),
//! so on the local grid the genuine behaviour is "these messages never occur".
//! Second Life *can* answer a message it has blacklisted (a deprecated
//! feature the modern viewer no longer uses) with `FeatureDisabled` — the
//! reference viewer's handler literally logs "Blacklisted Feature Response".
//! The deterministic candidate a client can still send is the legacy UDP
//! `FetchInventoryDescendents` inventory fetch, which Second Life deprecated
//! in favour of the CAPS `FetchInventoryDescendents2`/AISv3 paths.
//! Empirically (aditi, 2026-08-12) Second Life takes the third road: it
//! silently drops the deprecated fetch — no `FeatureDisabled`, no `Error`,
//! no reply at all.
//!
//! The runtime's own inventory commands prefer the CAPS route whenever the
//! region serves the cap (both grids do), so the case hand-builds the raw wire
//! message and sends it via [`Command::Send`] — bypassing the cap-preferring
//! router — then records whichever of the four honest outcomes the grid
//! produces: a `FeatureDisabled` (asserted and recorded), an `Error` (ditto),
//! a normal `InventoryDescendents` reply (the grid still serves the deprecated
//! path — partial), or silence (partial on a grid documented to ignore the
//! message; a failure on OpenSim, which demonstrably serves UDP inventory).
//!
//! The fake grid takes the second road deliberately: it serves no UDP
//! inventory at all, and of the two answers a grid without that path can give,
//! only `FeatureDisabled` is observable — silence is indistinguishable from a
//! lost packet. Its
//! [`LegacyUdpInventory`](sl_fake_grid::LegacyUdpInventory) policy therefore
//! defaults to refusing the fetch, which is what makes this case assert
//! something offline instead of recording `partial` after its whole reply
//! window; the policy's other setting reproduces Second Life's silence.
//!
//! Whatever the live outcome, the *decode* of both messages is guaranteed by
//! the in-process client ↔ `SimSession` round-trip
//! (`sl-proto/tests/sim_session.rs`,
//! `session_error_and_feature_disabled_reach_client`).

use std::time::Instant;

use sl_client_tokio::{AnyMessage, Command, Event, Reliability, Throttle};
use sl_wire::messages::{
    FetchInventoryDescendents, FetchInventoryDescendentsAgentDataBlock,
    FetchInventoryDescendentsInventoryDataBlock,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, count_metric, is_opensim, secs_metric};

/// The grid's observed answer to the deprecated UDP inventory fetch.
enum ProbeOutcome {
    /// The blacklist refusal: a `FeatureDisabled` naming the refused feature.
    FeatureDisabled {
        /// The human-readable `ErrorMessage` field.
        message: String,
        /// Whether the notice's `AgentID` matched the probing agent (recorded,
        /// not asserted — some grids send a nil id here).
        agent_matches_self: bool,
    },
    /// A generic `Error` message answered the probe.
    ServerError {
        /// The HTTP-like error code.
        code: i32,
        /// The short machine-readable token.
        token: String,
        /// The originating-system path (e.g. `message/handler`).
        system: String,
        /// The human-readable description.
        message: String,
    },
    /// The grid still serves the deprecated path: a normal reply for the
    /// probed folder.
    InventoryDescendents {
        /// Immediate sub-folders in the reply.
        folders: usize,
        /// Items directly in the folder.
        items: usize,
    },
    /// No reply of any kind within the wait window.
    Silence,
}

/// Sends the deprecated UDP `FetchInventoryDescendents` for the agent's own
/// inventory root and asserts / records the grid's answer.
///
/// The struct name carries a `Case` suffix (unlike most cases) because the
/// protocol carrier type is already called
/// [`ServerError`](sl_client_tokio::Event::ServerError).
#[expect(
    clippy::module_name_repetitions,
    reason = "the bare `ServerError` name is the protocol carrier type; the case struct needs a distinct name"
)]
#[derive(Debug)]
pub struct ServerErrorCase;

impl GridTest for ServerErrorCase {
    fn name(&self) -> &'static str {
        "server-error"
    }

    fn description(&self) -> &'static str {
        "Probe the deprecated UDP inventory fetch and assert Error / FeatureDisabled handling"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Fake, Grid::Opensim, Grid::Aditi]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            // A UDP reply budget, as the regular inventory fetch sets before its
            // requests.
            session
                .send(Command::SetThrottle(Throttle::preset_1000()))
                .await?;

            // A real folder to probe: the agent's own inventory root, from the
            // locally-synthesized roots query (login-skeleton data).
            session.send(Command::QueryInventoryRoots).await?;
            let agent_root = session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::InventoryRoots { agent_root, .. } => *agent_root,
                    _ => None,
                })
                .await?;

            let agent_id = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            let session_id = session
                .session_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;

            // The probe: the deprecated UDP inventory fetch, hand-built and sent
            // raw so the runtime's cap-preferring inventory router cannot
            // re-route it over CAPS. Field-for-field the message the sans-IO
            // session itself sends on a capless grid.
            let probe = AnyMessage::FetchInventoryDescendents(FetchInventoryDescendents {
                agent_data: FetchInventoryDescendentsAgentDataBlock {
                    agent_id: agent_id.uuid(),
                    session_id,
                },
                inventory_data: FetchInventoryDescendentsInventoryDataBlock {
                    folder_id: agent_root.uuid(),
                    owner_id: agent_id.uuid(),
                    sort_order: 0, // 0 = by name
                    fetch_folders: true,
                    fetch_items: true,
                },
            });
            let started_at = Instant::now();
            session
                .send(Command::Send {
                    message: Box::new(probe),
                    reliability: Reliability::Reliable,
                })
                .await?;

            // The first probe-related event decides the outcome; a timeout is
            // itself an outcome (the grid ignored the message), not an error.
            let outcome = match session
                .wait_for(REPLY_TIMEOUT, |event| match event {
                    Event::FeatureDisabled(disabled) => Some(ProbeOutcome::FeatureDisabled {
                        message: disabled.message.clone(),
                        agent_matches_self: disabled.agent == agent_id,
                    }),
                    Event::ServerError(error) => Some(ProbeOutcome::ServerError {
                        code: error.code,
                        token: error.token.clone(),
                        system: error.system.clone(),
                        message: error.message.clone(),
                    }),
                    Event::InventoryDescendents {
                        folder_id,
                        folders,
                        items,
                        ..
                    } if *folder_id == agent_root => Some(ProbeOutcome::InventoryDescendents {
                        folders: folders.len(),
                        items: items.len(),
                    }),
                    _ => None,
                })
                .await
            {
                Ok(outcome) => outcome,
                Err(TestFailure::Timeout(_)) => ProbeOutcome::Silence,
                Err(other) => return Err(other),
            };
            let elapsed = started_at.elapsed();

            match outcome {
                ProbeOutcome::FeatureDisabled {
                    message,
                    agent_matches_self,
                } => {
                    check(
                        !message.trim().is_empty(),
                        "expected FeatureDisabled to carry a non-empty ErrorMessage",
                    )?;
                    let metrics = ctx.metrics();
                    metrics.set("reply_kind", "feature_disabled");
                    metrics.set("feature_disabled_message", message);
                    metrics.set("agent_matches_self", agent_matches_self);
                    metrics.set_timing(&secs_metric("probe_reply"), elapsed.as_secs_f64());
                }
                ProbeOutcome::ServerError {
                    code,
                    token,
                    system,
                    message,
                } => {
                    check(
                        !message.trim().is_empty(),
                        "expected Error to carry a non-empty Message",
                    )?;
                    let metrics = ctx.metrics();
                    metrics.set("reply_kind", "server_error");
                    metrics.set("server_error_code", code);
                    metrics.set("server_error_token", token);
                    metrics.set("server_error_system", system);
                    metrics.set("server_error_message", message);
                    metrics.set_timing(&secs_metric("probe_reply"), elapsed.as_secs_f64());
                }
                ProbeOutcome::InventoryDescendents { folders, items } => {
                    let folders_count = i64::try_from(folders).unwrap_or(-1);
                    let items_count = i64::try_from(items).unwrap_or(-1);
                    let metrics = ctx.metrics();
                    metrics.set("reply_kind", "inventory_descendents");
                    metrics.set(&count_metric("udp_folders"), folders_count);
                    metrics.set(&count_metric("udp_items"), items_count);
                    metrics.set_timing(&secs_metric("probe_reply"), elapsed.as_secs_f64());
                    check(
                        folders_count >= 0 && items_count >= 0,
                        "inventory count exceeded i64",
                    )?;
                    if is_opensim(grid) {
                        ctx.mark_partial(
                            "OpenSim never emits Error/FeatureDisabled (its source has no \
                             sender for either); the UDP FetchInventoryDescendents probe is \
                             answered normally — decode is covered by the sl-proto \
                             sim_session round-trip test",
                        );
                    } else {
                        ctx.mark_partial(
                            "grid answered the deprecated UDP FetchInventoryDescendents \
                             normally instead of refusing it; no Error/FeatureDisabled \
                             provoked",
                        );
                    }
                }
                ProbeOutcome::Silence => {
                    ctx.metrics().set("reply_kind", "none");
                    if is_opensim(grid) {
                        // OpenSim demonstrably serves UDP inventory; silence there
                        // is a real anomaly, not a documented gap.
                        return Err(TestFailure::Assertion(
                            "OpenSim serves UDP inventory, but the FetchInventoryDescendents \
                             probe drew no reply"
                                .to_owned(),
                        ));
                    }
                    ctx.mark_partial(
                        "grid ignored the deprecated UDP FetchInventoryDescendents probe \
                         (no reply of any kind); no Error/FeatureDisabled provoked",
                    );
                }
            }
            Ok(())
        })
    }
}
