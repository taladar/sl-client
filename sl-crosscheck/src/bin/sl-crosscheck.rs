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

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;
use sl_crosscheck::launch::{Launch, RunDirs, Viewer};
use sl_crosscheck::plan::{CameraSpec, CaptureSpec, RegionPoint, RunPlan, parse_region_point};
use sl_crosscheck::process::{self, Ending};
use sl_crosscheck::status::Artefacts;
use sl_crosscheck::summary::{RunSummary, ViewerRun};
use sl_crosscheck::{files, launch};
use sl_fake_grid::fixtures::scenarios;
use sl_fake_grid::{AccountConfig, FakeGridBuilder, GridIdentity, RegionConfig};

/// The workspace root, as it stood when this binary was built. The viewer's own
/// vendored-asset defaults are resolved the same way, so a build that has been
/// moved away from its sources loses both together rather than one confusingly.
const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Command-line options.
#[derive(Debug, Parser)]
#[command(author, version, about)]
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
    #[arg(long)]
    day_position: Option<f32>,

    /// The account both viewers log in as, `First:Last:password`.
    #[arg(long, default_value = "Test:User:password")]
    account: String,

    /// Seconds a viewer is given before it is asked to quit. Derived from the
    /// timings above when unset.
    #[arg(long)]
    deadline: Option<f32>,
}

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
    let grid = runtime.block_on(async {
        FakeGridBuilder::new()
            .http_port(options.port)
            .grid_identity(GridIdentity {
                name: format!("Fake Grid ({})", options.scenario),
                nick: "fakegrid".to_owned(),
                ..GridIdentity::default()
            })
            .region(scene.dress(RegionConfig::default()))
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
            frames: options.frames,
            interval: options.interval,
            settle_timeout: options.settle_timeout,
            login_timeout: options.login_timeout,
            day_position: options.day_position,
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
        "\n{}\n\ncollected in {}",
        summary.render(),
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
