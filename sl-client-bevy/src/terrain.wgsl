// Terrain texture-splat material: blends a region's four ground ("detail")
// textures by a per-vertex four-component weight (computed on the CPU from the
// ground elevation by the `sl-terrain` crate) and applies a simple directional
// plus ambient light so the ground reads with relief.
//
// This is deliberately not a full PBR material: it binds only the four detail
// textures and hard-codes a sun direction matching the viewer's directional
// light, avoiding the view/light bind groups so it stays robust across Bevy
// point releases. Advanced terrain materials (PBR / normal / specular) are a
// deferred non-goal of the minimum-viable viewer.

#import bevy_pbr::{
    mesh_functions,
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

    // Direction toward the sun, matching the viewer's directional light
    // (which looks along (-0.4, -1.0, -0.3)).
    let sun_dir = normalize(vec3<f32>(0.4, 1.0, 0.3));
    let normal = normalize(in.world_normal);
    let diffuse = max(dot(normal, sun_dir), 0.0);
    let light = 0.35 + 0.65 * diffuse;
    return vec4<f32>(base.rgb * light, 1.0);
}
