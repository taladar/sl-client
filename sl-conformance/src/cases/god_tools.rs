//! A census of the god tools: what a grid answers an avatar that asks for god
//! powers, and what each avatar is told when a god — or a resident trying to
//! be one — kicks, freezes, rewrites the region or forces an owner.
//!
//! Avatar **A** is a grid god where the test accounts hold one; **B** is a
//! resident; **C** is the estate owner where the test avatars hold estate
//! rights. On OpenSim A is an account of user level 250
//! (`--avatar grid-god --secondary resident --tertiary estate-owner`), and C a
//! god inside its own region by OpenSim's `region_owner_is_god`. On Second
//! Life no test account is a god and none holds estate rights, so all three
//! are residents standing at the fixtures' build location, and only what a
//! resident is answered is measured.
//!
//! One question a leg:
//!
//! 1. **Powers**: each avatar asks for god powers and gives them up
//!    (`RequestGodlikePowers`).
//! 2. **Refusals**: B, holding no powers, freezes A over UDP and over the
//!    `UntrustedSimulatorMessage` capability (and once with A's id and its own
//!    session), sends a `GodlikeMessage`, rewrites the region
//!    (`GodUpdateRegionInfo`, with the values the region states, so nothing
//!    changes even if a grid took it), returns A's objects
//!    (`SimWideDeletes`), forces the parcel's owner, marks the parcel as
//!    content and forces the owner of A's cube.
//! 3. **Out of god mode**: A, a god that has given its powers up, freezes B
//!    and forces the owner of its own cube to B.
//! 4. **God mode** (a god account only): A freezes and unfreezes B over each
//!    transport, kicks B, is kicked by C (a lesser god), kicks everybody,
//!    sends the `GodlikeMessage`, rewrites the region, returns B's objects,
//!    forces the parcel's owner to itself and back, marks the parcel as
//!    content, and forces the cube's owner to B and back.
//!
//! Every leg records what each avatar was told (`*_told_a`, `*_told_b`,
//! `*_told_c`). Every id is described by whose it is and every name is
//! scrubbed, so no metric names an avatar.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/estate.md` § God tools; every run is held to it.
//!
//! `3av`, live grids.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    AgentKey, AnyMessage, Command, DeRezDestination, Event, GodKickAction, GodKickTarget,
    GodRegionUpdate, InventoryFolderKey, ObjectKey, OwnerKey, ParcelInfo, PrimShape,
    RegionCoordinates, RegionHandle, RegionLimits, Reliability, ScopedObjectId, ScopedParcelId,
    SimWideDeleteFlags, TransactionId, Uuid, Vector,
};

use sl_wire::messages::{GodKickUser, GodKickUserUserInfoBlock};

use crate::cases::asset_http::without_ids;
use crate::cases::estate::describe_llsd;
use crate::cases::groups::names_of;
use crate::cases::land_transactions::{read_parcel, region_named, teleport};
use crate::cases::object_rez_derez::Answer;
use crate::cases::object_rez_derez::{REZ_LIFT_M, SETTLE_IDLE, SETTLE_WINDOW};
use crate::cases::task_inventory::{OPENSIM_START, hold_to_measured, login_folders};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, settle_scene, wait_for_own_new_object};

/// How long the avatars are listened to after a leg's request.
const WINDOW: Duration = Duration::from_secs(8);

/// How long a teleport back to the spot may take.
const TELEPORT: Duration = Duration::from_secs(60);

/// How long the cube may take to appear.
const REZ: Duration = Duration::from_secs(30);

/// Where the avatars stand on OpenSim: the Default Region's centre.
const OPENSIM_SPOT: Vector = Vector {
    x: 128.0,
    y: 128.0,
    z: 25.0,
};

/// The reason every kick and freeze gives.
const REASON: &str = "god tools census";

/// The method of the census's `GodlikeMessage`: the reference's god-mode
/// telehub read (`LLSelectMgr::sendGodlikeRequest("telehub", "info ui")`),
/// which changes nothing.
const GODLIKE_METHOD: &str = "telehub";

/// The parameter of the census's `GodlikeMessage`.
const GODLIKE_PARAMETER: &str = "info ui";

/// The first sequence id of the census's own parcel reads.
const SEQUENCE_BASE: i32 = 7500;

/// The legs only a god account can run, in order: what each grid without
/// one records instead.
const GOD_LEGS: [&str; 18] = [
    "god_on",
    "god_freeze_udp",
    "god_unfreeze_udp",
    "god_freeze",
    "god_unfreeze",
    "god_kick",
    "god_wars_on",
    "god_wars",
    "god_wars_off",
    "god_kick_all",
    "god_godlike_message",
    "god_region_info",
    "god_sim_wide_deletes",
    "god_parcel_owner",
    "god_parcel_owner_back",
    "god_mark_content",
    "god_object_owner",
    "god_object_owner_back",
];

/// The reads only a god account takes, keyed as they are recorded.
const GOD_READS: [&str; 7] = [
    "god_kick_b_was",
    "god_kick_all_b_was",
    "god_kick_all_c_was",
    "parcel_owner_after_force",
    "parcel_owner_after_force_back",
    "cube_owner_after_force",
    "cube_owner_after_force_back",
];

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/estate.md § God tools (god-tools, 2026-10-18)";

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
/// Held as anything on Second Life: whose the parcel is (it is the land's).
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("a_is_god", each(Answer::Flag(false), Answer::Flag(true))),
    (
        "cube_owner_after_force",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("B"),
        ),
    ),
    (
        "cube_owner_after_force_back",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("A"),
        ),
    ),
    (
        "cube_owner_after_latent",
        each(Answer::Text("A"), Answer::Text("B")),
    ),
    ("cube_owner_after_refusal", both(Answer::Text("A"))),
    (
        "god_freeze_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("agent alert \"/User Frozen\""),
        ),
    ),
    (
        "god_freeze_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("agent alert \"/god tools census\""),
        ),
    ),
    (
        "god_freeze_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_freeze_udp_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("agent alert \"/User Frozen\""),
        ),
    ),
    (
        "god_freeze_udp_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("agent alert \"/god tools census\""),
        ),
    ),
    (
        "god_freeze_udp_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_godlike_message_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_godlike_message_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_godlike_message_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_kick_all_b_was",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("logged out"),
        ),
    ),
    (
        "god_kick_all_c_was",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("logged out"),
        ),
    ),
    (
        "god_kick_all_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_kick_all_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text(
                "kicked B \"god tools census\"; disconnected Kicked { message: \"god tools census\" }",
            ),
        ),
    ),
    (
        "god_kick_all_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text(
                "kicked C \"god tools census\"; disconnected Kicked { message: \"god tools census\" }",
            ),
        ),
    ),
    (
        "god_kick_b_was",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("logged out"),
        ),
    ),
    (
        "god_kick_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_kick_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text(
                "kicked B \"god tools census\"; disconnected Kicked { message: \"god tools census\" }",
            ),
        ),
    ),
    (
        "god_kick_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_mark_content_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("alert \"Please check ini files.\n[LandManagement] config section.\""),
        ),
    ),
    (
        "god_mark_content_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_mark_content_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_object_owner_back_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_object_owner_back_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_object_owner_back_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_object_owner_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_object_owner_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_object_owner_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_on_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("granted god level 250"),
        ),
    ),
    (
        "god_on_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_on_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_parcel_owner_back_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("parcel properties (owner C)"),
        ),
    ),
    (
        "god_parcel_owner_back_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_parcel_owner_back_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_parcel_owner_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("parcel properties (owner A)"),
        ),
    ),
    (
        "god_parcel_owner_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_parcel_owner_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_region_info_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_region_info_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_region_info_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_sim_wide_deletes_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_sim_wide_deletes_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_sim_wide_deletes_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_unfreeze_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("agent alert \"/User Unfrozen\""),
        ),
    ),
    (
        "god_unfreeze_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("agent alert \"/god tools census\""),
        ),
    ),
    (
        "god_unfreeze_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_unfreeze_udp_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("agent alert \"/User Unfrozen\""),
        ),
    ),
    (
        "god_unfreeze_udp_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("agent alert \"/god tools census\""),
        ),
    ),
    (
        "god_unfreeze_udp_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_wars_off_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_wars_off_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_wars_off_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("granted god level 0"),
        ),
    ),
    (
        "god_wars_on_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_wars_on_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_wars_on_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("granted god level 220"),
        ),
    ),
    (
        "god_wars_told_a",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("agent alert \"/Kick from <C> ignored, kick reason: god tools census\""),
        ),
    ),
    (
        "god_wars_told_b",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "god_wars_told_c",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("nothing"),
        ),
    ),
    (
        "latent_freeze_told_a",
        each(
            Answer::Text("nothing"),
            Answer::Text("agent alert \"/User Frozen\""),
        ),
    ),
    (
        "latent_freeze_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("agent alert \"/god tools census\""),
        ),
    ),
    ("latent_freeze_told_c", both(Answer::Text("nothing"))),
    ("latent_object_owner_told_a", both(Answer::Text("nothing"))),
    ("latent_object_owner_told_b", both(Answer::Text("nothing"))),
    ("latent_object_owner_told_c", both(Answer::Text("nothing"))),
    ("parcel_owner", each(Answer::Any, Answer::Text("C"))),
    (
        "parcel_owner_after_force",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("A"),
        ),
    ),
    (
        "parcel_owner_after_force_back",
        each(
            Answer::Text("not measured: no god account on Second Life"),
            Answer::Text("C"),
        ),
    ),
    (
        "powers_a_told_a",
        each(
            Answer::Text(
                "alert \"Request for special powers failed. This request has been logged.\" keyed SpecialPowersRequestFailedLogged []",
            ),
            Answer::Text("granted god level 250"),
        ),
    ),
    ("powers_a_told_b", both(Answer::Text("nothing"))),
    ("powers_a_told_c", both(Answer::Text("nothing"))),
    ("powers_b_told_a", both(Answer::Text("nothing"))),
    (
        "powers_b_told_b",
        each(
            Answer::Text(
                "alert \"Request for special powers failed. This request has been logged.\" keyed SpecialPowersRequestFailedLogged []",
            ),
            Answer::Text("agent alert \"/Request for god powers denied\""),
        ),
    ),
    ("powers_b_told_c", both(Answer::Text("nothing"))),
    ("powers_c_told_a", both(Answer::Text("nothing"))),
    ("powers_c_told_b", both(Answer::Text("nothing"))),
    (
        "powers_c_told_c",
        each(
            Answer::Text(
                "alert \"Request for special powers failed. This request has been logged.\" keyed SpecialPowersRequestFailedLogged []",
            ),
            Answer::Text("granted god level 220"),
        ),
    ),
    (
        "powers_drop_a_told_a",
        each(Answer::Text("nothing"), Answer::Text("granted god level 0")),
    ),
    ("powers_drop_a_told_b", both(Answer::Text("nothing"))),
    ("powers_drop_a_told_c", both(Answer::Text("nothing"))),
    ("powers_drop_b_told_a", both(Answer::Text("nothing"))),
    ("powers_drop_b_told_b", both(Answer::Text("nothing"))),
    ("powers_drop_b_told_c", both(Answer::Text("nothing"))),
    ("powers_drop_c_told_a", both(Answer::Text("nothing"))),
    ("powers_drop_c_told_b", both(Answer::Text("nothing"))),
    (
        "powers_drop_c_told_c",
        each(Answer::Text("nothing"), Answer::Text("granted god level 0")),
    ),
    (
        "refuse_freeze_cap_answer",
        each(
            Answer::Text("HTTP 200, undef"),
            Answer::Text("HTTP 200, no body"),
        ),
    ),
    (
        "refuse_freeze_cap_as_a_answer",
        each(
            Answer::Text("HTTP 200, undef"),
            Answer::Text("HTTP 401, no body"),
        ),
    ),
    (
        "refuse_freeze_cap_as_a_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "refuse_freeze_cap_as_a_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "refuse_freeze_cap_as_a_told_c",
        both(Answer::Text("nothing")),
    ),
    ("refuse_freeze_cap_told_a", both(Answer::Text("nothing"))),
    ("refuse_freeze_cap_told_b", both(Answer::Text("nothing"))),
    ("refuse_freeze_cap_told_c", both(Answer::Text("nothing"))),
    ("refuse_freeze_udp_told_a", both(Answer::Text("nothing"))),
    ("refuse_freeze_udp_told_b", both(Answer::Text("nothing"))),
    ("refuse_freeze_udp_told_c", both(Answer::Text("nothing"))),
    (
        "refuse_godlike_message_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "refuse_godlike_message_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "refuse_godlike_message_told_c",
        both(Answer::Text("nothing")),
    ),
    ("refuse_mark_content_told_a", both(Answer::Text("nothing"))),
    (
        "refuse_mark_content_told_b",
        each(
            Answer::Text("nothing"),
            Answer::Text("alert \"Request denied. You're not priviliged.\""),
        ),
    ),
    ("refuse_mark_content_told_c", both(Answer::Text("nothing"))),
    ("refuse_object_owner_told_a", both(Answer::Text("nothing"))),
    ("refuse_object_owner_told_b", both(Answer::Text("nothing"))),
    ("refuse_object_owner_told_c", both(Answer::Text("nothing"))),
    (
        "refuse_other_message_answer",
        each(
            Answer::Text("HTTP 404, not LLSD \"Not Found\""),
            Answer::Text("HTTP 400, no body"),
        ),
    ),
    ("refuse_other_message_told_a", both(Answer::Text("nothing"))),
    ("refuse_other_message_told_b", both(Answer::Text("nothing"))),
    ("refuse_other_message_told_c", both(Answer::Text("nothing"))),
    ("refuse_parcel_owner_told_a", both(Answer::Text("nothing"))),
    ("refuse_parcel_owner_told_b", both(Answer::Text("nothing"))),
    ("refuse_parcel_owner_told_c", both(Answer::Text("nothing"))),
    ("refuse_region_info_told_a", both(Answer::Text("nothing"))),
    ("refuse_region_info_told_b", both(Answer::Text("nothing"))),
    ("refuse_region_info_told_c", both(Answer::Text("nothing"))),
    (
        "refuse_sim_wide_deletes_told_a",
        both(Answer::Text("nothing")),
    ),
    (
        "refuse_sim_wide_deletes_told_b",
        both(Answer::Text("nothing")),
    ),
    (
        "refuse_sim_wide_deletes_told_c",
        both(Answer::Text("nothing")),
    ),
];

/// God tools: powers, refusals and what a god's actions tell each avatar.
#[derive(Debug)]
pub struct GodTools;

impl GridTest for GodTools {
    fn name(&self) -> &'static str {
        "god-tools"
    }

    fn description(&self) -> &'static str {
        "God powers, god kicks and freezes over UDP and UntrustedSimulatorMessage, godlike \
         messages, god region info, sim-wide deletes and forced owners: refusals and what each \
         avatar is told"
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

    fn rezzes_objects(&self) -> bool {
        true
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(1800)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let trash = login_folders(ctx.primary()).await?.trash;
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics, trash).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            outcome.and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
        })
    }
}

/// Which avatar acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The grid god, where there is one.
    A,
    /// The resident.
    B,
    /// The estate owner, where the test avatars hold estate rights.
    C,
}

/// Who is who, for describing ids and scrubbing names.
#[derive(Debug, Clone, Default)]
struct Scene {
    /// Avatar A.
    a: Uuid,
    /// Avatar B.
    b: Uuid,
    /// Avatar C.
    c: Uuid,
    /// The region the census stands in.
    region: Option<RegionHandle>,
    /// A's cube, once rezzed.
    cube: Option<ObjectKey>,
    /// Every way a grid may write an avatar's name, with the word that
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
        } else if id == self.c {
            "C"
        } else if Some(id) == self.cube.map(|cube| cube.uuid()) {
            "the cube"
        } else if id == sl_wire::ALL_AGENTS {
            "everyone"
        } else {
            "another"
        }
    }

    /// `text` with every name of the avatars' replaced, and every id but the
    /// avatars' too: an avatar's id is named by whose it is.
    fn scrub(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for (id, word) in [(self.a, "<A>"), (self.b, "<B>"), (self.c, "<C>")] {
            if !id.is_nil() {
                out = out.replace(&id.to_string(), word);
            }
        }
        out = without_ids(&out);
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
}

/// Folds one event into `got`; `None` for an event the census does not read.
fn fold(scene: &Scene, got: &mut Got, event: &Event) -> Option<()> {
    let line = match event {
        Event::GodlikePowersGranted { god_level } => format!("granted god level {god_level}"),
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
        Event::InstantMessageReceived(im) => format!(
            "im {:?} from {} as \"{}\" \"{}\"",
            im.dialog,
            scene.whose(im.from_agent_id.uuid()),
            scene.scrub(&im.from_agent_name),
            scene.scrub(&im.message)
        ),
        Event::ChatReceived(chat) => format!(
            "chat {:?} {:?} from \"{}\" \"{}\"",
            chat.source,
            chat.chat_type,
            scene.scrub(&chat.from_name),
            scene.scrub(&chat.message)
        ),
        Event::ParcelProperties(parcel) => format!(
            "parcel properties (owner {})",
            scene.whose(parcel.owner.uuid())
        ),
        Event::TelehubInfo(_) => "telehub info".to_owned(),
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
        Event::TeleportLocal { position, .. } => {
            format!("teleport local to {:.0},{:.0}", position.x(), position.y())
        }
        Event::TeleportFailed { reason, .. } => {
            format!("teleport failed \"{}\"", scene.scrub(reason))
        }
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

/// The census's state: the three sessions and who is who.
struct Census<'s> {
    /// Avatar A.
    a: &'s mut Session,
    /// Avatar B.
    b: &'s mut Session,
    /// Avatar C.
    c: &'s mut Session,
    /// Who is who.
    scene: Scene,
    /// Where the avatars stand.
    spot: Vector,
    /// The next sequence id of a parcel read.
    sequence: i32,
    /// The avatars whose session ended in the last leg (a kick): their
    /// handles still say connected until they are logged back in.
    gone: HashSet<char>,
    /// The objects A saw before its cube, none of which is it.
    seen: HashSet<ScopedObjectId>,
    /// Where A's cube goes.
    cube_at: Vector,
}

impl Census<'_> {
    /// The session of `who`.
    const fn session(&mut self, who: Who) -> &mut Session {
        match who {
            Who::A => self.a,
            Who::B => self.b,
            Who::C => self.c,
        }
    }

    /// Lets every avatar hear out what the grid still had to say.
    async fn settle(&mut self) -> Result<(), TestFailure> {
        let scene = self.scene.clone();
        let (got_a, got_b, got_c) = tokio::join!(
            watch(&scene, self.a, WINDOW),
            watch(&scene, self.b, WINDOW),
            watch(&scene, self.c, WINDOW)
        );
        let _heard: (Got, Got, Got) = (got_a?, got_b?, got_c?);
        Ok(())
    }

    /// Listens to every avatar for a window and records what each was told
    /// under `key`.
    async fn hear(&mut self, metrics: &mut Metrics, key: &str) -> Result<(), TestFailure> {
        let scene = self.scene.clone();
        let (got_a, got_b, got_c) = tokio::join!(
            watch(&scene, self.a, WINDOW),
            watch(&scene, self.b, WINDOW),
            watch(&scene, self.c, WINDOW)
        );
        for (got, side, letter) in [(got_a?, "a", 'a'), (got_b?, "b", 'b'), (got_c?, "c", 'c')] {
            if got.disconnected {
                self.gone.insert(letter);
            }
            metrics.set(&format!("{key}_told_{side}"), describe_got(&got));
        }
        Ok(())
    }

    /// Sends `commands` as `who` and records what each avatar was told under
    /// `key`.
    async fn leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        commands: Vec<Command>,
    ) -> Result<(), TestFailure> {
        for command in commands {
            self.session(who).send(command).await?;
        }
        self.hear(metrics, key).await
    }

    /// Posts `body` to `who`'s `UntrustedSimulatorMessage` capability, records
    /// the answer under `{key}_answer` and what each avatar was told under
    /// `key`.
    async fn post_leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        body: String,
    ) -> Result<(), TestFailure> {
        let scene = self.scene.clone();
        let answer = post_untrusted(&scene, self.session(who), body).await;
        metrics.set(&format!("{key}_answer"), answer);
        self.hear(metrics, key).await
    }

    /// Brings `who` back to the spot after a leg that sent it away: logs it
    /// back in if it is gone, and teleports it to the spot, recording where it
    /// was found under `key`.
    async fn bring_back(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
    ) -> Result<(), TestFailure> {
        let region = self
            .scene
            .region
            .ok_or_else(|| TestFailure::State("no region".to_owned()))?;
        let letter = match who {
            Who::A => 'a',
            Who::B => 'b',
            Who::C => 'c',
        };
        let gone = self.gone.remove(&letter);
        let spot = self.spot.clone();
        let session = self.session(who);
        if gone || !session.is_connected() {
            metrics.set(key, "logged out");
            session.relogin().await?;
            session.wait_for_region(REGION_TIMEOUT).await?;
        } else if session.region_handle() == Some(region) {
            metrics.set(key, "in this region");
        } else {
            metrics.set(key, "in another region");
        }
        if session.region_handle() != Some(region) {
            teleport(session, region, &spot).await?;
        } else {
            session
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
            let arrived = session
                .wait_for(TELEPORT, |event| match event {
                    Event::TeleportLocal { .. } | Event::RegionChanged { .. } => Some(Ok(())),
                    Event::TeleportFailed { reason, .. } => Some(Err(reason.clone())),
                    _ => None,
                })
                .await?;
            if let Err(reason) = arrived {
                return Err(TestFailure::State(format!(
                    "an avatar could not be brought back: {}",
                    self.scene.scrub(&reason)
                )));
            }
        }
        self.settle().await
    }

    /// The parcel record under the spot, as A reads it.
    async fn parcel(&mut self) -> Result<ParcelInfo, TestFailure> {
        self.sequence = self.sequence.saturating_add(1);
        let spot = self.spot.clone();
        read_parcel(self.a, &spot, self.sequence).await
    }

    /// The cube's owner, in a word, as A reads it.
    async fn cube_owner(&mut self) -> Result<String, TestFailure> {
        let Some(cube) = self.scene.cube else {
            return Ok("no cube".to_owned());
        };
        self.a
            .send(Command::RequestObjectPropertiesFamily {
                request_flags: 0,
                object_id: cube,
            })
            .await?;
        let scene = self.scene.clone();
        let owner = self
            .a
            .wait_for(WINDOW, |event| match event {
                Event::ObjectPropertiesFamily { properties } if properties.object_id == cube => {
                    Some(properties.owner.uuid())
                }
                _ => None,
            })
            .await;
        match owner {
            Ok(owner) => Ok(scene.whose(owner).to_owned()),
            Err(TestFailure::Timeout(_)) => Ok("no answer".to_owned()),
            Err(other) => Err(other),
        }
    }
}

/// A `GodKickUser` as a raw UDP message from `session`'s agent, naming `god`
/// and `god_session` as the god — the UDP road, whatever capability the client
/// would take.
fn raw_god_kick(god: Uuid, god_session: Uuid, target: Uuid, action: GodKickAction) -> AnyMessage {
    let mut reason = REASON.as_bytes().to_vec();
    reason.push(0);
    AnyMessage::GodKickUser(GodKickUser {
        user_info: GodKickUserUserInfoBlock {
            god_id: god,
            god_session_id: god_session,
            agent_id: target,
            kick_flags: action.to_wire(),
            reason,
        },
    })
}

/// The `UntrustedSimulatorMessage` body of a god kick naming `god` and
/// `god_session`.
fn untrusted_god_kick(god: Uuid, god_session: Uuid, target: Uuid, action: GodKickAction) -> String {
    sl_wire::build_god_kick_user_untrusted(&sl_wire::GodKickUserRequest {
        god: AgentKey::from(god),
        god_session,
        target,
        kick_flags: action.to_wire(),
        reason: REASON.to_owned(),
    })
}

/// `session`'s `UntrustedSimulatorMessage` answer to `body`, in words: the
/// status and, where there is one, the body.
async fn post_untrusted(scene: &Scene, session: &Session, body: String) -> String {
    let Some(url) = session.cap("UntrustedSimulatorMessage") else {
        return "no capability".to_owned();
    };
    let answer = reqwest::Client::new()
        .post(url)
        .header("Content-Type", "application/llsd+xml")
        .body(body)
        .send()
        .await;
    let response = match answer {
        Ok(response) => response,
        Err(error) => return format!("unreachable ({})", scene.scrub(&error.to_string())),
    };
    let status = response.status().as_u16();
    let text = response.text().await.unwrap_or_default();
    if text.trim().is_empty() {
        return format!("HTTP {status}, no body");
    }
    if !text.contains("<llsd") {
        return format!(
            "HTTP {status}, not LLSD \"{}\"",
            scene.scrub(&text.chars().take(40).collect::<String>())
        );
    }
    let Ok(llsd) = sl_wire::parse_llsd_xml(&text) else {
        return format!("HTTP {status}, unparsable LLSD");
    };
    let names = crate::cases::estate::Scene {
        a: scene.a,
        b: scene.b,
        region: scene.region,
        names: scene.names.clone(),
    };
    format!(
        "HTTP {status}, {}",
        scene.scrub(&describe_llsd(&names, &llsd))
    )
}

/// The region's own values as a `GodUpdateRegionInfo`, so a grid that took
/// one would change nothing.
fn identity_update(limits: &RegionLimits) -> Option<GodRegionUpdate> {
    Some(GodRegionUpdate {
        sim_name: limits.sim_name.clone()?,
        estate_id: limits.estate_id,
        parent_estate_id: limits.parent_estate_id,
        region_flags: limits.region_flags_extended.unwrap_or(limits.region_flags),
        billable_factor: limits.billable_factor,
        price_per_meter: i32::try_from(limits.price_per_meter.0).ok()?,
        redirect_grid_x: limits.redirect_grid_x,
        redirect_grid_y: limits.redirect_grid_y,
    })
}

/// The census: the avatars at the spot, the cube, the legs, and the clean-up
/// whatever became of them.
async fn census(
    ctx: &mut TestContext,
    metrics: &mut Metrics,
    trash: InventoryFolderKey,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let somewhere_point = ctx.build_position();
    let somewhere = ctx
        .build_region()
        .zip(ctx.build_position())
        .map(|(region, point)| (region.to_owned(), point));
    let Some((a, b, c)) = ctx.all_three() else {
        return Err(TestFailure::State(
            "god-tools needs three avatars".to_owned(),
        ));
    };
    for session in [&mut *a, &mut *b, &mut *c] {
        session.wait_for_region(REGION_TIMEOUT).await?;
    }
    let id = |session: &Session| {
        session
            .agent_id()
            .map(|agent| agent.uuid())
            .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))
    };
    let (a_id, b_id, c_id) = (id(a)?, id(b)?, id(c)?);
    let mut names: Vec<(String, &'static str)> = Vec::new();
    for (session, word) in [(&*a, "<A>"), (&*b, "<B>"), (&*c, "<C>")] {
        for name in names_of(&session.avatar_legacy_name()) {
            names.push((name, word));
        }
    }
    // The longest first, so a legacy name is not half-replaced by its first
    // name.
    names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
    let spot = if is_opensim(grid) {
        OPENSIM_SPOT
    } else {
        let (region, point) = somewhere.ok_or_else(|| {
            TestFailure::State("god-tools needs a build_location in the fixtures".to_owned())
        })?;
        let handle = region_named(a, &region).await?;
        for session in [&mut *a, &mut *b, &mut *c] {
            teleport(session, handle, &point).await?;
        }
        point
    };
    // Where A's cube goes: against the anchor the rez cases place against,
    // with the objects already there remembered so none is taken for it.
    let (seen, anchor) = settle_scene(a, grid, somewhere_point, SETTLE_WINDOW, SETTLE_IDLE).await?;
    let anchor = anchor.ok_or_else(|| {
        TestFailure::Assertion("the arrival streamed nothing to place a cube against".to_owned())
    })?;
    let region = a.region_handle();
    let mut census = Census {
        a,
        b,
        c,
        scene: Scene {
            a: a_id,
            b: b_id,
            c: c_id,
            region,
            cube: None,
            names,
        },
        spot,
        sequence: SEQUENCE_BASE,
        gone: HashSet::new(),
        seen,
        cube_at: Vector {
            x: anchor.x,
            y: anchor.y,
            z: anchor.z + REZ_LIFT_M,
        },
    };
    census.settle().await?;
    let mut cube: Option<ScopedObjectId> = None;
    let mut parcel_owner: Option<OwnerKey> = None;
    let outcome = legs(&mut census, metrics, grid, &mut cube, &mut parcel_owner).await;
    let cleaned = clean_up(&mut census, cube, parcel_owner, trash).await;
    outcome.and(cleaned)
}

/// Every leg, in order; `cube` and `parcel_owner` say what the clean-up has
/// to undo.
async fn legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    grid: Grid,
    cube: &mut Option<ScopedObjectId>,
    parcel_owner: &mut Option<OwnerKey>,
) -> Result<(), TestFailure> {
    // The region's own values, for the region-info legs.
    census.a.send(Command::RequestRegionInfo).await?;
    let limits = census
        .a
        .wait_for(WINDOW, |event| match event {
            Event::RegionLimits(limits) => Some(limits.clone()),
            _ => None,
        })
        .await?;
    let update = identity_update(&limits)
        .ok_or_else(|| TestFailure::State("the region stated no usable record".to_owned()))?;
    census.settle().await?;

    // A's cube, for the forced-owner legs.
    census
        .a
        .send(Command::RezObject {
            shape: PrimShape::cube(census.cube_at.clone()),
            group_id: None,
        })
        .await?;
    let rezzed = wait_for_own_new_object(census.a, &census.seen, REZ)
        .await?
        .map_err(|reason| {
            TestFailure::Assertion(format!(
                "no cube appeared after RezObject: {}",
                census.scene.scrub(&reason)
            ))
        })?;
    *cube = Some(rezzed.scoped_id());
    census.scene.cube = Some(rezzed.full_id);
    census.settle().await?;
    let parcel = census.parcel().await?;
    *parcel_owner = Some(parcel.owner);
    metrics.set("parcel_owner", census.scene.whose(parcel.owner.uuid()));
    let circuit = census
        .a
        .circuit_id()
        .ok_or_else(|| TestFailure::State("no root circuit".to_owned()))?;
    let scoped_parcel = ScopedParcelId::new(circuit, parcel.local_id);
    census.settle().await?;

    // 1. Powers.
    for (who, word) in [(Who::A, "a"), (Who::B, "b"), (Who::C, "c")] {
        census
            .leg(
                metrics,
                &format!("powers_{word}"),
                who,
                vec![Command::RequestGodlikePowers { godlike: true }],
            )
            .await?;
        census
            .leg(
                metrics,
                &format!("powers_drop_{word}"),
                who,
                vec![Command::RequestGodlikePowers { godlike: false }],
            )
            .await?;
    }
    let a_is_god = metrics
        .get("powers_a_told_a")
        .is_some_and(|told| format!("{told:?}").contains("granted god level 2"));
    metrics.set("a_is_god", a_is_god);

    // 2. Refusals: B holds no powers anywhere.
    let (a, b, c) = (census.scene.a, census.scene.b, census.scene.c);
    let b_session = census
        .b
        .session_id()
        .ok_or_else(|| TestFailure::State("no session id".to_owned()))?;
    census
        .leg(
            metrics,
            "refuse_freeze_udp",
            Who::B,
            vec![Command::Send {
                message: Box::new(raw_god_kick(b, b_session, a, GodKickAction::Freeze)),
                reliability: Reliability::Reliable,
            }],
        )
        .await?;
    census
        .post_leg(
            metrics,
            "refuse_freeze_cap",
            Who::B,
            untrusted_god_kick(b, b_session, a, GodKickAction::Freeze),
        )
        .await?;
    census
        .post_leg(
            metrics,
            "refuse_freeze_cap_as_a",
            Who::B,
            untrusted_god_kick(a, b_session, c, GodKickAction::Freeze),
        )
        .await?;
    census
        .post_leg(
            metrics,
            "refuse_other_message",
            Who::B,
            "<llsd><map><key>message</key><string>CensusNoSuchMessage</string>\
             <key>body</key><map /></map></llsd>"
                .to_owned(),
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_godlike_message",
            Who::B,
            vec![Command::SendGodlikeMessage {
                method: GODLIKE_METHOD.to_owned(),
                params: vec![GODLIKE_PARAMETER.to_owned()],
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_region_info",
            Who::B,
            vec![Command::GodUpdateRegionInfo {
                update: update.clone(),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_sim_wide_deletes",
            Who::B,
            vec![Command::SimWideDeletes {
                owner: AgentKey::from(a),
                flags: SimWideDeleteFlags {
                    others_land_only: false,
                    always_return_objects: true,
                    scripted_only: true,
                },
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_parcel_owner",
            Who::B,
            vec![Command::ParcelGodForceOwner {
                parcel: scoped_parcel_of(census.b, &parcel)?,
                owner: OwnerKey::Agent(AgentKey::from(b)),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "refuse_mark_content",
            Who::B,
            vec![Command::ParcelGodMarkAsContent {
                parcel: scoped_parcel_of(census.b, &parcel)?,
            }],
        )
        .await?;
    let cube_in_b = scoped_object_of(census.b, &rezzed)?;
    census
        .leg(
            metrics,
            "refuse_object_owner",
            Who::B,
            vec![Command::ForceObjectOwner {
                local_ids: vec![cube_in_b],
                owner: AgentKey::from(b),
                group_id: None,
            }],
        )
        .await?;
    let owner = census.cube_owner().await?;
    metrics.set("cube_owner_after_refusal", owner);

    // 3. Out of god mode: A has given its powers up.
    census
        .leg(
            metrics,
            "latent_freeze",
            Who::A,
            vec![Command::GodKickUser {
                target: GodKickTarget::Agent(AgentKey::from(b)),
                action: GodKickAction::Freeze,
                reason: REASON.to_owned(),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "latent_object_owner",
            Who::A,
            vec![Command::ForceObjectOwner {
                local_ids: vec![rezzed.scoped_id()],
                owner: AgentKey::from(b),
                group_id: None,
            }],
        )
        .await?;
    let owner = census.cube_owner().await?;
    metrics.set("cube_owner_after_latent", owner);

    if !a_is_god {
        let words = if is_opensim(grid) {
            "not measured: A is no god here"
        } else {
            "not measured: no god account on Second Life"
        };
        for key in GOD_LEGS {
            for side in ["told_a", "told_b", "told_c"] {
                metrics.set(&format!("{key}_{side}"), words);
            }
        }
        for key in GOD_READS {
            metrics.set(key, words);
        }
        return Ok(());
    }

    // 4. God mode.
    let a_session = census
        .a
        .session_id()
        .ok_or_else(|| TestFailure::State("no session id".to_owned()))?;
    census
        .leg(
            metrics,
            "god_on",
            Who::A,
            vec![Command::RequestGodlikePowers { godlike: true }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_freeze_udp",
            Who::A,
            vec![Command::Send {
                message: Box::new(raw_god_kick(a, a_session, b, GodKickAction::Freeze)),
                reliability: Reliability::Reliable,
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_unfreeze_udp",
            Who::A,
            vec![Command::Send {
                message: Box::new(raw_god_kick(a, a_session, b, GodKickAction::Unfreeze)),
                reliability: Reliability::Reliable,
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_freeze",
            Who::A,
            vec![Command::GodKickUser {
                target: GodKickTarget::Agent(AgentKey::from(b)),
                action: GodKickAction::Freeze,
                reason: REASON.to_owned(),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_unfreeze",
            Who::A,
            vec![Command::GodKickUser {
                target: GodKickTarget::Agent(AgentKey::from(b)),
                action: GodKickAction::Unfreeze,
                reason: REASON.to_owned(),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_kick",
            Who::A,
            vec![Command::GodKickUser {
                target: GodKickTarget::Agent(AgentKey::from(b)),
                action: GodKickAction::Kick,
                reason: REASON.to_owned(),
            }],
        )
        .await?;
    census.bring_back(metrics, "god_kick_b_was", Who::B).await?;
    census
        .leg(
            metrics,
            "god_wars_on",
            Who::C,
            vec![Command::RequestGodlikePowers { godlike: true }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_wars",
            Who::C,
            vec![Command::GodKickUser {
                target: GodKickTarget::Agent(AgentKey::from(a)),
                action: GodKickAction::Kick,
                reason: REASON.to_owned(),
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_wars_off",
            Who::C,
            vec![Command::RequestGodlikePowers { godlike: false }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_kick_all",
            Who::A,
            vec![Command::GodKickUser {
                target: GodKickTarget::Everyone,
                action: GodKickAction::Kick,
                reason: REASON.to_owned(),
            }],
        )
        .await?;
    census
        .bring_back(metrics, "god_kick_all_b_was", Who::B)
        .await?;
    census
        .bring_back(metrics, "god_kick_all_c_was", Who::C)
        .await?;
    census
        .leg(
            metrics,
            "god_godlike_message",
            Who::A,
            vec![Command::SendGodlikeMessage {
                method: GODLIKE_METHOD.to_owned(),
                params: vec![GODLIKE_PARAMETER.to_owned()],
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_region_info",
            Who::A,
            vec![Command::GodUpdateRegionInfo { update }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_sim_wide_deletes",
            Who::A,
            vec![Command::SimWideDeletes {
                owner: AgentKey::from(b),
                flags: SimWideDeleteFlags {
                    others_land_only: false,
                    always_return_objects: true,
                    scripted_only: true,
                },
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_parcel_owner",
            Who::A,
            vec![Command::ParcelGodForceOwner {
                parcel: scoped_parcel,
                owner: OwnerKey::Agent(AgentKey::from(a)),
            }],
        )
        .await?;
    let after = census.parcel().await?;
    metrics.set(
        "parcel_owner_after_force",
        census.scene.whose(after.owner.uuid()),
    );
    census
        .leg(
            metrics,
            "god_parcel_owner_back",
            Who::A,
            vec![Command::ParcelGodForceOwner {
                parcel: scoped_parcel,
                owner: parcel.owner,
            }],
        )
        .await?;
    let after = census.parcel().await?;
    metrics.set(
        "parcel_owner_after_force_back",
        census.scene.whose(after.owner.uuid()),
    );
    if after.owner == parcel.owner {
        *parcel_owner = None;
    }
    census
        .leg(
            metrics,
            "god_mark_content",
            Who::A,
            vec![Command::ParcelGodMarkAsContent {
                parcel: scoped_parcel,
            }],
        )
        .await?;
    census
        .leg(
            metrics,
            "god_object_owner",
            Who::A,
            vec![Command::ForceObjectOwner {
                local_ids: vec![rezzed.scoped_id()],
                owner: AgentKey::from(b),
                group_id: None,
            }],
        )
        .await?;
    let owner = census.cube_owner().await?;
    metrics.set("cube_owner_after_force", owner);
    census
        .leg(
            metrics,
            "god_object_owner_back",
            Who::A,
            vec![Command::ForceObjectOwner {
                local_ids: vec![rezzed.scoped_id()],
                owner: AgentKey::from(a),
                group_id: None,
            }],
        )
        .await?;
    let owner = census.cube_owner().await?;
    metrics.set("cube_owner_after_force_back", owner);
    Ok(())
}

/// The parcel `parcel` (read by A) as `session`'s root circuit names it.
fn scoped_parcel_of(session: &Session, parcel: &ParcelInfo) -> Result<ScopedParcelId, TestFailure> {
    let circuit = session
        .circuit_id()
        .ok_or_else(|| TestFailure::State("no root circuit".to_owned()))?;
    Ok(ScopedParcelId::new(circuit, parcel.local_id))
}

/// The object `object` (seen by A) as `session`'s root circuit names it: the
/// region-local id is the region's, the circuit `session`'s own.
fn scoped_object_of(
    session: &Session,
    object: &sl_client_tokio::Object,
) -> Result<ScopedObjectId, TestFailure> {
    let circuit = session
        .circuit_id()
        .ok_or_else(|| TestFailure::State("no root circuit".to_owned()))?;
    Ok(ScopedObjectId::new(circuit, object.local_id))
}

/// Leaves the region as the census found it: nobody in god mode, the parcel
/// with its owner and the cube in A's Trash.
async fn clean_up(
    census: &mut Census<'_>,
    cube: Option<ScopedObjectId>,
    parcel_owner: Option<OwnerKey>,
    trash: InventoryFolderKey,
) -> Result<(), TestFailure> {
    if census.a.is_connected() {
        census
            .a
            .send(Command::RequestGodlikePowers { godlike: true })
            .await?;
    }
    if let Some(owner) = parcel_owner
        && census.a.is_connected()
    {
        // The parcel was forced away and not back: force it back while A is
        // still a god.
        let parcel = census.parcel().await?;
        if parcel.owner != owner {
            let scoped = scoped_parcel_of(census.a, &parcel)?;
            census
                .a
                .send(Command::ParcelGodForceOwner {
                    parcel: scoped,
                    owner,
                })
                .await?;
        }
    }
    if let Some(cube) = cube
        && census.a.is_connected()
    {
        census
            .a
            .send(Command::ForceObjectOwner {
                local_ids: vec![cube],
                owner: AgentKey::from(census.scene.a),
                group_id: None,
            })
            .await?;
    }
    for who in [Who::A, Who::C] {
        let session = census.session(who);
        if session.is_connected() {
            session
                .send(Command::RequestGodlikePowers { godlike: false })
                .await?;
        }
    }
    census.settle().await?;
    let Some(cube) = cube else {
        return Ok(());
    };
    if !census.a.is_connected() {
        return Err(TestFailure::State(
            "the cube was left standing: A was gone".to_owned(),
        ));
    }
    census
        .a
        .send(Command::DerezObjects {
            local_ids: vec![cube],
            destination: DeRezDestination::Trash(trash),
            transaction_id: TransactionId::from(Uuid::new_v4()),
            group_id: None,
        })
        .await?;
    let gone = census
        .a
        .wait_for(REZ, |event| match event {
            Event::ObjectRemoved { local_id, .. } if *local_id == cube => Some(()),
            _ => None,
        })
        .await;
    match gone {
        Ok(()) => Ok(()),
        Err(TestFailure::Timeout(_)) => Err(TestFailure::Assertion(
            "the cube was never removed on clean-up".to_owned(),
        )),
        Err(other) => Err(other),
    }
}
