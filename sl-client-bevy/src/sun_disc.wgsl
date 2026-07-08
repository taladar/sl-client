// Sun / moon disc billboard material: a port of the Second Life / Firestorm
// deferred heavenly-body shaders (`class1/deferred/sunDiscV.glsl` + `sunDiscF.glsl`
// and `moonV.glsl` + `moonF.glsl`, drawn by `LLDrawPoolWLSky::renderHeavenlyBodies`).
// It shades a camera-facing quad textured with the sky frame's sun / moon disc.
//
// The reference sun disc simply samples (and, during a day-cycle transition,
// blends) the two disc textures — it does *not* tint by the bound diffuse colour.
// The moon disc multiplies by the sky's moon brightness, discards its
// near-transparent pixels (so stars show through the quad), and fades its alpha
// near the horizon. Both behaviours are selected by `moon_mode`.
//
// This is gated behind the `bevy_pbr` feature: only the windowed viewer needs a
// renderer.

#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
}

// The per-frame disc inputs. Four scalars packed into a single 16-byte std140
// slot, matching the Rust `SunDiscParams` (`ShaderType`) exactly.
struct SunDiscParams {
    // Overall brightness multiplier: the sky's moon brightness for the moon,
    // 1.0 for the sun.
    brightness: f32,
    // Blend factor between the current (`diffuse`) and next (`alt_diffuse`) disc
    // textures during a day-cycle transition. 0.0 until the day cycle (P22.6)
    // drives it, so only `diffuse` is used for now.
    blend_factor: f32,
    // 1.0 to apply the moon's transparent-pixel discard and near-horizon alpha
    // fade, 0.0 for the sun.
    moon_mode: f32,
    // The body's up component (Bevy y) for the moon's near-horizon alpha fade.
    up_component: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> disc: SunDiscParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var alt_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var alt_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
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
    out.uv = vertex.uv;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let a = textureSample(diffuse_texture, diffuse_sampler, in.uv);
    let b = textureSample(alt_texture, alt_sampler, in.uv);
    var c = mix(a, b, disc.blend_factor);

    // `moonF.glsl`: the moon texture carries transparent pixels
    // (<0x55,0x55,0x55,0x00>); discard them so the quad never hides stars.
    if (disc.moon_mode >= 0.5 && c.a <= 2.0 / 255.0) {
        discard;
    }

    var rgb = c.rgb * disc.brightness;
    var alpha = c.a;

    // `moonF.glsl`: restore the pre-EEP alpha fade of the moon near the horizon.
    if (disc.moon_mode >= 0.5 && disc.up_component > 0.0) {
        alpha = alpha * clamp(disc.up_component * disc.up_component * 4.0, 0.0, 1.0);
    }

    return vec4<f32>(rgb, alpha);
}
