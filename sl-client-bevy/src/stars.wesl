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
// sky.
//
// The reference adds a star to the sky's *encoded* G-buffer value and decodes the
// sum afterwards (`softenLight`); a blend state cannot read the destination to do
// that, so the fragment evaluates the sky itself (`sky_colour.wesl`, the dome's
// own maths and inputs) and adds the difference the reference's sum makes. The per-star twinkle seed (`screenpos`) is the reference's:
// the model-space vertex position scaled by a sawtooth of time.
//
// This is gated behind the `bevy_pbr` feature: only the windowed viewer needs a
// renderer.

import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::{globals, view},
    view_transformations::position_world_to_clip,
};
import sl_client_bevy::sky_colour::{SkyParams, sky_decoded_colour, sky_encoded_colour_along};

// The twinkle clock scale: elapsed seconds * 0.5 (the reference `sStarTime` /
// `WATER_TIME` uniform). Applied to `globals.time` here rather than uploaded as
// a uniform, so the running twinkle never rewrites the material. `globals.time`
// wraps hourly (3600 s), and 3600 * 0.5 is an exact multiple of the 1.25 s
// sawtooth period below, so the wrap is seamless.
const STAR_TIME_SCALE: f32 = 0.5;

// The per-frame star inputs. One live scalar plus a `vec3` pad, filling a single
// 16-byte std140 slot, matching the Rust `StarParams` (`ShaderType`) exactly.
struct StarParams {
    // Star-field opacity: `star_brightness / 500` (the reference `custom_alpha`),
    // clamped to 1.0.
    custom_alpha: f32,
    // Padding to a 16-byte std140 slot.
    reserved: vec3<f32>,
};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> star: StarParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var diffuse_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var diffuse_sampler: sampler;
// The sky dome's own inputs, mirrored from its material.
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var<uniform> sky: SkyParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var rainbow_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(5) var rainbow_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(6) var halo_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(7) var halo_sampler: sampler;

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
    // The fragment's world position, for the view direction the sky behind it is
    // evaluated along.
    @location(3) world_position: vec3<f32>,
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
    out.world_position = world_position.xyz;
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
    let t = (globals.time * STAR_TIME_SCALE) % 1.25;
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
    let col = textureSample(diffuse_texture, diffuse_sampler, in.uv);

    // Tint by the per-star colour (a near-white with a little red / blue variance).
    // The bloom texel stays encoded: `starsF.glsl` adds the raw texel to the
    // encoded sky, and the sum is what gets decoded.
    let star_rgb = col.rgb * in.color.rgb;

    // `starsF.glsl`: fade the whole field in with `custom_alpha`, amplify, and
    // twinkle.
    let factor = smoothstep(0.0, 0.9, star.custom_alpha);
    var alpha = col.a * factor * 32.0;
    alpha = alpha * twinkle(in.screenpos);

    // The reference blends with `BT_ADD_WITH_ALPHA` (`SRC_ALPHA, ONE`) into an
    // 8-bit G-buffer target, which clamps the fragment to [0, 1] before blending,
    // so a star adds `rgb * min(alpha, 1)`.
    let coverage = clamp(alpha, 0.0, 1.0);

    // What it adds to is the sky's encoded value, which the 8-bit target clamps
    // again, and `softenLight` decodes the sum. Evaluate the sky the dome drew at
    // this pixel (the view direction, at the dome's radius) and add the difference
    // between the decoded sum and the decoded sky alone. On a mid-blue daytime sky
    // that is several times what adding the decoded texel in linear light gave
    // (`viewer-stars-add-in-encoded-space`), and it carries `sky_hdr_scale` too.
    let direction = normalize(in.world_position - view.world_position);
    let sky_encoded = sky_encoded_colour_along(
        sky,
        direction,
        rainbow_texture,
        rainbow_sampler,
        halo_texture,
        halo_sampler,
    );
    let with_star = clamp(sky_encoded + star_rgb * coverage, vec3<f32>(0.0), vec3<f32>(1.0));
    let added = sky_decoded_colour(sky, with_star) - sky_decoded_colour(sky, sky_encoded);

    // Bevy's `AlphaMode::Add` is the premultiplied pipeline (`ONE,
    // ONE_MINUS_SRC_ALPHA`), so the alpha must be zero or the sky behind the star
    // is scaled by `1 - alpha`: handed the amplified alpha raw (up to 32) it drew
    // black specks over the daytime sky (`viewer-stars-render-black-in-daylight`).
    // The decode is monotonic, so `added` is never negative; the `max` states that
    // a star never takes light away.
    return vec4<f32>(max(added, vec3<f32>(0.0)), 0.0);
}
