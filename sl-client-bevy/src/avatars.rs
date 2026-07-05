//! Bevy integration for the pure [`sl_avatar`] crate (P13.1): the system-avatar
//! counterpart of [`meshes`](crate::meshes) and [`prims`](crate::prims).
//!
//! Two bridges are provided, mirroring the mesh/prim `to_bevy_*` helpers:
//!
//! - [`to_bevy_base_mesh`] turns one decoded base-body part
//!   ([`sl_avatar::BaseMesh`]) into a Bevy [`Mesh`] carrying the `JOINT_INDEX` /
//!   `JOINT_WEIGHT` attributes a `SkinnedMesh` needs, alongside position /
//!   normal / UV0.
//! - [`BevySkeleton`] converts a parsed [`sl_avatar::Skeleton`] into the data a
//!   per-avatar Bevy skeleton *instance* is spawned from: each joint's local
//!   rest [`Transform`], its parent index, and its rest (bind-pose) global
//!   matrix — the raw material for the `SkinnedMeshInverseBindposes` a base part
//!   is skinned against. [`BevySkeleton::base_mesh_skin`] resolves a base part's
//!   own joint-name table against the skeleton so the caller can fill a
//!   `SkinnedMesh`.
//!
//! Like the rest of this crate's `to_bevy_*` bridges, geometry and joint
//! transforms are kept in Second Life's right-handed **Z-up** space; the single
//! Second Life → Bevy axis change is applied once, by the viewer, at the root
//! entity that carries the whole avatar (as terrain and object meshes already
//! do). Spawning the actual joint / mesh entities is the viewer's job (P13.2);
//! this module stays free of `World` / `Commands`, producing only the
//! conversion data.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::math::{Mat4, Quat, Vec3};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::transform::components::Transform;
use sl_avatar::{BaseMesh, Joint, MorphedMesh, SkeletalDeformations, Skeleton};

/// Converts one decoded base-body part into a Bevy [`Mesh`] (a `TriangleList`
/// with position, normal, and UV0 attributes plus `u32` indices).
///
/// When the part carries per-vertex skin weights, the mesh also gets the
/// `JOINT_INDEX` (`Uint16x4`) and `JOINT_WEIGHT` (`Float32x4`) attributes a Bevy
/// `SkinnedMesh` consumes. The legacy base body binds each vertex between two
/// *adjacent* joints in the part's own joint-name table (`joint` and
/// `joint + 1`, blended by [`VertexSkinWeight::blend`](sl_avatar::VertexSkinWeight::blend)),
/// so only the first two of Bevy's four influence slots are used and the joint
/// indices are the part-local table indices — the caller fills
/// `SkinnedMesh.joints` in that same order (see
/// [`BevySkeleton::base_mesh_skin`]).
#[must_use]
pub fn to_bevy_base_mesh(base: &BaseMesh) -> Mesh {
    build_base_mesh(base, base.positions(), base.normals())
}

/// Converts a base-body part into a Bevy [`Mesh`] using morphed geometry (P13.3):
/// identical to [`to_bevy_base_mesh`] but the positions and normals come from a
/// [`MorphedMesh`] ([`MorphWeights::apply`](sl_avatar::MorphWeights::apply))
/// so the body takes its real, visual-param-driven shape. The UV, skin, and index
/// data are unchanged, so the result stays skin-compatible with the un-morphed
/// mesh (same vertex count and `JOINT_INDEX` / `JOINT_WEIGHT` attributes), and a
/// re-morph simply swaps this mesh on the same skeleton instance.
#[must_use]
pub fn to_bevy_morphed_mesh(base: &BaseMesh, morphed: &MorphedMesh) -> Mesh {
    build_base_mesh(base, morphed.positions(), morphed.normals())
}

/// Shared builder for [`to_bevy_base_mesh`] / [`to_bevy_morphed_mesh`]: builds the
/// `TriangleList` from the given per-vertex `positions` / `normals` (either the
/// base rest values or their morphed counterparts) plus the part's own UV, skin,
/// and face data.
fn build_base_mesh(base: &BaseMesh, positions: &[[f32; 3]], normals: &[[f32; 3]]) -> Mesh {
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.to_vec());
    if !normals.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.to_vec());
    }
    if !base.tex_coords().is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, base.tex_coords().to_vec());
    }
    if base.has_weights() {
        let last_joint = base.joint_names().len().saturating_sub(1);
        let mut joint_indices: Vec<[u16; 4]> = Vec::with_capacity(base.weights().len());
        let mut joint_weights: Vec<[f32; 4]> = Vec::with_capacity(base.weights().len());
        for weight in base.weights() {
            let first = clamp_joint_index(weight.joint, last_joint);
            let second = clamp_joint_index(weight.joint.saturating_add(1), last_joint);
            let blend = weight.blend.clamp(0.0, 1.0);
            joint_indices.push([first, second, 0, 0]);
            joint_weights.push([1.0 - blend, blend, 0.0, 0.0]);
        }
        // `Vec<[u16; 4]>` has no `Into<VertexAttributeValues>` (its `TryFrom` is
        // ambiguous between `Uint16x4` and `Unorm16x4`), so name the variant.
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(joint_indices),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, joint_weights);
    }
    let mut indices: Vec<u32> = Vec::with_capacity(base.faces().len().saturating_mul(3));
    for face in base.faces() {
        let &[a, b, c] = face;
        indices.push(u32::from(a));
        indices.push(u32::from(b));
        indices.push(u32::from(c));
    }
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Clamps a base-mesh joint-table index to the last valid slot and narrows it to
/// the `u16` a Bevy `JOINT_INDEX` attribute holds (the base body's joint tables
/// are tiny, so this never truncates in practice).
fn clamp_joint_index(joint: usize, last_joint: usize) -> u16 {
    u16::try_from(joint.min(last_joint)).unwrap_or(u16::MAX)
}

/// The skinning inputs for one base part resolved against a [`BevySkeleton`]: the
/// skeleton joint indices its own joint-name table maps to (in table order) and
/// the parallel inverse bind matrices.
///
/// The caller turns [`joints`](Self::joints) into the `SkinnedMesh.joints`
/// entity list (index → the skeleton instance's spawned joint entity) and
/// uploads [`inverse_bindposes`](Self::inverse_bindposes) as a
/// `SkinnedMeshInverseBindposes` asset. Both are ordered to match the mesh's
/// `JOINT_INDEX` attribute produced by [`to_bevy_base_mesh`].
#[derive(Clone, Debug, PartialEq)]
pub struct BaseMeshSkin {
    /// Skeleton joint indices, one per entry of the part's joint-name table.
    pub joints: Vec<usize>,
    /// Inverse bind matrices (Second Life Z-up space), parallel to
    /// [`joints`](Self::joints).
    pub inverse_bindposes: Vec<Mat4>,
}

/// A per-avatar Bevy skeleton, converted from a parsed [`sl_avatar::Skeleton`].
///
/// Holds, in the skeleton's own joint order (parents before children), each
/// joint's local rest [`Transform`], its parent joint index, and its rest global
/// (bind-pose) matrix — everything the viewer needs to spawn a joint-entity
/// hierarchy and skin the base meshes to it. Transforms stay in Second Life
/// Z-up space; the viewer applies the axis change once at the avatar root.
#[derive(Clone, Debug)]
pub struct BevySkeleton {
    /// Local rest transform of each joint (relative to its parent), Z-up.
    locals: Vec<Transform>,
    /// Parent joint index of each joint, or `None` for a root.
    parents: Vec<Option<usize>>,
    /// Global rest (bind-pose) matrix of each joint, Z-up.
    bind_globals: Vec<Mat4>,
    /// Each joint's canonical name, parallel to [`locals`](Self::locals); used to
    /// look a joint's skeletal deformation up by bone name.
    names: Vec<String>,
    /// Joint canonical-name / alias → index (a canonical name wins over an
    /// alias, matching [`Skeleton`]'s own lookup).
    lookup: HashMap<String, usize>,
}

impl BevySkeleton {
    /// Builds the Bevy skeleton data from a parsed [`Skeleton`].
    ///
    /// The joint order is preserved, so index `i` here is joint `i` of the
    /// source skeleton. Because a parent always precedes its children, each
    /// joint's global matrix is composed from its already-computed parent.
    #[must_use]
    pub fn from_skeleton(skeleton: &Skeleton) -> Self {
        let joints = skeleton.joints();
        let mut locals = Vec::with_capacity(joints.len());
        let mut parents = Vec::with_capacity(joints.len());
        let mut names = Vec::with_capacity(joints.len());
        let mut bind_globals: Vec<Mat4> = Vec::with_capacity(joints.len());
        for joint in joints {
            let local = joint_transform(joint);
            let parent_global = joint
                .parent
                .and_then(|parent| bind_globals.get(parent).copied())
                .unwrap_or(Mat4::IDENTITY);
            // `Mat4::mul_mat4` rather than `*` to stay clear of the workspace
            // `arithmetic_side_effects` lint.
            bind_globals.push(parent_global.mul_mat4(&local.to_matrix()));
            parents.push(joint.parent);
            names.push(joint.name.clone());
            locals.push(local);
        }

        // Rebuild the name/alias lookup with the same precedence `Skeleton` uses
        // (aliases first, canonical names overwrite) so this type is standalone.
        let mut lookup = HashMap::new();
        for (index, joint) in joints.iter().enumerate() {
            for alias in &joint.aliases {
                lookup.entry(alias.clone()).or_insert(index);
            }
        }
        for (index, joint) in joints.iter().enumerate() {
            lookup.insert(joint.name.clone(), index);
        }

        Self {
            locals,
            parents,
            bind_globals,
            names,
            lookup,
        }
    }

    /// The number of joints.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.locals.len()
    }

    /// Whether the skeleton has no joints.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.locals.is_empty()
    }

    /// The local rest transforms, in joint order (parents before children); the
    /// viewer spawns one joint entity per entry with this as its `Transform`.
    #[must_use]
    pub fn local_transforms(&self) -> &[Transform] {
        &self.locals
    }

    /// The parent joint index of each joint (`None` for a root), in joint order;
    /// the viewer parents each spawned joint entity accordingly.
    #[must_use]
    pub fn parents(&self) -> &[Option<usize>] {
        &self.parents
    }

    /// The per-joint **local** rest transforms deformed by `deform`
    /// (`param_skeleton` scale / offset), in joint order — what the viewer sets
    /// each spawned joint entity's `Transform` to so a shaped avatar's
    /// proportions match (P13.4). At rest (no deformation) this equals
    /// [`local_transforms`](Self::local_transforms).
    ///
    /// The Second Life skeleton has semantics a plain nested transform hierarchy
    /// cannot express: a bone's own scale stretches only its bound geometry (it
    /// is *not* inherited into a child's world scale, unlike
    /// `LLAvatarJointCollisionVolume`), while a parent's *local* scale does
    /// stretch its immediate child's position offset (the `scaleChildOffset`
    /// mechanism that drives height / limb length — Firestorm `LLXformMatrix`).
    /// So the deformed **world** matrix of each joint is built by that exact
    /// recurrence here, and each returned local transform is
    /// `parent_world⁻¹ · own_world` — the relative transform that, re-composed by
    /// Bevy's ordinary hierarchy propagation, reproduces the correct world matrix
    /// regardless of how Bevy accumulates scale. (For the transmitted skeletal
    /// params, adjacent scaled bones are axis-aligned, so these relative
    /// transforms carry no shear and decompose losslessly into a `Transform`.)
    ///
    /// The rest (bind-pose) globals — and hence the inverse bindposes a base part
    /// is skinned against — are left untouched, so the deformation shows up as
    /// the skin's deviation from its bind pose.
    #[must_use]
    pub fn deformed_local_transforms(&self, deform: &SkeletalDeformations) -> Vec<Transform> {
        // First pass: each joint's deformed world position / rotation / local
        // scale, and the full world matrix, by the Second Life recurrence.
        let mut world_rot: Vec<Quat> = Vec::with_capacity(self.locals.len());
        let mut world_pos: Vec<Vec3> = Vec::with_capacity(self.locals.len());
        let mut local_scale: Vec<Vec3> = Vec::with_capacity(self.locals.len());
        let mut world: Vec<Mat4> = Vec::with_capacity(self.locals.len());
        for (index, local) in self.locals.iter().enumerate() {
            let name = self.names.get(index).map_or("", String::as_str);
            let deform_scale = deform.scale(name);
            let deform_offset = deform.offset(name);
            // Component-wise so the workspace `arithmetic_side_effects` lint does
            // not trip on the glam `Vec3` operators.
            let scale = Vec3::new(
                local.scale.x + deform_scale[0],
                local.scale.y + deform_scale[1],
                local.scale.z + deform_scale[2],
            );
            let position = Vec3::new(
                local.translation.x + deform_offset[0],
                local.translation.y + deform_offset[1],
                local.translation.z + deform_offset[2],
            );
            let (rotation, translation) = match self.parents.get(index).copied().flatten() {
                Some(parent) => {
                    let parent_rot = world_rot.get(parent).copied().unwrap_or(Quat::IDENTITY);
                    let parent_pos = world_pos.get(parent).copied().unwrap_or(Vec3::ZERO);
                    let parent_scale = local_scale.get(parent).copied().unwrap_or(Vec3::ONE);
                    // Child offset scaled by the parent's *local* scale, rotated
                    // into and translated by the parent's world frame.
                    let scaled = Vec3::new(
                        parent_scale.x * position.x,
                        parent_scale.y * position.y,
                        parent_scale.z * position.z,
                    );
                    let rotated = parent_rot.mul_vec3(scaled);
                    (
                        parent_rot.mul_quat(local.rotation),
                        Vec3::new(
                            parent_pos.x + rotated.x,
                            parent_pos.y + rotated.y,
                            parent_pos.z + rotated.z,
                        ),
                    )
                }
                None => (local.rotation, position),
            };
            world_rot.push(rotation);
            world_pos.push(translation);
            local_scale.push(scale);
            world.push(Mat4::from_scale_rotation_translation(
                scale,
                rotation,
                translation,
            ));
        }

        // Second pass: relative-to-parent local transforms.
        let mut out = Vec::with_capacity(self.locals.len());
        for (index, own) in world.iter().enumerate() {
            let matrix = match self.parents.get(index).copied().flatten() {
                Some(parent) => world
                    .get(parent)
                    .map_or(*own, |parent_world| parent_world.inverse().mul_mat4(own)),
                None => *own,
            };
            out.push(Transform::from_matrix(matrix));
        }
        out
    }

    /// The index of the joint with the given canonical name or alias.
    #[must_use]
    pub fn find(&self, name: &str) -> Option<usize> {
        self.lookup.get(name).copied()
    }

    /// The inverse bind matrix of the joint at `index`, or `None` if out of
    /// range.
    #[must_use]
    pub fn inverse_bindpose(&self, index: usize) -> Option<Mat4> {
        self.bind_globals.get(index).map(|global| global.inverse())
    }

    /// Resolves a base part's own joint-name table against this skeleton,
    /// producing the [`BaseMeshSkin`] the caller feeds into a `SkinnedMesh`.
    ///
    /// Returns `None` if any of the part's joint names is absent from the
    /// skeleton (the part cannot be skinned to it).
    #[must_use]
    pub fn base_mesh_skin(&self, base: &BaseMesh) -> Option<BaseMeshSkin> {
        let joints: Vec<usize> = base
            .joint_names()
            .iter()
            .map(|name| self.find(name))
            .collect::<Option<Vec<usize>>>()?;
        let inverse_bindposes: Vec<Mat4> = joints
            .iter()
            .map(|&index| self.inverse_bindpose(index))
            .collect::<Option<Vec<Mat4>>>()?;
        Some(BaseMeshSkin {
            joints,
            inverse_bindposes,
        })
    }
}

/// Builds a joint's local rest [`Transform`] from its Second Life fields (Z-up
/// metres, Euler XYZ degrees, unitless scale).
fn joint_transform(joint: &Joint) -> Transform {
    Transform {
        translation: Vec3::from_array(joint.pos),
        rotation: euler_deg_to_quat(joint.rot),
        scale: Vec3::from_array(joint.scale),
    }
}

/// Converts Second Life Euler XYZ angles (in degrees) into a Bevy [`Quat`],
/// matching Firestorm's `mayaQ(x, y, z, LLQuaternion::XYZ)` — the rotation that
/// applies X, then Y, then Z.
///
/// In Second Life's row-vector convention `mayaQ` is `xQ * yQ * zQ`; expressed
/// in glam's column-vector convention (where `a * b` applies `b` first) that
/// same rotation is `zQ * yQ * xQ`.
fn euler_deg_to_quat(rot: [f32; 3]) -> Quat {
    let [x, y, z] = rot;
    // `Quat::mul_quat` rather than the `*` operator to stay clear of the
    // workspace `arithmetic_side_effects` lint.
    Quat::from_rotation_z(z.to_radians())
        .mul_quat(Quat::from_rotation_y(y.to_radians()))
        .mul_quat(Quat::from_rotation_x(x.to_radians()))
}

#[cfg(test)]
mod tests {
    use super::{BevySkeleton, to_bevy_base_mesh};
    use bevy::math::Vec3;
    use bevy::mesh::{Mesh, VertexAttributeValues};
    use bevy::transform::components::Transform;
    use pretty_assertions::assert_eq;
    use sl_avatar::{BaseMesh, SkeletalDeformations, Skeleton, VisualParams};

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// The committed minimal skeleton fixture (four bones: `mPelvis` → `mTorso`
    /// → `mChest`, plus `mHipRight`), reused from `sl-avatar`'s test assets.
    const MINI_SKELETON: &str = include_str!("../../sl-avatar/tests/fixtures/mini_skeleton.xml");
    /// The committed minimal base-mesh fixture (four vertices, two faces, joints
    /// `mPelvis` / `mTorso`), reused from `sl-avatar`'s test assets.
    const MINI_BASEMESH: &[u8] = include_bytes!("../../sl-avatar/tests/fixtures/mini_basemesh.llm");

    /// A skeleton with a single 90° yaw (about Z) bone, to check the Euler
    /// conversion in isolation.
    const YAW_SKELETON: &str = r#"<?xml version="1.0"?>
<linden_skeleton num_bones="1" num_collision_volumes="0" version="2.0">
  <bone connected="false" end="0 0 0.1" group="Torso" name="mPelvis" pivot="0 0 0"
        pos="0 0 1" rot="0.0 0.0 90.0" scale="1 1 1" support="base"/>
</linden_skeleton>"#;

    #[test]
    fn skeleton_preserves_joints_roots_and_parents() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        let bevy = BevySkeleton::from_skeleton(&skeleton);
        assert_eq!(bevy.len(), skeleton.len());
        // `mPelvis` is the sole root; `mTorso` hangs off it, `mChest` off that.
        let pelvis = bevy.find("mPelvis").ok_or("mPelvis present")?;
        let torso = bevy.find("mTorso").ok_or("mTorso present")?;
        let chest = bevy.find("mChest").ok_or("mChest present")?;
        assert_eq!(bevy.parents().get(pelvis), Some(&None));
        assert_eq!(bevy.parents().get(torso), Some(&Some(pelvis)));
        assert_eq!(bevy.parents().get(chest), Some(&Some(torso)));
        // Alias resolution matches the underlying skeleton.
        assert_eq!(bevy.find("chest"), Some(chest));
        Ok(())
    }

    #[test]
    fn bind_globals_compose_down_the_hierarchy() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        let bevy = BevySkeleton::from_skeleton(&skeleton);
        let chest = bevy.find("mChest").ok_or("mChest present")?;
        // Every rest rotation in the fixture is zero and every scale is one, so a
        // joint's bind-pose translation is the sum of its ancestors' positions:
        // mPelvis (0, 0, 1.067) + mTorso (0, 0, 0.084) + mChest (-0.015, 0, 0.205).
        let inverse = bevy.inverse_bindpose(chest).ok_or("chest bindpose")?;
        let origin = inverse.inverse().transform_point3(Vec3::ZERO);
        assert!((origin.x - (-0.015)).abs() < 1e-4);
        assert!(origin.y.abs() < 1e-4);
        assert!((origin.z - (1.067 + 0.084 + 0.205)).abs() < 1e-4);
        Ok(())
    }

    #[test]
    fn euler_conversion_matches_a_known_yaw() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(YAW_SKELETON)?;
        let bevy = BevySkeleton::from_skeleton(&skeleton);
        let transform = bevy.local_transforms().first().ok_or("one joint")?;
        // A +90° turn about Z carries +X onto +Y.
        let rotated = transform.rotation * Vec3::X;
        assert!((rotated - Vec3::Y).length() < 1e-5);
        Ok(())
    }

    #[test]
    fn base_mesh_carries_skin_attributes() -> Result<(), TestError> {
        let base = BaseMesh::from_bytes(MINI_BASEMESH)?;
        let mesh = to_bevy_base_mesh(&base);
        let vertex_count = base.vertex_count();
        // Positions, joint indices, and joint weights are all one-per-vertex.
        assert!(matches!(
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            Some(VertexAttributeValues::Float32x3(values)) if values.len() == vertex_count
        ));
        let Some(VertexAttributeValues::Uint16x4(joint_indices)) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX)
        else {
            return Err("JOINT_INDEX is not a Uint16x4 attribute".into());
        };
        assert_eq!(joint_indices.len(), vertex_count);
        let Some(VertexAttributeValues::Float32x4(joint_weights)) =
            mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT)
        else {
            return Err("JOINT_WEIGHT is not a Float32x4 attribute".into());
        };
        assert_eq!(joint_weights.len(), vertex_count);
        // The two-joint blend is a partition of unity, and only the first two
        // influence slots are used.
        for weight in joint_weights {
            let [a, b, c, d] = *weight;
            assert!((a + b - 1.0).abs() < 1e-5);
            assert_eq!((c, d), (0.0, 0.0));
        }
        // Two triangles → six indices.
        assert_eq!(
            mesh.indices().map(bevy::mesh::Indices::len),
            Some(base.faces().len() * 3)
        );
        Ok(())
    }

    /// A visual-param table with one transmitted `param_skeleton` that scales
    /// `mTorso` up along Z, to exercise the deformed-transform recurrence.
    const TORSO_SCALE_LAD: &str = r#"<?xml version="1.0"?>
<linden_avatar version="2.0">
  <skeleton file_name="avatar_skeleton.xml">
    <param id="33" group="0" name="Height" value_min="0" value_max="1" value_default="0">
      <param_skeleton>
        <bone name="mTorso" scale="0 0 0.1"/>
      </param_skeleton>
    </param>
  </skeleton>
</linden_avatar>"#;

    /// Reconstruct each joint's world matrix from the relative-to-parent local
    /// transforms (the Bevy hierarchy composition the viewer relies on).
    fn compose_globals(skeleton: &BevySkeleton, locals: &[Transform]) -> Vec<bevy::math::Mat4> {
        let mut globals: Vec<bevy::math::Mat4> = Vec::with_capacity(locals.len());
        for (index, local) in locals.iter().enumerate() {
            let parent_global = skeleton
                .parents()
                .get(index)
                .copied()
                .flatten()
                .and_then(|parent| globals.get(parent).copied())
                .unwrap_or(bevy::math::Mat4::IDENTITY);
            globals.push(parent_global.mul_mat4(&local.to_matrix()));
        }
        globals
    }

    #[test]
    fn deformed_transforms_match_rest_without_deformation() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        let bevy = BevySkeleton::from_skeleton(&skeleton);
        let deformed = bevy.deformed_local_transforms(&SkeletalDeformations::default());
        assert_eq!(deformed.len(), bevy.len());
        for (rest, moved) in bevy.local_transforms().iter().zip(deformed.iter()) {
            assert!(rest.translation.abs_diff_eq(moved.translation, 1e-4));
            assert!(rest.scale.abs_diff_eq(moved.scale, 1e-4));
            assert!(rest.rotation.abs_diff_eq(moved.rotation, 1e-4));
        }
        Ok(())
    }

    #[test]
    fn bone_scale_stretches_child_position_but_not_child_scale() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        let bevy = BevySkeleton::from_skeleton(&skeleton);
        let params = VisualParams::from_xml(TORSO_SCALE_LAD)?;
        // `mTorso` scaled up along Z by 0.1 at full Height weight.
        let deform = SkeletalDeformations::from_appearance(&params, &[255]);

        let rest = compose_globals(&bevy, bevy.local_transforms());
        let moved = compose_globals(&bevy, &bevy.deformed_local_transforms(&deform));

        let torso = bevy.find("mTorso").ok_or("mTorso")?;
        let chest = bevy.find("mChest").ok_or("mChest")?;

        let (torso_scale, _, _) = moved
            .get(torso)
            .ok_or("torso global")?
            .to_scale_rotation_translation();
        // mTorso's own world scale takes the +0.1 (rest 1 -> 1.1).
        assert!(
            (torso_scale.z - 1.1).abs() < 1e-3,
            "torso z scale {torso_scale}"
        );

        let (chest_scale, _, chest_pos) = moved
            .get(chest)
            .ok_or("chest global")?
            .to_scale_rotation_translation();
        // The child bone's world scale is NOT inherited (stays ~1, not 1.1).
        assert!(
            (chest_scale.z - 1.0).abs() < 1e-3,
            "chest z scale {chest_scale}"
        );
        // But its world position IS stretched up by the parent's local scale:
        // rest chest Z + 0.1 * (chest local Z offset 0.205) = +0.0205.
        let (_, _, rest_chest_pos) = rest
            .get(chest)
            .ok_or("rest chest global")?
            .to_scale_rotation_translation();
        assert!(
            (chest_pos.z - rest_chest_pos.z - 0.0205).abs() < 1e-3,
            "chest lifted by {}",
            chest_pos.z - rest_chest_pos.z
        );
        Ok(())
    }

    #[test]
    fn base_mesh_skin_resolves_against_the_skeleton() -> Result<(), TestError> {
        let skeleton = Skeleton::from_xml(MINI_SKELETON)?;
        let bevy = BevySkeleton::from_skeleton(&skeleton);
        let base = BaseMesh::from_bytes(MINI_BASEMESH)?;
        let skin = bevy.base_mesh_skin(&base).ok_or("skin resolves")?;
        // The part's two joints (`mPelvis`, `mTorso`) map to their skeleton
        // indices, with one inverse bindpose each.
        assert_eq!(skin.joints.len(), base.joint_names().len());
        assert_eq!(skin.inverse_bindposes.len(), skin.joints.len());
        assert_eq!(skin.joints.first().copied(), bevy.find("mPelvis"));
        assert_eq!(skin.joints.get(1).copied(), bevy.find("mTorso"));
        Ok(())
    }

    #[test]
    fn base_mesh_skin_is_none_when_a_joint_is_missing() -> Result<(), TestError> {
        // A skeleton with only `mPelvis`; the base mesh also needs `mTorso`.
        let only_pelvis = r#"<?xml version="1.0"?>
<linden_skeleton num_bones="1" num_collision_volumes="0" version="2.0">
  <bone connected="false" end="0 0 0.1" group="Torso" name="mPelvis" pivot="0 0 0"
        pos="0 0 1" rot="0 0 0" scale="1 1 1" support="base"/>
</linden_skeleton>"#;
        let skeleton = Skeleton::from_xml(only_pelvis)?;
        let bevy = BevySkeleton::from_skeleton(&skeleton);
        let base = BaseMesh::from_bytes(MINI_BASEMESH)?;
        assert!(bevy.base_mesh_skin(&base).is_none());
        Ok(())
    }
}
