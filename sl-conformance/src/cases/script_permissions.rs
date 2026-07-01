//! Receive a scripted-object permission request (`llRequestPermissions`), grant
//! it, read the grant back from the mirror, then revoke it.
//!
//! A script asks the agent for LSL permissions with `llRequestPermissions`: the
//! simulator sends a `ScriptQuestion` naming the holding object, its script item,
//! the owner and the requested `PERMISSION_*` bitfield. The agent answers with a
//! `ScriptAnswerYes` granting a subset (an empty subset is an explicit deny), and
//! may later withdraw a grant with `RevokePermissions`. `sl-proto` keeps a
//! **local mirror** of what the agent answered — never a security boundary, the
//! simulator stays authoritative — readable through
//! [`Command::QueryScriptPermissions`], which the runtime answers by synthesizing
//! an [`Event::ScriptPermissionState`] snapshot (no wire traffic).
//!
//! This case exercises all three edges. It waits for the request the Default
//! Region's scripted test prim (`SLClientScriptTester`, the Phase-8 #8 fixture —
//! `llRequestPermissions(av, PERMISSION_DEBIT)` on a 4 s timer) raises on login,
//! asserts the parse (a holder, a script item, `DEBIT` in the requested set),
//! grants exactly the requested subset with
//! [`Command::AnswerScriptPermissions`], then queries the mirror and asserts the
//! grant is recorded (`Granted`, not `Denied`, carrying `DEBIT`). It then revokes
//! with [`Command::RevokeScriptPermissions`] and queries once more.
//!
//! The revoke is faithful to the documented mirror policy: `RevokePermissions`
//! puts the full requested bitfield on the wire, but the mirror only *follows*
//! the animation bits (`TRIGGER_ANIMATION` / `OVERRIDE_ANIMATIONS`) — every other
//! permission, `DEBIT` among them, the simulator keeps enforcing, so the
//! conservative mirror leaves the grant in place. So after revoking `DEBIT` the
//! snapshot still shows the grant: the assertion records that server-enforced
//! behaviour rather than expecting a local clear. Because a `RevokePermissions`
//! carries no application-level acknowledgement, the circuit staying healthy — a
//! keep-alive ping still round-tripping — is read as "no error", exactly as
//! [`super::script_dialog`] reads its reply.
//!
//! No new client code — the [`ScriptPermissionRequest`](Event::ScriptPermissionRequest)
//! event and the answer / query / revoke commands all existed (the request was
//! verified end-to-end in the Phase-8 #8 setup); only the new case.
//!
//! `1av`, `[both]`. On OpenSim the avatar is forced into the "Default Region",
//! whose `SLClientScriptTester` prim guarantees a request (its absence fails the
//! case); the fixture prim is wiped by any non-merge OAR load, so restoring it is
//! a `load oar --merge slclient8.oar` (memory
//! `sl-client-opensim-scripted-object-testing`). On Second Life no scripted
//! object requests permissions from this avatar, so a window with no request
//! records `partial` rather than failed. The aditi run is deferred with the rest
//! of the Aditi batch (no aditi record this session).

use std::time::Duration;

use sl_client_tokio::{
    Command, Event, ScriptPermissionRequest as ScriptPermissionRequestEvent, ScriptPermissions,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, which
/// holds the `SLClientScriptTester` scripted prim that fires the permission
/// request. On Second Life the avatar keeps `"last"` (a named OpenSim region is
/// meaningless there), and no scripted request is expected.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// How long to watch for a `ScriptPermissionRequest` after the region is ready.
///
/// The test prim fires on a few-second `llSetTimerEvent` loop, but the first tick
/// has to fall after login settles and the interest list streams the object; the
/// window spans several such ticks. Kept generous for Aditi network jitter
/// (where, absent a fixture, it simply times out into `partial`).
const REQUEST_WINDOW: Duration = Duration::from_secs(30);

/// How long to observe the circuit after the revoke for a keep-alive ping.
///
/// `RevokePermissions` has no application-level acknowledgement, so the circuit
/// staying healthy — a root-simulator keep-alive ping (≈ 5 s interval) still
/// round-tripping — is the "no error" signal, exactly as
/// [`super::script_dialog`] confirms its unacknowledged reply.
const OBSERVE_WINDOW: Duration = Duration::from_secs(15);

/// Receives a scripted-object permission request, grants it, reads the grant back
/// from the mirror, then revokes it, confirming the circuit stays healthy.
///
/// Named `…Case` rather than `ScriptPermissions` to avoid clashing with the
/// [`ScriptPermissions`] flags type this case operates on.
#[expect(
    clippy::module_name_repetitions,
    reason = "the bare `ScriptPermissions` name is the flags type; the case struct needs a distinct name"
)]
#[derive(Debug)]
pub struct ScriptPermissionsCase;

impl GridTest for ScriptPermissionsCase {
    fn name(&self) -> &'static str {
        "script-permissions"
    }

    fn description(&self) -> &'static str {
        "Request, grant, and revoke script permissions"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            // Wait for the scripted prim's permission request. `ScriptQuestion`
            // fires each time the test prim calls `llRequestPermissions`; the
            // first one inside the window is the one this case answers.
            let request: Option<ScriptPermissionRequestEvent> = {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                let started = std::time::Instant::now();
                match session
                    .wait_for(REQUEST_WINDOW, |event| match event {
                        Event::ScriptPermissionRequest(request) => Some((**request).clone()),
                        _ => None,
                    })
                    .await
                {
                    Ok(request) => {
                        ctx.metrics().set_timing(
                            &secs_metric("request_rtt"),
                            started.elapsed().as_secs_f64(),
                        );
                        Some(request)
                    }
                    // No request streamed inside the window; handled per grid below
                    // (a hard failure on OpenSim, `partial` on SL).
                    Err(TestFailure::Timeout(_)) => None,
                    Err(other) => return Err(other),
                }
            };

            let request = match request {
                Some(request) => request,
                None if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "no ScriptPermissionRequest arrived from the Default Region test prim"
                            .to_owned(),
                    ));
                }
                None => {
                    // On Second Life no scripted object is requesting permissions
                    // from this avatar, so the absence is a legitimately
                    // incomplete run.
                    ctx.mark_partial(
                        "landing region raised no script permission request in window",
                    );
                    return Ok(());
                }
            };

            // The request must carry `DEBIT` (what the Phase-8 fixture asks for);
            // an unexpected set means the wrong prim or a parse fault.
            if !request.permissions.contains(ScriptPermissions::DEBIT) {
                return Err(TestFailure::Assertion(format!(
                    "script permission request did not include DEBIT (got {:#x})",
                    request.permissions.0
                )));
            }

            let task_id = request.task_id;
            let item_id = request.item_id;
            let experience_id = request.experience_id;

            // Grant exactly the requested subset. The answer is a reliable
            // message; a failure to encode or enqueue it propagates from `send`.
            let session = ctx.primary();
            session
                .send(Command::AnswerScriptPermissions {
                    task_id,
                    item_id,
                    permissions: request.permissions,
                    experience_id,
                })
                .await?;

            // Read the grant back from the mirror. The runtime synthesizes the
            // snapshot locally in reply to the query — no wire round-trip — and
            // the answer command is processed before it (an ordered channel), so
            // the first snapshot reflects the grant just made.
            let granted = query_grant(session, task_id, item_id).await?;
            let granted = granted.ok_or_else(|| {
                TestFailure::Assertion(
                    "the granted script permission was not recorded in the mirror".to_owned(),
                )
            })?;
            if granted.denied {
                return Err(TestFailure::Assertion(
                    "the grant was mirrored as an explicit deny".to_owned(),
                ));
            }
            if !granted.permissions.contains(ScriptPermissions::DEBIT) {
                return Err(TestFailure::Assertion(format!(
                    "the mirrored grant did not include DEBIT (got {:#x})",
                    granted.permissions.0
                )));
            }

            // Revoke the granted permission. The full bitfield goes on the wire,
            // but the mirror only follows the animation bits — `DEBIT` is
            // server-enforced, so the conservative mirror keeps the grant. This
            // asserts that documented behaviour rather than a local clear.
            session
                .send(Command::RevokeScriptPermissions {
                    object_id: task_id,
                    permissions: request.permissions,
                })
                .await?;
            let after_revoke = query_grant(session, task_id, item_id).await?;
            let debit_retained = after_revoke
                .as_ref()
                .is_some_and(|grant| grant.permissions.contains(ScriptPermissions::DEBIT));
            if !debit_retained {
                return Err(TestFailure::Assertion(
                    "revoking DEBIT cleared the server-enforced grant from the mirror".to_owned(),
                ));
            }

            // The revoke carries no application-level response, so a keep-alive
            // ping answered after it is the "no error" signal: the request was
            // accepted and the session is still live.
            let rtt = match session
                .wait_for(OBSERVE_WINDOW, |event| match event {
                    Event::Ping {
                        child: false, rtt, ..
                    } => Some(*rtt),
                    _ => None,
                })
                .await
            {
                Ok(rtt) => rtt,
                Err(TestFailure::Timeout(_)) => {
                    return Err(TestFailure::Assertion(
                        "no keep-alive ping observed after revoking the script permission"
                            .to_owned(),
                    ));
                }
                Err(other) => return Err(other),
            };

            let metrics = ctx.metrics();
            metrics.set("task_id", task_id.to_string());
            metrics.set("item_id", item_id.uuid().to_string());
            metrics.set("object_name", request.object_name.clone());
            metrics.set("object_owner", request.object_owner.clone());
            metrics.set("requested_permissions", i64::from(request.permissions.0));
            metrics.set("granted_permissions", i64::from(granted.permissions.0));
            metrics.set("is_experience", experience_id.is_some());
            metrics.set("debit_retained_after_revoke", debit_retained);
            metrics.set_timing(&secs_metric("ping_rtt"), rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// One recorded grant flattened out of a queried
/// [`Event::ScriptPermissionState`] snapshot for the holder `(task_id, item_id)`,
/// or `None` when the mirror holds no entry for it.
struct MirroredGrant {
    /// The granted permission subset (empty when `denied`).
    permissions: ScriptPermissions,
    /// Whether the entry is an explicit deny rather than a grant.
    denied: bool,
}

/// Queries the session's script-permission mirror and returns the recorded grant
/// for `(task_id, item_id)`, or `None` if that holder is absent.
///
/// [`Command::QueryScriptPermissions`] is answered locally with a synthesized
/// [`Event::ScriptPermissionState`] snapshot — no wire round-trip — so the first
/// such event after the query is this query's answer.
async fn query_grant(
    session: &mut crate::context::Session,
    task_id: sl_client_tokio::ObjectKey,
    item_id: sl_client_tokio::InventoryKey,
) -> Result<Option<MirroredGrant>, TestFailure> {
    session.send(Command::QueryScriptPermissions).await?;
    let state = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ScriptPermissionState(state) => Some(state.clone()),
            _ => None,
        })
        .await?;
    Ok(state
        .grants
        .into_iter()
        .find(|grant| grant.task_id == task_id && grant.item_id == item_id)
        .map(|grant| MirroredGrant {
            permissions: grant.granted,
            denied: grant.denied,
        }))
}
