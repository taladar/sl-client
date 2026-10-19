//! A census of the home location and of landmarks: how each grid answers
//! "Set Home to Here" over both roads, what it does with a home teleport, what
//! a landmark it creates holds, and how a teleport to that landmark goes.
//!
//! 1. **The capability**: does the seed grant `HomeLocation`?
//! 2. **Home at login**: does the login response name a home?
//! 3. **Set home over UDP** (`SetStartLocationRequest`, sent raw as the
//!    reference sends it) where the avatar stands: what the agent is told.
//! 4. **Set home over the capability**, raw, as the reference POSTs it: at a
//!    corner of the region (another parcel on most land), outside the region,
//!    with a body that names no location, and where the avatar stands — the
//!    answer's status, media type and shape, and what the agent is told
//!    beside it.
//! 5. **Our client's** [`Command::SetStartLocation`] where the avatar stands:
//!    which road it took and what it read.
//! 6. **A landmark** where the avatar stands, created as the reference creates
//!    one (an empty `CreateInventoryItem` of the landmark classes in the
//!    Landmarks folder): the item the grid makes, and the asset body it
//!    writes.
//! 7. **Home**: a teleport home ([`Command::TeleportViaLandmark`] of none) —
//!    its progress lines and where it lands — and home set again, raw and by
//!    our client, where it landed: the one spot the avatar is known to be
//!    allowed a home.
//! 8. **A teleport to the landmark**, from home, and **another** from the
//!    landmark's own spot: the progress lines and where each lands.
//!
//! The landmark is deleted at the end. Home stays where the avatar stands.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/teleport.md` § Home and landmarks; every run is held to
//! it.
//!
//! `1av`, live and fake grids.

use std::time::{Duration, Instant};

use reqwest::header::CONTENT_TYPE;
use sl_client_tokio::{
    AnyMessage, AssetKey, AssetType, Command, Diagnostic, Event, FolderType, HomeLocationAnswer,
    InventoryFolderKey, InventoryType, NewInventoryItem, RegionCoordinates, Reliability,
    StartLocationSlot, Uuid, Vector, WearableType,
};
use sl_wire::messages::{
    SetStartLocationRequest, SetStartLocationRequestAgentDataBlock,
    SetStartLocationRequestStartLocationDataBlock,
};

use crate::cases::asset_http::without_ids;
use crate::cases::land_transactions::{region_named, teleport};
use crate::cases::object_edit::hex;
use crate::cases::object_rez_derez::{Answer, SETTLE_IDLE, SETTLE_WINDOW};
use crate::cases::task_inventory::{OPENSIM_START, hold_to_measured, shape_of};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim, settle_scene_with_avatar};
use crate::teleport_trace::{TeleportTrace, watch_teleport};

/// The capability the census measures.
const CAPABILITY: &str = "HomeLocation";

/// How long to poll for the capability after region arrival: the seed fetch
/// races the arrival, so the map may fill in shortly afterwards.
const CAP_WAIT: Duration = Duration::from_secs(20);

/// How long the agent is listened to after each request.
const WINDOW: Duration = Duration::from_secs(6);

/// How long one answer (a capability's, an item's, an asset's) may take.
const REPLY: Duration = Duration::from_secs(30);

/// What a raw answer reads on a grid that grants no capability.
const NO_CAPABILITY: &str = "no capability";

/// The landmark's name.
const LANDMARK_NAME: &str = "SLClientLandmarkCensus";

/// The landmark's description.
const LANDMARK_DESCRIPTION: &str = "sl-conformance landmarks census";

/// The reference viewer's `PERM_ALL`, the next-owner mask it creates a
/// landmark with.
const PERM_ALL: u32 = 0x7fff_ffff;

/// How far from a position an arrival may land and still be "there".
const THERE_M: f32 = 2.0;

/// Where the measurements below are written down.
const SOURCE: &str =
    "book/src/gridspec/teleport.md § Home and landmarks (landmarks-home, 2026-10-19)";

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
/// under. Where an arrival lands is left out: it is the land's (a telehub
/// catches every teleport into Second Life's sandbox region), and so is where
/// the account's home already was.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "cap_at_home_body",
        each(
            Answer::Text(
                "{HomeLocation:{LocationPos:{X:real,Y:real,Z:real}},success:string}; success string \"false\"; the position as sent",
            ),
            Answer::Text(
                "{HomeLocation:{LocationPos:{X:real,Y:real,Z:real}},success:string}; success string \"true\"; the position as sent",
            ),
        ),
    ),
    (
        "cap_at_home_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("text/html;charset=UTF-8"),
        ),
    ),
    ("cap_at_home_status", both(Answer::Count(200))),
    (
        "cap_at_home_told",
        each(
            Answer::Text(
                "alert \"You can only set your 'Home Location' on your land or at a mainland Infohub.\" keyed HomeLocationLimits",
            ),
            Answer::Text("alert \"Home position set.\" keyed HomePositionSet"),
        ),
    ),
    (
        "cap_corner_body",
        each(
            Answer::Text(
                "{HomeLocation:{LocationPos:{X:real,Y:real,Z:real}},success:string}; success string \"false\"; the position as sent",
            ),
            Answer::Text(
                "{HomeLocation:{LocationPos:{X:real,Y:real,Z:real}},success:string}; success string \"true\"; the position as sent",
            ),
        ),
    ),
    (
        "cap_corner_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("text/html;charset=UTF-8"),
        ),
    ),
    ("cap_corner_status", both(Answer::Count(200))),
    (
        "cap_corner_told",
        each(
            Answer::Text(
                "alert \"You can only set your 'Home Location' on your land or at a mainland Infohub.\" keyed HomeLocationLimits",
            ),
            Answer::Text("alert \"Home position set.\" keyed HomePositionSet"),
        ),
    ),
    (
        "cap_empty_body",
        each(
            Answer::Text(
                "{HomeLocation:{LocationPos:undef},success:string}; success string \"true\"; no position",
            ),
            Answer::Text("{success:string}; success string \"false\"; no position"),
        ),
    ),
    (
        "cap_empty_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("text/html;charset=UTF-8"),
        ),
    ),
    ("cap_empty_status", both(Answer::Count(200))),
    (
        "cap_empty_told",
        each(
            Answer::Text("nothing"),
            Answer::Text("alert \"Set Home request failed\""),
        ),
    ),
    (
        "cap_here_body",
        each(
            Answer::Text(
                "{HomeLocation:{LocationPos:{X:real,Y:real,Z:real}},success:string}; success string \"false\"; the position as sent",
            ),
            Answer::Text(
                "{HomeLocation:{LocationPos:{X:real,Y:real,Z:real}},success:string}; success string \"true\"; the position as sent",
            ),
        ),
    ),
    (
        "cap_here_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("text/html;charset=UTF-8"),
        ),
    ),
    ("cap_here_status", both(Answer::Count(200))),
    (
        "cap_here_told",
        each(
            Answer::Text(
                "alert \"You can only set your 'Home Location' on your land or at a mainland Infohub.\" keyed HomeLocationLimits",
            ),
            Answer::Text("alert \"Home position set.\" keyed HomePositionSet"),
        ),
    ),
    (
        "cap_outside_body",
        each(
            Answer::Text(
                "{HomeLocation:{LocationPos:{X:real,Y:real,Z:real}},success:string}; success string \"false\"; the position as sent",
            ),
            Answer::Text("{success:string}; success string \"false\"; no position"),
        ),
    ),
    (
        "cap_outside_content_type",
        each(
            Answer::Text("application/llsd+xml"),
            Answer::Text("text/html;charset=UTF-8"),
        ),
    ),
    ("cap_outside_status", both(Answer::Count(200))),
    (
        "cap_outside_told",
        each(
            Answer::Text("nothing"),
            Answer::Text("alert \"Set Home request failed\""),
        ),
    ),
    ("capability_granted", both(Answer::Flag(true))),
    (
        "client_at_home_answer",
        each(
            Answer::Text("refused, the position as sent"),
            Answer::Text("stored, the position as sent"),
        ),
    ),
    (
        "client_at_home_told",
        each(
            Answer::Text("home location answer, refused"),
            Answer::Text(
                "alert \"Home position set.\" keyed HomePositionSet; home location answer, stored",
            ),
        ),
    ),
    (
        "client_here_answer",
        each(
            Answer::Text("refused, the position as sent"),
            Answer::Text("stored, the position as sent"),
        ),
    ),
    (
        "client_here_told",
        each(
            Answer::Text("home location answer, refused"),
            Answer::Text(
                "alert \"Home position set.\" keyed HomePositionSet; home location answer, stored",
            ),
        ),
    ),
    ("home_teleport_answer", both(Answer::Text("local"))),
    (
        "home_teleport_local_flags",
        each(Answer::Text("0x00020"), Answer::Text("0x00022")),
    ),
    ("home_teleport_outcome", both(Answer::Text("local"))),
    (
        "home_teleport_progress_flags",
        each(Answer::Text("0x00020"), Answer::Text("")),
    ),
    (
        "home_teleport_progress_lines",
        each(Answer::Text("sending_home"), Answer::Text("")),
    ),
    (
        "home_teleport_sequence",
        each(
            Answer::Text("started,progress,local"),
            Answer::Text("started,local"),
        ),
    ),
    (
        "home_teleport_start_flags",
        each(Answer::Text("0x00020"), Answer::Text("0x00022")),
    ),
    ("landmark_again_answer", both(Answer::Text("local"))),
    (
        "landmark_again_local_flags",
        each(Answer::Text("0x00008"), Answer::Text("0x0000a")),
    ),
    ("landmark_again_outcome", both(Answer::Text("local"))),
    (
        "landmark_again_progress_flags",
        each(Answer::Text("0x00008"), Answer::Text("")),
    ),
    (
        "landmark_again_progress_lines",
        each(Answer::Text("sending_landmark"), Answer::Text("")),
    ),
    (
        "landmark_again_sequence",
        each(
            Answer::Text("started,progress,local"),
            Answer::Text("started,local"),
        ),
    ),
    (
        "landmark_again_start_flags",
        each(Answer::Text("0x00008"), Answer::Text("0x0000a")),
    ),
    (
        "landmark_body",
        each(
            Answer::Text(
                "Landmark version 2 / region_id <this region> / local_pos where the avatar stood (3 numbers)",
            ),
            Answer::Text(
                "Landmark version 2 / region_id <this region> / local_pos where the avatar stood (3 numbers) / region_handle <this region's>",
            ),
        ),
    ),
    // Second Life writes the position to two decimals; OpenSim as its floats
    // print, which turns on the position.
    (
        "landmark_body_decimals",
        each(Answer::Count(2), Answer::Any),
    ),
    (
        "landmark_item",
        each(
            Answer::Text(
                "type 3 inv 3 in landmarks, asset set, flags 0x0, base 0x7fffffff owner 0x7fffffff group 0x7fffffff everyone 0x7fffbfff next 0x7fffffff, sale 0, created by self, owned by self, name as sent, description as sent",
            ),
            Answer::Text(
                "type 3 inv 3 in landmarks, asset set, flags 0x0, base 0x0009e000 owner 0x0009e000 group 0x0009e000 everyone 0x0009a000 next 0x7fffffff, sale 0, created by self, owned by self, name as sent, description as sent",
            ),
        ),
    ),
    ("landmark_teleport_answer", both(Answer::Text("local"))),
    (
        "landmark_teleport_local_flags",
        each(Answer::Text("0x00008"), Answer::Text("0x0000a")),
    ),
    ("landmark_teleport_outcome", both(Answer::Text("local"))),
    (
        "landmark_teleport_progress_flags",
        each(Answer::Text("0x00008"), Answer::Text("")),
    ),
    (
        "landmark_teleport_progress_lines",
        each(Answer::Text("sending_landmark"), Answer::Text("")),
    ),
    (
        "landmark_teleport_sequence",
        each(
            Answer::Text("started,progress,local"),
            Answer::Text("started,local"),
        ),
    ),
    (
        "landmark_teleport_start_flags",
        each(Answer::Text("0x00008"), Answer::Text("0x0000a")),
    ),
    ("landmark_told", both(Answer::Text("nothing"))),
    (
        "login_home",
        each(Answer::Text("none"), Answer::Text("sent")),
    ),
    (
        "udp_here_told",
        each(
            Answer::Text(
                "alert \"You can only set your 'Home Location' on your land or at a mainland Infohub.\" keyed ListValidHomeLocations",
            ),
            Answer::Text("agent alert \"Home position set.\""),
        ),
    ),
];

/// The census of the home location and of landmarks.
#[derive(Debug)]
pub struct LandmarksHome;

impl GridTest for LandmarksHome {
    fn name(&self) -> &'static str {
        "landmarks-home"
    }

    fn description(&self) -> &'static str {
        "Set Home over UDP and the HomeLocation capability, a home teleport, and a landmark: \
         the item and body a grid makes and a teleport to it"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
    }

    fn start_location(&self, grid: Grid) -> &'static str {
        if is_opensim(grid) {
            OPENSIM_START
        } else {
            "last"
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_secs(600)
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

/// The two folders of the login skeleton the census files into.
#[derive(Debug, Clone, Copy)]
struct Folders {
    /// The Landmarks folder (the root where the skeleton has none).
    landmarks: InventoryFolderKey,
}

/// Reads the login skeleton's folders.
async fn folders(session: &mut Session) -> Result<Folders, TestFailure> {
    let skeleton = session
        .wait_for(REPLY, |event| match event {
            Event::InventorySkeleton(folders) => Some(folders.clone()),
            _ => None,
        })
        .await?;
    let root = skeleton
        .iter()
        .find(|folder| folder.parent_id.is_none())
        .map(|folder| folder.folder_id)
        .ok_or_else(|| {
            TestFailure::Assertion("inventory skeleton had no root folder".to_owned())
        })?;
    let landmarks = skeleton
        .iter()
        .find(|folder| folder.folder_type == FolderType::Landmark.to_code())
        .map_or(root, |folder| folder.folder_id);
    Ok(Folders { landmarks })
}

/// Runs every leg, recording into `metrics`.
#[expect(
    clippy::too_many_lines,
    reason = "one linear flow of legs, each a request and a window to hear its answer in"
)]
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_point = ctx.build_position();
    let somewhere = ctx
        .build_region()
        .zip(ctx.build_position())
        .map(|(region, point)| (region.to_owned(), point));
    let session = ctx.primary();
    // The skeleton comes with the login, ahead of the region: read it first.
    let folders = folders(session).await?;
    session.wait_for_region(REGION_TIMEOUT).await?;
    metrics.set(
        "login_home",
        session
            .login_success()
            .map_or("no login answer", |success| {
                if success.home.is_some() {
                    "sent"
                } else {
                    "none"
                }
            }),
    );
    // On Second Life, stand where the fixtures say building is allowed, as
    // the rez censuses do, so the census sets home on land it measured.
    if !is_opensim(grid)
        && let Some((region, point)) = somewhere
    {
        let here = session
            .region_identity()
            .and_then(|identity| identity.sim_name.as_ref())
            .is_some_and(|name| name.to_string().eq_ignore_ascii_case(&region));
        if !here {
            let handle = region_named(session, &region).await?;
            teleport(session, handle, &point).await?;
        }
    }
    let settled =
        settle_scene_with_avatar(session, grid, build_point, SETTLE_WINDOW, SETTLE_IDLE).await?;
    let here = settled.avatar.ok_or_else(|| {
        TestFailure::Assertion("our own avatar never appeared in the object stream".to_owned())
    })?;
    let here = RegionCoordinates::new(here.x, here.y, here.z);
    let started = Instant::now();
    while session.cap(CAPABILITY).is_none() && started.elapsed() < CAP_WAIT {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let cap = session.cap(CAPABILITY);
    metrics.set("capability_granted", cap.is_some());
    let facing = Vector {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };

    // 3. Set home over UDP, as the reference sends it.
    let agent = session
        .agent_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;
    let session_id = session
        .session_id()
        .ok_or_else(|| TestFailure::Assertion("login reported no session id".to_owned()))?;
    let raw = AnyMessage::SetStartLocationRequest(SetStartLocationRequest {
        agent_data: SetStartLocationRequestAgentDataBlock {
            agent_id: agent.uuid(),
            session_id,
        },
        start_location_data: SetStartLocationRequestStartLocationDataBlock {
            sim_name: b"\0".to_vec(),
            location_id: StartLocationSlot::Home.to_code(),
            location_pos: Vector {
                x: here.x(),
                y: here.y(),
                z: here.z(),
            },
            location_look_at: facing.clone(),
        },
    });
    session
        .send(Command::Send {
            message: Box::new(raw),
            reliability: Reliability::Reliable,
        })
        .await?;
    let told = listen(session, WINDOW).await?;
    metrics.set("udp_here_told", told.words());

    // 4. Set home over the capability, raw.
    let http = reqwest::Client::builder()
        .timeout(REPLY)
        .build()
        .map_err(|error| TestFailure::State(format!("building an HTTP client failed: {error}")))?;
    let corner = RegionCoordinates::new(8.0, 8.0, here.z());
    let outside = RegionCoordinates::new(300.0, 300.0, here.z());
    for (word, body, sent) in [
        (
            "cap_corner",
            Some(request_body(corner, &facing)),
            Some(corner),
        ),
        (
            "cap_outside",
            Some(request_body(outside, &facing)),
            Some(outside),
        ),
        (
            "cap_empty",
            Some("<?xml version=\"1.0\"?><llsd><map></map></llsd>".to_owned()),
            None,
        ),
        ("cap_here", Some(request_body(here, &facing)), Some(here)),
    ] {
        ask_raw(&http, cap.as_deref(), word, body, sent, metrics).await;
        let told = listen(session, WINDOW).await?;
        metrics.set(&format!("{word}_told"), told.words());
    }

    // 5. Our client's own Set Home.
    client_set_home(session, "client_here", here, &facing, metrics).await?;

    // 6. A landmark where the avatar stands.
    let Some(item) = create_landmark(session, folders, agent.uuid(), here, metrics).await? else {
        for key in [
            "landmark_body",
            "landmark_teleport_lands",
            "landmark_again_lands",
        ] {
            metrics.set(key, "no landmark");
        }
        return Ok(());
    };

    // 7. Home, and home set again where it landed.
    session
        .send(Command::TeleportViaLandmark { landmark: None })
        .await?;
    let trace = watch_teleport(session, REGION_TIMEOUT).await?;
    trace.record("home_teleport_", metrics);
    metrics.set("home_teleport_lands", landed(&trace, here));
    let home = trace.local.as_ref().map_or(here, |local| local.position);
    let _settled = listen(session, WINDOW).await?;
    ask_raw(
        &http,
        cap.as_deref(),
        "cap_at_home",
        Some(request_body(home, &facing)),
        Some(home),
        metrics,
    )
    .await;
    let told = listen(session, WINDOW).await?;
    metrics.set("cap_at_home_told", told.words());
    client_set_home(session, "client_at_home", home, &facing, metrics).await?;

    // 8. A teleport to the landmark, and another from where it lands.
    for prefix in ["landmark_teleport_", "landmark_again_"] {
        session
            .send(Command::TeleportViaLandmark {
                landmark: Some(AssetKey::from(item.asset_id)),
            })
            .await?;
        let trace = watch_teleport(session, REGION_TIMEOUT).await?;
        trace.record(prefix, metrics);
        metrics.set(&format!("{prefix}lands"), landed(&trace, here));
        let _settled = listen(session, WINDOW).await?;
    }

    session
        .send(Command::RemoveInventoryItems(vec![item.item_id]))
        .await?;
    let _cleaned = listen(session, WINDOW).await?;
    Ok(())
}

/// Sets home at `position` with our client's own command and records, under
/// `word`, what the agent was told and what the client read.
async fn client_set_home(
    session: &mut Session,
    word: &str,
    position: RegionCoordinates,
    facing: &Vector,
    metrics: &mut Metrics,
) -> Result<(), TestFailure> {
    session
        .send(Command::SetStartLocation {
            slot: StartLocationSlot::Home,
            position,
            look_at: facing.clone(),
        })
        .await?;
    let told = listen(session, WINDOW).await?;
    metrics.set(&format!("{word}_told"), told.words());
    metrics.set(
        &format!("{word}_answer"),
        told.answer.as_ref().map_or_else(
            || "none".to_owned(),
            |answer| describe_answer(answer, position),
        ),
    );
    Ok(())
}

/// Creates a landmark where the avatar stands, `here`, as the reference does,
/// and records the item the grid made and the body it wrote. `None` when no
/// item came.
async fn create_landmark(
    session: &mut Session,
    folders: Folders,
    agent: Uuid,
    here: RegionCoordinates,
    metrics: &mut Metrics,
) -> Result<Option<sl_client_tokio::InventoryItem>, TestFailure> {
    session
        .send(Command::CreateInventoryItem(NewInventoryItem {
            folder_id: folders.landmarks,
            transaction_id: Uuid::nil(),
            next_owner_mask: PERM_ALL,
            asset_type: AssetType::Landmark,
            inv_type: InventoryType::Landmark,
            wearable_type: WearableType::Shape,
            name: LANDMARK_NAME.to_owned(),
            description: LANDMARK_DESCRIPTION.to_owned(),
        }))
        .await?;
    let created = session
        .wait_for(REPLY, |event| match event {
            Event::InventoryItemCreated {
                sim_approved, item, ..
            } if item.name.contains(LANDMARK_NAME) => Some((*sim_approved, item.clone())),
            _ => None,
        })
        .await;
    let item = match created {
        Ok((approved, item)) => {
            metrics.set(
                "landmark_item",
                describe_item(&item, approved, agent, folders.landmarks),
            );
            item
        }
        Err(TestFailure::Timeout(_)) => {
            metrics.set("landmark_item", "none");
            return Ok(None);
        }
        Err(other) => return Err(other),
    };
    let told = listen(session, WINDOW).await?;
    metrics.set("landmark_told", told.words());

    // The body the grid wrote.
    if item.asset_id.is_nil() {
        metrics.set("landmark_body", "no asset");
        metrics.set("landmark_body_decimals", -1_i64);
        return Ok(Some(item));
    }
    let identity = session.region_identity().cloned();
    session
        .send(Command::FetchAsset {
            asset_id: AssetKey::from(item.asset_id),
            asset_type: AssetType::Landmark,
            byte_range: None,
        })
        .await?;
    let body = session
        .wait_for(REPLY, |event| match event {
            Event::AssetReceived(asset) if asset.id == item.asset_id => {
                Some(Ok(asset.data.clone()))
            }
            Event::AssetTransferFailed {
                asset_id, status, ..
            } if *asset_id == item.asset_id => Some(Err(format!("{status:?}"))),
            _ => None,
        })
        .await?;
    match body {
        Ok(data) => {
            let text = String::from_utf8_lossy(&data);
            metrics.set(
                "landmark_body",
                describe_body(
                    &text,
                    identity.as_ref().map(|identity| identity.region_id),
                    session.region_handle().map(|handle| handle.get()),
                    here,
                ),
            );
            metrics.set("landmark_body_decimals", body_decimals(&text));
        }
        Err(status) => {
            metrics.set("landmark_body", format!("not fetched: {status}"));
            metrics.set("landmark_body_decimals", -1_i64);
        }
    }
    Ok(Some(item))
}

/// The body of a `HomeLocation` POST of `position`, as the reference sends it.
fn request_body(position: RegionCoordinates, look_at: &Vector) -> String {
    sl_wire::build_home_location_request(&sl_wire::HomeLocationRequest {
        location_id: StartLocationSlot::Home.to_code(),
        position,
        look_at: look_at.clone(),
    })
}

/// POSTs `body` to the capability and records the whole answer under `word`:
/// `_status`, `_content_type` and `_body`.
async fn ask_raw(
    http: &reqwest::Client,
    cap: Option<&str>,
    word: &str,
    body: Option<String>,
    sent: Option<RegionCoordinates>,
    metrics: &mut Metrics,
) {
    let Some(cap) = cap else {
        for suffix in ["status", "content_type", "body"] {
            metrics.set(&format!("{word}_{suffix}"), NO_CAPABILITY);
        }
        return;
    };
    let mut request = http.post(cap).header(CONTENT_TYPE, "application/llsd+xml");
    if let Some(body) = body {
        request = request.body(body);
    }
    match request.send().await {
        Ok(response) => {
            metrics.set(
                &format!("{word}_status"),
                i64::from(response.status().as_u16()),
            );
            metrics.set(
                &format!("{word}_content_type"),
                response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map_or_else(|| "none".to_owned(), ToOwned::to_owned),
            );
            let text = response.text().await.unwrap_or_default();
            metrics.set(&format!("{word}_body"), describe_raw(&text, sent));
        }
        Err(error) => {
            let failure = format!("no answer: {}", without_ids(&error.to_string()));
            for suffix in ["status", "content_type", "body"] {
                metrics.set(&format!("{word}_{suffix}"), failure.as_str());
            }
        }
    }
}

/// A raw answer in words: its LLSD shape, how it spells `success`, and the
/// stored position against the one `sent`. A body that is no LLSD is named by
/// its first words.
fn describe_raw(text: &str, sent: Option<RegionCoordinates>) -> String {
    if !text.contains("<llsd") {
        let words: Vec<&str> = text.split_whitespace().take(12).collect();
        return if words.is_empty() {
            "empty".to_owned()
        } else {
            format!("not LLSD: {}", without_ids(&words.join(" ")))
        };
    }
    let Ok(body) = sl_wire::parse_llsd_xml(text) else {
        return "unreadable LLSD".to_owned();
    };
    let success = match body.get("success") {
        Some(sl_wire::Llsd::String(text)) => format!("string \"{text}\""),
        Some(sl_wire::Llsd::Boolean(flag)) => format!("boolean {flag}"),
        Some(other) => format!("{} {other:?}", shape_of(other)),
        None => "none".to_owned(),
    };
    let stored = sl_wire::parse_home_location_answer(&body)
        .ok()
        .and_then(|answer| answer.position)
        .map_or_else(
            || "no position".to_owned(),
            |position| {
                sent.map_or_else(|| "a position".to_owned(), |sent| relation(position, sent))
            },
        );
    format!("{}; success {success}; {stored}", shape_of(&body))
}

/// What our client read, in words.
fn describe_answer(answer: &HomeLocationAnswer, sent: RegionCoordinates) -> String {
    format!(
        "{}, {}",
        if answer.success { "stored" } else { "refused" },
        answer.position.map_or_else(
            || "no position".to_owned(),
            |position| relation(position, sent)
        )
    )
}

/// How a position a grid names relates to the one it was given.
fn relation(position: RegionCoordinates, sent: RegionCoordinates) -> String {
    let near = |a: f32, b: f32| (a - b).abs() < 0.001;
    if near(position.x(), sent.x()) && near(position.y(), sent.y()) && near(position.z(), sent.z())
    {
        "the position as sent".to_owned()
    } else if near(position.x(), sent.x().round())
        && near(position.y(), sent.y().round())
        && near(position.z(), sent.z().round())
    {
        "the position rounded".to_owned()
    } else {
        format!(
            "another position ({:.2} {:.2} {:.2} for {:.2} {:.2} {:.2})",
            position.x(),
            position.y(),
            position.z(),
            sent.x(),
            sent.y(),
            sent.z()
        )
    }
}

/// Where a teleport ended against where it was meant to: `there` within
/// [`THERE_M`] of `meant`, `elsewhere` otherwise, or how it ended.
fn landed(trace: &TeleportTrace, meant: RegionCoordinates) -> String {
    match &trace.local {
        Some(local) => {
            let (dx, dy) = (
                local.position.x() - meant.x(),
                local.position.y() - meant.y(),
            );
            if dx.hypot(dy) <= THERE_M {
                "there".to_owned()
            } else {
                "elsewhere in the region".to_owned()
            }
        }
        None => trace.answer().to_owned(),
    }
}

/// The created item in words.
fn describe_item(
    item: &sl_client_tokio::InventoryItem,
    approved: bool,
    agent: Uuid,
    landmarks: InventoryFolderKey,
) -> String {
    let kept = |text: &str, sent: &str| {
        if text == sent {
            "as sent".to_owned()
        } else {
            format!("\"{}\"", without_ids(text))
        }
    };
    format!(
        "{}type {} inv {} in {}, asset {}, flags {:#x}, base {} owner {} group {} everyone {} next \
         {}, sale {}, created by {}, owned by {}, name {}, description {}",
        if approved { "" } else { "not approved: " },
        item.item_type,
        item.inv_type,
        if item.folder_id == landmarks {
            "landmarks"
        } else {
            "another folder"
        },
        if item.asset_id.is_nil() { "nil" } else { "set" },
        item.flags,
        hex(item.permissions.base),
        hex(item.permissions.owner),
        hex(item.permissions.group),
        hex(item.permissions.everyone),
        hex(item.permissions.next_owner),
        item.sale_type,
        if item.creator_id.uuid() == agent {
            "self"
        } else {
            "another"
        },
        if item.owner.uuid() == agent {
            "self"
        } else {
            "another"
        },
        kept(&item.name, LANDMARK_NAME),
        kept(&item.description, LANDMARK_DESCRIPTION)
    )
}

/// A landmark body in words, line by line: the region id and handle against
/// the region's, and the position against where the avatar stood.
fn describe_body(
    text: &str,
    region_id: Option<Uuid>,
    region_handle: Option<u64>,
    standing: RegionCoordinates,
) -> String {
    let lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "region_id" => format!(
                    "region_id {}",
                    if value.parse::<Uuid>().ok() == region_id {
                        "<this region>"
                    } else {
                        "<another id>"
                    }
                ),
                "region_handle" => format!(
                    "region_handle {}",
                    if value.parse::<u64>().ok() == region_handle {
                        "<this region's>"
                    } else {
                        "<another>"
                    }
                ),
                "local_pos" | "position" => {
                    let numbers: Vec<&str> = value.split_whitespace().collect();
                    let parsed: Vec<f32> = numbers
                        .iter()
                        .filter_map(|number| number.parse::<f32>().ok())
                        .collect();
                    let near = match parsed.as_slice() {
                        [x, y, _z] => {
                            let horizontal = (x - standing.x()).hypot(y - standing.y());
                            if horizontal <= THERE_M {
                                "where the avatar stood"
                            } else {
                                "elsewhere"
                            }
                        }
                        _ => "not three numbers",
                    };
                    format!("{key} {near} ({} numbers)", numbers.len())
                }
                _ => without_ids(line),
            }
        })
        .collect();
    if lines.is_empty() {
        "empty".to_owned()
    } else {
        lines.join(" / ")
    }
}

/// How many decimals a landmark body writes its position's first number to,
/// or `-1` for a body that names no position.
fn body_decimals(text: &str) -> i64 {
    text.lines()
        .filter_map(|line| {
            line.trim()
                .strip_prefix("local_pos ")
                .or_else(|| line.trim().strip_prefix("position "))
        })
        .find_map(|rest| rest.split_whitespace().next())
        .map_or(-1, |number| {
            number.split_once('.').map_or(0, |(_whole, fraction)| {
                i64::try_from(fraction.len()).unwrap_or(i64::MAX)
            })
        })
}

/// What the agent was told in a window: one line per message the census
/// reads, and our client's reading of a `HomeLocation` answer.
#[derive(Debug, Default)]
struct Told {
    /// One line per message, in arrival order.
    lines: Vec<String>,
    /// Our client's reading of a `HomeLocation` answer, if one came.
    answer: Option<HomeLocationAnswer>,
}

impl Told {
    /// The lines, sorted — an alert and a capability's answer race — and
    /// joined, or `nothing`.
    fn words(&self) -> String {
        if self.lines.is_empty() {
            return "nothing".to_owned();
        }
        let mut lines = self.lines.clone();
        lines.sort();
        lines.join("; ")
    }
}

/// Folds one event into `told`; `None` for an event the census does not read.
fn fold(told: &mut Told, event: &Event) -> Option<()> {
    let line = match event {
        Event::AlertMessage {
            message,
            alert_info,
            ..
        } => {
            let keys = alert_info
                .iter()
                .map(|info| info.message.clone())
                .collect::<Vec<_>>()
                .join(", ");
            if keys.is_empty() {
                format!("alert \"{}\"", without_ids(message))
            } else {
                format!("alert \"{}\" keyed {keys}", without_ids(message))
            }
        }
        Event::AgentAlertMessage { message, modal, .. } => format!(
            "{} \"{}\"",
            if *modal {
                "modal agent alert"
            } else {
                "agent alert"
            },
            without_ids(message)
        ),
        Event::HomeLocationAnswered(answer) => {
            told.answer = Some(answer.clone());
            format!(
                "home location answer, {}",
                if answer.success { "stored" } else { "refused" }
            )
        }
        Event::ChatReceived(chat) if !chat.message.is_empty() => {
            format!("chat \"{}\"", without_ids(&chat.message))
        }
        Event::InventoryBulkUpdate { items, .. } => format!("bulk update of {} items", items.len()),
        _ => return None,
    };
    told.lines.push(line);
    Some(())
}

/// Listens on `session` for `window`.
async fn listen(session: &mut Session, window: Duration) -> Result<Told, TestFailure> {
    let mut told = Told::default();
    let seen = session.diagnostics().len();
    let started = Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
        match session
            .wait_for(remaining, |event| fold(&mut told, event))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => break,
            Err(other) => return Err(other),
        }
    }
    for diagnostic in session.diagnostics().iter().skip(seen) {
        if let Diagnostic::UnknownCapsEvent { message } = diagnostic {
            told.lines.push(format!("unread event {message}"));
        }
    }
    Ok(told)
}

#[cfg(test)]
mod tests {
    use super::{body_decimals, describe_body, describe_raw, relation};
    use pretty_assertions::assert_eq;
    use sl_client_tokio::{RegionCoordinates, Uuid};

    /// A body is described against the region and the spot, ids hidden.
    #[test]
    fn a_body_is_described_in_words() {
        let region = Uuid::from_u128(7);
        let text = format!(
            "Landmark version 2\nregion_id {region}\nlocal_pos 52.500000 62.000000 33.000000\n\
             region_handle 42\n"
        );
        assert_eq!(
            describe_body(
                &text,
                Some(region),
                Some(42),
                RegionCoordinates::new(52.0, 62.0, 33.0)
            ),
            "Landmark version 2 / region_id <this region> / local_pos where the avatar stood \
             (3 numbers) / region_handle <this region's>"
        );
        assert_eq!(body_decimals(&text), 6);
        assert_eq!(body_decimals("Landmark version 2\nlocal_pos 52 62 33\n"), 0);
        assert_eq!(body_decimals(""), -1);
    }

    /// A raw answer names its shape, its `success` and its position.
    #[test]
    fn a_raw_answer_is_described_in_words() {
        let sent = RegionCoordinates::new(52.25, 62.0, 33.0);
        let text = "<llsd><map><key>success</key><string>true</string><key>HomeLocation</key>\
                    <map><key>LocationPos</key><map><key>X</key><real>52.25</real><key>Y</key>\
                    <real>62</real><key>Z</key><real>33</real></map></map></map></llsd>";
        assert_eq!(
            describe_raw(text, Some(sent)),
            "{HomeLocation:{LocationPos:{X:real,Y:real,Z:real}},success:string}; success string \
             \"true\"; the position as sent"
        );
        assert_eq!(describe_raw("", None), "empty");
        assert_eq!(
            relation(RegionCoordinates::new(52.0, 62.0, 33.0), sent),
            "the position rounded"
        );
    }
}
