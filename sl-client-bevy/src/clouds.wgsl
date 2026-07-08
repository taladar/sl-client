// Cloud-layer material: a port of the Second Life / Firestorm deferred cloud
// shaders (`class1/deferred/cloudsV.glsl` + `cloudsF.glsl`, drawn by
// `LLDrawPoolWLSky::renderSkyCloudsDeferred` over the WL sky dome). It shades a
// large inward-facing dome — the same camera-centred sphere the sky uses — with
// the scrolling, multi-octave cloud noise layer.
//
// Firestorm computes the cloud lighting per *vertex* (`cloudsV`) and samples the
// noise per *fragment* (`cloudsF`); here the whole thing is evaluated per
// *fragment* from the camera-relative dome position (the same algorithm, so the
// result is smooth without a dense mesh), matching how `sky.wgsl` ports the sky
// dome. The cloud texcoords come from the reference dome's planar UV, which here
// is derived from the fragment's view direction.
//
// The `blue_horizon` / `blue_density` / `haze_*` / `density_multiplier` / `max_y`
// / `glow` / `sunlight_color` / `ambient_color` inputs and the sun / moon light
// direction (`lightnorm`) are the sky frame's, so the cloud lighting matches the
// sky. The `cloud_*` inputs and the (accumulated-scroll) `cloud_pos_density1` are
// supplied per frame from the region's EEP sky settings.
//
// This is gated behind the `bevy_pbr` feature: only the windowed viewer needs a
// renderer.

#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
}

// The cloud + atmospheric inputs for one sky frame. Laid out as `vec3` + trailing
// scalar pairs so the std140 uniform layout matches the Rust `CloudParams`
// (`ShaderType`) exactly.
struct CloudParams {
    // Sun (or, at night, moon) direction, Bevy Y-up, clamped like the reference
    // `LLEnvironment::getClampedLightNorm`.
    lightnorm: vec3<f32>,
    // 1.0 when the sun is up, 0.0 when only the moon is (day vs night light).
    sun_up_factor: f32,
    sunlight_color: vec3<f32>,
    haze_horizon: f32,
    moonlight_color: vec3<f32>,
    haze_density: f32,
    ambient_color: vec3<f32>,
    cloud_shadow: f32,
    blue_horizon: vec3<f32>,
    density_multiplier: f32,
    blue_density: vec3<f32>,
    max_y: f32,
    // glow.x = size (spread), glow.z = focus (a negative exponent).
    glow: vec3<f32>,
    sun_moon_glow_factor: f32,
    cloud_color: vec3<f32>,
    // The reference `cloud_scale`; < 0.001 discards the layer.
    cloud_scale: f32,
    // Cloud layer 1 position (X, Y — already offset by the accumulated scroll) and
    // density (Z).
    cloud_pos_density1: vec3<f32>,
    cloud_variance: f32,
    // Cloud layer 2 detail position (X, Y) and density (Z).
    cloud_pos_density2: vec3<f32>,
    // Blend factor between the current and next noise textures during a day-cycle
    // transition. 0.0 until the day cycle (P22.6) drives it.
    blend_factor: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> cloud: CloudParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var noise_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var noise_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var noise_next_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var noise_next_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // The dome vertex position in the entity's local space. The dome entity is
    // kept centred on the camera each frame, so this is the camera-relative offset
    // the cloud model is evaluated along (the reference's `rel_pos`).
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
    // Skybox depth: force the cloud dome to the reverse-Z far clip plane (ndc z =
    // 0) so it renders as an infinitely distant backdrop, occluded by real scene
    // geometry at any altitude (as `sky.wgsl` does for the sky dome). The layer is
    // alpha-blended and does not write depth, so it composites over the sky.
    out.clip_position.z = 0.0;
    out.local_position = vertex.position;
    return out;
}

// Sample the cloud noise, mixing the current and next textures by the blend factor
// (`cloudsF.glsl` `cloudNoise`).
fn cloud_noise(uv: vec2<f32>) -> vec4<f32> {
    let a = textureSample(noise_texture, noise_sampler, uv);
    let b = textureSample(noise_next_texture, noise_next_sampler, uv);
    return mix(a, b, cloud.blend_factor);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    if (cloud.cloud_scale < 0.001) {
        discard;
    }

    // --- cloudsV.glsl: texcoords from the dome's planar UV. ---
    // The reference dome's planar UV is ((-z + 1) / 2, (-x + 1) / 2) of the unit
    // view direction (Y-up), so the cloud texture is projected top-down.
    let dir = normalize(in.local_position);
    var base_uv = vec2<f32>((-dir.z + 1.0) / 2.0, (-dir.x + 1.0) / 2.0);

    // SL-13084: the custom cloud textures are flipped horizontally.
    var uv0 = vec2<f32>(-base_uv.x, base_uv.y);
    uv0 = (uv0 - 0.5) / cloud.cloud_scale + 0.5;

    // The self-shadow layer is offset toward the (horizontal) light direction.
    var uv1v = uv0;
    uv1v.x += cloud.lightnorm.x * 0.0125;
    uv1v.y += cloud.lightnorm.z * 0.0125;

    let uv2v = uv0 * 16.0;
    let uv3v = uv1v * 16.0;

    // --- cloudsV.glsl: altitude projection to the cloud layer. ---
    var rel_pos = in.local_position + vec3<f32>(0.0, 50.0, 0.0);
    var altitude_blend_factor = clamp((rel_pos.y + 512.0) / cloud.max_y, 0.0, 1.0);
    if (rel_pos.y > 0.0) {
        rel_pos = rel_pos * (cloud.max_y / rel_pos.y);
    }
    if (rel_pos.y < 0.0) {
        // SL-11589: clouds do not droop below the horizon.
        altitude_blend_factor = 0.0;
        rel_pos = rel_pos * (-32000.0 / rel_pos.y);
    }

    let rel_pos_norm = normalize(rel_pos);
    let rel_pos_len = length(rel_pos);

    // --- cloudsV.glsl: cloud lighting. ---
    var sunlight = cloud.sunlight_color;
    let light_atten = (cloud.blue_density + vec3<f32>(cloud.haze_density * 0.25))
        * (cloud.density_multiplier * cloud.max_y);

    var combined_haze = abs(cloud.blue_density) + vec3<f32>(abs(cloud.haze_density));
    let blue_weight = cloud.blue_density / combined_haze;
    let haze_weight = vec3<f32>(cloud.haze_density) / combined_haze;

    var off_axis = 1.0 / max(1e-6, max(0.0, rel_pos_norm.y) + cloud.lightnorm.y);
    sunlight = sunlight * exp(-light_atten * off_axis);

    let density_dist = rel_pos_len * cloud.density_multiplier;
    combined_haze = exp(-combined_haze * density_dist);

    var haze_glow = 1.0 - dot(rel_pos_norm, cloud.lightnorm);
    haze_glow = max(haze_glow, 0.001);
    haze_glow = haze_glow * cloud.glow.x;
    haze_glow = pow(haze_glow, cloud.glow.z);
    haze_glow = haze_glow * cloud.sun_moon_glow_factor;
    // For the sun, add the minimum anti-solar illumination; for the moon, none.
    haze_glow = select(0.0, haze_glow + 0.25, cloud.sun_moon_glow_factor >= 1.0);

    // More clouds lift ambient and dim direct sunlight.
    var tmp_ambient = cloud.ambient_color;
    tmp_ambient = tmp_ambient + (vec3<f32>(1.0) - tmp_ambient) * cloud.cloud_shadow * 0.5;
    sunlight = sunlight * (1.0 - cloud.cloud_shadow);

    let additive_below_cloud = cloud.blue_horizon * blue_weight * (sunlight + tmp_ambient)
        + (cloud.haze_horizon * haze_weight) * (sunlight * haze_glow + tmp_ambient);

    // CLOUDS: re-derive the sunlight along the (near-vertical) cloud ray.
    sunlight = cloud.sunlight_color;
    off_axis = 1.0 / max(1e-6, cloud.lightnorm.y * 2.0);
    sunlight = sunlight * exp(-light_atten * off_axis);

    var cloud_color_sun = (sunlight * haze_glow) * cloud.cloud_color;
    var cloud_color_ambient = tmp_ambient * cloud.cloud_color;

    combined_haze = sqrt(combined_haze);
    cloud_color_sun = cloud_color_sun * combined_haze;
    cloud_color_ambient = cloud_color_ambient * combined_haze;
    let haze_below_cloud = additive_below_cloud * (vec3<f32>(1.0) - combined_haze);

    var cloud_density = 2.0 * (cloud.cloud_shadow - 0.25);
    cloud_color_ambient = cloud_color_ambient + haze_below_cloud;

    // --- cloudsF.glsl: multi-octave noise sampling. ---
    var uv1 = uv0;
    var uv2 = uv1v;
    let uv3 = uv2v;
    let uv4 = uv3v;

    let variance = cloud.cloud_variance * (1.0 - cloud.cloud_scale * 0.25);
    let disturbance = vec2<f32>(
        cloud_noise(uv1 / 8.0).x,
        cloud_noise((uv3 + uv1) / 16.0).x,
    ) * variance;
    let disturbance2 = vec2<f32>(
        cloud_noise((uv1 + uv3) / 4.0).x,
        cloud_noise((uv4 + uv2) / 8.0).x,
    ) * variance;

    uv1 += cloud.cloud_pos_density1.xy + (disturbance * 0.2);
    uv2 += cloud.cloud_pos_density1.xy;
    let uv3o = uv3 + cloud.cloud_pos_density2.xy;
    let uv4o = uv4 + cloud.cloud_pos_density2.xy;

    let density_variance = min(
        1.0,
        (disturbance.x * 2.0 + disturbance.y * 2.0 + disturbance2.x + disturbance2.y) * 4.0,
    );
    cloud_density = cloud_density * (1.0 - density_variance * density_variance);

    // Main cloud opacity.
    var alpha1 = (cloud_noise(uv1).x - 0.5)
        + (cloud_noise(uv3o).x - 0.5) * cloud.cloud_pos_density2.z;
    alpha1 = min(max(alpha1 + cloud_density, 0.0) * 10.0 * cloud.cloud_pos_density1.z, 1.0);
    alpha1 = 1.0 - alpha1 * alpha1;
    alpha1 = 1.0 - alpha1 * alpha1;
    alpha1 = alpha1 * altitude_blend_factor;
    alpha1 = clamp(alpha1, 0.0, 1.0);

    // Self-shadow opacity: (1 - alpha2) is the incoming sunlight fraction.
    var alpha2 = cloud_noise(uv2).x - 0.5;
    alpha2 = min(max(alpha2 + cloud_density, 0.0) * 2.5 * cloud.cloud_pos_density1.z, 1.0);
    alpha2 = 1.0 - alpha2;
    alpha2 = 1.0 - alpha2 * alpha2;

    var color = cloud_color_sun * (1.0 - alpha2) + cloud_color_ambient;
    color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    color = color * 2.0;

    return vec4<f32>(color, alpha1);
}
