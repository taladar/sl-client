//! A census of instant messages: what a grid passes on of one, what it says
//! of it, what it tells the sender, and what it keeps for an avatar that is
//! away.
//!
//! A viewer's `ImprovedInstantMessage` ([`Command::InstantMessage`],
//! [`Command::ImTyping`], [`Command::AutoResponse`]) and a script's
//! `llInstantMessage` both reach their avatar as an `ImprovedInstantMessage`
//! ([`Event::InstantMessageReceived`], [`Event::ImTyping`]): a dialog, who it
//! is from and under what name, an id, a region and a place, whether it was
//! kept while the avatar was away, a time, an estate, a bucket and a text.
//!
//! Avatar **A** writes and owns the probe prim; avatar **B** is written to,
//! mutes A for a while and goes away three times. One question a leg:
//!
//! 1. **A message**: every field of it as B is sent it, and whether A is
//!    told anything.
//! 2. **Text**: no text, a line break, and text at and past the length a
//!    message may have.
//! 3. **Typing** and an **automatic reply**.
//! 4. **Whom**: a message to oneself, to an id that is nobody's and to no id.
//! 5. **What a viewer may claim**: a message whose name, id, region, place,
//!    time, estate and bucket are made up, one flagged as kept, and dialogs
//!    a viewer does not send.
//! 6. **A burst**: twenty messages in a second.
//! 7. **A script's message**: to another avatar, to its owner, to nobody,
//!    without text and with too much of it.
//! 8. **A mute**: B mutes A; A's message, its typing and its prim's message;
//!    B's message to the avatar it muted.
//! 9. **Away**: B logs out and A sends it two messages, a typing notice, an
//!    automatic reply, a long message and a script's message. B comes back
//!    and is first left alone, then asks for what was kept — over
//!    `RetrieveInstantMessages` the first time, by reading the
//!    `ReadOfflineMsgs` capability itself the second, and as the client
//!    reads it ([`Command::RequestOfflineMessages`]) the third — and asks
//!    again by the other road.
//!
//! The script is the instrument of leg 7 and of the script's part of legs 8
//! and 9: it sends what its owner tells it to on a channel and says when it
//! has.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/chat.md` § Instant messages; every run is held to it.
//!
//! `2av`, `[both]`, and both fake flavours, which run no script: there the
//! cube is not rezzed, the script's legs are not asked, and the metrics in
//! `FAKE_EXEMPT` are not held. The cube is derezzed to the Trash, also when
//! a leg fails, and B's mute of A is taken back.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sl_client_tokio::{
    AgentKey, AnyMessage, ChatChannel, ChatMessage, ChatType, Command, Event, ImDialog,
    InstantMessage, MuteFlags, MuteType, Object, Reliability, RestoreItem, RezScriptParams,
    ScriptTarget, ScriptUploadLocation, Uuid, Vector,
};
use sl_wire::Llsd;
use sl_wire::messages::{
    ImprovedInstantMessage, ImprovedInstantMessageAgentDataBlock,
    ImprovedInstantMessageEstateBlockBlock, ImprovedInstantMessageMessageBlockBlock,
};

use crate::cases::asset_http::without_ids;
use crate::cases::effects_sounds::distance;
use crate::cases::local_chat::text_text;
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, STEP_TIMEOUT, clean_up,
    rez_cube,
};
use crate::cases::task_inventory::{
    DRAIN, OPENSIM_START, cap_get, hold_to_measured, http_client, listen, listing, login_folders,
    shape_of,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, settle_scene_with_avatar};

/// What every line the probe says to its owner begins with.
const PROBE_TAG: &str = "SLCIM|";

/// What every text of the census begins with.
const TEXT_TAG: &str = "im-";

/// The name the probe script is saved under.
const PROBE_NAME: &str = "SLClientImProbe";

/// The channel the probe takes its orders on.
const PROBE_CHANNEL: i32 = 7_734_293;

/// How long the script's long message is, in bytes.
const LONG_SCRIPT_BYTES: usize = 1500;

/// How long the long message is that is sent to an avatar that is away, in
/// bytes.
const LONG_AWAY_BYTES: usize = 2000;

/// How long a fake grid is given to drop an avatar that logged out: it does
/// at once.
const FAKE_AWAY_SETTLE: Duration = Duration::from_secs(1);

/// How long an avatar that came back to a fake grid is left alone.
const FAKE_UNASKED_WINDOW: Duration = Duration::from_secs(3);

/// How long an avatar waits on a fake grid for what it asked for.
const FAKE_FETCH_WINDOW: Duration = Duration::from_secs(3);

/// How long after the last kept message a fetch from a fake grid is still
/// listened to.
const FAKE_FETCH_QUIET: Duration = Duration::from_secs(1);

/// How long a fake grid is given to act on a mute, and to answer for the mute
/// list.
const FAKE_MUTE_SETTLE: Duration = Duration::from_millis(500);

/// How long the capability map may take to arrive after a login on a fake
/// grid.
const FAKE_CAP_WAIT: Duration = Duration::from_secs(3);

/// The pause between two commands of one leg.
const BETWEEN: Duration = Duration::from_millis(150);

/// How many messages the burst is.
const BURST: u8 = 20;

/// How far beside avatar A the cube is rezzed, in metres.
const CUBE_AWAY_M: f32 = 2.0;

/// How far from where its source stands a message may be stated and still be
/// stated there, in metres.
const AT_M: f32 = 2.5;

/// How far from an instant a message's time may be and still be that
/// instant's, in seconds: the two clocks are a grid's and ours.
const TIME_SLACK_SECS: u64 = 8;

/// How long a grid is given to act on a mute or on its removal.
const MUTE_SETTLE: Duration = Duration::from_secs(3);

/// How long a mute list may take to arrive.
const MUTE_LIST_WINDOW: Duration = Duration::from_secs(15);

/// How long a grid is given to drop an avatar that logged out, so that the
/// next message is kept for it rather than sent after a closing session.
const AWAY_SETTLE: Duration = Duration::from_secs(8);

/// How long an avatar that came back is left alone, to see what it is sent
/// without asking.
const UNASKED_WINDOW: Duration = Duration::from_secs(25);

/// How long an avatar waits for what it asked for.
const FETCH_WINDOW: Duration = Duration::from_secs(20);

/// How long after the last kept message a fetch is still listened to.
const FETCH_QUIET: Duration = Duration::from_secs(5);

/// How long the capability is asked again for while it answers nothing.
const CAP_BUDGET: Duration = Duration::from_secs(40);

/// The pause between two readings of the capability.
const CAP_AGAIN: Duration = Duration::from_secs(5);

/// How long the capability map may take to arrive after a login.
const CAP_WAIT: Duration = Duration::from_secs(15);

/// The capability that serves what was kept for an avatar.
const READ_OFFLINE_MSGS: &str = "ReadOfflineMsgs";

/// What a metric reads where the grid has no such capability.
const NO_CAPABILITY: &str = "no capability";

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/chat.md § Instant messages (instant-messages, 2026-10-13)";

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

/// What each grid was measured to answer, by the metric the census records it
/// under: every run is held to it.
///
/// Not held on Second Life: how many messages of the burst arrive before it
/// suspends the sender (fourteen of twenty, twice).
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("auto_response_told_a", both(Answer::Text("nothing"))),
    (
        "auto_response_told_b",
        each(
            Answer::Text(
                "DoNotDisturbAutoResponse from avatar A, A's name, to avatar B, id the session of the two avatars, no region, at zero, not kept, no time, estate 0, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "DoNotDisturbAutoResponse from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    (
        "away_auto_response_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "DoNotDisturbAutoResponse from avatar A, the name \"System\", to avatar B, id the session of the two avatars, no region, at zero, not kept, the time it arrived, estate 0, not from a group, no bucket, text \"Unable to send instant message. User is not logged in.\"",
            ),
        ),
    ),
    (
        "away_first_told_a",
        each(
            Answer::Text(
                "Message from avatar B, the name \"Second Life\", to avatar A, id the session of the two avatars, no region, at zero, not kept, no time, estate 0, not from a group, a bucket of one zero byte, text \"User not online - message will be stored and delivered later.\"",
            ),
            Answer::Text(
                "Message from avatar B, the name \"System\", to avatar A, id the session of the two avatars, no region, at zero, not kept, the time it arrived, estate 0, not from a group, no bucket, text \"User is not logged in. Message saved.\"",
            ),
        ),
    ),
    (
        "away_long_told_a",
        each(
            Answer::Text(
                "Message from avatar B, the name \"Second Life\", to avatar A, id the session of the two avatars, no region, at zero, not kept, no time, estate 0, not from a group, a bucket of one zero byte, text \"User not online - message will be stored and delivered later.\"",
            ),
            Answer::Text(
                "Message from avatar B, the name \"System\", to avatar A, id the session of the two avatars, no region, at zero, not kept, the time it arrived, estate 0, not from a group, no bucket, text \"User is not logged in. Message saved.\"",
            ),
        ),
    ),
    ("away_script_told_a", both(Answer::Text("nothing"))),
    (
        "away_second_told_a",
        each(
            Answer::Text(
                "Message from avatar B, the name \"Second Life\", to avatar A, id the session of the two avatars, no region, at zero, not kept, no time, estate 0, not from a group, a bucket of one zero byte, text \"User not online - message will be stored and delivered later.\"",
            ),
            Answer::Text(
                "Message from avatar B, the name \"System\", to avatar A, id the session of the two avatars, no region, at zero, not kept, the time it arrived, estate 0, not from a group, no bucket, text \"User is not logged in. Message saved.\"",
            ),
        ),
    ),
    ("away_typing_told_a", both(Answer::Text("nothing"))),
    ("burst_in_order", both(Answer::Flag(true))),
    (
        "burst_told_a",
        each(
            Answer::Text(
                "Message from nobody, the name \"Second Life\", to avatar A, id nobody, no region, at zero, not kept, no time, estate 0, not from a group, a bucket of one zero byte, text \"You have sent out excessive instant messages, and your ability to send IMs has been temporarily suspended. Try again later.\"",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("burst_told_b_count", each(Answer::Any, Answer::Count(20))),
    (
        "cap_answer",
        each(
            Answer::Text("200, an array holding an array of messages"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cap_asked_again",
        each(
            Answer::Text("200, an empty array, 0 messages"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cap_auto_response",
        each(
            Answer::Text(
                "{asset_id=the session of the two avatars, binary_bucket=1 bytes, dialog=20, estate_id=1, from_agent_id=avatar A, from_agent_name=A's name, from_group=false, message=as sent, message_time=a date and time, offline=1, parent_estate_id=1, position=[3], region_id=another id, timestamp=the time it was sent, to_agent_id=avatar B, transaction-id=nobody}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cap_first",
        each(
            Answer::Text(
                "{asset_id=the session of the two avatars, binary_bucket=1 bytes, dialog=0, estate_id=1, from_agent_id=avatar A, from_agent_name=A's name, from_group=false, message=as sent, message_time=a date and time, offline=1, parent_estate_id=1, position=[3], region_id=another id, timestamp=the time it was sent, to_agent_id=avatar B, transaction-id=nobody}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cap_long",
        each(
            Answer::Text(
                "{asset_id=the session of the two avatars, binary_bucket=1 bytes, dialog=0, estate_id=1, from_agent_id=avatar A, from_agent_name=A's name, from_group=false, message=the first 1023 bytes, message_time=a date and time, offline=1, parent_estate_id=1, position=[3], region_id=another id, timestamp=the time it was sent, to_agent_id=avatar B, transaction-id=nobody}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cap_order",
        each(Answer::Text("as sent"), Answer::Text("no capability")),
    ),
    (
        "cap_script",
        each(
            Answer::Text(
                "{asset_id=the prim, binary_bucket=15 bytes, dialog=19, estate_id=1, from_agent_id=avatar A, from_agent_name=the prim's name, from_group=false, message=as sent, message_time=a date and time, offline=1, parent_estate_id=1, position=[3], region_id=another id, timestamp=the time it was sent, to_agent_id=avatar B, transaction-id=nobody}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cap_second",
        each(
            Answer::Text(
                "{asset_id=the session of the two avatars, binary_bucket=1 bytes, dialog=0, estate_id=1, from_agent_id=avatar A, from_agent_name=A's name, from_group=false, message=as sent, message_time=a date and time, offline=1, parent_estate_id=1, position=[3], region_id=another id, timestamp=the time it was sent, to_agent_id=avatar B, transaction-id=nobody}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cap_then_udp",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    (
        "cap_typing",
        each(Answer::Text("nothing"), Answer::Text("no capability")),
    ),
    ("cap_unasked", both(Answer::Text("nothing"))),
    ("client_also_kept", both(Answer::Text("nothing"))),
    (
        "client_auto_response",
        each(
            Answer::Text(
                "DoNotDisturbAutoResponse from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, kept, the time it was sent, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "client_first",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, kept, the time it was sent, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    (
        "client_long",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, kept, the time it was sent, estate 1, not from a group, a bucket of one zero byte, text the first 1023 bytes",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, kept, the time it was sent, estate 1, not from a group, no bucket, text the first 1023 bytes",
            ),
        ),
    ),
    ("client_order", both(Answer::Text("as sent"))),
    (
        "client_script",
        each(
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at the prim, kept, the time it was sent, estate 1, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at the prim, kept, the time it was sent, estate 101, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
        ),
    ),
    (
        "client_second",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, kept, the time it was sent, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    ("client_typing", both(Answer::Text("nothing"))),
    ("client_unasked", both(Answer::Text("nothing"))),
    ("dialog_from_task_told_a", both(Answer::Text("nothing"))),
    (
        "dialog_from_task_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "FromTask from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    ("dialog_message_box_told_a", both(Answer::Text("nothing"))),
    ("dialog_message_box_told_b", both(Answer::Text("nothing"))),
    ("dialog_unknown_told_a", both(Answer::Text("nothing"))),
    ("dialog_unknown_told_b", both(Answer::Text("nothing"))),
    ("im_1023_bytes_kept_bytes", both(Answer::Count(1023))),
    ("im_1023_bytes_told_a", both(Answer::Text("nothing"))),
    (
        "im_1023_bytes_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    ("im_1024_bytes_kept_bytes", both(Answer::Count(1023))),
    ("im_1024_bytes_told_a", both(Answer::Text("nothing"))),
    (
        "im_1024_bytes_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text the first 1023 bytes",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text the first 1023 bytes",
            ),
        ),
    ),
    ("im_1100_bytes_kept_bytes", both(Answer::Count(1023))),
    ("im_1100_bytes_told_a", both(Answer::Text("nothing"))),
    (
        "im_1100_bytes_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text the first 1023 bytes",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text the first 1023 bytes",
            ),
        ),
    ),
    ("im_2000_bytes_kept_bytes", both(Answer::Count(1023))),
    ("im_2000_bytes_told_a", both(Answer::Text("nothing"))),
    (
        "im_2000_bytes_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text the first 1023 bytes",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text the first 1023 bytes",
            ),
        ),
    ),
    ("im_3000_bytes_kept_bytes", both(Answer::Count(1023))),
    ("im_3000_bytes_told_a", both(Answer::Text("nothing"))),
    (
        "im_3000_bytes_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text the first 1023 bytes",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text the first 1023 bytes",
            ),
        ),
    ),
    ("im_claiming_told_a", both(Answer::Text("nothing"))),
    (
        "im_claiming_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, the time as sent, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, the name as sent, to avatar B, id the id as sent, a region, at the place as sent, not kept, the time it was sent, estate 1, not from a group, the bucket as sent, text as sent",
            ),
        ),
    ),
    ("im_flagged_as_kept_told_a", both(Answer::Text("nothing"))),
    (
        "im_flagged_as_kept_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    ("im_line_break_told_a", both(Answer::Text("nothing"))),
    (
        "im_line_break_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    ("im_of_the_muter_told_a", both(Answer::Text("the message"))),
    ("im_of_the_muter_told_b", both(Answer::Text("nothing"))),
    (
        "im_to_an_unknown_id_told_a",
        each(
            Answer::Text(
                "Message from the unknown id, the name \"Second Life\", to avatar A, id the session of avatar A and the unknown id, no region, at zero, not kept, no time, estate 0, not from a group, a bucket of one zero byte, text \"User not online - message will be stored and delivered later.\"",
            ),
            Answer::Text(
                "Message from the unknown id, the name \"System\", to avatar A, id the session of avatar A and the unknown id, no region, at zero, not kept, the time it arrived, estate 0, not from a group, no bucket, text \"User is not logged in. Message saved.\"",
            ),
        ),
    ),
    ("im_to_no_id_told_a", both(Answer::Text("nothing"))),
    (
        "im_to_oneself_told_a",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar A, id avatar A, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar A, id avatar A, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    ("im_told_a", both(Answer::Text("nothing"))),
    (
        "im_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    ("im_without_text_told_a", both(Answer::Text("nothing"))),
    (
        "im_without_text_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    (
        "mute_list_after_the_mute",
        both(Answer::Text("names avatar A")),
    ),
    ("muted_im_told_a", both(Answer::Text("nothing"))),
    (
        "muted_im_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    (
        "muted_script_im_told_b",
        each(
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at zero, not kept, no time, estate 1, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at the prim, not kept, the time it was sent, estate 101, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
        ),
    ),
    (
        "muted_typing_start_told_b",
        both(Answer::Text(
            "start typing from avatar A, A's name, id the session of the two avatars",
        )),
    ),
    (
        "script_im_long_told_b",
        each(
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at zero, not kept, no time, estate 1, not from a group, a bucket naming a region and a place in it, text the first 1023 bytes",
            ),
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at the prim, not kept, the time it was sent, estate 101, not from a group, a bucket naming a region and a place in it, text the first 1023 bytes",
            ),
        ),
    ),
    (
        "script_im_to_an_unknown_id_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "script_im_to_its_owner_told_a",
        each(
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar A, id the prim, a region, at zero, not kept, no time, estate 1, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar A, id the prim, a region, at the prim, not kept, the time it was sent, estate 101, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
        ),
    ),
    ("script_im_told_a", both(Answer::Text("nothing"))),
    (
        "script_im_told_b",
        each(
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at zero, not kept, no time, estate 1, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at the prim, not kept, the time it was sent, estate 101, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
        ),
    ),
    (
        "script_im_without_text_told_b",
        each(
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at zero, not kept, no time, estate 1, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at the prim, not kept, the time it was sent, estate 101, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
        ),
    ),
    ("typing_start_told_a", both(Answer::Text("nothing"))),
    (
        "typing_start_told_b",
        both(Answer::Text(
            "start typing from avatar A, A's name, id the session of the two avatars",
        )),
    ),
    ("typing_stop_told_a", both(Answer::Text("nothing"))),
    (
        "typing_stop_told_b",
        both(Answer::Text(
            "stop typing from avatar A, A's name, id the session of the two avatars",
        )),
    ),
    ("udp_also_kept", both(Answer::Text("nothing"))),
    ("udp_asked_again", both(Answer::Text("nothing"))),
    (
        "udp_auto_response",
        each(
            Answer::Text(
                "DoNotDisturbAutoResponse from avatar A, A's name, to avatar B, id the session of the two avatars, no region, at zero, kept, the time it was sent, estate 0, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "udp_first",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, kept, the time it was sent, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    (
        "udp_long",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, kept, the time it was sent, estate 1, not from a group, a bucket of one zero byte, text the first 1023 bytes",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, kept, the time it was sent, estate 1, not from a group, no bucket, text the first 1023 bytes",
            ),
        ),
    ),
    ("udp_order", both(Answer::Text("as sent"))),
    (
        "udp_script",
        each(
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at zero, kept, the time it was sent, estate 1, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
            Answer::Text(
                "FromTask from avatar A, the prim's name, to avatar B, id the prim, a region, at the prim, kept, the time it was sent, estate 101, not from a group, a bucket naming a region and a place in it, text as sent",
            ),
        ),
    ),
    (
        "udp_second",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, kept, the time it was sent, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
    (
        "udp_then_the_capability",
        each(
            Answer::Text("200, an empty array, 0 messages"),
            Answer::Text("no capability"),
        ),
    ),
    ("udp_typing", both(Answer::Text("nothing"))),
    ("udp_unasked", both(Answer::Text("nothing"))),
    ("unmuted_im_told_a", both(Answer::Text("nothing"))),
    (
        "unmuted_im_told_b",
        each(
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at avatar A, not kept, no time, estate 1, not from a group, a bucket of one zero byte, text as sent",
            ),
            Answer::Text(
                "Message from avatar A, A's name, to avatar B, id the session of the two avatars, a region, at zero, not kept, the time it was sent, estate 1, not from a group, no bucket, text as sent",
            ),
        ),
    ),
];

/// The metrics a fake flavour is not held to, by prefix or whole name, each
/// with why.
const FAKE_EXEMPT: &[(&str, &str)] = &[
    ("script_im_", "the fake grid runs no scripts"),
    ("muted_script_im_", "the fake grid runs no scripts"),
    ("away_script_", "the fake grid runs no scripts"),
    ("udp_script", "the fake grid runs no scripts"),
    ("cap_script", "the fake grid runs no scripts"),
    ("client_script", "the fake grid runs no scripts"),
    (
        "mute_list_after_the_mute",
        "the fake grid keeps no mute list (server-fake-grid-mute-list)",
    ),
];

/// Why `grid` is not held to the metric `key`, if it is a fake flavour and
/// [`FAKE_EXEMPT`] names the metric.
fn fake_exemption(grid: Grid, key: &str) -> Option<&'static str> {
    if !grid.is_fake() {
        return None;
    }
    FAKE_EXEMPT
        .iter()
        .find(|(prefix, _why)| key.starts_with(prefix))
        .map(|(_prefix, why)| *why)
}

/// How long the census waits for each thing it waits for: the live grids'
/// windows, or a fake grid's, which answers at once.
#[derive(Debug, Clone, Copy)]
struct Windows {
    /// How long a grid is given to drop an avatar that logged out.
    away_settle: Duration,
    /// How long an avatar that came back is left alone.
    unasked: Duration,
    /// How long an avatar waits for what it asked for.
    fetch: Duration,
    /// How long after the last kept message a fetch is still listened to.
    fetch_quiet: Duration,
    /// How long a grid is given to act on a mute or on its removal.
    mute_settle: Duration,
    /// How long a mute list may take to arrive.
    mute_list: Duration,
    /// How long the capability map may take to arrive after a login.
    cap_wait: Duration,
}

impl Windows {
    /// The windows of a run on `grid`.
    const fn of(grid: Grid) -> Self {
        if grid.is_fake() {
            Self {
                away_settle: FAKE_AWAY_SETTLE,
                unasked: FAKE_UNASKED_WINDOW,
                fetch: FAKE_FETCH_WINDOW,
                fetch_quiet: FAKE_FETCH_QUIET,
                mute_settle: FAKE_MUTE_SETTLE,
                mute_list: FAKE_MUTE_SETTLE,
                cap_wait: FAKE_CAP_WAIT,
            }
        } else {
            Self {
                away_settle: AWAY_SETTLE,
                unasked: UNASKED_WINDOW,
                fetch: FETCH_WINDOW,
                fetch_quiet: FETCH_QUIET,
                mute_settle: MUTE_SETTLE,
                mute_list: MUTE_LIST_WINDOW,
                cap_wait: CAP_WAIT,
            }
        }
    }
}

/// The dialogs no viewer sends to an avatar, each with the word its metrics
/// carry: a message box, a script's message and a number that is no dialog.
const DIALOGS: [(u8, &str); 3] = [(1, "message_box"), (19, "from_task"), (60, "unknown")];

/// The lengths of the long texts, in bytes, each with the word its metrics
/// carry.
const LENGTHS: [(usize, &str); 5] = [
    (1023, "1023"),
    (1024, "1024"),
    (1100, "1100"),
    (2000, "2000"),
    (3000, "3000"),
];

/// The probe: a script that sends the instant message its owner orders on the
/// probe's channel — `target|kind|text` — and says when it has.
fn probe_source() -> String {
    format!(
        r#"string T = "{PROBE_TAG}";
default
{{
    state_entry()
    {{
        llListen({PROBE_CHANNEL}, "", llGetOwner(), "");
        llOwnerSay(T + "R|" + llGetObjectName());
    }}
    listen(integer channel, string name, key id, string what)
    {{
        list parts = llParseStringKeepNulls(what, ["|"], []);
        key target = (key)llList2String(parts, 0);
        string kind = llList2String(parts, 1);
        string text = llList2String(parts, 2);
        if (kind == "long")
        {{
            while (llStringLength(text) < {LONG_SCRIPT_BYTES}) text += "0123456789";
        }}
        llInstantMessage(target, text);
        llOwnerSay(T + "D|" + kind);
    }}
}}
"#
    )
}

/// What a viewer made up about a message it sent.
#[derive(Debug, Clone)]
struct Claim {
    /// The name it says it is from.
    name: &'static str,
    /// The id.
    id: Uuid,
    /// The region.
    region: Uuid,
    /// The place.
    position: Vector,
    /// The time.
    timestamp: u32,
    /// The estate.
    estate: u32,
    /// The bucket.
    bucket: &'static [u8],
}

/// What a leg sent, for reading what came of it.
#[derive(Debug, Clone)]
struct Sent {
    /// What the text begins with, and nothing else of the census does; empty
    /// for a message without text.
    tag: String,
    /// The text.
    text: String,
    /// When it was sent.
    at: SystemTime,
    /// What the viewer made up about it, where it did.
    claim: Option<Claim>,
}

impl Sent {
    /// Whether `text` is this message's, cut or not.
    fn is(&self, text: &str) -> bool {
        if self.tag.is_empty() {
            text.is_empty()
        } else {
            text.starts_with(self.tag.as_str())
        }
    }
}

/// A start or a stop of typing as it arrived.
#[derive(Debug, Clone)]
struct Typed {
    /// Whose it is.
    from: Uuid,
    /// The name it came under.
    name: String,
    /// The id it carries.
    id: Uuid,
    /// Whether it is a start.
    start: bool,
}

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Got {
    /// Every instant message, in arrival order.
    ims: Vec<InstantMessage>,
    /// Every start and stop of typing, in arrival order.
    typed: Vec<Typed>,
    /// The text of every alert.
    alerts: Vec<String>,
    /// Every line of chat, in arrival order.
    lines: Vec<ChatMessage>,
    /// How saving a script ended, once it has.
    uploaded: Option<Result<bool, String>>,
}

impl Got {
    /// What the probe said after `prefix`, if it said such a line.
    fn said_after(&self, prefix: &str) -> Option<&str> {
        let wanted = format!("{PROBE_TAG}{prefix}");
        self.lines
            .iter()
            .find_map(|chat| chat.message.strip_prefix(wanted.as_str()))
    }

    /// Whether anything that was kept for the avatar is among the messages.
    fn any_kept(&self) -> bool {
        self.ims.iter().any(|im| im.offline)
    }
}

/// Listens on `session` until `done` says the leg has what it came for and
/// `quiet` more has passed, or for `limit` at most.
async fn watch_for(
    session: &mut Session,
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
                    Event::InstantMessageReceived(im) => got.ims.push((**im).clone()),
                    Event::ImTyping {
                        from_agent_id,
                        from_agent_name,
                        session_id,
                        typing,
                    } => got.typed.push(Typed {
                        from: from_agent_id.uuid(),
                        name: from_agent_name.clone(),
                        id: *session_id,
                        start: *typing,
                    }),
                    Event::ChatReceived(chat) => got.lines.push((**chat).clone()),
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
async fn watch(session: &mut Session, window: Duration) -> Result<Got, TestFailure> {
    watch_for(session, window, window, |_got| false).await
}

/// The two ids exclusive-ored: the id a viewer gives the session of two
/// avatars.
pub(crate) fn session_of(left: Uuid, right: Uuid) -> Uuid {
    let mut bytes = [0_u8; 16];
    for (slot, (a, b)) in bytes
        .iter_mut()
        .zip(left.as_bytes().iter().zip(right.as_bytes()))
    {
        *slot = a ^ b;
    }
    Uuid::from_bytes(bytes)
}

/// The seconds since the epoch at `instant`.
fn epoch_secs(instant: SystemTime) -> u64 {
    instant
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The ids, names and places a metric names by what they are rather than by
/// their value.
#[derive(Debug, Clone)]
struct Scene {
    /// Avatar A: writes, owns the cube.
    a: Uuid,
    /// Avatar B: is written to, mutes, goes away.
    b: Uuid,
    /// The name A logged in under.
    a_name: String,
    /// The name B logged in under.
    b_name: String,
    /// Where A stands.
    a_at: Vector,
    /// The cube, once there is one: its id, where it stands and its name.
    prim: Option<(Uuid, Vector, String)>,
    /// The ids the census made up to write to: nobody's.
    unknown: Vec<Uuid>,
}

impl Scene {
    /// What `id` is the id of.
    fn id_text(&self, id: Uuid, claim: Option<&Claim>) -> &'static str {
        let prim = self.prim.as_ref().map(|(prim, ..)| *prim);
        if id.is_nil() {
            "nobody"
        } else if id == self.a {
            "avatar A"
        } else if id == self.b {
            "avatar B"
        } else if Some(id) == prim {
            "the prim"
        } else if id == session_of(self.a, self.b) {
            "the session of the two avatars"
        } else if self.unknown.contains(&id) {
            "the unknown id"
        } else if self
            .unknown
            .iter()
            .any(|unknown| id == session_of(self.a, *unknown))
        {
            "the session of avatar A and the unknown id"
        } else if claim.is_some_and(|claim| claim.id == id) {
            "the id as sent"
        } else if prim.is_some_and(|prim| id == session_of(prim, self.b)) {
            "the session of the prim and avatar B"
        } else if prim.is_some_and(|prim| id == session_of(prim, self.a)) {
            "the session of the prim and avatar A"
        } else {
            "another id"
        }
    }

    /// Whose name `name` is.
    fn name_text(&self, name: &str, claim: Option<&Claim>) -> String {
        if name.is_empty() {
            "no name".to_owned()
        } else if name.eq_ignore_ascii_case(&self.a_name) {
            "A's name".to_owned()
        } else if name.eq_ignore_ascii_case(&self.b_name) {
            "B's name".to_owned()
        } else if self
            .prim
            .as_ref()
            .is_some_and(|(_prim, _at, prim_name)| prim_name == name)
        {
            "the prim's name".to_owned()
        } else if claim.is_some_and(|claim| claim.name == name) {
            "the name as sent".to_owned()
        } else {
            format!("the name {:?}", self.scrubbed(name))
        }
    }

    /// Where a message from `from` is stated to have been sent.
    fn place_text(&self, im: &InstantMessage, claim: Option<&Claim>) -> &'static str {
        let at = Vector {
            x: im.position.x(),
            y: im.position.y(),
            z: im.position.z(),
        };
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let near = |place: Option<&Vector>| place.is_some_and(|place| distance(&at, place) < AT_M);
        let at_the_prim = near(self.prim.as_ref().map(|(_prim, place, _name)| place));
        let at_a = near(Some(&self.a_at));
        // The cube stands two metres from avatar A: a place near both is the
        // place of whichever the message is from.
        let scripted = im.dialog == ImDialog::FromTask;
        if distance(&at, &zero) < 0.01 {
            "at zero"
        } else if claim.is_some_and(|claim| distance(&at, &claim.position) < 0.01) {
            "at the place as sent"
        } else if at_the_prim && (scripted || !at_a) {
            "at the prim"
        } else if at_a {
            "at avatar A"
        } else {
            "elsewhere"
        }
    }

    /// `text` with nothing in it that is one run's alone: the avatars' names
    /// and every id.
    fn scrubbed(&self, text: &str) -> String {
        let mut out = without_ids(text);
        for (name, word) in [(&self.a_name, "<A>"), (&self.b_name, "<B>")] {
            if !name.is_empty() {
                out = out.replace(name.as_str(), word);
            }
        }
        if out.len() > 240 {
            let cut = (0..=240)
                .rev()
                .find(|at| out.is_char_boundary(*at))
                .unwrap_or(0);
            out.truncate(cut);
            out.push('…');
        }
        out
    }

    /// One message as a metric states it. `sent` is what the leg sent, where
    /// the message is the leg's own; a message that is the grid's states its
    /// text as it is. `fetched` is when the avatar asked for what was kept.
    fn im_text(
        &self,
        im: &InstantMessage,
        sent: Option<&Sent>,
        fetched: Option<SystemTime>,
    ) -> String {
        let claim = sent.and_then(|sent| sent.claim.as_ref());
        let region = match im.region_id {
            None => "no region",
            Some(region) if claim.is_some_and(|claim| claim.region == region) => {
                "the region as sent"
            }
            Some(_) => "a region",
        };
        let text = sent.map_or_else(
            || format!("{:?}", self.scrubbed(&im.message)),
            |sent| text_text(&im.message, &sent.text),
        );
        format!(
            "{:?} from {}, {}, to {}, id {}, {region}, {}, {}, {}, estate {}, {}, {}, text {text}",
            im.dialog,
            self.id_text(im.from_agent_id.uuid(), claim),
            self.name_text(&im.from_agent_name, claim),
            self.id_text(im.to_agent_id.uuid(), claim),
            self.id_text(im.id, claim),
            self.place_text(im, claim),
            if im.offline { "kept" } else { "not kept" },
            time_text(
                im.timestamp,
                sent.map(|sent| sent.at),
                fetched,
                claim.map(|claim| claim.timestamp)
            ),
            match claim {
                Some(claim) if claim.estate == im.parent_estate_id => "as sent".to_owned(),
                _ => im.parent_estate_id.to_string(),
            },
            if im.from_group {
                "from a group"
            } else {
                "not from a group"
            },
            bucket_text(&im.binary_bucket, claim),
        )
    }

    /// What `got` holds of the message `sent`: `nothing`, or the first such
    /// message — and how many, where that is not one.
    fn told(&self, got: &Got, sent: &Sent, fetched: Option<SystemTime>) -> String {
        let ims: Vec<&InstantMessage> = got.ims.iter().filter(|im| sent.is(&im.message)).collect();
        let Some(first) = ims.first() else {
            return "nothing".to_owned();
        };
        let text = self.im_text(first, Some(sent), fetched);
        if ims.len() == 1 {
            text
        } else {
            format!("{} times, {text}", ims.len())
        }
    }

    /// What `got` holds of avatar A starting (or stopping) to type.
    fn typed(&self, got: &Got, start: bool) -> String {
        let notices: Vec<&Typed> = got
            .typed
            .iter()
            .filter(|typed| typed.from == self.a && typed.start == start)
            .collect();
        let Some(first) = notices.first() else {
            return "nothing".to_owned();
        };
        let text = format!(
            "{} typing from {}, {}, id {}",
            if first.start { "start" } else { "stop" },
            self.id_text(first.from, None),
            self.name_text(&first.name, None),
            self.id_text(first.id, None),
        );
        if notices.len() == 1 {
            text
        } else {
            format!("{} times, {text}", notices.len())
        }
    }

    /// Everything `got` holds that is not one of `own`: each message, typing
    /// notice and alert as the grid worded it, or `nothing`.
    fn everything(&self, got: &Got, own: &[&Sent]) -> String {
        let mut all: Vec<String> = got
            .ims
            .iter()
            .filter(|im| !own.iter().any(|sent| sent.is(&im.message)))
            .map(|im| self.im_text(im, None, None))
            .collect();
        all.extend(got.typed.iter().map(|typed| {
            format!(
                "{} typing from {}",
                if typed.start { "start" } else { "stop" },
                self.id_text(typed.from, None)
            )
        }));
        all.extend(
            got.alerts
                .iter()
                .map(|alert| format!("alert {:?}", self.scrubbed(alert))),
        );
        if all.is_empty() {
            "nothing".to_owned()
        } else {
            all.join(" | ")
        }
    }

    /// One record of the `ReadOfflineMsgs` capability as a metric states it:
    /// every key, and what its value is.
    fn record_text(&self, record: &Llsd, sent: Option<&Sent>, fetched: SystemTime) -> String {
        let Llsd::Map(map) = record else {
            return shape_of(record);
        };
        let mut entries: Vec<(&String, &Llsd)> = map.iter().collect();
        entries.sort_unstable_by_key(|(key, _value)| *key);
        let entries: Vec<String> = entries
            .into_iter()
            .map(|(key, value)| {
                let text = match value {
                    Llsd::Uuid(id) => self.id_text(*id, None).to_owned(),
                    Llsd::String(text) => match (key.as_str(), Uuid::parse_str(text)) {
                        (_, Ok(id)) => format!("the text of {}", self.id_text(id, None)),
                        ("message", _) => sent.map_or_else(
                            || format!("{:?}", self.scrubbed(text)),
                            |sent| text_text(text, &sent.text),
                        ),
                        ("from_agent_name", _) => self.name_text(text, None),
                        ("message_time", _) => "a date and time".to_owned(),
                        _ => format!("{:?}", self.scrubbed(text)),
                    },
                    Llsd::Integer(number) if key == "timestamp" => time_text(
                        u32::try_from(*number).ok().filter(|time| *time != 0),
                        sent.map(|sent| sent.at),
                        Some(fetched),
                        None,
                    )
                    .to_owned(),
                    Llsd::Integer(number) => number.to_string(),
                    Llsd::Real(number) if *number == 0.0 => "0".to_owned(),
                    Llsd::Real(_) => "a number".to_owned(),
                    Llsd::Boolean(flag) => flag.to_string(),
                    Llsd::Binary(bytes) => format!("{} bytes", bytes.len()),
                    Llsd::Array(items) => format!("[{}]", items.len()),
                    other => shape_of(other),
                };
                format!("{key}={text}")
            })
            .collect();
        format!("{{{}}}", entries.join(", "))
    }
}

/// When a message says it was sent: `sent` is when the leg sent it,
/// `fetched` when the avatar asked for what was kept, `claimed` the time a
/// viewer made up.
fn time_text(
    stated: Option<u32>,
    sent: Option<SystemTime>,
    fetched: Option<SystemTime>,
    claimed: Option<u32>,
) -> &'static str {
    let Some(stated) = stated else {
        return "no time";
    };
    let near =
        |instant: SystemTime| epoch_secs(instant).abs_diff(u64::from(stated)) <= TIME_SLACK_SECS;
    if claimed == Some(stated) {
        "the time as sent"
    } else if sent.is_some_and(near) {
        "the time it was sent"
    } else if fetched.is_some_and(near) {
        "the time it was asked for"
    } else if sent.is_none() && near(SystemTime::now()) {
        "the time it arrived"
    } else {
        "another time"
    }
}

/// What a bucket holds.
fn bucket_text(bucket: &[u8], claim: Option<&Claim>) -> String {
    let text = String::from_utf8_lossy(bucket);
    let text = text.trim_end_matches('\0');
    if bucket.is_empty() {
        "no bucket".to_owned()
    } else if claim.is_some_and(|claim| claim.bucket == bucket) {
        "the bucket as sent".to_owned()
    } else if bucket == [0] {
        "a bucket of one zero byte".to_owned()
    } else if names_a_place(text) {
        "a bucket naming a region and a place in it".to_owned()
    } else {
        format!("a bucket of {} bytes", bucket.len())
    }
}

/// Whether `text` reads `region/x/y/z`.
fn names_a_place(text: &str) -> bool {
    let mut parts = text.rsplitn(4, '/');
    let numbers = parts
        .by_ref()
        .take(3)
        .filter(|part| part.parse::<f32>().is_ok())
        .count();
    numbers == 3 && parts.next().is_some_and(|region| !region.is_empty())
}

/// The messages a `ReadOfflineMsgs` answer holds, and how it holds them.
fn kept_records(body: Option<&Llsd>) -> (&'static str, Vec<Llsd>) {
    match body {
        None => ("no LLSD", Vec::new()),
        Some(Llsd::Array(items)) => match items.first() {
            None => ("an empty array", Vec::new()),
            Some(Llsd::Array(inner)) => ("an array holding an array of messages", inner.clone()),
            Some(Llsd::Map(_)) => ("an array of messages", items.clone()),
            Some(_) => ("an array of something else", Vec::new()),
        },
        Some(map @ Llsd::Map(_)) => match map.get("messages") {
            Some(Llsd::Array(items)) => ("a map with an array of messages", items.clone()),
            _ => ("a map without messages", Vec::new()),
        },
        Some(_) => ("something else", Vec::new()),
    }
}

/// The text of a `ReadOfflineMsgs` record.
fn record_message(record: &Llsd) -> &str {
    record
        .get("message")
        .and_then(Llsd::as_str)
        .unwrap_or_default()
}

/// The dialog of a `ReadOfflineMsgs` record.
fn record_dialog(record: &Llsd) -> Option<i32> {
    record.get("dialog").and_then(Llsd::as_i32)
}

/// Which of the two avatars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The one that writes and owns the cube.
    A,
    /// The one that is written to.
    B,
}

/// What a leg brought each avatar.
struct Brought {
    /// What A was sent.
    a: Got,
    /// What B was sent.
    b: Got,
}

/// How an avatar that came back asks for what was kept for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fetch {
    /// `RetrieveInstantMessages`.
    Udp,
    /// The `ReadOfflineMsgs` capability, read by the case itself.
    Capability,
    /// [`Command::RequestOfflineMessages`]: the capability where the region
    /// grants it, `RetrieveInstantMessages` where it does not.
    Client,
}

impl Fetch {
    /// The word the round's metrics begin with.
    const fn word(self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Capability => "cap",
            Self::Client => "client",
        }
    }
}

/// What A sends an avatar that is away: the word each one's metrics carry.
const AWAY: [&str; 6] = [
    "first",
    "second",
    "typing",
    "auto_response",
    "long",
    "script",
];

/// Everything the legs share.
struct Census<'a> {
    /// Avatar A's session.
    a: &'a mut Session,
    /// Avatar B's session.
    b: &'a mut Session,
    /// The ids, names and places the metrics name.
    scene: Scene,
    /// How long the acting avatar listens after a gesture.
    quiet: Duration,
    /// How long the queue of the other one is then read for.
    drain: Duration,
    /// How many texts the census has made.
    said: u32,
    /// Whether B is logged in.
    b_is_here: bool,
    /// How long each wait is.
    windows: Windows,
}

impl Census<'_> {
    /// A text of the census for `word`, and its tag.
    fn sent(&mut self, word: &str) -> Sent {
        self.said = self.said.saturating_add(1);
        let tag = format!("{TEXT_TAG}{}-", self.said);
        Sent {
            text: format!("{tag}{word}"),
            tag,
            at: SystemTime::now(),
            claim: None,
        }
    }

    /// A text of the census that is `bytes` long.
    fn long(&mut self, bytes: usize) -> Sent {
        let mut sent = self.sent("");
        while sent.text.len().saturating_add(10) <= bytes {
            sent.text.push_str("0123456789");
        }
        while sent.text.len() < bytes {
            sent.text.push('x');
        }
        sent
    }

    /// Has `who` send `commands`, a moment apart, and returns what the two
    /// avatars were then sent — once `done` says the sender has what the leg
    /// came for. An avatar that is away is sent nothing.
    async fn act_until(
        &mut self,
        who: Who,
        commands: Vec<Command>,
        done: impl FnMut(&Got) -> bool,
    ) -> Result<Brought, TestFailure> {
        let several = commands.len() > 1;
        let (actor, listener) = match who {
            Who::A => (&mut *self.a, &mut *self.b),
            Who::B => (&mut *self.b, &mut *self.a),
        };
        for command in commands {
            actor.send(command).await?;
            if several {
                tokio::time::sleep(BETWEEN).await;
            }
        }
        let acted = watch_for(actor, STEP_TIMEOUT, self.quiet, done).await?;
        let listened = if who == Who::B || self.b_is_here {
            watch(listener, self.drain).await?
        } else {
            Got::default()
        };
        Ok(match who {
            Who::A => Brought {
                a: acted,
                b: listened,
            },
            Who::B => Brought {
                a: listened,
                b: acted,
            },
        })
    }

    /// Has `who` send `commands`, a moment apart, and returns what the two
    /// avatars were then sent.
    async fn act(&mut self, who: Who, commands: Vec<Command>) -> Result<Brought, TestFailure> {
        self.act_until(who, commands, |_got| true).await
    }

    /// An `ImprovedInstantMessage` of A's as a viewer may write it: to `to`,
    /// of the dialog `dialog`, flagged as kept or not, and with whatever
    /// `sent` claims.
    fn raw(&self, to: Uuid, dialog: u8, kept: bool, sent: &Sent) -> Result<Command, TestFailure> {
        let session_id = self
            .a
            .session_id()
            .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;
        let claim = sent.claim.as_ref();
        let mut name = claim
            .map_or(self.scene.a_name.as_str(), |claim| claim.name)
            .as_bytes()
            .to_vec();
        name.push(0);
        let mut text = sent.text.as_bytes().to_vec();
        text.push(0);
        let zero = Vector {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let message = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: self.scene.a,
                session_id,
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id: to,
                parent_estate_id: claim.map_or(0, |claim| claim.estate),
                region_id: claim.map_or_else(Uuid::nil, |claim| claim.region),
                position: claim.map_or(zero, |claim| claim.position.clone()),
                offline: u8::from(kept),
                dialog,
                id: claim.map_or_else(|| session_of(self.scene.a, to), |claim| claim.id),
                timestamp: claim.map_or(0, |claim| claim.timestamp),
                from_agent_name: name,
                message: text,
                binary_bucket: claim.map_or_else(Vec::new, |claim| claim.bucket.to_vec()),
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        Ok(Command::Send {
            message: Box::new(message),
            reliability: Reliability::Reliable,
        })
    }

    /// Has A send `command`, which carries `sent`, and records under `key`
    /// what B was sent of it and everything A was sent.
    async fn leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        command: Command,
        sent: &Sent,
    ) -> Result<Brought, TestFailure> {
        let brought = self.act(Who::A, vec![command]).await?;
        metrics.set(
            &format!("{key}_told_b"),
            self.scene.told(&brought.b, sent, None),
        );
        metrics.set(
            &format!("{key}_told_a"),
            self.scene.everything(&brought.a, &[]),
        );
        let also = self.scene.everything(&brought.b, &[sent]);
        if also != "nothing" {
            metrics.set(&format!("{key}_also_told_b"), also);
        }
        Ok(brought)
    }

    /// An ordinary instant message of A's to B that carries `sent`.
    fn to_b(&self, sent: &Sent) -> Command {
        Command::InstantMessage {
            to_agent_id: AgentKey::from(self.scene.b),
            message: sent.text.clone(),
        }
    }

    /// Has A tell the probe to send `target` an instant message of `kind`
    /// that carries `sent`, and returns what the avatars were then sent.
    async fn order(
        &mut self,
        target: Uuid,
        kind: &str,
        sent: &Sent,
    ) -> Result<Brought, TestFailure> {
        let done = format!("D|{kind}");
        let brought = self
            .act_until(
                Who::A,
                vec![Command::Chat {
                    message: format!("{target}|{kind}|{}", sent.text),
                    chat_type: ChatType::Normal,
                    channel: ChatChannel(PROBE_CHANNEL),
                }],
                |got| got.said_after(&done).is_some(),
            )
            .await?;
        if brought.a.said_after(&done).is_none() {
            return Err(TestFailure::Assertion(format!(
                "the probe never said it sent its {kind} message"
            )));
        }
        Ok(brought)
    }

    /// Puts the probe script into `prim` and has it run; returns the name
    /// the prim says it has.
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
        let got = watch_for(self.a, STEP_TIMEOUT, self.quiet, |got| {
            got.uploaded.is_some() && got.said_after("R|").is_some()
        })
        .await?;
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

    /// What B's account says of messages sent while it is away, turned off
    /// where it sends them on as mail: a message that went out as mail is not
    /// kept.
    async fn mail(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        self.b.send(Command::RequestUserInfo).await?;
        let info = match self
            .b
            .wait_for(STEP_TIMEOUT, |event| match event {
                Event::UserInfo(info) => Some(info.clone()),
                _ => None,
            })
            .await
        {
            Ok(info) => Some(info),
            Err(TestFailure::Timeout(_)) => None,
            Err(other) => return Err(other),
        };
        metrics.set(
            "b_messages_as_mail",
            match info.as_ref().map(|info| info.im_via_email) {
                None => "no answer",
                Some(true) => "on, turned off",
                Some(false) => "off",
            },
        );
        if let Some(info) = info.filter(|info| info.im_via_email) {
            self.b
                .send(Command::UpdateUserInfo {
                    im_via_email: false,
                    directory_visibility: info.directory_visibility,
                })
                .await?;
            watch(self.b, self.windows.mute_settle).await?;
        }
        Ok(())
    }

    /// Reads the `ReadOfflineMsgs` capability as B: the status, how the body
    /// holds its messages, and the messages. `None` where the region grants
    /// no such capability.
    async fn read_capability(&self) -> Result<Option<(u16, &'static str, Vec<Llsd>)>, TestFailure> {
        let Some(url) = self.b.cap(READ_OFFLINE_MSGS) else {
            return Ok(None);
        };
        let answer = cap_get(&http_client()?, &url).await?;
        let (held, records) = kept_records(answer.body.as_ref());
        Ok(Some((answer.status, held, records)))
    }

    /// Empties what is kept for B by both roads, and records how much there
    /// was.
    async fn empty_the_store(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        self.b.send(Command::RetrieveInstantMessages).await?;
        let windows = self.windows;
        let got = watch_for(self.b, windows.fetch, windows.fetch_quiet, Got::any_kept).await?;
        let by_capability = self
            .read_capability()
            .await?
            .map_or(0, |(_status, _held, records)| records.len());
        metrics.set(
            "kept_before_the_census_count",
            i64::try_from(
                got.ims
                    .iter()
                    .filter(|im| im.offline)
                    .count()
                    .saturating_add(by_capability),
            )
            .unwrap_or(i64::MAX),
        );
        Ok(())
    }

    /// The legs beside each other that need no script: a message and its
    /// text, typing, an automatic reply, whom, what a viewer may claim, and
    /// a burst.
    async fn viewer_legs(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let b = AgentKey::from(self.scene.b);

        let sent = self.sent("im");
        self.leg(metrics, "im", self.to_b(&sent), &sent).await?;

        let empty = Sent {
            tag: String::new(),
            text: String::new(),
            at: SystemTime::now(),
            claim: None,
        };
        self.leg(metrics, "im_without_text", self.to_b(&empty), &empty)
            .await?;

        let mut broken = self.sent("line");
        broken
            .text
            .push_str("\nbreak\tand a tab, spaces at the end   ");
        self.leg(metrics, "im_line_break", self.to_b(&broken), &broken)
            .await?;

        for (bytes, word) in LENGTHS {
            let sent = self.long(bytes);
            let key = format!("im_{word}_bytes");
            let brought = self.leg(metrics, &key, self.to_b(&sent), &sent).await?;
            if let Some(im) = brought.b.ims.iter().find(|im| sent.is(&im.message)) {
                let kept = i64::try_from(im.message.len()).unwrap_or(i64::MAX);
                let name = i64::try_from(im.from_agent_name.len()).unwrap_or(i64::MAX);
                metrics.set(&format!("{key}_kept_bytes"), kept);
                metrics.set(
                    &format!("{key}_kept_bytes_and_name_bytes"),
                    kept.saturating_add(name),
                );
            }
        }

        for (key, start) in [("typing_start", true), ("typing_stop", false)] {
            let brought = self
                .act(
                    Who::A,
                    vec![Command::ImTyping {
                        to_agent_id: b,
                        typing: start,
                    }],
                )
                .await?;
            metrics.set(
                &format!("{key}_told_b"),
                self.scene.typed(&brought.b, start),
            );
            metrics.set(
                &format!("{key}_told_a"),
                self.scene.everything(&brought.a, &[]),
            );
        }

        let sent = self.sent("auto");
        self.leg(
            metrics,
            "auto_response",
            Command::AutoResponse {
                to_agent_id: b,
                message: sent.text.clone(),
            },
            &sent,
        )
        .await?;

        // Whom: oneself, an id that is nobody's, no id.
        let sent = self.sent("self");
        let brought = self
            .act(
                Who::A,
                vec![Command::InstantMessage {
                    to_agent_id: AgentKey::from(self.scene.a),
                    message: sent.text.clone(),
                }],
            )
            .await?;
        metrics.set("im_to_oneself_told_a", {
            let own = self.scene.told(&brought.a, &sent, None);
            let other = self.scene.everything(&brought.a, &[&sent]);
            if other == "nothing" {
                own
            } else {
                format!("{own} | {other}")
            }
        });
        let unknown = Uuid::new_v4();
        self.scene.unknown.push(unknown);
        for (key, to) in [
            ("im_to_an_unknown_id", unknown),
            ("im_to_no_id", Uuid::nil()),
        ] {
            let sent = self.sent("whom");
            let brought = self
                .act(
                    Who::A,
                    vec![Command::InstantMessage {
                        to_agent_id: AgentKey::from(to),
                        message: sent.text.clone(),
                    }],
                )
                .await?;
            metrics.set(
                &format!("{key}_told_a"),
                self.scene.everything(&brought.a, &[]),
            );
        }

        // What a viewer may claim.
        let mut claimed = self.sent("claim");
        claimed.claim = Some(Claim {
            name: "Somebody Else",
            id: Uuid::new_v4(),
            region: Uuid::new_v4(),
            position: Vector {
                x: 11.0,
                y: 22.0,
                z: 33.0,
            },
            timestamp: 1_234_567,
            estate: 77,
            bucket: b"bucket",
        });
        let command = self.raw(self.scene.b, 0, false, &claimed)?;
        self.leg(metrics, "im_claiming", command, &claimed).await?;

        let flagged = self.sent("kept");
        let command = self.raw(self.scene.b, 0, true, &flagged)?;
        self.leg(metrics, "im_flagged_as_kept", command, &flagged)
            .await?;

        for (dialog, word) in DIALOGS {
            let sent = self.sent(word);
            let command = self.raw(self.scene.b, dialog, false, &sent)?;
            self.leg(metrics, &format!("dialog_{word}"), command, &sent)
                .await?;
        }

        // A burst.
        let burst: Vec<Sent> = std::iter::repeat_with(|| self.sent("burst"))
            .take(usize::from(BURST))
            .collect();
        for sent in &burst {
            self.a.send(self.to_b(sent)).await?;
        }
        let quiet = self.quiet;
        let wanted = usize::from(BURST);
        let got = watch_for(self.b, STEP_TIMEOUT, quiet, |got| {
            got.ims
                .iter()
                .filter(|im| im.message.ends_with("burst"))
                .count()
                >= wanted
        })
        .await?;
        let arrived: Vec<usize> = got
            .ims
            .iter()
            .filter_map(|im| burst.iter().position(|sent| sent.is(&im.message)))
            .collect();
        metrics.set(
            "burst_told_b_count",
            i64::try_from(arrived.len()).unwrap_or(i64::MAX),
        );
        metrics.set("burst_in_order", arrived.is_sorted());
        metrics.set(
            "burst_told_a",
            self.scene.everything(&watch(self.a, quiet).await?, &[]),
        );
        Ok(())
    }

    /// The legs a script is asked: its message to another avatar, to its
    /// owner and to nobody, without text and with too much of it.
    async fn script_legs(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let sent = self.sent("script");
        let brought = self.order(self.scene.b, "other", &sent).await?;
        metrics.set("script_im_told_b", self.scene.told(&brought.b, &sent, None));
        metrics.set("script_im_told_a", self.scene.everything(&brought.a, &[]));
        if let Some(im) = brought.b.ims.iter().find(|im| sent.is(&im.message)) {
            metrics.set(
                "script_im_bucket_as_text",
                self.scene
                    .scrubbed(String::from_utf8_lossy(&im.binary_bucket).trim_end_matches('\0')),
            );
        }

        let sent = self.sent("owner");
        let brought = self.order(self.scene.a, "owner", &sent).await?;
        metrics.set(
            "script_im_to_its_owner_told_a",
            self.scene.told(&brought.a, &sent, None),
        );

        let sent = self.sent("nobody");
        let unknown = Uuid::new_v4();
        self.scene.unknown.push(unknown);
        let brought = self.order(unknown, "nobody", &sent).await?;
        metrics.set(
            "script_im_to_an_unknown_id_told_a",
            self.scene.everything(&brought.a, &[]),
        );

        let empty = Sent {
            tag: String::new(),
            text: String::new(),
            at: SystemTime::now(),
            claim: None,
        };
        let brought = self.order(self.scene.b, "empty", &empty).await?;
        metrics.set(
            "script_im_without_text_told_b",
            self.scene.told(&brought.b, &empty, None),
        );

        let mut long = self.sent("");
        let ordered = long.clone();
        while long.text.len() < LONG_SCRIPT_BYTES {
            long.text.push_str("0123456789");
        }
        let brought = self.order(self.scene.b, "long", &ordered).await?;
        metrics.set(
            "script_im_long_told_b",
            self.scene.told(&brought.b, &long, None),
        );
        Ok(())
    }

    /// The legs of a mute: B mutes A, and A's message, its typing and its
    /// prim's message are asked again; so is B's message to the avatar it
    /// muted. The mute is taken back whatever became of the legs.
    async fn mute_legs(
        &mut self,
        metrics: &mut Metrics,
        scripted: bool,
    ) -> Result<(), TestFailure> {
        self.b
            .send(Command::Mute {
                id: self.scene.a,
                name: self.scene.a_name.clone(),
                mute_type: MuteType::Agent,
                flags: MuteFlags::default(),
            })
            .await?;
        let outcome = self.muted(metrics, scripted).await;
        let unmuted = self
            .b
            .send(Command::Unmute {
                id: self.scene.a,
                name: self.scene.a_name.clone(),
            })
            .await;
        outcome.and(unmuted)?;
        watch(self.b, self.windows.mute_settle).await?;
        let sent = self.sent("unmuted");
        self.leg(metrics, "unmuted_im", self.to_b(&sent), &sent)
            .await?;
        Ok(())
    }

    /// What a grid passes on while B mutes A.
    async fn muted(&mut self, metrics: &mut Metrics, scripted: bool) -> Result<(), TestFailure> {
        watch(self.b, self.windows.mute_settle).await?;
        self.b.send(Command::RequestMuteList).await?;
        let a = self.scene.a;
        let listed = match self
            .b
            .wait_for(self.windows.mute_list, |event| match event {
                Event::MuteList(entries) => Some(if entries.iter().any(|entry| entry.id == a) {
                    "names avatar A"
                } else {
                    "does not name avatar A"
                }),
                Event::MuteListUnchanged => Some("unchanged"),
                _ => None,
            })
            .await
        {
            Ok(listed) => listed,
            Err(TestFailure::Timeout(_)) => "no answer",
            Err(other) => return Err(other),
        };
        metrics.set("mute_list_after_the_mute", listed);

        let sent = self.sent("muted");
        self.leg(metrics, "muted_im", self.to_b(&sent), &sent)
            .await?;

        let brought = self
            .act(
                Who::A,
                vec![Command::ImTyping {
                    to_agent_id: AgentKey::from(self.scene.b),
                    typing: true,
                }],
            )
            .await?;
        metrics.set(
            "muted_typing_start_told_b",
            self.scene.typed(&brought.b, true),
        );
        self.a
            .send(Command::ImTyping {
                to_agent_id: AgentKey::from(self.scene.b),
                typing: false,
            })
            .await?;
        // The stop is not what the next leg asks about.
        watch(self.b, self.drain).await?;

        if scripted {
            let sent = self.sent("muted-script");
            let brought = self.order(self.scene.b, "muted", &sent).await?;
            metrics.set(
                "muted_script_im_told_b",
                self.scene.told(&brought.b, &sent, None),
            );
        }

        let sent = self.sent("muter");
        let brought = self
            .act(
                Who::B,
                vec![Command::InstantMessage {
                    to_agent_id: AgentKey::from(self.scene.a),
                    message: sent.text.clone(),
                }],
            )
            .await?;
        metrics.set(
            "im_of_the_muter_told_a",
            if brought.a.ims.iter().any(|im| sent.is(&im.message)) {
                "the message"
            } else {
                "nothing"
            },
        );
        metrics.set(
            "im_of_the_muter_told_b",
            self.scene.everything(&brought.b, &[]),
        );
        Ok(())
    }

    /// One round of B being away: B logs out, A sends it the [`AWAY`]
    /// messages, B comes back, is left alone, and then asks by `fetch`.
    async fn away_round(
        &mut self,
        metrics: &mut Metrics,
        fetch: Fetch,
        scripted: bool,
        first: bool,
    ) -> Result<(), TestFailure> {
        let round = fetch.word();
        let b = AgentKey::from(self.scene.b);
        self.b.disconnect().await?;
        self.b_is_here = false;
        tokio::time::sleep(self.windows.away_settle).await;

        // What A sends, and what it is told of each.
        let mut sent: Vec<(&str, Sent)> = Vec::new();
        for word in AWAY {
            let (brought, this) = match word {
                "typing" => {
                    let this = self.sent(word);
                    let brought = self
                        .act(
                            Who::A,
                            vec![Command::ImTyping {
                                to_agent_id: b,
                                typing: true,
                            }],
                        )
                        .await?;
                    (brought, this)
                }
                "auto_response" => {
                    let this = self.sent(word);
                    let command = Command::AutoResponse {
                        to_agent_id: b,
                        message: this.text.clone(),
                    };
                    (self.act(Who::A, vec![command]).await?, this)
                }
                "long" => {
                    let this = self.long(LONG_AWAY_BYTES);
                    (self.act(Who::A, vec![self.to_b(&this)]).await?, this)
                }
                "script" => {
                    if !scripted {
                        continue;
                    }
                    let this = self.sent(word);
                    (self.order(self.scene.b, "away", &this).await?, this)
                }
                _ => {
                    let this = self.sent(word);
                    (self.act(Who::A, vec![self.to_b(&this)]).await?, this)
                }
            };
            if first {
                metrics.set(
                    &format!("away_{word}_told_a"),
                    self.scene.everything(&brought.a, &[]),
                );
            }
            sent.push((word, this));
        }

        // B comes back, and is left alone. Nothing waits for the region
        // here: that wait would throw away what the arrival brings.
        self.b.relogin().await?;
        self.b_is_here = true;
        let windows = self.windows;
        let unasked = watch(self.b, windows.unasked).await?;
        metrics.set(
            &format!("{round}_unasked"),
            self.delivered_words(&unasked, &sent),
        );
        let waited = Instant::now();
        while self.b.cap(READ_OFFLINE_MSGS).is_none() && waited.elapsed() < windows.cap_wait {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let fetched = SystemTime::now();
        match fetch {
            Fetch::Udp | Fetch::Client => {
                self.b
                    .send(if fetch == Fetch::Udp {
                        Command::RetrieveInstantMessages
                    } else {
                        Command::RequestOfflineMessages
                    })
                    .await?;
                let mut got =
                    watch_for(self.b, windows.fetch, windows.fetch_quiet, Got::any_kept).await?;
                // What came unasked is the round's too.
                let mut all = unasked;
                all.ims.append(&mut got.ims);
                all.typed.append(&mut got.typed);
                self.record_delivery(metrics, round, &all, &sent, fetched);
                if fetch == Fetch::Udp {
                    self.b.send(Command::RetrieveInstantMessages).await?;
                    let again =
                        watch_for(self.b, windows.fetch, windows.fetch_quiet, Got::any_kept)
                            .await?;
                    metrics.set("udp_asked_again", self.delivered_words(&again, &sent));
                    let by_capability = self.read_capability().await?;
                    metrics.set(
                        "udp_then_the_capability",
                        by_capability.map_or_else(
                            || NO_CAPABILITY.to_owned(),
                            |(status, held, records)| {
                                format!("{status}, {held}, {} messages", records.len())
                            },
                        ),
                    );
                }
            }
            Fetch::Capability => self.capability_round(metrics, &sent, fetched).await?,
        }
        Ok(())
    }

    /// The words of the messages in `sent` that `got` holds, in arrival
    /// order, or `nothing`.
    fn delivered_words(&self, got: &Got, sent: &[(&str, Sent)]) -> String {
        let mut words: Vec<&str> = got
            .ims
            .iter()
            .filter_map(|im| {
                sent.iter()
                    .find(|(word, sent)| *word != "typing" && sent.is(&im.message))
                    .map(|(word, _sent)| *word)
            })
            .collect();
        if got
            .typed
            .iter()
            .any(|typed| typed.from == self.scene.a && typed.start)
        {
            words.push("typing");
        }
        if words.is_empty() {
            "nothing".to_owned()
        } else {
            words.join(", ")
        }
    }

    /// Records under `round` what `got` holds of each message in `sent`, in
    /// which order the two ordinary ones came, and what else was kept.
    fn record_delivery(
        &self,
        metrics: &mut Metrics,
        round: &str,
        got: &Got,
        sent: &[(&str, Sent)],
        fetched: SystemTime,
    ) {
        for (word, this) in sent {
            let told = if *word == "typing" {
                self.scene.typed(got, true)
            } else {
                self.scene.told(got, this, Some(fetched))
            };
            metrics.set(&format!("{round}_{word}"), told);
        }
        let place = |wanted: &str| {
            let (_word, this) = sent.iter().find(|(word, _sent)| *word == wanted)?;
            got.ims.iter().position(|im| this.is(&im.message))
        };
        metrics.set(
            &format!("{round}_order"),
            match (place("first"), place("second")) {
                (Some(first), Some(second)) if first < second => "as sent",
                (Some(_), Some(_)) => "the later one first",
                _ => "not both came",
            },
        );
        let own: Vec<&Sent> = sent.iter().map(|(_word, sent)| sent).collect();
        let others = Got {
            ims: got.ims.iter().filter(|im| im.offline).cloned().collect(),
            ..Got::default()
        };
        metrics.set(
            &format!("{round}_also_kept"),
            self.scene.everything(&others, &own),
        );
    }

    /// The round in which B reads the capability itself: what it answers,
    /// how each message reads in it, what it answers when asked again, and
    /// what `RetrieveInstantMessages` then still brings.
    async fn capability_round(
        &mut self,
        metrics: &mut Metrics,
        sent: &[(&str, Sent)],
        fetched: SystemTime,
    ) -> Result<(), TestFailure> {
        let started = Instant::now();
        let mut answer = self.read_capability().await?;
        let mut readings: i64 = 1;
        while answer
            .as_ref()
            .is_some_and(|(_status, _held, records)| records.is_empty())
            && started.elapsed() < CAP_BUDGET
        {
            tokio::time::sleep(CAP_AGAIN).await;
            answer = self.read_capability().await?;
            readings = readings.saturating_add(1);
        }
        let Some((status, held, records)) = answer else {
            // No capability: the round's messages are read the other way, so
            // that the next round starts with nothing kept.
            for (word, _sent) in sent {
                metrics.set(&format!("cap_{word}"), NO_CAPABILITY);
            }
            for key in ["cap_answer", "cap_order", "cap_asked_again", "cap_then_udp"] {
                metrics.set(key, NO_CAPABILITY);
            }
            self.b.send(Command::RetrieveInstantMessages).await?;
            let windows = self.windows;
            watch_for(self.b, windows.fetch, windows.fetch_quiet, Got::any_kept).await?;
            return Ok(());
        };
        metrics.set("cap_answer", format!("{status}, {held}"));
        metrics.set("cap_readings_until_it_answered_count", readings);
        metrics.set(
            "cap_messages_count",
            i64::try_from(records.len()).unwrap_or(i64::MAX),
        );
        let find = |word: &str, this: &Sent| {
            records.iter().position(|record| {
                if word == "typing" {
                    record_dialog(record) == Some(i32::from(ImDialog::TypingStart.to_u8()))
                } else {
                    this.is(record_message(record))
                        && record_dialog(record) != Some(i32::from(ImDialog::TypingStart.to_u8()))
                }
            })
        };
        for (word, this) in sent {
            let text = find(word, this).and_then(|at| records.get(at)).map_or_else(
                || "nothing".to_owned(),
                |record| {
                    self.scene
                        .record_text(record, (*word != "typing").then_some(this), fetched)
                },
            );
            metrics.set(&format!("cap_{word}"), text);
        }
        let place = |wanted: &str| {
            let (word, this) = sent.iter().find(|(word, _sent)| *word == wanted)?;
            find(word, this)
        };
        metrics.set(
            "cap_order",
            match (place("first"), place("second")) {
                (Some(first), Some(second)) if first < second => "as sent",
                (Some(_), Some(_)) => "the later one first",
                _ => "not both came",
            },
        );
        let again = self.read_capability().await?;
        metrics.set(
            "cap_asked_again",
            again.map_or_else(
                || NO_CAPABILITY.to_owned(),
                |(status, held, records)| format!("{status}, {held}, {} messages", records.len()),
            ),
        );
        self.b.send(Command::RetrieveInstantMessages).await?;
        let windows = self.windows;
        let got = watch_for(self.b, windows.fetch, windows.fetch_quiet, Got::any_kept).await?;
        metrics.set("cap_then_udp", self.delivered_words(&got, sent));
        Ok(())
    }
}

/// A census of instant messages: what a grid passes on of one, what it says
/// of it, what it tells the sender, and what it keeps for an avatar that is
/// away.
#[derive(Debug)]
pub struct InstantMessages;

impl GridTest for InstantMessages {
    fn name(&self) -> &'static str {
        "instant-messages"
    }

    fn description(&self) -> &'static str {
        "What a grid passes on of a viewer's and a script's instant messages, what it says of \
         each, what a mute changes, and what it keeps for an avatar that is away"
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
        Duration::from_secs(2400)
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
            let held: Vec<(&str, Measured<Answer>)> = MEASURED
                .iter()
                .filter(|(key, _measured)| fake_exemption(grid, key).is_none())
                .copied()
                .collect();
            outcome
                .and(cleanup)
                .and_then(|()| hold_to_measured(grid, &metrics, &held, SOURCE))
        })
    }
}

/// The census: both avatars to the spot, the legs, and B's three absences.
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let pace = Pace::of(grid);
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "instant-messages needs a secondary avatar".to_owned(),
        ));
    };
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    a.wait_for_region(REGION_TIMEOUT).await?;
    let settled =
        settle_scene_with_avatar(a, grid, build_position, SETTLE_WINDOW, pace.settle_idle).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let a_at = settled.avatar.ok_or_else(|| {
        TestFailure::Assertion("avatar A was never sent its own avatar".to_owned())
    })?;
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
        prim: None,
        unknown: Vec::new(),
    };
    let mut census = Census {
        a,
        b,
        scene,
        quiet: run.pace.quiet,
        drain: DRAIN,
        said: 0,
        b_is_here: true,
        windows: Windows::of(grid),
    };
    watch(census.b, census.drain).await?;
    census.mail(metrics).await?;
    census.empty_the_store(metrics).await?;

    census.viewer_legs(metrics).await?;

    // A grid that runs no script is asked the rest without one.
    let scripted = !grid.is_fake();
    if scripted {
        census_scripted(&mut census, &mut run, residue, metrics).await?;
    }
    census.mute_legs(metrics, scripted).await?;
    census
        .away_round(metrics, Fetch::Udp, scripted, true)
        .await?;
    census
        .away_round(metrics, Fetch::Capability, scripted, false)
        .await?;
    census
        .away_round(metrics, Fetch::Client, scripted, false)
        .await
}

/// Rezzes the cube beside A, puts the probe into it and asks the script's
/// legs.
async fn census_scripted(
    census: &mut Census<'_>,
    run: &mut Run,
    residue: &mut Residue,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    let cube = rez_cube(census.a, run, residue, 0).await?;
    census.scene.prim = Some((
        cube.full_id.uuid(),
        cube.motion.position.clone(),
        String::new(),
    ));
    let name = census.install(&cube).await?;
    if let Some((_prim, _at, prim_name)) = &mut census.scene.prim {
        *prim_name = name;
    }
    census.script_legs(metrics).await
}

#[cfg(test)]
mod tests {
    use super::{bucket_text, kept_records, names_a_place, probe_source, session_of, time_text};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::Uuid;
    use sl_wire::Llsd;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn the_probe_compiles() -> Result<(), String> {
        let source = probe_source();
        sl_lsl_runtime::compile(&source)
            .map(|_program| ())
            .map_err(|errors| format!("the probe does not compile: {errors:?}\n{source}"))
    }

    #[test]
    fn a_session_id_is_the_same_from_either_side() {
        let (left, right) = (Uuid::from_u128(0x0f0f), Uuid::from_u128(0xff00));
        assert_eq!(session_of(left, right), session_of(right, left));
        assert_eq!(session_of(left, right), Uuid::from_u128(0xf00f));
    }

    #[test]
    fn what_a_metric_says_of_a_bucket() {
        assert_eq!(bucket_text(&[], None), "no bucket");
        assert_eq!(bucket_text(&[0], None), "a bucket of one zero byte");
        assert_eq!(
            bucket_text(b"Some Region/12/34.5/6\0", None),
            "a bucket naming a region and a place in it"
        );
        assert_eq!(bucket_text(b"abc", None), "a bucket of 3 bytes");
        assert!(!names_a_place("12/34/5"));
        assert!(!names_a_place("a/b/c/d"));
    }

    #[test]
    fn what_a_metric_says_of_a_time() {
        let sent = UNIX_EPOCH + Duration::from_secs(1_000_000);
        let fetched = UNIX_EPOCH + Duration::from_secs(1_000_300);
        assert_eq!(time_text(None, Some(sent), None, None), "no time");
        assert_eq!(
            time_text(Some(1_000_003), Some(sent), Some(fetched), None),
            "the time it was sent"
        );
        assert_eq!(
            time_text(Some(1_000_299), Some(sent), Some(fetched), None),
            "the time it was asked for"
        );
        assert_eq!(
            time_text(Some(5), Some(sent), Some(fetched), Some(5)),
            "the time as sent"
        );
        assert_eq!(
            time_text(Some(5), Some(SystemTime::now()), None, None),
            "another time"
        );
    }

    #[test]
    fn how_an_answer_holds_its_messages() {
        let message = Llsd::Map(std::iter::once(("dialog".to_owned(), Llsd::Integer(0))).collect());
        let flat = Llsd::Array(vec![message.clone()]);
        let nested = Llsd::Array(vec![Llsd::Array(vec![message.clone(), message.clone()])]);
        let map = Llsd::Map(
            std::iter::once(("messages".to_owned(), Llsd::Array(vec![message]))).collect(),
        );
        assert_eq!(kept_records(None), ("no LLSD", Vec::new()));
        assert_eq!(kept_records(Some(&flat)).0, "an array of messages");
        let (held, records) = kept_records(Some(&nested));
        assert_eq!(held, "an array holding an array of messages");
        assert_eq!(records.len(), 2);
        assert_eq!(
            kept_records(Some(&map)).0,
            "a map with an array of messages"
        );
        assert_eq!(
            kept_records(Some(&Llsd::Array(Vec::new()))).0,
            "an empty array"
        );
    }
}
