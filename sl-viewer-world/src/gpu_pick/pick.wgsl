// The GPU pick pass (roadmap/context/gpu-avatars.md §6): draw each candidate
// mesh into the cursor-cropped Rgba32Uint ID target, writing
// (tag, depth bits, sequence, 0) per fragment over a Depth32Float test —
// nearest-wins exactly like the visible pass.
//
// Two variants share this module: the static one places vertices by the
// per-draw `clip_from_local` (the cropped clip_from_world folded with the
// entity's world transform on the CPU); the SKINNED one reproduces Bevy's
// skinning by blending the palette matrices out of the very
// `SkinUniforms.current_buffer` the visible pass consumes (world-space
// `joint_world * inverse_bind` rows, weights used raw exactly as rendered),
// so a GPU-posed avatar is picked where it is drawn.

/// The per-draw parameters (a dynamic-offset uniform row per candidate).
struct PickUniform {
    /// Static: cropped clip_from_world * world_from_local.
    /// Skinned: the cropped clip_from_world (the palette supplies the rest).
    clip_from_local: mat4x4<f32>,
    /// The encoded pick tag (class:4 | index:28).
    tag: u32,
    /// The submission sequence, for readback correlation.
    sequence: u32,
    /// Skinned only: this entity's first palette row in the skin buffer.
    skin_base: u32,
    /// Padding.
    _pad: u32,
}

@group(0) @binding(0) var<uniform> pick: PickUniform;

#ifdef SKINNED
/// Bevy's live skin palette buffer (`SkinUniforms.current_buffer`).
@group(0) @binding(1) var<storage, read> pick_joints: array<mat4x4<f32>>;
#endif

struct VertexIn {
    @location(0) position: vec3<f32>,
#ifdef SKINNED
    @location(1) joint_indices: vec4<u32>,
    @location(2) joint_weights: vec4<f32>,
#endif
}

struct VertexOut {
    @builtin(position) clip_position: vec4<f32>,
}

@vertex
fn vertex(in: VertexIn) -> VertexOut {
    var out: VertexOut;
#ifdef SKINNED
    // Bevy's skin_model: the weighted palette sum replaces the model matrix.
    let model = in.joint_weights.x * pick_joints[pick.skin_base + in.joint_indices.x]
        + in.joint_weights.y * pick_joints[pick.skin_base + in.joint_indices.y]
        + in.joint_weights.z * pick_joints[pick.skin_base + in.joint_indices.z]
        + in.joint_weights.w * pick_joints[pick.skin_base + in.joint_indices.w];
    out.clip_position = pick.clip_from_local * (model * vec4<f32>(in.position, 1.0));
#else
    out.clip_position = pick.clip_from_local * vec4<f32>(in.position, 1.0);
#endif
    return out;
}

@fragment
fn fragment(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<u32> {
    // The fragment's own depth rides the G channel, so a single readback
    // carries hit identity and hit depth for the surviving (nearest) fragment.
    return vec4<u32>(pick.tag, bitcast<u32>(frag.z), pick.sequence, 0u);
}
