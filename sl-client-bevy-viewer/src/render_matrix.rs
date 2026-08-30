//! The render **matrix**'s first tier, R0: every subject scene, captured once,
//! must paint its own silhouette — plus the first staged cells, which prove the
//! staging mechanism has teeth before the context axes grow on it.
//!
//! The silhouette is *measured*, not declared: the geometry tier's CPU app
//! tessellates the same scene, its world-space bounds are projected through the
//! very camera that draws the readback frame, and the pixels inside that disc
//! must differ from the frame's own sampled background. No golden image, no
//! per-scene pixel knowledge — a scene that renders nothing at all fails, for
//! every registered subject at once.
//!
//! Scenes that stage the whole frame (the skies, the seas, terrain, the light
//! and reflection scenes whose subject *is* an interaction) are excluded by
//! name, each with its reason, and a registry guard fails when the two lists
//! stop covering the registry — so a new scene must choose a side.

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::prelude::*;
use pretty_assertions::assert_eq;

use crate::pixel_oracle::{
    CellVerdict, Frame, Marker, Silhouette, corner_background, coverage_not_background, health,
    read_cell,
};
use crate::render_readback::{CAPTURE_AT_SECS, Projected, capture, capture_with};
use crate::render_scene::{
    MATRIX_DISTANCE, RenderScene, SCENE_WATER_LEVEL, SCENES, SceneAssets, SceneCamera, SceneCx,
    SceneLighting, Timeline, scene_root, scene_root_transform,
};
use crate::render_test::{TestError, advance_to, scene_geometry, spawn_scene};
use crate::world_api::ViewerCamera;

/// The least fraction of its own silhouette a subject must paint. A convex
/// solid fills most of its projected disc; 0.15 tolerates sparse subjects
/// (grass blades, a flexi streamer) and still catches "drew nothing".
const MIN_COVERAGE: f32 = 0.15;

/// How far a corner pixel may sit from the corners' own mean before the
/// corners stop counting as one background — at which point the subject has
/// filled the screen and coverage against them would measure nothing.
const CORNER_UNIFORMITY: f32 = 0.05;

/// Per-scene minimum coverage below [`MIN_COVERAGE`], for subjects that are
/// honestly sparse inside their own bounds — with the reason. A thin ribbon
/// waving through its swept volume paints little of the box that holds it.
const SPARSE: &[(&str, f32, &str)] = &[(
    "flexi-streamer",
    0.02,
    "a thin ribbon inside the box swept by its own waving",
)];

/// The scenes R0 does not sweep, each with its reason. A scene is either swept
/// or in this list; the registry guard below holds the two to that.
const SELF_STAGED: &[(&str, &str)] = &[
    (
        "sky-sunrise",
        "the sky is the frame; there is no silhouette",
    ),
    ("sky-midday", "the sky is the frame; there is no silhouette"),
    ("sky-sunset", "the sky is the frame; there is no silhouette"),
    (
        "sky-midnight",
        "the sky is the frame; there is no silhouette",
    ),
    ("terrain-patch", "the ground fills the frame"),
    ("terrain-patch-seam", "the ground fills the frame"),
    (
        "water-surface",
        "the sea fills the frame; its own tests read through it",
    ),
    (
        "water-straddling-translucent-prim",
        "covered by its own waterline test",
    ),
    (
        "water-translucency-under-sea",
        "covered by the walked matrix",
    ),
    (
        "water-translucency-under-backdrop",
        "covered by the walked matrix",
    ),
    (
        "water-translucency-grazing-sea",
        "covered by the walked matrix",
    ),
    (
        "water-translucency-grazing-backdrop",
        "covered by the walked matrix",
    ),
    (
        "water-translucency-over-sea",
        "covered by the walked matrix",
    ),
    (
        "water-translucency-over-backdrop",
        "covered by the walked matrix",
    ),
    (
        "water-translucent-cap-pair",
        "covered by its own sealed-plate test",
    ),
    (
        "projector-light-on-wall",
        "the subject is light on a wall, not a solid; its cell is the lit region",
    ),
    (
        "point-light-between-prims",
        "the subject is falloff between prims, not a solid",
    ),
    (
        "metallic-sphere-among-prims",
        "covered by the mirror's own reflection test",
    ),
    (
        "particles-fountain",
        "a falling cloud's bounds leave the frame; the timeline tier covers its \
         motion, and its pixels await a staged particle cell",
    ),
];

/// Whether `scene` is swept by R0.
fn swept(scene: &RenderScene) -> bool {
    !SELF_STAGED.iter().any(|(id, _reason)| *id == scene.id)
}

/// The scene's world-space bounds — the box holding every vertex — measured
/// from the geometry tier's CPU tessellation of the same scene at the **same
/// scene time [`capture`] renders it** (its last timeline sample, at least the
/// static capture time), so a fountain's bounds are the cloud on the frame and
/// not the lone seed particle of its first instant. `None` when the scene puts
/// no vertex anywhere (which the geometry tier's own sweep already fails).
fn measured_bounds(scene: &RenderScene, cx: SceneCx) -> Option<(Vec3, Vec3)> {
    let mut app = spawn_scene(cx, scene);
    let at = scene
        .timeline
        .samples
        .last()
        .copied()
        .unwrap_or(0.0)
        .max(CAPTURE_AT_SECS);
    advance_to(&mut app, at);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut any = false;
    for geometry in scene_geometry(&mut app) {
        for position in &geometry.positions {
            let world = geometry.world.transform_point3(*position);
            if !world.is_finite() {
                continue;
            }
            min = min.min(world);
            max = max.max(world);
            any = true;
        }
    }
    if !any {
        return None;
    }
    Some((min, max))
}

/// The bounds' centre and its eight real corners, for projection.
fn bound_points(min: Vec3, max: Vec3) -> Vec<Vec3> {
    let centre = Vec3::new(
        f32::midpoint(min.x, max.x),
        f32::midpoint(min.y, max.y),
        f32::midpoint(min.z, max.z),
    );
    let mut points = vec![centre];
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            for z in [min.z, max.z] {
                points.push(Vec3::new(x, y, z));
            }
        }
    }
    points
}

/// The projected silhouette: the centre's pixel and the farthest projected
/// corner's distance from it. `None` when the centre did not project.
fn silhouette_from(projected: &Projected, count: usize) -> Option<Silhouette> {
    let centre = projected.get(0)?;
    let mut radius = 0.0_f32;
    for index in 1..count {
        if let Some(point) = projected.get(index) {
            radius = radius.max(point.distance(centre));
        }
    }
    (radius > 1.0).then_some(Silhouette { centre, radius })
}

/// **Every swept scene paints its own measured silhouette.**
///
/// The R0 cell of the matrix: one capture per subject scene, the geometry
/// tier's own bounds projected through the camera that drew it, and the disc
/// must (a) hold pixels that differ from the sampled background and (b) not be
/// the whole frame. This is the check that catches "the scene renders nothing
/// at all" — the failure a forgotten plugin, a faulted shader or a broken
/// spawn actually produces — for every subject at once.
#[test]
fn every_swept_scene_paints_its_own_silhouette() -> Result<(), TestError> {
    let mut failures = Vec::new();
    let mut report = Vec::new();
    for scene in SCENES.iter().filter(|scene| swept(scene)) {
        let Some((min, max)) = measured_bounds(scene, SceneCx::new()) else {
            // No CPU geometry: the geometry tier's own sweep owns that failure.
            report.push(format!("{}: no CPU geometry, not sampled", scene.id));
            continue;
        };
        let points = bound_points(min, max);
        let Some((frame, projected)) = capture(scene, SceneCx::new(), &points) else {
            warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
            return Ok(());
        };
        let checked = check_silhouette(scene.id, &frame, &projected, points.len());
        match checked {
            Ok(line) => report.push(line),
            Err(line) => failures.push(line),
        }
    }
    assert!(
        failures.is_empty(),
        "scene(s) did not paint their own silhouette:\n  {}\n  whole sweep:\n  {}",
        failures.join("\n  "),
        report.join("\n  "),
    );
    Ok(())
}

/// One scene's silhouette verdict: a report line, or a failure line.
fn check_silhouette(
    id: &str,
    frame: &Frame,
    projected: &Projected,
    count: usize,
) -> Result<String, String> {
    let state = health(frame);
    if state.all_black || state.all_transparent {
        return Err(format!("{id}: the whole frame is black or transparent"));
    }
    let Some(silhouette) = silhouette_from(projected, count) else {
        return Err(format!(
            "{id}: the measured bounds did not project onto the frame"
        ));
    };
    let background = corner_background(frame);
    // The corners must agree with each other, or they are not background at
    // all — the direct form of "the subject must not fill the screen".
    if !corners_uniform(frame, background) {
        return Err(format!(
            "{id}: the frame's corners disagree about the background — the subject \
             fills the screen and coverage against the corners would measure nothing"
        ));
    }
    let painted = coverage_not_background(frame, silhouette, background);
    let minimum = SPARSE
        .iter()
        .find(|(sparse, _minimum, _reason)| *sparse == id)
        .map_or(MIN_COVERAGE, |(_sparse, minimum, _reason)| *minimum);
    if painted < minimum {
        return Err(format!(
            "{id}: painted only {painted:.3} of its own silhouette (centre {}, radius {:.0} px)",
            silhouette.centre, silhouette.radius
        ));
    }
    Ok(format!("{id}: painted {painted:.3}"))
}

/// Whether every corner pixel sits within [`CORNER_UNIFORMITY`] of the
/// corners' mean, per colour channel.
fn corners_uniform(frame: &Frame, mean: Vec4) -> bool {
    let UVec2 {
        x: width,
        y: height,
    } = frame.size();
    let corners = [
        (0, 0),
        (width.saturating_sub(1), 0),
        (0, height.saturating_sub(1)),
        (width.saturating_sub(1), height.saturating_sub(1)),
    ];
    corners.into_iter().all(|(x, y)| {
        frame.pixel(x, y).is_some_and(|pixel| {
            let delta = [pixel.x - mean.x, pixel.y - mean.y, pixel.z - mean.z];
            delta
                .iter()
                .all(|channel| channel.abs() <= CORNER_UNIFORMITY)
        })
    })
}

/// The registry guard: the exclusion list names only registered scenes, and
/// every scene is either swept or excluded — a new scene must choose a side,
/// and a deleted one must leave the list.
#[test]
fn every_scene_is_swept_or_excluded_for_a_reason() {
    let mut wrong = Vec::new();
    for (id, _reason) in SELF_STAGED {
        if !SCENES.iter().any(|scene| scene.id == *id) {
            wrong.push(format!("excluded scene `{id}` is not registered"));
        }
    }
    let swept_count = SCENES.iter().filter(|scene| swept(scene)).count();
    let excluded = SELF_STAGED.len();
    if swept_count.saturating_add(excluded) != SCENES.len() {
        wrong.push(format!(
            "{} scene(s), {swept_count} swept + {excluded} excluded",
            SCENES.len()
        ));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ---------------------------------------------------------------------------
// The staged cells: a marker subject placed around a scene by `capture_with`,
// proving occlusion and the first eye context are decidable before the context
// axes are grown on the registry.
// ---------------------------------------------------------------------------

/// A scene that spawns nothing itself: the staged cells inject their subjects
/// through [`capture_with`]'s prepare hook instead.
fn empty_spawn(_cx: SceneCx, _root: Entity, _commands: &mut Commands, _assets: &mut SceneAssets) {}

/// The empty stage, framed like the gallery frames a unit subject.
const EMPTY_STAGE: RenderScene = RenderScene {
    id: "test-empty-stage",
    what: "an empty stage for injected marker subjects",
    timeline: Timeline { samples: &[0.0] },
    lighting: SceneLighting::Own,
    camera: SceneCamera {
        position: Vec3::new(0.0, -4.0, 0.0),
        look_at: Vec3::new(0.0, 0.0, 0.0),
    },
    spawn: empty_spawn,
};

/// A Second Life point in the readback app's Bevy world.
fn bevy_point(sl: Vec3) -> Vec3 {
    scene_root_transform().rotation.mul_vec3(sl)
}

/// Spawn an unlit marker-coloured cuboid at a Second Life position.
fn spawn_marker_cuboid(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    at: Vec3,
    size: Vec3,
    rgb: (f32, f32, f32),
) {
    // The extents are Second Life axes too: map them through the basis, so a
    // wall thin in SL `y` (toward the camera) is thin in Bevy depth, not height.
    let extent = bevy_point(size).abs();
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(extent.x, extent.y, extent.z))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(rgb.0, rgb.1, rgb.2),
            unlit: true,
            ..StandardMaterial::default()
        })),
        Transform::from_translation(bevy_point(at)),
        Name::new("marker-cuboid"),
    ));
}

/// The green subject box every staged cell reads at its centre.
fn spawn_green_subject(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    spawn_marker_cuboid(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::ZERO,
        Vec3::splat(1.0),
        (0.0, 1.0, 0.0),
    );
}

/// An opaque red wall between the empty stage's camera and its subject.
fn spawn_red_wall(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    spawn_marker_cuboid(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::new(0.0, -2.0, 0.0),
        Vec3::new(4.0, 0.1, 4.0),
        (1.0, 0.0, 0.0),
    );
}

/// Read the staged subject's centre cell.
fn subject_cell(frame: &Frame, projected: &Projected) -> Option<CellVerdict> {
    read_cell(frame, projected.get(0), Marker::Green, Marker::Red)
}

/// Pin the staged cells' view to linear output, every frame: the markers are
/// read as exact channels, and Bevy's default tone mapper lifts a pure colour's
/// other channels above the presence threshold (measured: a bare green subject
/// read `Translucent` under it).
fn pin_linear_output(mut cameras: Query<&mut Tonemapping, With<ViewerCamera>>) {
    for mut tonemapping in &mut cameras {
        if *tonemapping != Tonemapping::None {
            *tonemapping = Tonemapping::None;
        }
    }
}

/// **The staging has teeth: an unoccluded subject reads solid, and one behind
/// an opaque wall reads missing.**
///
/// This is the `InFrontOf(OpaquePrim) → Hidden` cell of the context matrix in
/// its smallest form, and the pair proves both directions — the check passes on
/// the visible subject (so the hidden verdict is not vacuous) and fires on the
/// occluded one (so a regression that draws through an opaque surface fails).
#[test]
fn an_opaque_wall_in_front_of_the_subject_hides_it() -> Result<(), TestError> {
    let subject_point = [bevy_point(Vec3::ZERO)];
    let Some((frame, projected)) =
        capture_with(&EMPTY_STAGE, SceneCx::new(), &subject_point, |app| {
            app.add_systems(Startup, spawn_green_subject)
                .add_systems(Update, pin_linear_output);
        })
    else {
        warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
        return Ok(());
    };
    let unoccluded = subject_cell(&frame, &projected);
    assert_eq!(
        unoccluded,
        Some(CellVerdict::Solid),
        "the bare subject must read solid, or the hidden verdict below proves nothing"
    );

    let Some((frame, projected)) =
        capture_with(&EMPTY_STAGE, SceneCx::new(), &subject_point, |app| {
            app.add_systems(Startup, (spawn_green_subject, spawn_red_wall))
                .add_systems(Update, pin_linear_output);
        })
    else {
        return Ok(());
    };
    let occluded = subject_cell(&frame, &projected);
    assert_eq!(
        occluded,
        Some(CellVerdict::Missing),
        "an opaque wall stands between the camera and the subject, so the subject's \
         green must not reach the frame — drawing through an opaque surface is the bug \
         this cell exists to catch"
    );
    Ok(())
}

/// The empty stage re-aimed under the sea, its subject submerged.
const UNDERWATER_STAGE: RenderScene = RenderScene {
    id: "test-underwater-stage",
    what: "an underwater eye looking at a submerged marker subject over the sea scene",
    timeline: Timeline { samples: &[0.0] },
    lighting: SceneLighting::Own,
    camera: SceneCamera {
        position: Vec3::new(0.0, -MATRIX_DISTANCE, SCENE_WATER_LEVEL - 3.0),
        look_at: Vec3::new(0.0, 0.0, SCENE_WATER_LEVEL - 3.0),
    },
    spawn: empty_spawn,
};

/// The submerged green subject, three metres under the sea surface.
fn spawn_submerged_subject(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    spawn_marker_cuboid(
        &mut commands,
        &mut meshes,
        &mut materials,
        Vec3::new(0.0, 0.0, SCENE_WATER_LEVEL - 3.0),
        Vec3::splat(2.0),
        (0.0, 1.0, 0.0),
    );
}

/// Stage the sea itself: the registered `water-surface` scene spawned beside
/// the injected subject, under its own root, exactly as the rig spawns it.
fn spawn_sea_scene(mut commands: Commands, mut assets: SceneAssets) {
    if let Some(scene) = SCENES.iter().find(|scene| scene.id == "water-surface") {
        let root = commands.spawn(scene_root()).id();
        (scene.spawn)(SceneCx::new(), root, &mut commands, &mut assets);
    }
}

/// **A submerged subject is still drawn from an underwater eye** — the first
/// `Eye` context cell: the sea, an opaque subject below it, and a camera below
/// it too. The depth-writing sea and the pre-water transparency split must not
/// swallow content on the eye's own side of the surface.
#[test]
fn a_submerged_subject_is_drawn_from_an_underwater_eye() -> Result<(), TestError> {
    let subject_point = [bevy_point(Vec3::new(0.0, -1.0, SCENE_WATER_LEVEL - 3.0))];
    let Some((frame, projected)) =
        capture_with(&UNDERWATER_STAGE, SceneCx::new(), &subject_point, |app| {
            app.add_systems(Startup, (spawn_submerged_subject, spawn_sea_scene))
                .add_systems(Update, pin_linear_output);
        })
    else {
        warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
        return Ok(());
    };
    let verdict = subject_cell(&frame, &projected);
    assert!(
        matches!(verdict, Some(CellVerdict::Solid | CellVerdict::Translucent)),
        "the submerged subject's green did not reach the frame from an underwater eye \
         (verdict {verdict:?}) — content on the eye's own side of the surface must not \
         be swallowed by the sea's depth or the pre-water split"
    );
    Ok(())
}
