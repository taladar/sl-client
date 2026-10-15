//! A census of group chat and ad-hoc conferences: what starting, joining,
//! answering, speaking in, moderating and leaving a session bring each of
//! three avatars, what a grid keeps of a session for an avatar that is away,
//! and what its server history holds.
//!
//! A group session's id is the group's; a viewer opens it with
//! `IM_SESSION_GROUP_START` ([`Command::StartGroupSession`]) and speaks into
//! it with `IM_SESSION_SEND`. An ad-hoc conference is started over the
//! `ChatSessionRequest` capability's `start conference`, or with the
//! deprecated `IM_SESSION_CONFERENCE_START` where a region has no such
//! capability ([`Command::StartConference`]). Both reach a participant as an
//! `ImprovedInstantMessage` ([`Event::GroupSessionMessage`],
//! [`Event::ConferenceSessionMessage`]) or as an event-queue
//! `ChatterBoxInvitation` ([`Event::ConferenceInvited`]), and a grid states a
//! session's roster in `ChatterBoxSessionAgentListUpdates`
//! ([`Event::ChatSessionAgentsUpdated`]).
//!
//! Avatars **A** and **B** are members of the census group, avatar **C** is
//! not. One question a leg:
//!
//! 1. **A group session**: A opens it; A speaks before B has joined; B
//!    accepts; B and A speak; C, who is no member, opens it and speaks.
//! 2. **Moderation**: A takes B's text away over the capability's `mute
//!    update`, B speaks, A gives it back.
//! 3. **Leaving and declining**: B leaves the session and A speaks; B
//!    declines and A speaks; B joins again.
//! 4. **Server history**: B reads the group session's history over the
//!    capability, as the client does, and again a little later; A says ten,
//!    twenty and thirty lines, and B reads it after each.
//! 5. **Away**: B logs out and A speaks; B comes back and is left alone,
//!    then asks for what was kept and reads the history, before and after
//!    it joins the session again.
//! 6. **A conference**: A starts one with B and C and speaks; B accepts and
//!    speaks; C declines and A speaks; A invites C again, C accepts and
//!    speaks; B reads the conference's history; C leaves and B speaks; A, who
//!    started it, leaves and B speaks.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/chat.md` § Group and conference sessions; every run is
//! held to it.
//!
//! `3av`, `[both]`. The census measures in the fixtures' `census_group` (the
//! `groups` census founds one where none is named); B is left a member of it,
//! and C is left as it was found. The fake grid has no group chat yet
//! (`server-fake-grid-group-chat`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sl_client_tokio::{
    AgentKey, AnyMessage, ChatLifecycleView, ChatSessionAgentUpdate, ChatSessionInfo,
    ChatSessionKind, Command, Diagnostic, Event, GroupKey, GroupMembership, ImDialog, ImSessionId,
    InstantMessage, RegionCoordinates, Reliability, ServerHistoryMessage, SessionTransition, Uuid,
    Vector,
};
use sl_wire::Llsd;
use sl_wire::messages::{
    ImprovedInstantMessage, ImprovedInstantMessageAgentDataBlock,
    ImprovedInstantMessageEstateBlockBlock, ImprovedInstantMessageMessageBlockBlock,
};

use crate::cases::asset_http::without_ids;
use crate::cases::friends_presence::kept_records;
use crate::cases::instant_messages::session_of;
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{
    CapAnswer, DRAIN, OPENSIM_START, cap_get, cap_post, hold_to_measured, http_client, shape_of,
};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, count_metric, is_opensim};

/// What every text of the census begins with.
const TEXT_TAG: &str = "gc-";

/// How long the acting avatar listens after a gesture.
const LISTEN: Duration = Duration::from_secs(6);

/// How long a grid is given to act on a change before the next leg.
const SETTLE: Duration = Duration::from_secs(3);

/// How long a grid is given to drop an avatar that logged out.
const AWAY_SETTLE: Duration = Duration::from_secs(8);

/// How long an avatar that came back is left alone, to see what it is sent
/// without asking.
const UNASKED_WINDOW: Duration = Duration::from_secs(25);

/// How long an avatar waits for what it asked for.
const FETCH_WINDOW: Duration = Duration::from_secs(15);

/// How long the capability map may take to arrive after a login.
const CAP_WAIT: Duration = Duration::from_secs(15);

/// How many lines A says in each step before the history is read again.
const BURSTS: [u32; 3] = [10, 20, 30];

/// The pause between two lines of the burst.
const BURST_PAUSE: Duration = Duration::from_millis(250);

/// How long B waits before reading the history again.
const HISTORY_PAUSE: Duration = Duration::from_secs(10);

/// How far from now a message's time may be and still be now, in seconds:
/// the two clocks are a grid's and ours.
const TIME_SLACK_SECS: u64 = 10;

/// The capability every modern session request goes to.
const CHAT_SESSION_REQUEST: &str = "ChatSessionRequest";

/// The capability that serves what was kept for an avatar.
const READ_OFFLINE_MSGS: &str = "ReadOfflineMsgs";

/// What a metric reads where the grid has no such capability.
const NO_CAPABILITY: &str = "no capability";

/// What a metric about B reads while B is away.
const AWAY: &str = "away";

/// Where the measurements below are written down.
const SOURCE: &str =
    "book/src/gridspec/chat.md § Group and conference sessions (group-chat, 2026-10-15)";

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
/// Not held: which session lines the client's own history read caught (the
/// reply races the reads around it), and A's roster pushes when it starts the
/// conference (their order changes). Held as anything on Second Life: the
/// conference roster pushes after a decline, a leave and an invitation, and
/// B's on rejoining after its return, which arrive in one run and not the
/// next.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("c_a_leave_told_a", both(Answer::Text("nothing"))),
    (
        "c_a_leave_told_b",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    ("c_a_leave_told_c", both(Answer::Text("nothing"))),
    ("c_after_a_left_told_a", both(Answer::Text("nothing"))),
    (
        "c_after_a_left_told_b",
        each(
            Answer::Text(
                "conference message SessionSend from B, B's name, to B, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 78-c_after_a_left | conference message SessionSend from nil, the name \"Second Life\", to B, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text \"You are the only participant in this IM session. Your message is not being broadcast at this time.\"",
            ),
            Answer::Text(
                "group message SessionSend from B (a group's), B's name, to B, id A's id for the conference, a region, place zero, estate 1, now, not kept, a bucket of 1 zero bytes, text line 78-c_after_a_left",
            ),
        ),
    ),
    ("c_after_a_left_told_c", both(Answer::Text("nothing"))),
    (
        "c_after_c_left_told_a",
        each(
            Answer::Text(
                "conference message SessionSend from B, B's name, to A, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 77-c_after_c_left",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "c_after_c_left_told_b",
        each(
            Answer::Text(
                "conference message SessionSend from B, B's name, to B, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 77-c_after_c_left",
            ),
            Answer::Text(
                "group message SessionSend from B (a group's), B's name, to B, id A's id for the conference, a region, place zero, estate 1, now, not kept, a bucket of 1 zero bytes, text line 77-c_after_c_left",
            ),
        ),
    ),
    ("c_after_c_left_told_c", both(Answer::Text("nothing"))),
    (
        "c_after_decline_told_a",
        each(
            Answer::Text(
                "conference message SessionSend from A, A's name, to A, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 75-c_after_decline",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to A, id A's id for the conference, a region, place zero, estate 1, now, not kept, a bucket of 1 zero bytes, text line 75-c_after_decline",
            ),
        ),
    ),
    (
        "c_after_decline_told_b",
        each(
            Answer::Text(
                "conference message SessionSend from A, A's name, to B, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 75-c_after_decline",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("c_after_decline_told_c", both(Answer::Text("nothing"))),
    (
        "c_b_accept_raw",
        each(Answer::Text("200: {}"), Answer::Text("no capability")),
    ),
    (
        "c_b_accept_sessions",
        each(
            Answer::Text(
                "conference session the grid's id for the conference: joined, participants [A, B, C], voice yes; group session the group: joined, participants [A, B], voice yes",
            ),
            Answer::Text(
                "conference session A's id for the conference: joined, participants [], voice no; group session the group: joined, participants [B], voice no",
            ),
        ),
    ),
    ("c_b_accept_told_a", both(Answer::Text("nothing"))),
    (
        "c_b_accept_told_b",
        each(
            Answer::Text(
                "roster of the grid's id for the conference: C ENTER voice yes moderator no; A ENTER voice yes moderator no text muted no; B ENTER voice yes moderator no text muted no",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("c_b_accept_told_c", both(Answer::Text("nothing"))),
    ("c_b_leave_told_a", both(Answer::Text("nothing"))),
    ("c_b_leave_told_b", both(Answer::Text("nothing"))),
    ("c_b_leave_told_c", both(Answer::Text("nothing"))),
    (
        "c_b_says_told_a",
        each(
            Answer::Text(
                "conference message SessionSend from B, B's name, to A, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 74-c_b_says",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "c_b_says_told_b",
        each(
            Answer::Text(
                "conference message SessionSend from B, B's name, to B, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 74-c_b_says",
            ),
            Answer::Text(
                "group message SessionSend from B (a group's), B's name, to B, id A's id for the conference, a region, place zero, estate 1, now, not kept, a bucket of 1 zero bytes, text line 74-c_b_says",
            ),
        ),
    ),
    (
        "c_b_says_told_c",
        each(
            Answer::Text(
                "invitation SessionSend to the grid's id for the conference, from B, B's name, session name the name \"<A> Conference\", no region, place zero, estate 0, now, a bucket of 25 bytes reading \"<A> Conference\", text line 74-c_b_says",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "c_c_accept_raw",
        each(Answer::Text("200: {}"), Answer::Text("no capability")),
    ),
    (
        "c_c_accept_sessions",
        each(
            Answer::Text(
                "conference session the grid's id for the conference: joined, participants [A, B, C], voice yes; group session the group: joined, participants [], voice no",
            ),
            Answer::Text(
                "conference session A's id for the conference: joined, participants [], voice no; group session the group: joined, participants [C], voice no",
            ),
        ),
    ),
    ("c_c_accept_told_a", both(Answer::Text("nothing"))),
    ("c_c_accept_told_b", both(Answer::Text("nothing"))),
    (
        "c_c_accept_told_c",
        each(
            Answer::Text(
                "roster of the grid's id for the conference: C ENTER voice yes moderator no text muted no; A ENTER voice yes moderator no text muted no; B ENTER voice yes moderator no text muted no",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "c_c_decline_told_a",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "c_c_decline_told_b",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    ("c_c_decline_told_c", both(Answer::Text("nothing"))),
    (
        "c_c_leave_told_a",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "c_c_leave_told_b",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    ("c_c_leave_told_c", both(Answer::Text("nothing"))),
    (
        "c_c_says_told_a",
        each(
            Answer::Text(
                "conference message SessionSend from C, C's name, to A, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 76-c_c_says",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "c_c_says_told_b",
        each(
            Answer::Text(
                "conference message SessionSend from C, C's name, to B, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 76-c_c_says",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "c_c_says_told_c",
        each(
            Answer::Text(
                "conference message SessionSend from C, C's name, to C, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 76-c_c_says",
            ),
            Answer::Text(
                "group message SessionSend from C (a group's), C's name, to C, id A's id for the conference, a region, place zero, estate 1, now, not kept, a bucket of 1 zero bytes, text line 76-c_c_says",
            ),
        ),
    ),
    (
        "c_first_told_a",
        each(
            Answer::Text(
                "conference message SessionSend from A, A's name, to A, id the grid's id for the conference, no region, place zero, estate 0, now, not kept, a bucket of 25 bytes reading \"<A> Conference\", text line 73-c_first",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to A, id A's id for the conference, a region, place zero, estate 1, now, not kept, a bucket of 1 zero bytes, text line 73-c_first",
            ),
        ),
    ),
    (
        "c_first_told_b",
        each(
            Answer::Text(
                "invitation SessionSend to the grid's id for the conference, from A, A's name, session name the name \"<A> Conference\", no region, place zero, estate 0, now, a bucket of 25 bytes reading \"<A> Conference\", text line 73-c_first",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "c_first_told_c",
        each(
            Answer::Text(
                "invitation SessionSend to the grid's id for the conference, from A, A's name, session name the name \"<A> Conference\", no region, place zero, estate 0, now, a bucket of 25 bytes reading \"<A> Conference\", text line 73-c_first",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "c_history",
        each(Answer::Text("403: no LLSD"), Answer::Text("no capability")),
    ),
    ("c_history_lines_count", both(Answer::Count(-1))),
    (
        "c_id",
        each(
            Answer::Text("the grid's id for the conference"),
            Answer::Text("A's id for the conference"),
        ),
    ),
    (
        "c_reinvite_told_a",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    (
        "c_reinvite_told_b",
        each(Answer::Any, Answer::Text("nothing")),
    ),
    ("c_reinvite_told_c", both(Answer::Text("nothing"))),
    (
        "c_start_reply",
        each(
            Answer::Text(
                "start of A's id for the conference became the grid's id for the conference, taken, name the name \"<A> Conference\", voice yes",
            ),
            Answer::Text("no reply"),
        ),
    ),
    ("c_start_told_b", both(Answer::Text("nothing"))),
    ("c_start_told_c", both(Answer::Text("nothing"))),
    ("c_was_a_member", both(Answer::Flag(false))),
    (
        "g_a_says_told_a",
        each(
            Answer::Text(
                "group message SessionSend from A, A's name, to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 3-g_a_says",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 3-g_a_says",
            ),
        ),
    ),
    (
        "g_a_says_told_b",
        each(
            Answer::Text(
                "group message SessionSend from A, A's name, to B, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 3-g_a_says",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to B, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 3-g_a_says",
            ),
        ),
    ),
    ("g_a_says_told_c", both(Answer::Text("nothing"))),
    (
        "g_accept_raw",
        each(Answer::Text("200: {}"), Answer::Text("no capability")),
    ),
    (
        "g_accept_sessions",
        each(
            Answer::Text("group session the group: joined, participants [A, B], voice yes"),
            Answer::Text("group session the group: joined, participants [B], voice no"),
        ),
    ),
    ("g_accept_told_a", both(Answer::Text("nothing"))),
    (
        "g_accept_told_b",
        each(
            Answer::Text(
                "roster of the group: A ENTER voice yes moderator yes; B ENTER voice yes moderator no text muted no | server history of the group: this run's lines [1-g_first], earlier runs' lines no, other lines no",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("g_accept_told_c", both(Answer::Text("nothing"))),
    (
        "g_after_decline_told_a",
        each(
            Answer::Text(
                "group message SessionSend from A, A's name, to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 9-g_after_decline | group message SessionSend from nil, the name \"Second Life\", to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text \"You are the only participant in this IM session. Your message is not being broadcast at this time.\"",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 9-g_after_decline",
            ),
        ),
    ),
    (
        "g_after_decline_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to B, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 9-g_after_decline",
            ),
        ),
    ),
    ("g_after_decline_told_c", both(Answer::Text("nothing"))),
    (
        "g_after_leave_told_a",
        each(
            Answer::Text(
                "group message SessionSend from A, A's name, to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 8-g_after_leave | group message SessionSend from nil, the name \"Second Life\", to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text \"You are the only participant in this IM session. Your message is not being broadcast at this time.\"",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 8-g_after_leave",
            ),
        ),
    ),
    (
        "g_after_leave_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to B, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 8-g_after_leave",
            ),
        ),
    ),
    ("g_after_leave_told_c", both(Answer::Text("nothing"))),
    (
        "g_after_rejoin_told_a",
        each(
            Answer::Text(
                "group message SessionSend from A, A's name, to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 10-g_after_rejoin",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 10-g_after_rejoin",
            ),
        ),
    ),
    (
        "g_after_rejoin_told_b",
        each(
            Answer::Text(
                "group message SessionSend from A, A's name, to B, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 10-g_after_rejoin",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to B, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 10-g_after_rejoin",
            ),
        ),
    ),
    ("g_after_rejoin_told_c", both(Answer::Text("nothing"))),
    (
        "g_away_capability",
        each(
            Answer::Text("200: this run's lines [], earlier runs' lines no, other lines no"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "g_away_history",
        each(
            Answer::Text(
                "200: an array; this run's lines [], earlier runs' lines no, other lines no; a record -",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "g_away_history_joined",
        each(
            Answer::Text(
                "200: an array; this run's lines [], earlier runs' lines no, other lines no; a record -",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "g_away_history_joined_lines_count",
        each(Answer::Count(0), Answer::Count(-1)),
    ),
    (
        "g_away_history_lines_count",
        each(Answer::Count(0), Answer::Count(-1)),
    ),
    (
        "g_away_rejoin_told_a",
        each(
            Answer::Text(
                "roster of the group: B ENTER voice yes moderator no | roster of the group: B ENTER voice yes moderator no",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "g_away_rejoin_told_b",
        each(
            Answer::Any,
            Answer::Text(
                "roster of the group: B ENTER voice no moderator no text muted no | start of the group kept its id, taken, name the group's name, voice no",
            ),
        ),
    ),
    ("g_away_rejoin_told_c", both(Answer::Text("nothing"))),
    ("g_away_retrieved", both(Answer::Text("nothing"))),
    ("g_away_unasked", both(Answer::Text("nothing"))),
    (
        "g_b_says_told_a",
        each(
            Answer::Text(
                "group message SessionSend from B, B's name, to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 2-g_b_says",
            ),
            Answer::Text(
                "group message SessionSend from B (a group's), B's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 2-g_b_says",
            ),
        ),
    ),
    (
        "g_b_says_told_b",
        each(
            Answer::Text(
                "group message SessionSend from B, B's name, to B, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 2-g_b_says",
            ),
            Answer::Text(
                "group message SessionSend from B (a group's), B's name, to B, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 2-g_b_says",
            ),
        ),
    ),
    ("g_b_says_told_c", both(Answer::Text("nothing"))),
    ("g_burst1_heard_by_b_count", both(Answer::Count(10))),
    (
        "g_burst1_history",
        each(
            Answer::Text("200: 0 lines, 0 of this run's, oldest -, newest -"),
            Answer::Text("no capability"),
        ),
    ),
    ("g_burst2_heard_by_b_count", both(Answer::Count(20))),
    (
        "g_burst2_history",
        each(
            Answer::Text("200: 0 lines, 0 of this run's, oldest -, newest -"),
            Answer::Text("no capability"),
        ),
    ),
    ("g_burst3_heard_by_b_count", both(Answer::Count(30))),
    (
        "g_burst3_history",
        each(
            Answer::Text("200: 0 lines, 0 of this run's, oldest -, newest -"),
            Answer::Text("no capability"),
        ),
    ),
    ("g_decline_told_a", both(Answer::Text("nothing"))),
    ("g_decline_told_b", both(Answer::Text("nothing"))),
    ("g_decline_told_c", both(Answer::Text("nothing"))),
    (
        "g_empty_bucket_told_a",
        each(
            Answer::Text("answer about the group: \"message\" refused, error \"generic\""),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 4-g_empty_bucket",
            ),
        ),
    ),
    (
        "g_empty_bucket_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to B, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 4-g_empty_bucket",
            ),
        ),
    ),
    ("g_empty_bucket_told_c", both(Answer::Text("nothing"))),
    (
        "g_first_told_a",
        each(
            Answer::Text(
                "group message SessionSend from A, A's name, to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 1-g_first",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 1-g_first",
            ),
        ),
    ),
    (
        "g_first_told_b",
        each(
            Answer::Text(
                "invitation SessionSend to the group (a group's), from A, A's name, session name the group's name, no region, place zero, estate 0, now, a bucket of 19 bytes reading \"<the group's name>\", text line 1-g_first",
            ),
            Answer::Text(
                "invitation SessionSend to the group (a group's), from A, A's name, session name the group's name, no region, place zero, estate 1, now, a bucket of 19 bytes reading \"<the group's name>\", text line 1-g_first | roster of the group: B ENTER voice no moderator no text muted no",
            ),
        ),
    ),
    ("g_first_told_c", both(Answer::Text("nothing"))),
    (
        "g_history",
        each(
            Answer::Text(
                "200: an array; this run's lines [1-g_first, 2-g_b_says, 3-g_a_says, 7-g_unmuted_says, 8-g_after_leave, 9-g_after_decline, 10-g_after_rejoin], earlier runs' lines no, other lines no; a record {from:string,from_id:uuid,message:string,num:integer,time:integer}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "g_history_again",
        each(
            Answer::Text(
                "200: an array; this run's lines [1-g_first, 2-g_b_says, 3-g_a_says, 7-g_unmuted_says, 8-g_after_leave, 9-g_after_decline, 10-g_after_rejoin], earlier runs' lines no, other lines no; a record {from:string,from_id:uuid,message:string,num:integer,time:integer}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "g_history_again_lines_count",
        each(Answer::Count(7), Answer::Count(-1)),
    ),
    (
        "g_history_lines_count",
        each(Answer::Count(7), Answer::Count(-1)),
    ),
    (
        "g_leave_told_a",
        each(
            Answer::Text(
                "roster of the group: B LEAVE moderator no | roster of the group: B LEAVE moderator no",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("g_leave_told_b", both(Answer::Text("nothing"))),
    ("g_leave_told_c", both(Answer::Text("nothing"))),
    (
        "g_mute_answer",
        each(Answer::Text("200: {}"), Answer::Text("no capability")),
    ),
    (
        "g_mute_told_a",
        each(
            Answer::Text(
                "roster of the group: B no transition moderator no text muted yes | roster of the group: B no transition moderator no text muted yes",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "g_mute_told_b",
        each(
            Answer::Text("roster of the group: B no transition moderator no text muted yes"),
            Answer::Text("nothing"),
        ),
    ),
    ("g_mute_told_c", both(Answer::Text("nothing"))),
    (
        "g_muted_says_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "group message SessionSend from B (a group's), B's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 6-g_muted_says",
            ),
        ),
    ),
    (
        "g_muted_says_told_b",
        each(
            Answer::Text("answer about the group: \"message\" refused, error \"generic\""),
            Answer::Text(
                "group message SessionSend from B (a group's), B's name, to B, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 6-g_muted_says",
            ),
        ),
    ),
    ("g_muted_says_told_c", both(Answer::Text("nothing"))),
    (
        "g_outsider_says_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "group message SessionSend from C (a group's), C's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 5-g_outsider_says",
            ),
        ),
    ),
    (
        "g_outsider_says_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "group message SessionSend from C (a group's), C's name, to B, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 5-g_outsider_says",
            ),
        ),
    ),
    (
        "g_outsider_says_told_c",
        each(
            Answer::Text("answer about the group: \"message\" refused, error \"generic\""),
            Answer::Text(
                "group message SessionSend from C (a group's), C's name, to C, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 5-g_outsider_says",
            ),
        ),
    ),
    ("g_outsider_start_told_a", both(Answer::Text("nothing"))),
    ("g_outsider_start_told_b", both(Answer::Text("nothing"))),
    (
        "g_outsider_start_told_c",
        each(
            Answer::Text(
                "start of the group kept its id, refused, name no name, voice no, error \"unverified\"",
            ),
            Answer::Text(
                "roster of the group: C ENTER voice no moderator no text muted no | start of the group kept its id, taken, name the group's name, voice no",
            ),
        ),
    ),
    (
        "g_rejoin_told_a",
        each(
            Answer::Text(
                "roster of the group: B ENTER voice yes moderator no text muted no | roster of the group: B ENTER voice yes moderator no text muted no",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "g_rejoin_told_b",
        each(
            Answer::Text(
                "roster of the group: A ENTER voice yes moderator yes; B ENTER voice yes moderator no text muted no | roster of the group: B ENTER voice yes moderator no text muted no | start of the group kept its id, taken, name the group's name, voice yes | server history of the group: this run's lines [1-g_first, 2-g_b_says, 3-g_a_says, 7-g_unmuted_says, 8-g_after_leave, 9-g_after_decline], earlier runs' lines no, other lines no",
            ),
            Answer::Text(
                "roster of the group: B ENTER voice no moderator no text muted no | start of the group kept its id, taken, name the group's name, voice no",
            ),
        ),
    ),
    ("g_rejoin_told_c", both(Answer::Text("nothing"))),
    (
        "g_start_told_a",
        each(
            Answer::Text(
                "roster of the group: A ENTER voice yes moderator yes; B ENTER voice yes moderator no | start of the group kept its id, taken, name the group's name, voice yes",
            ),
            Answer::Text(
                "roster of the group: A ENTER voice no moderator no text muted no | start of the group kept its id, taken, name the group's name, voice no",
            ),
        ),
    ),
    ("g_start_told_b", both(Answer::Text("nothing"))),
    ("g_start_told_c", both(Answer::Text("nothing"))),
    (
        "g_unmute_answer",
        each(Answer::Text("200: {}"), Answer::Text("no capability")),
    ),
    (
        "g_unmute_told_a",
        each(
            Answer::Text(
                "roster of the group: B no transition moderator no text muted no | roster of the group: B no transition moderator no text muted no",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "g_unmute_told_b",
        each(
            Answer::Text("roster of the group: B no transition moderator no text muted no"),
            Answer::Text("nothing"),
        ),
    ),
    ("g_unmute_told_c", both(Answer::Text("nothing"))),
    (
        "g_unmuted_says_told_a",
        each(
            Answer::Text(
                "group message SessionSend from B, B's name, to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 7-g_unmuted_says",
            ),
            Answer::Text(
                "group message SessionSend from B (a group's), B's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 7-g_unmuted_says",
            ),
        ),
    ),
    (
        "g_unmuted_says_told_b",
        each(
            Answer::Text(
                "group message SessionSend from B, B's name, to B, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 7-g_unmuted_says",
            ),
            Answer::Text(
                "group message SessionSend from B (a group's), B's name, to B, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 7-g_unmuted_says",
            ),
        ),
    ),
    ("g_unmuted_says_told_c", both(Answer::Text("nothing"))),
    (
        "g_while_away_told_a",
        each(
            Answer::Text(
                "group message SessionSend from A, A's name, to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 71-g_while_away | group message SessionSend from nil, the name \"Second Life\", to A, id the group, no region, place zero, estate 0, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text \"You are the only participant in this IM session. Your message is not being broadcast at this time.\" | roster of the group: B LEAVE moderator no | roster of the group: B LEAVE moderator no",
            ),
            Answer::Text(
                "group message SessionSend from A (a group's), A's name, to A, id the group, a region, place zero, estate 1, now, not kept, a bucket of 19 bytes reading \"<the group's name>\", text line 71-g_while_away",
            ),
        ),
    ),
    ("g_while_away_told_b", both(Answer::Text("away"))),
    ("g_while_away_told_c", both(Answer::Text("nothing"))),
    (
        "session_capabilities_granted",
        each(
            Answer::Text("ChatSessionRequest, ReadOfflineMsgs"),
            Answer::Text("none"),
        ),
    ),
];

/// An invitation as a session was sent it.
#[derive(Debug, Clone)]
struct Invitation {
    /// The session it invites to.
    session_id: Uuid,
    /// Who it is from.
    from: AgentKey,
    /// Under what name.
    from_name: String,
    /// The dialog it states.
    dialog: ImDialog,
    /// Whether it says it is a group's.
    from_group: bool,
    /// The session's name.
    session_name: String,
    /// The text it carries.
    message: String,
    /// The region it states.
    region_id: Uuid,
    /// The place it states.
    position: RegionCoordinates,
    /// The estate it states.
    estate: u32,
    /// The time it states.
    timestamp: Option<u32>,
    /// Its bucket.
    bucket: Vec<u8>,
}

/// A start reply as a session was sent it.
#[derive(Debug, Clone)]
struct Started {
    /// The id the start named.
    temp: Uuid,
    /// The id the grid gave the session.
    real: Uuid,
    /// Whether it started.
    success: bool,
    /// The session's name.
    name: String,
    /// Whether it has voice.
    voice: bool,
    /// Why it did not start.
    error: String,
}

/// What one session was sent during a window.
#[derive(Debug, Default)]
struct Got {
    /// Every invitation, in arrival order.
    invitations: Vec<Invitation>,
    /// Every session message, by whether it was read as a group's.
    messages: Vec<(bool, InstantMessage)>,
    /// Every UDP participant notice: whether it was read as a group's, the
    /// session, the agent, whether it joined.
    participants: Vec<(bool, Uuid, AgentKey, bool)>,
    /// Every roster update.
    rosters: Vec<(Uuid, Vec<ChatSessionAgentUpdate>)>,
    /// Every start reply.
    started: Vec<Started>,
    /// Every answer to something said or done in a session: the session,
    /// whether it was taken, what it was, why not.
    replies: Vec<(Uuid, bool, String, String)>,
    /// Every session the grid closed: the session, why.
    closed: Vec<(Uuid, String)>,
    /// Every server history the client read.
    histories: Vec<(ChatSessionKind, Vec<ServerHistoryMessage>)>,
    /// Every other instant message.
    ims: Vec<InstantMessage>,
    /// The text of every alert.
    alerts: Vec<String>,
    /// Every list of memberships.
    memberships: Vec<Vec<GroupMembership>>,
    /// Every join answered.
    joined: Vec<(Uuid, bool)>,
    /// Every list of sessions the client keeps.
    sessions: Vec<Arc<[ChatSessionInfo]>>,
    /// Every event-queue message the client did not read, or could not.
    unread: Vec<String>,
}

impl Got {
    /// Whether anything that was kept for the avatar is among the messages.
    fn any_kept(&self) -> bool {
        self.ims.iter().any(|im| im.offline) || self.messages.iter().any(|(_, im)| im.offline)
    }
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(got: &mut Got, event: &Event) -> Option<()> {
    match event {
        Event::ConferenceInvited {
            session_id,
            from_agent_id,
            from_name,
            dialog,
            from_group,
            session_name,
            message,
            region_id,
            position,
            parent_estate_id,
            timestamp,
            binary_bucket,
        } => got.invitations.push(Invitation {
            session_id: *session_id,
            from: *from_agent_id,
            from_name: from_name.clone(),
            dialog: *dialog,
            from_group: *from_group,
            session_name: session_name.clone(),
            message: message.clone(),
            region_id: *region_id,
            position: *position,
            estate: *parent_estate_id,
            timestamp: *timestamp,
            bucket: binary_bucket.clone(),
        }),
        Event::GroupSessionMessage { im, .. } => got.messages.push((true, (**im).clone())),
        Event::ConferenceSessionMessage { im, .. } => got.messages.push((false, (**im).clone())),
        Event::GroupSessionParticipant {
            group_id,
            agent_id,
            joined,
        } => got
            .participants
            .push((true, group_id.uuid(), *agent_id, *joined)),
        Event::ConferenceSessionParticipant {
            session_id,
            agent_id,
            joined,
        } => got
            .participants
            .push((false, *session_id, *agent_id, *joined)),
        Event::ChatSessionAgentsUpdated {
            session_id,
            updates,
        } => got.rosters.push((*session_id, updates.clone())),
        Event::ChatSessionStarted {
            temp_session_id,
            session_id,
            success,
            session_name,
            voice_enabled,
            error,
        } => got.started.push(Started {
            temp: temp_session_id.get(),
            real: session_id.get(),
            success: *success,
            name: session_name.clone(),
            voice: *voice_enabled,
            error: error.clone(),
        }),
        Event::ChatSessionEventReply {
            session_id,
            success,
            event,
            error,
        } => got
            .replies
            .push((*session_id, *success, event.clone(), error.clone())),
        Event::ChatSessionForceClosed { session_id, reason } => {
            got.closed.push((*session_id, reason.clone()));
        }
        Event::SessionServerHistory { kind, messages } => {
            got.histories.push((*kind, messages.clone()));
        }
        Event::InstantMessageReceived(im) => got.ims.push((**im).clone()),
        Event::AlertMessage { message, .. } | Event::AgentAlertMessage { message, .. } => {
            got.alerts.push(message.clone());
        }
        Event::GroupMemberships(groups) => got.memberships.push(groups.clone()),
        Event::JoinGroupResult { group_id, success } => {
            got.joined.push((group_id.uuid(), *success));
        }
        Event::ChatSessions(sessions) => got.sessions.push(Arc::clone(sessions)),
        _ => return None,
    }
    Some(())
}

/// Listens on `session` until `done` says it has what it came for and `quiet`
/// more has passed, or for `limit` at most. What the client could not read
/// off the event queue meanwhile is among what was got.
async fn watch_for(
    session: &mut Session,
    limit: Duration,
    quiet: Duration,
    mut done: impl FnMut(&Got) -> bool,
) -> Result<Got, TestFailure> {
    let before = session.diagnostics().len();
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
            break;
        }
        match session
            .wait_for(remaining, |event| fold(&mut got, event))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    got.unread = session
        .diagnostics()
        .iter()
        .skip(before)
        .filter_map(|diagnostic| match diagnostic {
            Diagnostic::UnknownCapsEvent { message } => {
                Some(format!("unread event-queue message {message}"))
            }
            Diagnostic::CapsDecodeFailed { message, .. } => {
                Some(format!("event-queue message {message} that did not decode"))
            }
            _ => None,
        })
        .collect();
    Ok(got)
}

/// Listens on `session` for `window`.
async fn watch(session: &mut Session, window: Duration) -> Result<Got, TestFailure> {
    watch_for(session, window, window, |_got| false).await
}

/// Which of the three avatars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// A member, who owns the census group and starts the conference.
    A,
    /// A member.
    B,
    /// No member.
    C,
}

/// What a leg brought each avatar.
struct Brought {
    /// What A was sent.
    a: Got,
    /// What B was sent.
    b: Got,
    /// What C was sent.
    c: Got,
}

/// The seconds since the epoch now.
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// A place as a metric states it.
fn place_text(position: &RegionCoordinates) -> &'static str {
    if position.x() == 0.0 && position.y() == 0.0 && position.z() == 0.0 {
        "place zero"
    } else {
        "a place"
    }
}

/// A time as a metric states it: none, now, or another time.
fn time_text(timestamp: Option<u32>) -> &'static str {
    match timestamp {
        None => "no time",
        Some(stated) if u64::from(stated).abs_diff(now_secs()) <= TIME_SLACK_SECS => "now",
        Some(_) => "another time",
    }
}

/// Whether `value` is "yes" or "no", for a metric.
const fn yes(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// The ids and names a metric names by what they are rather than by their
/// value.
#[derive(Debug, Clone, Default)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// Avatar C.
    c: Uuid,
    /// Every way the grid writes A's name.
    a_names: Vec<String>,
    /// Every way the grid writes B's name.
    b_names: Vec<String>,
    /// Every way the grid writes C's name.
    c_names: Vec<String>,
    /// The census group.
    group: Uuid,
    /// The census group's name.
    group_name: String,
    /// What a census text of this run begins with.
    run_tag: String,
    /// Ids the census made or was given, by what a metric calls them.
    named: Vec<(Uuid, &'static str)>,
}

impl Scene {
    /// What `id` is the id of.
    fn id_text(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nil"
        } else if id == self.a {
            "A"
        } else if id == self.b {
            "B"
        } else if id == self.c {
            "C"
        } else if id == self.group {
            "the group"
        } else {
            self.named
                .iter()
                .rev()
                .find(|(named, _what)| *named == id)
                .map_or("another id", |(_named, what)| *what)
        }
    }

    /// Whose name `name` is.
    fn name_text(&self, name: &str) -> String {
        let is = |names: &[String]| names.iter().any(|known| name.eq_ignore_ascii_case(known));
        if name.is_empty() {
            "no name".to_owned()
        } else if is(&self.a_names) {
            "A's name".to_owned()
        } else if is(&self.b_names) {
            "B's name".to_owned()
        } else if is(&self.c_names) {
            "C's name".to_owned()
        } else if name == self.group_name {
            "the group's name".to_owned()
        } else {
            format!("the name {:?}", self.scrubbed(name))
        }
    }

    /// `text` with nothing in it that is one run's alone: the avatars' names,
    /// the group's name and every id.
    fn scrubbed(&self, text: &str) -> String {
        let mut out = text.to_owned();
        let mut names: Vec<(&str, &str)> = vec![(self.group_name.as_str(), "<the group's name>")];
        names.extend(self.a_names.iter().map(|name| (name.as_str(), "<A>")));
        names.extend(self.b_names.iter().map(|name| (name.as_str(), "<B>")));
        names.extend(self.c_names.iter().map(|name| (name.as_str(), "<C>")));
        // The longest first, so that a name inside another is not cut out of it.
        names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
        for (name, word) in names {
            if !name.is_empty() {
                out = out.replace(name, word);
            }
        }
        out = without_ids(&out);
        if out.len() > 200 {
            let cut = (0..=200)
                .rev()
                .find(|at| out.is_char_boundary(*at))
                .unwrap_or(0);
            out.truncate(cut);
            out.push('…');
        }
        out
    }

    /// A text as a metric states it: a line of this run by its number and
    /// word, a line of an earlier run as such, anything else scrubbed.
    fn text_text(&self, text: &str) -> String {
        if let Some(line) = text.strip_prefix(&self.run_tag) {
            format!("line {line}")
        } else if text.starts_with(TEXT_TAG) {
            "an earlier run's line".to_owned()
        } else if text.is_empty() {
            "no text".to_owned()
        } else {
            format!("{:?}", self.scrubbed(text))
        }
    }

    /// A bucket as a metric states it.
    fn bucket_text(&self, bucket: &[u8]) -> String {
        if bucket.is_empty() {
            return "an empty bucket".to_owned();
        }
        if bucket.iter().all(|byte| *byte == 0) {
            return format!("a bucket of {} zero bytes", bucket.len());
        }
        match std::str::from_utf8(bucket) {
            Ok(text) => format!(
                "a bucket of {} bytes reading {:?}",
                bucket.len(),
                self.scrubbed(text.trim_end_matches('\0'))
            ),
            Err(_) => format!("a bucket of {} bytes", bucket.len()),
        }
    }

    /// An invitation as a metric states it.
    fn invitation_text(&self, invitation: &Invitation) -> String {
        format!(
            "invitation {:?} to {}{}, from {}, {}, session name {}, {}, {}, estate {}, {}, {}, text {}",
            invitation.dialog,
            self.id_text(invitation.session_id),
            if invitation.from_group {
                " (a group's)"
            } else {
                ""
            },
            self.id_text(invitation.from.uuid()),
            self.name_text(&invitation.from_name),
            self.name_text(&invitation.session_name),
            if invitation.region_id.is_nil() {
                "no region"
            } else {
                "a region"
            },
            place_text(&invitation.position),
            invitation.estate,
            time_text(invitation.timestamp),
            self.bucket_text(&invitation.bucket),
            self.text_text(&invitation.message),
        )
    }

    /// A message as a metric states it; `group` is whether the client read
    /// it as a group's.
    fn message_text(&self, group: bool, im: &InstantMessage) -> String {
        format!(
            "{} message {:?} from {}{}, {}, to {}, id {}, {}, {}, estate {}, {}, {}, {}, text {}",
            if group { "group" } else { "conference" },
            im.dialog,
            self.id_text(im.from_agent_id.uuid()),
            if im.from_group { " (a group's)" } else { "" },
            self.name_text(&im.from_agent_name),
            self.id_text(im.to_agent_id.uuid()),
            self.id_text(im.id),
            if im.region_id.is_some() {
                "a region"
            } else {
                "no region"
            },
            place_text(&im.position),
            im.parent_estate_id,
            time_text(im.timestamp),
            if im.offline { "kept" } else { "not kept" },
            self.bucket_text(&im.binary_bucket),
            self.text_text(&im.message),
        )
    }

    /// Another instant message as a metric states it.
    fn im_text(&self, im: &InstantMessage) -> String {
        format!(
            "instant message {:?} from {}, {}, id {}, {}, text {}",
            im.dialog,
            self.id_text(im.from_agent_id.uuid()),
            self.name_text(&im.from_agent_name),
            self.id_text(im.id),
            if im.offline { "kept" } else { "not kept" },
            self.text_text(&im.message),
        )
    }

    /// A roster update as a metric states it.
    fn roster_text(&self, session: Uuid, updates: &[ChatSessionAgentUpdate]) -> String {
        let entries: Vec<String> = updates
            .iter()
            .map(|update| {
                let mut said = vec![self.id_text(update.agent.uuid()).to_owned()];
                said.push(
                    match update.transition {
                        None => "no transition",
                        Some(SessionTransition::Enter) => "ENTER",
                        Some(SessionTransition::Leave) => "LEAVE",
                        Some(SessionTransition::Unrecognised) => "another transition",
                    }
                    .to_owned(),
                );
                for (word, value) in [
                    ("voice", update.can_voice_chat),
                    ("moderator", update.is_moderator),
                    ("text muted", update.text_muted),
                    ("voice muted", update.voice_muted),
                ] {
                    if let Some(value) = value {
                        said.push(format!("{word} {}", yes(value)));
                    }
                }
                if update.legacy {
                    said.push("in the old map".to_owned());
                }
                said.join(" ")
            })
            .collect();
        format!(
            "roster of {}: {}",
            self.id_text(session),
            if entries.is_empty() {
                "nobody".to_owned()
            } else {
                entries.join("; ")
            }
        )
    }

    /// A start reply as a metric states it.
    fn started_text(&self, started: &Started) -> String {
        format!(
            "start of {} {}, {}, name {}, voice {}{}",
            self.id_text(started.temp),
            if started.real == started.temp {
                "kept its id".to_owned()
            } else {
                format!("became {}", self.id_text(started.real))
            },
            if started.success { "taken" } else { "refused" },
            self.name_text(&started.name),
            yes(started.voice),
            if started.error.is_empty() {
                String::new()
            } else {
                format!(", error {:?}", self.scrubbed(&started.error))
            },
        )
    }

    /// The lines of this run among `texts`, in order, as a metric states
    /// them, and whether an earlier run's are there too.
    fn lines_text<'t>(&self, texts: impl Iterator<Item = &'t str>) -> String {
        let mut ours = Vec::new();
        let mut earlier = false;
        let mut others = 0_u32;
        for text in texts {
            if let Some(line) = text.strip_prefix(&self.run_tag) {
                ours.push(line.to_owned());
            } else if text.starts_with(TEXT_TAG) {
                earlier = true;
            } else {
                others = others.saturating_add(1);
            }
        }
        format!(
            "this run's lines [{}], earlier runs' lines {}, other lines {}",
            ours.join(", "),
            yes(earlier),
            yes(others > 0),
        )
    }

    /// The census's sessions among `sessions`, as a metric states them.
    fn sessions_text(&self, sessions: &[ChatSessionInfo]) -> String {
        let mut ours: Vec<String> = sessions
            .iter()
            .filter_map(|info| {
                let (word, id) = match info.kind {
                    ChatSessionKind::Group { group_id } => ("group", group_id.uuid()),
                    ChatSessionKind::Conference { id } => ("conference", id.get()),
                    ChatSessionKind::Direct { .. } => return None,
                };
                let name = self.id_text(id);
                if name == "another id" {
                    return None;
                }
                let mut participants: Vec<&str> = info
                    .participants
                    .iter()
                    .map(|agent| self.id_text(agent.uuid()))
                    .collect();
                participants.sort_unstable();
                Some(format!(
                    "{word} session {name}: {}, participants [{}], voice {}",
                    match &info.lifecycle {
                        ChatLifecycleView::Joined => "joined".to_owned(),
                        ChatLifecycleView::Invited { inviter, .. } => {
                            format!("invited by {}", self.id_text(inviter.uuid()))
                        }
                    },
                    participants.join(", "),
                    yes(info.has_voice),
                ))
            })
            .collect();
        ours.sort_unstable();
        if ours.is_empty() {
            "no session of the census's".to_owned()
        } else {
            ours.join("; ")
        }
    }

    /// Everything `got` holds, as a metric states it.
    fn everything(&self, got: &Got) -> String {
        let mut all: Vec<String> = got
            .invitations
            .iter()
            .map(|invitation| self.invitation_text(invitation))
            .collect();
        all.extend(
            got.messages
                .iter()
                .map(|(group, im)| self.message_text(*group, im)),
        );
        all.extend(
            got.participants
                .iter()
                .map(|(group, session, agent, joined)| {
                    format!(
                        "{} participant notice of {}: {} {}",
                        if *group { "group" } else { "conference" },
                        self.id_text(*session),
                        self.id_text(agent.uuid()),
                        if *joined { "joined" } else { "left" },
                    )
                }),
        );
        all.extend(
            got.rosters
                .iter()
                .map(|(session, updates)| self.roster_text(*session, updates)),
        );
        all.extend(got.started.iter().map(|started| self.started_text(started)));
        all.extend(got.replies.iter().map(|(session, success, event, error)| {
            format!(
                "answer about {}: {:?} {}{}",
                self.id_text(*session),
                self.scrubbed(event),
                if *success { "taken" } else { "refused" },
                if error.is_empty() {
                    String::new()
                } else {
                    format!(", error {:?}", self.scrubbed(error))
                },
            )
        }));
        all.extend(got.closed.iter().map(|(session, reason)| {
            format!(
                "{} closed: {:?}",
                self.id_text(*session),
                self.scrubbed(reason)
            )
        }));
        all.extend(got.histories.iter().map(|(kind, messages)| {
            format!(
                "server history of {}: {}",
                self.id_text(kind_uuid(*kind)),
                self.lines_text(messages.iter().map(|message| message.text.as_str())),
            )
        }));
        all.extend(got.ims.iter().map(|im| self.im_text(im)));
        all.extend(
            got.alerts
                .iter()
                .map(|alert| format!("alert {:?}", self.scrubbed(alert))),
        );
        all.extend(got.unread.iter().cloned());
        if all.is_empty() {
            "nothing".to_owned()
        } else {
            all.join(" | ")
        }
    }

    /// An LLSD value as a metric states it: every id named, every text
    /// scrubbed, a map in key order.
    fn llsd_text(&self, value: &Llsd) -> String {
        match value {
            Llsd::Map(map) => {
                let mut entries: Vec<String> = map
                    .iter()
                    .map(|(key, value)| {
                        let key = Uuid::parse_str(key)
                            .map_or_else(|_| key.clone(), |id| self.id_text(id).to_owned());
                        format!("{key}: {}", self.llsd_text(value))
                    })
                    .collect();
                entries.sort_unstable();
                format!("{{{}}}", entries.join(", "))
            }
            Llsd::Array(items) => format!(
                "[{}]",
                items
                    .iter()
                    .map(|item| self.llsd_text(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Llsd::Undef => "undef".to_owned(),
            Llsd::Boolean(flag) => flag.to_string(),
            Llsd::Integer(number) => number.to_string(),
            Llsd::Real(_) => "a real".to_owned(),
            Llsd::String(text) => self.text_text(text),
            Llsd::Uuid(id) => self.id_text(*id).to_owned(),
            Llsd::Date(_) => "a date".to_owned(),
            Llsd::Uri(_) => "a uri".to_owned(),
            Llsd::Binary(bytes) => format!("{} bytes", bytes.len()),
        }
    }

    /// A capability's answer as a metric states it.
    fn answer_text(&self, answer: Option<&CapAnswer>) -> String {
        match answer {
            None => NO_CAPABILITY.to_owned(),
            Some(answer) => format!(
                "{}: {}",
                answer.status,
                answer
                    .body
                    .as_ref()
                    .map_or_else(|| "no LLSD".to_owned(), |body| self.llsd_text(body)),
            ),
        }
    }

    /// A long history the capability answered, as a metric states it: the
    /// status, how many of this run's lines it holds, the oldest and the
    /// newest of them, and whether anything else is there.
    fn history_span_text(&self, answer: Option<&CapAnswer>) -> String {
        let Some(answer) = answer else {
            return NO_CAPABILITY.to_owned();
        };
        let Some(Llsd::Array(records)) = &answer.body else {
            return format!("{}: not an array", answer.status);
        };
        let ours: Vec<&str> = records
            .iter()
            .filter_map(|record| record.get("message").and_then(Llsd::as_str))
            .filter_map(|text| text.strip_prefix(self.run_tag.as_str()))
            .collect();
        format!(
            "{}: {} lines, {} of this run's, oldest {}, newest {}",
            answer.status,
            records.len(),
            ours.len(),
            ours.first().copied().unwrap_or("-"),
            ours.last().copied().unwrap_or("-"),
        )
    }

    /// A history the capability answered, as a metric states it: the status,
    /// this run's lines, and the shape of a record.
    fn history_text(&self, answer: Option<&CapAnswer>) -> String {
        let Some(answer) = answer else {
            return NO_CAPABILITY.to_owned();
        };
        match &answer.body {
            Some(Llsd::Array(records)) => format!(
                "{}: an array; {}; a record {}",
                answer.status,
                self.lines_text(
                    records
                        .iter()
                        .filter_map(|record| record.get("message").and_then(Llsd::as_str))
                ),
                records.first().map_or_else(|| "-".to_owned(), shape_of),
            ),
            Some(other) => format!("{}: {}", answer.status, self.llsd_text(other)),
            None => format!("{}: no LLSD", answer.status),
        }
    }
}

/// The id a chat session kind names.
const fn kind_uuid(kind: ChatSessionKind) -> Uuid {
    match kind {
        ChatSessionKind::Group { group_id } => group_id.uuid(),
        ChatSessionKind::Conference { id } => id.get(),
        ChatSessionKind::Direct { peer } => peer.uuid(),
    }
}

/// How many records a history the capability answered holds.
fn history_lines(answer: Option<&CapAnswer>) -> i64 {
    match answer.and_then(|answer| answer.body.as_ref()) {
        Some(Llsd::Array(records)) => i64::try_from(records.len()).unwrap_or(i64::MAX),
        _ => -1,
    }
}

/// A `ChatSessionRequest` body: `method` for `session`, with `params` where
/// the method takes any.
fn request_body(method: &str, session: Uuid, params: Option<Llsd>) -> Llsd {
    let mut map = HashMap::from([
        ("method".to_owned(), Llsd::String(method.to_owned())),
        ("session-id".to_owned(), Llsd::Uuid(session)),
    ]);
    if let Some(params) = params {
        map.insert("params".to_owned(), params);
    }
    Llsd::Map(map)
}

/// The `mute update` parameters that take `agent`'s text away (`muted`) or
/// give it back.
fn mute_params(agent: Uuid, muted: bool) -> Llsd {
    Llsd::Map(HashMap::from([
        ("agent_id".to_owned(), Llsd::Uuid(agent)),
        (
            "mute_info".to_owned(),
            Llsd::Map(HashMap::from([("text".to_owned(), Llsd::Boolean(muted))])),
        ),
    ]))
}

/// The state of a census, and the three sessions it drives.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is one thing the clean up has to undo"
)]
struct Census<'a> {
    /// Avatar A's session.
    a: &'a mut Session,
    /// Avatar B's session.
    b: &'a mut Session,
    /// Avatar C's session.
    c: &'a mut Session,
    /// The ids and names the metrics name.
    scene: Scene,
    /// How many texts the census has made.
    said: u32,
    /// Whether B is logged in.
    b_is_here: bool,
    /// Whether C was a member of the census group and left it for the run.
    c_rejoins: bool,
    /// Whether B's text is taken away in the group session.
    b_is_muted: bool,
    /// Whether the conference is still open on any side.
    conference_open: bool,
    /// The conference, by the id the grid gave it.
    conference: Option<ImSessionId>,
    /// The HTTP client the capabilities are asked with.
    http: reqwest::Client,
}

impl Census<'_> {
    /// A text of the census for `word`.
    fn text(&mut self, word: &str) -> String {
        self.said = self.said.saturating_add(1);
        format!("{}{}-{word}", self.scene.run_tag, self.said)
    }

    /// The census group.
    fn group(&self) -> GroupKey {
        GroupKey::from(self.scene.group)
    }

    /// The session of `who`.
    const fn session(&mut self, who: Who) -> &mut Session {
        match who {
            Who::A => self.a,
            Who::B => self.b,
            Who::C => self.c,
        }
    }

    /// The conference, by the id the grid gave it.
    fn conference(&self) -> Result<ImSessionId, TestFailure> {
        self.conference
            .ok_or_else(|| TestFailure::State("the conference was not started".to_owned()))
    }

    /// Has `who` send `commands` and returns what the three avatars were then
    /// sent: the actor listens for [`LISTEN`], the others for a moment after.
    /// An avatar that is away is sent nothing.
    async fn act(&mut self, who: Who, commands: Vec<Command>) -> Result<Brought, TestFailure> {
        for command in commands {
            self.session(who).send(command).await?;
        }
        let acted = watch(self.session(who), LISTEN).await?;
        let mut heard = vec![(who, acted)];
        for other in [Who::A, Who::B, Who::C] {
            if other != who && (other != Who::B || self.b_is_here) {
                heard.push((other, watch(self.session(other), DRAIN).await?));
            }
        }
        let mut brought = Brought {
            a: Got::default(),
            b: Got::default(),
            c: Got::default(),
        };
        for (whose, got) in heard {
            match whose {
                Who::A => brought.a = got,
                Who::B => brought.b = got,
                Who::C => brought.c = got,
            }
        }
        Ok(brought)
    }

    /// Has `who` send `commands` and records under `key` what each avatar was
    /// then sent: `{key}_told_a`, `{key}_told_b` and `{key}_told_c`.
    async fn leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        commands: Vec<Command>,
    ) -> Result<Brought, TestFailure> {
        let brought = self.act(who, commands).await?;
        self.record(metrics, key, &brought);
        Ok(brought)
    }

    /// Records under `key` what each avatar was told.
    fn record(&self, metrics: &mut Metrics, key: &str, brought: &Brought) {
        metrics.set(&format!("{key}_told_a"), self.scene.everything(&brought.a));
        metrics.set(
            &format!("{key}_told_b"),
            if self.b_is_here {
                self.scene.everything(&brought.b)
            } else {
                AWAY.to_owned()
            },
        );
        metrics.set(&format!("{key}_told_c"), self.scene.everything(&brought.c));
    }

    /// Has `who` POST `body` to the `ChatSessionRequest` capability; `None`
    /// where the region grants none.
    async fn chat_request(
        &mut self,
        who: Who,
        body: Llsd,
    ) -> Result<Option<CapAnswer>, TestFailure> {
        let Some(url) = self.session(who).cap(CHAT_SESSION_REQUEST) else {
            return Ok(None);
        };
        Ok(Some(cap_post(&self.http, &url, body.to_llsd_xml()).await?))
    }

    /// Has `who` accept the invitation to `session` over the capability
    /// itself, recorded under `{key}_raw`, and then as the client does,
    /// recorded under `key`; then asks the client what it keeps of its
    /// sessions, recorded under `{key}_sessions`.
    async fn accept(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        session: Uuid,
        from_group: bool,
    ) -> Result<(), TestFailure> {
        let raw = self
            .chat_request(who, request_body("accept invitation", session, None))
            .await?;
        metrics.set(&format!("{key}_raw"), self.scene.answer_text(raw.as_ref()));
        self.leg(
            metrics,
            key,
            who,
            vec![Command::AcceptChatInvite {
                session_id: ImSessionId::from(session),
                from_group,
            }],
        )
        .await?;
        self.sessions(metrics, &format!("{key}_sessions"), who)
            .await
    }

    /// Records under `key` what the client of `who` keeps of the census's
    /// sessions.
    async fn sessions(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
    ) -> Result<(), TestFailure> {
        let session = self.session(who);
        session.send(Command::QueryChatSessions).await?;
        let got = watch_for(session, FETCH_WINDOW, Duration::ZERO, |got| {
            !got.sessions.is_empty()
        })
        .await?;
        metrics.set(
            key,
            got.sessions.last().map_or_else(
                || "no answer".to_owned(),
                |sessions| self.scene.sessions_text(sessions),
            ),
        );
        Ok(())
    }

    /// Has `who` read the history of `session` over the capability itself,
    /// recorded under `key` (its size under the `{key}_lines` count).
    async fn history(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        session: Uuid,
    ) -> Result<(), TestFailure> {
        let answer = self
            .chat_request(who, request_body("fetch history", session, None))
            .await?;
        metrics.set(key, self.scene.history_text(answer.as_ref()));
        metrics.set(
            &count_metric(&format!("{key}_lines")),
            history_lines(answer.as_ref()),
        );
        Ok(())
    }

    /// Whether `who` is a member of the census group.
    async fn is_member(&mut self, who: Who) -> Result<bool, TestFailure> {
        let group = self.scene.group;
        let session = self.session(who);
        session.send(Command::RequestAgentDataUpdate).await?;
        let got = watch_for(session, FETCH_WINDOW, Duration::from_secs(1), |got| {
            !got.memberships.is_empty()
        })
        .await?;
        Ok(got
            .memberships
            .last()
            .is_some_and(|groups| groups.iter().any(|entry| entry.group_id.uuid() == group)))
    }

    /// The census group, its name, and that B is a member of it and C is not.
    async fn the_census_group(
        &mut self,
        metrics: &mut Metrics,
        group: GroupKey,
    ) -> Result<(), TestFailure> {
        self.scene.group = group.uuid();
        self.a.send(Command::RequestGroupProfile(group)).await?;
        let a = &mut *self.a;
        let profile = a
            .wait_for(FETCH_WINDOW, |event| match event {
                Event::GroupProfileReceived(profile) if profile.group_id == group => {
                    Some(profile.name.clone())
                }
                _ => None,
            })
            .await?;
        self.scene.group_name = profile;
        if !self.is_member(Who::B).await? {
            self.b.send(Command::JoinGroup(group)).await?;
            let got = watch_for(self.b, FETCH_WINDOW, SETTLE, |got| !got.joined.is_empty()).await?;
            if !got
                .joined
                .iter()
                .any(|(joined, success)| *joined == group.uuid() && *success)
            {
                return Err(TestFailure::Assertion(
                    "B could not join the census group".to_owned(),
                ));
            }
        }
        if self.is_member(Who::C).await? {
            self.c.send(Command::LeaveGroup(group)).await?;
            watch(self.c, SETTLE).await?;
            self.c_rejoins = true;
        }
        metrics.set("c_was_a_member", self.c_rejoins);
        let granted: Vec<&str> = [CHAT_SESSION_REQUEST, READ_OFFLINE_MSGS]
            .into_iter()
            .filter(|name| self.a.cap(name).is_some())
            .collect();
        metrics.set(
            "session_capabilities_granted",
            if granted.is_empty() {
                "none".to_owned()
            } else {
                granted.join(", ")
            },
        );
        Ok(())
    }

    /// `who` speaks into the group session; recorded under `key`.
    async fn group_says(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
    ) -> Result<(), TestFailure> {
        let message = self.text(key);
        let group_id = self.group();
        self.leg(
            metrics,
            key,
            who,
            vec![Command::SendGroupMessage { group_id, message }],
        )
        .await
        .map(|_brought| ())
    }

    /// `who` speaks into the conference; recorded under `key`.
    async fn conference_says(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
    ) -> Result<(), TestFailure> {
        let message = self.text(key);
        let session_id = self.conference()?;
        self.leg(
            metrics,
            key,
            who,
            vec![Command::SendConferenceMessage {
                session_id,
                message,
            }],
        )
        .await
        .map(|_brought| ())
    }

    /// `who` speaks into the group session with a hand-built message whose
    /// bucket is `bucket` rather than the reference's one zero byte; recorded
    /// under `key`.
    async fn group_says_raw(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        bucket: Vec<u8>,
    ) -> Result<(), TestFailure> {
        let text = self.text(key);
        let (agent, names) = match who {
            Who::A => (self.scene.a, &self.scene.a_names),
            Who::B => (self.scene.b, &self.scene.b_names),
            Who::C => (self.scene.c, &self.scene.c_names),
        };
        let mut name = names.first().cloned().unwrap_or_default().into_bytes();
        name.push(0);
        let mut message = text.into_bytes();
        message.push(0);
        let group = self.scene.group;
        let session_id = self
            .session(who)
            .session_id()
            .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;
        let im = AnyMessage::ImprovedInstantMessage(ImprovedInstantMessage {
            agent_data: ImprovedInstantMessageAgentDataBlock {
                agent_id: agent,
                session_id,
            },
            message_block: ImprovedInstantMessageMessageBlockBlock {
                from_group: false,
                to_agent_id: group,
                parent_estate_id: 0,
                region_id: Uuid::nil(),
                position: Vector {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                offline: 0,
                dialog: ImDialog::SessionSend.to_u8(),
                id: group,
                timestamp: 0,
                from_agent_name: name,
                message,
                binary_bucket: bucket,
            },
            estate_block: ImprovedInstantMessageEstateBlockBlock { estate_id: 0 },
            meta_data: Vec::new(),
        });
        self.leg(
            metrics,
            key,
            who,
            vec![Command::Send {
                message: Box::new(im),
                reliability: Reliability::Reliable,
            }],
        )
        .await
        .map(|_brought| ())
    }

    /// The group session: A opens it and speaks, B accepts, B and A speak,
    /// C opens it and speaks.
    async fn group_session(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.group();
        self.leg(
            metrics,
            "g_start",
            Who::A,
            vec![Command::StartGroupSession(group)],
        )
        .await?;
        self.group_says(metrics, "g_first", Who::A).await?;
        self.accept(metrics, "g_accept", Who::B, group.uuid(), true)
            .await?;
        self.group_says(metrics, "g_b_says", Who::B).await?;
        self.group_says(metrics, "g_a_says", Who::A).await?;
        self.group_says_raw(metrics, "g_empty_bucket", Who::A, Vec::new())
            .await?;
        self.leg(
            metrics,
            "g_outsider_start",
            Who::C,
            vec![Command::StartGroupSession(group)],
        )
        .await?;
        self.group_says(metrics, "g_outsider_says", Who::C).await
    }

    /// Moderation: A takes B's text away, B speaks, A gives it back.
    async fn moderation(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.scene.group;
        let b = self.scene.b;
        let answer = self
            .chat_request(
                Who::A,
                request_body("mute update", group, Some(mute_params(b, true))),
            )
            .await?;
        self.b_is_muted = answer.is_some();
        metrics.set("g_mute_answer", self.scene.answer_text(answer.as_ref()));
        self.leg(metrics, "g_mute", Who::A, Vec::new()).await?;
        self.group_says(metrics, "g_muted_says", Who::B).await?;
        let answer = self
            .chat_request(
                Who::A,
                request_body("mute update", group, Some(mute_params(b, false))),
            )
            .await?;
        self.b_is_muted = false;
        metrics.set("g_unmute_answer", self.scene.answer_text(answer.as_ref()));
        self.leg(metrics, "g_unmute", Who::A, Vec::new()).await?;
        self.group_says(metrics, "g_unmuted_says", Who::B).await
    }

    /// B leaves the session and A speaks; B declines and A speaks; B joins
    /// again.
    async fn leaving(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.group();
        self.leg(
            metrics,
            "g_leave",
            Who::B,
            vec![Command::LeaveGroupSession(group)],
        )
        .await?;
        self.group_says(metrics, "g_after_leave", Who::A).await?;
        self.leg(
            metrics,
            "g_decline",
            Who::B,
            vec![Command::DeclineChatInvite {
                session_id: ImSessionId::from(group.uuid()),
                from_group: true,
            }],
        )
        .await?;
        self.group_says(metrics, "g_after_decline", Who::A).await?;
        self.leg(
            metrics,
            "g_rejoin",
            Who::B,
            vec![
                Command::StartGroupSession(group),
                Command::AcceptChatInvite {
                    session_id: ImSessionId::from(group.uuid()),
                    from_group: true,
                },
            ],
        )
        .await?;
        self.group_says(metrics, "g_after_rejoin", Who::A).await
    }

    /// The group session's history: read over the capability and by the
    /// client, then again after a burst.
    async fn group_history(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let group = self.scene.group;
        self.history(metrics, "g_history", Who::B, group).await?;
        let brought = self
            .act(
                Who::B,
                vec![Command::FetchSessionHistory {
                    kind: ChatSessionKind::Group {
                        group_id: self.group(),
                    },
                }],
            )
            .await?;
        metrics.set(
            "g_history_client",
            self.scene.everything(&Got {
                histories: brought.b.histories,
                ..Got::default()
            }),
        );
        tokio::time::sleep(HISTORY_PAUSE).await;
        self.history(metrics, "g_history_again", Who::B, group)
            .await?;
        for (step, lines) in BURSTS.iter().enumerate() {
            for _line in 0..*lines {
                let message = self.text("burst");
                self.a
                    .send(Command::SendGroupMessage {
                        group_id: self.group(),
                        message,
                    })
                    .await?;
                tokio::time::sleep(BURST_PAUSE).await;
            }
            let brought = self.act(Who::A, Vec::new()).await?;
            let key = format!("g_burst{}", step.saturating_add(1));
            metrics.set(
                &count_metric(&format!("{key}_heard_by_b")),
                i64::try_from(
                    brought
                        .b
                        .messages
                        .iter()
                        .filter(|(_, im)| im.message.ends_with("-burst"))
                        .count(),
                )
                .unwrap_or(i64::MAX),
            );
            tokio::time::sleep(HISTORY_PAUSE).await;
            let answer = self
                .chat_request(Who::B, request_body("fetch history", group, None))
                .await?;
            metrics.set(
                &format!("{key}_history"),
                self.scene.history_span_text(answer.as_ref()),
            );
        }
        Ok(())
    }

    /// B logs out and A speaks; B comes back, is left alone, asks for what
    /// was kept and reads the history.
    async fn away(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        self.b.disconnect().await?;
        self.b_is_here = false;
        tokio::time::sleep(AWAY_SETTLE).await;
        self.group_says(metrics, "g_while_away", Who::A).await?;
        self.b.relogin().await?;
        self.b_is_here = true;
        // Nothing waits for the region here: that wait would throw away what
        // the arrival brings.
        let unasked = watch(self.b, UNASKED_WINDOW).await?;
        metrics.set("g_away_unasked", self.scene.everything(&unasked));
        let waited = Instant::now();
        while self.b.cap(READ_OFFLINE_MSGS).is_none() && waited.elapsed() < CAP_WAIT {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let kept = match self.b.cap(READ_OFFLINE_MSGS) {
            Some(url) => {
                let answer = cap_get(&self.http, &url).await?;
                let records = kept_records(answer.body.as_ref());
                format!(
                    "{}: {}",
                    answer.status,
                    self.scene.lines_text(
                        records
                            .iter()
                            .filter_map(|record| record.get("message").and_then(Llsd::as_str))
                    )
                )
            }
            None => NO_CAPABILITY.to_owned(),
        };
        metrics.set("g_away_capability", kept);
        self.b.send(Command::RetrieveInstantMessages).await?;
        let got = watch_for(self.b, FETCH_WINDOW, Duration::from_secs(3), Got::any_kept).await?;
        metrics.set("g_away_retrieved", self.scene.everything(&got));
        let group = self.scene.group;
        self.history(metrics, "g_away_history", Who::B, group)
            .await?;
        self.leg(
            metrics,
            "g_away_rejoin",
            Who::B,
            vec![
                Command::StartGroupSession(self.group()),
                Command::AcceptChatInvite {
                    session_id: ImSessionId::from(group),
                    from_group: true,
                },
            ],
        )
        .await?;
        self.history(metrics, "g_away_history_joined", Who::B, group)
            .await
    }

    /// The conference.
    async fn conference_legs(&mut self, metrics: &mut Metrics) -> Result<(), TestFailure> {
        let temp = Uuid::new_v4();
        self.scene.named.push((temp, "A's id for the conference"));
        let opening = self.text("c_start");
        self.conference_open = true;
        let brought = self
            .leg(
                metrics,
                "c_start",
                Who::A,
                vec![Command::StartConference {
                    session_id: ImSessionId::from(temp),
                    invitees: vec![AgentKey::from(self.scene.b), AgentKey::from(self.scene.c)],
                    message: opening,
                }],
            )
            .await?;
        let real = brought
            .a
            .started
            .iter()
            .find(|started| started.temp == temp && started.success)
            .map_or(temp, |started| started.real);
        if real != temp {
            self.scene
                .named
                .push((real, "the grid's id for the conference"));
            // The start leg's metrics named the grid's id before it had a
            // name; state them again.
            self.record(metrics, "c_start", &brought);
        }
        self.conference = Some(ImSessionId::from(real));
        metrics.set("c_id", self.scene.id_text(real));
        metrics.set(
            "c_start_reply",
            brought
                .a
                .started
                .iter()
                .map(|started| self.scene.started_text(started))
                .reduce(|all, one| format!("{all} | {one}"))
                .unwrap_or_else(|| "no reply".to_owned()),
        );
        self.conference_says(metrics, "c_first", Who::A).await?;
        self.accept(metrics, "c_b_accept", Who::B, real, false)
            .await?;
        self.conference_says(metrics, "c_b_says", Who::B).await?;
        let session_id = self.conference()?;
        self.leg(
            metrics,
            "c_c_decline",
            Who::C,
            vec![Command::DeclineChatInvite {
                session_id,
                from_group: false,
            }],
        )
        .await?;
        self.conference_says(metrics, "c_after_decline", Who::A)
            .await?;
        self.leg(
            metrics,
            "c_reinvite",
            Who::A,
            vec![Command::InviteToChatSession {
                session_id,
                invitees: vec![AgentKey::from(self.scene.c)],
            }],
        )
        .await?;
        self.accept(metrics, "c_c_accept", Who::C, real, false)
            .await?;
        self.conference_says(metrics, "c_c_says", Who::C).await?;
        self.history(metrics, "c_history", Who::B, real).await?;
        self.leg(
            metrics,
            "c_c_leave",
            Who::C,
            vec![Command::LeaveConference { session_id }],
        )
        .await?;
        self.conference_says(metrics, "c_after_c_left", Who::B)
            .await?;
        self.leg(
            metrics,
            "c_a_leave",
            Who::A,
            vec![Command::LeaveConference { session_id }],
        )
        .await?;
        self.conference_says(metrics, "c_after_a_left", Who::B)
            .await?;
        self.leg(
            metrics,
            "c_b_leave",
            Who::B,
            vec![Command::LeaveConference { session_id }],
        )
        .await?;
        self.conference_open = false;
        Ok(())
    }

    /// Leaves what the census changed: B back and a member, its text given
    /// back, the sessions left, C a member again where it was one.
    async fn clean_up(&mut self) -> Result<(), TestFailure> {
        let group = self.group();
        if !self.b_is_here {
            self.b.relogin().await?;
            self.b_is_here = true;
        }
        if self.b_is_muted {
            let (group_id, b) = (self.scene.group, self.scene.b);
            self.chat_request(
                Who::A,
                request_body("mute update", group_id, Some(mute_params(b, false))),
            )
            .await?;
        }
        if self.conference_open
            && let Some(session_id) = self.conference
        {
            for who in [Who::A, Who::B, Who::C] {
                self.session(who)
                    .send(Command::LeaveConference { session_id })
                    .await?;
            }
        }
        for who in [Who::A, Who::B, Who::C] {
            self.session(who)
                .send(Command::LeaveGroupSession(group))
                .await?;
        }
        if !self.is_member(Who::B).await? {
            self.b.send(Command::JoinGroup(group)).await?;
            watch(self.b, SETTLE).await?;
        }
        if self.c_rejoins {
            self.c.send(Command::JoinGroup(group)).await?;
            watch(self.c, SETTLE).await?;
        }
        Ok(())
    }
}

/// Every way a grid writes the name of the avatar whose legacy name is
/// `legacy`: the legacy name, a Resident's first name, and the user name.
fn names_of(legacy: &str) -> Vec<String> {
    let mut names = vec![legacy.to_owned()];
    let (first, last) = legacy.split_once(' ').unwrap_or((legacy, ""));
    if last.is_empty() || last.eq_ignore_ascii_case("Resident") {
        names.push(first.to_owned());
        names.push(first.to_lowercase());
    } else {
        names.push(format!("{first}.{last}").to_lowercase());
    }
    names
}

/// A census of group chat and ad-hoc conferences.
#[derive(Debug)]
pub struct GroupChat;

impl GridTest for GroupChat {
    fn name(&self) -> &'static str {
        "group-chat"
    }

    fn description(&self) -> &'static str {
        "What starting, answering, speaking in, moderating and leaving a group session and an \
         ad-hoc conference bring three avatars, what is kept for an avatar that is away, and what \
         the server history holds"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        3
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1800)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// The census: the three avatars in, the census group settled, the legs, and
/// the clean up whatever became of them.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let group = ctx.census_group().ok_or_else(|| {
        TestFailure::State(
            "group-chat measures in the groups census's group: name census_group in the fixtures"
                .to_owned(),
        )
    })?;
    let Some((a, b, c)) = ctx.all_three() else {
        return Err(TestFailure::State(
            "group-chat needs three avatars".to_owned(),
        ));
    };
    let id_of = |session: &Session| {
        session
            .agent_id()
            .map(|agent| agent.uuid())
            .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))
    };
    let (a_id, b_id, c_id) = (id_of(a)?, id_of(b)?, id_of(c)?);
    for session in [&mut *a, &mut *b, &mut *c] {
        session.wait_for_region(REGION_TIMEOUT).await?;
    }
    let run_tag = format!(
        "{TEXT_TAG}{}-",
        Uuid::new_v4()
            .simple()
            .to_string()
            .get(..6)
            .unwrap_or("run")
    );
    let scene = Scene {
        a: a_id,
        b: b_id,
        c: c_id,
        a_names: names_of(&a.avatar_legacy_name()),
        b_names: names_of(&b.avatar_legacy_name()),
        c_names: names_of(&c.avatar_legacy_name()),
        run_tag,
        named: vec![
            (session_of(a_id, b_id), "A and B's one-to-one id"),
            (session_of(a_id, c_id), "A and C's one-to-one id"),
            (session_of(b_id, c_id), "B and C's one-to-one id"),
        ],
        ..Scene::default()
    };
    let mut census = Census {
        a,
        b,
        c,
        scene,
        said: 0,
        b_is_here: true,
        c_rejoins: false,
        b_is_muted: false,
        conference_open: false,
        conference: None,
        http: http_client()?,
    };
    let outcome = legs(&mut census, metrics, group).await;
    let cleaned = census.clean_up().await;
    outcome.and(cleaned)
}

/// Every leg, in order.
async fn legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    group: GroupKey,
) -> Result<(), TestFailure> {
    for who in [Who::A, Who::B, Who::C] {
        watch(census.session(who), DRAIN).await?;
    }
    census.the_census_group(metrics, group).await?;
    tokio::time::sleep(SETTLE).await;
    census.group_session(metrics).await?;
    census.moderation(metrics).await?;
    census.leaving(metrics).await?;
    census.group_history(metrics).await?;
    census.away(metrics).await?;
    census.conference_legs(metrics).await
}

#[cfg(test)]
mod tests {
    use super::{Scene, TEXT_TAG, place_text, request_body};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{RegionCoordinates, Uuid};
    use sl_wire::Llsd;

    /// A scene whose run writes `gc-abc-`.
    fn scene() -> Scene {
        Scene {
            a: Uuid::from_u128(1),
            b: Uuid::from_u128(2),
            group: Uuid::from_u128(3),
            group_name: "Census Group".to_owned(),
            a_names: vec!["Ann Tester".to_owned()],
            run_tag: format!("{TEXT_TAG}abc-"),
            ..Scene::default()
        }
    }

    #[test]
    fn a_line_is_named_by_whose_run_it_is() {
        let scene = scene();
        assert_eq!(scene.text_text("gc-abc-4-g_first"), "line 4-g_first");
        assert_eq!(scene.text_text("gc-xyz-4-g_first"), "an earlier run's line");
        assert_eq!(
            scene.text_text("Ann Tester joined Census Group"),
            "\"<A> joined <the group's name>\""
        );
        assert_eq!(
            scene.lines_text(["gc-abc-1-a", "gc-old-1-a", "gc-abc-2-b"].into_iter()),
            "this run's lines [1-a, 2-b], earlier runs' lines yes, other lines no"
        );
    }

    #[test]
    fn an_answer_names_its_ids() {
        let scene = scene();
        let body = Llsd::Map(
            [(
                "agent_info".to_owned(),
                Llsd::Map(
                    [(
                        Uuid::from_u128(2).to_string(),
                        Llsd::Map(
                            [("is_moderator".to_owned(), Llsd::Boolean(false))]
                                .into_iter()
                                .collect(),
                        ),
                    )]
                    .into_iter()
                    .collect(),
                ),
            )]
            .into_iter()
            .collect(),
        );
        assert_eq!(
            scene.llsd_text(&body),
            "{agent_info: {B: {is_moderator: false}}}"
        );
        assert_eq!(
            scene.llsd_text(&request_body("fetch history", Uuid::from_u128(3), None)),
            "{method: \"fetch history\", session-id: the group}"
        );
    }

    #[test]
    fn a_place_is_zero_or_somewhere() {
        assert_eq!(
            place_text(&RegionCoordinates::new(0.0, 0.0, 0.0)),
            "place zero"
        );
        assert_eq!(
            place_text(&RegionCoordinates::new(1.0, 0.0, 0.0)),
            "a place"
        );
    }
}
