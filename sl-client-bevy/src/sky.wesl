// Atmospheric sky-dome material: a faithful port of the Second Life / Firestorm
// deferred sky shaders (`class1/deferred/skyV.glsl` + `skyF.glsl`) — the legacy
// two-colour exponential atmosphere (blue / haze scattering with an anti-solar
// glow), plus the rainbow / halo overlays. It shades a large inward-facing dome
// centred on the camera.
//
// The atmosphere itself lives in `sky_colour.wesl`, shared with the star field.
// The `blue_horizon` / `blue_density` / `haze_*` / `density_multiplier` /
// `max_y` / `glow` / `sunlight_color` / `ambient_color` inputs and the sun / moon
// light direction (`lightnorm`) are supplied per frame from the region's EEP
// sky settings.
//
// This is gated behind the `bevy_pbr` feature: only the windowed viewer needs a
// renderer.

import bevy_pbr::render::{
    mesh_functions,
    view_transformations::position_world_to_clip,
};
import sl_client_bevy::sky_colour::{SkyParams, sky_decoded_colour, sky_encoded_colour};

@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> sky: SkyParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var rainbow_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(2) var rainbow_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(3) var halo_texture: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(4) var halo_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // The dome vertex position in the entity's local space. The dome entity is
    // kept centred on the camera each frame, so this is the camera-relative
    // offset the atmospheric model is evaluated along (the reference's `rel_pos`).
    @location(0) local_position: vec3<f32>,
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
    // Skybox depth: force the dome to the reverse-Z far clip plane (ndc z = 0) so
    // it renders as an infinitely distant backdrop — real scene geometry at ANY
    // altitude (a 2000 m skybox, a tall build) occludes it, and the dome never
    // hides objects beyond its own radius. Bevy's mesh pipeline uses a
    // `GreaterEqual` depth test, so `0 >= 0` still draws the dome over the cleared
    // (far) background while `0 >= any nearer geometry` fails.
    out.clip_position.z = 0.0;
    out.local_position = vertex.position;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The value the reference's `skyF` leaves in its 8-bit G-buffer, then what its
    // `softenLight` makes of it (both in `sky_colour.wesl`, which the star field
    // evaluates too). The reference decodes the *composited* sky once; this
    // forward path decodes each sky element and blends in linear light, except for
    // the stars, which add the reference's encoded-space sum exactly.
    let encoded = sky_encoded_colour(
        sky,
        in.local_position,
        rainbow_texture,
        rainbow_sampler,
        halo_texture,
        halo_sampler,
    );
    let out = sky_decoded_colour(sky, encoded);
    // Alpha carries the SL glow mask (the viewer's `glow` pass): the sky never
    // glows, so it writes 0 — matching the reference, where the WL sky's SKIP_ATMOS
    // pixels get `frag_color.a = 0` and so feed nothing to the glow buffer. The dome
    // is opaque, so this alpha is not a blend factor.
    return vec4<f32>(out, 0.0);
}
