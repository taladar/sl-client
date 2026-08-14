// The GPU-avatar pose pipeline (`roadmap/context/gpu-avatars.md` §2.2):
// `sample` (pass A) interpolates each deduplicated (clip, phase) job's
// keyframe tracks into the pose cache — an exact port of
// `sl_anim`'s `playback_time` + `sample_curve` (binary search, short-arc
// nlerp); `blend` (pass B) gathers ≤ MAX_ACTIVE contributions per
// (avatar, joint), blends them by priority with the running weight budget —
// an exact port of `blend_joint` + `Motion::pose_weight` — folds the
// procedural idle adjusters in and applies the sparse CPU corrections,
// writing the avatar's `LocalPose`; `fk` (pass C) runs the Second Life
// skeletal recurrence — an exact port of
// `BevySkeleton::deformed_world_matrices`' inner loop over CPU-composed rest
// rows and that local pose — into per-joint Bevy-world matrices;
// `palettes` (pass D) multiplies them with each instance's inverse bindposes
// and writes the result into Bevy's `SkinUniforms` palette buffer at the
// offsets Bevy allocated (the spike-proven §2.4 write-in). `readback_palette`
// is the debug verdict channel: it copies a chosen instance's just-written
// palette range next to the CPU-expected palette so the CPU can diff them
// race-free.
//
// Every struct here mirrors a `#[derive(ShaderType)]` struct in `types.rs`;
// the Rust packing tests pin the byte layouts to what these declarations mean
// under std430/std140.

/// Mirror of `types.rs` `GpuComputeParams` (uniform, std140).
struct Params {
    avatar_count: u32,
    joint_count: u32,
    instance_count: u32,
    max_skin_joints: u32,
    readback_instance: u32,
    readback_joint_count: u32,
    sample_job_count: u32,
    correction_count: u32,
    now: f32,
    idle_now: f32,
    chest_joint: u32,
    torso_joint: u32,
    flags: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

/// Mirror of `types.rs` `GpuAvatarFrame`.
struct AvatarFrame {
    root: mat4x4<f32>,
    slot: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

/// Mirror of `types.rs` `GpuRestJoint`.
struct RestJoint {
    rest_pos: vec3<f32>,
    parent: u32,
    rest_rot: vec4<f32>,
    local_scale: vec3<f32>,
    flags: u32,
}

/// Mirror of `types.rs` `GpuLocalPose`.
struct LocalPose {
    rot: vec4<f32>,
    pos: vec3<f32>,
    flags: u32,
}

/// Mirror of `types.rs` `GpuSkinInstance`.
struct SkinInstance {
    avatar_slot: u32,
    palette_offset: u32,
    joint_count: u32,
    joint_map_offset: u32,
    ibp_offset: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

/// Mirror of `types.rs` `GpuClipHeader`.
struct ClipHeader {
    duration: f32,
    loop_in: f32,
    loop_out: f32,
    ease_in: f32,
    ease_out: f32,
    flags: u32,
    track_count: u32,
    track_offset: u32,
    track_of_joint_offset: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

/// Mirror of `types.rs` `GpuJointTrack`.
struct JointTrack {
    joint: u32,
    priority: i32,
    rot_offset: u32,
    rot_count: u32,
    pos_offset: u32,
    pos_count: u32,
    pad0: u32,
    pad1: u32,
}

/// Mirror of `types.rs` `GpuSampleJob`.
struct SampleJob {
    clip_id: u32,
    cache_base: u32,
    phase: f32,
    pad0: u32,
}

/// Mirror of `types.rs` `GpuPlayState`.
struct PlayState {
    clip_id: u32,
    cache_base: u32,
    start: f32,
    stopped_at: f32,
    order: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

/// Mirror of `types.rs` `GpuCorrection`.
struct Correction {
    avatar: u32,
    joint: u32,
    flags: u32,
    pad0: u32,
    rot: vec4<f32>,
    pos: vec3<f32>,
    pad1: f32,
}

// `types.rs` flag constants, mirrored.
const PARENT_NONE: u32 = 0xffffffffu;
const REST_FLAG_VOLUME: u32 = 1u;
const REST_FLAG_PELVIS: u32 = 2u;
const REST_FLAG_OVERRIDE: u32 = 4u;
const POSE_FLAG_ROT: u32 = 1u;
const POSE_FLAG_POS: u32 = 2u;
const CLIP_NONE: u32 = 0xffffffffu;
const TRACK_NONE: u32 = 0xffffffffu;
const CLIP_FLAG_LOOPS: u32 = 1u;
const PARAMS_FLAG_TPOSE: u32 = 1u;
const JOINT_NONE: u32 = 0xffffffffu;

// `types.rs` MAX_GPU_JOINTS: the pass-C private array size; staging rejects a
// bigger skeleton.
const MAX_GPU_JOINTS: u32 = 256u;

// `types.rs` MAX_ACTIVE_CLIPS: one avatar's playback row block.
const MAX_ACTIVE: u32 = 16u;

// `sl_anim::blend::MAX_JOINT_CONTRIBUTIONS`: the reference blends 4 slots.
const MAX_CONTRIBUTIONS: u32 = 4u;

// `procedural.rs` constants, mirrored: the breathe pitch strength, the torso
// sway amplitude (1 degree, in radians) and its time scale.
const BREATHE_ROT_MOTION_STRENGTH: f32 = 0.05;
const TORSO_NOISE_AMOUNT_RAD: f32 = 0.017453292519943295;
const TORSO_NOISE_SPEED: f32 = 0.2;
const TAU: f32 = 6.283185307179586;

@group(0) @binding(0) var<uniform> params: Params;
/// One entry per posed avatar this frame (compact, not slot-indexed).
@group(0) @binding(1) var<storage, read> frames: array<AvatarFrame>;
/// Slot-indexed rest rows: `rest_joints[slot * joint_count + j]`.
@group(0) @binding(2) var<storage, read> rest_joints: array<RestJoint>;
/// Slot-indexed CPU-blended local pose rows, same indexing.
@group(0) @binding(3) var<storage, read> local_pose: array<LocalPose>;
/// Slot-indexed joint world matrices — pass C output, pass D input.
@group(0) @binding(4) var<storage, read_write> joint_world: array<mat4x4<f32>>;
/// The shared canonical-joint-index pool all instances' joint maps live in.
@group(0) @binding(5) var<storage, read> joint_map: array<u32>;
/// The shared inverse-bindpose pool.
@group(0) @binding(6) var<storage, read> ibps: array<mat4x4<f32>>;
/// The ghost skin instances pass D writes.
@group(0) @binding(7) var<storage, read> instances: array<SkinInstance>;
/// Bevy's live `SkinUniforms.current_buffer` — the exact storage buffer
/// `skinning.wgsl` reads joint palettes from, bound read-write (the
/// spike-proven binding).
@group(0) @binding(8) var<storage, read_write> palette: array<mat4x4<f32>>;
/// The CPU-expected palette of the readback instance (readback pipeline only).
@group(0) @binding(9) var<storage, read> expected: array<mat4x4<f32>>;
/// The readback destination: `readback_joint_count` copied palette entries
/// followed by the same count of expected entries (readback pipeline only).
@group(0) @binding(10) var<storage, read_write> readback_dest: array<mat4x4<f32>>;
/// This frame's deduplicated sample jobs (pass A input).
@group(0) @binding(11) var<storage, read> jobs: array<SampleJob>;
/// The clip arena's headers (passes A+B).
@group(0) @binding(12) var<storage, read> clip_headers: array<ClipHeader>;
/// The clip arena's shared joint-track pool (passes A+B).
@group(0) @binding(13) var<storage, read> clip_tracks: array<JointTrack>;
/// The clip arena's shared joint→track lookup pool (pass B).
@group(0) @binding(14) var<storage, read> track_of_joint: array<u32>;
/// The clip arena's shared keyframe time pool (pass A).
@group(0) @binding(15) var<storage, read> key_times: array<f32>;
/// The clip arena's shared keyframe value pool (pass A).
@group(0) @binding(16) var<storage, read> key_values: array<vec4<f32>>;
/// The transient per-frame pose cache — pass A output, pass B input.
@group(0) @binding(17) var<storage, read_write> pose_cache: array<LocalPose>;
/// The frame-indexed playback row blocks (`MAX_ACTIVE` per avatar, pass B).
@group(0) @binding(18) var<storage, read> playback: array<PlayState>;
/// The sparse CPU corrections, sorted by (avatar, joint) (pass B).
@group(0) @binding(19) var<storage, read> corrections: array<Correction>;
/// The slot-indexed local pose as pass B **writes** it (the same buffer
/// binding 3 reads in pass C, bound read-write under pass B's layout).
@group(0) @binding(20) var<storage, read_write> local_pose_out: array<LocalPose>;
/// The per-slot posed **world-space** AABB (Phase 5 frustum culling): two
/// `vec4` per slot — `bounds_out[2*slot]` the min `xyz`, `bounds_out[2*slot+1]`
/// the max `xyz` (the `w` lanes are padding). Pass `bounds` reduces pass C's
/// posed joint world positions into this; the CPU reads it back and sets each
/// avatar's `Aabb` so off-screen avatars frustum-cull (the `bounds` layout
/// only).
@group(0) @binding(21) var<storage, read_write> bounds_out: array<vec4<f32>>;

// The fixed per-slot capacity of `bounds_out` (mirrors `render.rs`
// `BOUND_SLOT_CAP`): a slot at or beyond this is not written, so its avatar
// keeps the CPU's generous default AABB (unculled) — over-inclusive is safe.
const BOUND_SLOT_CAP: u32 = 4096u;

/// Hamilton product `a * b`, glam `Quat::mul_quat`'s component formula.
fn quat_mul(a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(
        a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    );
}

/// Rotate `v` by quaternion `q` — glam `Quat::mul_vec3`'s formula.
fn quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let b = q.xyz;
    let b2 = dot(b, b);
    return v * (q.w * q.w - b2) + b * (dot(v, b) * 2.0) + cross(b, v) * (q.w * 2.0);
}

/// Compose a TRS matrix exactly like glam's
/// `Mat4::from_scale_rotation_translation`: rotation basis from the
/// quaternion, each basis column scaled by its axis scale, translation in the
/// w column.
fn compose_trs(scale: vec3<f32>, q: vec4<f32>, t: vec3<f32>) -> mat4x4<f32> {
    let x2 = q.x + q.x;
    let y2 = q.y + q.y;
    let z2 = q.z + q.z;
    let xx = q.x * x2;
    let xy = q.x * y2;
    let xz = q.x * z2;
    let yy = q.y * y2;
    let yz = q.y * z2;
    let zz = q.z * z2;
    let wx = q.w * x2;
    let wy = q.w * y2;
    let wz = q.w * z2;
    let c0 = vec3<f32>(1.0 - (yy + zz), xy + wz, xz - wy) * scale.x;
    let c1 = vec3<f32>(xy - wz, 1.0 - (xx + zz), yz + wx) * scale.y;
    let c2 = vec3<f32>(xz + wy, yz - wx, 1.0 - (xx + yy)) * scale.z;
    return mat4x4<f32>(
        vec4<f32>(c0, 0.0),
        vec4<f32>(c1, 0.0),
        vec4<f32>(c2, 0.0),
        vec4<f32>(t, 1.0),
    );
}

/// Pass C — hierarchical FK, one thread per avatar (§2.2 v1): a serial walk
/// over the canonical joints (parents precede children by construction; the
/// one forward reference — the appended synthetic identity root — takes the
/// same identity fallback the CPU reference takes), running the exact
/// `deformed_world_matrices` recurrence, then composing the frame's root
/// affine into each joint's final matrix.
@compute @workgroup_size(64)
fn fk(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.avatar_count) {
        return;
    }
    let frame = frames[gid.x];
    let base = frame.slot * params.joint_count;
    var world_rot: array<vec4<f32>, MAX_GPU_JOINTS>;
    var world_pos: array<vec3<f32>, MAX_GPU_JOINTS>;
    for (var j = 0u; j < params.joint_count; j++) {
        let rest = rest_joints[base + j];
        let pose = local_pose[base + j];
        var local_rot = rest.rest_rot;
        if ((pose.flags & POSE_FLAG_ROT) != 0u) {
            local_rot = pose.rot;
        }
        // The position-key semantics, arm for arm with the CPU reference:
        // additive for `mPelvis` / collision volumes, absolute otherwise
        // unless a rig override wins.
        var position = rest.rest_pos;
        if ((pose.flags & POSE_FLAG_POS) != 0u) {
            if ((rest.flags & (REST_FLAG_VOLUME | REST_FLAG_PELVIS)) != 0u) {
                position = rest.rest_pos + pose.pos;
            } else if ((rest.flags & REST_FLAG_OVERRIDE) == 0u) {
                position = pose.pos;
            }
        }
        var rot = local_rot;
        var pos = position;
        // A forward parent reference (`parent >= j`, only the synthetic
        // identity root in practice) takes the identity/zero/unit-scale
        // fallback, mirroring the CPU reference's `Vec::get` behaviour.
        if (rest.parent != PARENT_NONE && rest.parent < j) {
            let parent_rot = world_rot[rest.parent];
            let parent_pos = world_pos[rest.parent];
            let parent_scale = rest_joints[base + rest.parent].local_scale;
            // Child offset scaled by the parent's *local* scale, rotated into
            // and translated by the parent's world frame.
            let rotated = quat_rotate(parent_rot, parent_scale * position);
            rot = quat_mul(parent_rot, local_rot);
            pos = parent_pos + rotated;
        }
        world_rot[j] = rot;
        world_pos[j] = pos;
        // Own scale enters only the final matrix (never inherited), then the
        // root affine.
        joint_world[base + j] = frame.root * compose_trs(rest.local_scale, rot, pos);
    }
}

/// Pass — posed world-space bounds (Phase 5): one thread per posed avatar
/// frame, reducing that slot's `joint_count` posed joint world **positions**
/// (pass C's output, read here) into an axis-aligned min/max box in Bevy world
/// space. The CPU reads it back, expands it by a flesh + motion margin, and
/// sets the avatar's `Aabb`, so an off-screen avatar frustum-culls instead of
/// carrying `NoFrustumCulling`. Runs after `fk` in the same compute pass
/// (storage writes are visible between dispatches). A joint span is a
/// conservative under-bound of the skinned flesh — the CPU margin covers the
/// difference.
@compute @workgroup_size(64)
fn bounds(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.avatar_count) {
        return;
    }
    let frame = frames[gid.x];
    if (frame.slot >= BOUND_SLOT_CAP) {
        return;
    }
    let base = frame.slot * params.joint_count;
    // The 4th column of each joint world matrix is its Bevy-world translation.
    var lo = joint_world[base][3].xyz;
    var hi = lo;
    for (var j = 1u; j < params.joint_count; j++) {
        let p = joint_world[base + j][3].xyz;
        lo = min(lo, p);
        hi = max(hi, p);
    }
    bounds_out[2u * frame.slot] = vec4<f32>(lo, 0.0);
    bounds_out[2u * frame.slot + 1u] = vec4<f32>(hi, 0.0);
}

/// Pass D — skin palettes: one thread per (palette entry, instance), writing
/// `world[joint_map[k]] * ibp[k]` into Bevy's palette buffer at the
/// Bevy-allocated offset. Runs after `fk` in the same compute pass (storage
/// writes are visible between dispatches of one pass).
@compute @workgroup_size(64)
fn palettes(@builtin(global_invocation_id) gid: vec3<u32>) {
    let instance_index = gid.y;
    if (instance_index >= params.instance_count) {
        return;
    }
    let inst = instances[instance_index];
    let k = gid.x;
    if (k >= inst.joint_count) {
        return;
    }
    let cj = joint_map[inst.joint_map_offset + k];
    let world = joint_world[inst.avatar_slot * params.joint_count + cj];
    palette[inst.palette_offset + k] = world * ibps[inst.ibp_offset + k];
}

/// The debug verdict readback (the spike's compute-copy pattern —
/// `SkinUniforms.current_buffer` has no `COPY_SRC`, so a buffer-to-buffer copy
/// is rejected and the palette range must be lifted through a storage
/// binding): copy the readback instance's just-written palette range into the
/// destination, followed by this frame's CPU-expected palette — both halves
/// from the same submission, so the CPU comparison is race-free against a
/// moving avatar.
@compute @workgroup_size(64)
fn readback_palette(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (params.readback_instance >= params.instance_count) {
        return;
    }
    let inst = instances[params.readback_instance];
    let k = gid.x;
    if (k >= params.readback_joint_count) {
        return;
    }
    readback_dest[k] = palette[inst.palette_offset + k];
    readback_dest[params.readback_joint_count + k] = expected[k];
}

// ---------------------------------------------------------------------------
// Passes A + B (`roadmap/context/gpu-avatars.md` §2.2): clip sample and
// priority/ease blend — exact ports of `sl_anim`'s sampling / blending.
// ---------------------------------------------------------------------------

/// The reference viewer's `cubic_step(x)`: smoothstep with the argument
/// clamped to `0..=1` (`sl_anim::sample::cubic_step`).
fn cubic_step(x: f32) -> f32 {
    let c = clamp(x, 0.0, 1.0);
    return c * c * (3.0 - 2.0 * c);
}

/// Port of `Motion::playback_time`: map elapsed seconds (already clamped
/// non-negative by the caller) to the time within the motion, honouring the
/// loop points. WGSL `%` is the same truncated remainder Rust's `%` is.
fn playback_time(clip: ClipHeader, elapsed: f32) -> f32 {
    let time = max(elapsed, 0.0);
    if ((clip.flags & CLIP_FLAG_LOOPS) == 0u) {
        return time;
    }
    if (clip.duration == 0.0) {
        return 0.0;
    }
    if (time <= clip.loop_out) {
        return time;
    }
    let span = clip.loop_out - clip.loop_in;
    if (span == 0.0) {
        return clip.loop_out;
    }
    return clip.loop_in + (time - clip.loop_out) % span;
}

/// Port of `Motion::pose_weight`: the cubic ease-in/out weight at `elapsed`
/// wall seconds; `stopped_at < 0` encodes "still signalled".
fn pose_weight(clip: ClipHeader, elapsed: f32, stopped_at: f32) -> f32 {
    var ease_in = 1.0;
    if (clip.ease_in > 0.0) {
        ease_in = cubic_step(elapsed / clip.ease_in);
    }
    var has_start = false;
    var start = 0.0;
    if ((clip.flags & CLIP_FLAG_LOOPS) == 0u) {
        start = max(clip.duration - clip.ease_out, 0.0);
        has_start = true;
    }
    if (stopped_at >= 0.0) {
        if (has_start) {
            start = min(stopped_at, start);
        } else {
            start = stopped_at;
            has_start = true;
        }
    }
    if (!has_start) {
        return ease_in;
    }
    if (elapsed < start) {
        return ease_in;
    }
    if (clip.ease_out <= 0.0) {
        return 0.0;
    }
    var residual = 1.0;
    if (clip.ease_in > 0.0) {
        residual = cubic_step(start / clip.ease_in);
    }
    let fraction = (elapsed - start) / clip.ease_out;
    return residual * cubic_step(1.0 - fraction);
}

/// The first key index in `[offset, offset+count)` whose time is `>= time`
/// (`count` when past the last) — the reference `lower_bound`.
fn lower_bound(offset: u32, count: u32, time: f32) -> u32 {
    var lo = 0u;
    var hi = count;
    while (lo < hi) {
        let mid = (lo + hi) / 2u;
        if (key_times[offset + mid] >= time) {
            hi = mid;
        } else {
            lo = mid + 1u;
        }
    }
    return lo;
}

/// Normalize a quaternion, degenerate → identity
/// (`sl_anim::sample::normalize_quaternion`).
fn normalize_quat(q: vec4<f32>) -> vec4<f32> {
    let length_sq = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    if (length_sq > 0.0) {
        let inv = 1.0 / sqrt(length_sq);
        return q * inv;
    }
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

/// Spherical quaternion interpolation with the short-arc flip
/// (`sl_anim::sample::slerp_quaternions`).
fn slerp_quat(fraction: f32, a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    let raw_cos = dot(a, b);
    let flip = raw_cos < 0.0;
    var cos_theta = raw_cos;
    if (flip) {
        cos_theta = -raw_cos;
    }
    var beta = 1.0 - fraction;
    var alpha = fraction;
    if (1.0 - cos_theta >= 0.00001) {
        let theta = acos(cos_theta);
        let sin_theta = sin(theta);
        beta = sin(theta - fraction * theta) / sin_theta;
        alpha = sin(fraction * theta) / sin_theta;
    }
    if (flip) {
        beta = -beta;
    }
    return beta * a + alpha * b;
}

/// Short-arc normalized quaternion interpolation
/// (`sl_anim::sample::nlerp_quaternions`): plain lerp same-hemisphere, slerp
/// otherwise.
fn nlerp_quat(fraction: f32, a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    if (dot(a, b) < 0.0) {
        return slerp_quat(fraction, a, b);
    }
    let inv = 1.0 - fraction;
    return normalize_quat(inv * a + fraction * b);
}

/// Sample one keyframe channel at `time` — the reference `getValue` clamp /
/// interpolate logic (`sl_anim::sample::sample_curve`). `is_rotation` selects
/// nlerp vs component lerp. The caller guarantees `count > 0`.
fn sample_channel(offset: u32, count: u32, time: f32, is_rotation: bool) -> vec4<f32> {
    let right = lower_bound(offset, count, time);
    if (right >= count) {
        return key_values[offset + count - 1u];
    }
    if (right == 0u) {
        return key_values[offset];
    }
    let before = key_times[offset + right - 1u];
    let after = key_times[offset + right];
    let span = after - before;
    if (span == 0.0) {
        return key_values[offset + right];
    }
    let fraction = (time - before) / span;
    let a = key_values[offset + right - 1u];
    let b = key_values[offset + right];
    if (is_rotation) {
        return nlerp_quat(fraction, a, b);
    }
    // The reference's component lerp form `a + fraction * (b - a)`
    // (`lerp_vector3`), kept literally for parity with the CPU mirror.
    return a + fraction * (b - a);
}

/// Pass A — clip sample (§2.2, dedup'd): thread (job, t) samples track `t` of
/// the job's clip at the job's phase into the pose cache. One workgroup row
/// per job; x covers the clip's tracks.
@compute @workgroup_size(64)
fn sample(@builtin(global_invocation_id) gid: vec3<u32>) {
    let job_index = gid.y;
    if (job_index >= params.sample_job_count) {
        return;
    }
    let job = jobs[job_index];
    let clip = clip_headers[job.clip_id];
    let t = gid.x;
    if (t >= clip.track_count) {
        return;
    }
    let track = clip_tracks[clip.track_offset + t];
    let time = playback_time(clip, job.phase);
    var out: LocalPose;
    out.rot = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    out.pos = vec3<f32>(0.0, 0.0, 0.0);
    out.flags = 0u;
    if (track.rot_count > 0u) {
        out.rot = sample_channel(track.rot_offset, track.rot_count, time, true);
        out.flags |= POSE_FLAG_ROT;
    }
    if (track.pos_count > 0u) {
        out.pos = sample_channel(track.pos_offset, track.pos_count, time, false).xyz;
        out.flags |= POSE_FLAG_POS;
    }
    pose_cache[job.cache_base + t] = out;
}

/// The breathing chest delta at idle time `t`
/// (`procedural::breathe_rotation`): a pitch of `sin(t) * strength` about the
/// local Y axis, as a quaternion.
fn breathe_rotation(t: f32) -> vec4<f32> {
    let angle = sin(t) * BREATHE_ROT_MOTION_STRENGTH;
    let half = angle * 0.5;
    return vec4<f32>(0.0, sin(half), 0.0, cos(half));
}

/// One band-limited noise stream (`procedural::body_noise_component`).
fn body_noise_component(t: f32, phase: f32) -> f32 {
    let a = sin(t * TAU + phase);
    let b = sin(t * 13.7 + phase * 1.7);
    let c = sin(t * 3.1 + phase * 0.5);
    return a * 0.6 + b * 0.25 + c * 0.15;
}

/// The torso idle-sway delta at idle time `time`
/// (`procedural::body_noise_rotation`): `from_rotation_x(rx) * from_rotation_y(ry)`.
fn body_noise_rotation(time: f32) -> vec4<f32> {
    let t = time * TORSO_NOISE_SPEED;
    let rx = TORSO_NOISE_AMOUNT_RAD * body_noise_component(t, 0.0);
    let ry = TORSO_NOISE_AMOUNT_RAD * body_noise_component(t, 2.0);
    let qx = vec4<f32>(sin(rx * 0.5), 0.0, 0.0, cos(rx * 0.5));
    let qy = vec4<f32>(0.0, sin(ry * 0.5), 0.0, cos(ry * 0.5));
    return quat_mul(qx, qy);
}

/// Pass B — per-joint priority/ease blend + idle + corrections (§2.2): one
/// thread per (avatar, joint). Gathers ≤ MAX_ACTIVE contributions (skipping
/// empty slots, no-track joints, zero weights and channel-less cache rows —
/// `resolve_pose`'s gather semantics), sorts by (priority desc, recency
/// desc), caps at 4 and folds with the running weight budget — the exact
/// `blend_joint` port — then composes the procedural idle deltas and applies
/// this joint's sparse CPU correction, writing the avatar's `LocalPose` row.
@compute @workgroup_size(64)
fn blend(@builtin(global_invocation_id) gid: vec3<u32>) {
    let avatar = gid.y;
    if (avatar >= params.avatar_count) {
        return;
    }
    let joint = gid.x;
    if (joint >= params.joint_count) {
        return;
    }
    let slot = frames[avatar].slot;

    // Gather.
    var prio: array<i32, MAX_ACTIVE>;
    var ord: array<u32, MAX_ACTIVE>;
    var wgt: array<f32, MAX_ACTIVE>;
    var val: array<LocalPose, MAX_ACTIVE>;
    var count = 0u;
    for (var s = 0u; s < MAX_ACTIVE; s++) {
        let play = playback[avatar * MAX_ACTIVE + s];
        if (play.clip_id == CLIP_NONE) {
            continue;
        }
        let clip = clip_headers[play.clip_id];
        let track = track_of_joint[clip.track_of_joint_offset + joint];
        if (track == TRACK_NONE) {
            continue;
        }
        let w = pose_weight(clip, params.now - play.start, play.stopped_at);
        if (w <= 0.0) {
            continue;
        }
        let cached = pose_cache[play.cache_base + track];
        if (cached.flags == 0u) {
            continue;
        }
        prio[count] = clip_tracks[clip.track_offset + track].priority;
        ord[count] = play.order;
        wgt[count] = w;
        val[count] = cached;
        count += 1u;
    }

    // Insertion sort, priority descending then recency descending — the keys
    // are unique per avatar (order stamps are), so the order is total.
    for (var i = 1u; i < count; i++) {
        let p = prio[i];
        let o = ord[i];
        let w = wgt[i];
        let v = val[i];
        var j = i;
        while (j > 0u) {
            let before = j - 1u;
            let wins = (prio[before] > p) || (prio[before] == p && ord[before] > o);
            if (wins) {
                break;
            }
            prio[j] = prio[before];
            ord[j] = ord[before];
            wgt[j] = wgt[before];
            val[j] = val[before];
            j -= 1u;
        }
        prio[j] = p;
        ord[j] = o;
        wgt[j] = w;
        val[j] = v;
    }

    // Fold the top 4 highest-first with the running weight budget
    // (`blend_joint`: new_sum = min(1, w + sum); nlerp(sum / new_sum,
    // incoming, accumulated)).
    var out: LocalPose;
    out.rot = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    out.pos = vec3<f32>(0.0, 0.0, 0.0);
    out.flags = 0u;
    var sum_rotation = 0.0;
    var sum_position = 0.0;
    let cap = min(count, MAX_CONTRIBUTIONS);
    for (var i = 0u; i < cap; i++) {
        if ((val[i].flags & POSE_FLAG_ROT) != 0u) {
            if ((out.flags & POSE_FLAG_ROT) == 0u) {
                out.rot = val[i].rot;
                out.flags |= POSE_FLAG_ROT;
                sum_rotation = wgt[i];
            } else {
                let new_sum = min(wgt[i] + sum_rotation, 1.0);
                let fraction = sum_rotation / new_sum;
                out.rot = nlerp_quat(fraction, val[i].rot, out.rot);
                sum_rotation = new_sum;
            }
        }
        if ((val[i].flags & POSE_FLAG_POS) != 0u) {
            if ((out.flags & POSE_FLAG_POS) == 0u) {
                out.pos = val[i].pos;
                out.flags |= POSE_FLAG_POS;
                sum_position = wgt[i];
            } else {
                let new_sum = min(wgt[i] + sum_position, 1.0);
                let fraction = sum_position / new_sum;
                // `lerp_vector3(fraction, incoming, accumulated)`, literal form.
                out.pos = val[i].pos + fraction * (out.pos - val[i].pos);
                sum_position = new_sum;
            }
        }
    }

    // The procedural idle adjusters (`procedural::apply_idle_adjustments`):
    // a small delta composed on top of the blended channel. Skipped under the
    // T-pose freeze.
    if ((params.flags & PARAMS_FLAG_TPOSE) == 0u) {
        if (joint == params.chest_joint && params.chest_joint != JOINT_NONE) {
            var base = vec4<f32>(0.0, 0.0, 0.0, 1.0);
            if ((out.flags & POSE_FLAG_ROT) != 0u) {
                base = out.rot;
            }
            out.rot = quat_mul(base, breathe_rotation(params.idle_now));
            out.flags |= POSE_FLAG_ROT;
        }
        if (joint == params.torso_joint && params.torso_joint != JOINT_NONE) {
            var base = vec4<f32>(0.0, 0.0, 0.0, 1.0);
            if ((out.flags & POSE_FLAG_ROT) != 0u) {
                base = out.rot;
            }
            out.rot = quat_mul(base, body_noise_rotation(params.idle_now));
            out.flags |= POSE_FLAG_ROT;
        }
    }

    // The sparse CPU correction (look-at / reach / IK / physics, §5.3):
    // binary-search my (avatar, joint) entry and replace the channels it
    // carries.
    let key = avatar * params.joint_count + joint;
    var lo = 0u;
    var hi = params.correction_count;
    while (lo < hi) {
        let mid = (lo + hi) / 2u;
        let entry_key = corrections[mid].avatar * params.joint_count + corrections[mid].joint;
        if (entry_key < key) {
            lo = mid + 1u;
        } else {
            hi = mid;
        }
    }
    if (lo < params.correction_count) {
        let c = corrections[lo];
        if (c.avatar == avatar && c.joint == joint) {
            if ((c.flags & POSE_FLAG_ROT) != 0u) {
                out.rot = c.rot;
                out.flags |= POSE_FLAG_ROT;
            }
            if ((c.flags & POSE_FLAG_POS) != 0u) {
                out.pos = c.pos;
                out.flags |= POSE_FLAG_POS;
            }
        }
    }

    local_pose_out[slot * params.joint_count + joint] = out;
}
