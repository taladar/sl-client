//! Mesh-accurate avatar picking: resolve a world ray to the avatar whose
//! **posed** geometry it actually hits.
//!
//! A rigged avatar's rendered triangles live nowhere a [`bevy::picking`] ray
//! cast can see them: a skinned mesh's CPU-side `ATTRIBUTE_POSITION` is the
//! **bind pose**, the GPU applies the joint matrix palette in the vertex
//! shader, and the body parts opt out of `Aabb` culling bounds entirely
//! (`NoFrustumCulling`), so `MeshRayCast` never even considers them. The
//! previous pick therefore used only the fitted **box collider**
//! ([`crate::avatars::fit_avatar_pick_colliders`]) — shape- and pose-adaptive,
//! but not silhouette-accurate: a click just *off* an avatar could still pick
//! it.
//!
//! This module reproduces the GPU skinning on the CPU **on demand** (only when
//! a pick is requested — a right-click, or the debug pick inspector) and ray
//! tests the posed triangles, the same skinning reproduction the R13 geometry
//! diagnostic validated: `world = Σ wᵢ · (joint_worldᵢ · inverse_bindᵢ) ·
//! rest`, with the joint worlds read from the very joint-entity
//! `GlobalTransform`s the render palette is built from. The reference viewer
//! ray tests a worn rigged mesh's posed octree triangles the same way
//! (`pick_rigged`); for the system body it intersects fitted collision-volume
//! ellipsoids (`LLVOAvatar::lineSegmentIntersect`), which the posed system-body
//! triangles here strictly refine.
//!
//! The fitted box stays, in two supporting roles:
//!
//! - **broad phase** — an avatar is CPU-skinned only if the ray passes within
//!   [`LIMB_REACH_MARGIN`] of the box's bounding sphere (the box hugs the
//!   torso, so an outstretched limb needs the margin);
//! - **fallback** — an avatar with *no* visible decoded geometry yet (its worn
//!   mesh body still downloading while the system body is alpha-hidden) is
//!   still pickable via the box, so a just-arriving avatar never becomes
//!   unclickable.
//!
//! The placeholder sphere of a coarse-only avatar (or a `--viewer-assets`-less
//! run) is rigid and correctly placed, so it is intersected analytically.
//!
//! Entry point: [`AvatarPicker::pick`]. Consumers: the shared right-click
//! resolver in [`crate::avatar_menu`], which routes a hit either to the avatar
//! pies or — when the nearest triangle belongs to a worn rigged attachment
//! ([`AvatarRayHit::worn`]) — to the attachment pies
//! ([`crate::attachment_menu`]); planned: inventory drag-and-drop onto an
//! avatar.
//!
//! Known approximation: the CPU skin reads only positions, joints, and
//! weights — the render-time morph targets (breathing, body physics, P31.12a)
//! are not folded in, so the pick surface can sit a centimetre or two off the
//! drawn pixels mid-bounce. The reference's collision volumes are far coarser.

use bevy::ecs::system::SystemParam;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;
use sl_client_bevy::{AgentKey, ScopedObjectId};

use crate::avatars::{AVATAR_SPHERE_RADIUS, AvatarPickCollider, AvatarPickTarget, AvatarSphere};
use crate::objects::WornPickTarget;

/// How far, in metres, a posed limb can plausibly reach beyond the fitted pick
/// box's bounding sphere. The box hugs the torso (fixed reference width /
/// depth), so a ray aimed at an outstretched arm can pass well outside it; the
/// broad phase widens the box's bounding sphere by this margin before deciding
/// whether an avatar is worth CPU-skinning. Generous on purpose: a false
/// positive only costs one skinning pass on an explicit pick, a false negative
/// makes a limb unclickable.
const LIMB_REACH_MARGIN: f32 = 1.5;

/// How a resolved avatar hit was computed, surfaced so the debug pick
/// inspector (and tests) can tell an exact hit from the box fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PickAccuracy {
    /// The ray hit posed geometry: CPU-skinned triangles, a rigid part's
    /// placed triangles, or the placeholder sphere.
    Mesh,
    /// The ray hit only the fitted pick box of an avatar that has no visible
    /// decoded geometry yet — the placeholder path.
    BoxFallback,
}

/// A resolved avatar pick along a ray.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AvatarRayHit {
    /// The picked avatar.
    pub(crate) agent: AgentKey,
    /// The distance along the ray, in metres, at which the avatar was hit.
    pub(crate) distance: f32,
    /// Whether posed geometry or the fallback box produced the hit.
    pub(crate) accuracy: PickAccuracy,
    /// The **worn object** the hit submesh belongs to, when the nearest
    /// triangle came from a worn rigged attachment
    /// ([`WornPickTarget`]) — such a pick resolves to the attachment pies
    /// ([`crate::attachment_menu`]), not the avatar ones. `None` when the hit
    /// is the system body, a rigid body part, the placeholder sphere, or the
    /// box fallback.
    pub(crate) worn: Option<ScopedObjectId>,
}

/// The outcome of ray testing one avatar's posed geometry.
enum MeshPickOutcome {
    /// The nearest posed-triangle intersection, in metres along the ray,
    /// with the worn object of the submesh it struck (if any).
    Hit(f32, Option<ScopedObjectId>),
    /// The avatar has visible geometry and the ray hits none of it — a
    /// mesh-accurate *no pick* (the click was off the silhouette).
    Miss,
    /// The avatar has no visible decoded geometry at all; the caller falls
    /// back to the fitted box.
    NoGeometry,
}

/// Everything a mesh-accurate avatar pick reads, bundled as one
/// [`SystemParam`] so consumers add a single parameter.
#[derive(SystemParam)]
pub(crate) struct AvatarPicker<'w, 's> {
    /// Every rigged avatar's fitted pick box: the broad-phase volume and the
    /// no-geometry fallback.
    colliders: Query<'w, 's, (&'static AvatarPickCollider, &'static GlobalTransform)>,
    /// Placeholder spheres (coarse-only avatars, or a run without
    /// `--viewer-assets`): rigid and correctly placed, intersected
    /// analytically.
    spheres:
        Query<'w, 's, (&'static AvatarPickTarget, &'static GlobalTransform), With<AvatarSphere>>,
    /// Every mesh piece tagged as part of an avatar — the skinned base-body
    /// parts, the worn rigged submeshes, and the rigid parts (eyeballs). The
    /// pick box is excluded: it is broad phase, not geometry.
    #[expect(
        clippy::type_complexity,
        reason = "a query's term list is its type; splitting it loses the single-query guarantee"
    )]
    parts: Query<
        'w,
        's,
        (
            &'static AvatarPickTarget,
            &'static Mesh3d,
            &'static InheritedVisibility,
            Option<&'static SkinnedMesh>,
            &'static GlobalTransform,
            Option<&'static WornPickTarget>,
        ),
        Without<AvatarPickCollider>,
    >,
    /// Joint-entity globals, read to rebuild the skin matrix palette exactly
    /// as the GPU sees it.
    globals: Query<'w, 's, &'static GlobalTransform>,
    /// Mesh assets, for the rest positions / joint indices / weights /
    /// triangle indices.
    meshes: Res<'w, Assets<Mesh>>,
    /// Inverse-bindpose assets, the other half of the skin palette.
    bindposes: Res<'w, Assets<SkinnedMeshInverseBindposes>>,
}

impl AvatarPicker<'_, '_> {
    /// Resolve `ray` to the nearest avatar whose posed geometry (or fallback
    /// volume) it hits, or `None` when the ray misses every avatar — including
    /// when it merely passes near one without touching its silhouette.
    pub(crate) fn pick(&self, ray: Ray3d) -> Option<AvatarRayHit> {
        let mut best: Option<AvatarRayHit> = None;
        // Placeholder spheres: rigid, correctly placed, analytic.
        for (target, global) in &self.spheres {
            if let Some(distance) =
                ray_sphere_entry(ray, global.translation(), AVATAR_SPHERE_RADIUS)
            {
                consider(
                    &mut best,
                    AvatarRayHit {
                        agent: target.agent(),
                        distance,
                        accuracy: PickAccuracy::Mesh,
                        worn: None,
                    },
                );
            }
        }
        // Rigged bodies: broad phase on the fitted box (widened for limbs),
        // then the posed triangles decide.
        for (collider, global) in &self.colliders {
            let agent = collider.agent();
            let (centre, radius) = box_bounding_sphere(global);
            if ray_sphere_entry(ray, centre, radius + LIMB_REACH_MARGIN).is_none() {
                continue;
            }
            match self.nearest_part_hit(agent, ray) {
                MeshPickOutcome::Hit(distance, worn) => consider(
                    &mut best,
                    AvatarRayHit {
                        agent,
                        distance,
                        accuracy: PickAccuracy::Mesh,
                        worn,
                    },
                ),
                // Geometry exists and the ray misses it: mesh-accurate no
                // pick for this avatar (another may still win).
                MeshPickOutcome::Miss => {}
                MeshPickOutcome::NoGeometry => {
                    if let Some(distance) = ray_box_entry(ray, global) {
                        consider(
                            &mut best,
                            AvatarRayHit {
                                agent,
                                distance,
                                accuracy: PickAccuracy::BoxFallback,
                                worn: None,
                            },
                        );
                    }
                }
            }
        }
        best
    }

    /// Ray test one avatar's visible mesh pieces against their **posed**
    /// world-space triangles: CPU-skin each skinned piece from its live joint
    /// palette, place each rigid piece by its (posed) `GlobalTransform`, and
    /// return the nearest intersection — together with the worn object of the
    /// piece it landed on, so a hit on a worn attachment submesh routes to the
    /// attachment pies rather than the avatar ones.
    fn nearest_part_hit(&self, agent: AgentKey, ray: Ray3d) -> MeshPickOutcome {
        let mut nearest: Option<(f32, Option<ScopedObjectId>)> = None;
        let mut any_geometry = false;
        for (target, mesh3d, visibility, skinned, global, worn) in &self.parts {
            if target.agent() != agent || !visibility.get() {
                continue;
            }
            let Some(mesh) = self.meshes.get(&mesh3d.0) else {
                continue;
            };
            let Some(VertexAttributeValues::Float32x3(positions)) =
                mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            else {
                continue;
            };
            let world_positions: Vec<Vec3> = match skinned {
                Some(skin) => {
                    // A skinned piece renders wherever its joint palette puts
                    // it; its own transform is irrelevant (Bevy's skinning
                    // shader replaces the model matrix with the palette).
                    let Some(palette) = self.palette(skin) else {
                        continue;
                    };
                    let (
                        Some(VertexAttributeValues::Uint16x4(joint_indices)),
                        Some(VertexAttributeValues::Float32x4(joint_weights)),
                    ) = (
                        mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX),
                        mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT),
                    )
                    else {
                        continue;
                    };
                    skinned_world_positions(positions, joint_indices, joint_weights, &palette)
                }
                // A rigid piece (an eyeball) is placed by its posed global.
                None => positions
                    .iter()
                    .map(|position| global.transform_point(Vec3::from_array(*position)))
                    .collect(),
            };
            any_geometry = true;
            if let Some(distance) = nearest_triangle_entry(&world_positions, mesh.indices(), ray) {
                let candidate = (distance, worn.map(|target| target.scoped));
                nearest = Some(nearest.map_or(candidate, |current| {
                    if distance < current.0 {
                        candidate
                    } else {
                        current
                    }
                }));
            }
        }
        match nearest {
            Some((distance, worn)) => MeshPickOutcome::Hit(distance, worn),
            None if any_geometry => MeshPickOutcome::Miss,
            None => MeshPickOutcome::NoGeometry,
        }
    }

    /// Rebuild a skinned piece's world-space matrix palette — per slot,
    /// `joint_world · inverse_bind`, exactly what the GPU palette holds —
    /// or `None` when the bindposes asset or a joint entity is gone.
    fn palette(&self, skin: &SkinnedMesh) -> Option<Vec<Mat4>> {
        let inverse_bindposes = self.bindposes.get(&skin.inverse_bindposes)?;
        let mut palette = Vec::with_capacity(inverse_bindposes.len());
        for (joint, inverse_bind) in skin.joints.iter().zip(inverse_bindposes.iter()) {
            let global = self.globals.get(*joint).ok()?;
            palette.push(Mat4::from(global.affine()).mul_mat4(inverse_bind));
        }
        Some(palette)
    }
}

/// Keep the closer of the current `best` and `candidate`.
fn consider(best: &mut Option<AvatarRayHit>, candidate: AvatarRayHit) {
    if best
        .as_ref()
        .is_none_or(|current| candidate.distance < current.distance)
    {
        *best = Some(candidate);
    }
}

/// `a - b`, written per component to stay clear of the workspace
/// `arithmetic_side_effects` lint on the glam `Vec3` operators.
const fn diff(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// CPU-reproduce the GPU matrix-palette skinning for every vertex: `world =
/// Σ wᵢ · palette[jᵢ] · rest`, with the weights used **raw** (the mesh
/// builders already stored what the GPU consumes — renormalized for a worn
/// rig, a partition of unity for a base part).
fn skinned_world_positions(
    positions: &[[f32; 3]],
    joint_indices: &[[u16; 4]],
    joint_weights: &[[f32; 4]],
    palette: &[Mat4],
) -> Vec<Vec3> {
    positions
        .iter()
        .zip(joint_indices.iter().zip(joint_weights.iter()))
        .map(|(position, (joints, weights))| {
            let rest = Vec3::from_array(*position);
            let mut skinned = Vec3::ZERO;
            for (joint, weight) in joints.iter().zip(weights.iter()) {
                if *weight <= 0.0 {
                    continue;
                }
                let Some(matrix) = palette.get(usize::from(*joint)) else {
                    continue;
                };
                skinned = matrix
                    .transform_point3(rest)
                    .mul_add(Vec3::splat(*weight), skinned);
            }
            skinned
        })
        .collect()
}

/// The nearest forward intersection of `ray` with the triangle list described
/// by `indices` over `positions` (non-indexed consecutive triples when the
/// mesh has no index buffer), or `None` if the ray misses every triangle.
fn nearest_triangle_entry(
    positions: &[Vec3],
    indices: Option<&Indices>,
    ray: Ray3d,
) -> Option<f32> {
    let mut nearest: Option<f32> = None;
    let mut consider_triangle = |a: Vec3, b: Vec3, c: Vec3| {
        if let Some(distance) = ray_triangle_entry(ray, a, b, c) {
            nearest = Some(nearest.map_or(distance, |current| current.min(distance)));
        }
    };
    match indices {
        Some(indices) => {
            let mut iter = indices.iter();
            while let (Some(a), Some(b), Some(c)) = (iter.next(), iter.next(), iter.next()) {
                let (Some(a), Some(b), Some(c)) =
                    (positions.get(a), positions.get(b), positions.get(c))
                else {
                    continue;
                };
                consider_triangle(*a, *b, *c);
            }
        }
        None => {
            for triangle in positions.chunks_exact(3) {
                if let [a, b, c] = triangle {
                    consider_triangle(*a, *b, *c);
                }
            }
        }
    }
    nearest
}

/// Möller–Trumbore ray/triangle intersection, **double-sided** (an avatar's
/// clothing and hair are routinely seen from their back faces), returning the
/// forward distance along the ray or `None`.
fn ray_triangle_entry(ray: Ray3d, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let edge_ab = diff(b, a);
    let edge_ac = diff(c, a);
    let direction = ray.direction.as_vec3();
    let p = direction.cross(edge_ac);
    let determinant = edge_ab.dot(p);
    // Parallel (or degenerate) triangle.
    if determinant.abs() < f32::EPSILON {
        return None;
    }
    let inverse_determinant = 1.0 / determinant;
    let origin_offset = diff(ray.origin, a);
    let u = origin_offset.dot(p) * inverse_determinant;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = origin_offset.cross(edge_ab);
    let v = direction.dot(q) * inverse_determinant;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = edge_ac.dot(q) * inverse_determinant;
    (distance > 0.0).then_some(distance)
}

/// The entry distance of `ray` into the sphere at `centre` with `radius`, in
/// metres along the ray: `0` when the origin is already inside, `None` when
/// the ray misses or the sphere is entirely behind it.
fn ray_sphere_entry(ray: Ray3d, centre: Vec3, radius: f32) -> Option<f32> {
    let to_centre = diff(centre, ray.origin);
    let along = to_centre.dot(ray.direction.as_vec3());
    let closest_sq = to_centre.length_squared() - along * along;
    let radius_sq = radius * radius;
    if closest_sq > radius_sq {
        return None;
    }
    let half_chord = (radius_sq - closest_sq).sqrt();
    let entry = along - half_chord;
    if entry >= 0.0 {
        Some(entry)
    } else if along + half_chord >= 0.0 {
        // The origin is inside the sphere.
        Some(0.0)
    } else {
        // Entirely behind the ray.
        None
    }
}

/// The bounding sphere of the unit-cube pick box under `global`: its world
/// centre, and a radius bounding the transformed corners (half the sum of the
/// world-space axis lengths bounds `|±½x ±½y ±½z|`).
fn box_bounding_sphere(global: &GlobalTransform) -> (Vec3, f32) {
    let matrix = global.affine().matrix3;
    let radius = 0.5 * (matrix.x_axis.length() + matrix.y_axis.length() + matrix.z_axis.length());
    (global.translation(), radius)
}

/// The entry distance of `ray` into the unit-cube pick box under `global`, in
/// **world** metres along the ray (`0` when the origin is inside), or `None`
/// on a miss. Works in the box's local frame: with the local direction taken
/// un-normalized, the slab parameter *is* the world distance.
fn ray_box_entry(ray: Ray3d, global: &GlobalTransform) -> Option<f32> {
    let inverse = global.affine().inverse();
    let origin = inverse.transform_point3(ray.origin).to_array();
    let direction = inverse
        .transform_vector3(ray.direction.as_vec3())
        .to_array();
    let mut entry = 0.0_f32;
    let mut exit = f32::INFINITY;
    for (o, d) in origin.iter().zip(direction.iter()) {
        if d.abs() < f32::EPSILON {
            // Parallel to this slab: inside it or never.
            if o.abs() > 0.5 {
                return None;
            }
            continue;
        }
        let t1 = (-0.5 - o) / d;
        let t2 = (0.5 - o) / d;
        let (near, far) = if t1 <= t2 { (t1, t2) } else { (t2, t1) };
        entry = entry.max(near);
        exit = exit.min(far);
    }
    (entry <= exit).then_some(entry)
}

#[cfg(test)]
mod tests {
    use bevy::asset::RenderAssetUsages;
    use bevy::ecs::system::SystemState;
    use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
    use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;
    use sl_client_bevy::AgentKey;

    use super::{
        AvatarPicker, PickAccuracy, ray_box_entry, ray_sphere_entry, ray_triangle_entry,
        skinned_world_positions,
    };
    use crate::avatars::{AvatarPickCollider, AvatarPickTarget};

    /// A boxed error so tests can use `?` instead of the disallowed
    /// `unwrap` / `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A ray from `origin` towards `target`, or a test error for a degenerate
    /// direction.
    fn ray_towards(origin: Vec3, target: Vec3) -> Result<Ray3d, TestError> {
        let direction = super::diff(target, origin);
        Ok(Ray3d::new(
            origin,
            Dir3::new(direction).map_err(|error| format!("ray direction: {error:?}"))?,
        ))
    }

    #[test]
    fn triangle_intersection_is_double_sided_and_forward_only() -> Result<(), TestError> {
        let (a, b, c) = (
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        // Front side.
        let front = ray_towards(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO)?;
        let hit = ray_triangle_entry(front, a, b, c).ok_or("front hit")?;
        assert!((hit - 5.0).abs() < 1e-4, "front distance, got {hit}");
        // Back side (reversed winding as seen by the ray) still hits.
        let back = ray_towards(Vec3::new(0.0, 0.0, -5.0), Vec3::ZERO)?;
        let hit = ray_triangle_entry(back, a, b, c).ok_or("back hit")?;
        assert!((hit - 5.0).abs() < 1e-4, "back distance, got {hit}");
        // A triangle behind the ray is not hit.
        let away = ray_towards(Vec3::new(0.0, 0.0, 5.0), Vec3::new(0.0, 0.0, 10.0))?;
        assert!(
            ray_triangle_entry(away, a, b, c).is_none(),
            "behind the ray must miss"
        );
        // Off the silhouette misses.
        let off = ray_towards(Vec3::new(5.0, 0.0, 5.0), Vec3::new(5.0, 0.0, -5.0))?;
        assert!(
            ray_triangle_entry(off, a, b, c).is_none(),
            "off-triangle must miss"
        );
        Ok(())
    }

    #[test]
    fn sphere_entry_handles_outside_inside_and_behind() -> Result<(), TestError> {
        let centre = Vec3::new(0.0, 0.0, -10.0);
        let ray = ray_towards(Vec3::ZERO, centre)?;
        let entry = ray_sphere_entry(ray, centre, 2.0).ok_or("outside hit")?;
        assert!((entry - 8.0).abs() < 1e-4, "entry distance, got {entry}");
        // Origin inside the sphere clamps to zero.
        let inside = ray_sphere_entry(ray, Vec3::new(0.0, 0.0, -1.0), 2.0).ok_or("inside hit")?;
        assert!(inside.abs() < 1e-6, "inside must clamp to 0, got {inside}");
        // A sphere behind the origin misses.
        assert!(
            ray_sphere_entry(ray, Vec3::new(0.0, 0.0, 10.0), 2.0).is_none(),
            "behind must miss"
        );
        // A sideways offset larger than the radius misses.
        assert!(
            ray_sphere_entry(ray, Vec3::new(5.0, 0.0, -10.0), 2.0).is_none(),
            "off-axis must miss"
        );
        Ok(())
    }

    #[test]
    fn box_entry_respects_scale_rotation_and_inside_clamp() -> Result<(), TestError> {
        // A box scaled to (0.45, 0.6, 2.0), rotated a quarter turn about Y,
        // sitting 10 m down the -Z axis: the ray meets the rotated local-x
        // slab (depth 0.45) first, at 10 − 0.225.
        let global = GlobalTransform::from(Transform {
            translation: Vec3::new(0.0, 0.0, -10.0),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            scale: Vec3::new(0.45, 0.6, 2.0),
        });
        let ray = ray_towards(Vec3::ZERO, Vec3::new(0.0, 0.0, -10.0))?;
        let entry = ray_box_entry(ray, &global).ok_or("box hit")?;
        assert!(
            (entry - (10.0 - 0.225)).abs() < 1e-3,
            "rotated-slab entry, got {entry}"
        );
        // An origin inside the box clamps to zero.
        let around_origin = GlobalTransform::from(Transform::from_scale(Vec3::splat(4.0)));
        let inside = ray_box_entry(ray, &around_origin).ok_or("inside hit")?;
        assert!(inside.abs() < 1e-6, "inside must clamp to 0, got {inside}");
        // Aimed past the box: miss.
        let off = ray_towards(Vec3::new(5.0, 0.0, 0.0), Vec3::new(5.0, 0.0, -10.0))?;
        assert!(ray_box_entry(off, &global).is_none(), "off-box must miss");
        Ok(())
    }

    #[test]
    fn skinning_follows_the_palette_and_blends_weights() {
        // Two palette slots: identity, and a +2 m X translation.
        let palette = vec![
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)),
        ];
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]];
        let joint_indices = [[0, 0, 0, 0], [1, 0, 0, 0], [0, 1, 0, 0]];
        let joint_weights = [
            [1.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.5, 0.5, 0.0, 0.0],
        ];
        let skinned = skinned_world_positions(&positions, &joint_indices, &joint_weights, &palette);
        assert_eq!(
            skinned,
            vec![
                // Fully on the identity joint: unmoved.
                Vec3::ZERO,
                // Fully on the translated joint: carried +2 m.
                Vec3::new(3.0, 0.0, 0.0),
                // An even blend: carried half way.
                Vec3::new(2.0, 0.0, 0.0),
            ]
        );
    }

    /// A single-triangle skinned mesh whose one palette slot is driven by
    /// `joint`: at rest the triangle spans ±1 m around the origin in the XY
    /// plane.
    fn skinned_triangle_mesh() -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[-1.0_f32, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]],
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(vec![[0, 0, 0, 0]; 3]),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_JOINT_WEIGHT,
            vec![[1.0_f32, 0.0, 0.0, 0.0]; 3],
        )
        .with_inserted_indices(Indices::U16(vec![0, 1, 2]))
    }

    /// The world the ECS-level tests share: mesh / bindpose assets plus one
    /// rigged avatar (collider + one skinned triangle posed by one joint).
    ///
    /// The joint carries a +5 m X translation, so the posed triangle sits at
    /// `x = 5` while its bind pose spans the origin — a pick aimed at either
    /// place tells exactly which geometry was tested. Returns the world and
    /// the avatar's agent key.
    fn rigged_pick_world(part_visibility: Visibility) -> (World, AgentKey) {
        let agent = AgentKey::from(sl_client_bevy::Uuid::from_u128(0xA));
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<SkinnedMeshInverseBindposes>>();
        let mesh = world
            .resource_mut::<Assets<Mesh>>()
            .add(skinned_triangle_mesh());
        let bindposes = world
            .resource_mut::<Assets<SkinnedMeshInverseBindposes>>()
            .add(SkinnedMeshInverseBindposes::from(vec![Mat4::IDENTITY]));
        // The posed joint: +5 m X.
        let joint = world
            .spawn(GlobalTransform::from(Transform::from_translation(
                Vec3::new(5.0, 0.0, 0.0),
            )))
            .id();
        // The skinned part. `InheritedVisibility` is normally computed by the
        // visibility propagation systems; the tests set it directly.
        let inherited = if part_visibility == Visibility::Hidden {
            InheritedVisibility::HIDDEN
        } else {
            InheritedVisibility::VISIBLE
        };
        world.spawn((
            AvatarPickTarget::new(agent),
            Mesh3d(mesh),
            inherited,
            SkinnedMesh {
                inverse_bindposes: bindposes,
                joints: vec![joint],
            },
            GlobalTransform::default(),
        ));
        // The fitted box, centred between the bind and posed locations so the
        // broad phase admits both aims (and the box alone would wrongly catch
        // a bind-pose aim).
        world.spawn((
            AvatarPickCollider::new(agent),
            GlobalTransform::from(Transform {
                translation: Vec3::new(2.5, 0.0, 0.0),
                scale: Vec3::new(0.45, 0.6, 2.0),
                ..Transform::default()
            }),
        ));
        (world, agent)
    }

    /// Run one pick against `world`.
    fn pick_in(world: &mut World, ray: Ray3d) -> Result<Option<super::AvatarRayHit>, TestError> {
        let mut state = SystemState::<AvatarPicker>::new(world);
        let picker = state
            .get(world)
            .map_err(|error| format!("picker params: {error}"))?;
        Ok(picker.pick(ray))
    }

    #[test]
    fn pick_hits_the_posed_triangles_not_the_bind_pose() -> Result<(), TestError> {
        let (mut world, agent) = rigged_pick_world(Visibility::Inherited);
        // Aimed at the POSED location (x = 5): hit, mesh-accurate.
        let posed = ray_towards(Vec3::new(5.0, 0.0, 10.0), Vec3::new(5.0, 0.0, 0.0))?;
        let hit = pick_in(&mut world, posed)?.ok_or("posed aim must hit")?;
        assert_eq!(hit.agent, agent);
        assert_eq!(hit.accuracy, PickAccuracy::Mesh);
        assert!((hit.distance - 10.0).abs() < 1e-3, "got {}", hit.distance);
        // Aimed at the BIND location (the origin): a mesh-accurate miss even
        // though the broad phase admits the avatar.
        let bind = ray_towards(Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO)?;
        assert!(
            pick_in(&mut world, bind)?.is_none(),
            "a bind-pose aim must not pick"
        );
        // Aimed straight through the fitted box (centre x = 2.5) but off the
        // posed triangles: with geometry present the box must NOT pick — this
        // is the silhouette accuracy the box approximation lacked.
        let through_box = ray_towards(Vec3::new(2.5, 0.0, 10.0), Vec3::new(2.5, 0.0, 0.0))?;
        assert!(
            pick_in(&mut world, through_box)?.is_none(),
            "the box must not pick when geometry is visible"
        );
        Ok(())
    }

    #[test]
    fn pick_falls_back_to_the_box_only_without_visible_geometry() -> Result<(), TestError> {
        // All geometry hidden: the box is the only pickable stand-in.
        let (mut world, agent) = rigged_pick_world(Visibility::Hidden);
        let at_box = ray_towards(Vec3::new(2.5, 0.0, 10.0), Vec3::new(2.5, 0.0, 0.0))?;
        let hit = pick_in(&mut world, at_box)?.ok_or("box fallback must hit")?;
        assert_eq!(hit.agent, agent);
        assert_eq!(hit.accuracy, PickAccuracy::BoxFallback);
        // Near the (hidden) posed triangle but outside the box: no pick — the
        // fallback is the box, not the widened broad-phase sphere.
        let past_box = ray_towards(Vec3::new(5.0, 0.0, 10.0), Vec3::new(5.0, 0.0, 0.0))?;
        assert!(
            pick_in(&mut world, past_box)?.is_none(),
            "the widened broad phase must not itself pick"
        );
        Ok(())
    }

    /// A hit on a submesh tagged as a worn attachment reports the worn object
    /// (so the resolver can open the attachment pies), while an untagged part
    /// — the system body — reports none.
    #[test]
    fn pick_reports_the_worn_object_of_an_attachment_submesh() -> Result<(), TestError> {
        use sl_client_bevy::{CircuitId, RegionLocalObjectId, ScopedObjectId};

        let (mut world, agent) = rigged_pick_world(Visibility::Inherited);
        let posed = ray_towards(Vec3::new(5.0, 0.0, 10.0), Vec3::new(5.0, 0.0, 0.0))?;
        // Untagged (a base body part): no worn object.
        let hit = pick_in(&mut world, posed)?.ok_or("posed aim must hit")?;
        assert_eq!(hit.agent, agent);
        assert_eq!(hit.worn, None, "an untagged part must not report worn");
        // Tag the part as a worn attachment submesh: the same hit now carries
        // the worn object's identity.
        let scoped = ScopedObjectId::new(CircuitId::new(1), RegionLocalObjectId::new(42));
        let parts: Vec<Entity> = world
            .query_filtered::<Entity, With<AvatarPickTarget>>()
            .iter(&world)
            .collect();
        for part in parts {
            world
                .entity_mut(part)
                .insert(crate::objects::WornPickTarget { scoped });
        }
        let hit = pick_in(&mut world, posed)?.ok_or("tagged aim must hit")?;
        assert_eq!(
            hit.worn,
            Some(scoped),
            "a worn submesh hit must resolve its worn object"
        );
        Ok(())
    }

    #[test]
    fn pick_reaches_a_limb_outside_the_box() -> Result<(), TestError> {
        // The posed triangle (x = 5) sits ~2.3 m outside the box's +x face
        // (box centre x = 2.5, world half-extent 0.225): an outstretched limb.
        // The widened broad phase must still admit the avatar so the triangle
        // itself decides.
        let (mut world, agent) = rigged_pick_world(Visibility::Inherited);
        let at_limb = ray_towards(Vec3::new(4.8, 0.5, 10.0), Vec3::new(4.8, 0.5, 0.0))?;
        let hit = pick_in(&mut world, at_limb)?.ok_or("limb aim must hit")?;
        assert_eq!(hit.agent, agent);
        assert_eq!(hit.accuracy, PickAccuracy::Mesh);
        Ok(())
    }
}
