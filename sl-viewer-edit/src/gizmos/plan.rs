//! Answering [`ManipulatorProbes`]: where to press on a handle, and the pointer
//! path that moves, turns or stretches the selection by a given amount.
//!
//! Nothing here re-derives the drag: a press point is one the rig's own hit
//! test (the same ray cast the hover uses) puts on the handle, the drag state
//! is [`begin_drag`]'s for that press, and the path is its math run
//! backwards — the point on the drag plane (or the handle's line, or the ring)
//! that yields the amount, projected back to the screen. The snap regime is a
//! choice of side: the free regime keeps the release on the axis, the line or
//! inside the tick circle; the grid regime puts it well past the guide.

use bevy::camera::primitives::Aabb;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::{
    CORNER_FACTOR_STEP, DragStart, DragTargets, GizmoDrag, GizmoHandle, GizmoPart, GizmoRoot,
    begin_drag, clamp_corner_factor, primary_face_fold, ring_axes, sl_vec3, struck_handle, vec3_sl,
};
use crate::coords::{bevy_to_sl_vec, sl_to_bevy_vec};
use crate::edit_math::{
    SNAP_ANGLE_DEG, clamp_scale, ray_plane_intersect, snap_angle, snap_to_grid, vadd, vscale, vsub,
};
use crate::world_api::{
    EditToolState, ManipulatorAmount, ManipulatorAnswer, ManipulatorPlan, ManipulatorProbes,
    ManipulatorQuery, ManipulatorRefusal, SnapRegime, ViewerCamera,
};

/// How many press points a request is answered with at most.
const MAX_PRESSES: usize = 6;

/// Where press candidates sit in a handle's bounds, as fractions of its
/// half-extents: the centre, halfway, and near the rim — the rim so a ring
/// (whose bounds' centre is its hole) is found too.
const PRESS_LATTICE: [f32; 5] = [0.0, -0.5, 0.5, -0.9, 0.9];

/// Press candidates nearer each other than this, logical pixels, are one.
const PRESS_DEDUPE_PIXELS: f32 = 2.0;

/// How far past the snap guide a grid-regime release sits, as a multiple of
/// the guide's distance — well clear of the boundary, so the pick ray's slant
/// cannot put it back inside.
const GRID_REACH: f32 = 2.0;

/// How far outside the tick circle a grid-regime ring drag runs, as a
/// multiple of its radius — the circle is already wide, so just clear of it.
const RING_GRID_REACH: f32 = 1.25;

/// How far inside the tick circle a free ring drag runs when it was grabbed
/// outside it, as a fraction of the circle's radius.
const FREE_RING_REACH: f32 = 0.8;

/// The longest step a ring path takes round the ring, radians — short enough
/// that no step wraps the drag's per-frame angle.
const RING_STEP: f32 = core::f32::consts::PI / 12.0;

/// What the planner reads of the rig: the handles, their bounds, the ray cast
/// that hits them and the root whose scale sets the drag's screen ratio.
#[derive(SystemParam)]
pub(super) struct PlanRig<'w, 's> {
    /// The handle entities, their part, bounds and placement.
    handles: Query<
        'w,
        's,
        (
            &'static GizmoHandle,
            &'static Aabb,
            &'static GlobalTransform,
        ),
    >,
    /// The handle entities and their part, as the rig's hit test reads them.
    parts: Query<'w, 's, (Entity, &'static GizmoHandle)>,
    /// The ray cast the hover makes.
    ray_cast: MeshRayCast<'w, 's>,
    /// The rig root, whose scale is the constant-screen-size factor.
    rigs: Query<'w, 's, &'static Transform, With<GizmoRoot>>,
    /// The world camera, whose rays the rig is hit and projected by.
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<ViewerCamera>>,
}

/// Answer every waiting [`ManipulatorProbes`] request from the rig as it
/// stands this frame. Runs only in build mode, with the rest of the rig.
pub(super) fn plan_manipulator_drags(
    mut probes: ResMut<ManipulatorProbes>,
    tool: Res<EditToolState>,
    mut rig: PlanRig,
    targets: DragTargets,
) {
    if !probes.has_requests() {
        return;
    }
    for (id, query) in probes.take_requests() {
        let answer = answer(&query, &tool, &mut rig, &targets);
        probes.answer(id, answer);
    }
}

/// One request's answer.
fn answer(
    query: &ManipulatorQuery,
    tool: &EditToolState,
    rig: &mut PlanRig,
    targets: &DragTargets,
) -> ManipulatorAnswer {
    if query.regime == SnapRegime::Grid && !tool.snap {
        return Err(ManipulatorRefusal::SnappingOff);
    }
    let part = GizmoPart::from(query.handle);
    let Some((aabb, global)) = rig
        .handles
        .iter()
        .find(|(handle, _aabb, _global)| handle.part == part)
        .map(|(_handle, aabb, global)| (*aabb, *global))
    else {
        return Err(ManipulatorRefusal::NoHandle);
    };
    let Ok((camera, camera_transform)) = rig.cameras.single() else {
        return Err(ManipulatorRefusal::Unreachable);
    };
    let (camera, camera_transform) = (camera.clone(), *camera_transform);
    let viewport = camera
        .logical_viewport_size()
        .ok_or(ManipulatorRefusal::Unreachable)?;
    let rig_scale = rig
        .rigs
        .single()
        .map_or(1.0, |transform| transform.scale.x.max(1.0e-3));
    let presses = presses(
        part,
        &aabb,
        &global,
        &camera,
        &camera_transform,
        viewport,
        rig,
    );
    if presses.is_empty() {
        return Err(ManipulatorRefusal::Unreachable);
    }
    let project = |point: Vec3| {
        camera
            .world_to_viewport(&camera_transform, sl_to_bevy_vec(&vec3_sl(point)))
            .ok()
            .filter(|at| at.x >= 0.0 && at.y >= 0.0 && at.x < viewport.x && at.y < viewport.y)
    };
    let camera_forward = sl_vec3(bevy_to_sl_vec(*camera_transform.forward()));
    let mut plans = Vec::new();
    let mut refusal = ManipulatorRefusal::Degenerate;
    for (press, ray) in presses {
        let start = DragStart {
            part,
            ray,
            camera_transform: &camera_transform,
            rig_scale,
            shift: false,
            now: 0.0,
        };
        let Some(drag) = begin_drag(&start, tool, targets) else {
            continue;
        };
        match plan(&drag, query, tool, ray, camera_forward) {
            Ok((points, predicted)) => {
                let Some(path) = points.into_iter().map(project).collect::<Option<Vec<_>>>() else {
                    refusal = ManipulatorRefusal::OutOfView;
                    continue;
                };
                plans.push(ManipulatorPlan {
                    press,
                    path,
                    predicted,
                });
            }
            Err(this) => refusal = this,
        }
    }
    if plans.is_empty() {
        Err(refusal)
    } else {
        Ok(plans)
    }
}

/// The press points the rig's own hit test puts on the handle `part`, nearest
/// the handle's projected centre first, with the ray through each.
fn presses(
    part: GizmoPart,
    aabb: &Aabb,
    global: &GlobalTransform,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    viewport: Vec2,
    rig: &mut PlanRig,
) -> Vec<(Vec2, Ray3d)> {
    let centre = Vec3::from(aabb.center);
    let half = Vec3::from(aabb.half_extents);
    let project = |local: Vec3| {
        camera
            .world_to_viewport(camera_transform, global.transform_point(local))
            .ok()
            .filter(|at| at.x >= 0.0 && at.y >= 0.0 && at.x < viewport.x && at.y < viewport.y)
    };
    let Some(anchor) = project(centre) else {
        return Vec::new();
    };
    let mut points: Vec<Vec2> = PRESS_LATTICE
        .into_iter()
        .flat_map(|x| PRESS_LATTICE.into_iter().map(move |y| (x, y)))
        .flat_map(|(x, y)| PRESS_LATTICE.into_iter().map(move |z| Vec3::new(x, y, z)))
        .filter_map(|fraction| {
            project(Vec3::new(
                centre.x + fraction.x * half.x,
                centre.y + fraction.y * half.y,
                centre.z + fraction.z * half.z,
            ))
        })
        .collect();
    points.sort_by(|a, b| a.distance(anchor).total_cmp(&b.distance(anchor)));
    let mut accepted: Vec<(Vec2, Ray3d)> = Vec::new();
    for point in points {
        if accepted.len() >= MAX_PRESSES {
            break;
        }
        if accepted
            .iter()
            .any(|(kept, _ray)| kept.distance(point) < PRESS_DEDUPE_PIXELS)
        {
            continue;
        }
        let Ok(ray) = camera.viewport_to_world(camera_transform, point) else {
            continue;
        };
        if struck_handle(ray, &rig.parts, &mut rig.ray_cast) == Some(part) {
            accepted.push((point, ray));
        }
    }
    accepted
}

/// The release path (Second Life world points, press excluded) and the
/// predicted result of dragging `drag` by `query`.
fn plan(
    drag: &GizmoDrag,
    query: &ManipulatorQuery,
    tool: &EditToolState,
    ray: Ray3d,
    camera_forward: Vec3,
) -> Result<(Vec<Vec3>, ManipulatorAmount), ManipulatorRefusal> {
    let grid = query.regime == SnapRegime::Grid;
    let reach = drag.snap_offset * GRID_REACH;
    match (drag.part, query.amount) {
        (GizmoPart::TranslateAxis(axis), ManipulatorAmount::Distance(distance)) => {
            let world_axis = drag.frame.mul_vec3(axis.unit());
            // On the axis, `distance` beyond where the press met the drag plane.
            let along = vsub(drag.start_hit, drag.pivot).dot(world_axis);
            let on_axis = vadd(drag.pivot, vscale(world_axis, along + distance));
            if !grid {
                return Ok((vec![on_axis], ManipulatorAmount::Distance(distance)));
            }
            let across = drag.plane_normal.cross(world_axis).normalize_or_zero();
            if across == Vec3::ZERO {
                return Err(ManipulatorRefusal::Degenerate);
            }
            let coord = vadd(drag.pivot, vscale(world_axis, distance)).dot(world_axis);
            let snapped = snap_to_grid(coord, tool.grid_unit);
            Ok((
                vec![vadd(on_axis, vscale(across, reach))],
                ManipulatorAmount::Distance(distance + snapped - coord),
            ))
        }
        (GizmoPart::TranslatePlane(axis), ManipulatorAmount::Offset([u, v])) => {
            if tool.snap && !grid {
                return Err(ManipulatorRefusal::PadSnaps);
            }
            let (first, second) = axis.others();
            let first_axis = drag.frame.mul_vec3(first.unit());
            let second_axis = drag.frame.mul_vec3(second.unit());
            let delta = vadd(vscale(first_axis, u), vscale(second_axis, v));
            let end = vadd(drag.start_hit, delta);
            if !grid {
                return Ok((vec![end], ManipulatorAmount::Offset([u, v])));
            }
            // The pad's own snap: every coordinate to the grid, kept in the
            // plane.
            let target = vadd(drag.pivot, delta);
            let snapped = Vec3::new(
                snap_to_grid(target.x, tool.grid_unit),
                snap_to_grid(target.y, tool.grid_unit),
                snap_to_grid(target.z, tool.grid_unit),
            );
            let correction = vsub(snapped, target);
            let in_plane = vsub(
                correction,
                vscale(drag.plane_normal, correction.dot(drag.plane_normal)),
            );
            let moved = vadd(delta, in_plane);
            Ok((
                vec![end],
                ManipulatorAmount::Offset([moved.dot(first_axis), moved.dot(second_axis)]),
            ))
        }
        (GizmoPart::RotateRing(axis), ManipulatorAmount::Angle(angle)) => {
            let origin = sl_vec3(bevy_to_sl_vec(ray.origin));
            let direction = sl_vec3(bevy_to_sl_vec(*ray.direction));
            let hit = ray_plane_intersect(origin, direction, drag.pivot, drag.plane_normal)
                .ok_or(ManipulatorRefusal::Degenerate)?;
            let grabbed = vsub(hit, drag.pivot).length();
            let radius = if grid {
                grabbed.max(drag.snap_offset * RING_GRID_REACH)
            } else if grabbed < drag.snap_offset * FREE_RING_REACH {
                grabbed
            } else {
                drag.snap_offset * FREE_RING_REACH
            };
            let (axis_a, axis_b) = ring_axes(drag.frame, axis);
            let at = |phase: f32| {
                vadd(
                    drag.pivot,
                    vadd(
                        vscale(axis_a, radius * phase.cos()),
                        vscale(axis_b, radius * phase.sin()),
                    ),
                )
            };
            // Out to the chosen radius first (no angle), then round the ring.
            let steps = (angle.abs() / RING_STEP).ceil().max(1.0);
            let count = u32_from_steps(steps);
            let mut path = vec![at(drag.start_angle)];
            path.extend((1..=count).map(|step| {
                let fraction = f32_from(step) / f32_from(count);
                at(drag.start_angle + angle * fraction)
            }));
            let predicted = if grid {
                snap_angle(drag.start_twist + angle, SNAP_ANGLE_DEG.to_radians()) - drag.start_twist
            } else {
                angle
            };
            Ok((path, ManipulatorAmount::Angle(predicted)))
        }
        (GizmoPart::ScaleFace(axis, positive), ManipulatorAmount::Distance(change)) => {
            let sign = if positive { 1.0 } else { -1.0 };
            let dir = vscale(drag.frame.mul_vec3(axis.unit()), sign);
            let (_fold, extent, alignment) = primary_face_fold(drag, dir);
            let size_mult = if tool.stretch_both { 2.0 } else { 1.0 };
            let travel = change * alignment / size_mult;
            let on_line = vadd(drag.pivot, vscale(dir, drag.start_param + travel));
            if !grid {
                return Ok((
                    vec![on_line],
                    ManipulatorAmount::Distance(clamp_scale(extent + change) - extent),
                ));
            }
            let across = dir.cross(camera_forward).normalize_or_zero();
            if across == Vec3::ZERO {
                return Err(ManipulatorRefusal::Degenerate);
            }
            let snapped = clamp_scale(snap_to_grid(extent + change, tool.grid_unit));
            Ok((
                vec![vadd(on_line, vscale(across, reach))],
                ManipulatorAmount::Distance(snapped - extent),
            ))
        }
        (GizmoPart::ScaleCorner(signs), ManipulatorAmount::Factor(factor)) => {
            let dir = drag.corner_dir(signs);
            let ratio = if tool.stretch_both {
                factor
            } else {
                (factor - 0.5) * 2.0
            };
            if ratio <= 0.0 {
                return Err(ManipulatorRefusal::WrongAmount);
            }
            let on_line = vadd(drag.pivot, vscale(dir, ratio * drag.start_param));
            if !grid {
                return Ok((
                    vec![on_line],
                    ManipulatorAmount::Factor(clamp_corner_factor(factor, &drag.objects)),
                ));
            }
            let across = dir.cross(camera_forward).normalize_or_zero();
            if across == Vec3::ZERO {
                return Err(ManipulatorRefusal::Degenerate);
            }
            let stepped = ((factor / CORNER_FACTOR_STEP).round() * CORNER_FACTOR_STEP)
                .max(CORNER_FACTOR_STEP);
            Ok((
                vec![vadd(on_line, vscale(across, reach))],
                ManipulatorAmount::Factor(clamp_corner_factor(stepped, &drag.objects)),
            ))
        }
        _mismatch => Err(ManipulatorRefusal::WrongAmount),
    }
}

/// A whole, positive step count as a `u32` (saturating; a ring path is never
/// that long).
fn u32_from_steps(steps: f32) -> u32 {
    // No float-to-int conversion without `as`: count up to it.
    let mut count = 1_u32;
    while f32_from(count) < steps && count < u32::from(u16::MAX) {
        count = count.saturating_add(1);
    }
    count
}

/// A small count as an `f32`, losslessly for anything a ring path counts.
fn f32_from(count: u32) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}
