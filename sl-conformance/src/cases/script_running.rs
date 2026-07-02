//! Query, toggle, and reset a task script's run state.
//!
//! Three commands drive a script's life *after* it is compiled: the viewer's
//! object-Contents "Running" checkbox reads the state with `GetScriptRunning`
//! ([`Command::RequestScriptRunning`] → [`Event::ScriptRunning`]) and writes it
//! with `SetScriptRunning` ([`Command::SetScriptRunning`]); the "Reset" button
//! re-initialises the script with `ScriptReset` ([`Command::ResetScript`]). Only
//! the *get* draws a reply — `SetScriptRunning`/`ScriptReset` are fire-and-forget
//! — so every mutation is verified by a follow-up query, and the reset (which
//! leaves a running script running) is read the way [`super::script_dialog`] and
//! [`super::script_permissions`] read their reply-less commands: the circuit
//! staying healthy, a keep-alive ping still round-tripping.
//!
//! The case owns a throwaway script it can freely toggle rather than borrowing a
//! fixture prim: it rezzes a container cube and creates a **new script directly
//! in it** with [`Command::RezScript`] + [`RestoreItem::new_script`] (the
//! viewer's object-Contents "New Script"). OpenSim's `RezNewScript` fills a
//! default body *and starts it*, so the script is running the moment it appears
//! in the task inventory. The flow:
//!
//! 1. Rez a container cube against a reference primitive.
//! 2. Create a new script in it, then fetch the listing for its task item id.
//! 3. Query the run state → assert **running** (the auto-started script).
//! 4. `SetScriptRunning(false)` → query → assert **stopped**.
//! 5. `SetScriptRunning(true)` → query → assert **running** again.
//! 6. `ResetScript` → query → assert still **running**, and the circuit healthy.
//! 7. Clean up: derez the container to Trash.
//!
//! `1av`. **OpenSim only for now.** The script lives in a prim's *task*
//! inventory, reached through the same `RezScript` task-write Second Life
//! silently drops (the open investigation tracked with [`super::script_upload`]
//! and in `TEST_ROADMAP.md`'s Phase Z) — so there is no way to plant a
//! toggleable script on SL yet, and the SL variant defers with the rest of the
//! task-inventory batch. On OpenSim the get is answered over the CAPS event
//! queue (`ScriptRunningReply`) when the region has one, so this session also
//! exercises that decode path.

use std::collections::HashSet;
use std::time::Duration;

use sl_client_tokio::{
    Command, DeRezDestination, Event, FolderType, InventoryFolder, InventoryFolderKey,
    InventoryKey, InventoryType, Object, ObjectKey, PrimShape, RestoreItem, RezScriptParams,
    ScopedObjectId, TransactionId, Uuid, Vector, pcode,
};

use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, is_opensim, secs_metric};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, whose
/// test prim serves as the rez placement reference. On Second Life the avatar
/// keeps `"last"`.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The overall budget for settling the initial scene (collecting pre-existing
/// object ids and a reference primitive to place the throwaway against).
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle: once no new [`Event::ObjectAdded`] has
/// arrived for this long the initial scene is considered fully streamed.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// How long to wait for each step's confirming event (a rezzed object, a
/// task-inventory reply, the object being removed).
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// The per-attempt timeout for one `GetScriptRunning` round trip inside the
/// run-state poll loop. `GetScriptRunning` returns *nothing* while the script
/// engine has no live instance yet (the script is still compiling/starting), so
/// a single query can legitimately draw no reply — kept short so the loop
/// re-queries promptly.
const QUERY_ATTEMPT: Duration = Duration::from_secs(5);

/// The overall budget for a run-state to settle to the value a mutation
/// requested — generous for the engine's asynchronous compile/start and stop.
const RUN_STATE_DEADLINE: Duration = Duration::from_secs(30);

/// How long to observe the circuit after the reset for a keep-alive ping. The
/// root-simulator ping runs at a ≈ 5 s interval; a healthy ping inside this
/// window is the reset's "no error" signal.
const PING_WINDOW: Duration = Duration::from_secs(12);

/// How far above the reference primitive to rez the container cube, in metres —
/// clear of the reference, well inside the same parcel so the rez check passes.
const CONTAINER_LIFT_M: f32 = 1.0;

/// Rezzes a container cube, creates a running script in it, then queries,
/// toggles, and resets the script's run state.
#[derive(Debug)]
pub struct ScriptRunning;

impl GridTest for ScriptRunning {
    fn name(&self) -> &'static str {
        "script-running"
    }

    fn description(&self) -> &'static str {
        "Query, toggle, and reset a task script's run state"
    }

    fn grids(&self) -> &'static [Grid] {
        // OpenSim only: the script lives in task inventory, reached through the
        // same `RezScript` task-write Second Life silently drops (tracked with
        // `script-upload` in Phase Z).
        &[Grid::Opensim]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one linear flow: settle, rez container, create script, query/toggle/reset, clean up"
    )]
    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();

            // The login inventory skeleton arrives before the region is ready —
            // capture the Trash (cleanup) folder first, before `wait_for_region`
            // would discard it.
            let trash_folder = {
                let session = ctx.primary();
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
                    })?
            };

            let (mut seen, reference) = {
                let session = ctx.primary();
                session.wait_for_region(REGION_TIMEOUT).await?;
                settle_scene(session).await?
            };

            let reference = match reference {
                Some(reference) => reference,
                None if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "no primitive appeared in the Default Region object stream".to_owned(),
                    ));
                }
                None => {
                    ctx.mark_partial("landing region streamed no primitive to place against");
                    return Ok(());
                }
            };

            let container_position = lift(&reference.motion.position, CONTAINER_LIFT_M);
            let session = ctx.primary();

            // 1. Rez the container cube whose task inventory holds the script.
            session
                .send(Command::RezObject {
                    shape: PrimShape::cube(container_position),
                    group_id: None,
                })
                .await?;
            let container = match wait_for_new_object(session, &seen).await? {
                Some(container) => container,
                None if is_opensim(grid) => {
                    return Err(TestFailure::Assertion(
                        "no new object appeared after RezObject for the container".to_owned(),
                    ));
                }
                None => {
                    ctx.mark_partial(
                        "landing region refused the container rez (no object appeared)",
                    );
                    return Ok(());
                }
            };
            let container_id = container.scoped_id();
            let object_id = container.full_id;
            seen.insert(container_id);

            // 2. Create a new script directly in the container — the viewer's
            //    object-Contents "New Script". OpenSim's `RezNewScript` fills a
            //    default body and starts it, so the script is running as soon as
            //    it appears in the task inventory.
            let agent_key = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            session
                .send(Command::RezScript {
                    target: container_id,
                    params: Box::new(RezScriptParams {
                        group_id: None,
                        enabled: true,
                        item: RestoreItem::new_script(
                            agent_key,
                            container.full_id,
                            "New Script",
                            Uuid::new_v4(),
                        ),
                    }),
                })
                .await?;

            // The new script lands in the task inventory asynchronously; poll the
            // fetched listing until the script appears.
            let poll_started = std::time::Instant::now();
            let item_id = loop {
                session
                    .send(Command::FetchTaskInventory {
                        target: container_id,
                    })
                    .await?;
                let found = session
                    .wait_for(STEP_TIMEOUT, |event| match event {
                        Event::TaskInventoryContents { task, items, .. }
                            if *task == container.full_id =>
                        {
                            Some(
                                items
                                    .iter()
                                    .find(|entry| entry.inv_type == InventoryType::Script)
                                    .map(|entry| entry.item_id),
                            )
                        }
                        _ => None,
                    })
                    .await?;
                if let Some(item_id) = found {
                    break item_id;
                }
                if poll_started.elapsed() >= STEP_TIMEOUT {
                    return Err(TestFailure::Assertion(
                        "the new script did not appear in the container's task inventory"
                            .to_owned(),
                    ));
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            };

            // 3. Query the freshly created script's run state → it auto-started,
            //    so it must settle to running.
            let started = std::time::Instant::now();
            poll_run_state(session, object_id, item_id, true).await?;
            let query_rtt = started.elapsed();

            // 4. Stop it → query → must read stopped.
            let started = std::time::Instant::now();
            session
                .send(Command::SetScriptRunning {
                    object_id,
                    item_id,
                    running: false,
                })
                .await?;
            poll_run_state(session, object_id, item_id, false).await?;
            let stop_rtt = started.elapsed();

            // 5. Start it again → query → must read running.
            let started = std::time::Instant::now();
            session
                .send(Command::SetScriptRunning {
                    object_id,
                    item_id,
                    running: true,
                })
                .await?;
            poll_run_state(session, object_id, item_id, true).await?;
            let start_rtt = started.elapsed();

            // 6. Reset the (running) script. `ScriptReset` draws no reply and
            //    leaves a running script running, so it is verified two ways: a
            //    follow-up query still reads running, and a keep-alive ping still
            //    round-trips (the reliable reset was accepted, the circuit lives).
            session
                .send(Command::ResetScript { object_id, item_id })
                .await?;
            poll_run_state(session, object_id, item_id, true).await?;
            let ping_rtt = match session
                .wait_for(PING_WINDOW, |event| match event {
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
                        "no keep-alive ping observed after resetting the script".to_owned(),
                    ));
                }
                Err(other) => return Err(other),
            };

            // 7. Clean up: derez the container to Trash (confirmed by its
            //    KillObject).
            session
                .send(Command::DerezObjects {
                    local_ids: vec![container_id],
                    destination: DeRezDestination::Trash(trash_folder),
                    transaction_id: TransactionId::from(Uuid::new_v4()),
                    group_id: None,
                })
                .await?;
            session
                .wait_for(STEP_TIMEOUT, |event| match event {
                    Event::ObjectRemoved { local_id, .. } if *local_id == container_id => {
                        Some(*local_id)
                    }
                    _ => None,
                })
                .await?;

            let metrics = ctx.metrics();
            metrics.set("container_object", object_id.to_string());
            metrics.set("task_script_item", item_id.uuid().to_string());
            metrics.set_timing(&secs_metric("query_rtt"), query_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("stop_rtt"), stop_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("start_rtt"), start_rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("reset_ping_rtt"), ping_rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// Polls `GetScriptRunning` until the script's run state settles to `expected`,
/// re-querying across the engine's asynchronous compile/start/stop. Each attempt
/// waits [`QUERY_ATTEMPT`] for a reply — a query issued before the engine has a
/// live instance draws none, so a per-attempt timeout re-queries rather than
/// failing — bounded overall by [`RUN_STATE_DEADLINE`].
async fn poll_run_state(
    session: &mut Session,
    object_id: ObjectKey,
    item_id: InventoryKey,
    expected: bool,
) -> Result<(), TestFailure> {
    let started = std::time::Instant::now();
    let mut last: Option<bool> = None;
    loop {
        session
            .send(Command::RequestScriptRunning { object_id, item_id })
            .await?;
        match session
            .wait_for(QUERY_ATTEMPT, |event| match event {
                Event::ScriptRunning {
                    object_id: reply_object,
                    item_id: reply_item,
                    running,
                } if *reply_object == object_id && *reply_item == item_id => Some(*running),
                _ => None,
            })
            .await
        {
            Ok(running) => {
                if running == expected {
                    return Ok(());
                }
                last = Some(running);
            }
            Err(TestFailure::Timeout(_)) => {}
            Err(other) => return Err(other),
        }
        if started.elapsed() >= RUN_STATE_DEADLINE {
            let observed = last.map_or_else(
                || "no ScriptRunningReply".to_owned(),
                |value| format!("running = {value}"),
            );
            return check(
                false,
                &format!(
                    "script run state never settled to running = {expected} (last: {observed})"
                ),
            );
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// A point `lift` metres above `base`.
fn lift(base: &Vector, lift: f32) -> Vector {
    Vector {
        x: base.x,
        y: base.y,
        z: base.z + lift,
    }
}

/// Drains the region's initial object-update burst, returning every region-local
/// id sighted and the first primitive seen (the placement reference). The drain
/// ends once no new [`Event::ObjectAdded`] has arrived for [`SETTLE_IDLE`], or the
/// overall [`SETTLE_WINDOW`] elapses.
async fn settle_scene(
    session: &mut Session,
) -> Result<(HashSet<ScopedObjectId>, Option<Object>), TestFailure> {
    let mut seen = HashSet::new();
    let mut reference: Option<Object> = None;
    let started = std::time::Instant::now();
    loop {
        let remaining = SETTLE_WINDOW.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        let cap = remaining.min(SETTLE_IDLE);
        match session
            .wait_for(cap, |event| match event {
                Event::ObjectAdded(object) => Some((**object).clone()),
                _ => None,
            })
            .await
        {
            Ok(object) => {
                if reference.is_none() && object.pcode == pcode::PRIMITIVE {
                    reference = Some(object.clone());
                }
                seen.insert(object.scoped_id());
            }
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    Ok((seen, reference))
}

/// Waits for the next [`Event::ObjectAdded`] whose region-local id is not in
/// `seen` — the freshly rezzed object. Returns `None` if none appears within
/// [`STEP_TIMEOUT`].
async fn wait_for_new_object(
    session: &mut Session,
    seen: &HashSet<ScopedObjectId>,
) -> Result<Option<Object>, TestFailure> {
    match session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) if !seen.contains(&object.scoped_id()) => {
                Some((**object).clone())
            }
            _ => None,
        })
        .await
    {
        Ok(object) => Ok(Some(object)),
        Err(TestFailure::Timeout(_)) => Ok(None),
        Err(other) => Err(other),
    }
}

/// The agent inventory root folder id from a login skeleton (`None` if empty or
/// rootless).
fn agent_root(folders: &[InventoryFolder]) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.parent_id.is_none())
        .map(|folder| folder.folder_id)
}

/// The id of the first folder of the given well-known type in a login skeleton
/// (`None` if absent).
fn folder_of_type(
    folders: &[InventoryFolder],
    folder_type: FolderType,
) -> Option<InventoryFolderKey> {
    folders
        .iter()
        .find(|folder| folder.folder_type == folder_type.to_code())
        .map(|folder| folder.folder_id)
}
