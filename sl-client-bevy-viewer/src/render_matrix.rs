//! The render **context matrix** (`viewer-render-context-matrix`): every
//! registered subject, captured on its own and then under the contexts it
//! declares, each cell read against a rule rather than a golden image.
//!
//! - **R0, canonical**: every subject once, on the bare rig. Its silhouette is
//!   *measured* — the geometry tier tessellates the same scene, its bounds are
//!   projected through the very camera that draws the frame — and the pixels
//!   inside it must differ from the frame's own sampled background. A scene that
//!   renders nothing at all fails, for every subject at once.
//! - **R1, single axis**: every subject under every value of every axis it lists
//!   in `applies` — an eye under, grazing or over the sea; the four times of
//!   day; a mirror beside it; an opaque or translucent wall in front of or
//!   behind it. `crate::render_stage` stages the cell and its `expectation()`
//!   says what it must read.
//! - **R2, curated pairs**: the combinations worth their capture — the rack's
//!   eyes with a wall behind (whether each face is see-through, not merely
//!   drawn), a mirror under the sea, an avatar under fogged water, a glow prim
//!   over the sea, a translucent box in a mirror over the sea and grazing the
//!   sea with a wall behind, a texture animation in a sunlit mirror.
//! - **All pairs**, opt-in (`SL_VIEWER_RENDER_MATRIX=full`): every subject under
//!   every pair of its axes, generated.
//!
//! Each sweep declares a **capture budget**, and the registry guard fails when a
//! new subject or axis would exceed it — before anyone pays the GPU time.
//!
//! Scenes that stage the whole frame (the skies, the seas, terrain, the light
//! and reflection scenes whose subject *is* an interaction) carry no subject and
//! are listed here with their reasons; a registry guard fails when the list and
//! the registry disagree, so a new scene must choose a side.

use bevy::prelude::*;

use crate::pixel_oracle::{
    Frame, Silhouette, corner_background, coverage_not_background, differing_pixels, health,
};
use crate::render_readback::{Projected, capture};
use crate::render_scene::{
    ActorKind, Axis, ContextSet, Eye, Layering, RenderScene, SCENES, SceneCx, TimeOfDay, Toggle,
    Viewpoint, scene_root_transform,
};
use crate::render_stage::{CellOutcome, coverage_floor, measured_bounds, run_cell, silhouette};
use crate::render_test::TestError;

/// How far a corner pixel may sit from the corners' own mean before the
/// corners stop counting as one background — at which point the subject has
/// filled the screen and coverage against them would measure nothing.
const CORNER_UNIFORMITY: f32 = 0.05;

/// The scenes with no subject, each with its reason. A scene is a subject or in
/// this list; the registry guard below holds the two to that.
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

/// The most axes a subject may declare: past four, its all-pairs cells grow
/// faster than anything they could find.
const MAX_APPLIES: usize = 4;

/// The R1 sweep's capture budget: one capture per subject per value of each axis
/// it declares. Today's registry takes 85 — about four minutes of release GPU
/// time on one adapter.
const R1_BUDGET: usize = 90;

/// The R2 sweep's capture budget: its curated cells, plus the untoggled twin of
/// every cell with a toggle.
const R2_BUDGET: usize = 12;

/// The opt-in all-pairs sweep's capture budget.
const FULL_BUDGET: usize = 160;

/// The environment variable that runs the all-pairs sweep, and its value.
const FULL_SWEEP: (&str, &str) = ("SL_VIEWER_RENDER_MATRIX", "full");

/// The registered subjects.
fn subjects() -> impl Iterator<Item = &'static RenderScene> {
    SCENES.iter().filter(|scene| scene.subject.is_some())
}

/// A registered scene by id.
fn scene(id: &str) -> Result<&'static RenderScene, TestError> {
    Ok(SCENES
        .iter()
        .find(|scene| scene.id == id)
        .ok_or_else(|| format!("the `{id}` scene is not registered"))?)
}

// ---------------------------------------------------------------------------
// R0: every subject paints its own silhouette.
// ---------------------------------------------------------------------------

/// The bounds' centre and its eight real corners, in **Bevy** world space, for
/// projection.
fn bound_points(min: Vec3, max: Vec3) -> Vec<Vec3> {
    let basis = scene_root_transform().rotation;
    let centre = Vec3::new(
        f32::midpoint(min.x, max.x),
        f32::midpoint(min.y, max.y),
        f32::midpoint(min.z, max.z),
    );
    let mut points = vec![basis.mul_vec3(centre)];
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            for z in [min.z, max.z] {
                points.push(basis.mul_vec3(Vec3::new(x, y, z)));
            }
        }
    }
    points
}

/// **Every subject paints its own measured silhouette.**
///
/// The R0 cell of the matrix: one capture per subject, the geometry tier's own
/// bounds projected through the camera that drew it, and the disc must (a) hold
/// pixels that differ from the sampled background and (b) not be the whole
/// frame. This is the check that catches "the scene renders nothing at all" —
/// the failure a forgotten plugin, a faulted shader or a broken spawn actually
/// produces — for every subject at once.
#[test]
fn every_subject_paints_its_own_silhouette() -> Result<(), TestError> {
    let mut failures = Vec::new();
    let mut report = Vec::new();
    for scene in subjects() {
        let Some((min, max)) = measured_bounds(scene) else {
            // No CPU geometry: the geometry tier's own sweep owns that failure.
            report.push(format!("{}: no CPU geometry, not sampled", scene.id));
            continue;
        };
        let points = bound_points(min, max);
        let Some((frame, projected)) = capture(scene, SceneCx::new(), &points) else {
            warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
            return Ok(());
        };
        match check_silhouette(scene.id, &frame, &projected) {
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
fn check_silhouette(id: &str, frame: &Frame, projected: &Projected) -> Result<String, String> {
    let state = health(frame);
    if state.all_black || state.all_transparent {
        return Err(format!("{id}: the whole frame is black or transparent"));
    }
    let Some(silhouette) = silhouette(projected, 0) else {
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
    let minimum = coverage_floor(id);
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

// ---------------------------------------------------------------------------
// The registry guards.
// ---------------------------------------------------------------------------

/// The self-staged list names only registered scenes, and a scene has no subject
/// exactly when it is listed — a new scene must choose a side, and a deleted one
/// must leave the list.
#[test]
fn every_scene_is_a_subject_or_self_staged_for_a_reason() {
    let mut wrong = Vec::new();
    for (id, _reason) in SELF_STAGED {
        match SCENES.iter().find(|scene| scene.id == *id) {
            None => wrong.push(format!("self-staged scene `{id}` is not registered")),
            Some(scene) if scene.subject.is_some() => wrong.push(format!(
                "`{id}` is listed as self-staged but declares a subject"
            )),
            Some(_) => {}
        }
    }
    for scene in SCENES {
        let listed = SELF_STAGED.iter().any(|(id, _reason)| *id == scene.id);
        if scene.subject.is_none() && !listed {
            wrong.push(format!(
                "`{}` declares no subject and is not listed as self-staged",
                scene.id
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every subject's `applies` names each axis once and at most [`MAX_APPLIES`]
/// of them, and a self-staged scene names none.
#[test]
fn every_subject_declares_a_small_distinct_set_of_axes() {
    let mut wrong = Vec::new();
    for scene in SCENES {
        if scene.subject.is_none() && !scene.applies.is_empty() {
            wrong.push(format!(
                "`{}` is self-staged but declares axes, which it cannot take",
                scene.id
            ));
        }
        if scene.applies.len() > MAX_APPLIES {
            wrong.push(format!(
                "`{}` declares {} axes, past the {MAX_APPLIES} the matrix allows",
                scene.id,
                scene.applies.len()
            ));
        }
        for (index, axis) in scene.applies.iter().enumerate() {
            if scene
                .applies
                .iter()
                .skip(index.saturating_add(1))
                .any(|other| other == axis)
            {
                wrong.push(format!("`{}` declares {} twice", scene.id, axis.name()));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Every R1 cell: each subject under each value of each axis it declares.
fn r1_cells(axis: Axis) -> Vec<(&'static RenderScene, ContextSet)> {
    subjects()
        .filter(|scene| scene.applies.contains(&axis))
        .flat_map(|scene| {
            ContextSet::along(axis)
                .into_iter()
                .map(move |cell| (scene, cell))
        })
        .collect()
}

/// Every all-pairs cell: each subject under every combination of two values of
/// two different axes it declares.
fn full_cells() -> Vec<(&'static RenderScene, ContextSet)> {
    let mut cells = Vec::new();
    for scene in subjects() {
        for (index, first) in scene.applies.iter().enumerate() {
            for second in scene.applies.iter().skip(index.saturating_add(1)) {
                for a in ContextSet::along(*first) {
                    for b in ContextSet::along(*second) {
                        if let Some(cell) = a.with(b) {
                            cells.push((scene, cell));
                        }
                    }
                }
            }
        }
    }
    cells
}

/// The curated R2 cells: the combinations worth their capture, each with what
/// it asks. Built from the named scenes at run time, so a typo is a failure
/// rather than a cell that silently never runs.
const CURATED: &[(&str, ContextSet, &str)] = &[
    (
        "translucent-box-rack",
        ContextSet {
            eye: Some(Eye::Under),
            layering: Some(Layering::Behind(ActorKind::OpaquePrim)),
            ..ContextSet::NONE
        },
        "every face seen from under the sea shows the wall behind it",
    ),
    (
        "translucent-box-rack",
        ContextSet {
            eye: Some(Eye::Grazing),
            layering: Some(Layering::Behind(ActorKind::OpaquePrim)),
            ..ContextSet::NONE
        },
        "every face seen grazing the sea shows the wall behind it",
    ),
    (
        "translucent-box-rack",
        ContextSet {
            eye: Some(Eye::Over),
            layering: Some(Layering::Behind(ActorKind::OpaquePrim)),
            ..ContextSet::NONE
        },
        "every face seen from over the sea shows the wall behind it",
    ),
    (
        "prim-box",
        ContextSet {
            eye: Some(Eye::Under),
            viewpoint: Some(Viewpoint::InMirror),
            ..ContextSet::NONE
        },
        "a mirror half under the sea still shows the box beside it",
    ),
    (
        "avatar-morphed-body",
        ContextSet {
            eye: Some(Eye::Under),
            toggle: Some(Toggle::UnderwaterFog),
            ..ContextSet::NONE
        },
        "an avatar a few metres away under fogged water is still drawn",
    ),
    (
        "bump-face",
        ContextSet {
            eye: Some(Eye::Over),
            time: Some(TimeOfDay::Midday),
            ..ContextSet::NONE
        },
        "the glow, shiny, bump and fullbright prims are drawn over a sunlit sea",
    ),
    (
        "prim-translucent-box",
        ContextSet {
            eye: Some(Eye::Over),
            viewpoint: Some(Viewpoint::InMirror),
            ..ContextSet::NONE
        },
        "a translucent box over the sea is reflected in a mirror",
    ),
    (
        "prim-translucent-box",
        ContextSet {
            eye: Some(Eye::Grazing),
            layering: Some(Layering::Behind(ActorKind::OpaquePrim)),
            ..ContextSet::NONE
        },
        "a translucent box grazing the sea shows the wall behind it",
    ),
    (
        "texture-anim-flipbook",
        ContextSet {
            time: Some(TimeOfDay::Midday),
            viewpoint: Some(Viewpoint::InMirror),
            ..ContextSet::NONE
        },
        "a texture-animated prim is reflected in a sunlit mirror",
    ),
];

/// How many captures the curated sweep takes: one per cell, and a second for
/// each cell with a toggle (the same cell untoggled, which must differ).
fn curated_captures() -> usize {
    CURATED
        .iter()
        .map(|(_id, cell, _what)| if cell.toggle.is_some() { 2 } else { 1 })
        .sum()
}

/// **The sweeps stay inside their capture budgets**, and every curated cell
/// names a registered subject and only axes it declares (or a toggle).
#[test]
fn the_sweeps_stay_inside_their_capture_budgets() {
    let r1: usize = [Axis::Eye, Axis::Time, Axis::Viewpoint, Axis::Layering]
        .into_iter()
        .map(|axis| r1_cells(axis).len())
        .sum();
    assert!(
        r1 <= R1_BUDGET,
        "the R1 sweep takes {r1} captures, past its budget of {R1_BUDGET} — trim a subject's \
         `applies` or raise the budget deliberately"
    );
    let r2 = curated_captures();
    assert!(
        r2 <= R2_BUDGET,
        "the R2 sweep takes {r2} captures, past its budget of {R2_BUDGET}"
    );
    let full = full_cells().len();
    assert!(
        full <= FULL_BUDGET,
        "the all-pairs sweep takes {full} captures, past its budget of {FULL_BUDGET}"
    );
    let mut wrong = Vec::new();
    for (id, cell, _what) in CURATED {
        match SCENES.iter().find(|scene| scene.id == *id) {
            None => wrong.push(format!("curated cell names unregistered `{id}`")),
            Some(scene) if scene.subject.is_none() => {
                wrong.push(format!("curated cell names self-staged `{id}`"));
            }
            Some(scene) => {
                let set = [
                    (cell.eye.is_some(), Axis::Eye),
                    (cell.time.is_some(), Axis::Time),
                    (cell.viewpoint.is_some(), Axis::Viewpoint),
                    (cell.layering.is_some(), Axis::Layering),
                ];
                for (on, axis) in set {
                    if on && !scene.applies.contains(&axis) {
                        wrong.push(format!(
                            "curated `{id}` [{}] uses the {} axis, which `{id}` does not declare",
                            cell.name(),
                            axis.name()
                        ));
                    }
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

// ---------------------------------------------------------------------------
// Running sweeps.
// ---------------------------------------------------------------------------

/// What a sweep collected: every report line, every failure, and each cell's
/// outcome for a caller that compares cells.
#[derive(Default)]
struct Sweep {
    /// Every reading's line.
    report: Vec<String>,
    /// The readings that broke their expectation.
    failures: Vec<String>,
    /// Each cell's outcome, by scene id and cell.
    outcomes: Vec<(&'static str, ContextSet, CellOutcome)>,
}

/// Run `cells`. `Ok(None)` when no frame came back (no GPU adapter).
fn sweep(cells: Vec<(&'static RenderScene, ContextSet)>) -> Result<Option<Sweep>, TestError> {
    let mut collected = Sweep::default();
    for (scene, cell) in cells {
        let Some(mut outcome) = run_cell(scene, cell)? else {
            return Ok(None);
        };
        collected.report.append(&mut outcome.report);
        collected.failures.append(&mut outcome.failures);
        collected.outcomes.push((scene.id, cell, outcome));
    }
    Ok(Some(collected))
}

/// Fail with the sweep's failures and its whole report, if there are any.
fn assert_sweep(name: &str, collected: &Sweep) {
    assert!(
        collected.failures.is_empty(),
        "the {name} sweep found cell(s) that broke their expectation:\n  {}\n  whole sweep:\n  {}",
        collected.failures.join("\n  "),
        collected.report.join("\n  "),
    );
}

/// Run one R1 axis and assert it.
fn r1(axis: Axis) -> Result<Option<Sweep>, TestError> {
    let Some(collected) = sweep(r1_cells(axis))? else {
        warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
        return Ok(None);
    };
    assert_sweep(&format!("R1 {}", axis.name()), &collected);
    Ok(Some(collected))
}

/// **R1, eye**: every subject that declares the eye axis, centred on the
/// waterline, is drawn from under, grazing and over the sea — and every band of
/// the translucent box rack survives the depth-writing sea from every eye, the
/// verdict set the hand-staged translucency matrix used to give.
#[test]
fn r1_every_subject_is_drawn_from_every_eye() -> Result<(), TestError> {
    r1(Axis::Eye)?;
    Ok(())
}

/// **R1, layering**: an opaque wall in front hides the subject, a translucent
/// one shows it through, and a wall behind shows through a translucent subject
/// and is covered by an opaque one. The opaque-in-front cell is the matrix's
/// teeth: drawing through an opaque surface fails it, and its visible siblings
/// prove the hidden verdict is not vacuous.
#[test]
fn r1_every_subject_is_layered_by_its_actor() -> Result<(), TestError> {
    r1(Axis::Layering)?;
    Ok(())
}

/// **R1, viewpoint**: every subject that declares it is reflected in a mirror
/// beside it.
#[test]
fn r1_every_subject_is_reflected_in_a_mirror() -> Result<(), TestError> {
    r1(Axis::Viewpoint)?;
    Ok(())
}

/// The luminance a sweep recorded for `id` at `time`: the plate's and the
/// subject's own.
fn luminance_at(collected: &Sweep, id: &str, time: TimeOfDay) -> (Option<f32>, Option<f32>) {
    collected
        .outcomes
        .iter()
        .find(|(scene, cell, _outcome)| *scene == id && cell.time == Some(time))
        .map_or((None, None), |(_scene, _cell, outcome)| {
            (outcome.plate_luminance, outcome.subject_luminance)
        })
}

/// **R1, time of day**: every subject that declares it is drawn under the real
/// sky at each anchor, and the light follows the sun — the matte white plate
/// beside it is brighter at midday than at midnight, and so is a lit
/// (non-emissive) subject's own surface.
#[test]
fn r1_every_subject_is_lit_by_the_time_of_day() -> Result<(), TestError> {
    let Some(collected) = r1(Axis::Time)? else {
        return Ok(());
    };
    let mut wrong = Vec::new();
    for scene in subjects().filter(|scene| scene.applies.contains(&Axis::Time)) {
        let (plate_noon, subject_noon) = luminance_at(&collected, scene.id, TimeOfDay::Midday);
        let (plate_night, subject_night) = luminance_at(&collected, scene.id, TimeOfDay::Midnight);
        match plate_noon.zip(plate_night) {
            Some((noon, night)) if noon > night => {}
            Some((noon, night)) => wrong.push(format!(
                "{}: the plate reads {noon:.3} at midday and {night:.3} at midnight — the sky's \
                 light does not follow the sun",
                scene.id
            )),
            None => wrong.push(format!("{}: the plate did not project", scene.id)),
        }
        let emissive = scene.subject.is_some_and(|subject| subject.emissive);
        if !emissive {
            match subject_noon.zip(subject_night) {
                Some((noon, night)) if noon > night => {}
                Some((noon, night)) => wrong.push(format!(
                    "{}: the subject's own surface reads {noon:.3} at midday and {night:.3} at \
                     midnight — a lit surface must be brighter under the sun",
                    scene.id
                )),
                None => wrong.push(format!("{}: the subject showed no pixels", scene.id)),
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "the time-of-day luminance ordering failed:\n  {}",
        wrong.join("\n  ")
    );
    Ok(())
}

/// **R2, the curated pairs** — and each toggled cell differs from the same cell
/// untoggled, or the toggle did nothing and the cell proved nothing.
#[test]
fn r2_the_curated_pairs_hold() -> Result<(), TestError> {
    let mut cells = Vec::new();
    for (id, cell, _what) in CURATED {
        cells.push((scene(id)?, *cell));
    }
    let Some(collected) = sweep(cells)? else {
        warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
        return Ok(());
    };
    assert_sweep("R2", &collected);
    let mut wrong = Vec::new();
    for (id, cell, outcome) in &collected.outcomes {
        let Some(toggle) = cell.toggle else {
            continue;
        };
        let untoggled = ContextSet {
            toggle: None,
            ..*cell
        };
        let Some(twin) = run_cell(scene(id)?, untoggled)? else {
            return Ok(());
        };
        let differing = outcome
            .frames
            .as_ref()
            .zip(twin.frames.as_ref())
            .map_or(0, |((with, _), (twin_with, _))| {
                differing_pixels(with, twin_with, None)
            });
        if differing < TOGGLE_MIN_DIFFERING {
            wrong.push(format!(
                "{id} [{}]: switching on {} changed only {differing} pixels",
                cell.name(),
                toggle.name()
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    Ok(())
}

/// The least number of pixels a toggle must change in its cell: a pass that ran
/// over a 65 536-pixel frame and changed a few dozen did not run.
const TOGGLE_MIN_DIFFERING: u32 = 500;

/// **All pairs**, opt-in: every subject under every pair of its axes. Skipped
/// unless `SL_VIEWER_RENDER_MATRIX=full`.
#[test]
fn all_pairs_hold_when_asked_for() -> Result<(), TestError> {
    let (variable, value) = FULL_SWEEP;
    if std::env::var(variable).ok().as_deref() != Some(value) {
        return Ok(());
    }
    let Some(collected) = sweep(full_cells())? else {
        warn!("skipping: no frame came back, so this machine has no usable GPU adapter");
        return Ok(());
    };
    assert_sweep("all-pairs", &collected);
    Ok(())
}

/// The silhouette reader stays honest on a synthetic frame: a disc of colour on
/// a uniform background reads as painted, and an empty frame does not.
#[test]
fn the_silhouette_check_tells_painted_from_empty() -> Result<(), TestError> {
    let size = crate::render_readback::FRAME;
    let mut bytes = Vec::new();
    let mut painted = Vec::new();
    let centre = Vec2::splat(crate::pixel_oracle::f32_from_u32(size) / 2.0);
    for y in 0..size {
        for x in 0..size {
            let at = Vec2::new(
                crate::pixel_oracle::f32_from_u32(x),
                crate::pixel_oracle::f32_from_u32(y),
            );
            bytes.extend_from_slice(&[30, 30, 30, 255]);
            if at.distance(centre) <= 30.0 {
                painted.extend_from_slice(&[200, 120, 60, 255]);
            } else {
                painted.extend_from_slice(&[30, 30, 30, 255]);
            }
        }
    }
    let empty = Frame::from_rgba8(bytes, size, size).ok_or("a full frame")?;
    let drawn = Frame::from_rgba8(painted, size, size).ok_or("a full frame")?;
    let projected = Projected(
        core::iter::once(Some(centre))
            .chain(core::iter::repeat_n(
                Some(Vec2::new(centre.x + 30.0, centre.y)),
                8,
            ))
            .collect(),
    );
    let disc = Silhouette {
        centre,
        radius: 30.0,
    };
    assert!(coverage_not_background(&drawn, disc, corner_background(&drawn)) > 0.9);
    check_silhouette("synthetic", &drawn, &projected)?;
    if let Ok(line) = check_silhouette("synthetic", &empty, &projected) {
        return Err(format!("an empty frame read as painted: {line}").into());
    }
    Ok(())
}
