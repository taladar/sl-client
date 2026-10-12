//! A census of touching and grabbing: what a script is told of a click, a
//! held button and a drag, which prim of a linkset is told, and what a drag
//! does to the prim itself.
//!
//! A viewer has three messages for a left button on a prim — `ObjectGrab` at
//! the press ([`Command::GrabObject`]), `ObjectGrabUpdate` while it is held
//! ([`Command::GrabObjectUpdate`]) and `ObjectDeGrab` at the release
//! ([`Command::DegrabObject`]) — and three more for turning what it holds
//! ([`Command::SpinObjectStart`] and its two siblings). No grid answers any
//! of them. What they do shows in two places: in what a script in the prim is
//! handed (`touch_start`, `touch`, `touch_end` and the `llDetected*` values),
//! and in where the prim is afterwards.
//!
//! The case rezzes cubes as the primary avatar, has a second avatar stand by,
//! and asks one question a leg:
//!
//! 1. **A prim with no script**: a click, and then a drag, a drag with no
//!    grab before it and a spin — by the owner and by the other avatar.
//! 2. **The flags a viewer reads** to know a prim takes a touch, as a script
//!    with touch handlers lands in it, is replaced by one without, and goes.
//! 3. **A click**, with no surface data, with it, and with data no prim has.
//! 4. **A held button**, with and without updates: how many `touch` events a
//!    script gets, and which message's surface data each event carries.
//! 5. **Messages out of order**: a release with no press, two presses and
//!    one release, an update with no press.
//! 6. **Another avatar's click**, and whom the script is told of.
//! 7. **Click actions**: whether a prim set to sit or to take no click still
//!    hands its script a touch.
//! 8. **A drag of a scripted prim.**
//! 9. **A linkset**: which of the root's and a child's scripts is told of a
//!    touch on the child, with each setting of `llPassTouches`.
//! 10. **A physical prim**: a spin and a drag by its owner, and a drag by
//!     the other avatar.
//!
//! The script is the probe: it says each event it is handed, with the
//! detected values, to its owner. A grid that runs no scripts answers legs 2
//! to 9 with silence, so only leg 1 is asked of the fake grid.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/building.md` § Touch and grab; every run is held to it.
//!
//! `2av`, `[both]`, the first leg offline on both fake flavours. The cubes
//! are derezzed to the Trash, also when a leg fails.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, ClickAction, Command, Event, InventoryKey, Object, ObjectFlagSettings,
    RegionLocalObjectId, RestoreItem, RezScriptParams, Rotation, ScopedObjectId, ScriptTarget,
    ScriptUploadLocation, SurfaceInfo, Uuid, Vector,
};

use crate::cases::object_edit::{quarter_turn, reread};
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up,
    rez_cube,
};
use crate::cases::task_inventory::{
    DRAIN, FAKE_DRAIN, Heard, OPENSIM_START, find_object, hold_to_measured, listen, listing,
    login_folders,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, settle_scene};

/// What every line the probe says begins with.
const PROBE_TAG: &str = "SLCTG";

/// The name of the probe script in a prim.
const PROBE_NAME: &str = "SLClientTouchProbe";

/// The update flag of a prim that holds a running script.
const FLAGS_SCRIPTED: u32 = 1 << 6;

/// The update flag of a prim a script of which handles touches.
const FLAGS_HANDLE_TOUCH: u32 = 1 << 7;

/// The update flag of a physical prim.
const FLAGS_USE_PHYSICS: u32 = 1;

/// How long a button is held.
const HOLD: Duration = Duration::from_secs(2);

/// The time between two updates of a held button.
const UPDATE_EVERY: Duration = Duration::from_millis(100);

/// The updates of a held button.
const HOLD_UPDATES: u16 = 10;

/// The updates of a drag.
const DRAG_UPDATES: u16 = 5;

/// The updates of a drag of a physical prim: three seconds of them.
const PHYSICAL_DRAG_UPDATES: u16 = 30;

/// How far a drag asks a prim to go, in metres.
const DRAG_M: f32 = 1.0;

/// How far a drag asks a physical prim to go, in metres.
const PULL_M: f32 = 0.5;

/// How far a prim has to have gone to have moved at all, in metres.
const MOVED_M: f32 = 0.05;

/// How far a physical prim has to have gone to have been pulled, in metres.
const PULLED_M: f32 = 0.01;

/// The grab offset a press states.
const GRAB_OFFSET: Vector = Vector {
    x: 0.11,
    y: 0.22,
    z: 0.33,
};

/// Where on a prim the first surface is, from its centre.
const FIRST_SPOT: Vector = Vector {
    x: 0.1,
    y: 0.2,
    z: 0.3,
};

/// Where on a prim the second surface is, from its centre.
const SECOND_SPOT: Vector = Vector {
    x: 0.3,
    y: 0.2,
    z: 0.1,
};

/// Where the measurements below are written down.
const SOURCE: &str =
    "book/src/gridspec/building.md § Touch and grab (object-touch-grab, 2026-10-12)";

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

/// What each grid was measured to answer to a prim with no script, by the
/// metric the census records it under: every run, live or fake, is held to
/// it.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("plain_click_told_other", both(Answer::Text("nothing"))),
    ("plain_click_told_toucher", both(Answer::Text("nothing"))),
    (
        "plain_owner_drag_moves_the_prim",
        both(Answer::Text("to where the grab is")),
    ),
    ("plain_owner_drag_told_other", both(Answer::Text("terse"))),
    ("plain_owner_drag_told_toucher", both(Answer::Text("terse"))),
    (
        "plain_owner_drag_with_no_grab_moves_the_prim",
        both(Answer::Text("to where the grab is")),
    ),
    (
        "plain_owner_drag_with_no_grab_told_other",
        both(Answer::Text("terse")),
    ),
    (
        "plain_owner_drag_with_no_grab_told_toucher",
        both(Answer::Text("terse")),
    ),
    (
        "plain_owner_second_spin_leaves_the_prim",
        both(Answer::Text("as the last update turned it")),
    ),
    (
        "plain_owner_spin_leaves_the_prim",
        both(Answer::Text("as the last update turned it")),
    ),
    ("plain_owner_spin_told_other", both(Answer::Text("terse"))),
    ("plain_owner_spin_told_toucher", both(Answer::Text("terse"))),
    (
        "plain_stranger_drag_moves_the_prim",
        both(Answer::Text("not at all")),
    ),
    (
        "plain_stranger_drag_told_other",
        each(Answer::Text("nothing"), Answer::Text("terse")),
    ),
    (
        "plain_stranger_drag_told_toucher",
        each(Answer::Text("nothing"), Answer::Text("terse")),
    ),
    (
        "plain_stranger_spin_leaves_the_prim",
        both(Answer::Text("as it was")),
    ),
    (
        "plain_stranger_spin_told_toucher",
        each(Answer::Text("nothing"), Answer::Text("terse")),
    ),
];

/// What each live grid was measured to answer where a script or a physics
/// engine is needed to ask.
const MEASURED_LIVE: &[(&str, Measured<Answer>)] = &[
    (
        "bare_child_passing_always_child_click_tells",
        both(Answer::Text("root (link 2)")),
    ),
    (
        "bare_child_passing_if_not_handled_child_click_tells",
        both(Answer::Text("root (link 2)")),
    ),
    (
        "bare_child_passing_never_child_click_tells",
        each(Answer::Text("nobody"), Answer::Text("root (link 2)")),
    ),
    ("bare_script_click_events", both(Answer::Text("none"))),
    ("bare_script_flags", both(Answer::Text("scripted"))),
    (
        "bare_script_flags_for_the_other",
        both(Answer::Text("scripted")),
    ),
    ("bare_script_told_other", both(Answer::Text("full"))),
    ("bare_script_told_owner", both(Answer::Text("full"))),
    (
        "bare_scripted_owner_drag_moves_the_prim",
        both(Answer::Text("to where the grab is")),
    ),
    (
        "bare_scripted_owner_drag_told_other",
        both(Answer::Text("terse")),
    ),
    (
        "bare_scripted_owner_drag_told_toucher",
        both(Answer::Text("terse")),
    ),
    (
        "both_scripts_child_click_tells",
        both(Answer::Text("child (link 2)")),
    ),
    (
        "both_scripts_root_click_tells",
        both(Answer::Text("root (link 1)")),
    ),
    (
        "child_passing_always_child_click_tells",
        both(Answer::Text("child (link 2)+root (link 2)")),
    ),
    (
        "child_passing_if_not_handled_child_click_tells",
        both(Answer::Text("child (link 2)")),
    ),
    (
        "child_passing_never_child_click_tells",
        each(
            Answer::Text("child (link 2)"),
            Answer::Text("child (link 2)+root (link 2)"),
        ),
    ),
    (
        "click_action_disabled_click_events",
        each(Answer::Text("start,touch,end"), Answer::Text("start,end")),
    ),
    (
        "click_action_disabled_reads_back_as",
        both(Answer::Count(8)),
    ),
    (
        "click_action_disabled_shown_to_the_other_as",
        each(Answer::Count(8), Answer::Count(-1)),
    ),
    (
        "click_action_disabled_told_other",
        each(Answer::Text("full"), Answer::Text("nothing")),
    ),
    (
        "click_action_disabled_told_owner",
        each(Answer::Text("full"), Answer::Text("nothing")),
    ),
    (
        "click_action_sit_click_events",
        each(Answer::Text("start,touch,end"), Answer::Text("start,end")),
    ),
    ("click_action_sit_reads_back_as", both(Answer::Count(1))),
    (
        "click_action_sit_shown_to_the_other_as",
        each(Answer::Count(1), Answer::Count(-1)),
    ),
    (
        "click_action_sit_told_other",
        each(Answer::Text("full"), Answer::Text("nothing")),
    ),
    (
        "click_action_sit_told_owner",
        each(Answer::Text("full"), Answer::Text("nothing")),
    ),
    ("click_end_grab", both(Answer::Text("zero"))),
    (
        "click_end_surface",
        both(Answer::Text(
            "face -1, st <-1.00,-1.00,0.00>, uv <-1.00,-1.00,0.00>, pos zero, normal <0.00,0.00,0.00>, binormal <0.00,0.00,0.00>",
        )),
    ),
    (
        "click_end_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "click_events",
        each(Answer::Text("start,touch,end"), Answer::Text("start,end")),
    ),
    (
        "click_start_grab",
        each(Answer::Text("zero"), Answer::Text("<0.11,0.22,0.33>")),
    ),
    (
        "click_start_surface",
        both(Answer::Text(
            "face -1, st <-1.00,-1.00,0.00>, uv <-1.00,-1.00,0.00>, pos zero, normal <0.00,0.00,0.00>, binormal <0.00,0.00,0.00>",
        )),
    ),
    (
        "click_start_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    ("click_told_other", both(Answer::Text("nothing"))),
    ("click_told_toucher", both(Answer::Text("nothing"))),
    ("click_touches_handed", both(Answer::Count(1))),
    ("click_with_a_surface_end_grab", both(Answer::Text("zero"))),
    (
        "click_with_a_surface_end_surface",
        both(Answer::Text(
            "face 3, st <0.40,0.60,0.00>, uv <0.25,0.75,0.00>, pos prim+<0.10,0.20,0.30>, normal <0.00,0.00,1.00>, binormal <0.00,1.00,0.00>",
        )),
    ),
    (
        "click_with_a_surface_end_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "click_with_a_surface_events",
        each(Answer::Text("start,touch,end"), Answer::Text("start,end")),
    ),
    (
        "click_with_a_surface_start_grab",
        each(Answer::Text("zero"), Answer::Text("<0.11,0.22,0.33>")),
    ),
    (
        "click_with_a_surface_start_surface",
        both(Answer::Text(
            "face 3, st <0.40,0.60,0.00>, uv <0.25,0.75,0.00>, pos prim+<0.10,0.20,0.30>, normal <0.00,0.00,1.00>, binormal <0.00,1.00,0.00>",
        )),
    ),
    (
        "click_with_a_surface_start_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "click_with_a_surface_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "click_with_a_surface_told_toucher",
        both(Answer::Text("nothing")),
    ),
    (
        "click_with_a_surface_touches_handed",
        both(Answer::Count(1)),
    ),
    (
        "click_with_no_such_surface_end_grab",
        both(Answer::Text("zero")),
    ),
    (
        "click_with_no_such_surface_end_surface",
        both(Answer::Text(
            "face 99, st <2.50,-1.50,0.00>, uv <1.50,-0.50,0.00>, pos prim+<0.00,0.00,5.00>, normal <0.00,0.00,3.00>, binormal <0.00,0.00,0.00>",
        )),
    ),
    (
        "click_with_no_such_surface_end_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "click_with_no_such_surface_events",
        each(Answer::Text("start,touch,end"), Answer::Text("start,end")),
    ),
    (
        "click_with_no_such_surface_start_grab",
        each(Answer::Text("zero"), Answer::Text("<0.11,0.22,0.33>")),
    ),
    (
        "click_with_no_such_surface_start_surface",
        both(Answer::Text(
            "face 99, st <2.50,-1.50,0.00>, uv <1.50,-0.50,0.00>, pos prim+<0.00,0.00,5.00>, normal <0.00,0.00,3.00>, binormal <0.00,0.00,0.00>",
        )),
    ),
    (
        "click_with_no_such_surface_start_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "click_with_no_such_surface_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "click_with_no_such_surface_told_toucher",
        both(Answer::Text("nothing")),
    ),
    (
        "click_with_no_such_surface_touches_handed",
        both(Answer::Count(1)),
    ),
    ("hold_end_grab", both(Answer::Text("zero"))),
    (
        "hold_end_surface",
        both(Answer::Text(
            "face -1, st <-1.00,-1.00,0.00>, uv <-1.00,-1.00,0.00>, pos zero, normal <0.00,0.00,0.00>, binormal <0.00,0.00,0.00>",
        )),
    ),
    (
        "hold_end_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "hold_events",
        each(Answer::Text("start,touch,end"), Answer::Text("start,end")),
    ),
    (
        "hold_start_grab",
        each(Answer::Text("zero"), Answer::Text("<0.11,0.22,0.33>")),
    ),
    (
        "hold_start_surface",
        both(Answer::Text(
            "face 3, st <0.40,0.60,0.00>, uv <0.25,0.75,0.00>, pos prim+<0.10,0.20,0.30>, normal <0.00,0.00,1.00>, binormal <0.00,1.00,0.00>",
        )),
    ),
    (
        "hold_start_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    ("hold_told_other", both(Answer::Text("nothing"))),
    ("hold_told_toucher", both(Answer::Text("nothing"))),
    (
        "hold_touch_events_come",
        each(
            Answer::Text("on the region's own clock"),
            Answer::Text("never"),
        ),
    ),
    ("hold_touches_handed", both(Answer::Count(1))),
    (
        "hold_with_bare_updates_end_grab",
        both(Answer::Text("zero")),
    ),
    (
        "hold_with_bare_updates_end_surface",
        both(Answer::Text(
            "face -1, st <-1.00,-1.00,0.00>, uv <-1.00,-1.00,0.00>, pos zero, normal <0.00,0.00,0.00>, binormal <0.00,0.00,0.00>",
        )),
    ),
    (
        "hold_with_bare_updates_end_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "hold_with_bare_updates_events",
        both(Answer::Text("start,touch,end")),
    ),
    (
        "hold_with_bare_updates_last_touch_grab",
        each(
            Answer::Text("<0.39,-0.22,-0.33>"),
            Answer::Text("<0.50,0.00,0.00>"),
        ),
    ),
    (
        "hold_with_bare_updates_last_touch_surface",
        both(Answer::Text(
            "face -1, st <-1.00,-1.00,0.00>, uv <-1.00,-1.00,0.00>, pos zero, normal <0.00,0.00,0.00>, binormal <0.00,0.00,0.00>",
        )),
    ),
    (
        "hold_with_bare_updates_last_touch_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "hold_with_bare_updates_start_grab",
        each(Answer::Text("zero"), Answer::Text("<0.11,0.22,0.33>")),
    ),
    (
        "hold_with_bare_updates_start_surface",
        both(Answer::Text(
            "face 3, st <0.40,0.60,0.00>, uv <0.25,0.75,0.00>, pos prim+<0.10,0.20,0.30>, normal <0.00,0.00,1.00>, binormal <0.00,1.00,0.00>",
        )),
    ),
    (
        "hold_with_bare_updates_start_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "hold_with_bare_updates_told_other",
        each(Answer::Text("nothing"), Answer::Text("terse")),
    ),
    (
        "hold_with_bare_updates_told_toucher",
        each(Answer::Text("nothing"), Answer::Text("terse")),
    ),
    (
        "hold_with_bare_updates_touch_events_come",
        each(
            Answer::Text("on the region's own clock"),
            Answer::Text("one an update"),
        ),
    ),
    (
        "hold_with_bare_updates_touches_handed",
        both(Answer::Count(1)),
    ),
    ("hold_with_updates_end_grab", both(Answer::Text("zero"))),
    (
        "hold_with_updates_end_surface",
        both(Answer::Text(
            "face 3, st <0.40,0.60,0.00>, uv <0.25,0.75,0.00>, pos prim+<0.10,0.20,0.30>, normal <0.00,0.00,1.00>, binormal <0.00,1.00,0.00>",
        )),
    ),
    (
        "hold_with_updates_end_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "hold_with_updates_events",
        both(Answer::Text("start,touch,end")),
    ),
    (
        "hold_with_updates_last_touch_grab",
        each(
            Answer::Text("<0.39,-0.22,-0.33>"),
            Answer::Text("<0.50,0.00,0.00>"),
        ),
    ),
    (
        "hold_with_updates_last_touch_surface",
        both(Answer::Text(
            "face 2, st <0.30,0.70,0.00>, uv <0.12,0.88,0.00>, pos prim+<0.30,0.20,0.10>, normal <1.00,0.00,0.00>, binormal <0.00,0.00,1.00>",
        )),
    ),
    (
        "hold_with_updates_last_touch_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "hold_with_updates_start_grab",
        each(Answer::Text("zero"), Answer::Text("<0.11,0.22,0.33>")),
    ),
    (
        "hold_with_updates_start_surface",
        both(Answer::Text(
            "face 3, st <0.40,0.60,0.00>, uv <0.25,0.75,0.00>, pos prim+<0.10,0.20,0.30>, normal <0.00,0.00,1.00>, binormal <0.00,1.00,0.00>",
        )),
    ),
    (
        "hold_with_updates_start_who",
        each(
            Answer::Text("the owner, type 3, link 0"),
            Answer::Text("the owner, type 1, link 0"),
        ),
    ),
    (
        "hold_with_updates_told_other",
        each(Answer::Text("nothing"), Answer::Text("terse")),
    ),
    (
        "hold_with_updates_told_toucher",
        each(Answer::Text("nothing"), Answer::Text("terse")),
    ),
    (
        "hold_with_updates_touch_events_come",
        each(
            Answer::Text("on the region's own clock"),
            Answer::Text("one an update"),
        ),
    ),
    ("hold_with_updates_touches_handed", both(Answer::Count(1))),
    (
        "linkset_child_script_flags",
        both(Answer::Text("scripted+touch")),
    ),
    (
        "linkset_child_script_flags_for_the_other",
        both(Answer::Text("scripted+touch")),
    ),
    (
        "linkset_child_script_told_other",
        both(Answer::Text("full")),
    ),
    (
        "linkset_child_script_told_owner",
        both(Answer::Text("full")),
    ),
    (
        "linkset_root_script_child_flags",
        each(Answer::Text("scripted+touch"), Answer::Text("scripted")),
    ),
    (
        "linkset_root_script_flags",
        both(Answer::Text("scripted+touch")),
    ),
    (
        "linkset_root_script_flags_for_the_other",
        both(Answer::Text("scripted+touch")),
    ),
    (
        "linkset_root_script_tells_of_the_child",
        each(Answer::Text("nothing"), Answer::Text("full")),
    ),
    ("linkset_root_script_told_other", both(Answer::Text("full"))),
    ("linkset_root_script_told_owner", both(Answer::Text("full"))),
    (
        "physical_owner_drag_leaves_the_prim",
        each(
            Answer::Text("pulled along"),
            Answer::Text("left where it was"),
        ),
    ),
    (
        "physical_owner_spin_leaves_the_prim",
        both(Answer::Text("turned some other way")),
    ),
    ("physical_prim_is_physical", both(Answer::Flag(true))),
    (
        "physical_stranger_drag_leaves_the_prim",
        both(Answer::Text("left where it was")),
    ),
    ("probe_says_it_started", both(Answer::Flag(true))),
    ("release_with_no_press_events", both(Answer::Text("end"))),
    (
        "root_script_alone_child_click_tells",
        both(Answer::Text("root (link 2)")),
    ),
    (
        "root_script_alone_root_click_tells",
        both(Answer::Text("root (link 1)")),
    ),
    ("script_removed_flags", both(Answer::Text("neither"))),
    (
        "script_removed_flags_for_the_other",
        both(Answer::Text("neither")),
    ),
    ("script_removed_told_other", both(Answer::Text("full"))),
    ("script_removed_told_owner", both(Answer::Text("full"))),
    (
        "scripted_owner_drag_events",
        both(Answer::Text("start,touch,end")),
    ),
    (
        "scripted_owner_drag_moves_the_prim",
        both(Answer::Text("not at all")),
    ),
    (
        "scripted_owner_drag_told_other",
        each(Answer::Text("nothing"), Answer::Text("terse")),
    ),
    (
        "scripted_owner_drag_told_toucher",
        each(Answer::Text("nothing"), Answer::Text("terse")),
    ),
    ("stranger_click_end_grab", both(Answer::Text("zero"))),
    (
        "stranger_click_end_surface",
        both(Answer::Text(
            "face 3, st <0.40,0.60,0.00>, uv <0.25,0.75,0.00>, pos prim+<0.10,0.20,0.30>, normal <0.00,0.00,1.00>, binormal <0.00,1.00,0.00>",
        )),
    ),
    (
        "stranger_click_end_who",
        each(
            Answer::Text("the other avatar, type 3, link 0"),
            Answer::Text("the other avatar, type 1, link 0"),
        ),
    ),
    (
        "stranger_click_events",
        each(Answer::Text("start,touch,end"), Answer::Text("start,end")),
    ),
    (
        "stranger_click_start_grab",
        each(Answer::Text("zero"), Answer::Text("<0.11,0.22,0.33>")),
    ),
    (
        "stranger_click_start_surface",
        both(Answer::Text(
            "face 3, st <0.40,0.60,0.00>, uv <0.25,0.75,0.00>, pos prim+<0.10,0.20,0.30>, normal <0.00,0.00,1.00>, binormal <0.00,1.00,0.00>",
        )),
    ),
    (
        "stranger_click_start_who",
        each(
            Answer::Text("the other avatar, type 3, link 0"),
            Answer::Text("the other avatar, type 1, link 0"),
        ),
    ),
    ("stranger_click_told_other", both(Answer::Text("nothing"))),
    ("stranger_click_told_toucher", both(Answer::Text("nothing"))),
    ("stranger_click_touches_handed", both(Answer::Count(1))),
    ("touch_script_flags", both(Answer::Text("scripted+touch"))),
    (
        "touch_script_flags_for_the_other",
        both(Answer::Text("scripted+touch")),
    ),
    ("touch_script_told_other", both(Answer::Text("full"))),
    ("touch_script_told_owner", both(Answer::Text("full"))),
    (
        "two_presses_and_a_release_events",
        each(
            Answer::Text("start,touch,end"),
            Answer::Text("start,start,end"),
        ),
    ),
    ("unscripted_flags", both(Answer::Text("neither"))),
    (
        "updates_with_no_press_events",
        each(Answer::Text("start,touch"), Answer::Text("touch")),
    ),
];

/// The probe: a script that says every touch event it is handed to its
/// owner, with what it detects. `handlers` leaves the three touch handlers
/// in or out, and `pass` is what it sets `llPassTouches` to as it starts —
/// or leaves it alone.
fn probe_source(handlers: bool, pass: Option<i32>) -> String {
    let pass_call = pass.map_or_else(String::new, |pass| format!("llPassTouches({pass});"));
    let pass_word = pass.map_or_else(|| "unset".to_owned(), |pass| pass.to_string());
    let touch_handlers = if handlers {
        r#"
    touch_start(integer n)
    {
        held = 0;
        last = "";
        llResetTime();
        say("S", n, block());
    }
    touch(integer n)
    {
        ++held;
        last = block();
        if (held == 1) say("F", n, last);
    }
    touch_end(integer n)
    {
        if (held > 0) say("L", n, last);
        say("E", n, block());
    }"#
    } else {
        ""
    };
    format!(
        r#"integer held;
string last;
string block()
{{
    return (string)llDetectedLinkNumber(0) + "|" + (string)llDetectedTouchFace(0)
        + "|" + (string)llDetectedTouchST(0) + "|" + (string)llDetectedTouchUV(0)
        + "|" + (string)llDetectedTouchPos(0) + "|" + (string)llDetectedTouchNormal(0)
        + "|" + (string)llDetectedTouchBinormal(0) + "|" + (string)llDetectedGrab(0)
        + "|" + (string)llDetectedKey(0) + "|" + (string)llDetectedType(0);
}}
say(string what, integer n, string detected)
{{
    llOwnerSay("{PROBE_TAG}|" + (string)llGetLinkNumber() + "|" + what + "|" + (string)n
        + "|" + (string)held + "|" + (string)llGetTime() + "|" + detected);
}}
default
{{
    state_entry()
    {{
        {pass_call}
        llOwnerSay("{PROBE_TAG}|" + (string)llGetLinkNumber() + "|R|{pass_word}");
    }}{touch_handlers}
}}
"#
    )
}

/// What a script detected of a touch.
#[derive(Debug, Clone, PartialEq)]
struct Detected {
    /// `llDetectedLinkNumber`.
    link: i32,
    /// `llDetectedTouchFace`.
    face: i32,
    /// `llDetectedTouchST`.
    st: [f32; 3],
    /// `llDetectedTouchUV`.
    uv: [f32; 3],
    /// `llDetectedTouchPos`.
    position: [f32; 3],
    /// `llDetectedTouchNormal`.
    normal: [f32; 3],
    /// `llDetectedTouchBinormal`.
    binormal: [f32; 3],
    /// `llDetectedGrab`.
    grab: [f32; 3],
    /// `llDetectedKey`.
    key: String,
    /// `llDetectedType`.
    kind: i32,
}

/// One line the probe said.
#[derive(Debug, Clone, PartialEq)]
struct Line {
    /// The link number of the prim the script is in: 0 for a prim on its
    /// own, 1 for a root.
    script: i32,
    /// `R` as the script starts, `S` for a `touch_start`, `F` for the first
    /// `touch`, `L` for the last one (said at the `touch_end`) and `E` for a
    /// `touch_end`.
    event: char,
    /// How many touches the event was handed.
    touches: i32,
    /// How many `touch` events the script has had since the `touch_start`.
    held: i32,
    /// The script's seconds since the `touch_start`.
    time: f32,
    /// What it detected.
    detected: Option<Detected>,
}

/// An LSL vector, `<x, y, z>`, as three numbers.
fn parse_vector(text: &str) -> Option<[f32; 3]> {
    let inner = text.trim().strip_prefix('<')?.strip_suffix('>')?;
    let mut parts = inner.split(',').map(|part| part.trim().parse::<f32>());
    let vector = [
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    ];
    parts.next().is_none().then_some(vector)
}

/// A line of chat as a line of the probe's, if it is one.
fn parse_line(text: &str) -> Option<Line> {
    let mut fields = text.split('|');
    if fields.next()? != PROBE_TAG {
        return None;
    }
    let script = fields.next()?.parse().ok()?;
    let event = fields.next()?.chars().next()?;
    if event == 'R' {
        return Some(Line {
            script,
            event,
            touches: 0,
            held: 0,
            time: 0.0,
            detected: None,
        });
    }
    let touches = fields.next()?.parse().ok()?;
    let held = fields.next()?.parse().ok()?;
    let time = fields.next()?.parse().ok()?;
    let detected = Detected {
        link: fields.next()?.parse().ok()?,
        face: fields.next()?.parse().ok()?,
        st: parse_vector(fields.next()?)?,
        uv: parse_vector(fields.next()?)?,
        position: parse_vector(fields.next()?)?,
        normal: parse_vector(fields.next()?)?,
        binormal: parse_vector(fields.next()?)?,
        grab: parse_vector(fields.next()?)?,
        key: fields.next()?.to_owned(),
        kind: fields.next()?.parse().ok()?,
    };
    Some(Line {
        script,
        event,
        touches,
        held,
        time,
        detected: Some(detected),
    })
}

/// The probe's lines among the chat a session heard.
fn lines_of(heard: &Heard) -> Vec<Line> {
    heard
        .chats
        .iter()
        .filter_map(|chat| parse_line(chat))
        .collect()
}

/// A number to two places, with no negative zero.
fn two_places(value: f32) -> String {
    let text = format!("{value:.2}");
    if text == "-0.00" {
        "0.00".to_owned()
    } else {
        text
    }
}

/// Three numbers as `<x,y,z>`, each to two places.
fn vector_text(vector: [f32; 3]) -> String {
    let [x, y, z] = vector;
    format!("<{},{},{}>", two_places(x), two_places(y), two_places(z))
}

/// A vector as three numbers.
const fn components(vector: &Vector) -> [f32; 3] {
    [vector.x, vector.y, vector.z]
}

/// The length of the difference of two points.
fn distance(left: [f32; 3], right: [f32; 3]) -> f32 {
    let [lx, ly, lz] = left;
    let [rx, ry, rz] = right;
    let (dx, dy, dz) = (lx - rx, ly - ry, lz - rz);
    dz.mul_add(dz, dx.mul_add(dx, dy * dy)).sqrt()
}

/// A point as a metric states it: `zero`, an offset from the prim at `prim`
/// where it is near it, and the point itself otherwise.
fn point_text(point: [f32; 3], prim: &Vector) -> String {
    let origin = components(prim);
    if distance(point, [0.0, 0.0, 0.0]) < 0.005 {
        "zero".to_owned()
    } else if distance(point, origin) < 10.0 {
        let [px, py, pz] = point;
        let [ox, oy, oz] = origin;
        format!("prim+{}", vector_text([px - ox, py - oy, pz - oz]))
    } else {
        vector_text(point)
    }
}

impl Detected {
    /// The surface a script read, as a metric states it.
    fn surface(&self, prim: &Vector) -> String {
        format!(
            "face {}, st {}, uv {}, pos {}, normal {}, binormal {}",
            self.face,
            vector_text(self.st),
            vector_text(self.uv),
            point_text(self.position, prim),
            vector_text(self.normal),
            vector_text(self.binormal),
        )
    }

    /// Whom a script was told of, as a metric states it.
    fn who(&self, owner: AgentKey, stranger: AgentKey) -> String {
        let key = if self.key == owner.uuid().to_string() {
            "the owner"
        } else if self.key == stranger.uuid().to_string() {
            "the other avatar"
        } else if self.key == Uuid::nil().to_string() {
            "nobody"
        } else {
            "somebody else"
        };
        format!("{key}, type {}, link {}", self.kind, self.link)
    }
}

/// The surface data of a touch at `spot` from the centre of a prim at
/// `prim`.
fn surface_at(prim: &Vector, spot: &Vector, face_index: i32) -> SurfaceInfo {
    let first = face_index == 3;
    SurfaceInfo {
        uv: if first { [0.25, 0.75] } else { [0.125, 0.875] },
        st: if first { [0.4, 0.6] } else { [0.3, 0.7] },
        face_index,
        position: Vector {
            x: prim.x + spot.x,
            y: prim.y + spot.y,
            z: prim.z + spot.z,
        },
        normal: if first {
            Vector {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            }
        } else {
            Vector {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            }
        },
        binormal: if first {
            Vector {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            }
        } else {
            Vector {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            }
        },
    }
}

/// Surface data no prim has: a face a cube lacks, and coordinates off it.
fn surface_of_nothing(prim: &Vector) -> SurfaceInfo {
    SurfaceInfo {
        uv: [1.5, -0.5],
        st: [2.5, -1.5],
        face_index: 99,
        position: Vector {
            x: prim.x,
            y: prim.y,
            z: prim.z + 5.0,
        },
        normal: Vector {
            x: 0.0,
            y: 0.0,
            z: 3.0,
        },
        binormal: Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    }
}

/// Which of the two avatars acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum By {
    /// The cube's owner.
    Owner,
    /// The other avatar.
    Stranger,
}

/// One step of a gesture.
#[derive(Debug)]
enum Step {
    /// A message.
    Send(Command),
    /// A pause.
    Wait(Duration),
}

/// A cube, as both avatars know it.
#[derive(Debug, Clone)]
struct Prim {
    /// The cube as its owner last heard of it.
    object: Object,
    /// The id the other avatar knows it by.
    theirs: ScopedObjectId,
}

impl Prim {
    /// The id `by` knows the cube by.
    const fn id(&self, by: By) -> ScopedObjectId {
        match by {
            By::Owner => self.object.scoped_id(),
            By::Stranger => self.theirs,
        }
    }

    /// Where the cube is.
    const fn position(&self) -> &Vector {
        &self.object.motion.position
    }

    /// A press on the cube by `by`.
    const fn grab(&self, by: By, surface: Option<SurfaceInfo>) -> Step {
        Step::Send(Command::GrabObject {
            local_id: self.id(by),
            grab_offset: GRAB_OFFSET,
            surface,
        })
    }

    /// An update of a held button, with the grab `offset` from where the
    /// cube is.
    fn update(&self, offset: &Vector, surface: Option<SurfaceInfo>) -> Step {
        let position = self.position();
        Step::Send(Command::GrabObjectUpdate {
            object_id: self.object.full_id,
            grab_offset_initial: GRAB_OFFSET,
            grab_position: Vector {
                x: position.x + offset.x,
                y: position.y + offset.y,
                z: position.z + offset.z,
            },
            time_since_last: 100,
            surface,
        })
    }

    /// A release of the cube by `by`.
    const fn degrab(&self, by: By, surface: Option<SurfaceInfo>) -> Step {
        Step::Send(Command::DegrabObject {
            local_id: self.id(by),
            surface,
        })
    }
}

/// What a gesture brought.
struct Touched {
    /// What the avatar that made it was sent.
    to_toucher: Heard,
    /// What the other one was.
    to_other: Heard,
    /// What the probe said.
    lines: Vec<Line>,
}

impl Touched {
    /// Every update of `prim` either avatar was sent, the owner's first.
    fn sightings(&self, prim: &Prim) -> Vec<&Object> {
        self.to_toucher
            .changed
            .iter()
            .chain(&self.to_other.changed)
            .filter(|object| object.full_id == prim.object.full_id)
            .collect()
    }

    /// The first line of `event`.
    fn line(&self, event: char) -> Option<&Line> {
        self.lines.iter().find(|line| line.event == event)
    }

    /// The touch events in the order the scripts said them: `start` and
    /// `end`, with `touch` between them where the script had any.
    fn events(&self) -> String {
        let mut events = Vec::new();
        for line in &self.lines {
            match line.event {
                'S' => events.push("start"),
                'F' => events.push("touch"),
                'E' => events.push("end"),
                _ => {}
            }
        }
        if events.is_empty() {
            "none".to_owned()
        } else {
            events.join(",")
        }
    }

    /// Which scripts of a linkset were handed the `touch_start`, and the
    /// link each was told of.
    fn told_scripts(&self) -> String {
        let mut scripts: Vec<String> = self
            .lines
            .iter()
            .filter(|line| line.event == 'S')
            .map(|line| {
                let script = match line.script {
                    1 => "root",
                    2 => "child",
                    _ => "another prim",
                };
                let link = line.detected.as_ref().map_or(-1, |detected| detected.link);
                format!("{script} (link {link})")
            })
            .collect();
        scripts.sort_unstable();
        if scripts.is_empty() {
            "nobody".to_owned()
        } else {
            scripts.join("+")
        }
    }
}

/// The forms `heard` was sent the object `local_id` in, `+`-joined, or
/// `nothing`.
fn forms(heard: &Heard, local_id: RegionLocalObjectId) -> String {
    let mut seen: Vec<String> = Vec::new();
    for (id, form) in &heard.updated {
        let word = format!("{form:?}").to_lowercase();
        if *id == local_id && !seen.contains(&word) {
            seen.push(word);
        }
    }
    if seen.is_empty() {
        "nothing".to_owned()
    } else {
        seen.join("+")
    }
}

/// The script and touch flags of an object update, as a metric states them.
const fn flags_text(flags: u32) -> &'static str {
    match (flags & FLAGS_SCRIPTED != 0, flags & FLAGS_HANDLE_TOUCH != 0) {
        (true, true) => "scripted+touch",
        (true, false) => "scripted",
        (false, true) => "touch",
        (false, false) => "neither",
    }
}

/// Whether two rotations turn an object the same way.
fn same_rotation(left: &Rotation, right: &Rotation) -> bool {
    let dot = left.s.mul_add(
        right.s,
        left.x
            .mul_add(right.x, left.y.mul_add(right.y, left.z * right.z)),
    );
    dot.abs() > 0.999
}

/// A census of touching and grabbing.
#[derive(Debug)]
pub struct ObjectTouchGrab;

impl GridTest for ObjectTouchGrab {
    fn name(&self) -> &'static str {
        "object-touch-grab"
    }

    fn description(&self) -> &'static str {
        "What a script is told of a click, a held button and a drag, which prim of a linkset is \
         told, and what a drag does to the prim"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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
        Duration::from_secs(1200)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let folders = login_folders(ctx.primary()).await?;
            let mut residue = Residue::default();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, folders, &mut residue, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            // Take the cubes away whatever became of the census.
            let cleanup = clean_up(ctx.primary(), &residue, folders.trash).await;
            outcome
                .and(cleanup)
                .and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
                .and_then(|()| {
                    if grid.is_fake() {
                        Ok(())
                    } else {
                        hold_to_measured(grid, &metrics, MEASURED_LIVE, SOURCE)
                    }
                })
        })
    }
}

/// Everything the legs share.
struct Census<'a> {
    /// The avatar that owns the cubes.
    owner: &'a mut Session,
    /// The other avatar.
    stranger: &'a mut Session,
    /// The owner.
    owner_id: AgentKey,
    /// The other avatar.
    stranger_id: AgentKey,
    /// The run.
    run: Run,
    /// How long the queue of the avatar that was not waited on is read for.
    drain: Duration,
}

impl Census<'_> {
    /// Rezzes a cube at `slot` and waits for the other avatar to be sent it.
    async fn rez(&mut self, residue: &mut Residue, slot: u8) -> Result<Prim, TestFailure> {
        let object = rez_cube(self.owner, &mut self.run, residue, slot).await?;
        let theirs = find_object(self.stranger, object.full_id).await?;
        listen(self.stranger, self.drain).await?;
        Ok(Prim { object, theirs })
    }

    /// Has `by` make the gesture `steps` and returns what the two avatars
    /// were sent and what the probe said of it.
    async fn gesture(&mut self, by: By, steps: Vec<Step>) -> Result<Touched, TestFailure> {
        let quiet = self.run.pace.quiet;
        let (toucher, other) = match by {
            By::Owner => (&mut *self.owner, &mut *self.stranger),
            By::Stranger => (&mut *self.stranger, &mut *self.owner),
        };
        for step in steps {
            match step {
                Step::Send(command) => toucher.send(command).await?,
                Step::Wait(pause) => tokio::time::sleep(pause).await,
            }
        }
        let to_toucher = listen(toucher, quiet).await?;
        let to_other = listen(other, self.drain).await?;
        // The probe speaks to its owner alone.
        let lines = match by {
            By::Owner => lines_of(&to_toucher),
            By::Stranger => lines_of(&to_other),
        };
        Ok(Touched {
            to_toucher,
            to_other,
            lines,
        })
    }

    /// A press and a release at once.
    async fn click(
        &mut self,
        by: By,
        prim: &Prim,
        surface: Option<SurfaceInfo>,
    ) -> Result<Touched, TestFailure> {
        self.gesture(
            by,
            vec![prim.grab(by, surface.clone()), prim.degrab(by, surface)],
        )
        .await
    }

    /// A press, `updates` updates asking the cube `offset` away from where
    /// it is, and a release; `grab` leaves the press and the release out.
    /// Brings `prim` up to where the grid then says it is.
    async fn drag(
        &mut self,
        by: By,
        prim: &mut Prim,
        offset: &Vector,
        updates: u16,
        grab: bool,
    ) -> Result<Dragged, TestFailure> {
        let before = prim.object.clone();
        let mut steps = Vec::new();
        if grab {
            steps.push(prim.grab(by, None));
        }
        for _ in 0..updates {
            steps.push(prim.update(offset, None));
            steps.push(Step::Wait(UPDATE_EVERY));
        }
        if grab {
            steps.push(prim.degrab(by, None));
        }
        let touched = self.gesture(by, steps).await?;
        reread(self.owner, &self.run, &mut prim.object).await?;
        let start = components(&before.motion.position);
        let end = components(prim.position());
        let furthest = touched
            .sightings(prim)
            .iter()
            .map(|object| distance(start, components(&object.motion.position)))
            .fold(distance(start, end), f32::max);
        let asked = [
            before.motion.position.x + offset.x,
            before.motion.position.y + offset.y,
            before.motion.position.z + offset.z,
        ];
        Ok(Dragged {
            touched,
            went: distance(start, end),
            short: distance(asked, end),
            furthest,
        })
    }

    /// A spin of the cube: a start, five updates that turn it by steps to a
    /// quarter turn about the vertical — to the left, or to the right — and
    /// a stop. Brings `prim` up to how the grid then says it stands.
    async fn spin(&mut self, by: By, prim: &mut Prim, left: bool) -> Result<Spun, TestFailure> {
        let before = prim.object.motion.rotation.clone();
        let object_id = prim.object.full_id;
        let mut steps = vec![Step::Send(Command::SpinObjectStart { object_id })];
        for step in 1..=DRAG_UPDATES {
            let quarter = if left {
                std::f32::consts::FRAC_PI_4
            } else {
                -std::f32::consts::FRAC_PI_4
            };
            let half_angle = quarter * f32::from(step) / f32::from(DRAG_UPDATES);
            steps.push(Step::Send(Command::SpinObjectUpdate {
                object_id,
                rotation: Rotation {
                    x: 0.0,
                    y: 0.0,
                    z: half_angle.sin(),
                    s: half_angle.cos(),
                },
            }));
            steps.push(Step::Wait(UPDATE_EVERY));
        }
        steps.push(Step::Send(Command::SpinObjectStop { object_id }));
        let touched = self.gesture(by, steps).await?;
        reread(self.owner, &self.run, &mut prim.object).await?;
        let turned = !same_rotation(&before, &prim.object.motion.rotation)
            || touched
                .sightings(prim)
                .iter()
                .any(|object| !same_rotation(&before, &object.motion.rotation));
        let mut asked = quarter_turn();
        if !left {
            asked.z = -asked.z;
        }
        let leaves = if !turned {
            "as it was"
        } else if same_rotation(&prim.object.motion.rotation, &asked) {
            "as the last update turned it"
        } else {
            "turned some other way"
        };
        Ok(Spun { touched, leaves })
    }

    /// Makes a new script in `prim` and saves `source` over it.
    async fn install(&mut self, prim: &Prim, source: &str) -> Result<Installed, TestFailure> {
        let target = prim.object.scoped_id();
        let task = prim.object.full_id;
        self.owner
            .send(Command::RezScript {
                target,
                params: Box::new(RezScriptParams {
                    group_id: None,
                    enabled: true,
                    item: RestoreItem::new_script(self.owner_id, task, PROBE_NAME, Uuid::new_v4()),
                }),
            })
            .await?;
        let started = Instant::now();
        let item = loop {
            listen(self.owner, Duration::from_millis(500)).await?;
            let held = listing(self.owner, target, task).await?;
            if let Some(item) = held.items.iter().find(|item| item.name == PROBE_NAME) {
                break item.item_id;
            }
            if started.elapsed() >= STEP_TIMEOUT {
                return Err(TestFailure::Assertion(
                    "the new script never appeared in the cube".to_owned(),
                ));
            }
        };
        let saved = self.save(prim, item, source).await?;
        Ok(Installed { item, saved })
    }

    /// Saves `source` over the script `item` of `prim`, to run, and listens
    /// until the script has said it started.
    async fn save(
        &mut self,
        prim: &Prim,
        item: InventoryKey,
        source: &str,
    ) -> Result<Touched, TestFailure> {
        self.owner
            .send(Command::UploadScript {
                location: ScriptUploadLocation::TaskInventory {
                    task_id: prim.object.full_id,
                    item_id: item,
                    running: true,
                    experience: None,
                },
                target: ScriptTarget::Mono,
                source: source.as_bytes().to_vec(),
            })
            .await?;
        // The script may speak before the capability's answer is read, so one
        // window hears both.
        let mut heard = Heard::default();
        let mut compiled: Option<Result<bool, String>> = None;
        let started = Instant::now();
        let mut ready_at: Option<Instant> = None;
        loop {
            let ready = compiled.is_some()
                && heard
                    .chats
                    .iter()
                    .filter_map(|chat| parse_line(chat))
                    .any(|line| line.event == 'R');
            if ready && ready_at.is_none() {
                ready_at = Some(Instant::now());
            }
            let remaining = ready_at.map_or_else(
                || STEP_TIMEOUT.saturating_sub(started.elapsed()),
                |at| self.run.pace.quiet.saturating_sub(at.elapsed()),
            );
            if remaining.is_zero() {
                break;
            }
            let outcome = self
                .owner
                .wait_for(remaining, |event| {
                    match event {
                        Event::ScriptUploaded { compiled: took, .. } => compiled = Some(Ok(*took)),
                        Event::AssetUploadFailed { reason } => {
                            compiled = Some(Err(reason.clone()));
                        }
                        Event::ChatReceived(chat) if !chat.message.is_empty() => {
                            heard.chats.push(chat.message.clone());
                        }
                        Event::ObjectUpdated(object) => heard.changed.push((**object).clone()),
                        Event::ObjectStreamBatch(batch) if !batch.child => heard.updated.extend(
                            batch
                                .entries
                                .iter()
                                .filter(|entry| entry.known)
                                .map(|entry| (entry.local_id, batch.form)),
                        ),
                        _ => return None,
                    }
                    Some(())
                })
                .await;
            match outcome {
                Ok(()) => {}
                Err(TestFailure::Timeout(_)) => break,
                Err(other) => return Err(other),
            }
        }
        match compiled {
            Some(Ok(true)) => {}
            Some(Ok(false)) => {
                return Err(TestFailure::Assertion(
                    "the probe script did not compile".to_owned(),
                ));
            }
            Some(Err(reason)) => {
                return Err(TestFailure::Assertion(format!(
                    "saving the probe script failed: {reason}"
                )));
            }
            None => {
                return Err(TestFailure::Assertion(
                    "saving the probe script was never answered".to_owned(),
                ));
            }
        }
        let to_other = listen(self.stranger, self.drain).await?;
        let lines = lines_of(&heard);
        Ok(Touched {
            to_toucher: heard,
            to_other,
            lines,
        })
    }

    /// Records the flags of `prim` as each avatar was last sent them in
    /// `touched`, and the forms the news came in.
    fn note_flags(metrics: &mut Metrics, key: &str, prim: &mut Prim, touched: &Touched) {
        let id = prim.object.full_id;
        let own = touched
            .to_toucher
            .changed
            .iter()
            .rfind(|object| object.full_id == id);
        let theirs = touched
            .to_other
            .changed
            .iter()
            .rfind(|object| object.full_id == id);
        metrics.set(
            &format!("{key}_flags"),
            own.map_or("no update", |object| flags_text(object.update_flags)),
        );
        metrics.set(
            &format!("{key}_flags_for_the_other"),
            theirs.map_or("no update", |object| flags_text(object.update_flags)),
        );
        metrics.set(
            &format!("{key}_told_owner"),
            forms(&touched.to_toucher, prim.object.local_id),
        );
        metrics.set(
            &format!("{key}_told_other"),
            forms(&touched.to_other, prim.object.local_id),
        );
        if let Some(object) = own {
            prim.object = object.clone();
        }
    }

    /// Records what a gesture on `prim` by `by` had its script handed.
    fn note_touch(&self, metrics: &mut Metrics, key: &str, prim: &Prim, touched: &Touched) {
        metrics.set(&format!("{key}_events"), touched.events());
        let position = prim.position();
        for (event, word) in [('S', "start"), ('L', "last_touch"), ('E', "end")] {
            let Some(detected) = touched.line(event).and_then(|line| line.detected.as_ref()) else {
                continue;
            };
            metrics.set(&format!("{key}_{word}_surface"), detected.surface(position));
            metrics.set(
                &format!("{key}_{word}_grab"),
                point_text(detected.grab, position),
            );
            metrics.set(
                &format!("{key}_{word}_who"),
                detected.who(self.owner_id, self.stranger_id),
            );
        }
        if let Some(end) = touched.line('E') {
            metrics.set(&format!("{key}_touch_events"), i64::from(end.held));
            if end.time > 0.5 {
                metrics.set(
                    &format!("{key}_touch_events_a_second"),
                    f64::from(end.held) / f64::from(end.time),
                );
                metrics.set(
                    &format!("{key}_touch_events_come"),
                    if end.held == 0 {
                        "never"
                    } else if end.held == i32::from(HOLD_UPDATES) {
                        "one an update"
                    } else {
                        "on the region's own clock"
                    },
                );
            }
            metrics.set(&format!("{key}_touches_handed"), i64::from(end.touches));
            metrics.set_timing(&format!("{key}_held_secs"), f64::from(end.time));
        }
        metrics.set(
            &format!("{key}_told_toucher"),
            forms(&touched.to_toucher, prim.object.local_id),
        );
        metrics.set(
            &format!("{key}_told_other"),
            forms(&touched.to_other, prim.object.local_id),
        );
    }

    /// Records what a drag of `prim` did to it and who was told.
    fn note_drag(metrics: &mut Metrics, key: &str, prim: &Prim, dragged: &Dragged) {
        metrics.set(&format!("{key}_moves_the_prim"), dragged.moved());
        metrics.set(&format!("{key}_went_m"), f64::from(dragged.went));
        metrics.set(
            &format!("{key}_told_toucher"),
            forms(&dragged.touched.to_toucher, prim.object.local_id),
        );
        metrics.set(
            &format!("{key}_told_other"),
            forms(&dragged.touched.to_other, prim.object.local_id),
        );
        if !dragged.touched.to_toucher.alerts.is_empty() {
            metrics.set(
                &format!("{key}_alerts"),
                dragged.touched.to_toucher.alerted(),
            );
        }
    }
}

/// A script made in a prim.
struct Installed {
    /// The script.
    item: InventoryKey,
    /// What saving its source brought.
    saved: Touched,
}

/// What a drag did.
struct Dragged {
    /// What it brought.
    touched: Touched,
    /// How far the prim is from where it was, in metres.
    went: f32,
    /// How far it is from where the drag asked it to be.
    short: f32,
    /// The furthest from where it was that it was seen.
    furthest: f32,
}

impl Dragged {
    /// What became of the prim, as a metric states it.
    fn moved(&self) -> &'static str {
        if self.short < MOVED_M {
            "to where the grab is"
        } else if self.went < MOVED_M {
            "not at all"
        } else {
            "part of the way"
        }
    }

    /// What became of a physical prim.
    fn pulled(&self) -> &'static str {
        if self.furthest >= PULLED_M {
            "pulled along"
        } else {
            "left where it was"
        }
    }
}

/// What a spin did.
struct Spun {
    /// What it brought.
    touched: Touched,
    /// How the prim stands afterwards.
    leaves: &'static str,
}

/// The legs, in order. Every cube is noted in `residue` as it appears, so
/// the caller can take it away when a leg fails.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of ten legs, each a gesture and a window to hear what it brought"
)]
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let pace = Pace::of(grid);
    let drain = if grid.is_fake() { FAKE_DRAIN } else { DRAIN };
    let Some((owner, stranger)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "object-touch-grab needs a secondary avatar".to_owned(),
        ));
    };
    let owner_id = owner
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let stranger_id = stranger
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

    // Both avatars to the spot, each with its scene settled.
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
    let run = Run {
        folders,
        owner: owner_id,
        pace,
        position: Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
        seen,
    };
    let mut census = Census {
        owner,
        stranger,
        owner_id,
        stranger_id,
        run,
        drain,
    };
    let sideways = Vector {
        x: 0.0,
        y: DRAG_M,
        z: 0.0,
    };

    // --- 1. A prim with no script. ---
    let mut plain = census.rez(residue, 0).await?;
    let clicked = census.click(By::Owner, &plain, None).await?;
    metrics.set(
        "plain_click_told_toucher",
        forms(&clicked.to_toucher, plain.object.local_id),
    );
    metrics.set(
        "plain_click_told_other",
        forms(&clicked.to_other, plain.object.local_id),
    );
    let dragged = census
        .drag(By::Owner, &mut plain, &sideways, DRAG_UPDATES, true)
        .await?;
    Census::note_drag(metrics, "plain_owner_drag", &plain, &dragged);
    let dragged = census
        .drag(By::Owner, &mut plain, &sideways, DRAG_UPDATES, false)
        .await?;
    Census::note_drag(metrics, "plain_owner_drag_with_no_grab", &plain, &dragged);
    let dragged = census
        .drag(By::Stranger, &mut plain, &sideways, DRAG_UPDATES, true)
        .await?;
    Census::note_drag(metrics, "plain_stranger_drag", &plain, &dragged);
    let spun = census.spin(By::Owner, &mut plain, true).await?;
    metrics.set("plain_owner_spin_leaves_the_prim", spun.leaves);
    metrics.set(
        "plain_owner_spin_told_toucher",
        forms(&spun.touched.to_toucher, plain.object.local_id),
    );
    metrics.set(
        "plain_owner_spin_told_other",
        forms(&spun.touched.to_other, plain.object.local_id),
    );
    let spun = census.spin(By::Owner, &mut plain, false).await?;
    metrics.set("plain_owner_second_spin_leaves_the_prim", spun.leaves);
    let spun = census.spin(By::Stranger, &mut plain, true).await?;
    metrics.set("plain_stranger_spin_leaves_the_prim", spun.leaves);
    metrics.set(
        "plain_stranger_spin_told_toucher",
        forms(&spun.touched.to_toucher, plain.object.local_id),
    );
    if grid.is_fake() {
        // Nothing runs a script here, and nothing falls.
        return Ok(());
    }

    // --- 2. The flags a viewer reads. ---
    let mut cube = census.rez(residue, 2).await?;
    metrics.set("unscripted_flags", flags_text(cube.object.update_flags));
    let installed = census.install(&cube, &probe_source(true, None)).await?;
    let script = installed.item;
    metrics.set("probe_says_it_started", installed.saved.line('R').is_some());
    Census::note_flags(metrics, "touch_script", &mut cube, &installed.saved);

    // --- 3. A click. ---
    let position = cube.position().clone();
    let clicked = census.click(By::Owner, &cube, None).await?;
    census.note_touch(metrics, "click", &cube, &clicked);
    let clicked = census
        .click(
            By::Owner,
            &cube,
            Some(surface_at(&position, &FIRST_SPOT, 3)),
        )
        .await?;
    census.note_touch(metrics, "click_with_a_surface", &cube, &clicked);
    let clicked = census
        .click(By::Owner, &cube, Some(surface_of_nothing(&position)))
        .await?;
    census.note_touch(metrics, "click_with_no_such_surface", &cube, &clicked);

    // --- 4. A held button. ---
    let held = census
        .gesture(
            By::Owner,
            vec![
                cube.grab(By::Owner, Some(surface_at(&position, &FIRST_SPOT, 3))),
                Step::Wait(HOLD),
                cube.degrab(By::Owner, None),
            ],
        )
        .await?;
    census.note_touch(metrics, "hold", &cube, &held);
    let still = Vector {
        x: 0.5,
        y: 0.0,
        z: 0.0,
    };
    let mut steps = vec![cube.grab(By::Owner, Some(surface_at(&position, &FIRST_SPOT, 3)))];
    for _ in 0..HOLD_UPDATES {
        steps.push(cube.update(&still, Some(surface_at(&position, &SECOND_SPOT, 2))));
        steps.push(Step::Wait(UPDATE_EVERY));
    }
    steps.push(cube.degrab(By::Owner, Some(surface_at(&position, &FIRST_SPOT, 3))));
    let held = census.gesture(By::Owner, steps).await?;
    census.note_touch(metrics, "hold_with_updates", &cube, &held);
    // The same with no surface in the updates: what a `touch` then reads.
    let mut steps = vec![cube.grab(By::Owner, Some(surface_at(&position, &FIRST_SPOT, 3)))];
    for _ in 0..HOLD_UPDATES {
        steps.push(cube.update(&still, None));
        steps.push(Step::Wait(UPDATE_EVERY));
    }
    steps.push(cube.degrab(By::Owner, None));
    let held = census.gesture(By::Owner, steps).await?;
    census.note_touch(metrics, "hold_with_bare_updates", &cube, &held);

    // --- 5. Messages out of order. ---
    let released = census
        .gesture(By::Owner, vec![cube.degrab(By::Owner, None)])
        .await?;
    metrics.set("release_with_no_press_events", released.events());
    let pressed_twice = census
        .gesture(
            By::Owner,
            vec![
                cube.grab(By::Owner, None),
                Step::Wait(Duration::from_millis(500)),
                cube.grab(By::Owner, None),
                Step::Wait(Duration::from_millis(500)),
                cube.degrab(By::Owner, None),
            ],
        )
        .await?;
    metrics.set("two_presses_and_a_release_events", pressed_twice.events());
    let mut steps = Vec::new();
    for _ in 0..DRAG_UPDATES {
        steps.push(cube.update(&still, None));
        steps.push(Step::Wait(UPDATE_EVERY));
    }
    let updated = census.gesture(By::Owner, steps).await?;
    metrics.set("updates_with_no_press_events", updated.events());
    // Whatever that left open is closed before the next leg.
    census
        .gesture(By::Owner, vec![cube.degrab(By::Owner, None)])
        .await?;

    // --- 6. Another avatar's click. ---
    let clicked = census
        .click(
            By::Stranger,
            &cube,
            Some(surface_at(&position, &FIRST_SPOT, 3)),
        )
        .await?;
    census.note_touch(metrics, "stranger_click", &cube, &clicked);

    // --- 7. Click actions. ---
    for (action, word) in [
        (ClickAction::Sit, "sit"),
        (ClickAction::Disabled, "disabled"),
    ] {
        let set = census
            .gesture(
                By::Owner,
                vec![Step::Send(Command::SetObjectClickAction {
                    local_id: cube.object.scoped_id(),
                    action,
                })],
            )
            .await?;
        metrics.set(
            &format!("click_action_{word}_told_owner"),
            forms(&set.to_toucher, cube.object.local_id),
        );
        metrics.set(
            &format!("click_action_{word}_told_other"),
            forms(&set.to_other, cube.object.local_id),
        );
        metrics.set(
            &format!("click_action_{word}_shown_to_the_other_as"),
            set.to_other
                .changed
                .iter()
                .rfind(|object| object.full_id == cube.object.full_id)
                .map_or(-1, |object| i64::from(object.click_action)),
        );
        reread(census.owner, &census.run, &mut cube.object).await?;
        metrics.set(
            &format!("click_action_{word}_reads_back_as"),
            i64::from(cube.object.click_action),
        );
        let clicked = census.click(By::Stranger, &cube, None).await?;
        metrics.set(
            &format!("click_action_{word}_click_events"),
            clicked.events(),
        );
    }
    census
        .gesture(
            By::Owner,
            vec![Step::Send(Command::SetObjectClickAction {
                local_id: cube.object.scoped_id(),
                action: ClickAction::Touch,
            })],
        )
        .await?;

    // --- 8. A drag of a scripted prim. ---
    reread(census.owner, &census.run, &mut cube.object).await?;
    let dragged = census
        .drag(By::Owner, &mut cube, &sideways, DRAG_UPDATES, true)
        .await?;
    Census::note_drag(metrics, "scripted_owner_drag", &cube, &dragged);
    metrics.set("scripted_owner_drag_events", dragged.touched.events());

    // A script with no touch handlers, and then none.
    let saved = census
        .save(&cube, script, &probe_source(false, None))
        .await?;
    Census::note_flags(metrics, "bare_script", &mut cube, &saved);
    let clicked = census.click(By::Owner, &cube, None).await?;
    metrics.set("bare_script_click_events", clicked.events());
    let dragged = census
        .drag(By::Owner, &mut cube, &sideways, DRAG_UPDATES, true)
        .await?;
    Census::note_drag(metrics, "bare_scripted_owner_drag", &cube, &dragged);
    let removed = census
        .gesture(
            By::Owner,
            vec![Step::Send(Command::RemoveTaskInventory {
                target: cube.object.scoped_id(),
                item_id: script,
            })],
        )
        .await?;
    Census::note_flags(metrics, "script_removed", &mut cube, &removed);

    // --- 9. A linkset. ---
    let mut root = census.rez(residue, 4).await?;
    let mut child = census.rez(residue, 5).await?;
    let linked = census
        .gesture(
            By::Owner,
            vec![Step::Send(Command::LinkObjects {
                local_ids: vec![root.object.scoped_id(), child.object.scoped_id()],
            })],
        )
        .await?;
    let is_linked = linked
        .sightings(&child)
        .iter()
        .any(|object| object.parent_id == root.object.local_id);
    if !is_linked {
        return Err(TestFailure::Assertion(
            "the two cubes never linked".to_owned(),
        ));
    }
    // The child goes when its root does.
    residue
        .roots
        .retain(|root| *root != child.object.scoped_id());
    let installed = census.install(&root, &probe_source(true, None)).await?;
    Census::note_flags(metrics, "linkset_root_script", &mut root, &installed.saved);
    metrics.set(
        "linkset_root_script_tells_of_the_child",
        forms(&installed.saved.to_toucher, child.object.local_id),
    );
    reread(census.owner, &census.run, &mut child.object).await?;
    metrics.set(
        "linkset_root_script_child_flags",
        flags_text(child.object.update_flags),
    );
    let clicked = census.click(By::Stranger, &child, None).await?;
    metrics.set(
        "root_script_alone_child_click_tells",
        clicked.told_scripts(),
    );
    let clicked = census.click(By::Stranger, &root, None).await?;
    metrics.set("root_script_alone_root_click_tells", clicked.told_scripts());

    let installed = census.install(&child, &probe_source(true, None)).await?;
    let child_script = installed.item;
    Census::note_flags(
        metrics,
        "linkset_child_script",
        &mut child,
        &installed.saved,
    );
    let clicked = census.click(By::Stranger, &child, None).await?;
    metrics.set("both_scripts_child_click_tells", clicked.told_scripts());
    let clicked = census.click(By::Stranger, &root, None).await?;
    metrics.set("both_scripts_root_click_tells", clicked.told_scripts());
    for (handlers, pass, key) in [
        (true, 1, "child_passing_always_child_click_tells"),
        (true, 2, "child_passing_never_child_click_tells"),
        (true, 0, "child_passing_if_not_handled_child_click_tells"),
        (
            false,
            0,
            "bare_child_passing_if_not_handled_child_click_tells",
        ),
        (false, 2, "bare_child_passing_never_child_click_tells"),
        (false, 1, "bare_child_passing_always_child_click_tells"),
    ] {
        census
            .save(&child, child_script, &probe_source(handlers, Some(pass)))
            .await?;
        let clicked = census.click(By::Stranger, &child, None).await?;
        metrics.set(key, clicked.told_scripts());
    }

    // --- 10. A physical prim. ---
    let plain_id = plain.object.scoped_id();
    let physical = |use_physics| Command::SetObjectFlags {
        local_id: plain_id,
        flags: ObjectFlagSettings {
            use_physics,
            is_temporary: false,
            is_phantom: false,
            casts_shadows: false,
        },
    };
    census
        .gesture(By::Owner, vec![Step::Send(physical(true))])
        .await?;
    // It falls, and comes to rest.
    listen(census.owner, pace.quiet).await?;
    listen(census.stranger, drain).await?;
    reread(census.owner, &census.run, &mut plain.object).await?;
    metrics.set(
        "physical_prim_is_physical",
        plain.object.update_flags & FLAGS_USE_PHYSICS != 0,
    );
    // The other avatar first, then the owner's spin, and the owner's drag
    // last: Second Life pulls a physical prim hard, and what is asked after
    // it has gone is asked of nothing.
    let along = Vector {
        x: 0.0,
        y: PULL_M,
        z: 0.0,
    };
    for (by, key) in [
        (By::Stranger, "physical_stranger_drag"),
        (By::Owner, "physical_owner_drag"),
    ] {
        if by == By::Owner {
            let spun = census.spin(By::Owner, &mut plain, true).await?;
            metrics.set("physical_owner_spin_leaves_the_prim", spun.leaves);
            listen(census.owner, pace.quiet).await?;
            listen(census.stranger, drain).await?;
            reread(census.owner, &census.run, &mut plain.object).await?;
        }
        let dragged = census
            .drag(by, &mut plain, &along, PHYSICAL_DRAG_UPDATES, true)
            .await?;
        metrics.set(&format!("{key}_leaves_the_prim"), dragged.pulled());
        metrics.set(&format!("{key}_furthest_m"), f64::from(dragged.furthest));
        metrics.set(&format!("{key}_went_m"), f64::from(dragged.went));
        metrics.set(
            &format!("{key}_told_toucher"),
            forms(&dragged.touched.to_toucher, plain.object.local_id),
        );
        metrics.set(
            &format!("{key}_told_other"),
            forms(&dragged.touched.to_other, plain.object.local_id),
        );
    }
    census
        .gesture(By::Owner, vec![Step::Send(physical(false))])
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Line, parse_line, parse_vector, point_text, probe_source, vector_text};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::Vector;

    #[test]
    fn a_probe_line_parses() -> Result<(), String> {
        let line = parse_line(
            "SLCTG|1|E|1|12|2.003400|2|3|<0.40000, 0.60000, 0.00000>|<0.25000, 0.75000, \
             0.00000>|<10.10000, 20.20000, 30.30000>|<0.00000, 0.00000, 1.00000>|<0.00000, \
             1.00000, 0.00000>|<0.00000, 0.00000, 0.00000>|\
             11111111-2222-3333-4444-555555555555|3",
        );
        let Some(Line {
            script,
            event,
            touches,
            held,
            detected: Some(detected),
            ..
        }) = line
        else {
            return Err(format!("the line did not parse: {line:?}"));
        };
        assert_eq!((script, event, touches, held), (1, 'E', 1, 12));
        assert_eq!((detected.link, detected.face, detected.kind), (2, 3, 3));
        assert_eq!(vector_text(detected.st), "<0.40,0.60,0.00>");
        assert_eq!(
            point_text(
                detected.position,
                &Vector {
                    x: 10.0,
                    y: 20.0,
                    z: 30.0
                }
            ),
            "prim+<0.10,0.20,0.30>"
        );
        Ok(())
    }

    #[test]
    fn a_ready_line_and_other_chat() {
        assert_eq!(
            parse_line("SLCTG|2|R|unset").map(|line| (line.script, line.event)),
            Some((2, 'R'))
        );
        assert_eq!(parse_line("Hello, Avatar!"), None);
        assert_eq!(parse_vector("<1, 2>"), None);
    }

    #[test]
    fn the_probe_compiles() -> Result<(), String> {
        for (handlers, pass) in [(true, None), (false, Some(2)), (true, Some(1))] {
            let source = probe_source(handlers, pass);
            sl_lsl_runtime::compile(&source)
                .map(|_program| ())
                .map_err(|errors| format!("the probe does not compile: {errors:?}\n{source}"))?;
        }
        Ok(())
    }
}
