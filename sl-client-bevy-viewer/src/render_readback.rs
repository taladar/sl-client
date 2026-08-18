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

use std::sync::{Arc, Mutex};

use bevy::app::ScheduleRunnerPlugin;
use bevy::camera::{Exposure, Hdr, RenderTarget};
use bevy::light::DirectionalLightShadowMap;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{TextureFormat, TextureUsages};
use bevy::winit::WinitPlugin;

use crate::camera::ViewerCamera;
use crate::face_material::SlFaceMaterialPlugin;
use crate::probes::ReflectionProbePlugin;
use sl_client_bevy::{
    CloudMaterialPlugin, SkyMaterialPlugin, StarMaterialPlugin, SunDiscMaterialPlugin,
    TerrainMaterialPlugin, WaterMaterialPlugin,
};

use crate::render_scene::{
    RenderScene, SceneAssets, SceneCx, SceneRuntimePlugin, scene_root, scene_root_transform,
};

/// The rendered frame's size, in pixels.
///
/// Small, deliberately. Every assertion here is about *where a colour landed*,
/// which a 256² frame answers as well as a 4K one — and the frame is rendered by
/// a probe rig that re-renders the scene six times per capture, so the cost is
/// paid over and over.
const FRAME: u32 = 256;

/// How many frames to run before reading back.
///
/// Large, and it has to be. `crate::probes` amortizes its capture at **one cube
/// face per frame, in six-frame bursts**, and then Bevy filters the assembled cube
/// into the diffuse / radiance maps the PBR shader samples — so a probe's
/// environment is not merely incomplete but *empty* for a long while after the
/// scene spawns.
///
/// Measured, not guessed: at 90 frames the mirror reads pure **black** (a metallic
/// surface takes all its colour from the environment map, so an empty cube is no
/// colour at all) and the check fails for entirely the wrong reason. At 400 it
/// reflects correctly. This is the one genuinely expensive check in the suite —
/// roughly 20 s — and it is the price of asking a question about pixels.
const WARMUP_FRAMES: usize = 400;

/// The frame, read back from the GPU as linear RGBA.
#[derive(Clone, Debug)]
pub(crate) struct Frame {
    /// Row-major `Rgba8` pixels, `FRAME * FRAME * 4` bytes.
    pixels: Vec<u8>,
}

impl Frame {
    /// The pixel at `(x, y)` as linear `(r, g, b, a)` in `0..=1`, or `None` if the
    /// coordinate is outside the frame.
    pub(crate) fn pixel(&self, x: u32, y: u32) -> Option<Vec4> {
        if x >= FRAME || y >= FRAME {
            return None;
        }
        let index = usize::try_from(y)
            .ok()?
            .checked_mul(usize::try_from(FRAME).ok()?)?
            .checked_add(usize::try_from(x).ok()?)?
            .checked_mul(4)?;
        let texel = self.pixels.get(index..index.checked_add(4)?)?;
        match texel {
            [r, g, b, a] => Some(Vec4::new(
                f32::from(*r) / 255.0,
                f32::from(*g) / 255.0,
                f32::from(*b) / 255.0,
                f32::from(*a) / 255.0,
            )),
            _other => None,
        }
    }
}

/// Where a readback lands: filled by the `ReadbackComplete` observer, drained by
/// [`capture`].
///
/// A shared cell rather than a `Message`, because the readback completes in the
/// render world a frame or more after it is asked for, and the test needs to poll
/// for it rather than be handed it inside a system.
#[derive(Resource, Clone, Default)]
struct Captured(Arc<Mutex<Option<Vec<u8>>>>);

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
/// [`capture`] reads one frame after a long warm-up, [`capture_over_time`] reads
/// two at different `globals.time` values.
fn build_readback_app(scene: &RenderScene, cx: SceneCx) -> (App, Captured) {
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
            // The test harness owns the subscriber (`crate::render_test`'s
            // `capture_logs` may be installed); two would clash.
            .disable::<LogPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(core::time::Duration::ZERO));

    // `scene_root()` propagates the reflection-probe render layers down the scene
    // with `Propagate<RenderLayers>`, which needs this plugin (the full viewer adds
    // it in `lib.rs`). Without it the probe capture cameras — which render the
    // probe layers, not the main layer — would see an empty world and a mirror
    // would reflect nothing.
    app.add_plugins(bevy::app::HierarchyPropagatePlugin::<
        bevy::camera::visibility::RenderLayers,
    >::new(PostUpdate));

    // The viewer's real reflection probes, as the gallery runs them — without
    // these a mirror reflects nothing at all and the check is vacuous.
    app.add_plugins(ReflectionProbePlugin)
        // The viewer's real custom-material pipelines and its own drivers, as the
        // gallery runs them: this is the **third** app that spawns a registered
        // scene, so it is the third that renders nothing at all for a scene whose
        // shader or driver it forgot. See `SceneRuntimePlugin` — the material
        // plugins go first, and it fills in whatever they did not register.
        .add_plugins((
            SlFaceMaterialPlugin,
            TerrainMaterialPlugin,
            SkyMaterialPlugin,
            SunDiscMaterialPlugin,
            CloudMaterialPlugin,
            StarMaterialPlugin,
            WaterMaterialPlugin,
        ))
        .add_plugins(SceneRuntimePlugin)
        .insert_resource(DirectionalLightShadowMap::default())
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
            commands.spawn((
                Camera3d::default(),
                // In Bevy 0.19 the render target is its own component, not a
                // `Camera` field — the same way `crate::probes` targets its
                // capture faces.
                RenderTarget::Image(readback_target.clone().into()),
                Exposure::default(),
                Hdr,
                Transform::from_translation(position).looking_at(look_at, Vec3::Y),
                // `install_global_probe` binds the default probe to the entity
                // carrying this marker, and `drive_local_probes` poses its capture
                // rigs from it.
                ViewerCamera,
                Name::new("readback-camera"),
            ));
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
    let (mut app, captured) = build_readback_app(scene, cx);

    // `App::finish`/`cleanup` build the render app; if there is no adapter this is
    // where it gives up, and a machine without a GPU should skip rather than fail.
    app.finish();
    app.cleanup();
    for _frame in 0..WARMUP_FRAMES {
        app.update();
    }

    // Detected by **outcome**, not by inspecting the app: a frame either came back
    // off the GPU or it did not. Asking `get_sub_app(RenderApp)` looks like the
    // obvious test and is wrong — it reports `false` on a machine that renders
    // perfectly well (the sub-app is taken for the duration of the render
    // schedule), which would skip this tier everywhere and silently.
    let pixels = captured.0.lock().ok()?.take()?;

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
    Some((Frame { pixels }, projected))
}

/// Render `scene` at two different `globals.time` values and read both frames back,
/// for verifying **GPU-time-driven** animation. A texture animation now runs
/// entirely in the shader (`face_material.wgsl`'s `sl_animated_uv` from
/// `globals.time`), so it is invisible to any CPU-state digest — the only honest
/// check is that the rendered **pixels** actually differ over time. A fixed manual
/// timestep makes the two samples land at deterministic clock values regardless of
/// machine speed.
///
/// Returns `None` on a machine with no GPU adapter (like [`capture`]).
pub(crate) fn capture_over_time(scene: &RenderScene, cx: SceneCx) -> Option<(Frame, Frame)> {
    /// The manual per-update timestep: `globals.time` advances by exactly this each
    /// frame, so the two samples below are reproducible.
    const STEP: f32 = 1.0 / 30.0;
    /// Frames before the first read — enough for the scene to spawn, the animation
    /// driver to publish its params, and a readback to complete (an early cell).
    /// 400, matching `WARMUP_FRAMES`: on Mesa/RADV the async pipeline compile is
    /// slow, so a short warm-up makes the first sample read pre-render black and the
    /// two frames come back identical — a flaky failure under parallel GPU load.
    const WARMUP: usize = 400;
    /// Frames between the two reads: ~1.8 s of clock, many flipbook cells later.
    const BETWEEN: usize = 54;

    let (mut app, captured) = build_readback_app(scene, cx);
    // Deterministic time: each `update` advances the clock by exactly `STEP`
    // regardless of wall-clock, so `globals.time` (what the animation shader reads)
    // is reproducible frame-for-frame.
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        core::time::Duration::from_secs_f32(STEP),
    ));
    app.finish();
    app.cleanup();
    for _frame in 0..WARMUP {
        app.update();
    }
    let early = captured.0.lock().ok()?.take()?;
    for _frame in 0..BETWEEN {
        app.update();
    }
    let later = captured.0.lock().ok()?.take()?;
    Some((Frame { pixels: early }, Frame { pixels: later }))
}

/// Build a headless app with the **cached-static sun-shadow feature active**
/// ([`crate::shadow_visibility::ShadowVisibilityPlugin`]): an angled sun, a large
/// white ground plane at `y = 0`, and a white 2 m cube caster at each of `casters`
/// (Bevy world space, above the ground), plus a top-down render-to-texture camera
/// and a `Readback`. The caller drives the `update`s and reads the [`Captured`]
/// cell exactly as [`capture`] does.
///
/// This exists so the shadow bake — completeness of the retained static map, and
/// its correctness across a re-bake — can be checked headlessly with white cubes
/// (no grid, no complex prims); the sun's `Transform` can be mutated between reads
/// to force the projection re-bake path.
#[cfg(test)]
fn build_shadow_app(sun_dir: Vec3, casters: &[Vec3]) -> (App, Captured) {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<LogPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(core::time::Duration::ZERO))
    // The feature under test: the off-thread caster cull + cached-static split.
    .add_plugins(crate::shadow_visibility::ShadowVisibilityPlugin)
    .insert_resource(DirectionalLightShadowMap { size: 2048 })
    .init_resource::<Captured>()
    // Re-mark every caster's transform changed each frame, without moving it:
    // reproduces the server's periodic terse-`ObjectUpdate` flood, which is what
    // decayed the retained static bins in the real viewer (the "61 of 447" drop).
    // A caster whose bounds did not really change must stay in the static bake.
    .add_systems(
        Update,
        |mut casters: Query<&mut Transform, With<Mesh3d>>| {
            for mut transform in &mut casters {
                transform.set_changed();
            }
        },
    );

    let captured = app.world().resource::<Captured>().clone();

    let mut target = Image::new_target_texture(FRAME, FRAME, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(target);

    let casters = casters.to_vec();
    let readback_target = target.clone();
    app.add_systems(
        Startup,
        move |mut commands: Commands,
              mut meshes: ResMut<Assets<Mesh>>,
              mut materials: ResMut<Assets<StandardMaterial>>| {
            commands.spawn((
                DirectionalLight {
                    shadow_maps_enabled: true,
                    illuminance: 10_000.0,
                    ..default()
                },
                bevy::light::CascadeShadowConfigBuilder {
                    num_cascades: 4,
                    maximum_distance: 200.0,
                    ..default()
                }
                .build(),
                Transform::default().looking_to(sun_dir, Vec3::Y),
                // Tag so the test can find and re-aim the sun for the re-bake check.
                ShadowTestSun,
            ));
            let white = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                ..default()
            });
            let ground = meshes.add(Plane3d::default().mesh().size(400.0, 400.0));
            commands.spawn((Mesh3d(ground), MeshMaterial3d(white.clone())));
            let cube = meshes.add(Cuboid::new(2.0, 2.0, 2.0));
            for pos in &casters {
                commands.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(white.clone()),
                    Transform::from_translation(*pos),
                ));
            }
            commands.spawn((
                Camera3d::default(),
                RenderTarget::Image(readback_target.clone().into()),
                Exposure::default(),
                Hdr,
                // Low ambient so a sun shadow is crisply dark (not washed grey) —
                // the pixel checks want an unmistakable dark patch.
                AmbientLight {
                    color: Color::WHITE,
                    brightness: 8.0,
                    ..default()
                },
                // Straight down onto the ground; `up = Z` since the view axis is -Y.
                Transform::from_xyz(0.0, 80.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
                ViewerCamera,
            ));
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

/// A **faithful headless reproduction** of the interactive symptom: a raised
/// platform of static cube casters whose shadows fall on the ground below, viewed
/// by a camera the test **pans** each step. Unlike [`build_shadow_app`] there is
/// **no per-frame `Changed` flood** — the casters are genuinely still, exactly
/// like an in-world sky platform that has finished rezzing — so the cached-static
/// bake must keep every caster's shadow across the camera-motion-driven re-bakes
/// with the *incremental* (retained) bins, not because every caster is re-queued
/// every frame. The camera starts angled at the platform; the test moves it via
/// the [`ViewerCamera`] transform.
/// A small integer avalanche (SplitMix32-style) for the deterministic background
/// scatter in [`build_shadow_platform_app`] — turns a counter into well-spread
/// pseudo-random bits without an RNG dependency, and stable across runs.
#[cfg(test)]
fn mix_u32(x: u32) -> u32 {
    let mut z = x.wrapping_add(0x9E37_79B9);
    z = (z ^ (z >> 16)).wrapping_mul(0x21F0_AAAD);
    z = (z ^ (z >> 15)).wrapping_mul(0x735A_2D97);
    z ^ (z >> 15)
}

#[cfg(test)]
fn build_shadow_platform_app(
    sun_dir: Vec3,
    casters: &[Vec3],
    no_indirect: bool,
    background: u32,
) -> (App, Captured) {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .disable::<WinitPlugin>()
            .disable::<LogPlugin>(),
    )
    .add_plugins(ScheduleRunnerPlugin::run_loop(core::time::Duration::ZERO))
    .add_plugins(crate::shadow_visibility::ShadowVisibilityPlugin)
    .insert_resource(DirectionalLightShadowMap { size: 2048 })
    .init_resource::<Captured>();

    let captured = app.world().resource::<Captured>().clone();

    let mut target = Image::new_target_texture(FRAME, FRAME, TextureFormat::Rgba8UnormSrgb, None);
    target.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(target);

    let casters = casters.to_vec();
    let readback_target = target.clone();
    app.add_systems(
        Startup,
        move |mut commands: Commands,
              mut meshes: ResMut<Assets<Mesh>>,
              mut materials: ResMut<Assets<StandardMaterial>>| {
            commands.spawn((
                DirectionalLight {
                    shadow_maps_enabled: true,
                    illuminance: 10_000.0,
                    ..default()
                },
                bevy::light::CascadeShadowConfigBuilder {
                    num_cascades: 4,
                    maximum_distance: 300.0,
                    ..default()
                }
                .build(),
                Transform::default().looking_to(sun_dir, Vec3::Y),
                ShadowTestSun,
            ));
            let white = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                ..default()
            });
            let ground = meshes.add(Plane3d::default().mesh().size(600.0, 600.0));
            commands.spawn((Mesh3d(ground), MeshMaterial3d(white.clone())));
            let cube = meshes.add(Cuboid::new(2.0, 2.0, 2.0));
            for pos in &casters {
                commands.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(white.clone()),
                    Transform::from_translation(*pos),
                ));
            }
            // Scatter `background` extra static casters across a wide area at mixed
            // heights, well away from the measured platform's shadow discs. These
            // do not get sampled; they exist only to fill the *shared* static shadow
            // phase to a region-like scale (thousands of retained bins across all
            // four cascades), so the incremental queue / binning / batching is
            // stressed the way it is in-world — the factor the 16-prim scene lacks.
            // A deterministic pseudo-random scatter (no RNG dependency, stable runs).
            for k in 0..background {
                let h = mix_u32(k);
                // Spread over roughly [-260, 260] in X/Z, skipping the central
                // region the platform + its shadows occupy.
                let ux = f32::from(u16::try_from(h & 0xFFFF).unwrap_or(0)) / 65_535.0;
                let uz = f32::from(u16::try_from((h >> 16) & 0xFFFF).unwrap_or(0)) / 65_535.0;
                let x = -260.0 + ux * 520.0;
                let z = -260.0 + uz * 520.0;
                if x.abs() < 45.0 && z.abs() < 45.0 {
                    continue;
                }
                let y = 3.0 + f32::from(u16::try_from((h >> 8) & 0x3F).unwrap_or(0));
                commands.spawn((
                    Mesh3d(cube.clone()),
                    MeshMaterial3d(white.clone()),
                    Transform::from_translation(Vec3::new(x, y, z)),
                ));
            }
            let camera = commands
                .spawn((
                    Camera3d::default(),
                    RenderTarget::Image(readback_target.clone().into()),
                    Exposure::default(),
                    Hdr,
                    AmbientLight {
                        color: Color::WHITE,
                        brightness: 8.0,
                        ..default()
                    },
                    // Angled high view of the platform, looking at the origin. The
                    // test pans this in X to drive the static-cascade re-bakes.
                    Transform::from_xyz(0.0, 140.0, 60.0).looking_at(Vec3::ZERO, Vec3::Y),
                    ViewerCamera,
                ))
                .id();
            // Forcing the view onto the non-indirect (CPU direct) draw path makes
            // `prepare_lights` resolve `gpu_preprocessing_mode = None`, which is the
            // path many GPUs (and the interactive session under test) actually use —
            // and the one on which the *retained* static bins must still render
            // completely without a per-frame re-queue.
            if no_indirect {
                commands
                    .entity(camera)
                    .insert(bevy::render::view::NoIndirectDrawing);
            }
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

/// Marks the test sun so a re-bake check can re-aim it. Test-only.
#[cfg(test)]
#[derive(Component)]
struct ShadowTestSun;

/// The ground-plane landing point of the shadow cast by a caster at `caster`
/// under sun direction `sun_dir` (both Bevy world space): where `caster` projects
/// along the sun ray onto `y = 0`.
#[cfg(test)]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "test-only shadow projection over bounded scene coordinates; the sun \
              direction is a fixed non-horizontal test vector so d.y is never zero"
)]
fn shadow_center(caster: Vec3, sun_dir: Vec3) -> Vec3 {
    let d = sun_dir.normalize();
    caster - (caster.y / d.y) * d
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::print_stderr,
        reason = "these readback tests print pixel/luma diagnostics to stderr so a \
                  failure (or a skipped no-GPU run) is legible in the test log"
    )]
    #![expect(
        clippy::expect_used,
        clippy::panic,
        reason = "a failed expectation / panic is the intended failure signal in a \
                  readback unit test"
    )]

    use super::{
        Captured, FRAME, Frame, ShadowTestSun, WARMUP_FRAMES, build_readback_app, build_shadow_app,
        build_shadow_platform_app, capture, capture_over_time, shadow_center,
    };
    use crate::render_scene::{SCENES, SceneCx};
    use crate::render_test::TestError;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// How saturated a pixel must be to count as "one of the coloured
    /// neighbours" rather than the grey backdrop or a specular highlight.
    ///
    /// The neighbours are deliberately near-primary (0.9 in one channel, 0.1 in
    /// the others), so a real reflection of one is unambiguous. The threshold only
    /// has to exclude grey — it is nowhere near having to *discriminate* between
    /// the four, which the dominant-channel test below does.
    const SATURATION: f32 = 0.06;

    /// Which channel dominates a pixel, if any does by [`SATURATION`].
    ///
    /// Returns the neighbour's name, so a failure says "the red one" rather than
    /// quoting a float triple nobody can picture.
    fn dominant(pixel: Vec4) -> Option<&'static str> {
        let (r, g, b) = (pixel.x, pixel.y, pixel.z);
        // Yellow is red+green, so it must be tested before either of them.
        if r > b + SATURATION && g > b + SATURATION && (r - g).abs() < SATURATION {
            return Some("yellow");
        }
        if r > g + SATURATION && r > b + SATURATION {
            return Some("red");
        }
        if g > r + SATURATION && g > b + SATURATION {
            return Some("green");
        }
        if b > r + SATURATION && b > g + SATURATION {
            return Some("blue");
        }
        None
    }

    /// The centroid, in pixels, of the pixels **inside the mirror's disc** whose
    /// dominant channel is `colour`.
    ///
    /// Restricted to the disc, and that restriction is the whole check. The
    /// coloured prims are *directly visible* in the frame as well as reflected, and
    /// a centroid over the whole frame is dominated by the prim itself — which does
    /// not move when the probe is wrong. The first version of this test did exactly
    /// that and passed happily with R22i reintroduced: it was measuring the cubes,
    /// not the mirror.
    fn centroid_in_disc(frame: &Frame, centre: Vec2, radius: f32, colour: &str) -> Option<Vec2> {
        let (mut sum, mut count) = (Vec2::ZERO, 0.0_f32);
        for y in 0..FRAME {
            for x in 0..FRAME {
                let point = Vec2::new(
                    f32::from(u16::try_from(x).unwrap_or(0)),
                    f32::from(u16::try_from(y).unwrap_or(0)),
                );
                let offset = Vec2::new(point.x - centre.x, point.y - centre.y);
                if offset.length() > radius {
                    continue;
                }
                let Some(pixel) = frame.pixel(x, y) else {
                    continue;
                };
                if dominant(pixel) == Some(colour) {
                    sum = Vec2::new(sum.x + point.x, sum.y + point.y);
                    count += 1.0;
                }
            }
        }
        if count < 4.0 {
            return None;
        }
        Some(Vec2::new(sum.x / count, sum.y / count))
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
        let found: Vec<(&str, Vec2)> = ["red", "green", "yellow"]
            .into_iter()
            .filter_map(|colour| {
                centroid_in_disc(&frame, centre, radius, colour).map(|at| (colour, at))
            })
            .collect();
        assert_eq!(
            found.len(),
            3_usize,
            "the red, green and yellow neighbours must each appear *in the mirror*; found {:?} \
             — if one is missing the mirror is not reflecting it at all, and every comparison \
             below would pass by looking at nothing",
            found.iter().map(|(colour, _)| *colour).collect::<Vec<_>>()
        );
        let at = |colour: &str| -> Vec2 {
            found
                .iter()
                .find(|(name, _)| *name == colour)
                .map_or(Vec2::ZERO, |(_, at)| *at)
        };
        let (red, green, yellow) = (at("red"), at("green"), at("yellow"));

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
        // difference is the animation. Require a substantial change (thousands of
        // bytes) so a stray texel could never pass it — a real cell change repaints a
        // large part of the face.
        let differing = early
            .pixels
            .iter()
            .zip(&later.pixels)
            .filter(|(before, after)| before != after)
            .count();
        assert!(
            differing > 1000,
            "the flipbook rendered near-identically at two different times ({differing} of {} \
             bytes differ) — its GPU texture animation did not change the frame, so the shader \
             is not animating what is on screen (or the prim did not render at all)",
            early.pixels.len(),
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
            !frame.pixels.is_empty(),
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
        let (mut app, captured) = build_readback_app(scene, SceneCx::new());
        app.finish();
        app.cleanup();
        for _frame in 0..WARMUP_FRAMES {
            app.update();
        }
        // No GPU adapter (no frame came back): skip, like the rest of this tier.
        if captured
            .0
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
            .is_none()
        {
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
        for _frame in 0..3 {
            app.update();
        }
        // Reaching here without an abort means the runtime re-prepare survived.
        Ok(())
    }

    // --- cached-static sun-shadow bake checks (white cubes, no grid) ---

    use crate::camera::ViewerCamera;

    /// Drive `app` for `frames` updates, then take the most recent read-back frame,
    /// or `None` if no GPU adapter produced one (skip, exactly as [`capture`]).
    fn drive(app: &mut App, captured: &Captured, frames: usize) -> Option<Frame> {
        for _frame in 0..frames {
            app.update();
        }
        let pixels = captured.0.lock().ok()?.take()?;
        Some(Frame { pixels })
    }

    /// Project Bevy-world `points` to frame pixels through the render camera.
    fn project(app: &mut App, points: &[Vec3]) -> Vec<Option<Vec2>> {
        let mut cameras = app
            .world_mut()
            .query_filtered::<(&Camera, &GlobalTransform), With<ViewerCamera>>();
        let Ok((camera, transform)) = cameras.single(app.world()) else {
            return vec![None; points.len()];
        };
        points
            .iter()
            .map(|point| camera.world_to_viewport(transform, *point).ok())
            .collect()
    }

    /// Luma of a colour (Rec. 709).
    fn luma(colour: Vec4) -> f32 {
        0.2126 * colour.x + 0.7152 * colour.y + 0.0722 * colour.z
    }

    /// Mean luma of the frame pixels within `radius` of `centre`, or `None` if
    /// empty. Used for the *lit* reference, which is a uniform bright patch.
    fn luma_in_disc(frame: &Frame, centre: Vec2, radius: f32) -> Option<f32> {
        let (mut sum, mut count) = (0.0_f32, 0.0_f32);
        for y in 0..FRAME {
            for x in 0..FRAME {
                let px = Vec2::new(
                    f32::from(u16::try_from(x).unwrap_or(0)),
                    f32::from(u16::try_from(y).unwrap_or(0)),
                );
                if Vec2::new(px.x - centre.x, px.y - centre.y).length() > radius {
                    continue;
                }
                if let Some(colour) = frame.pixel(x, y) {
                    sum += luma(colour);
                    count += 1.0;
                }
            }
        }
        (count > 0.0).then_some(sum / count)
    }

    /// The **darkest** luma within `radius` of `centre`. Robust for detecting a
    /// shadow patch even when a bright caster is nearby: the shadow's darkest pixel
    /// is unmistakable, and averaging would be pulled up by the white cube. `1.0`
    /// if the disc is empty.
    fn min_luma_near(frame: &Frame, centre: Vec2, radius: f32) -> f32 {
        let mut min = 1.0_f32;
        for y in 0..FRAME {
            for x in 0..FRAME {
                let px = Vec2::new(
                    f32::from(u16::try_from(x).unwrap_or(0)),
                    f32::from(u16::try_from(y).unwrap_or(0)),
                );
                if Vec2::new(px.x - centre.x, px.y - centre.y).length() > radius {
                    continue;
                }
                if let Some(colour) = frame.pixel(x, y) {
                    min = min.min(luma(colour));
                }
            }
        }
        min
    }

    /// Assert every caster's shadow patch is present (clearly darker than a lit
    /// reference) — the completeness check. `lit` is a ground point away from every
    /// shadow.
    fn assert_all_casters_shadowed(app: &mut App, frame: &Frame, casters: &[Vec3], sun: Vec3) {
        // A ground reference off to the side of every cube and shadow.
        let lit_world = Vec3::new(0.0, 0.0, 22.0);
        let mut points: Vec<Vec3> = casters.iter().map(|c| shadow_center(*c, sun)).collect();
        points.push(lit_world);
        let projected = project(app, &points);

        let lit_px = projected
            .last()
            .and_then(|p| *p)
            .expect("lit reference projected off-frame");
        let lit = luma_in_disc(frame, lit_px, 5.0).expect("lit disc empty");
        eprintln!("lit reference at world {lit_world:?} px {lit_px:?} luma {lit}");
        assert!(lit > 0.25, "lit ground should be bright, got luma {lit}");

        // Collect every caster's darkest nearby pixel first (print them all), then
        // assert — so a dropped caster is reported alongside the ones that survived.
        let shadows: Vec<(usize, Vec3, f32)> = casters
            .iter()
            .enumerate()
            .map(|(index, caster)| {
                let shadow_px = projected
                    .get(index)
                    .and_then(|p| *p)
                    .unwrap_or_else(|| panic!("caster {index} at {caster:?} shadow off-frame"));
                let shadow = min_luma_near(frame, shadow_px, 14.0);
                eprintln!("caster {index} at {caster:?} shadow px {shadow_px:?} min-luma {shadow}");
                (index, *caster, shadow)
            })
            .collect();
        for (index, caster, shadow) in shadows {
            assert!(
                shadow < lit * 0.6,
                "caster {index} at {caster:?}: shadow (min luma {shadow}) missing/too light vs lit {lit}"
            );
        }
    }

    /// Re-aim the tagged test sun (triggers the static-projection re-bake path).
    fn aim_sun(app: &mut App, sun_dir: Vec3) {
        let mut suns = app
            .world_mut()
            .query_filtered::<&mut Transform, With<ShadowTestSun>>();
        for mut transform in suns.iter_mut(app.world_mut()) {
            *transform = Transform::default().looking_to(sun_dir, Vec3::Y);
        }
    }

    /// Move the [`ViewerCamera`] to `eye`, looking at the origin — the pan that
    /// drives the static-cascade re-bakes in the platform reproduction.
    fn pan_camera(app: &mut App, eye: Vec3) {
        let mut cameras = app
            .world_mut()
            .query_filtered::<&mut Transform, With<ViewerCamera>>();
        for mut transform in cameras.iter_mut(app.world_mut()) {
            *transform = Transform::from_translation(eye).looking_at(Vec3::ZERO, Vec3::Y);
        }
    }

    /// For each caster, classify its ground shadow in `frame`: `'X'` present
    /// (clearly darker than the lit reference), `'.'` missing (ground is lit where
    /// its shadow should be), or `'o'` off-frame (its shadow projected outside the
    /// image — a framing artifact, not the bug). Returns the lit reference luma and
    /// the per-caster marks.
    fn platform_shadow_report(
        app: &mut App,
        frame: &Frame,
        casters: &[Vec3],
        sun: Vec3,
    ) -> (f32, Vec<char>) {
        let frame_max = f32::from(u16::try_from(FRAME).unwrap_or(0));
        // A ground point well away from every cube and every shadow.
        let lit_world = Vec3::new(0.0, 0.0, -30.0);
        let mut points: Vec<Vec3> = casters.iter().map(|c| shadow_center(*c, sun)).collect();
        points.push(lit_world);
        let projected = project(app, &points);
        let lit = projected
            .last()
            .and_then(|p| *p)
            .and_then(|px| luma_in_disc(frame, px, 4.0))
            .unwrap_or(1.0);
        let marks = casters
            .iter()
            .enumerate()
            .map(|(index, _)| match projected.get(index).and_then(|p| *p) {
                Some(px) if px.x >= 0.0 && px.y >= 0.0 && px.x < frame_max && px.y < frame_max => {
                    if min_luma_near(frame, px, 3.0) < lit * 0.55 {
                        'X'
                    } else {
                        '.'
                    }
                }
                _ => 'o',
            })
            .collect();
        (lit, marks)
    }

    /// **Reproduction of the interactive symptom.** A static 4×4 platform of cube
    /// casters (16 prims that never move) above the ground; the camera pans across
    /// it. Each step, record which casters still cast a ground shadow. Individual
    /// casters must not blink out of the cached-static bake as the camera moves —
    /// the failure the user sees interactively (a rotating subset of platform prims
    /// losing their shadow for a whole bake period). Prints a per-step presence
    /// matrix so the pattern is legible when it fails.
    #[test]
    fn platform_shadows_under_camera_motion() {
        // Run the reproduction on both draw paths: the indirect/GPU-culling path
        // (what this test GPU likely uses) and the CPU direct path (what the
        // interactive session likely uses). Printing both matrices localizes the
        // failure to a path rather than guessing.
        for no_indirect in [false, true] {
            let path = if no_indirect { "direct" } else { "indirect" };
            eprintln!("=== platform shadow reproduction: {path} draw path ===");
            let lost = run_platform_shadow_motion(no_indirect);
            assert!(
                lost.is_empty(),
                "[{path} path] casters lost their ground shadow during camera motion \
                 (x, caster indices): {lost:?}"
            );
        }
    }

    /// Drive the platform-under-camera-motion reproduction on one draw path and
    /// return the `(camera_x, caster indices)` where a shadow was lost. Prints a
    /// per-step presence matrix.
    fn run_platform_shadow_motion(no_indirect: bool) -> Vec<(f32, Vec<usize>)> {
        const COORDS: [f32; 4] = [-18.0, -6.0, 6.0, 18.0];
        let base_sun = Vec3::new(0.35, -1.0, 0.15);
        let mut casters = Vec::new();
        for &x in &COORDS {
            for &z in &COORDS {
                casters.push(Vec3::new(x, 25.0, z));
            }
        }
        // Fill the shared static shadow phase to a region-like scale (≈1500 extra
        // retained casters across all cascades) so the incremental binning is
        // stressed like the live grid — the 16 platform prims are the ones measured.
        let (mut app, captured) = build_shadow_platform_app(base_sun, &casters, no_indirect, 1500);
        app.finish();
        app.cleanup();

        // Warm up at the start pose so the initial bake settles.
        let Some(_frame) = drive(&mut app, &captured, WARMUP_FRAMES) else {
            eprintln!("no GPU adapter; skipping platform shadow readback");
            return Vec::new();
        };

        // Each step: pan the camera AND nudge the sun (a slow day cycle). The sun
        // nudge forces a static-projection re-bake every step — otherwise the huge
        // cascade margin means a modest pan never invalidates the retained bake and
        // the test is vacuous. A few frames per step let the re-bake settle before
        // the read-back.
        let mut lost: Vec<(f32, Vec<usize>)> = Vec::new();
        for i in -6i8..=6i8 {
            let cx = f32::from(i) * 4.0;
            let sun = Vec3::new(0.35 + f32::from(i) * 0.03, -1.0, 0.15);
            aim_sun(&mut app, sun);
            pan_camera(&mut app, Vec3::new(cx, 140.0, 60.0));
            // Settle the re-bake, then sample several consecutive frames: a caster
            // that blinks out for part of a bake period is flagged if it is missing
            // in ANY sampled frame (marks accumulate to the worst case per caster).
            if drive(&mut app, &captured, 6).is_none() {
                continue;
            }
            let mut worst: Vec<char> = vec!['X'; casters.len()];
            let mut lit_seen = 0.0_f32;
            for _sample in 0..8 {
                let Some(frame) = drive(&mut app, &captured, 1) else {
                    continue;
                };
                let (lit, marks) = platform_shadow_report(&mut app, &frame, &casters, sun);
                lit_seen = lit;
                for (slot, &mark) in worst.iter_mut().zip(&marks) {
                    // Precedence: missing ('.') is worst, then off-frame ('o').
                    if mark == '.' || (mark == 'o' && *slot == 'X') {
                        *slot = mark;
                    }
                }
            }
            let row: String = worst.iter().collect();
            eprintln!(
                "camera x={cx:>6.1} sun_x={:.2} lit={lit_seen:.3}  {row}",
                sun.x
            );
            let gone: Vec<usize> = worst
                .iter()
                .enumerate()
                .filter_map(|(index, &mark)| (mark == '.').then_some(index))
                .collect();
            if !gone.is_empty() {
                lost.push((cx, gone));
            }
        }
        lost
    }

    /// A settled static caster casts a shadow on the ground with the cached-static
    /// feature active — the baseline the completeness/retention checks rely on.
    #[test]
    fn a_static_caster_casts_a_shadow() {
        let sun = Vec3::new(0.4, -1.0, 0.2);
        let caster = Vec3::new(0.0, 6.0, 0.0);
        let (mut app, captured) = build_shadow_app(sun, &[caster]);
        app.finish();
        app.cleanup();
        let Some(frame) = drive(&mut app, &captured, WARMUP_FRAMES) else {
            eprintln!("no GPU adapter; skipping cached-static shadow readback");
            return;
        };
        assert_all_casters_shadowed(&mut app, &frame, &[caster], sun);
    }

    /// **Every static caster keeps its shadow across a re-bake** — the
    /// completeness/retention check the bake pipeline exists to satisfy (the
    /// "61 of 447" and blink-on-rebake failure modes). Baked once with the casters
    /// settled, then the sun is re-aimed to force the static-projection re-bake, and
    /// every caster must still be shadowed at its new position — none dropped from
    /// the rebuilt bake.
    #[test]
    fn static_casters_stay_shadowed_across_a_rebake() {
        let sun1 = Vec3::new(0.4, -1.0, 0.2);
        let casters = [
            Vec3::new(0.0, 6.0, -8.0),
            Vec3::new(0.0, 6.0, 0.0),
            Vec3::new(0.0, 6.0, 8.0),
        ];
        let (mut app, captured) = build_shadow_app(sun1, &casters);
        app.finish();
        app.cleanup();
        let Some(before) = drive(&mut app, &captured, WARMUP_FRAMES) else {
            eprintln!("no GPU adapter; skipping cached-static shadow readback");
            return;
        };
        assert_all_casters_shadowed(&mut app, &before, &casters, sun1);

        // Re-aim the sun: the caster set is unchanged, only the projection — the
        // frequent "day cycle / camera move" re-bake path.
        let sun2 = Vec3::new(-0.4, -1.0, 0.2);
        aim_sun(&mut app, sun2);
        let Some(after) = drive(&mut app, &captured, 150) else {
            return;
        };
        assert_all_casters_shadowed(&mut app, &after, &casters, sun2);
    }
}
