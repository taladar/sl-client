//! A census of how each grid bakes an avatar: who composites it, what the
//! agent is told of its own appearance and what a second viewer beside it is
//! told, the baked-texture cache, the central-bake handshake and the legacy
//! publish.
//!
//! Second Life central-bakes: the login names an appearance service, the
//! region sets `RegionProtocols` bit 0, the viewer bumps and bakes its Current
//! Outfit Folder over `IncrementCOFVersion` / `UpdateAvatarAppearance`, and
//! every viewer fetches the baked slots by URL from the service. A stock
//! OpenSim region has the viewer composite and upload its own bakes, publish
//! them with `AgentSetAppearance` and ask the region's cache for earlier ones
//! with `AgentCachedTexture`. The census asks each grid both halves and
//! records what each answers. Avatar **A** acts; **B** stands beside it and
//! only listens. The legs, in order:
//!
//! 1. **The service**: whether the login names `agent_appearance_service`
//!    and a `cof_version`, the region's central-bake bit, and which of the
//!    three appearance capabilities the seed grants.
//! 2. **Arrival**: A's own `AvatarAppearance` and B's of A — the
//!    `AppearanceData` block, its COF version against the login's, the flags,
//!    the visual parameters, the baked slots, the hover and attachment blocks.
//! 3. **The appearance service** (where one is named): A's head bake fetched
//!    by URL the way the reference fetches it, and a texture nobody baked.
//! 4. **The cache**: an `AgentCachedTexture` of the eleven baked slots with
//!    nil ids, then with made-up ids.
//! 5. **The bake handshake** (raw `UpdateAvatarAppearance` POSTs): COF version
//!    0, the version the grid expected, the same again, one ahead of it, our
//!    client's own request at 0, an `IncrementCOFVersion`, and a bake at the
//!    version it gave. Each records the whole answer and the appearances both
//!    viewers were sent after it.
//! 6. **The legacy publish**: an `AgentSetAppearance` of the plywood texture
//!    on every baked slot with made-up cache ids, the appearances both viewers
//!    were sent after it, any `RebakeAvatarTextures`, and the cache asked again
//!    with the published ids.
//! 7. **The restore**: A's arrival appearance published back (OpenSim keeps
//!    what was published), and on a central-baking grid a bake at the current
//!    version.
//!
//! COF versions grow from run to run, so every version is recorded against
//! the one the grid last stated (`current`, `current+5`), never as a number.
//!
//! What each live grid answered is stated as a [`Measured`] and written up in
//! `book/src/gridspec/appearance.md` § Baking; every run is held to it.
//!
//! `2av`, live and fake grids; on a fake grid B is shown nobody, so only A's
//! own column is held there.

use std::time::Duration;

use reqwest::header::CONTENT_TYPE;
use sl_client_tokio::{
    AvatarAppearance, Command, Event, TextureEntry, TextureFace, TextureKey, Uuid, Vector,
    avatar_texture, encode_texture_entry,
};
use sl_wire::{Llsd, parse_llsd_xml};

use crate::cases::asset_http::without_ids;
use crate::cases::object_rez_derez::Answer;
use crate::cases::task_inventory::hold_to_measured;
use crate::cases::upload_http::rendered;
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, fixtures, is_opensim, settle_scene_observing};

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/appearance.md § Baking (appearance-baking, 2026-10-19)";

/// Where both avatars log in on OpenSim: the level square in the block's
/// north-eastern region.
const OPENSIM_START: &str = "uri:Northeast Region&70&180&26";

/// The central-bake trigger.
const UPDATE_CAP: &str = "UpdateAvatarAppearance";

/// The COF version bump.
const INCREMENT_CAP: &str = "IncrementCOFVersion";

/// The legacy bake upload.
const UPLOAD_CAP: &str = "UploadBakedTexture";

/// What a leg records on a grid that grants no capability for it.
const NO_CAPABILITY: &str = "no capability";

/// How long a leg listens to both avatars on a live grid: a central bake
/// takes seconds.
const WINDOW: Duration = Duration::from_secs(10);

/// [`WINDOW`] on a fake grid.
const FAKE_WINDOW: Duration = Duration::from_millis(700);

/// How long a capability's answer may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the scene settle after login may take.
const SETTLE_WINDOW: Duration = Duration::from_secs(20);

/// The idle gap that ends the settle on a live grid.
const SETTLE_IDLE: Duration = Duration::from_secs(5);

/// [`SETTLE_IDLE`] on a fake grid.
const FAKE_SETTLE_IDLE: Duration = Duration::from_secs(1);

/// How far ahead of the expected version the "ahead" leg asks.
const AHEAD: i32 = 5;

/// Base of the made-up cache ids: no viewer's wearables hash to them.
const MADE_UP_CACHE: u128 = 0x5e7a_bace_c0de_4000_8000_0000_0000_0000;

/// Base of the cache ids the legacy publish names, so the cache asked again
/// afterwards can be matched against them.
const PUBLISHED_CACHE: u128 = 0x5e7a_bace_9b11_4000_8000_0000_0000_0000;

/// A texture id nobody baked, for the service's "not found".
const NOBODYS_BAKE: Uuid = Uuid::from_u128(0x2d6c_8f0e_51a4_4b39_9e07_c3f1_6a5d_8b20);

/// The number of visual parameters the legacy publish advertises: the
/// reference viewer's modern set.
const VISUAL_PARAM_COUNT: usize = 253;

/// A neutral visual-parameter value.
const NEUTRAL_VISUAL_PARAM: u8 = 128;

/// The bounding box the legacy publish advertises, in metres.
const AVATAR_SIZE: Vector = Vector {
    x: 0.45,
    y: 0.6,
    z: 1.9,
};

/// The `AgentCachedTexture` serials, one per query.
const SERIAL_NIL: i32 = 1;
/// The made-up query's serial.
const SERIAL_MADE_UP: i32 = 2;
/// The query after the publish.
const SERIAL_PUBLISHED: i32 = 3;

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

/// What each grid was measured to answer of A's own avatar, by the metric the
/// census records it under: every run, live or fake, is held to it.
const MEASURED: &[(&str, Measured<Answer>)] = &[
    ("arrival_own_appearances", both(Answer::Count(1))),
    (
        "arrival_own_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no block"),
        ),
    ),
    ("arrival_own_hover", both(Answer::Text("hover"))),
    (
        "bake_again_asked",
        each(Answer::Text("current"), Answer::Text("no capability")),
    ),
    (
        "bake_again_core_keys",
        each(
            Answer::Text("agent_id cof_version error success warnings"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_again_error",
        each(Answer::Text("undef"), Answer::Text("no capability")),
    ),
    (
        "bake_again_own_appearances",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "bake_again_own_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_again_own_hover",
        each(Answer::Text("hover"), Answer::Text("no capability")),
    ),
    (
        "bake_again_status",
        each(Answer::Count(200), Answer::Text("no capability")),
    ),
    (
        "bake_again_success",
        each(Answer::Text("true"), Answer::Text("no capability")),
    ),
    (
        "bake_again_versions",
        each(
            Answer::Text("cof_version=asked"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_ahead_asked",
        each(Answer::Text("current+5"), Answer::Text("no capability")),
    ),
    (
        "bake_ahead_core_keys",
        each(
            Answer::Text("agent_id code cof_version error expected observed success"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_ahead_error",
        each(
            Answer::Text("Cof Version Mismatch: asked != current"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_ahead_own_appearances",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "bake_ahead_own_block",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "bake_ahead_own_hover",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "bake_ahead_status",
        each(Answer::Count(400), Answer::Text("no capability")),
    ),
    (
        "bake_ahead_success",
        each(Answer::Text("false"), Answer::Text("no capability")),
    ),
    (
        "bake_ahead_versions",
        each(
            Answer::Text("cof_version=asked expected=current observed=asked"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_expected_asked",
        each(Answer::Text("current"), Answer::Text("no capability")),
    ),
    (
        "bake_expected_core_keys",
        each(
            Answer::Text("agent_id cof_version error success warnings"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_expected_error",
        each(Answer::Text("undef"), Answer::Text("no capability")),
    ),
    (
        "bake_expected_own_appearances",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "bake_expected_own_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_expected_own_hover",
        each(Answer::Text("hover"), Answer::Text("no capability")),
    ),
    (
        "bake_expected_status",
        each(Answer::Count(200), Answer::Text("no capability")),
    ),
    (
        "bake_expected_success",
        each(Answer::Text("true"), Answer::Text("no capability")),
    ),
    (
        "bake_expected_versions",
        each(
            Answer::Text("cof_version=asked"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_incremented_asked",
        each(Answer::Text("current+1"), Answer::Text("no capability")),
    ),
    (
        "bake_incremented_core_keys",
        each(
            Answer::Text("agent_id cof_version error success warnings"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_incremented_error",
        each(Answer::Text("undef"), Answer::Text("no capability")),
    ),
    (
        "bake_incremented_own_appearances",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "bake_incremented_own_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_incremented_own_hover",
        each(Answer::Text("hover"), Answer::Text("no capability")),
    ),
    (
        "bake_incremented_status",
        each(Answer::Count(200), Answer::Text("no capability")),
    ),
    (
        "bake_incremented_success",
        each(Answer::Text("true"), Answer::Text("no capability")),
    ),
    (
        "bake_incremented_versions",
        each(
            Answer::Text("cof_version=asked"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_restore_asked",
        each(Answer::Text("current"), Answer::Text("no capability")),
    ),
    (
        "bake_restore_core_keys",
        each(
            Answer::Text("agent_id cof_version error success warnings"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_restore_error",
        each(Answer::Text("undef"), Answer::Text("no capability")),
    ),
    (
        "bake_restore_own_appearances",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "bake_restore_own_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_restore_own_hover",
        each(Answer::Text("hover"), Answer::Text("no capability")),
    ),
    (
        "bake_restore_status",
        each(Answer::Count(200), Answer::Text("no capability")),
    ),
    (
        "bake_restore_success",
        each(Answer::Text("true"), Answer::Text("no capability")),
    ),
    (
        "bake_restore_versions",
        each(
            Answer::Text("cof_version=asked"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_stale_asked",
        each(Answer::Text("0"), Answer::Text("no capability")),
    ),
    (
        "bake_stale_core_keys",
        each(
            Answer::Text("agent_id code cof_version error expected observed success"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_stale_error",
        each(
            Answer::Text("Cof Version Mismatch: asked != current"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_stale_own_appearances",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "bake_stale_own_block",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "bake_stale_own_hover",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "bake_stale_status",
        each(Answer::Count(400), Answer::Text("no capability")),
    ),
    (
        "bake_stale_success",
        each(Answer::Text("false"), Answer::Text("no capability")),
    ),
    (
        "bake_stale_versions",
        each(
            Answer::Text("cof_version=asked expected=current observed=asked"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "cache_made_up",
        each(
            Answer::Text("nothing"),
            Answer::Text("11 of 11 entries in order, no hits"),
        ),
    ),
    (
        "cache_nil",
        each(
            Answer::Text("nothing"),
            Answer::Text("11 of 11 entries in order, no hits"),
        ),
    ),
    (
        "cache_published",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "11 of 11 entries in order, hits 8=published 9=published 10=published 11=published 19=published 20=published 40=published 41=published 42=published 43=published 44=published",
            ),
        ),
    ),
    ("cache_unasked_answers", both(Answer::Count(0))),
    (
        "cap_increment",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("cap_update", each(Answer::Flag(true), Answer::Flag(false))),
    ("cap_upload", each(Answer::Flag(false), Answer::Flag(true))),
    (
        "central_bake_bit",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "client_bake_stale",
        each(
            Answer::Text(
                "success false error \"Cof Version Mismatch: asked != current\" expected current",
            ),
            Answer::Text("no capability"),
        ),
    ),
    (
        "client_bake_stale_own_appearances",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "client_bake_stale_own_block",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "client_bake_stale_own_hover",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "increment",
        each(Answer::Text("current+1"), Answer::Text("no capability")),
    ),
    (
        "login_cof_named",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    ("publish_own_appearances", both(Answer::Count(0))),
    ("publish_own_block", both(Answer::Text("none"))),
    ("publish_own_hover", both(Answer::Text("none"))),
    ("publish_rebakes", both(Answer::Text(""))),
    ("restore_own", both(Answer::Text("no appearance"))),
    (
        "service_head",
        each(
            Answer::Text("200 image/x-j2c: a J2C codestream"),
            Answer::Text("no service"),
        ),
    ),
    (
        "service_named",
        each(Answer::Flag(true), Answer::Flag(false)),
    ),
    (
        "service_nobodys",
        each(
            Answer::Text(
                "404 application/llsd+xml: {agent_id=<id>,code=2,debug_cof=undef,error=\"Hash mismatch: <id>\",success=false}",
            ),
            Answer::Text("no service"),
        ),
    ),
    (
        "service_shape",
        each(Answer::Text("http, ends in /"), Answer::Text("none")),
    ),
];

/// What each grid was measured to tell B of A: held on the live grids only.
const MEASURED_LIVE: &[(&str, Measured<Answer>)] = &[
    ("arrival_seen_bakes_as_own", both(Answer::Text("the same"))),
    (
        "arrival_seen_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no block"),
        ),
    ),
    ("arrival_seen_hover", both(Answer::Text("hover"))),
    (
        "bake_again_seen_appearances",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "bake_again_seen_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_again_seen_hover",
        each(Answer::Text("hover"), Answer::Text("no capability")),
    ),
    (
        "bake_ahead_seen_appearances",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "bake_ahead_seen_block",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "bake_ahead_seen_hover",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "bake_expected_seen_appearances",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "bake_expected_seen_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_expected_seen_hover",
        each(Answer::Text("hover"), Answer::Text("no capability")),
    ),
    (
        "bake_incremented_seen_appearances",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "bake_incremented_seen_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_incremented_seen_hover",
        each(Answer::Text("hover"), Answer::Text("no capability")),
    ),
    (
        "bake_restore_seen_appearances",
        each(Answer::Count(1), Answer::Text("no capability")),
    ),
    (
        "bake_restore_seen_block",
        each(
            Answer::Text("version 1 cof current flags 0"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "bake_restore_seen_hover",
        each(Answer::Text("hover"), Answer::Text("no capability")),
    ),
    (
        "bake_stale_seen_appearances",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "bake_stale_seen_block",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "bake_stale_seen_hover",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "client_bake_stale_seen_appearances",
        each(Answer::Count(0), Answer::Text("no capability")),
    ),
    (
        "client_bake_stale_seen_block",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "client_bake_stale_seen_hover",
        each(Answer::Text("none"), Answer::Text("no capability")),
    ),
    (
        "publish_seen_appearance",
        each(
            Answer::Text("none"),
            Answer::Text(
                "no block; 253 params; published head upper lower eyes skirt hair left_arm left_leg aux1 aux2 aux3; hover; no attachments",
            ),
        ),
    ),
    (
        "publish_seen_appearances",
        each(Answer::Count(0), Answer::Count(1)),
    ),
    (
        "publish_seen_block",
        each(Answer::Text("none"), Answer::Text("no block")),
    ),
    (
        "publish_seen_hover",
        each(Answer::Text("none"), Answer::Text("hover")),
    ),
    (
        "restore_seen",
        each(
            Answer::Text("no appearance"),
            Answer::Text("the arrival's bakes"),
        ),
    ),
];

/// The appearance-baking census.
#[derive(Debug)]
pub struct AppearanceBaking;

impl GridTest for AppearanceBaking {
    fn name(&self) -> &'static str {
        "appearance-baking"
    }

    fn description(&self) -> &'static str {
        "Record how each grid bakes an avatar: the appearance service, the central-bake bit \
         and capabilities, the appearance the agent and a second viewer are sent, the \
         baked-texture cache, the COF-version handshake and the legacy publish"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Opensim, Grid::Aditi, Grid::FakeSl, Grid::FakeOpensim]
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
        Duration::from_secs(600)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            outcome
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

/// What one viewer was told of A, and A's own answers.
#[derive(Debug, Default)]
struct View {
    /// Every `AvatarAppearance` of A this viewer was sent, in order.
    appearances: Vec<AvatarAppearance>,
    /// The cache answers, by serial.
    cached: Vec<(i32, Vec<(u8, Uuid)>)>,
    /// The bakes the grid asked to have re-uploaded.
    rebakes: Vec<Uuid>,
    /// Our client's reading of a bake request's answer.
    bake_replies: Vec<(bool, Option<String>, Option<i32>)>,
    /// Our client's reading of an `IncrementCOFVersion` answer.
    increments: Vec<Option<i32>>,
}

impl View {
    /// Folds one event into the view; `actor` is A's id.
    fn note(&mut self, actor: Uuid, event: &Event) {
        match event {
            Event::AvatarAppearance(appearance) if appearance.avatar_id.uuid() == actor => {
                self.appearances.push((**appearance).clone());
            }
            Event::CachedTextureResponse { serial, textures } => {
                self.cached.push((*serial, textures.clone()));
            }
            Event::RebakeAvatarTextures { texture_id } => self.rebakes.push(texture_id.uuid()),
            Event::ServerAppearanceUpdate {
                success,
                error,
                expected_cof_version,
            } => self
                .bake_replies
                .push((*success, error.clone(), *expected_cof_version)),
            Event::CofVersionIncremented { version } => self.increments.push(*version),
            _ => {}
        }
    }
}

/// Listens on `session` for `window`, folding everything into `view`.
async fn listen(
    session: &mut Session,
    view: &mut View,
    actor: Uuid,
    window: Duration,
) -> Result<(), TestFailure> {
    // Nothing is ever accepted, so the wait reads every event of the window.
    match session
        .wait_for(window, |event| {
            view.note(actor, event);
            None::<()>
        })
        .await
    {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// Everything the legs share.
struct Census<'a> {
    /// Avatar A's session.
    a: &'a mut Session,
    /// Avatar B's session.
    b: &'a mut Session,
    /// A's id.
    actor: Uuid,
    /// What A was told.
    own: View,
    /// What B was told of A.
    seen: View,
    /// How long a leg listens.
    window: Duration,
    /// The COF version the grid last stated, where it stated one.
    current: Option<i32>,
    /// The HTTP client for the raw capability requests.
    http: reqwest::Client,
}

impl Census<'_> {
    /// Listens to both avatars for one window.
    async fn listen(&mut self) -> Result<(), TestFailure> {
        let actor = self.actor;
        let window = self.window;
        let (own, seen) = tokio::join!(
            listen(self.a, &mut self.own, actor, window),
            listen(self.b, &mut self.seen, actor, window)
        );
        own.and(seen)
    }

    /// How many appearances each viewer had been sent, for a later
    /// [`Census::record_appearances`].
    const fn mark(&self) -> (usize, usize) {
        (self.own.appearances.len(), self.seen.appearances.len())
    }

    /// Records the appearances each viewer was sent since `from` under `key`:
    /// how many, and the last one of each described.
    fn record_appearances(
        &self,
        metrics: &mut Metrics,
        key: &str,
        from: (usize, usize),
        published: Option<Uuid>,
    ) {
        for (side, view, start) in [("own", &self.own, from.0), ("seen", &self.seen, from.1)] {
            let new = view.appearances.get(start..).unwrap_or_default();
            metrics.set(
                &format!("{key}_{side}_appearances"),
                i64::try_from(new.len()).unwrap_or(i64::MAX),
            );
            let described = new.last().map_or_else(
                || "none".to_owned(),
                |last| describe_appearance(last, self.current, published),
            );
            metrics.set(&format!("{key}_{side}_appearance"), described);
            let (block, hover) = new.last().map_or_else(
                || ("none".to_owned(), "none"),
                |last| (describe_block(last, self.current), describe_hover(last)),
            );
            metrics.set(&format!("{key}_{side}_block"), block);
            metrics.set(&format!("{key}_{side}_hover"), hover);
        }
    }

    /// POSTs a bake of `cof_version` raw, records the answer under `key`,
    /// listens and records the appearances sent after it. Returns the version
    /// the answer said it expected, if it named one.
    async fn bake(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        cof_version: Option<i32>,
    ) -> Result<Option<i32>, TestFailure> {
        let Some(cap) = self.a.cap(UPDATE_CAP) else {
            record_unasked(metrics, key, NO_CAPABILITY);
            return Ok(None);
        };
        let Some(cof_version) = cof_version else {
            record_unasked(metrics, key, "no version to ask for");
            return Ok(None);
        };
        let asked = if cof_version == 0 {
            "0".to_owned()
        } else {
            relation(cof_version, self.current)
        };
        let from = self.mark();
        let body = sl_wire::build_update_avatar_appearance_request(cof_version);
        let reply = self
            .http
            .post(&cap)
            .header(CONTENT_TYPE, "application/llsd+xml")
            .body(body)
            .send()
            .await;
        metrics.set(&format!("{key}_asked"), asked);
        let mut expected = None;
        let mut success = false;
        match reply {
            Ok(response) => {
                metrics.set(
                    &format!("{key}_status"),
                    i64::from(response.status().as_u16()),
                );
                let text = response.text().await.unwrap_or_default();
                let llsd = Some(text.as_str())
                    .filter(|text| text.contains("<llsd"))
                    .and_then(|text| parse_llsd_xml(text).ok());
                if let Some(body) = llsd {
                    expected = body.get("expected").and_then(llsd_i32);
                    success = matches!(body.get("success"), Some(Llsd::Boolean(true)));
                    let answer = BakeAnswer::read(&body, cof_version, self.current);
                    metrics.set(&format!("{key}_success"), answer.success);
                    metrics.set(&format!("{key}_error"), answer.error);
                    metrics.set(&format!("{key}_versions"), answer.versions);
                    metrics.set(&format!("{key}_keys"), answer.keys);
                    metrics.set(&format!("{key}_core_keys"), answer.core_keys);
                } else {
                    let words = format!("not LLSD: \"{}\"", body_words(&text));
                    for suffix in ["success", "error", "versions", "keys", "core_keys"] {
                        metrics.set(&format!("{key}_{suffix}"), words.as_str());
                    }
                }
            }
            Err(error) => {
                let failure = format!("no answer: {}", without_ids(&error.to_string()));
                for suffix in [
                    "status",
                    "success",
                    "error",
                    "versions",
                    "keys",
                    "core_keys",
                ] {
                    metrics.set(&format!("{key}_{suffix}"), failure.as_str());
                }
            }
        }
        self.listen().await?;
        if success {
            self.current = Some(cof_version);
        }
        self.record_appearances(metrics, key, from, None);
        Ok(expected)
    }
}

/// The suffixes of a bake leg's answer metrics.
const ANSWER_SUFFIXES: [&str; 7] = [
    "asked",
    "status",
    "success",
    "error",
    "versions",
    "keys",
    "core_keys",
];

/// The suffixes of the appearances a leg records for each viewer.
const APPEARANCE_SUFFIXES: [&str; 4] = ["appearances", "appearance", "block", "hover"];

/// Records `why` under every metric of a bake leg that was never asked, so
/// each key exists on every grid.
fn record_unasked(metrics: &mut Metrics, key: &str, why: &str) {
    for suffix in ANSWER_SUFFIXES {
        metrics.set(&format!("{key}_{suffix}"), why);
    }
    record_no_appearances(metrics, key, why);
}

/// Records `why` under every appearance metric of a leg that was not run.
fn record_no_appearances(metrics: &mut Metrics, key: &str, why: &str) {
    for side in ["own", "seen"] {
        for suffix in APPEARANCE_SUFFIXES {
            metrics.set(&format!("{key}_{side}_{suffix}"), why);
        }
    }
}

/// An LLSD value as an `i32`, where it is a number.
fn llsd_i32(value: &Llsd) -> Option<i32> {
    match value {
        Llsd::Integer(number) => Some(*number),
        Llsd::Real(number) => format!("{number:.0}").parse().ok(),
        Llsd::String(text) => text.parse().ok(),
        _ => None,
    }
}

/// A bake answer as the metrics record it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BakeAnswer {
    /// `success`, or `absent`.
    success: String,
    /// `error`, every version in it named by what it is.
    error: String,
    /// The answer's version keys (`cof_version`, `expected`, `observed`),
    /// each named by what it is.
    versions: String,
    /// The answer's keys, sorted.
    keys: String,
    /// [`keys`](Self::keys) without the bake's own payload
    /// ([`PAYLOAD_KEYS`]).
    core_keys: String,
}

/// The keys of a bake answer that carry the bake itself, or on a refusal a
/// dump of the outfit, rather than the answer: Second Life's, which the fake
/// grid does not fabricate.
const PAYLOAD_KEYS: [&str; 5] = [
    "avatar_scale",
    "cof_raw",
    "debug_cof",
    "textures",
    "visual_params",
];

impl BakeAnswer {
    /// Reads `body`, the answer to a bake of `asked` while `current` was the
    /// version last stated.
    fn read(body: &Llsd, asked: i32, current: Option<i32>) -> Self {
        let Llsd::Map(map) = body else {
            let said = rendered(body, None);
            return Self {
                success: said.clone(),
                error: said.clone(),
                versions: said.clone(),
                keys: said.clone(),
                core_keys: said,
            };
        };
        let success = match map.get("success") {
            Some(Llsd::Boolean(flag)) => flag.to_string(),
            Some(other) => rendered(other, Some("success")),
            None => "absent".to_owned(),
        };
        let error = match map.get("error") {
            None => "absent".to_owned(),
            Some(Llsd::Undef) => "undef".to_owned(),
            Some(Llsd::String(text)) => named_versions(text, asked, current),
            Some(other) => rendered(other, Some("error")),
        };
        let versions: Vec<String> = ["cof_version", "expected", "observed"]
            .into_iter()
            .filter_map(|key| {
                let value = map.get(key)?;
                let said = llsd_i32(value).map_or_else(
                    || rendered(value, Some(key)),
                    |version| version_word(version, asked, current),
                );
                Some(format!("{key}={said}"))
            })
            .collect();
        let mut keys: Vec<&str> = map.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let core_keys: Vec<&str> = keys
            .iter()
            .copied()
            .filter(|key| !PAYLOAD_KEYS.contains(key))
            .collect();
        Self {
            success,
            error,
            versions: if versions.is_empty() {
                "none".to_owned()
            } else {
                versions.join(" ")
            },
            keys: keys.join(" "),
            core_keys: core_keys.join(" "),
        }
    }
}

/// What a version in an answer is: the one `asked`, the `current` one, or
/// how far from the current one it lies.
fn version_word(version: i32, asked: i32, current: Option<i32>) -> String {
    if version == asked {
        "asked".to_owned()
    } else if Some(version) == current {
        "current".to_owned()
    } else {
        relation(version, current)
    }
}

/// `text` with every number in it replaced by [`version_word`].
fn named_versions(text: &str, asked: i32, current: Option<i32>) -> String {
    text.split(' ')
        .map(|word| {
            word.parse::<i32>()
                .map_or_else(|_| word.to_owned(), |n| version_word(n, asked, current))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `version` against the version last stated: `current`, `current+N`,
/// `current-N`, or `N (none stated)` where none was.
fn relation(version: i32, current: Option<i32>) -> String {
    match current {
        None => format!("{version} (none stated)"),
        Some(current) => match version.saturating_sub(current) {
            0 => "current".to_owned(),
            above if above > 0 => format!("current+{above}"),
            below => format!("current{below}"),
        },
    }
}

/// The first words of a body that is not LLSD, ids scrubbed.
fn body_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(8).collect();
    if words.is_empty() {
        return "empty".to_owned();
    }
    without_ids(&words.join(" "))
}

/// An appearance in words: the `AppearanceData` block (its version, its COF
/// version against `current`, its flags), the visual parameters, the baked
/// slots, the hover and the attachment blocks.
fn describe_appearance(
    appearance: &AvatarAppearance,
    current: Option<i32>,
    published: Option<Uuid>,
) -> String {
    format!(
        "{}; {} params; {}; {}; {}",
        describe_block(appearance, current),
        appearance.visual_params.len(),
        describe_bakes(&appearance.texture_entry, published),
        describe_hover(appearance),
        if appearance.attachments.is_empty() {
            "no attachments"
        } else {
            "attachments"
        }
    )
}

/// An appearance's `AppearanceHover` block, in a word.
const fn describe_hover(appearance: &AvatarAppearance) -> &'static str {
    if appearance.hover_height.is_some() {
        "hover"
    } else {
        "no hover"
    }
}

/// An appearance's `AppearanceData` block in words: its version, its COF
/// version against `current` and its flags, or `no block`.
fn describe_block(appearance: &AvatarAppearance, current: Option<i32>) -> String {
    match appearance.appearance_version {
        None => "no block".to_owned(),
        Some(version) => {
            let cof = appearance
                .cof_version
                .map_or_else(|| "none".to_owned(), |cof| relation(cof, current));
            let flags = appearance
                .appearance_flags
                .map_or_else(|| "none".to_owned(), |flags| flags.to_string());
            format!("version {version} cof {cof} flags {flags}")
        }
    }
}

/// The baked slots in words: which hold a bake, which the default
/// placeholder, which the `published` texture, and which nothing.
fn describe_bakes(entry: &TextureEntry, published: Option<Uuid>) -> String {
    let mut groups: Vec<(&str, Vec<&str>)> = vec![
        ("baked", Vec::new()),
        ("published", Vec::new()),
        ("default", Vec::new()),
        ("nil", Vec::new()),
    ];
    for (slot, name) in avatar_texture::BAKED {
        let id = entry
            .faces
            .get(slot)
            .map_or_else(Uuid::nil, |face| face.texture_id.uuid());
        let group = if id.is_nil() {
            "nil"
        } else if Some(id) == published {
            "published"
        } else if id == avatar_texture::IMG_DEFAULT_AVATAR {
            "default"
        } else {
            "baked"
        };
        if let Some((_group, names)) = groups.iter_mut().find(|(word, _names)| *word == group) {
            names.push(name);
        }
    }
    let said: Vec<String> = groups
        .into_iter()
        .filter(|(_group, names)| !names.is_empty())
        .map(|(group, names)| format!("{group} {}", names.join(" ")))
        .collect();
    said.join(", ")
}

/// The baked slots an `AgentCachedTexture` asks about, with the cache id
/// `base` gives each (nil for a nil `base`).
fn query_slots(base: u128) -> Vec<(Uuid, u8)> {
    avatar_texture::BAKED
        .iter()
        .filter_map(|&(slot, _name)| {
            let index = u8::try_from(slot).ok()?;
            let id = if base == 0 {
                Uuid::nil()
            } else {
                Uuid::from_u128(base | u128::from(index))
            };
            Some((id, index))
        })
        .collect()
}

/// A cache answer in words, against what A's arrival appearance holds and the
/// `published` texture.
fn describe_cache(
    answer: Option<&[(u8, Uuid)]>,
    asked: usize,
    arrival: Option<&AvatarAppearance>,
    published: Option<Uuid>,
) -> String {
    let Some(answer) = answer else {
        return "nothing".to_owned();
    };
    let mut hits = Vec::new();
    for (slot, id) in answer {
        if id.is_nil() {
            continue;
        }
        let worn = arrival
            .and_then(|appearance| appearance.texture_entry.faces.get(usize::from(*slot)))
            .map(|face| face.texture_id.uuid());
        let what = if Some(*id) == published {
            "published"
        } else if Some(*id) == worn {
            "worn"
        } else {
            "other"
        };
        hits.push(format!("{slot}={what}"));
    }
    let order = if answer
        .iter()
        .map(|(slot, _id)| *slot)
        .eq(query_slots(0).into_iter().map(|(_id, slot)| slot))
    {
        "in order"
    } else {
        "out of order"
    };
    let hits = if hits.is_empty() {
        "no hits".to_owned()
    } else {
        format!("hits {}", hits.join(" "))
    };
    format!("{} of {asked} entries {order}, {hits}", answer.len())
}

/// The plywood texture on every baked slot, the rest nil.
fn plywood_entry(plywood: TextureKey) -> TextureEntry {
    let nil = TextureFace::new(TextureKey::from(Uuid::nil()));
    let mut faces = vec![nil; avatar_texture::COUNT];
    for (slot, _name) in avatar_texture::BAKED {
        if let Some(face) = faces.get_mut(slot) {
            *face = TextureFace::new(plywood);
        }
    }
    TextureEntry { faces }
}

/// Runs every leg, recording into `metrics`.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let build_position = ctx.build_position();
    let (window, settle_idle) = if grid.is_fake() {
        (FAKE_WINDOW, FAKE_SETTLE_IDLE)
    } else {
        (WINDOW, SETTLE_IDLE)
    };
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "appearance-baking needs a secondary avatar".to_owned(),
        ));
    };
    let actor = a
        .agent_id()
        .map(|agent| agent.uuid())
        .ok_or_else(|| TestFailure::Assertion("login reported no agent id".to_owned()))?;

    // --- 1. The service. ---
    let (service, login_cof) = a.login_success().map_or((None, None), |login| {
        (login.agent_appearance_service.clone(), login.cof_version)
    });
    metrics.set("service_named", service.is_some());
    metrics.set(
        "service_shape",
        service.as_ref().map_or_else(
            || "none".to_owned(),
            |url| {
                let path_ends = if url.path().ends_with('/') {
                    "ends in /"
                } else {
                    "no trailing /"
                };
                format!("{}, {path_ends}", url.scheme())
            },
        ),
    );
    metrics.set("login_cof_named", login_cof.is_some());
    a.wait_for_region(REGION_TIMEOUT).await?;
    b.wait_for_region(REGION_TIMEOUT).await?;
    metrics.set(
        "central_bake_bit",
        a.region_identity()
            .is_some_and(|identity| identity.region_protocols & 1 != 0),
    );
    for (word, cap) in [
        ("update", UPDATE_CAP),
        ("increment", INCREMENT_CAP),
        ("upload", UPLOAD_CAP),
    ] {
        metrics.set(&format!("cap_{word}"), a.cap(cap).is_some());
    }

    // --- 2. Arrival. ---
    let mut own = View::default();
    let _settled = settle_scene_observing(
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
    let http = reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        // A fresh connection per request: a "no answer" is then the grid's,
        // never a kept-alive connection the far end had already closed.
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|error| TestFailure::State(format!("building an HTTP client failed: {error}")))?;
    let mut census = Census {
        a,
        b,
        actor,
        own,
        seen,
        window,
        current: login_cof,
        http,
    };
    census.record_appearances(metrics, "arrival", (0, 0), None);
    let arrival = census.own.appearances.last().cloned();
    metrics.set(
        "arrival_seen_bakes_as_own",
        match (arrival.as_ref(), census.seen.appearances.last()) {
            (Some(own), Some(seen)) => {
                if baked_ids(own) == baked_ids(seen) {
                    "the same"
                } else {
                    "different"
                }
            }
            (None, _) => "no own appearance",
            (Some(_), None) => "none seen",
        },
    );

    // --- 3. The appearance service. ---
    service_fetch(&census, metrics, service.as_ref(), arrival.as_ref()).await;

    // --- 4. The cache. ---
    for (key, serial, base) in [
        ("cache_nil", SERIAL_NIL, 0),
        ("cache_made_up", SERIAL_MADE_UP, MADE_UP_CACHE),
    ] {
        let slots = query_slots(base);
        let asked = slots.len();
        census
            .a
            .send(Command::RequestCachedTextures { serial, slots })
            .await?;
        census.listen().await?;
        let answer = census
            .own
            .cached
            .iter()
            .find(|(answered, _textures)| *answered == serial)
            .map(|(_serial, textures)| textures.as_slice());
        metrics.set(key, describe_cache(answer, asked, arrival.as_ref(), None));
    }
    let unasked = census
        .own
        .cached
        .iter()
        .filter(|(serial, _textures)| ![SERIAL_NIL, SERIAL_MADE_UP].contains(serial))
        .count();
    metrics.set(
        "cache_unasked_answers",
        i64::try_from(unasked).unwrap_or(i64::MAX),
    );

    // --- 5. The bake handshake. ---
    let expected = census.bake(metrics, "bake_stale", Some(0)).await?;
    let target = expected.or(census.current);
    let _expected = census.bake(metrics, "bake_expected", target).await?;
    let again = census.current;
    let _expected = census.bake(metrics, "bake_again", again).await?;
    let ahead = census.current.map(|current| current.saturating_add(AHEAD));
    let _expected = census.bake(metrics, "bake_ahead", ahead).await?;
    client_bake(&mut census, metrics).await?;
    increment(&mut census, metrics).await?;

    // --- 6. The legacy publish. ---
    let plywood = fixtures::plywood_texture();
    let from = census.mark();
    let rebakes_before = census.own.rebakes.len();
    census
        .a
        .send(Command::SetAppearance {
            serial: 1,
            size: AVATAR_SIZE,
            texture_entry: encode_texture_entry(&plywood_entry(plywood)),
            visual_params: vec![NEUTRAL_VISUAL_PARAM; VISUAL_PARAM_COUNT],
            wearable_cache: query_slots(PUBLISHED_CACHE),
        })
        .await?;
    census.listen().await?;
    census.record_appearances(metrics, "publish", from, Some(plywood.uuid()));
    metrics.set(
        "publish_rebakes",
        census
            .own
            .rebakes
            .get(rebakes_before..)
            .unwrap_or_default()
            .iter()
            .map(|id| {
                if *id == plywood.uuid() {
                    "published"
                } else {
                    "other"
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    );
    let slots = query_slots(PUBLISHED_CACHE);
    let asked = slots.len();
    census
        .a
        .send(Command::RequestCachedTextures {
            serial: SERIAL_PUBLISHED,
            slots,
        })
        .await?;
    census.listen().await?;
    let answer = census
        .own
        .cached
        .iter()
        .find(|(answered, _textures)| *answered == SERIAL_PUBLISHED)
        .map(|(_serial, textures)| textures.as_slice());
    metrics.set(
        "cache_published",
        describe_cache(answer, asked, arrival.as_ref(), Some(plywood.uuid())),
    );

    // --- 7. The restore. ---
    if let Some(arrival) = arrival.as_ref() {
        let from = census.mark();
        census
            .a
            .send(Command::SetAppearance {
                serial: 2,
                size: AVATAR_SIZE,
                texture_entry: encode_texture_entry(&arrival.texture_entry),
                visual_params: arrival.visual_params.clone(),
                wearable_cache: Vec::new(),
            })
            .await?;
        census.listen().await?;
        for (side, view, start) in [("own", &census.own, from.0), ("seen", &census.seen, from.1)] {
            let restored = view
                .appearances
                .get(start..)
                .unwrap_or_default()
                .last()
                .map_or("no appearance", |last| {
                    if baked_ids(last) == baked_ids(arrival) {
                        "the arrival's bakes"
                    } else {
                        "other bakes"
                    }
                });
            metrics.set(&format!("restore_{side}"), restored);
        }
    } else {
        metrics.set("restore_own", "no arrival appearance");
        metrics.set("restore_seen", "no arrival appearance");
    }
    let current = census.current;
    let _expected = census.bake(metrics, "bake_restore", current).await?;
    Ok(())
}

/// The ids on an appearance's baked slots, in slot order.
fn baked_ids(appearance: &AvatarAppearance) -> Vec<Uuid> {
    avatar_texture::BAKED
        .iter()
        .map(|&(slot, _name)| {
            appearance
                .texture_entry
                .faces
                .get(slot)
                .map_or_else(Uuid::nil, |face| face.texture_id.uuid())
        })
        .collect()
}

/// Fetches A's head bake and a texture nobody baked from the appearance
/// service the way the reference builds the URL, recording each answer.
async fn service_fetch(
    census: &Census<'_>,
    metrics: &mut Metrics,
    service: Option<&url::Url>,
    arrival: Option<&AvatarAppearance>,
) {
    let Some(service) = service else {
        metrics.set("service_head", "no service");
        metrics.set("service_nobodys", "no service");
        return;
    };
    let head = arrival
        .and_then(|appearance| {
            appearance
                .texture_entry
                .faces
                .get(avatar_texture::HEAD_BAKED)
        })
        .map(|face| face.texture_id.uuid())
        .filter(|id| !id.is_nil());
    let agent = census.actor;
    for (key, texture) in [
        ("service_head", head),
        ("service_nobodys", Some(NOBODYS_BAKE)),
    ] {
        let Some(texture) = texture else {
            metrics.set(key, "no head bake to ask for");
            continue;
        };
        let url = format!("{service}texture/{agent}/head/{texture}");
        let said = match census.http.get(&url).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let media = response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map_or_else(|| "no type".to_owned(), without_ids);
                let bytes = response.bytes().await.unwrap_or_default();
                let body = if bytes.starts_with(&[0xff, 0x4f, 0xff, 0x51]) {
                    "a J2C codestream".to_owned()
                } else if bytes.is_empty() {
                    "no body".to_owned()
                } else {
                    let text = String::from_utf8_lossy(&bytes);
                    match Some(text.as_ref())
                        .filter(|text| text.contains("<llsd"))
                        .and_then(|text| parse_llsd_xml(text).ok())
                    {
                        Some(llsd) => rendered(&llsd, None),
                        None => format!(
                            "\"{}\"",
                            body_words(&text).chars().take(60).collect::<String>()
                        ),
                    }
                };
                format!("{status} {media}: {body}")
            }
            Err(error) => format!("no answer: {}", without_ids(&error.to_string())),
        };
        metrics.set(key, said);
    }
}

/// Our client's own bake request at version 0: what it reads the answer as.
async fn client_bake(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    if census.a.cap(UPDATE_CAP).is_none() {
        metrics.set("client_bake_stale", NO_CAPABILITY);
        record_no_appearances(metrics, "client_bake_stale", NO_CAPABILITY);
        return Ok(());
    }
    let before = census.own.bake_replies.len();
    let from = census.mark();
    census
        .a
        .send(Command::RequestServerAppearanceUpdate { cof_version: 0 })
        .await?;
    census.listen().await?;
    let current = census.current;
    let said = census
        .own
        .bake_replies
        .get(before..)
        .unwrap_or_default()
        .iter()
        .map(|(success, error, expected)| {
            let error = error.as_deref().map_or_else(
                || "none".to_owned(),
                |error| named_versions(&without_ids(error), 0, current),
            );
            let expected =
                expected.map_or_else(|| "none".to_owned(), |expected| relation(expected, current));
            format!("success {success} error \"{error}\" expected {expected}")
        })
        .collect::<Vec<_>>();
    metrics.set(
        "client_bake_stale",
        if said.is_empty() {
            "nothing".to_owned()
        } else {
            said.join("; ")
        },
    );
    census.record_appearances(metrics, "client_bake_stale", from, None);
    Ok(())
}

/// An `IncrementCOFVersion` through our client, then a bake at the version it
/// gave.
async fn increment(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    if census.a.cap(INCREMENT_CAP).is_none() {
        metrics.set("increment", NO_CAPABILITY);
        record_unasked(metrics, "bake_incremented", NO_CAPABILITY);
        return Ok(());
    }
    let before = census.own.increments.len();
    census.a.send(Command::IncrementCofVersion).await?;
    census.listen().await?;
    let answered = census
        .own
        .increments
        .get(before..)
        .unwrap_or_default()
        .first()
        .copied();
    let current = census.current;
    metrics.set(
        "increment",
        match answered {
            None => "nothing".to_owned(),
            Some(None) => "no version".to_owned(),
            Some(Some(version)) => relation(version, current),
        },
    );
    let incremented = answered.flatten();
    let _expected = census
        .bake(metrics, "bake_incremented", incremented)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{query_slots, relation};
    use pretty_assertions::assert_eq;

    /// A version is said against the one last stated.
    #[test]
    fn a_version_is_recorded_against_the_current_one() {
        assert_eq!(relation(7, Some(7)), "current");
        assert_eq!(relation(12, Some(7)), "current+5");
        assert_eq!(relation(0, Some(7)), "current-7");
        assert_eq!(relation(3, None), "3 (none stated)");
    }

    /// The cache is asked about the eleven baked slots, nil or made up.
    #[test]
    fn the_cache_is_asked_about_every_baked_slot() {
        assert_eq!(query_slots(0).len(), 11);
        assert!(query_slots(0).iter().all(|(id, _slot)| id.is_nil()));
        assert!(query_slots(1 << 64).iter().all(|(id, _slot)| !id.is_nil()));
    }
}
