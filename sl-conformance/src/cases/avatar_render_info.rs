//! A census of the two capabilities a region states an avatar's cost through:
//! `AvatarRenderInfo`, which the viewers of a region report each other's
//! complexity to and read it back from, and `AttachmentResources`, which tells
//! an agent what the scripts it wears use.
//!
//! Avatar **A** is reported on; **B** stands beside it and reports. The
//! render-info legs speak the capability raw, the way the reference's
//! `llavatarrenderinfoaccountant.cpp` does — a GET of
//! `{agents:{<id>:{weight}}, reportinglimit, overlimit}`, a POST of
//! `{agents:{<id>:{weight, tooComplex}}}` — so every answer is read as the
//! grid sent it, not as our client makes of it. The legs, in order:
//!
//! 1. **The grant**: whether each capability is in A's seed.
//! 2. **Attachment resources, unscripted**: what A is told of what it wears
//!    already (the aditi avatars wear a scripted animation overrider), raw and
//!    as our client reads it.
//! 3. **A scripted attachment**: a cube with a script that asks for a URL is
//!    attached from the world, and the report read again; then taken off and
//!    read a third time.
//! 4. **The move**: on Second Life both avatars teleport to a region nobody
//!    else is in, since the build location is a sandbox whose residents'
//!    viewers report on A and B as well.
//! 5. **The first read**: A's GET before the census has reported anything.
//! 6. **A report**: B reports A at a weight nobody else would state, and A
//!    reads until its entry carries that weight (or a window passes); B reads
//!    once too.
//! 7. **A report of oneself**: A reports its own weight, and reads until its
//!    entry changes.
//! 8. **Too complex**: B reports A again, too complex at another weight, and
//!    A reads until its weight or the over-limit count moves.
//! 9. **Refusals**: B reports an agent nobody is, an empty map, a body that
//!    is no map, an entry that is no map; and the methods the capability has
//!    no use for (`PUT`, `DELETE`).
//!
//! Every weight is named by the report that sent it (or as the mean of the
//! census's reports), never by its number, and every id by whose it is, so a
//! metric reads the same in every run.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/avatars.md` § Render info; every run is held to it.
//!
//! `2av`, live grids and both fake flavours. The fake grid runs no scripts and
//! is no sandbox, so there the scripted cube's legs and the move are left out,
//! and what only a live grid states (`MEASURED_LIVE`) is not held. The cube
//! is derezzed to the Trash and its item removed, also when a leg fails.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use sl_client_tokio::{
    AgentKey, AttachmentMode, AttachmentPoint, Command, Event, InventoryKey, InventoryType, Object,
    RestoreItem, RezScriptParams, Rotation, ScriptTarget, ScriptUploadLocation, Uuid, Vector,
};
use sl_wire::{AttachmentResourcesReport, Llsd, ResourceAmount};

use crate::cases::asset_http::without_ids;
use crate::cases::object_rez_derez::{
    Answer, Folders, Pace, REZ_LIFT_M, Residue, Run, SETTLE_WINDOW, clean_up, listen, rez_cube,
};
use crate::cases::task_inventory::{hold_to_measured, listing, login_folders};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim};
use crate::teleport_trace::{region_named, request_teleport, watch_teleport};

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/avatars.md § Render info (avatar-render-info, 2026-10-21)";

/// Where both avatars log in on OpenSim: the level square in the block's
/// north-eastern region, where a cube may be rezzed.
const OPENSIM_START: &str = "uri:Northeast Region&70&180&26";

/// The render-info capability.
const RENDER_INFO: &str = "AvatarRenderInfo";

/// The attachment-resources capability.
const ATTACHMENT_RESOURCES: &str = "AttachmentResources";

/// What a raw answer reads on a grid that grants no capability.
const NO_CAPABILITY: &str = "no capability";

/// How long one request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a leg reads the render info for its report to show.
const REPORT_WINDOW: Duration = Duration::from_secs(90);

/// How long between two reads of a leg: the reference reads every 15 s.
const READ_EVERY: Duration = Duration::from_secs(5);

/// A region on Second Life with nobody in it but the census: `Ahern`, which
/// `avatar-presence` measures in, and where the avatars stand.
const QUIET_REGION: &str = "Ahern";

/// Where in [`QUIET_REGION`] A stands; B stands two metres east.
const QUIET_X: f32 = 128.0;

/// See [`QUIET_X`].
const QUIET_Y: f32 = 128.0;

/// See [`QUIET_X`].
const QUIET_Z: f32 = 40.0;

/// How long a teleport, and the capabilities after it, may take.
const TELEPORT: Duration = Duration::from_secs(60);

/// How long a step of the scripted leg may take.
const STEP: Duration = Duration::from_secs(30);

/// How many characters of a body that is no LLSD the census keeps.
const BODY_CHARS: usize = 120;

/// The weight B reports A at first.
const FIRST_WEIGHT: i32 = 123_457;

/// The weight B reports A at as too complex.
const TOO_COMPLEX_WEIGHT: i32 = 234_571;

/// The weight A reports itself at.
const OWN_WEIGHT: i32 = 345_679;

/// The weight B reports an agent nobody is at.
const NOBODY_WEIGHT: i32 = 456_781;

/// What the scripted cube is called.
const CUBE_NAME: &str = "SLClientRenderInfoCube";

/// The name of the probe script.
const PROBE_NAME: &str = "SLClientRenderInfoProbe";

/// What every line the probe says begins with.
const PROBE_TAG: &str = "SLCRI";

/// Where the scripted cube is worn. `Add`, so nothing worn there is replaced.
const CUBE_POINT: AttachmentPoint = AttachmentPoint::RightPec;

/// How far beside A the cube stands, in metres.
const CUBE_AWAY_M: f32 = 2.0;

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
/// under. OpenSim grants no `AvatarRenderInfo`, so every render-info metric
/// reads `no capability` (or `absent`) there. On Second Life a few answers are
/// not the census's to hold ([`Answer::Any`]): how long a report took to show;
/// the weight a region states once anything but one report stands behind it,
/// which is the region's own reckoning (B appears in it half a minute in,
/// reported by nobody of ours); and everything the test avatar already wears
/// adds to the attachment report.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("attachment_resources_granted", both(Answer::Flag(true))),
    (
        "delete_body",
        each(
            Answer::Text("not LLSD \"Method Not Allowed\""),
            Answer::Text("no capability"),
        ),
    ),
    (
        "delete_content_type",
        each(Answer::Text("text/plain"), Answer::Text("no capability")),
    ),
    (
        "delete_status",
        each(Answer::Text("405"), Answer::Text("no capability")),
    ),
    (
        "first_read_agents",
        each(Answer::Text("none"), Answer::Text("absent")),
    ),
    (
        "first_read_body",
        each(
            Answer::Text("{agents:{},overlimit:integer,reportinglimit:integer}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "first_read_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "first_read_overlimit",
        each(Answer::Text("0"), Answer::Text("absent")),
    ),
    (
        "first_read_reportinglimit",
        each(Answer::Text("0"), Answer::Text("absent")),
    ),
    (
        "first_read_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    ("first_read_weight_a", both(Answer::Text("absent"))),
    ("first_read_weight_b", both(Answer::Text("absent"))),
    (
        "last_read_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "last_read_overlimit",
        each(Answer::Text("0"), Answer::Text("absent")),
    ),
    (
        "last_read_reportinglimit",
        each(Answer::Text("3"), Answer::Text("absent")),
    ),
    (
        "last_read_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "last_read_weight_a",
        each(Answer::Any, Answer::Text("absent")),
    ),
    (
        "last_read_weight_b",
        each(Answer::Any, Answer::Text("absent")),
    ),
    (
        "own_report_body",
        each(Answer::Text("{}"), Answer::Text("no capability")),
    ),
    (
        "own_report_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "own_report_read_agents",
        each(Answer::Text("A"), Answer::Text("absent")),
    ),
    (
        "own_report_read_body",
        each(
            Answer::Text("{agents:{A:{weight:integer}},overlimit:integer,reportinglimit:integer}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "own_report_read_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "own_report_read_overlimit",
        each(Answer::Text("0"), Answer::Text("absent")),
    ),
    (
        "own_report_read_reportinglimit",
        each(Answer::Text("2"), Answer::Text("absent")),
    ),
    (
        "own_report_read_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "own_report_read_weight_a",
        each(Answer::Any, Answer::Text("absent")),
    ),
    ("own_report_read_weight_b", both(Answer::Text("absent"))),
    (
        "own_report_shown_after",
        each(Answer::Any, Answer::Text("never")),
    ),
    (
        "own_report_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "put_body",
        each(
            Answer::Text("not LLSD \"Method Not Allowed\""),
            Answer::Text("no capability"),
        ),
    ),
    (
        "put_content_type",
        each(Answer::Text("text/plain"), Answer::Text("no capability")),
    ),
    (
        "put_status",
        each(Answer::Text("405"), Answer::Text("no capability")),
    ),
    (
        "render_info_granted",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "report_b_read_agents",
        each(Answer::Text("A"), Answer::Text("absent")),
    ),
    (
        "report_b_read_body",
        each(
            Answer::Text("{agents:{A:{weight:integer}},overlimit:integer,reportinglimit:integer}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "report_b_read_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "report_b_read_overlimit",
        each(Answer::Text("0"), Answer::Text("absent")),
    ),
    (
        "report_b_read_reportinglimit",
        each(Answer::Text("0"), Answer::Text("absent")),
    ),
    (
        "report_b_read_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "report_b_read_weight_a",
        each(Answer::Text("the first report's"), Answer::Text("absent")),
    ),
    ("report_b_read_weight_b", both(Answer::Text("absent"))),
    (
        "report_body",
        each(Answer::Text("{}"), Answer::Text("no capability")),
    ),
    (
        "report_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "report_empty_map_body",
        each(Answer::Text("{}"), Answer::Text("no capability")),
    ),
    (
        "report_empty_map_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "report_empty_map_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "report_entry_no_map_body",
        each(Answer::Text("{}"), Answer::Text("no capability")),
    ),
    (
        "report_entry_no_map_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "report_entry_no_map_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "report_no_map_body",
        each(Answer::Text("{}"), Answer::Text("no capability")),
    ),
    (
        "report_no_map_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "report_no_map_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "report_nobody_body",
        each(Answer::Text("{}"), Answer::Text("no capability")),
    ),
    (
        "report_nobody_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "report_nobody_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "report_read_agents",
        each(Answer::Text("A"), Answer::Text("absent")),
    ),
    (
        "report_read_body",
        each(
            Answer::Text("{agents:{A:{weight:integer}},overlimit:integer,reportinglimit:integer}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "report_read_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "report_read_overlimit",
        each(Answer::Text("0"), Answer::Text("absent")),
    ),
    (
        "report_read_reportinglimit",
        each(Answer::Text("1"), Answer::Text("absent")),
    ),
    (
        "report_read_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "report_read_weight_a",
        each(Answer::Text("the first report's"), Answer::Text("absent")),
    ),
    ("report_read_weight_b", both(Answer::Text("absent"))),
    (
        "report_shown_after",
        each(Answer::Any, Answer::Text("never")),
    ),
    (
        "report_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "too_complex_b_read_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "too_complex_b_read_overlimit",
        each(Answer::Text("0"), Answer::Text("absent")),
    ),
    (
        "too_complex_b_read_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "too_complex_b_read_weight_a",
        each(Answer::Any, Answer::Text("absent")),
    ),
    (
        "too_complex_b_read_weight_b",
        each(Answer::Any, Answer::Text("absent")),
    ),
    (
        "too_complex_body",
        each(Answer::Text("{}"), Answer::Text("no capability")),
    ),
    (
        "too_complex_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "too_complex_read_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "too_complex_read_overlimit",
        each(Answer::Text("0"), Answer::Text("absent")),
    ),
    (
        "too_complex_read_reportinglimit",
        each(Answer::Text("3"), Answer::Text("absent")),
    ),
    (
        "too_complex_read_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "too_complex_read_weight_a",
        each(Answer::Any, Answer::Text("absent")),
    ),
    (
        "too_complex_read_weight_b",
        each(Answer::Any, Answer::Text("absent")),
    ),
    (
        "too_complex_shown_after",
        each(Answer::Any, Answer::Text("never")),
    ),
    (
        "too_complex_status",
        each(Answer::Text("200"), Answer::Text("no capability")),
    ),
    (
        "worn_resources_body",
        each(
            Answer::Any,
            Answer::Text(
                "{attachments:[],summary:{available:[2 of {amount:integer,type:string}],used:[2 of {amount:integer,type:string}]}}",
            ),
        ),
    ),
    (
        "worn_resources_client",
        each(
            Answer::Any,
            Answer::Text("0 location(s) []; available memory=-1 urls=38; used memory=0 urls=0"),
        ),
    ),
    (
        "worn_resources_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("text/html;charset=UTF-8"),
        ),
    ),
    ("worn_resources_cube", both(Answer::Text("absent"))),
    ("worn_resources_status", both(Answer::Text("200"))),
    (
        "worn_resources_summary",
        each(
            Answer::Any,
            Answer::Text("available memory=-1 urls=38; used memory=0 urls=0"),
        ),
    ),
];

/// What only the live grids are held to: the scripted cube's legs, which the
/// fake grid cannot take (it runs no scripts), and what Second Life states of
/// its own reckoning — B in the statement, and a report of B nobody of ours
/// sent — which the fake grid has no reckoning to imitate
/// (`sl-fake-grid` `render_info`).
const MEASURED_LIVE: &[(&str, Measured<Answer>)] = &[
    (
        "detached_resources_body",
        each(
            Answer::Any,
            Answer::Text(
                "{attachments:[],summary:{available:[2 of {amount:integer,type:string}],used:[2 of {amount:integer,type:string}]}}",
            ),
        ),
    ),
    (
        "detached_resources_client",
        each(
            Answer::Any,
            Answer::Text("0 location(s) []; available memory=-1 urls=38; used memory=0 urls=0"),
        ),
    ),
    (
        "detached_resources_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("text/html;charset=UTF-8"),
        ),
    ),
    ("detached_resources_cube", both(Answer::Text("absent"))),
    ("detached_resources_status", both(Answer::Text("200"))),
    (
        "detached_resources_summary",
        each(
            Answer::Any,
            Answer::Text("available memory=-1 urls=38; used memory=0 urls=0"),
        ),
    ),
    (
        "last_read_agents",
        each(Answer::Text("A,B"), Answer::Text("absent")),
    ),
    (
        "last_read_body",
        each(
            Answer::Text(
                "{agents:{A:{weight:integer},B:{weight:integer}},overlimit:integer,reportinglimit:integer}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    ("scripted_attach_said", both(Answer::Text("A|1|30"))),
    ("scripted_attach_url", both(Answer::Text("nothing"))),
    ("scripted_attached", both(Answer::Flag(true))),
    ("scripted_detached", both(Answer::Flag(true))),
    (
        "scripted_resources_body",
        each(
            Answer::Any,
            Answer::Text(
                "{attachments:[1 of {location:string,objects:[1 of {id:uuid,is_group_owned:integer,location:[3 of real],name:string,owner_id:uuid,resources:{urls:integer}}]}],summary:{available:[2 of {amount:integer,type:string}],used:[2 of {amount:integer,type:string}]}}",
            ),
        ),
    ),
    (
        "scripted_resources_client",
        each(
            Answer::Any,
            Answer::Text(
                "1 location(s) [ATTACH_RIGHT_PEC: 1 (the cube: memory=absent urls=1)]; available memory=-1 urls=38; used memory=0 urls=1",
            ),
        ),
    ),
    (
        "scripted_resources_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("text/html;charset=UTF-8"),
        ),
    ),
    (
        "scripted_resources_cube",
        each(
            Answer::Text(
                "at \"ATTACH_RIGHT_PEC\", {id:uuid,is_group_owned:integer,location:[3 of real],name:string,owner_id:uuid,resources:{memory:integer,urls:integer}}, owner A, memory=65536 urls=1",
            ),
            Answer::Text(
                "at \"ATTACH_RIGHT_PEC\", {id:uuid,is_group_owned:integer,location:[3 of real],name:string,owner_id:uuid,resources:{urls:integer}}, owner A, memory=absent urls=1",
            ),
        ),
    ),
    ("scripted_resources_status", both(Answer::Text("200"))),
    (
        "scripted_resources_summary",
        each(
            Answer::Any,
            Answer::Text("available memory=-1 urls=38; used memory=0 urls=1"),
        ),
    ),
    ("scripted_rez_url", both(Answer::Text("granted"))),
    (
        "too_complex_b_read_agents",
        each(Answer::Text("A,B"), Answer::Text("absent")),
    ),
    (
        "too_complex_b_read_body",
        each(
            Answer::Text(
                "{agents:{A:{weight:integer},B:{weight:integer}},overlimit:integer,reportinglimit:integer}",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "too_complex_b_read_reportinglimit",
        each(Answer::Text("1"), Answer::Text("absent")),
    ),
    (
        "too_complex_read_agents",
        each(Answer::Text("A,B"), Answer::Text("absent")),
    ),
    (
        "too_complex_read_body",
        each(
            Answer::Text(
                "{agents:{A:{weight:integer},B:{weight:integer}},overlimit:integer,reportinglimit:integer}",
            ),
            Answer::Text("no capability"),
        ),
    ),
];

/// A census of `AvatarRenderInfo` and `AttachmentResources`.
#[derive(Debug)]
pub struct AvatarRenderInfo;

impl GridTest for AvatarRenderInfo {
    fn name(&self) -> &'static str {
        "avatar-render-info"
    }

    fn description(&self) -> &'static str {
        "Record what each grid answers a viewer's render-info reports and reads, and what it \
         tells an agent of the scripts it wears"
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
            let grid = ctx.grid();
            let folders = login_folders(ctx.primary()).await?;
            let mut metrics = Metrics::new();
            let mut left = Leftovers::default();
            let outcome = census(ctx, folders, &mut metrics, &mut left).await;
            ctx.metrics().merge(metrics.clone());
            // Take off and take away what is left, whatever became of the
            // census.
            let cleanup = tidy(ctx.primary(), &left, folders).await;
            outcome
                .and_then(|()| hold_to_measured(grid, &metrics, MEASURED, SOURCE))
                .and_then(|()| {
                    if grid.is_fake() {
                        Ok(())
                    } else {
                        hold_to_measured(grid, &metrics, MEASURED_LIVE, SOURCE)
                    }
                })
                .and(cleanup)
        })
    }
}

/// What the census leaves behind until it is taken away.
#[derive(Debug, Default)]
struct Leftovers {
    /// The cube while it stands, and the item it became.
    residue: Residue,
    /// The cube's item while it may still be worn.
    worn: Option<InventoryKey>,
}

/// Takes the cube off if it may still be worn, and takes away the cube and
/// its item.
async fn tidy(
    session: &mut Session,
    left: &Leftovers,
    folders: Folders,
) -> Result<(), TestFailure> {
    if let Some(item) = left.worn {
        session
            .send(Command::DetachAttachmentIntoInventory { item_id: item })
            .await?;
        let _heard = listen(session, STEP, Duration::from_secs(2), |_heard| false).await?;
    }
    clean_up(session, &left.residue, folders.trash).await
}

/// Who an id is, in the census's words.
#[derive(Debug, Clone, Copy)]
struct Cast {
    /// A.
    a: Uuid,
    /// B.
    b: Uuid,
    /// The agent nobody is.
    nobody: Uuid,
}

impl Cast {
    /// The census's word for `id`.
    fn word(&self, id: &str) -> String {
        match id.parse::<Uuid>() {
            Ok(id) if id == self.a => "A".to_owned(),
            Ok(id) if id == self.b => "B".to_owned(),
            Ok(id) if id == self.nobody => "nobody".to_owned(),
            Ok(_) => "another".to_owned(),
            Err(_) => format!("not an id \"{}\"", without_ids(id)),
        }
    }
}

/// The census's word for a weight a read states: the leg that sent it, or the
/// number when no leg did.
fn weight_word(weight: i64) -> String {
    let named = [
        (FIRST_WEIGHT, "the first report"),
        (TOO_COMPLEX_WEIGHT, "the too-complex report"),
        (OWN_WEIGHT, "the own report"),
    ];
    for (sent, word) in named
        .iter()
        .chain([(NOBODY_WEIGHT, "nobody's report")].iter())
    {
        if weight == i64::from(*sent) {
            return format!("{word}'s");
        }
    }
    // An aggregate of the census's own reports: the mean of two or of all
    // three, whichever way it is rounded.
    let mut means: Vec<(Vec<i64>, String)> = Vec::new();
    for (index, (first, first_word)) in named.iter().enumerate() {
        for (second, second_word) in named.iter().skip(index.saturating_add(1)) {
            means.push((
                vec![i64::from(*first), i64::from(*second)],
                format!("the mean of {first_word} and {second_word}"),
            ));
        }
    }
    means.push((
        named.iter().map(|(sent, _word)| i64::from(*sent)).collect(),
        "the mean of all three reports".to_owned(),
    ));
    for (values, word) in means {
        let sum: i64 = values.iter().sum();
        let count = i64::try_from(values.len()).unwrap_or(1);
        let floor = sum.div_euclid(count);
        if weight == floor || weight == floor.saturating_add(1) {
            return word;
        }
    }
    weight.to_string()
}

/// One raw answer of a capability.
#[derive(Debug, Clone)]
struct Raw {
    /// The status, or why there was none.
    status: Result<u16, String>,
    /// The media type, or `none`.
    content_type: String,
    /// The body as LLSD, when it was LLSD.
    body: Option<Llsd>,
    /// The body as text.
    text: String,
}

impl Raw {
    /// The status in a word: the number, or why there was none.
    fn status_word(&self) -> String {
        match &self.status {
            Ok(status) => status.to_string(),
            Err(reason) => reason.clone(),
        }
    }

    /// The body in words: its LLSD shape with every agent named by role, or
    /// the first words of what it was.
    fn body_word(&self, cast: &Cast) -> String {
        if self.status.as_ref().err().map(String::as_str) == Some(NO_CAPABILITY) {
            return NO_CAPABILITY.to_owned();
        }
        match &self.body {
            Some(body) => shape(body, cast),
            None => {
                let words: Vec<&str> = self.text.split_whitespace().collect();
                if words.is_empty() {
                    "empty".to_owned()
                } else {
                    let joined = words.join(" ");
                    format!(
                        "not LLSD \"{}\"",
                        without_ids(&joined.chars().take(BODY_CHARS).collect::<String>())
                    )
                }
            }
        }
    }

    /// Records the answer under `prefix`: `_status`, `_content_type` and
    /// `_body`.
    fn record(&self, metrics: &mut Metrics, prefix: &str, cast: &Cast) {
        metrics.set(&format!("{prefix}_status"), self.status_word());
        metrics.set(&format!("{prefix}_content_type"), self.content_type.clone());
        metrics.set(&format!("{prefix}_body"), self.body_word(cast));
    }
}

/// The shape of an LLSD value with every map key that is an agent's id named
/// by role: the kind of a scalar, a map's entries in key order, an array's
/// length and its first element.
fn shape(value: &Llsd, cast: &Cast) -> String {
    match value {
        Llsd::Map(map) => {
            let mut entries: Vec<(String, &Llsd)> = map
                .iter()
                .map(|(key, value)| {
                    let key = if key.parse::<Uuid>().is_ok() {
                        cast.word(key)
                    } else {
                        key.clone()
                    };
                    (key, value)
                })
                .collect();
            entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            let entries: Vec<String> = entries
                .into_iter()
                .map(|(key, value)| format!("{key}:{}", shape(value, cast)))
                .collect();
            format!("{{{}}}", entries.join(","))
        }
        Llsd::Array(items) => match items.first() {
            Some(first) => format!("[{} of {}]", items.len(), shape(first, cast)),
            None => "[]".to_owned(),
        },
        Llsd::Undef => "undef".to_owned(),
        Llsd::Boolean(_) => "boolean".to_owned(),
        Llsd::Integer(_) => "integer".to_owned(),
        Llsd::Real(_) => "real".to_owned(),
        Llsd::String(_) => "string".to_owned(),
        Llsd::Uuid(_) => "uuid".to_owned(),
        Llsd::Binary(_) => "binary".to_owned(),
        Llsd::Date(_) => "date".to_owned(),
        Llsd::Uri(_) => "uri".to_owned(),
    }
}

/// Asks `url` raw with `method`, sending `body` as LLSD when there is one.
async fn ask(
    http: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
    body: Option<String>,
) -> Raw {
    let mut request = http
        .request(method, url)
        .header(ACCEPT, "application/llsd+xml");
    if let Some(body) = body {
        request = request
            .header(CONTENT_TYPE, "application/llsd+xml")
            .body(body);
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map_or_else(|| "none".to_owned(), without_ids);
            let text = response.text().await.unwrap_or_default();
            let body = text
                .contains("<llsd")
                .then(|| sl_wire::parse_llsd_xml(&text).ok())
                .flatten();
            Raw {
                status: Ok(status),
                content_type,
                body,
                text,
            }
        }
        Err(error) => Raw {
            status: Err(format!("no answer: {}", without_ids(&error.to_string()))),
            content_type: "none".to_owned(),
            body: None,
            text: String::new(),
        },
    }
}

/// What one render-info read stated.
#[derive(Debug, Clone)]
struct Read {
    /// The raw answer.
    raw: Raw,
}

impl Read {
    /// The `agents` map's entries, each named by role, in order.
    fn agents(&self, cast: &Cast) -> String {
        match self.raw.body.as_ref().and_then(|body| body.get("agents")) {
            None => "absent".to_owned(),
            Some(Llsd::Map(map)) if map.is_empty() => "none".to_owned(),
            Some(Llsd::Map(map)) => {
                let mut words: Vec<String> = map.keys().map(|key| cast.word(key)).collect();
                let others = words.iter().filter(|word| *word == "another").count();
                words.retain(|word| word != "another");
                words.sort_unstable();
                if others > 0 {
                    words.push(format!("{others} other(s)"));
                }
                if words.is_empty() {
                    "none".to_owned()
                } else {
                    words.join(",")
                }
            }
            Some(_) => "not a map".to_owned(),
        }
    }

    /// The weight the `agents` map states for `agent`, if it states one.
    fn weight_of(&self, agent: Uuid) -> Option<i64> {
        let Some(Llsd::Map(agents)) = self.raw.body.as_ref().and_then(|body| body.get("agents"))
        else {
            return None;
        };
        agents
            .iter()
            .find(|(key, _value)| key.parse::<Uuid>().ok() == Some(agent))
            .and_then(|(_key, value)| value.get("weight"))
            .and_then(integer)
    }

    /// The top-level integer under `key`, if there is one.
    fn number(&self, key: &str) -> Option<i64> {
        self.raw
            .body
            .as_ref()
            .and_then(|body| body.get(key))
            .and_then(integer)
    }

    /// Records what the read stated under `prefix`: the whole answer, whom it
    /// lists, A's and B's weights and the region's two limits.
    fn record(&self, metrics: &mut Metrics, prefix: &str, cast: &Cast) {
        self.raw.record(metrics, prefix, cast);
        metrics.set(&format!("{prefix}_agents"), self.agents(cast));
        for (who, id) in [("a", cast.a), ("b", cast.b)] {
            metrics.set(
                &format!("{prefix}_weight_{who}"),
                self.weight_of(id)
                    .map_or_else(|| "absent".to_owned(), weight_word),
            );
        }
        for key in ["reportinglimit", "overlimit"] {
            metrics.set(
                &format!("{prefix}_{key}"),
                self.number(key)
                    .map_or_else(|| "absent".to_owned(), |number| number.to_string()),
            );
        }
    }
}

/// An LLSD value read as an integer, as the reference's `asInteger` reads a
/// weight: an integer, or a real cut off.
fn integer(value: &Llsd) -> Option<i64> {
    match value {
        Llsd::Integer(number) => Some(i64::from(*number)),
        Llsd::Real(number) => format!("{:.0}", number.trunc()).parse().ok(),
        Llsd::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// The LLSD-XML of a render-info report: one entry per `(agent, weight,
/// too_complex)`.
fn report_body(entries: &[(Uuid, i32, bool)]) -> String {
    let mut agents = String::new();
    for (agent, weight, too_complex) in entries {
        let _infallible = write!(
            agents,
            "<key>{agent}</key><map><key>weight</key><integer>{weight}</integer>\
             <key>tooComplex</key><boolean>{}</boolean></map>",
            if *too_complex { "true" } else { "false" }
        );
    }
    format!("<?xml version=\"1.0\" ?><llsd><map><key>agents</key><map>{agents}</map></map></llsd>")
}

/// The render-info capability of each avatar, and how to ask it.
struct RenderInfo<'h> {
    /// The client every request goes out on.
    http: &'h reqwest::Client,
    /// A's capability, if granted.
    a: Option<String>,
    /// B's capability, if granted.
    b: Option<String>,
}

impl RenderInfo<'_> {
    /// A read with the capability of `url`, or the stand-in of a grid that
    /// grants none.
    async fn read(&self, url: Option<&str>) -> Read {
        Read {
            raw: match url {
                Some(url) => ask(self.http, reqwest::Method::GET, url, None).await,
                None => no_capability(),
            },
        }
    }

    /// Sends `body` with `method` to the capability of `url`.
    async fn send(&self, url: Option<&str>, method: reqwest::Method, body: Option<String>) -> Raw {
        match url {
            Some(url) => ask(self.http, method, url, body).await,
            None => no_capability(),
        }
    }

    /// Reads with A's capability every [`READ_EVERY`] until `shown` says the
    /// read shows what the leg sent, or [`REPORT_WINDOW`] passes. Returns the
    /// last read and when it came, or `None` for the time when nothing showed.
    async fn read_until(&self, mut shown: impl FnMut(&Read) -> bool) -> (Read, Option<Duration>) {
        let started = Instant::now();
        loop {
            let read = self.read(self.a.as_deref()).await;
            if shown(&read) {
                return (read, Some(started.elapsed()));
            }
            if self.a.is_none() || started.elapsed() >= REPORT_WINDOW {
                return (read, None);
            }
            tokio::time::sleep(READ_EVERY).await;
        }
    }
}

/// A raw answer on a grid that grants no capability.
fn no_capability() -> Raw {
    Raw {
        status: Err(NO_CAPABILITY.to_owned()),
        content_type: NO_CAPABILITY.to_owned(),
        body: None,
        text: NO_CAPABILITY.to_owned(),
    }
}

/// Records how long a report took to show under `key`, or that it never did.
fn record_shown(metrics: &mut Metrics, key: &str, after: Option<Duration>) {
    match after {
        Some(after) => metrics.set_timing(key, after.as_secs_f64()),
        None => metrics.set(key, "never"),
    }
}

/// The census: both avatars settled beside each other, then every leg.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of legs, each a request and the reads it is answered by"
)]
async fn census(
    ctx: &mut TestContext,
    folders: Folders,
    metrics: &mut Metrics,
    left: &mut Leftovers,
) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let pace = Pace::of(grid);
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "avatar-render-info needs a secondary avatar".to_owned(),
        ));
    };
    let owner = a
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let b_id = b
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    let settled = crate::support::settle_scene_with_avatar(
        a,
        grid,
        build_position.clone(),
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    let _theirs = crate::support::settle_scene_with_avatar(
        b,
        grid,
        build_position,
        SETTLE_WINDOW,
        pace.settle_idle,
    )
    .await?;
    let cast = Cast {
        a: owner.uuid(),
        b: b_id.uuid(),
        nobody: Uuid::new_v4(),
    };
    let http = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| TestFailure::State(format!("building an HTTP client failed: {error}")))?;

    // --- 1. The grant. ---
    metrics.set("render_info_granted", a.cap(RENDER_INFO).is_some());
    let resources = a.cap(ATTACHMENT_RESOURCES);
    metrics.set("attachment_resources_granted", resources.is_some());

    // --- 7. Attachment resources, unscripted. ---
    resources_leg(a, &http, resources.as_deref(), metrics, "worn", &cast).await?;

    // --- 8. A scripted attachment, where scripts run: the fake grid runs
    // none. ---
    if !grid.is_fake() {
        let anchor = settled
            .avatar
            .clone()
            .or_else(|| settled.anchor.clone())
            .ok_or_else(|| {
                TestFailure::Assertion(
                    "the arrival streamed nothing to place a cube against".to_owned(),
                )
            })?;
        let mut run = Run {
            folders,
            owner,
            pace,
            position: Vector {
                x: anchor.x + CUBE_AWAY_M,
                y: anchor.y,
                z: anchor.z + REZ_LIFT_M,
            },
            seen: settled.seen,
        };
        let cube = scripted_cube(a, &mut run, &mut left.residue, owner, metrics).await?;
        a.send(Command::AttachObject {
            local_id: cube.scoped_id(),
            attachment_point: CUBE_POINT,
            mode: AttachmentMode::Add,
            rotation: Rotation {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                s: 1.0,
            },
        })
        .await?;
        let heard = listen(a, STEP, pace.quiet, |heard| {
            heard
                .added
                .iter()
                .chain(heard.changed.iter())
                .any(|object| worn_cube(object, &cube))
                && said(&heard.chats, "A|")
        })
        .await?;
        let worn = heard
            .added
            .iter()
            .chain(heard.changed.iter())
            .rev()
            .find(|object| worn_cube(object, &cube))
            .cloned();
        metrics.set("scripted_attached", worn.is_some());
        metrics.set("scripted_attach_said", said_line(&heard.chats, "A|"));
        metrics.set("scripted_attach_url", said_url(&heard.chats));
        for item in heard.items() {
            left.residue.items.push(item.item_id);
        }
        let Some(worn) = worn else {
            return Err(TestFailure::Assertion(
                "the scripted cube was never worn".to_owned(),
            ));
        };
        left.residue.roots.retain(|root| *root != cube.scoped_id());
        left.worn = worn
            .name_value_data("AttachItemID")
            .and_then(|value| value.trim().parse::<Uuid>().ok())
            .map(InventoryKey::from);
        resources_leg(a, &http, resources.as_deref(), metrics, "scripted", &cast).await?;
        a.send(Command::DetachObjects {
            local_ids: vec![worn.scoped_id()],
        })
        .await?;
        let heard = listen(a, STEP, pace.quiet, |heard| heard.killed(worn.scoped_id())).await?;
        metrics.set("scripted_detached", heard.killed(worn.scoped_id()));
        if heard.killed(worn.scoped_id()) {
            left.worn = None;
        }
        resources_leg(a, &http, resources.as_deref(), metrics, "detached", &cast).await?;
    }

    // The render-info legs, where no other viewer reports either avatar.
    if !is_opensim(grid) && !grid.is_fake() {
        move_to_quiet_region(a, b).await?;
    }
    render_info_legs(a, b, &http, &cast, metrics).await;
    Ok(())
}

/// Takes both avatars to [`QUIET_REGION`] and waits until each holds that
/// region's render-info capability: the build location is a sandbox full of
/// residents whose viewers report on A and B too, which makes every weight a
/// region states an aggregate of theirs.
async fn move_to_quiet_region(a: &mut Session, b: &mut Session) -> Result<(), TestFailure> {
    let region = region_named(a, QUIET_REGION).await?;
    for (session, x) in [(a, QUIET_X), (b, QUIET_X + 2.0)] {
        let before = session.cap(RENDER_INFO);
        request_teleport(
            session,
            region.region_handle,
            (x, QUIET_Y, QUIET_Z),
            (x, QUIET_Y + 1.0, QUIET_Z),
        )
        .await?;
        let trace = watch_teleport(session, TELEPORT).await?;
        if let Some(failure) = trace.failure {
            return Err(TestFailure::Assertion(format!(
                "the teleport to {QUIET_REGION} failed: {}",
                failure.reason
            )));
        }
        let started = Instant::now();
        while session.cap(RENDER_INFO).is_none() || session.cap(RENDER_INFO) == before {
            if started.elapsed() >= TELEPORT {
                return Err(TestFailure::Assertion(format!(
                    "{QUIET_REGION} never granted {RENDER_INFO}"
                )));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    Ok(())
}

/// The render-info legs, each a request and the reads it is answered by. A
/// grid that grants no capability records the stand-in for each.
async fn render_info_legs(
    a: &Session,
    b: &Session,
    http: &reqwest::Client,
    cast: &Cast,
    metrics: &mut Metrics,
) {
    let info = RenderInfo {
        http,
        a: a.cap(RENDER_INFO),
        b: b.cap(RENDER_INFO),
    };

    // --- 2. The first read. ---
    let first = info.read(info.a.as_deref()).await;
    first.record(metrics, "first_read", cast);

    // --- 3. A report. ---
    let report = info
        .send(
            info.b.as_deref(),
            reqwest::Method::POST,
            Some(report_body(&[(cast.a, FIRST_WEIGHT, false)])),
        )
        .await;
    report.record(metrics, "report", cast);
    let (read, after) = info
        .read_until(|read| read.weight_of(cast.a) == Some(i64::from(FIRST_WEIGHT)))
        .await;
    read.record(metrics, "report_read", cast);
    record_shown(metrics, "report_shown_after", after);
    let theirs = info.read(info.b.as_deref()).await;
    theirs.record(metrics, "report_b_read", cast);

    // --- 4. A report of oneself. ---
    let before = read.weight_of(cast.a);
    let own = info
        .send(
            info.a.as_deref(),
            reqwest::Method::POST,
            Some(report_body(&[(cast.a, OWN_WEIGHT, false)])),
        )
        .await;
    own.record(metrics, "own_report", cast);
    let (read, after) = info
        .read_until(|read| read.weight_of(cast.a) != before)
        .await;
    read.record(metrics, "own_report_read", cast);
    record_shown(metrics, "own_report_shown_after", after);

    // --- 5. Too complex. ---
    let before = (read.weight_of(cast.a), read.number("overlimit"));
    let too_complex = info
        .send(
            info.b.as_deref(),
            reqwest::Method::POST,
            Some(report_body(&[(cast.a, TOO_COMPLEX_WEIGHT, true)])),
        )
        .await;
    too_complex.record(metrics, "too_complex", cast);
    let (read, after) = info
        .read_until(|read| (read.weight_of(cast.a), read.number("overlimit")) != before)
        .await;
    read.record(metrics, "too_complex_read", cast);
    record_shown(metrics, "too_complex_shown_after", after);
    let theirs = info.read(info.b.as_deref()).await;
    theirs.record(metrics, "too_complex_b_read", cast);

    // --- 6. Refusals. ---
    let refusals = [
        (
            "report_nobody",
            reqwest::Method::POST,
            Some(report_body(&[(cast.nobody, NOBODY_WEIGHT, false)])),
        ),
        (
            "report_empty_map",
            reqwest::Method::POST,
            Some("<?xml version=\"1.0\" ?><llsd><map /></llsd>".to_owned()),
        ),
        (
            "report_no_map",
            reqwest::Method::POST,
            Some("<?xml version=\"1.0\" ?><llsd><string>census</string></llsd>".to_owned()),
        ),
        (
            "report_entry_no_map",
            reqwest::Method::POST,
            Some(format!(
                "<?xml version=\"1.0\" ?><llsd><map><key>agents</key><map><key>{}</key>\
                 <string>census</string></map></map></llsd>",
                cast.a
            )),
        ),
        ("put", reqwest::Method::PUT, Some(report_body(&[]))),
        ("delete", reqwest::Method::DELETE, None),
    ];
    for (prefix, method, body) in refusals {
        let raw = info.send(info.b.as_deref(), method, body).await;
        raw.record(metrics, prefix, cast);
    }
    let last = info.read(info.a.as_deref()).await;
    last.record(metrics, "last_read", cast);
}

/// Whether `object` is `cube` worn: both grids send an object attached from
/// the world again under its own ids, now with a parent (`attachments`).
fn worn_cube(object: &Object, cube: &Object) -> bool {
    object.parent_id.0 != 0 && object.full_id == cube.full_id
}

/// Whether the probe said a line beginning with `start`.
fn said(chats: &[(String, String)], start: &str) -> bool {
    chats
        .iter()
        .filter_map(|(_from, message)| message.strip_prefix(&format!("{PROBE_TAG}|")))
        .any(|line| line.starts_with(start))
}

/// The first line the probe said beginning with `start`, or `nothing`.
fn said_line(chats: &[(String, String)], start: &str) -> String {
    chats
        .iter()
        .filter_map(|(_from, message)| message.strip_prefix(&format!("{PROBE_TAG}|")))
        .find(|line| line.starts_with(start))
        .map_or_else(|| "nothing".to_owned(), without_ids)
}

/// What the probe last said of its URL request: granted, denied and why, or
/// nothing.
fn said_url(chats: &[(String, String)]) -> String {
    chats
        .iter()
        .filter_map(|(_from, message)| message.strip_prefix(&format!("{PROBE_TAG}|")))
        .rev()
        .find(|line| *line == "U" || line.starts_with("D|"))
        .map_or_else(
            || "nothing".to_owned(),
            |line| {
                if line == "U" {
                    "granted".to_owned()
                } else {
                    format!("denied: {}", without_ids(line.trim_start_matches("D|")))
                }
            },
        )
}

/// Rezzes a cube beside A, names it, puts the probe in it and waits until the
/// probe says it runs and what became of its URL request.
async fn scripted_cube(
    a: &mut Session,
    run: &mut Run,
    residue: &mut Residue,
    owner: AgentKey,
    metrics: &mut Metrics,
) -> Result<Object, TestFailure> {
    let cube = rez_cube(a, run, residue, 0).await?;
    a.send(Command::SetObjectName {
        local_id: cube.scoped_id(),
        name: CUBE_NAME.to_owned(),
    })
    .await?;
    a.send(Command::RezScript {
        target: cube.scoped_id(),
        params: Box::new(RezScriptParams {
            group_id: None,
            enabled: true,
            item: RestoreItem::new_script(owner, cube.full_id, PROBE_NAME, Uuid::new_v4()),
        }),
    })
    .await?;
    let started = Instant::now();
    let item = loop {
        let _heard = listen(a, Duration::from_millis(500), Duration::ZERO, |_heard| {
            false
        })
        .await?;
        let held = listing(a, cube.scoped_id(), cube.full_id).await?;
        if let Some(item) = held
            .items
            .iter()
            .find(|item| item.name == PROBE_NAME && item.inv_type == InventoryType::Script)
        {
            break item.item_id;
        }
        if started.elapsed() >= STEP {
            return Err(TestFailure::Assertion(
                "the new script never appeared in the cube".to_owned(),
            ));
        }
    };
    a.send(Command::UploadScript {
        location: ScriptUploadLocation::TaskInventory {
            task_id: cube.full_id,
            item_id: item,
            running: true,
            experience: None,
        },
        target: ScriptTarget::Mono,
        source: probe_source().into_bytes(),
    })
    .await?;
    let heard = listen(a, STEP, Duration::ZERO, |heard| {
        said(&heard.chats, "R") && (said(&heard.chats, "U") || said(&heard.chats, "D|"))
    })
    .await?;
    if !said(&heard.chats, "R") {
        return Err(TestFailure::Assertion(
            "the probe script never said it runs".to_owned(),
        ));
    }
    metrics.set("scripted_rez_url", said_url(&heard.chats));
    Ok(cube)
}

/// Asks `AttachmentResources` raw and through our client, recording both
/// under `prefix`.
async fn resources_leg(
    a: &mut Session,
    http: &reqwest::Client,
    url: Option<&str>,
    metrics: &mut Metrics,
    prefix: &str,
    cast: &Cast,
) -> Result<(), TestFailure> {
    let raw = match url {
        Some(url) => ask(http, reqwest::Method::GET, url, None).await,
        None => no_capability(),
    };
    raw.record(metrics, &format!("{prefix}_resources"), cast);
    metrics.set(
        &format!("{prefix}_resources_summary"),
        raw.body
            .as_ref()
            .and_then(|body| body.get("summary"))
            .map_or_else(|| "absent".to_owned(), summary_words),
    );
    metrics.set(
        &format!("{prefix}_resources_cube"),
        raw.body
            .as_ref()
            .map_or_else(|| "absent".to_owned(), |body| cube_entry(body, cast)),
    );
    let client = if url.is_some() {
        a.send(Command::RequestAttachmentResources).await?;
        match a
            .wait_for(REQUEST_TIMEOUT, |event| match event {
                Event::AttachmentResources(report) => Some(report_words(report)),
                _ => None,
            })
            .await
        {
            Ok(words) => words,
            Err(TestFailure::Timeout(_)) => "no event".to_owned(),
            Err(other) => return Err(other),
        }
    } else {
        NO_CAPABILITY.to_owned()
    };
    metrics.set(&format!("{prefix}_resources_client"), client);
    Ok(())
}

/// A resource summary's `available` and `used` lists in words:
/// `available memory=-1 urls=38; used memory=0 urls=0`.
fn summary_words(summary: &Llsd) -> String {
    let side = |key: &str| -> String {
        match summary.get(key) {
            Some(Llsd::Array(items)) => {
                let mut words: Vec<String> = items
                    .iter()
                    .map(|item| {
                        let kind = match item.get("type") {
                            Some(Llsd::String(kind)) => kind.clone(),
                            _ => "?".to_owned(),
                        };
                        let amount = item
                            .get("amount")
                            .and_then(integer)
                            .map_or_else(|| "?".to_owned(), |amount| amount.to_string());
                        format!("{kind}={amount}")
                    })
                    .collect();
                words.sort_unstable();
                if words.is_empty() {
                    format!("{key} none")
                } else {
                    format!("{key} {}", words.join(" "))
                }
            }
            Some(_) => format!("{key} not an array"),
            None => format!("{key} absent"),
        }
    };
    format!("{}; {}", side("available"), side("used"))
}

/// The scripted cube's entry in a raw report, in words: the location it is
/// listed under, its fields, its name and its resources — or `absent`.
fn cube_entry(body: &Llsd, cast: &Cast) -> String {
    let Some(Llsd::Array(locations)) = body.get("attachments") else {
        return "no attachments array".to_owned();
    };
    for location in locations {
        let Some(Llsd::Array(objects)) = location.get("objects") else {
            continue;
        };
        for object in objects {
            if object.get("name") != Some(&Llsd::String(CUBE_NAME.to_owned())) {
                continue;
            }
            let at = match location.get("location") {
                Some(Llsd::String(at)) => at.clone(),
                other => format!("{other:?}"),
            };
            let resources = object.get("resources").map_or_else(
                || "absent".to_owned(),
                |resources| {
                    let memory = resources
                        .get("memory")
                        .and_then(integer)
                        .map_or_else(|| "absent".to_owned(), |memory| memory.to_string());
                    let urls = resources
                        .get("urls")
                        .and_then(integer)
                        .map_or_else(|| "absent".to_owned(), |urls| urls.to_string());
                    format!("memory={memory} urls={urls}")
                },
            );
            let owner = match object.get("owner_id") {
                Some(Llsd::Uuid(owner)) => cast.word(&owner.to_string()),
                Some(Llsd::String(owner)) => cast.word(owner),
                other => format!("{other:?}"),
            };
            return format!(
                "at \"{at}\", {}, owner {owner}, {resources}",
                shape(object, cast)
            );
        }
    }
    "absent".to_owned()
}

/// Our client's reading of a report in words: each location with how many
/// objects it lists (the cube by name), and the summary.
fn report_words(report: &AttachmentResourcesReport) -> String {
    let mut locations: Vec<String> = report
        .attachments
        .iter()
        .map(|location| {
            let cube = location
                .objects
                .iter()
                .find(|object| object.name == CUBE_NAME)
                .map_or_else(String::new, |object| {
                    format!(
                        " (the cube: memory={} urls={})",
                        object
                            .resources
                            .memory
                            .map_or_else(|| "absent".to_owned(), |memory| memory.to_string()),
                        object
                            .resources
                            .urls
                            .map_or_else(|| "absent".to_owned(), |urls| urls.to_string())
                    )
                });
            format!("{}: {}{cube}", location.location, location.objects.len())
        })
        .collect();
    locations.sort_unstable();
    let amounts = |amounts: &[ResourceAmount]| -> String {
        let mut words: Vec<String> = amounts
            .iter()
            .map(|amount| format!("{}={}", amount.resource_type, amount.amount))
            .collect();
        words.sort_unstable();
        words.join(" ")
    };
    format!(
        "{} location(s) [{}]; available {}; used {}",
        report.attachments.len(),
        locations.join(", "),
        amounts(&report.summary.available),
        amounts(&report.summary.used)
    )
}

/// The probe: says it runs, asks for a URL, says what became of the request,
/// and says so again when it is attached.
fn probe_source() -> String {
    format!(
        r#"default
{{
    state_entry()
    {{
        llOwnerSay("{PROBE_TAG}|R");
        llRequestURL();
    }}
    attach(key id)
    {{
        llOwnerSay("{PROBE_TAG}|A|" + (string)(id != NULL_KEY) + "|" + (string)llGetAttached());
    }}
    http_request(key id, string method, string body)
    {{
        if (method == URL_REQUEST_GRANTED) llOwnerSay("{PROBE_TAG}|U");
        else if (method == URL_REQUEST_DENIED) llOwnerSay("{PROBE_TAG}|D|" + body);
        else llHTTPResponse(id, 200, "census");
    }}
}}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::{Cast, FIRST_WEIGHT, OWN_WEIGHT, probe_source, report_body, shape, weight_word};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::Uuid;

    /// The probe compiles.
    #[test]
    fn the_probe_compiles() -> Result<(), String> {
        let source = probe_source();
        sl_lsl_runtime::compile(&source)
            .map(|_program| ())
            .map_err(|errors| format!("the probe does not compile: {errors:?}\n{source}"))
    }

    /// A report reads back as the reference's shape, its agent named by role.
    #[test]
    fn a_report_names_its_agent_by_role() -> Result<(), String> {
        let cast = Cast {
            a: Uuid::new_v4(),
            b: Uuid::new_v4(),
            nobody: Uuid::new_v4(),
        };
        let body = report_body(&[(cast.a, FIRST_WEIGHT, true)]);
        let llsd = sl_wire::parse_llsd_xml(&body).map_err(|error| error.to_string())?;
        assert_eq!(
            shape(&llsd, &cast),
            "{agents:{A:{tooComplex:boolean,weight:integer}}}"
        );
        Ok(())
    }

    /// A weight a leg sent is named by the leg.
    #[test]
    fn a_sent_weight_is_named_by_its_leg() {
        assert_eq!(weight_word(i64::from(FIRST_WEIGHT)), "the first report's");
        assert_eq!(
            weight_word(i64::midpoint(
                i64::from(FIRST_WEIGHT),
                i64::from(OWN_WEIGHT)
            )),
            "the mean of the first report and the own report"
        );
        assert_eq!(weight_word(7), "7");
    }
}
