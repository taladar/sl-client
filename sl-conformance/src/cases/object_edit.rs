//! A census of what an avatar's edits of its own objects are answered with,
//! and of what each grid makes of an edit it will not take as sent.
//!
//! The case rezzes a handful of cubes, holds each selected as a build floater
//! would, and asks one question a leg, listening after each for everything
//! the grid says about it — the object's updates and the form each comes in,
//! its record, the kills, the alerts, the inventory announcements and the
//! instant messages:
//!
//! 1. **What an edit is answered with.** A material, a flag, a shape, a click
//!    action, the search flag, a move, a turn, a resize, and all three at
//!    once: the form the object comes back in, and whether the edit took.
//! 2. **Sizes a grid does not take.** A cube a millimetre across and one a
//!    kilometre long: what each reads afterwards.
//! 3. **Names a grid does not take.** A name and a description of 254
//!    characters, a name with a pipe and a line break in it, one that is not
//!    ASCII and an empty one.
//! 4. **Masks a grid does not take.** Every bit taken off the next-owner mask
//!    and every bit put on it, copy, move, modify and transfer for everyone,
//!    a share with a group, and a bit off the base and the owner masks.
//! 5. **A group and a deed that are nobody's.** The object set to a group the
//!    agent is not in, and deeded to it.
//! 6. **Undo.** One edit of each kind and then steps back until nothing
//!    answers, steps forward again, and two dozen moves to count how many a grid
//!    keeps.
//! 7. **Duplicate.** A copy by an offset of a cube with a name, a sale, a
//!    next-owner mask and something in its contents: what the copy's record
//!    says and whether the contents came along; then the same on a ray.
//! 8. **Places a grid does not take.** A move four kilometres up, one under
//!    the ground and one off the region's edge where no region is.
//!
//! What each live grid answered is stated in `MEASURED` and written up in
//! `book/src/gridspec/building.md` § Editing; every run, live or fake, is held
//! to it. What a grid makes of somebody *else's* edits is
//! `object-edit-stranger`'s.
//!
//! `1av`, `[both]`, offline on both fake flavours. Everything left standing
//! is derezzed to the Trash at the end, also when a leg fails, and the items
//! the legs made are removed from the inventory.

use std::time::Duration;

use sl_client_tokio::{
    ClickAction, Command, DeRezDestination, Event, GroupKey, LindenAmount, Material, Object,
    ObjectFlagSettings, ObjectProperties as Properties, ObjectTransform, OwnerKey, PermissionField,
    Permissions, PrimShapeParams, RegionLocalObjectId, Rotation, SaleType, ScopedObjectId,
    TaskInventoryKey, TransactionId, Uuid, Vector,
};

use crate::cases::object_rez_derez::{
    Answer, Folders, Heard, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up,
    joined, listen, note_items, rez_cube, wait_for_notice,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, REPLY_TIMEOUT, is_opensim, is_own, settle_scene};

/// The OpenSim start location: the "Default Region" (1000,1000), centred, where
/// this workspace's test object stands as the rez placement reference.
const OPENSIM_START: &str = "uri:Default Region&128&128&30";

/// The phantom flag of an object update's flags.
pub(crate) const FLAGS_PHANTOM: u32 = 1 << 10;

/// The include-in-search flag of an object update's flags.
const FLAGS_INCLUDE_IN_SEARCH: u32 = 1 << 15;

/// The flags of an object update that an edit sets: physical, phantom,
/// temporary and shown in search.
const FLAGS_OF_THE_OBJECT: u32 = 1 | FLAGS_PHANTOM | FLAGS_INCLUDE_IN_SEARCH | (1 << 29);

/// The flag an object update carries for a prim that was rezzed selected.
pub(crate) const FLAGS_CREATE_SELECTED: u32 = 1 << 1;

/// The quantized `profile_hollow` of a box hollowed by a quarter.
pub(crate) const HOLLOW_25_PCT: u16 = 12_500;

/// How far apart two positions may be and still be the same place, in
/// metres: a terse update quantises a region position to about 4 mm.
const POSITION_EPSILON_M: f32 = 0.05;

/// How far apart two rotations' components may be and still be the same.
const ROTATION_EPSILON: f32 = 0.01;

/// The longest text a name or a description field carries on the wire, less
/// its terminator.
const LONGEST_TEXT: usize = 254;

/// How many moves the depth leg makes, one after another.
const DEPTH_MOVES: u8 = 24;

/// How far each of those moves goes, in metres.
const DEPTH_STEP_M: f32 = 0.1;

/// How many steps back the depth leg asks for at most.
const DEPTH_UNDOS: u8 = 28;

/// How many steps back and forward the scope leg asks for.
const SCOPE_STEPS: u8 = 9;

/// The name of the cube that is duplicated.
const DUPLICATE_NAME: &str = "SLClientEditDup";

/// Its description.
const DUPLICATE_DESCRIPTION: &str = "object-edit duplicate";

/// The price it is put up for sale at, in L$.
pub(crate) const SALE_PRICE: u64 = 25;

/// How far along the region's y axis a copy is asked to stand from its
/// original, in metres.
const DUPLICATE_OFFSET_M: f32 = 1.5;

/// How far a move off the region's edge goes past it, in metres.
const PAST_THE_EDGE_M: f32 = 10.0;

/// The height of a move far above anything a grid lets an object stand at.
const FAR_ABOVE_M: f32 = 5000.0;

/// The height of a move under the ground.
const FAR_BELOW_M: f32 = -50.0;

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/building.md § Editing (object-edit, 2026-10-10)";

/// What each grid was measured to answer, by the metric the census records it
/// under. Every run — live or fake — is held to all of it at the end.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "accented_name_description_reads",
        each(
            Answer::Text("\"on??off ???\""),
            Answer::Text("\"6F 6E C3 A9 6F 66 66 20 E2 82 AC 00             on..off ....\""),
        ),
    ),
    (
        "accented_name_reads",
        each(
            Answer::Text("\"on??off ???\""),
            Answer::Text("\"6F 6E C3 A9 6F 66 66 20 E2 82 AC 00             on..off ....\""),
        ),
    ),
    (
        "all_three_answered_with",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    (
        "all_three_linkset_answered_with",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    (
        "all_three_linkset_took",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "all_three_took",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "base_less_transfer_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffdfff group 0x00000000 everyone 0x00000000 next 0x00002000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
        ),
    ),
    ("category_reads", both(Answer::Count(0))),
    (
        "click_action_answered_with",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    ("click_action_took", both(Answer::Flag(true))),
    ("duplicate_alerts", both(Answer::Text("none"))),
    ("duplicate_appears", both(Answer::Flag(true))),
    ("duplicate_brings", both(Answer::Text("full update"))),
    ("duplicate_contents_items", both(Answer::Count(1))),
    (
        "duplicate_creation_date",
        both(Answer::Text("the original's")),
    ),
    ("duplicate_creator_is_the_agent", both(Answer::Flag(true))),
    ("duplicate_description_copied", both(Answer::Flag(true))),
    (
        "duplicate_differs_from_the_original_in",
        both(Answer::Text("nothing")),
    ),
    ("duplicate_flagged_selected", both(Answer::Flag(false))),
    ("duplicate_from_task_id_set", both(Answer::Flag(false))),
    (
        "duplicate_inventory_serial",
        each(Answer::Count(1), Answer::Count(3)),
    ),
    ("duplicate_item_id_set", both(Answer::Flag(false))),
    ("duplicate_last_owner", both(Answer::Text("the agent"))),
    (
        "duplicate_last_owner_is_the_originals",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    (
        "duplicate_mask_next_owner",
        both(Answer::Text("0x0008a000")),
    ),
    ("duplicate_masks_copied", both(Answer::Flag(true))),
    ("duplicate_name_copied", both(Answer::Flag(true))),
    ("duplicate_owner_is_the_agent", both(Answer::Flag(true))),
    ("duplicate_sale_copied", both(Answer::Flag(true))),
    ("duplicate_sale_type", both(Answer::Count(2))),
    (
        "duplicate_stands",
        both(Answer::Text("where it was asked to")),
    ),
    ("empty_name_description_reads", both(Answer::Text("\"\""))),
    ("empty_name_reads", both(Answer::Text("\"\""))),
    (
        "everyone_copy_move_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x00088000 next 0x00002000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x00088000 next 0x00082000",
            ),
        ),
    ),
    (
        "everyone_everything_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x7fffbfff next 0x00002000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x0008e000 next 0x00082000",
            ),
        ),
    ),
    (
        "everyone_modify_transfer_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x0008a000 next 0x00002000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x0008e000 next 0x00082000",
            ),
        ),
    ),
    (
        "everyone_nothing_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x00000000 next 0x00002000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
        ),
    ),
    ("far_above_alerts", both(Answer::Text("none"))),
    (
        "far_above_announcement",
        each(
            Answer::Text("update-create-inventory-item"),
            Answer::Text("none"),
        ),
    ),
    (
        "far_above_answered_with",
        both(Answer::Text("terse update")),
    ),
    ("far_above_appears_elsewhere", both(Answer::Flag(false))),
    (
        "far_above_item_lands_in",
        each(Answer::Text("lost and found"), Answer::Text("nowhere")),
    ),
    (
        "far_above_kills_the_object",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "far_above_notice_from",
        each(Answer::Text("Second Life"), Answer::Text("nobody")),
    ),
    (
        "far_above_notice_reason",
        each(Answer::Text("it went off-world."), Answer::Text("none")),
    ),
    (
        "far_above_notice_sender_has_an_id",
        both(Answer::Flag(false)),
    ),
    (
        "far_above_reads",
        each(Answer::Text("gone"), Answer::Text("where it was sent")),
    ),
    (
        "far_above_told_by",
        each(Answer::Text("an instant message"), Answer::Text("nothing")),
    ),
    ("far_below_alerts", both(Answer::Text("none"))),
    ("far_below_announcement", both(Answer::Text("none"))),
    (
        "far_below_answered_with",
        both(Answer::Text("terse update")),
    ),
    ("far_below_appears_elsewhere", both(Answer::Flag(false))),
    ("far_below_item_lands_in", both(Answer::Text("nowhere"))),
    ("far_below_kills_the_object", both(Answer::Flag(false))),
    ("far_below_notice_from", both(Answer::Text("nobody"))),
    ("far_below_notice_reason", both(Answer::Text("none"))),
    (
        "far_below_notice_sender_has_an_id",
        both(Answer::Flag(false)),
    ),
    (
        "far_below_reads",
        each(
            Answer::Text("where it stood"),
            Answer::Text("where it was sent"),
        ),
    ),
    ("far_below_told_by", both(Answer::Text("nothing"))),
    (
        "foreign_deed_alerts",
        each(
            Answer::Text(
                "Deed failed because you do not have permission to deed objects for your group. [DeedFailedNoPermToDeedForGroup]",
            ),
            Answer::Text("none"),
        ),
    ),
    ("foreign_deed_owner_reads", both(Answer::Text("the agent"))),
    ("foreign_group_alerts", both(Answer::Text("none"))),
    ("foreign_group_reads", both(Answer::Text("none"))),
    (
        "group_nothing_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x00000000 next 0x00002000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
        ),
    ),
    (
        "group_share_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x0008e000 everyone 0x00000000 next 0x00002000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x0008e000 everyone 0x00000000 next 0x00082000",
            ),
        ),
    ),
    ("huge_alerts", both(Answer::Text("none"))),
    ("huge_answered_with", both(Answer::Text("full update"))),
    (
        "huge_reads",
        each(Answer::Text("64.000"), Answer::Text("256.000")),
    ),
    ("long_description_reads_bytes", both(Answer::Count(127))),
    ("long_name_reads_bytes", both(Answer::Count(63))),
    ("material_answered_with", both(Answer::Text("full update"))),
    ("material_took", both(Answer::Flag(true))),
    ("move_answered_with", both(Answer::Text("terse update"))),
    ("move_took", both(Answer::Flag(true))),
    (
        "new_prim_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
        ),
    ),
    (
        "next_owner_everything_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x00000000 next 0x7fffffff",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x00000000 next 0x0009e000",
            ),
        ),
    ),
    (
        "next_owner_nothing_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffffff group 0x00000000 everyone 0x00000000 next 0x00002000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
        ),
    ),
    ("off_the_edge_alerts", both(Answer::Text("none"))),
    (
        "off_the_edge_announcement",
        each(
            Answer::Text("update-create-inventory-item"),
            Answer::Text("none"),
        ),
    ),
    (
        "off_the_edge_answered_with",
        each(Answer::Text("terse update"), Answer::Text("full update")),
    ),
    ("off_the_edge_appears_elsewhere", both(Answer::Flag(false))),
    (
        "off_the_edge_item_lands_in",
        each(Answer::Text("lost and found"), Answer::Text("nowhere")),
    ),
    (
        "off_the_edge_kills_the_object",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "off_the_edge_notice_from",
        each(Answer::Text("Second Life"), Answer::Text("nobody")),
    ),
    (
        "off_the_edge_notice_reason",
        each(Answer::Text("it went off-world."), Answer::Text("none")),
    ),
    (
        "off_the_edge_notice_sender_has_an_id",
        both(Answer::Flag(false)),
    ),
    (
        "off_the_edge_reads",
        each(Answer::Text("gone"), Answer::Text("where it stood")),
    ),
    (
        "off_the_edge_told_by",
        each(Answer::Text("an instant message"), Answer::Text("nothing")),
    ),
    ("on_ray_alerts", both(Answer::Text("none"))),
    (
        "on_ray_appears",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("on_ray_at_a_prim_alerts", both(Answer::Text("none"))),
    ("on_ray_at_a_prim_appears", both(Answer::Flag(true))),
    ("on_ray_at_a_prim_brings", both(Answer::Text("full update"))),
    ("on_ray_at_a_prim_contents_items", both(Answer::Count(1))),
    (
        "on_ray_at_a_prim_creation_date",
        both(Answer::Text("the original's")),
    ),
    (
        "on_ray_at_a_prim_creator_is_the_agent",
        both(Answer::Flag(true)),
    ),
    (
        "on_ray_at_a_prim_description_copied",
        both(Answer::Flag(true)),
    ),
    (
        "on_ray_at_a_prim_differs_from_the_original_in",
        both(Answer::Text("nothing")),
    ),
    (
        "on_ray_at_a_prim_flagged_selected",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "on_ray_at_a_prim_from_task_id_set",
        both(Answer::Flag(false)),
    ),
    (
        "on_ray_at_a_prim_inventory_serial",
        each(Answer::Count(1), Answer::Count(3)),
    ),
    ("on_ray_at_a_prim_item_id_set", both(Answer::Flag(false))),
    (
        "on_ray_at_a_prim_last_owner",
        both(Answer::Text("the agent")),
    ),
    (
        "on_ray_at_a_prim_last_owner_is_the_originals",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    (
        "on_ray_at_a_prim_mask_next_owner",
        both(Answer::Text("0x0008a000")),
    ),
    ("on_ray_at_a_prim_masks_copied", both(Answer::Flag(true))),
    ("on_ray_at_a_prim_name_copied", both(Answer::Flag(true))),
    (
        "on_ray_at_a_prim_owner_is_the_agent",
        both(Answer::Flag(true)),
    ),
    ("on_ray_at_a_prim_sale_copied", both(Answer::Flag(true))),
    ("on_ray_at_a_prim_sale_type", both(Answer::Count(2))),
    (
        "on_ray_at_a_prim_stands",
        both(Answer::Text("where it was asked to")),
    ),
    ("on_ray_brings", each(Answer::Text("full update"), ABSENT)),
    ("on_ray_contents_items", each(Answer::Count(1), ABSENT)),
    (
        "on_ray_creation_date",
        each(Answer::Text("the original's"), ABSENT),
    ),
    (
        "on_ray_creator_is_the_agent",
        each(Answer::Flag(true), ABSENT),
    ),
    (
        "on_ray_description_copied",
        each(Answer::Flag(true), ABSENT),
    ),
    (
        "on_ray_differs_from_the_original_in",
        each(Answer::Text("nothing"), ABSENT),
    ),
    ("on_ray_flagged_selected", each(Answer::Flag(true), ABSENT)),
    ("on_ray_from_task_id_set", each(Answer::Flag(false), ABSENT)),
    ("on_ray_inventory_serial", each(Answer::Count(1), ABSENT)),
    ("on_ray_item_id_set", each(Answer::Flag(false), ABSENT)),
    ("on_ray_last_owner", each(Answer::Text("the agent"), ABSENT)),
    (
        "on_ray_last_owner_is_the_originals",
        each(Answer::Flag(false), ABSENT),
    ),
    (
        "on_ray_mask_next_owner",
        each(Answer::Text("0x0008a000"), ABSENT),
    ),
    ("on_ray_masks_copied", each(Answer::Flag(true), ABSENT)),
    ("on_ray_name_copied", each(Answer::Flag(true), ABSENT)),
    (
        "on_ray_owner_is_the_agent",
        each(Answer::Flag(true), ABSENT),
    ),
    ("on_ray_sale_copied", each(Answer::Flag(true), ABSENT)),
    ("on_ray_sale_type", each(Answer::Count(2), ABSENT)),
    (
        "on_ray_stands",
        each(
            Answer::Text("+0.00, +0.00, +0.25 from where it was asked to"),
            ABSENT,
        ),
    ),
    ("original_contents_items", both(Answer::Count(1))),
    ("original_inventory_serial", both(Answer::Count(1))),
    (
        "owner_less_transfer_masks",
        each(
            Answer::Text(
                "base 0x7fffffff owner 0x7fffdfff group 0x00000000 everyone 0x00000000 next 0x00002000",
            ),
            Answer::Text(
                "base 0x0009e000 owner 0x0009e000 group 0x00000000 everyone 0x00000000 next 0x00082000",
            ),
        ),
    ),
    ("phantom_answered_with", both(Answer::Text("full update"))),
    ("phantom_took", both(Answer::Flag(true))),
    (
        "piped_name_description_reads",
        each(Answer::Text("\"a?b?c\""), Answer::Text("\"a|b\"")),
    ),
    (
        "piped_name_reads",
        each(Answer::Text("\"a?b?c\""), Answer::Text("\"a|b\"")),
    ),
    ("redo_answered_with", both(Answer::Text("full update"))),
    (
        "redo_steps",
        both(Answer::Text("scale, rotation, position")),
    ),
    (
        "redone_differs_from_edited_in",
        both(Answer::Text("nothing")),
    ),
    ("resize_answered_with", both(Answer::Text("full update"))),
    ("resize_took", both(Answer::Flag(true))),
    (
        "search_answered_with",
        each(Answer::Text("full update"), Answer::Text("nothing")),
    ),
    ("search_took", both(Answer::Flag(true))),
    ("shape_answered_with", both(Answer::Text("full update"))),
    ("shape_took", both(Answer::Flag(true))),
    ("tiny_alerts", both(Answer::Text("none"))),
    ("tiny_answered_with", both(Answer::Text("full update"))),
    (
        "tiny_reads",
        each(Answer::Text("0.010"), Answer::Text("0.001")),
    ),
    ("turn_answered_with", both(Answer::Text("terse update"))),
    ("turn_took", both(Answer::Flag(true))),
    ("undo_alerts", both(Answer::Text("none"))),
    ("undo_answered_with", both(Answer::Text("terse update"))),
    ("undo_depth", each(Answer::Count(20), Answer::Count(5))),
    (
        "undo_depth_ends",
        both(Answer::Text("short of where the moves began")),
    ),
    (
        "undo_past_the_end_answered_with",
        both(Answer::Text("nothing")),
    ),
    ("undo_reverts_the_name", both(Answer::Flag(false))),
    ("undo_reverts_the_sale", both(Answer::Flag(false))),
    (
        "undo_steps",
        both(Answer::Text("position, rotation, scale")),
    ),
    (
        "undone_differs_from_edited_in",
        both(Answer::Text("position+rotation+scale")),
    ),
];

/// What a metric reads on a grid that never records it: a leg the grid's own
/// answer cut short.
const ABSENT: Answer = Answer::Text("not recorded");

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

/// Holds what the census recorded to [`MEASURED`], naming every metric that
/// reads otherwise — or was never recorded.
fn hold_to_measured(grid: Grid, metrics: &Metrics) -> Result<(), TestFailure> {
    let failures: Vec<String> = MEASURED
        .iter()
        .filter_map(|(key, measured)| {
            let expected = measured.on(grid);
            match metrics.get(key) {
                None if *expected == ABSENT => None,
                Some(value) if expected.is(value) => None,
                Some(value) => Some(format!("{key} was {value:?}, measured {expected:?}")),
                None => Some(format!("{key} was not recorded, measured {expected:?}")),
            }
        })
        .collect();
    if failures.is_empty() {
        return Ok(());
    }
    let verdict = if grid.is_fake() {
        "the fake grid no longer imitates"
    } else {
        "the grid no longer matches"
    };
    Err(TestFailure::Assertion(format!(
        "{grid}: {verdict} {SOURCE}: {}",
        failures.join("; ")
    )))
}

/// A census of what an avatar's edits of its own objects are answered with.
#[derive(Debug)]
pub struct ObjectEdit;

impl GridTest for ObjectEdit {
    fn name(&self) -> &'static str {
        "object-edit"
    }

    fn description(&self) -> &'static str {
        "What an edit of the agent's own object is answered with, what a grid makes of sizes, \
         names, masks and places it does not take, how far an undo reaches and what a duplicate \
         copies"
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

    fn timeout(&self) -> Duration {
        Duration::from_secs(1500)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            // The folders come with the login, before the region does.
            let folders = {
                let folders = ctx
                    .primary()
                    .wait_for(REPLY_TIMEOUT, |event| match event {
                        Event::InventorySkeleton(folders) => Some(folders.clone()),
                        _ => None,
                    })
                    .await?;
                Folders::of(&folders).ok_or_else(|| {
                    TestFailure::Assertion("inventory skeleton had no root folder".to_owned())
                })?
            };
            let grid = ctx.grid();
            let mut residue = Residue::default();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, folders, &mut residue, &mut metrics).await;
            let held = outcome.and_then(|()| hold_to_measured(grid, &metrics));
            ctx.metrics().merge(metrics);
            // Take away what is left whatever became of the census.
            let cleanup = clean_up(ctx.primary(), &residue, folders.trash).await;
            held.and(cleanup)
        })
    }
}

/// The legs, in order. Everything rezzed and every item made is noted in
/// `residue` as it appears, so the caller can take it away when a leg fails.
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let pace = Pace::of(grid);
    let session = ctx.primary();
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
    let mut run = Run {
        folders,
        owner,
        pace,
        position: Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen,
    };

    answers(session, &mut run, residue, metrics).await?;
    masks_and_groups(session, &mut run, residue, metrics).await?;
    undo(session, &mut run, residue, metrics).await?;
    duplicates(session, &mut run, residue, metrics).await?;
    places(session, &mut run, residue, metrics).await
}

/// Legs 1 to 3: what an edit is answered with, and the sizes and names a grid
/// does not take.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of edits, each a request and a window to hear its answer in"
)]
async fn answers(
    session: &mut Session,
    run: &mut Run,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let mut cube = rez_cube(session, run, residue, 0).await?;
    let local_id = cube.scoped_id();
    let stood = cube.motion.position.clone();
    hold_selected(session, run, &cube).await?;

    // --- 1. What an edit is answered with. ---
    let heard = ask(
        session,
        run,
        &mut cube,
        Command::SetObjectMaterial {
            local_id,
            material: Material::Metal,
        },
    )
    .await?;
    record_edit(
        metrics,
        "material",
        &heard,
        &cube,
        cube.material == Material::Metal.to_code(),
    );

    let heard = ask(
        session,
        run,
        &mut cube,
        Command::SetObjectFlags {
            local_id,
            flags: ObjectFlagSettings {
                is_phantom: true,
                ..ObjectFlagSettings::default()
            },
        },
    )
    .await?;
    record_edit(
        metrics,
        "phantom",
        &heard,
        &cube,
        cube.update_flags & FLAGS_PHANTOM != 0,
    );

    let heard = ask(
        session,
        run,
        &mut cube,
        Command::SetObjectShape {
            local_id,
            shape: hollow_box_shape(),
        },
    )
    .await?;
    record_edit(
        metrics,
        "shape",
        &heard,
        &cube,
        cube.shape.profile_hollow == HOLLOW_25_PCT,
    );

    let heard = ask(
        session,
        run,
        &mut cube,
        Command::SetObjectClickAction {
            local_id,
            action: ClickAction::Sit,
        },
    )
    .await?;
    record_edit(
        metrics,
        "click_action",
        &heard,
        &cube,
        cube.click_action == ClickAction::Sit.to_code(),
    );

    let heard = ask(
        session,
        run,
        &mut cube,
        Command::SetObjectIncludeInSearch {
            local_id,
            include: true,
        },
    )
    .await?;
    record_edit(
        metrics,
        "search",
        &heard,
        &cube,
        cube.update_flags & FLAGS_INCLUDE_IN_SEARCH != 0,
    );

    let up = lifted(&stood, 1.0);
    let heard = ask(session, run, &mut cube, moved(local_id, &up)).await?;
    record_edit(
        metrics,
        "move",
        &heard,
        &cube,
        near(&cube.motion.position, &up),
    );

    let heard = ask(
        session,
        run,
        &mut cube,
        transformed(
            local_id,
            ObjectTransform {
                position: None,
                rotation: Some(quarter_turn()),
                scale: None,
                group: true,
                uniform: false,
            },
        ),
    )
    .await?;
    record_edit(
        metrics,
        "turn",
        &heard,
        &cube,
        same_rotation(&cube.motion.rotation, &quarter_turn()),
    );

    let heard = ask(session, run, &mut cube, resized(local_id, 0.5, 0.75, 0.25)).await?;
    record_edit(
        metrics,
        "resize",
        &heard,
        &cube,
        (cube.scale.y - 0.75).abs() < 0.001,
    );

    // All three at once, which no build floater sends: of the prim alone,
    // and of its linkset.
    for (key, group, lift) in [("all_three", false, 2.0), ("all_three_linkset", true, 3.0)] {
        let to = lifted(&stood, lift);
        let heard = ask(
            session,
            run,
            &mut cube,
            transformed(
                local_id,
                ObjectTransform {
                    position: Some(to.clone()),
                    rotation: Some(Rotation {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        s: 1.0,
                    }),
                    scale: Some(Vector {
                        x: lift / 4.0,
                        y: 0.5,
                        z: 0.5,
                    }),
                    group,
                    uniform: false,
                },
            ),
        )
        .await?;
        record_edit(
            metrics,
            key,
            &heard,
            &cube,
            near(&cube.motion.position, &to) && (cube.scale.x - lift / 4.0).abs() < 0.001,
        );
    }
    ask(session, run, &mut cube, moved(local_id, &stood)).await?;

    // --- 2. Sizes a grid does not take. ---
    let heard = ask(
        session,
        run,
        &mut cube,
        resized(local_id, 0.001, 0.001, 0.001),
    )
    .await?;
    metrics.set("tiny_answered_with", answered(&heard, &cube));
    metrics.set("tiny_reads", millimetres(cube.scale.x));
    metrics.set("tiny_alerts", joined(&heard.alerts));

    let heard = ask(session, run, &mut cube, resized(local_id, 1000.0, 0.5, 0.5)).await?;
    metrics.set("huge_answered_with", answered(&heard, &cube));
    metrics.set("huge_reads", millimetres(cube.scale.x));
    metrics.set("huge_alerts", joined(&heard.alerts));
    // Not left a moment longer than it takes to read.
    ask(session, run, &mut cube, resized(local_id, 0.5, 0.5, 0.5)).await?;

    // --- 3. Names a grid does not take. ---
    let long = "n".repeat(LONGEST_TEXT);
    session
        .send(Command::SetObjectName {
            local_id,
            name: long.clone(),
        })
        .await?;
    session
        .send(Command::SetObjectDescription {
            local_id,
            description: long,
        })
        .await?;
    let record = record_of(session, run, &cube).await?;
    metrics.set("long_name_reads_bytes", count(record.name.len()));
    metrics.set(
        "long_description_reads_bytes",
        count(record.description.len()),
    );
    for (key, name) in [
        ("piped_name", "a|b\nc"),
        ("accented_name", "on\u{e9}off \u{20ac}"),
        ("empty_name", ""),
    ] {
        session
            .send(Command::SetObjectName {
                local_id,
                name: name.to_owned(),
            })
            .await?;
        session
            .send(Command::SetObjectDescription {
                local_id,
                description: name.to_owned(),
            })
            .await?;
        let record = record_of(session, run, &cube).await?;
        metrics.set(&format!("{key}_reads"), format!("{:?}", record.name));
        metrics.set(
            &format!("{key}_description_reads"),
            format!("{:?}", record.description),
        );
    }
    session
        .send(Command::SetObjectCategory {
            local_id,
            category: 3,
        })
        .await?;
    let record = record_of(session, run, &cube).await?;
    metrics.set("category_reads", i64::from(record.category));
    Ok(())
}

/// Legs 4 and 5: the masks a grid does not take, and a group and a deed that
/// are nobody's.
async fn masks_and_groups(
    session: &mut Session,
    run: &mut Run,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let cube = rez_cube(session, run, residue, 1).await?;
    let local_id = cube.scoped_id();
    let baseline = hold_selected(session, run, &cube).await?;
    record_masks(metrics, "new_prim", &baseline);

    let four = Permissions::MOVE
        .union(Permissions::COPY)
        .union(Permissions::MODIFY)
        .union(Permissions::TRANSFER);
    let everything = Permissions::from_bits(0x7fff_ffff);
    for (key, field, set, mask) in [
        (
            "next_owner_everything",
            PermissionField::NextOwner,
            true,
            everything,
        ),
        (
            "next_owner_nothing",
            PermissionField::NextOwner,
            false,
            everything,
        ),
        (
            "everyone_copy_move",
            PermissionField::Everyone,
            true,
            Permissions::COPY.union(Permissions::MOVE),
        ),
        (
            "everyone_modify_transfer",
            PermissionField::Everyone,
            true,
            Permissions::MODIFY.union(Permissions::TRANSFER),
        ),
        (
            "everyone_everything",
            PermissionField::Everyone,
            true,
            everything,
        ),
        (
            "everyone_nothing",
            PermissionField::Everyone,
            false,
            everything,
        ),
        ("group_share", PermissionField::Group, true, four),
        ("group_nothing", PermissionField::Group, false, everything),
        (
            "owner_less_transfer",
            PermissionField::Owner,
            false,
            Permissions::TRANSFER,
        ),
        (
            "base_less_transfer",
            PermissionField::Base,
            false,
            Permissions::TRANSFER,
        ),
    ] {
        session
            .send(Command::SetObjectPermissions {
                local_ids: vec![local_id],
                field,
                set,
                mask,
            })
            .await?;
        let record = record_of(session, run, &cube).await?;
        record_masks(metrics, key, &record);
    }

    // --- 5. A group and a deed that are nobody's. ---
    let nobodys = GroupKey::from(Uuid::new_v4());
    session
        .send(Command::SetObjectGroup {
            local_ids: vec![local_id],
            group_id: nobodys,
        })
        .await?;
    let heard = hear(session, run, &cube).await?;
    let record = record_of(session, run, &cube).await?;
    metrics.set(
        "foreign_group_reads",
        match record.group {
            None => "none",
            Some(group) if group == nobodys => "the group named",
            Some(_other) => "another group",
        },
    );
    metrics.set("foreign_group_alerts", joined(&heard.alerts));

    session
        .send(Command::DeedObjectsToGroup {
            local_ids: vec![local_id],
            group_id: nobodys,
        })
        .await?;
    let heard = hear(session, run, &cube).await?;
    let record = record_of(session, run, &cube).await?;
    metrics.set(
        "foreign_deed_owner_reads",
        match record.owner {
            OwnerKey::Agent(agent) if agent == run.owner => "the agent",
            OwnerKey::Agent(_other) => "another agent",
            OwnerKey::Group(group) if group == nobodys => "the group named",
            OwnerKey::Group(_other) => "another group",
        },
    );
    metrics.set("foreign_deed_alerts", joined(&heard.alerts));
    Ok(())
}

/// Leg 6: what an undo steps back out of, and how many steps a grid keeps.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of edits and of the steps back out of them"
)]
async fn undo(
    session: &mut Session,
    run: &mut Run,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let mut cube = rez_cube(session, run, residue, 2).await?;
    let local_id = cube.scoped_id();
    let stood = cube.motion.position.clone();
    hold_selected(session, run, &cube).await?;

    // One edit of each kind, the record's first and the transform's last.
    session
        .send(Command::SetObjectName {
            local_id,
            name: "SLClientEditUndo".to_owned(),
        })
        .await?;
    session
        .send(Command::SetObjectForSale {
            local_id,
            sale_type: SaleType::Copy,
            sale_price: Some(LindenAmount(SALE_PRICE)),
        })
        .await?;
    hear(session, run, &cube).await?;
    for command in [
        Command::SetObjectMaterial {
            local_id,
            material: Material::Metal,
        },
        Command::SetObjectClickAction {
            local_id,
            action: ClickAction::Sit,
        },
        Command::SetObjectFlags {
            local_id,
            flags: ObjectFlagSettings {
                is_phantom: true,
                ..ObjectFlagSettings::default()
            },
        },
        Command::SetObjectShape {
            local_id,
            shape: hollow_box_shape(),
        },
        resized(local_id, 0.5, 0.75, 0.25),
        transformed(
            local_id,
            ObjectTransform {
                position: None,
                rotation: Some(quarter_turn()),
                scale: None,
                group: true,
                uniform: false,
            },
        ),
        moved(local_id, &lifted(&stood, 1.0)),
    ] {
        ask(session, run, &mut cube, command).await?;
    }
    let edited = cube.clone();

    let mut steps = Vec::new();
    for step in 0..SCOPE_STEPS {
        let before = cube.clone();
        let heard = ask(
            session,
            run,
            &mut cube,
            Command::UndoObjects {
                local_ids: vec![local_id],
            },
        )
        .await?;
        if step == 0 {
            metrics.set("undo_answered_with", answered(&heard, &cube));
            metrics.set("undo_alerts", joined(&heard.alerts));
        }
        steps.push(differences(&before, &cube));
    }
    metrics.set("undo_steps", sequence(&steps));
    let record = record_of(session, run, &cube).await?;
    metrics.set("undo_reverts_the_name", record.name != "SLClientEditUndo");
    metrics.set(
        "undo_reverts_the_sale",
        record.sale_type != SaleType::Copy.to_code(),
    );
    metrics.set("undone_differs_from_edited_in", differences(&edited, &cube));

    let mut steps = Vec::new();
    for step in 0..SCOPE_STEPS {
        let before = cube.clone();
        let heard = ask(
            session,
            run,
            &mut cube,
            Command::RedoObjects {
                local_ids: vec![local_id],
            },
        )
        .await?;
        if step == 0 {
            metrics.set("redo_answered_with", answered(&heard, &cube));
        }
        steps.push(differences(&before, &cube));
    }
    metrics.set("redo_steps", sequence(&steps));
    metrics.set("redone_differs_from_edited_in", differences(&edited, &cube));

    // How many steps a grid keeps: two dozen moves of a cube nothing else was
    // done to, and back until nothing moves.
    let mut cube = rez_cube(session, run, residue, 7).await?;
    let local_id = cube.scoped_id();
    hold_selected(session, run, &cube).await?;
    let from = cube.motion.position.clone();
    for step in 1..=DEPTH_MOVES {
        let to = lifted(&from, f32::from(step) * DEPTH_STEP_M);
        ask(session, run, &mut cube, moved(local_id, &to)).await?;
    }
    let mut depth = 0_i64;
    let mut past_the_end = None;
    for _step in 0..DEPTH_UNDOS {
        let before = cube.clone();
        let heard = ask(
            session,
            run,
            &mut cube,
            Command::UndoObjects {
                local_ids: vec![local_id],
            },
        )
        .await?;
        if near(&before.motion.position, &cube.motion.position) {
            past_the_end = Some(answered(&heard, &cube));
            break;
        }
        depth = depth.saturating_add(1);
    }
    metrics.set("undo_depth", depth);
    metrics.set(
        "undo_past_the_end_answered_with",
        past_the_end.unwrap_or_else(|| "never reached".to_owned()),
    );
    metrics.set(
        "undo_depth_ends",
        if near(&cube.motion.position, &from) {
            "where the moves began"
        } else {
            "short of where the moves began"
        },
    );
    Ok(())
}

/// Leg 7: what a duplicate copies.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow: an original made, and two copies each read field by field"
)]
async fn duplicates(
    session: &mut Session,
    run: &mut Run,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    // Something to put into the original's contents: an item made of a cube.
    let donor = rez_cube(session, run, residue, 3).await?;
    session
        .send(Command::DerezObjects {
            local_ids: vec![donor.scoped_id()],
            destination: DeRezDestination::TakeIntoAgentInventory(run.folders.objects),
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        })
        .await?;
    let taken = listen(session, STEP_TIMEOUT, run.pace.quiet, |heard| {
        heard.killed(donor.scoped_id()) && !heard.announced.is_empty()
    })
    .await?;
    if taken.killed(donor.scoped_id()) {
        residue.roots.retain(|root| *root != donor.scoped_id());
    }
    note_items(residue, &taken);
    let item = taken.items().next().cloned().ok_or_else(|| {
        TestFailure::Assertion("a take was answered with no inventory item".to_owned())
    })?;

    let mut original = rez_cube(session, run, residue, 3).await?;
    let local_id = original.scoped_id();
    hold_selected(session, run, &original).await?;
    for command in [
        Command::SetObjectName {
            local_id,
            name: DUPLICATE_NAME.to_owned(),
        },
        Command::SetObjectDescription {
            local_id,
            description: DUPLICATE_DESCRIPTION.to_owned(),
        },
        Command::SetObjectPermissions {
            local_ids: vec![local_id],
            field: PermissionField::NextOwner,
            set: true,
            mask: Permissions::COPY,
        },
        Command::SetObjectForSale {
            local_id,
            sale_type: SaleType::Copy,
            sale_price: Some(LindenAmount(SALE_PRICE)),
        },
        Command::SetObjectClickAction {
            local_id,
            action: ClickAction::Sit,
        },
        Command::SetObjectMaterial {
            local_id,
            material: Material::Metal,
        },
        Command::UpdateTaskInventory {
            target: local_id,
            key: TaskInventoryKey::Item,
            item: Box::new(super::task_inventory::task_item(&item)),
        },
    ] {
        ask(session, run, &mut original, command).await?;
    }
    let record = record_of(session, run, &original).await?;
    metrics.set(
        "original_inventory_serial",
        i64::from(record.inventory_serial),
    );
    metrics.set(
        "original_contents_items",
        contents(session, run, &original).await?,
    );

    // A copy by an offset.
    session
        .send(Command::DuplicateObjects {
            local_ids: vec![local_id],
            offset: Vector {
                x: 0.0,
                y: DUPLICATE_OFFSET_M,
                z: 0.0,
            },
            group_id: None,
        })
        .await?;
    let expected = Vector {
        x: original.motion.position.x,
        y: original.motion.position.y + DUPLICATE_OFFSET_M,
        z: original.motion.position.z,
    };
    record_copy(
        session,
        run,
        residue,
        metrics,
        "duplicate",
        &original,
        &record,
        &expected,
    )
    .await?;

    // A copy on a ray whose end is where the copy is to stand, aimed at
    // nothing.
    let end = Vector {
        x: original.motion.position.x,
        y: DUPLICATE_OFFSET_M.mul_add(2.0, original.motion.position.y),
        z: original.motion.position.z,
    };
    session
        .send(Command::DuplicateObjectsOnRay {
            local_ids: vec![local_id],
            group_id: None,
            ray_start: lifted(&end, 3.0),
            ray_end: end.clone(),
            bypass_raycast: true,
            ray_end_is_intersection: true,
            copy_centers: true,
            copy_rotates: true,
            ray_target_id: None,
            duplicate_flags: FLAGS_CREATE_SELECTED,
        })
        .await?;
    record_copy(
        session, run, residue, metrics, "on_ray", &original, &record, &end,
    )
    .await?;

    // A copy on a ray cast down at the original, as a build tool's click on
    // an object casts it.
    let top = lifted(&original.motion.position, original.scale.z);
    session
        .send(Command::DuplicateObjectsOnRay {
            local_ids: vec![local_id],
            group_id: None,
            ray_start: lifted(&original.motion.position, 3.0),
            ray_end: original.motion.position.clone(),
            bypass_raycast: false,
            ray_end_is_intersection: false,
            copy_centers: true,
            copy_rotates: true,
            ray_target_id: Some(original.full_id),
            duplicate_flags: FLAGS_CREATE_SELECTED,
        })
        .await?;
    record_copy(
        session,
        run,
        residue,
        metrics,
        "on_ray_at_a_prim",
        &original,
        &record,
        &top,
    )
    .await
}

/// Where a cube standing at a place is sent to instead.
type Elsewhere = fn(&Vector) -> Vector;

/// Leg 8: the places a grid does not take.
async fn places(
    session: &mut Session,
    run: &mut Run,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let targets: [(&str, u8, Elsewhere); 3] = [
        ("far_above", 4, |stood| Vector {
            z: FAR_ABOVE_M,
            ..stood.clone()
        }),
        ("far_below", 5, |stood| Vector {
            z: FAR_BELOW_M,
            ..stood.clone()
        }),
        ("off_the_edge", 6, |stood| Vector {
            x: -PAST_THE_EDGE_M,
            ..stood.clone()
        }),
    ];
    for (key, slot, target) in targets {
        let mut cube = rez_cube(session, run, residue, slot).await?;
        let local_id = cube.scoped_id();
        let stood = cube.motion.position.clone();
        hold_selected(session, run, &cube).await?;
        let to = target(&stood);
        session.send(moved(local_id, &to)).await?;
        let mut heard = listen(
            session,
            run.pace.quiet.saturating_mul(2),
            run.pace.quiet,
            |heard| {
                heard.killed(local_id)
                    || !heard.alerts.is_empty()
                    || heard
                        .changed
                        .iter()
                        .any(|object| object.local_id == cube.local_id)
            },
        )
        .await?;
        if let Some(latest) = latest(&heard, &cube) {
            cube = latest;
        }
        let gone = heard.killed(local_id);
        if gone {
            residue.roots.retain(|root| *root != local_id);
            wait_for_notice(session, &mut heard, run.pace).await?;
        } else {
            reread_if_silent(session, run, &mut cube, &heard).await?;
        }
        note_items(residue, &heard);
        metrics.set(&format!("{key}_answered_with"), answered(&heard, &cube));
        metrics.set(&format!("{key}_kills_the_object"), gone);
        metrics.set(
            &format!("{key}_reads"),
            if gone {
                "gone".to_owned()
            } else if near(&cube.motion.position, &to) {
                "where it was sent".to_owned()
            } else if near(&cube.motion.position, &stood) {
                "where it stood".to_owned()
            } else {
                place(&cube.motion.position)
            },
        );
        metrics.set(&format!("{key}_announcement"), heard.announcement());
        metrics.set(
            &format!("{key}_item_lands_in"),
            heard
                .items()
                .next()
                .map_or("nowhere", |item| run.folders.word(item.folder_id)),
        );
        metrics.set(&format!("{key}_alerts"), joined(&heard.alerts));
        metrics.set(&format!("{key}_messages"), joined(&heard.messages));
        metrics.set(&format!("{key}_told_by"), heard.told_by());
        heard.record_notice(metrics, key);
        // The message names the object, the parcel and the spot; what it says
        // of why is the part that is the grid's.
        metrics.set(
            &format!("{key}_notice_reason"),
            heard
                .messages
                .first()
                .and_then(|message| message.rsplit_once(" because "))
                .map_or("none", |(_before, reason)| reason)
                .to_owned(),
        );
        metrics.set(
            &format!("{key}_appears_elsewhere"),
            heard
                .elsewhere
                .iter()
                .any(|object| object.full_id == cube.full_id),
        );
        if !gone && !near(&cube.motion.position, &stood) {
            // Back where the cleanup can reach it.
            ask(session, run, &mut cube, moved(local_id, &stood)).await?;
        }
    }
    Ok(())
}

/// Listens for a copy of `original` to appear, records under `prefix` what
/// the grid sent with it and what its record says against `record`, the
/// original's, and notes it for the cleanup.
#[expect(
    clippy::too_many_arguments,
    reason = "the run, where to note the copy, where to record it, and the three things a copy \
              is read against"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one field of the copy after another, each against the original's"
)]
async fn record_copy(
    session: &mut Session,
    run: &mut Run,
    residue: &mut Residue,
    metrics: &mut Metrics,
    prefix: &str,
    original: &Object,
    record: &Properties,
    expected: &Vector,
) -> Result<(), TestFailure> {
    let owner = run.owner.uuid();
    let seen = run.seen.clone();
    let is_new_root = move |object: &Object| {
        is_own(object, owner)
            && object.parent_id == RegionLocalObjectId(0)
            && !seen.contains(&object.scoped_id())
    };
    let heard = listen(
        session,
        run.pace.quiet.saturating_mul(3),
        run.pace.quiet,
        |heard| heard.added.iter().any(&is_new_root),
    )
    .await?;
    let copy = heard
        .added
        .iter()
        .find(|object| is_new_root(object))
        .cloned();
    run.seen.extend(heard.added.iter().map(Object::scoped_id));
    metrics.set(&format!("{prefix}_appears"), copy.is_some());
    metrics.set(&format!("{prefix}_alerts"), joined(&heard.alerts));
    let Some(copy) = copy else {
        return Ok(());
    };
    residue.roots.push(copy.scoped_id());
    metrics.set(&format!("{prefix}_brings"), heard.brought(&copy));
    metrics.set(
        &format!("{prefix}_flagged_selected"),
        copy.update_flags & FLAGS_CREATE_SELECTED != 0,
    );
    metrics.set(
        &format!("{prefix}_stands"),
        if near(&copy.motion.position, expected) {
            "where it was asked to".to_owned()
        } else if near(&copy.motion.position, &original.motion.position) {
            "where the original stands".to_owned()
        } else {
            let at = &copy.motion.position;
            format!(
                "{:+.2}, {:+.2}, {:+.2} from where it was asked to",
                at.x - expected.x,
                at.y - expected.y,
                at.z - expected.z
            )
        },
    );
    metrics.set(
        &format!("{prefix}_differs_from_the_original_in"),
        differences(
            &Object {
                motion: copy.motion.clone(),
                ..original.clone()
            },
            &Object {
                update_flags: original.update_flags,
                ..copy.clone()
            },
        ),
    );
    let copied = record_of(session, run, &copy).await?;
    metrics.set(&format!("{prefix}_name_copied"), copied.name == record.name);
    metrics.set(
        &format!("{prefix}_description_copied"),
        copied.description == record.description,
    );
    metrics.set(
        &format!("{prefix}_owner_is_the_agent"),
        copied.owner == OwnerKey::Agent(run.owner),
    );
    metrics.set(
        &format!("{prefix}_creator_is_the_agent"),
        copied.creator_id == run.owner,
    );
    metrics.set(
        &format!("{prefix}_last_owner"),
        if copied.last_owner_id.is_nil() {
            "nobody"
        } else if copied.last_owner_id == run.owner.uuid() {
            "the agent"
        } else {
            "somebody else"
        },
    );
    metrics.set(
        &format!("{prefix}_last_owner_is_the_originals"),
        copied.last_owner_id == record.last_owner_id,
    );
    metrics.set(
        &format!("{prefix}_creation_date"),
        if copied.creation_date == record.creation_date {
            "the original's"
        } else if copied.creation_date > record.creation_date {
            "its own"
        } else {
            "an earlier one"
        },
    );
    metrics.set(
        &format!("{prefix}_masks_copied"),
        copied.permissions == record.permissions,
    );
    metrics.set(
        &format!("{prefix}_mask_next_owner"),
        hex(copied.permissions.next_owner),
    );
    metrics.set(
        &format!("{prefix}_sale_copied"),
        copied.sale_type == record.sale_type && copied.sale_price == record.sale_price,
    );
    metrics.set(&format!("{prefix}_sale_type"), i64::from(copied.sale_type));
    metrics.set(
        &format!("{prefix}_item_id_set"),
        !copied.item_id.uuid().is_nil(),
    );
    metrics.set(
        &format!("{prefix}_from_task_id_set"),
        copied.from_task_id.is_some(),
    );
    metrics.set(
        &format!("{prefix}_inventory_serial"),
        i64::from(copied.inventory_serial),
    );
    metrics.set(
        &format!("{prefix}_contents_items"),
        contents(session, run, &copy).await?,
    );
    Ok(())
}

/// How many items `object` has in its contents.
pub(crate) async fn contents(
    session: &mut Session,
    run: &Run,
    object: &Object,
) -> Result<i64, TestFailure> {
    let task = object.full_id;
    session
        .send(Command::RequestTaskInventory {
            target: object.scoped_id(),
        })
        .await?;
    let reply = session
        .wait_for(STEP_TIMEOUT, |event| match event {
            Event::TaskInventoryReply(reply) if reply.task == task => Some(reply.clone()),
            _ => None,
        })
        .await?;
    if reply.filename.is_empty() {
        return Ok(0);
    }
    session
        .send(Command::FetchTaskInventory {
            target: object.scoped_id(),
        })
        .await?;
    let listing = crate::support::wait_for_task_listing(session, task, STEP_TIMEOUT).await?;
    // Whatever the listing's transfer left behind is not the next leg's.
    listen(session, run.pace.quiet, run.pace.quiet, |_heard| false).await?;
    Ok(count(listing.len()))
}

/// Selects `object` and leaves it selected, as a build floater holds what it
/// edits, returning its record.
async fn hold_selected(
    session: &mut Session,
    run: &Run,
    object: &Object,
) -> Result<Properties, TestFailure> {
    record_of(session, run, object).await
}

/// Selects `object` — again, where it is held selected already — and returns
/// the record the grid answers with.
pub(crate) async fn record_of(
    session: &mut Session,
    run: &Run,
    object: &Object,
) -> Result<Properties, TestFailure> {
    let object_id = object.full_id;
    session
        .send(Command::RequestObjectProperties {
            local_ids: vec![object.scoped_id()],
        })
        .await?;
    // The last record of the window: an edit sent just before the select may
    // be answered with a record of its own, ahead of the select's.
    let heard = listen(session, STEP_TIMEOUT, run.pace.quiet, |heard| {
        heard
            .properties
            .iter()
            .any(|properties| properties.object_id == object_id)
    })
    .await?;
    heard
        .properties
        .into_iter()
        .rfind(|properties| properties.object_id == object_id)
        .ok_or_else(|| {
            TestFailure::Assertion("a select of our own object went unanswered".to_owned())
        })
}

/// Sends `command`, an edit of `cube`, listens for what it brings and brings
/// `cube` up to what the grid last said of it.
pub(crate) async fn ask(
    session: &mut Session,
    run: &Run,
    cube: &mut Object,
    command: Command,
) -> Result<Heard, TestFailure> {
    session.send(command).await?;
    let heard = hear(session, run, cube).await?;
    if let Some(latest) = latest(&heard, cube) {
        *cube = latest;
    } else {
        // A grid that says nothing of an edit may have taken it all the same.
        reread_if_silent(session, run, cube, &heard).await?;
    }
    Ok(heard)
}

/// Listens until something is said about `cube` and a little longer, or for
/// as long as an answer may take.
pub(crate) async fn hear(
    session: &mut Session,
    run: &Run,
    cube: &Object,
) -> Result<Heard, TestFailure> {
    let local_id = cube.local_id;
    let object_id = cube.full_id;
    listen(
        session,
        run.pace.quiet,
        run.pace.quiet.checked_div(3).unwrap_or(run.pace.quiet),
        |heard| {
            !heard.alerts.is_empty()
                || heard
                    .changed
                    .iter()
                    .any(|object| object.local_id == local_id)
                || heard
                    .properties
                    .iter()
                    .any(|properties| properties.object_id == object_id)
                || heard
                    .kills
                    .iter()
                    .any(|(_after, ids)| ids.contains(&local_id))
        },
    )
    .await
}

/// `cube` as the last update of the window left it.
pub(crate) fn latest(heard: &Heard, cube: &Object) -> Option<Object> {
    heard
        .changed
        .iter()
        .rev()
        .find(|object| object.local_id == cube.local_id)
        .cloned()
}

/// Asks the grid for `cube` again where a window brought no update of it, so
/// that what is read of it afterwards is the grid's and not what was sent.
async fn reread_if_silent(
    session: &mut Session,
    run: &Run,
    cube: &mut Object,
    heard: &Heard,
) -> Result<(), TestFailure> {
    if latest(heard, cube).is_some() {
        return Ok(());
    }
    reread(session, run, cube).await
}

/// Asks the grid for `cube` again and brings it up to the answer, where
/// there is one.
pub(crate) async fn reread(
    session: &mut Session,
    run: &Run,
    cube: &mut Object,
) -> Result<(), TestFailure> {
    let local_id = cube.scoped_id();
    session
        .send(Command::RequestObjects {
            local_ids: vec![local_id],
        })
        .await?;
    match session
        .wait_for(run.pace.quiet, |event| match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                if object.scoped_id() == local_id =>
            {
                Some((**object).clone())
            }
            _ => None,
        })
        .await
    {
        Ok(object) => {
            *cube = object;
            Ok(())
        }
        Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// What a window brought of `cube`: the forms it came in, its record, an
/// alert — or `nothing`.
pub(crate) fn answered(heard: &Heard, cube: &Object) -> String {
    let brought = heard.brought(cube);
    match (brought.as_str(), heard.alerts.is_empty()) {
        (_, true) => brought,
        ("nothing", false) => "alert".to_owned(),
        (_, false) => format!("{brought}+alert"),
    }
}

/// Records what an edit was answered with and whether it took.
fn record_edit(metrics: &mut Metrics, key: &str, heard: &Heard, cube: &Object, took: bool) {
    metrics.set(&format!("{key}_answered_with"), answered(heard, cube));
    metrics.set(&format!("{key}_took"), took);
}

/// Records the five masks of `record` under `prefix`.
pub(crate) fn record_masks(metrics: &mut Metrics, prefix: &str, record: &Properties) {
    let masks = &record.permissions;
    metrics.set(
        &format!("{prefix}_masks"),
        format!(
            "base {} owner {} group {} everyone {} next {}",
            hex(masks.base),
            hex(masks.owner),
            hex(masks.group),
            hex(masks.everyone),
            hex(masks.next_owner)
        ),
    );
}

/// A mask as the eight hex digits the book writes it in.
pub(crate) fn hex(mask: Permissions) -> String {
    format!("{:#010x}", mask.bits())
}

/// A count as a metric.
pub(crate) fn count(count: usize) -> i64 {
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// A length in metres, to the millimetre.
fn millimetres(metres: f32) -> String {
    format!("{metres:.3}")
}

/// A region position, to the centimetre.
fn place(at: &Vector) -> String {
    format!("{:.2}, {:.2}, {:.2}", at.x, at.y, at.z)
}

/// What differs between two states of one object, of what an edit can change:
/// `position`, `rotation`, `scale`, `shape`, `material`, `click action` and
/// `flags`, `+`-joined — or `nothing`.
pub(crate) fn differences(before: &Object, after: &Object) -> String {
    let mut parts = Vec::new();
    if !near(&before.motion.position, &after.motion.position) {
        parts.push("position");
    }
    if !same_rotation(&before.motion.rotation, &after.motion.rotation) {
        parts.push("rotation");
    }
    if !near(&before.scale, &after.scale) {
        parts.push("scale");
    }
    if before.shape != after.shape {
        parts.push("shape");
    }
    if before.material != after.material {
        parts.push("material");
    }
    if before.click_action != after.click_action {
        parts.push("click action");
    }
    // The rest of the flags are the viewer's own business with the object —
    // whether it holds it selected, what it may do with it — and not the
    // object's.
    if (before.update_flags ^ after.update_flags) & FLAGS_OF_THE_OBJECT != 0 {
        parts.push("flags");
    }
    if parts.is_empty() {
        "nothing".to_owned()
    } else {
        parts.join("+")
    }
}

/// The steps of an undo or a redo, in order, without the run of `nothing` they
/// end in.
fn sequence(steps: &[String]) -> String {
    let kept = steps
        .iter()
        .rposition(|step| step != "nothing")
        .map_or(0, |last| last.saturating_add(1));
    let told: Vec<&str> = steps.iter().take(kept).map(String::as_str).collect();
    if told.is_empty() {
        "nothing".to_owned()
    } else {
        told.join(", ")
    }
}

/// `position`, `metres` higher.
pub(crate) fn lifted(position: &Vector, metres: f32) -> Vector {
    Vector {
        x: position.x,
        y: position.y,
        z: position.z + metres,
    }
}

/// A move of `local_id` to `position`.
pub(crate) fn moved(local_id: ScopedObjectId, position: &Vector) -> Command {
    transformed(
        local_id,
        ObjectTransform {
            position: Some(position.clone()),
            rotation: None,
            scale: None,
            group: true,
            uniform: false,
        },
    )
}

/// A resize of `local_id`.
pub(crate) const fn resized(local_id: ScopedObjectId, x: f32, y: f32, z: f32) -> Command {
    transformed(
        local_id,
        ObjectTransform {
            position: None,
            rotation: None,
            scale: Some(Vector { x, y, z }),
            group: true,
            uniform: false,
        },
    )
}

/// A `MultipleObjectUpdate` of `local_id`.
pub(crate) const fn transformed(local_id: ScopedObjectId, transform: ObjectTransform) -> Command {
    Command::UpdateObject {
        local_id,
        transform,
    }
}

/// A quarter turn about the Z axis.
pub(crate) const fn quarter_turn() -> Rotation {
    Rotation {
        x: 0.0,
        y: 0.0,
        z: std::f32::consts::FRAC_1_SQRT_2,
        s: std::f32::consts::FRAC_1_SQRT_2,
    }
}

/// Whether two vectors are the same, to within [`POSITION_EPSILON_M`].
pub(crate) fn near(left: &Vector, right: &Vector) -> bool {
    (left.x - right.x).abs() <= POSITION_EPSILON_M
        && (left.y - right.y).abs() <= POSITION_EPSILON_M
        && (left.z - right.z).abs() <= POSITION_EPSILON_M
}

/// Whether two rotations turn an object the same way: a rotation and its
/// negation do.
fn same_rotation(left: &Rotation, right: &Rotation) -> bool {
    let dot = left.s.mul_add(
        right.s,
        left.x
            .mul_add(right.x, left.y.mul_add(right.y, left.z * right.z)),
    );
    (dot.abs() - 1.0).abs() <= ROTATION_EPSILON
}

/// The path and profile of a box hollowed by a quarter.
pub(crate) const fn hollow_box_shape() -> PrimShapeParams {
    PrimShapeParams {
        // LL_PCODE_PATH_LINE / LL_PCODE_PROFILE_SQUARE — a box.
        path_curve: 0x10,
        profile_curve: 0x01,
        path_begin: 0,
        path_end: 0,
        // 200 - 1.0 / 0.01 = 100 (full top size on both axes).
        path_scale_x: 100,
        path_scale_y: 100,
        path_shear_x: 0,
        path_shear_y: 0,
        path_twist: 0,
        path_twist_begin: 0,
        path_radius_offset: 0,
        path_taper_x: 0,
        path_taper_y: 0,
        path_revolutions: 0,
        path_skew: 0,
        profile_begin: 0,
        profile_end: 0,
        profile_hollow: HOLLOW_25_PCT,
    }
}
