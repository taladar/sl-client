//! The **pixel** half of the render test harness (`viewer-render-readback-tier`):
//! render a registered scene headlessly, read the frame back off the GPU, and
//! decide things about it that geometry cannot answer.
//!
//! # Why this exists, and what it already caught
//!
//! [`crate::render_test`] answers "is this geometry valid" and cannot answer
//! **"did the right pixels light up"**. That gap is not academic — it is where
//! the whole reflection / lighting half of the registry lives, and the bugs found
//! there so far were all found by a human squinting at a mirror:
//!
//! - **R22i** — every local reflection probe reflected the world rotated 90°
//!   about X. No invariant broken, no log line, no crash: the probe captured, the
//!   volume bound, the mirror was shiny, and the reflection was plausible from any
//!   angle you had not thought about. It was found by a person asking "is the
//!   yellow one where the yellow one should be".
//! - **A probe volume that did not contain its own mirror**, so the sphere sat in
//!   the falloff band and blended a second, parallax-wrong reflection over the
//!   first. Found the same way: by looking.
//!
//! Both are decidable by *sampling a pixel*, which is what this module does. "Is
//! the yellow one where the yellow one should be" is a question a machine can
//! answer, and it should, because a human answering it needs a login-free gallery,
//! a mirror, four distinctly coloured neighbours, and the patience to notice.
//!
//! # What is asserted, and what deliberately is not
//!
//! **Not golden images.** Pixel-exact comparison across drivers turns the suite
//! into a driver-version detector, and a suite that fails on a Mesa upgrade is one
//! that gets disabled. Nothing here compares against a reference frame.
//!
//! What is asserted is *decidable*: **where a known colour lands**. The scene puts
//! a strongly and distinctly coloured prim on each side of a mirror, and the check
//! asks which side of the mirror each colour's reflection came back on — a
//! question with a right answer that no driver difference changes, and one that a
//! 90° rotation fails loudly.
//!
//! # Cost, and why it is a separate tier
//!
//! This needs a real GPU adapter. [`crate::render_test`] must never depend on one
//! — it is the tier that has to run everywhere, and it holds most of the value —
//! so the two are kept strictly apart and this one **skips** (loudly) when no
//! adapter is available rather than failing.

use core::time::Duration;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::RenderTarget;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::DirectionalLightShadowMap;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::pipelined_rendering::PipelinedRenderingPlugin;
use bevy::render::render_resource::{PipelineCache, TextureFormat, TextureUsages};
use bevy::render::{Render, RenderApp, RenderSystems};
use bevy::time::TimeUpdateStrategy;
use bevy::winit::WinitPlugin;

use crate::pixel_oracle::Frame;
use crate::probes::ProbeCaptureStats;
use crate::render_test::{LogCapture, capture_logs};
use crate::viewer_camera::viewer_camera_bundle;
use crate::viewer_plugins::ViewerRenderPlugins;
use crate::world_api::ViewerCamera;

use crate::render_scene::{
    CAP_PAIR_OPAQUE_X, CAP_PAIR_TOP, CAP_PAIR_TRANSLUCENT_X, MATRIX_BOXES, MATRIX_DISTANCE,
    MATRIX_EYES, MATRIX_HALF_DEPTH, MATRIX_HALF_HEIGHT, RenderScene, SCENE_WATER_LEVEL,
    STRADDLING_EMERGENT, SceneAssets, SceneCx, SceneRuntimePlugin, scene_root,
    scene_root_transform,
};

/// Half the straddling prim's emergent height — the middle of the band the water
/// scene's check samples. A constant rather than a division at the call site: the
/// workspace's `arithmetic_side_effects` lint bans the operator there.
const HALF_EMERGENT: f32 = STRADDLING_EMERGENT / 2.0;

/// One sample of the translucency matrix: which box, which band of it, and the
/// world point at the middle of that band on the box's **near** face.
///
/// The near face because that is the one the camera sees, and its middle because
/// a band's edges are a grazing-angle smear of it and its neighbour. A box's top
/// cap gets its own cell, sampled at the middle of the cap.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MatrixCell {
    /// The box's label, from `MATRIX_BOXES`.
    pub(crate) box_label: &'static str,
    /// Which part of the box this samples: `above`, `below`, or `top`.
    pub(crate) band: &'static str,
    /// Whether the sampled point is above the water surface.
    pub(crate) emergent: bool,
    /// The point to project, in Second Life metres (the frame the scenes are
    /// written in).
    pub(crate) point: Vec3,
}

/// Every cell of the translucency matrix: for each box, the middle of its
/// emergent band, the middle of its submerged band, and its top cap — skipping a
/// band the box does not have (one clear of the surface has only one).
pub(crate) fn matrix_cells() -> Vec<MatrixCell> {
    let mut cells = Vec::new();
    for (box_label, x, offset) in MATRIX_BOXES {
        let centre = SCENE_WATER_LEVEL + offset;
        let top = centre + MATRIX_HALF_HEIGHT;
        let bottom = centre - MATRIX_HALF_HEIGHT;
        // The near face stands half the box's depth toward the camera, which is
        // back along -Y.
        let near = -MATRIX_HALF_DEPTH;
        if top > SCENE_WATER_LEVEL {
            let from = bottom.max(SCENE_WATER_LEVEL);
            cells.push(MatrixCell {
                box_label,
                band: "above",
                emergent: true,
                point: Vec3::new(x, near, f32::midpoint(from, top)),
            });
        }
        if bottom < SCENE_WATER_LEVEL {
            let to = top.min(SCENE_WATER_LEVEL);
            cells.push(MatrixCell {
                box_label,
                band: "below",
                emergent: false,
                point: Vec3::new(x, near, f32::midpoint(bottom, to)),
            });
        }
        cells.push(MatrixCell {
            box_label,
            band: "top",
            emergent: top > SCENE_WATER_LEVEL,
            // The middle of the cap, not its near edge.
            point: Vec3::new(x, 0.0, top),
        });
    }
    cells
}

/// A frame row index as a float, to compare against a projected screen `y`.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "the frame is FRAME (256) rows, so a row index converts to f32 exactly"
)]
const fn row_to_f32(row: u32) -> f32 {
    row as f32
}

/// The rendered frame's size, in pixels.
///
/// Small, deliberately. Every assertion here is about *where a colour landed*,
/// which a 256² frame answers as well as a 4K one — and the frame is rendered by
/// a probe rig that re-renders the scene six times per capture, so the cost is
/// paid over and over.
const FRAME: u32 = 256;

/// The same frame side as a `u16`, which is the width a pixel coordinate
/// converts from **losslessly** — the form [`crate::render_test::framing_pixel`]
/// projects into. `u32::from` is not a `const` call yet, so this is a second
/// literal rather than a conversion, and
/// `tests::the_two_spellings_of_the_frame_size_agree` holds the two together.
pub(crate) const FRAME_SIDE: u16 = 256;

/// The manual per-frame timestep while the clock runs: `globals.time` — what the
/// water, flipbook and particle shaders read — advances by exactly this per
/// `update`, never by the wall clock, so what a frame shows depends on how many
/// frames were stepped and not on how fast the machine stepped them.
const STEP: f32 = 1.0 / 30.0;

/// The one-frame timestep as a `Duration`, for `TimeUpdateStrategy`.
const STEP_DURATION: Duration = Duration::from_nanos(33_333_333);

/// How many consecutive **quiet** frames — no pipeline queued or compiling, every
/// live reflection probe captured at least once — [`settle`] insists on before it
/// trusts the rig.
///
/// The old rig waited a fixed 400 frames, measured against the mirror: at 90 it
/// read pure **black** (a metallic surface takes all its colour from the
/// environment map, and `crate::probes` captures one cube face per frame in
/// six-frame bursts, after which Bevy filters the cube into the maps the shader
/// samples), at 400 it reflected correctly. That number was a proxy for two
/// things the rig can now observe directly — the pipeline queue and the probe
/// bursts — plus one it cannot: the environment-map filter, which runs in the
/// render world with nothing to poll. The streak covers that last one.
///
/// Measured (2026-08-30, RADV): with a streak of **1** the mirror settles on
/// frame 7 after its first probe burst and the check already passes — the
/// observable conditions carry it, and the filter finishes within the frames
/// [`frame_at`] steps afterwards. Thirty is margin for the filter, at a cost of
/// a fifth of a second of scene time per settle.
const QUIET_STREAK: u32 = 30;

/// The most frames [`settle`] steps before it gives up. Generous: a settle that
/// runs out is a real failure with a report, not a flake.
const MAX_SETTLE_FRAMES: u32 = 1500;

/// How many frames the rig steps without any frame coming back before it
/// concludes there is no GPU adapter. A working adapter returns its first frame
/// within a handful of updates even while every pipeline is still compiling.
const NO_ADAPTER_FRAMES: u32 = 90;

/// Frames stepped with the clock **held** before a frame is read. A readback
/// completes a frame or more after its render, so the slot must be given time to
/// hold a frame that was rendered under the held clock rather than the one before.
const HOLD_FRAMES: u32 = 4;

/// The scene time a static scene is captured at, in seconds. One second rather
/// than zero, so a driver that needs time to have passed — a fountain that has
/// emitted, a flexi that has come to rest — is captured doing what it does.
pub(crate) const CAPTURE_AT_SECS: f32 = 1.0;

/// A scene time in seconds as a whole number of [`STEP`] frames.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a small positive frame count computed from a scene time in seconds"
)]
fn frames_for(seconds: f32) -> u32 {
    (seconds / STEP).round().max(0.0) as u32
}

/// Serialise the GPU tests within one process.
///
/// `cargo nextest` runs each test in its own process, and there the `gpu`
/// test-group in `.config/nextest.toml` is what runs the readback tests one at a
/// time. Plain `cargo test` runs them as threads of one process, and two headless
/// render apps racing for the adapter under a concurrent build is exactly the
/// load pattern the tier used to flake under. Poisoning is ignored: a test that
/// failed must not fail the ones after it.
pub(crate) fn gpu_lock() -> MutexGuard<'static, ()> {
    static GPU: Mutex<()> = Mutex::new(());
    GPU.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How many pipelines the render world still has queued or compiling, mirrored
/// into the main world every frame.
///
/// Shared through an atomic rather than extracted, because extraction copies
/// main → render and this travels the other way. A frame rendered while a
/// pipeline is still compiling simply omits whatever that pipeline draws — the
/// "pre-render black" the old fixed warm-up was papering over.
#[derive(Resource, Clone, Default)]
pub(crate) struct PipelineStatus(Arc<AtomicU32>);

impl PipelineStatus {
    /// Pipelines queued or compiling as of the last render.
    pub(crate) fn waiting(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Publishes [`PipelineStatus`]: the same cell in both worlds, written from the
/// render world's cleanup set each frame.
struct PipelineStatusPlugin;

impl Plugin for PipelineStatusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PipelineStatus>();
    }

    fn finish(&self, app: &mut App) {
        let status = app.world().resource::<PipelineStatus>().clone();
        // No render app means no adapter; `settle` reports that by outcome.
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(status).add_systems(
                Render,
                publish_pipeline_status.in_set(RenderSystems::Cleanup),
            );
        }
    }
}

/// Render-world system: count the pipelines not yet ready into [`PipelineStatus`].
fn publish_pipeline_status(cache: Res<PipelineCache>, status: Res<PipelineStatus>) {
    let waiting = u32::try_from(cache.waiting_pipelines().count()).unwrap_or(u32::MAX);
    status.0.store(waiting, Ordering::Relaxed);
}

/// Why [`settle`] did not settle.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SettleError {
    /// No frame ever came back: this machine has no usable GPU adapter, and the
    /// tier skips rather than fails.
    NoAdapter,
    /// Frames came back but the quiet streak never held.
    NeverSettled {
        /// Frames stepped before giving up.
        frames: u32,
        /// Pipelines still queued or compiling on the last frame.
        waiting_pipelines: u32,
        /// Whether every live probe had captured by the last frame.
        probes_captured: bool,
    },
}

impl core::fmt::Display for SettleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoAdapter => {
                f.write_str("no frame came back, so this machine has no usable GPU adapter")
            }
            Self::NeverSettled {
                frames,
                waiting_pipelines,
                probes_captured,
            } => write!(
                f,
                "after {frames} frames {waiting_pipelines} pipeline(s) were still queued or \
                 compiling and every live reflection probe had{} captured",
                if *probes_captured { "" } else { " not" }
            ),
        }
    }
}

impl core::error::Error for SettleError {}

/// Step the rig until it is **quiet**: a frame has come back, no pipeline is queued
/// or compiling, and every live reflection probe has completed a burst — and has
/// stayed that way for [`QUIET_STREAK`] consecutive frames.
///
/// The clock is whatever the caller left it at (held at zero on a fresh rig, so a
/// scene settles at a known time), and nothing is read: the caller decides what
/// time to capture at and calls [`frame_at`].
pub(crate) fn settle(app: &mut App, captured: &Captured) -> Result<(), SettleError> {
    let mut quiet = 0_u32;
    let mut frames = 0_u32;
    let mut waiting = 0_u32;
    let mut probes = false;
    while frames < MAX_SETTLE_FRAMES {
        app.update();
        frames = frames.saturating_add(1);
        // Detected by **outcome**, not by inspecting the app: a frame either came
        // back off the GPU or it did not. `get_sub_app(RenderApp)` looks like the
        // obvious test and used to report `false` on a machine that rendered
        // perfectly well, which would have skipped this tier everywhere, silently.
        let frame_back = captured.0.lock().is_ok_and(|slot| slot.is_some());
        if !frame_back {
            if frames >= NO_ADAPTER_FRAMES {
                return Err(SettleError::NoAdapter);
            }
            continue;
        }
        waiting = app.world().resource::<PipelineStatus>().waiting();
        // A rig without the probe plugin has no probes to wait for.
        probes = app
            .world()
            .get_resource::<ProbeCaptureStats>()
            .is_none_or(ProbeCaptureStats::every_live_rig_captured);
        quiet = if waiting == 0 && probes {
            quiet.saturating_add(1)
        } else {
            0
        };
        if quiet >= QUIET_STREAK {
            return Ok(());
        }
    }
    Err(SettleError::NeverSettled {
        frames,
        waiting_pipelines: waiting,
        probes_captured: probes,
    })
}

/// Advance the scene clock by `seconds` (one [`STEP`] per frame), hold it, settle
/// again — something that first appears once time has passed, a particle burst or
/// a flexi at rest, may queue a pipeline of its own — and read the frame rendered
/// under the held clock.
///
/// Every frame this returns was rendered at a scene time that is a function of
/// the calls made, never of how long the machine took to compile or capture.
/// Returns `None` only when no frame came back; a rig that stops settling
/// panics with the report, because that is a failure and not a skip.
pub(crate) fn frame_at(app: &mut App, captured: &Captured, seconds: f32) -> Option<Vec<u8>> {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(STEP_DURATION));
    for _frame in 0..frames_for(seconds) {
        app.update();
    }
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    if let Err(error) = settle(app, captured) {
        assert!(
            matches!(error, SettleError::NoAdapter),
            "the rig stopped settling after the clock advanced: {error}"
        );
        return None;
    }
    for _frame in 0..HOLD_FRAMES {
        app.update();
    }
    captured.0.lock().ok()?.take()
}

/// Fail loudly on anything the rig logged at `WARN` or above while it rendered.
///
/// This is where the harness's log universal finally bites for the render world:
/// R26 was logged by Bevy's mesh allocator, which lives in the render app, and
/// with pipelined rendering off that app runs on this thread, under this
/// capture.
fn check_logs(logs: &LogCapture, scene: &str) {
    let events = logs.events();
    assert!(
        events.is_empty(),
        "rendering `{scene}` logged a warning or an error:\n  {}",
        events.join("\n  ")
    );
}

/// Where a readback lands: filled by the `ReadbackComplete` observer, drained by
/// [`capture`].
///
/// A shared cell rather than a `Message`, because the readback completes in the
/// render world a frame or more after it is asked for, and the test needs to poll
/// for it rather than be handed it inside a system.
#[derive(Resource, Clone, Default)]
pub(crate) struct Captured(Arc<Mutex<Option<Vec<u8>>>>);

/// Where a set of world points landed on the frame, in pixels.
///
/// Returned alongside the frame because a pixel check almost always needs to
/// restrict itself to **one object's** pixels, and the only honest way to know
/// which those are is to ask the same camera that drew them. Guessing a disc from
/// the field of view by hand is how a check ends up measuring the background.
#[derive(Clone, Debug, Default)]
pub(crate) struct Projected(pub(crate) Vec<Option<Vec2>>);

/// A world point projected to the frame, by index into the `points` given to
/// [`capture`].
impl Projected {
    /// The `index`th point's pixel position, if it is in front of the camera.
    pub(crate) fn get(&self, index: usize) -> Option<Vec2> {
        self.0.get(index).copied().flatten()
    }
}

/// Build the headless readback app for `scene`: the viewer's real material
/// pipelines and scene runtime, a render-to-texture camera at the scene's declared
/// pose, and a `Readback` that drains each rendered frame into the returned
/// [`Captured`] cell. The caller drives the `update`s and reads the cell —
/// [`settle`] until the rig is quiet, then [`frame_at`] a chosen scene time.
///
/// The clock starts **held at zero** and only moves when a caller advances it,
/// so a scene settles at a known time and is captured at a known time.
pub(crate) fn build_readback_app(scene: &RenderScene, cx: SceneCx) -> (App, Captured) {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                // Headless: no window at all, and the app must not exit for the
                // lack of one.
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(ImagePlugin::default_nearest())
            // No event loop: the test drives `update` itself, so the frames are
            // counted rather than raced.
            .disable::<WinitPlugin>()
            // No render thread either: with the render app run inline, one
            // `update` is exactly one rendered frame, and everything the render
            // world logs lands on this thread's log capture.
            .disable::<PipelinedRenderingPlugin>()
            // The test harness owns the subscriber (`crate::render_test`'s
            // `capture_logs` may be installed); two would clash.
            .disable::<LogPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(Duration::ZERO))
    .add_plugins(PipelineStatusPlugin)
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));

    // The viewer's own render stack, in its bare form: the material pipelines
    // every registered scene is drawn with, the reflection probes (without which a
    // mirror reflects nothing and the check is vacuous), the water-relative
    // transparency ordering (without which a water scene renders a *different
    // picture* — the sea writes depth, so which translucent content is drawn before
    // it is decided there), the waterline split and the probe render-layer
    // propagation `scene_root()` relies on. Not the sky, the sea or the lights:
    // the registered scenes stage their own, and two skies would be no scene at
    // all. `SceneRuntimePlugin` then fills in the drivers the viewer registers
    // elsewhere, so a scene renders here what it renders in the viewer.
    app.add_plugins(ViewerRenderPlugins::bare())
        .add_plugins(SceneRuntimePlugin)
        .insert_resource(DirectionalLightShadowMap::default())
        // No flat ambient, stated rather than inherited from Bevy's default. Every
        // assertion here is about a *reflection*, and the probes supplying the
        // ambient is precisely the viewer's lighting model (the sky scales the
        // ambient it asks for by `probes::probe_ambient_scale`, `0.0` by default).
        // Bevy's default 80 nits of fill would wash the very contrast between a
        // mirror's four coloured neighbours that decides which side each landed on.
        .insert_resource(GlobalAmbientLight {
            brightness: 0.0,
            ..default()
        })
        .init_resource::<Captured>();

    let captured = app.world().resource::<Captured>().clone();

    // The render target: an ordinary image, plus `COPY_SRC` so the readback can
    // lift it back off the GPU.
    // `new_target_texture` sets TEXTURE_BINDING | COPY_DST | RENDER_ATTACHMENT, as
    // `crate::probes` relies on for its capture faces; the readback additionally
    // reads the frame as a copy source.
    let mut target = Image::new_target_texture(FRAME, FRAME, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(target);

    let scene_camera = scene.camera;
    let spawn = scene.spawn;
    let readback_target = target.clone();
    app.add_systems(
        Startup,
        move |mut commands: Commands, mut assets: SceneAssets| {
            let root = commands.spawn(scene_root()).id();
            spawn(cx, root, &mut commands, &mut assets);

            // The scene's declared camera pose, converted from Second Life
            // region-local metres exactly as `crate::render_gallery` converts it.
            let basis = scene_root_transform().rotation;
            let position = basis.mul_vec3(scene_camera.position);
            let look_at = basis.mul_vec3(scene_camera.look_at);
            // The viewer's own camera bundle (its exposure, HDR target and the
            // `ViewerCamera` marker `install_global_probe` binds the default probe
            // to), aimed into the readback target — in Bevy 0.19 the render target
            // is its own component, the same way `crate::probes` targets its
            // capture faces.
            commands
                .spawn((
                    viewer_camera_bundle(
                        Transform::from_translation(position).looking_at(look_at, Vec3::Y),
                    ),
                    RenderTarget::Image(readback_target.clone().into()),
                    Name::new("readback-camera"),
                ))
                // The bundle switches Bevy's tone mapper off because the viewer
                // tone-maps in its own pass — which the bare stack does not run.
                // Without any tone mapper the HDR frame lands in the 8-bit target
                // raw and clipped, and a half-transparent face blended over a
                // bright plate reads as opaque. Keep Bevy's, as the rig always had.
                .insert(Tonemapping::default());
            commands.spawn(Readback::texture(readback_target.clone()));
        },
    );
    app.add_observer(
        move |readback: On<ReadbackComplete>, captured: Res<Captured>| {
            if let Ok(mut slot) = captured.0.lock() {
                *slot = Some(readback.data.clone());
            }
        },
    );

    (app, captured)
}

/// Render one registered scene, read the frame back, and project `points` (in
/// **Bevy world space**) onto it.
///
/// Returns `None` when no frame came back — see [the module docs](self): a
/// machine with no GPU adapter cannot answer these questions and should say so
/// rather than fail.
pub(crate) fn capture(
    scene: &RenderScene,
    cx: SceneCx,
    points: &[Vec3],
) -> Option<(Frame, Projected)> {
    capture_with(scene, cx, points, |_app| {})
}

/// [`capture`], with a `prepare` hook that may add systems or resources to the
/// built app before it runs — how a test stages something *around* a scene (an
/// occluder, a marker subject, a sea) without registering a scene for it.
pub(crate) fn capture_with(
    scene: &RenderScene,
    cx: SceneCx,
    points: &[Vec3],
    prepare: impl FnOnce(&mut App),
) -> Option<(Frame, Projected)> {
    let _gpu = gpu_lock();
    let (logs, _guard) = capture_logs();
    let (mut app, captured) = build_readback_app(scene, cx);
    prepare(&mut app);

    // `App::finish`/`cleanup` build the render app; if there is no adapter this is
    // where it gives up, and a machine without a GPU should skip rather than fail.
    app.finish();
    app.cleanup();
    if let Err(error) = settle(&mut app, &captured) {
        assert!(
            matches!(error, SettleError::NoAdapter),
            "the `{}` scene never settled: {error}",
            scene.id
        );
        return None;
    }

    // A scene with a timeline is captured at its last sample — the moment its
    // own declaration says it has done what it does — and a static one at
    // `CAPTURE_AT_SECS`.
    let at = scene
        .timeline
        .samples
        .last()
        .copied()
        .unwrap_or(0.0)
        .max(CAPTURE_AT_SECS);
    let frame = Frame::from_rgba8(frame_at(&mut app, &captured, at)?, FRAME, FRAME)?;
    check_logs(&logs, scene.id);

    // Project through the very camera that drew the frame, rather than
    // re-deriving its projection by hand.
    let mut cameras = app
        .world_mut()
        .query_filtered::<(&Camera, &GlobalTransform), With<ViewerCamera>>();
    let projected = cameras
        .single(app.world())
        .map(|(camera, transform)| {
            Projected(
                points
                    .iter()
                    .map(|point| camera.world_to_viewport(transform, *point).ok())
                    .collect(),
            )
        })
        .unwrap_or_default();
    Some((frame, projected))
}

/// Render `scene` at two different `globals.time` values and read both frames back,
/// for verifying **GPU-time-driven** animation. A texture animation now runs
/// entirely in the shader (`face_material.wgsl`'s `sl_animated_uv` from
/// `globals.time`), so it is invisible to any CPU-state digest — the only honest
/// check is that the rendered **pixels** actually differ over time. Both samples
/// land at fixed scene times, so identical times render identical bytes and any
/// difference is the animation.
///
/// Returns `None` on a machine with no GPU adapter (like [`capture`]).
pub(crate) fn capture_over_time(scene: &RenderScene, cx: SceneCx) -> Option<(Frame, Frame)> {
    /// The first sample's scene time: past the start, a few flipbook cells in.
    const EARLY_SECS: f32 = 0.5;
    /// Scene time between the two samples: many flipbook cells later.
    const BETWEEN_SECS: f32 = 1.8;

    let _gpu = gpu_lock();
    let (logs, _guard) = capture_logs();
    let (mut app, captured) = build_readback_app(scene, cx);
    app.finish();
    app.cleanup();
    if let Err(error) = settle(&mut app, &captured) {
        assert!(
            matches!(error, SettleError::NoAdapter),
            "the `{}` scene never settled: {error}",
            scene.id
        );
        return None;
    }
    let early = Frame::from_rgba8(frame_at(&mut app, &captured, EARLY_SECS)?, FRAME, FRAME)?;
    let later = Frame::from_rgba8(frame_at(&mut app, &captured, BETWEEN_SECS)?, FRAME, FRAME)?;
    check_logs(&logs, scene.id);
    Some((early, later))
}

#[cfg(test)]
mod tests {
    use super::{
        CAP_PAIR_OPAQUE_X, CAP_PAIR_TOP, CAP_PAIR_TRANSLUCENT_X, CAPTURE_AT_SECS, FRAME,
        HALF_EMERGENT, MATRIX_DISTANCE, MATRIX_EYES, SCENE_WATER_LEVEL, SettleError,
        build_readback_app, capture, capture_over_time, check_logs, frame_at, gpu_lock,
        matrix_cells, row_to_f32, settle,
    };
    use crate::pixel_oracle::{
        CellVerdict, Marker, Silhouette, centroid, differing_pixels, dominant, read_cell,
    };
    use crate::render_scene::{SCENES, SceneCx};
    use crate::render_test::{TestError, capture_logs};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// The frame's two spellings are the same number.
    #[test]
    fn the_two_spellings_of_the_frame_size_agree() {
        assert_eq!(
            u32::from(super::FRAME_SIDE),
            FRAME,
            "the CPU projection would map onto a different frame from the one the rig renders"
        );
    }

    /// **The CPU framing projection is the camera that actually drew the frame.**
    ///
    /// [`crate::render_test::framing_pixel`] reproduces this rig's camera without
    /// a renderer, so the render tier can *record* where a subject's centre lands
    /// in the picture — a baseline fact that costs no GPU. That is only worth
    /// anything while the reproduction is faithful, and it is a reproduction:
    /// a changed basis, pose or projection here would leave the recorded pixels
    /// looking perfectly stable while the picture moved.
    ///
    /// So the two are held to each other on any machine that can render, at points
    /// spread across the frame rather than at the centre alone — a centre-only
    /// check passes under a wrong field of view.
    ///
    /// Skips when no frame came back (no GPU adapter).
    #[test]
    fn the_cpu_framing_projection_agrees_with_the_rendered_camera() -> Result<(), TestError> {
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "prim-box")
            .ok_or("the `prim-box` scene is not registered")?;
        // Bevy world space, spread over the frame: the origin the camera aims at,
        // and four points off it on each axis.
        let points = [
            Vec3::ZERO,
            Vec3::new(0.4, 0.0, 0.0),
            Vec3::new(-0.4, 0.0, 0.0),
            Vec3::new(0.0, 0.4, 0.0),
            Vec3::new(0.0, 0.0, 0.4),
        ];
        let Some((_frame, projected)) = capture(scene, SceneCx::new(), &points) else {
            return Ok(());
        };
        for (index, point) in points.iter().enumerate() {
            let rendered = projected
                .get(index)
                .ok_or("the rig's own camera put a test point off the frame")?;
            let cpu = crate::render_test::framing_pixel(scene.camera, *point)
                .ok_or("the CPU projection put a test point off the frame")?;
            let dx = (rendered.x - cpu.x).abs();
            let dy = (rendered.y - cpu.y).abs();
            assert!(
                dx < 0.5 && dy < 0.5,
                "the CPU projection of {point:?} lands at {cpu} where the camera that drew the \
                 frame puts it at {rendered} — the reproduction has drifted from the rig, so \
                 every recorded framing pixel is measuring something else"
            );
        }
        Ok(())
    }

    /// **Each neighbour's reflection lands on the mirror's own side of it.**
    ///
    /// The check the mirror scene exists for, and the one that would have caught
    /// **R22i** — every local reflection probe reflecting the world rotated 90°
    /// about X — without a human noticing that a yellow reflection faced the
    /// camera instead of pointing down.
    ///
    /// The claim is deliberately geometric rather than photometric: the red prim is
    /// at `-X` and the green at `+X`, so on a mirror ball between them the red
    /// reflection must come back on the **opposite side of the ball** from the
    /// green — and the pair on the other axis (blue behind, yellow below) likewise.
    /// No golden image, no exact pixel, nothing a driver version moves. An axis
    /// swap fails it.
    ///
    /// Skips when no frame came back (no GPU adapter — see the module docs),
    /// because a machine that cannot render cannot answer.
    #[test]
    fn the_mirror_reflects_each_neighbour_on_its_own_side() -> Result<(), TestError> {
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "metallic-sphere-among-prims")
            .ok_or("the `metallic-sphere-among-prims` scene is not registered")?;
        // The mirror's centre, and a point on its silhouette — projected by the
        // camera that draws the frame, so the disc is measured rather than
        // guessed. The sphere is a 1 m ball at the scene origin; `Vec3::Y * 0.5`
        // is on its surface, and any perpendicular offset would do.
        let Some((frame, projected)) = capture(
            scene,
            SceneCx::new(),
            &[Vec3::ZERO, Vec3::new(0.0, 0.5, 0.0)],
        ) else {
            warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
            return Ok(());
        };
        let (centre, edge) = projected
            .get(0)
            .zip(projected.get(1))
            .ok_or("the mirror did not project onto the frame — the camera is not looking at it")?;
        // Inside the silhouette, not on it: the rim is a grazing-angle smear of
        // everything at once and says nothing about direction.
        let radius = Vec2::new(edge.x - centre.x, edge.y - centre.y).length() * 0.85;
        assert!(
            radius > 8.0,
            "the mirror covers only {radius} px of the frame — too few to tell a reflection's \
             side from rounding"
        );

        // Blue is deliberately not required. It sits *behind* the ball, and on a
        // mirror sphere the world behind reflects into the **limb** — a
        // grazing-angle sliver a few pixels wide, which is a flake waiting to
        // happen rather than a check.
        let disc = Silhouette { centre, radius };
        let found: Vec<(Marker, Vec2)> = [Marker::Red, Marker::Green, Marker::Yellow]
            .into_iter()
            .filter_map(|marker| centroid(&frame, disc, marker).map(|at| (marker, at)))
            .collect();
        assert_eq!(
            found.len(),
            3_usize,
            "the red, green and yellow neighbours must each appear *in the mirror*; found {:?} \
             — if one is missing the mirror is not reflecting it at all, and every comparison \
             below would pass by looking at nothing",
            found
                .iter()
                .map(|(marker, _)| marker.name())
                .collect::<Vec<_>>()
        );
        let at = |marker: Marker| -> Vec2 {
            found
                .iter()
                .find(|(name, _)| *name == marker)
                .map_or(Vec2::ZERO, |(_, at)| *at)
        };
        let (red, green, yellow) = (at(Marker::Red), at(Marker::Green), at(Marker::Yellow));

        // Red (`-X`) and green (`+X`) must come back on opposite sides of the ball.
        let horizontal = (red.x - green.x).abs();
        assert!(
            horizontal > radius * 0.5,
            "the red (-X) and green (+X) neighbours must reflect on opposite sides of the \
             mirror, but landed only {horizontal} px apart across a {radius} px disc (red at \
             {red}, green at {green})"
        );

        // **The R22i check.** Yellow is *below* the mirror, so it must reflect off
        // the ball's underside — screen-down, which is `+y` (the projected `edge`
        // above sits at a smaller `y` than the centre, so world up is screen up).
        //
        // This pair is the whole point and the horizontal one above cannot replace
        // it: R22i rotates the sampled direction about **X**, and a rotation about
        // X does not move the X axis — red and green stay exactly where they
        // belong while the world turns underneath them. Under the bug the
        // downward neighbour is read as pointing at the viewer and its reflection
        // walks to the middle of the ball, which is precisely how a human
        // described it: "the yellow reflection faces the camera instead of facing
        // downwards".
        let below = yellow.y - centre.y;
        assert!(
            below > radius * 0.3,
            "the yellow neighbour is below the mirror, so its reflection must come back off the \
             underside — but it landed {below} px below the centre of a {radius} px disc \
             (yellow at {yellow}, centre {centre}). A reflection of the world-below arriving at \
             the middle of the ball is R22i: the probe is sampling its cube through the Second \
             Life -> Bevy basis change instead of in the world space it was captured in"
        );
        Ok(())
    }

    /// **A texture animation actually moves on screen.**
    ///
    /// The flipbook's animation runs entirely in the shader
    /// (`face_material.wgsl`'s `sl_animated_uv`, evaluated from `globals.time`), so
    /// its material's CPU state does not change frame to frame — a CPU-state digest
    /// (`crate::render_test`) cannot see it, and a CPU re-evaluation would only test
    /// the Rust reference against itself. The one honest check is that the **rendered
    /// pixels** differ at two different times: render the 4×4 flipbook near the start
    /// and ~1.8 s later (many cells on, at 8 fps) and require the frame to have
    /// changed. This is what fails if the WGSL animation is wrong or never runs.
    #[test]
    fn a_texture_animation_actually_moves_on_screen() -> Result<(), TestError> {
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "texture-anim-flipbook")
            .ok_or("the texture-anim-flipbook scene is not registered")?;
        let Some((early, later)) = capture_over_time(scene, SceneCx::new()) else {
            // No GPU adapter: skip, like the rest of this pixel tier.
            return Ok(());
        };
        // Deterministic rendering: identical times render identical bytes, so any
        // difference is the animation. Require a substantial change (hundreds of
        // pixels) so a stray texel could never pass it — a real cell change repaints a
        // large part of the face.
        let differing = differing_pixels(&early, &later, None);
        assert!(
            differing > 250,
            "the flipbook rendered near-identically at two different times ({differing} of {} \
             pixels differ) — its GPU texture animation did not change the frame, so the shader \
             is not animating what is on screen (or the prim did not render at all)",
            early.size().element_product(),
        );
        Ok(())
    }

    /// **A legacy Blinn-Phong material with a specular map renders without crashing.**
    ///
    /// The `legacy-material-face` scene sets a normal *and* a specular map (the full
    /// legacy workflow), so rendering it exercises the shader's `MAP_FLAG_SPEC`
    /// re-sample path and the analytic Blinn-Phong lobe on the GPU — the path that
    /// only runs once a specular map is actually assigned (the build tool's Material
    /// tab, or a sim material that carries one). A shader / bind-group fault there
    /// aborts the render, so simply getting a frame back is the check.
    #[test]
    fn a_legacy_specular_material_renders() -> Result<(), TestError> {
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "legacy-material-face")
            .ok_or("the legacy-material-face scene is not registered")?;
        let Some((frame, _projected)) = capture(scene, SceneCx::new(), &[]) else {
            // No GPU adapter: skip, like the rest of this pixel tier.
            return Ok(());
        };
        assert!(
            !frame.bytes().is_empty(),
            "the legacy specular scene produced an empty frame — the specular-map render path faulted"
        );
        Ok(())
    }

    /// **Adding a specular map to a live legacy material does not fault the render.**
    ///
    /// The build tool's Material tab previews a legacy edit by mutating an
    /// **already-prepared** face material in place — the case the static
    /// [`a_legacy_specular_material_renders`] scene does not cover. Bevy frees and
    /// **fully recreates** an `ExtendedMaterial`'s (bindless) bind group on any
    /// change, so turning the specular map from the fallback to a real texture at
    /// runtime re-prepares the material against the live GPU resources. This drives
    /// exactly that transition (clear the specular map, render, re-add it, render) on
    /// the legacy scene's materials — which carry a normal map too, so it is the real
    /// "add a specular map to a bumped face" edit. A bind-group / bindless fault
    /// aborts the render; getting frames back after the mutation is the check.
    #[test]
    fn adding_a_specular_map_at_runtime_does_not_crash() -> Result<(), TestError> {
        use crate::face_material::{FaceMaterial, MAP_FLAG_SPEC};

        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "legacy-material-face")
            .ok_or("the legacy-material-face scene is not registered")?;
        let _gpu = gpu_lock();
        let (logs, _guard) = capture_logs();
        let (mut app, captured) = build_readback_app(scene, SceneCx::new());
        app.finish();
        app.cleanup();
        match settle(&mut app, &captured) {
            Ok(()) => {}
            // No GPU adapter (no frame came back): skip, like the rest of this tier.
            Err(SettleError::NoAdapter) => return Ok(()),
            Err(error) => return Err(format!("the legacy scene never settled: {error}").into()),
        }
        if frame_at(&mut app, &captured, CAPTURE_AT_SECS).is_none() {
            return Ok(());
        }

        // The legacy face materials and their (real) specular map handles.
        let spec_faces: Vec<(AssetId<FaceMaterial>, Handle<Image>)> = app
            .world()
            .resource::<Assets<FaceMaterial>>()
            .iter()
            .filter(|(_id, material)| material.extension.params.map_flags & MAP_FLAG_SPEC != 0)
            .map(|(id, material)| (id, material.extension.specular_map.clone()))
            .collect();
        assert!(
            !spec_faces.is_empty(),
            "the legacy scene has no specular-mapped face to mutate — the fixture changed"
        );

        // Real -> fallback: drop the specular map (the live preview clearing it).
        {
            let mut materials = app.world_mut().resource_mut::<Assets<FaceMaterial>>();
            for (id, _handle) in &spec_faces {
                if let Some(mut material) = materials.get_mut(*id) {
                    material.extension.specular_map = Handle::default();
                    material.extension.params.map_flags &= !MAP_FLAG_SPEC;
                }
            }
        }
        app.update();

        // Fallback -> real: re-add the specular map at runtime (the faulting edit).
        {
            let mut materials = app.world_mut().resource_mut::<Assets<FaceMaterial>>();
            for (id, handle) in &spec_faces {
                if let Some(mut material) = materials.get_mut(*id) {
                    material.extension.specular_map = handle.clone();
                    material.extension.params.map_flags |= MAP_FLAG_SPEC;
                }
            }
        }
        // A frame after the re-prepare, rendered settled: reaching here without an
        // abort means the runtime re-prepare survived, and the log check catches a
        // fault that was reported rather than fatal.
        if frame_at(&mut app, &captured, 0.0).is_none() {
            return Err("no frame came back after the specular map was re-added".into());
        }
        check_logs(&logs, "legacy-material-face (runtime specular map)");
        Ok(())
    }

    /// **The sea shows what is behind it.**
    ///
    /// The water surface is opaque, as the reference's is, and everything you see
    /// through it is a sample of a copy of the screen taken before the surface was
    /// drawn (`WaterMaterial::reads_view_transmission_texture`, the reference's
    /// `screenTex`). So the `water-surface` scene puts a strongly red slab on the sea
    /// bed, under the water and in front of the camera: with the refraction sample
    /// working, its colour reaches the frame *through* the sea; with the sample gone
    /// — an unbound copy, a wrong uv, a surface that went back to being a flat tint —
    /// an opaque sea hides it completely and no pixel is red.
    ///
    /// Nothing else in that scene is red, so this needs no golden image and no
    /// driver-stable exact value: it asks whether any pixel of the sea is dominated by
    /// the one channel only the submerged slab can supply.
    #[test]
    fn the_sea_shows_what_is_behind_it() -> Result<(), TestError> {
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "water-surface")
            .ok_or("the `water-surface` scene is not registered")?;
        let Some((frame, _projected)) = capture(scene, SceneCx::new(), &[]) else {
            // No GPU adapter: skip, like the rest of this pixel tier.
            warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
            return Ok(());
        };

        // Count the pixels the slab's red could have reached. A whole-frame sweep
        // rather than a projected point: the refraction *displaces* the sample, so
        // where exactly the slab lands is precisely what this must not assume.
        let mut red = 0_u32;
        for y in 0..FRAME {
            for x in 0..FRAME {
                let Some(pixel) = frame.pixel(x, y) else {
                    continue;
                };
                if dominant(pixel) == Some(Marker::Red) {
                    red = red.saturating_add(1);
                }
            }
        }
        assert!(
            red > 100,
            "no part of the sea shows the red slab lying on the sea bed ({red} pixels \
             are red), so the water surface is not sampling the screen copy behind it \
             — an opaque sea with no refraction hides whatever is under it",
        );
        Ok(())
    }

    /// **A translucent prim that stands out of the water is still drawn.**
    ///
    /// `crate::transparency` sorts each translucent item into the pre-water bucket
    /// or the post-water one by **its centre height**, and the sea is drawn between
    /// them, opaque and writing depth. A prim centred *on* the waterline has
    /// fragments on both sides but only one centre, so all of it goes in one bucket
    /// — and when that is the pre-water one, the half standing above the surface is
    /// drawn before the sea, writes no depth of its own (it is alpha-blended), and
    /// the sea behind it then paints straight over it. It does not merely sort
    /// wrong: it vanishes.
    ///
    /// The reference has no such case, because it draws the same faces in **both**
    /// alpha pools, each clipped per fragment against the water plane
    /// (`lldrawpoolalpha.cpp`'s `waterSign` / `WATER_WATERPLANE`).
    ///
    /// The scene's prim is the only green thing in it, so this asks whether any
    /// pixel *above the waterline* is dominated by green — a question with a right
    /// answer that no driver difference changes. The band is located by projecting
    /// the prim's emergent half through the very camera that drew the frame, rather
    /// than by assuming where it landed.
    #[test]
    fn a_translucent_prim_standing_out_of_the_water_is_drawn() -> Result<(), TestError> {
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "water-straddling-translucent-prim")
            .ok_or("the `water-straddling-translucent-prim` scene is not registered")?;
        // The waterline at the prim, and the middle of its emergent half — in Bevy's
        // frame (Y up), which is what `capture` projects.
        let Some((frame, projected)) = capture(
            scene,
            SceneCx::new(),
            &[
                Vec3::new(0.0, SCENE_WATER_LEVEL, 0.0),
                Vec3::new(0.0, SCENE_WATER_LEVEL + HALF_EMERGENT, 0.0),
            ],
        ) else {
            warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
            return Ok(());
        };
        let (waterline, emergent) = projected
            .get(0)
            .zip(projected.get(1))
            .ok_or("the prim did not project onto the frame — the camera is not looking at it")?;
        // Screen `y` grows downward, so the emergent half is *above* the waterline
        // on screen: everything between the two rows is the band under test.
        assert!(
            emergent.y < waterline.y,
            "the emergent half projected below the waterline ({emergent:?} vs {waterline:?}) — \
             the scene's camera is not above the surface",
        );
        let top = emergent.y.max(0.0);
        let bottom = waterline.y.min(row_to_f32(FRAME));
        let mut green = 0_u32;
        for y in 0..FRAME {
            let row = row_to_f32(y);
            if row < top || row > bottom {
                continue;
            }
            for x in 0..FRAME {
                let Some(pixel) = frame.pixel(x, y) else {
                    continue;
                };
                if dominant(pixel) == Some(Marker::Green) {
                    green = green.saturating_add(1);
                }
            }
        }
        assert!(
            green > 100,
            "the half of the translucent prim that stands above the water is not on screen \
             ({green} green pixels between rows {top} and {bottom}), so the depth-writing sea \
             painted over it — a prim straddling the surface is bucketed whole by its centre, \
             and the reference instead clips the same faces per fragment into both alpha pools",
        );
        Ok(())
    }

    /// How steep the view to a sample must be — rise over run — before the matrix
    /// asserts on a band that sits on the **far side of the water surface from the
    /// eye**.
    ///
    /// Such a band is seen *through* the surface, so what reaches the frame is a
    /// refracted sample, displaced by the wave normal where the ray crosses. The
    /// displacement grows without bound as the view flattens, and the waves animate
    /// from `globals.time`, so a shallow cell lands somewhere slightly different
    /// every run: `grazing: sunk below` came back `solid` standalone and
    /// `background` under a loaded parallel run. Below this slope such a cell is
    /// reported and not asserted; above it the displacement is small against the
    /// band and the sample is stable.
    const REFRACTED_MIN_SLOPE: f32 = 0.06;

    /// How close, in metres, a box's cap may come to the eye's own height before
    /// the matrix stops asserting on it.
    ///
    /// A cap is horizontal, so an eye level with it sees it exactly edge-on: it
    /// covers no pixels worth sampling and the projected point lands on whatever is
    /// behind. The grazing eye is level with *some* cap by construction — that is
    /// what grazing means — so those cells are reported and skipped rather than
    /// asserted, which is honest about what the fixture can see.
    const EDGE_ON_MARGIN: f32 = 1.0;

    /// What one walk of a matrix scene reports: a line per cell, and the subset of
    /// those lines whose verdict was not the wanted one. `None` where the machine
    /// has no GPU adapter and the tier skips.
    type MatrixWalk = Option<(Vec<String>, Vec<String>)>;

    /// Walk one matrix scene at eye height `eye` (an offset from the water level)
    /// and return a line per cell, plus the cells whose verdict is not `wanted`.
    fn walk_matrix(id: &str, eye: f32, wanted: &[CellVerdict]) -> Result<MatrixWalk, TestError> {
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == id)
            .ok_or_else(|| TestError::from(format!("the `{id}` scene is not registered")))?;
        let cells = matrix_cells();
        // Second Life metres to Bevy's frame, the same basis change the scene root
        // and the declared camera pose go through.
        let basis = crate::render_scene::scene_root_transform().rotation;
        let points: Vec<Vec3> = cells
            .iter()
            .map(|cell| basis.mul_vec3(cell.point))
            .collect();
        let Some((frame, projected)) = capture(scene, SceneCx::new(), &points) else {
            return Ok(None);
        };
        let mut report = Vec::new();
        let mut wrong = Vec::new();
        for (index, cell) in cells.iter().enumerate() {
            let verdict = read_cell(&frame, projected.get(index), Marker::Green, Marker::Red);
            let shown = verdict.map_or_else(
                || "off-frame".to_owned(),
                |verdict| format!("{verdict:?}").to_lowercase(),
            );
            let side = if cell.emergent {
                "emergent"
            } else {
                "submerged"
            };
            let eye_height = SCENE_WATER_LEVEL + eye;
            // A cap the eye is level with is edge-on; see `EDGE_ON_MARGIN`.
            let edge_on = cell.band == "top" && (cell.point.z - eye_height).abs() < EDGE_ON_MARGIN;
            // A band on the far side of the surface from the eye is seen through it,
            // and refracted; see `REFRACTED_MIN_SLOPE`.
            let across = cell.emergent != (eye_height > SCENE_WATER_LEVEL);
            let run = cell
                .point
                .distance(Vec3::new(0.0, -MATRIX_DISTANCE, eye_height));
            let slope = if run > 0.0 {
                (cell.point.z - eye_height).abs() / run
            } else {
                f32::INFINITY
            };
            let skipped = if edge_on {
                " [edge-on, not asserted]"
            } else if across && slope < REFRACTED_MIN_SLOPE {
                " [refracted at a grazing angle, not asserted]"
            } else {
                ""
            };
            let line = format!(
                "{} {} ({side}) → {shown}{skipped}",
                cell.box_label, cell.band
            );
            if skipped.is_empty() && !verdict.is_some_and(|verdict| wanted.contains(&verdict)) {
                wrong.push(line.clone());
            }
            report.push(line);
        }
        Ok(Some((report, wrong)))
    }

    /// **A half-transparent top cap is not drawn as an opaque one.**
    ///
    /// Seen from above, a translucent prim's top cap reads as solid on the grid
    /// ([[viewer-translucent-top-face-reads-opaque]]), and a photograph cannot say
    /// whether it is drawing opaque or blending: alpha blends **linear radiance**
    /// and the tone mapper compresses afterwards, so a face much brighter than what
    /// is behind it keeps most of its opaque *appearance* at half coverage.
    ///
    /// The scene supplies the control the grid cannot — two boxes identical but for
    /// their alpha, each with a red plate sealed inside just under its cap. The
    /// plate is the only red in the scene and it does not move, so the question is
    /// the same three-way one the matrix asks and not a distance against the
    /// animated sea: red through the half-transparent cap, none through the opaque
    /// one.
    #[test]
    fn a_half_transparent_cap_shows_what_is_sealed_under_it() -> Result<(), TestError> {
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "water-translucent-cap-pair")
            .ok_or("the `water-translucent-cap-pair` scene is not registered")?;
        let basis = crate::render_scene::scene_root_transform().rotation;
        let caps = [CAP_PAIR_TRANSLUCENT_X, CAP_PAIR_OPAQUE_X]
            .map(|x| basis.mul_vec3(Vec3::new(x, 0.0, CAP_PAIR_TOP)));
        let Some((frame, projected)) = capture(scene, SceneCx::new(), &caps) else {
            warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
            return Ok(());
        };
        let (translucent, opaque) = projected
            .get(0)
            .zip(projected.get(1))
            .ok_or("a cap did not project onto the frame — the camera is not looking at them")?;
        // The same patch read the matrix uses, so one stray pixel decides nothing.
        let through = |at| {
            read_cell(&frame, Some(at), Marker::Green, Marker::Red)
                .is_some_and(|verdict| verdict == CellVerdict::Translucent)
        };
        assert!(
            through(translucent),
            "the plate sealed under the half-transparent cap did not reach the frame, so that \
             cap is drawing solid rather than compositing what is behind it",
        );
        assert!(
            !through(opaque),
            "the plate reached the frame through the **opaque** cap — the scene is not framing \
             what this test thinks it is, so its other half proves nothing",
        );
        Ok(())
    }

    /// **Every translucent face over open sea is drawn.**
    ///
    /// The sea is opaque and writes depth; a translucent face writes none. So any
    /// face handed to the pre-water pass that is *not* actually behind the surface
    /// is painted over by the sea and disappears — which is the shape of the defect
    /// reported from the grid, where the emergent half of a prim resting mostly
    /// submerged is simply absent.
    ///
    /// Walked over every box in `MATRIX_BOXES` and every band of it, from three eye
    /// heights, because which side of the surface is the far one flips with the eye
    /// and the bucket flips with it. Nothing in these scenes is red, so a cell is
    /// either the prim's green or it is not the prim.
    #[test]
    fn every_translucent_face_over_the_sea_is_drawn() -> Result<(), TestError> {
        let mut all = Vec::new();
        let mut wrong = Vec::new();
        for (eye, height) in MATRIX_EYES {
            let id = format!("water-translucency-{eye}-sea");
            // Both verdicts mean "the prim's green is there": nothing in these
            // scenes is deliberately red, but a face drawn over a *bright*
            // background — sun glint, a pale horizon — carries the red channel
            // anyway, and that is a drawn face, not a see-through one. Only
            // `missing` and `background` mean the face is not on screen.
            let Some((report, missing)) =
                walk_matrix(&id, height, &[CellVerdict::Solid, CellVerdict::Translucent])?
            else {
                warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
                return Ok(());
            };
            all.extend(report.iter().map(|line| format!("{eye}: {line}")));
            wrong.extend(missing.iter().map(|line| format!("{eye}: {line}")));
        }
        assert!(
            wrong.is_empty(),
            "translucent faces are missing from the frame over open sea — the depth-writing \
             sea painted over faces the water bucket put in front of it.\n  wrong:\n    {}\n  \
             whole matrix:\n    {}",
            wrong.join("\n    "),
            all.join("\n    "),
        );
        Ok(())
    }

    /// **Every translucent face shows what is behind it.**
    ///
    /// The same walk, with an opaque wall standing behind the boxes: its red can
    /// only reach the frame *through* a box, so a cell carrying both channels is a
    /// face that is drawn **and** see-through, one carrying only green is a face
    /// that is drawn opaque, and one carrying only red is a face that is not drawn
    /// at all. That three-way split is what a photograph of the live grid cannot
    /// give, because a brightly lit face blended over a dark background still
    /// tone-maps bright ([[viewer-translucent-top-face-reads-opaque]]).
    #[test]
    fn every_translucent_face_shows_what_is_behind_it() -> Result<(), TestError> {
        let mut all = Vec::new();
        let mut wrong = Vec::new();
        for (eye, height) in MATRIX_EYES {
            let id = format!("water-translucency-{eye}-backdrop");
            let Some((report, opaque)) = walk_matrix(&id, height, &[CellVerdict::Translucent])?
            else {
                warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
                return Ok(());
            };
            all.extend(report.iter().map(|line| format!("{eye}: {line}")));
            wrong.extend(opaque.iter().map(|line| format!("{eye}: {line}")));
        }
        assert!(
            wrong.is_empty(),
            "a half-transparent face did not show the opaque wall behind it — `solid` means it \
             drew but nothing came through, `missing` means it did not draw at all.\n  wrong:\n    \
             {}\n  whole matrix:\n    {}",
            wrong.join("\n    "),
            all.join("\n    "),
        );
        Ok(())
    }
}
