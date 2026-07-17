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

use crate::camera::FlyCamera;
use crate::probes::ReflectionProbePlugin;
use crate::render_scene::{RenderScene, SceneAssets, SceneCx, scene_root, scene_root_transform};

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

    // The viewer's real reflection probes, as the gallery runs them — without
    // these a mirror reflects nothing at all and the check is vacuous.
    app.add_plugins(ReflectionProbePlugin)
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
        move |mut commands: Commands,
              mut meshes: ResMut<Assets<Mesh>>,
              mut materials: ResMut<Assets<StandardMaterial>>,
              mut images: ResMut<Assets<Image>>,
              mut inverse_bindposes: ResMut<
            Assets<bevy::mesh::skinning::SkinnedMeshInverseBindposes>,
        >| {
            let root = commands.spawn(scene_root()).id();
            let mut assets = SceneAssets {
                meshes: &mut meshes,
                materials: &mut materials,
                images: &mut images,
                inverse_bindposes: &mut inverse_bindposes,
            };
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
                FlyCamera::default(),
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
        .query_filtered::<(&Camera, &GlobalTransform), With<FlyCamera>>();
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

#[cfg(test)]
mod tests {
    use super::{FRAME, Frame, capture};
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
}
