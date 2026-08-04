// GPU-instanced particle shader (viewer-perf-gpu-particles). One draw of a shared unit
// quad, once per live particle, with the per-particle state supplied as an instance-rate
// vertex buffer (`@location(3..=7)`, matching `ParticleInstance` in particle_render.rs).
//
// The vertex stage expands each particle into a camera-facing billboard (a port of
// `LLVOPartGroup::getGeometry`, including the `FOLLOW_VELOCITY` re-orientation) in world
// space — each particle carries its absolute world position, so the mesh transform is
// never read. The fragment stage samples the source's diffuse texture (`@group(3)`),
// tints it by the per-instance colour, and either returns it directly (unlit — emissive
// / additive / HUD clouds, `PARTICLE_UNLIT`) or runs it through Bevy's PBR lighting
// (non-emissive clouds, matching the reference lighting all but `EMISSIVE` particles).
//
// The pipeline is specialized from Bevy's `MeshPipeline`, so `@group(0)` is the mesh-view
// bind group (the lights / view uniforms `apply_pbr_lighting` needs) and the inherited
// shader defs match its layout — see particle_render.rs::specialize.

#import bevy_pbr::{
    mesh_view_bindings::view,
    pbr_types,
    pbr_functions,
}

// The source's diffuse texture + sampler (@group(3), built per cloud in
// particle_render.rs::prepare_particle_bind_groups).
@group(3) @binding(0) var particle_texture: texture_2d<f32>;
@group(3) @binding(1) var particle_sampler: sampler;

// `part_flags::FOLLOW_VELOCITY` (llpartdata.h `LL_PART_FOLLOW_VELOCITY_MASK`).
const FOLLOW_VELOCITY: u32 = 0x20u;

struct Vertex {
    // The shared unit-quad corner in [-0.5, 0.5] (xy), z = 0.
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    // Per-instance particle state.
    @location(3) i_position: vec3<f32>,
    @location(4) i_scale: vec2<f32>,
    @location(5) i_color: vec4<f32>,
    @location(6) i_velocity: vec3<f32>,
    @location(7) i_flags: u32,
    @location(8) i_glow: f32,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_position: vec4<f32>,
    @location(3) world_normal: vec3<f32>,
    @location(4) glow: f32,
};

// Normalize `v`, or return `fallback` when it is too short to normalize stably (the WGSL
// counterpart of particles.rs::normalize_or).
fn normalize_or(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    if dot(v, v) > 1e-12 {
        return normalize(v);
    }
    return fallback;
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    // The camera-facing basis: at = particle - camera; right ⊥ (at, up); up ⊥ (right, at).
    let at = vertex.i_position - view.world_position;
    let up_world = vec3<f32>(0.0, 1.0, 0.0);
    var right = normalize_or(cross(at, up_world), vec3<f32>(1.0, 0.0, 0.0));
    var up = normalize_or(cross(right, at), vec3<f32>(0.0, 1.0, 0.0));

    // FOLLOW_VELOCITY: streak the sprite along its direction of travel (the reference's
    // re-orientation of the billboard basis onto the projected velocity).
    if (vertex.i_flags & FOLLOW_VELOCITY) != 0u && dot(vertex.i_velocity, vertex.i_velocity) > 0.0 {
        let nv = normalize(vertex.i_velocity);
        let f0 = dot(nv, right);
        let f1 = dot(nv, up);
        let plen = sqrt(f0 * f0 + f1 * f1);
        if plen > 1e-4 {
            let f0n = f0 / plen;
            let f1n = f1 / plen;
            up = normalize_or(right * f0n + up * f1n, up);
            right = normalize_or(right * f1n - up * f0n, right);
        }
    }

    // Expand the unit-quad corner (in [-0.5, 0.5]) by the instance size along the basis.
    let offset = right * (vertex.position.x * vertex.i_scale.x)
        + up * (vertex.position.y * vertex.i_scale.y);
    let world = vertex.i_position + offset;

    var out: VertexOutput;
    out.world_position = vec4<f32>(world, 1.0);
    out.position = view.clip_from_world * vec4<f32>(world, 1.0);
    out.uv = vertex.uv;
    out.color = vertex.i_color;
    out.glow = vertex.i_glow;
    // The billboard faces the camera, so its normal points back toward the eye.
    out.world_normal = normalize_or(-at, vec3<f32>(0.0, 0.0, 1.0));
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(particle_texture, particle_sampler, in.uv);
    let base = tex * in.color;

    // A billboard is a matte (roughness 1, metallic 0) surface tinted by the sprite; build
    // the PbrInput even on the unlit path so the shared fog / post step has the fragment's
    // world position and frag coord.
    var pbr_input = pbr_types::pbr_input_new();
    pbr_input.material.base_color = base;
    pbr_input.material.perceptual_roughness = 1.0;
    pbr_input.material.metallic = 0.0;
    pbr_input.frag_coord = in.position;
    pbr_input.world_position = in.world_position;
    pbr_input.world_normal = normalize(in.world_normal);
    pbr_input.N = normalize(in.world_normal);
    pbr_input.is_orthographic = view.clip_from_view[3].w == 1.0;
    pbr_input.V = pbr_functions::calculate_view(in.world_position, pbr_input.is_orthographic);

    var color: vec4<f32>;
#ifdef PARTICLE_UNLIT
    // Emissive / additive / HUD clouds are drawn at full texture brightness — the
    // reference forces FULLBRIGHT on EMISSIVE particles, and a HUD layer has no light.
    color = base;
#else
    // Non-emissive clouds are lit, matching the reference (`llvopartgroup.cpp:359` only
    // sets FULLBRIGHT for EMISSIVE particles).
    color = pbr_functions::apply_pbr_lighting(pbr_input);
#endif
    // Fog / any in-shader post, exactly as the StandardMaterial and face-material paths do.
    color = pbr_functions::main_pass_post_lighting_processing(pbr_input, color);

#ifdef PARTICLE_ADDITIVE
    // The additive path carries the per-particle glow (`PSYS_PART_*_GLOW`) in alpha:
    // the additive blend adds `color.rgb` (the fire look, which ignores alpha) and
    // accumulates this alpha into the scene alpha — the glow mask (`crate::glow`) — so
    // a glowing particle blooms and a non-glowing one (glow 0) does not.
    color.a = clamp(in.glow, 0.0, 1.0);
#endif
    return color;
}
