//! A census of local chat: what a region passes on of a line, to whom, how
//! far, and what it says of it.
//!
//! A viewer's `ChatFromViewer` ([`Command::Chat`], [`Command::Typing`]) and a
//! script's `llSay` family both come back as `ChatFromSimulator`
//! ([`Event::ChatReceived`], [`Event::ChatTyping`]): a type, a source and its
//! kind, an owner, a name, how audible the line is, where it was said, and
//! its text. A region answers the speaker nothing else; what it does with a
//! line shows in what the two avatars are then sent.
//!
//! Avatar **A** speaks, owns the probe prim and moves away; avatar **B**
//! stays where it is and gives the probe its orders. One question a leg:
//!
//! 1. **Every chat type** a viewer may write into a line, the ones a viewer
//!    never sends included.
//! 2. **Typing**: start and stop, a start that carries text, and one on a
//!    channel.
//! 3. **Channels**: a say on a channel that is not the public one, and on the
//!    debug channel.
//! 4. **Text**: no text, text at and past the length a line may have, text
//!    cut in the middle of a character, an emote, a line break.
//! 5. **A burst**: twenty lines in a second.
//! 6. **A script's lines**: each of the functions a script speaks with.
//! 7. **A channel and a script**: whether a viewer's line on a channel
//!    reaches a script that listens there.
//! 8. **Distance**: A moves away in three steps — past a whisper's reach,
//!    past a say's, past a shout's — and its own lines, its typing and the
//!    script's lines are asked again at each; and, at the first, whether it
//!    is the avatars or their cameras a grid measures between.
//!
//! The script is the instrument of legs 6 and 7 and of the script's part of
//! leg 8: it says what it is told to on a channel and says when it has. A
//! grid that runs no scripts is asked the rest, and moves A by teleport where
//! a live grid has it fly (a telehub catches a teleport there).
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/chat.md`; every run is held to it.
//!
//! `2av`, `[both]`, everything but the script's legs offline on both fake
//! flavours. The cube is derezzed to the Trash, also when a leg fails.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, Camera, ChatAudible, ChatChannel, ChatMessage, ChatSource, ChatType, Command,
    ControlFlags, Event, Object, ParcelInfo, RegionCoordinates, RegionHandle, RestoreItem,
    RezScriptParams, ScriptTarget, ScriptUploadLocation, Uuid, Vector,
};

use crate::cases::effects_sounds::{distance, vector_text};
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
const PROBE_TAG: &str = "SLCLC|";

/// What every line an avatar of the census says begins with.
pub(crate) const LINE_TAG: &str = "lc-";

/// The name of the probe script in the cube.
const PROBE_NAME: &str = "SLClientChatProbe";

/// The channel the probe takes its orders on.
const PROBE_CHANNEL: i32 = 7_734_292;

/// The debug channel: where a script's errors are said.
const DEBUG_CHANNEL: i32 = i32::MAX;

/// The length the probe's long line is built up to, in bytes.
const LONG_SAY_BYTES: usize = 1500;

/// The time between two lines of one leg.
pub(crate) const BETWEEN: Duration = Duration::from_millis(150);

/// How many lines the burst sends, in about a second.
const BURST: u8 = 20;

/// How far the cube stands from its owner, in metres.
const CUBE_AWAY_M: f32 = 2.0;

/// The sequence id of the census's parcel request.
const PARCEL_SEQUENCE: i32 = 7_701;

/// How far apart the two avatars may stand at the start, in metres.
const BESIDE_M: f32 = 5.0;

/// How far from where its source is known to be a line may be stated and
/// still be stated there, in metres.
const AT_M: f32 = 2.5;

/// The speed a climb is planned at, in metres a second: what Second Life
/// lifts a flying avatar at.
const CLIMB_M_PER_S: f32 = 16.0;

/// How long A's climb to one step may take.
const CLIMB_LIMIT: Duration = Duration::from_secs(120);

/// How long A's way down may take.
const DESCENT_LIMIT: Duration = Duration::from_secs(180);

/// How long an avatar hovers after a push before its position is read.
const HOVER: Duration = Duration::from_secs(3);

/// How long A flies sideways for before it climbs.
const STEP_ASIDE: Duration = Duration::from_millis(150);

/// How far above the ground A counts as landed, in metres.
const LANDED_M: f32 = 2.5;

/// One step of A's way up.
struct Step {
    /// The word the step's metrics begin with.
    word: &'static str,
    /// The least A has to be above B, in metres: what a flight climbs to.
    least: f32,
    /// How far above B a teleport puts A, in metres.
    placed: f32,
    /// The distances between the two avatars the step's answers hold for, in
    /// metres: past one type's reach and short of the next one's.
    band: (f32, f32),
}

/// The steps of A's way up: past a whisper's ten metres, past a say's
/// twenty, past a shout's hundred.
const STEPS: [Step; 3] = [
    Step {
        word: "nearby",
        least: 11.0,
        placed: 15.0,
        band: (10.5, 19.5),
    },
    Step {
        word: "further",
        least: 35.0,
        placed: 36.0,
        band: (20.5, 95.0),
    },
    Step {
        word: "far",
        least: 120.0,
        placed: 121.0,
        band: (100.5, 400.0),
    },
];

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/chat.md (local-chat, 2026-10-13)";

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

/// What each grid was measured to do with a viewer's lines, by the metric
/// the census records it under: every run, live or fake, is held to it.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "debug_type_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "debug_type_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "direct_type_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "direct from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "direct_type_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "direct from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "far_debug_type_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    ("far_say_of_b_told_a", both(Answer::Text("nothing"))),
    (
        "far_say_on_the_debug_channel_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    ("far_say_told_b", both(Answer::Text("nothing"))),
    ("far_shout_of_b_told_a", both(Answer::Text("nothing"))),
    ("far_shout_told_b", both(Answer::Text("nothing"))),
    (
        "far_typing_start_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("start typing from avatar A, A's name, fully audible, at its source"),
        ),
    ),
    (
        "far_typing_stop_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("stop typing from avatar A, A's name, fully audible, at its source"),
        ),
    ),
    ("far_whisper_of_b_told_a", both(Answer::Text("nothing"))),
    ("far_whisper_told_b", both(Answer::Text("nothing"))),
    (
        "further_debug_type_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    ("further_say_of_b_told_a", both(Answer::Text("nothing"))),
    (
        "further_say_on_the_debug_channel_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    ("further_say_told_b", both(Answer::Text("nothing"))),
    (
        "further_shout_of_b_told_a",
        both(Answer::Text(
            "shout from the agent avatar B, owner avatar B, B's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "further_shout_told_b",
        both(Answer::Text(
            "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "further_typing_start_told_b",
        each(
            Answer::Text("start typing from avatar A, A's name, barely audible, at its source"),
            Answer::Text("start typing from avatar A, A's name, fully audible, at its source"),
        ),
    ),
    (
        "further_typing_stop_told_b",
        each(
            Answer::Text("stop typing from avatar A, A's name, barely audible, at its source"),
            Answer::Text("stop typing from avatar A, A's name, fully audible, at its source"),
        ),
    ),
    ("further_whisper_of_b_told_a", both(Answer::Text("nothing"))),
    ("further_whisper_told_b", both(Answer::Text("nothing"))),
    (
        "nearby_debug_type_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "nearby_say_of_b_told_a",
        both(Answer::Text(
            "say from the agent avatar B, owner avatar B, B's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "nearby_say_on_the_debug_channel_told_b",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "nearby_say_told_b",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "nearby_shout_of_b_told_a",
        both(Answer::Text(
            "shout from the agent avatar B, owner avatar B, B's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "nearby_shout_told_b",
        both(Answer::Text(
            "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "nearby_typing_start_told_b",
        both(Answer::Text(
            "start typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    (
        "nearby_typing_stop_told_b",
        both(Answer::Text(
            "stop typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    ("nearby_whisper_of_b_told_a", both(Answer::Text("nothing"))),
    ("nearby_whisper_told_b", both(Answer::Text("nothing"))),
    (
        "nearby_with_the_camera_of_a_beside_b_whisper_of_b_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "nearby_with_the_camera_of_a_beside_b_whisper_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "nearby_with_the_camera_of_b_beside_a_whisper_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "owner_type_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "owner from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "owner_type_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "owner from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    ("parcel_shows_its_avatars", both(Answer::Text("yes"))),
    (
        "region_info_chat_ranges",
        each(
            Answer::Text("whisper 10 m, say 20 m, shout 100 m, offsets 0/0/0, flags 0"),
            Answer::Text("none stated"),
        ),
    ),
    (
        "region_type_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "region from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "region_type_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "region from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    ("say_burst_of_twenty_passed_on", both(Answer::Count(20))),
    (
        "say_ending_in_spaces_told_a",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_ending_in_spaces_told_b",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_of_1000_bytes_told_a",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_of_1000_bytes_told_b",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_of_1023_bytes_told_a",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_1023_bytes_told_b",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_1024_bytes_told_a",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_1024_bytes_told_b",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_1100_bytes_told_a",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_1100_bytes_told_b",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_1200_bytes_of_two_byte_characters_kept_bytes",
        each(Answer::Any, Answer::Count(1022)),
    ),
    (
        "say_of_1200_bytes_of_two_byte_characters_told_a",
        each(
            Answer::Any,
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1022 bytes",
            ),
        ),
    ),
    (
        "say_of_1200_bytes_of_two_byte_characters_told_b",
        each(
            Answer::Any,
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1022 bytes",
            ),
        ),
    ),
    (
        "say_of_2000_bytes_kept_bytes",
        each(Answer::Any, Answer::Count(1000)),
    ),
    (
        "say_of_2000_bytes_kept_bytes_and_name_bytes",
        each(Answer::Count(1151), Answer::Any),
    ),
    (
        "say_of_2000_bytes_of_b_kept_bytes",
        each(Answer::Any, Answer::Count(1000)),
    ),
    (
        "say_of_2000_bytes_of_b_kept_bytes_and_name_bytes",
        each(Answer::Count(1151), Answer::Any),
    ),
    (
        "say_of_2000_bytes_of_b_told_a",
        each(
            Answer::Any,
            Answer::Text(
                "say from the agent avatar B, owner avatar B, B's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_2000_bytes_told_a",
        each(
            Answer::Any,
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_2000_bytes_told_b",
        each(
            Answer::Any,
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_3000_bytes_kept_bytes",
        each(Answer::Any, Answer::Count(1000)),
    ),
    (
        "say_of_3000_bytes_kept_bytes_and_name_bytes",
        each(Answer::Count(1151), Answer::Any),
    ),
    (
        "say_of_3000_bytes_of_b_kept_bytes",
        each(Answer::Any, Answer::Count(1000)),
    ),
    (
        "say_of_3000_bytes_of_b_kept_bytes_and_name_bytes",
        each(Answer::Count(1151), Answer::Any),
    ),
    (
        "say_of_3000_bytes_of_b_told_a",
        each(
            Answer::Any,
            Answer::Text(
                "say from the agent avatar B, owner avatar B, B's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_3000_bytes_told_a",
        each(
            Answer::Any,
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_3000_bytes_told_b",
        each(
            Answer::Any,
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "say_of_an_emote_told_b",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_of_no_text_told_a",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_of_no_text_told_b",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_on_a_high_channel_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "say_on_a_high_channel_told_b",
        both(Answer::Text("nothing")),
    ),
    ("say_on_channel_1_told_a", both(Answer::Text("nothing"))),
    ("say_on_channel_1_told_b", both(Answer::Text("nothing"))),
    (
        "say_on_channel_minus_1_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "say_on_channel_minus_1_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "say_on_the_debug_channel_told_a",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "say_on_the_debug_channel_told_b",
        each(
            Answer::Text(
                "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "say_on_the_lowest_channel_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "say_on_the_lowest_channel_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "say_told_a",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_told_b",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_with_a_line_break_told_a",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "say_with_a_line_break_told_b",
        both(Answer::Text(
            "say from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "shout_on_the_debug_channel_told_a",
        each(
            Answer::Text(
                "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "shout_on_the_debug_channel_told_b",
        each(
            Answer::Text(
                "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "shout_told_a",
        both(Answer::Text(
            "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "shout_told_b",
        both(Answer::Text(
            "shout from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "type_10_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "type 10 from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "type_10_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "type 10 from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "type_255_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "type 255 from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "type_255_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "type 255 from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "type_3_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "type 3 from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "type_3_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "type 3 from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "typing_start_on_channel_5_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "typing_start_on_channel_5_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "typing_start_told_a",
        both(Answer::Text(
            "start typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    (
        "typing_start_told_b",
        both(Answer::Text(
            "start typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    (
        "typing_start_with_text_told_a",
        both(Answer::Text(
            "start typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    (
        "typing_start_with_text_told_b",
        both(Answer::Text(
            "start typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    (
        "typing_stop_again_told_a",
        both(Answer::Text(
            "stop typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    (
        "typing_stop_again_told_b",
        both(Answer::Text(
            "stop typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    (
        "typing_stop_told_a",
        both(Answer::Text(
            "stop typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    (
        "typing_stop_told_b",
        both(Answer::Text(
            "stop typing from avatar A, A's name, fully audible, at its source",
        )),
    ),
    (
        "whisper_on_the_debug_channel_told_a",
        each(
            Answer::Text(
                "whisper from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "whisper_on_the_debug_channel_told_b",
        each(
            Answer::Text(
                "whisper from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "whisper_told_a",
        both(Answer::Text(
            "whisper from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "whisper_told_b",
        both(Answer::Text(
            "whisper from the agent avatar A, owner avatar A, A's name, fully audible, at its source, text as sent",
        )),
    ),
];

/// What each live grid was measured to answer where a script is needed to
/// ask.
const MEASURED_LIVE: &[(&str, Measured<Answer>)] = &[
    (
        "far_script_debug_say_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "far_script_debug_shout_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "far_script_owner_say_told_a",
        each(
            Answer::Text(
                "owner from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "owner from the object the prim, owner the prim, the prim's name, fully audible, at the middle of the region, text as sent",
            ),
        ),
    ),
    (
        "far_script_region_say_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "far_script_say_to_owner_told_a",
        both(Answer::Text(
            "direct from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    ("far_script_say_told_a", both(Answer::Text("nothing"))),
    ("far_script_shout_told_a", both(Answer::Text("nothing"))),
    ("far_script_whisper_told_a", both(Answer::Text("nothing"))),
    (
        "further_script_debug_say_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "further_script_debug_shout_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "debug from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "further_script_owner_say_told_a",
        each(
            Answer::Text(
                "owner from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "owner from the object the prim, owner the prim, the prim's name, fully audible, at the middle of the region, text as sent",
            ),
        ),
    ),
    (
        "further_script_region_say_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "further_script_say_to_owner_told_a",
        both(Answer::Text(
            "direct from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    ("further_script_say_told_a", both(Answer::Text("nothing"))),
    (
        "further_script_shout_told_a",
        each(
            Answer::Text(
                "shout from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "shout from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "further_script_whisper_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "nearby_script_debug_say_told_a",
        both(Answer::Text(
            "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "nearby_script_debug_shout_told_a",
        each(
            Answer::Text(
                "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "nearby_script_owner_say_told_a",
        each(
            Answer::Text(
                "owner from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "owner from the object the prim, owner the prim, the prim's name, fully audible, at the middle of the region, text as sent",
            ),
        ),
    ),
    (
        "nearby_script_region_say_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "nearby_script_say_to_owner_told_a",
        both(Answer::Text(
            "direct from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "nearby_script_say_told_a",
        both(Answer::Text(
            "say from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "nearby_script_shout_told_a",
        each(
            Answer::Text(
                "shout from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "shout from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "nearby_script_whisper_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "say_on_channel_1_reached_a_script",
        both(Answer::Flag(true)),
    ),
    (
        "say_on_channel_minus_1_reached_a_script",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    (
        "say_on_the_debug_channel_reached_a_script",
        both(Answer::Flag(true)),
    ),
    (
        "script_debug_region_say_told_a",
        each(
            Answer::Text(
                "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "script_debug_region_say_told_b",
        each(
            Answer::Text(
                "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "script_debug_say_to_other_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "script_debug_say_to_other_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "script_debug_say_told_a",
        both(Answer::Text(
            "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_debug_say_told_b",
        both(Answer::Text(
            "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_debug_shout_told_a",
        each(
            Answer::Text(
                "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "script_debug_shout_told_b",
        each(
            Answer::Text(
                "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "debug from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "script_debug_whisper_told_a",
        both(Answer::Text(
            "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_debug_whisper_told_b",
        both(Answer::Text(
            "debug from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_empty_say_told_a",
        both(Answer::Text(
            "say from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_empty_say_told_b",
        both(Answer::Text(
            "say from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_long_say_told_a",
        each(
            Answer::Text(
                "say from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text the first 1024 bytes",
            ),
            Answer::Text(
                "say from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "script_long_say_told_b",
        each(
            Answer::Text(
                "say from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text the first 1024 bytes",
            ),
            Answer::Text(
                "say from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text the first 1000 bytes",
            ),
        ),
    ),
    (
        "script_owner_say_told_a",
        each(
            Answer::Text(
                "owner from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "owner from the object the prim, owner the prim, the prim's name, fully audible, at the middle of the region, text as sent",
            ),
        ),
    ),
    ("script_owner_say_told_b", both(Answer::Text("nothing"))),
    (
        "script_region_say_on_a_channel_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "script_region_say_on_a_channel_told_b",
        both(Answer::Text("nothing")),
    ),
    ("script_region_say_told_a", both(Answer::Text("nothing"))),
    ("script_region_say_told_b", both(Answer::Text("nothing"))),
    (
        "script_say_to_other_on_a_channel_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "script_say_to_other_on_a_channel_told_b",
        both(Answer::Text("nothing")),
    ),
    ("script_say_to_other_told_a", both(Answer::Text("nothing"))),
    (
        "script_say_to_other_told_b",
        both(Answer::Text(
            "direct from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_say_to_owner_told_a",
        both(Answer::Text(
            "direct from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    ("script_say_to_owner_told_b", both(Answer::Text("nothing"))),
    (
        "script_say_told_a",
        both(Answer::Text(
            "say from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_say_told_b",
        both(Answer::Text(
            "say from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_shout_told_a",
        each(
            Answer::Text(
                "shout from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "shout from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "script_shout_told_b",
        each(
            Answer::Text(
                "shout from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
            ),
            Answer::Text(
                "shout from the object the prim, owner nobody, the prim's name, fully audible, at its source, text as sent",
            ),
        ),
    ),
    (
        "script_unnamed_say_told_a",
        both(Answer::Text(
            "say from the object the prim, owner avatar A, no name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_unnamed_say_told_b",
        both(Answer::Text(
            "say from the object the prim, owner avatar A, no name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_whisper_told_a",
        both(Answer::Text(
            "whisper from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
    (
        "script_whisper_told_b",
        both(Answer::Text(
            "whisper from the object the prim, owner avatar A, the prim's name, fully audible, at its source, text as sent",
        )),
    ),
];

/// Every chat type the census writes into a line on the public channel, by
/// the word its metrics carry. The two typing types are a leg of their own.
const TYPES: [(u8, &str); 10] = [
    (0, "whisper"),
    (1, "say"),
    (2, "shout"),
    (3, "type_3"),
    (6, "debug_type"),
    (7, "region_type"),
    (8, "owner_type"),
    (9, "direct_type"),
    (10, "type_10"),
    (255, "type_255"),
];

/// The channels a say is sent on, by the word its metrics carry.
const CHANNELS: [(i32, &str); 5] = [
    (1, "channel_1"),
    (-1, "channel_minus_1"),
    (2_000_000_000, "a_high_channel"),
    (i32::MIN, "the_lowest_channel"),
    (DEBUG_CHANNEL, "the_debug_channel"),
];

/// The channels the probe listens on beside its own, by the word a metric
/// carries.
const HEARD_CHANNELS: [(i32, &str); 3] = [
    (1, "channel_1"),
    (-1, "channel_minus_1"),
    (DEBUG_CHANNEL, "the_debug_channel"),
];

/// What the probe is told to say in the script leg.
const SCRIPT_LEGS: [&str; 17] = [
    "whisper",
    "say",
    "shout",
    "region_say",
    "region_say_on_a_channel",
    "owner_say",
    "say_to_owner",
    "say_to_other",
    "say_to_other_on_a_channel",
    "debug_whisper",
    "debug_say",
    "debug_shout",
    "debug_region_say",
    "debug_say_to_other",
    "empty_say",
    "long_say",
    "unnamed_say",
];

/// What the probe is told to say at each step of A's way up.
const RANGE_SCRIPT_LEGS: [&str; 8] = [
    "whisper",
    "say",
    "shout",
    "region_say",
    "owner_say",
    "say_to_owner",
    "debug_say",
    "debug_shout",
];

/// The probe: a script that says what whoever speaks on the probe's channel
/// names, and says when it has; and tells its owner of every line of the
/// census it hears on another channel.
fn probe_source() -> String {
    format!(
        r#"string T = "{PROBE_TAG}";
default
{{
    state_entry()
    {{
        llListen({PROBE_CHANNEL}, "", NULL_KEY, "");
        llListen(1, "", NULL_KEY, "");
        llListen(-1, "", NULL_KEY, "");
        llListen(DEBUG_CHANNEL, "", NULL_KEY, "");
        llOwnerSay(T + "R|" + llGetObjectName());
    }}
    listen(integer channel, string name, key id, string what)
    {{
        if (channel != {PROBE_CHANNEL})
        {{
            if (llSubStringIndex(what, "{LINE_TAG}") == 0)
                llOwnerSay(T + "H|" + (string)channel + "|" + what);
            return;
        }}
        if (what == "whisper") llWhisper(0, T + what);
        else if (what == "say") llSay(0, T + what);
        else if (what == "shout") llShout(0, T + what);
        else if (what == "region_say") llRegionSay(0, T + what);
        else if (what == "region_say_on_a_channel") llRegionSay(5, T + what);
        else if (what == "owner_say") llOwnerSay(T + what);
        else if (what == "say_to_owner") llRegionSayTo(llGetOwner(), 0, T + what);
        else if (what == "say_to_other") llRegionSayTo(id, 0, T + what);
        else if (what == "say_to_other_on_a_channel") llRegionSayTo(id, 5, T + what);
        else if (what == "debug_whisper") llWhisper(DEBUG_CHANNEL, T + what);
        else if (what == "debug_say") llSay(DEBUG_CHANNEL, T + what);
        else if (what == "debug_shout") llShout(DEBUG_CHANNEL, T + what);
        else if (what == "debug_region_say") llRegionSay(DEBUG_CHANNEL, T + what);
        else if (what == "debug_say_to_other") llRegionSayTo(id, DEBUG_CHANNEL, T + what);
        else if (what == "empty_say") llSay(0, "");
        else if (what == "long_say")
        {{
            string long = T + what + "|";
            while (llStringLength(long) < {LONG_SAY_BYTES}) long += "0123456789";
            llSay(0, long);
        }}
        else if (what == "unnamed_say")
        {{
            string named = llGetObjectName();
            llSetObjectName("");
            llSay(0, T + what);
            llSetObjectName(named);
        }}
        llSay(0, T + "D|" + what);
    }}
}}
"#
    )
}

/// The line the probe's `long_say` builds.
fn long_say_text() -> String {
    let mut long = format!("{PROBE_TAG}long_say|");
    while long.len() < LONG_SAY_BYTES {
        long.push_str("0123456789");
    }
    long
}

/// A start or a stop of typing as it arrived.
#[derive(Debug, Clone)]
pub(crate) struct Typed {
    /// The name it came under.
    pub(crate) name: String,
    /// Whose it is.
    pub(crate) source: Uuid,
    /// Whether it is a start.
    pub(crate) start: bool,
    /// How audible the grid says the typist is.
    pub(crate) audible: ChatAudible,
    /// Where the grid says the typist is.
    pub(crate) position: RegionCoordinates,
}

/// What one session was sent during a window.
#[derive(Debug, Default)]
pub(crate) struct Got {
    /// Every line of chat, in arrival order.
    pub(crate) lines: Vec<ChatMessage>,
    /// Every start and stop of typing, in arrival order.
    pub(crate) typed: Vec<Typed>,
    /// The text of every alert.
    pub(crate) alerts: Vec<String>,
    /// How saving a script ended, once it has.
    pub(crate) uploaded: Option<Result<bool, String>>,
    /// Where the session's own avatar was last said to be.
    pub(crate) own: Option<Vector>,
}

impl Got {
    /// Whether the probe said `line`.
    fn said(&self, line: &str) -> bool {
        let wanted = format!("{PROBE_TAG}{line}");
        self.lines.iter().any(|chat| chat.message == wanted)
    }

    /// What the probe said after `prefix`, if it said such a line.
    fn said_after(&self, prefix: &str) -> Option<&str> {
        let wanted = format!("{PROBE_TAG}{prefix}");
        self.lines
            .iter()
            .find_map(|chat| chat.message.strip_prefix(wanted.as_str()))
    }
}

/// Listens on `session`, whose avatar is `agent`, until `done` says the leg
/// has what it came for and `quiet` more has passed, or for `limit` at most.
pub(crate) async fn watch_for(
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
                    Event::ChatReceived(chat) => got.lines.push((**chat).clone()),
                    Event::ChatTyping {
                        from_name,
                        source_id,
                        typing,
                        audible,
                        position,
                    } => got.typed.push(Typed {
                        name: from_name.clone(),
                        source: *source_id,
                        start: *typing,
                        audible: *audible,
                        position: *position,
                    }),
                    Event::ObjectAdded(object) | Event::ObjectUpdated(object)
                        if object.full_id.uuid() == agent =>
                    {
                        got.own = Some(object.motion.position.clone());
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
pub(crate) async fn watch(
    session: &mut Session,
    agent: Uuid,
    window: Duration,
) -> Result<Got, TestFailure> {
    watch_for(session, agent, window, window, |_got| false).await
}

/// Which lines of a window are the ones a leg asks about.
#[derive(Debug, Clone)]
enum Wanted {
    /// The line whose text is exactly this.
    Exact(String),
    /// The lines whose text begins with this.
    Prefix(String),
    /// The lines of this source that have no text.
    EmptyFrom(Uuid),
}

impl Wanted {
    /// Whether `line` is one of them.
    fn is(&self, line: &ChatMessage) -> bool {
        match self {
            Self::Exact(text) => line.message == *text,
            Self::Prefix(prefix) => line.message.starts_with(prefix.as_str()),
            Self::EmptyFrom(source) => {
                line.message.is_empty() && line.source.source_id() == *source
            }
        }
    }
}

/// The word a metric names a chat type by.
pub(crate) fn type_text(chat_type: ChatType) -> String {
    match chat_type {
        ChatType::Whisper => "whisper".to_owned(),
        ChatType::Normal => "say".to_owned(),
        ChatType::Shout => "shout".to_owned(),
        ChatType::StartTyping => "start typing".to_owned(),
        ChatType::StopTyping => "stop typing".to_owned(),
        ChatType::DebugChannel => "debug".to_owned(),
        ChatType::Region => "region".to_owned(),
        ChatType::Owner => "owner".to_owned(),
        ChatType::Direct => "direct".to_owned(),
        other => format!("type {}", other.to_u8()),
    }
}

/// The word a metric names an audibility by.
pub(crate) fn audible_text(audible: ChatAudible) -> String {
    match audible {
        ChatAudible::Fully => "fully audible".to_owned(),
        ChatAudible::Barely => "barely audible".to_owned(),
        ChatAudible::Not => "not audible".to_owned(),
        ChatAudible::Unknown(byte) => format!("audible {byte}"),
        other => format!("audible {other:?}"),
    }
}

/// What a metric says of the text `got` where `sent` was said.
pub(crate) fn text_text(got: &str, sent: &str) -> String {
    if got == sent {
        "as sent".to_owned()
    } else if got.is_empty() {
        "none".to_owned()
    } else if sent.starts_with(got) {
        format!("the first {} bytes", got.len())
    } else {
        format!("other ({} bytes)", got.len())
    }
}

/// The ids, names and places a metric names by what they are rather than by
/// their value.
#[derive(Debug, Clone)]
struct Scene {
    /// Avatar A: speaks, owns the cube, moves away.
    a: Uuid,
    /// Avatar B: stays, gives the probe its orders.
    b: Uuid,
    /// The name A logged in under.
    a_name: String,
    /// The name B logged in under.
    b_name: String,
    /// Where A was last said to be.
    a_at: Vector,
    /// Where B was last said to be.
    b_at: Vector,
    /// The cube, once there is one: its id, where it stands and its name.
    prim: Option<(Uuid, Vector, String)>,
}

impl Scene {
    /// Whose id `id` is.
    fn who(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nobody"
        } else if id == self.a {
            "avatar A"
        } else if id == self.b {
            "avatar B"
        } else if self.prim.as_ref().is_some_and(|(prim, ..)| *prim == id) {
            "the prim"
        } else {
            "somebody else"
        }
    }

    /// Whose name `name` is.
    fn whose_name(&self, name: &str) -> &'static str {
        if name.is_empty() {
            "no name"
        } else if name.eq_ignore_ascii_case(&self.a_name) {
            "A's name"
        } else if name.eq_ignore_ascii_case(&self.b_name) {
            "B's name"
        } else if self
            .prim
            .as_ref()
            .is_some_and(|(_prim, _at, prim_name)| prim_name == name)
        {
            "the prim's name"
        } else {
            "another name"
        }
    }

    /// Where `id` is known to be.
    fn place_of(&self, id: Uuid) -> Option<&Vector> {
        if id == self.a {
            Some(&self.a_at)
        } else if id == self.b {
            Some(&self.b_at)
        } else {
            self.prim
                .as_ref()
                .filter(|(prim, ..)| *prim == id)
                .map(|(_prim, at, _name)| at)
        }
    }

    /// Where a line of `source` is stated to have been said, as a metric
    /// names it.
    fn place(&self, position: &RegionCoordinates, source: Uuid) -> &'static str {
        let at = Vector {
            x: position.x(),
            y: position.y(),
            z: position.z(),
        };
        let near = |place: Option<&Vector>| place.is_some_and(|place| distance(&at, place) < AT_M);
        let middle = Vector {
            x: 128.0,
            y: 128.0,
            z: 30.0,
        };
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        if near(self.place_of(source)) {
            "at its source"
        } else if near(self.prim.as_ref().map(|(_prim, at, _name)| at)) {
            "at the prim"
        } else if near(Some(&self.a_at)) {
            "at avatar A"
        } else if near(Some(&self.b_at)) {
            "at avatar B"
        } else if distance(&at, &zero) < 0.01 {
            "at zero"
        } else if distance(&at, &middle) < 0.01 {
            "at the middle of the region"
        } else {
            "elsewhere"
        }
    }

    /// One line, as a metric states it; `sent` is the text that was said.
    fn line_text(&self, line: &ChatMessage, sent: &str) -> String {
        let source = match line.source {
            ChatSource::System => "the system".to_owned(),
            ChatSource::Agent(agent) => format!("the agent {}", self.who(agent.uuid())),
            ChatSource::Object(object) => format!("the object {}", self.who(object.uuid())),
            ChatSource::Unknown {
                source_type,
                source_id,
            } => format!("source type {source_type} {}", self.who(source_id)),
        };
        format!(
            "{} from {source}, owner {}, {}, {}, {}, text {}",
            type_text(line.chat_type),
            self.who(line.owner_id.unwrap_or_else(Uuid::nil)),
            self.whose_name(&line.from_name),
            audible_text(line.audible),
            self.place(&line.position, line.source.source_id()),
            text_text(&line.message, sent),
        )
    }

    /// What `got` holds of the lines `wanted`, where `sent` was said:
    /// `nothing`, or the first of them — and how many, where that is not one.
    fn told(&self, got: &Got, wanted: &Wanted, sent: &str) -> String {
        let lines: Vec<&ChatMessage> = got.lines.iter().filter(|line| wanted.is(line)).collect();
        let Some(first) = lines.first() else {
            return "nothing".to_owned();
        };
        let text = self.line_text(first, sent);
        if lines.len() == 1 {
            text
        } else {
            format!("{} times, {text}", lines.len())
        }
    }

    /// What `got` holds of `typist` starting (or stopping) to type:
    /// `nothing`, or the first such notice — and how many, where that is not
    /// one.
    fn typed(&self, got: &Got, typist: Uuid, start: bool) -> String {
        let notices: Vec<&Typed> = got
            .typed
            .iter()
            .filter(|typed| typed.source == typist && typed.start == start)
            .collect();
        let Some(first) = notices.first() else {
            return "nothing".to_owned();
        };
        let text = format!(
            "{} typing from {}, {}, {}, {}",
            if first.start { "start" } else { "stop" },
            self.who(first.source),
            self.whose_name(&first.name),
            audible_text(first.audible),
            self.place(&first.position, first.source),
        );
        if notices.len() == 1 {
            text
        } else {
            format!("{} times, {text}", notices.len())
        }
    }
}

/// Which of the two avatars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The one that speaks, owns the cube and moves away.
    A,
    /// The one that stays.
    B,
}

/// What a gesture of one avatar's brought each.
struct Brought {
    /// What A was sent.
    a: Got,
    /// What B was.
    b: Got,
}

/// One line an avatar is to say.
struct Say {
    /// What the line's metrics begin with.
    key: String,
    /// The type it is sent as.
    chat_type: ChatType,
    /// The channel it is sent on.
    channel: i32,
    /// What its text begins with, and no other line's does.
    tag: String,
    /// Its text.
    text: String,
}

/// Whom a leg records the answer of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Record {
    /// Both avatars.
    Both,
    /// The one that did not speak.
    Listener,
}

/// Everything the legs share.
struct Census<'a> {
    /// Avatar A's session.
    a: &'a mut Session,
    /// Avatar B's session.
    b: &'a mut Session,
    /// The ids, names and places the metrics name.
    scene: Scene,
    /// The region both stand in.
    home: RegionHandle,
    /// How long the acting avatar listens after a gesture.
    quiet: Duration,
    /// How long the queue of the other one is then read for.
    drain: Duration,
    /// How many lines the census has said.
    said: u32,
    /// Whether A is moved by teleport rather than by flying.
    teleports: bool,
}

impl Census<'_> {
    /// A line of the census: the tag that begins it, and the tag with `word`
    /// after it.
    fn tagged(&mut self, word: &str) -> (String, String) {
        self.said = self.said.saturating_add(1);
        let tag = format!("{LINE_TAG}{}-", self.said);
        let text = format!("{tag}{word}");
        (tag, text)
    }

    /// A line of the type `chat_type` on `channel`, its metrics under `key`.
    fn line(&mut self, key: &str, chat_type: ChatType, channel: i32) -> Say {
        let (tag, text) = self.tagged(key);
        Say {
            key: key.to_owned(),
            chat_type,
            channel,
            tag,
            text,
        }
    }

    /// A say on the public channel whose text is `bytes` long, the last of
    /// them `filler` over and over.
    fn long_line(&mut self, key: &str, bytes: usize, filler: &str) -> Say {
        let (tag, mut text) = self.tagged("");
        while text.len().saturating_add(filler.len()) <= bytes {
            text.push_str(filler);
        }
        Say {
            key: key.to_owned(),
            chat_type: ChatType::Normal,
            channel: 0,
            tag,
            text,
        }
    }

    /// Has `who` send `commands`, a moment apart, and returns what the two
    /// avatars were then sent — once `done` says the sender has what the leg
    /// came for.
    async fn act_until(
        &mut self,
        who: Who,
        commands: Vec<Command>,
        done: impl FnMut(&Got) -> bool,
    ) -> Result<Brought, TestFailure> {
        let several = commands.len() > 1;
        let (actor, actor_id, listener, listener_id) = match who {
            Who::A => (&mut *self.a, self.scene.a, &mut *self.b, self.scene.b),
            Who::B => (&mut *self.b, self.scene.b, &mut *self.a, self.scene.a),
        };
        for command in commands {
            actor.send(command).await?;
            if several {
                tokio::time::sleep(BETWEEN).await;
            }
        }
        let acted = watch_for(actor, actor_id, STEP_TIMEOUT, self.quiet, done).await?;
        let listened = watch(listener, listener_id, self.drain).await?;
        let brought = match who {
            Who::A => Brought {
                a: acted,
                b: listened,
            },
            Who::B => Brought {
                a: listened,
                b: acted,
            },
        };
        if let Some(at) = &brought.a.own {
            self.scene.a_at = at.clone();
        }
        if let Some(at) = &brought.b.own {
            self.scene.b_at = at.clone();
        }
        Ok(brought)
    }

    /// Has `who` send `commands`, a moment apart, and returns what the two
    /// avatars were then sent.
    async fn act(&mut self, who: Who, commands: Vec<Command>) -> Result<Brought, TestFailure> {
        self.act_until(who, commands, |_got| true).await
    }

    /// Has `who` say `says` and records under `prefix` and each one's key
    /// what the avatars were sent of it.
    async fn lines(
        &mut self,
        metrics: &mut Metrics,
        who: Who,
        prefix: &str,
        says: &[Say],
        record: Record,
    ) -> Result<Brought, TestFailure> {
        let commands = says
            .iter()
            .map(|say| Command::Chat {
                message: say.text.clone(),
                chat_type: say.chat_type,
                channel: ChatChannel(say.channel),
            })
            .collect();
        let brought = self.act(who, commands).await?;
        for say in says {
            let wanted = Wanted::Prefix(say.tag.clone());
            let told_a = self.scene.told(&brought.a, &wanted, &say.text);
            let told_b = self.scene.told(&brought.b, &wanted, &say.text);
            let key = &say.key;
            if record == Record::Both || who == Who::B {
                metrics.set(&format!("{prefix}{key}_told_a"), told_a);
            }
            if record == Record::Both || who == Who::A {
                metrics.set(&format!("{prefix}{key}_told_b"), told_b);
            }
        }
        Ok(brought)
    }

    /// Records how many bytes of each of `says` the lines in `heard` kept,
    /// and that with the bytes of the name the line came under.
    fn kept(metrics: &mut Metrics, says: &[Say], heard: &Got) {
        for say in says {
            let Some(line) = heard
                .lines
                .iter()
                .find(|line| line.message.starts_with(say.tag.as_str()))
            else {
                continue;
            };
            let kept = i64::try_from(line.message.len()).unwrap_or(i64::MAX);
            let name = i64::try_from(line.from_name.len()).unwrap_or(i64::MAX);
            metrics.set(&format!("{}_kept_bytes", say.key), kept);
            metrics.set(
                &format!("{}_kept_bytes_and_name_bytes", say.key),
                kept.saturating_add(name),
            );
        }
    }

    /// Has A send a start (or a stop) of typing — as [`Command::Typing`]
    /// sends it, or with `text` on `channel` — and records under `key` what
    /// the avatars were sent of it.
    async fn typing(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        start: bool,
        (text, channel): (&str, i32),
        record: Record,
    ) -> Result<(), TestFailure> {
        let chat_type = if start {
            ChatType::StartTyping
        } else {
            ChatType::StopTyping
        };
        let brought = self
            .act(
                Who::A,
                vec![Command::Chat {
                    message: text.to_owned(),
                    chat_type,
                    channel: ChatChannel(channel),
                }],
            )
            .await?;
        if record == Record::Both {
            metrics.set(
                &format!("{key}_told_a"),
                self.scene.typed(&brought.a, self.scene.a, start),
            );
        }
        metrics.set(
            &format!("{key}_told_b"),
            self.scene.typed(&brought.b, self.scene.a, start),
        );
        Ok(())
    }

    /// Has B tell the probe to do `order` and returns what the two avatars
    /// were sent by the time it said it had.
    async fn order(&mut self, order: &str) -> Result<Brought, TestFailure> {
        let done = format!("D|{order}");
        let brought = self
            .act_until(
                Who::B,
                vec![Command::Chat {
                    message: order.to_owned(),
                    chat_type: ChatType::Normal,
                    channel: ChatChannel(PROBE_CHANNEL),
                }],
                |got| got.said(&done),
            )
            .await?;
        if brought.b.said(&done) {
            Ok(brought)
        } else {
            Err(TestFailure::Assertion(format!(
                "the probe never said it had done `{order}`"
            )))
        }
    }

    /// Which lines the probe's `order` makes, and the text it says.
    fn ordered(&self, order: &str) -> (Wanted, String) {
        let prim = self
            .scene
            .prim
            .as_ref()
            .map_or_else(Uuid::nil, |(prim, ..)| *prim);
        match order {
            "empty_say" => (Wanted::EmptyFrom(prim), String::new()),
            "long_say" => (
                Wanted::Prefix(format!("{PROBE_TAG}long_say|")),
                long_say_text(),
            ),
            other => {
                let text = format!("{PROBE_TAG}{other}");
                (Wanted::Exact(text.clone()), text)
            }
        }
    }

    /// Has the probe do `order` and records under `prefix` what its owner
    /// was sent — and what B was, where `record` asks for both.
    async fn script(
        &mut self,
        metrics: &mut Metrics,
        prefix: &str,
        order: &str,
        record: Record,
    ) -> Result<(), TestFailure> {
        let brought = self.order(order).await?;
        let (wanted, text) = self.ordered(order);
        metrics.set(
            &format!("{prefix}script_{order}_told_a"),
            self.scene.told(&brought.a, &wanted, &text),
        );
        if record == Record::Both {
            metrics.set(
                &format!("{prefix}script_{order}_told_b"),
                self.scene.told(&brought.b, &wanted, &text),
            );
        }
        Ok(())
    }

    /// Makes a new script in `prim`, saves the probe over it and waits for it
    /// to say it runs; the name the probe says its prim has.
    async fn install(&mut self, prim: &Object) -> Result<String, TestFailure> {
        let target = prim.scoped_id();
        let task = prim.full_id;
        let owner = AgentKey::from(self.scene.a);
        self.a
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
            listen(self.a, Duration::from_millis(500)).await?;
            let held = listing(self.a, target, task).await?;
            if let Some(item) = held.items.iter().find(|item| item.name == PROBE_NAME) {
                break item.item_id;
            }
            if started.elapsed() >= STEP_TIMEOUT {
                return Err(TestFailure::Assertion(
                    "the new script never appeared in the cube".to_owned(),
                ));
            }
        };
        self.a
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
        let got = watch_for(self.a, self.scene.a, STEP_TIMEOUT, self.quiet, |got| {
            got.uploaded.is_some() && got.said_after("R|").is_some()
        })
        .await?;
        watch(self.b, self.scene.b, self.drain).await?;
        match (&got.uploaded, got.said_after("R|")) {
            (Some(Ok(true)), Some(name)) => Ok(name.to_owned()),
            (Some(Ok(true)), None) => Err(TestFailure::Assertion(
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

    /// The record of the parcel A stands on, if the grid answers with one.
    async fn parcel_here(&mut self) -> Result<Option<ParcelInfo>, TestFailure> {
        let at = self.scene.a_at.clone();
        self.a
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
            .a
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

    /// The chat ranges the region's `RegionInfo` states, as a metric names
    /// them.
    async fn stated_ranges(&mut self) -> Result<String, TestFailure> {
        self.a.send(Command::RequestRegionInfo).await?;
        let answer = self
            .a
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::RegionLimits(limits) => Some(limits.chat_settings.clone()),
                _ => None,
            })
            .await;
        match answer {
            Ok(Some(chat)) => Ok(format!(
                "whisper {:.0} m, say {:.0} m, shout {:.0} m, offsets {:.0}/{:.0}/{:.0}, flags {}",
                chat.whisper_range,
                chat.normal_range,
                chat.shout_range,
                chat.whisper_offset,
                chat.normal_offset,
                chat.shout_offset,
                chat.flags,
            )),
            Ok(None) => Ok("none stated".to_owned()),
            Err(TestFailure::Timeout(_)) => Ok("no answer".to_owned()),
            Err(other) => Err(other),
        }
    }

    /// Listens to both avatars for `window` and brings where they are up to
    /// date.
    async fn follow(&mut self, window: Duration) -> Result<(), TestFailure> {
        let got = watch(self.a, self.scene.a, window).await?;
        if let Some(at) = got.own {
            self.scene.a_at = at;
        }
        let got = watch(self.b, self.scene.b, self.drain).await?;
        if let Some(at) = got.own {
            self.scene.b_at = at;
        }
        Ok(())
    }

    /// Listens to A alone for `window` and brings where it is up to date:
    /// for while a key of A's is held, which listening to B as well would
    /// hold for longer.
    async fn follow_a(&mut self, window: Duration) -> Result<(), TestFailure> {
        let got = watch(self.a, self.scene.a, window).await?;
        if let Some(at) = got.own {
            self.scene.a_at = at;
        }
        Ok(())
    }

    /// Has A fly a few metres west, away from B and from the cube east of
    /// it. Two avatars that logged in at one spot stand one on the other,
    /// and the lower one would carry the upper one up with it.
    async fn step_aside(&mut self) -> Result<(), TestFailure> {
        let west = crate::crossing::Edge::West.facing();
        self.a
            .send(Command::SetRotation {
                body: west.clone(),
                head: west,
            })
            .await?;
        self.a
            .send(Command::SetControls(
                ControlFlags::FLY | ControlFlags::AT_POS,
            ))
            .await?;
        self.follow_a(STEP_ASIDE).await?;
        self.a.send(Command::SetControls(ControlFlags::FLY)).await?;
        self.follow(HOVER).await
    }

    /// Puts the camera of `who` a metre above `at`, looking along the ground.
    async fn camera(&self, who: Who, at: &Vector) -> Result<(), TestFailure> {
        let eye = Vector {
            x: at.x,
            y: at.y,
            z: at.z + 1.0,
        };
        let target = Vector {
            x: self.scene.b_at.x + 5.0,
            y: self.scene.b_at.y,
            z: self.scene.b_at.z,
        };
        let session = match who {
            Who::A => &*self.a,
            Who::B => &*self.b,
        };
        session
            .send(Command::SetCamera(Camera::looking_at(eye, target)))
            .await
    }

    /// Has A fly up until it is at least `least` metres above B, in pushes
    /// sized by what is left to climb.
    async fn rise(&mut self, least: f32) -> Result<(), TestFailure> {
        let fly = ControlFlags::FLY;
        let started = Instant::now();
        loop {
            // Above B and above the prim: a cube rezzed beside an avatar that
            // had not yet come down from its flight hangs well over the
            // ground B stands on, and its lines have their reach as well.
            let floor = self
                .scene
                .prim
                .as_ref()
                .map_or(self.scene.b_at.z, |(_prim, at, _name)| {
                    at.z.max(self.scene.b_at.z)
                });
            let short = least - (self.scene.a_at.z - floor);
            if short <= 0.0 {
                break;
            }
            if started.elapsed() >= CLIMB_LIMIT {
                return Err(TestFailure::Assertion(format!(
                    "avatar A did not climb {least} m above B: it is at {}",
                    vector_text(&self.scene.a_at)
                )));
            }
            // A third of what is left at a time: OpenSim lifts an avatar a
            // good deal faster than the speed this is planned at, and a push
            // sized to the whole of it carried A past the step's band.
            let push = Duration::from_secs_f32((short / CLIMB_M_PER_S / 3.0).clamp(0.1, 4.0));
            self.a
                .send(Command::SetControls(fly | ControlFlags::UP_POS))
                .await?;
            self.follow_a(push).await?;
            self.a.send(Command::SetControls(fly)).await?;
            self.follow(HOVER).await?;
        }
        Ok(())
    }

    /// Puts A `placed` metres above B by teleport.
    async fn place(&mut self, placed: f32) -> Result<(), TestFailure> {
        let target = Vector {
            x: self.scene.b_at.x,
            y: self.scene.b_at.y,
            z: self.scene.b_at.z + placed,
        };
        self.a
            .send(Command::Teleport {
                region_handle: self.home,
                position: RegionCoordinates::new(target.x, target.y, target.z),
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            })
            .await?;
        let landing = self
            .a
            .wait_for(REGION_TIMEOUT, |event| match event {
                Event::TeleportLocal { position, .. } => Some(*position),
                _ => None,
            })
            .await?;
        self.scene.a_at = Vector {
            x: landing.x(),
            y: landing.y(),
            z: landing.z(),
        };
        Ok(())
    }

    /// Moves A to `step` of its way up — flying, or by teleport where the
    /// grid moves no avatar — and holds the distance to the step's band.
    async fn move_to(&mut self, step: &Step) -> Result<f32, TestFailure> {
        if self.teleports {
            self.place(step.placed).await?;
        } else {
            self.rise(step.least).await?;
        }
        let at = self.scene.a_at.clone();
        self.camera(Who::A, &at).await?;
        self.follow(Duration::from_secs(1)).await?;
        let apart = distance(&self.scene.a_at, &self.scene.b_at);
        let from_the_prim = self
            .scene
            .prim
            .as_ref()
            .map_or(apart, |(_prim, at, _name)| distance(&self.scene.a_at, at));
        for (what, metres) in [("B", apart), ("the prim", from_the_prim)] {
            if metres <= step.band.0 || metres >= step.band.1 {
                return Err(TestFailure::Assertion(format!(
                    "the `{}` step wants avatar A between {} m and {} m from {what}, and it is \
                     {metres:.1} m",
                    step.word, step.band.0, step.band.1
                )));
            }
        }
        Ok(apart)
    }

    /// Brings A down to the ground at `ground` and lets it stand.
    async fn land(&mut self, ground: f32) -> Result<(), TestFailure> {
        let fly = ControlFlags::FLY;
        let started = Instant::now();
        self.a
            .send(Command::SetControls(fly | ControlFlags::UP_NEG))
            .await?;
        while self.scene.a_at.z > ground + LANDED_M && started.elapsed() < DESCENT_LIMIT {
            self.follow(Duration::from_secs(1)).await?;
        }
        self.a
            .send(Command::SetControls(ControlFlags::empty()))
            .await?;
        self.follow(HOVER).await?;
        self.a.send(Command::FinishAnimation).await?;
        let at = self.scene.a_at.clone();
        self.camera(Who::A, &at).await?;
        self.follow(HOVER).await
    }
}

/// A census of local chat.
#[derive(Debug)]
pub struct LocalChat;

impl GridTest for LocalChat {
    fn name(&self) -> &'static str {
        "local-chat"
    }

    fn description(&self) -> &'static str {
        "What a region passes on of a viewer's and a script's lines of local chat, to whom, how \
         far, and what it says of each"
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

/// The legs that need no script and no distance: every type, typing,
/// channels, text and a burst.
async fn beside_legs(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    // --- 1. Every chat type. ---
    let says: Vec<Say> = TYPES
        .iter()
        .map(|(code, word)| census.line(word, ChatType::from_u8(*code), 0))
        .collect();
    census
        .lines(metrics, Who::A, "", &says, Record::Both)
        .await?;

    // --- 2. Typing. ---
    census
        .typing(metrics, "typing_start", true, ("", 0), Record::Both)
        .await?;
    census
        .typing(metrics, "typing_stop", false, ("", 0), Record::Both)
        .await?;
    // A stop nobody saw a start of, a second time.
    census
        .typing(metrics, "typing_stop_again", false, ("", 0), Record::Both)
        .await?;
    let (_tag, text) = census.tagged("typing");
    census
        .typing(
            metrics,
            "typing_start_with_text",
            true,
            (&text, 0),
            Record::Both,
        )
        .await?;
    census.act(Who::A, vec![Command::Typing(false)]).await?;
    census
        .typing(
            metrics,
            "typing_start_on_channel_5",
            true,
            ("", 5),
            Record::Both,
        )
        .await?;
    census.act(Who::A, vec![Command::Typing(false)]).await?;

    // --- 3. Channels. ---
    let mut says: Vec<Say> = CHANNELS
        .iter()
        .map(|(channel, word)| census.line(&format!("say_on_{word}"), ChatType::Normal, *channel))
        .collect();
    says.push(census.line("shout_on_the_debug_channel", ChatType::Shout, DEBUG_CHANNEL));
    says.push(census.line(
        "whisper_on_the_debug_channel",
        ChatType::Whisper,
        DEBUG_CHANNEL,
    ));
    census
        .lines(metrics, Who::A, "", &says, Record::Both)
        .await?;

    // --- 4. Text. ---
    // No text at all: nothing to tag, so it is asked alone.
    let brought = census
        .act(
            Who::A,
            vec![Command::Chat {
                message: String::new(),
                chat_type: ChatType::Normal,
                channel: ChatChannel(0),
            }],
        )
        .await?;
    let empty = Wanted::EmptyFrom(census.scene.a);
    metrics.set(
        "say_of_no_text_told_a",
        census.scene.told(&brought.a, &empty, ""),
    );
    metrics.set(
        "say_of_no_text_told_b",
        census.scene.told(&brought.b, &empty, ""),
    );
    let mut says = vec![
        census.long_line("say_of_1000_bytes", 1000, "x"),
        census.long_line("say_of_1023_bytes", 1023, "x"),
        census.long_line("say_of_1024_bytes", 1024, "x"),
        census.long_line("say_of_1100_bytes", 1100, "x"),
        census.long_line("say_of_2000_bytes", 2000, "x"),
        census.long_line("say_of_3000_bytes", 3000, "x"),
        // Two bytes a character, an odd number of bytes before them: a cut at
        // an even count of bytes falls inside a character.
        census.long_line("say_of_1200_bytes_of_two_byte_characters", 1200, "\u{e9}"),
    ];
    let (tag, text) = census.tagged("line\nbreak\tand tab");
    says.push(Say {
        key: "say_with_a_line_break".to_owned(),
        chat_type: ChatType::Normal,
        channel: 0,
        tag,
        text,
    });
    let (tag, text) = census.tagged("spaces after   ");
    says.push(Say {
        key: "say_ending_in_spaces".to_owned(),
        chat_type: ChatType::Normal,
        channel: 0,
        tag,
        text,
    });
    let brought = census
        .lines(metrics, Who::A, "", &says, Record::Both)
        .await?;
    Census::kept(metrics, &says, &brought.b);
    // The same from the other avatar, whose name is another: a grid that
    // cuts a line to what fits a packet cuts it by the name it sends it under.
    let says = vec![
        census.long_line("say_of_2000_bytes_of_b", 2000, "x"),
        census.long_line("say_of_3000_bytes_of_b", 3000, "x"),
    ];
    let brought = census
        .lines(metrics, Who::B, "", &says, Record::Listener)
        .await?;
    Census::kept(metrics, &says, &brought.a);
    // An emote begins with `/me`, so it cannot begin with a tag: asked alone
    // and found by its ending.
    let (tag, _text) = census.tagged("");
    let emote = format!("/me {tag}waves");
    let brought = census
        .act(
            Who::A,
            vec![Command::Chat {
                message: emote.clone(),
                chat_type: ChatType::Normal,
                channel: ChatChannel(0),
            }],
        )
        .await?;
    let wanted = Wanted::Exact(emote.clone());
    metrics.set(
        "say_of_an_emote_told_b",
        census.scene.told(&brought.b, &wanted, &emote),
    );

    // --- 5. Twenty in about a second. ---
    let burst: Vec<Say> = std::iter::repeat_with(|| census.line("burst", ChatType::Normal, 0))
        .take(usize::from(BURST))
        .collect();
    for say in &burst {
        census
            .a
            .send(Command::Chat {
                message: say.text.clone(),
                chat_type: say.chat_type,
                channel: ChatChannel(say.channel),
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let brought = census.act(Who::A, Vec::new()).await?;
    let passed = burst
        .iter()
        .filter(|say| brought.b.lines.iter().any(|line| line.message == say.text))
        .count();
    metrics.set(
        "say_burst_of_twenty_passed_on",
        i64::try_from(passed).unwrap_or(i64::MAX),
    );
    Ok(())
}

/// The legs a script is needed for while the avatars stand together: a
/// script's lines, and a viewer's line on a channel a script listens on.
async fn script_legs(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    // --- 6. A script's lines. ---
    for order in SCRIPT_LEGS {
        census.script(metrics, "", order, Record::Both).await?;
    }

    // --- 7. A channel and a script. ---
    let says: Vec<Say> = HEARD_CHANNELS
        .iter()
        .map(|(channel, word)| census.line(&format!("say_on_{word}"), ChatType::Normal, *channel))
        .collect();
    let commands = says
        .iter()
        .map(|say| Command::Chat {
            message: say.text.clone(),
            chat_type: say.chat_type,
            channel: ChatChannel(say.channel),
        })
        .collect();
    let brought = census.act(Who::A, commands).await?;
    for say in &says {
        let report = format!("H|{}|{}", say.channel, say.text);
        metrics.set(
            &format!("{}_reached_a_script", say.key),
            brought.a.said(&report),
        );
    }
    Ok(())
}

/// Leg 8: A moves away step by step, and its lines, its typing and the
/// script's lines are asked again at each.
async fn distance_legs(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let scripted = census.scene.prim.is_some();
    if !census.teleports {
        census.step_aside().await?;
    }
    for (index, step) in STEPS.iter().enumerate() {
        let apart = census.move_to(step).await?;
        let word = step.word;
        metrics.set(&format!("{word}_distance_m"), f64::from(apart));
        let prefix = format!("{word}_");
        let says = vec![
            census.line("whisper", ChatType::Whisper, 0),
            census.line("say", ChatType::Normal, 0),
            census.line("shout", ChatType::Shout, 0),
            census.line("debug_type", ChatType::DebugChannel, 0),
            census.line("say_on_the_debug_channel", ChatType::Normal, DEBUG_CHANNEL),
        ];
        census
            .lines(metrics, Who::A, &prefix, &says, Record::Listener)
            .await?;
        // The other way round: the one that stayed speaks.
        let says = vec![
            census.line("whisper_of_b", ChatType::Whisper, 0),
            census.line("say_of_b", ChatType::Normal, 0),
            census.line("shout_of_b", ChatType::Shout, 0),
        ];
        census
            .lines(metrics, Who::B, &prefix, &says, Record::Listener)
            .await?;
        census
            .typing(
                metrics,
                &format!("{word}_typing_start"),
                true,
                ("", 0),
                Record::Listener,
            )
            .await?;
        census
            .typing(
                metrics,
                &format!("{word}_typing_stop"),
                false,
                ("", 0),
                Record::Listener,
            )
            .await?;
        if index == 0 {
            // Past a whisper's reach, with a camera where the other avatar
            // is: a grid that measures from the camera passes the whisper on.
            let (a_at, b_at) = (census.scene.a_at.clone(), census.scene.b_at.clone());
            census.camera(Who::A, &b_at).await?;
            census.follow(Duration::from_secs(2)).await?;
            let prefix = format!("{word}_with_the_camera_of_a_beside_b_");
            let says = vec![census.line("whisper", ChatType::Whisper, 0)];
            census
                .lines(metrics, Who::A, &prefix, &says, Record::Listener)
                .await?;
            let says = vec![census.line("whisper_of_b", ChatType::Whisper, 0)];
            census
                .lines(metrics, Who::B, &prefix, &says, Record::Listener)
                .await?;
            census.camera(Who::A, &a_at).await?;
            census.camera(Who::B, &a_at).await?;
            census.follow(Duration::from_secs(2)).await?;
            let prefix = format!("{word}_with_the_camera_of_b_beside_a_");
            let says = vec![census.line("whisper", ChatType::Whisper, 0)];
            census
                .lines(metrics, Who::A, &prefix, &says, Record::Listener)
                .await?;
            census.camera(Who::B, &b_at).await?;
            census.follow(Duration::from_secs(2)).await?;
        }
        if scripted {
            for order in RANGE_SCRIPT_LEGS {
                census
                    .script(metrics, &prefix, order, Record::Listener)
                    .await?;
            }
        }
    }
    Ok(())
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
    // Where avatars are neither hidden nor silenced, where the fixtures name
    // such a spot: a sandbox may be a parcel that hides its avatars.
    let build_position = ctx.sound_position().or_else(|| ctx.build_position());
    let pace = Pace::of(grid);
    let drain = if grid.is_fake() { FAKE_DRAIN } else { DRAIN };
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "local-chat needs a secondary avatar".to_owned(),
        ));
    };
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

    // Both avatars to the spot, each with its scene settled.
    a.wait_for_region(REGION_TIMEOUT).await?;
    let settled = settle_scene_with_avatar(
        a,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let theirs =
        settle_scene_with_avatar(b, grid, build_position, SETTLE_WINDOW, pace.settle_idle).await?;
    let a_at = settled.avatar.ok_or_else(|| {
        TestFailure::Assertion("avatar A was never sent its own avatar".to_owned())
    })?;
    let b_at = theirs.avatar.ok_or_else(|| {
        TestFailure::Assertion("avatar B was never sent its own avatar".to_owned())
    })?;
    let home = a
        .region_handle()
        .ok_or_else(|| TestFailure::Assertion("avatar A is in no region".to_owned()))?;
    if b.region_handle() != Some(home) {
        return Err(TestFailure::Assertion(
            "the two avatars did not log in to the same region".to_owned(),
        ));
    }
    let mut run = Run {
        folders,
        owner: a_id,
        pace,
        position: Vector {
            x: a_at.x + CUBE_AWAY_M,
            y: a_at.y,
            z: a_at.z + REZ_LIFT_M,
        },
        seen: settled.seen,
    };
    let scene = Scene {
        a: a_id.uuid(),
        b: b_id.uuid(),
        a_name: a.avatar_legacy_name(),
        b_name: b.avatar_legacy_name(),
        a_at,
        b_at,
        prim: None,
    };
    let mut census = Census {
        a,
        b,
        scene,
        home,
        quiet: run.pace.quiet,
        drain,
        said: 0,
        teleports: grid.is_fake(),
    };
    // A viewer's camera is where its avatar is; a session's is wherever it
    // was last put.
    let (a_at, b_at) = (census.scene.a_at.clone(), census.scene.b_at.clone());
    census.camera(Who::A, &a_at).await?;
    census.camera(Who::B, &b_at).await?;
    census.follow(drain).await?;
    // Side by side: a flight to the spot ends wherever it comes down.
    if distance(&census.scene.a_at, &census.scene.b_at) > BESIDE_M {
        let (from, to) = (census.scene.b_at.clone(), census.scene.a_at.clone());
        census.scene.b_at = walk_within_region(census.b, from, &to, |_event| {}).await?;
        census.follow(HOVER).await?;
        census.b.send(Command::FinishAnimation).await?;
        let at = census.scene.b_at.clone();
        census.camera(Who::B, &at).await?;
        census.follow(HOVER).await?;
    }
    metrics.set(
        "start_distance_m",
        f64::from(distance(&census.scene.a_at, &census.scene.b_at)),
    );
    let parcel = census.parcel_here().await?;
    metrics.set(
        "parcel_shows_its_avatars",
        match parcel.as_ref().map(|parcel| parcel.see_avs) {
            None => "no answer",
            Some(Some(true)) => "yes",
            Some(Some(false)) => "no",
            Some(None) => "not stated",
        },
    );
    let ranges = census.stated_ranges().await?;
    metrics.set("region_info_chat_ranges", ranges);

    beside_legs(&mut census, metrics).await?;
    if grid.is_fake() {
        // Nothing runs a script here.
        return distance_legs(&mut census, metrics).await;
    }
    // Beside where A stands by now: it was still coming down from its
    // flight to the spot when the scene settled, and a cube rezzed beside
    // where it was then hangs in the air.
    run.position = Vector {
        x: census.scene.a_at.x + CUBE_AWAY_M,
        y: census.scene.a_at.y,
        z: census.scene.a_at.z + REZ_LIFT_M,
    };
    let cube = rez_cube(census.a, &mut run, residue, 0).await?;
    find_object(census.b, cube.full_id).await?;
    watch(census.b, census.scene.b, census.drain).await?;
    census.scene.prim = Some((
        cube.full_id.uuid(),
        cube.motion.position.clone(),
        String::new(),
    ));
    let name = census.install(&cube).await?;
    if let Some((_prim, _at, prim_name)) = &mut census.scene.prim {
        *prim_name = name;
    }
    script_legs(&mut census, metrics).await?;
    let ground = census.scene.a_at.z;
    distance_legs(&mut census, metrics).await?;
    census.land(ground).await
}

#[cfg(test)]
mod tests {
    use super::{LONG_SAY_BYTES, long_say_text, probe_source, text_text};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_probe_compiles() -> Result<(), String> {
        let source = probe_source();
        sl_lsl_runtime::compile(&source)
            .map(|_program| ())
            .map_err(|errors| format!("the probe does not compile: {errors:?}\n{source}"))
    }

    #[test]
    fn what_a_metric_says_of_a_text() {
        assert_eq!(text_text("sentence", "sentence"), "as sent");
        assert_eq!(text_text("", "sentence"), "none");
        assert_eq!(text_text("sent", "sentence"), "the first 4 bytes");
        assert_eq!(text_text("send", "sentence"), "other (4 bytes)");
        assert!(long_say_text().len() >= LONG_SAY_BYTES);
    }
}
