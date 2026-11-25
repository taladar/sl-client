//! **Replay benchmark** (`viewer-perf-region-capture-replay`): time a fixed
//! number of frames of a replayed scene, so two builds can be compared on
//! identical input.
//!
//! A run goes through three phases:
//!
//! 1. **Loading.** The simulation clock is paused until the scene has been quiet
//!    for a run of frames, while real time keeps running for what
//!    waits on the outside world, such as the retries of an asset the recording
//!    lacks. Everything that starts while it loads
//!    therefore starts at time zero, whichever order the assets arrived in and
//!    however long they took, so an animation's phase does not depend on the
//!    speed of the machine or of its disk.
//! 2. **Warm-up.** The clock advances by [`STEP`] per frame, and nothing is
//!    timed. Exposure adaptation, reflection probes and GPU pipelines settle.
//! 3. **Measuring.** The clock keeps its fixed step, the camera follows its
//!    path by frame number, and each frame's wall-clock duration is recorded.
//!
//! Every frame therefore simulates the same moment in every run, and only the
//! time it took to produce differs.
//!
//! A headless viewer renders inline by default: the render world runs on the
//! main thread after the main world, so a frame costs both added together. A
//! windowed viewer overlaps them on Bevy's render thread, and its frame costs
//! the longer of the two plus the extract between them.
//! [`ReplayBenchmark::pipelined`] keeps that render thread, so the frame time
//! is the one a live window would show; inline frames are the ones to profile,
//! because each system's cost then lands on the frame that ran it. Particle emitters still draw from an
//! unseeded random source, so their exact positions vary between runs.

use core::time::Duration;
use std::path::{Path, PathBuf};
use std::time::Instant;

use bevy::camera::visibility::VisibilitySystems;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use serde::{Deserialize, Serialize};
use sl_viewer_world_api::{
    AvatarAnchor, ObjectCategory, SceneObject, SuppressAllAvatars, TerrainSurface, ViewerCamera,
};
use sl_viewer_world_objects::objects::WorldRootObject;
use sl_viewer_world_scene::environment::EnvironmentState;

use crate::quiescence::SceneQuiescence;

/// The simulated time each frame advances by once the scene has loaded.
pub const STEP: Duration = Duration::from_nanos(16_666_667);

/// How many consecutive quiet frames count as loaded.
const QUIET_HOLD_FRAMES: u32 = 30;

/// The version of the report's layout. Version 2 added
/// [`BenchmarkReport::pipelined`].
const REPORT_SCHEMA_VERSION: u32 = 2;

/// Which part of the scene a benchmark draws, to attribute frame cost.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum BenchmarkLayers {
    /// Everything the recording holds.
    #[default]
    All,
    /// Terrain and objects, without avatars or what they wear.
    World,
    /// Avatars and what they wear, without terrain or other objects.
    Avatars,
}

impl BenchmarkLayers {
    /// Whether avatars are drawn.
    const fn shows_avatars(self) -> bool {
        !matches!(self, Self::World)
    }

    /// Whether terrain and in-world objects are drawn.
    const fn shows_world(self) -> bool {
        !matches!(self, Self::Avatars)
    }
}

/// One point on a camera path, in Second Life region coordinates (Z-up metres).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraKey {
    /// Where the camera is.
    pub position: [f32; 3],
    /// The point it looks at.
    pub look_at: [f32; 3],
}

impl CameraKey {
    /// The camera pose of this key in Bevy's Y-up space.
    const fn endpoints(self) -> (Vec3, Vec3) {
        (sl_point(self.position), sl_point(self.look_at))
    }
}

/// A Second Life Z-up point as a Bevy Y-up one.
const fn sl_point([x, y, z]: [f32; 3]) -> Vec3 {
    Vec3::new(x, z, -y)
}

/// Read a camera path: a JSON array of [`CameraKey`]s.
///
/// # Errors
///
/// Returns a description if the file cannot be read or parsed, or holds no
/// keys.
pub fn load_camera_path(path: &Path) -> Result<Vec<CameraKey>, String> {
    let text = fs_err::read_to_string(path).map_err(|error| error.to_string())?;
    let keys: Vec<CameraKey> = serde_json::from_str(&text)
        .map_err(|error| format!("camera path {}: {error}", path.display()))?;
    if keys.is_empty() {
        return Err(format!("camera path {} has no keys", path.display()));
    }
    Ok(keys)
}

/// The camera pose at `progress` (`0.0..=1.0`) along `keys`, with equal time
/// between neighbouring keys. `None` for an empty path.
fn pose_on_path(keys: &[CameraKey], progress: f32) -> Option<Transform> {
    let (first, rest) = keys.split_first()?;
    let (mut eye, mut focus) = first.endpoints();
    if let Some(segments) = u16::try_from(rest.len()).ok().filter(|count| *count > 0) {
        let along = progress.clamp(0.0, 1.0) * f32::from(segments);
        let mut start = *first;
        for (index, key) in rest.iter().enumerate() {
            let begins = u16::try_from(index).map_or(f32::MAX, f32::from);
            let fraction = along - begins;
            if fraction <= 1.0 || index.saturating_add(1) == rest.len() {
                let (from_eye, from_focus) = start.endpoints();
                let (to_eye, to_focus) = key.endpoints();
                let fraction = fraction.clamp(0.0, 1.0);
                eye = from_eye.lerp(to_eye, fraction);
                focus = from_focus.lerp(to_focus, fraction);
                break;
            }
            start = *key;
        }
    }
    Some(Transform::from_translation(eye).looking_at(focus, Vec3::Y))
}

/// What a benchmark run does.
#[derive(Debug, Clone, Resource)]
pub struct ReplayBenchmark {
    /// Where the JSON report goes.
    pub output: PathBuf,
    /// How many frames are timed.
    pub frames: u32,
    /// How many frames run between loading and timing.
    pub warmup: u32,
    /// Which part of the scene is drawn.
    pub layers: BenchmarkLayers,
    /// The pinned position in the day cycle, reported with the results.
    pub day_position: f32,
    /// The camera path, or empty to hold the start pose.
    pub camera_path: Vec<CameraKey>,
    /// How long the scene may take to load before the run fails.
    pub load_timeout: Duration,
    /// The version of the build, reported with the results.
    pub version: String,
    /// Whether the render world runs on Bevy's render thread, overlapping the
    /// main world as in a windowed viewer, instead of inline after it.
    pub pipelined: bool,
}

/// Where a run has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Waiting for the scene to go quiet, for this many frames so far.
    Loading {
        /// Consecutive quiet frames.
        quiet_frames: u32,
    },
    /// Running untimed frames at the fixed step.
    WarmingUp {
        /// Frames run so far.
        frame: u32,
    },
    /// Timing frames.
    Measuring,
    /// The report has been written or the run has failed.
    Finished,
}

/// The progress of the running benchmark.
#[derive(Debug, Resource)]
struct BenchmarkRun {
    /// The current phase.
    phase: Phase,
    /// When the run started, for the load timeout.
    started: Instant,
    /// How long loading took.
    load_time: Duration,
    /// The start of the frame being timed.
    frame_start: Option<Instant>,
    /// The durations of the frames timed so far, in milliseconds.
    frame_ms: Vec<f64>,
}

/// The percentile summary of a set of frame times, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FrameTimeSummary {
    /// The arithmetic mean.
    pub mean_ms: f64,
    /// The median.
    pub p50_ms: f64,
    /// The 95th percentile.
    pub p95_ms: f64,
    /// The 99th percentile.
    pub p99_ms: f64,
    /// The slowest frame.
    pub max_ms: f64,
}

/// The nearest-rank percentile of `sorted`, or `0.0` if it is empty.
fn percentile(sorted: &[f64], percent: u32) -> f64 {
    let count = sorted.len();
    let rank = count
        .saturating_mul(usize::try_from(percent).unwrap_or(usize::MAX))
        .div_ceil(100)
        .saturating_sub(1);
    sorted
        .get(rank.min(count.saturating_sub(1)))
        .copied()
        .unwrap_or(0.0)
}

/// Summarise a set of frame times.
#[must_use]
pub fn summarise(frame_ms: &[f64]) -> FrameTimeSummary {
    let mut sorted = frame_ms.to_vec();
    sorted.sort_by(f64::total_cmp);
    let total: f64 = sorted.iter().sum();
    let count = u32::try_from(sorted.len()).map_or_else(|_| f64::from(u32::MAX), f64::from);
    FrameTimeSummary {
        mean_ms: if sorted.is_empty() {
            0.0
        } else {
            total / count
        },
        p50_ms: percentile(&sorted, 50),
        p95_ms: percentile(&sorted, 95),
        p99_ms: percentile(&sorted, 99),
        max_ms: sorted.last().copied().unwrap_or(0.0),
    }
}

/// The report a run writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkReport {
    /// The version of this layout.
    pub schema_version: u32,
    /// The version of the build that ran.
    pub version: String,
    /// Which part of the scene was drawn.
    pub layers: BenchmarkLayers,
    /// The pinned position in the day cycle.
    pub day_position: f32,
    /// What that pin selected: the region's own sky, or a stand-in.
    pub day_position_source: String,
    /// The simulated seconds per frame.
    pub step_seconds: f64,
    /// How many untimed frames ran before measuring.
    pub warmup_frames: u32,
    /// How many keys the camera path had.
    pub camera_keys: usize,
    /// How long the scene took to load, in seconds.
    pub load_seconds: f64,
    /// Whether the render world overlapped the main world on its own thread
    /// (`false` in a version 1 report, which only rendered inline).
    #[serde(default)]
    pub pipelined: bool,
    /// The percentile summary of [`Self::frame_ms`].
    pub summary: FrameTimeSummary,
    /// Every timed frame, in milliseconds, in order.
    pub frame_ms: Vec<f64>,
}

/// Runs a benchmark in a `--replay` app.
#[derive(Debug)]
pub struct ReplayBenchmarkPlugin {
    /// What the run does.
    pub benchmark: ReplayBenchmark,
}

impl Plugin for ReplayBenchmarkPlugin {
    fn build(&self, app: &mut App) {
        if !self.benchmark.layers.shows_avatars() {
            app.insert_resource(SuppressAllAvatars);
        }
        if let Some(mut clock) = app.world_mut().get_resource_mut::<Time<Virtual>>() {
            clock.pause();
        }
        app.insert_resource(self.benchmark.clone())
            .insert_resource(BenchmarkRun {
                phase: Phase::Loading { quiet_frames: 0 },
                started: Instant::now(),
                load_time: Duration::ZERO,
                frame_start: None,
                frame_ms: Vec::new(),
            })
            .add_systems(First, advance_benchmark)
            .add_systems(
                Update,
                follow_camera_path
                    .after(crate::camera::drive_flycam)
                    .before(crate::camera::position_camera),
            )
            .add_systems(
                PostUpdate,
                hide_world_layer.before(VisibilitySystems::VisibilityPropagate),
            );
    }
}

/// Step the run's phases, and time the frame that just ended.
fn advance_benchmark(
    benchmark: Res<ReplayBenchmark>,
    mut run: ResMut<BenchmarkRun>,
    mut simulation: ResMut<Time<Virtual>>,
    mut commands: Commands,
    quiescence: SceneQuiescence,
    environment: Res<EnvironmentState>,
    mut exit: MessageWriter<AppExit>,
) {
    match run.phase {
        Phase::Loading { quiet_frames } => {
            let quiet_frames = if quiescence.is_quiet() {
                quiet_frames.saturating_add(1)
            } else {
                0
            };
            if quiet_frames >= QUIET_HOLD_FRAMES {
                run.load_time = run.started.elapsed();
                info!(
                    "replay benchmark: loaded in {:.1} s, warming up for {} frame(s)",
                    run.load_time.as_secs_f64(),
                    benchmark.warmup
                );
                commands.insert_resource(TimeUpdateStrategy::ManualDuration(STEP));
                simulation.unpause();
                run.phase = Phase::WarmingUp { frame: 0 };
            } else if run.started.elapsed() > benchmark.load_timeout {
                error!(
                    "replay benchmark: the scene did not load within {} s; outstanding: {:?}",
                    benchmark.load_timeout.as_secs(),
                    quiescence.breakdown()
                );
                run.phase = Phase::Finished;
                exit.write(AppExit::error());
            } else {
                run.phase = Phase::Loading { quiet_frames };
            }
        }
        Phase::WarmingUp { frame } => {
            if frame >= benchmark.warmup {
                run.phase = Phase::Measuring;
                run.frame_start = Some(Instant::now());
            } else {
                run.phase = Phase::WarmingUp {
                    frame: frame.saturating_add(1),
                };
            }
        }
        Phase::Measuring => {
            let now = Instant::now();
            if let Some(start) = run.frame_start {
                let elapsed = now.duration_since(start).as_secs_f64() * 1000.0;
                run.frame_ms.push(elapsed);
            }
            run.frame_start = Some(now);
            if run.frame_ms.len() >= usize::try_from(benchmark.frames).unwrap_or(usize::MAX) {
                run.phase = Phase::Finished;
                let report = BenchmarkReport {
                    schema_version: REPORT_SCHEMA_VERSION,
                    version: benchmark.version.clone(),
                    layers: benchmark.layers,
                    day_position: benchmark.day_position,
                    day_position_source: format!("{:?}", environment.day_position_pin()),
                    step_seconds: STEP.as_secs_f64(),
                    warmup_frames: benchmark.warmup,
                    camera_keys: benchmark.camera_path.len(),
                    load_seconds: run.load_time.as_secs_f64(),
                    pipelined: benchmark.pipelined,
                    summary: summarise(&run.frame_ms),
                    frame_ms: core::mem::take(&mut run.frame_ms),
                };
                exit.write(match write_report(&benchmark.output, &report) {
                    Ok(()) => {
                        info!(
                            "replay benchmark: p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms; wrote {}",
                            report.summary.p50_ms,
                            report.summary.p95_ms,
                            report.summary.p99_ms,
                            benchmark.output.display()
                        );
                        AppExit::Success
                    }
                    Err(reason) => {
                        error!("replay benchmark: {reason}");
                        AppExit::error()
                    }
                });
            }
        }
        Phase::Finished => {}
    }
}

/// Write `report` to `path` as JSON.
fn write_report(path: &Path, report: &BenchmarkReport) -> Result<(), String> {
    let text = serde_json::to_string(report).map_err(|error| error.to_string())?;
    fs_err::write(path, text).map_err(|error| error.to_string())
}

/// Put the camera where its path says for this frame. Warm-up holds the first
/// key, so the frames before measuring already look at what is measured.
fn follow_camera_path(
    benchmark: Res<ReplayBenchmark>,
    run: Res<BenchmarkRun>,
    mut cameras: Query<&mut Transform, With<ViewerCamera>>,
) {
    if benchmark.camera_path.is_empty() {
        return;
    }
    let progress = match run.phase {
        Phase::Loading { .. } | Phase::WarmingUp { .. } => 0.0,
        Phase::Measuring => {
            let last = benchmark.frames.saturating_sub(1).max(1);
            let frame = u32::try_from(run.frame_ms.len()).unwrap_or(u32::MAX);
            // Both fit f64 exactly; the ratio is a fraction of the path.
            let ratio = f64::from(frame.min(last)) / f64::from(last);
            #[expect(
                clippy::as_conversions,
                clippy::cast_possible_truncation,
                reason = "a 0.0..=1.0 fraction; f32 precision is ample for a camera path"
            )]
            let progress = ratio as f32;
            progress
        }
        Phase::Finished => 1.0,
    };
    let Some(pose) = pose_on_path(&benchmark.camera_path, progress) else {
        return;
    };
    if let Ok(mut transform) = cameras.single_mut() {
        transform.set_if_neq(pose);
    }
}

/// The root entities of in-world objects: not avatars' own, and not terrain.
type WorldRootFilter = (
    With<WorldRootObject>,
    Without<AvatarAnchor>,
    Without<TerrainSurface>,
);

/// Hide terrain and in-world objects when only avatars are measured. New ones
/// keep arriving while the scene loads, so this runs every frame, and writes
/// only what changes so that it adds no change-detection work of its own.
fn hide_world_layer(
    benchmark: Res<ReplayBenchmark>,
    mut terrain: Query<&mut Visibility, (With<TerrainSurface>, Without<SceneObject>)>,
    mut objects: Query<(&SceneObject, &mut Visibility), WorldRootFilter>,
) {
    if benchmark.layers.shows_world() {
        return;
    }
    for mut visibility in &mut terrain {
        visibility.set_if_neq(Visibility::Hidden);
    }
    for (object, mut visibility) in &mut objects {
        if object.category != ObjectCategory::Avatar {
            visibility.set_if_neq(Visibility::Hidden);
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    use super::{BenchmarkLayers, BenchmarkReport, CameraKey, percentile, pose_on_path, summarise};

    /// Whether two values agree to well within what these tests distinguish.
    fn close(left: f64, right: f64) -> bool {
        (left - right).abs() < 1e-3
    }

    /// A key at `x` looking along +X.
    const fn key(x: f32) -> CameraKey {
        CameraKey {
            position: [x, 0.0, 0.0],
            look_at: [x + 1.0, 0.0, 0.0],
        }
    }

    #[test]
    fn percentiles_use_the_nearest_rank() {
        let sorted: Vec<f64> = (1..=100).map(f64::from).collect();
        for percent in [50, 95, 99, 100] {
            assert!(close(percentile(&sorted, percent), f64::from(percent)));
        }
    }

    /// A version 1 report predates the render-thread switch and only ever
    /// rendered inline, so it must still read, as an inline run.
    #[test]
    fn a_version_one_report_reads_as_an_inline_run() -> Result<(), serde_json::Error> {
        let report: BenchmarkReport = serde_json::from_str(
            r#"{"schema_version":1,"version":"0.1.0","layers":"all","day_position":0.5,
            "day_position_source":"Region","step_seconds":0.016666667,"warmup_frames":300,
            "camera_keys":0,"load_seconds":2.0,"summary":{"mean_ms":1.0,"p50_ms":1.0,
            "p95_ms":1.0,"p99_ms":1.0,"max_ms":1.0},"frame_ms":[1.0]}"#,
        )?;
        assert!(!report.pipelined);
        Ok(())
    }

    #[test]
    fn a_short_run_never_reads_past_its_end() {
        assert!(close(percentile(&[7.0], 99), 7.0));
        assert!(close(percentile(&[], 99), 0.0));
    }

    #[test]
    fn the_summary_does_not_depend_on_frame_order() {
        let summary = summarise(&[4.0, 1.0, 3.0, 2.0]);
        assert!(close(summary.mean_ms, 2.5));
        assert!(close(summary.p50_ms, 2.0));
        assert!(close(summary.max_ms, 4.0));
    }

    #[test]
    fn a_path_interpolates_between_its_keys() {
        let keys = [key(0.0), key(10.0), key(30.0)];
        for (progress, expected) in [
            (0.0, 0.0),
            (0.25, 5.0),
            (0.5, 10.0),
            (0.75, 20.0),
            (1.0, 30.0),
        ] {
            let x = pose_on_path(&keys, progress).map(|pose: Transform| pose.translation.x);
            assert!(x.is_some_and(|x| close(f64::from(x), expected)));
        }
    }

    #[test]
    fn a_single_key_holds_still() {
        let pose = pose_on_path(&[key(3.0)], 0.7).map(|pose| pose.translation);
        assert_eq!(pose, Some(Vec3::new(3.0, 0.0, 0.0)));
    }

    #[test]
    fn an_empty_path_has_no_pose() {
        assert_eq!(pose_on_path(&[], 0.5), None);
    }

    #[test]
    fn layers_select_what_is_drawn() {
        assert!(BenchmarkLayers::All.shows_avatars() && BenchmarkLayers::All.shows_world());
        assert!(!BenchmarkLayers::World.shows_avatars());
        assert!(!BenchmarkLayers::Avatars.shows_world());
    }
}
