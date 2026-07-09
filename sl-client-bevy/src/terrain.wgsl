// Terrain texture-splat material: blends a region's four ground ("detail")
// textures by a per-vertex four-component weight (computed on the CPU from the
// ground elevation by the `sl-terrain` crate) and applies a simple directional
// plus ambient light so the ground reads with relief.
//
// This is deliberately not a full PBR material: it binds only the four detail
// textures and a single directional (sun / moon) term. It does, however, read
// the shared view + light bind group (group 0) so it tracks the scene's real
// sun/moon direction (the day cycle) and — for P24 — samples the directional
// light's cascaded shadow maps so the ground receives shadows cast by avatars,
// prims, and terrain relief. Advanced terrain materials (PBR / normal /
// specular) remain a deferred non-goal of the minimum-viable viewer.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings as view_bindings,
    mesh_view_types,
    shadows,
    view_transformations::position_world_to_clip,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var detail0_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var detail0_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var detail1_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var detail1_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var detail2_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var detail2_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var detail3_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var detail3_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) weights: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) weights: vec4<f32>,
    @location(3) world_position: vec4<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
    out.clip_position = position_world_to_clip(world_position.xyz);
    out.world_position = world_position;
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
    out.uv = vertex.uv;
    out.weights = vertex.weights;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Re-normalise the interpolated weights so the blend stays energy-preserving
    // between vertices; fall back to the lowest detail texture if they vanish.
    let clamped = max(in.weights, vec4<f32>(0.0));
    let total = clamped.x + clamped.y + clamped.z + clamped.w;
    let weights = select(vec4<f32>(1.0, 0.0, 0.0, 0.0), clamped / total, total > 0.0001);

    let c0 = textureSample(detail0_texture, detail0_sampler, in.uv);
    let c1 = textureSample(detail1_texture, detail1_sampler, in.uv);
    let c2 = textureSample(detail2_texture, detail2_sampler, in.uv);
    let c3 = textureSample(detail3_texture, detail3_sampler, in.uv);
    let base = c0 * weights.x + c1 * weights.y + c2 * weights.z + c3 * weights.w;

    let normal = normalize(in.world_normal);

    // Direction toward the active sun / moon, taken from the scene's first
    // directional light so the ground tracks the day cycle. Fall back to a fixed
    // overhead-ish direction if (unexpectedly) no directional light is present.
    var sun_dir = normalize(vec3<f32>(0.4, 1.0, 0.3));
    var shadow = 1.0;
    if (view_bindings::lights.n_directional_lights > 0u) {
        let light = &view_bindings::lights.directional_lights[0];
        sun_dir = (*light).direction_to_light;

        // P24: sample the directional light's cascaded shadow maps so the ground
        // receives shadows. `view_z` is the fragment's depth in view space (what
        // selects the cascade); `clip_position.xy` is the fragment coordinate.
        if (((*light).flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u) {
            let view_z = dot(vec4<f32>(
                view_bindings::view.view_from_world[0].z,
                view_bindings::view.view_from_world[1].z,
                view_bindings::view.view_from_world[2].z,
                view_bindings::view.view_from_world[3].z,
            ), in.world_position);
            shadow = shadows::fetch_directional_shadow(
                0u,
                in.world_position,
                normal,
                view_z,
                in.clip_position.xy,
            );
        }
    }

    // A flat ambient fill plus the shadowed direct (sun / moon) term.
    let diffuse = max(dot(normal, sun_dir), 0.0);
    let light = 0.35 + 0.65 * diffuse * shadow;
    return vec4<f32>(base.rgb * light, 1.0);
}
