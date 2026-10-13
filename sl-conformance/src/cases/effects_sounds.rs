//! A census of viewer effects and sounds: which of them a region passes on,
//! to whom, how far, and under whose name.
//!
//! Two things a viewer sends are meant for the *other* viewers rather than
//! for the simulator: a `ViewerEffect` ([`Command::ViewerEffect`] — the gaze
//! and pointing hints, the editing beam, the spheres and spirals) and a
//! `SoundTrigger` ([`Command::TriggerSound`] — the sound of a gesture). A
//! region answers neither; what it does with them shows in what the avatar
//! standing beside the sender is sent. A script makes sounds too, and those
//! come in four messages (`SoundTrigger`, `AttachedSound`,
//! `AttachedSoundGainChange`, `PreloadSound`) and in the object's own update.
//!
//! The case has one avatar send and a second one watch, and asks one question
//! a leg:
//!
//! 1. **Every effect type** there is, and two there are not: which are
//!    passed on, and whether the sender is sent its own.
//! 2. **What is passed on of an effect**: several in one message, one that
//!    names another agent, one whose payload is not what its type says, odd
//!    durations, the same effect twice and twenty in a second.
//! 3. **A viewer's sound**: who is told, under whose name, and what becomes
//!    of a gain out of range, a sound that is none, a position far off or
//!    outside the region, and the handle of another region.
//! 4. **A script's sounds**: what each of the sound functions has the two
//!    avatars sent.
//! 5. **Distance**: the watcher climbs away in three steps, and the effects,
//!    the viewer's sound and the script's are asked again at each — and once
//!    with the watcher's camera left beside the sender.
//! 6. **An arrival**: the watcher logs in again beside a prim that is looping
//!    a sound, and what tells it so.
//! 7. **A parcel that bars avatar sounds**: the viewer's sound once more, on
//!    the build location's parcel where the fixtures name another spot for
//!    the rest (`sound_location`), or on the sender's own land with the
//!    switch turned off for the moment. The second is recorded and not held:
//!    it is asked only of a run made as the land's owner.
//!
//! The script is the instrument of legs 4 to 6: it does what its owner says
//! on a channel and says so. A grid that runs no scripts and moves no avatar
//! answers them with nothing, so only legs 1 to 3 are asked of the fake grid.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/avatars.md` § Effects and sounds; every run is held to
//! it.
//!
//! `2av`, `[both]`, the first three legs offline on both fake flavours. The
//! cube is derezzed to the Trash, also when a leg fails. On Second Life the
//! whole of it wants a parcel that lets everybody's avatar sounds play — a
//! sandbox usually does not, and the grid then passes no viewer's sound on.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AssetKey, Camera, ChatChannel, ChatType, Command, ControlFlags, Event,
    GlobalCoordinates, LookAtType, Object, ObjectKey, ObjectUpdateForm, ParcelInfo, PointAtType,
    RegionCoordinates, RegionHandle, RegionLocalObjectId, RestoreItem, RezScriptParams,
    ScriptTarget, ScriptUploadLocation, SoundFlags, SoundPreload, UI_SOUND_CLICK, Uuid, Vector,
    ViewerEffect, ViewerEffectData, ViewerEffectType,
};

use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up,
    rez_cube,
};
use crate::cases::task_inventory::{
    DRAIN, FAKE_DRAIN, find_object, hold_to_measured, listen, listing, login_folders,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, settle_scene_with_avatar, walk_within_region};

/// Where both avatars log in on OpenSim: the flattened square with open sky
/// above it. The middle of the "Default Region" is roofed, and a climb from
/// there goes nowhere.
const OPENSIM_START: &str = "uri:Northeast Region&70&180&26";

/// What every line the probe says begins with.
const PROBE_TAG: &str = "SLCES";

/// The name of the probe script in the cube.
const PROBE_NAME: &str = "SLClientSoundProbe";

/// The channel the probe takes its orders on.
const PROBE_CHANNEL: i32 = 7_734_291;

/// The gain the probe plays at.
const PROBE_GAIN: f32 = 0.5;

/// The time between two messages of one gesture.
const BETWEEN: Duration = Duration::from_millis(150);

/// How many effects the burst sends, in about a second.
const EFFECT_BURST: u8 = 20;

/// How many sounds the burst sends, in about a second.
const SOUND_BURST: u8 = 10;

/// How far the cube stands from its owner, in metres.
const CUBE_AWAY_M: f32 = 2.0;

/// How far above the sender a sound is stated, step by step, in metres.
const SOUND_HEIGHTS_M: [u8; 4] = [15, 40, 90, 110];

/// The sequence id of the census's parcel requests.
const PARCEL_SEQUENCE: i32 = 7_700;

/// How long a parcel edit is given to take effect.
const PARCEL_EDIT_SETTLE: Duration = Duration::from_secs(2);

/// What a leg that was not asked of a grid records.
const NOT_ASKED: &str = "not asked";

/// How far above the sender a sound is stated that is out of everybody's
/// earshot, in metres.
const FAR_ABOVE_M: f32 = 150.0;

/// How far apart the two avatars may stand at the start, in metres.
const BESIDE_M: f32 = 5.0;

/// The speed a climb is planned at, in metres a second: what Second Life
/// lifts a flying avatar at.
const CLIMB_M_PER_S: f32 = 16.0;

/// How long the watcher's climb to one step may take.
const CLIMB_LIMIT: Duration = Duration::from_secs(120);

/// How long the watcher's way down may take.
const DESCENT_LIMIT: Duration = Duration::from_secs(180);

/// How long the watcher hovers after a push before its position is read.
const HOVER: Duration = Duration::from_secs(3);

/// How far above the ground the watcher counts as landed, in metres.
const LANDED_M: f32 = 2.5;

/// How long an arrival is listened to.
const ARRIVAL: Duration = Duration::from_secs(30);

/// The steps of the watcher's climb: the least it has to be above the sender
/// at each, in metres, and the word the step's metrics begin with. The first
/// is past the ten metres OpenSim passes a gaze within, the second past a
/// say's twenty, the third past the hundred metres a sound carries there.
const STEPS: [(f32, &str); 3] = [(12.0, "nearby"), (35.0, "further"), (120.0, "far")];

/// Where the measurements below are written down.
const SOURCE: &str =
    "book/src/gridspec/avatars.md § Effects and sounds (effects-sounds, 2026-10-13)";

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

/// What each grid was measured to do with a viewer's effects and sounds, by
/// the metric the census records it under: every run, live or fake, is held
/// to it.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "effect_animal_controls_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_animal_controls_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_batch_of_three_told_other",
        both(Answer::Text("messages of 3")),
    ),
    (
        "effect_batch_of_three_told_sender",
        each(Answer::Text("messages of 3"), Answer::Text("nothing")),
    ),
    (
        "effect_beam_of_255_bytes_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    (
        "effect_beam_of_no_bytes_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    ("effect_beam_told_other", both(Answer::Text("as sent"))),
    (
        "effect_beam_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_beam_with_the_other_as_its_source_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    (
        "effect_beam_with_the_other_as_its_source_told_sender",
        both(Answer::Text("nothing")),
    ),
    ("effect_blob_told_other", both(Answer::Text("as sent"))),
    (
        "effect_blob_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    ("effect_burst_of_twenty_passed_on", both(Answer::Count(20))),
    ("effect_cloth_told_other", both(Answer::Text("as sent"))),
    (
        "effect_cloth_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    ("effect_connector_told_other", both(Answer::Text("as sent"))),
    (
        "effect_connector_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    ("effect_edit_told_other", both(Answer::Text("as sent"))),
    (
        "effect_edit_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_flexible_object_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_flexible_object_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    ("effect_glow_told_other", both(Answer::Text("as sent"))),
    (
        "effect_glow_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    ("effect_icon_told_other", both(Answer::Text("as sent"))),
    (
        "effect_icon_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_lasting_a_billion_seconds_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_lasting_minus_five_seconds_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_lasting_no_time_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_local_animation_object_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_local_animation_object_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_look_at_of_no_bytes_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    (
        "effect_look_at_of_one_byte_more_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_look_at_of_three_bytes_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    ("effect_look_at_told_other", both(Answer::Text("as sent"))),
    (
        "effect_look_at_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_look_at_with_the_other_as_its_source_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    (
        "effect_look_at_with_the_other_as_its_source_told_sender",
        both(Answer::Text("nothing")),
    ),
    ("effect_name_tag_told_other", both(Answer::Text("as sent"))),
    (
        "effect_name_tag_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_naming_a_stranger_told_other",
        each(
            Answer::Text("with another agent (the sender)"),
            Answer::Text("as sent"),
        ),
    ),
    (
        "effect_naming_a_stranger_told_sender",
        each(
            Answer::Text("with another agent (the sender)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "effect_naming_nobody_told_other",
        each(
            Answer::Text("with another agent (the sender)"),
            Answer::Text("as sent"),
        ),
    ),
    (
        "effect_naming_nobody_told_sender",
        each(
            Answer::Text("with another agent (the sender)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "effect_naming_the_other_avatar_told_other",
        each(
            Answer::Text("with another agent (the sender)"),
            Answer::Text("as sent"),
        ),
    ),
    (
        "effect_naming_the_other_avatar_told_sender",
        each(
            Answer::Text("with another agent (the sender)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "effect_point_at_of_no_bytes_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    (
        "effect_point_at_of_three_bytes_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    ("effect_point_at_told_other", both(Answer::Text("as sent"))),
    (
        "effect_point_at_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_point_at_with_the_other_as_its_source_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    (
        "effect_point_at_with_the_other_as_its_source_told_sender",
        both(Answer::Text("nothing")),
    ),
    ("effect_point_told_other", both(Answer::Text("as sent"))),
    (
        "effect_point_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_reset_skeleton_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_reset_skeleton_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_sent_twice_told_other",
        both(Answer::Text("2 times, as sent")),
    ),
    (
        "effect_sphere_of_255_bytes_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_sphere_of_no_bytes_told_other",
        both(Answer::Text("as sent")),
    ),
    ("effect_sphere_told_other", both(Answer::Text("as sent"))),
    (
        "effect_sphere_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    ("effect_spiral_told_other", both(Answer::Text("as sent"))),
    (
        "effect_spiral_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_text_told_other",
        each(Answer::Text("nothing"), Answer::Text("as sent")),
    ),
    ("effect_text_told_sender", both(Answer::Text("nothing"))),
    ("effect_trail_told_other", both(Answer::Text("as sent"))),
    (
        "effect_trail_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    ("effect_type_20_told_other", both(Answer::Text("as sent"))),
    (
        "effect_type_20_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    ("effect_type_255_told_other", both(Answer::Text("as sent"))),
    (
        "effect_type_255_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "effect_voice_visualizer_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "effect_voice_visualizer_told_sender",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    ("parcel_avatar_sounds", both(Answer::Text("everyone's"))),
    (
        "sound_110m_above_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "sound_110m_above_told_sender",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "sound_15m_above_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_15m_above_told_sender",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_40m_above_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_40m_above_told_sender",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_90m_above_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_90m_above_told_sender",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_at_gain_0_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 0.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 0.00",
            ),
        ),
    ),
    (
        "sound_at_gain_2_5_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 2.50",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 2.50",
            ),
        ),
    ),
    (
        "sound_at_gain_minus_1_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain -1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain -1.00",
            ),
        ),
    ),
    ("sound_burst_of_ten_passed_on", both(Answer::Count(10))),
    (
        "sound_far_above_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "sound_far_above_told_sender",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "sound_in_another_region_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, the region asked for, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, the region asked for, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_in_another_region_told_sender",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, the region asked for, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, the region asked for, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_nobody_has_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_outside_the_region_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "sound_outside_the_region_told_sender",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "sound_that_is_none_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "sound_told_sender",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
];

/// What each live grid was measured to answer where a script or a moving
/// avatar is needed to ask.
const MEASURED_LIVE: &[(&str, Measured<Answer>)] = &[
    (
        "arrival_beside_a_looping_prim_told",
        each(
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 0.00",
            ),
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 100.00",
            ),
        ),
    ),
    (
        "barred_parcel_avatar_sounds",
        each(Answer::Text("the group's only"), Answer::Text("not asked")),
    ),
    (
        "far_effect_beam_told_other",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "far_effect_look_at_told_other",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "far_effect_point_at_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "far_effect_sphere_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "far_script_play_told_other",
        both(Answer::Text(
            "attached(the sound, owner the sender, gain 0.50, flags none)",
        )),
    ),
    (
        "far_script_preload_told_other",
        each(
            Answer::Text("preload(the sound, owner the sender)"),
            Answer::Text("preload(the sound, owner the prim)"),
        ),
    ),
    (
        "far_script_trigger_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "far_sound_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "further_effect_beam_told_other",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "further_effect_look_at_told_other",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "further_effect_point_at_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "further_effect_sphere_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "further_script_play_told_other",
        both(Answer::Text(
            "attached(the sound, owner the sender, gain 0.50, flags none)",
        )),
    ),
    (
        "further_script_preload_told_other",
        each(
            Answer::Text("preload(the sound, owner the sender)"),
            Answer::Text("preload(the sound, owner the prim)"),
        ),
    ),
    (
        "further_script_trigger_told_other",
        each(
            Answer::Text(
                "trigger(owner the sender, object the prim, parent nobody, gain 0.50, at the prim)",
            ),
            Answer::Text(
                "trigger(owner the sender, object the prim, parent the prim, gain 0.50, at the prim)",
            ),
        ),
    ),
    (
        "further_sound_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "nearby_effect_beam_told_other",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "nearby_effect_look_at_told_other",
        each(Answer::Text("as sent"), Answer::Text("nothing")),
    ),
    (
        "nearby_effect_point_at_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "nearby_effect_sphere_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "nearby_script_play_told_other",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags none) + update[full]: none, owner nobody, gain 0.00, flags none, radius 0.00",
            ),
            Answer::Text("attached(the sound, owner the sender, gain 0.50, flags none)"),
        ),
    ),
    (
        "nearby_script_preload_told_other",
        each(
            Answer::Text("preload(the sound, owner the sender)"),
            Answer::Text("preload(the sound, owner the prim)"),
        ),
    ),
    (
        "nearby_script_trigger_told_other",
        each(
            Answer::Text(
                "trigger(owner the sender, object the prim, parent nobody, gain 0.50, at the prim)",
            ),
            Answer::Text(
                "trigger(owner the sender, object the prim, parent the prim, gain 0.50, at the prim)",
            ),
        ),
    ),
    (
        "nearby_sound_told_other",
        each(
            Answer::Text(
                "owner the sender, object the sender, parent nobody, this region, position as sent, gain 1.00",
            ),
            Answer::Text(
                "owner the sender, object the sender, parent the sender, this region, position as sent, gain 1.00",
            ),
        ),
    ),
    (
        "nearby_with_the_camera_beside_effect_beam_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "nearby_with_the_camera_beside_effect_look_at_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "nearby_with_the_camera_beside_effect_point_at_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "nearby_with_the_camera_beside_effect_sphere_told_other",
        both(Answer::Text("as sent")),
    ),
    (
        "script_adjust_volume_told_other",
        each(
            Answer::Text(
                "gain(0.25) + update[full]: the sound, owner the sender, gain 0.25, flags loop, radius 0.00",
            ),
            Answer::Text("gain(0.25)"),
        ),
    ),
    (
        "script_adjust_volume_told_owner",
        each(
            Answer::Text(
                "gain(0.25) + update[full]: the sound, owner the sender, gain 0.25, flags loop, radius 0.00",
            ),
            Answer::Text("gain(0.25)"),
        ),
    ),
    (
        "script_loop_master_told_other",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags loop|master) + update[full]: the sound, owner the sender, gain 0.50, flags loop|master, radius 0.00",
            ),
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop|master, radius 100.00",
            ),
        ),
    ),
    (
        "script_loop_master_told_owner",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags loop|master) + update[full]: the sound, owner the sender, gain 0.50, flags loop|master, radius 0.00",
            ),
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop|master, radius 100.00",
            ),
        ),
    ),
    (
        "script_loop_slave_told_other",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags loop|slave) + update[full]: the sound, owner the sender, gain 0.50, flags loop|slave, radius 0.00",
            ),
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop|slave, radius 100.00",
            ),
        ),
    ),
    (
        "script_loop_slave_told_owner",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags loop|slave) + update[full]: the sound, owner the sender, gain 0.50, flags loop|slave, radius 0.00",
            ),
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop|slave, radius 100.00",
            ),
        ),
    ),
    (
        "script_loop_told_other",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags loop) + update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 0.00",
            ),
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 100.00",
            ),
        ),
    ),
    (
        "script_loop_told_owner",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags loop) + update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 0.00",
            ),
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 100.00",
            ),
        ),
    ),
    (
        "script_loop_with_a_radius_told_other",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags loop) + update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 5.00",
            ),
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 5.00",
            ),
        ),
    ),
    (
        "script_loop_with_a_radius_told_owner",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags loop) + update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 5.00",
            ),
            Answer::Text(
                "update[full]: the sound, owner the sender, gain 0.50, flags loop, radius 5.00",
            ),
        ),
    ),
    (
        "script_play_slave_told_other",
        each(
            Answer::Text("attached(the sound, owner the sender, gain 0.50, flags slave|pending)"),
            Answer::Text("attached(the sound, owner the sender, gain 0.50, flags slave)"),
        ),
    ),
    (
        "script_play_slave_told_owner",
        each(
            Answer::Text("attached(the sound, owner the sender, gain 0.50, flags slave|pending)"),
            Answer::Text("attached(the sound, owner the sender, gain 0.50, flags slave)"),
        ),
    ),
    (
        "script_play_told_other",
        both(Answer::Text(
            "attached(the sound, owner the sender, gain 0.50, flags none)",
        )),
    ),
    (
        "script_play_told_owner",
        both(Answer::Text(
            "attached(the sound, owner the sender, gain 0.50, flags none)",
        )),
    ),
    (
        "script_preload_told_other",
        each(
            Answer::Text("preload(the sound, owner the sender)"),
            Answer::Text("preload(the sound, owner the prim)"),
        ),
    ),
    (
        "script_preload_told_owner",
        each(
            Answer::Text("preload(the sound, owner the sender)"),
            Answer::Text("preload(the sound, owner the prim)"),
        ),
    ),
    (
        "script_queued_play_told_other",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags queue) + update[full]: none, owner nobody, gain 0.00, flags queue, radius 0.00",
            ),
            Answer::Text("attached(the sound, owner the sender, gain 0.50, flags queue)"),
        ),
    ),
    (
        "script_queued_play_told_owner",
        each(
            Answer::Text(
                "attached(the sound, owner the sender, gain 0.50, flags queue) + update[full]: none, owner nobody, gain 0.00, flags queue, radius 0.00",
            ),
            Answer::Text("attached(the sound, owner the sender, gain 0.50, flags queue)"),
        ),
    ),
    (
        "script_queueing_on_told_other",
        each(
            Answer::Text(
                "update[full]: none, owner nobody, gain 0.00, flags queue|stop, radius 0.00",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "script_queueing_on_told_owner",
        each(
            Answer::Text(
                "update[full]: none, owner nobody, gain 0.00, flags queue|stop, radius 0.00",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "script_radius_set_told_other",
        each(
            Answer::Text("update[full]: none, owner nobody, gain 0.00, flags none, radius 5.00"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "script_radius_set_told_owner",
        each(
            Answer::Text("update[full]: none, owner nobody, gain 0.00, flags none, radius 5.00"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "script_stop_told_other",
        each(
            Answer::Text(
                "attached(none, owner the sender, gain 0.00, flags stop) + update[full]: none, owner nobody, gain 0.00, flags stop, radius 0.00",
            ),
            Answer::Text(
                "update[full]: none, owner the sender, gain 0.00, flags stop, radius 100.00",
            ),
        ),
    ),
    (
        "script_stop_told_owner",
        each(
            Answer::Text(
                "attached(none, owner the sender, gain 0.00, flags stop) + update[full]: none, owner nobody, gain 0.00, flags stop, radius 0.00",
            ),
            Answer::Text(
                "update[full]: none, owner the sender, gain 0.00, flags stop, radius 100.00",
            ),
        ),
    ),
    (
        "script_trigger_limited_around_told_other",
        each(
            Answer::Text(
                "trigger(owner the sender, object the prim, parent nobody, gain 0.50, at the prim)",
            ),
            Answer::Text(
                "trigger(owner the sender, object the prim, parent the prim, gain 0.49, at the prim)",
            ),
        ),
    ),
    (
        "script_trigger_limited_around_told_owner",
        each(
            Answer::Text(
                "trigger(owner the sender, object the prim, parent nobody, gain 0.50, at the prim)",
            ),
            Answer::Text(
                "trigger(owner the sender, object the prim, parent the prim, gain 0.49, at the prim)",
            ),
        ),
    ),
    (
        "script_trigger_limited_elsewhere_told_other",
        both(Answer::Text("nothing")),
    ),
    (
        "script_trigger_limited_elsewhere_told_owner",
        both(Answer::Text("nothing")),
    ),
    (
        "script_trigger_told_other",
        each(
            Answer::Text(
                "trigger(owner the sender, object the prim, parent nobody, gain 0.50, at the prim)",
            ),
            Answer::Text(
                "trigger(owner the sender, object the prim, parent the prim, gain 0.50, at the prim)",
            ),
        ),
    ),
    (
        "script_trigger_told_owner",
        each(
            Answer::Text(
                "trigger(owner the sender, object the prim, parent nobody, gain 0.50, at the prim)",
            ),
            Answer::Text(
                "trigger(owner the sender, object the prim, parent the prim, gain 0.50, at the prim)",
            ),
        ),
    ),
    (
        "sound_on_a_barred_parcel_told_other",
        each(Answer::Text("nothing"), Answer::Text("not asked")),
    ),
    (
        "sound_on_a_barred_parcel_told_sender",
        each(Answer::Text("nothing"), Answer::Text("not asked")),
    ),
];

/// Every effect type the census sends, by the word its metrics carry.
const EFFECT_TYPES: [(u8, &str); 22] = [
    (0, "text"),
    (1, "icon"),
    (2, "connector"),
    (3, "flexible_object"),
    (4, "animal_controls"),
    (5, "local_animation_object"),
    (6, "cloth"),
    (7, "beam"),
    (8, "glow"),
    (9, "point"),
    (10, "trail"),
    (11, "sphere"),
    (12, "spiral"),
    (13, "edit"),
    (14, "look_at"),
    (15, "point_at"),
    (16, "voice_visualizer"),
    (17, "name_tag"),
    (18, "blob"),
    (19, "reset_skeleton"),
    (20, "type_20"),
    (255, "type_255"),
];

/// The effects asked again at each step of the climb.
const RANGE_EFFECTS: [(u8, &str); 4] = [
    (14, "look_at"),
    (15, "point_at"),
    (7, "beam"),
    (11, "sphere"),
];

/// What the probe is told to do in the script leg: the order, and the word
/// its metrics carry — or none for an order that only sets up the next one.
const SCRIPT_LEGS: [(&str, Option<&str>); 20] = [
    ("trigger", Some("trigger")),
    ("play", Some("play")),
    ("preload", Some("preload")),
    ("loop", Some("loop")),
    ("adjust", Some("adjust_volume")),
    ("stop", Some("stop")),
    ("limited_around", Some("trigger_limited_around")),
    ("limited_elsewhere", Some("trigger_limited_elsewhere")),
    ("loop_master", Some("loop_master")),
    ("stop", None),
    ("loop_slave", Some("loop_slave")),
    ("stop", None),
    ("play_slave", Some("play_slave")),
    ("queue_on", Some("queueing_on")),
    ("play", Some("queued_play")),
    ("queue_off", None),
    ("radius_on", Some("radius_set")),
    ("loop", Some("loop_with_a_radius")),
    ("stop", None),
    ("radius_off", None),
];

/// The probe: a script that makes the sound its owner names on the probe's
/// channel, and says when it has.
fn probe_source(sound: Uuid) -> String {
    format!(
        r#"string S = "{sound}";
default
{{
    state_entry()
    {{
        llListen({PROBE_CHANNEL}, "", llGetOwner(), "");
        llOwnerSay("{PROBE_TAG}|R");
    }}
    listen(integer channel, string name, key id, string what)
    {{
        vector at = llGetPos();
        if (what == "trigger") llTriggerSound(S, {PROBE_GAIN});
        else if (what == "play") llPlaySound(S, {PROBE_GAIN});
        else if (what == "loop") llLoopSound(S, {PROBE_GAIN});
        else if (what == "adjust") llAdjustSoundVolume(0.25);
        else if (what == "stop") llStopSound();
        else if (what == "preload") llPreloadSound(S);
        else if (what == "limited_around")
            llTriggerSoundLimited(S, {PROBE_GAIN}, at + <10.0, 10.0, 10.0>, at - <10.0, 10.0, 10.0>);
        else if (what == "limited_elsewhere")
            llTriggerSoundLimited(S, {PROBE_GAIN}, at + <30.0, 30.0, 30.0>, at + <20.0, 20.0, 20.0>);
        else if (what == "loop_master") llLoopSoundMaster(S, {PROBE_GAIN});
        else if (what == "loop_slave") llLoopSoundSlave(S, {PROBE_GAIN});
        else if (what == "play_slave") llPlaySoundSlave(S, {PROBE_GAIN});
        else if (what == "queue_on") llSetSoundQueueing(TRUE);
        else if (what == "queue_off") llSetSoundQueueing(FALSE);
        else if (what == "radius_on") llSetSoundRadius(5.0);
        else if (what == "radius_off") llSetSoundRadius(0.0);
        llOwnerSay("{PROBE_TAG}|D|" + what);
    }}
}}
"#
    )
}

/// One thing a session was sent, in the order it came.
#[derive(Debug, Clone)]
enum Told {
    /// One `ViewerEffect` message.
    Effects(Vec<ViewerEffect>),
    /// A `SoundTrigger`.
    Trigger(Trigger),
    /// An `AttachedSound`.
    Attached {
        /// The sound.
        sound: Uuid,
        /// The object it is attached to.
        object: ObjectKey,
        /// The owner it states.
        owner: Uuid,
        /// The gain.
        gain: f32,
        /// The flags.
        flags: SoundFlags,
    },
    /// An `AttachedSoundGainChange`.
    Gain {
        /// The object.
        object: ObjectKey,
        /// The gain.
        gain: f32,
    },
    /// One entry of a `PreloadSound`.
    Preload(SoundPreload),
    /// An object as it read once an update had come.
    Object(Box<Object>),
    /// The form an update of an object came in.
    Form(RegionLocalObjectId, ObjectUpdateForm),
}

/// A `SoundTrigger` as it arrived.
#[derive(Debug, Clone)]
struct Trigger {
    /// The sound.
    sound: Uuid,
    /// The owner it states.
    owner: Uuid,
    /// The object it states.
    object: ObjectKey,
    /// The parent it states.
    parent: Option<ObjectKey>,
    /// The region it states.
    handle: RegionHandle,
    /// Where it states the sound is.
    position: Vector,
    /// The gain.
    gain: f32,
}

/// Whose avatar sounds a parcel lets play, as its record states it.
const fn avatar_sounds_text(parcel: Option<&ParcelInfo>) -> &'static str {
    match parcel {
        None => "no answer",
        Some(parcel) => match (parcel.any_av_sounds, parcel.group_av_sounds) {
            (Some(true), _) => "everyone's",
            (Some(false), Some(true)) => "the group's only",
            (Some(false), _) => "nobody's",
            (None, _) => "not stated",
        },
    }
}

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Got {
    /// The effects and sounds, and the updates of objects, in arrival order.
    told: Vec<Told>,
    /// Every line of chat with text.
    chats: Vec<String>,
    /// The text of every alert.
    alerts: Vec<String>,
    /// How saving a script ended, once it has.
    uploaded: Option<Result<bool, String>>,
    /// Where the session's own avatar was last said to be.
    own: Option<Vector>,
}

impl Got {
    /// Every effect that arrived, message by message.
    fn effect_blocks(&self) -> impl Iterator<Item = &ViewerEffect> {
        self.told
            .iter()
            .filter_map(|told| match told {
                Told::Effects(effects) => Some(effects.iter()),
                _ => None,
            })
            .flatten()
    }

    /// Whether the probe said `line`.
    fn said(&self, line: &str) -> bool {
        let wanted = format!("{PROBE_TAG}|{line}");
        self.chats.contains(&wanted)
    }
}

/// Listens on `session`, whose avatar is `agent`, until `done` says the leg
/// has what it came for and `quiet` more has passed, or for `limit` at most.
async fn watch_for(
    session: &mut Session,
    agent: Uuid,
    limit: Duration,
    quiet: Duration,
    mut done: impl FnMut(&Got) -> bool,
) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    let started = Instant::now();
    let mut done_at: Option<Instant> = None;
    loop {
        if done_at.is_none() && done(&got) {
            done_at = Some(Instant::now());
        }
        let remaining = done_at.map_or_else(
            || limit.saturating_sub(started.elapsed()),
            |at| quiet.saturating_sub(at.elapsed()),
        );
        if remaining.is_zero() {
            return Ok(got);
        }
        let outcome = session
            .wait_for(remaining, |event| {
                match event {
                    Event::ViewerEffect(effects) => got.told.push(Told::Effects(effects.clone())),
                    Event::SoundTrigger {
                        sound_id,
                        owner_id,
                        object_id,
                        parent_id,
                        region_handle,
                        position,
                        gain,
                    } => got.told.push(Told::Trigger(Trigger {
                        sound: *sound_id,
                        owner: *owner_id,
                        object: *object_id,
                        parent: *parent_id,
                        handle: *region_handle,
                        position: position.clone(),
                        gain: *gain,
                    })),
                    Event::AttachedSound {
                        sound_id,
                        object_id,
                        owner_id,
                        gain,
                        flags,
                    } => got.told.push(Told::Attached {
                        sound: *sound_id,
                        object: *object_id,
                        owner: *owner_id,
                        gain: *gain,
                        flags: *flags,
                    }),
                    Event::AttachedSoundGainChange { object_id, gain } => {
                        got.told.push(Told::Gain {
                            object: *object_id,
                            gain: *gain,
                        });
                    }
                    Event::PreloadSound { sounds } => {
                        got.told.extend(sounds.iter().copied().map(Told::Preload));
                    }
                    Event::ObjectAdded(object) | Event::ObjectUpdated(object) => {
                        if object.full_id.uuid() == agent {
                            got.own = Some(object.motion.position.clone());
                        } else {
                            got.told.push(Told::Object(object.clone()));
                        }
                    }
                    Event::ObjectStreamBatch(batch) if !batch.child => {
                        got.told.extend(
                            batch
                                .entries
                                .iter()
                                .map(|entry| Told::Form(entry.local_id, batch.form)),
                        );
                    }
                    Event::ChatReceived(chat) if !chat.message.is_empty() => {
                        got.chats.push(chat.message.clone());
                    }
                    Event::AlertMessage { message, .. }
                    | Event::AgentAlertMessage { message, .. } => got.alerts.push(message.clone()),
                    Event::ScriptUploaded { compiled, .. } => got.uploaded = Some(Ok(*compiled)),
                    Event::AssetUploadFailed { reason } => {
                        got.uploaded = Some(Err(reason.clone()));
                    }
                    _ => return None,
                }
                Some(())
            })
            .await;
        match outcome {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(got),
            Err(other) => return Err(other),
        }
    }
}

/// Listens on `session` for `window`.
async fn watch(session: &mut Session, agent: Uuid, window: Duration) -> Result<Got, TestFailure> {
    watch_for(session, agent, window, window, |_got| false).await
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

/// A vector as `<x,y,z>`, each to one place.
fn vector_text(vector: &Vector) -> String {
    format!("<{:.1},{:.1},{:.1}>", vector.x, vector.y, vector.z)
}

/// The distance between two points.
fn distance(left: &Vector, right: &Vector) -> f32 {
    let (dx, dy, dz) = (left.x - right.x, left.y - right.y, left.z - right.z);
    dz.mul_add(dz, dx.mul_add(dx, dy * dy)).sqrt()
}

/// The flags of an attached sound, by name.
fn flags_text(flags: u8) -> String {
    let names: Vec<&str> = [
        (SoundFlags::LOOP, "loop"),
        (SoundFlags::SYNC_MASTER, "master"),
        (SoundFlags::SYNC_SLAVE, "slave"),
        (SoundFlags::SYNC_PENDING, "pending"),
        (SoundFlags::QUEUE, "queue"),
        (SoundFlags::STOP, "stop"),
    ]
    .into_iter()
    .filter(|(bit, _name)| flags & bit != 0)
    .map(|(_bit, name)| name)
    .collect();
    let unnamed = flags & !0x3f;
    match (names.is_empty(), unnamed) {
        (true, 0) => "none".to_owned(),
        (_, 0) => names.join("|"),
        _ => format!("{}|{unnamed:#x}", names.join("|")),
    }
}

/// The ids a metric names by what they are rather than by their value.
#[derive(Debug, Clone, Copy)]
struct Names {
    /// The avatar that sends.
    sender: Uuid,
    /// The avatar that watches.
    other: Uuid,
    /// The cube, once there is one.
    cube: Option<Uuid>,
    /// The sound the probe makes.
    sound: Uuid,
    /// The region both stand in.
    home: RegionHandle,
}

impl Names {
    /// Whose id `id` is.
    fn who(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nobody"
        } else if id == self.sender {
            "the sender"
        } else if id == self.other {
            "the other avatar"
        } else if Some(id) == self.cube {
            "the prim"
        } else {
            "somebody else"
        }
    }

    /// Which sound `id` is.
    fn which_sound(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "none"
        } else if id == self.sound {
            "the sound"
        } else {
            "another sound"
        }
    }

    /// What `got` holds of the effect `sent`: `nothing`, `as sent`, or what
    /// of it came otherwise — and how often, where that is not once.
    fn relayed(&self, got: &Got, sent: &ViewerEffect) -> String {
        let mut renamed = false;
        let mut blocks: Vec<&ViewerEffect> = got
            .effect_blocks()
            .filter(|block| block.id == sent.id)
            .collect();
        if blocks.is_empty() {
            renamed = true;
            blocks = got
                .effect_blocks()
                .filter(|block| {
                    block.effect_type == sent.effect_type
                        && block.data.to_wire() == sent.data.to_wire()
                        && block.duration.to_bits() == sent.duration.to_bits()
                })
                .collect();
        }
        let Some(first) = blocks.first() else {
            return "nothing".to_owned();
        };
        let mut differs = Vec::new();
        if renamed {
            differs.push("id".to_owned());
        }
        if first.agent_id != sent.agent_id {
            differs.push(format!("agent ({})", self.who(first.agent_id.uuid())));
        }
        if first.effect_type != sent.effect_type {
            differs.push(format!("type ({})", first.effect_type.to_code()));
        }
        if first.duration.to_bits() != sent.duration.to_bits() {
            differs.push(format!("duration ({})", first.duration));
        }
        if first.color != sent.color {
            differs.push("colour".to_owned());
        }
        if first.data.to_wire() != sent.data.to_wire() {
            differs.push(format!("payload ({} bytes)", first.data.to_wire().len()));
        }
        let text = if differs.is_empty() {
            "as sent".to_owned()
        } else {
            format!("with another {}", differs.join(", "))
        };
        if blocks.len() == 1 {
            text
        } else {
            format!("{} times, {text}", blocks.len())
        }
    }

    /// One `SoundTrigger`, as a metric states it. `sent` is where the sound
    /// was asked for and in which region, for a viewer's own; a script's is
    /// measured against where its prim stands.
    fn trigger_text(&self, trigger: &Trigger, sent: &Vector, asked: RegionHandle) -> String {
        let region = if trigger.handle == self.home {
            "this region"
        } else if trigger.handle == asked {
            "the region asked for"
        } else {
            "some other region"
        };
        let position = if distance(&trigger.position, sent) < 0.05 {
            "as sent".to_owned()
        } else {
            vector_text(&trigger.position)
        };
        format!(
            "owner {}, object {}, parent {}, {region}, position {position}, gain {}",
            self.who(trigger.owner),
            self.who(trigger.object.uuid()),
            self.who(
                trigger
                    .parent
                    .map_or_else(Uuid::nil, |parent| parent.uuid())
            ),
            two_places(trigger.gain),
        )
    }

    /// What `got` holds of a viewer's sound `sound` asked for at `sent` in
    /// the region `asked`: `nothing`, or the trigger — and how often, where
    /// that is not once.
    fn heard(&self, got: &Got, sound: Uuid, sent: &Vector, asked: RegionHandle) -> String {
        let triggers: Vec<&Trigger> = got
            .told
            .iter()
            .filter_map(|told| match told {
                Told::Trigger(trigger) if trigger.sound == sound => Some(trigger),
                _ => None,
            })
            .collect();
        let Some(first) = triggers.first() else {
            return "nothing".to_owned();
        };
        let text = self.trigger_text(first, sent, asked);
        if triggers.len() == 1 {
            text
        } else {
            format!("{} times, {text}", triggers.len())
        }
    }

    /// Everything `got` holds of the sounds of the cube `prim`: the sound
    /// messages in the order they came, and what an update of the prim itself
    /// then said of its sound — or `nothing`.
    fn sounds(&self, got: &Got, prim: &Object) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut forms: Vec<String> = Vec::new();
        let mut state: Option<&Object> = None;
        for told in &got.told {
            let part = match told {
                Told::Trigger(trigger) if trigger.sound == self.sound => {
                    let place = if distance(&trigger.position, &prim.motion.position) < 0.5 {
                        "at the prim".to_owned()
                    } else {
                        format!("at {}", vector_text(&trigger.position))
                    };
                    let region = if trigger.handle == self.home {
                        ""
                    } else {
                        ", in another region"
                    };
                    format!(
                        "trigger(owner {}, object {}, parent {}, gain {}, {place}{region})",
                        self.who(trigger.owner),
                        self.who(trigger.object.uuid()),
                        self.who(
                            trigger
                                .parent
                                .map_or_else(Uuid::nil, |parent| parent.uuid())
                        ),
                        two_places(trigger.gain),
                    )
                }
                Told::Attached {
                    sound,
                    object,
                    owner,
                    gain,
                    flags,
                } if *object == prim.full_id => format!(
                    "attached({}, owner {}, gain {}, flags {})",
                    self.which_sound(*sound),
                    self.who(*owner),
                    two_places(*gain),
                    flags_text(flags.0),
                ),
                Told::Gain { object, gain } if *object == prim.full_id => {
                    format!("gain({})", two_places(*gain))
                }
                Told::Preload(preload) if preload.object_id == prim.full_id => format!(
                    "preload({}, owner {})",
                    self.which_sound(preload.sound_id),
                    self.who(preload.owner_id),
                ),
                Told::Object(object) if object.full_id == prim.full_id => {
                    state = Some(object);
                    continue;
                }
                Told::Form(object, form) if *object == prim.local_id => {
                    let word = format!("{form:?}").to_lowercase();
                    if !forms.contains(&word) {
                        forms.push(word);
                    }
                    continue;
                }
                _ => continue,
            };
            if parts.last() != Some(&part) {
                parts.push(part);
            }
        }
        if !forms.is_empty() {
            let said = if forms.iter().all(|form| form == "terse") {
                String::new()
            } else {
                state.map_or_else(String::new, |object| {
                    format!(
                        ": {}, owner {}, gain {}, flags {}, radius {}",
                        self.which_sound(object.sound),
                        self.who(object.owner_id),
                        two_places(object.gain),
                        flags_text(object.sound_flags),
                        two_places(object.sound_radius),
                    )
                })
            };
            parts.push(format!("update[{}]{said}", forms.join("+")));
        }
        if parts.is_empty() {
            "nothing".to_owned()
        } else {
            parts.join(" + ")
        }
    }
}

/// What a gesture of the sender's brought each avatar.
struct Brought {
    /// What the sender was sent.
    sender: Got,
    /// What the watcher was.
    other: Got,
}

/// Everything the legs share.
struct Census<'a> {
    /// The avatar that sends, and owns the cube.
    sender: &'a mut Session,
    /// The avatar that watches.
    other: &'a mut Session,
    /// The ids the metrics name.
    names: Names,
    /// How long the sender listens after a gesture.
    quiet: Duration,
    /// How long the queue of the watcher is then read for.
    drain: Duration,
    /// Where the sender stands.
    sender_at: Vector,
    /// Where the watcher was last said to be.
    other_at: Vector,
}

impl Census<'_> {
    /// A point of the region as a global one.
    fn global(&self, at: &Vector) -> GlobalCoordinates {
        let handle = self.names.home.0;
        let east = u32::try_from(handle >> 32).unwrap_or(0);
        let north = u32::try_from(handle & 0xffff_ffff).unwrap_or(0);
        GlobalCoordinates::new(
            f64::from(east) + f64::from(at.x),
            f64::from(north) + f64::from(at.y),
            f64::from(at.z),
        )
    }

    /// The payload a viewer sends with an effect of the type `code`: a gaze,
    /// a pointing, the beam family's, and four bytes for the rest.
    fn payload(&self, code: u8) -> ViewerEffectData {
        let sender = AgentKey::from(self.names.sender);
        let target_position = self.global(&self.other_at);
        match code {
            14 => ViewerEffectData::LookAt {
                source: Some(sender),
                target: None,
                target_position,
                look_at_type: LookAtType::Focus,
            },
            15 => ViewerEffectData::PointAt {
                source: Some(sender),
                target: None,
                target_position,
                point_at_type: PointAtType::Select,
            },
            7..=9 | 11..=13 => ViewerEffectData::Spiral {
                source: Some(ObjectKey::from(self.names.sender)),
                target: None,
                position: target_position,
            },
            _ => ViewerEffectData::Raw(vec![1, 2, 3, 4]),
        }
    }

    /// An effect of the type `code` as a viewer sends it, under a new id.
    fn effect(&self, code: u8) -> ViewerEffect {
        ViewerEffect {
            id: Uuid::new_v4(),
            agent_id: AgentKey::from(self.names.sender),
            effect_type: ViewerEffectType::from_code(code),
            duration: 0.5,
            color: [255, 255, 255, 255],
            data: self.payload(code),
        }
    }

    /// Has the sender send `commands`, a moment apart, and returns what the
    /// two avatars were then sent — once `done` says the sender has what the
    /// leg came for.
    async fn act_until(
        &mut self,
        commands: Vec<Command>,
        done: impl FnMut(&Got) -> bool,
    ) -> Result<Brought, TestFailure> {
        let several = commands.len() > 1;
        for command in commands {
            self.sender.send(command).await?;
            if several {
                tokio::time::sleep(BETWEEN).await;
            }
        }
        let sender = watch_for(
            self.sender,
            self.names.sender,
            STEP_TIMEOUT,
            self.quiet,
            done,
        )
        .await?;
        let other = watch(self.other, self.names.other, self.drain).await?;
        if let Some(at) = &other.own {
            self.other_at = at.clone();
        }
        Ok(Brought { sender, other })
    }

    /// Has the sender send `commands`, a moment apart, and returns what the
    /// two avatars were then sent.
    async fn act(&mut self, commands: Vec<Command>) -> Result<Brought, TestFailure> {
        self.act_until(commands, |_got| true).await
    }

    /// Sends `effects`, each in a message of its own, and records under
    /// `prefix` and each one's word what the two avatars were sent of it.
    async fn effects(
        &mut self,
        metrics: &mut Metrics,
        prefix: &str,
        effects: Vec<(&str, ViewerEffect)>,
        of_sender: bool,
    ) -> Result<(), TestFailure> {
        let commands = effects
            .iter()
            .map(|(_word, effect)| Command::ViewerEffect(vec![effect.clone()]))
            .collect();
        let brought = self.act(commands).await?;
        for (word, effect) in &effects {
            metrics.set(
                &format!("{prefix}effect_{word}_told_other"),
                self.names.relayed(&brought.other, effect),
            );
            if of_sender {
                metrics.set(
                    &format!("{prefix}effect_{word}_told_sender"),
                    self.names.relayed(&brought.sender, effect),
                );
            }
        }
        Ok(())
    }

    /// Has the sender trigger `sound` at `position` in the region `handle`
    /// and records under `key` what the two avatars were sent of it.
    async fn sound(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        (sound, gain): (Uuid, f32),
        handle: RegionHandle,
        position: &Vector,
        of_sender: bool,
    ) -> Result<(), TestFailure> {
        let brought = self
            .act(vec![Command::TriggerSound {
                sound: AssetKey::from(sound),
                gain,
                region_handle: handle,
                position: RegionCoordinates::new(position.x, position.y, position.z),
            }])
            .await?;
        metrics.set(
            &format!("{key}_told_other"),
            self.names.heard(&brought.other, sound, position, handle),
        );
        if of_sender {
            metrics.set(
                &format!("{key}_told_sender"),
                self.names.heard(&brought.sender, sound, position, handle),
            );
        }
        Ok(())
    }

    /// Tells the probe to do `order` and returns what the two avatars were
    /// sent by the time it said it had.
    async fn order(&mut self, order: &str) -> Result<Brought, TestFailure> {
        let done = format!("D|{order}");
        let brought = self
            .act_until(
                vec![Command::Chat {
                    message: order.to_owned(),
                    chat_type: ChatType::Normal,
                    channel: ChatChannel(PROBE_CHANNEL),
                }],
                |got| got.said(&done),
            )
            .await?;
        if brought.sender.said(&done) {
            Ok(brought)
        } else {
            Err(TestFailure::Assertion(format!(
                "the probe never said it had done `{order}`"
            )))
        }
    }

    /// Makes a new script in `prim`, saves the probe over it and waits for it
    /// to say it runs.
    async fn install(&mut self, prim: &Object) -> Result<(), TestFailure> {
        let target = prim.scoped_id();
        let task = prim.full_id;
        let owner = AgentKey::from(self.names.sender);
        self.sender
            .send(Command::RezScript {
                target,
                params: Box::new(RezScriptParams {
                    group_id: None,
                    enabled: true,
                    item: RestoreItem::new_script(owner, task, PROBE_NAME, Uuid::new_v4()),
                }),
            })
            .await?;
        let started = Instant::now();
        let item = loop {
            listen(self.sender, Duration::from_millis(500)).await?;
            let held = listing(self.sender, target, task).await?;
            if let Some(item) = held.items.iter().find(|item| item.name == PROBE_NAME) {
                break item.item_id;
            }
            if started.elapsed() >= STEP_TIMEOUT {
                return Err(TestFailure::Assertion(
                    "the new script never appeared in the cube".to_owned(),
                ));
            }
        };
        self.sender
            .send(Command::UploadScript {
                location: ScriptUploadLocation::TaskInventory {
                    task_id: task,
                    item_id: item,
                    running: true,
                    experience: None,
                },
                target: ScriptTarget::Mono,
                source: probe_source(self.names.sound).into_bytes(),
            })
            .await?;
        // The script may speak before the capability's answer is read, so one
        // window hears both.
        let got = watch_for(
            self.sender,
            self.names.sender,
            STEP_TIMEOUT,
            self.quiet,
            |got| got.uploaded.is_some() && got.said("R"),
        )
        .await?;
        watch(self.other, self.names.other, self.drain).await?;
        match (&got.uploaded, got.said("R")) {
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

    /// The record of the parcel the sender stands on, if the grid answers
    /// with one.
    async fn parcel_here(&mut self) -> Result<Option<ParcelInfo>, TestFailure> {
        let at = self.sender_at.clone();
        self.sender
            .send(Command::RequestParcelProperties {
                west: at.x - 0.5,
                south: at.y - 0.5,
                east: at.x + 0.5,
                north: at.y + 0.5,
                sequence_id: PARCEL_SEQUENCE,
                snap_selection: false,
            })
            .await?;
        let record = self
            .sender
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::ParcelProperties(parcel) if parcel.sequence_id == PARCEL_SEQUENCE => {
                    Some((**parcel).clone())
                }
                _ => None,
            })
            .await;
        match record {
            Ok(parcel) => Ok(Some(parcel)),
            Err(TestFailure::Timeout(_)) => Ok(None),
            Err(other) => Err(other),
        }
    }

    /// Whose avatar sounds the parcel the sender stands on lets play, as its
    /// record states it.
    async fn parcel_avatar_sounds(&mut self) -> Result<String, TestFailure> {
        let record = self.parcel_here().await?;
        Ok(avatar_sounds_text(record.as_ref()).to_owned())
    }

    /// Where the sender owns the parcel it stands on: bars avatar sounds on
    /// it, asks for a sound, and lets them play again. Records nothing where
    /// the land is somebody else's.
    async fn bar_own_parcel(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let Some(parcel) = self.parcel_here().await? else {
            return Ok(());
        };
        if parcel.owner.uuid() != self.names.sender {
            return Ok(());
        }
        let mut barring = parcel.to_update();
        barring.any_av_sounds = Some(false);
        barring.group_av_sounds = Some(false);
        self.sender
            .send(Command::UpdateParcel(Box::new(barring)))
            .await?;
        tokio::time::sleep(PARCEL_EDIT_SETTLE).await;
        let outcome = self.sound_on_own_barred_parcel(metrics).await;
        // As it was, whatever became of the leg.
        self.sender
            .send(Command::UpdateParcel(Box::new(parcel.to_update())))
            .await?;
        tokio::time::sleep(PARCEL_EDIT_SETTLE).await;
        outcome
    }

    /// The question [`bar_own_parcel`](Self::bar_own_parcel) asks while the
    /// parcel bars avatar sounds.
    async fn sound_on_own_barred_parcel(
        &mut self,
        metrics: &mut Metrics,
    ) -> Result<(), TestFailure> {
        let sounds = self.parcel_avatar_sounds().await?;
        metrics.set("own_parcel_barred_avatar_sounds", sounds);
        let (home, here) = (self.names.home, self.sender_at.clone());
        self.sound(
            metrics,
            "sound_on_an_own_barred_parcel",
            (UI_SOUND_CLICK, 1.0),
            home,
            &here,
            true,
        )
        .await
    }

    /// Listens to the watcher for `window` and brings where it is up to date.
    async fn follow(&mut self, window: Duration) -> Result<(), TestFailure> {
        let got = watch(self.other, self.names.other, window).await?;
        if let Some(at) = got.own {
            self.other_at = at;
        }
        Ok(())
    }

    /// Puts the watcher's camera a metre above `at`, looking down at the
    /// sender — or along the ground, where it is the sender it stands at.
    async fn camera(&self, at: &Vector) -> Result<(), TestFailure> {
        let eye = Vector {
            x: at.x,
            y: at.y,
            z: at.z + 1.0,
        };
        let target = Vector {
            x: self.sender_at.x + 5.0,
            y: self.sender_at.y,
            z: self.sender_at.z,
        };
        self.other
            .send(Command::SetCamera(Camera::looking_at(eye, target)))
            .await
    }

    /// Has the watcher climb until it is at least `least` metres above the
    /// sender, in pushes sized by what is left to climb, and puts its camera
    /// where it then hovers.
    async fn rise(&mut self, least: f32) -> Result<(), TestFailure> {
        let fly = ControlFlags::FLY;
        let started = Instant::now();
        loop {
            let short = least - (self.other_at.z - self.sender_at.z);
            if short <= 0.0 {
                break;
            }
            if started.elapsed() >= CLIMB_LIMIT {
                return Err(TestFailure::Assertion(format!(
                    "the watcher did not climb {least} m above the sender: it is at {}",
                    vector_text(&self.other_at)
                )));
            }
            let push = Duration::from_secs_f32((short / CLIMB_M_PER_S).clamp(0.4, 8.0));
            self.other
                .send(Command::SetControls(fly | ControlFlags::UP_POS))
                .await?;
            self.follow(push).await?;
            self.other.send(Command::SetControls(fly)).await?;
            self.follow(HOVER).await?;
        }
        let at = self.other_at.clone();
        self.camera(&at).await?;
        self.follow(Duration::from_secs(1)).await
    }

    /// Brings the watcher down to the ground at `ground` and lets it stand.
    async fn land(&mut self, ground: f32) -> Result<(), TestFailure> {
        let fly = ControlFlags::FLY;
        let started = Instant::now();
        self.other
            .send(Command::SetControls(fly | ControlFlags::UP_NEG))
            .await?;
        while self.other_at.z > ground + LANDED_M && started.elapsed() < DESCENT_LIMIT {
            self.follow(Duration::from_secs(1)).await?;
        }
        self.other
            .send(Command::SetControls(ControlFlags::empty()))
            .await?;
        self.follow(HOVER).await?;
        self.other.send(Command::FinishAnimation).await?;
        let at = self.other_at.clone();
        self.camera(&at).await?;
        self.follow(HOVER).await
    }
}

/// A census of viewer effects and sounds.
#[derive(Debug)]
pub struct EffectsSounds;

impl GridTest for EffectsSounds {
    fn name(&self) -> &'static str {
        "effects-sounds"
    }

    fn description(&self) -> &'static str {
        "Which viewer effects and sounds a region passes on, to whom, how far and under whose \
         name, and what a script's sounds have each avatar sent"
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
        Duration::from_secs(1500)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let folders = login_folders(ctx.primary()).await?;
            let mut residue = Residue::default();
            let mut metrics = Metrics::new();
            let outcome = census(ctx, folders, &mut residue, &mut metrics).await;
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

/// The legs a fake grid is asked too: every effect type, what is passed on of
/// an effect, and a viewer's sound.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of legs, each a gesture and what the two avatars were sent of it"
)]
async fn viewer_legs(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    // --- 1. Every effect type. ---
    let effects = EFFECT_TYPES
        .iter()
        .map(|(code, word)| (*word, census.effect(*code)))
        .collect();
    census.effects(metrics, "", effects, true).await?;

    // --- 2. What is passed on of an effect. ---
    // Three in one message.
    let batch = vec![census.effect(11), census.effect(12), census.effect(9)];
    let brought = census
        .act(vec![Command::ViewerEffect(batch.clone())])
        .await?;
    let shape = |got: &Got| {
        let sizes: Vec<String> = got
            .told
            .iter()
            .filter_map(|told| match told {
                Told::Effects(effects)
                    if effects
                        .iter()
                        .any(|effect| batch.iter().any(|sent| sent.id == effect.id)) =>
                {
                    Some(effects.len().to_string())
                }
                _ => None,
            })
            .collect();
        if sizes.is_empty() {
            "nothing".to_owned()
        } else {
            format!("messages of {}", sizes.join(", "))
        }
    };
    metrics.set("effect_batch_of_three_told_other", shape(&brought.other));
    metrics.set("effect_batch_of_three_told_sender", shape(&brought.sender));

    // Under another agent's name, and with a payload that names another.
    let mut of_the_other = census.effect(11);
    of_the_other.agent_id = AgentKey::from(census.names.other);
    let mut of_a_stranger = census.effect(11);
    of_a_stranger.agent_id = AgentKey::from(Uuid::new_v4());
    let mut of_nobody = census.effect(11);
    of_nobody.agent_id = AgentKey::from(Uuid::nil());
    let mut gaze_of_the_other = census.effect(14);
    if let ViewerEffectData::LookAt { source, .. } = &mut gaze_of_the_other.data {
        *source = Some(AgentKey::from(census.names.other));
    }
    let mut pointing_of_the_other = census.effect(15);
    if let ViewerEffectData::PointAt { source, .. } = &mut pointing_of_the_other.data {
        *source = Some(AgentKey::from(census.names.other));
    }
    let mut beam_of_the_other = census.effect(7);
    if let ViewerEffectData::Spiral { source, .. } = &mut beam_of_the_other.data {
        *source = Some(ObjectKey::from(census.names.other));
    }
    census
        .effects(
            metrics,
            "",
            vec![
                ("naming_the_other_avatar", of_the_other),
                ("naming_a_stranger", of_a_stranger),
                ("naming_nobody", of_nobody),
                ("look_at_with_the_other_as_its_source", gaze_of_the_other),
                (
                    "point_at_with_the_other_as_its_source",
                    pointing_of_the_other,
                ),
                ("beam_with_the_other_as_its_source", beam_of_the_other),
            ],
            true,
        )
        .await?;

    // Payloads that are not what the type says, and odd durations.
    let mut short_gaze = census.effect(14);
    short_gaze.data = ViewerEffectData::Raw(vec![1, 2, 3]);
    let mut empty_gaze = census.effect(14);
    empty_gaze.data = ViewerEffectData::Raw(Vec::new());
    let mut long_gaze = census.effect(14);
    let mut bytes = long_gaze.data.to_wire();
    bytes.push(0);
    long_gaze.data = ViewerEffectData::Raw(bytes);
    let mut short_pointing = census.effect(15);
    short_pointing.data = ViewerEffectData::Raw(vec![1, 2, 3]);
    let mut empty_pointing = census.effect(15);
    empty_pointing.data = ViewerEffectData::Raw(Vec::new());
    let mut long_beam = census.effect(7);
    long_beam.data = ViewerEffectData::Raw(vec![7; 255]);
    let mut empty_beam = census.effect(7);
    empty_beam.data = ViewerEffectData::Raw(Vec::new());
    let mut long_sphere = census.effect(11);
    long_sphere.data = ViewerEffectData::Raw(vec![7; 255]);
    let mut empty_sphere = census.effect(11);
    empty_sphere.data = ViewerEffectData::Raw(Vec::new());
    let mut endless = census.effect(11);
    endless.duration = 1.0e9;
    endless.color = [1, 2, 3, 4];
    let mut backwards = census.effect(11);
    backwards.duration = -5.0;
    let mut instant = census.effect(11);
    instant.duration = 0.0;
    census
        .effects(
            metrics,
            "",
            vec![
                ("look_at_of_three_bytes", short_gaze),
                ("look_at_of_no_bytes", empty_gaze),
                ("look_at_of_one_byte_more", long_gaze),
                ("point_at_of_three_bytes", short_pointing),
                ("point_at_of_no_bytes", empty_pointing),
                ("beam_of_255_bytes", long_beam),
                ("beam_of_no_bytes", empty_beam),
                ("sphere_of_255_bytes", long_sphere),
                ("sphere_of_no_bytes", empty_sphere),
                ("lasting_a_billion_seconds", endless),
                ("lasting_minus_five_seconds", backwards),
                ("lasting_no_time", instant),
            ],
            false,
        )
        .await?;

    // The same effect twice.
    let twice = census.effect(11);
    let brought = census
        .act(vec![
            Command::ViewerEffect(vec![twice.clone()]),
            Command::ViewerEffect(vec![twice.clone()]),
        ])
        .await?;
    metrics.set(
        "effect_sent_twice_told_other",
        census.names.relayed(&brought.other, &twice),
    );

    // Twenty in about a second.
    let burst: Vec<ViewerEffect> = std::iter::repeat_with(|| census.effect(11))
        .take(usize::from(EFFECT_BURST))
        .collect();
    for effect in &burst {
        census
            .sender
            .send(Command::ViewerEffect(vec![effect.clone()]))
            .await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let brought = census.act(Vec::new()).await?;
    let passed = burst
        .iter()
        .filter(|sent| brought.other.effect_blocks().any(|got| got.id == sent.id))
        .count();
    metrics.set(
        "effect_burst_of_twenty_passed_on",
        i64::try_from(passed).unwrap_or(i64::MAX),
    );

    // --- 3. A viewer's sound. ---
    let home = census.names.home;
    let here = census.sender_at.clone();
    let click = UI_SOUND_CLICK;
    census
        .sound(metrics, "sound", (click, 1.0), home, &here, true)
        .await?;
    census
        .sound(
            metrics,
            "sound_at_gain_2_5",
            (click, 2.5),
            home,
            &here,
            false,
        )
        .await?;
    census
        .sound(
            metrics,
            "sound_at_gain_minus_1",
            (click, -1.0),
            home,
            &here,
            false,
        )
        .await?;
    census
        .sound(metrics, "sound_at_gain_0", (click, 0.0), home, &here, false)
        .await?;
    census
        .sound(
            metrics,
            "sound_that_is_none",
            (Uuid::nil(), 1.0),
            home,
            &here,
            false,
        )
        .await?;
    census
        .sound(
            metrics,
            "sound_nobody_has",
            (Uuid::new_v4(), 1.0),
            home,
            &here,
            false,
        )
        .await?;
    // Stated further and further above the sender: how far from where a
    // sound is said to be it is still passed on.
    for height in SOUND_HEIGHTS_M {
        let stated = Vector {
            x: here.x,
            y: here.y,
            z: here.z + f32::from(height),
        };
        census
            .sound(
                metrics,
                &format!("sound_{height}m_above"),
                (click, 1.0),
                home,
                &stated,
                true,
            )
            .await?;
    }
    let above = Vector {
        x: here.x,
        y: here.y,
        z: here.z + FAR_ABOVE_M,
    };
    census
        .sound(metrics, "sound_far_above", (click, 1.0), home, &above, true)
        .await?;
    let outside = Vector {
        x: -50.0,
        y: here.y,
        z: here.z,
    };
    census
        .sound(
            metrics,
            "sound_outside_the_region",
            (click, 1.0),
            home,
            &outside,
            true,
        )
        .await?;
    // The region a region's width to the east, whether there is one or not.
    let next_door = RegionHandle(home.0.saturating_add(256 << 32));
    census
        .sound(
            metrics,
            "sound_in_another_region",
            (click, 1.0),
            next_door,
            &here,
            true,
        )
        .await?;

    // Ten in about a second.
    for _ in 0..SOUND_BURST {
        census
            .sender
            .send(Command::TriggerSound {
                sound: AssetKey::from(click),
                gain: 1.0,
                region_handle: home,
                position: RegionCoordinates::new(here.x, here.y, here.z),
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let brought = census.act(Vec::new()).await?;
    let passed = brought
        .other
        .told
        .iter()
        .filter(|told| matches!(told, Told::Trigger(trigger) if trigger.sound == click))
        .count();
    metrics.set(
        "sound_burst_of_ten_passed_on",
        i64::try_from(passed).unwrap_or(i64::MAX),
    );
    Ok(())
}

/// The legs that need a script and an avatar that moves: a script's sounds,
/// distance, and an arrival.
async fn live_legs(
    census: &mut Census<'_>,
    run: &mut Run,
    residue: &mut Residue,
    barred: Option<Vector>,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    // --- 4. A script's sounds. ---
    let cube = rez_cube(census.sender, run, residue, 0).await?;
    find_object(census.other, cube.full_id).await?;
    census.names.cube = Some(cube.full_id.uuid());
    watch(census.other, census.names.other, census.drain).await?;
    census.install(&cube).await?;
    for (order, word) in SCRIPT_LEGS {
        let brought = census.order(order).await?;
        let Some(word) = word else {
            continue;
        };
        metrics.set(
            &format!("script_{word}_told_owner"),
            census.names.sounds(&brought.sender, &cube),
        );
        metrics.set(
            &format!("script_{word}_told_other"),
            census.names.sounds(&brought.other, &cube),
        );
    }

    // --- 5. Distance. ---
    let home = census.names.home;
    let here = census.sender_at.clone();
    let ground = census.other_at.z;
    for (step, (least, word)) in STEPS.into_iter().enumerate() {
        census.rise(least).await?;
        metrics.set(
            &format!("{word}_distance_m"),
            f64::from(distance(&census.other_at, &census.sender_at)),
        );
        let prefix = format!("{word}_");
        let effects = RANGE_EFFECTS
            .iter()
            .map(|(code, name)| (*name, census.effect(*code)))
            .collect();
        census.effects(metrics, &prefix, effects, false).await?;
        if step == 0 {
            // The watcher's camera beside the sender, its avatar where it
            // hovers: a grid that measures from the camera passes them on.
            census.camera(&here).await?;
            census.follow(Duration::from_secs(2)).await?;
            let effects = RANGE_EFFECTS
                .iter()
                .map(|(code, name)| (*name, census.effect(*code)))
                .collect();
            census
                .effects(metrics, "nearby_with_the_camera_beside_", effects, false)
                .await?;
            let at = census.other_at.clone();
            census.camera(&at).await?;
            census.follow(Duration::from_secs(2)).await?;
        }
        census
            .sound(
                metrics,
                &format!("{word}_sound"),
                (UI_SOUND_CLICK, 1.0),
                home,
                &here,
                false,
            )
            .await?;
        for order in ["trigger", "play", "preload"] {
            let brought = census.order(order).await?;
            metrics.set(
                &format!("{word}_script_{order}_told_other"),
                census.names.sounds(&brought.other, &cube),
            );
        }
    }
    census.land(ground).await?;

    // --- 6. An arrival. ---
    census.order("loop").await?;
    census.other.disconnect().await?;
    census.other.relogin().await?;
    // Not `wait_for_region`: it would drop what the region sends meanwhile.
    // And the camera at the prim at once: a login may land at a hub out of
    // sight of it, and a grid streams by where the camera is.
    census.camera(&here).await?;
    let arrival = watch(census.other, census.names.other, ARRIVAL).await?;
    metrics.set(
        "arrival_beside_a_looping_prim_told",
        census.names.sounds(&arrival, &cube),
    );
    census.order("stop").await?;

    // --- 7. A parcel that bars avatar sounds. ---
    let Some(barred) = barred else {
        // No such parcel named: the sender's own, where it has one.
        census.bar_own_parcel(metrics).await?;
        for key in [
            "barred_parcel_avatar_sounds",
            "sound_on_a_barred_parcel_told_other",
            "sound_on_a_barred_parcel_told_sender",
        ] {
            metrics.set(key, NOT_ASKED);
        }
        return Ok(());
    };
    // The cube goes first: it is not taken from across the region.
    clean_up(census.sender, residue, run.folders.trash).await?;
    residue.roots.clear();
    let from = census.sender_at.clone();
    census.sender_at = walk_within_region(census.sender, from, &barred, |_event| {}).await?;
    let landed = watch(census.sender, census.names.sender, HOVER).await?;
    if let Some(at) = landed.own {
        census.sender_at = at;
    }
    let from = census.other_at.clone();
    census.other_at = walk_within_region(census.other, from, &barred, |_event| {}).await?;
    census.follow(HOVER).await?;
    let sounds = census.parcel_avatar_sounds().await?;
    metrics.set("barred_parcel_avatar_sounds", sounds);
    let here = census.sender_at.clone();
    census
        .sound(
            metrics,
            "sound_on_a_barred_parcel",
            (UI_SOUND_CLICK, 1.0),
            home,
            &here,
            true,
        )
        .await
}

/// The legs, in order. The cube is noted in `residue` as it appears, so the
/// caller can take it away when a leg fails.
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    // Where avatar sounds are allowed, where the fixtures name such a spot:
    // the build location is then the parcel that bars them.
    let sound_position = ctx.sound_position();
    let barred = if sound_position.is_some() {
        ctx.build_position()
    } else {
        None
    };
    let build_position = sound_position.or_else(|| ctx.build_position());
    let pace = Pace::of(grid);
    let drain = if grid.is_fake() { FAKE_DRAIN } else { DRAIN };
    let Some((sender, other)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "effects-sounds needs a secondary avatar".to_owned(),
        ));
    };
    let sender_id = sender
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let other_id = other
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

    // Both avatars to the spot, each with its scene settled.
    sender.wait_for_region(REGION_TIMEOUT).await?;
    let settled = settle_scene_with_avatar(
        sender,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    other.wait_for_region(REGION_TIMEOUT).await?;
    let theirs =
        settle_scene_with_avatar(other, grid, build_position, SETTLE_WINDOW, pace.settle_idle)
            .await?;
    let sender_at = settled.avatar.ok_or_else(|| {
        TestFailure::Assertion("the sender was never sent its own avatar".to_owned())
    })?;
    let other_at = theirs.avatar.ok_or_else(|| {
        TestFailure::Assertion("the watcher was never sent its own avatar".to_owned())
    })?;
    let home = sender
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("the sender is in no region".to_owned()))?;
    if other.region_handle() != Some(home) {
        return Err(TestFailure::Assertion(
            "the two avatars did not log in to the same region".to_owned(),
        ));
    }
    let mut run = Run {
        folders,
        owner: sender_id,
        pace,
        position: Vector {
            x: sender_at.x + CUBE_AWAY_M,
            y: sender_at.y,
            z: sender_at.z + REZ_LIFT_M,
        },
        seen: settled.seen,
    };
    let mut census = Census {
        sender,
        other,
        names: Names {
            sender: sender_id.uuid(),
            other: other_id.uuid(),
            cube: None,
            sound: UI_SOUND_CLICK,
            home,
        },
        quiet: run.pace.quiet,
        drain,
        sender_at,
        other_at: other_at.clone(),
    };
    // A viewer's camera is where its avatar is; a session's is wherever it
    // was last put.
    census.camera(&other_at).await?;
    census.follow(drain).await?;
    // Side by side: a flight to the spot ends wherever it comes down.
    if distance(&census.sender_at, &census.other_at) > BESIDE_M {
        let (from, to) = (census.other_at.clone(), census.sender_at.clone());
        census.other_at = walk_within_region(census.other, from, &to, |_event| {}).await?;
        census.follow(HOVER).await?;
        census.other.send(Command::FinishAnimation).await?;
        let at = census.other_at.clone();
        census.camera(&at).await?;
        census.follow(HOVER).await?;
    }
    metrics.set(
        "start_distance_m",
        f64::from(distance(&census.sender_at, &census.other_at)),
    );
    let sounds = census.parcel_avatar_sounds().await?;
    metrics.set("parcel_avatar_sounds", sounds);

    viewer_legs(&mut census, metrics).await?;
    if grid.is_fake() {
        // Nothing runs a script here, and no avatar moves.
        return Ok(());
    }
    live_legs(&mut census, &mut run, residue, barred, metrics).await
}

#[cfg(test)]
mod tests {
    use super::{flags_text, probe_source, two_places};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::UI_SOUND_CLICK;

    #[test]
    fn the_probe_compiles() -> Result<(), String> {
        let source = probe_source(UI_SOUND_CLICK);
        sl_lsl_runtime::compile(&source)
            .map(|_program| ())
            .map_err(|errors| format!("the probe does not compile: {errors:?}\n{source}"))
    }

    #[test]
    fn sound_flags_by_name() {
        assert_eq!(flags_text(0), "none");
        assert_eq!(flags_text(1), "loop");
        assert_eq!(flags_text(0b10_0101), "loop|slave|stop");
        assert_eq!(flags_text(0x41), "loop|0x40");
        assert_eq!(two_places(-0.001), "0.00");
    }
}
