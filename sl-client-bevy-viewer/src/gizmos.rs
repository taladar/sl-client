//! The transform gizmos (`viewer-transform-gizmos`): interactive move /
//! rotate / stretch manipulators for the selected objects
//! ([`crate::edit_selection`]), with grid snapping and world / local grid
//! frames, pushing edits to the simulator as `MultipleObjectUpdate`
//! ([`Command::UpdateObject`]).
//!
//! # Model
//!
//! - While the build tool is active with a non-empty selection, a **gizmo
//!   rig** sits at the selection pivot, oriented to the grid frame
//!   ([`crate::edit_tool::GridFrame`]) and scaled each frame to a constant
//!   on-screen size. The active tool ([`crate::edit_tool::EditTool`]) picks
//!   the handles: two-headed axis arrows + planar pads (move), axis rings
//!   (rotate), face + corner cubes (stretch).
//! - The rig renders through its **own overlay camera** (a child of the main
//!   camera on [`EDIT_GIZMO_RENDER_LAYER`], drawn between the world and the
//!   HUD) so handles are never occluded by scene geometry, the reference's
//!   always-on-top manipulator rendering.
//! - **Dragging** intersects the mouse ray with the reference viewer's drag
//!   geometry ([`crate::edit_math`]): a camera-facing plane through the drag
//!   axis (translate), the ring plane (rotate), or the handle's line
//!   (stretch). Edits apply to the scene immediately (the local echo the
//!   reference performs with `setPosition` / `setRotation`) and are sent on
//!   release — except a stretch, which also streams at the reference's 10 Hz
//!   while dragging. Snapping quantises translate coordinates and stretch
//!   extents to the grid unit and rotations to 5.625° detents while
//!   [`EditToolState::snap`] is on.
//! - **Frames**: `World` aligns the rig to the region axes, `Local` to the
//!   primary selection's rotation; `Reference` (set by the future
//!   grid-options task) currently falls back to `World`. A face stretch drags
//!   along the **grid-frame** axis and each object folds the delta onto its
//!   own nearest local scale axis, divided by the alignment (the reference's
//!   `stretchFace` `nearestAxis` fold) — which is what makes world- and
//!   local-frame stretching genuinely different. Stretch-both-sides scales
//!   about the centre; off, a face stretch holds the opposite face and a
//!   corner stretch scales about the opposite corner with the reference's
//!   halved cursor mapping (`0.5 + t/2`).
//!
//! Deliberate simplifications vs. the reference (`llmaniptranslate` /
//! `llmaniprotate` / `llmanipscale`): the stretch rig is a fixed-size widget
//! rather than handles on the selection's bounding box (the drag math is
//! ratio-based, so behaviour matches); corner drags scale about the selection
//! pivot, snapping to quarter-factor steps; there is no free-rotate sphere
//! and no copy-on-drag; and the manipulators read the pointer directly (the
//! keyboard-only input action map has no pointer axis to consume).
//!
//! Reference (Firestorm, read-only): `llmanip`, `llmaniptranslate`,
//! `llmaniprotate`, `llmanipscale`, `llselectmgr` (`sendMultipleUpdate`).

use std::collections::HashSet;

use bevy::app::Propagate;
use bevy::camera::Hdr;
use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::ecs::system::SystemParam;
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;
use sl_client_bevy::{Command, ObjectTransform, Rotation, ScopedObjectId, SlCommand, Vector};

use crate::camera::ViewerCamera;
use crate::coords::{
    bevy_to_sl_vec, sl_rotation_to_quat, sl_to_bevy_object_rotation, sl_to_bevy_rotation,
    sl_to_bevy_vec,
};
use crate::edit_math::{
    MAX_PRIM_SCALE, MIN_PRIM_SCALE, SNAP_ANGLE_DEG, clamp_scale, closest_line_param,
    constant_screen_scale, manip_plane_normal, nearest_local_axis, project_onto_axis,
    quat_to_rotation, ray_plane_intersect, ring_angle, snap_angle, snap_to_grid, vadd, vscale,
    vsub,
};
use crate::edit_selection::SelectionSet;
use crate::edit_tool::{EditTool, EditToolState, GridFrame};
use crate::hud_pick::pointer_over_blocking_ui;
use crate::objects::{ObjectCategory, ObjectSlMotion, ObjectState, SceneObject};

/// The render layer the gizmo rig (and only it) lives on, drawn by the
/// overlay camera between the world (order 0) and the HUD (order 2).
pub(crate) const EDIT_GIZMO_RENDER_LAYER: usize = 3;

/// Whether an entity's [`RenderLayers`] put it on the gizmo layer — the
/// counterpart of [`crate::hud::on_hud_layer`], used to keep gizmo handles out
/// of world picks.
pub(crate) fn on_gizmo_layer(layers: Option<&RenderLayers>) -> bool {
    layers.is_some_and(|layers| layers.intersects(&RenderLayers::layer(EDIT_GIZMO_RENDER_LAYER)))
}

/// The gizmo rig's target on-screen size, in logical pixels (arrow length
/// from the pivot), the reference's constant-screen-size arrows.
const GIZMO_SCREEN_PX: f32 = 110.0;

/// How often a stretch drag streams `MultipleObjectUpdate`s, in seconds — the
/// reference's `UPDATE_DELAY` (10 Hz).
const SCALE_STREAM_INTERVAL: f32 = 0.1;

/// One of the three grid axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GizmoAxis {
    /// The frame's X axis (red).
    X,
    /// The frame's Y axis (green).
    Y,
    /// The frame's Z axis (blue).
    Z,
}

impl GizmoAxis {
    /// All three, in order.
    const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    /// The axis' unit vector in the (Second Life space) grid frame.
    const fn unit(self) -> Vec3 {
        match self {
            Self::X => Vec3::X,
            Self::Y => Vec3::Y,
            Self::Z => Vec3::Z,
        }
    }

    /// The axis' index into a `[f32; 3]` / `Vector` triple.
    const fn index(self) -> usize {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
        }
    }

    /// The reference viewer's axis colour (X red, Y green, Z blue).
    const fn color(self) -> Color {
        match self {
            Self::X => Color::srgba(0.9, 0.2, 0.15, 1.0),
            Self::Y => Color::srgba(0.2, 0.85, 0.2, 1.0),
            Self::Z => Color::srgba(0.2, 0.4, 0.95, 1.0),
        }
    }

    /// The rotation carrying a Y-axis-authored Bevy primitive (cylinder /
    /// cone) onto this Second Life axis, in the rig's (Second Life) local
    /// space.
    fn orientation(self) -> Quat {
        match self {
            Self::X => Quat::from_rotation_z(-core::f32::consts::FRAC_PI_2),
            Self::Y => Quat::IDENTITY,
            Self::Z => Quat::from_rotation_x(core::f32::consts::FRAC_PI_2),
        }
    }

    /// The other two axes, in a fixed order — the plane a box edge along this
    /// axis is positioned in.
    const fn others(self) -> (Self, Self) {
        match self {
            Self::X => (Self::Y, Self::Z),
            Self::Y => (Self::X, Self::Z),
            Self::Z => (Self::X, Self::Y),
        }
    }

    /// One component of a glam [`Vec3`] by axis.
    const fn of(self, vector: Vec3) -> f32 {
        match self {
            Self::X => vector.x,
            Self::Y => vector.y,
            Self::Z => vector.z,
        }
    }
}

/// One interactive handle of the rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GizmoPart {
    /// A translate arrow along an axis.
    TranslateAxis(GizmoAxis),
    /// A translate pad in the plane whose **normal** is the axis.
    TranslatePlane(GizmoAxis),
    /// A rotate ring about an axis.
    RotateRing(GizmoAxis),
    /// A stretch face handle on an axis; `true` is the positive side.
    ScaleFace(GizmoAxis, bool),
    /// A stretch corner handle; each `bool` is that axis' sign.
    ScaleCorner([bool; 3]),
}

/// Marks a handle entity with its part.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct GizmoHandle {
    /// Which part this handle is.
    part: GizmoPart,
}

/// Marks the rig root entity.
#[derive(Component, Debug)]
pub(crate) struct GizmoRoot;

/// One wireframe edge of the stretch tool's selection bounding box, keyed by
/// the axis it runs along and the signs of its position on the other two axes
/// — repositioned every frame as the live box changes.
#[derive(Component, Debug)]
pub(crate) struct GizmoBoxEdge {
    /// The axis the edge runs along.
    axis: GizmoAxis,
    /// Its sign on the first other axis ([`GizmoAxis::others`] order).
    sign_a: f32,
    /// Its sign on the second other axis.
    sign_b: f32,
}

/// The mutable scene-transform query the edit write-backs use, disjoint from
/// the rig root's own transform (`Without<GizmoRoot>`) so a system may read
/// the rig while writing object transforms without a B0001 access conflict.
pub(crate) type EditTransformQuery<'w, 's> =
    Query<'w, 's, &'static mut Transform, Without<GizmoRoot>>;

/// The rig's positionable children — stretch handles and box-wireframe edges
/// — as one query (a type alias per `clippy::type_complexity`), disjoint from
/// the rig root so [`place_gizmo_rig`] can move both.
type GizmoPartsQuery<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static GizmoHandle>,
        Option<&'static GizmoBoxEdge>,
        &'static mut Transform,
    ),
    (
        bevy::ecs::query::Or<(With<GizmoHandle>, With<GizmoBoxEdge>)>,
        Without<GizmoRoot>,
    ),
>;

/// Marks the gizmo overlay camera.
#[derive(Component, Debug)]
struct GizmoCamera;

/// The shared handle meshes and materials.
#[derive(Resource, Debug)]
pub(crate) struct GizmoAssets {
    /// The arrow shaft (a thin cylinder along Y).
    shaft: Handle<Mesh>,
    /// The arrow head (a cone along Y).
    cone: Handle<Mesh>,
    /// The planar pad (a thin square).
    pad: Handle<Mesh>,
    /// The rotate ring (a torus about Y).
    ring: Handle<Mesh>,
    /// The stretch handle cube.
    cube: Handle<Mesh>,
    /// A unit cuboid the snap-guide lines / ticks scale into shape.
    unit: Handle<Mesh>,
    /// Per-axis solid materials.
    axis: [Handle<StandardMaterial>; 3],
    /// Per-axis translucent pad materials.
    pad_axis: [Handle<StandardMaterial>; 3],
    /// The corner-handle material.
    corner: Handle<StandardMaterial>,
    /// The hover / active highlight material.
    hover: Handle<StandardMaterial>,
    /// The white snap-guide ruler material.
    snap_guide: Handle<StandardMaterial>,
}

impl FromWorld for GizmoAssets {
    /// Build the shared meshes and unlit materials once.
    fn from_world(world: &mut World) -> Self {
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        // Chunky like the reference's manipulators: thin shafts / rings are
        // hard to aim a cursor at, and the mesh IS the pick target.
        let shaft = meshes.add(Cylinder {
            radius: 0.035,
            half_height: 1.0,
        });
        let cone = meshes.add(Cone {
            radius: 0.10,
            height: 0.28,
        });
        let pad = meshes.add(Cuboid::new(0.24, 0.24, 0.02));
        let ring = meshes.add(Torus {
            minor_radius: 0.05,
            major_radius: 1.0,
        });
        let cube = meshes.add(Cuboid::new(0.11, 0.11, 0.11));
        let unit = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let mut materials = world.resource_mut::<Assets<StandardMaterial>>();
        let mut solid = |color: Color, alpha: bool| {
            materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                alpha_mode: if alpha {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Opaque
                },
                cull_mode: None,
                ..Default::default()
            })
        };
        let axis = [
            solid(GizmoAxis::X.color(), false),
            solid(GizmoAxis::Y.color(), false),
            solid(GizmoAxis::Z.color(), false),
        ];
        let pad_axis = [
            solid(GizmoAxis::X.color().with_alpha(0.45), true),
            solid(GizmoAxis::Y.color().with_alpha(0.45), true),
            solid(GizmoAxis::Z.color().with_alpha(0.45), true),
        ];
        let corner = solid(Color::srgba(0.9, 0.9, 0.9, 1.0), false);
        let hover = solid(Color::srgba(1.0, 0.85, 0.1, 1.0), false);
        let snap_guide = solid(Color::srgba(1.0, 1.0, 1.0, 0.85), true);
        Self {
            shaft,
            cone,
            pad,
            ring,
            cube,
            unit,
            axis,
            pad_axis,
            corner,
            hover,
            snap_guide,
        }
    }
}

/// Marks the root of the snap-guide ruler shown during an axis-translate drag
/// (the reference's `renderSnapGuides`): two white lines parallel to the drag
/// axis at the snap offset, with tick marks on the absolute world grid.
/// Despawned the moment no translate drag is live.
#[derive(Component, Debug)]
pub(crate) struct SnapGuideRoot;

/// The held-mark highlight on a snap guide: a brighter notch slid to the grid
/// mark / detent the drag currently holds ([`GizmoDrag::snap_progress`]),
/// hidden while the drag is free.
#[derive(Component, Debug)]
pub(crate) struct SnapGuideMarker {
    /// The marker's fixed perpendicular offset in the guide root's (Second
    /// Life) local space — a translate ruler line's side offset; zero on the
    /// rotate wheel.
    offset: Vec3,
}

/// Slide the snap-guide held-mark highlights to the grid mark / detent the
/// live drag holds, and hide them while it is free.
fn update_snap_guide_markers(
    interaction: Res<GizmoInteraction>,
    mut markers: Query<(&SnapGuideMarker, &mut Transform, &mut Visibility)>,
) {
    let Some(drag) = &interaction.drag else {
        // No drag: the guide (and its markers) despawn this frame anyway.
        return;
    };
    for (marker, mut transform, mut visibility) in &mut markers {
        match (drag.part, drag.snap_progress) {
            (GizmoPart::TranslateAxis(axis), Some(along)) => {
                let world_axis = drag.frame.mul_vec3(axis.unit());
                transform.translation = vadd(marker.offset, vscale(world_axis, along));
                *visibility = Visibility::Visible;
            }
            (GizmoPart::RotateRing(axis), Some(angle)) => {
                let (axis_a, axis_b) = ring_axes(drag.frame, axis);
                let radial = vadd(vscale(axis_a, angle.cos()), vscale(axis_b, angle.sin()));
                let normal = axis_a.cross(axis_b);
                transform.translation = vscale(radial, drag.snap_offset);
                transform.rotation = Quat::from_mat3(&bevy::math::Mat3::from_cols(
                    radial,
                    normal,
                    radial.cross(normal),
                ));
                *visibility = Visibility::Visible;
            }
            (GizmoPart::ScaleFace(axis, positive), Some(along)) => {
                let sign = if positive { 1.0 } else { -1.0 };
                let dir = vscale(drag.frame.mul_vec3(axis.unit()), sign);
                transform.translation = vadd(marker.offset, vscale(dir, along));
                *visibility = Visibility::Visible;
            }
            (GizmoPart::ScaleCorner(signs), Some(along)) => {
                let dir = drag.corner_dir(signs);
                transform.translation = vadd(marker.offset, vscale(dir, along));
                *visibility = Visibility::Visible;
            }
            _free => {
                *visibility = Visibility::Hidden;
            }
        }
    }
}

/// Where the snap-guide ruler sits, in **rig units** (multiplied by the rig's
/// constant-screen-size scale into metres): the off-axis distance the cursor
/// must cross to enter the snap regime — the reference's `mSnapOffsetMeters`
/// (`SnapMargin` scaled to the arrow length).
const SNAP_GUIDE_OFFSET_RIG: f32 = 0.45;

/// Where the rotate snap-guide tick circle sits, in rig units (the ring's
/// major radius is 1): the in-plane cursor distance beyond which a ring drag
/// enters the snap regime — the reference's `SNAP_GUIDE_INNER_RADIUS` band
/// outside the ring.
const ROTATE_SNAP_GUIDE_RADIUS_RIG: f32 = 1.35;

/// The corner (uniform) stretch's snapped-factor step: quarter multiples of
/// the starting size, with the guide marking the ×0.5 multiples and ×1.0
/// specially.
const CORNER_FACTOR_STEP: f32 = 0.25;

/// Spawn the snap-guide ruler for an axis drag: two lines parallel to the
/// (world) `axis` through the pivot, offset by ±`snap_offset` along `perp`,
/// each carrying tick marks at absolute-grid multiples of `grid_unit`.
/// Everything is world-sized (not rig-scaled), so the ticks measure real
/// metres.
#[expect(
    clippy::too_many_arguments,
    reason = "the ruler is built from the drag's full geometry: pivots in both spaces, the two \
              in-plane directions, the snap offset and the grid unit"
)]
fn spawn_snap_guide(
    commands: &mut Commands,
    assets: &GizmoAssets,
    pivot_bevy: Vec3,
    pivot_sl: Vec3,
    axis_world_sl: Vec3,
    perp_world_sl: Vec3,
    snap_offset: f32,
    grid_unit: f32,
) {
    let root = commands
        .spawn((
            SnapGuideRoot,
            // World-positioned, axis-aligned via the two direction vectors
            // below; the root itself carries only the pivot and the Second
            // Life → Bevy basis change (directions are given in Second Life
            // space).
            Transform {
                translation: pivot_bevy,
                rotation: sl_to_bevy_rotation(),
                scale: Vec3::ONE,
            },
            Visibility::default(),
            Propagate(RenderLayers::layer(EDIT_GIZMO_RENDER_LAYER)),
            Name::new("edit-gizmo:snap-guide"),
        ))
        .id();
    // Orientation carrying the unit cuboid's local Y onto the axis, X onto
    // the perpendicular (in the root's Second Life local space).
    let orientation = Quat::from_mat3(&bevy::math::Mat3::from_cols(
        perp_world_sl,
        axis_world_sl,
        perp_world_sl.cross(axis_world_sl),
    ));
    let half_span = (grid_unit * 12.0).clamp(3.0, 48.0);
    // Ticks sit on the ABSOLUTE world grid along the axis (the same grid the
    // drag snaps to): the offset from the pivot to its nearest grid multiple
    // anchors the ladder.
    let pivot_coord = pivot_sl.dot(axis_world_sl);
    let base = snap_to_grid(pivot_coord, grid_unit) - pivot_coord;
    let step = grid_unit.max(0.01);
    for side in [-1.0_f32, 1.0_f32] {
        let offset = vscale(perp_world_sl, snap_offset * side);
        // The guide line.
        commands.spawn((
            Mesh3d(assets.unit.clone()),
            MeshMaterial3d(assets.snap_guide.clone()),
            Transform {
                translation: offset,
                rotation: orientation,
                scale: Vec3::new(0.012, half_span * 2.0, 0.012),
            },
            ChildOf(root),
        ));
        // The tick marks, every grid unit on the absolute grid, graded like a
        // tape measure: every 10th grid mark longest, every 5th medium, the
        // rest short.
        let mut along = base - half_span;
        while along <= half_span {
            let grid_index = ((pivot_coord + along) / step).round();
            let tenth = (grid_index / 10.0).round() * 10.0;
            let fifth = (grid_index / 5.0).round() * 5.0;
            let length = if (grid_index - tenth).abs() < 0.25 {
                0.17
            } else if (grid_index - fifth).abs() < 0.25 {
                0.12
            } else {
                0.07
            };
            commands.spawn((
                Mesh3d(assets.unit.clone()),
                MeshMaterial3d(assets.snap_guide.clone()),
                Transform {
                    translation: vadd(offset, vscale(axis_world_sl, along)),
                    rotation: orientation,
                    scale: Vec3::new(length, 0.012, 0.012),
                },
                ChildOf(root),
            ));
            along += step;
        }
        // The held-mark highlight: slides to the grid mark the drag holds,
        // hidden while the drag is free.
        commands.spawn((
            SnapGuideMarker { offset },
            Mesh3d(assets.unit.clone()),
            MeshMaterial3d(assets.hover.clone()),
            Transform {
                translation: offset,
                rotation: orientation,
                scale: Vec3::new(0.24, 0.05, 0.05),
            },
            Visibility::Hidden,
            ChildOf(root),
        ));
    }
}

/// Spawn the rotate snap-guide: a circle of tick marks in the ring's plane at
/// the snap radius, one per 5.625° detent (a longer tick every 22.5°) — the
/// reference's rotation snap guides. Crossing the circle with the cursor
/// engages the angle detents.
fn spawn_rotate_snap_guide(
    commands: &mut Commands,
    assets: &GizmoAssets,
    pivot_bevy: Vec3,
    axis_a_sl: Vec3,
    axis_b_sl: Vec3,
    radius: f32,
    anchor: f32,
) {
    let root = commands
        .spawn((
            SnapGuideRoot,
            Transform {
                translation: pivot_bevy,
                rotation: sl_to_bevy_rotation(),
                scale: Vec3::ONE,
            },
            Visibility::default(),
            Propagate(RenderLayers::layer(EDIT_GIZMO_RENDER_LAYER)),
            Name::new("edit-gizmo:rotate-snap-guide"),
        ))
        .id();
    let detents = 64_u32;
    for index in 0..detents {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "a detent index (< 64) is exactly representable in f32"
        )]
        // `anchor` places tick 0 where the held-mark lands when the object's
        // absolute twist about the axis is 0° — so the graded cardinals mean
        // the OBJECT standing at 0° / 90° / 180° / 270° in the grid frame,
        // repeatable across drags whatever the grab point.
        let angle = anchor + (index as f32) * SNAP_ANGLE_DEG.to_radians();
        let radial = vadd(
            vscale(axis_a_sl, angle.cos()),
            vscale(axis_b_sl, angle.sin()),
        );
        // Graded like the translate ruler: the ±90° / 180° cardinals (from
        // the grab) longest (and thicker, so they pop), the 22.5°
        // quarter-detents medium, plain detents short.
        let length = if index % 16 == 0 {
            0.22
        } else if index % 4 == 0 {
            0.12
        } else {
            0.06
        };
        let girth = if index % 16 == 0 { 0.03 } else { 0.014 };
        let orientation = Quat::from_mat3(&bevy::math::Mat3::from_cols(
            radial,
            axis_a_sl.cross(axis_b_sl),
            radial.cross(axis_a_sl.cross(axis_b_sl)),
        ));
        commands.spawn((
            Mesh3d(assets.unit.clone()),
            MeshMaterial3d(assets.snap_guide.clone()),
            Transform {
                translation: vscale(radial, radius),
                rotation: orientation,
                scale: Vec3::new(length, girth, girth),
            },
            ChildOf(root),
        ));
    }
    // The held-detent highlight: slides around the circle to the detent the
    // drag holds, hidden while the rotation is free.
    commands.spawn((
        SnapGuideMarker { offset: Vec3::ZERO },
        Mesh3d(assets.unit.clone()),
        MeshMaterial3d(assets.hover.clone()),
        Transform::from_scale(Vec3::new(0.28, 0.05, 0.05)),
        Visibility::Hidden,
        ChildOf(root),
    ));
}

/// One tick of a scale snap-guide ruler: its position along the drag line
/// (line-param metres from the pivot) and its size.
struct ScaleTick {
    /// Position along the drag direction, in metres from the pivot.
    along: f32,
    /// Tick length (across the line), in metres.
    length: f32,
    /// Tick girth, in metres.
    girth: f32,
}

/// The tick ladder for a **face** (single-axis) stretch: the absolute size
/// grid of the primary's affected LOCAL axis (tape-measure graded like the
/// translate ruler) plus the reference's scale-factor marks — ×0.5 multiples
/// of the starting size medium-large, ×1.0 (the original size) largest.
/// `alignment` maps local-extent space onto the (possibly world-frame) drag
/// line: one metre of local size is `alignment` metres of cursor travel.
fn face_scale_ticks(
    start_param: f32,
    start_extent: f32,
    grid_unit: f32,
    alignment: f32,
) -> Vec<ScaleTick> {
    let step = grid_unit.max(0.01);
    let alignment = alignment.max(1.0e-4);
    let reach = (start_extent * 2.0).max(step * 8.0).clamp(1.0, 48.0);
    let mut ticks = Vec::new();
    // The absolute size grid: extent e = k·grid, positioned at
    // start_param + (e - start_extent) · alignment.
    let mut extent = (((start_extent - reach) / step).ceil() * step).max(step);
    let mut guard = 0_u32;
    while extent <= start_extent + reach && guard < 400 {
        guard = guard.saturating_add(1);
        let grid_index = (extent / step).round();
        let tenth = (grid_index / 10.0).round() * 10.0;
        let fifth = (grid_index / 5.0).round() * 5.0;
        let (length, girth) = if (grid_index - tenth).abs() < 0.25 {
            (0.17, 0.012)
        } else if (grid_index - fifth).abs() < 0.25 {
            (0.12, 0.012)
        } else {
            (0.07, 0.012)
        };
        ticks.push(ScaleTick {
            along: start_param + (extent - start_extent) * alignment,
            length,
            girth,
        });
        extent += step;
    }
    // The factor marks: ×0.5 multiples of the starting size, ×1.0 largest.
    for half_steps in 1_u32..=8_u32 {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "a half-step count (≤ 8) is exactly representable in f32"
        )]
        let factor = (half_steps as f32) * 0.5;
        let extent = start_extent * factor;
        if (extent - start_extent).abs() <= reach {
            let one = (factor - 1.0).abs() < 1.0e-3;
            ticks.push(ScaleTick {
                along: start_param + (extent - start_extent) * alignment,
                length: if one { 0.30 } else { 0.20 },
                girth: if one { 0.035 } else { 0.022 },
            });
        }
    }
    ticks
}

/// The tick ladder for a **corner** (uniform) stretch: pure factor space —
/// quarter-step ticks, ×0.5 multiples medium, whole multiples long, ×1.0 (the
/// original size) largest. With stretch-both-sides OFF the cursor mapping is
/// halved (the reference's `0.5 + t/2`), so a factor `f` sits at cursor param
/// `(f − 0.5) · 2 · start_param` instead of `f · start_param`.
fn corner_scale_ticks(start_param: f32, stretch_both: bool) -> Vec<ScaleTick> {
    let mut ticks = Vec::new();
    for quarter_steps in 1_u32..=12_u32 {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "a quarter-step count (≤ 12) is exactly representable in f32"
        )]
        let factor = (quarter_steps as f32) * CORNER_FACTOR_STEP;
        let one = (factor - 1.0).abs() < 1.0e-3;
        let whole = (factor - factor.round()).abs() < 1.0e-3;
        let half = (factor * 2.0 - (factor * 2.0).round()).abs() < 1.0e-3;
        let (length, girth) = if one {
            (0.30, 0.04)
        } else if whole {
            (0.22, 0.03)
        } else if half {
            (0.13, 0.014)
        } else {
            (0.07, 0.012)
        };
        let ratio = if stretch_both {
            factor
        } else {
            (factor - 0.5) * 2.0
        };
        // A tick behind the pivot (a shrunken far-side factor with the
        // halved mapping) would sit inside the object; skip it.
        if ratio > 0.05 {
            ticks.push(ScaleTick {
                along: ratio * start_param,
                length,
                girth,
            });
        }
    }
    ticks
}

/// Spawn a scale snap-guide ruler: two lines along the (world) drag direction
/// `dir` through the pivot, offset by ±`snap_offset` along `perp`, carrying
/// the given tick ladder, plus the sliding held-mark highlights.
fn spawn_scale_snap_guide(
    commands: &mut Commands,
    assets: &GizmoAssets,
    pivot_bevy: Vec3,
    dir_sl: Vec3,
    perp_sl: Vec3,
    snap_offset: f32,
    ticks: &[ScaleTick],
) {
    let root = commands
        .spawn((
            SnapGuideRoot,
            Transform {
                translation: pivot_bevy,
                rotation: sl_to_bevy_rotation(),
                scale: Vec3::ONE,
            },
            Visibility::default(),
            Propagate(RenderLayers::layer(EDIT_GIZMO_RENDER_LAYER)),
            Name::new("edit-gizmo:scale-snap-guide"),
        ))
        .id();
    let orientation = Quat::from_mat3(&bevy::math::Mat3::from_cols(
        perp_sl,
        dir_sl,
        perp_sl.cross(dir_sl),
    ));
    let (lo, hi) = ticks.iter().fold((0.0_f32, 0.0_f32), |(lo, hi), tick| {
        (lo.min(tick.along), hi.max(tick.along))
    });
    let mid = (lo + hi) * 0.5;
    let span = (hi - lo).max(0.5) + 0.3;
    for side in [-1.0_f32, 1.0_f32] {
        let offset = vscale(perp_sl, snap_offset * side);
        commands.spawn((
            Mesh3d(assets.unit.clone()),
            MeshMaterial3d(assets.snap_guide.clone()),
            Transform {
                translation: vadd(offset, vscale(dir_sl, mid)),
                rotation: orientation,
                scale: Vec3::new(0.012, span, 0.012),
            },
            ChildOf(root),
        ));
        for tick in ticks {
            commands.spawn((
                Mesh3d(assets.unit.clone()),
                MeshMaterial3d(assets.snap_guide.clone()),
                Transform {
                    translation: vadd(offset, vscale(dir_sl, tick.along)),
                    rotation: orientation,
                    scale: Vec3::new(tick.length, tick.girth, tick.girth),
                },
                ChildOf(root),
            ));
        }
        // The held-mark highlight (slid by `update_snap_guide_markers`).
        commands.spawn((
            SnapGuideMarker { offset },
            Mesh3d(assets.unit.clone()),
            MeshMaterial3d(assets.hover.clone()),
            Transform {
                translation: offset,
                rotation: orientation,
                scale: Vec3::new(0.24, 0.05, 0.05),
            },
            Visibility::Hidden,
            ChildOf(root),
        ));
    }
}

/// The screen-space live-value read-out shown beside the gizmo during a drag
/// (the reference's in-viewport position / degrees / size display), spawned
/// lazily and hidden between drags.
#[derive(Resource, Debug, Default)]
struct GizmoReadoutUi {
    /// The read-out's UI node, once spawned.
    node: Option<Entity>,
    /// Its text child.
    text: Option<Entity>,
}

/// Show the live drag value beside the gizmo: project the selection pivot to
/// the viewport and park a small label under it, filled from
/// [`GizmoDrag::readout`].
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the drag state, \
              the lazily spawned label's bookkeeping, the camera / window to project with, and \
              the node / text / visibility queries that place it"
)]
fn update_gizmo_readout(
    interaction: Res<GizmoInteraction>,
    mut ui: ResMut<GizmoReadoutUi>,
    root: Option<Res<crate::ui::UiRoot>>,
    cameras: Query<(&Camera, &GlobalTransform), With<ViewerCamera>>,
    mut nodes: Query<&mut Node>,
    mut texts: Query<&mut Text>,
    mut visibilities: Query<&mut Visibility>,
    mut commands: Commands,
) {
    let shown = interaction
        .drag
        .as_ref()
        .filter(|drag| !drag.readout.is_empty());
    let Some(drag) = shown else {
        if let Some(node) = ui.node
            && let Ok(mut visibility) = visibilities.get_mut(node)
        {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    let Ok(at) = camera.world_to_viewport(camera_transform, sl_to_bevy_vec(&vec3_sl(drag.pivot)))
    else {
        return;
    };
    // Lazily spawn the label on first use.
    if ui.node.is_none() {
        let Some(root) = root.map(|root| root.0) else {
            return;
        };
        let node = commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                    ..Default::default()
                },
                BackgroundColor(Color::srgba(0.05, 0.05, 0.08, 0.8)),
                Pickable::IGNORE,
                Visibility::Hidden,
                Name::new("edit-gizmo:readout"),
                ChildOf(root),
            ))
            .id();
        let text = commands
            .spawn((
                Text::default(),
                crate::ui_font::UiFont::Mono.at(13.0),
                TextColor(Color::srgba(1.0, 0.92, 0.5, 1.0)),
                Pickable::IGNORE,
                ChildOf(node),
            ))
            .id();
        ui.node = Some(node);
        ui.text = Some(text);
        return;
    }
    if let Some(node) = ui.node
        && let Ok(mut layout) = nodes.get_mut(node)
    {
        // Park the label a little under the pivot so it never hides the
        // handle being dragged.
        layout.left = Val::Px(at.x + 18.0);
        layout.top = Val::Px(at.y + 26.0);
    }
    if let Some(node) = ui.node
        && let Ok(mut visibility) = visibilities.get_mut(node)
    {
        *visibility = Visibility::Visible;
    }
    if let Some(text) = ui.text
        && let Ok(mut value) = texts.get_mut(text)
        && value.0 != drag.readout
    {
        value.0.clone_from(&drag.readout);
    }
}

/// One dragged object's start-of-drag snapshot.
#[derive(Debug, Clone)]
struct DragObject {
    /// The object's region-scoped id (the update's address).
    scoped: ScopedObjectId,
    /// Its scene entity.
    entity: Entity,
    /// Its geometry-holder entity, for the live scale echo.
    geometry: Option<Entity>,
    /// Whether it is a linkset root (wire frame = region) or a linked part
    /// (wire frame = parent-relative).
    is_root: bool,
    /// Its parent entity when a linked part, to fold a world edit back into
    /// the parent frame.
    parent: Option<Entity>,
    /// Start scale.
    start_scale: Vector,
    /// Start world position, Second Life space.
    start_world_pos: Vec3,
    /// Start world rotation, Second Life space.
    start_world_rot: Quat,
}

/// A live drag.
#[derive(Debug, Clone)]
struct GizmoDrag {
    /// The dragged part.
    part: GizmoPart,
    /// The selection pivot at drag start (Second Life space).
    pivot: Vec3,
    /// The grid frame at drag start (Second Life space rotation).
    frame: Quat,
    /// The drag plane's normal (translate / rotate), Second Life space.
    plane_normal: Vec3,
    /// The plane hit at drag start (translate), Second Life space.
    start_hit: Vec3,
    /// The handle-line parameter at drag start (stretch).
    start_param: f32,
    /// The ring angle at drag start (rotate), radians.
    start_angle: f32,
    /// The primary object's **absolute twist** about the ring axis at drag
    /// start (rotate), radians — what the detents quantise, so a snapped
    /// rotation always lands the object on repeatable orientations
    /// (0° / 90° / … in the grid frame) regardless of where the ring was
    /// grabbed.
    start_twist: f32,
    /// The raw ring angle seen last frame (rotate), for the per-step wrap.
    last_ring_angle: f32,
    /// The continuous, wrap-free rotation accumulated since drag start
    /// (rotate), radians — what the detents quantise and the read-out shows,
    /// so the value grows and shrinks linearly across the ±180° seam.
    accumulated_angle: f32,
    /// The off-axis cursor distance (metres, in the drag plane) beyond which
    /// an axis-translate drag enters the **snap regime** — the reference's
    /// `mSnapOffsetMeters`, where the white snap-guide ruler sits. For a ring
    /// drag it is instead the in-plane radius of the detent tick circle.
    snap_offset: f32,
    /// While a translate drag is snapped: the pivot-relative distance along
    /// the drag axis of the grid mark currently held, so the guide can
    /// highlight it. `None` while free.
    snap_progress: Option<f32>,
    /// The live value read-out shown beside the gizmo while dragging — the
    /// reference's in-viewport position / degrees / size display. Rebuilt
    /// each drag frame; empty until the first movement.
    readout: String,
    /// The per-object snapshots.
    objects: Vec<DragObject>,
    /// Whether any motion has been applied (a no-op drag sends nothing).
    moved: bool,
    /// Seconds (app time) of the last streamed update (stretch only).
    last_stream: f32,
    /// The selection bounding box's half-extents along the grid-frame axes at
    /// drag start (stretch only; [`Vec3::ONE`] otherwise) — the corner
    /// handles' diagonal runs through the actual box corner.
    bbox_ext: Vec3,
    /// The **display** box while a stretch drag is live: `(world centre,
    /// frame-space half-extents)`, derived from the start box plus the drag —
    /// a face drag changes it on the dragged axis alone (grabbed side follows
    /// the cursor; the opposite side pinned, or mirrored with
    /// stretch-both-sides) instead of re-fitting an AABB whose extents would
    /// couple on a rotated object.
    live_box: Option<(Vec3, Vec3)>,
}

impl GizmoDrag {
    /// The primary (last-selected) object's world rotation at drag start —
    /// what a face stretch folds through onto a local scale axis.
    fn primary_rot(&self) -> Quat {
        self.objects
            .last()
            .map_or(Quat::IDENTITY, |object| object.start_world_rot)
    }

    /// A corner handle's drag direction: through the actual bounding-box
    /// corner (the extents-weighted diagonal), in Second Life world space —
    /// the reference's `mScaleDir = box_corner - center`.
    fn corner_dir(&self, signs: [bool; 3]) -> Vec3 {
        let [x, y, z] = signs;
        let signed = Vec3::new(
            if x { self.bbox_ext.x } else { -self.bbox_ext.x },
            if y { self.bbox_ext.y } else { -self.bbox_ext.y },
            if z { self.bbox_ext.z } else { -self.bbox_ext.z },
        );
        self.frame.mul_vec3(signed.normalize_or_zero())
    }
}

/// The bounding box of a set of oriented boxes `(world position, world
/// rotation, size)` — all Second Life space — in the grid `frame`: its world
/// centre and half-extents along the frame axes. The reference's
/// `getBBoxOfSelection` the stretch handles mount on.
fn bbox_from_parts(
    parts: impl Iterator<Item = (Vec3, Quat, Vec3)>,
    frame: Quat,
) -> Option<(Vec3, Vec3)> {
    let inverse = frame.inverse();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for (position, rotation, size) in parts {
        let half = vscale(size, 0.5);
        for index in 0_u8..8_u8 {
            let corner = Vec3::new(
                if index & 1 == 0 { -half.x } else { half.x },
                if index & 2 == 0 { -half.y } else { half.y },
                if index & 4 == 0 { -half.z } else { half.z },
            );
            let world = vadd(position, rotation.mul_vec3(corner));
            let framed = inverse.mul_vec3(world);
            min = min.min(framed);
            max = max.max(framed);
        }
    }
    if !min.x.is_finite() || !max.x.is_finite() {
        return None;
    }
    let center = frame.mul_vec3(vscale(vadd(min, max), 0.5));
    let ext = vscale(vsub(max, min), 0.5).max(Vec3::splat(0.01));
    Some((center, ext))
}

/// [`bbox_from_parts`] over a drag's start-of-drag snapshots.
fn bbox_from_snapshots(objects: &[DragObject], frame: Quat) -> Option<(Vec3, Vec3)> {
    bbox_from_parts(
        objects.iter().map(|object| {
            (
                object.start_world_pos,
                object.start_world_rot,
                Vec3::new(
                    object.start_scale.x,
                    object.start_scale.y,
                    object.start_scale.z,
                ),
            )
        }),
        frame,
    )
}

/// [`bbox_from_parts`] over the LIVE selection (entity globals + the
/// [`ObjectSlMotion`] mirror), for the rig placement between drags.
fn bbox_live(
    selection: &SelectionSet,
    frame: Quat,
    globals: &Query<&GlobalTransform>,
    motions: &Query<&ObjectSlMotion>,
) -> Option<(Vec3, Vec3)> {
    bbox_from_parts(
        selection.iter().filter_map(|node| {
            let global = globals.get(node.entity).ok()?;
            let motion = motions.get(node.entity).ok()?;
            Some((
                sl_vec3(bevy_to_sl_vec(global.translation())),
                sl_world_rotation(global.rotation()),
                Vec3::new(motion.scale.x, motion.scale.y, motion.scale.z),
            ))
        }),
        frame,
    )
}

/// The gizmo pointer state the selection click defers to.
#[derive(Resource, Debug, Default)]
pub(crate) struct GizmoInteraction {
    /// The handle under the cursor, if any.
    hovered: Option<GizmoPart>,
    /// The live drag, if any.
    drag: Option<GizmoDrag>,
}

impl GizmoInteraction {
    /// Whether the gizmo owns the pointer (hovering a handle or dragging) —
    /// the guard that keeps a manipulator press from doubling as a selection
    /// click.
    pub(crate) const fn claims_pointer(&self) -> bool {
        self.hovered.is_some() || self.drag.is_some()
    }
}

/// Which tool the currently spawned rig was built for (so a tool change
/// rebuilds it).
#[derive(Resource, Debug, Default)]
struct BuiltRig {
    /// The spawned rig root and its tool, if a rig exists.
    current: Option<(Entity, EditTool)>,
}

/// The plugin wiring the transform gizmos into the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct EditGizmoPlugin;

impl Plugin for EditGizmoPlugin {
    /// Register the gizmo resources and systems.
    fn build(&self, app: &mut App) {
        app.init_resource::<GizmoAssets>()
            .init_resource::<GizmoInteraction>()
            .init_resource::<BuiltRig>()
            .init_resource::<GizmoReadoutUi>()
            .add_systems(
                Update,
                (
                    spawn_gizmo_camera,
                    maintain_gizmo_rig,
                    drive_gizmo_interaction,
                    update_snap_guide_markers,
                    update_gizmo_readout,
                    place_gizmo_rig,
                    tint_gizmo_handles,
                )
                    .chain(),
            );
    }
}

/// The main viewer camera's entity + projection, excluding the overlay camera
/// itself (a type alias for the query, per `clippy::type_complexity`).
type MainCameraQuery<'w, 's> =
    Query<'w, 's, (Entity, &'static Projection), (With<ViewerCamera>, Without<GizmoCamera>)>;

/// The main viewer camera's projection, only when it changed this frame.
type ChangedMainProjectionQuery<'w, 's> = Query<
    'w,
    's,
    &'static Projection,
    (
        With<ViewerCamera>,
        Without<GizmoCamera>,
        Changed<Projection>,
    ),
>;

/// Spawn the gizmo overlay camera as a child of the viewer camera, once it
/// exists: a perspective camera drawing only the gizmo layer, composited over
/// the world (order 1, between the world's 0 and the HUD's 2) without
/// clearing it.
fn spawn_gizmo_camera(
    cameras: MainCameraQuery,
    existing: Query<&GizmoCamera>,
    mut commands: Commands,
) {
    if !existing.is_empty() {
        return;
    }
    let Ok((camera, projection)) = cameras.single() else {
        return;
    };
    let overlay = commands
        .spawn((
            GizmoCamera,
            Camera3d::default(),
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..Default::default()
            },
            projection.clone(),
            Transform::default(),
            RenderLayers::layer(EDIT_GIZMO_RENDER_LAYER),
            // Must match the world camera's sample count and HDR-ness — this
            // camera draws into the frame the world camera left there.
            Msaa::Sample4,
            Hdr,
            Tonemapping::None,
            ChildOf(camera),
        ))
        .id();
    debug!("gizmos: spawned overlay camera {overlay:?}");
}

/// Spawn / despawn / rebuild the rig to match the tool state and selection,
/// and keep the overlay camera's projection identical to the main camera's so
/// gizmo picking (main-camera rays) and gizmo rendering agree.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection state the rig mirrors, the shared assets, the rig / interaction \
              bookkeeping, and the two camera queries the projection mirror reads"
)]
fn maintain_gizmo_rig(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    assets: Res<GizmoAssets>,
    mut built: ResMut<BuiltRig>,
    mut interaction: ResMut<GizmoInteraction>,
    main_camera: ChangedMainProjectionQuery,
    mut overlay_camera: Query<&mut Projection, With<GizmoCamera>>,
    mut commands: Commands,
) {
    // Mirror a main-camera projection change onto the overlay camera.
    if let (Ok(main), Ok(mut overlay)) = (main_camera.single(), overlay_camera.single_mut()) {
        *overlay = main.clone();
    }

    // Never rebuild the rig under a live drag: a mid-drag modifier change
    // (the `Ctrl` / `Ctrl+Shift` tool chords) must not despawn the handle
    // being dragged — the reference likewise keeps the active manipulation.
    if interaction.drag.is_some() {
        return;
    }
    let want = (tool.active && !selection.is_empty()).then_some(tool.effective_tool());
    let up_to_date = match (built.current, want) {
        (None, None) => true,
        (Some((_root, current)), Some(target)) => current == target,
        _mismatch => false,
    };
    if up_to_date {
        return;
    }
    if let Some((root, _tool)) = built.current.take() {
        commands.entity(root).despawn();
    }
    interaction.hovered = None;
    interaction.drag = None;
    if let Some(target) = want {
        let root = spawn_rig(&mut commands, &assets, target);
        built.current = Some((root, target));
    }
}

/// Spawn the rig root and the handles for `tool`.
fn spawn_rig(commands: &mut Commands, assets: &GizmoAssets, tool: EditTool) -> Entity {
    let root = commands
        .spawn((
            GizmoRoot,
            Transform::default(),
            Visibility::default(),
            Propagate(RenderLayers::layer(EDIT_GIZMO_RENDER_LAYER)),
            Name::new("edit-gizmo"),
        ))
        .id();
    let handle = |part: GizmoPart,
                  mesh: &Handle<Mesh>,
                  material: &Handle<StandardMaterial>,
                  transform: Transform,
                  commands: &mut Commands| {
        commands.spawn((
            GizmoHandle { part },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            transform,
            ChildOf(root),
        ));
    };
    match tool {
        EditTool::Move => {
            for axis in GizmoAxis::ALL {
                let material = assets.axis.get(axis.index()).unwrap_or(&assets.corner);
                let orientation = axis.orientation();
                handle(
                    GizmoPart::TranslateAxis(axis),
                    &assets.shaft,
                    material,
                    Transform::from_rotation(orientation),
                    commands,
                );
                for positive in [true, false] {
                    let sign = if positive { 1.0 } else { -1.0 };
                    let flip = if positive {
                        Quat::IDENTITY
                    } else {
                        Quat::from_rotation_z(core::f32::consts::PI)
                    };
                    handle(
                        GizmoPart::TranslateAxis(axis),
                        &assets.cone,
                        material,
                        Transform {
                            translation: vscale(axis.unit(), 1.06 * sign),
                            rotation: orientation.mul_quat(flip),
                            scale: Vec3::ONE,
                        },
                        commands,
                    );
                }
                // The pad in the plane whose normal is this axis, offset into
                // the plane's quadrant.
                let pad_material = assets.pad_axis.get(axis.index()).unwrap_or(&assets.corner);
                let (pad_rotation, pad_offset) = match axis {
                    GizmoAxis::X => (
                        Quat::from_rotation_y(core::f32::consts::FRAC_PI_2),
                        Vec3::new(0.0, 0.42, 0.42),
                    ),
                    GizmoAxis::Y => (
                        Quat::from_rotation_x(core::f32::consts::FRAC_PI_2),
                        Vec3::new(0.42, 0.0, 0.42),
                    ),
                    GizmoAxis::Z => (Quat::IDENTITY, Vec3::new(0.42, 0.42, 0.0)),
                };
                handle(
                    GizmoPart::TranslatePlane(axis),
                    &assets.pad,
                    pad_material,
                    Transform {
                        translation: pad_offset,
                        rotation: pad_rotation,
                        scale: Vec3::ONE,
                    },
                    commands,
                );
            }
        }
        EditTool::Rotate => {
            for axis in GizmoAxis::ALL {
                let material = assets.axis.get(axis.index()).unwrap_or(&assets.corner);
                // The torus is authored about Bevy Y; carry Y onto the axis.
                handle(
                    GizmoPart::RotateRing(axis),
                    &assets.ring,
                    material,
                    Transform::from_rotation(axis.orientation()),
                    commands,
                );
            }
        }
        EditTool::Stretch => {
            for axis in GizmoAxis::ALL {
                let material = assets.axis.get(axis.index()).unwrap_or(&assets.corner);
                for positive in [true, false] {
                    let sign = if positive { 1.0 } else { -1.0 };
                    handle(
                        GizmoPart::ScaleFace(axis, positive),
                        &assets.cube,
                        material,
                        Transform::from_translation(vscale(axis.unit(), 1.0 * sign)),
                        commands,
                    );
                }
            }
            for signs in [
                [false, false, false],
                [true, false, false],
                [false, true, false],
                [false, false, true],
                [true, true, false],
                [true, false, true],
                [false, true, true],
                [true, true, true],
            ] {
                let offset = corner_direction(signs);
                handle(
                    GizmoPart::ScaleCorner(signs),
                    &assets.cube,
                    &assets.corner,
                    Transform::from_translation(vscale(offset, 0.62)),
                    commands,
                );
            }
            // The selection bounding box's wireframe — the box the handles
            // mount on (the reference's stretch-tool box), repositioned live
            // by `place_gizmo_rig`.
            for axis in GizmoAxis::ALL {
                for sign_a in [-1.0_f32, 1.0_f32] {
                    for sign_b in [-1.0_f32, 1.0_f32] {
                        commands.spawn((
                            GizmoBoxEdge {
                                axis,
                                sign_a,
                                sign_b,
                            },
                            Mesh3d(assets.unit.clone()),
                            MeshMaterial3d(assets.snap_guide.clone()),
                            Transform::default(),
                            ChildOf(root),
                        ));
                    }
                }
            }
        }
    }
    root
}

/// A corner's unit direction in the rig frame from its per-axis signs.
fn corner_direction(signs: [bool; 3]) -> Vec3 {
    let [x, y, z] = signs;
    Vec3::new(
        if x { 1.0 } else { -1.0 },
        if y { 1.0 } else { -1.0 },
        if z { 1.0 } else { -1.0 },
    )
    .normalize()
}

/// The selection pivot in Bevy world space: the mean of the selected
/// entities' translations.
fn selection_pivot_bevy(
    selection: &SelectionSet,
    globals: &Query<&GlobalTransform>,
) -> Option<Vec3> {
    let mut sum = Vec3::ZERO;
    let mut count = 0.0_f32;
    for node in selection.iter() {
        if let Ok(global) = globals.get(node.entity) {
            sum = vadd(sum, global.translation());
            count += 1.0;
        }
    }
    (count > 0.0).then(|| vscale(sum, 1.0 / count))
}

/// The grid frame's rotation in Second Life space for the current settings:
/// world = identity, local = the primary selection's world rotation,
/// reference = world until the grid-options task supplies a reference object.
fn grid_frame_rotation(
    tool: &EditToolState,
    selection: &SelectionSet,
    globals: &Query<&GlobalTransform>,
) -> Quat {
    match tool.frame {
        GridFrame::World | GridFrame::Reference => Quat::IDENTITY,
        GridFrame::Local => selection
            .primary()
            .and_then(|node| globals.get(node.entity).ok())
            .map_or(Quat::IDENTITY, |global| {
                sl_world_rotation(global.rotation())
            }),
    }
}

// (The stretch box is aligned to the GRID frame — world axes in world mode,
// the primary's axes in local mode — like every other manipulator; a face
// drag changes the box on that axis alone, with the display box carried in
// [`GizmoDrag::live_box`] during the drag.)

/// A Bevy world rotation as the equivalent Second Life world rotation (strip
/// the basis change off the left).
fn sl_world_rotation(bevy_rotation: Quat) -> Quat {
    sl_to_bevy_rotation().inverse().mul_quat(bevy_rotation)
}

/// Place the rig at the live selection pivot, grid-frame orientation, and
/// constant-screen-size scale.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection state, the camera / window for the constant-screen scale, the rig root, \
              and the handle / box-edge children the stretch tool repositions live"
)]
fn place_gizmo_rig(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    interaction: Res<GizmoInteraction>,
    globals: Query<&GlobalTransform>,
    motions: Query<&ObjectSlMotion>,
    cameras: Query<(&GlobalTransform, &Projection), With<ViewerCamera>>,
    windows: Query<&Window>,
    mut rigs: Query<&mut Transform, With<GizmoRoot>>,
    mut parts: GizmoPartsQuery,
) {
    let Ok(mut rig) = rigs.single_mut() else {
        return;
    };
    let Some(pivot) = selection_pivot_bevy(&selection, &globals) else {
        return;
    };
    let frame = grid_frame_rotation(&tool, &selection, &globals);
    let Ok((camera_transform, projection)) = cameras.single() else {
        return;
    };
    let fov = match projection {
        Projection::Perspective(perspective) => perspective.fov,
        _other => core::f32::consts::FRAC_PI_4,
    };
    let height = windows.single().map_or(720.0, bevy::window::Window::height);

    // The stretch rig mounts on the live selection BOUNDING BOX (world units,
    // the reference's stretch-tool box): a single object's own oriented box,
    // or the grid-aligned combined box of a multi-selection. The other tools
    // keep the constant-screen-size widget at the pivot.
    let box_frame = frame;
    // While a stretch drag is live the DISPLAY box comes from the drag state
    // (only the dragged axis changes); between drags it re-fits the live
    // selection.
    let stretch_box = interaction
        .drag
        .as_ref()
        .and_then(|drag| drag.live_box)
        .or_else(|| bbox_live(&selection, box_frame, &globals, &motions));
    if tool.effective_tool() == EditTool::Stretch
        && let Some((center, ext)) = stretch_box
    {
        let center_bevy = sl_to_bevy_vec(&vec3_sl(center));
        let distance = vsub(center_bevy, camera_transform.translation()).length();
        let handle_scale = Vec3::splat(constant_screen_scale(
            distance,
            fov,
            height,
            GIZMO_SCREEN_PX,
        ));
        let edge_girth = 0.012 * constant_screen_scale(distance, fov, height, GIZMO_SCREEN_PX);
        *rig = Transform {
            translation: center_bevy,
            rotation: sl_to_bevy_rotation().mul_quat(box_frame),
            scale: Vec3::ONE,
        };
        for (handle, edge, mut transform) in &mut parts {
            if let Some(handle) = handle {
                match handle.part {
                    GizmoPart::ScaleFace(axis, positive) => {
                        let sign = if positive { 1.0 } else { -1.0 };
                        transform.translation = vscale(axis.unit(), axis.of(ext) * sign);
                        transform.scale = handle_scale;
                    }
                    GizmoPart::ScaleCorner(signs) => {
                        let [x, y, z] = signs;
                        transform.translation = Vec3::new(
                            if x { ext.x } else { -ext.x },
                            if y { ext.y } else { -ext.y },
                            if z { ext.z } else { -ext.z },
                        );
                        transform.scale = handle_scale;
                    }
                    _other => {}
                }
            } else if let Some(edge) = edge {
                let (other_a, other_b) = edge.axis.others();
                transform.translation = vadd(
                    vscale(other_a.unit(), edge.sign_a * other_a.of(ext)),
                    vscale(other_b.unit(), edge.sign_b * other_b.of(ext)),
                );
                transform.rotation = edge.axis.orientation();
                transform.scale = Vec3::new(edge_girth, edge.axis.of(ext) * 2.0, edge_girth);
            }
        }
        return;
    }

    let distance = vsub(pivot, camera_transform.translation()).length();
    let scale = constant_screen_scale(distance, fov, height, GIZMO_SCREEN_PX);
    *rig = Transform {
        translation: pivot,
        // The rig's local space is Second Life space in the grid frame: the
        // basis change carries it into Bevy's Y-up world.
        rotation: sl_to_bevy_rotation().mul_quat(frame),
        scale: Vec3::splat(scale),
    };
}

/// The pointer / camera / occlusion inputs the gizmo interaction reads,
/// bundled as one [`SystemParam`] to stay inside Bevy's system-parameter
/// limit.
#[derive(SystemParam)]
pub(crate) struct GizmoPointer<'w, 's> {
    /// The mouse buttons.
    buttons: Res<'w, ButtonInput<MouseButton>>,
    /// The keyboard, for the Alt (camera-orbit) guard.
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    /// The `bevy_ui` hover map, for the UI-occlusion guard.
    hover_map: Res<'w, HoverMap>,
    /// Pickability, for the UI-occlusion guard.
    pickables: Query<'w, 's, &'static Pickable>,
    /// Node sizes, for the UI-occlusion guard.
    node_sizes: Query<'w, 's, &'static ComputedNode>,
    /// The window, for the cursor position.
    windows: Query<'w, 's, &'static Window>,
    /// The world camera, to build the pick ray (the overlay camera shares its
    /// pose and projection).
    cameras: Query<'w, 's, (&'static Camera, &'static GlobalTransform), With<ViewerCamera>>,
}

/// The hover + drag state machine. Runs before the selection's pointer
/// handler ([`crate::edit_selection`] orders itself after this) so a press on
/// a handle claims the pointer first.
#[expect(
    clippy::too_many_arguments,
    reason = "a Bevy system's parameters are its injected resources / queries: the tool / \
              selection state, the bundled pointer inputs, the pick machinery, the handle / \
              object queries, the time source for the stream throttle, and the outgoing command \
              writer"
)]
pub(crate) fn drive_gizmo_interaction(
    tool: Res<EditToolState>,
    selection: Res<SelectionSet>,
    pointer: GizmoPointer,
    handles: Query<(Entity, &GizmoHandle)>,
    mut ray_cast: MeshRayCast,
    state: Res<ObjectState>,
    globals: Query<&GlobalTransform>,
    time: Res<Time>,
    assets: Res<GizmoAssets>,
    rigs: Query<&Transform, With<GizmoRoot>>,
    guides: Query<Entity, With<SnapGuideRoot>>,
    mut interaction: ResMut<GizmoInteraction>,
    mut motions: Query<(&mut ObjectSlMotion, &SceneObject)>,
    mut transforms: EditTransformQuery,
    mut ecs: Commands,
    mut commands: MessageWriter<SlCommand>,
) {
    // The snap-guide ruler lives exactly as long as a translate drag: no
    // drag, no ruler (covers release, tool switch, and deactivation alike).
    if interaction.drag.is_none() {
        for guide in guides.iter() {
            ecs.entity(guide).despawn();
        }
    }
    if !tool.active || selection.is_empty() {
        interaction.hovered = None;
        interaction.drag = None;
        return;
    }
    let Ok(window) = pointer.windows.single() else {
        return;
    };
    let Ok((camera, camera_transform)) = pointer.cameras.single() else {
        return;
    };
    let buttons = &pointer.buttons;
    let keyboard = &pointer.keyboard;
    let cursor = window.cursor_position();

    // ---- A live drag. -------------------------------------------------------
    if interaction.drag.is_some() {
        if !buttons.pressed(MouseButton::Left) {
            // Release: final send.
            if let Some(drag) = interaction.drag.take()
                && drag.moved
            {
                send_drag_updates(&drag, &tool, &motions, &mut commands);
            }
            return;
        }
        let ray = cursor.and_then(|cursor| camera.viewport_to_world(camera_transform, cursor).ok());
        if let Some(ray) = ray {
            let now = time.elapsed_secs();
            // Take the drag out to appease the borrow checker, put it back after.
            if let Some(mut drag) = interaction.drag.take() {
                update_drag(
                    &mut drag,
                    ray,
                    &tool,
                    &globals,
                    &mut motions,
                    &mut transforms,
                );
                // A stretch streams at the reference's 10 Hz.
                if matches!(
                    drag.part,
                    GizmoPart::ScaleFace(..) | GizmoPart::ScaleCorner(..)
                ) && drag.moved
                    && now - drag.last_stream >= SCALE_STREAM_INTERVAL
                {
                    drag.last_stream = now;
                    send_drag_updates(&drag, &tool, &motions, &mut commands);
                }
                interaction.drag = Some(drag);
            }
        }
        return;
    }

    // ---- Hover. -------------------------------------------------------------
    let alt = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    let over_ui =
        pointer_over_blocking_ui(&pointer.hover_map, &pointer.pickables, &pointer.node_sizes);
    interaction.hovered = None;
    let Some(cursor) = cursor else {
        return;
    };
    if over_ui || alt {
        return;
    }
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let handle_entities: HashSet<Entity> = handles.iter().map(|(entity, _handle)| entity).collect();
    let handle_filter = |entity: Entity| handle_entities.contains(&entity);
    let settings = MeshRayCastSettings::default()
        // The rig is drawn by the overlay camera; the main camera's view
        // visibility for it reads false, so use inherited visibility.
        .with_visibility(bevy::picking::mesh_picking::ray_cast::RayCastVisibility::Visible)
        .with_filter(&handle_filter);
    let hovered = ray_cast
        .cast_ray(ray, &settings)
        .first()
        .and_then(|(entity, _hit)| handles.get(*entity).ok())
        .map(|(_entity, handle)| handle.part);
    interaction.hovered = hovered;

    // ---- Press: begin a drag. -----------------------------------------------
    if let Some(part) = hovered
        && buttons.just_pressed(MouseButton::Left)
    {
        let rig_scale = rigs
            .single()
            .map_or(1.0, |transform| transform.scale.x.max(1.0e-3));
        interaction.drag = begin_drag(
            part,
            ray,
            camera_transform,
            &tool,
            &selection,
            &state,
            &globals,
            &motions,
            rig_scale,
            time.elapsed_secs(),
        );
        // A translate drag with snapping on shows the reference's white
        // snap-guide ruler; a ring drag its detent tick circle. Crossing
        // either with the cursor engages the grid.
        if let Some(drag) = &interaction.drag
            && tool.snap
        {
            match drag.part {
                GizmoPart::TranslateAxis(axis) => {
                    let axis_world = drag.frame.mul_vec3(axis.unit());
                    let perp = drag.plane_normal.cross(axis_world).normalize_or_zero();
                    if perp != Vec3::ZERO {
                        spawn_snap_guide(
                            &mut ecs,
                            &assets,
                            sl_to_bevy_vec(&vec3_sl(drag.pivot)),
                            drag.pivot,
                            axis_world,
                            perp,
                            drag.snap_offset,
                            tool.grid_unit,
                        );
                    }
                }
                GizmoPart::RotateRing(axis) => {
                    let (axis_a, axis_b) = ring_axes(drag.frame, axis);
                    spawn_rotate_snap_guide(
                        &mut ecs,
                        &assets,
                        sl_to_bevy_vec(&vec3_sl(drag.pivot)),
                        axis_a,
                        axis_b,
                        drag.snap_offset,
                        // Anchored so tick k sits where the MARKER lands when
                        // the object's absolute twist is k detents: the
                        // cardinals then mean the object standing at
                        // 0° / 90° / 180° / 270° in the grid frame.
                        drag.start_angle - drag.start_twist,
                    );
                }
                GizmoPart::ScaleFace(axis, positive) => {
                    let sign = if positive { 1.0 } else { -1.0 };
                    let dir = vscale(drag.frame.mul_vec3(axis.unit()), sign);
                    let camera_forward = sl_vec3(bevy_to_sl_vec(*camera_transform.forward()));
                    if let Some(normal) = manip_plane_normal(dir, camera_forward) {
                        let (_axis, p_extent, p_align) = primary_face_fold(drag, dir);
                        // Stretch-both-sides halves the cursor travel per
                        // unit of size, so the ruler compresses to match.
                        let per_size = if tool.stretch_both {
                            p_align * 0.5
                        } else {
                            p_align
                        };
                        let ticks =
                            face_scale_ticks(drag.start_param, p_extent, tool.grid_unit, per_size);
                        spawn_scale_snap_guide(
                            &mut ecs,
                            &assets,
                            sl_to_bevy_vec(&vec3_sl(drag.pivot)),
                            dir,
                            normal.cross(dir).normalize_or_zero(),
                            drag.snap_offset,
                            &ticks,
                        );
                    }
                }
                GizmoPart::ScaleCorner(signs) => {
                    let dir = drag.corner_dir(signs);
                    let camera_forward = sl_vec3(bevy_to_sl_vec(*camera_transform.forward()));
                    if let Some(normal) = manip_plane_normal(dir, camera_forward) {
                        let ticks = corner_scale_ticks(drag.start_param, tool.stretch_both);
                        spawn_scale_snap_guide(
                            &mut ecs,
                            &assets,
                            sl_to_bevy_vec(&vec3_sl(drag.pivot)),
                            dir,
                            normal.cross(dir).normalize_or_zero(),
                            drag.snap_offset,
                            &ticks,
                        );
                    }
                }
                _other => {}
            }
        }
    }
}

/// Snapshot the selection and the drag geometry at press.
#[expect(
    clippy::too_many_arguments,
    reason = "the drag snapshot reads the same state bundle the interaction system holds"
)]
fn begin_drag(
    part: GizmoPart,
    ray: Ray3d,
    camera_transform: &GlobalTransform,
    tool: &EditToolState,
    selection: &SelectionSet,
    state: &ObjectState,
    globals: &Query<&GlobalTransform>,
    motions: &Query<(&mut ObjectSlMotion, &SceneObject)>,
    rig_scale: f32,
    now: f32,
) -> Option<GizmoDrag> {
    let pivot_bevy = selection_pivot_bevy(selection, globals)?;
    let pivot = sl_vec3(bevy_to_sl_vec(pivot_bevy));
    let frame = grid_frame_rotation(tool, selection, globals);
    let ray_origin = sl_vec3(bevy_to_sl_vec(ray.origin));
    let ray_dir = sl_vec3(bevy_to_sl_vec(*ray.direction));
    let camera_forward = sl_vec3(bevy_to_sl_vec(*camera_transform.forward()));

    // Per-object snapshots.
    let mut objects = Vec::new();
    for node in selection.iter() {
        let Ok((motion, _scene)) = motions.get(node.entity) else {
            continue;
        };
        let Ok(global) = globals.get(node.entity) else {
            continue;
        };
        let parent = state.parent_entity_of(&node.scoped);
        objects.push(DragObject {
            scoped: node.scoped,
            entity: node.entity,
            geometry: state.geometry_of(&node.scoped),
            is_root: motion.is_root,
            parent,
            start_scale: motion.scale.clone(),
            start_world_pos: sl_vec3(bevy_to_sl_vec(global.translation())),
            start_world_rot: sl_world_rotation(global.rotation()),
        });
    }
    if objects.is_empty() {
        return None;
    }
    // A stretch works on the selection BOUNDING BOX (the reference's
    // `getBBoxOfSelection`): its centre is the drag pivot and its extents
    // carry the corner diagonals, so the box the handles mount on genuinely
    // stretches along the grid axis dragged.
    let (pivot, bbox_ext) = if matches!(part, GizmoPart::ScaleFace(..) | GizmoPart::ScaleCorner(..))
    {
        bbox_from_snapshots(&objects, frame).unwrap_or((pivot, Vec3::ONE))
    } else {
        (pivot, Vec3::ONE)
    };

    let mut drag = GizmoDrag {
        part,
        pivot,
        frame,
        plane_normal: Vec3::Z,
        start_hit: pivot,
        start_param: 0.0,
        start_angle: 0.0,
        start_twist: 0.0,
        last_ring_angle: 0.0,
        accumulated_angle: 0.0,
        // The snap-guide ruler distance scales with the rig, so it sits at
        // the same on-screen spot whatever the camera range.
        snap_offset: rig_scale * SNAP_GUIDE_OFFSET_RIG,
        snap_progress: None,
        readout: String::new(),
        objects,
        moved: false,
        last_stream: now,
        bbox_ext,
        live_box: None,
    };
    match part {
        GizmoPart::TranslateAxis(axis) => {
            let world_axis = frame.mul_vec3(axis.unit());
            let normal = manip_plane_normal(world_axis, camera_forward)?;
            drag.plane_normal = normal;
            drag.start_hit = ray_plane_intersect(ray_origin, ray_dir, pivot, normal)?;
        }
        GizmoPart::TranslatePlane(axis) => {
            let normal = frame.mul_vec3(axis.unit());
            drag.plane_normal = normal;
            drag.start_hit = ray_plane_intersect(ray_origin, ray_dir, pivot, normal)?;
        }
        GizmoPart::RotateRing(axis) => {
            let normal = frame.mul_vec3(axis.unit());
            drag.plane_normal = normal;
            let hit = ray_plane_intersect(ray_origin, ray_dir, pivot, normal)?;
            let (axis_a, axis_b) = ring_axes(frame, axis);
            drag.start_angle = ring_angle(vsub(hit, pivot), axis_a, axis_b);
            drag.last_ring_angle = drag.start_angle;
            // The primary's absolute twist about the axis: the repeatable
            // value the detents quantise.
            drag.start_twist = drag.objects.last().map_or(0.0, |object| {
                crate::edit_math::twist_about_axis(object.start_world_rot, normal)
            });
            // For a ring the snap boundary is the detent tick circle's radius.
            drag.snap_offset = rig_scale * ROTATE_SNAP_GUIDE_RADIUS_RIG;
        }
        GizmoPart::ScaleFace(axis, positive) => {
            // The handle direction follows the GRID frame (the rig's drawn
            // axes); each object folds it onto its own nearest local scale
            // axis when the stretch applies — the reference's `stretchFace`,
            // which is what makes world-frame and local-frame stretching
            // genuinely different modes.
            let sign = if positive { 1.0 } else { -1.0 };
            let dir = vscale(frame.mul_vec3(axis.unit()), sign);
            drag.start_param = closest_line_param(pivot, dir, ray_origin, ray_dir)?;
        }
        GizmoPart::ScaleCorner(signs) => {
            let dir = drag.corner_dir(signs);
            let param = closest_line_param(pivot, dir, ray_origin, ray_dir)?;
            if param.abs() < 1.0e-4 {
                return None;
            }
            drag.start_param = param;
        }
    }
    Some(drag)
}

/// The two in-plane orthonormal axes a ring's angle is measured against.
fn ring_axes(frame: Quat, axis: GizmoAxis) -> (Vec3, Vec3) {
    match axis {
        GizmoAxis::X => (frame.mul_vec3(Vec3::Y), frame.mul_vec3(Vec3::Z)),
        GizmoAxis::Y => (frame.mul_vec3(Vec3::Z), frame.mul_vec3(Vec3::X)),
        GizmoAxis::Z => (frame.mul_vec3(Vec3::X), frame.mul_vec3(Vec3::Y)),
    }
}

/// A `Vector` (Second Life wire type) as a glam [`Vec3`].
const fn sl_vec3(vector: Vector) -> Vec3 {
    Vec3::new(vector.x, vector.y, vector.z)
}

/// A glam [`Vec3`] as the wire `Vector`.
const fn vec3_sl(vector: Vec3) -> Vector {
    Vector {
        x: vector.x,
        y: vector.y,
        z: vector.z,
    }
}

/// Advance a drag from the current mouse ray: compute each object's new
/// transform, apply it locally (entity + `ObjectSlMotion` echo), and mark the
/// drag moved.
fn update_drag(
    drag: &mut GizmoDrag,
    ray: Ray3d,
    tool: &EditToolState,
    globals: &Query<&GlobalTransform>,
    motions: &mut Query<(&mut ObjectSlMotion, &SceneObject)>,
    transforms: &mut EditTransformQuery,
) {
    let ray_origin = sl_vec3(bevy_to_sl_vec(ray.origin));
    let ray_dir = sl_vec3(bevy_to_sl_vec(*ray.direction));

    match drag.part {
        GizmoPart::TranslateAxis(axis) => {
            let world_axis = drag.frame.mul_vec3(axis.unit());
            let Some(hit) = ray_plane_intersect(ray_origin, ray_dir, drag.pivot, drag.plane_normal)
            else {
                return;
            };
            let mut distance = project_onto_axis(vsub(hit, drag.start_hit), world_axis);
            // The reference's snap regime: the grid engages only once the
            // cursor strays past the white snap-guide ruler (its off-axis
            // distance in the drag plane crosses `snap_offset`); near the
            // axis the drag stays free.
            let from_pivot = vsub(hit, drag.pivot);
            let off_axis = vsub(from_pivot, vscale(world_axis, from_pivot.dot(world_axis)));
            if tool.snap && off_axis.length() > drag.snap_offset {
                // Snap the moved coordinate (pivot + distance along the axis)
                // to the absolute grid along that axis.
                let coord =
                    project_onto_axis(vadd(drag.pivot, vscale(world_axis, distance)), world_axis);
                let snapped = snap_to_grid(coord, tool.grid_unit);
                distance += snapped - coord;
                // The guide highlights the grid mark being held.
                drag.snap_progress = Some(snapped - project_onto_axis(drag.pivot, world_axis));
            } else {
                drag.snap_progress = None;
            }
            let delta = vscale(world_axis, distance);
            let moved_pivot = vadd(drag.pivot, delta);
            drag.readout = format!(
                "X {:.3}  Y {:.3}  Z {:.3}",
                moved_pivot.x, moved_pivot.y, moved_pivot.z
            );
            apply_translate(drag, delta, globals, motions, transforms);
        }
        GizmoPart::TranslatePlane(_axis) => {
            let Some(hit) = ray_plane_intersect(ray_origin, ray_dir, drag.pivot, drag.plane_normal)
            else {
                return;
            };
            let mut delta = vsub(hit, drag.start_hit);
            if tool.snap {
                // Snap both in-plane coordinates to the grid.
                let target = vadd(drag.pivot, delta);
                let snapped = Vec3::new(
                    snap_to_grid(target.x, tool.grid_unit),
                    snap_to_grid(target.y, tool.grid_unit),
                    snap_to_grid(target.z, tool.grid_unit),
                );
                // Keep the snap inside the drag plane: project the snap
                // correction off the plane normal.
                let correction = vsub(snapped, target);
                let in_plane = vsub(
                    correction,
                    vscale(drag.plane_normal, correction.dot(drag.plane_normal)),
                );
                delta = vadd(delta, in_plane);
            }
            let moved_pivot = vadd(drag.pivot, delta);
            drag.readout = format!(
                "X {:.3}  Y {:.3}  Z {:.3}",
                moved_pivot.x, moved_pivot.y, moved_pivot.z
            );
            apply_translate(drag, delta, globals, motions, transforms);
        }
        GizmoPart::RotateRing(axis) => {
            let Some(hit) = ray_plane_intersect(ray_origin, ray_dir, drag.pivot, drag.plane_normal)
            else {
                return;
            };
            let (axis_a, axis_b) = ring_axes(drag.frame, axis);
            let angle = ring_angle(vsub(hit, drag.pivot), axis_a, axis_b);
            // Accumulate a continuous, wrap-free rotation: wrap only the
            // per-frame *step* into (-π, π], so crossing the atan2 seam never
            // jumps the total by a full turn (and multi-turn drags keep
            // counting).
            drag.accumulated_angle += crate::edit_math::wrap_angle(angle - drag.last_ring_angle);
            drag.last_ring_angle = angle;
            let mut delta_angle = drag.accumulated_angle;
            // The reference's rotate snap regime: the detents engage only
            // once the cursor is outside the tick circle; inside the ring
            // the rotation stays free.
            let from_pivot = vsub(hit, drag.pivot);
            let in_plane = vsub(
                from_pivot,
                vscale(drag.plane_normal, from_pivot.dot(drag.plane_normal)),
            );
            if tool.snap && in_plane.length() > drag.snap_offset {
                // Quantise the object's ABSOLUTE twist about the axis (not
                // the grab-relative delta), so snapped rotations land on
                // repeatable orientations — 0° / 90° / 180° / 270° in the
                // grid frame whatever the grab point.
                let snapped_total =
                    snap_angle(drag.start_twist + delta_angle, SNAP_ANGLE_DEG.to_radians());
                delta_angle = snapped_total - drag.start_twist;
                // The guide highlights the detent being held.
                drag.snap_progress = Some(drag.start_angle + delta_angle);
            } else {
                drag.snap_progress = None;
            }
            // The read-out shows the object's absolute angle about the axis
            // (matching the build floater's rotation fields), normalized to
            // [0°, 360°).
            drag.readout = format!(
                "{:.2}\u{b0}",
                (drag.start_twist + delta_angle)
                    .to_degrees()
                    .rem_euclid(360.0)
            );
            let delta = Quat::from_axis_angle(drag.plane_normal, delta_angle);
            apply_rotate(drag, delta, globals, motions, transforms);
        }
        GizmoPart::ScaleFace(axis, positive) => {
            let sign = if positive { 1.0 } else { -1.0 };
            let dir = vscale(drag.frame.mul_vec3(axis.unit()), sign);
            let Some(param) = closest_line_param(drag.pivot, dir, ray_origin, ray_dir) else {
                return;
            };
            let mut travel = param - drag.start_param;
            // The PRIMARY's fold of the (possibly world-frame) drag onto its
            // nearest local scale axis — what the snap ladder and read-out
            // are labelled in.
            let (_fold_axis, p_extent, p_align) = primary_face_fold(drag, dir);
            // Stretch-both-sides moves BOTH box faces equally, so the same
            // cursor travel produces twice the size change.
            let size_mult = if tool.stretch_both { 2.0 } else { 1.0 };
            // Cursor travel per unit of primary local size.
            let per_size = p_align / size_mult;
            // The scale snap regime, like translate: the size ruler engages
            // only once the cursor strays off the handle's line past the
            // guide. Snapping quantises the PRIMARY's local extent to the
            // absolute grid; the cursor travel follows through the mapping.
            if tool.snap
                && off_line_distance(ray_origin, ray_dir, drag.pivot, dir)
                    .is_some_and(|distance| distance > drag.snap_offset)
            {
                let snapped =
                    clamp_scale(snap_to_grid(p_extent + travel / per_size, tool.grid_unit));
                travel = (snapped - p_extent) * per_size;
                // The guide highlights the held size mark, in cursor
                // (line-param) space.
                drag.snap_progress = Some(drag.start_param + travel);
            } else {
                drag.snap_progress = None;
            }
            drag.readout = format!("{:.3} m", clamp_scale(p_extent + travel / per_size));
            // The display box changes on the dragged axis ALONE: the grabbed
            // side follows the cursor; with stretch-both-sides the opposite
            // face mirrors it (centre fixed), otherwise it stays pinned (the
            // centre shifts half the travel toward the grab).
            let ext_grow = if tool.stretch_both {
                travel
            } else {
                travel * 0.5
            };
            let mut ext = drag.bbox_ext;
            match axis {
                GizmoAxis::X => ext.x = (ext.x + ext_grow).max(0.01),
                GizmoAxis::Y => ext.y = (ext.y + ext_grow).max(0.01),
                GizmoAxis::Z => ext.z = (ext.z + ext_grow).max(0.01),
            }
            let center = if tool.stretch_both {
                drag.pivot
            } else {
                vadd(drag.pivot, vscale(dir, travel * 0.5))
            };
            drag.live_box = Some((center, ext));
            apply_face_scale(drag, dir, travel * size_mult, tool, motions, transforms);
        }
        GizmoPart::ScaleCorner(signs) => {
            let dir = drag.corner_dir(signs);
            let Some(param) = closest_line_param(drag.pivot, dir, ray_origin, ray_dir) else {
                return;
            };
            let ratio = (param / drag.start_param).clamp(0.01, 100.0);
            // With stretch-both-sides OFF the reference halves the cursor
            // mapping (`0.5 + t/2`, `dragCorner`): the box grows one way
            // only — about the opposite corner — so the dragged corner needs
            // twice the travel for the same factor.
            let mut factor = if tool.stretch_both {
                ratio
            } else {
                0.5 + ratio * 0.5
            };
            // The corner's snap regime quantises the uniform factor to
            // quarter steps (the guide's ×0.25 tick ladder, with the ×0.5
            // multiples and ×1.0 marked large).
            if tool.snap
                && off_line_distance(ray_origin, ray_dir, drag.pivot, dir)
                    .is_some_and(|distance| distance > drag.snap_offset)
            {
                factor = ((factor / CORNER_FACTOR_STEP).round() * CORNER_FACTOR_STEP)
                    .max(CORNER_FACTOR_STEP);
                // The guide highlights the held factor, back in cursor
                // (line-param) space through the same mapping.
                let marker_ratio = if tool.stretch_both {
                    factor
                } else {
                    (factor - 0.5) * 2.0
                };
                drag.snap_progress = Some(marker_ratio * drag.start_param);
            } else {
                drag.snap_progress = None;
            }
            // One SHARED factor clamp (the reference's): the whole selection
            // keeps its proportions when an object hits the prim size limits.
            factor = clamp_corner_factor(factor, &drag.objects);
            drag.readout = format!("\u{d7}{factor:.2}");
            // The display box scales by the factor about the mode's pivot
            // (centre, or the opposite corner without stretch-both-sides).
            let scale_about = if tool.stretch_both {
                drag.pivot
            } else {
                vsub(drag.pivot, vscale(dir, drag.start_param))
            };
            let center = vadd(scale_about, vscale(vsub(drag.pivot, scale_about), factor));
            drag.live_box = Some((center, vscale(drag.bbox_ext, factor).max(Vec3::splat(0.01))));
            apply_corner_scale(drag, factor, dir, tool, motions, transforms);
        }
    }
}

/// The primary (last-selected) object's fold of a face-stretch drag direction
/// onto its own nearest local scale axis: the axis, its start extent, and the
/// (guarded) alignment — the reference's `nearestAxis` half of `stretchFace`,
/// shared by the drag, the snap ladder, and the read-out.
fn primary_face_fold(drag: &GizmoDrag, world_dir: Vec3) -> (GizmoAxis, f32, f32) {
    let local_dir = drag.primary_rot().inverse().mul_vec3(world_dir);
    let (index, _sign, alignment) = nearest_local_axis(local_dir);
    let axis = match index {
        0 => GizmoAxis::X,
        1 => GizmoAxis::Y,
        _other => GizmoAxis::Z,
    };
    let extent = drag
        .objects
        .last()
        .map_or(1.0, |object| axis_component(&object.start_scale, axis));
    (axis, extent, alignment.max(1.0e-4))
}

/// The shared corner-stretch factor range that keeps EVERY dragged object's
/// scale components inside the prim size limits — the reference's global
/// factor clamp, which preserves the selection's proportions instead of
/// distorting whichever object saturates first.
fn clamp_corner_factor(factor: f32, objects: &[DragObject]) -> f32 {
    let mut max_factor = MAX_PRIM_SCALE / MIN_PRIM_SCALE;
    let mut min_factor = MIN_PRIM_SCALE / MAX_PRIM_SCALE;
    for object in objects {
        for component in [
            object.start_scale.x,
            object.start_scale.y,
            object.start_scale.z,
        ] {
            let component = component.max(1.0e-4);
            max_factor = max_factor.min(MAX_PRIM_SCALE / component);
            min_factor = min_factor.max(MIN_PRIM_SCALE / component);
        }
    }
    factor.clamp(min_factor, max_factor.max(min_factor))
}

/// The perpendicular distance of the mouse ray from the drag line, measured in
/// the camera-facing plane through the line — the "how far off the handle's
/// axis is the cursor" the scale snap regime keys on. `None` when the view is
/// degenerate (line towards the camera).
fn off_line_distance(ray_origin: Vec3, ray_dir: Vec3, pivot: Vec3, dir: Vec3) -> Option<f32> {
    let normal = manip_plane_normal(dir, ray_dir)?;
    let hit = ray_plane_intersect(ray_origin, ray_dir, pivot, normal)?;
    let from_pivot = vsub(hit, pivot);
    Some(vsub(from_pivot, vscale(dir, from_pivot.dot(dir))).length())
}

/// Apply a world-space translation `delta` to every dragged object.
fn apply_translate(
    drag: &mut GizmoDrag,
    delta: Vec3,
    globals: &Query<&GlobalTransform>,
    motions: &mut Query<(&mut ObjectSlMotion, &SceneObject)>,
    transforms: &mut EditTransformQuery,
) {
    if delta.length_squared() > 1.0e-10 {
        drag.moved = true;
    } else if !drag.moved {
        return;
    }
    for object in &drag.objects {
        let new_world = vadd(object.start_world_pos, delta);
        let new_wire = if object.is_root {
            vec3_sl(new_world)
        } else {
            // A linked part's wire position is parent-relative: carry the new
            // world point into the parent's frame.
            match parent_local_point(object, new_world, globals) {
                Some(local) => vec3_sl(local),
                None => continue,
            }
        };
        write_object_edit(object, Some(new_wire), None, None, motions, transforms);
    }
}

/// Apply a world-space rotation `delta` about the pivot to every dragged
/// object.
fn apply_rotate(
    drag: &mut GizmoDrag,
    delta: Quat,
    globals: &Query<&GlobalTransform>,
    motions: &mut Query<(&mut ObjectSlMotion, &SceneObject)>,
    transforms: &mut EditTransformQuery,
) {
    if delta.angle_between(Quat::IDENTITY) > 1.0e-5 {
        drag.moved = true;
    } else if !drag.moved {
        return;
    }
    for object in &drag.objects {
        let new_world_rot = delta.mul_quat(object.start_world_rot).normalize();
        let new_world_pos = vadd(
            drag.pivot,
            delta.mul_vec3(vsub(object.start_world_pos, drag.pivot)),
        );
        let (new_pos, new_rot) = if object.is_root {
            (vec3_sl(new_world_pos), quat_to_rotation(new_world_rot))
        } else {
            let Some(local_pos) = parent_local_point(object, new_world_pos, globals) else {
                continue;
            };
            let Some(local_rot) = parent_local_rotation(object, new_world_rot, globals) else {
                continue;
            };
            (vec3_sl(local_pos), local_rot)
        };
        write_object_edit(
            object,
            Some(new_pos),
            Some(new_rot),
            None,
            motions,
            transforms,
        );
    }
}

/// Apply a face-handle stretch, the reference's `stretchFace`: each object
/// folds the world drag onto its own **nearest local scale axis** (delta
/// divided by the alignment, so its world extent along the drag grows by the
/// dragged amount), holding the opposite face (or the centre, with
/// stretch-both-sides) in place.
fn apply_face_scale(
    drag: &mut GizmoDrag,
    world_dir: Vec3,
    extent_delta: f32,
    tool: &EditToolState,
    motions: &mut Query<(&mut ObjectSlMotion, &SceneObject)>,
    transforms: &mut EditTransformQuery,
) {
    if extent_delta.abs() > 1.0e-6 {
        drag.moved = true;
    } else if !drag.moved {
        return;
    }
    for object in &drag.objects {
        // The drag direction in this object's own frame picks the affected
        // local axis; the delta divides by the alignment (`stretchFace`'s
        // `delta_local_mag / (axis · dir_local)`).
        let local_dir = object.start_world_rot.inverse().mul_vec3(world_dir);
        let (index, axis_sign, alignment) = nearest_local_axis(local_dir);
        if alignment < 1.0e-4 {
            continue;
        }
        let axis = match index {
            0 => GizmoAxis::X,
            1 => GizmoAxis::Y,
            _other => GizmoAxis::Z,
        };
        let start_extent = axis_component(&object.start_scale, axis);
        let new_extent = clamp_scale(start_extent + extent_delta / alignment);
        let actual_delta = new_extent - start_extent;
        let mut new_scale = object.start_scale.clone();
        set_axis_component(&mut new_scale, axis, new_extent);
        // Without stretch-both-sides the opposite face stays put: the centre
        // shifts half the growth along the object's own (world-ised) dragged
        // local axis — the reference's `axis * 0.5 * desired_delta_size`. A
        // linked part keeps its parent-relative position.
        let new_position = if tool.stretch_both || !object.is_root {
            None
        } else {
            let world_axis = vscale(object.start_world_rot.mul_vec3(axis.unit()), axis_sign);
            let shift = vscale(world_axis, actual_delta * 0.5);
            Some(vec3_sl(vadd(object.start_world_pos, shift)))
        };
        write_object_edit(
            object,
            new_position,
            None,
            Some(new_scale),
            motions,
            transforms,
        );
    }
}

/// Apply a corner-handle stretch: a uniform factor, about the selection
/// centre with stretch-both-sides on, or about the **opposite corner** (the
/// grab point mirrored through the pivot — the reference's
/// `mDragFarHitGlobal`) with it off, so the far corner stays put.
fn apply_corner_scale(
    drag: &mut GizmoDrag,
    factor: f32,
    world_dir: Vec3,
    tool: &EditToolState,
    motions: &mut Query<(&mut ObjectSlMotion, &SceneObject)>,
    transforms: &mut EditTransformQuery,
) {
    if (factor - 1.0).abs() > 1.0e-6 {
        drag.moved = true;
    } else if !drag.moved {
        return;
    }
    let scale_about = if tool.stretch_both {
        drag.pivot
    } else {
        vsub(drag.pivot, vscale(world_dir, drag.start_param))
    };
    for object in &drag.objects {
        // The shared factor is pre-clamped (`clamp_corner_factor`); the
        // per-component clamp is only a belt-and-braces guard.
        let new_scale = Vector {
            x: clamp_scale(object.start_scale.x * factor),
            y: clamp_scale(object.start_scale.y * factor),
            z: clamp_scale(object.start_scale.z * factor),
        };
        // Positions scale about the mode's pivot point.
        let new_world = vadd(
            scale_about,
            vscale(vsub(object.start_world_pos, scale_about), factor),
        );
        let new_position = object.is_root.then(|| vec3_sl(new_world));
        write_object_edit(
            object,
            new_position,
            None,
            Some(new_scale),
            motions,
            transforms,
        );
    }
}

/// One component of a `Vector` by axis.
const fn axis_component(vector: &Vector, axis: GizmoAxis) -> f32 {
    match axis {
        GizmoAxis::X => vector.x,
        GizmoAxis::Y => vector.y,
        GizmoAxis::Z => vector.z,
    }
}

/// Set one component of a `Vector` by axis.
const fn set_axis_component(vector: &mut Vector, axis: GizmoAxis, value: f32) {
    match axis {
        GizmoAxis::X => vector.x = value,
        GizmoAxis::Y => vector.y = value,
        GizmoAxis::Z => vector.z = value,
    }
}

/// Carry a Second Life **world** point into a linked part's parent frame (the
/// wire frame its position updates use), via the parent entity's global
/// transform.
fn parent_local_point(
    object: &DragObject,
    world: Vec3,
    globals: &Query<&GlobalTransform>,
) -> Option<Vec3> {
    let parent = object.parent?;
    let parent_global = globals.get(parent).ok()?;
    let world_bevy = sl_to_bevy_vec(&vec3_sl(world));
    Some(
        parent_global
            .affine()
            .inverse()
            .transform_point3(world_bevy),
    )
}

/// Carry a Second Life **world** rotation into a linked part's parent frame.
fn parent_local_rotation(
    object: &DragObject,
    world_rot: Quat,
    globals: &Query<&GlobalTransform>,
) -> Option<Rotation> {
    let parent = object.parent?;
    let parent_global = globals.get(parent).ok()?;
    let world_bevy = sl_to_bevy_rotation().mul_quat(world_rot);
    let local = parent_global.rotation().inverse().mul_quat(world_bevy);
    Some(quat_to_rotation(local.normalize()))
}

/// Apply one object's new wire-frame values to its `ObjectSlMotion` mirror and
/// its scene transforms — the local echo of an edit.
fn write_object_edit(
    object: &DragObject,
    position: Option<Vector>,
    rotation: Option<Rotation>,
    scale: Option<Vector>,
    motions: &mut Query<(&mut ObjectSlMotion, &SceneObject)>,
    transforms: &mut EditTransformQuery,
) {
    let Ok((mut motion, scene)) = motions.get_mut(object.entity) else {
        return;
    };
    if let Some(position) = position {
        motion.position = position;
    }
    if let Some(rotation) = rotation {
        motion.rotation = rotation;
    }
    if let Some(scale) = scale {
        motion.scale = scale;
    }
    let motion = motion.clone();
    write_back_motion(&motion, scene, object.entity, object.geometry, transforms);
}

/// Write an [`ObjectSlMotion`]'s values onto the scene: the object entity's
/// transform (position + rotation, the [`crate::objects`] frame conventions)
/// and — for the plainly scaled categories — the geometry holder's scale.
/// Shared by the gizmo drags and the build floater's numeric commits.
pub(crate) fn write_back_motion(
    motion: &ObjectSlMotion,
    scene: &SceneObject,
    entity: Entity,
    geometry: Option<Entity>,
    transforms: &mut EditTransformQuery,
) {
    if let Ok(mut transform) = transforms.get_mut(entity) {
        if motion.is_root {
            transform.translation = sl_to_bevy_vec(&motion.position);
            transform.rotation = sl_to_bevy_object_rotation(&motion.rotation);
        } else {
            transform.translation =
                Vec3::new(motion.position.x, motion.position.y, motion.position.z);
            transform.rotation = sl_rotation_to_quat(&motion.rotation);
        }
    }
    // Only the categories whose holder carries the plain anisotropic scale
    // get the live scale echo; trees / grass holders are special and are left
    // to the simulator's authoritative update. (A flexi prim's identity
    // holder is briefly overwritten here, then restored by the simulator's
    // echo re-running `holder_transform` — an accepted transient.)
    if matches!(
        scene.category,
        ObjectCategory::Prim | ObjectCategory::Sculpt | ObjectCategory::Mesh
    ) && let Some(geometry) = geometry
        && let Ok(mut holder) = transforms.get_mut(geometry)
    {
        holder.scale = Vec3::new(motion.scale.x, motion.scale.y, motion.scale.z);
    }
}

/// Send the drag's final (or streamed) `MultipleObjectUpdate`s: one per
/// dragged object, with the changed components and the linked-set / uniform
/// flags the reference sets.
fn send_drag_updates(
    drag: &GizmoDrag,
    tool: &EditToolState,
    motions: &Query<(&mut ObjectSlMotion, &SceneObject)>,
    commands: &mut MessageWriter<SlCommand>,
) {
    for object in &drag.objects {
        let Ok((motion, _scene)) = motions.get(object.entity) else {
            continue;
        };
        let transform = match drag.part {
            GizmoPart::TranslateAxis(..) | GizmoPart::TranslatePlane(..) => ObjectTransform {
                position: Some(motion.position.clone()),
                group: !tool.edit_linked,
                ..Default::default()
            },
            GizmoPart::RotateRing(..) => ObjectTransform {
                // Rotation and position together: rotating about the
                // selection pivot moves every non-pivot object.
                position: Some(motion.position.clone()),
                rotation: Some(motion.rotation.clone()),
                group: !tool.edit_linked,
                ..Default::default()
            },
            GizmoPart::ScaleFace(..) | GizmoPart::ScaleCorner(..) => ObjectTransform {
                position: Some(motion.position.clone()),
                scale: Some(motion.scale.clone()),
                group: !tool.edit_linked,
                // The reference sets `UPD_UNIFORM` only on a CORNER drag with
                // stretch-both-sides — never on a face drag, where the sim
                // could take it as licence to scale every axis.
                uniform: matches!(drag.part, GizmoPart::ScaleCorner(..)) && tool.stretch_both,
                ..Default::default()
            },
        };
        debug!(
            "gizmos: sending {:?} update for {:?} (type byte {:#04x})",
            drag.part,
            object.scoped,
            transform.type_byte()
        );
        commands.write(SlCommand(Command::UpdateObject {
            local_id: object.scoped,
            transform,
        }));
    }
}

/// Tint the hovered / dragged handle with the highlight material, and restore
/// the part colour otherwise.
fn tint_gizmo_handles(
    interaction: Res<GizmoInteraction>,
    assets: Res<GizmoAssets>,
    mut handles: Query<(&GizmoHandle, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if !interaction.is_changed() {
        return;
    }
    let active = interaction
        .drag
        .as_ref()
        .map(|drag| drag.part)
        .or(interaction.hovered);
    for (handle, mut material) in &mut handles {
        let want = if Some(handle.part) == active {
            assets.hover.clone()
        } else {
            match handle.part {
                GizmoPart::TranslateAxis(axis)
                | GizmoPart::RotateRing(axis)
                | GizmoPart::ScaleFace(axis, _) => assets
                    .axis
                    .get(axis.index())
                    .unwrap_or(&assets.corner)
                    .clone(),
                GizmoPart::TranslatePlane(axis) => assets
                    .pad_axis
                    .get(axis.index())
                    .unwrap_or(&assets.corner)
                    .clone(),
                GizmoPart::ScaleCorner(..) => assets.corner.clone(),
            }
        };
        if material.0 != want {
            material.0 = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{GizmoAxis, corner_direction, ring_axes, sl_world_rotation};
    use bevy::math::{Quat, Vec3};
    use pretty_assertions::assert_eq;

    /// The axis units and indices line up.
    #[test]
    fn axis_units() {
        assert_eq!(GizmoAxis::X.unit(), Vec3::X);
        assert_eq!(GizmoAxis::Y.unit(), Vec3::Y);
        assert_eq!(GizmoAxis::Z.unit(), Vec3::Z);
        assert_eq!(GizmoAxis::X.index(), 0);
        assert_eq!(GizmoAxis::Z.index(), 2);
    }

    /// Corner directions are unit length and signed per axis.
    #[test]
    fn corner_directions() {
        let corner = corner_direction([true, false, true]);
        assert!((corner.length() - 1.0).abs() < 1.0e-6);
        assert!(corner.x > 0.0 && corner.y < 0.0 && corner.z > 0.0);
    }

    /// Ring axes are the two in-plane axes, right-handed about the ring
    /// normal: a × b = normal.
    #[test]
    fn ring_axes_are_right_handed() {
        for axis in GizmoAxis::ALL {
            let (a, b) = ring_axes(Quat::IDENTITY, axis);
            let normal = axis.unit();
            assert!(a.cross(b).abs_diff_eq(normal, 1.0e-6), "{axis:?}");
        }
    }

    /// Stripping the basis change off a Bevy world rotation recovers the
    /// Second Life world rotation.
    #[test]
    fn world_rotation_strips_the_basis() {
        let sl_rot = Quat::from_rotation_z(0.7);
        let bevy_rot = crate::coords::sl_to_bevy_rotation().mul_quat(sl_rot);
        assert!(sl_world_rotation(bevy_rot).abs_diff_eq(sl_rot, 1.0e-6));
    }
}
