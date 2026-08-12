// The GPU-avatar pose pipeline (`roadmap/context/gpu-avatars.md` §2.2 passes
// C and D): `fk` runs the Second Life skeletal recurrence — an exact port of
// `BevySkeleton::deformed_world_matrices`' inner loop over CPU-composed rest
// rows and the CPU-blended local pose — into per-joint Bevy-world matrices;
// `palettes` multiplies them with each ghost instance's inverse bindposes and
// writes the result into Bevy's `SkinUniforms` palette buffer at the offsets
// Bevy allocated (the spike-proven §2.4 write-in). `readback_palette` is the
// debug verdict channel: it copies a chosen instance's just-written palette
// range next to the CPU-expected palette so the CPU can diff them race-free.
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
    pad0: u32,
    pad1: u32,
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

// `types.rs` flag constants, mirrored.
const PARENT_NONE: u32 = 0xffffffffu;
const REST_FLAG_VOLUME: u32 = 1u;
const REST_FLAG_PELVIS: u32 = 2u;
const REST_FLAG_OVERRIDE: u32 = 4u;
const POSE_FLAG_ROT: u32 = 1u;
const POSE_FLAG_POS: u32 = 2u;

// `types.rs` MAX_GPU_JOINTS: the pass-C private array size; staging rejects a
// bigger skeleton.
const MAX_GPU_JOINTS: u32 = 256u;

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
