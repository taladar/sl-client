// The GPU-avatar keystone spike (see `gpu_avatar_spike.rs`): overwrite one
// skin's palette range inside Bevy's `SkinUniforms` joint-matrix buffer with a
// known, visually unambiguous transform, from a compute pass encoded after
// `prepare_skins`' staging upload and before every draw pass.
//
// The buffer bound at binding 0 IS `SkinUniforms.current_buffer` — the exact
// storage buffer `skinning.wgsl` reads joint matrices from — bound here as
// `storage, read_write` instead of Bevy's read-only view. Whether that binding
// and the write-before-draw ordering hold is the experiment
// (`roadmap/context/gpu-avatars.md` §2.4 / §9.1 risk 1).

/// One palette overwrite job: which slice of the palette buffer to stomp and
/// with what.
struct SpikeParams {
    /// The target mesh entity's world matrix. Writing exactly this for every
    /// joint renders the mesh in bind pose at its own place in the world
    /// (each palette entry is `world_from_joint * inverse_bindpose`; making
    /// them all `base` collapses the pose to the bind pose under `base`).
    base: mat4x4<f32>,
    /// The target skin's first joint matrix, as an index in `mat4x4` elements
    /// into the palette buffer — `SkinUniforms::skin_index(main_entity)`,
    /// re-resolved every frame because the allocator can move skins.
    offset: u32,
    /// How many joint matrices the target skin owns.
    count: u32,
    /// 0 = identity mode (write `base`); 1 = marker mode (write
    /// `base * MARKER`, an unmistakable 45-degree-roll + 1.5x-scale deform).
    mode: u32,
}

/// Bevy's live skin palette buffer (`SkinUniforms.current_buffer`), bound
/// read-write. `skinning.wgsl` binds this same buffer read-only in the vertex
/// stage of every skinned draw this frame.
@group(0) @binding(0) var<storage, read_write> palette: array<mat4x4<f32>>;
/// This frame's overwrite job.
@group(0) @binding(1) var<uniform> params: SpikeParams;

// A 45-degree rotation about +Z composed with a uniform 1.5x scale
// (cos 45 * 1.5 = sin 45 * 1.5 = 1.0606602). Column-major, like every WGSL
// matrix. The Rust test mirrors these literals exactly.
const MARKER: mat4x4<f32> = mat4x4<f32>(
    vec4<f32>(1.0606602, 1.0606602, 0.0, 0.0),
    vec4<f32>(-1.0606602, 1.0606602, 0.0, 0.0),
    vec4<f32>(0.0, 0.0, 1.5, 0.0),
    vec4<f32>(0.0, 0.0, 0.0, 1.0),
);

/// The verdict readback destination (binding 2 is only part of the
/// *readback* pipeline's layout; the main `spike` entry point never
/// references it). `SkinUniforms.current_buffer` is created with
/// `STORAGE | COPY_DST` and **no `COPY_SRC`** — wgpu rejects
/// `copy_buffer_to_buffer` out of it — so lifting the palette range off the
/// GPU has to be a compute copy through the storage binding.
@group(0) @binding(2) var<storage, read_write> readback: array<mat4x4<f32>>;

/// The value the spike writes for every joint of the target skin this frame.
fn spike_value() -> mat4x4<f32> {
    var value = params.base;
    if (params.mode == 1u) {
        value = params.base * MARKER;
    }
    return value;
}

/// One thread per joint matrix of the target skin.
@compute @workgroup_size(64)
fn spike(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.count) {
        return;
    }
    palette[params.offset + gid.x] = spike_value();
}

/// Copy the target's palette range (at the Bevy-allocated offset) into
/// `readback`, plus **this frame's expected value** into the extra slot
/// `readback[count]` — computed here from the same `params` the overwrite
/// used, so the CPU comparison is race-free against a moving avatar (the
/// readback arrives frames later, when `base` may already differ).
/// Dispatched in a second compute pass after `spike`.
@compute @workgroup_size(64)
fn spike_readback(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.count) {
        return;
    }
    readback[gid.x] = palette[params.offset + gid.x];
    if (gid.x == 0u) {
        readback[params.count] = spike_value();
    }
}
