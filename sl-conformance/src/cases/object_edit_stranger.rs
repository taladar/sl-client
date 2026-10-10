//! What a grid makes of an edit of somebody else's object: who may do what,
//! and what the one who may not is told.
//!
//! [`super::object_edit`] measures an avatar editing its own objects. Here
//! the **owner** (the primary) rezzes a cube and the **stranger** (the
//! secondary), who holds no right over it, tries everything a build floater
//! and a pie menu can send. After each attempt the owner asks the grid for the
//! cube and its record again, so that whether the attempt took is read from
//! the grid and not from what either side was sent:
//!
//! 1. **No rights.** A move, a resize, a turn, a material, a flag, a shape, a
//!    click action, a name, a description, a sale, a mask, a duplicate, an
//!    undo of the owner's move, and every way of taking the cube away.
//! 2. **Anyone may move.** The owner sets the move bit for everyone; the
//!    stranger moves, resizes, renames and takes.
//! 3. **Anyone may copy.** The owner takes modify off the next owner and sets
//!    the copy bit for everyone; the stranger duplicates the cube and takes a
//!    copy, and the copy's record says whose it is and what its masks became.
//! 4. **A member without the power.** The stranger sets a cube of its own to
//!    a group it is a plain member of and deeds it. Where the stranger is in
//!    no such group the owner founds one and the stranger joins — on OpenSim;
//!    on Second Life a group costs money and a slot, so there the leg runs
//!    only with a group at hand.
//!
//! What each live grid answered is stated in `MEASURED` and written up in
//! `book/src/gridspec/building.md` § Somebody else's object; every run is
//! held to it. The fake grid lets anybody edit anything
//! (`server-fake-grid-edit-permission-enforcement`), so the case is live
//! only.
//!
//! `2av`, `[both]`. Everything either avatar left standing is derezzed to its
//! Trash at the end, also when a leg fails.

use std::time::Duration;

use sl_client_tokio::{
    ClickAction, Command, DeRezDestination, Event, GroupKey, LindenAmount, Material, Object,
    ObjectFlagSettings, ObjectKey, ObjectProperties as Properties, ObjectTransform, OwnerKey,
    PermissionField, Permissions, RegionLocalObjectId, SaleType, TransactionId, Uuid, Vector,
    prim_flags,
};

use crate::cases::object_edit::{
    SALE_PRICE, answered, differences, hear, hex, hollow_box_shape, latest, lifted, moved, near,
    quarter_turn, record_masks, record_of, reread, resized, transformed,
};
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up, joined,
    listen, note_items, rez_cube,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{
    REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, is_own, membership_group, settle_scene,
};

/// The OpenSim start location: the "Default Region" (1000,1000), centred.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The name the owner's cube is given.
const CUBE_NAME: &str = "SLClientEditStranger";

/// The name the stranger tries to give it.
const STRANGERS_NAME: &str = "renamed by a stranger";

/// How far along the region's y axis the stranger's own cubes stand from the
/// owner's, in metres.
const STRANGERS_ROW_M: f32 = 3.0;

/// The group power to deed an object (`GP_OBJECT_DEED`).
const POWER_DEED_OBJECT: u64 = 1 << 36;

/// Where the measurements below are written down.
const SOURCE: &str =
    "book/src/gridspec/building.md § Somebody else's object (object-edit-stranger, 2026-10-10)";

/// What each grid was measured to answer, by the metric the case records it
/// under.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "anyone_may_copy_duplicate_alerts",
        both(Answer::Text("none")),
    ),
    (
        "anyone_may_copy_duplicate_appears",
        both(Answer::Flag(true)),
    ),
    (
        "anyone_may_copy_duplicate_creator_is_the_originals",
        both(Answer::Flag(true)),
    ),
    (
        "anyone_may_copy_duplicate_last_owner",
        both(Answer::Text("the original's owner")),
    ),
    (
        "anyone_may_copy_duplicate_masks",
        each(
            Answer::Text(
                "base 0x0008a000 owner 0x0008a000 group 0x00000000 everyone 0x00088000 next 0x0008a000",
            ),
            Answer::Text(
                "base 0x0009a000 owner 0x0008a000 group 0x00000000 everyone 0x00088000 next 0x0008a000",
            ),
        ),
    ),
    (
        "anyone_may_copy_duplicate_name_copied",
        both(Answer::Flag(true)),
    ),
    (
        "anyone_may_copy_duplicate_owner",
        both(Answer::Text("the stranger")),
    ),
    (
        "anyone_may_copy_original_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x00088000 next 0x0008a000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x00088000 next 0x0008a000",
            ),
        ),
    ),
    (
        "anyone_may_copy_stranger_may",
        each(
            Answer::Text("move+copy+transfer+owner-modify"),
            Answer::Text("move+copy+transfer"),
        ),
    ),
    (
        "anyone_may_copy_take_copy_alerts",
        both(Answer::Text("none")),
    ),
    (
        "anyone_may_copy_take_copy_announcement",
        both(Answer::Text("update-create-inventory-item")),
    ),
    (
        "anyone_may_copy_take_copy_item_lands_in",
        both(Answer::Text("objects")),
    ),
    (
        "anyone_may_copy_take_copy_item_masks",
        each(
            Answer::Text("base 0x0008a000 owner 0x0008a000 next 0x0008a000"),
            Answer::Text("base 0x0008a005 owner 0x0008a005 next 0x0008a000"),
        ),
    ),
    (
        "anyone_may_copy_take_copy_kills_it_for_the_stranger",
        both(Answer::Flag(false)),
    ),
    (
        "anyone_may_copy_take_copy_kills_the_object",
        both(Answer::Flag(false)),
    ),
    (
        "anyone_may_copy_take_copy_owner_announcement",
        both(Answer::Text("none")),
    ),
    ("anyone_may_move_move_alerts", both(Answer::Text("none"))),
    (
        "anyone_may_move_move_answered_with",
        both(Answer::Text("terse update")),
    ),
    (
        "anyone_may_move_move_changes",
        both(Answer::Text("position")),
    ),
    ("anyone_may_move_name_alerts", both(Answer::Text("none"))),
    (
        "anyone_may_move_name_answered_with",
        each(Answer::Text("properties"), Answer::Text("nothing")),
    ),
    (
        "anyone_may_move_name_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "anyone_may_move_resize_alerts",
        each(
            Answer::Text("Can't rescale -- permission denied"),
            Answer::Text("none"),
        ),
    ),
    (
        "anyone_may_move_resize_answered_with",
        each(Answer::Text("full update+alert"), Answer::Text("nothing")),
    ),
    (
        "anyone_may_move_resize_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "anyone_may_move_stranger_may",
        each(
            Answer::Text("move+transfer+owner-modify"),
            Answer::Text("move+transfer"),
        ),
    ),
    (
        "anyone_may_move_take_alerts",
        each(
            Answer::Text(
                "Removal of the object 'SLClientEditStranger' from the simulator is disallowed by the permissions system. [NoPermsNoRemoval]",
            ),
            Answer::Text("none"),
        ),
    ),
    (
        "anyone_may_move_take_announcement",
        both(Answer::Text("none")),
    ),
    (
        "anyone_may_move_take_item_lands_in",
        both(Answer::Text("nowhere")),
    ),
    (
        "anyone_may_move_take_kills_it_for_the_stranger",
        both(Answer::Flag(false)),
    ),
    (
        "anyone_may_move_take_kills_the_object",
        both(Answer::Flag(false)),
    ),
    (
        "anyone_may_move_take_owner_announcement",
        both(Answer::Text("none")),
    ),
    ("anyone_may_move_turn_alerts", both(Answer::Text("none"))),
    (
        "anyone_may_move_turn_answered_with",
        both(Answer::Text("terse update")),
    ),
    (
        "anyone_may_move_turn_changes",
        both(Answer::Text("rotation")),
    ),
    ("stranger_click_action_alerts", both(Answer::Text("none"))),
    (
        "stranger_click_action_answered_with",
        both(Answer::Text("nothing")),
    ),
    (
        "stranger_click_action_changes",
        both(Answer::Text("nothing")),
    ),
    ("stranger_delete_alerts", both(Answer::Text("none"))),
    ("stranger_delete_announcement", both(Answer::Text("none"))),
    (
        "stranger_delete_item_lands_in",
        both(Answer::Text("nowhere")),
    ),
    (
        "stranger_delete_kills_it_for_the_stranger",
        both(Answer::Flag(false)),
    ),
    (
        "stranger_delete_kills_the_object",
        both(Answer::Flag(false)),
    ),
    (
        "stranger_delete_owner_announcement",
        both(Answer::Text("none")),
    ),
    ("stranger_description_alerts", both(Answer::Text("none"))),
    (
        "stranger_description_answered_with",
        each(Answer::Text("properties"), Answer::Text("nothing")),
    ),
    (
        "stranger_description_changes",
        both(Answer::Text("nothing")),
    ),
    (
        "stranger_duplicate_alerts",
        each(
            Answer::Text(
                "Copy failed because you lack permission to copy the object 'SLClientEditStranger'. [NoCopyPermsNoObject]",
            ),
            Answer::Text("none"),
        ),
    ),
    ("stranger_duplicate_appears", both(Answer::Flag(false))),
    ("stranger_mask_alerts", both(Answer::Text("none"))),
    (
        "stranger_mask_answered_with",
        both(Answer::Text("full update+properties")),
    ),
    ("stranger_mask_changes", both(Answer::Text("nothing"))),
    ("stranger_material_alerts", both(Answer::Text("none"))),
    (
        "stranger_material_answered_with",
        both(Answer::Text("nothing")),
    ),
    ("stranger_material_changes", both(Answer::Text("nothing"))),
    (
        "stranger_may",
        each(
            Answer::Text("transfer+owner-modify"),
            Answer::Text("transfer"),
        ),
    ),
    (
        "stranger_move_alerts",
        each(
            Answer::Text("Can't reposition -- permission denied"),
            Answer::Text("none"),
        ),
    ),
    (
        "stranger_move_answered_with",
        each(
            Answer::Text("terse update+alert"),
            Answer::Text("full update"),
        ),
    ),
    ("stranger_move_changes", both(Answer::Text("nothing"))),
    ("stranger_name_alerts", both(Answer::Text("none"))),
    (
        "stranger_name_answered_with",
        each(Answer::Text("properties"), Answer::Text("nothing")),
    ),
    ("stranger_name_changes", both(Answer::Text("nothing"))),
    ("stranger_object_delete_alerts", both(Answer::Text("none"))),
    (
        "stranger_object_delete_announcement",
        both(Answer::Text("none")),
    ),
    (
        "stranger_object_delete_item_lands_in",
        both(Answer::Text("nowhere")),
    ),
    (
        "stranger_object_delete_kills_it_for_the_stranger",
        both(Answer::Flag(false)),
    ),
    (
        "stranger_object_delete_kills_the_object",
        both(Answer::Flag(false)),
    ),
    (
        "stranger_object_delete_owner_announcement",
        both(Answer::Text("none")),
    ),
    (
        "stranger_phantom_alerts",
        each(
            Answer::Text("You don't have permissions to modify that object"),
            Answer::Text("none"),
        ),
    ),
    (
        "stranger_phantom_answered_with",
        each(Answer::Text("full update+alert"), Answer::Text("nothing")),
    ),
    ("stranger_phantom_changes", both(Answer::Text("nothing"))),
    (
        "stranger_resize_alerts",
        each(
            Answer::Text("Can't rescale -- permission denied"),
            Answer::Text("none"),
        ),
    ),
    (
        "stranger_resize_answered_with",
        each(Answer::Text("full update+alert"), Answer::Text("nothing")),
    ),
    ("stranger_resize_changes", both(Answer::Text("nothing"))),
    (
        "stranger_return_alerts",
        each(
            Answer::Text(
                "Removal of the object 'SLClientEditStranger' from the simulator is disallowed by the permissions system. [NoPermsNoRemoval]",
            ),
            Answer::Text("none"),
        ),
    ),
    ("stranger_return_announcement", both(Answer::Text("none"))),
    (
        "stranger_return_item_lands_in",
        both(Answer::Text("nowhere")),
    ),
    (
        "stranger_return_kills_it_for_the_stranger",
        both(Answer::Flag(false)),
    ),
    (
        "stranger_return_kills_the_object",
        both(Answer::Flag(false)),
    ),
    (
        "stranger_return_owner_announcement",
        both(Answer::Text("none")),
    ),
    (
        "stranger_sale_alerts",
        each(
            Answer::Text("none"),
            Answer::Text("/You don't have permission to set object on sale"),
        ),
    ),
    (
        "stranger_sale_answered_with",
        each(Answer::Text("properties"), Answer::Text("alert")),
    ),
    ("stranger_sale_changes", both(Answer::Text("nothing"))),
    (
        "stranger_shape_alerts",
        each(
            Answer::Text("You are not allowed to change this shape."),
            Answer::Text("none"),
        ),
    ),
    (
        "stranger_shape_answered_with",
        each(Answer::Text("full update+alert"), Answer::Text("nothing")),
    ),
    ("stranger_shape_changes", both(Answer::Text("nothing"))),
    (
        "stranger_take_alerts",
        each(
            Answer::Text(
                "Removal of the object 'SLClientEditStranger' from the simulator is disallowed by the permissions system. [NoPermsNoRemoval]",
            ),
            Answer::Text("none"),
        ),
    ),
    ("stranger_take_announcement", both(Answer::Text("none"))),
    (
        "stranger_take_copy_alerts",
        each(
            Answer::Text(
                "You cannot copy your selection because you do not have permission to copy the object 'SLClientEditStranger'. [NoCopyNoSelCopy]",
            ),
            Answer::Text("No permission to take copy object SLClientEditStranger"),
        ),
    ),
    (
        "stranger_take_copy_announcement",
        both(Answer::Text("none")),
    ),
    (
        "stranger_take_copy_item_lands_in",
        both(Answer::Text("nowhere")),
    ),
    (
        "stranger_take_copy_kills_it_for_the_stranger",
        both(Answer::Flag(false)),
    ),
    (
        "stranger_take_copy_kills_the_object",
        both(Answer::Flag(false)),
    ),
    (
        "stranger_take_copy_owner_announcement",
        both(Answer::Text("none")),
    ),
    ("stranger_take_item_lands_in", both(Answer::Text("nowhere"))),
    (
        "stranger_take_kills_it_for_the_stranger",
        both(Answer::Flag(false)),
    ),
    ("stranger_take_kills_the_object", both(Answer::Flag(false))),
    (
        "stranger_take_owner_announcement",
        both(Answer::Text("none")),
    ),
    (
        "stranger_turn_alerts",
        each(
            Answer::Text("Can't rotate -- permission denied"),
            Answer::Text("none"),
        ),
    ),
    (
        "stranger_turn_answered_with",
        each(Answer::Text("terse update+alert"), Answer::Text("nothing")),
    ),
    ("stranger_turn_changes", both(Answer::Text("nothing"))),
    ("stranger_undo_alerts", both(Answer::Text("none"))),
    (
        "stranger_undo_answered_with",
        each(Answer::Text("nothing"), Answer::Text("terse update")),
    ),
    (
        "stranger_undo_changes",
        each(Answer::Text("nothing"), Answer::Text("position")),
    ),
    // What an update a stranger is sent shows.
    (
        "anyone_may_move_move_update_shows",
        both(Answer::Text("the object as the grid has it")),
    ),
    (
        "anyone_may_move_name_update_shows",
        both(Answer::Text("no update")),
    ),
    (
        "anyone_may_move_resize_update_shows",
        each(
            Answer::Text("the object as the grid has it"),
            Answer::Text("no update"),
        ),
    ),
    (
        "anyone_may_move_turn_update_shows",
        both(Answer::Text("the object as the grid has it")),
    ),
    (
        "stranger_click_action_update_shows",
        both(Answer::Text("no update")),
    ),
    (
        "stranger_description_update_shows",
        both(Answer::Text("no update")),
    ),
    (
        "stranger_mask_update_shows",
        both(Answer::Text("the object as the grid has it")),
    ),
    (
        "stranger_material_update_shows",
        both(Answer::Text("no update")),
    ),
    (
        "stranger_move_update_shows",
        both(Answer::Text("the object as the grid has it")),
    ),
    (
        "stranger_name_update_shows",
        both(Answer::Text("no update")),
    ),
    (
        "stranger_phantom_update_shows",
        each(
            Answer::Text("the object as the grid has it"),
            Answer::Text("no update"),
        ),
    ),
    (
        "stranger_resize_update_shows",
        each(
            Answer::Text("the object as the grid has it"),
            Answer::Text("no update"),
        ),
    ),
    (
        "stranger_sale_update_shows",
        both(Answer::Text("no update")),
    ),
    (
        "stranger_shape_update_shows",
        each(
            Answer::Text("the object as the grid has it"),
            Answer::Text("no update"),
        ),
    ),
    (
        "stranger_turn_update_shows",
        each(
            Answer::Text("the object as the grid has it"),
            Answer::Text("no update"),
        ),
    ),
    (
        "stranger_undo_update_shows",
        each(
            Answer::Text("no update"),
            Answer::Text("the object as the grid has it"),
        ),
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

/// What a grid was measured to answer in the deed leg, where it ran.
const MEASURED_DEED: &[(&str, Measured<Answer>)] = &[
    (
        "member_deed_alerts",
        each(
            Answer::Text(
                "Deed failed because you do not have permission to deed objects for your group. [DeedFailedNoPermToDeedForGroup]",
            ),
            Answer::Text("none"),
        ),
    ),
    (
        "member_deed_answered_with",
        each(Answer::Text("alert"), Answer::Text("properties")),
    ),
    ("member_deed_owner_reads", both(Answer::Text("the agent"))),
    ("member_group_alerts", both(Answer::Text("none"))),
    ("member_group_reads", both(Answer::Text("the group"))),
];

/// Holds what the case recorded to [`MEASURED`] — and, where the deed leg had
/// a group to run with, to [`MEASURED_DEED`].
fn hold_to_measured(grid: Grid, metrics: &Metrics) -> Result<(), TestFailure> {
    let deed: &[(&str, Measured<Answer>)] = if metrics.get("member_group_reads").is_some() {
        MEASURED_DEED
    } else {
        &[]
    };
    let failures: Vec<String> = MEASURED
        .iter()
        .chain(deed)
        .filter_map(|(key, measured)| {
            let expected = measured.on(grid);
            match metrics.get(key) {
                Some(value) if expected.is(value) => None,
                Some(value) => Some(format!("{key} was {value:?}, measured {expected:?}")),
                None => Some(format!("{key} was not recorded, measured {expected:?}")),
            }
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    Err(TestFailure::Assertion(format!(
        "{grid}: the grid no longer matches {SOURCE}: {}",
        failures.join("; ")
    )))
}

/// Measures what a stranger's edits of somebody else's object are answered
/// with.
#[derive(Debug)]
pub struct ObjectEditStranger;

impl GridTest for ObjectEditStranger {
    fn name(&self) -> &'static str {
        "object-edit-stranger"
    }

    fn description(&self) -> &'static str {
        "What an edit, a take, a duplicate and an undo of somebody else's object are answered \
         with, with no right over it and with the move and the copy bits set for everyone"
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

    fn timeout(&self) -> Duration {
        Duration::from_secs(1500)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let grid = ctx.grid();
            let (owners, strangers) = {
                let Some((owner, stranger)) = ctx.primary_and_secondary() else {
                    return Err(TestFailure::Assertion(
                        "object-edit-stranger needs a secondary avatar".to_owned(),
                    ));
                };
                (folders_of(owner).await?, folders_of(stranger).await?)
            };
            let mut left = Left::default();
            let mut metrics = Metrics::new();
            let outcome = measure(ctx, owners, strangers, &mut left, &mut metrics).await;
            let held = outcome.and_then(|()| hold_to_measured(grid, &metrics));
            ctx.metrics().merge(metrics);
            let mut cleanup = Ok(());
            if let Some((owner, stranger)) = ctx.primary_and_secondary() {
                if let Some(group) = left.joined {
                    cleanup = cleanup.and(stranger.send(Command::LeaveGroup(group)).await);
                }
                cleanup = cleanup
                    .and(clean_up(stranger, &left.strangers, strangers.trash).await)
                    .and(clean_up(owner, &left.owners, owners.trash).await);
            }
            held.and(cleanup)
        })
    }
}

/// What the two avatars left behind.
#[derive(Debug, Default)]
struct Left {
    /// The owner's objects and items.
    owners: Residue,
    /// The stranger's.
    strangers: Residue,
    /// The group the stranger joined for the deed leg.
    joined: Option<GroupKey>,
}

/// The well-known folders of `session`'s login skeleton.
async fn folders_of(session: &mut Session) -> Result<Folders, TestFailure> {
    let folders = session
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::InventorySkeleton(folders) => Some(folders.clone()),
            _ => None,
        })
        .await?;
    Folders::of(&folders)
        .ok_or_else(|| TestFailure::Assertion("inventory skeleton had no root folder".to_owned()))
}

/// Settles `session`'s scene and returns what it needs to rez and listen.
async fn arrive(
    session: &mut Session,
    grid: Grid,
    build_position: Option<Vector>,
    folders: Folders,
) -> Result<Run, TestFailure> {
    let pace = Pace::of(grid);
    let owner = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    session.wait_for_region(REGION_TIMEOUT).await?;
    let (seen, anchor) = settle_scene(
        session,
        grid,
        build_position,
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    let anchor = anchor.ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    Ok(Run {
        folders,
        owner,
        pace,
        position: Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen,
    })
}

/// The owner's cube, as each of the two sees it.
#[derive(Debug)]
struct Target {
    /// The cube as the owner last read it.
    owners: Object,
    /// The cube as the stranger last heard of it.
    strangers: Object,
    /// The cube's record as the owner last read it.
    record: Properties,
}

/// What an attempt is expected to change, which decides how the owner reads
/// whether it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reads {
    /// The object: its place, its size, its shape, its flags.
    Object,
    /// The record: its name, its sale, its masks.
    Record,
}

/// The legs, in order.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of attempts, each a request and what both sides read after it"
)]
async fn measure(
    ctx: &mut TestContext,
    owners: Folders,
    strangers: Folders,
    left: &mut Left,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let Some((owner, stranger)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State("the secondary went away".to_owned()));
    };
    let mut owner_run = arrive(owner, grid, build_position.clone(), owners).await?;
    let mut stranger_run = arrive(stranger, grid, build_position, strangers).await?;
    // The stranger's own cubes stand in a row beside the owner's.
    stranger_run.position = Vector {
        y: owner_run.position.y + STRANGERS_ROW_M,
        ..owner_run.position.clone()
    };

    let mut target = make_target(owner, stranger, &mut owner_run, left).await?;
    metrics.set("stranger_may", may(&target.strangers));

    // --- 1. No rights. ---
    let stood = target.owners.motion.position.clone();
    let id = target.strangers.scoped_id();
    for (key, reads, command) in [
        ("move", Reads::Object, moved(id, &lifted(&stood, 1.0))),
        ("resize", Reads::Object, resized(id, 0.5, 0.75, 0.25)),
        (
            "turn",
            Reads::Object,
            transformed(
                id,
                ObjectTransform {
                    position: None,
                    rotation: Some(quarter_turn()),
                    scale: None,
                    group: true,
                    uniform: false,
                },
            ),
        ),
        (
            "material",
            Reads::Object,
            Command::SetObjectMaterial {
                local_id: id,
                material: Material::Metal,
            },
        ),
        (
            "phantom",
            Reads::Object,
            Command::SetObjectFlags {
                local_id: id,
                flags: ObjectFlagSettings {
                    is_phantom: true,
                    ..ObjectFlagSettings::default()
                },
            },
        ),
        (
            "shape",
            Reads::Object,
            Command::SetObjectShape {
                local_id: id,
                shape: hollow_box_shape(),
            },
        ),
        (
            "click_action",
            Reads::Object,
            Command::SetObjectClickAction {
                local_id: id,
                action: ClickAction::Sit,
            },
        ),
        (
            "name",
            Reads::Record,
            Command::SetObjectName {
                local_id: id,
                name: STRANGERS_NAME.to_owned(),
            },
        ),
        (
            "description",
            Reads::Record,
            Command::SetObjectDescription {
                local_id: id,
                description: STRANGERS_NAME.to_owned(),
            },
        ),
        (
            "sale",
            Reads::Record,
            Command::SetObjectForSale {
                local_id: id,
                sale_type: SaleType::Copy,
                sale_price: Some(LindenAmount(SALE_PRICE)),
            },
        ),
        (
            "mask",
            Reads::Record,
            Command::SetObjectPermissions {
                local_ids: vec![id],
                field: PermissionField::Everyone,
                set: true,
                mask: Permissions::COPY.union(Permissions::MOVE),
            },
        ),
    ] {
        attempt(
            owner,
            stranger,
            &owner_run,
            &stranger_run,
            &mut target,
            metrics,
            &format!("stranger_{key}"),
            reads,
            command,
        )
        .await?;
    }

    // An undo of a move the owner made.
    let owners_id = target.owners.scoped_id();
    owner.send(moved(owners_id, &lifted(&stood, 1.0))).await?;
    hear(owner, &owner_run, &target.owners).await?;
    reread(owner, &owner_run, &mut target.owners).await?;
    hear(stranger, &stranger_run, &target.strangers).await?;
    attempt(
        owner,
        stranger,
        &owner_run,
        &stranger_run,
        &mut target,
        metrics,
        "stranger_undo",
        Reads::Object,
        Command::UndoObjects {
            local_ids: vec![id],
        },
    )
    .await?;

    // A duplicate.
    duplicate(
        owner,
        stranger,
        &mut owner_run,
        &mut stranger_run,
        &target,
        left,
        metrics,
        "stranger_duplicate",
    )
    .await?;

    // Every way of taking it away. Each is given a cube of its own where the
    // last one worked.
    for (key, destination) in [
        (
            "take_copy",
            DeRezDestination::AcquireToAgentInventory(strangers.objects),
        ),
        (
            "take",
            DeRezDestination::TakeIntoAgentInventory(strangers.objects),
        ),
        ("delete", DeRezDestination::Trash(strangers.trash)),
        ("return", DeRezDestination::ReturnToOwner),
    ] {
        let command = Command::DerezObjects {
            local_ids: vec![target.strangers.scoped_id()],
            destination,
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        };
        if removal(
            owner,
            stranger,
            &owner_run,
            &stranger_run,
            &target,
            left,
            metrics,
            &format!("stranger_{key}"),
            command,
        )
        .await?
        {
            target = make_target(owner, stranger, &mut owner_run, left).await?;
        }
    }
    let command = Command::DeleteObjects {
        local_ids: vec![target.strangers.scoped_id()],
    };
    if removal(
        owner,
        stranger,
        &owner_run,
        &stranger_run,
        &target,
        left,
        metrics,
        "stranger_object_delete",
        command,
    )
    .await?
    {
        target = make_target(owner, stranger, &mut owner_run, left).await?;
    }

    // --- 2. Anyone may move. ---
    let owners_id = target.owners.scoped_id();
    owner
        .send(Command::SetObjectPermissions {
            local_ids: vec![owners_id],
            field: PermissionField::Everyone,
            set: true,
            mask: Permissions::MOVE,
        })
        .await?;
    refresh(owner, stranger, &owner_run, &stranger_run, &mut target).await?;
    metrics.set("anyone_may_move_stranger_may", may(&target.strangers));
    let stood = target.owners.motion.position.clone();
    let id = target.strangers.scoped_id();
    for (key, reads, command) in [
        ("move", Reads::Object, moved(id, &lifted(&stood, 1.0))),
        ("resize", Reads::Object, resized(id, 0.5, 0.75, 0.25)),
        (
            "turn",
            Reads::Object,
            transformed(
                id,
                ObjectTransform {
                    position: None,
                    rotation: Some(quarter_turn()),
                    scale: None,
                    group: true,
                    uniform: false,
                },
            ),
        ),
        (
            "name",
            Reads::Record,
            Command::SetObjectName {
                local_id: id,
                name: STRANGERS_NAME.to_owned(),
            },
        ),
    ] {
        attempt(
            owner,
            stranger,
            &owner_run,
            &stranger_run,
            &mut target,
            metrics,
            &format!("anyone_may_move_{key}"),
            reads,
            command,
        )
        .await?;
    }
    let command = Command::DerezObjects {
        local_ids: vec![id],
        destination: DeRezDestination::TakeIntoAgentInventory(strangers.objects),
        transaction_id: TransactionId::from(Uuid::new_v4()),
        group_id: None,
    };
    if removal(
        owner,
        stranger,
        &owner_run,
        &stranger_run,
        &target,
        left,
        metrics,
        "anyone_may_move_take",
        command,
    )
    .await?
    {
        target = make_target(owner, stranger, &mut owner_run, left).await?;
    }

    // --- 3. Anyone may copy, and the next owner may not modify. ---
    let owners_id = target.owners.scoped_id();
    for (field, set, mask) in [
        (PermissionField::NextOwner, true, Permissions::COPY),
        (PermissionField::NextOwner, false, Permissions::MODIFY),
        (PermissionField::Everyone, true, Permissions::COPY),
    ] {
        owner
            .send(Command::SetObjectPermissions {
                local_ids: vec![owners_id],
                field,
                set,
                mask,
            })
            .await?;
    }
    refresh(owner, stranger, &owner_run, &stranger_run, &mut target).await?;
    record_masks(metrics, "anyone_may_copy_original", &target.record);
    metrics.set("anyone_may_copy_stranger_may", may(&target.strangers));
    duplicate(
        owner,
        stranger,
        &mut owner_run,
        &mut stranger_run,
        &target,
        left,
        metrics,
        "anyone_may_copy_duplicate",
    )
    .await?;
    let command = Command::DerezObjects {
        local_ids: vec![target.strangers.scoped_id()],
        destination: DeRezDestination::AcquireToAgentInventory(strangers.objects),
        transaction_id: TransactionId::from(Uuid::new_v4()),
        group_id: None,
    };
    removal(
        owner,
        stranger,
        &owner_run,
        &stranger_run,
        &target,
        left,
        metrics,
        "anyone_may_copy_take_copy",
        command,
    )
    .await?;

    // --- 4. A member without the power. ---
    let group = group_without_the_power(ctx, left, metrics).await?;
    let Some(group) = group else {
        return Ok(());
    };
    let Some((_owner, stranger)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State("the secondary went away".to_owned()));
    };
    let cube = rez_cube(stranger, &mut stranger_run, &mut left.strangers, 2).await?;
    let local_id = cube.scoped_id();
    stranger
        .send(Command::SetObjectGroup {
            local_ids: vec![local_id],
            group_id: group,
        })
        .await?;
    let heard = hear(stranger, &stranger_run, &cube).await?;
    let record = record_of(stranger, &stranger_run, &cube).await?;
    metrics.set(
        "member_group_reads",
        match record.group {
            None => "none",
            Some(set) if set == group => "the group",
            Some(_other) => "another group",
        },
    );
    metrics.set("member_group_alerts", joined(&heard.alerts));
    stranger
        .send(Command::DeedObjectsToGroup {
            local_ids: vec![local_id],
            group_id: group,
        })
        .await?;
    let heard = hear(stranger, &stranger_run, &cube).await?;
    let record = record_of(stranger, &stranger_run, &cube).await?;
    metrics.set(
        "member_deed_owner_reads",
        match record.owner {
            OwnerKey::Agent(agent) if agent == stranger_run.owner => "the agent",
            OwnerKey::Agent(_other) => "another agent",
            OwnerKey::Group(owner) if owner == group => "the group",
            OwnerKey::Group(_other) => "another group",
        },
    );
    metrics.set("member_deed_alerts", joined(&heard.alerts));
    metrics.set("member_deed_answered_with", answered(&heard, &cube));
    Ok(())
}

/// A group the stranger is a member of and may not deed objects for: one it
/// is in already, or — on a grid where a group costs nothing — one the owner
/// founds for the run and the stranger joins.
async fn group_without_the_power(
    ctx: &mut TestContext,
    left: &mut Left,
    metrics: &mut Metrics,
) -> Result<Option<GroupKey>, TestFailure> {
    let grid = ctx.grid();
    let Some(stranger) = ctx.secondary() else {
        return Err(TestFailure::State("the secondary went away".to_owned()));
    };
    stranger.send(Command::RequestAgentDataUpdate).await?;
    let memberships = match stranger
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::GroupMemberships(groups) => Some(groups.clone()),
            _ => None,
        })
        .await
    {
        Ok(groups) => groups,
        Err(TestFailure::Timeout(_)) => Vec::new(),
        Err(other) => return Err(other),
    };
    if let Some(found) = memberships
        .iter()
        .find(|membership| membership.group_powers & POWER_DEED_OBJECT == 0)
    {
        metrics.set("member_group_is", "one the stranger was in");
        return Ok(Some(found.group_id));
    }
    if !is_opensim(grid) {
        metrics.set("member_group_is", "none at hand");
        return Ok(None);
    }
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis());
    let group = membership_group(
        ctx,
        0,
        &format!("slc deed {unique}"),
        "throwaway group for the object-edit-stranger conformance case",
    )
    .await?
    .group_id;
    let Some(stranger) = ctx.secondary() else {
        return Err(TestFailure::State("the secondary went away".to_owned()));
    };
    stranger.send(Command::JoinGroup(group)).await?;
    let joined = stranger
        .wait_for(REPLY_TIMEOUT, |event| match event {
            Event::JoinGroupResult {
                group_id: joined,
                success,
            } if *joined == group => Some(*success),
            _ => None,
        })
        .await?;
    if !joined {
        return Err(TestFailure::Assertion(
            "the stranger could not join the group the owner founded".to_owned(),
        ));
    }
    left.joined = Some(group);
    metrics.set("member_group_is", "one founded for the run");
    Ok(Some(group))
}

/// The owner rezzes and names a cube, and the stranger finds it.
async fn make_target(
    owner: &mut Session,
    stranger: &mut Session,
    owner_run: &mut Run,
    left: &mut Left,
) -> Result<Target, TestFailure> {
    let cube = rez_cube(owner, owner_run, &mut left.owners, 0).await?;
    owner
        .send(Command::SetObjectName {
            local_id: cube.scoped_id(),
            name: CUBE_NAME.to_owned(),
        })
        .await?;
    let record = record_of(owner, owner_run, &cube).await?;
    let strangers = find_object(stranger, cube.full_id).await?;
    Ok(Target {
        owners: cube,
        strangers,
        record,
    })
}

/// The object `object_id` as `session` knows it, waiting for the region to
/// send it if it has not yet.
async fn find_object(session: &mut Session, object_id: ObjectKey) -> Result<Object, TestFailure> {
    session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.full_id == object_id =>
            {
                Some((**object).clone())
            }
            _ => None,
        })
        .await
        .map_err(|failure| match failure {
            TestFailure::Timeout(_) => TestFailure::Assertion(
                "the stranger was never sent the cube the owner rezzed".to_owned(),
            ),
            other => other,
        })
}

/// Brings both views of the target and its record up to the grid's, after an
/// edit by the owner.
async fn refresh(
    owner: &mut Session,
    stranger: &mut Session,
    owner_run: &Run,
    stranger_run: &Run,
    target: &mut Target,
) -> Result<(), TestFailure> {
    target.record = record_of(owner, owner_run, &target.owners).await?;
    reread(owner, owner_run, &mut target.owners).await?;
    // Whatever the edit sent the stranger first, then what the grid says now.
    let heard = hear(stranger, stranger_run, &target.strangers).await?;
    if let Some(latest) = latest(&heard, &target.strangers) {
        target.strangers = latest;
    }
    reread(stranger, stranger_run, &mut target.strangers).await
}

/// The stranger sends `command`, an edit of the target; what it hears and
/// whether the owner reads a change are recorded under `key`.
#[expect(
    clippy::too_many_arguments,
    reason = "two sessions and what each knows of the run, the target, and what to record"
)]
async fn attempt(
    owner: &mut Session,
    stranger: &mut Session,
    owner_run: &Run,
    stranger_run: &Run,
    target: &mut Target,
    metrics: &mut Metrics,
    key: &str,
    reads: Reads,
    command: Command,
) -> Result<(), TestFailure> {
    stranger.send(command).await?;
    let heard = hear(stranger, stranger_run, &target.strangers).await?;
    let told = latest(&heard, &target.strangers);
    metrics.set(
        &format!("{key}_answered_with"),
        answered(&heard, &target.strangers),
    );
    metrics.set(&format!("{key}_alerts"), joined(&heard.alerts));
    let took = match reads {
        Reads::Object => {
            let before = target.owners.clone();
            reread(owner, owner_run, &mut target.owners).await?;
            differences(&before, &target.owners)
        }
        Reads::Record => {
            let before = target.record.clone();
            target.record = record_of(owner, owner_run, &target.owners).await?;
            record_differences(&before, &target.record)
        }
    };
    metrics.set(&format!("{key}_changes"), took);
    // What the stranger was sent, against what the owner reads: an update that
    // puts the object back where the grid has it, or one that shows the edit.
    metrics.set(
        &format!("{key}_update_shows"),
        match &told {
            None => "no update",
            Some(told)
                if near(&told.motion.position, &target.owners.motion.position)
                    && near(&told.scale, &target.owners.scale) =>
            {
                "the object as the grid has it"
            }
            Some(_other) => "something else",
        },
    );
    if let Some(told) = told {
        target.strangers = told;
    }
    Ok(())
}

/// The stranger sends `command`, which is to take the target away; what both
/// hear is recorded under `key`. Returns whether the target is gone.
#[expect(
    clippy::too_many_arguments,
    reason = "two sessions and what each knows of the run, the target, and what to record"
)]
async fn removal(
    owner: &mut Session,
    stranger: &mut Session,
    owner_run: &Run,
    stranger_run: &Run,
    target: &Target,
    left: &mut Left,
    metrics: &mut Metrics,
    key: &str,
    command: Command,
) -> Result<bool, TestFailure> {
    let theirs = target.strangers.scoped_id();
    let owners = target.owners.scoped_id();
    stranger.send(command).await?;
    let heard = listen(
        stranger,
        stranger_run.pace.quiet.saturating_mul(2),
        stranger_run.pace.quiet,
        |heard| heard.killed(theirs) || !heard.alerts.is_empty() || !heard.announced.is_empty(),
    )
    .await?;
    let owner_heard = listen(owner, owner_run.pace.quiet, owner_run.pace.quiet, |heard| {
        heard.killed(owners)
    })
    .await?;
    note_items(&mut left.strangers, &heard);
    note_items(&mut left.owners, &owner_heard);
    let gone = owner_heard.killed(owners);
    if gone {
        left.owners.roots.retain(|root| *root != owners);
    }
    metrics.set(&format!("{key}_kills_the_object"), gone);
    metrics.set(
        &format!("{key}_kills_it_for_the_stranger"),
        heard.killed(theirs),
    );
    metrics.set(&format!("{key}_announcement"), heard.announcement());
    metrics.set(
        &format!("{key}_item_lands_in"),
        heard
            .items()
            .next()
            .map_or("nowhere", |item| stranger_run.folders.word(item.folder_id)),
    );
    if let Some(item) = heard.items().next() {
        metrics.set(
            &format!("{key}_item_masks"),
            format!(
                "base {} owner {} next {}",
                hex(item.permissions.base),
                hex(item.permissions.owner),
                hex(item.permissions.next_owner)
            ),
        );
    }
    metrics.set(&format!("{key}_alerts"), joined(&heard.alerts));
    metrics.set(&format!("{key}_messages"), joined(&heard.messages));
    metrics.set(
        &format!("{key}_owner_announcement"),
        owner_heard.announcement(),
    );
    metrics.set(
        &format!("{key}_owner_messages"),
        joined(&owner_heard.messages),
    );
    Ok(gone)
}

/// The stranger duplicates the target; whether a copy appears and what its
/// record says are recorded under `key`.
#[expect(
    clippy::too_many_arguments,
    reason = "two sessions and what each knows of the run, the target, and what to record"
)]
async fn duplicate(
    owner: &mut Session,
    stranger: &mut Session,
    owner_run: &mut Run,
    stranger_run: &mut Run,
    target: &Target,
    left: &mut Left,
    metrics: &mut Metrics,
    key: &str,
) -> Result<(), TestFailure> {
    stranger
        .send(Command::DuplicateObjects {
            local_ids: vec![target.strangers.scoped_id()],
            offset: Vector {
                x: 0.0,
                y: -STRANGERS_ROW_M,
                z: 0.0,
            },
            group_id: None,
        })
        .await?;
    let agent = stranger_run.owner.uuid();
    let known = stranger_run.seen.clone();
    let original = target.strangers.full_id;
    let is_new_root = move |object: &Object| {
        object.parent_id == RegionLocalObjectId(0)
            && object.full_id != original
            && !known.contains(&object.scoped_id())
    };
    let heard = listen(
        stranger,
        stranger_run.pace.quiet.saturating_mul(2),
        stranger_run.pace.quiet,
        |heard| !heard.alerts.is_empty() || heard.added.iter().any(|object| is_own(object, agent)),
    )
    .await?;
    let copy = heard
        .added
        .iter()
        .find(|object| is_new_root(object) && is_own(object, agent))
        .cloned();
    stranger_run
        .seen
        .extend(heard.added.iter().map(Object::scoped_id));
    // The owner's side sees the copy too, and it is not one of its own.
    let owner_heard = listen(
        owner,
        owner_run.pace.quiet,
        owner_run.pace.quiet,
        |_heard| false,
    )
    .await?;
    owner_run
        .seen
        .extend(owner_heard.added.iter().map(Object::scoped_id));
    metrics.set(&format!("{key}_appears"), copy.is_some());
    metrics.set(&format!("{key}_alerts"), joined(&heard.alerts));
    let Some(copy) = copy else {
        return Ok(());
    };
    left.strangers.roots.push(copy.scoped_id());
    let record = record_of(stranger, stranger_run, &copy).await?;
    metrics.set(
        &format!("{key}_owner"),
        match record.owner {
            OwnerKey::Agent(agent) if agent == stranger_run.owner => "the stranger",
            OwnerKey::Agent(agent) if agent == owner_run.owner => "the original's owner",
            OwnerKey::Agent(_) | OwnerKey::Group(_) => "somebody else",
        },
    );
    metrics.set(
        &format!("{key}_creator_is_the_originals"),
        record.creator_id == target.record.creator_id,
    );
    metrics.set(
        &format!("{key}_last_owner"),
        if record.last_owner_id == owner_run.owner.uuid() {
            "the original's owner"
        } else if record.last_owner_id == stranger_run.owner.uuid() {
            "the stranger"
        } else if record.last_owner_id.is_nil() {
            "nobody"
        } else {
            "somebody else"
        },
    );
    metrics.set(
        &format!("{key}_name_copied"),
        record.name == target.record.name,
    );
    record_masks(metrics, key, &record);
    Ok(())
}

/// What `after` says that `before` did not, of what a stranger's edit of a
/// record may change — or `nothing`.
fn record_differences(before: &Properties, after: &Properties) -> String {
    let mut parts = Vec::new();
    if before.name != after.name {
        parts.push("name");
    }
    if before.description != after.description {
        parts.push("description");
    }
    if before.sale_type != after.sale_type || before.sale_price != after.sale_price {
        parts.push("sale");
    }
    if before.permissions != after.permissions {
        parts.push("masks");
    }
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join("+")
    }
}

/// What an object's update flags say the viewer they were sent to may do
/// with it.
fn may(object: &Object) -> String {
    let flags = object.update_flags;
    let parts: Vec<&str> = [
        (prim_flags::OBJECT_YOU_OWNER, "own"),
        (prim_flags::OBJECT_MODIFY, "modify"),
        (prim_flags::OBJECT_MOVE, "move"),
        (prim_flags::OBJECT_COPY, "copy"),
        (prim_flags::OBJECT_TRANSFER, "transfer"),
        (prim_flags::OBJECT_OWNER_MODIFY, "owner-modify"),
    ]
    .into_iter()
    .filter(|(flag, _word)| flags & flag != 0)
    .map(|(_flag, word)| word)
    .collect();
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join("+")
    }
}
