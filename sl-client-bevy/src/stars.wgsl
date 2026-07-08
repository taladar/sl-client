// Star-field material: a port of the Second Life / Firestorm deferred star
// shaders (`class1/deferred/starsV.glsl` + `starsF.glsl`, drawn by
// `LLDrawPoolWLSky::renderStarsDeferred` / `LLVOWLSky::drawStars`). It shades a
// mesh of small camera-facing quads — one per star — textured with the sky's
// bloom texture and twinkling over time.
//
// The reference smashes the star vertices to the far clip plane (`pos.z = pos.w`)
// so they render behind everything; this port instead places the star quads on a
// sphere just inside the (opaque) sky / cloud domes so they depth-test in *front*
// of the sky and are occluded by near scene geometry, and relies on additive
// blending (the reference `BT_ADD_WITH_ALPHA`) to add the star light over the
// dark night sky. The per-star twinkle seed (`screenpos`) is the reference's:
// the model-space vertex position scaled by a sawtooth of time.
//
// This is gated behind the `bevy_pbr` feature: only the windowed viewer needs a
// renderer.

#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
}

// The per-frame star inputs. Two live scalars plus a `vec2` pad, filling a single
// 16-byte std140 slot, matching the Rust `StarParams` (`ShaderType`) exactly.
struct StarParams {
    // Star-field opacity: `star_brightness / 500` (the reference `custom_alpha`),
    // clamped to 1.0.
    custom_alpha: f32,
    // Twinkle animation time: elapsed seconds * 0.5 (the reference `sStarTime` /
    // `WATER_TIME` uniform).
    time: f32,
    // Padding to a 16-byte std140 slot.
    reserved: vec2<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> star: StarParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var diffuse_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var diffuse_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    // The per-star twinkle seed (`starsV.glsl` `screenpos`).
    @location(2) screenpos: vec2<f32>,
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
    // Skybox depth: force the star quads to the reverse-Z far clip plane (ndc z =
    // 0) so the field renders as an infinitely distant backdrop, occluded by real
    // scene geometry at any altitude (as `sky.wgsl` does for the sky dome). This is
    // our analogue of the reference `starsV.glsl` `pos.z = pos.w` far-plane smash —
    // the field is additive and does not write depth, so it composites over the
    // sky, and the (nearer, 2000 m) sun / moon discs still draw in front of it.
    out.clip_position.z = 0.0;

    // `starsV.glsl`: seed the twinkle from the (model-space, pre-transform) vertex
    // position scaled by a sawtooth of time (`mod(time, 1.25)`). The star mesh is
    // centred on the camera and slowly rotated by the model matrix, so the
    // model-space position stays stable and the twinkle does not swim with it.
    let t = star.time % 1.25;
    out.screenpos = vertex.position.xy * vec2<f32>(t, t);
    out.uv = vertex.uv;
    out.color = vertex.color;
    return out;
}

// `starsF.glsl` `twinkle()`: a per-fragment sawtooth of the screen seed.
fn twinkle(screenpos: vec2<f32>) -> f32 {
    let d = fract(screenpos.x + screenpos.y);
    return abs(d);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The reference deferred `starsF.glsl` samples `diffuseMap` twice and `mix`es
    // by `blend_factor` — i.e. the same map for both — so a single sample is
    // faithful (the day-cycle bloom blend is a no-op for stars).
    var col = textureSample(diffuse_texture, diffuse_sampler, in.uv);

    // Tint by the per-star colour (a near-white with a little red / blue variance).
    let rgb = col.rgb * in.color.rgb;

    // `starsF.glsl`: fade the whole field in with `custom_alpha`, amplify, and
    // twinkle. The additive blend makes the black bloom texels invisible, so only
    // the bright star texels light the sky.
    let factor = smoothstep(0.0, 0.9, star.custom_alpha);
    var alpha = col.a * factor * 32.0;
    alpha = alpha * twinkle(in.screenpos);

    return vec4<f32>(rgb, alpha);
}
