//! The GPU-avatar pose pipeline's **data model** (`roadmap/context/gpu-avatars.md`
//! §1): the `#[derive(ShaderType)]` structs mirrored 1:1 by `pose.wgsl`, the
//! CPU-side **rest composition** (the head of
//! [`BevySkeleton::deformed_world_matrices`] folded into per-joint
//! [`GpuRestJoint`] rows, §1.3(c)), the dense per-joint conversion of a blended
//! [`AnimationPose`], the **clip arena** (§1.2(a): every decoded `.anim` as GPU
//! keyframe data, uploaded once, deduplicated by asset), and the Rust mirrors
//! of the WGSL passes — [`reference_fk`] for pass C and the
//! `mirror_*` sample/blend family for passes A+B — golden-tested against
//! `deformed_world_matrices` / [`sl_client_bevy::sample_motion`] /
//! [`sl_anim::blend_joint`] themselves so every shader has a bit-exact CPU
//! reference to be compared to.

use std::collections::HashMap;

use bevy::math::{Mat4, Quat, Vec3, Vec4};
use bevy::render::render_resource::ShaderType;
use sl_anim::Motion;
use sl_client_bevy::{
    AssetKey, BevySkeleton, JointOverrides, SkeletalDeformations, VolumeDeformations,
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

/// How many motions one avatar's GPU playback row block carries
/// (§1.3(d) `MAX_ACTIVE`): the reference blends 4 per joint; 16 active motions
/// covers AO + gesture + typing + dance stacks. A larger playing set keeps its
/// 16 most recently activated motions.
pub(crate) const MAX_ACTIVE_CLIPS: usize = 16;

/// The [`GpuPlayState::clip_id`] sentinel for an empty playback slot.
pub(crate) const CLIP_NONE: u32 = u32::MAX;

/// The track-of-joint sentinel: the clip has no track for this joint.
pub(crate) const TRACK_NONE: u32 = u32::MAX;

/// [`GpuClipHeader::flags`] bit: the motion loops.
pub(crate) const CLIP_FLAG_LOOPS: u32 = 1;

/// The [`GpuPlayState::stopped_at`] sentinel for a motion the simulator has
/// not (yet) dropped: any negative value means "still signalled" — a real
/// stop time is `now - start` at the stop, which is never negative.
pub(crate) const PLAY_STOPPED_NONE: f32 = -1.0;

/// [`GpuComputeParams::flags`] bit: the debug T-pose freeze is on, so pass B
/// must not fold the procedural idle adjusters in (the scheduler stages no
/// playback then, so the blend already contributes nothing else).
pub(crate) const PARAMS_FLAG_TPOSE: u32 = 1;

/// The [`GpuComputeParams::chest_joint`] / [`GpuComputeParams::torso_joint`]
/// sentinel for a skeleton without that joint (pass B then skips its idle
/// adjuster, exactly as the CPU `apply_idle_adjustments` skips a missing
/// joint).
pub(crate) const JOINT_NONE: u32 = u32::MAX;

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
/// affine (SL→Bevy axis change + world placement) and which avatar slot it
/// belongs to — pass C reads these compactly, one per dispatched thread.
/// std430 stride 80 B; mirrored in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuAvatarFrame {
    /// The Bevy-world matrix every joint world matrix is composed under — the
    /// avatar root global.
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
    /// How many avatars passes B and C cover this frame (= `frames` rows).
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
    /// How many sample jobs pass A runs this frame (§2.1).
    pub(crate) sample_job_count: u32,
    /// How many sparse CPU corrections pass B folds in (§5.3).
    pub(crate) correction_count: u32,
    /// The wall clock (`Time::elapsed_secs`) pass B's ease weights run on —
    /// the same `now` the CPU reconcile stamped `start` / `stopped_at` with.
    pub(crate) now: f32,
    /// The 15 Hz-quantised procedural idle clock (the CPU pose driver's
    /// `POSE_IDLE_HZ` grid), so the GPU breathe / sway is bit-comparable to
    /// the CPU's between ticks.
    pub(crate) idle_now: f32,
    /// The canonical index of `mChest` (the breathe adjuster's joint), or
    /// [`JOINT_NONE`].
    pub(crate) chest_joint: u32,
    /// The canonical index of `mTorso` (the body-noise adjuster's joint), or
    /// [`JOINT_NONE`].
    pub(crate) torso_joint: u32,
    /// The [`PARAMS_FLAG_TPOSE`] bit.
    pub(crate) flags: u32,
    /// std140 padding up to the 16-byte struct alignment.
    pub(crate) pad0: u32,
    /// std140 padding.
    pub(crate) pad1: u32,
    /// std140 padding.
    pub(crate) pad2: u32,
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
/// rest" row. Test-only since Phase 4 removed the CPU-blended live upload — the
/// headless FK tests hand-stage a `LocalPose` block with it.
#[cfg(test)]
pub(crate) fn pose_rows(
    pose: &sl_client_bevy::AnimationPose,
    joint_count: usize,
) -> Vec<GpuLocalPose> {
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
        // root affine — the same compose the CPU skinning path used.
        out.push(root.mul_mat4(&Mat4::from_scale_rotation_translation(
            joint.local_scale,
            rotation,
            translation,
        )));
    }
    out
}

// ---------------------------------------------------------------------------
// Phase 2 (§1.2(a), §1.3(d), §2.2 passes A+B): clip data, playback, jobs,
// corrections — and the Rust mirrors the golden tests + readback verdict use.
// ---------------------------------------------------------------------------

/// One uploaded clip's header (§1.2(a)): the playback-timing scalars
/// [`Motion`] carries plus where its tracks and joint→track lookup live in the
/// shared pools. std430 stride 48 B; mirrored by `ClipHeader` in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuClipHeader {
    /// The motion's total duration, seconds.
    pub(crate) duration: f32,
    /// The loop restart point, seconds within the motion.
    pub(crate) loop_in: f32,
    /// The loop wrap point, seconds within the motion.
    pub(crate) loop_out: f32,
    /// The ease-in duration, seconds of wall time.
    pub(crate) ease_in: f32,
    /// The ease-out duration, seconds of wall time.
    pub(crate) ease_out: f32,
    /// The [`CLIP_FLAG_LOOPS`] bit.
    pub(crate) flags: u32,
    /// How many joint tracks the clip carries.
    pub(crate) track_count: u32,
    /// Where the clip's tracks start in the shared track pool.
    pub(crate) track_offset: u32,
    /// Where the clip's `N_J` joint→track entries start in the shared
    /// track-of-joint pool ([`TRACK_NONE`] = no track for that joint).
    pub(crate) track_of_joint_offset: u32,
    /// std430 padding up to the 16-byte struct alignment.
    pub(crate) pad0: u32,
    /// std430 padding.
    pub(crate) pad1: u32,
    /// std430 padding.
    pub(crate) pad2: u32,
}

/// One joint track of one uploaded clip (§1.2(a)): its canonical joint, its
/// **effective** priority (`USE_MOTION` already resolved to the motion's base
/// priority at upload), and where its keyframes live in the shared key pools.
/// std430 stride 32 B; mirrored by `JointTrack` in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuJointTrack {
    /// The canonical joint index the track animates.
    pub(crate) joint: u32,
    /// The track's effective animation priority
    /// ([`sl_anim::JointMotion::effective_priority`], resolved at upload).
    pub(crate) priority: i32,
    /// Where the rotation keys start in the shared time/value pools.
    pub(crate) rot_offset: u32,
    /// How many rotation keys the track carries (0 = no rotation channel).
    pub(crate) rot_count: u32,
    /// Where the position keys start in the shared time/value pools.
    pub(crate) pos_offset: u32,
    /// How many position keys the track carries (0 = no position channel).
    pub(crate) pos_count: u32,
    /// std430 padding up to the 8-field stride.
    pub(crate) pad0: u32,
    /// std430 padding.
    pub(crate) pad1: u32,
}

/// One deduplicated sample job (§2.1): sample `clip_id`'s tracks at `phase`
/// (seconds of motion-elapsed time — the loop wrap happens GPU-side) into the
/// pose cache at `cache_base`. The CPU scheduler resolves each play state's
/// walk-speed-skewed clock into this phase, so the play state itself needs no
/// `anim_offset` on the GPU. std430 stride 16 B; mirrored in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuSampleJob {
    /// The clip to sample (an index into the clip-header pool).
    pub(crate) clip_id: u32,
    /// Where this job's `track_count` sampled tracks land in the pose cache.
    pub(crate) cache_base: u32,
    /// The motion-elapsed sampling time, seconds (bucketed for far avatars).
    pub(crate) phase: f32,
    /// std430 padding up to the 16-byte struct alignment.
    pub(crate) pad0: u32,
}

/// One playback slot of one avatar's `MAX_ACTIVE` block (§1.3(d)): which clip
/// plays, where its sampled tracks sit in this frame's pose cache, and the
/// wall-time ease inputs. Uploaded only when the rows' content changes (the
/// per-frame phase lives in the jobs instead). std430 stride 32 B; mirrored by
/// `PlayState` in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, PartialEq)]
pub(crate) struct GpuPlayState {
    /// The playing clip, or [`CLIP_NONE`] for an empty slot.
    pub(crate) clip_id: u32,
    /// This frame's pose-cache base for the clip's sampled tracks.
    pub(crate) cache_base: u32,
    /// `Time::elapsed_secs` at activation (the ease-in origin).
    pub(crate) start: f32,
    /// Elapsed-since-start at which the motion was dropped (the ease-out
    /// origin), or [`PLAY_STOPPED_NONE`] while still signalled.
    pub(crate) stopped_at: f32,
    /// The activation recency stamp, truncated to `u32` (only the relative
    /// order within one avatar's slots is ever compared).
    pub(crate) order: u32,
    /// std430 padding up to the 32-byte stride.
    pub(crate) pad0: u32,
    /// std430 padding.
    pub(crate) pad1: u32,
    /// std430 padding.
    pub(crate) pad2: u32,
}

impl Default for GpuPlayState {
    fn default() -> Self {
        Self {
            clip_id: CLIP_NONE,
            cache_base: 0,
            start: 0.0,
            stopped_at: PLAY_STOPPED_NONE,
            order: 0,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        }
    }
}

/// One sparse CPU adjuster correction (§1.3(e), §5.3): the final CPU-computed
/// local channels of one (avatar, joint) the look-at / reach / IK / physics
/// folds changed this frame, replacing pass B's blended channels outright.
/// Sorted by `(avatar, joint)` so pass B binary-searches its own entry.
/// std430 stride 48 B; mirrored by `Correction` in `pose.wgsl`.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct GpuCorrection {
    /// The avatar's **frame index** (into this frame's `frames` rows — not the
    /// slot; pass B is dispatched over frames).
    pub(crate) avatar: u32,
    /// The corrected canonical joint.
    pub(crate) joint: u32,
    /// Which channels the correction replaces ([`POSE_FLAG_ROT`] /
    /// [`POSE_FLAG_POS`]).
    pub(crate) flags: u32,
    /// std430 padding up to the vec4 alignment.
    pub(crate) pad0: u32,
    /// The replacement local rotation (`xyzw`), when [`POSE_FLAG_ROT`] is set.
    pub(crate) rot: Vec4,
    /// The replacement local position (SL Z-up metres), when
    /// [`POSE_FLAG_POS`] is set.
    pub(crate) pos: Vec3,
    /// std430 padding up to the 48-byte stride.
    pub(crate) pad1: u32,
}

/// The §1.2(a) **clip arena**: every decoded `.anim` uploaded once as GPU
/// keyframe data — headers, tracks, the per-clip joint→track lookup, and the
/// shared keyframe time/value pools (rotation values are `xyzw` quaternions,
/// position values `xyz` with `w` unused). Grow-and-copy (append-only `Arc`
/// vecs the staging snapshot clones cheaply); deduplicated by asset id.
#[derive(Default)]
pub(crate) struct ClipArena {
    /// One header per uploaded clip; a `clip_id` indexes this.
    headers: std::sync::Arc<Vec<GpuClipHeader>>,
    /// The shared joint-track pool.
    tracks: std::sync::Arc<Vec<GpuJointTrack>>,
    /// The shared joint→track lookup pool (`N_J` entries per clip).
    track_of_joint: std::sync::Arc<Vec<u32>>,
    /// The shared keyframe time pool (rotation and position keys alike).
    key_times: std::sync::Arc<Vec<f32>>,
    /// The shared keyframe value pool, parallel to [`Self::key_times`].
    key_values: std::sync::Arc<Vec<Vec4>>,
    /// Asset id → uploaded clip id (the dedup).
    ids: HashMap<AssetKey, u32>,
    /// Bumped on every upload — the render side re-uploads only on a bump.
    generation: u64,
}

impl ClipArena {
    /// The uploaded clip id for `id`, uploading the decoded `motion` on first
    /// sight (§1.2(a): keyframes kept exactly as decoded; joint names resolved
    /// to canonical indices via `joint_index`; a track whose joint the
    /// skeleton lacks is dropped, exactly as the CPU `resolve_pose` skips it).
    /// `None` only when a pool offset would overflow `u32` (unreachable for
    /// real content).
    pub(crate) fn ensure_clip(
        &mut self,
        id: AssetKey,
        motion: &Motion,
        joint_count: u32,
        joint_index: impl Fn(&str) -> Option<usize>,
    ) -> Option<u32> {
        if let Some(&clip) = self.ids.get(&id) {
            return Some(clip);
        }
        let clip_id = u32::try_from(self.headers.len()).ok()?;
        let track_offset = u32::try_from(self.tracks.len()).ok()?;
        let track_of_joint_offset = u32::try_from(self.track_of_joint.len()).ok()?;
        let joint_count_usize = usize::try_from(joint_count).ok()?;

        let lookup_base = self.track_of_joint.len();
        arena_pool_fill(&mut self.track_of_joint, TRACK_NONE, joint_count_usize);

        let mut tracks: Vec<GpuJointTrack> = Vec::new();
        for joint_motion in &motion.joints {
            let Some(joint) = joint_index(&joint_motion.name).and_then(|i| u32::try_from(i).ok())
            else {
                // Not a skeleton joint: the CPU sampler skips it too.
                continue;
            };
            let rot_offset = u32::try_from(self.key_times.len()).ok()?;
            {
                let times = std::sync::Arc::make_mut(&mut self.key_times);
                let values = std::sync::Arc::make_mut(&mut self.key_values);
                for key in &joint_motion.rotation_keys {
                    times.push(key.time);
                    let [x, y, z, w] = key.rotation;
                    values.push(Vec4::new(x, y, z, w));
                }
            }
            let rot_count = u32::try_from(joint_motion.rotation_keys.len()).ok()?;
            let pos_offset = u32::try_from(self.key_times.len()).ok()?;
            {
                let times = std::sync::Arc::make_mut(&mut self.key_times);
                let values = std::sync::Arc::make_mut(&mut self.key_values);
                for key in &joint_motion.position_keys {
                    times.push(key.time);
                    let [x, y, z] = key.position;
                    values.push(Vec4::new(x, y, z, 0.0));
                }
            }
            let pos_count = u32::try_from(joint_motion.position_keys.len()).ok()?;
            let track_index = u32::try_from(tracks.len()).ok()?;
            // Last track wins a duplicate joint, like the reference viewer's
            // joint-keyed track map.
            let joint_usize = usize::try_from(joint).ok()?;
            if let Some(slot) = std::sync::Arc::make_mut(&mut self.track_of_joint)
                .get_mut(lookup_base.checked_add(joint_usize)?)
            {
                *slot = track_index;
            }
            tracks.push(GpuJointTrack {
                joint,
                priority: joint_motion.effective_priority(motion.base_priority),
                rot_offset,
                rot_count,
                pos_offset,
                pos_count,
                pad0: 0,
                pad1: 0,
            });
        }
        let track_count = u32::try_from(tracks.len()).ok()?;
        std::sync::Arc::make_mut(&mut self.tracks).extend_from_slice(&tracks);
        std::sync::Arc::make_mut(&mut self.headers).push(GpuClipHeader {
            duration: motion.duration,
            loop_in: motion.loop_in_point,
            loop_out: motion.loop_out_point,
            ease_in: motion.ease_in_duration,
            ease_out: motion.ease_out_duration,
            flags: if motion.loops { CLIP_FLAG_LOOPS } else { 0 },
            track_count,
            track_offset,
            track_of_joint_offset,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        });
        self.generation = self.generation.wrapping_add(1);
        let _prev = self.ids.insert(id, clip_id);
        Some(clip_id)
    }

    /// The track count of an uploaded clip (0 for an unknown id).
    pub(crate) fn track_count(&self, clip_id: u32) -> u32 {
        usize::try_from(clip_id)
            .ok()
            .and_then(|index| self.headers.get(index))
            .map_or(0, |header| header.track_count)
    }

    /// A borrowed view of the arena's pools for the CPU mirrors.
    pub(crate) fn slices(&self) -> ClipSlices<'_> {
        ClipSlices {
            headers: &self.headers,
            tracks: &self.tracks,
            track_of_joint: &self.track_of_joint,
            key_times: &self.key_times,
            key_values: &self.key_values,
        }
    }

    /// The pool `Arc`s and generation, for the staging snapshot.
    pub(crate) fn staged(&self) -> StagedClipPools {
        (
            std::sync::Arc::clone(&self.headers),
            std::sync::Arc::clone(&self.tracks),
            std::sync::Arc::clone(&self.track_of_joint),
            std::sync::Arc::clone(&self.key_times),
            std::sync::Arc::clone(&self.key_values),
            self.generation,
        )
    }
}

/// Extend an `Arc<Vec<u32>>` with `count` copies of `value` (a helper keeping
/// [`ClipArena::ensure_clip`] readable).
fn arena_pool_fill(pool: &mut std::sync::Arc<Vec<u32>>, value: u32, count: usize) {
    std::sync::Arc::make_mut(pool).extend(std::iter::repeat_n(value, count));
}

/// The clip arena's pool `Arc`s plus generation, as [`ClipArena::staged`]
/// hands them to the staging snapshot: `(headers, tracks, track_of_joint,
/// key_times, key_values, generation)`.
pub(crate) type StagedClipPools = (
    std::sync::Arc<Vec<GpuClipHeader>>,
    std::sync::Arc<Vec<GpuJointTrack>>,
    std::sync::Arc<Vec<u32>>,
    std::sync::Arc<Vec<f32>>,
    std::sync::Arc<Vec<Vec4>>,
    u64,
);

/// A borrowed view of the clip arena's pools — what the CPU mirrors (and the
/// readback expectation) sample from, so they read exactly the bytes the GPU
/// reads.
#[derive(Clone, Copy)]
pub(crate) struct ClipSlices<'a> {
    /// The clip headers.
    pub(crate) headers: &'a [GpuClipHeader],
    /// The shared track pool.
    pub(crate) tracks: &'a [GpuJointTrack],
    /// The shared joint→track lookup pool.
    pub(crate) track_of_joint: &'a [u32],
    /// The shared keyframe time pool.
    pub(crate) key_times: &'a [f32],
    /// The shared keyframe value pool.
    pub(crate) key_values: &'a [Vec4],
}

/// The reference viewer's `cubic_step` (the animation ease ramp), mirrored
/// bit-for-bit from `sl_anim::sample`'s private copy.
fn mirror_cubic_step(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Mirror of [`Motion::playback_time`] over an uploaded header: map elapsed
/// seconds to the time within the motion, honouring the loop points.
pub(crate) fn mirror_playback_time(header: &GpuClipHeader, elapsed: f32) -> f32 {
    let time = elapsed.max(0.0);
    if header.flags & CLIP_FLAG_LOOPS == 0 {
        return time;
    }
    if header.duration == 0.0 {
        return 0.0;
    }
    if time <= header.loop_out {
        return time;
    }
    let span = header.loop_out - header.loop_in;
    if span == 0.0 {
        return header.loop_out;
    }
    header.loop_in + (time - header.loop_out) % span
}

/// Mirror of [`Motion::pose_weight`] over an uploaded header: the cubic
/// ease-in/out weight at `elapsed` wall seconds, `stopped_at` in the
/// [`PLAY_STOPPED_NONE`] encoding.
pub(crate) fn mirror_pose_weight(header: &GpuClipHeader, elapsed: f32, stopped_at: f32) -> f32 {
    let ease_in_weight = |at: f32| {
        if header.ease_in <= 0.0 {
            1.0
        } else {
            mirror_cubic_step(at / header.ease_in)
        }
    };
    let ease_in = ease_in_weight(elapsed);
    // ease_out_start: a non-looping motion ends at its duration; an explicit
    // stop wins when earlier.
    let natural =
        (header.flags & CLIP_FLAG_LOOPS == 0).then(|| (header.duration - header.ease_out).max(0.0));
    let stopped = (stopped_at >= 0.0).then_some(stopped_at);
    let start = match (stopped, natural) {
        (Some(stop), Some(nat)) => stop.min(nat),
        (Some(stop), None) => stop,
        (None, natural) => match natural {
            Some(nat) => nat,
            None => return ease_in,
        },
    };
    if elapsed < start {
        return ease_in;
    }
    if header.ease_out <= 0.0 {
        return 0.0;
    }
    let residual = ease_in_weight(start);
    let fraction = (elapsed - start) / header.ease_out;
    residual * mirror_cubic_step(1.0 - fraction)
}

/// The `lower_bound` binary search the WGSL sampler runs: the first index in
/// `times` whose value is `>= time` (== `times.len()` when past the last).
/// Identical result to `sl_anim`'s linear `position` scan on ascending keys.
fn mirror_lower_bound(times: &[f32], time: f32) -> usize {
    let mut lo = 0_usize;
    let mut hi = times.len();
    while lo < hi {
        let mid = lo.midpoint(hi);
        if times.get(mid).is_some_and(|&at| at >= time) {
            hi = mid;
        } else {
            lo = mid.saturating_add(1);
        }
    }
    lo
}

/// Component lerp of two quaternions then renormalize — `sl_anim`'s
/// `lerp_quaternions`, mirrored bit-for-bit.
fn mirror_lerp_quat(fraction: f32, a: Vec4, b: Vec4) -> Vec4 {
    let inv = 1.0 - fraction;
    mirror_normalize_quat(Vec4::new(
        inv * a.x + fraction * b.x,
        inv * a.y + fraction * b.y,
        inv * a.z + fraction * b.z,
        inv * a.w + fraction * b.w,
    ))
}

/// Spherical quaternion interpolation with the short-arc flip — `sl_anim`'s
/// `slerp_quaternions`, mirrored bit-for-bit.
fn mirror_slerp_quat(fraction: f32, a: Vec4, b: Vec4) -> Vec4 {
    let raw_cos = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
    let flip = raw_cos < 0.0;
    let cos_theta = if flip { -raw_cos } else { raw_cos };
    let (mut beta, alpha) = if 1.0 - cos_theta < 0.00001 {
        (1.0 - fraction, fraction)
    } else {
        let theta = cos_theta.acos();
        let sin_theta = theta.sin();
        (
            (theta - fraction * theta).sin() / sin_theta,
            (fraction * theta).sin() / sin_theta,
        )
    };
    if flip {
        beta = -beta;
    }
    Vec4::new(
        beta * a.x + alpha * b.x,
        beta * a.y + alpha * b.y,
        beta * a.z + alpha * b.z,
        beta * a.w + alpha * b.w,
    )
}

/// Normalize a quaternion, degenerate → identity — `sl_anim`'s
/// `normalize_quaternion`, mirrored bit-for-bit.
fn mirror_normalize_quat(q: Vec4) -> Vec4 {
    let length_sq = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    if length_sq > 0.0 {
        let inv = 1.0 / length_sq.sqrt();
        Vec4::new(q.x * inv, q.y * inv, q.z * inv, q.w * inv)
    } else {
        Vec4::new(0.0, 0.0, 0.0, 1.0)
    }
}

/// Short-arc normalized quaternion interpolation — `sl_anim`'s
/// `nlerp_quaternions`, mirrored bit-for-bit (plain lerp same-hemisphere,
/// slerp otherwise).
pub(crate) fn mirror_nlerp_quat(fraction: f32, a: Vec4, b: Vec4) -> Vec4 {
    let dot = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
    if dot < 0.0 {
        mirror_slerp_quat(fraction, a, b)
    } else {
        mirror_lerp_quat(fraction, a, b)
    }
}

/// Component vector lerp — `sl_anim`'s `lerp_vector3`, mirrored bit-for-bit.
fn mirror_lerp_vec3(fraction: f32, a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.x + fraction * (b.x - a.x),
        a.y + fraction * (b.y - a.y),
        a.z + fraction * (b.z - a.z),
    )
}

/// Sample one keyframe channel at `time` — the reference `getValue` clamp /
/// exact-key / interpolate logic over the shared pools, mirroring the WGSL
/// sampler (and `sl_anim`'s `sample_curve`) exactly. `None` for an empty
/// channel.
fn mirror_sample_channel(
    slices: ClipSlices<'_>,
    offset: u32,
    count: u32,
    time: f32,
    interp: impl Fn(f32, Vec4, Vec4) -> Vec4,
) -> Option<Vec4> {
    let start = usize::try_from(offset).ok()?;
    let count = usize::try_from(count).ok()?;
    if count == 0 {
        return None;
    }
    let times = slices.key_times.get(start..start.checked_add(count)?)?;
    let values = slices.key_values.get(start..start.checked_add(count)?)?;
    let right = mirror_lower_bound(times, time);
    if right >= count {
        return values.last().copied();
    }
    let Some(left) = right.checked_sub(1) else {
        return values.first().copied();
    };
    let before = times.get(left).copied()?;
    let after = times.get(right).copied()?;
    let span = after - before;
    if span == 0.0 {
        return values.get(right).copied();
    }
    let fraction = (time - before) / span;
    Some(interp(
        fraction,
        values.get(left).copied()?,
        values.get(right).copied()?,
    ))
}

/// The Rust mirror of one pass-A thread: sample track `track_index` (a pool
/// index) at motion time `time` into a pose-cache row.
pub(crate) fn mirror_sample_track(
    slices: ClipSlices<'_>,
    track_index: u32,
    time: f32,
) -> GpuLocalPose {
    let mut row = GpuLocalPose::default();
    let Some(track) = usize::try_from(track_index)
        .ok()
        .and_then(|index| slices.tracks.get(index))
    else {
        return row;
    };
    if let Some(rot) = mirror_sample_channel(
        slices,
        track.rot_offset,
        track.rot_count,
        time,
        mirror_nlerp_quat,
    ) {
        row.rot = rot;
        row.flags |= POSE_FLAG_ROT;
    }
    if let Some(pos) = mirror_sample_channel(
        slices,
        track.pos_offset,
        track.pos_count,
        time,
        |f, a, b| mirror_lerp_vec3(f, a.truncate(), b.truncate()).extend(0.0),
    ) {
        row.pos = pos.truncate();
        row.flags |= POSE_FLAG_POS;
    }
    row
}

/// The Rust mirror of pass A whole: run every staged sample job into a CPU
/// pose cache (indexed exactly like the GPU one: `job.cache_base + t`).
pub(crate) fn mirror_pose_cache(
    slices: ClipSlices<'_>,
    jobs: &[GpuSampleJob],
    cache_len: u32,
) -> Vec<GpuLocalPose> {
    let mut cache = vec![GpuLocalPose::default(); usize::try_from(cache_len).unwrap_or(0)];
    for job in jobs {
        let Some(header) = usize::try_from(job.clip_id)
            .ok()
            .and_then(|index| slices.headers.get(index))
        else {
            continue;
        };
        let time = mirror_playback_time(header, job.phase);
        for t in 0..header.track_count {
            let Some(track_index) = header.track_offset.checked_add(t) else {
                continue;
            };
            let row = mirror_sample_track(slices, track_index, time);
            let slot = job
                .cache_base
                .checked_add(t)
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| cache.get_mut(index));
            if let Some(slot) = slot {
                *slot = row;
            }
        }
    }
    cache
}

/// The Rust mirror of one pass-B thread: gather this joint's contributions
/// from one avatar's playback slots, blend them by priority with the running
/// weight budget (an exact port of [`sl_anim::blend_joint`] under
/// `resolve_pose`'s gather semantics: zero-weight and channel-less
/// contributions never enter the slot cap), fold the procedural idle
/// adjusters, and apply this joint's correction if one is staged.
#[expect(
    clippy::too_many_arguments,
    reason = "the mirror takes exactly the WGSL pass's bindings — the arena view, the \
              avatar's playback slots, the pose cache, the joint, and the frame params; \
              packing them into a struct would only obscure the 1:1 WGSL correspondence"
)]
pub(crate) fn mirror_blend_joint(
    slices: ClipSlices<'_>,
    plays: &[GpuPlayState],
    cache: &[GpuLocalPose],
    joint: u32,
    now: f32,
    idle: Option<f32>,
    chest_joint: u32,
    torso_joint: u32,
    corrections: &[(u32, GpuLocalPose)],
) -> GpuLocalPose {
    /// One gathered contribution, ordered like the WGSL's fixed arrays.
    struct Contribution {
        /// The track's effective priority.
        priority: i32,
        /// The play state's recency stamp.
        order: u32,
        /// The motion's ease weight this frame.
        weight: f32,
        /// The cached sampled channels.
        value: GpuLocalPose,
    }
    let mut contributions: Vec<Contribution> = Vec::new();
    for play in plays {
        if play.clip_id == CLIP_NONE {
            continue;
        }
        let Some(header) = usize::try_from(play.clip_id)
            .ok()
            .and_then(|index| slices.headers.get(index))
        else {
            continue;
        };
        let track = header
            .track_of_joint_offset
            .checked_add(joint)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| slices.track_of_joint.get(index))
            .copied()
            .unwrap_or(TRACK_NONE);
        if track == TRACK_NONE {
            continue;
        }
        let weight = mirror_pose_weight(header, now - play.start, play.stopped_at);
        if weight <= 0.0 {
            continue;
        }
        let value = play
            .cache_base
            .checked_add(track)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| cache.get(index))
            .copied()
            .unwrap_or_default();
        if value.flags == 0 {
            continue;
        }
        let priority = header
            .track_offset
            .checked_add(track)
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| slices.tracks.get(index))
            .map_or(0, |entry| entry.priority);
        contributions.push(Contribution {
            priority,
            order: play.order,
            weight,
            value,
        });
    }
    // Priority descending, recency descending for ties — `blend_joint`'s slot
    // order (the keys are unique per avatar, so the order is total).
    contributions.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| b.order.cmp(&a.order))
    });
    contributions.truncate(4);
    let mut out = GpuLocalPose::default();
    let mut sum_rotation = 0.0_f32;
    let mut sum_position = 0.0_f32;
    for contribution in &contributions {
        if contribution.value.flags & POSE_FLAG_ROT != 0 {
            if out.flags & POSE_FLAG_ROT == 0 {
                out.rot = contribution.value.rot;
                out.flags |= POSE_FLAG_ROT;
                sum_rotation = contribution.weight;
            } else {
                let new_sum = (contribution.weight + sum_rotation).min(1.0);
                let fraction = sum_rotation / new_sum;
                out.rot = mirror_nlerp_quat(fraction, contribution.value.rot, out.rot);
                sum_rotation = new_sum;
            }
        }
        if contribution.value.flags & POSE_FLAG_POS != 0 {
            if out.flags & POSE_FLAG_POS == 0 {
                out.pos = contribution.value.pos;
                out.flags |= POSE_FLAG_POS;
                sum_position = contribution.weight;
            } else {
                let new_sum = (contribution.weight + sum_position).min(1.0);
                let fraction = sum_position / new_sum;
                out.pos = mirror_lerp_vec3(fraction, contribution.value.pos, out.pos);
                sum_position = new_sum;
            }
        }
    }
    // The procedural idle adjusters (P31.8), composed exactly like the CPU
    // `apply_idle_adjustments`: a small delta on top of whatever the blend
    // produced for the joint.
    if let Some(idle_now) = idle {
        if joint == chest_joint && chest_joint != JOINT_NONE {
            let base = if out.flags & POSE_FLAG_ROT != 0 {
                Quat::from_xyzw(out.rot.x, out.rot.y, out.rot.z, out.rot.w)
            } else {
                Quat::IDENTITY
            };
            let composed = base.mul_quat(crate::procedural::breathe_rotation(idle_now));
            out.rot = Vec4::new(composed.x, composed.y, composed.z, composed.w);
            out.flags |= POSE_FLAG_ROT;
        }
        if joint == torso_joint && torso_joint != JOINT_NONE {
            let base = if out.flags & POSE_FLAG_ROT != 0 {
                Quat::from_xyzw(out.rot.x, out.rot.y, out.rot.z, out.rot.w)
            } else {
                Quat::IDENTITY
            };
            let composed = base.mul_quat(crate::procedural::body_noise_rotation(idle_now));
            out.rot = Vec4::new(composed.x, composed.y, composed.z, composed.w);
            out.flags |= POSE_FLAG_ROT;
        }
    }
    // The sparse CPU correction, replacing the affected channels outright
    // (the corrections slice is this avatar's, sorted by joint).
    if let Ok(found) = corrections.binary_search_by_key(&joint, |&(at, _value)| at)
        && let Some(&(_joint, value)) = corrections.get(found)
    {
        if value.flags & POSE_FLAG_ROT != 0 {
            out.rot = value.rot;
            out.flags |= POSE_FLAG_ROT;
        }
        if value.flags & POSE_FLAG_POS != 0 {
            out.pos = value.pos;
            out.flags |= POSE_FLAG_POS;
        }
    }
    out
}

/// The Rust mirror of passes A+B for **one avatar**: its full `LocalPose` row
/// block from the staged playback slots, jobs and corrections — the CPU
/// reference the real-placement readback verdict compares the GPU's
/// `LocalPose` against (via [`reference_fk`] into palettes).
#[expect(
    clippy::too_many_arguments,
    reason = "the mirror takes exactly the WGSL passes' bindings and frame params; \
              packing them into a struct would only obscure the 1:1 WGSL \
              correspondence"
)]
pub(crate) fn mirror_local_pose(
    slices: ClipSlices<'_>,
    plays: &[GpuPlayState],
    jobs: &[GpuSampleJob],
    cache_len: u32,
    joint_count: u32,
    now: f32,
    idle: Option<f32>,
    chest_joint: u32,
    torso_joint: u32,
    corrections: &[(u32, GpuLocalPose)],
) -> Vec<GpuLocalPose> {
    let cache = mirror_pose_cache(slices, jobs, cache_len);
    (0..joint_count)
        .map(|joint| {
            mirror_blend_joint(
                slices,
                plays,
                &cache,
                joint,
                now,
                idle,
                chest_joint,
                torso_joint,
                corrections,
            )
        })
        .collect()
}
