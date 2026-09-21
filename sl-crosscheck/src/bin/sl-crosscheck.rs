//! The cross-check runner: start a fake grid, photograph it with both viewers,
//! collect what they wrote.
//!
//! ```text
//! cargo build --release -p sl-client-bevy-viewer
//! cargo run --release -p sl-crosscheck -- \
//!     --scenario catalogue --look-at mesh-cube \
//!     --firestorm "${FIRESTORM_BUILD}/newview/packaged/firestorm"
//! ```
//!
//! The grid runs **inside this process** rather than as a spawned
//! `sl-fake-grid`. Not for tidiness: a readiness probe against a port proves the
//! *port* answers, not that the grid you started did — the launcher script grew
//! a check for exactly that after happily reporting a leftover grid from an
//! earlier run as ready — and binding the port here makes that class of mistake
//! impossible. An address already in use is an immediate, honest error, and the
//! grid is ready when `start()` returns rather than when a poll says so.
//!
//! One grid serves both viewers, one after the other. Sequentially because they
//! log in as the same avatar and would otherwise contend for it, and because two
//! GPU-bound viewers photographing the same scene at once are two viewers
//! photographing a machine under load.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use sl_crosscheck::launch::{Launch, RunDirs, Viewer};
use sl_crosscheck::plan::{
    CameraSpec, CaptureAudio, CaptureSpec, FirestormSkin, RegionPoint, RunPlan, SlClientSkin,
    parse_region_point,
};
use sl_crosscheck::process::{self, Ending};
use sl_crosscheck::status::Artefacts;
use sl_crosscheck::summary::{RunSummary, ViewerRun};
use sl_crosscheck::{files, launch};
use sl_fake_grid::fixtures::scenarios;
use sl_fake_grid::{
    AccountConfig, Action, At, FakeGridBuilder, GridIdentity, RegionConfig, Scenario, Timeline,
};
use sl_types::lsl::Vector;
use sl_types::map::RegionCoordinates;

/// The workspace root, as it stood when this binary was built. The viewer's own
/// vendored-asset defaults are resolved the same way, so a build that has been
/// moved away from its sources loses both together rather than one confusingly.
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Command-line options.
#[derive(Debug, Parser)]
#[command(author, version, about)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "a command line is a flat list of independent switches — three capture layers and a \
              neighbour, each meaningful in any combination — and clap parses this struct field \
              for field; folding them into an enum would invent groupings the command line does \
              not have"
)]
struct Options {
    /// The named scene every region is dressed with. The names come from the
    /// shared fixture registry, so a run says which scene it photographed
    /// without anyone retyping a command line.
    #[arg(
        long,
        default_value = "catalogue",
        value_parser = clap::builder::PossibleValuesParser::new(scenarios::names()),
    )]
    scenario: String,

    /// The fixed TCP port the grid serves login, CAPS and `get_grid_info` on.
    /// Fixed rather than ephemeral because both viewers are configured before
    /// either starts, and Firestorm caches a grid in its grid manager between
    /// runs.
    #[arg(long, default_value_t = 9100)]
    port: u16,

    /// Where the run's artefacts go. Defaults to
    /// `crosscheck-runs/<scenario>` beneath the current directory.
    #[arg(long)]
    run_dir: Option<PathBuf>,

    /// This workspace's viewer. Defaults to the release build beside the
    /// sources; build it with `cargo build --release -p sl-client-bevy-viewer`.
    #[arg(long)]
    viewer: Option<PathBuf>,

    /// The patched Firestorm's launcher (`packaged/firestorm` in its build
    /// tree, wherever this machine keeps it). Without it only this viewer runs,
    /// and the report says the other half was skipped rather than implying it
    /// failed. Set `SL_CROSSCHECK_FIRESTORM` in an uncommitted `.env` beside
    /// the sources to stop typing it.
    #[arg(long, env = "SL_CROSSCHECK_FIRESTORM")]
    firestorm: Option<PathBuf>,

    /// Run only this viewer. Both by default.
    #[arg(long, value_enum)]
    only: Option<Which>,

    /// Aim both cameras at the landmark of this name in the chosen scenario —
    /// `mesh-cube` rather than a position nobody can check. `sl-fake-grid`
    /// logs a scene's landmarks on startup, and so does this.
    #[arg(long)]
    look_at: Option<String>,

    /// How far south of the landmark the camera stands, in metres.
    #[arg(long, default_value_t = 8.0)]
    look_from: f32,

    /// How far above the landmark the camera stands, in metres.
    #[arg(long, default_value_t = 2.0)]
    look_above: f32,

    /// Put the camera at this region-local `x,y,z` instead of deriving one from
    /// `--look-at`. Combines with `--look-at`, which then only aims it.
    #[arg(long, value_parser = parse_region_point, allow_hyphen_values = true)]
    camera_position: Option<RegionPoint>,

    /// Aim the camera at this region-local `x,y,z`, instead of at a landmark.
    #[arg(long, value_parser = parse_region_point, allow_hyphen_values = true)]
    camera_look_at: Option<RegionPoint>,

    /// The pixel grid every frame is rendered at, `WIDTHxHEIGHT`.
    #[arg(long, default_value = "1920x1080", value_parser = parse_size)]
    capture_size: (u32, u32),

    /// Put each viewer's own interface in the frames. Off by default: two
    /// viewers' interfaces are not the same interface, and a renderer comparison
    /// wants the world.
    #[arg(long)]
    capture_ui: bool,

    /// Put the HUD-attachment layer in the frames.
    #[arg(long)]
    capture_hud: bool,

    /// Put the edit-tool gizmo overlay in the frames.
    #[arg(long)]
    capture_gizmos: bool,

    /// Let both viewers make sound. Off by default: a scene's looping sound
    /// sources would otherwise play through this machine's speakers for the
    /// whole run.
    #[arg(long)]
    capture_audio: bool,

    /// How many frames each viewer captures.
    #[arg(long, default_value_t = 30)]
    frames: usize,

    /// Seconds between frames.
    #[arg(long, default_value_t = 0.5)]
    interval: f32,

    /// Seconds to wait for the scene to stop loading before capturing anyway.
    #[arg(long, default_value_t = 25.0)]
    settle_timeout: f32,

    /// Seconds to wait to get in world before giving up on a run.
    #[arg(long, default_value_t = 180.0)]
    login_timeout: f32,

    /// Pin the sun at this day position in `[0, 1]`. Unset leaves each viewer
    /// its own default, and the two defaults are not the same — pin it for any
    /// comparison involving light, which is all of them.
    ///
    /// Both viewers pin by sampling the **region's** day cycle at the position,
    /// so a run that asks for one also dresses every region with a cycle it can
    /// sample: a scene whose environment schedules a single sky renders that sky
    /// whatever the position, and gets the four legacy WindLight presets instead
    /// (a scene that carries its own multi-frame cycle keeps it). A viewer that
    /// cannot honour the pin says so in its `harness-status.json` and fails the
    /// run — a capture lit by a sky nobody chose looks exactly like a good one.
    #[arg(long)]
    day_position: Option<f32>,

    /// Pin the camera's vertical field of view, in degrees, in both viewers.
    /// Unset leaves each its own default — which agree at the reference's 60°
    /// since `viewer-camera-fov-parity`, but a run that would rather say so
    /// than rely on it can.
    #[arg(long)]
    fov: Option<f32>,

    /// The skin this workspace's viewer wears — a directory under its
    /// `assets/skins/`. Unset leaves it in its own default.
    ///
    /// There is deliberately no option that dresses both viewers at once: the
    /// two skin namespaces are unrelated and a name valid here is generally not
    /// one there. The run refuses a skin this viewer does not ship rather than
    /// falling back to a default and capturing the wrong interface.
    #[arg(long)]
    sl_client_skin: Option<String>,

    /// The theme overlay for this workspace's viewer — a file under the chosen
    /// skin's `themes/`, without the extension.
    #[arg(long)]
    sl_client_theme: Option<String>,

    /// The skin Firestorm wears — either the folder in its `skins.xml` or the
    /// name its preferences panel shows (`vintage` or `Vintage`), matched
    /// case-insensitively. Its harness refuses a name it cannot resolve and
    /// lists the ones it has.
    #[arg(long)]
    firestorm_skin: Option<String>,

    /// The theme Firestorm wears, likewise by folder or by name. Its default
    /// theme for every skin has an *empty* folder, so that one can only be
    /// asked for by name (`Classic`, `Grey`).
    #[arg(long)]
    firestorm_theme: Option<String>,

    /// The account both viewers log in as, `First:Last:password`.
    #[arg(long, default_value = "Test:User:password")]
    account: String,

    /// Seconds a viewer is given before it is asked to quit. Derived from the
    /// timings above when unset.
    #[arg(long)]
    deadline: Option<f32>,

    /// Stand the scene's **second** region one slot east of the first, and let
    /// the grid announce it, so a framing can hold the line between two
    /// regions. Only a scene that says how it dresses both halves can do this
    /// — `border` is the one that does.
    #[arg(long)]
    neighbour: bool,

    /// Walk the agent over the east border this many seconds after it arrives.
    /// Implies `--neighbour`: a crossing needs somewhere to cross into.
    #[arg(long, value_name = "SECONDS")]
    cross_after: Option<f32>,
}

/// The name of the region the agent logs into.
const NEAR_REGION: &str = "Fake Region";

/// The name of the region one slot east of it, when a run asks for the pair.
const FAR_REGION: &str = "Fake Region East";

/// How far inside the far region's west edge a scripted crossing lands the
/// agent, in metres. The vehicles stand at
/// `border::VEHICLE_FROM_BORDER`; landing beside them is what puts the
/// crossing in the same framing as the thing that crossed with it.
const CROSSING_LANDING_X: f32 = 6.0;

/// The eastward speed a scripted crossing carries over, in metres per second —
/// a walk, because the velocity in a `CrossedRegion` is what keeps the client's
/// momentum rather than teleport-stopping it dead.
const CROSSING_SPEED: f32 = 1.5;

/// Which half of the pair to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Which {
    /// This workspace's viewer only.
    SlClient,
    /// Firestorm only.
    Firestorm,
}

/// Parse a `WIDTHxHEIGHT` capture size.
fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (width, height) = text
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got {text:?}"))?;
    let parse = |raw: &str| {
        raw.trim()
            .parse::<u32>()
            .map_err(|error| error.to_string())
            .and_then(|value| {
                if value == 0 {
                    Err("a capture dimension must be positive".to_owned())
                } else {
                    Ok(value)
                }
            })
    };
    Ok((parse(width)?, parse(height)?))
}

/// Split a `First:Last:password` account argument.
fn parse_account(raw: &str) -> Result<(String, String, String), String> {
    let mut parts = raw.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(first), Some(last), Some(password))
            if !first.is_empty() && !last.is_empty() && !password.is_empty() =>
        {
            Ok((first.to_owned(), last.to_owned(), password.to_owned()))
        }
        _malformed => Err(format!(
            "unparsable --account {raw:?} (want First:Last:password)"
        )),
    }
}

/// Where the camera goes: an explicit position, a landmark, or neither.
fn resolve_camera(
    options: &Options,
    scene: &scenarios::NamedScenario,
) -> Result<Option<CameraSpec>, String> {
    let landmark = match &options.look_at {
        Some(name) => Some(scene.landmark(name).ok_or_else(|| {
            let known: Vec<String> = scene
                .landmarks()
                .into_iter()
                .map(|landmark| landmark.name)
                .collect();
            format!(
                "the {} scene has no landmark {name:?}; it has {}",
                scene.name,
                known.join(", ")
            )
        })?),
        None => None,
    };
    let subject = landmark.map(|landmark| {
        RegionPoint::new(
            landmark.position.x,
            landmark.position.y,
            landmark.position.z,
        )
    });
    Ok(match (options.camera_position, subject) {
        (Some(position), _subject) => Some(CameraSpec {
            position,
            look_at: options.camera_look_at.or(subject),
        }),
        (None, Some(subject)) => {
            let mut camera = CameraSpec::facing(subject, options.look_from, options.look_above);
            if let Some(look_at) = options.camera_look_at {
                camera.look_at = Some(look_at);
            }
            Some(camera)
        }
        // A `--camera-look-at` with nothing to look from is not a camera: say so
        // rather than aiming from wherever each viewer happened to start, which
        // is a different place in each of them.
        (None, None) if options.camera_look_at.is_some() => {
            return Err(
                "--camera-look-at needs somewhere to look from: pass --camera-position or \
                 --look-at <landmark>"
                    .to_owned(),
            );
        }
        (None, None) => None,
    })
}

/// Check that this viewer's asset tree actually ships the skin (and theme) the
/// run named, before anything is launched.
///
/// Firestorm's harness refuses a skin it cannot resolve and says which it has;
/// this side does not — an unknown id reaches `bevy_flair` as a stylesheet path
/// that fails to load, and the run captures an **unstyled** interface while the
/// other viewer captures the skin that was asked for. That pair looks like a
/// catastrophic styling bug in this viewer rather than like a typo, which is
/// worth the half-second of `stat` calls to rule out.
///
/// # Errors
///
/// Returns a message naming the skins the tree does ship, when the named skin
/// or theme is not among them.
fn check_skin_is_shipped(asset_root: &Path, wanted: &SlClientSkin) -> Result<(), String> {
    // Typed, rather than two `&str`s: the `FirestormSkin` beside it in the plan
    // holds values this tree knows nothing about, and checking one against the
    // other's asset directory would reject every correct run.
    let Some(skin) = wanted.skin.as_deref() else {
        return Ok(());
    };
    let skins = asset_root.join("assets").join("skins");
    let available = || {
        let mut names: Vec<String> = fs_err::read_dir(&skins)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().join("skin.css").is_file())
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort();
        names.join(", ")
    };

    let dir = skins.join(skin);
    if !dir.join("skin.css").is_file() {
        return Err(format!(
            "--sl-client-skin {skin}: {} ships no such skin; available: {}",
            asset_root.display(),
            available()
        ));
    }
    if let Some(theme) = wanted.theme.as_deref() {
        let overlay = dir.join("themes").join(format!("{theme}.css"));
        if !overlay.is_file() {
            return Err(format!(
                "--sl-client-theme {theme}: the {skin} skin has no such theme ({} is not \
                 there)",
                overlay.display()
            ));
        }
    }
    Ok(())
}

/// The regions this run's grid is built from: one, or the scene's pair when
/// `--neighbour` (or a `--cross-after` that implies it) asked for two.
///
/// # Errors
///
/// Returns a message when a pair was asked for and the scene has no second
/// half. Standing up two copies of a one-region scene would look like it
/// worked — two identical regions, a border with nothing to say — so this
/// refuses rather than obliging.
fn regions_for(
    options: &Options,
    scene: &scenarios::NamedScenario,
) -> Result<Vec<RegionConfig>, String> {
    let mut regions = undressed_regions_for(options, scene)?;
    if options.day_position.is_some() {
        for region in &mut regions {
            if region.ensure_day_cycle_can_be_sampled() {
                tracing::info!(
                    "{}: the scene's environment schedules one sky, which no day position can \
                     choose between, so the region now serves the four legacy WindLight presets \
                     (midnight, sunrise, midday, sunset at 0.0 / 0.25 / 0.5 / 0.75)",
                    region.name
                );
            }
        }
    }
    Ok(regions)
}

/// [`regions_for`] before the pinned sun is taken into account: the scene's own
/// regions, dressed as the scene dresses them.
fn undressed_regions_for(
    options: &Options,
    scene: &scenarios::NamedScenario,
) -> Result<Vec<RegionConfig>, String> {
    let want_pair = options.neighbour || options.cross_after.is_some();
    let near = RegionConfig {
        name: NEAR_REGION.to_owned(),
        ..RegionConfig::default()
    };
    if !want_pair {
        return Ok(vec![scene.dress(near)]);
    }
    let pair = scene.pair().ok_or_else(|| {
        format!(
            "the {} scene is about one region, so there is no neighbour to stand up; the \
             scenes that have a second half are: {}",
            scene.name,
            scenarios::all()
                .iter()
                .filter(|scene| scene.pair().is_some())
                .map(|scene| scene.name)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let far = RegionConfig {
        name: FAR_REGION.to_owned(),
        grid_x: near.grid_x.saturating_add(1),
        ..RegionConfig::default()
    };
    let mut near = pair.dress_near(near);
    if let Some(after) = options.cross_after {
        near.scenario = Some(crossing_script(
            near.scenario.take().unwrap_or_default(),
            after,
        ));
    }
    Ok(vec![near, pair.dress_far(far)])
}

/// `scenario` with a step that walks the agent over the east border `after`
/// seconds from its arrival.
///
/// The script goes on the region rather than on the fixture because *when* a
/// crossing happens is a property of the run — it has to land inside the
/// capture window, which the frame count and interval decide — while *what*
/// stands either side of the border is a property of the scene.
fn crossing_script(mut scenario: Scenario, after: f32) -> Scenario {
    scenario.timeline = Timeline::new().then(
        At::AfterArrival(core::time::Duration::from_secs_f32(after)),
        Action::CrossRegion {
            region: FAR_REGION.to_owned(),
            position: RegionCoordinates::new(
                CROSSING_LANDING_X,
                sl_fake_grid::fixtures::border::MARKER_Y,
                // The ground plus half an avatar: a landing position is the
                // avatar's *centre*, and handing the terrain height straight
                // over buries it to the knees on the far side of the line.
                f32::from(sl_fake_grid::scenario::STOCK_TERRAIN_HEIGHT_M)
                    + sl_fake_grid::AVATAR_CENTRE_ABOVE_GROUND_M,
            ),
            velocity: Vector {
                x: CROSSING_SPEED,
                y: 0.0,
                z: 0.0,
            },
        },
    );
    scenario
}

/// Run one viewer and collect what it left.
fn run_viewer(
    launch: &Launch,
    deadline: core::time::Duration,
    interrupted: &Arc<AtomicBool>,
) -> ViewerRun {
    tracing::info!("running {}", launch.viewer.name());
    match process::run(launch, deadline, interrupted) {
        Ok(ran) => {
            if ran.ending == Ending::Killed {
                tracing::error!(
                    "{} had to be killed; the grid may still hold its session",
                    launch.viewer.name()
                );
            }
            ViewerRun::ran(
                launch.viewer,
                ran.ending,
                ran.duration,
                Artefacts::collect(&launch.artefacts),
            )
        }
        Err(error) => {
            tracing::error!("{} could not be run: {error}", launch.viewer.name());
            ViewerRun::failed_to_start(launch.viewer, error.to_string())
        }
    }
}

/// Print the run's report to standard output.
#[expect(
    clippy::print_stdout,
    reason = "the report is this binary's primary output"
)]
fn report(text: &str) {
    println!("{text}");
}

#[expect(
    clippy::too_many_lines,
    reason = "one function is the run: resolve what was asked for, start the grid, drive each \
              viewer, write the summary — splitting it would only move the order of those steps \
              somewhere else"
)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_error| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    // Before `parse`, because clap reads the environment-backed options as it
    // parses. The machine-specific part of a run — where this machine keeps its
    // Firestorm build — belongs in an uncommitted `.env` beside the sources
    // rather than in this repository or in a command line typed from memory.
    // A missing `.env` is the ordinary case, not an error.
    //
    // A parse failure is reported **without the line that caused it**: a `.env`
    // is where a person keeps things they did not want in this repository, and
    // a harness that echoes one into a run log has published it.
    match dotenvy::dotenv() {
        Ok(path) => tracing::debug!("read {} for SL_CROSSCHECK_* settings", path.display()),
        Err(error) if error.not_found() => {}
        Err(dotenvy::Error::LineParse(_line, index)) => tracing::warn!(
            "a line in .env could not be parsed (at character {index}), so none of it was \
             applied; the line itself is not logged, in case it holds a secret"
        ),
        Err(error) => tracing::warn!("could not read .env: {error}"),
    }
    let options = Options::parse();
    // Unreachable in practice — clap's possible-value parser rejects an unknown
    // name first — but the registry, not this binary, decides which names exist.
    let scene = scenarios::scenario(&options.scenario)
        .ok_or_else(|| format!("unknown scenario {:?}", options.scenario))?;
    let (first, last, password) = parse_account(&options.account)?;
    let camera = resolve_camera(&options, &scene)?;

    // Both binaries are checked before a grid exists: a missing viewer found
    // twenty minutes into a run is a missing viewer that wasted twenty minutes.
    let viewer_bin = options.viewer.clone().unwrap_or_else(|| {
        PathBuf::from(WORKSPACE_ROOT).join("target/release/sl-client-bevy-viewer")
    });
    let want_ours = options.only != Some(Which::Firestorm);
    let want_theirs = options.only != Some(Which::SlClient);
    if want_ours && !process::is_executable(&viewer_bin) {
        return Err(format!(
            "no viewer at {}; build it with `cargo build --release -p sl-client-bevy-viewer` \
             or pass --viewer",
            viewer_bin.display()
        )
        .into());
    }
    if let Some(firestorm) = &options.firestorm
        && want_theirs
        && !process::is_executable(firestorm)
    {
        return Err(format!("no Firestorm launcher at {}", firestorm.display()).into());
    }

    let dirs = RunDirs::new(
        options
            .run_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("crosscheck-runs").join(&options.scenario)),
    );
    dirs.create()?;

    // The grid lives on its own runtime while the main thread supervises the
    // viewers: process supervision is blocking work, and a blocking main thread
    // must not be the one the grid's tasks are waiting on.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let regions = regions_for(&options, &scene)?;
    let grid = runtime.block_on(async {
        let mut builder = FakeGridBuilder::new()
            .http_port(options.port)
            .grid_identity(GridIdentity {
                name: format!("Fake Grid ({})", options.scenario),
                nick: "fakegrid".to_owned(),
                ..GridIdentity::default()
            });
        for region in regions {
            builder = builder.region(region);
        }
        builder
            .account(AccountConfig::new(&first, &last, &password))
            .start()
            .await
    })?;
    tracing::info!(
        "scenario {:?} on {} — {}",
        scene.name,
        grid.login_uri(),
        scene.summary
    );
    let region_names = grid.region_names();
    if region_names.len() > 1 {
        tracing::info!("regions west to east: {}", region_names.join(", "));
    }
    if let Some(after) = options.cross_after {
        tracing::info!(
            "the agent walks into {FAR_REGION} {after} s after it arrives; the capture window \
             has to straddle that"
        );
    }
    for landmark in scene.landmarks() {
        tracing::info!(
            "landmark {:?} at <{}, {}, {}>",
            landmark.name,
            landmark.position.x,
            landmark.position.y,
            landmark.position.z
        );
    }

    let plan = RunPlan {
        scenario: options.scenario.clone(),
        login_uri: grid.login_uri(),
        first_name: first,
        last_name: last,
        password,
        capture: CaptureSpec {
            width: options.capture_size.0,
            height: options.capture_size.1,
            ui: options.capture_ui,
            hud: options.capture_hud,
            gizmos: options.capture_gizmos,
            audio: if options.capture_audio {
                CaptureAudio::Audible
            } else {
                CaptureAudio::Muted
            },
            frames: options.frames,
            interval: options.interval,
            settle_timeout: options.settle_timeout,
            login_timeout: options.login_timeout,
            day_position: options.day_position,
            fov_degrees: options.fov,
        },
        sl_client_skin: SlClientSkin {
            skin: options.sl_client_skin.clone(),
            theme: options.sl_client_theme.clone(),
        },
        firestorm_skin: FirestormSkin {
            skin: options.firestorm_skin.clone(),
            theme: options.firestorm_theme.clone(),
        },
        camera,
    };
    let config = files::write(&dirs.config(), &plan)?;
    let deadline = core::time::Duration::from_secs_f32(
        options
            .deadline
            .unwrap_or_else(|| plan.capture.suggested_deadline_secs()),
    );
    let interrupted = process::interrupt_flag()?;

    let asset_root = PathBuf::from(WORKSPACE_ROOT).join("sl-client-bevy-viewer");
    // Only when this viewer is in the run: a `--only firestorm` run is entitled
    // to name a skin this side does not have, and the harness over there does
    // its own checking.
    if want_ours {
        check_skin_is_shipped(&asset_root, &plan.sl_client_skin)?;
    }
    if !(plan.sl_client_skin.is_unset() && plan.firestorm_skin.is_unset()) && !plan.capture.ui {
        // A warning rather than an error: the skin is still applied, and a run
        // may well want it for something other than the frames. But a world-only
        // frame holds no interface, so nothing a skin changes can appear in it,
        // and a run that dressed a viewer to photograph none of it is almost
        // always one that meant to pass --capture-ui.
        tracing::warn!(
            "a skin was named but the frames hold the world only; pass --capture-ui to see it"
        );
    }

    let mut runs = Vec::new();
    if want_ours {
        let launch = launch::sl_client(
            &viewer_bin,
            &dirs,
            &plan,
            &config,
            fs_err::metadata(&asset_root)
                .is_ok()
                .then_some(asset_root.as_path()),
        );
        runs.push(run_viewer(&launch, deadline, &interrupted));
    } else {
        runs.push(ViewerRun::skipped(Viewer::SlClient, "--only firestorm"));
    }
    match (
        &options.firestorm,
        want_theirs,
        interrupted.load(Ordering::Relaxed),
    ) {
        (_firestorm, _wanted, true) => runs.push(ViewerRun::skipped(
            Viewer::Firestorm,
            "the run was interrupted",
        )),
        (Some(firestorm), true, false) => {
            let launch = launch::firestorm(firestorm, &dirs, &plan, &config)?;
            runs.push(run_viewer(&launch, deadline, &interrupted));
        }
        (None, true, false) => runs.push(ViewerRun::skipped(
            Viewer::Firestorm,
            "no --firestorm launcher was given",
        )),
        (_firestorm, false, false) => {
            runs.push(ViewerRun::skipped(Viewer::Firestorm, "--only sl-client"));
        }
    }

    let summary = RunSummary::new(&plan, runs);
    let path = dirs.root.join("run.json");
    fs_err::write(&path, serde_json::to_string_pretty(&summary)?)?;
    // The grid goes down before the report is printed, so nothing is still
    // listening while a person reads it and starts the next run.
    drop(grid);
    runtime.shutdown_timeout(core::time::Duration::from_secs(5));

    report(&format!(
        "\n{}\n\ncollected in {}\ncompare them with: sl-crosscheck-report {}",
        summary.render(),
        dirs.root.display(),
        dirs.root.display()
    ));
    // The exit status is about the *run* — did every viewer that was asked to
    // run produce frames — never about whether the two drew the same thing:
    // this binary has not looked at a pixel. A deliberate one-sided run
    // (`--only`, or no Firestorm to point at) therefore succeeds.
    if summary.ran_as_asked() {
        Ok(())
    } else {
        Err("a viewer that was asked to run produced nothing usable".into())
    }
}
