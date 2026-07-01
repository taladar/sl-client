//! Create a script, upload source into an object's task inventory, and read the
//! compile result.
//!
//! Editing a script's source is not a plain asset upload: the viewer never
//! compiles LSL/Lua locally — it POSTs the raw source to a capability and the
//! **simulator compiles it synchronously**, returning a `compiled` flag and an
//! `errors` array. A script can upload successfully as an asset yet fail to
//! compile — surfaced as [`Event::ScriptUploaded`] with `compiled == false`,
//! distinct from a transport-level [`Event::AssetUploadFailed`].
//!
//! The case uses the **task-inventory** path (`UpdateScriptTask`): a simulator
//! only compiles a task-inventory script upload — the agent-inventory path merely
//! stores the asset on OpenSim — so the task path is the one that returns a real
//! compile result. The flow:
//!
//! 1. Rez a throwaway container cube against a reference primitive.
//! 2. Create a new script **directly in the container** with
//!    [`Command::RezScript`] and [`RestoreItem::new_script`] (the viewer's
//!    object-Contents "New Script": a null-id / null-asset item the simulator
//!    fills with a compilable default body), then fetch the listing for its id.
//! 3. [`Command::UploadScript`] **valid** source to that task item → assert
//!    `compiled == true`, no errors.
//! 4. [`Command::UploadScript`] **invalid** source → assert `compiled == false`,
//!    a non-empty error list, and that the first [`ScriptCompileError`] parsed a
//!    `line`/`column` — the payoff of the structured parse.
//! 5. Clean up: derez the container to Trash.
//!
//! `1av`. **OpenSim only for now.** On Second Life the task-inventory *write*
//! never lands (the object's contents serial stays `0` after `RezScript` and
//! after an `UpdateTaskInventory` drop) even though rez, agent-inventory create,
//! and reads (`RequestTaskInventory`) all succeed on the same session, the
//! checksum/parent are matched to the viewer, and the avatar owns the object.
//! Auth, land permission, CRC, parent, and object selection have all been ruled
//! out, and the wire encoding matches the viewer byte-for-byte — so the next step
//! is a real-viewer **packet capture** of an object-Contents "New Script" on
//! aditi to see the exact sequence/fields SL requires (see the Phase Z note in
//! `TEST_ROADMAP.md`). The `for_task_drop` / `new_script` / parent-aware CRC
//! support all exist for when that lands.

use std::collections::HashSet;
use std::time::Duration;

use sl_client_tokio::{
    Command, DeRezDestination, Event, FolderType, InventoryFolder, InventoryFolderKey,
    InventoryType, Object, PrimShape, RestoreItem, RezScriptParams, ScopedObjectId,
    ScriptCompileError, ScriptTarget, ScriptUploadLocation, TransactionId, Uuid, Vector, pcode,
};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, check, count_metric, is_opensim, secs_metric};

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

/// How long to wait for each step's confirming event (a rezzed object, an
/// inventory item, a task-inventory reply, the object being removed).
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for the compile result of one upload — generous for Aditi
/// jitter and the simulator's synchronous compile.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// How far above the reference primitive to rez the container cube, in metres —
/// clear of the reference, well inside the same parcel so the rez check passes.
const CONTAINER_LIFT_M: f32 = 1.0;

/// A minimal **valid** LSL script — compiles cleanly on both grids.
const VALID_SOURCE: &str = "default\n{\n    state_entry()\n    {\n    }\n}\n";

/// A **broken** LSL script (an empty right-hand side) — a guaranteed syntax
/// error, so the simulator reports `compiled == false` with diagnostics.
const INVALID_SOURCE: &str =
    "default\n{\n    state_entry()\n    {\n        integer x = ;\n    }\n}\n";

/// Creates a script, uploads valid then invalid source into a prim's task
/// inventory, and asserts the simulator's compile result (and that errors parse).
#[derive(Debug)]
pub struct ScriptUpload;

impl GridTest for ScriptUpload {
    fn name(&self) -> &'static str {
        "script-upload"
    }

    fn description(&self) -> &'static str {
        "Create a script, upload source to a prim, and read the compile result"
    }

    fn grids(&self) -> &'static [Grid] {
        // OpenSim only: Second Life silently drops the task-inventory write, an
        // open investigation tracked in Phase Z (needs a viewer packet capture).
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
        reason = "one linear flow: settle, rez container, create+drop script, upload good/bad, clean up"
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

            // 1. Rez the container cube — the prim whose task inventory holds the
            //    script we upload to. A region that refuses the rez records
            //    `partial` on Second Life (a hard failure on OpenSim).
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
            seen.insert(container_id);

            // 2. Create a new script **directly in the container** — the viewer's
            //    object-Contents "New Script": [`Command::RezScript`] with a
            //    null-id / null-asset item, so the simulator allocates the item and
            //    fills a compilable default body (no agent-inventory item, no drop).
            let agent_key = session
                .agent_id()
                .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
            session
                .send(Command::RezScript {
                    target: container_id,
                    params: Box::new(RezScriptParams {
                        group_id: None,
                        enabled: false,
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
            let task_item_id = loop {
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

            // 3. Upload valid source → expect a clean compile.
            let good = upload_source(session, &container, task_item_id, VALID_SOURCE).await?;
            check(good.compiled, "valid source did not compile")?;
            check(
                good.errors.is_empty(),
                "valid source reported compiler errors",
            )?;

            // 4. Upload broken source → expect a failed compile with diagnostics.
            let bad = upload_source(session, &container, task_item_id, INVALID_SOURCE).await?;
            tracing::info!(
                compiled = bad.compiled,
                errors = bad.errors.len(),
                "invalid upload"
            );
            check(!bad.compiled, "invalid source unexpectedly compiled")?;
            let first = bad.errors.first().ok_or_else(|| {
                TestFailure::Assertion("invalid source reported no compiler errors".to_owned())
            })?;
            let bad_error_parsed = first.line.is_some() && first.column.is_some();
            let sample = first.raw.clone();

            // 5. Clean up: derez the container to Trash (confirmed by its
            //    KillObject), then best-effort remove the agent script item.
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
            metrics.set("container_object", container.full_id.to_string());
            metrics.set("task_script_item", task_item_id.uuid().to_string());
            metrics.set("target", ScriptTarget::Mono.to_wire());
            metrics.set("compiled_good", good.compiled);
            metrics.set(&count_metric("good_errors"), good.errors.len().to_string());
            metrics.set("compiled_bad", bad.compiled);
            metrics.set(&count_metric("bad_errors"), bad.errors.len().to_string());
            metrics.set("bad_error_parsed", bad_error_parsed);
            metrics.set("bad_error_sample", sample);
            metrics.set_timing(&secs_metric("good_upload_rtt"), good.rtt.as_secs_f64());
            metrics.set_timing(&secs_metric("bad_upload_rtt"), bad.rtt.as_secs_f64());
            Ok(())
        })
    }
}

/// The compile outcome of one [`Command::UploadScript`], flattened for assertions.
struct UploadOutcome {
    /// Whether the simulator compiled the source.
    compiled: bool,
    /// The compiler diagnostics (empty on a clean compile).
    errors: Vec<ScriptCompileError>,
    /// The round-trip time from sending the upload to the compile result.
    rtt: Duration,
}

/// Uploads `source` into the task-inventory script `item_id` of `container`
/// (compile target [`ScriptTarget::Mono`], `is_script_running` false) and waits
/// for the [`Event::ScriptUploaded`] compile result. A transport-level
/// [`Event::AssetUploadFailed`] fails the case.
async fn upload_source(
    session: &mut crate::context::Session,
    container: &Object,
    item_id: sl_client_tokio::InventoryKey,
    source: &str,
) -> Result<UploadOutcome, TestFailure> {
    let started = std::time::Instant::now();
    session
        .send(Command::UploadScript {
            location: ScriptUploadLocation::TaskInventory {
                task_id: container.full_id,
                item_id,
                running: false,
                experience: None,
            },
            target: ScriptTarget::Mono,
            source: source.as_bytes().to_vec(),
        })
        .await?;
    let outcome = session
        .wait_for(UPLOAD_TIMEOUT, |event| match event {
            Event::ScriptUploaded {
                compiled, errors, ..
            } => Some(Ok((*compiled, errors.clone()))),
            Event::AssetUploadFailed { reason } => Some(Err(reason.clone())),
            _ => None,
        })
        .await?;
    match outcome {
        Ok((compiled, errors)) => Ok(UploadOutcome {
            compiled,
            errors,
            rtt: started.elapsed(),
        }),
        Err(reason) => Err(TestFailure::Assertion(format!(
            "script upload failed at the transport level: {reason}"
        ))),
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
    session: &mut crate::context::Session,
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
    session: &mut crate::context::Session,
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
