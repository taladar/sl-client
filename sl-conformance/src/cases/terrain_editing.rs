//! A census of terrain editing: what each brush does to the ground and who is
//! told, the limits a region keeps an edit within, undo, bake, the RAW
//! heightmap transfers, the terrain textures and heights, and the Second Life
//! `ModifyRegion` capability — for an avatar with the rights and for one
//! without.
//!
//! Avatar **A** holds estate rights where the test avatars do (OpenSim:
//! `--avatar estate-owner --secondary resident`, both in the Default Region);
//! avatar **B** is a resident. On Second Life neither holds rights
//! (`gridspec-aditi-test-land`): both stand at the fixtures' build location
//! and A's legs measure what a resident is answered there.
//!
//! The brush lands 8 m north-east of where both avatars stand, so no avatar is
//! lifted by its own edit; on OpenSim that is the middle of patch (8, 8), so a
//! 4 m brush stays inside one patch. Every height is read off the land patches
//! each avatar was sent (`Session::ground_height`), so "what changed" is what
//! a viewer would draw.
//!
//! One question a leg:
//!
//! 1. **The setting**: the region's terrain limits and `BLOCK_TERRAFORM`, the
//!    parcel under the brush — whose it is and whether it allows terraforming.
//! 2. **The capability** `ModifyRegion`, read raw: A's and B's GET, B's POST of
//!    the overrides it read.
//! 3. **A's brushes**, one stroke each, point form: raise, lower, flatten,
//!    smooth, roughen, revert; then the area form (a raise over a rectangle,
//!    with the parcel's id) and its revert. For each: the change at the
//!    brush's centre, and what each avatar was told — how many land patches,
//!    where, how soon.
//! 4. **Undo**: a raise, then `UndoLand`.
//! 5. **The limits** (A's rights only): the region's raise and lower limits
//!    set to 2 m over `setregionterrain`, then a 5 m raise by A; then B made
//!    an estate manager and a 5 m raise and a 5 m lower by B — OpenSim's
//!    region owner is a god by default, and a god's edits are not limited.
//!    How far each got. B's rights and the limits restored.
//! 6. **Bake** (A's rights only): a raise, a bake, a revert — whether the
//!    revert still undoes the raise; then the raise lowered back and baked
//!    again, so the region is left as it was.
//! 7. **The textures and heights** (A's rights only): one corner's start
//!    height raised over the four terrain-tab messages; what each avatar is
//!    told, and the handshake's composition; restored.
//! 8. **B's refusals**: a raise, a bake, the texture write and a RAW download,
//!    each as B; on Second Life also A's, as the resident it is there.
//! 9. **The RAW heightmap**: A's download (its size and how long it took)
//!    and B's, before anything else; at the end an upload of the downloaded
//!    bytes back by A — what each avatar is told of an upload that changes
//!    nothing — and by B.
//!
//! Where A holds rights the census bakes the ground as found before its first
//! stroke (`bake_start`): a revert goes to the baked heightmap, which need not
//! be the ground as it stands.
//!
//! Every id is described by whose it is and every name is scrubbed, so no
//! metric names an avatar. What each live grid answered is stated as a
//! [`Measured`] and written up in `book/src/gridspec/terrain.md` § Editing;
//! every run is held to it.
//!
//! The ground is left as found: every stroke that changed it is reverted, and
//! where the revert leaves more than a centimetre of difference under the
//! brush, the RAW heightmap downloaded before the first stroke is uploaded
//! back.
//!
//! `2av`.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use sl_client_tokio::{
    Command, EstateAccessDelta, Event, LandBrushAction, LandBrushSize, LandEdit, OwnerKey,
    ParcelFlags, ParcelInfo, RegionFlags, RegionLimits, RegionLocalParcelId, RegionTerrainUpdate,
    TerraformArea, Throttle, Vector,
};

use crate::cases::estate::Scene;
use crate::cases::groups::names_of;
use crate::cases::land_transactions::{region_named, teleport};
use crate::cases::object_rez_derez::Answer;
use crate::cases::region_info::raw_cap;
use crate::cases::task_inventory::{OPENSIM_START, hold_to_measured};
use crate::context::{Session, TestContext, TestFailure};
use crate::grid::Grid;
use crate::measured::Measured;
use crate::metrics::Metrics;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, is_opensim};

/// How long both avatars are listened to after a leg's request.
const WINDOW: Duration = Duration::from_secs(6);

/// How long a RAW heightmap may take to cross in either direction: about
/// 832 KB over `Xfer`, one confirmed packet at a time.
const TRANSFER: Duration = Duration::from_secs(300);

/// How long a refused transfer is waited for before it is called refused.
const REFUSED_TRANSFER: Duration = Duration::from_secs(15);

/// The quiet that ends a window waiting for a grid to finish re-sending the
/// ground after an upload.
const QUIET: Duration = Duration::from_secs(3);

/// Where both avatars stand on OpenSim: the Default Region's centre.
const OPENSIM_SPOT: Vector = Vector {
    x: 128.0,
    y: 128.0,
    z: 25.0,
};

/// How far north and east of the avatars the brush lands, in metres.
const BRUSH_OFFSET: f32 = 8.0;

/// The strength of an ordinary stroke: the wire's `Seconds`.
const STROKE: f32 = 1.0;

/// The strength of a stroke meant to run into a limit.
const PAST_THE_LIMIT: f32 = 5.0;

/// The limit the census sets for its limit leg, in metres either way.
const TEST_LIMIT: f32 = 2.0;

/// How far the flatten brush's reference height lies above the ground under
/// it, in metres.
const FLATTEN_ABOVE: f32 = 1.0;

/// The half-width of the area form's rectangle, in metres.
const AREA_HALF: f32 = 4.0;

/// How many revert strokes the census spends restoring the ground where it
/// has no RAW heightmap to restore it from.
const REVERT_STROKES: usize = 4;

/// The difference under the brush (metres) above which the census restores
/// the ground before its next leg.
const RESTORED_WITHIN: f32 = 0.01;

/// The smallest move (metres) [`Census::reach`] counts.
const REACH_FLOOR: f32 = 0.05;

/// The half-width of the square under the brush the census compares, in
/// whole metres.
const PATCH_WATCH: u32 = 7;

/// The viewer-side filename the transfers are named with.
const RAW_FILENAME: &str = "terrain.raw";

/// A distinctive `ParcelProperties` sequence id for the parcel under the brush.
const SEQUENCE_ID: i32 = 5197;

/// What a grid without estate rights for the test avatars records for a leg
/// only an estate manager can run.
const NO_RIGHTS: &str = "not measured: no estate rights on aditi (gridspec-aditi-test-land)";

/// What the RAW legs record where no RAW heightmap was downloaded to upload.
const NO_RAW: &str = "not measured: no RAW downloaded";

/// The metrics only an avatar with estate rights records: what a grid without
/// rights records instead.
const OWNER_KEYS: [&str; 30] = [
    "limit_set_tally_a",
    "limit_set_tally_b",
    "textures_tally_a",
    "textures_tally_b",
    "bake_start_told_a",
    "bake_start_told_b",
    "limit_manager_told_a",
    "limit_manager_told_b",
    "limit_manager_raise_told_a",
    "limit_manager_raise_told_b",
    "limit_manager_raise_change",
    "limit_manager_lower_change",
    "limit_set_told_a",
    "limit_set_told_b",
    "limit_raise_told_a",
    "limit_raise_told_b",
    "limit_raise_change",
    "limit_lower_told_a",
    "limit_lower_told_b",
    "limit_lower_change",
    "limit_restore_told_a",
    "limit_restore_told_b",
    "bake_told_a",
    "bake_told_b",
    "bake_revert_change",
    "textures_told_a",
    "textures_told_b",
    "textures_handshake",
    "textures_restore_told_a",
    "textures_restore_told_b",
];

/// Where the measurements below are written down.
const SOURCE: &str = "book/src/gridspec/terrain.md § Editing (terrain-editing, 2026-10-18)";

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

/// One land patch, the brush's: what both avatars are told of a stroke that
/// moved the ground inside one patch.
const ONE_PATCH: &str = "1 land patch at +0,+0";

/// A refused `ModifyRegion` write, as Second Life words it.
const MODIFY_REFUSED: &str = "success false, message \"Unable to modify region <region>: Not an estate owner or estate manager\"";

/// What each grid was measured to answer, by the metric the census records it
/// under: every run is held to it.
///
/// Not held: the region's configuration — its limits, its flags, the parcel,
/// the ground's height — how soon a patch came, and what follows from strokes
/// whose start is the stroke before's (smooth, roughen, revert).
const MEASURED: &[(&str, Measured<Answer>)] = &[
    (
        "a_holds_rights",
        each(Answer::Flag(false), Answer::Flag(true)),
    ),
    // A's strokes: a resident's ignored on Second Life, the owner's applied
    // on OpenSim and sent to both avatars as the one patch they moved.
    (
        "raise_told_a",
        each(Answer::Text("nothing"), Answer::Text(ONE_PATCH)),
    ),
    (
        "raise_told_b",
        each(Answer::Text("nothing"), Answer::Text(ONE_PATCH)),
    ),
    (
        "raise_change",
        each(Answer::Text("none"), Answer::Text("+1.0 m")),
    ),
    (
        "raise_reach",
        each(Answer::Text("nothing moved"), Answer::Text("3.5 m")),
    ),
    (
        "lower_told_a",
        each(Answer::Text("nothing"), Answer::Text(ONE_PATCH)),
    ),
    (
        "lower_change",
        each(Answer::Text("none"), Answer::Text("-1.0 m")),
    ),
    (
        "flatten_told_a",
        each(Answer::Text("nothing"), Answer::Text(ONE_PATCH)),
    ),
    (
        "flatten_change",
        each(Answer::Text("none"), Answer::Text("+1.0 m")),
    ),
    (
        "smooth_told_a",
        each(Answer::Text("nothing"), Answer::Text(ONE_PATCH)),
    ),
    (
        "roughen_told_a",
        each(Answer::Text("nothing"), Answer::Text(ONE_PATCH)),
    ),
    (
        "revert_told_a",
        each(Answer::Text("nothing"), Answer::Text(ONE_PATCH)),
    ),
    (
        "area_raise_told_a",
        each(Answer::Text("nothing"), Answer::Text(ONE_PATCH)),
    ),
    (
        "area_raise_change",
        each(Answer::Text("none"), Answer::Text("+1.0 m")),
    ),
    (
        "area_raise_corner",
        each(Answer::Text("none"), Answer::Text("+1.0 m")),
    ),
    // Undo: nothing on either grid.
    (
        "undo_raise_change",
        each(Answer::Text("none"), Answer::Text("+1.0 m")),
    ),
    ("undo_told_a", both(Answer::Text("nothing"))),
    ("undo_told_b", both(Answer::Text("nothing"))),
    ("undo_change", both(Answer::Text("none"))),
    // The limits: OpenSim's owner is a god and unlimited, a manager held.
    (
        "limit_raise_change",
        each(Answer::Text(NO_RIGHTS), Answer::Text("+5.0 m")),
    ),
    (
        "limit_manager_raise_change",
        each(Answer::Text(NO_RIGHTS), Answer::Text("+2.0 m")),
    ),
    (
        "limit_manager_lower_change",
        each(Answer::Text(NO_RIGHTS), Answer::Text("-2.0 m")),
    ),
    (
        "limit_set_tally_a",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text("handshake ×4; region info ×5"),
        ),
    ),
    (
        "limit_set_tally_b",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text("handshake ×4; region info ×5"),
        ),
    ),
    // Bake: silent, and a revert after it keeps the raise.
    (
        "bake_told_a",
        each(Answer::Text(NO_RIGHTS), Answer::Text("nothing")),
    ),
    (
        "bake_revert_change",
        each(Answer::Text(NO_RIGHTS), Answer::Text("none")),
    ),
    // The terrain tab.
    (
        "textures_tally_a",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text("handshake ×4; region info ×5"),
        ),
    ),
    (
        "textures_handshake",
        each(
            Answer::Text(NO_RIGHTS),
            Answer::Text("start height 0: 10 -> 11"),
        ),
    ),
    // A resident's attempts: ignored without a word on both grids.
    ("b_raise_told_a", both(Answer::Text("nothing"))),
    ("b_raise_told_b", both(Answer::Text("nothing"))),
    ("b_raise_change", both(Answer::Text("none"))),
    ("b_bake_told_b", both(Answer::Text("nothing"))),
    ("b_textures_told_a", both(Answer::Text("nothing"))),
    ("b_textures_told_b", both(Answer::Text("nothing"))),
    ("raw_download_b", both(Answer::Text("nothing"))),
    // The RAW heightmap.
    (
        "raw_download_a",
        each(
            Answer::Text("nothing"),
            Answer::Text(
                "alert \"Terrain file written, starting download...\"; file of 851968 bytes",
            ),
        ),
    ),
    (
        "raw_upload_a",
        each(
            Answer::Text(NO_RAW),
            Answer::Text(
                "alert \"Terrain Upload Complete. Loading....\"; upload of 851968 bytes confirmed",
            ),
        ),
    ),
    (
        "raw_upload_after_told_a",
        each(
            Answer::Text(NO_RAW),
            Answer::Text(
                "alert \"Your terrain was loaded as a .raw file. It may take a few moments to \
                 appear.\"; 0 of 256 land patches (0 sent)",
            ),
        ),
    ),
    (
        "raw_upload_after_told_b",
        each(
            Answer::Text(NO_RAW),
            Answer::Text("0 of 256 land patches (0 sent)"),
        ),
    ),
    (
        "raw_upload_b",
        each(Answer::Text(NO_RAW), Answer::Text("nothing")),
    ),
    // The PBR terrain overrides.
    (
        "modify_region_get_a",
        each(
            Answer::Text("HTTP 200, {overrides: [undef, undef, undef, undef], success: true}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "modify_region_get_b",
        each(
            Answer::Text("HTTP 200, {overrides: [undef, undef, undef, undef], success: true}"),
            Answer::Text("no capability"),
        ),
    ),
    (
        "modify_region_post_b",
        each(
            Answer::Text(
                "HTTP 403, success false, message \"Unable to modify region <region>: Not an \
                 estate owner or estate manager\"",
            ),
            Answer::Text("nothing read to send"),
        ),
    ),
    ("modify_region_post_b_told_b", both(Answer::Text("nothing"))),
    (
        "modify_region_client_read_a",
        each(
            Answer::Text("success true, overrides undef, undef, undef, undef"),
            Answer::Text("no answer"),
        ),
    ),
    (
        "modify_region_client_write_b",
        each(
            Answer::Text(MODIFY_REFUSED),
            Answer::Text("nothing read to send"),
        ),
    ),
];

/// What a Second-Life-flavoured fake grid is not held to: who holds estate
/// rights. Its first account is an estate manager, as on OpenSim, where on
/// aditi neither test avatar is (`gridspec-aditi-test-land`): so the census
/// treats it as the resident aditi's A is, but the grid lets it transfer the
/// RAW heightmap, which aditi does not.
const NOT_IMITATED_AS_SECOND_LIFE: &[&str] = &[
    "a_holds_rights",
    "raw_download_a",
    "raw_upload_a",
    "raw_upload_after_told_a",
    "raw_upload_after_told_b",
    "raw_upload_b",
];

/// What an OpenSim-flavoured fake grid is not held to yet: terraforming,
/// which it does not do (`server-fake-grid-terraform`).
const NOT_IMITATED_AS_OPENSIM: &[&str] = &[
    "raise_told_a",
    "raise_told_b",
    "raise_change",
    "raise_reach",
    "lower_told_a",
    "lower_change",
    "flatten_told_a",
    "flatten_change",
    "smooth_told_a",
    "roughen_told_a",
    "revert_told_a",
    "area_raise_told_a",
    "area_raise_change",
    "area_raise_corner",
    "undo_raise_change",
    "limit_raise_change",
    "limit_manager_raise_change",
    "limit_manager_lower_change",
];

/// The ground census.
#[derive(Debug)]
pub struct TerrainEditing;

impl GridTest for TerrainEditing {
    fn name(&self) -> &'static str {
        "terrain-editing"
    }

    fn description(&self) -> &'static str {
        "Each terraform brush, its limits, undo and bake, the RAW transfers, the terrain \
         textures and the ModifyRegion capability, with rights and without"
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
        Duration::from_secs(1500)
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let mut metrics = Metrics::new();
            let outcome = census(ctx, &mut metrics).await;
            let grid = ctx.grid();
            ctx.metrics().merge(metrics.clone());
            let exempt: &[&str] = match grid {
                Grid::FakeSl => NOT_IMITATED_AS_SECOND_LIFE,
                Grid::FakeOpensim => NOT_IMITATED_AS_OPENSIM,
                Grid::Opensim | Grid::Aditi => &[],
            };
            let held: Vec<(&str, Measured<Answer>)> = MEASURED
                .iter()
                .filter(|(key, _measured)| !exempt.contains(key))
                .copied()
                .collect();
            outcome.and_then(|()| hold_to_measured(grid, &metrics, &held, SOURCE))
        })
    }
}

/// Which avatar acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Who {
    /// The avatar with estate rights, where there is one.
    A,
    /// The resident.
    B,
}

/// What one session was told during a window.
#[derive(Debug, Default)]
struct Told {
    /// One line per message the census reads, in arrival order; land patches
    /// are folded into one line per run of them.
    lines: Vec<String>,
    /// The land patches of the census's region, by position.
    patches: BTreeSet<(u32, u32)>,
    /// How many land patch messages' worth arrived (a patch sent twice counts
    /// twice).
    patch_count: usize,
    /// How long after the window opened the first land patch came.
    first_patch: Option<Duration>,
    /// The last handshake's terrain composition among them.
    handshake: Option<sl_client_tokio::RegionTerrainComposition>,
    /// A downloaded file, if one finished.
    downloaded: Option<Vec<u8>>,
    /// The byte count of an upload the grid confirmed, if one did.
    uploaded: Option<usize>,
}

/// The patch a region-local metre lies in.
const fn patch_of(x: f32, y: f32) -> (u32, u32) {
    (metre(x) / 16, metre(y) / 16)
}

/// A region-local coordinate as a whole metre, clamped into a region.
const fn metre(value: f32) -> u32 {
    let rounded = value.clamp(0.0, 4095.0).round();
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=4095 and rounded, so the f32 → u32 narrowing is exact"
    )]
    let whole = rounded as u32;
    whole
}

/// Folds one event into `told`; `None` for an event the census does not read.
fn fold(scene: &Scene, told: &mut Told, opened: Instant, event: &Event) -> Option<()> {
    let line = match event {
        Event::TerrainPatch(patch) => {
            if !patch.layer.is_land() || Some(patch.region_handle) != scene.region {
                return None;
            }
            if told.first_patch.is_none() {
                told.first_patch = Some(opened.elapsed());
            }
            let _new = told.patches.insert((patch.patch_x, patch.patch_y));
            told.patch_count = told.patch_count.saturating_add(1);
            if told.lines.last().is_some_and(|line| line == "land patches") {
                return Some(());
            }
            "land patches".to_owned()
        }
        Event::RegionLimits(_) => "region info".to_owned(),
        Event::RegionInfoHandshake(identity) => {
            if Some(identity.region_handle) != scene.region {
                return None;
            }
            told.handshake = Some(identity.terrain);
            "handshake".to_owned()
        }
        Event::AlertMessage { message, .. } => format!("alert \"{}\"", scene.scrub(message)),
        Event::AgentAlertMessage { message, .. } => {
            format!("agent alert \"{}\"", scene.scrub(message))
        }
        Event::ServerFileDownloaded { data, .. } => {
            told.downloaded = Some(data.clone());
            format!("file of {} bytes", data.len())
        }
        Event::XferUploaded { byte_count, .. } => {
            told.uploaded = Some(*byte_count);
            format!("upload of {byte_count} bytes confirmed")
        }
        Event::XferAborted { result, .. } => format!("transfer aborted ({result})"),
        _ => return None,
    };
    told.lines.push(line);
    Some(())
}

/// What `told` holds, in words, the patches as offsets from `brush`;
/// `nothing` for an empty window.
fn describe_told(told: &Told, brush: (u32, u32)) -> String {
    if told.lines.is_empty() {
        return "nothing".to_owned();
    }
    let patches = describe_patches(told, brush);
    told.lines
        .iter()
        .map(|line| {
            if line == "land patches" {
                patches.clone()
            } else {
                line.clone()
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The land patches in `told`, in words: how many messages' worth, and where,
/// as offsets from the brush's patch.
fn describe_patches(told: &Told, brush: (u32, u32)) -> String {
    let offsets: Vec<String> = told
        .patches
        .iter()
        .map(|(x, y)| {
            let dx = i64::from(*x).saturating_sub(i64::from(brush.0));
            let dy = i64::from(*y).saturating_sub(i64::from(brush.1));
            format!("{dx:+},{dy:+}")
        })
        .collect();
    let repeated = if told.patch_count == told.patches.len() {
        String::new()
    } else {
        format!(" ({} sent)", told.patch_count)
    };
    format!(
        "{} land patch{} at {}{repeated}",
        told.patches.len(),
        if told.patches.len() == 1 { "" } else { "es" },
        offsets.join(" ")
    )
}

/// Listens on `session` for `window`.
async fn watch(
    scene: &Scene,
    session: &mut Session,
    window: Duration,
) -> Result<Told, TestFailure> {
    let mut told = Told::default();
    let started = Instant::now();
    loop {
        let remaining = window.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(told);
        }
        match session
            .wait_for(remaining, |event| fold(scene, &mut told, started, event))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(told),
            Err(other) => return Err(other),
        }
    }
}

/// Listens on `session` until a transfer ends — a file arrives, an upload is
/// confirmed or aborted — or `limit` passes.
async fn watch_transfer(
    scene: &Scene,
    session: &mut Session,
    limit: Duration,
) -> Result<Told, TestFailure> {
    let mut told = Told::default();
    let started = Instant::now();
    let ended = session
        .wait_for(limit, |event| {
            fold(scene, &mut told, started, event)?;
            matches!(
                event,
                Event::ServerFileDownloaded { .. }
                    | Event::XferUploaded { .. }
                    | Event::XferAborted { .. }
            )
            .then_some(())
        })
        .await;
    match ended {
        Ok(()) | Err(TestFailure::Timeout(_)) => Ok(told),
        Err(other) => Err(other),
    }
}

/// Listens on `session` until nothing the census reads has come for
/// [`QUIET`], or `limit` passes.
async fn watch_until_quiet(
    scene: &Scene,
    session: &mut Session,
    limit: Duration,
) -> Result<Told, TestFailure> {
    let mut told = Told::default();
    let started = Instant::now();
    loop {
        let remaining = limit.saturating_sub(started.elapsed()).min(QUIET);
        if remaining.is_zero() {
            return Ok(told);
        }
        match session
            .wait_for(remaining, |event| fold(scene, &mut told, started, event))
            .await
        {
            Ok(()) => {}
            Err(TestFailure::Timeout(_)) => return Ok(told),
            Err(other) => return Err(other),
        }
    }
}

/// The census's state: the two sessions and where the brush lands.
struct Census<'s> {
    /// Avatar A.
    a: &'s mut Session,
    /// Avatar B.
    b: &'s mut Session,
    /// Who is who.
    scene: Scene,
    /// The region-local metre the brush lands on.
    brush: (f32, f32),
    /// The parcel under the brush, as A was answered.
    parcel: Option<RegionLocalParcelId>,
    /// The heights under the brush before the census changed anything, as A
    /// was told them.
    start: Vec<((u32, u32), f32)>,
    /// Whether A holds estate rights.
    rights: bool,
    /// The grid the census runs on.
    grid: Grid,
    /// The RAW heightmap A downloaded, kept to restore the ground from.
    raw: Option<Vec<u8>>,
}

impl Census<'_> {
    /// The session of `who`.
    const fn session(&mut self, who: Who) -> &mut Session {
        match who {
            Who::A => self.a,
            Who::B => self.b,
        }
    }

    /// The patch the brush lands in.
    const fn brush_patch(&self) -> (u32, u32) {
        patch_of(self.brush.0, self.brush.1)
    }

    /// The ground under the brush's centre, as `who` was last told.
    fn centre(&self, who: Who) -> Option<f32> {
        let session = match who {
            Who::A => &*self.a,
            Who::B => &*self.b,
        };
        session.ground_height(metre(self.brush.0), metre(self.brush.1))
    }

    /// The heights of the square under the brush, as A was last told them.
    fn square(&self) -> Vec<((u32, u32), f32)> {
        let (cx, cy) = (metre(self.brush.0), metre(self.brush.1));
        let mut heights = Vec::new();
        for x in cx.saturating_sub(PATCH_WATCH)..=cx.saturating_add(PATCH_WATCH) {
            for y in cy.saturating_sub(PATCH_WATCH)..=cy.saturating_add(PATCH_WATCH) {
                if let Some(height) = self.a.ground_height(x, y) {
                    heights.push(((x, y), height));
                }
            }
        }
        heights
    }

    /// The largest difference between the square under the brush now and as
    /// it stood before the census, in metres.
    fn drift(&self) -> f32 {
        let now = self.square();
        self.start
            .iter()
            .filter_map(|(cell, before)| {
                now.iter()
                    .find(|(other, _height)| other == cell)
                    .map(|(_cell, after)| (after - before).abs())
            })
            .fold(0.0, f32::max)
    }

    /// How far from the brush's centre the ground moved since `before`, in
    /// words: the distance to the farthest cell that moved more than a tenth
    /// of the largest move (and more than [`REACH_FLOOR`]), to the nearest
    /// half metre. A re-sent patch is quantized afresh, so every cell of it
    /// moves a little; only a move the stroke made counts.
    fn reach(&self, before: &[((u32, u32), f32)]) -> String {
        let (cx, cy) = (metre(self.brush.0), metre(self.brush.1));
        let now = self.square();
        let largest = before
            .iter()
            .filter_map(|(cell, height)| {
                now.iter()
                    .find(|(other, _height)| other == cell)
                    .map(|(_cell, after)| (after - height).abs())
            })
            .fold(0.0, f32::max);
        let threshold = (largest / 10.0).max(REACH_FLOOR);
        let farthest = before
            .iter()
            .filter(|(cell, height)| {
                now.iter()
                    .find(|(other, _height)| other == cell)
                    .is_some_and(|(_cell, after)| (after - height).abs() > threshold)
            })
            .map(|((x, y), _height)| {
                let dx = f64::from(*x) - f64::from(cx);
                let dy = f64::from(*y) - f64::from(cy);
                dx.hypot(dy)
            })
            .fold(None, |farthest: Option<f64>, distance| {
                Some(farthest.map_or(distance, |known| known.max(distance)))
            });
        farthest.map_or_else(
            || "nothing moved".to_owned(),
            |distance| format!("{:.1} m", (distance * 2.0).round() / 2.0),
        )
    }

    /// Listens to both avatars for a window.
    async fn hear(&mut self) -> Result<(Told, Told), TestFailure> {
        let scene = self.scene.clone();
        let (told_a, told_b) =
            tokio::join!(watch(&scene, self.a, WINDOW), watch(&scene, self.b, WINDOW));
        Ok((told_a?, told_b?))
    }

    /// Sends `commands` as `who`, listens to both avatars and records what
    /// each was told under `key`.
    async fn leg(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        commands: Vec<Command>,
    ) -> Result<(Told, Told), TestFailure> {
        for command in commands {
            self.session(who).send(command).await?;
        }
        let (told_a, told_b) = self.hear().await?;
        let brush = self.brush_patch();
        metrics.set(&format!("{key}_told_a"), describe_told(&told_a, brush));
        metrics.set(&format!("{key}_told_b"), describe_told(&told_b, brush));
        metrics.set(&format!("{key}_tally_a"), tally(&told_a));
        metrics.set(&format!("{key}_tally_b"), tally(&told_b));
        if let Some(first) = told_a.first_patch {
            metrics.set(&format!("{key}_first_patch_a_secs"), first.as_secs_f64());
        }
        Ok((told_a, told_b))
    }

    /// A stroke of `action` at the brush's centre (or over `area`), sent as
    /// `who`, recorded under `key`: what each avatar was told and how the
    /// centre moved, as `who` and as the other avatar saw it.
    async fn stroke(
        &mut self,
        metrics: &mut Metrics,
        key: &str,
        who: Who,
        edit: LandEdit,
    ) -> Result<(), TestFailure> {
        let before = self.centre(Who::A);
        let square = self.square();
        let _told = self
            .leg(metrics, key, who, vec![Command::ModifyLand(edit)])
            .await?;
        metrics.set(&format!("{key}_reach"), self.reach(&square));
        let after_a = self.centre(Who::A);
        let after_b = self.centre(Who::B);
        metrics.set(&format!("{key}_change"), change(before, after_a));
        metrics.set(
            &format!("{key}_b_sees_same"),
            match (after_a, after_b) {
                (Some(a), Some(b)) => (a - b).abs() < 0.05,
                _ => false,
            },
        );
        Ok(())
    }

    /// A point-form edit of `action` with `strength` at the brush's centre,
    /// leveling toward `height`.
    fn point(&self, action: LandBrushAction, strength: f32, height: f32) -> LandEdit {
        LandEdit {
            action,
            brush_radius: LandBrushSize::Large.into(),
            strength,
            height,
            parcel: None,
            area: TerraformArea::point(self.brush.0, self.brush.1),
        }
    }

    /// An area-form edit of `action` over the rectangle around the brush's
    /// centre, naming the parcel under it.
    fn area(&self, action: LandBrushAction, strength: f32) -> LandEdit {
        LandEdit {
            action,
            brush_radius: LandBrushSize::Large.into(),
            strength,
            height: self.centre(Who::A).unwrap_or(0.0),
            parcel: self.parcel,
            area: TerraformArea::new(
                self.brush.0 - AREA_HALF,
                self.brush.1 - AREA_HALF,
                self.brush.0 + AREA_HALF,
                self.brush.1 + AREA_HALF,
            ),
        }
    }

    /// Puts the ground under the brush back as it was before the census,
    /// where it moved more than [`RESTORED_WITHIN`]: from the RAW heightmap
    /// downloaded first where there is one, a revert stroke at a time
    /// otherwise. How far off it was left.
    async fn reset(&mut self) -> Result<f32, TestFailure> {
        if self.drift() <= RESTORED_WITHIN {
            return Ok(self.drift());
        }
        if let Some(raw) = self.raw.clone() {
            self.a
                .send(Command::RequestRegionTerrainUpload {
                    viewer_filename: RAW_FILENAME.to_owned(),
                    data: raw,
                })
                .await?;
            let scene = self.scene.clone();
            let _told = watch_transfer(&scene, self.a, TRANSFER).await?;
            let (settled_a, settled_b) = tokio::join!(
                watch_until_quiet(&scene, self.a, TRANSFER),
                watch_until_quiet(&scene, self.b, TRANSFER)
            );
            let (_settled_a, _settled_b) = (settled_a?, settled_b?);
            return Ok(self.drift());
        }
        let mut strokes = 0_usize;
        while strokes < REVERT_STROKES && self.drift() > RESTORED_WITHIN {
            let edit = self.point(LandBrushAction::Revert, STROKE, 0.0);
            self.a.send(Command::ModifyLand(edit)).await?;
            let _told = self.hear().await?;
            strokes = strokes.saturating_add(1);
        }
        Ok(self.drift())
    }
}

/// How the centre moved from `before` to `after`, in words, to the nearest
/// half metre: a re-sent patch is quantized afresh, which moves a height by a
/// tenth of a metre either way from run to run.
fn change(before: Option<f32>, after: Option<f32>) -> String {
    match (before, after) {
        (Some(before), Some(after)) => {
            let halves = ((after - before) * 2.0).round() / 2.0;
            if halves == 0.0 {
                "none".to_owned()
            } else {
                format!("{halves:+.1} m")
            }
        }
        _ => "no ground".to_owned(),
    }
}

/// How many of each line `told` holds, in words, by kind: the order a burst
/// of them comes in is not the grid's to keep.
fn tally(told: &Told) -> String {
    let mut kinds: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for line in &told.lines {
        let count = kinds.entry(line.as_str()).or_default();
        *count = count.saturating_add(1);
    }
    if kinds.is_empty() {
        return "nothing".to_owned();
    }
    kinds
        .iter()
        .map(|(kind, count)| format!("{kind} ×{count}"))
        .collect::<Vec<_>>()
        .join("; ")
}

/// A `ModifyRegion` answer as `raw_cap` read it, in words, with the region's
/// id in a refusal's message replaced by `<region>`.
fn describe_modify_region(read: &str, llsd: Option<&sl_wire::Llsd>, region: &str) -> String {
    let Some(llsd) = llsd else {
        return read.to_owned();
    };
    let Some(message) = llsd.get("message").and_then(sl_wire::Llsd::as_str) else {
        return read.to_owned();
    };
    let status = read.split(',').next().unwrap_or(read);
    let success = matches!(llsd.get("success"), Some(sl_wire::Llsd::Boolean(true)));
    format!(
        "{status}, success {success}, message \"{}\"",
        message.replace(region, "<region>")
    )
}

/// Where the brush lands for an avatar standing at `point`.
fn brush_for(point: &Vector) -> (f32, f32) {
    (point.x + BRUSH_OFFSET, point.y + BRUSH_OFFSET)
}

/// The terrain-tab settings that leave the region as `limits` and
/// `composition` say it stands.
fn terrain_update(
    limits: &RegionLimits,
    composition: &sl_client_tokio::RegionTerrainComposition,
) -> RegionTerrainUpdate {
    RegionTerrainUpdate {
        water_height: limits.water_height,
        terrain_raise_limit: limits.terrain_raise_limit,
        terrain_lower_limit: limits.terrain_lower_limit,
        use_estate_sun: limits.use_estate_sun,
        fixed_sun: limits.flags().contains(RegionFlags::SUN_FIXED),
        sun_hour: limits.sun_hour,
        detail_textures: composition.detail_textures,
        start_heights: composition.start_heights,
        height_ranges: composition.height_ranges,
    }
}

/// A `ModifyRegion` answer as the client read it, in words, the region's id
/// replaced by `<region>`.
fn describe_overrides_answer(answer: &sl_wire::ModifyRegionAnswer, region: &str) -> String {
    let mut words = vec![format!("success {}", answer.success)];
    if let Some(message) = &answer.message {
        words.push(format!(
            "message \"{}\"",
            message.replace(region, "<region>")
        ));
    }
    if let Some(overrides) = &answer.overrides {
        let slots: Vec<&str> = overrides
            .0
            .iter()
            .map(|slot| match slot {
                sl_wire::Llsd::Undef => "undef",
                sl_wire::Llsd::Map(map) if map.is_empty() => "empty",
                _ => "an override",
            })
            .collect();
        words.push(format!("overrides {}", slots.join(", ")));
    }
    words.join(", ")
}

/// Raises the first corner's start height by a metre: the change the census
/// writes to the terrain tab.
fn raise_first_corner(update: &mut RegionTerrainUpdate) {
    if let Some(start) = update.start_heights.first_mut() {
        *start += 1.0;
    }
}

/// How `after` differs from `before`, in words.
fn composition_change(
    before: &sl_client_tokio::RegionTerrainComposition,
    after: &sl_client_tokio::RegionTerrainComposition,
) -> String {
    let mut words = Vec::new();
    for (index, (old, new)) in before
        .start_heights
        .iter()
        .zip(after.start_heights.iter())
        .enumerate()
    {
        if (old - new).abs() > 0.001 {
            words.push(format!("start height {index}: {old} -> {new}"));
        }
    }
    for (index, (old, new)) in before
        .height_ranges
        .iter()
        .zip(after.height_ranges.iter())
        .enumerate()
    {
        if (old - new).abs() > 0.001 {
            words.push(format!("range {index}: {old} -> {new}"));
        }
    }
    if before.detail_textures != after.detail_textures {
        words.push("textures changed".to_owned());
    }
    if words.is_empty() {
        "no change".to_owned()
    } else {
        words.join("; ")
    }
}

/// The census: both avatars at the spot, then the legs.
async fn census(ctx: &mut TestContext, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let grid = ctx.grid();
    let somewhere = ctx
        .build_region()
        .zip(ctx.build_position())
        .map(|(region, point)| (region.to_owned(), point));
    let Some((a, b)) = ctx.primary_and_secondary() else {
        return Err(TestFailure::State(
            "terrain-editing needs a secondary avatar".to_owned(),
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
    names.sort_by_key(|(name, _word)| std::cmp::Reverse(name.len()));
    let spot = if grid.is_fake() {
        OPENSIM_SPOT
    } else if is_opensim(grid) {
        let handle = a
            .region_handle()
            .ok_or_else(|| TestFailure::State("no region".to_owned()))?;
        teleport(b, handle, &OPENSIM_SPOT).await?;
        OPENSIM_SPOT
    } else {
        let (region, point) = somewhere.ok_or_else(|| {
            TestFailure::State("terrain-editing needs a build_location in the fixtures".to_owned())
        })?;
        let handle = region_named(a, &region).await?;
        teleport(a, handle, &point).await?;
        teleport(b, handle, &point).await?;
        point
    };
    a.send(Command::SetThrottle(Throttle::preset_1000()))
        .await?;
    b.send(Command::SetThrottle(Throttle::preset_1000()))
        .await?;
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
        brush: brush_for(&spot),
        parcel: None,
        start: Vec::new(),
        rights: false,
        grid,
        raw: None,
    };
    // Let the arrival's ground come in to both before anything is measured.
    let _settled = census.hear().await?;
    let _settled = census.hear().await?;
    census.start = census.square();
    let outcome = legs(&mut census, metrics).await;
    let restored = restore(&mut census, metrics).await;
    outcome.and(restored)
}

/// Every leg, in order.
async fn legs(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    // 1. The setting.
    let handshake = census
        .a
        .region_identity()
        .cloned()
        .ok_or_else(|| TestFailure::State("A read no handshake".to_owned()))?;
    // A fake grid imitating Second Life answers as aditi does to a resident:
    // its manager's legs are as unmeasured as aditi's.
    let rights = handshake.is_estate_manager && census.grid != Grid::FakeSl;
    census.rights = rights;
    metrics.set("a_holds_rights", handshake.is_estate_manager);
    metrics.set("owner_legs_run", rights);
    metrics.set(
        "region_blocks_terraform",
        handshake.flags().contains(RegionFlags::BLOCK_TERRAFORM),
    );
    census.a.send(Command::RequestRegionInfo).await?;
    let limits = census
        .a
        .wait_for(WINDOW, |event| match event {
            Event::RegionLimits(limits) => Some(limits.clone()),
            _ => None,
        })
        .await?;
    metrics.set("raise_limit", f64::from(limits.terrain_raise_limit));
    metrics.set("lower_limit", f64::from(limits.terrain_lower_limit));
    let (x, y) = census.brush;
    census
        .a
        .send(Command::RequestParcelProperties {
            west: x - 1.0,
            south: y - 1.0,
            east: x + 1.0,
            north: y + 1.0,
            sequence_id: SEQUENCE_ID,
            snap_selection: false,
        })
        .await?;
    let parcel: ParcelInfo = census
        .a
        .wait_for(WINDOW, |event| match event {
            Event::ParcelProperties(parcel) if parcel.sequence_id == SEQUENCE_ID => {
                Some((**parcel).clone())
            }
            _ => None,
        })
        .await?;
    census.parcel = Some(parcel.local_id);
    metrics.set("parcel_owner", census.scene.whose(parcel.owner.uuid()));
    metrics.set(
        "parcel_allows_terraform",
        parcel.flags().contains(ParcelFlags::ALLOW_TERRAFORM),
    );
    metrics.set(
        "ground_before",
        census
            .centre(Who::A)
            .map_or_else(|| "no ground".to_owned(), |height| format!("{height:.1}")),
    );

    // 2. The capability, raw.
    let scene = census.scene.clone();
    let region_id = handshake.region_id.to_string();
    let (get_a, _llsd) = raw_cap(&scene, census.a, "ModifyRegion", None).await;
    metrics.set("modify_region_get_a", get_a);
    let (get_b, read_b) = raw_cap(&scene, census.b, "ModifyRegion", None).await;
    metrics.set("modify_region_get_b", get_b);
    let echo = read_b
        .as_ref()
        .and_then(|llsd| llsd.get("overrides"))
        .cloned()
        .map(|overrides| {
            sl_wire::Llsd::Map(std::collections::HashMap::from([(
                "overrides".to_owned(),
                overrides,
            )]))
            .to_llsd_xml()
        });
    let (post_b, post_llsd) = match echo {
        Some(body) => raw_cap(&scene, census.b, "ModifyRegion", Some(body)).await,
        None => ("nothing read to send".to_owned(), None),
    };
    metrics.set(
        "modify_region_post_b",
        describe_modify_region(&post_b, post_llsd.as_ref(), &region_id),
    );
    let _told = census
        .leg(metrics, "modify_region_post_b", Who::B, Vec::new())
        .await?;
    // The same through the client's own commands: A reads, B writes back
    // what A read.
    census
        .a
        .send(Command::RequestTerrainMaterialOverrides)
        .await?;
    let read = census
        .a
        .wait_for(WINDOW, |event| match event {
            Event::TerrainMaterialOverrides(answer) => Some(Some(answer.as_ref().clone())),
            _ => None,
        })
        .await
        .or_else(|failure| match failure {
            TestFailure::Timeout(_) => Ok(None),
            other => Err(other),
        })?;
    metrics.set(
        "modify_region_client_read_a",
        read.as_ref().map_or_else(
            || "no answer".to_owned(),
            |answer| describe_overrides_answer(answer, &region_id),
        ),
    );
    if let Some(overrides) = read.and_then(|answer| answer.overrides) {
        census
            .b
            .send(Command::SetTerrainMaterialOverrides(Box::new(overrides)))
            .await?;
        let written = census
            .b
            .wait_for(WINDOW, |event| match event {
                Event::TerrainMaterialOverridesWritten(answer) => {
                    Some(describe_overrides_answer(answer, &region_id))
                }
                _ => None,
            })
            .await
            .or_else(|failure| match failure {
                TestFailure::Timeout(_) => Ok("no answer".to_owned()),
                other => Err(other),
            })?;
        metrics.set("modify_region_client_write_b", written);
    } else {
        metrics.set("modify_region_client_write_b", "nothing read to send");
    }

    // 9 (first half). The RAW heightmap, before anything changes the
    // ground; then, with rights, the ground as found baked, so a revert
    // returns to it.
    raw_download(census, metrics).await?;
    if rights {
        let _told = census
            .leg(
                metrics,
                "bake_start",
                Who::A,
                vec![Command::BakeRegionTerrain],
            )
            .await?;
    }

    // 3. A's brushes, point form, then the area form.
    let raise = census.point(LandBrushAction::Raise, STROKE, 0.0);
    census.stroke(metrics, "raise", Who::A, raise).await?;
    let lower = census.point(LandBrushAction::Lower, STROKE, 0.0);
    census.stroke(metrics, "lower", Who::A, lower).await?;
    let level = census.centre(Who::A).unwrap_or(0.0) + FLATTEN_ABOVE;
    let flatten = census.point(LandBrushAction::Level, STROKE, level);
    census.stroke(metrics, "flatten", Who::A, flatten).await?;
    let smooth = census.point(LandBrushAction::Smooth, STROKE, 0.0);
    census.stroke(metrics, "smooth", Who::A, smooth).await?;
    let noise = census.point(LandBrushAction::Noise, STROKE, 0.0);
    census.stroke(metrics, "roughen", Who::A, noise).await?;
    let revert = census.point(LandBrushAction::Revert, STROKE, 0.0);
    census.stroke(metrics, "revert", Who::A, revert).await?;
    metrics.set("revert_left", format!("{:.2}", census.drift()));
    let _left = census.reset().await?;
    let area_raise = census.area(LandBrushAction::Raise, STROKE);
    census
        .stroke(metrics, "area_raise", Who::A, area_raise)
        .await?;
    metrics.set("area_raise_corner", corner_change(census));
    let area_revert = census.area(LandBrushAction::Revert, STROKE);
    census
        .stroke(metrics, "area_revert", Who::A, area_revert)
        .await?;
    let _left = census.reset().await?;

    // 4. Undo.
    let raise = census.point(LandBrushAction::Raise, STROKE, 0.0);
    census.stroke(metrics, "undo_raise", Who::A, raise).await?;
    let before = census.centre(Who::A);
    let _told = census
        .leg(metrics, "undo", Who::A, vec![Command::UndoLand])
        .await?;
    metrics.set("undo_change", change(before, census.centre(Who::A)));
    let _left = census.reset().await?;

    // 5–7. A's rights only.
    if rights {
        owner_legs(census, metrics, &limits).await?;
    } else {
        for key in OWNER_KEYS {
            metrics.set(key, NO_RIGHTS);
        }
    }

    // 8. B's refusals (and on Second Life, A's as the resident it is there).
    let raise = census.point(LandBrushAction::Raise, STROKE, 0.0);
    census.stroke(metrics, "b_raise", Who::B, raise).await?;
    let _told = census
        .leg(metrics, "b_bake", Who::B, vec![Command::BakeRegionTerrain])
        .await?;
    let composition = handshake.terrain;
    let mut write = terrain_update(&limits, &composition);
    raise_first_corner(&mut write);
    let _told = census
        .leg(
            metrics,
            "b_textures",
            Who::B,
            vec![Command::SetRegionTerrain(write.clone())],
        )
        .await?;
    if !rights {
        let _told = census
            .leg(metrics, "a_bake", Who::A, vec![Command::BakeRegionTerrain])
            .await?;
        let _told = census
            .leg(
                metrics,
                "a_textures",
                Who::A,
                vec![Command::SetRegionTerrain(write)],
            )
            .await?;
    }

    // 9 (second half). The RAW heightmap uploaded back.
    raw_upload(census, metrics).await
}

/// How a corner of the area form's rectangle moved, in words: the area form
/// raises the whole rectangle, the point form only a sphere.
fn corner_change(census: &Census<'_>) -> String {
    let (x, y) = census.brush;
    let corner = (metre(x - AREA_HALF + 1.0), metre(y - AREA_HALF + 1.0));
    let before = census
        .start
        .iter()
        .find(|(cell, _height)| *cell == corner)
        .map(|(_cell, height)| *height);
    change(before, census.a.ground_height(corner.0, corner.1))
}

/// The legs only an estate manager runs: the limits, bake, the textures.
async fn owner_legs(
    census: &mut Census<'_>,
    metrics: &mut Metrics,
    limits: &RegionLimits,
) -> Result<(), TestFailure> {
    let composition = census
        .a
        .region_identity()
        .map(|identity| identity.terrain)
        .ok_or_else(|| TestFailure::State("A read no handshake".to_owned()))?;
    let original = terrain_update(limits, &composition);

    // 5. The limits.
    let tight = RegionTerrainUpdate {
        terrain_raise_limit: TEST_LIMIT,
        terrain_lower_limit: -TEST_LIMIT,
        ..original.clone()
    };
    let _told = census
        .leg(
            metrics,
            "limit_set",
            Who::A,
            vec![Command::SetRegionTerrain(tight)],
        )
        .await?;
    let raise = census.point(LandBrushAction::Raise, PAST_THE_LIMIT, 0.0);
    census.stroke(metrics, "limit_raise", Who::A, raise).await?;
    let _left = census.reset().await?;
    let lower = census.point(LandBrushAction::Lower, PAST_THE_LIMIT, 0.0);
    census.stroke(metrics, "limit_lower", Who::A, lower).await?;
    let _left = census.reset().await?;
    // The same as B, made an estate manager: a manager is no god.
    let b = sl_client_tokio::AgentKey::from(census.scene.b);
    let _told = census
        .leg(
            metrics,
            "limit_manager",
            Who::A,
            vec![Command::UpdateEstateAccess {
                delta: EstateAccessDelta::ManagerAdd,
                target: OwnerKey::Agent(b),
            }],
        )
        .await?;
    let raise = census.point(LandBrushAction::Raise, PAST_THE_LIMIT, 0.0);
    let managed = census
        .stroke(metrics, "limit_manager_raise", Who::B, raise)
        .await;
    let _left = census.reset().await?;
    let lower = census.point(LandBrushAction::Lower, PAST_THE_LIMIT, 0.0);
    let managed = managed.and(
        census
            .stroke(metrics, "limit_manager_lower", Who::B, lower)
            .await,
    );
    let _left = census.reset().await?;
    let _told = census
        .leg(
            metrics,
            "limit_unmanager",
            Who::A,
            vec![Command::UpdateEstateAccess {
                delta: EstateAccessDelta::ManagerRemove,
                target: OwnerKey::Agent(b),
            }],
        )
        .await?;
    managed?;
    let _told = census
        .leg(
            metrics,
            "limit_restore",
            Who::A,
            vec![Command::SetRegionTerrain(original.clone())],
        )
        .await?;

    // 6. Bake.
    let raise = census.point(LandBrushAction::Raise, STROKE, 0.0);
    census.stroke(metrics, "bake_raise", Who::A, raise).await?;
    let raised = census.centre(Who::A);
    let _told = census
        .leg(metrics, "bake", Who::A, vec![Command::BakeRegionTerrain])
        .await?;
    let revert = census.point(LandBrushAction::Revert, STROKE, 0.0);
    census
        .stroke(metrics, "bake_revert", Who::A, revert)
        .await?;
    metrics.set("bake_revert_change", change(raised, census.centre(Who::A)));
    // The raise and an equal lower cancel exactly; bake that back.
    let lower = census.point(LandBrushAction::Lower, STROKE, 0.0);
    census.stroke(metrics, "bake_lower", Who::A, lower).await?;
    let _told = census
        .leg(
            metrics,
            "bake_again",
            Who::A,
            vec![Command::BakeRegionTerrain],
        )
        .await?;

    // 7. The textures and heights.
    let mut write = original.clone();
    raise_first_corner(&mut write);
    let (told_a, _told_b) = census
        .leg(
            metrics,
            "textures",
            Who::A,
            vec![Command::SetRegionTerrain(write)],
        )
        .await?;
    metrics.set(
        "textures_handshake",
        told_a.handshake.as_ref().map_or_else(
            || "none".to_owned(),
            |after| composition_change(&composition, after),
        ),
    );
    let _told = census
        .leg(
            metrics,
            "textures_restore",
            Who::A,
            vec![Command::SetRegionTerrain(original)],
        )
        .await?;
    Ok(())
}

/// The RAW heightmap downloaded, by A and by B, before anything changes the
/// ground: A's copy is what the census restores the ground from.
async fn raw_download(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let scene = census.scene.clone();
    let brush = census.brush_patch();
    let started = Instant::now();
    census
        .a
        .send(Command::RequestRegionTerrainDownload {
            viewer_filename: RAW_FILENAME.to_owned(),
        })
        .await?;
    let limit = if census.rights {
        TRANSFER
    } else {
        REFUSED_TRANSFER
    };
    let told = watch_transfer(&scene, census.a, limit).await?;
    metrics.set("raw_download_a", describe_told(&told, brush));
    if told.downloaded.is_some() {
        metrics.set("raw_download_a_secs", started.elapsed().as_secs_f64());
    }
    census.raw = told.downloaded;
    census
        .b
        .send(Command::RequestRegionTerrainDownload {
            viewer_filename: RAW_FILENAME.to_owned(),
        })
        .await?;
    let told = watch_transfer(&scene, census.b, REFUSED_TRANSFER).await?;
    metrics.set("raw_download_b", describe_told(&told, brush));
    Ok(())
}

/// The RAW heightmap downloaded first, uploaded back by A once the strokes
/// are reverted — an upload that changes nothing, or next to nothing — and
/// by B.
async fn raw_upload(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let left = census.reset().await?;
    metrics.set("raw_upload_drift", format!("{left:.2}"));
    let Some(raw) = census.raw.clone() else {
        for key in [
            "raw_upload_a",
            "raw_upload_b",
            "raw_upload_after_told_a",
            "raw_upload_after_told_b",
        ] {
            metrics.set(key, NO_RAW);
        }
        return Ok(());
    };
    let scene = census.scene.clone();
    let brush = census.brush_patch();
    let started = Instant::now();
    census
        .a
        .send(Command::RequestRegionTerrainUpload {
            viewer_filename: RAW_FILENAME.to_owned(),
            data: raw.clone(),
        })
        .await?;
    let told = watch_transfer(&scene, census.a, TRANSFER).await?;
    metrics.set("raw_upload_a", describe_told(&told, brush));
    if told.uploaded.is_some() {
        metrics.set("raw_upload_a_secs", started.elapsed().as_secs_f64());
    }
    let (after_a, after_b) = tokio::join!(
        watch_until_quiet(&scene, census.a, TRANSFER),
        watch_until_quiet(&scene, census.b, TRANSFER)
    );
    let (after_a, after_b) = (after_a?, after_b?);
    metrics.set("raw_upload_after_told_a", describe_upload(&after_a));
    metrics.set("raw_upload_after_told_b", describe_upload(&after_b));
    census
        .b
        .send(Command::RequestRegionTerrainUpload {
            viewer_filename: RAW_FILENAME.to_owned(),
            data: raw,
        })
        .await?;
    let told = watch_transfer(&scene, census.b, REFUSED_TRANSFER).await?;
    metrics.set("raw_upload_b", describe_told(&told, brush));
    Ok(())
}

/// What a session was told after an upload, in words: the lines other than
/// the land patches, and how many patches of the region came.
fn describe_upload(told: &Told) -> String {
    let mut words: Vec<String> = told
        .lines
        .iter()
        .filter(|line| *line != "land patches")
        .cloned()
        .collect();
    words.push(format!(
        "{} of 256 land patches ({} sent)",
        told.patches.len(),
        told.patch_count
    ));
    words.join("; ")
}

/// Leaves the ground as the census found it, failing the run where it could
/// not.
async fn restore(census: &mut Census<'_>, metrics: &mut Metrics) -> Result<(), TestFailure> {
    let left = census.reset().await?;
    metrics.set("left_after_reset", format!("{left:.2}"));
    if left <= RESTORED_WITHIN {
        return Ok(());
    }
    Err(TestFailure::State(format!(
        "the ground under the brush is {left:.2} m off where it started"
    )))
}
