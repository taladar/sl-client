//! What a second avatar may read of, and write into, the contents of a prim
//! that is not its own.
//!
//! The primary avatar rezzes a cube and puts a notecard and a script in it.
//! The secondary — no friend with edit rights, no group in common — then
//! asks what [`super::task_inventory`] asks as the owner:
//!
//! 1. **The listing**, over UDP and over the `RequestTaskInventory`
//!    capability: whether it is answered, and whether the items' assets are
//!    named.
//! 2. **A script's experience**, over `GetMetadata`.
//! 3. **Four writes**: a drop of a notecard of its own, a new script, a
//!    remove of the owner's notecard and a move of it into its own
//!    inventory. After each the case notes what the stranger was sent, what
//!    the owner was, and — read back by the owner — whether the contents
//!    changed.
//!
//! A write that does not land is the one a viewer has to survive: its
//! Contents tab has shown the change already. What each grid answers is
//! written up in `book/src/gridspec/building.md` § Contents.
//!
//! `2av`, live grids only: the fake grid lets a stranger write anything
//! (`server-fake-grid-edit-permission-enforcement`).

use sl_client_tokio::{
    AssetType, Command, InventoryType, Object, RestoreItem, RezScriptParams, TaskInventoryKey,
    Uuid, task_inventory_cap_url,
};

use crate::cases::object_edit::count;
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, clean_up, rez_cube,
};
use crate::cases::task_inventory::{
    DRAIN, Listing, NO_CAPABILITY, OPENSIM_START, Written, cap_get, cap_post, create_item,
    drop_item, find_object, hold_to_measured, http_client, listen, listing, login_folders,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, settle_scene};

/// The notecard the owner puts in the cube.
const OWNERS_NOTE: &str = "SLClientStrangerOwnersNote";

/// The script the owner makes in it.
const OWNERS_SCRIPT: &str = "SLClientStrangerOwnersScript";

/// The notecard the stranger tries to drop.
const STRANGERS_NOTE: &str = "SLClientStrangersNote";

/// The script the stranger tries to make.
const STRANGERS_SCRIPT: &str = "SLClientStrangersScript";

/// Where the measurements below are written down.
const SOURCE: &str =
    "book/src/gridspec/building.md § Contents (task-inventory-stranger, 2026-10-12)";

/// What each grid was measured to answer, by the metric the census records it
/// under.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "stranger_asks_the_experience",
        each(Answer::Text("200: undefined"), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "stranger_drop_brings",
        each(
            Answer::Text("alert: Unable to edit this! [UnableEditItem]"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "stranger_drop_tells_the_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "stranger_lists",
        each(
            Answer::Text("serial 3, 2 item(s)"),
            Answer::Text("no answer"),
        ),
    ),
    (
        "stranger_lists_over_the_cap",
        each(
            Answer::Text("200: serial 3, 2 item(s)"),
            Answer::Text(NO_CAPABILITY),
        ),
    ),
    (
        "stranger_move_brings",
        each(
            Answer::Text("alert: Not permitted to copy that inventory. [NoPermToCopyInventory]"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "stranger_move_gives_the_stranger",
        both(Answer::Text("nothing")),
    ),
    (
        "stranger_move_tells_the_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "stranger_remove_brings",
        each(
            Answer::Text("alert: Not permitted to edit this. [NoPermissionToEdit]"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "stranger_remove_tells_the_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "stranger_script_brings",
        each(
            Answer::Text("alert: Not permitted to edit this! [NoPermToEdit]"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "stranger_script_tells_the_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "stranger_is_shown_assets",
        each(Answer::Flag(false), Answer::Text("no answer")),
    ),
    (
        "stranger_is_shown_assets_over_the_cap",
        each(Answer::Flag(false), Answer::Text(NO_CAPABILITY)),
    ),
    (
        "stranger_is_shown_the_owners_ids",
        each(Answer::Flag(true), Answer::Text("no answer")),
    ),
];

/// An answer both grids give.
const fn both(answer: Answer) -> Measured<Answer> {
    Measured {
        second_life: answer,
        opensim: answer,
        source: SOURCE,
    }
}

/// An answer the grids differ in.
const fn each(second_life: Answer, opensim: Answer) -> Measured<Answer> {
    Measured {
        second_life,
        opensim,
        source: SOURCE,
    }
}

/// A census of what a stranger may read and write of a prim's contents.
#[derive(Debug)]
pub struct TaskInventoryStranger;

impl GridTest for TaskInventoryStranger {
    fn name(&self) -> &'static str {
        "task-inventory-stranger"
    }

    fn description(&self) -> &'static str {
        "What a second avatar may read of and write into the contents of a prim it does not own"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
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
            let folders = login_folders(ctx.primary()).await?;
            let Some(stranger) = ctx.secondary() else {
                return Err(TestFailure::State(
                    "task-inventory-stranger needs a secondary avatar".to_owned(),
                ));
            };
            let strangers_folders = login_folders(stranger).await?;
            let mut residue = Residue::default();
            let mut strangers_residue = Residue::default();
            let mut metrics = Metrics::new();
            let outcome = census(
                ctx,
                folders,
                strangers_folders,
                &mut residue,
                &mut strangers_residue,
                &mut metrics,
            )
            .await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            // Take the cube and the items away whatever became of the census.
            let cleanup = clean_up(ctx.primary(), &residue, folders.trash).await;
            let strangers_cleanup = match ctx.secondary() {
                Some(stranger) => {
                    clean_up(stranger, &strangers_residue, strangers_folders.trash).await
                }
                None => Ok(()),
            };
            outcome
                .and(cleanup)
                .and(strangers_cleanup)
                .and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// The two avatars and the cube between them.
struct Census<'a> {
    /// The cube's owner.
    owner: &'a mut Session,
    /// The other avatar.
    stranger: &'a mut Session,
    /// How long to listen.
    pace: Pace,
    /// The cube, as its owner knows it.
    cube: Object,
    /// The cube's contents as last read by the owner.
    held: Listing,
}

impl Census<'_> {
    /// Has the stranger send `command`, a write into the cube, and returns
    /// what it and the owner were sent, with the contents as the owner then
    /// reads them.
    async fn write(&mut self, command: Command) -> Result<Written, TestFailure> {
        let before = self.held.clone();
        self.stranger.send(command).await?;
        let to_writer = listen(self.stranger, self.pace.quiet).await?;
        let to_other = listen(self.owner, DRAIN).await?;
        self.held = listing(self.owner, self.cube.scoped_id(), self.cube.full_id).await?;
        Ok(Written {
            to_writer,
            to_other,
            before,
            after: self.held.clone(),
            flag_bits: None,
        })
    }

    /// Records one write: what it brought the stranger, and what it told the
    /// owner.
    fn note(&self, metrics: &mut Metrics, what: &str, written: &Written) {
        let before = written.before.serial;
        // The two stand in one region, where the cube has one local id.
        metrics.set(
            &format!("{what}_brings"),
            written.outcome(&written.to_writer.told(&self.cube, before)),
        );
        metrics.set(
            &format!("{what}_tells_the_owner"),
            written.to_other.told(&self.cube, before),
        );
    }
}

/// The legs, in order.
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    strangers_folders: Folders,
    residue: &mut Residue,
    strangers_residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let pace = Pace::of(grid);
    let Some((owner, stranger)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "task-inventory-stranger needs a secondary avatar".to_owned(),
        ));
    };
    let owner_id = owner
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let stranger_id = stranger
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

    owner.wait_for_region(REGION_TIMEOUT).await?;
    let (seen, anchor) = settle_scene(
        owner,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    stranger.wait_for_region(REGION_TIMEOUT).await?;
    settle_scene(
        stranger,
        grid,
        build_position,
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    let anchor = anchor.ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    let mut run = Run {
        folders,
        owner: owner_id,
        pace,
        position: sl_client_tokio::Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen,
    };
    let http = http_client()?;

    // The owner's cube, with a notecard and a script in it.
    let note = create_item(
        owner,
        residue,
        &folders,
        AssetType::Notecard,
        InventoryType::Notecard,
        OWNERS_NOTE,
    )
    .await?;
    let cube = rez_cube(owner, &mut run, residue, 0).await?;
    let task = cube.full_id;
    owner
        .send(Command::UpdateTaskInventory {
            target: cube.scoped_id(),
            key: TaskInventoryKey::Item,
            item: Box::new(drop_item(&note, task)?),
        })
        .await?;
    owner
        .send(Command::RezScript {
            target: cube.scoped_id(),
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: true,
                item: RestoreItem::new_script(owner_id, task, OWNERS_SCRIPT, Uuid::new_v4()),
            }),
        })
        .await?;
    listen(owner, pace.quiet).await?;
    let held = listing(owner, cube.scoped_id(), task).await?;
    metrics.set("owner_lists_items", count(held.items.len()));
    let owners_note = held
        .items
        .iter()
        .find(|item| item.name == OWNERS_NOTE)
        .map(|item| item.item_id)
        .ok_or_else(|| {
            TestFailure::Assertion("the owner's notecard never landed in its cube".to_owned())
        })?;
    let owners_script = held
        .items
        .iter()
        .find(|item| item.name == OWNERS_SCRIPT)
        .map(|item| item.item_id);

    // --- 1. The listing, as a stranger reads it. ---
    let theirs = find_object(stranger, task).await?;
    listen(stranger, DRAIN).await?;
    match listing(stranger, theirs, task).await {
        Ok(read) => {
            metrics.set(
                "stranger_lists",
                format!("serial {}, {} item(s)", read.serial, read.items.len()),
            );
            metrics.set(
                "stranger_is_shown_assets",
                read.items.iter().any(|item| item.asset_id.is_some()),
            );
            metrics.set(
                "stranger_is_shown_the_owners_ids",
                read.items.iter().any(|item| item.item_id == owners_note),
            );
        }
        Err(TestFailure::Timeout(_)) => {
            for key in [
                "stranger_lists",
                "stranger_is_shown_assets",
                "stranger_is_shown_the_owners_ids",
            ] {
                metrics.set(key, "no answer");
            }
        }
        Err(other) => return Err(other),
    }
    match stranger.cap("RequestTaskInventory") {
        Some(base) => {
            let answer = cap_get(&http, &task_inventory_cap_url(&base, task, None)).await?;
            metrics.set("stranger_lists_over_the_cap", answer.summary(task));
            metrics.set(
                "stranger_is_shown_assets_over_the_cap",
                answer.listing(task).is_some_and(|(_serial, items)| {
                    items.iter().any(|item| item.asset_id.is_some())
                }),
            );
        }
        None => {
            metrics.set("stranger_lists_over_the_cap", NO_CAPABILITY);
            metrics.set("stranger_is_shown_assets_over_the_cap", NO_CAPABILITY);
        }
    }

    // --- 2. A script's experience. ---
    match (stranger.cap("GetMetadata"), owners_script) {
        (Some(url), Some(script)) => {
            let answer = cap_post(
                &http,
                &url,
                sl_client_tokio::build_get_metadata_request(task, script),
            )
            .await?;
            metrics.set("stranger_asks_the_experience", answer.shape());
        }
        (None, _) => metrics.set("stranger_asks_the_experience", NO_CAPABILITY),
        (Some(_), None) => metrics.set("stranger_asks_the_experience", "no script landed"),
    }

    // --- 3. Four writes. ---
    let strangers_note = create_item(
        stranger,
        strangers_residue,
        &strangers_folders,
        AssetType::Notecard,
        InventoryType::Notecard,
        STRANGERS_NOTE,
    )
    .await?;
    let mut census = Census {
        owner,
        stranger,
        pace,
        cube,
        held,
    };
    let dropped = census
        .write(Command::UpdateTaskInventory {
            target: theirs,
            key: TaskInventoryKey::Item,
            item: Box::new(drop_item(&strangers_note, task)?),
        })
        .await?;
    census.note(metrics, "stranger_drop", &dropped);

    let made = census
        .write(Command::RezScript {
            target: theirs,
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: true,
                item: RestoreItem::new_script(stranger_id, task, STRANGERS_SCRIPT, Uuid::new_v4()),
            }),
        })
        .await?;
    census.note(metrics, "stranger_script", &made);

    let moved = census
        .write(Command::MoveTaskInventory {
            target: theirs,
            folder_id: strangers_folders.root,
            item_id: owners_note,
        })
        .await?;
    census.note(metrics, "stranger_move", &moved);
    strangers_residue.items.extend(
        moved
            .to_writer
            .announced
            .iter()
            .map(|(_shape, item)| item.item_id),
    );
    metrics.set(
        "stranger_move_gives_the_stranger",
        if moved
            .to_writer
            .announced
            .iter()
            .any(|(_shape, item)| i32::from(item.item_type) == AssetType::Notecard.to_code())
        {
            "the item"
        } else {
            "nothing"
        },
    );

    let removed = census
        .write(Command::RemoveTaskInventory {
            target: theirs,
            item_id: owners_note,
        })
        .await?;
    census.note(metrics, "stranger_remove", &removed);
    strangers_residue.items.extend(
        removed
            .to_writer
            .announced
            .iter()
            .map(|(_shape, item)| item.item_id),
    );

    metrics.set("final_items", count(census.held.items.len()));
    Ok(())
}
