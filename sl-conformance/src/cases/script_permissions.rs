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
//!
//! On both fake grids the case brings its own fixture, since no prim of the
//! scripts scene may ask the case's avatar for `DEBIT`: a script asks only its
//! owner for it. It rezzes a cube, saves into it a script that asks its owner
//! for `DEBIT` as it starts and says what it was granted, and answers the
//! question that script raises. The grant then has an observer the live grids
//! lack — the script's own `run_time_permissions` — so the case also waits for
//! the script to say it holds `DEBIT`.

use std::time::Duration;

use sl_client_tokio::{
    AgentKey, ChatSource, Command, DeRezDestination, Event, FolderType, InventoryFolderKey,
    InventoryKey, InventoryType, Object, ObjectKey, PrimShape, RestoreItem, RezScriptParams,
    ScriptPermissionRequest as ScriptPermissionRequestEvent, ScriptPermissions, ScriptTarget,
    ScriptUploadLocation, TransactionId, Uuid,
};

use super::script_upload::{agent_root, folder_of_type, lift};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, check_eq, is_opensim, secs_metric, settle_scene,
    wait_for_own_new_object,
};

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

/// How long the fake grids' run waits for each step of planting its own
/// fixture: the cube, the new script, the saved source.
const FAKE_STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// The fixture script the fake grids' run saves into its cube: it asks its
/// owner for `DEBIT` as it starts, as the Default Region's test prim asks on
/// its timer, and tells its owner what it was granted — wherever the cube
/// was rezzed, out of the range of a say.
const FAKE_SOURCE: &str = "default\n{\n    state_entry()\n    {\n        \
    llRequestPermissions(llGetOwner(), PERMISSION_DEBIT);\n    }\n\n    \
    run_time_permissions(integer granted)\n    {\n        \
    llOwnerSay(\"granted \" + (string)granted);\n    }\n}\n";

/// The line [`FAKE_SOURCE`] tells its owner once it holds `DEBIT` (`0x2`) alone.
const FAKE_GRANTED_LINE: &str = "granted 2";

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
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn rezzes_objects(&self) -> bool {
        true
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
            if grid.is_fake() {
                return run_fake(ctx).await;
            }

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
    session: &mut Session,
    task_id: ObjectKey,
    item_id: InventoryKey,
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

/// The fake grids' run: rez a cube, save [`FAKE_SOURCE`] into it, answer the
/// question it raises, hear it say it holds `DEBIT`, read the grant back from
/// the mirror, revoke it, and derez the cube.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow: plant the fixture, answer it, read back, revoke, clean up"
)]
async fn run_fake(ctx: &mut TestContext) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let session = ctx.primary();
    let trash = trash_folder(session)
        .await
        .map_err(|failure| waiting_for("the inventory skeleton", failure))?;
    session
        .wait_for_region(REGION_TIMEOUT)
        .await
        .map_err(|failure| waiting_for("the region", failure))?;
    let (seen, reference) = settle_scene(
        session,
        grid,
        build_position,
        Duration::from_secs(15),
        Duration::from_secs(2),
    )
    .await
    .map_err(|failure| waiting_for("the settled scene", failure))?;
    let reference = reference.ok_or_else(|| {
        TestFailure::Assertion("the fake region streamed no primitive to place against".to_owned())
    })?;
    session
        .send(Command::RezObject {
            shape: PrimShape::cube(lift(&reference, 1.0)),
            group_id: None,
        })
        .await?;
    let cube = wait_for_own_new_object(session, &seen, FAKE_STEP_TIMEOUT)
        .await
        .map_err(|failure| waiting_for("the rezzed cube", failure))?
        .map_err(|reason| TestFailure::Assertion(format!("the cube was not rezzed: {reason}")))?;
    let item = new_script(session, &cube)
        .await
        .map_err(|failure| waiting_for("the new script", failure))?;
    session
        .send(Command::UploadScript {
            location: ScriptUploadLocation::TaskInventory {
                task_id: cube.full_id,
                item_id: item,
                running: true,
                experience: None,
            },
            target: ScriptTarget::Mono,
            source: FAKE_SOURCE.as_bytes().to_vec(),
        })
        .await?;
    // The script may ask before the save is answered, so one wait takes both.
    let started = std::time::Instant::now();
    let mut compiled: Option<bool> = None;
    let mut asked: Option<ScriptPermissionRequestEvent> = None;
    let waited = session
        .wait_for(FAKE_STEP_TIMEOUT, |event| {
            match event {
                Event::ScriptUploaded { compiled: done, .. } => compiled = Some(*done),
                Event::ScriptPermissionRequest(request) => asked = Some((**request).clone()),
                _ => {}
            }
            (compiled == Some(false) || (compiled.is_some() && asked.is_some())).then_some(())
        })
        .await;
    match waited {
        Ok(()) => {}
        Err(TestFailure::Timeout(_)) => {
            return Err(TestFailure::Assertion(format!(
                "planting the fixture script stalled: the save was {}, and {}",
                compiled.map_or("never answered", |done| if done {
                    "compiled"
                } else {
                    "refused"
                }),
                if asked.is_some() {
                    "the question came"
                } else {
                    "no question came"
                }
            )));
        }
        Err(other) => return Err(other),
    }
    let request_rtt = started.elapsed();
    check_eq("fixture script compiled", &compiled, &Some(true))?;
    let request = asked
        .ok_or_else(|| TestFailure::Assertion("the fixture script asked no question".to_owned()))?;
    check_eq("question's prim", &request.task_id, &cube.full_id)?;
    check_eq("question's script", &request.item_id, &item)?;
    check_eq(
        "question's permissions",
        &request.permissions,
        &ScriptPermissions(ScriptPermissions::DEBIT),
    )?;
    session
        .send(Command::AnswerScriptPermissions {
            task_id: request.task_id,
            item_id: request.item_id,
            permissions: request.permissions,
            experience_id: request.experience_id,
        })
        .await?;
    let speaker = ChatSource::Object(cube.full_id);
    let answer = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::ChatReceived(chat) if chat.source == speaker => Some(chat.message.clone()),
            _ => None,
        })
        .await
        .map_err(|failure| waiting_for("the script's answer", failure))?;
    check_eq("the script's answer", &answer.as_str(), &FAKE_GRANTED_LINE)?;
    let granted = query_grant(session, request.task_id, request.item_id)
        .await
        .map_err(|failure| waiting_for("the mirror after the grant", failure))?
        .ok_or_else(|| {
            TestFailure::Assertion(
                "the granted script permission was not recorded in the mirror".to_owned(),
            )
        })?;
    check_eq("mirrored deny", &granted.denied, &false)?;
    check_eq(
        "mirrored grant",
        &granted.permissions,
        &ScriptPermissions(ScriptPermissions::DEBIT),
    )?;
    session
        .send(Command::RevokeScriptPermissions {
            object_id: request.task_id,
            permissions: request.permissions,
        })
        .await?;
    let after_revoke = query_grant(session, request.task_id, request.item_id)
        .await
        .map_err(|failure| waiting_for("the mirror after the revoke", failure))?;
    let debit_retained = after_revoke
        .as_ref()
        .is_some_and(|grant| grant.permissions.contains(ScriptPermissions::DEBIT));
    check_eq(
        "DEBIT kept in the mirror after the revoke",
        &debit_retained,
        &true,
    )?;
    let local_id = cube.scoped_id();
    session
        .send(Command::DerezObjects {
            local_ids: vec![local_id],
            destination: DeRezDestination::Trash(trash),
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        })
        .await?;
    session
        .wait_for(FAKE_STEP_TIMEOUT, |event| match event {
            Event::ObjectRemoved { local_id: gone, .. } if *gone == local_id => Some(()),
            _ => None,
        })
        .await
        .map_err(|failure| waiting_for("the derezzed cube", failure))?;
    let metrics = ctx.metrics();
    metrics.set_timing(&secs_metric("request_rtt"), request_rtt.as_secs_f64());
    metrics.set("task_id", request.task_id.to_string());
    metrics.set("item_id", request.item_id.uuid().to_string());
    metrics.set("object_name", request.object_name.clone());
    metrics.set("object_owner", request.object_owner.clone());
    metrics.set("requested_permissions", i64::from(request.permissions.0));
    metrics.set("granted_permissions", i64::from(granted.permissions.0));
    metrics.set("is_experience", request.experience_id.is_some());
    metrics.set("debit_retained_after_revoke", debit_retained);
    metrics.set("script_answer", answer);
    Ok(())
}

/// The Trash folder of the login skeleton (the root, failing that), read
/// before anything else waits and discards it.
async fn trash_folder(session: &mut Session) -> Result<InventoryFolderKey, TestFailure> {
    let folders = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::InventorySkeleton(folders) => Some(folders.clone()),
            _ => None,
        })
        .await?;
    folder_of_type(&folders, FolderType::Trash)
        .or_else(|| agent_root(&folders))
        .ok_or_else(|| {
            TestFailure::Assertion(
                "inventory skeleton had neither a Trash nor a root folder".to_owned(),
            )
        })
}

/// Makes a new script in `cube` (the viewer's object-Contents "New Script")
/// and the id it is listed under once it appears.
async fn new_script(session: &mut Session, cube: &Object) -> Result<InventoryKey, TestFailure> {
    let owner: AgentKey = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let target = cube.scoped_id();
    session
        .send(Command::RezScript {
            target,
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: false,
                item: RestoreItem::new_script(owner, cube.full_id, "New Script", Uuid::new_v4()),
            }),
        })
        .await?;
    let started = std::time::Instant::now();
    loop {
        session
            .send(Command::FetchTaskInventory {
                target,
                held_serial: None,
            })
            .await?;
        let found = crate::support::wait_for_task_listing(session, cube.full_id, FAKE_STEP_TIMEOUT)
            .await?
            .iter()
            .find(|entry| entry.inv_type == InventoryType::Script)
            .map(|entry| entry.item_id);
        if let Some(item) = found {
            return Ok(item);
        }
        if started.elapsed() >= FAKE_STEP_TIMEOUT {
            return Err(TestFailure::Assertion(
                "the new script did not appear in the cube's task inventory".to_owned(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// `failure`, named after what the fake grids' run was waiting for when it
/// timed out.
fn waiting_for(what: &str, failure: TestFailure) -> TestFailure {
    match failure {
        TestFailure::Timeout(reason) => {
            TestFailure::Timeout(format!("waiting for {what}: {reason}"))
        }
        other => other,
    }
}
