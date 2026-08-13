//! The GPU-avatar pose pipeline's **data model** (`roadmap/context/gpu-avatars.md`
//! §1): the `#[derive(ShaderType)]` structs mirrored 1:1 by `pose.wgsl`, the
//! CPU-side **rest composition** (the head of
//! [`BevySkeleton::deformed_world_matrices`] folded into per-joint
//! [`GpuRestJoint`] rows, §1.3(c)), the dense per-joint conversion of a blended
//! [`AnimationPose`], and [`reference_fk`] — a Rust mirror of the WGSL pass-C
//! recurrence, golden-tested against `deformed_world_matrices` itself so the
//! shader has a bit-exact CPU reference to be compared to.

use bevy::math::{Mat4, Quat, Vec3, Vec4};
use bevy::render::render_resource::ShaderType;
use sl_client_bevy::{
    AnimationPose, BevySkeleton, JointOverrides, SkeletalDeformations, VolumeDeformations,
};

/// The `parent` sentinel for a joint with no parent (the skeleton root). Also
/// the CPU-side fallback for a parent index that does not fit `u32` (cannot
/// happen for a real skeleton, but the conversion must total).
pub(crate) const PARENT_NONE: u32 = u32::MAX;

/// [`GpuRestJoint::flags`] bit: the joint is a collision volume, so an animated
/// position key **adds** to its rest position (body physics deltas).
pub(crate) const REST_FLAG_VOLUME: u32 = 1;

/// [`GpuRestJoint::flags`] bit: the joint is `mPelvis`, whose position keys are
/// likewise **additive** (the historical pre-Bento pelvis offset).
pub(crate) const REST_FLAG_PELVIS: u32 = 2;

/// [`GpuRestJoint::flags`] bit: a worn rig overrides this joint's position, so
/// an absolute animated position key is ignored (override wins, exactly as in
/// [`BevySkeleton::deformed_world_matrices`]).
pub(crate) const REST_FLAG_OVERRIDE: u32 = 4;

/// [`GpuLocalPose::flags`] bit: the pose animates this joint's local rotation.
pub(crate) const POSE_FLAG_ROT: u32 = 1;

/// [`GpuLocalPose::flags`] bit: the pose animates this joint's local position.
pub(crate) const POSE_FLAG_POS: u32 = 2;

/// The most joints one avatar skeleton may carry on the GPU — the WGSL pass-C
/// private world-rotation/-position arrays are sized to exactly this, so a
/// bigger skeleton must be rejected at staging time (the real skeleton is
/// ~200: 133 bones + ~26 collision volumes + the synthetic root + ~38
/// attachment points).
pub(crate) const MAX_GPU_JOINTS: u32 = 256;

/// One canonical joint of one avatar's **rest skeleton** as the GPU FK consumes
/// it (§1.3(c)): the shape deformation, volume morphs and rig overrides are
/// pre-folded on the CPU (see [`compose_rest_joints`]) so the shader never
/// needs to know about sliders or rigs individually. std430 stride 48 B;
/// mirrored by `GpuRestJoint` in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuRestJoint {
    /// The joint's composed local rest position: the rig override when one
    /// exists, else the `avatar_skeleton.xml` rest offset by the appearance
    /// deformation (+ volume morph displacement for a collision volume).
    pub(crate) rest_pos: Vec3,
    /// The canonical index of the parent joint, or [`PARENT_NONE`].
    pub(crate) parent: u32,
    /// The joint's local rest rotation as an `xyzw` quaternion (bones rest at
    /// identity; collision volumes and attachment points carry authored ones).
    pub(crate) rest_rot: Vec4,
    /// The joint's composed **local** scale (deformed, or pinned to the
    /// default under an override + `lock_scale`). Scales only the joint's own
    /// bound geometry plus its immediate children's offsets — never inherited.
    pub(crate) local_scale: Vec3,
    /// The [`REST_FLAG_VOLUME`] / [`REST_FLAG_PELVIS`] / [`REST_FLAG_OVERRIDE`]
    /// bits.
    pub(crate) flags: u32,
}

/// One joint of one avatar's CPU-blended **local pose** (§1.3(f) `LocalPose`):
/// in Phase 1 the CPU still samples and blends the playing motions (plus the
/// procedural adjusters), and this row carries the resulting local channels to
/// pass C. std430 stride 32 B; mirrored by `GpuLocalPose` in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuLocalPose {
    /// The animated local rotation (`xyzw`), meaningful only when
    /// [`POSE_FLAG_ROT`] is set.
    pub(crate) rot: Vec4,
    /// The animated local position (SL Z-up metres), meaningful only when
    /// [`POSE_FLAG_POS`] is set.
    pub(crate) pos: Vec3,
    /// The [`POSE_FLAG_ROT`] / [`POSE_FLAG_POS`] bits; `0` = the joint keeps
    /// its deformed rest channels.
    pub(crate) flags: u32,
}

/// One posed avatar's per-frame **frame data** (§1.3(e)): the Bevy-world root
/// affine (SL→Bevy axis change + world placement + the ghost display offset)
/// and which avatar slot it belongs to — pass C reads these compactly, one per
/// dispatched thread. std430 stride 80 B; mirrored in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuAvatarFrame {
    /// The Bevy-world matrix every joint world matrix is composed under —
    /// `ghost_offset * avatar_root_global`.
    pub(crate) root: Mat4,
    /// The avatar's dense slot: the row block `slot * joint_count ..` of the
    /// rest / local-pose / joint-world buffers.
    pub(crate) slot: u32,
    /// std430 padding up to the 16-byte struct alignment.
    pub(crate) pad0: u32,
    /// std430 padding.
    pub(crate) pad1: u32,
    /// std430 padding.
    pub(crate) pad2: u32,
}

/// One skinned **ghost instance** for pass D (§1.3(f) `SkinInstance`, with the
/// per-mesh-skin fields of §1.2(b) resolved in — the joint map and inverse
/// bindposes themselves stay deduplicated in the shared pool buffers, only
/// their offsets are carried here, which keeps the pipeline at 8 storage
/// bindings). std430 stride 32 B; mirrored in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuSkinInstance {
    /// The wearer's avatar slot (into `JointWorld`).
    pub(crate) avatar_slot: u32,
    /// The instance's palette start inside Bevy's `SkinUniforms` buffer, in
    /// `mat4` elements — `SkinUniforms::skin_index(ghost)`, re-resolved every
    /// frame because the allocator can move and the buffers can reallocate.
    pub(crate) palette_offset: u32,
    /// How many palette entries (skin joints) the instance owns.
    pub(crate) joint_count: u32,
    /// Where the instance's `joint_count` canonical-index entries start in the
    /// shared joint-map pool.
    pub(crate) joint_map_offset: u32,
    /// Where the instance's `joint_count` inverse bindposes start in the
    /// shared inverse-bindpose pool.
    pub(crate) ibp_offset: u32,
    /// std430 padding up to the 16-byte struct alignment.
    pub(crate) pad0: u32,
    /// std430 padding.
    pub(crate) pad1: u32,
    /// std430 padding.
    pub(crate) pad2: u32,
}

/// The per-frame dispatch parameters (§1.3(e) `GpuFrameParams`), uploaded as a
/// uniform. Mirrored by `Params` in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuComputeParams {
    /// How many avatars pass C poses this frame (= `frames` rows).
    pub(crate) avatar_count: u32,
    /// The canonical skeleton's joint count `N_J` (row stride of the
    /// rest / local-pose / joint-world buffers).
    pub(crate) joint_count: u32,
    /// How many skin instances pass D writes this frame.
    pub(crate) instance_count: u32,
    /// The largest `joint_count` over the instances (pass D's x-dispatch).
    pub(crate) max_skin_joints: u32,
    /// The instance index the debug readback pass copies, or `u32::MAX`.
    pub(crate) readback_instance: u32,
    /// How many palette entries the readback pass copies.
    pub(crate) readback_joint_count: u32,
    /// std140 padding up to the 16-byte struct alignment.
    pub(crate) pad0: u32,
    /// std140 padding.
    pub(crate) pad1: u32,
}

/// Compose one avatar's [`GpuRestJoint`] rows from the same inputs the CPU pose
/// path feeds [`BevySkeleton::deformed_world_matrices`] — an exact port of that
/// function's **per-joint head** (everything before the animated-pose folds):
/// the appearance deformation and volume-morph displacement shift the rest
/// position and scale, a rig position override replaces the position (and pins
/// the scale when the rig locks it), and the additive/absolute/override flags
/// are recorded so the GPU recurrence can reproduce the position-key semantics
/// exactly.
///
/// Re-run only when the avatar's `pose_inputs_generation` bumps (shape edit,
/// appearance message, override add/remove) — the composed rows are otherwise
/// pose-independent.
pub(crate) fn compose_rest_joints(
    skeleton: &BevySkeleton,
    deform: &SkeletalDeformations,
    volumes: &VolumeDeformations,
    overrides: &JointOverrides,
) -> Vec<GpuRestJoint> {
    let locals = skeleton.local_transforms();
    let parents = skeleton.parents();
    let mut out = Vec::with_capacity(locals.len());
    for (index, local) in locals.iter().enumerate() {
        let name = skeleton.joint_name(index).unwrap_or("");
        // Bones and collision volumes live in disjoint name spaces, so summing
        // both lookups equals choosing between them — the same fold
        // `deformed_world_matrices` performs.
        let volume = volumes.get(name).copied().unwrap_or_default();
        let bone_scale = deform.scale(name);
        let bone_offset = deform.offset(name);
        let deform_scale = [
            bone_scale[0] + volume.scale[0],
            bone_scale[1] + volume.scale[1],
            bone_scale[2] + volume.scale[2],
        ];
        let deform_offset = [
            bone_offset[0] + volume.position[0],
            bone_offset[1] + volume.position[1],
            bone_offset[2] + volume.position[2],
        ];
        let override_pos = overrides.position(index);
        // An overridden joint with a scale lock keeps its default scale (the
        // rig fits at that scale); every other joint takes the
        // appearance-driven scale. Component-wise, matching the reference.
        let scale = if override_pos.is_some() && overrides.lock_scale() {
            local.scale
        } else {
            Vec3::new(
                local.scale.x + deform_scale[0],
                local.scale.y + deform_scale[1],
                local.scale.z + deform_scale[2],
            )
        };
        // The joint's base local position: a rig override, else the appearance
        // offset shifts the default rest position.
        let base_position = match override_pos {
            Some(pos) => pos,
            None => Vec3::new(
                local.translation.x + deform_offset[0],
                local.translation.y + deform_offset[1],
                local.translation.z + deform_offset[2],
            ),
        };
        let mut flags = 0_u32;
        if skeleton.is_collision_volume(index) {
            flags |= REST_FLAG_VOLUME;
        }
        if name == "mPelvis" {
            flags |= REST_FLAG_PELVIS;
        }
        if override_pos.is_some() {
            flags |= REST_FLAG_OVERRIDE;
        }
        let parent = parents
            .get(index)
            .copied()
            .flatten()
            .and_then(|parent| u32::try_from(parent).ok())
            .unwrap_or(PARENT_NONE);
        out.push(GpuRestJoint {
            rest_pos: base_position,
            parent,
            rest_rot: Vec4::new(
                local.rotation.x,
                local.rotation.y,
                local.rotation.z,
                local.rotation.w,
            ),
            local_scale: scale,
            flags,
        });
    }
    out
}

/// Densify one avatar's blended [`AnimationPose`] into `joint_count`
/// [`GpuLocalPose`] rows (the §1.3(f) `LocalPose` upload): each animated
/// joint's channels with their flags, every other joint an all-zero "keep the
/// rest" row.
pub(crate) fn pose_rows(pose: &AnimationPose, joint_count: usize) -> Vec<GpuLocalPose> {
    (0..joint_count)
        .map(|index| {
            let mut row = GpuLocalPose::default();
            if let Some(rotation) = pose.rotation(index) {
                row.rot = Vec4::new(rotation.x, rotation.y, rotation.z, rotation.w);
                row.flags |= POSE_FLAG_ROT;
            }
            if let Some(position) = pose.position(index) {
                row.pos = position;
                row.flags |= POSE_FLAG_POS;
            }
            row
        })
        .collect()
}

/// The Rust mirror of the WGSL pass-C recurrence: every joint's Bevy-world
/// matrix from the composed rest rows, the dense local pose, and the root
/// affine — **operation-for-operation identical** to
/// [`BevySkeleton::deformed_world_matrices`] followed by the pose driver's
/// `root_matrix * world[j]` compose, so the golden tests can assert bit-equal
/// results against the CPU pose path.
///
/// The one structural subtlety ported deliberately: the reference resolves a
/// **forward parent reference** (the synthetic root is appended *after* its
/// children) through `Vec::get` on the incrementally built world vectors,
/// falling back to identity rotation / zero position / **unit parent scale**.
/// The `computed` guard below (and the matching `parent >= j` test in the
/// WGSL) reproduces exactly that.
pub(crate) fn reference_fk(rest: &[GpuRestJoint], pose: &[GpuLocalPose], root: Mat4) -> Vec<Mat4> {
    let mut world_rot: Vec<Quat> = Vec::with_capacity(rest.len());
    let mut world_pos: Vec<Vec3> = Vec::with_capacity(rest.len());
    let mut out = Vec::with_capacity(rest.len());
    for (index, joint) in rest.iter().enumerate() {
        let row = pose.get(index).copied().unwrap_or_default();
        let local_rotation = if row.flags & POSE_FLAG_ROT != 0 {
            Quat::from_xyzw(row.rot.x, row.rot.y, row.rot.z, row.rot.w)
        } else {
            Quat::from_xyzw(
                joint.rest_rot.x,
                joint.rest_rot.y,
                joint.rest_rot.z,
                joint.rest_rot.w,
            )
        };
        let base_position = joint.rest_pos;
        // The position-key semantics of `deformed_world_matrices`, arm for arm:
        // additive for `mPelvis` and collision volumes (even under an
        // override), absolute for any other joint unless a rig override wins.
        let position = if row.flags & POSE_FLAG_POS != 0 {
            if joint.flags & (REST_FLAG_VOLUME | REST_FLAG_PELVIS) != 0 {
                Vec3::new(
                    base_position.x + row.pos.x,
                    base_position.y + row.pos.y,
                    base_position.z + row.pos.z,
                )
            } else if joint.flags & REST_FLAG_OVERRIDE == 0 {
                row.pos
            } else {
                base_position
            }
        } else {
            base_position
        };
        let parent = usize::try_from(joint.parent)
            .ok()
            .filter(|_| joint.parent != PARENT_NONE);
        let (rotation, translation) = match parent {
            Some(parent_index) => {
                // `computed` is false for a forward reference (only the
                // synthetic identity root in practice) — the reference's
                // `Vec::get` fallbacks, reproduced.
                let computed = parent_index < index;
                let parent_rot = if computed {
                    world_rot
                        .get(parent_index)
                        .copied()
                        .unwrap_or(Quat::IDENTITY)
                } else {
                    Quat::IDENTITY
                };
                let parent_pos = if computed {
                    world_pos.get(parent_index).copied().unwrap_or(Vec3::ZERO)
                } else {
                    Vec3::ZERO
                };
                let parent_scale = if computed {
                    rest.get(parent_index)
                        .map_or(Vec3::ONE, |parent_joint| parent_joint.local_scale)
                } else {
                    Vec3::ONE
                };
                // Child offset scaled by the parent's *local* scale, rotated
                // into and translated by the parent's world frame.
                let scaled = Vec3::new(
                    parent_scale.x * position.x,
                    parent_scale.y * position.y,
                    parent_scale.z * position.z,
                );
                let rotated = parent_rot.mul_vec3(scaled);
                (
                    parent_rot.mul_quat(local_rotation),
                    Vec3::new(
                        parent_pos.x + rotated.x,
                        parent_pos.y + rotated.y,
                        parent_pos.z + rotated.z,
                    ),
                )
            }
            None => (local_rotation, position),
        };
        world_rot.push(rotation);
        world_pos.push(translation);
        // Own scale enters only the final matrix (never inherited), then the
        // root affine — exactly `write_joint_globals`' compose.
        out.push(root.mul_mat4(&Mat4::from_scale_rotation_translation(
            joint.local_scale,
            rotation,
            translation,
        )));
    }
    out
}
