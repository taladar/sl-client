//! A census of an avatar's animations: what a grid states of them, to the
//! avatar's own viewer and to another one beside it, as the avatar starts and
//! stops them.
//!
//! A viewer starts and stops its avatar's animations with an `AgentAnimation`
//! (a list of `(animation, start)` pairs), and the simulator states the
//! avatar's whole set back to every viewer in view in an `AvatarAnimation`:
//! one entry per animation playing, with the sequence number it was started
//! under, and an `AnimationSourceList` naming the object that started it. A
//! stop is an animation dropping out of a later statement. What varies is
//! when a statement is sent, what it holds beside what was asked for, what a
//! sequence number says, and whom a source names.
//!
//! Avatar **A** acts; **B** stands beside it and only listens. The legs, in
//! order:
//!
//! 1. **Arrival** — the sets the settle after login brought, and a quiet
//!    stretch after it: what A and B are told of A while nothing happens.
//! 2. **Play and stop** a built-in gesture (`clap`), each asked twice: is a
//!    second start a restart, is a stop of something not playing answered.
//! 3. **Several in one message**: two started together, then both stopped and
//!    a third started in one message.
//! 4. **An animation nobody has** (a made-up id) started and stopped.
//! 5. **A one-shot left playing**: does the grid ever drop it by itself.
//! 6. **The grid's own animations**: `stand` stopped and started by hand,
//!    `walk` started and stopped by hand, typing begun and ended, the away
//!    control set and cleared.
//! 7. **The reset** the reference sends with its *Stop animations* (a stop of
//!    the nil id).
//! 8. **A script's animation** (live grids): a probe in a cube asks for
//!    `PERMISSION_TRIGGER_ANIMATION`, starts and stops `dance2`, has it stopped
//!    by the avatar instead, and is deleted while it plays.
//!
//! Each leg records the changes each viewer was told (`*_own`, `*_seen`),
//! whether a statement repeated the set unchanged (`*_restated`), A's set
//! afterwards with each entry's source (`*_set`), and for a start what its
//! sequence number is next to the others' (`*_sequence`).
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/avatars.md` § Animations; every run is held to it.
//!
//! `2av`, `[both]`, legs 1 to 7 offline on both fake flavours: there B is
//! shown nobody, so only A's own column is held.

use std::collections::{BTreeMap, HashSet};
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AnimationKey, ChatChannel, ChatType, Command, ControlFlags, Event,
    InventoryFolderKey, Object, ObjectKey, PlayingAnimation, PrimShape, RegionLocalObjectId,
    RestoreItem, RezScriptParams, ScopedObjectId, ScriptPermissionRequest, ScriptPermissions,
    ScriptTarget, ScriptUploadLocation, Uuid, Vector, pcode,
};

use crate::cases::object_rez_derez::{Answer, Residue, clean_up};
use crate::cases::task_inventory::{hold_to_measured, listing, login_folders};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, is_own, settle_scene_observing};

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/avatars.md § Animations (animations, 2026-10-19)";

/// Where both avatars log in on OpenSim: the level square in the block's
/// north-eastern region, where a cube may be rezzed.
const OPENSIM_START: &str = "uri:Northeast Region&70&180&26";

/// The built-in gesture the census plays and stops (`ANIM_AGENT_CLAP`): a
/// one-shot, and no part of an avatar's baseline.
const CLAP: &str = "clap";

/// The second built-in started beside it (`ANIM_AGENT_EXPRESS_LAUGH`).
const LAUGH: &str = "express_laugh";

/// The third, started as the other two stop (`ANIM_AGENT_DANCE1`), a loop.
const DANCE: &str = "dance1";

/// The built-in the probe script plays, by the name `llStartAnimation` takes.
const SCRIPT_DANCE: &str = "dance2";

/// An id no animation has: what a viewer naming an asset that does not exist
/// sends.
const MADE_UP: Uuid = Uuid::from_u128(0x6c1f_3d52_9a0b_4e77_8d2c_5a1e_0b9f_4c37);

/// What a statement entry for [`MADE_UP`] is called in the metrics.
const MADE_UP_WORD: &str = "made-up";

/// What an entry that is no built-in and not [`MADE_UP`] is called.
const ASSET_WORD: &str = "asset";

/// How long a leg listens to both avatars on a live grid.
const WINDOW: Duration = Duration::from_secs(3);

/// [`WINDOW`] on a fake grid.
const FAKE_WINDOW: Duration = Duration::from_millis(500);

/// How long the quiet stretch after the arrival lasts on a live grid.
const IDLE: Duration = Duration::from_secs(12);

/// [`IDLE`] on a fake grid.
const FAKE_IDLE: Duration = Duration::from_secs(1);

/// How long a one-shot is left playing on a live grid: several times as long
/// as the clap lasts.
const ONE_SHOT: Duration = Duration::from_secs(15);

/// [`ONE_SHOT`] on a fake grid.
const FAKE_ONE_SHOT: Duration = Duration::from_secs(1);

/// How long the scene settle after login may take.
const SETTLE_WINDOW: Duration = Duration::from_secs(15);

/// The idle gap that ends the settle on a live grid.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// [`SETTLE_IDLE`] on a fake grid.
const FAKE_SETTLE_IDLE: Duration = Duration::from_secs(1);

/// How long a rez, a script's save or the probe's answer may take.
const STEP: Duration = Duration::from_secs(30);

/// How far from A the cube stands, in metres.
const CUBE_AWAY_M: f32 = 2.0;

/// How far from where it was asked for the new cube may stand, in metres.
const CUBE_FOUND_WITHIN_M: f32 = 1.5;

/// What every line the probe says begins with.
const PROBE_TAG: &str = "SLCAN";

/// The name of the probe script in the cube.
const PROBE_NAME: &str = "SLClientAnimationProbe";

/// The channel the probe takes its orders on.
const PROBE_CHANNEL: i32 = 7_734_301;

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

/// What each grid was measured to state of A's own animations, by the metric
/// the census records it under: every run, live or fake, is held to it.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("after_reset_own", both(Answer::Text("-dance1"))),
    (
        "after_reset_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("after_reset_set", each(Answer::Any, Answer::Text("stand"))),
    (
        "arrival_own_first",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("away_off_own", both(Answer::Text("nothing"))),
    (
        "away_off_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("away_off_set", each(Answer::Any, Answer::Text("stand"))),
    ("away_on_own", both(Answer::Text("nothing"))),
    (
        "away_on_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("away_on_set", each(Answer::Any, Answer::Text("stand"))),
    ("before_reset_own", both(Answer::Text("+dance1"))),
    (
        "before_reset_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "before_reset_set",
        each(Answer::Any, Answer::Text("dance1+stand")),
    ),
    (
        "idle_attachments_named",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("idle_own", both(Answer::Text("nothing"))),
    ("idle_own_order", each(Answer::Any, Answer::Text("stand"))),
    ("idle_own_restated", each(Answer::Any, Answer::Flag(false))),
    (
        "idle_own_sources",
        each(Answer::Text("one per entry"), Answer::Text("none")),
    ),
    ("idle_own_statements", each(Answer::Any, Answer::Count(0))),
    ("idle_set", each(Answer::Any, Answer::Text("stand"))),
    ("one_shot_left_own", both(Answer::Text("+clap"))),
    (
        "one_shot_left_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "one_shot_left_set",
        each(Answer::Any, Answer::Text("clap+stand")),
    ),
    ("one_shot_stopped_own", both(Answer::Text("-clap"))),
    (
        "one_shot_stopped_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "one_shot_stopped_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    (
        "physical_events_own",
        each(Answer::Text("one empty block"), Answer::Text("no block")),
    ),
    ("play_again_own", each(Answer::Any, Answer::Text("nothing"))),
    (
        "play_again_own_restated",
        each(Answer::Any, Answer::Flag(true)),
    ),
    (
        "play_again_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "play_again_set",
        each(Answer::Any, Answer::Text("clap+stand")),
    ),
    ("play_made_up_own", both(Answer::Text("+made-up"))),
    (
        "play_made_up_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "play_made_up_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "play_made_up_set",
        each(Answer::Any, Answer::Text("made-up+stand")),
    ),
    ("play_own", both(Answer::Text("+clap"))),
    ("play_own_restated", each(Answer::Any, Answer::Flag(false))),
    (
        "play_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    ("play_set", each(Answer::Any, Answer::Text("clap+stand"))),
    ("play_stand_own", each(Answer::Any, Answer::Text("nothing"))),
    (
        "play_stand_own_restated",
        each(Answer::Any, Answer::Flag(true)),
    ),
    ("play_stand_set", each(Answer::Any, Answer::Text("stand"))),
    (
        "play_two_order",
        each(Answer::Any, Answer::Text("stand,clap,express_laugh")),
    ),
    ("play_two_own", both(Answer::Text("+clap +express_laugh"))),
    (
        "play_two_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "play_two_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "play_two_set",
        each(Answer::Any, Answer::Text("clap+express_laugh+stand")),
    ),
    ("play_walk_own", both(Answer::Text("+walk"))),
    (
        "play_walk_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "play_walk_set",
        each(Answer::Any, Answer::Text("stand+walk")),
    ),
    ("reset_own", each(Answer::Any, Answer::Text("nothing"))),
    ("reset_own_restated", each(Answer::Any, Answer::Flag(true))),
    ("reset_set", each(Answer::Any, Answer::Text("dance1+stand"))),
    ("stop_again_own", both(Answer::Text("nothing"))),
    (
        "stop_again_own_restated",
        each(Answer::Any, Answer::Flag(true)),
    ),
    ("stop_again_set", each(Answer::Any, Answer::Text("stand"))),
    ("stop_dance_own", both(Answer::Text("-dance1"))),
    (
        "stop_dance_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("stop_dance_set", each(Answer::Any, Answer::Text("stand"))),
    ("stop_made_up_own", both(Answer::Text("-made-up"))),
    (
        "stop_made_up_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("stop_made_up_set", each(Answer::Any, Answer::Text("stand"))),
    ("stop_own", both(Answer::Text("-clap"))),
    ("stop_own_restated", each(Answer::Any, Answer::Flag(false))),
    ("stop_set", each(Answer::Any, Answer::Text("stand"))),
    ("stop_stand_own", each(Answer::Any, Answer::Text("nothing"))),
    (
        "stop_stand_own_restated",
        each(Answer::Any, Answer::Flag(true)),
    ),
    ("stop_stand_set", each(Answer::Any, Answer::Text("stand"))),
    ("stop_walk_own", both(Answer::Text("-walk"))),
    (
        "stop_walk_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("stop_walk_set", each(Answer::Any, Answer::Text("stand"))),
    (
        "swap_own",
        both(Answer::Text("+dance1 -clap -express_laugh")),
    ),
    ("swap_own_restated", each(Answer::Any, Answer::Flag(false))),
    (
        "swap_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    ("swap_set", each(Answer::Any, Answer::Text("dance1+stand"))),
    ("typing_off_own", both(Answer::Text("nothing"))),
    (
        "typing_off_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("typing_off_set", each(Answer::Any, Answer::Text("stand"))),
    ("typing_on_own", both(Answer::Text("nothing"))),
    (
        "typing_on_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("typing_on_set", each(Answer::Any, Answer::Text("stand"))),
];

/// What each grid was measured to state of A to B, and of a script's
/// animation: held on the live grids only, since a fake grid shows B nobody
/// and runs no script.
const MEASURED_LIVE: &[(&str, Measured<Answer>)] = &[
    ("after_reset_seen", both(Answer::Text("-dance1"))),
    (
        "after_reset_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "after_reset_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    (
        "arrival_seen_first",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("away_off_seen", both(Answer::Text("nothing"))),
    (
        "away_off_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "away_off_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("away_on_seen", both(Answer::Text("nothing"))),
    (
        "away_on_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("away_on_seen_set", each(Answer::Any, Answer::Text("stand"))),
    ("before_reset_seen", both(Answer::Text("+dance1"))),
    (
        "before_reset_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "before_reset_seen_set",
        each(Answer::Any, Answer::Text("dance1+stand")),
    ),
    ("cube_deleted_own", both(Answer::Text("nothing"))),
    (
        "cube_deleted_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("cube_deleted_seen", both(Answer::Text("nothing"))),
    (
        "cube_deleted_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "cube_deleted_seen_set",
        each(Answer::Any, Answer::Text("dance2+stand")),
    ),
    (
        "cube_deleted_set",
        each(Answer::Any, Answer::Text("dance2@cube+stand")),
    ),
    ("idle_seen", both(Answer::Text("nothing"))),
    ("idle_seen_is_own", each(Answer::Any, Answer::Flag(true))),
    ("idle_seen_order", each(Answer::Any, Answer::Text("stand"))),
    ("idle_seen_restated", each(Answer::Any, Answer::Flag(false))),
    ("idle_seen_set", each(Answer::Any, Answer::Text("stand"))),
    (
        "idle_seen_sources",
        each(Answer::Text("only the first entry's"), Answer::Text("none")),
    ),
    ("idle_seen_statements", each(Answer::Any, Answer::Count(0))),
    ("one_shot_left_seen", both(Answer::Text("+clap"))),
    (
        "one_shot_left_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "one_shot_left_seen_set",
        each(Answer::Any, Answer::Text("clap+stand")),
    ),
    ("one_shot_stopped_seen", both(Answer::Text("-clap"))),
    (
        "one_shot_stopped_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "one_shot_stopped_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    (
        "physical_events_seen",
        each(Answer::Text("one empty block"), Answer::Text("no block")),
    ),
    (
        "play_again_seen",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "play_again_seen_restated",
        each(Answer::Any, Answer::Flag(true)),
    ),
    (
        "play_again_seen_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "play_again_seen_set",
        each(Answer::Any, Answer::Text("clap+stand")),
    ),
    (
        "play_again_sequence_own_and_seen",
        each(Answer::Any, Answer::Text("the same")),
    ),
    ("play_made_up_seen", both(Answer::Text("+made-up"))),
    (
        "play_made_up_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "play_made_up_seen_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "play_made_up_seen_set",
        each(Answer::Any, Answer::Text("made-up+stand")),
    ),
    (
        "play_made_up_sequence_own_and_seen",
        both(Answer::Text("the same")),
    ),
    ("play_seen", both(Answer::Text("+clap"))),
    ("play_seen_restated", each(Answer::Any, Answer::Flag(false))),
    (
        "play_seen_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "play_seen_set",
        each(Answer::Any, Answer::Text("clap+stand")),
    ),
    ("play_sequence_own_and_seen", both(Answer::Text("the same"))),
    (
        "play_stand_seen",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "play_stand_seen_restated",
        each(Answer::Any, Answer::Flag(true)),
    ),
    (
        "play_stand_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("play_two_seen", both(Answer::Text("+clap +express_laugh"))),
    (
        "play_two_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "play_two_seen_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "play_two_seen_set",
        each(Answer::Any, Answer::Text("clap+express_laugh+stand")),
    ),
    (
        "play_two_sequence_own_and_seen",
        both(Answer::Text("the same")),
    ),
    ("play_walk_seen", both(Answer::Text("+walk"))),
    (
        "play_walk_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "play_walk_seen_set",
        each(Answer::Any, Answer::Text("stand+walk")),
    ),
    ("reset_seen", each(Answer::Any, Answer::Text("nothing"))),
    ("reset_seen_restated", each(Answer::Any, Answer::Flag(true))),
    (
        "reset_seen_set",
        each(Answer::Any, Answer::Text("dance1+stand")),
    ),
    ("script_ask_own", both(Answer::Text("nothing"))),
    (
        "script_ask_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("script_ask_permissions", both(Answer::Count(16))),
    ("script_ask_seen", both(Answer::Text("nothing"))),
    (
        "script_ask_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "script_ask_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("script_ask_set", each(Answer::Any, Answer::Text("stand"))),
    ("script_restart_own", both(Answer::Text("+dance2"))),
    (
        "script_restart_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("script_restart_seen", both(Answer::Text("+dance2"))),
    (
        "script_restart_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "script_restart_seen_set",
        each(Answer::Any, Answer::Text("dance2+stand")),
    ),
    (
        "script_restart_set",
        each(Answer::Any, Answer::Text("dance2@cube+stand")),
    ),
    ("script_start_again_own", both(Answer::Text("+dance2"))),
    (
        "script_start_again_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("script_start_again_seen", both(Answer::Text("+dance2"))),
    (
        "script_start_again_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "script_start_again_seen_set",
        each(Answer::Any, Answer::Text("dance2+stand")),
    ),
    (
        "script_start_again_set",
        each(Answer::Any, Answer::Text("dance2@cube+stand")),
    ),
    (
        "script_start_order",
        each(Answer::Any, Answer::Text("dance2@cube,stand")),
    ),
    ("script_start_own", both(Answer::Text("+dance2"))),
    (
        "script_start_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("script_start_seen", both(Answer::Text("+dance2"))),
    (
        "script_start_seen_order",
        each(Answer::Any, Answer::Text("stand,dance2")),
    ),
    (
        "script_start_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "script_start_seen_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "script_start_seen_set",
        each(Answer::Any, Answer::Text("dance2+stand")),
    ),
    (
        "script_start_seen_sources",
        each(Answer::Text("only the first entry's"), Answer::Text("none")),
    ),
    (
        "script_start_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "script_start_sequence_own_and_seen",
        both(Answer::Text("the same")),
    ),
    (
        "script_start_set",
        each(Answer::Any, Answer::Text("dance2@cube+stand")),
    ),
    (
        "script_start_sources",
        each(
            Answer::Text("one per entry"),
            Answer::Text("only the first entry's"),
        ),
    ),
    ("script_stop_own", both(Answer::Text("-dance2"))),
    (
        "script_stop_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    ("script_stop_seen", both(Answer::Text("-dance2"))),
    (
        "script_stop_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "script_stop_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("script_stop_set", each(Answer::Any, Answer::Text("stand"))),
    ("script_stopped_by_agent_own", both(Answer::Text("-dance2"))),
    (
        "script_stopped_by_agent_own_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "script_stopped_by_agent_seen",
        both(Answer::Text("-dance2")),
    ),
    (
        "script_stopped_by_agent_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "script_stopped_by_agent_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    (
        "script_stopped_by_agent_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("stop_again_seen", both(Answer::Text("nothing"))),
    (
        "stop_again_seen_restated",
        each(Answer::Any, Answer::Flag(true)),
    ),
    (
        "stop_again_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("stop_dance_seen", both(Answer::Text("-dance1"))),
    (
        "stop_dance_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "stop_dance_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("stop_made_up_seen", both(Answer::Text("-made-up"))),
    (
        "stop_made_up_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "stop_made_up_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("stop_seen", both(Answer::Text("-clap"))),
    ("stop_seen_restated", each(Answer::Any, Answer::Flag(false))),
    ("stop_seen_set", each(Answer::Any, Answer::Text("stand"))),
    (
        "stop_stand_seen",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "stop_stand_seen_restated",
        each(Answer::Any, Answer::Flag(true)),
    ),
    (
        "stop_stand_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("stop_walk_seen", both(Answer::Text("-walk"))),
    (
        "stop_walk_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "stop_walk_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    (
        "swap_seen",
        both(Answer::Text("+dance1 -clap -express_laugh")),
    ),
    ("swap_seen_restated", each(Answer::Any, Answer::Flag(false))),
    (
        "swap_seen_sequence",
        each(Answer::Any, Answer::Text("above the others")),
    ),
    (
        "swap_seen_set",
        each(Answer::Any, Answer::Text("dance1+stand")),
    ),
    ("swap_sequence_own_and_seen", both(Answer::Text("the same"))),
    ("typing_off_seen", both(Answer::Text("nothing"))),
    (
        "typing_off_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "typing_off_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
    ("typing_on_seen", both(Answer::Text("nothing"))),
    (
        "typing_on_seen_restated",
        each(Answer::Any, Answer::Flag(false)),
    ),
    (
        "typing_on_seen_set",
        each(Answer::Any, Answer::Text("stand")),
    ),
];

/// The animation census.
#[derive(Debug)]
pub struct Animations;

impl GridTest for Animations {
    fn name(&self) -> &'static str {
        "animations"
    }

    fn description(&self) -> &'static str {
        "Start and stop an avatar's animations and record what the grid states of them to the \
         avatar and to a second one beside it: restarts, stops of nothing, several in one \
         message, an unknown id, a one-shot left playing, the grid's own animations, the reset \
         and a script's animation"
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
        Duration::from_secs(900)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let folders = login_folders(ctx.primary()).await?;
            let mut residue = Residue::default();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, folders.trash, &mut residue, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            // Take the cube away whatever became of the census.
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

/// The id of the built-in called `name`.
fn builtin(name: &str) -> Result<Uuid, TestFailure> {
    sl_anim::builtin_animation_by_name(name)
        .map(|animation| animation.id)
        .ok_or_else(|| TestFailure::State(format!("`{name}` is no built-in animation")))
}

/// What a statement entry is called: its built-in name, [`MADE_UP_WORD`] or
/// [`ASSET_WORD`].
fn word(anim_id: Uuid) -> String {
    if anim_id == MADE_UP {
        return MADE_UP_WORD.to_owned();
    }
    sl_anim::builtin_animation(anim_id).map_or_else(
        || ASSET_WORD.to_owned(),
        |animation| animation.name.to_owned(),
    )
}

/// What one viewer was told of A, and what it knows of A's attachments.
#[derive(Debug, Default)]
struct View {
    /// A's avatar's region-local id in this viewer's stream, once sent.
    avatar_local: Option<RegionLocalObjectId>,
    /// The objects attached to A, by id.
    attachments: HashSet<Uuid>,
    /// Every statement of A's set this viewer was sent, in order.
    statements: Vec<Vec<PlayingAnimation>>,
    /// The shape of each statement's `PhysicalAvatarEventList`, in words,
    /// with how many statements had it.
    physical: BTreeMap<String, usize>,
    /// The probe's lines, without the tag.
    said: Vec<String>,
    /// The permission requests this viewer was sent.
    asked: Vec<ScriptPermissionRequest>,
    /// The answer to a script's save, once given.
    uploaded: Option<Result<bool, String>>,
    /// The objects A came to own that were new, in order.
    added: Vec<Object>,
}

impl View {
    /// Folds one event into the view; `actor` is A's id.
    fn note(&mut self, actor: Uuid, event: &Event) {
        match event {
            Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
                if object.full_id.uuid() == actor {
                    self.avatar_local = Some(object.local_id);
                } else if object.parent_id != RegionLocalObjectId(0)
                    && self.avatar_local == Some(object.parent_id)
                {
                    let _new = self.attachments.insert(object.full_id.uuid());
                }
                if matches!(event, Event::ObjectAdded(_))
                    && object.pcode == pcode::PRIMITIVE
                    && object.parent_id == RegionLocalObjectId(0)
                    && is_own(object, actor)
                {
                    self.added.push((**object).clone());
                }
            }
            Event::AvatarAnimation {
                avatar_id,
                animations,
                physical_events,
            } if avatar_id.uuid() == actor => {
                self.statements.push(animations.clone());
                let shape = match physical_events.as_slice() {
                    [] => "no block".to_owned(),
                    [only] if only.is_empty() => "one empty block".to_owned(),
                    blocks => format!(
                        "{} blocks of {} bytes",
                        blocks.len(),
                        blocks.iter().map(Vec::len).sum::<usize>()
                    ),
                };
                let count = self.physical.entry(shape).or_insert(0);
                *count = count.saturating_add(1);
            }
            Event::ChatReceived(chat) => {
                if let Some(line) = chat
                    .message
                    .strip_prefix(PROBE_TAG)
                    .and_then(|rest| rest.strip_prefix('|'))
                {
                    self.said.push(line.to_owned());
                }
            }
            Event::ScriptPermissionRequest(request) => self.asked.push((**request).clone()),
            Event::ScriptUploaded { compiled, .. } => self.uploaded = Some(Ok(*compiled)),
            Event::AssetUploadFailed { reason } => self.uploaded = Some(Err(reason.clone())),
            _ => {}
        }
    }

    /// A's set as last stated, or `None` before any statement.
    fn last(&self) -> Option<&[PlayingAnimation]> {
        self.statements.last().map(Vec::as_slice)
    }

    /// Whether the probe said `line`.
    fn said(&self, line: &str) -> bool {
        self.said.iter().any(|said| said == line)
    }
}

/// Listens on `session` for `window`, folding everything into `view`.
async fn listen(
    session: &mut Session,
    view: &mut View,
    actor: Uuid,
    window: Duration,
) -> Result<(), TestFailure> {
    listen_until(session, view, actor, window, |_view| false).await
}

/// Listens on `session` until `done` holds of `view`, or for `limit` at most.
async fn listen_until(
    session: &mut Session,
    view: &mut View,
    actor: Uuid,
    limit: Duration,
    done: impl Fn(&View) -> bool,
) -> Result<(), TestFailure> {
    let started = Instant::now();
    loop {
        if done(view) {
            return Ok(());
        }
        let remaining = limit.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(());
        }
        // One event at a time, so that `done` sees each as it arrives.
        match session
            .wait_for(remaining, |event| {
                view.note(actor, event);
                Some(())
            })
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(()),
            Err(other) => return Err(other),
        }
    }
}

/// Who an entry's source is, as the metrics say it.
fn source_word(source: Option<ObjectKey>, actor: Uuid, view: &View, cube: Option<Uuid>) -> String {
    let Some(source) = source else {
        return String::new();
    };
    let source = source.uuid();
    let who = if source.is_nil() {
        "nil"
    } else if source == actor {
        "self"
    } else if Some(source) == cube {
        "cube"
    } else if view.attachments.contains(&source) {
        "attachment"
    } else {
        "object"
    };
    format!("@{who}")
}

/// A set in words: each entry's name and source, sorted, a repeated one
/// counted (`asset@attachment×3`), or `-` for an empty set and `none` for no
/// statement at all.
fn describe_set(
    set: Option<&[PlayingAnimation]>,
    actor: Uuid,
    view: &View,
    cube: Option<Uuid>,
) -> String {
    let Some(set) = set else {
        return "none".to_owned();
    };
    if set.is_empty() {
        return "-".to_owned();
    }
    let mut counted: BTreeMap<String, usize> = BTreeMap::new();
    for entry in set {
        let name = format!(
            "{}{}",
            word(entry.anim_id),
            source_word(entry.source_id, actor, view, cube)
        );
        let count = counted.entry(name).or_insert(0);
        *count = count.saturating_add(1);
    }
    counted
        .into_iter()
        .map(|(name, count)| {
            if count == 1 {
                name
            } else {
                format!("{name}×{count}")
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// The entries of a set in the order the grid stated them, by name and
/// source.
fn describe_order(
    set: Option<&[PlayingAnimation]>,
    actor: Uuid,
    view: &View,
    cube: Option<Uuid>,
) -> String {
    set.map_or_else(
        || "none".to_owned(),
        |set| {
            set.iter()
                .map(|entry| {
                    format!(
                        "{}{}",
                        word(entry.anim_id),
                        source_word(entry.source_id, actor, view, cube)
                    )
                })
                .collect::<Vec<_>>()
                .join(",")
        },
    )
}

/// How a statement's `AnimationSourceList` lines up with its entries: none,
/// one per entry, or so many of so many.
fn describe_sources(set: Option<&[PlayingAnimation]>) -> String {
    let Some(set) = set else {
        return "none stated".to_owned();
    };
    let sourced = set.iter().filter(|entry| entry.source_id.is_some()).count();
    if sourced == 0 {
        "none".to_owned()
    } else if sourced == set.len() {
        "one per entry".to_owned()
    } else if sourced == 1 && set.first().is_some_and(|entry| entry.source_id.is_some()) {
        "only the first entry's".to_owned()
    } else {
        format!("{sourced} of {}", set.len())
    }
}

/// A set's entries for the animations in `subjects`, with their sequence
/// numbers, by name. A leg looks at its own animations only: an avatar
/// wearing an animation overrider (both aditi avatars do) has the overrider's
/// animations, and the postures it puts in place of `stand`, coming and going
/// all the time.
fn named(set: &[PlayingAnimation], subjects: &[Uuid]) -> BTreeMap<String, Vec<i32>> {
    let mut named: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    for entry in set {
        if subjects.contains(&entry.anim_id) {
            named
                .entry(word(entry.anim_id))
                .or_default()
                .push(entry.sequence_id);
        }
    }
    named
}

/// What changed from `before` to `after` among `subjects`: `+name` for one
/// added, `-name` for one gone, `~name` for one restarted under a new sequence
/// number; empty when nothing did.
fn changes(
    before: &[PlayingAnimation],
    after: &[PlayingAnimation],
    subjects: &[Uuid],
) -> Vec<String> {
    let (was, now) = (named(before, subjects), named(after, subjects));
    let mut out = Vec::new();
    for (name, sequences) in &now {
        match was.get(name) {
            None => out.push(format!("+{name}")),
            Some(earlier) if earlier != sequences => out.push(format!("~{name}")),
            Some(_same) => {}
        }
    }
    for name in was.keys() {
        if !now.contains_key(name) {
            out.push(format!("-{name}"));
        }
    }
    out
}

/// What the statements from `from` on told a viewer of `subjects`, against the
/// set it knew before them: one entry per statement that changed one of them,
/// `|`-joined, or `nothing`; and whether any statement left them as they were
/// (a restatement, or another animation's change).
fn told_since(view: &View, from: usize, subjects: &[Uuid]) -> (String, bool) {
    let mut before: Vec<PlayingAnimation> = from
        .checked_sub(1)
        .and_then(|index| view.statements.get(index))
        .cloned()
        .unwrap_or_default();
    let mut told = Vec::new();
    let mut restated = false;
    for statement in view.statements.iter().skip(from) {
        let changed = changes(&before, statement, subjects);
        if changed.is_empty() {
            restated = true;
        } else {
            told.push(changed.join(" "));
        }
        before.clone_from(statement);
    }
    let text = if told.is_empty() {
        "nothing".to_owned()
    } else {
        told.join(" | ")
    };
    (text, restated)
}

/// Where `subject`'s sequence number stands in the last statement holding it:
/// above every other entry's or not. Not whether it is the very next: a
/// grid's counter numbers other things too (OpenSim's skipped numbers between
/// the arrival's `stand` and the first start).
fn sequence_of(view: &View, subject: Uuid) -> String {
    let Some(set) = view
        .statements
        .iter()
        .rev()
        .find(|set| set.iter().any(|entry| entry.anim_id == subject))
    else {
        return "not stated".to_owned();
    };
    let Some(own) = set
        .iter()
        .find(|entry| entry.anim_id == subject)
        .map(|entry| entry.sequence_id)
    else {
        return "not stated".to_owned();
    };
    let highest = set
        .iter()
        .filter(|entry| entry.anim_id != subject)
        .map(|entry| entry.sequence_id)
        .max();
    match highest {
        None => "alone".to_owned(),
        Some(highest) if own > highest => "above the others".to_owned(),
        Some(_highest) => "not above the others".to_owned(),
    }
}

/// The shapes of the `PhysicalAvatarEventList`s a viewer was sent, sorted,
/// or `no statement`.
fn describe_physical(view: &View) -> String {
    if view.physical.is_empty() {
        "no statement".to_owned()
    } else {
        view.physical.keys().cloned().collect::<Vec<_>>().join(", ")
    }
}

/// The census's state: the two sessions and what each was told.
struct Census<'s> {
    /// Avatar A, which acts.
    a: &'s mut Session,
    /// Avatar B, which listens.
    b: &'s mut Session,
    /// A's id.
    actor: Uuid,
    /// What A was told.
    own: View,
    /// What B was told of A.
    seen: View,
    /// How long a leg listens.
    window: Duration,
    /// The probe's cube, once rezzed.
    cube: Option<Object>,
}

impl Census<'_> {
    /// Listens to both avatars for `window`.
    async fn listen(&mut self, window: Duration) -> Result<(), TestFailure> {
        let actor = self.actor;
        let (own, seen) = tokio::join!(
            listen(self.a, &mut self.own, actor, window),
            listen(self.b, &mut self.seen, actor, window)
        );
        own.and(seen)
    }

    /// The cube's id, once there is one.
    fn cube_id(&self) -> Option<Uuid> {
        self.cube.as_ref().map(|cube| cube.full_id.uuid())
    }

    /// Records what both viewers were told since `from` (A's and B's statement
    /// counts) under `key`.
    fn record(&self, metrics: &mut Metrics, key: &str, from: (usize, usize), subjects: &[Uuid]) {
        let (own, own_restated) = told_since(&self.own, from.0, subjects);
        let (seen, seen_restated) = told_since(&self.seen, from.1, subjects);
        metrics.set(&format!("{key}_own"), own);
        metrics.set(&format!("{key}_own_restated"), own_restated);
        metrics.set(&format!("{key}_seen"), seen);
        metrics.set(&format!("{key}_seen_restated"), seen_restated);
        let cube = self.cube_id();
        metrics.set(
            &format!("{key}_set"),
            describe_set(self.own.last(), self.actor, &self.own, cube),
        );
        metrics.set(
            &format!("{key}_seen_set"),
            describe_set(self.seen.last(), self.actor, &self.seen, cube),
        );
    }

    /// The statement counts so far, for a later [`Census::record`].
    const fn mark(&self) -> (usize, usize) {
        (self.own.statements.len(), self.seen.statements.len())
    }

    /// Sends `commands` as A, listens for `window` and records what was told
    /// of `subjects` under `key`.
    async fn leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        commands: Vec<Command>,
        window: Duration,
        subjects: &[Uuid],
    ) -> Result<(), TestFailure> {
        let from = self.mark();
        for command in commands {
            self.a.send(command).await?;
        }
        self.listen(window).await?;
        self.record(metrics, key, from, subjects);
        Ok(())
    }

    /// [`Census::leg`] for a start, also recording where the sequence number
    /// of the last of `subjects` — the one started — stands in each viewer's
    /// statement and whether the two agree.
    async fn start_leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        commands: Vec<Command>,
        subjects: &[Uuid],
    ) -> Result<(), TestFailure> {
        let window = self.window;
        self.leg(metrics, key, commands, window, subjects).await?;
        if let Some(&started) = subjects.last() {
            self.record_sequence(metrics, key, started);
        }
        Ok(())
    }

    /// Records where `subject`'s sequence number stands, under `key`.
    fn record_sequence(&self, metrics: &mut Metrics, key: &str, subject: Uuid) {
        metrics.set(&format!("{key}_sequence"), sequence_of(&self.own, subject));
        metrics.set(
            &format!("{key}_seen_sequence"),
            sequence_of(&self.seen, subject),
        );
        let in_set = |view: &View| {
            view.statements.iter().rev().find_map(|set| {
                set.iter()
                    .find(|entry| entry.anim_id == subject)
                    .map(|entry| entry.sequence_id)
            })
        };
        let agree = match (in_set(&self.own), in_set(&self.seen)) {
            (Some(own), Some(seen)) if own == seen => "the same",
            (Some(_own), Some(_seen)) => "different",
            _ => "not both stated",
        };
        metrics.set(&format!("{key}_sequence_own_and_seen"), agree);
    }

    /// Tells the probe `order` and waits until it says it has, then records
    /// what both viewers were told under `key`.
    async fn order(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        order: &str,
        subjects: &[Uuid],
    ) -> Result<(), TestFailure> {
        let from = self.mark();
        let done = format!("D|{order}");
        let said_before = self.own.said.len();
        self.a
            .send(Command::Chat {
                message: order.to_owned(),
                chat_type: ChatType::Normal,
                channel: ChatChannel(PROBE_CHANNEL),
            })
            .await?;
        let actor = self.actor;
        listen_until(self.a, &mut self.own, actor, STEP, |view| {
            view.said.iter().skip(said_before).any(|line| *line == done)
        })
        .await?;
        if !self
            .own
            .said
            .iter()
            .skip(said_before)
            .any(|line| *line == done)
        {
            return Err(TestFailure::Assertion(format!(
                "the probe never said it had done `{order}`"
            )));
        }
        let window = self.window;
        self.listen(window).await?;
        self.record(metrics, key, from, subjects);
        Ok(())
    }
}

/// The probe: a script that does what its owner says on the probe's channel
/// and says when it has.
fn probe_source() -> String {
    format!(
        r#"default
{{
    state_entry()
    {{
        llListen({PROBE_CHANNEL}, "", llGetOwner(), "");
        llOwnerSay("{PROBE_TAG}|R");
    }}
    listen(integer channel, string name, key id, string what)
    {{
        if (what == "ask") llRequestPermissions(llGetOwner(), PERMISSION_TRIGGER_ANIMATION);
        else if (what == "start") llStartAnimation("{SCRIPT_DANCE}");
        else if (what == "stop") llStopAnimation("{SCRIPT_DANCE}");
        llOwnerSay("{PROBE_TAG}|D|" + what);
    }}
    run_time_permissions(integer granted)
    {{
        llOwnerSay("{PROBE_TAG}|P|" + (string)granted);
    }}
}}
"#
    )
}

/// The census: both avatars settled beside each other, then every leg.
async fn census(
    ctx: &mut TestContext,
    trash: InventoryFolderKey,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let (window, idle, one_shot, settle_idle) = if grid.is_fake() {
        (FAKE_WINDOW, FAKE_IDLE, FAKE_ONE_SHOT, FAKE_SETTLE_IDLE)
    } else {
        (WINDOW, IDLE, ONE_SHOT, SETTLE_IDLE)
    };
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "animations needs a secondary avatar".to_owned(),
        ));
    };
    let actor = a
        .agent_id()
        .map(|agent| agent.uuid())
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;

    // --- 1. Arrival. ---
    let mut own = View::default();
    let settled = settle_scene_observing(
        a,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        settle_idle,
        |event| own.note(actor, event),
    )
    .await?;
    let mut seen = View::default();
    let _theirs = settle_scene_observing(
        b,
        grid,
        build_position,
        SETTLE_WINDOW,
        settle_idle,
        |event| seen.note(actor, event),
    )
    .await?;
    if a.region_handle() != b.region_handle() {
        return Err(TestFailure::Assertion(
            "the two avatars are not in the same region".to_owned(),
        ));
    }
    let standing = settled
        .avatar
        .ok_or_else(|| TestFailure::Assertion("A was never sent its own avatar".to_owned()))?;
    metrics.set(
        "arrival_own_first",
        describe_set(own.statements.first().map(Vec::as_slice), actor, &own, None),
    );
    metrics.set(
        "arrival_own_statements",
        i64::try_from(own.statements.len()).unwrap_or(i64::MAX),
    );
    metrics.set(
        "arrival_seen_first",
        describe_set(
            seen.statements.first().map(Vec::as_slice),
            actor,
            &seen,
            None,
        ),
    );
    metrics.set(
        "arrival_seen_statements",
        i64::try_from(seen.statements.len()).unwrap_or(i64::MAX),
    );
    let mut census = Census {
        a,
        b,
        actor,
        own,
        seen,
        window,
        cube: None,
    };

    // The quiet stretch: what is said of an avatar that does nothing.
    let from = census.mark();
    census.listen(idle).await?;
    census.record(metrics, "idle", from, &[]);
    metrics.set(
        "idle_own_statements",
        i64::try_from(census.own.statements.len().saturating_sub(from.0)).unwrap_or(i64::MAX),
    );
    metrics.set(
        "idle_seen_statements",
        i64::try_from(census.seen.statements.len().saturating_sub(from.1)).unwrap_or(i64::MAX),
    );
    metrics.set("idle_own_sources", describe_sources(census.own.last()));
    metrics.set("idle_seen_sources", describe_sources(census.seen.last()));
    metrics.set(
        "idle_own_order",
        describe_order(census.own.last(), actor, &census.own, None),
    );
    metrics.set(
        "idle_seen_order",
        describe_order(census.seen.last(), actor, &census.seen, None),
    );
    metrics.set(
        "idle_seen_is_own",
        census.own.last().is_some() && census.own.last() == census.seen.last(),
    );
    metrics.set(
        "idle_attachments_named",
        census.own.last().is_some_and(|set| {
            set.iter().any(|entry| {
                entry
                    .source_id
                    .is_some_and(|source| census.own.attachments.contains(&source.uuid()))
            })
        }),
    );

    viewer_legs(&mut census, metrics, one_shot).await?;
    metrics.set("physical_events_own", describe_physical(&census.own));
    metrics.set("physical_events_seen", describe_physical(&census.seen));
    if grid.is_fake() {
        // Nothing runs a script here.
        return Ok(());
    }
    script_legs(&mut census, trash, residue, metrics, &standing).await
}

/// Legs 2 to 7: everything A asks of the grid itself.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of legs, each a request and a window to hear its answer in"
)]
async fn viewer_legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    one_shot: Duration,
) -> Result<(), TestFailure> {
    let window = census.window;
    let clap = builtin(CLAP)?;
    let laugh = builtin(LAUGH)?;
    let dance = builtin(DANCE)?;
    let stand = builtin("stand")?;
    let walk = builtin("walk")?;
    let typing = builtin("type")?;
    let away = builtin("away")?;
    let key = AnimationKey::from;

    // --- 2. Play and stop, each twice. ---
    census
        .start_leg(
            metrics,
            "play",
            vec![Command::PlayAnimation(key(clap))],
            &[clap],
        )
        .await?;
    census
        .start_leg(
            metrics,
            "play_again",
            vec![Command::PlayAnimation(key(clap))],
            &[clap],
        )
        .await?;
    census
        .leg(
            metrics,
            "stop",
            vec![Command::StopAnimation(key(clap))],
            window,
            &[clap],
        )
        .await?;
    census
        .leg(
            metrics,
            "stop_again",
            vec![Command::StopAnimation(key(clap))],
            window,
            &[clap],
        )
        .await?;

    // --- 3. Several in one message. ---
    census
        .start_leg(
            metrics,
            "play_two",
            vec![Command::SetAnimations(vec![
                (key(clap), true),
                (key(laugh), true),
            ])],
            &[clap, laugh],
        )
        .await?;
    metrics.set(
        "play_two_order",
        describe_order(census.own.last(), census.actor, &census.own, None),
    );
    census
        .start_leg(
            metrics,
            "swap",
            vec![Command::SetAnimations(vec![
                (key(clap), false),
                (key(laugh), false),
                (key(dance), true),
            ])],
            &[clap, laugh, dance],
        )
        .await?;
    census
        .leg(
            metrics,
            "stop_dance",
            vec![Command::StopAnimation(key(dance))],
            window,
            &[clap, laugh, dance],
        )
        .await?;

    // --- 4. An animation nobody has. ---
    census
        .start_leg(
            metrics,
            "play_made_up",
            vec![Command::PlayAnimation(key(MADE_UP))],
            &[MADE_UP],
        )
        .await?;
    census
        .leg(
            metrics,
            "stop_made_up",
            vec![Command::StopAnimation(key(MADE_UP))],
            window,
            &[MADE_UP],
        )
        .await?;

    // --- 5. A one-shot left playing. ---
    census
        .leg(
            metrics,
            "one_shot_left",
            vec![Command::PlayAnimation(key(clap))],
            one_shot,
            &[clap],
        )
        .await?;
    census
        .leg(
            metrics,
            "one_shot_stopped",
            vec![Command::StopAnimation(key(clap))],
            window,
            &[clap],
        )
        .await?;

    // --- 6. The grid's own animations. ---
    census
        .leg(
            metrics,
            "stop_stand",
            vec![Command::StopAnimation(key(stand))],
            window,
            &[stand],
        )
        .await?;
    census
        .leg(
            metrics,
            "play_stand",
            vec![Command::PlayAnimation(key(stand))],
            window,
            &[stand],
        )
        .await?;
    census
        .leg(
            metrics,
            "play_walk",
            vec![Command::PlayAnimation(key(walk))],
            window,
            &[walk, stand],
        )
        .await?;
    census
        .leg(
            metrics,
            "stop_walk",
            vec![Command::StopAnimation(key(walk))],
            window,
            &[walk, stand],
        )
        .await?;
    census
        .leg(
            metrics,
            "typing_on",
            vec![Command::Typing(true)],
            window,
            &[typing, stand],
        )
        .await?;
    census
        .leg(
            metrics,
            "typing_off",
            vec![Command::Typing(false)],
            window,
            &[typing, stand],
        )
        .await?;
    census
        .leg(
            metrics,
            "away_on",
            vec![Command::SetControls(ControlFlags::AWAY)],
            window,
            &[typing, stand],
        )
        .await?;
    census
        .leg(
            metrics,
            "away_off",
            vec![Command::SetControls(ControlFlags::empty())],
            window,
            &[away, stand],
        )
        .await?;

    // --- 7. The reset. ---
    // With an animation of A's own playing, so the reset has something to
    // stop if it stops anything.
    census
        .leg(
            metrics,
            "before_reset",
            vec![Command::PlayAnimation(key(dance))],
            window,
            &[dance],
        )
        .await?;
    census
        .leg(
            metrics,
            "reset",
            vec![Command::SetAnimations(vec![(key(Uuid::nil()), false)])],
            window,
            &[dance, stand],
        )
        .await?;
    census
        .leg(
            metrics,
            "after_reset",
            vec![Command::StopAnimation(key(dance))],
            window,
            &[dance, stand],
        )
        .await
}

/// Leg 8: a script's animation, on a live grid.
async fn script_legs(
    census: &mut Census<'_>,
    trash: InventoryFolderKey,
    residue: &mut Residue,
    metrics: &mut Metrics,
    standing: &Vector,
) -> Result<(), TestFailure> {
    let dance = builtin(SCRIPT_DANCE)?;
    let actor = census.actor;
    // The cube, beside A.
    let known = census.own.added.len();
    let spot = Vector {
        x: standing.x + CUBE_AWAY_M,
        y: standing.y,
        z: standing.z,
    };
    census
        .a
        .send(Command::RezObject {
            shape: PrimShape::cube(spot.clone()),
            group_id: None,
        })
        .await?;
    listen_until(census.a, &mut census.own, actor, STEP, |view| {
        view.added
            .iter()
            .skip(known)
            .any(|object| stands_near(object, &spot))
    })
    .await?;
    let cube = census
        .own
        .added
        .iter()
        .skip(known)
        .find(|object| stands_near(object, &spot))
        .cloned()
        .ok_or_else(|| TestFailure::Assertion("no cube appeared after RezObject".to_owned()))?;
    residue.roots.push(cube.scoped_id());
    census.cube = Some(cube.clone());
    install(census, &cube).await?;

    // The permission, granted.
    let asked_before = census.own.asked.len();
    census.order(metrics, "script_ask", "ask", &[dance]).await?;
    listen_until(census.a, &mut census.own, actor, STEP, |view| {
        view.asked.len() > asked_before
    })
    .await?;
    let request = census.own.asked.get(asked_before).cloned().ok_or_else(|| {
        TestFailure::Assertion("the probe's permission request never arrived".to_owned())
    })?;
    metrics.set("script_ask_permissions", i64::from(request.permissions.0));
    census
        .a
        .send(Command::AnswerScriptPermissions {
            task_id: request.task_id,
            item_id: request.item_id,
            permissions: ScriptPermissions(ScriptPermissions::TRIGGER_ANIMATION),
            experience_id: request.experience_id,
        })
        .await?;
    let granted = format!("P|{}", ScriptPermissions::TRIGGER_ANIMATION);
    listen_until(census.a, &mut census.own, actor, STEP, |view| {
        view.said(&granted)
    })
    .await?;
    if !census.own.said(&granted) {
        return Err(TestFailure::Assertion(
            "the probe never said it was granted the permission".to_owned(),
        ));
    }

    census
        .order(metrics, "script_start", "start", &[dance])
        .await?;
    census.record_sequence(metrics, "script_start", dance);
    let cube_id = census.cube_id();
    metrics.set("script_start_sources", describe_sources(census.own.last()));
    metrics.set(
        "script_start_seen_sources",
        describe_sources(census.seen.last()),
    );
    metrics.set(
        "script_start_order",
        describe_order(census.own.last(), actor, &census.own, cube_id),
    );
    metrics.set(
        "script_start_seen_order",
        describe_order(census.seen.last(), actor, &census.seen, cube_id),
    );
    census
        .order(metrics, "script_stop", "stop", &[dance])
        .await?;
    census
        .order(metrics, "script_restart", "start", &[dance])
        .await?;
    let window = census.window;
    census
        .leg(
            metrics,
            "script_stopped_by_agent",
            vec![Command::StopAnimation(AnimationKey::from(dance))],
            window,
            &[dance],
        )
        .await?;
    census
        .order(metrics, "script_start_again", "start", &[dance])
        .await?;

    // The cube deleted while its animation plays.
    let from = census.mark();
    clean_up(census.a, residue, trash).await?;
    residue.roots.clear();
    census.listen(window).await?;
    census.record(metrics, "cube_deleted", from, &[dance]);
    // Whatever the grid left playing, stopped.
    census
        .a
        .send(Command::StopAnimation(AnimationKey::from(dance)))
        .await?;
    census.listen(window).await
}

/// Whether `object` stands within [`CUBE_FOUND_WITHIN_M`] of `spot`,
/// horizontally.
fn stands_near(object: &Object, spot: &Vector) -> bool {
    let (dx, dy) = (
        object.motion.position.x - spot.x,
        object.motion.position.y - spot.y,
    );
    dx.mul_add(dx, dy * dy) <= CUBE_FOUND_WITHIN_M * CUBE_FOUND_WITHIN_M
}

/// Makes a new script in `cube`, saves the probe over it and waits for it to
/// say it runs.
async fn install(census: &mut Census<'_>, cube: &Object) -> Result<(), TestFailure> {
    let target: ScopedObjectId = cube.scoped_id();
    let task = cube.full_id;
    let actor = census.actor;
    census
        .a
        .send(Command::RezScript {
            target,
            params: Box::new(RezScriptParams {
                group_id: None,
                enabled: true,
                item: RestoreItem::new_script(
                    AgentKey::from(actor),
                    task,
                    PROBE_NAME,
                    Uuid::new_v4(),
                ),
            }),
        })
        .await?;
    let started = Instant::now();
    let item = loop {
        listen(census.a, &mut census.own, actor, Duration::from_millis(500)).await?;
        let held = listing(census.a, target, task).await?;
        if let Some(item) = held.items.iter().find(|item| item.name == PROBE_NAME) {
            break item.item_id;
        }
        if started.elapsed() >= STEP {
            return Err(TestFailure::Assertion(
                "the new script never appeared in the cube".to_owned(),
            ));
        }
    };
    census
        .a
        .send(Command::UploadScript {
            location: ScriptUploadLocation::TaskInventory {
                task_id: task,
                item_id: item,
                running: true,
                experience: None,
            },
            target: ScriptTarget::Mono,
            source: probe_source().into_bytes(),
        })
        .await?;
    // The script may speak before the capability's answer is read, so one
    // window hears both.
    census.own.uploaded = None;
    listen_until(census.a, &mut census.own, actor, STEP, |view| {
        view.uploaded.is_some() && view.said("R")
    })
    .await?;
    match (&census.own.uploaded, census.own.said("R")) {
        (Some(Ok(true)), true) => Ok(()),
        (Some(Ok(true)), false) => Err(TestFailure::Assertion(
            "the probe script never said it runs".to_owned(),
        )),
        (Some(Ok(false)), _) => Err(TestFailure::Assertion(
            "the probe script did not compile".to_owned(),
        )),
        (Some(Err(reason)), _) => Err(TestFailure::Assertion(format!(
            "saving the probe script failed: {reason}"
        ))),
        (None, _) => Err(TestFailure::Assertion(
            "saving the probe script was never answered".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{MADE_UP, View, changes, describe_set, probe_source, told_since};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{ObjectKey, PlayingAnimation, Uuid};

    /// One entry of a statement.
    fn entry(anim_id: Uuid, sequence_id: i32, source: Option<Uuid>) -> PlayingAnimation {
        PlayingAnimation {
            anim_id,
            sequence_id,
            source_id: source.map(ObjectKey::from),
        }
    }

    /// The id of the built-in `name`.
    fn id(name: &str) -> Result<Uuid, String> {
        sl_anim::builtin_animation_by_name(name)
            .map(|animation| animation.id)
            .ok_or_else(|| format!("{name} is no built-in"))
    }

    #[test]
    fn the_probe_compiles() -> Result<(), String> {
        let source = probe_source();
        sl_lsl_runtime::compile(&source)
            .map(|_program| ())
            .map_err(|errors| format!("the probe does not compile: {errors:?}\n{source}"))
    }

    #[test]
    fn changes_name_starts_stops_and_restarts() -> Result<(), String> {
        let (stand, clap) = (id("stand")?, id("clap")?);
        let asset = Uuid::from_u128(7);
        let before = vec![entry(stand, 1, None), entry(asset, 2, None)];
        let after = vec![entry(stand, 1, None), entry(clap, 3, None)];
        assert_eq!(changes(&before, &after, &[clap]), vec!["+clap".to_owned()]);
        let restarted = vec![entry(stand, 1, None), entry(clap, 4, None)];
        assert_eq!(
            changes(&after, &restarted, &[clap]),
            vec!["~clap".to_owned()]
        );
        assert_eq!(
            changes(&restarted, &before, &[clap]),
            vec!["-clap".to_owned()]
        );
        // An animation the leg is not about is not looked at.
        assert!(changes(&before, &after, &[stand]).is_empty());
        Ok(())
    }

    #[test]
    fn a_set_is_described_by_name_and_source() -> Result<(), String> {
        let actor = Uuid::from_u128(1);
        let worn = Uuid::from_u128(2);
        let mut view = View::default();
        let _new = view.attachments.insert(worn);
        let set = vec![
            entry(id("stand")?, 1, Some(actor)),
            entry(Uuid::from_u128(9), 2, Some(worn)),
            entry(Uuid::from_u128(10), 3, Some(worn)),
            entry(MADE_UP, 4, None),
        ];
        assert_eq!(
            describe_set(Some(&set), actor, &view, None),
            "asset@attachment×2+made-up+stand@self"
        );
        assert_eq!(describe_set(None, actor, &view, None), "none");
        Ok(())
    }

    #[test]
    fn what_a_viewer_was_told_since_a_mark() -> Result<(), String> {
        let (stand, clap) = (id("stand")?, id("clap")?);
        let mut view = View::default();
        view.statements.push(vec![entry(stand, 1, None)]);
        assert_eq!(told_since(&view, 1, &[clap]), ("nothing".to_owned(), false));
        view.statements.push(vec![entry(stand, 1, None)]);
        assert_eq!(told_since(&view, 1, &[clap]), ("nothing".to_owned(), true));
        view.statements
            .push(vec![entry(stand, 1, None), entry(clap, 2, None)]);
        assert_eq!(told_since(&view, 1, &[clap]), ("+clap".to_owned(), true));
        Ok(())
    }
}
