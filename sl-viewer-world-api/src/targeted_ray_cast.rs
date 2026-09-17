//! A mesh ray cast over a **named set of entities**, not the whole scene.
//!
//! Bevy's [`MeshRayCast`] runs its AABB broad phase — a `par_iter` over
//! **every** mesh entity in the world — before it ever consults the settings'
//! filter. A filter that admits a
//! single face, or only the few HUD meshes, therefore still pays for a sweep of
//! the whole region: tens of thousands of AABB tests farmed out to the compute
//! pool, whose join waits on whatever else those workers are busy with. The
//! hover tooltip ran exactly that sweep on every dwelt frame, just to learn that
//! no HUD attachment was under the cursor, and it was the whole of that system's
//! cost (`viewer-hover-tooltip-202ms-frame-spike`).
//!
//! [`TargetedRayCast`] takes the candidates up front instead, so its cost is the
//! size of the candidate set. It keeps `MeshRayCast`'s semantics for the cases
//! the viewer uses: an entity needs a [`Mesh3d`], a [`GlobalTransform`] and an
//! [`Aabb`] to be struck, backfaces are culled unless the entity carries
//! [`RayCastBackfaces`], candidates are tested nearest-AABB first, and the walk
//! stops at the first surface hit whose distance no remaining box can beat.

use bevy::camera::primitives::Aabb;
use bevy::ecs::system::SystemParam;
use bevy::math::FloatOrd;
use bevy::math::bounding::Aabb3d;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::picking::mesh_picking::ray_cast::{
    Backfaces, RayMeshHit, ray_aabb_intersection_3d, ray_mesh_intersection,
};
use bevy::prelude::*;

/// Which candidates a [`TargetedRayCast`] may strike, by visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetVisibility {
    /// Ignore visibility: a hidden candidate is struck too (a refinement of a
    /// pick that has already established the entity was drawn).
    Any,
    /// Only candidates visible in the hierarchy ([`InheritedVisibility`]) — the
    /// HUD's test, whose geometry no world camera's per-view flag describes.
    Visible,
}

/// The mesh ray caster over an explicit candidate set — see the module docs.
#[expect(
    missing_debug_implementations,
    reason = "it holds Bevy's `Assets<Mesh>`, which does not implement Debug; a \
              hand-written impl could only print the field names"
)]
#[derive(SystemParam)]
pub struct TargetedRayCast<'w, 's> {
    /// The pieces a candidate is tested through.
    targets: Query<
        'w,
        's,
        (
            &'static Mesh3d,
            &'static GlobalTransform,
            &'static Aabb,
            &'static InheritedVisibility,
            Has<RayCastBackfaces>,
        ),
    >,
    /// The mesh assets the narrow phase reads the triangles from.
    meshes: Res<'w, Assets<Mesh>>,
}

impl TargetedRayCast<'_, '_> {
    /// The nearest surface `ray` strikes among `candidates`, with the entity it
    /// belongs to; `None` when it strikes none of them. A candidate lacking a
    /// mesh, transform or bounding box (or whose mesh asset is not loaded) is
    /// silently skipped, as `MeshRayCast` skips it.
    #[must_use]
    pub fn nearest(
        &self,
        ray: Ray3d,
        candidates: impl IntoIterator<Item = Entity>,
        visibility: TargetVisibility,
    ) -> Option<(Entity, RayMeshHit)> {
        // Broad phase over the candidates only.
        let mut boxes: Vec<(FloatOrd, Entity)> = candidates
            .into_iter()
            .filter_map(|entity| {
                let (_mesh, transform, aabb, inherited, _backfaces) =
                    self.targets.get(entity).ok()?;
                if visibility == TargetVisibility::Visible && !inherited.get() {
                    return None;
                }
                ray_aabb_intersection_3d(
                    ray,
                    &Aabb3d::new(aabb.center, aabb.half_extents),
                    &transform.affine(),
                )
                .map(|distance| (FloatOrd(distance), entity))
            })
            .collect();
        boxes.sort_unstable_by_key(|(near, _entity)| *near);

        // Narrow phase, nearest box first; once a box starts beyond the
        // nearest surface already found, nothing later can be nearer.
        let mut nearest: Option<(Entity, RayMeshHit)> = None;
        for (near, entity) in boxes {
            if nearest
                .as_ref()
                .is_some_and(|(_entity, hit)| near > FloatOrd(hit.distance))
            {
                break;
            }
            let Ok((mesh, transform, _aabb, _inherited, backfaces)) = self.targets.get(entity)
            else {
                continue;
            };
            let Some(mesh) = self.meshes.get(&mesh.0) else {
                continue;
            };
            let cull = if backfaces {
                Backfaces::Include
            } else {
                Backfaces::Cull
            };
            if let Some(hit) = ray_over_mesh(mesh, &transform.affine(), ray, cull)
                && nearest
                    .as_ref()
                    .is_none_or(|(_entity, best)| hit.distance < best.distance)
            {
                nearest = Some((entity, hit));
            }
        }
        nearest
    }
}

/// Intersect `ray` with one triangle-list mesh under `transform` — Bevy's own
/// (crate-private) `ray_intersection_over_mesh`, reading the same attributes:
/// positions required, normals and the first UV set when present.
fn ray_over_mesh(
    mesh: &Mesh,
    transform: &bevy::math::Affine3A,
    ray: Ray3d,
    cull: Backfaces,
) -> Option<RayMeshHit> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let positions = mesh
        .try_attribute(Mesh::ATTRIBUTE_POSITION)
        .ok()?
        .as_float3()?;
    let normals = mesh
        .try_attribute(Mesh::ATTRIBUTE_NORMAL)
        .ok()
        .and_then(VertexAttributeValues::as_float3);
    let uvs = mesh
        .try_attribute(Mesh::ATTRIBUTE_UV_0)
        .ok()
        .and_then(|uvs| match uvs {
            VertexAttributeValues::Float32x2(uvs) => Some(uvs.as_slice()),
            _other => None,
        });
    match mesh.try_indices().ok() {
        Some(Indices::U16(indices)) => {
            ray_mesh_intersection(ray, transform, positions, normals, Some(indices), uvs, cull)
        }
        Some(Indices::U32(indices)) => {
            ray_mesh_intersection(ray, transform, positions, normals, Some(indices), uvs, cull)
        }
        None => ray_mesh_intersection::<u32>(ray, transform, positions, normals, None, uvs, cull),
    }
}

#[cfg(test)]
mod tests {
    use super::{TargetVisibility, TargetedRayCast};
    use bevy::camera::primitives::Aabb;
    use bevy::prelude::*;
    use pretty_assertions::assert_eq;

    /// What a test system observed: the struck entity, by visibility mode.
    #[derive(Resource, Default)]
    struct Observed {
        /// The nearest hit ignoring visibility.
        any: Option<Entity>,
        /// The nearest hit among visible candidates.
        visible: Option<Entity>,
        /// The nearest hit when only the far cube is a candidate.
        far_only: Option<Entity>,
    }

    /// The two cubes the ray passes through, near first.
    #[derive(Resource)]
    struct Cubes {
        /// The cube nearer the ray origin (hidden).
        near: Entity,
        /// The cube further along the ray.
        far: Entity,
    }

    /// Cast down −Z through both cubes three ways and record the answers.
    fn cast(cast: TargetedRayCast, cubes: Res<Cubes>, mut observed: ResMut<Observed>) {
        let ray = Ray3d::new(Vec3::new(0.0, 0.0, 10.0), Dir3::NEG_Z);
        let both = [cubes.far, cubes.near];
        observed.any = cast
            .nearest(ray, both, TargetVisibility::Any)
            .map(|(entity, _hit)| entity);
        observed.visible = cast
            .nearest(ray, both, TargetVisibility::Visible)
            .map(|(entity, _hit)| entity);
        observed.far_only = cast
            .nearest(ray, [cubes.far], TargetVisibility::Any)
            .map(|(entity, _hit)| entity);
    }

    /// The nearest candidate wins regardless of the order it was named in, a
    /// hidden one is skipped only under `Visible`, and an entity left out of
    /// the candidate set is never struck even when it is nearer.
    #[test]
    fn nearest_respects_candidates_order_and_visibility() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_resource::<Observed>();
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(1.0, 1.0, 1.0));
        let aabb = Aabb::from_min_max(Vec3::splat(-0.5), Vec3::splat(0.5));
        let near = app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                GlobalTransform::from_translation(Vec3::new(0.0, 0.0, 3.0)),
                aabb,
                InheritedVisibility::HIDDEN,
            ))
            .id();
        let far = app
            .world_mut()
            .spawn((
                Mesh3d(mesh),
                GlobalTransform::from_translation(Vec3::ZERO),
                aabb,
                InheritedVisibility::VISIBLE,
            ))
            .id();
        app.insert_resource(Cubes { near, far })
            .add_systems(Update, cast);
        app.update();

        let observed = app.world().resource::<Observed>();
        assert_eq!(observed.any, Some(near));
        assert_eq!(observed.visible, Some(far));
        assert_eq!(observed.far_only, Some(far));
    }
}
