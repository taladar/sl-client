//! A census of the estate: what a grid answers when the Region/Estate floater
//! reads the estate, and what each avatar is told when an estate manager acts
//! on another resident — access lists, messages, the covenant, freezing,
//! ejecting, sending home, kicking and restarting the region.
//!
//! Avatar **A** is the estate's owner where the test avatars hold estate
//! rights; avatar **B** is another resident. On OpenSim A is the estate owner
//! (`--avatar estate-owner --secondary resident`) and both stand in the
//! Default Region. On Second Life neither test avatar holds rights on any
//! estate (`gridspec-aditi-test-land`), so both stand at the fixtures' build
//! location and only what a resident without rights sees is measured.
//!
//! One question a leg:
//!
//! 1. **Reads**, by both avatars: the estate's configuration (`getinfo`), the
//!    covenant summary (`EstateCovenantRequest`) and the four access lists over
//!    the `EstateAccess` capability, recorded raw.
//! 2. **Refusals**: B, without rights, kicks A, sends A home, messages the
//!    region and the estate, restarts the region, edits the access lists and
//!    the covenant, and freezes and ejects A.
//! 3. **Messages** (A's rights only): a region message and an estate message.
//! 4. **Access** (A's rights only): B allowed and removed, then banned while in
//!    the region and unbanned; the capability read after each.
//! 5. **The covenant** (A's rights only): set to an id, read back, cleared.
//! 6. **Freeze and eject** (A's rights only): B frozen, unfrozen, frozen until
//!    it wears off, ejected, and ejected with a parcel ban.
//! 7. **Home and kick** (A's rights only): B sent home, everyone sent home, B
//!    kicked.
//! 8. **Restart** (A's rights only, last): a cancel with nothing pending, then a
//!    restart in 30 s, heard out by both avatars until the region goes down.
//!
//! Every leg records what A and B were told (`*_told_a`, `*_told_b`). Every id
//! is described by whose it is and every name is scrubbed, so no metric names
//! an avatar.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/estate.md`; every run is held to it.
//!
//! `2av`, live grids.

use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AnyMessage, AssetKey, Command, EjectAction, EstateAccessDelta, EstateAccessKind,
    EstateCovenant, EstateInfo, Event, FreezeAction, InstantMessage, OwnerKey, ParcelAccessScope,
    RegionCoordinates, RegionHandle, Reliability, Uuid, Vector,
};
use sl_wire::Llsd;
use sl_wire::messages::{
    EstateOwnerMessage, EstateOwnerMessageAgentDataBlock, EstateOwnerMessageMethodDataBlock,
    EstateOwnerMessageParamListBlock,
};

use crate::cases::asset_http::without_ids;
use crate::cases::groups::names_of;
use crate::cases::land_transactions::{read_parcel, region_named, teleport};
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::{OPENSIM_START, hold_to_measured};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim};

/// How long both avatars are listened to after a leg's request.
const WINDOW: Duration = Duration::from_secs(8);

/// How long a freeze is listened to for its wearing off: OpenSim's lasts 30 s.
const FREEZE_WINDOW: Duration = Duration::from_secs(40);

/// How long a restart is listened to: its notices, and the region going down.
const RESTART_WINDOW: Duration = Duration::from_secs(75);

/// How many seconds ahead the census schedules its restart.
const RESTART_SECONDS: i32 = 30;

/// How long a teleport back to the spot may take.
const TELEPORT: Duration = Duration::from_secs(60);

/// Where both avatars stand on OpenSim: the Default Region's centre.
const OPENSIM_SPOT: Vector = Vector {
    x: 128.0,
    y: 128.0,
    z: 25.0,
};

/// The covenant id the census sets: an id no asset has, so nothing is fetched.
const COVENANT: Uuid = Uuid::from_u128(0x6573_7461_7465_636f_7665_6e61_6e74_0001);

/// The text of the census's messages.
const MESSAGE: &str = "estate census message";

/// The first sequence id of the census's own parcel reads.
const SEQUENCE_BASE: i32 = 7300;

/// The legs only an avatar with estate rights can run, in order: what each
/// grid without them records instead.
const OWNER_LEGS: [&str; 23] = [
    "b_set_home",
    "region_message",
    "estate_message",
    "allow_add",
    "allow_remove",
    "ban_add",
    "ban_remove",
    "covenant_set",
    "covenant_reset",
    "freeze",
    "unfreeze",
    "freeze_expiry",
    "eject",
    "eject_ban",
    "eject_ban_clear",
    "home",
    "home_all",
    "kick",
    "restart_cancel_idle",
    "restart",
    "manager_add",
    "manager_remove",
    "unknown_method",
];

/// The capability reads only an avatar with estate rights takes, keyed as
/// they are recorded.
const OWNER_READS: [&str; 12] = [
    "access_cap_after_allow",
    "access_cap_after_ban",
    "access_cap_after_manager",
    "access_cap_b_as_manager",
    "covenant_after",
    "info_b_as_manager",
    "ban_b_was",
    "eject_b_was",
    "eject_ban_b_was",
    "home_b_was",
    "home_all_b_was",
    "kick_b_was",
];

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/estate.md (estate, 2026-10-17)";

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
/// Not held: the estate's id.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "a_holds_rights",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    (
        "access_cap_a",
        each(
            Answer::Text(
                "HTTP 200, {AllowedAgents: undef, AllowedGroups: undef, BannedAgents: undef, Managers: undef}",
            ),
            Answer::Text(
                "HTTP 200, {AllowedAgents: [], AllowedGroups: [], BannedAgents: [], Managers: []}",
            ),
        ),
    ),
    (
        "access_cap_after_allow",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "HTTP 200, {AllowedAgents: [{id: B}], AllowedGroups: [], BannedAgents: [], Managers: []}",
            ),
        ),
    ),
    (
        "access_cap_after_ban",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "HTTP 200, {AllowedAgents: [], AllowedGroups: [], BannedAgents: [{ban_date: \"9999-99-99 99:99\", banning_id: A, id: B, last_login_date: \"na\"}], Managers: []}",
            ),
        ),
    ),
    (
        "access_cap_after_manager",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "HTTP 200, {AllowedAgents: [], AllowedGroups: [], BannedAgents: [], Managers: [{agent_id: B}]}",
            ),
        ),
    ),
    (
        "access_cap_b",
        each(
            Answer::Text(
                "HTTP 200, {AllowedAgents: undef, AllowedGroups: undef, BannedAgents: undef, Managers: undef}",
            ),
            Answer::Text("HTTP 401, not LLSD \"\""),
        ),
    ),
    (
        "access_cap_b_as_manager",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "HTTP 200, {AllowedAgents: [], AllowedGroups: [], BannedAgents: [], Managers: [{agent_id: B}]}",
            ),
        ),
    ),
    (
        "allow_add_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("setaccess AllowedAgents [B]; setaccess BannedAgents []"),
        ),
    ),
    (
        "allow_add_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "allow_remove_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("setaccess AllowedAgents []; setaccess BannedAgents []"),
        ),
    ),
    (
        "allow_remove_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "b_set_home_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "b_set_home_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("agent alert \"Home position set.\""),
        ),
    ),
    (
        "ban_add_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("setaccess AllowedAgents []; setaccess BannedAgents [B]"),
        ),
    ),
    (
        "ban_add_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("teleport start 0x22; teleport local to 128,128"),
        ),
    ),
    (
        "ban_b_was",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("in this region"),
        ),
    ),
    (
        "ban_remove_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("setaccess AllowedAgents []; setaccess BannedAgents []"),
        ),
    ),
    (
        "ban_remove_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "covenant_a",
        each(
            Answer::Text("covenant: none, estate \"mainland\", owner nobody"),
            Answer::Text("covenant: none, estate the estate, owner A"),
        ),
    ),
    (
        "covenant_after",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("covenant: the census's covenant (time set), estate the estate, owner A"),
        ),
    ),
    (
        "covenant_b",
        each(
            Answer::Text("covenant: none, estate \"mainland\", owner nobody"),
            Answer::Text("covenant: none, estate the estate, owner A"),
        ),
    ),
    (
        "covenant_reset_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "covenant_reset_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "covenant_set_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "covenant_set_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "eject_b_was",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("in this region"),
        ),
    ),
    (
        "eject_ban_b_was",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("in this region"),
        ),
    ),
    (
        "eject_ban_clear_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "eject_ban_clear_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "eject_ban_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("alert \"Avatar Ejected.\""),
        ),
    ),
    (
        "eject_ban_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("alert \"You have been ejected by <A>\""),
        ),
    ),
    (
        "eject_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("alert \"Avatar Ejected.\""),
        ),
    ),
    (
        "eject_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("alert \"You have been ejected by <A>\""),
        ),
    ),
    (
        "estate_message_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("agent alert \"/estate census message\""),
        ),
    ),
    (
        "estate_message_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("agent alert \"/estate census message\""),
        ),
    ),
    (
        "freeze_expiry_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("alert \"Avatar Frozen.\""),
        ),
    ),
    (
        "freeze_expiry_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "alert \"<A> has frozen you for 30 seconds.  You cannot move or interact with the world.\"; agent alert \"/The freeze has worn off; you may go about your business.\"",
            ),
        ),
    ),
    (
        "freeze_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("alert \"Avatar Frozen.\""),
        ),
    ),
    (
        "freeze_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "alert \"<A> has frozen you for 30 seconds.  You cannot move or interact with the world.\"",
            ),
        ),
    ),
    (
        "home_all_b_was",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("in this region"),
        ),
    ),
    (
        "home_all_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "teleport start 0x10; teleport start 0x22; teleport local to 128,128; teleport start 0x22; teleport local to 128,128; teleport start 0x10; teleport start 0x22; teleport local to 128,128; teleport start 0x22; teleport local to 128,128; teleport start 0x10; teleport start 0x22; teleport local to 128,128; teleport start 0x22; teleport local to 128,128; teleport start 0x10; teleport start 0x22; teleport local to 128,128; teleport start 0x22; teleport local to 128,128",
            ),
        ),
    ),
    (
        "home_all_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "teleport start 0x10; teleport start 0x22; teleport local to 128,128; teleport start 0x22; teleport local to 128,128; teleport start 0x10; teleport start 0x22; teleport local to 128,128; teleport start 0x22; teleport local to 128,128; teleport start 0x10; teleport start 0x22; teleport local to 128,128; teleport start 0x22; teleport local to 128,128; teleport start 0x10; teleport start 0x22; teleport local to 128,128; teleport start 0x22; teleport local to 128,128",
            ),
        ),
    ),
    (
        "home_b_was",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("in this region"),
        ),
    ),
    (
        "home_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "home_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("teleport start 0x10; teleport start 0x22; teleport local to 128,128"),
        ),
    ),
    (
        "info_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "info: owner A, flags 0x14128026, sun 0, parent set, covenant nobody, abuse email none; setaccess Managers []; setaccess AllowedAgents []; setaccess AllowedGroups []; setaccess BannedAgents []",
            ),
        ),
    ),
    ("info_b", both(Answer::Text("nothing"))),
    (
        "info_b_as_manager",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "info: owner A, flags 0x14128026, sun 0, parent set, covenant nobody, abuse email none; setaccess Managers [B]; setaccess AllowedAgents []; setaccess AllowedGroups []; setaccess BannedAgents []",
            ),
        ),
    ),
    (
        "kick_b_was",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("logged out"),
        ),
    ),
    (
        "kick_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "kick_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "kicked B \"You have been kicked out\"; disconnected Kicked { message: \"You have been kicked out\" }",
            ),
        ),
    ),
    (
        "manager_add_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("setaccess Managers [B]"),
        ),
    ),
    (
        "manager_add_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "manager_remove_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("setaccess Managers []"),
        ),
    ),
    (
        "manager_remove_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    ("refuse_access_told_a", both(Answer::Text("nothing"))),
    ("refuse_access_told_b", both(Answer::Text("nothing"))),
    ("refuse_covenant_told_a", both(Answer::Text("nothing"))),
    ("refuse_covenant_told_b", both(Answer::Text("nothing"))),
    ("refuse_eject_told_a", both(Answer::Text("nothing"))),
    (
        "refuse_eject_told_b",
        each(
            Answer::Text(
                "alert \"Eject failed because you don't have admin permission for that parcel.\" keyed AvatarEjectFailed []",
            ),
            Answer::Text("nothing"),
        ),
    ),
    (
        "refuse_estate_message_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "refuse_estate_message_told_b",
        both(Answer::Text("nothing")),
    ),
    ("refuse_freeze_told_a", both(Answer::Text("nothing"))),
    (
        "refuse_freeze_told_b",
        each(
            Answer::Text(
                "alert \"Freeze failed because you don't have admin permission for that parcel.\" keyed AvatarFreezeFailure []",
            ),
            Answer::Text("nothing"),
        ),
    ),
    ("refuse_home_told_a", both(Answer::Text("nothing"))),
    ("refuse_home_told_b", both(Answer::Text("nothing"))),
    ("refuse_kick_told_a", both(Answer::Text("nothing"))),
    ("refuse_kick_told_b", both(Answer::Text("nothing"))),
    (
        "refuse_region_message_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "refuse_region_message_told_b",
        both(Answer::Text("nothing")),
    ),
    ("refuse_restart_told_a", both(Answer::Text("nothing"))),
    ("refuse_restart_told_b", both(Answer::Text("nothing"))),
    ("refuse_telehub_told_a", both(Answer::Text("nothing"))),
    ("refuse_telehub_told_b", both(Answer::Text("nothing"))),
    (
        "region_message_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("agent alert \"/estate census message\""),
        ),
    ),
    (
        "region_message_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("agent alert \"/estate census message\""),
        ),
    ),
    (
        "restart_cancel_idle_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "restart_cancel_idle_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "restart_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "agent alert \"/Region will restart in 30 seconds\"; agent alert \"/Region will restart in 15 seconds\"; kicked A \"The simulator is going down.\"; disconnected Kicked { message: \"The simulator is going down.\" }",
            ),
        ),
    ),
    (
        "restart_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text(
                "agent alert \"/Region will restart in 30 seconds\"; agent alert \"/Region will restart in 15 seconds\"; kicked B \"The simulator is going down.\"; disconnected Kicked { message: \"The simulator is going down.\" }",
            ),
        ),
    ),
    (
        "unfreeze_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("alert \"Avatar Unfrozen.\""),
        ),
    ),
    (
        "unfreeze_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("alert \"<A> has unfrozen you.\""),
        ),
    ),
    (
        "unknown_method_told_a",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "unknown_method_told_b",
        each(
            Answer::Text("not measured: no estate rights on aditi (gridspec-aditi-test-land)"),
            Answer::Text("nothing"),
        ),
    ),
];

/// Estate reads, refusals and actions: what each avatar is told.
#[derive(Debug)]
pub struct Estate;

impl GridTest for Estate {
    fn name(&self) -> &'static str {
        "estate"
    }

    fn description(&self) -> &'static str {
        "Estate reads, access lists, messages, the covenant, freeze, eject, home, kick and \
         restart: refusals and what each avatar is told"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi]
    }

    fn accounts(&self) -> u8 {
        2
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

/// Which avatar acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The avatar with estate rights, where there is one.
    A,
    /// The other resident.
    B,
}

/// Who is who, for describing ids and scrubbing names.
#[derive(Debug, Clone, Default)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// The region the census stands in.
    region: Option<RegionHandle>,
    /// Every way a grid may write A's or B's name, with the word that
    /// replaces it.
    names: Vec<(String, &'static str)>,
}

impl Scene {
    /// Whose `id` is, in a word.
    fn whose(&self, id: Uuid) -> &'static str {
        if id.is_nil() {
            "nobody"
        } else if id == self.a {
            "A"
        } else if id == self.b {
            "B"
        } else if id == COVENANT {
            "the census's covenant"
        } else {
            "another"
        }
    }

    /// `text` with every id and every name of A's and B's replaced.
    fn scrub(&self, text: &str) -> String {
        let mut out = without_ids(text);
        for (name, word) in &self.names {
            if !name.is_empty() {
                out = out.replace(name.as_str(), word);
            }
        }
        out
    }
}

/// What one session was sent during a window, in words, in arrival order.
#[derive(Debug, Default)]
struct Got {
    /// One line per message the census reads.
    lines: Vec<String>,
    /// Whether the session was disconnected.
    disconnected: bool,
    /// The access lists among them, kind and members.
    lists: Vec<(EstateAccessKind, Vec<Uuid>)>,
    /// The covenant summaries among them.
    covenants: Vec<EstateCovenant>,
}

/// An estate configuration in words; the estate's id and name are left out
/// (`estate_id` is recorded on its own).
fn describe_info(scene: &Scene, info: &EstateInfo) -> String {
    format!(
        "info: owner {}, flags {:#x}, sun {}, parent {}, covenant {}, abuse email {}",
        scene.whose(info.estate_owner),
        info.estate_flags,
        info.sun_position,
        if info.parent_estate == 0 { "0" } else { "set" },
        info.covenant_id.map_or("none", |id| scene.whose(id)),
        if info.abuse_email.is_empty() {
            "none"
        } else {
            "set"
        }
    )
}

/// A covenant summary in words; its time only where `timed` — whether a
/// covenant was ever set is the grid's history, not its behaviour, except
/// right after the census set one.
fn describe_covenant(
    scene: &Scene,
    covenant: &EstateCovenant,
    estate_name: &str,
    timed: bool,
) -> String {
    let time = if !timed {
        ""
    } else if covenant.covenant_timestamp == 0 {
        " (time 0)"
    } else {
        " (time set)"
    };
    format!(
        "covenant: {}{time}, estate {}, owner {}",
        covenant.covenant_id.map_or("none", |id| scene.whose(id)),
        if covenant.estate_name.is_empty() {
            "unnamed".to_owned()
        } else if covenant.estate_name == estate_name {
            "the estate".to_owned()
        } else {
            format!("\"{}\"", scene.scrub(&covenant.estate_name))
        },
        scene.whose(covenant.estate_owner_id)
    )
}

/// An instant message in words.
fn describe_im(scene: &Scene, im: &InstantMessage) -> String {
    format!(
        "im {:?}{} from {} as \"{}\" \"{}\"",
        im.dialog,
        if im.from_group { " (group)" } else { "" },
        scene.whose(im.from_agent_id.uuid()),
        scene.scrub(&im.from_agent_name),
        scene.scrub(&im.message)
    )
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(scene: &Scene, got: &mut Got, event: &Event) -> Option<()> {
    let line = match event {
        Event::AlertMessage {
            message,
            alert_info,
            ..
        } => {
            let keys = alert_info
                .iter()
                .map(|info| format!("{} [{}]", info.message, scene.scrub(&info.extra_params)))
                .collect::<Vec<_>>()
                .join(", ");
            if keys.is_empty() {
                format!("alert \"{}\"", scene.scrub(message))
            } else {
                format!("alert \"{}\" keyed {keys}", scene.scrub(message))
            }
        }
        Event::AgentAlertMessage { message, modal, .. } => format!(
            "{} \"{}\"",
            if *modal {
                "modal agent alert"
            } else {
                "agent alert"
            },
            scene.scrub(message)
        ),
        Event::InstantMessageReceived(im) => describe_im(scene, im),
        Event::ChatReceived(chat) => format!(
            "chat {:?} {:?} from \"{}\" \"{}\"",
            chat.source,
            chat.chat_type,
            scene.scrub(&chat.from_name),
            scene.scrub(&chat.message)
        ),
        Event::EstateInfo(info) => describe_info(scene, info),
        Event::EstateAccessList { kind, members, .. } => {
            got.lists.push((*kind, members.clone()));
            format!(
                "setaccess {kind:?} [{}]",
                members
                    .iter()
                    .map(|id| scene.whose(*id))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        Event::EstateCovenant(covenant) => {
            got.covenants.push(covenant.clone());
            "covenant reply".to_owned()
        }
        Event::Kicked(kick) => format!(
            "kicked {} \"{}\"",
            scene.whose(kick.agent.uuid()),
            scene.scrub(&kick.reason)
        ),
        Event::Disconnected(reason) => {
            got.disconnected = true;
            format!("disconnected {}", scene.scrub(&format!("{reason:?}")))
        }
        Event::TeleportStarted { flags } => format!("teleport start {:#x}", flags.0),
        Event::TeleportProgress { message, .. } => {
            format!("teleport progress \"{}\"", scene.scrub(message))
        }
        Event::TeleportLocal { position, .. } => {
            format!("teleport local to {:.0},{:.0}", position.x(), position.y())
        }
        Event::TeleportFailed { reason, .. } => {
            format!("teleport failed \"{}\"", scene.scrub(reason))
        }
        Event::TeleportFinished {
            region_handle,
            flags,
            ..
        } => format!(
            "teleport finish {:#x} to {}",
            flags.0,
            if Some(*region_handle) == scene.region {
                "this region"
            } else {
                "another region"
            }
        ),
        Event::RegionChanged { region_handle, .. } => format!(
            "region changed to {}",
            if Some(*region_handle) == scene.region {
                "this region"
            } else {
                "another region"
            }
        ),
        Event::LoggedOut => "logged out".to_owned(),
        _ => return None,
    };
    got.lines.push(line);
    Some(())
}

/// Listens on `session` for `window`, or until it is disconnected.
async fn watch(scene: &Scene, session: &mut Session, window: Duration) -> Result<Got, TestFailure> {
    let mut got = Got::default();
    if !session.is_connected() {
        got.lines.push("not logged in".to_owned());
        return Ok(got);
    }
    let started = Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
        if remaining.is_zero() || got.disconnected {
            return Ok(got);
        }
        match session
            .wait_for(remaining, |event| fold(scene, &mut got, event))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(got),
            Err(TestFailure::Disconnected(reason)) => {
                got.disconnected = true;
                got.lines
                    .push(format!("channel closed {}", scene.scrub(&reason)));
                return Ok(got);
            }
            Err(other) => return Err(other),
        }
    }
}

/// What `got` holds, in words; `nothing` for an empty window.
fn describe_got(got: &Got) -> String {
    if got.lines.is_empty() {
        "nothing".to_owned()
    } else {
        got.lines.join("; ")
    }
}

/// The `EstateAccess` capability's answer to `session`, in words: the status,
/// the key set and each list's members by whose they are, and for a ban the
/// fields it carries.
async fn access_cap(scene: &Scene, session: &Session) -> String {
    let Some(url) = session.cap("EstateAccess") else {
        return "no capability".to_owned();
    };
    let answer = reqwest::Client::new().get(url).send().await;
    let response = match answer {
        Ok(response) => response,
        Err(error) => return format!("unreachable ({})", scene.scrub(&error.to_string())),
    };
    let status = response.status().as_u16();
    let body = response.text().await.unwrap_or_default();
    if !body.contains("<llsd") {
        return format!(
            "HTTP {status}, not LLSD \"{}\"",
            scene.scrub(&body.chars().take(40).collect::<String>())
        );
    }
    let Ok(llsd) = sl_wire::parse_llsd_xml(&body) else {
        return format!("HTTP {status}, unparsable LLSD");
    };
    format!("HTTP {status}, {}", describe_llsd(scene, &llsd))
}

/// An LLSD answer in words: a map's keys in order, an array's elements, ids
/// by whose they are, dates by their form.
fn describe_llsd(scene: &Scene, value: &Llsd) -> String {
    match value {
        Llsd::Map(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .filter_map(|key| {
                    map.get(key)
                        .map(|inner| format!("{key}: {}", describe_llsd(scene, inner)))
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        Llsd::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|item| describe_llsd(scene, item))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Llsd::Uuid(id) => scene.whose(*id).to_owned(),
        Llsd::String(text) => describe_text(scene, text),
        Llsd::Date(text) => format!("date {}", describe_text(scene, text)),
        Llsd::Undef => "undef".to_owned(),
        Llsd::Boolean(flag) => flag.to_string(),
        Llsd::Integer(number) => format!("int {number}"),
        other => scene.scrub(&format!("{other:?}")),
    }
}

/// A string field in words: an id by whose it is, a date by its form, other
/// text scrubbed.
fn describe_text(scene: &Scene, text: &str) -> String {
    if let Ok(id) = Uuid::parse_str(text) {
        return format!("\"{}\"", scene.whose(id));
    }
    let shape: String = text
        .chars()
        .map(|character| {
            if character.is_ascii_digit() {
                '9'
            } else {
                character
            }
        })
        .collect();
    let zeros = text
        .chars()
        .all(|character| !character.is_ascii_digit() || character == '0');
    if shape != text {
        return format!("\"{shape}\"{}", if zeros { " (zero)" } else { "" });
    }
    format!("\"{}\"", scene.scrub(text))
}

/// The census's state: the two sessions and who is who.
struct Census<'s> {
    /// Avatar A.
    a: &'s mut Session,
    /// Avatar B.
    b: &'s mut Session,
    /// Who is who.
    scene: Scene,
    /// Where both stand.
    spot: Vector,
    /// The estate's name, to compare a covenant summary's against.
    estate_name: String,
    /// The next sequence id of a parcel read.
    sequence: i32,
    /// Whether B's session ended in the last leg (a kick, a failed send
    /// home): its handle still says connected until it is logged back in.
    b_gone: bool,
}

impl Census<'_> {
    /// The session of `who`.
    const fn session(&mut self, who: Who) -> &mut Session {
        match who {
            Who::A => self.a,
            Who::B => self.b,
        }
    }

    /// The agent id of the other avatar than `who`.
    fn other(&self, who: Who) -> AgentKey {
        AgentKey::from(match who {
            Who::A => self.scene.b,
            Who::B => self.scene.a,
        })
    }

    /// Lets both avatars hear out what the grid still had to say.
    async fn settle(&mut self) -> Result<(), TestFailure> {
        let scene = self.scene.clone();
        let (got_a, got_b) =
            tokio::join!(watch(&scene, self.a, WINDOW), watch(&scene, self.b, WINDOW));
        let _heard: (Got, Got) = (got_a?, got_b?);
        Ok(())
    }

    /// Sends `commands` as `who`, listens to both avatars for `window` and
    /// records what each was told under `key`.
    async fn leg_for(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        commands: Vec<Command>,
        window: Duration,
    ) -> Result<(Got, Got), TestFailure> {
        for command in commands {
            self.session(who).send(command).await?;
        }
        let scene = self.scene.clone();
        let (got_a, got_b) =
            tokio::join!(watch(&scene, self.a, window), watch(&scene, self.b, window));
        let (got_a, got_b) = (got_a?, got_b?);
        if got_b.disconnected {
            self.b_gone = true;
        }
        metrics.set(&format!("{key}_told_a"), describe_got(&got_a));
        metrics.set(&format!("{key}_told_b"), describe_got(&got_b));
        Ok((got_a, got_b))
    }

    /// [`Census::leg_for`] with the usual window.
    async fn leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        commands: Vec<Command>,
    ) -> Result<(Got, Got), TestFailure> {
        self.leg_for(metrics, key, who, commands, WINDOW).await
    }

    /// Brings B back to the spot after a leg that sent it away: logs it back
    /// in if it is gone, and teleports it to the spot, recording where it was
    /// found under `key`.
    async fn bring_back(&mut self, metrics: &mut Metrics, key: &str) -> Result<(), TestFailure> {
        let region = self
            .scene
            .region
            .ok_or_else(|| TestFailure::State("no region".to_owned()))?;
        if self.b_gone || !self.b.is_connected() {
            metrics.set(&format!("{key}_b_was"), "logged out");
            self.b.relogin().await?;
            self.b.wait_for_region(REGION_TIMEOUT).await?;
            self.b_gone = false;
        } else if self.b.region_handle() == Some(region) {
            metrics.set(&format!("{key}_b_was"), "in this region");
        } else {
            metrics.set(&format!("{key}_b_was"), "in another region");
        }
        let spot = self.spot.clone();
        self.b
            .send(Command::Teleport {
                region_handle: region,
                position: RegionCoordinates::new(spot.x, spot.y, spot.z + 2.0),
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            })
            .await?;
        let arrived = self
            .b
            .wait_for(TELEPORT, |event| match event {
                Event::TeleportLocal { .. } | Event::RegionChanged { .. } => Some(Ok(())),
                Event::TeleportFailed { reason, .. } => Some(Err(reason.clone())),
                _ => None,
            })
            .await?;
        if let Err(reason) = arrived {
            return Err(TestFailure::State(format!(
                "B could not be brought back: {}",
                self.scene.scrub(&reason)
            )));
        }
        self.settle().await
    }

    /// Reads the estate configuration as `who`, recording what came under
    /// `key`, and returns the access lists that trailed it.
    async fn read_info(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
    ) -> Result<Got, TestFailure> {
        self.session(who).send(Command::RequestEstateInfo).await?;
        let scene = self.scene.clone();
        let got = watch(&scene, self.session(who), WINDOW).await?;
        metrics.set(key, describe_got(&got));
        Ok(got)
    }

    /// Reads the covenant summary as `who`, recording it under `key` — with
    /// its time where `timed`.
    async fn read_covenant(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        timed: bool,
    ) -> Result<(), TestFailure> {
        self.session(who)
            .send(Command::RequestEstateCovenant)
            .await?;
        let scene = self.scene.clone();
        let got = watch(&scene, self.session(who), WINDOW).await?;
        let words = got.covenants.last().map_or_else(
            || describe_got(&got),
            |covenant| describe_covenant(&scene, covenant, &self.estate_name, timed),
        );
        metrics.set(key, words);
        Ok(())
    }

    /// Reads the access lists over the capability as `who`, recording them
    /// under `key`.
    async fn read_access_cap(&mut self, metrics: &mut Metrics, key: &str, who: Who) {
        let scene = self.scene.clone();
        let words = access_cap(&scene, self.session(who)).await;
        metrics.set(key, words);
    }

    /// The parcel record under the spot, as A reads it.
    async fn parcel(&mut self) -> Result<sl_client_tokio::ParcelInfo, TestFailure> {
        self.sequence = self.sequence.saturating_add(1);
        let spot = self.spot.clone();
        read_parcel(self.a, &spot, self.sequence).await
    }
}

/// The census: both avatars at the spot, the legs, and the clean-up whatever
/// became of them.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let somewhere = ctx
        .build_region()
        .zip(ctx.build_position())
        .map(|(region, point)| (region.to_owned(), point));
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "estate needs a secondary avatar".to_owned(),
        ));
    };
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let a_id = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?
        .uuid();
    let mut names: Vec<(String, &'static str)> = Vec::new();
    for name in names_of(&a.avatar_legacy_name()) {
        names.push((name, "<A>"));
    }
    for name in names_of(&b.avatar_legacy_name()) {
        names.push((name, "<B>"));
    }
    // The longest first, so a legacy name is not half-replaced by its first
    // name.
    names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
    let spot = if is_opensim(grid) {
        OPENSIM_SPOT
    } else {
        let (region, point) = somewhere.ok_or_else(|| {
            TestFailure::State("estate needs a build_location in the fixtures".to_owned())
        })?;
        let handle = region_named(a, &region).await?;
        teleport(a, handle, &point).await?;
        teleport(b, handle, &point).await?;
        point
    };
    let region = a.region_handle();
    let mut census = Census {
        a,
        b,
        scene: Scene {
            a: a_id,
            b: b_id,
            region,
            names,
        },
        spot,
        estate_name: String::new(),
        sequence: SEQUENCE_BASE,
        b_gone: false,
    };
    census.settle().await?;
    let outcome = legs(&mut census, metrics, grid).await;
    let cleaned = clean_up(&census, grid).await;
    outcome.and(cleaned)?;
    restart(&mut census, metrics).await
}

/// Every leg, in order.
async fn legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    grid: Grid,
) -> Result<(), TestFailure> {
    // 1. Reads.
    census.a.send(Command::RequestEstateInfo).await?;
    let info = census
        .a
        .wait_for(WINDOW, |event| match event {
            Event::EstateInfo(info) => Some(Some((**info).clone())),
            _ => None,
        })
        .await
        .or_else(|failure| match failure {
            TestFailure::Timeout(_) => Ok(None),
            other => Err(other),
        })?;
    let rights = info
        .as_ref()
        .is_some_and(|info| info.estate_owner == census.scene.a);
    metrics.set("a_holds_rights", rights);
    if let Some(info) = &info {
        census.estate_name.clone_from(&info.estate_name);
        metrics.set("estate_id", info.estate_id);
    }
    census.settle().await?;
    census.read_info(metrics, "info_a", Who::A).await?;
    census.read_info(metrics, "info_b", Who::B).await?;
    census
        .read_covenant(metrics, "covenant_a", Who::A, false)
        .await?;
    census
        .read_covenant(metrics, "covenant_b", Who::B, false)
        .await?;
    census
        .read_access_cap(metrics, "access_cap_a", Who::A)
        .await;
    census
        .read_access_cap(metrics, "access_cap_b", Who::B)
        .await;

    // 2. Refusals: B has no rights anywhere.
    let a = census.other(Who::B);
    census
        .leg(
            metrics,
            "refuse_kick",
            Who::B,
            vec![Command::KickEstateUser { target: a }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_home",
            Who::B,
            vec![Command::TeleportHomeUser { target: a }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_region_message",
            Who::B,
            vec![Command::SendRegionMessage {
                message: MESSAGE.to_owned(),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_estate_message",
            Who::B,
            vec![Command::SendEstateMessage {
                message: MESSAGE.to_owned(),
            }],
        )
        .await?;
    // A delay rather than a schedule: should a grid ever take it, nothing
    // restarts.
    census
        .leg(
            metrics,
            "refuse_restart",
            Who::B,
            vec![Command::RestartRegion { seconds: -1 }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_access",
            Who::B,
            vec![Command::UpdateEstateAccess {
                delta: EstateAccessDelta::AllowedAgentAdd,
                target: OwnerKey::Agent(AgentKey::from(census.scene.b)),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_covenant",
            Who::B,
            vec![Command::SetEstateCovenant {
                covenant: Some(AssetKey::from(COVENANT)),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_freeze",
            Who::B,
            vec![Command::FreezeUser {
                target: a,
                action: FreezeAction::Freeze,
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_eject",
            Who::B,
            vec![Command::EjectUser {
                target: a,
                action: EjectAction::Eject,
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_telehub",
            Who::B,
            vec![Command::RequestTelehubInfo],
        )
        .await?;

    if !rights {
        let words = if is_opensim(grid) {
            "not measured: A holds no estate rights here"
        } else {
            "not measured: no estate rights on aditi (gridspec-aditi-test-land)"
        };
        for key in OWNER_LEGS {
            for side in ["told_a", "told_b"] {
                metrics.set(&format!("{key}_{side}"), words);
            }
        }
        for key in OWNER_READS {
            metrics.set(key, words);
        }
        return Ok(());
    }

    // B's home where it stands, so sending it home is a teleport rather than
    // a failure (an avatar with no home is kicked instead: see `ban_add`).
    let spot = census.spot.clone();
    census
        .leg(
            metrics,
            "b_set_home",
            Who::B,
            vec![Command::SetStartLocation {
                slot: sl_client_tokio::StartLocationSlot::Home,
                position: RegionCoordinates::new(spot.x, spot.y, spot.z),
                look_at: Vector {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                },
            }],
        )
        .await?;

    // 3. Messages.
    census
        .leg(
            metrics,
            "region_message",
            Who::A,
            vec![Command::SendRegionMessage {
                message: MESSAGE.to_owned(),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "estate_message",
            Who::A,
            vec![Command::SendEstateMessage {
                message: MESSAGE.to_owned(),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "unknown_method",
            Who::A,
            vec![Command::Send {
                message: Box::new(raw_estate_message(census.a, "censusnosuchmethod", &[])?),
                reliability: Reliability::Reliable,
            }],
        )
        .await?;

    // 4. Access.
    let b = AgentKey::from(census.scene.b);
    census
        .leg(
            metrics,
            "allow_add",
            Who::A,
            vec![Command::UpdateEstateAccess {
                delta: EstateAccessDelta::AllowedAgentAdd,
                target: OwnerKey::Agent(b),
            }],
        )
        .await?;
    census
        .read_access_cap(metrics, "access_cap_after_allow", Who::A)
        .await;
    census
        .leg(
            metrics,
            "allow_remove",
            Who::A,
            vec![Command::UpdateEstateAccess {
                delta: EstateAccessDelta::AllowedAgentRemove,
                target: OwnerKey::Agent(b),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "manager_add",
            Who::A,
            vec![Command::UpdateEstateAccess {
                delta: EstateAccessDelta::ManagerAdd,
                target: OwnerKey::Agent(b),
            }],
        )
        .await?;
    census
        .read_access_cap(metrics, "access_cap_after_manager", Who::A)
        .await;
    census
        .read_access_cap(metrics, "access_cap_b_as_manager", Who::B)
        .await;
    census
        .read_info(metrics, "info_b_as_manager", Who::B)
        .await?;
    census
        .leg(
            metrics,
            "manager_remove",
            Who::A,
            vec![Command::UpdateEstateAccess {
                delta: EstateAccessDelta::ManagerRemove,
                target: OwnerKey::Agent(b),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "ban_add",
            Who::A,
            vec![Command::UpdateEstateAccess {
                delta: EstateAccessDelta::BannedAgentAdd,
                target: OwnerKey::Agent(b),
            }],
        )
        .await?;
    census
        .read_access_cap(metrics, "access_cap_after_ban", Who::A)
        .await;
    census
        .leg(
            metrics,
            "ban_remove",
            Who::A,
            vec![Command::UpdateEstateAccess {
                delta: EstateAccessDelta::BannedAgentRemove,
                target: OwnerKey::Agent(b),
            }],
        )
        .await?;
    census.bring_back(metrics, "ban").await?;

    // 5. The covenant.
    census
        .leg(
            metrics,
            "covenant_set",
            Who::A,
            vec![Command::SetEstateCovenant {
                covenant: Some(AssetKey::from(COVENANT)),
            }],
        )
        .await?;
    census
        .read_covenant(metrics, "covenant_after", Who::B, true)
        .await?;
    census
        .leg(
            metrics,
            "covenant_reset",
            Who::A,
            vec![Command::SetEstateCovenant { covenant: None }],
        )
        .await?;

    // 6. Freeze and eject.
    census
        .leg(
            metrics,
            "freeze",
            Who::A,
            vec![Command::FreezeUser {
                target: b,
                action: FreezeAction::Freeze,
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "unfreeze",
            Who::A,
            vec![Command::FreezeUser {
                target: b,
                action: FreezeAction::Unfreeze,
            }],
        )
        .await?;
    census
        .leg_for(
            metrics,
            "freeze_expiry",
            Who::A,
            vec![Command::FreezeUser {
                target: b,
                action: FreezeAction::Freeze,
            }],
            FREEZE_WINDOW,
        )
        .await?;
    census
        .leg(
            metrics,
            "eject",
            Who::A,
            vec![Command::EjectUser {
                target: b,
                action: EjectAction::Eject,
            }],
        )
        .await?;
    census.bring_back(metrics, "eject").await?;
    census
        .leg(
            metrics,
            "eject_ban",
            Who::A,
            vec![Command::EjectUser {
                target: b,
                action: EjectAction::EjectAndBan,
            }],
        )
        .await?;
    let parcel = census.parcel().await?;
    let circuit = census
        .a
        .circuit_id()
        .ok_or_else(|| TestFailure::State("no root circuit".to_owned()))?;
    let scoped = sl_client_tokio::ScopedParcelId::new(circuit, parcel.local_id);
    census
        .leg(
            metrics,
            "eject_ban_clear",
            Who::A,
            vec![
                Command::RequestParcelAccessList {
                    local_id: scoped,
                    scope: ParcelAccessScope::Ban,
                },
                Command::UpdateParcelAccessList {
                    local_id: scoped,
                    scope: ParcelAccessScope::Ban,
                    entries: Vec::new(),
                },
            ],
        )
        .await?;
    census.bring_back(metrics, "eject_ban").await?;

    // 7. Home and kick.
    census
        .leg(
            metrics,
            "home",
            Who::A,
            vec![Command::TeleportHomeUser { target: b }],
        )
        .await?;
    census.bring_back(metrics, "home").await?;
    census
        .leg(
            metrics,
            "home_all",
            Who::A,
            vec![Command::TeleportHomeAllUsers],
        )
        .await?;
    census.bring_back(metrics, "home_all").await?;
    census
        .leg(
            metrics,
            "kick",
            Who::A,
            vec![Command::KickEstateUser { target: b }],
        )
        .await?;
    census.bring_back(metrics, "kick").await?;
    Ok(())
}

/// The restart legs, after the clean-up: the region goes down.
async fn restart(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    if metrics.get("a_holds_rights") != Some(&crate::record::MetricValue::Bool(true)) {
        return Ok(());
    }
    // The clean-up's replies, heard out before the restart's window opens.
    census.settle().await?;
    census
        .leg(
            metrics,
            "restart_cancel_idle",
            Who::A,
            vec![Command::RestartRegion { seconds: -1 }],
        )
        .await?;
    census
        .leg_for(
            metrics,
            "restart",
            Who::A,
            vec![Command::RestartRegion {
                seconds: RESTART_SECONDS,
            }],
            RESTART_WINDOW,
        )
        .await?;
    Ok(())
}

/// An `EstateOwnerMessage` with `method` and `params`, as `session`'s agent,
/// for a method the client has no command for.
fn raw_estate_message(
    session: &Session,
    method: &str,
    params: &[&str],
) -> Result<AnyMessage, TestFailure> {
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::State("no agent id".to_owned()))?;
    let session_id = session
        .session_id()
        .ok_or_else(|| TestFailure::State("no session id".to_owned()))?;
    let with_nul = |text: &str| {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        bytes
    };
    let param_list = if params.is_empty() {
        vec![EstateOwnerMessageParamListBlock {
            parameter: Vec::new(),
        }]
    } else {
        params
            .iter()
            .map(|param| EstateOwnerMessageParamListBlock {
                parameter: with_nul(param),
            })
            .collect()
    };
    Ok(AnyMessage::EstateOwnerMessage(EstateOwnerMessage {
        agent_data: EstateOwnerMessageAgentDataBlock {
            agent_id: agent.uuid(),
            session_id,
            transaction_id: Uuid::nil(),
        },
        method_data: EstateOwnerMessageMethodDataBlock {
            method: with_nul(method),
            invoice: Uuid::new_v4(),
        },
        param_list,
    }))
}

/// Leaves the estate as the census found it — B on no list, no covenant —
/// where A could change it; after a restart the region is coming back up
/// and there is nothing left to undo.
async fn clean_up(census: &Census<'_>, grid: Grid) -> Result<(), TestFailure> {
    if !is_opensim(grid) || !census.a.is_connected() {
        return Ok(());
    }
    let b = AgentKey::from(census.scene.b);
    for delta in [
        EstateAccessDelta::AllowedAgentRemove,
        EstateAccessDelta::BannedAgentRemove,
        EstateAccessDelta::ManagerRemove,
    ] {
        census
            .a
            .send(Command::UpdateEstateAccess {
                delta,
                target: OwnerKey::Agent(b),
            })
            .await?;
    }
    census
        .a
        .send(Command::SetEstateCovenant { covenant: None })
        .await?;
    Ok(())
}
