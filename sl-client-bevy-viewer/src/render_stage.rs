//! The render **context matrix**'s stage (`viewer-render-context-matrix`): what
//! a cell of the matrix stands a registered subject among, how the cell is
//! captured, and what the subject must look like there.
//!
//! A cell is a subject and a [`ContextSet`]. [`stage`] turns the pair into a
//! placement — the subject centred on the waterline when there is an eye, a
//! camera framing it and whatever was staged beside it, a mirror, an actor, a
//! calibration plate, a sky, a sea — written once for every subject, so a new
//! subject is captured under every context its `applies` lists without a line
//! of per-scene staging. [`capture_cell`] renders the cell twice in one app, with
//! the subject and then with it hidden, so a subject is read by what it changed —
//! and a marked one's points and reflection by its colour. [`expectation`] is the rule table that
//! says what each cell must read — `Hidden` behind an opaque actor, `SeeThrough`
//! behind a translucent one or in front of a wall when it is itself
//! translucent, `Visible` otherwise — and a cell is never given a verdict by a
//! per-cell table.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;

use crate::environment::EnvironmentState;
use crate::pixel_oracle::{CellVerdict, Frame, Marker, Silhouette, luminance, read_cell};
use crate::render_readback::{
    CAPTURE_AT_SECS, FRAME, Projected, SettleError, SubjectRoot, build_posed_app, check_logs,
    frame_at, gpu_lock, settle,
};
use crate::render_scene::{
    ActorKind, ContextSet, Eye, Layering, RenderScene, SCENE_WATER_LEVEL, SceneAssets, SceneCamera,
    SceneCx, SubjectMarker, SubjectPoint, SubjectSignature, TimeOfDay, Toggle, Viewpoint,
    scene_root, scene_root_transform, spawn_stage_actor, spawn_stage_mirror, spawn_stage_plate,
    spawn_stage_sea, spawn_stage_sky,
};
use crate::render_test::{advance_to, capture_logs, scene_geometry, spawn_scene};
use crate::underwater_fog::UnderwaterFogPlugin;
use crate::viewer_camera::viewer_projection;
use crate::world_api::ViewerCamera;

/// The least fraction of its own silhouette a subject must paint — or, read by
/// difference, change. A convex solid fills most of its projected disc; 0.15
/// tolerates sparse subjects and still catches "drew nothing".
const MIN_COVERAGE: f32 = 0.15;

/// Per-scene minimum coverage below [`MIN_COVERAGE`], for subjects that are
/// honestly sparse inside their own bounds — with the reason. A thin ribbon
/// waving through its swept volume paints little of the box that holds it, and
/// so does a body standing in the T-pose its meshes are authored in.
///
/// Each floor is set from a measurement with headroom, not from the number the
/// scene happens to print: the point is still to catch "drew nothing" and
/// "drew half the subject", so a floor that sits just under today's reading
/// would be a flake and one at zero would be decoration.
pub(crate) const SPARSE: &[(&str, f32, &str)] = &[
    (
        "flexi-streamer",
        0.02,
        "a thin ribbon inside the box swept by its own waving",
    ),
    (
        "avatar-base-part",
        // Measured 0.125 against the vendored Linden body.
        0.08,
        "the rest body stands in the T-pose its `.llm` meshes are authored in, so its \
         1.7 m arm span boxes a thin cross of geometry and most of the disc that span \
         defines is air — `avatar-morphed-body` reads higher only because the shaped \
         appearance flares its skirt out to fill some of that box",
    ),
    (
        "translucent-box-rack",
        // Measured 0.139.
        0.08,
        "five boxes spaced along a row 18 m long: the disc their bounds define is \
         mostly the sky between and above them",
    ),
];

/// The coverage floor for scene `id`: its [`SPARSE`] entry, or [`MIN_COVERAGE`].
pub(crate) fn coverage_floor(id: &str) -> f32 {
    SPARSE
        .iter()
        .find(|(sparse, _minimum, _reason)| *sparse == id)
        .map_or(MIN_COVERAGE, |(_sparse, minimum, _reason)| *minimum)
}

/// How close, in metres along a face's normal, the eye may come to the face's
/// plane before a point on it stops being asserted.
///
/// A rack cap is horizontal, so an eye level with it sees it exactly edge-on: it
/// covers no pixels worth sampling and the projected point lands on whatever is
/// behind. The grazing eye is level with *some* cap by construction — that is
/// what grazing means — so those cells are reported and not asserted, which is
/// honest about what the fixture can see.
const EDGE_ON_MARGIN: f32 = 1.0;

/// How steep the view to a point must be — rise over distance — before a point
/// on the **far side of the water surface from the eye** is asserted.
///
/// Such a point is seen *through* the surface, so what reaches the frame is a
/// refracted sample, displaced by the wave normal where the ray crosses. The
/// displacement grows without bound as the view flattens, and the waves animate
/// from `globals.time`, so a shallow cell lands somewhere slightly different
/// every run: `grazing: sunk below` came back `solid` standalone and
/// `background` under a loaded parallel run. Below this slope such a point is
/// reported and not asserted; above it the displacement is small against the
/// band and the sample is stable.
const REFRACTED_MIN_SLOPE: f32 = 0.06;

/// The least number of mirror pixels the subject's reflection must account for.
/// A reflection on a convex mirror is small — a few dozen pixels for a subject
/// beside a sphere in a 256² frame — so this asks "is it there at all", which a
/// mirror that reflects nothing fails by a wide margin.
const MIN_REFLECTED_PIXELS: u32 = 12;

/// The fraction of a mirror's projected radius that is read: the rim is a
/// grazing-angle smear of everything at once.
const MIRROR_READ_FRACTION: f32 = 0.85;

/// Margin added around everything a widened camera has to fit, as a fraction.
const FRAMING_MARGIN: f32 = 1.15;

// ---------------------------------------------------------------------------
// Vector arithmetic. The workspace's `arithmetic_side_effects` lint bans the
// overloaded operators on `Vec3`, so the stage's geometry goes through these.
// ---------------------------------------------------------------------------

/// `a + b`.
const fn add(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x + b.x, a.y + b.y, a.z + b.z)
}

/// `a - b`.
const fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// `a * k`.
const fn scale(a: Vec3, k: f32) -> Vec3 {
    Vec3::new(a.x * k, a.y * k, a.z * k)
}

/// The midpoint of `a` and `b`.
fn midpoint(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        f32::midpoint(a.x, b.x),
        f32::midpoint(a.y, b.y),
        f32::midpoint(a.z, b.z),
    )
}

// ---------------------------------------------------------------------------
// Measuring the subject.
// ---------------------------------------------------------------------------

/// The scene's bounds — the box holding every vertex — in **Second Life**
/// metres, measured from the geometry tier's CPU tessellation of the same scene
/// at the same scene time the pixel tier renders it (its last timeline sample,
/// at least the static capture time), so a fountain's bounds are the cloud on
/// the frame and not the lone seed particle of its first instant. `None` when
/// the scene puts no vertex anywhere (which the geometry tier's own sweep
/// already fails).
pub(crate) fn measured_bounds(scene: &RenderScene) -> Option<(Vec3, Vec3)> {
    let mut app = spawn_scene(SceneCx::new(), scene);
    advance_to(&mut app, capture_time(scene));
    let to_sl = scene_root_transform().rotation.inverse();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for geometry in scene_geometry(&mut app) {
        for position in &geometry.positions {
            let world = geometry.world.transform_point3(*position);
            if !world.is_finite() {
                continue;
            }
            let sl = to_sl.mul_vec3(world);
            min = min.min(sl);
            max = max.max(sl);
            any = true;
        }
    }
    any.then_some((min, max))
}

/// The scene time a scene is captured at: its last timeline sample, and at
/// least [`CAPTURE_AT_SECS`].
fn capture_time(scene: &RenderScene) -> f32 {
    scene
        .timeline
        .samples
        .last()
        .copied()
        .unwrap_or(0.0)
        .max(CAPTURE_AT_SECS)
}

// ---------------------------------------------------------------------------
// Staging a cell.
// ---------------------------------------------------------------------------

/// The `Viewpoint::InMirror` stage's mirror, in Second Life metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MirrorStage {
    /// The sphere's centre.
    pub(crate) centre: Vec3,
    /// The sphere's diameter.
    pub(crate) diameter: f32,
    /// The probe volume's diameter, which contains the subject.
    pub(crate) volume: f32,
}

/// The `Layering` stage's actor, in Second Life metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ActorStage {
    /// Opaque or half transparent.
    pub(crate) kind: ActorKind,
    /// The wall's centre.
    pub(crate) centre: Vec3,
    /// Its extents before `rotation`: thin along local `y`.
    pub(crate) size: Vec3,
    /// What turns local `y` toward the eye.
    pub(crate) rotation: Quat,
}

/// The `Time` stage's calibration plate, in Second Life metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlateStage {
    /// The plate's centre.
    pub(crate) centre: Vec3,
    /// Its edge length.
    pub(crate) size: f32,
    /// Its orientation: its face turned halfway from the eye to the zenith.
    pub(crate) rotation: Quat,
}

/// One cell's placement: everything [`stage`] decided, in Second Life metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Staged {
    /// Where the camera stands and what it aims at.
    pub(crate) camera: SceneCamera,
    /// How far the subject's root is moved.
    pub(crate) offset: Vec3,
    /// The subject's bounds, after the offset.
    pub(crate) bounds: (Vec3, Vec3),
    /// The mirror, if the cell is seen in one.
    pub(crate) mirror: Option<MirrorStage>,
    /// The actor, if the cell layers one.
    pub(crate) actor: Option<ActorStage>,
    /// The calibration plate, if the cell has a time of day.
    pub(crate) plate: Option<PlateStage>,
    /// Whether the sea is staged.
    pub(crate) sea: bool,
    /// The sky's time of day, if one is staged.
    pub(crate) sky: Option<TimeOfDay>,
    /// Whether the underwater fog pass runs.
    pub(crate) fog: bool,
}

impl Staged {
    /// The subject's centre, after the offset.
    pub(crate) fn centre(&self) -> Vec3 {
        midpoint(self.bounds.0, self.bounds.1)
    }

    /// The radius of the sphere that holds the subject's bounds.
    pub(crate) fn radius(&self) -> f32 {
        scale(sub(self.bounds.1, self.bounds.0), 0.5)
            .length()
            .max(0.25)
    }
}

/// Place a cell: stand the subject (whose measured bounds are `bounds`) among
/// `cx`'s contexts and frame it.
///
/// - **Eye**: the subject's centre goes on the waterline, the sea is staged,
///   and the camera keeps its declared distance and aim but stands at the eye's
///   height over the water — for the translucent box rack, exactly the
///   translucency matrix's camera.
/// - **Time**: the real sky stack at that anchor, and a matte white plate beside
///   the subject (camera-left) as the luminance reference.
/// - **Viewpoint::InMirror**: a mirror sphere beside it (camera-right, a little
///   toward the eye), inside a probe volume that contains the subject.
/// - **Layering**: a red wall facing the eye, either 40% of the way from the
///   subject to the eye and large enough to cover it, or a metre beyond it and
///   large enough to fill everything around it.
///
/// A plate or a mirror widens the camera, along its declared direction, until
/// everything staged fits the frame.
pub(crate) fn stage(scene: &RenderScene, cx: SceneCx, bounds: (Vec3, Vec3)) -> Staged {
    let contexts = cx.contexts;
    let local_centre = midpoint(bounds.0, bounds.1);
    let offset = if contexts.eye.is_some() {
        Vec3::new(0.0, 0.0, SCENE_WATER_LEVEL - local_centre.z)
    } else {
        Vec3::ZERO
    };
    let mut staged = Staged {
        camera: SceneCamera {
            position: add(scene.camera.position, offset),
            look_at: add(scene.camera.look_at, offset),
        },
        offset,
        bounds: (add(bounds.0, offset), add(bounds.1, offset)),
        mirror: None,
        actor: None,
        plate: None,
        sea: contexts.eye.is_some(),
        sky: contexts.time,
        fog: contexts.toggle == Some(Toggle::UnderwaterFog),
    };
    let centre = staged.centre();
    let radius = staged.radius();
    let (forward, right) = horizontal_basis(staged.camera);

    if contexts.viewpoint == Some(Viewpoint::InMirror) {
        let diameter = (radius * 1.2).clamp(1.0, 6.0);
        let mirror_centre = add(
            centre,
            add(
                scale(right, radius + diameter * 0.5 + 0.5),
                scale(forward, -radius * 0.5),
            ),
        );
        let reach = mirror_centre.distance(centre) + radius + 1.0;
        staged.mirror = Some(MirrorStage {
            centre: mirror_centre,
            diameter,
            volume: reach + reach,
        });
    }
    if contexts.time.is_some() {
        let size = (radius * 0.8).clamp(0.5, 4.0);
        // A face turned to the camera alone is lit only by what the probes give
        // it — the bare rig has no flat ambient — and that does not follow the
        // sky (measured: 0.149 at midday and at midnight alike). Turned halfway
        // up, the face the camera reads takes the sun or the moon directly.
        let facing = scale(forward, -1.0);
        let tilted = add(facing, Vec3::Z).try_normalize().unwrap_or(Vec3::Z);
        staged.plate = Some(PlateStage {
            centre: add(centre, scale(right, -(radius + size * 0.5 + 0.5))),
            size,
            rotation: Quat::from_rotation_arc(facing, tilted)
                .mul_quat(Quat::from_rotation_arc(Vec3::NEG_Y, facing)),
        });
    }
    widen_to_fit(&mut staged, centre, radius);
    if let Some(eye) = contexts.eye {
        staged.camera.position.z = staged.camera.look_at.z + eye.offset();
    }
    if let Some(layering) = contexts.layering {
        staged.actor = Some(place_actor(
            layering,
            staged.camera.position,
            centre,
            radius,
        ));
    }
    staged
}

/// The camera's horizontal forward and right, in Second Life axes (Z up):
/// right is forward × up. Forward falls back to `+Y` for a camera looking
/// straight down.
fn horizontal_basis(camera: SceneCamera) -> (Vec3, Vec3) {
    let delta = sub(camera.look_at, camera.position);
    let forward = Vec3::new(delta.x, delta.y, 0.0)
        .try_normalize()
        .unwrap_or(Vec3::Y);
    (forward, Vec3::new(forward.y, -forward.x, 0.0))
}

/// Pull the camera back along its own direction, re-aimed at the middle of
/// everything staged, until the subject and any plate or mirror fit the frame.
/// A cell with neither keeps its declared framing.
fn widen_to_fit(staged: &mut Staged, centre: Vec3, radius: f32) {
    let mut spheres = vec![(centre, radius)];
    if let Some(mirror) = staged.mirror {
        spheres.push((mirror.centre, mirror.diameter * 0.5));
    }
    if let Some(plate) = staged.plate {
        spheres.push((plate.centre, plate.size * 0.5));
    }
    if spheres.len() < 2 {
        return;
    }
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for (at, reach) in &spheres {
        low = low.min(sub(*at, Vec3::splat(*reach)));
        high = high.max(add(*at, Vec3::splat(*reach)));
    }
    let look_at = midpoint(low, high);
    let needed = spheres
        .iter()
        .map(|(at, reach)| at.distance(look_at) + reach)
        .fold(0.0_f32, f32::max);
    let back = sub(staged.camera.position, staged.camera.look_at);
    let distance = back.length();
    let direction = back.try_normalize().unwrap_or(Vec3::NEG_Y);
    let half_fov = viewer_projection().fov * 0.5;
    let fit = needed / half_fov.sin() * FRAMING_MARGIN;
    staged.camera = SceneCamera {
        position: add(look_at, scale(direction, distance.max(fit))),
        look_at,
    };
}

/// The red wall of a `Layering` cell, facing the eye at `eye`.
fn place_actor(layering: Layering, eye: Vec3, centre: Vec3, radius: f32) -> ActorStage {
    let toward_eye = sub(eye, centre).try_normalize().unwrap_or(Vec3::NEG_Y);
    let rotation = Quat::from_rotation_arc(Vec3::Y, toward_eye);
    match layering {
        Layering::InFrontOf(kind) => {
            let gap = (eye.distance(centre) - radius) * 0.4;
            let side = radius * 2.5;
            ActorStage {
                kind,
                centre: add(centre, scale(toward_eye, radius + gap)),
                size: Vec3::new(side, 0.1, side),
                rotation,
            }
        }
        Layering::Behind(kind) => {
            let side = radius.mul_add(4.0, 4.0);
            ActorStage {
                kind,
                centre: add(centre, scale(toward_eye, -(radius + 1.0))),
                size: Vec3::new(side, 0.5, side),
                rotation,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Capturing a cell.
// ---------------------------------------------------------------------------

/// Where each projected point of a cell sits in [`CellCapture::projected`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct PointLayout {
    /// The subject's centre; its eight bound corners follow it.
    pub(crate) centre: usize,
    /// The mirror's centre and a point on its rim, if there is a mirror.
    pub(crate) mirror: Option<(usize, usize)>,
    /// The plate's centre, if there is a plate.
    pub(crate) plate: Option<usize>,
    /// The first of the subject's own points.
    pub(crate) points: usize,
}

/// The world points (in **Bevy** world space) a cell projects, and where each
/// sits in the list.
fn cell_points(staged: &Staged, signature: &SubjectSignature) -> (Vec<Vec3>, PointLayout) {
    let basis = scene_root_transform().rotation;
    let mut sl = vec![staged.centre()];
    let (min, max) = staged.bounds;
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            for z in [min.z, max.z] {
                sl.push(Vec3::new(x, y, z));
            }
        }
    }
    // The rim point lies along the camera's horizontal right, square to the
    // view: one straight above the centre is foreshortened by an eye looking
    // down or up at the mirror, and read a disc half the sphere's size.
    let (_forward, right) = horizontal_basis(staged.camera);
    let mirror = staged.mirror.map(|mirror| {
        let at = sl.len();
        sl.push(mirror.centre);
        sl.push(add(mirror.centre, scale(right, mirror.diameter * 0.5)));
        (at, at.saturating_add(1))
    });
    let plate = staged.plate.map(|plate| {
        let at = sl.len();
        sl.push(plate.centre);
        at
    });
    let points = sl.len();
    sl.extend(
        signature
            .points
            .iter()
            .map(|point| add(point.at, staged.offset)),
    );
    let layout = PointLayout {
        centre: 0,
        mirror,
        plate,
        points,
    };
    (
        sl.into_iter().map(|point| basis.mul_vec3(point)).collect(),
        layout,
    )
}

/// One cell, captured: the frame with the subject, the same frame with the
/// subject hidden, and where every point of the cell landed.
#[derive(Debug)]
pub(crate) struct CellCapture {
    /// The frame with the subject.
    pub(crate) with: Frame,
    /// The same cell with the subject hidden.
    pub(crate) without: Frame,
    /// Every projected point.
    pub(crate) projected: Projected,
    /// Where each point sits in `projected`.
    pub(crate) layout: PointLayout,
}

/// Pin the cell's view to linear output, every frame: the markers are read as
/// exact channels, and Bevy's default tone mapper lifts a pure colour's other
/// channels above the presence threshold (measured: a bare green subject read
/// `Translucent` under it).
fn pin_linear_output(mut cameras: Query<&mut Tonemapping, With<ViewerCamera>>) {
    for mut tonemapping in &mut cameras {
        if *tonemapping != Tonemapping::None {
            *tonemapping = Tonemapping::None;
        }
    }
}

/// Render a staged cell: the subject among its staged contexts, settled and read
/// at the scene's capture time; then the subject hidden and the cell read again.
///
/// One app for both, so the two frames differ by the subject and by nothing a
/// rebuilt rig could change. `None` when no frame came back (no GPU adapter).
pub(crate) fn capture_cell(
    scene: &RenderScene,
    cx: SceneCx,
    staged: &Staged,
    signature: &SubjectSignature,
) -> Option<CellCapture> {
    let _gpu = gpu_lock();
    let (logs, _guard) = capture_logs();
    let (mut app, captured) = build_posed_app(scene, cx, staged.camera, staged.offset);
    let stage_content = *staged;
    app.add_systems(
        Startup,
        move |mut commands: Commands, mut assets: SceneAssets| {
            spawn_stage(&stage_content, &mut commands, &mut assets);
        },
    )
    .add_systems(Update, pin_linear_output);
    if staged.fog {
        app.init_resource::<EnvironmentState>()
            .add_plugins(UnderwaterFogPlugin);
    }
    app.finish();
    app.cleanup();
    let label = format!("{} [{}]", scene.id, cx.contexts.name());
    if let Err(error) = settle(&mut app, &captured) {
        assert!(
            matches!(error, SettleError::NoAdapter),
            "the `{label}` cell never settled: {error}"
        );
        return None;
    }
    let with = Frame::from_rgba8(
        frame_at(&mut app, &captured, capture_time(scene))?,
        FRAME,
        FRAME,
    )?;
    let mut subjects = app
        .world_mut()
        .query_filtered::<Entity, With<SubjectRoot>>();
    let roots: Vec<Entity> = subjects.iter(app.world()).collect();
    for root in roots {
        app.world_mut().entity_mut(root).insert(Visibility::Hidden);
    }
    let without = Frame::from_rgba8(frame_at(&mut app, &captured, 0.0)?, FRAME, FRAME)?;
    check_logs(&logs, &label);

    let (points, layout) = cell_points(staged, signature);
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
    Some(CellCapture {
        with,
        without,
        projected,
        layout,
    })
}

/// Spawn everything a cell stages beside its subject, under a root of its own
/// that carries the same basis change and probe-layer propagation as a scene
/// root — so a staged mirror reflects the staged sea and sky as well.
fn spawn_stage(staged: &Staged, commands: &mut Commands, assets: &mut SceneAssets) {
    let root = commands
        .spawn(scene_root())
        .insert(Name::new("stage-root"))
        .id();
    if staged.sea {
        spawn_stage_sea(root, commands, assets);
    }
    if let Some(time) = staged.sky {
        spawn_stage_sky(time, root, commands, assets);
    }
    if let Some(mirror) = staged.mirror {
        spawn_stage_mirror(
            mirror.centre,
            mirror.diameter,
            mirror.volume,
            root,
            commands,
            assets,
        );
    }
    if let Some(actor) = staged.actor {
        spawn_stage_actor(
            actor.kind,
            actor.centre,
            actor.size,
            actor.rotation,
            root,
            commands,
            assets,
        );
    }
    if let Some(plate) = staged.plate {
        spawn_stage_plate(
            plate.centre,
            plate.size,
            plate.rotation,
            root,
            commands,
            assets,
        );
    }
}

// ---------------------------------------------------------------------------
// Reading a cell.
// ---------------------------------------------------------------------------

/// What a subject (or one point of it) came back as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seen {
    /// Drawn, and nothing of the actor shows at it.
    Solid,
    /// Drawn, and the actor shows at it too: one is seen through the other.
    Through,
    /// Not on screen: the subject changed nothing, or only the actor is there.
    Hidden,
}

/// What a cell must read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expect {
    /// On screen — solid or seen through something.
    Visible,
    /// On screen, with the actor showing at it too.
    SeeThrough,
    /// Not on screen.
    Hidden,
    /// Not asserted, for the reason given; the reading is still reported.
    Report(&'static str),
}

impl Expect {
    /// Whether `seen` satisfies this expectation.
    pub(crate) fn accepts(self, seen: Seen) -> bool {
        match self {
            Self::Visible => matches!(seen, Seen::Solid | Seen::Through),
            Self::SeeThrough => seen == Seen::Through,
            Self::Hidden => seen == Seen::Hidden,
            Self::Report(_) => true,
        }
    }
}

impl From<SubjectMarker> for Marker {
    fn from(marker: SubjectMarker) -> Self {
        match marker {
            SubjectMarker::Red => Self::Red,
            SubjectMarker::Green => Self::Green,
            SubjectMarker::Blue => Self::Blue,
            SubjectMarker::Yellow => Self::Yellow,
        }
    }
}

/// The colour a cell's actors wear, as a marker.
const ACTOR: Marker = Marker::Red;

/// The marker a subject is read *against*: the actor's red, unless the subject
/// is red itself.
fn other_than(marker: Marker) -> Marker {
    if marker == ACTOR { Marker::Blue } else { ACTOR }
}

/// Read one point of a marker subject at `at`: its colour, the actor's, both or
/// neither.
/// `None` when the point did not project.
pub(crate) fn read_marker(frame: &Frame, at: Option<Vec2>, marker: Marker) -> Option<Seen> {
    read_cell(frame, at, marker, other_than(marker)).map(|verdict| match verdict {
        CellVerdict::Solid => Seen::Solid,
        CellVerdict::Translucent => Seen::Through,
        CellVerdict::Missing | CellVerdict::Background => Seen::Hidden,
    })
}

/// Read a subject by **difference**: the fraction of its silhouette
/// that changed when it was hidden decides whether it was drawn (against
/// `floor`), and the actor's red at its centre decides whether it was drawn
/// solid or seen through.
pub(crate) fn read_difference(
    with: &Frame,
    without: &Frame,
    silhouette: Silhouette,
    floor: f32,
) -> Seen {
    if changed_fraction(with, without, silhouette) < floor {
        return Seen::Hidden;
    }
    let actor_at_centre = read_cell(with, Some(silhouette.centre), ACTOR, Marker::Blue)
        .is_some_and(|verdict| matches!(verdict, CellVerdict::Solid | CellVerdict::Translucent));
    if actor_at_centre {
        Seen::Through
    } else {
        Seen::Solid
    }
}

/// Every pixel coordinate of `frame` inside `disc`.
fn disc_pixels(frame: &Frame, disc: Silhouette) -> impl Iterator<Item = (u32, u32)> + '_ {
    let UVec2 {
        x: width,
        y: height,
    } = frame.size();
    (0..height)
        .flat_map(move |y| (0..width).map(move |x| (x, y)))
        .filter(move |(x, y)| inside(disc, *x, *y))
}

/// Whether pixel `(x, y)` lies inside `disc`.
fn inside(disc: Silhouette, x: u32, y: u32) -> bool {
    Vec2::new(f32_from(x), f32_from(y)).distance(disc.centre) <= disc.radius
}

/// A pixel index as a coordinate. Exact for any frame a test renders.
fn f32_from(value: u32) -> f32 {
    crate::pixel_oracle::f32_from_u32(value)
}

/// How far, per channel in `0..=1`, a pixel must change between a cell's two
/// frames before it counts as the subject's.
///
/// Much finer than the pixel oracle's general step, and safe to be: the two
/// frames are the same render on a held clock but for the subject, so every
/// pixel it did not touch comes back byte for byte identical (measured: zero
/// delta everywhere off the subject). A dark subject at midnight changes its
/// own pixels by only a few steps of an 8-bit target — the tree's leaves by
/// eight, exactly the general step — and must still count as drawn.
const SUBJECT_STEP: f32 = 2.5 / 255.0;

/// Whether a pixel changed between a cell's two frames by more than
/// [`SUBJECT_STEP`] in any colour channel.
fn differs(a: Vec4, b: Vec4) -> bool {
    [a.x - b.x, a.y - b.y, a.z - b.z]
        .iter()
        .any(|channel| channel.abs() > SUBJECT_STEP)
}

/// The fraction of `disc`'s pixels that differ between `with` and `without`.
/// Zero for an empty disc.
pub(crate) fn changed_fraction(with: &Frame, without: &Frame, disc: Silhouette) -> f32 {
    let (mut changed, mut total) = (0_u32, 0_u32);
    for (x, y) in disc_pixels(with, disc) {
        let (Some(a), Some(b)) = (with.pixel(x, y), without.pixel(x, y)) else {
            continue;
        };
        total = total.saturating_add(1);
        if differs(a, b) {
            changed = changed.saturating_add(1);
        }
    }
    if total == 0 {
        return 0.0;
    }
    f32_from(changed) / f32_from(total)
}

/// How many of the mirror's pixels the subject accounts for: inside `mirror`
/// but outside the subject's own `silhouette`, changing when it was hidden.
///
/// By difference and not by marker colour: a reflection is lit by what lights
/// the mirror's side of the stage, and under water a marked box's reflection
/// comes back a dark shape that carries no channel dominance at all.
pub(crate) fn reflected_pixels(
    with: &Frame,
    without: &Frame,
    mirror: Silhouette,
    silhouette: Silhouette,
) -> u32 {
    let mut count = 0_u32;
    for (x, y) in disc_pixels(with, mirror) {
        if inside(silhouette, x, y) {
            continue;
        }
        let (Some(a), Some(b)) = (with.pixel(x, y), without.pixel(x, y)) else {
            continue;
        };
        if differs(a, b) {
            count = count.saturating_add(1);
        }
    }
    count
}

/// The mean luminance of the 3×3 patch around `at`, or `None` when it did not
/// project onto the frame.
pub(crate) fn patch_luminance(frame: &Frame, at: Option<Vec2>) -> Option<f32> {
    let at = at?;
    let disc = Silhouette {
        centre: at,
        radius: 1.5,
    };
    mean_luminance(frame, disc_pixels(frame, disc))
}

/// The mean luminance of the subject's **own** pixels — those inside its
/// silhouette that changed when it was hidden — or `None` when none did.
pub(crate) fn subject_luminance(with: &Frame, without: &Frame, disc: Silhouette) -> Option<f32> {
    let own = disc_pixels(with, disc).filter(|(x, y)| {
        with.pixel(*x, *y)
            .zip(without.pixel(*x, *y))
            .is_some_and(|(a, b)| differs(a, b))
    });
    mean_luminance(with, own)
}

/// The mean luminance of `frame` over `pixels`, or `None` for none.
fn mean_luminance(frame: &Frame, pixels: impl Iterator<Item = (u32, u32)>) -> Option<f32> {
    let (mut sum, mut count) = (0.0_f32, 0_u32);
    for (x, y) in pixels {
        if let Some(pixel) = frame.pixel(x, y) {
            sum += luminance(pixel);
            count = count.saturating_add(1);
        }
    }
    (count > 0).then(|| sum / f32_from(count))
}

/// The subject's projected silhouette: its centre's pixel and the farthest
/// projected bound corner's distance from it. `None` when the centre did not
/// project.
pub(crate) fn silhouette(projected: &Projected, centre: usize) -> Option<Silhouette> {
    let at = projected.get(centre)?;
    let mut radius = 0.0_f32;
    for index in 1..=8 {
        if let Some(point) = projected.get(centre.saturating_add(index)) {
            radius = radius.max(point.distance(at));
        }
    }
    (radius > 1.0).then_some(Silhouette { centre: at, radius })
}

// ---------------------------------------------------------------------------
// What a cell must read.
// ---------------------------------------------------------------------------

/// What the subject — or, given `point`, one point of it — must read in a cell.
///
/// Rules, not a table, applied in order:
///
/// 1. an **opaque** actor in front hides it;
/// 2. a **translucent** actor in front is seen through;
/// 3. a wall behind a **translucent** subject shows through it;
/// 4. with an eye, a point whose face is **edge-on** to the eye, or which lies
///    across the water surface from the eye at a slope under
///    [`REFRACTED_MIN_SLOPE`], is reported and not asserted;
/// 5. otherwise it is visible.
pub(crate) fn expectation(
    signature: &SubjectSignature,
    staged: &Staged,
    contexts: ContextSet,
    point: Option<&SubjectPoint>,
) -> Expect {
    match contexts.layering {
        Some(Layering::InFrontOf(ActorKind::OpaquePrim)) => return Expect::Hidden,
        Some(Layering::InFrontOf(ActorKind::TranslucentPrim)) => return Expect::SeeThrough,
        Some(Layering::Behind(_)) if signature.translucent => {
            if let Some(report) = eye_report(staged, contexts, point) {
                return report;
            }
            return Expect::SeeThrough;
        }
        Some(Layering::Behind(_)) | None => {}
    }
    eye_report(staged, contexts, point).unwrap_or(Expect::Visible)
}

/// Rule 4 of [`expectation`]: whether an eye cell's point is too edge-on, or
/// too shallowly refracted, to assert.
fn eye_report(
    staged: &Staged,
    contexts: ContextSet,
    point: Option<&SubjectPoint>,
) -> Option<Expect> {
    let (Some(_eye), Some(point)) = (contexts.eye, point) else {
        return None;
    };
    let at = add(point.at, staged.offset);
    let eye = staged.camera.position;
    let to_eye = sub(eye, at);
    if to_eye.dot(point.normal).abs() < EDGE_ON_MARGIN {
        return Some(Expect::Report("edge-on to the eye"));
    }
    let across = (at.z > SCENE_WATER_LEVEL) != (eye.z > SCENE_WATER_LEVEL);
    let distance = to_eye.length();
    let slope = if distance > 0.0 {
        (at.z - eye.z).abs() / distance
    } else {
        f32::INFINITY
    };
    (across && slope < REFRACTED_MIN_SLOPE)
        .then_some(Expect::Report("refracted at a grazing angle"))
}

// ---------------------------------------------------------------------------
// Running a cell end to end.
// ---------------------------------------------------------------------------

/// What one cell came back with.
#[derive(Debug, Default)]
pub(crate) struct CellOutcome {
    /// A line per reading.
    pub(crate) report: Vec<String>,
    /// The readings that broke their expectation.
    pub(crate) failures: Vec<String>,
    /// The calibration plate's luminance, in a cell with a time of day.
    pub(crate) plate_luminance: Option<f32>,
    /// The subject's own pixels' luminance.
    pub(crate) subject_luminance: Option<f32>,
    /// Both frames, for a caller comparing two cells.
    pub(crate) frames: Option<(Frame, Frame)>,
}

/// Stage, capture and read one cell of `scene` under `contexts`.
///
/// `Ok(None)` when no frame came back (no GPU adapter). An `Err` is a cell that
/// could not be set up at all — a scene with no subject, or one the geometry
/// tier finds no vertex in — which is a registry fault and not a reading.
pub(crate) fn run_cell(
    scene: &RenderScene,
    contexts: ContextSet,
) -> Result<Option<CellOutcome>, String> {
    let cell = format!("{} [{}]", scene.id, contexts.name());
    let signature = scene
        .subject
        .ok_or_else(|| format!("{cell}: a self-staged scene takes no contexts"))?;
    let bounds = measured_bounds(scene)
        .ok_or_else(|| format!("{cell}: the scene puts no vertex anywhere"))?;
    let cx = SceneCx::among(contexts);
    let staged = stage(scene, cx, bounds);
    let Some(capture) = capture_cell(scene, cx, &staged, &signature) else {
        return Ok(None);
    };
    let mut outcome = CellOutcome::default();
    let marker = signature.marker.map(Marker::from);
    let disc = silhouette(&capture.projected, capture.layout.centre);

    // A subject without points is read by difference over its whole silhouette,
    // marked or not: its centre is a single point, and with an eye it lies on the
    // waterline, where one patch reads the surface rather than the subject.
    if signature.points.is_empty() {
        let seen = disc.map(|disc| {
            read_difference(
                &capture.with,
                &capture.without,
                disc,
                difference_floor(scene.id, contexts),
            )
        });
        let expect = expectation(&signature, &staged, contexts, None);
        record(&mut outcome, &cell, "subject", expect, seen);
    } else {
        for (index, point) in signature.points.iter().enumerate() {
            let at = capture
                .projected
                .get(capture.layout.points.saturating_add(index));
            let seen = marker.and_then(|marker| read_marker(&capture.with, at, marker));
            let expect = expectation(&signature, &staged, contexts, Some(point));
            record(&mut outcome, &cell, point.label, expect, seen);
        }
    }

    if let (Some((centre, rim)), Some(disc)) = (capture.layout.mirror, disc) {
        let mirror = capture
            .projected
            .get(centre)
            .zip(capture.projected.get(rim))
            .map(|(at, edge)| Silhouette {
                centre: at,
                radius: at.distance(edge) * MIRROR_READ_FRACTION,
            });
        match mirror {
            Some(mirror) => {
                let count = reflected_pixels(&capture.with, &capture.without, mirror, disc);
                let line = format!(
                    "{cell}: reflection → {count} px (mirror disc at {:.0} r {:.0}, subject at \
                     {:.0} r {:.0})",
                    mirror.centre, mirror.radius, disc.centre, disc.radius
                );
                if count < MIN_REFLECTED_PIXELS {
                    outcome.failures.push(format!(
                        "{line} (wanted at least {MIN_REFLECTED_PIXELS}: the mirror beside the \
                         subject does not show it)"
                    ));
                }
                outcome.report.push(line);
            }
            None => outcome
                .failures
                .push(format!("{cell}: the mirror did not project onto the frame")),
        }
    }

    outcome.plate_luminance = capture
        .layout
        .plate
        .and_then(|plate| patch_luminance(&capture.with, capture.projected.get(plate)));
    outcome.subject_luminance =
        disc.and_then(|disc| subject_luminance(&capture.with, &capture.without, disc));
    outcome.frames = Some((capture.with, capture.without));
    Ok(Some(outcome))
}

/// The fraction of a point-less subject's silhouette that must change for it
/// to read as drawn: its coverage floor, halved for a grazing eye.
///
/// A grazing eye sees the half of a waterline-centred subject that is below the
/// surface only through it, refracted at the shallow slope rule 4 of
/// [`expectation`] refuses to assert for a point, so only the half above the
/// water is owed.
fn difference_floor(id: &str, contexts: ContextSet) -> f32 {
    let floor = coverage_floor(id);
    if contexts.eye == Some(Eye::Grazing) {
        floor * 0.5
    } else {
        floor
    }
}

/// Record one reading against its expectation.
fn record(outcome: &mut CellOutcome, cell: &str, what: &str, expect: Expect, seen: Option<Seen>) {
    let shown = seen.map_or_else(
        || "off-frame".to_owned(),
        |seen| format!("{seen:?}").to_lowercase(),
    );
    let note = match expect {
        Expect::Report(reason) => format!(" [{reason}, not asserted]"),
        Expect::Visible | Expect::SeeThrough | Expect::Hidden => String::new(),
    };
    let line = format!("{cell}: {what} → {shown}, want {expect:?}{note}");
    let held = matches!(expect, Expect::Report(_)) || seen.is_some_and(|seen| expect.accepts(seen));
    if !held {
        outcome.failures.push(line.clone());
    }
    outcome.report.push(line);
}

#[cfg(test)]
mod tests {
    use super::{
        Expect, Seen, changed_fraction, expectation, read_difference, read_marker,
        reflected_pixels, stage,
    };
    use crate::pixel_oracle::{Frame, Marker, Silhouette};
    use crate::render_readback::FRAME;
    use crate::render_scene::{
        ActorKind, ContextSet, Eye, Layering, RACK_POINTS, SCENE_WATER_LEVEL, SCENES, SceneCx,
        SubjectSignature, TimeOfDay, Viewpoint,
    };
    use crate::render_test::TestError;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// A frame of one colour, with a disc of another painted at the centre.
    fn frame_with_disc(
        background: [u8; 4],
        disc: Option<[u8; 4]>,
        radius: f32,
    ) -> Result<Frame, TestError> {
        let side = usize::try_from(FRAME)?;
        let centre = Vec2::splat(crate::pixel_oracle::f32_from_u32(FRAME) / 2.0);
        let mut bytes = Vec::with_capacity(side.saturating_mul(side).saturating_mul(4));
        for y in 0..FRAME {
            for x in 0..FRAME {
                let at = Vec2::new(
                    crate::pixel_oracle::f32_from_u32(x),
                    crate::pixel_oracle::f32_from_u32(y),
                );
                let colour = match disc {
                    Some(colour) if at.distance(centre) <= radius => colour,
                    Some(_) | None => background,
                };
                bytes.extend_from_slice(&colour);
            }
        }
        Ok(Frame::from_rgba8(bytes, FRAME, FRAME).ok_or("a full frame")?)
    }

    /// The centre disc the synthetic frames paint.
    fn centre_disc(radius: f32) -> Silhouette {
        Silhouette {
            centre: Vec2::splat(crate::pixel_oracle::f32_from_u32(FRAME) / 2.0),
            radius,
        }
    }

    const GREY: [u8; 4] = [40, 40, 40, 255];
    const DARK: [u8; 4] = [10, 10, 12, 255];
    const RED: [u8; 4] = [230, 10, 10, 255];
    const GREEN: [u8; 4] = [10, 230, 10, 255];
    const GREEN_OVER_RED: [u8; 4] = [120, 120, 10, 255];

    /// **The difference reading has teeth**: identical frames read hidden, a
    /// subject that painted its disc reads solid, and one with the actor's red at
    /// its centre reads seen-through.
    #[test]
    fn the_difference_reading_tells_drawn_from_not() -> Result<(), TestError> {
        let without = frame_with_disc(GREY, None, 0.0)?;
        let disc = centre_disc(20.0);
        assert_eq!(
            read_difference(&without, &without, disc, 0.15),
            Seen::Hidden
        );
        let drawn = frame_with_disc(GREY, Some(DARK), 20.0)?;
        assert_eq!(read_difference(&drawn, &without, disc, 0.15), Seen::Solid);
        let through = frame_with_disc(GREY, Some(RED), 20.0)?;
        assert_eq!(
            read_difference(&through, &without, disc, 0.15),
            Seen::Through
        );
        assert!(changed_fraction(&drawn, &without, disc) > 0.9);
        Ok(())
    }

    /// **The marker reading has teeth**: green alone is solid, green with the
    /// actor's red is seen-through, red alone is hidden.
    #[test]
    fn the_marker_reading_tells_solid_through_and_hidden() -> Result<(), TestError> {
        let at = Some(centre_disc(1.0).centre);
        let solid = frame_with_disc(GREY, Some(GREEN), 10.0)?;
        assert_eq!(read_marker(&solid, at, Marker::Green), Some(Seen::Solid));
        let through = frame_with_disc(GREY, Some(GREEN_OVER_RED), 10.0)?;
        assert_eq!(
            read_marker(&through, at, Marker::Green),
            Some(Seen::Through)
        );
        let hidden = frame_with_disc(GREY, Some(RED), 10.0)?;
        assert_eq!(read_marker(&hidden, at, Marker::Green), Some(Seen::Hidden));
        Ok(())
    }

    /// **The reflection count has teeth**: a mirror disc that does not change
    /// when the subject is hidden counts nothing, and one that does counts its
    /// changed pixels — outside the subject's own silhouette only.
    #[test]
    fn the_reflection_count_ignores_the_subject_itself() -> Result<(), TestError> {
        let without = frame_with_disc(GREY, None, 0.0)?;
        let mirror = centre_disc(30.0);
        let elsewhere = Silhouette {
            centre: Vec2::new(10.0, 10.0),
            radius: 5.0,
        };
        assert_eq!(reflected_pixels(&without, &without, mirror, elsewhere), 0);
        let reflecting = frame_with_disc(GREY, Some(DARK), 10.0)?;
        assert!(reflected_pixels(&reflecting, &without, mirror, elsewhere) > 200);
        // The same change, but inside the subject's own silhouette: not a
        // reflection.
        let covered = centre_disc(12.0);
        assert_eq!(reflected_pixels(&reflecting, &without, mirror, covered), 0);
        Ok(())
    }

    /// The rule table, cell by cell for one subject of each kind.
    #[test]
    fn the_rules_give_each_layering_its_verdict() -> Result<(), TestError> {
        let lit = SubjectSignature {
            marker: None,
            points: &[],
            translucent: false,
            emissive: false,
        };
        let glass = SubjectSignature {
            translucent: true,
            ..lit
        };
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "prim-box")
            .ok_or("prim-box is registered")?;
        let bounds = (Vec3::splat(-0.5), Vec3::splat(0.5));
        let cases = [
            (
                Layering::InFrontOf(ActorKind::OpaquePrim),
                lit,
                Expect::Hidden,
            ),
            (
                Layering::InFrontOf(ActorKind::TranslucentPrim),
                lit,
                Expect::SeeThrough,
            ),
            (
                Layering::Behind(ActorKind::OpaquePrim),
                lit,
                Expect::Visible,
            ),
            (
                Layering::Behind(ActorKind::OpaquePrim),
                glass,
                Expect::SeeThrough,
            ),
            (
                Layering::InFrontOf(ActorKind::OpaquePrim),
                glass,
                Expect::Hidden,
            ),
        ];
        for (layering, signature, want) in cases {
            let contexts = ContextSet {
                layering: Some(layering),
                ..ContextSet::NONE
            };
            let staged = stage(scene, SceneCx::among(contexts), bounds);
            assert_eq!(
                expectation(&signature, &staged, contexts, None),
                want,
                "{}",
                contexts.name()
            );
        }
        Ok(())
    }

    /// The eye rules reproduce the walked translucency matrix's skips: from the
    /// grazing eye a cap level with it is edge-on, and from under the sea a band
    /// above the surface at a shallow slope is not asserted.
    #[test]
    fn the_eye_rules_skip_edge_on_and_shallow_refracted_points() -> Result<(), TestError> {
        let rack = SCENES
            .iter()
            .find(|scene| scene.id == "translucent-box-rack")
            .ok_or("the rack is registered")?;
        let signature = rack.subject.ok_or("the rack is a subject")?;
        let bounds = (Vec3::new(-10.5, -1.5, -3.75), Vec3::new(10.5, 1.5, 3.75));
        let point = |label: &str| -> Result<_, TestError> {
            Ok(RACK_POINTS
                .iter()
                .find(|point| point.label == label)
                .ok_or_else(|| format!("{label} is a rack point"))?)
        };
        let grazing = ContextSet {
            eye: Some(Eye::Grazing),
            ..ContextSet::NONE
        };
        let staged = stage(rack, SceneCx::among(grazing), bounds);
        assert!(
            (staged.camera.position.z - (SCENE_WATER_LEVEL + 0.05)).abs() < 1e-4,
            "the grazing eye stands five centimetres over the water"
        );
        assert!(matches!(
            expectation(&signature, &staged, grazing, Some(point("centred top")?)),
            Expect::Visible
        ));
        assert!(matches!(
            expectation(&signature, &staged, grazing, Some(point("under top")?)),
            Expect::Report(_)
        ));
        assert!(matches!(
            expectation(&signature, &staged, grazing, Some(point("sunk below")?)),
            Expect::Report(_)
        ));
        Ok(())
    }

    /// A mirror and a plate widen the framing so both fit; a bare cell keeps
    /// the declared one.
    #[test]
    fn staging_props_widens_the_camera() -> Result<(), TestError> {
        let scene = SCENES
            .iter()
            .find(|scene| scene.id == "prim-box")
            .ok_or("prim-box is registered")?;
        let bounds = (Vec3::splat(-0.5), Vec3::splat(0.5));
        let bare = stage(scene, SceneCx::new(), bounds);
        assert_eq!(bare.camera, scene.camera);
        let both = ContextSet {
            viewpoint: Some(Viewpoint::InMirror),
            time: Some(TimeOfDay::Midday),
            ..ContextSet::NONE
        };
        let staged = stage(scene, SceneCx::among(both), bounds);
        let declared = scene.camera.position.distance(scene.camera.look_at);
        let widened = staged.camera.position.distance(staged.camera.look_at);
        assert!(widened > declared, "{widened} m against {declared} m");
        let mirror = staged.mirror.ok_or("a mirror is staged")?;
        let plate = staged.plate.ok_or("a plate is staged")?;
        assert!(mirror.centre.x > 0.0, "the mirror stands camera-right");
        assert!(plate.centre.x < 0.0, "the plate stands camera-left");
        assert!(
            mirror.volume * 0.5 > mirror.centre.distance(Vec3::ZERO) + 0.5,
            "the probe volume contains the subject it must show"
        );
        Ok(())
    }
}
