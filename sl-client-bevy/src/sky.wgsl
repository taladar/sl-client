// Atmospheric sky-dome material: a faithful port of the Second Life / Firestorm
// deferred sky shaders (`class1/deferred/skyV.glsl` + `skyF.glsl`) — the legacy
// two-colour exponential atmosphere (blue / haze scattering with an anti-solar
// glow), plus the rainbow / halo overlays. It shades a large inward-facing dome
// centred on the camera.
//
// Firestorm computes the haze colour per *vertex* on a tessellated dome; here it
// is computed per *fragment* from the camera-relative dome position (the same
// algorithm, evaluated per pixel), so the result is smooth without a dense mesh.
// The `blue_horizon` / `blue_density` / `haze_*` / `density_multiplier` /
// `max_y` / `glow` / `sunlight_color` / `ambient_color` inputs and the sun / moon
// light direction (`lightnorm`) are supplied per frame from the region's EEP
// sky settings.
//
// This is gated behind the `bevy_pbr` feature: only the windowed viewer needs a
// renderer.

#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
}

// The atmospheric inputs for one sky frame — the `LLSettingsSky` values Firestorm
// binds as sky-shader uniforms. Laid out as `vec3` + trailing scalar pairs so the
// std140 uniform layout matches the Rust `SkyParams` (`ShaderType`) exactly.
struct SkyParams {
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
    distance_multiplier: f32,
    // glow.x = size (spread), glow.z = focus (a negative exponent).
    glow: vec3<f32>,
    max_y: f32,
    sun_moon_glow_factor: f32,
    moisture_level: f32,
    droplet_radius: f32,
    ice_level: f32,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: SkyParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var rainbow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var rainbow_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var halo_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var halo_sampler: sampler;

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
    out.local_position = vertex.position;
    return out;
}

// Rainbow overlay (`skyF.glsl` `rainbow`): a moisture-scaled band sampled from the
// rainbow texture, keyed by the view/sun dot on its anti-solar side.
fn rainbow(d: f32) -> vec3<f32> {
    var dd = clamp(-0.575 - d, 0.0, 1.0);
    let interior_coord = max(0.0, dd - 0.25) * 4.2857;
    dd = clamp(dd, 0.0, 0.25) + interior_coord;
    let rad = (sky.droplet_radius - 5.0) / 1024.0;
    let sampled = textureSample(rainbow_texture, rainbow_sampler, vec2<f32>(rad + 0.5, dd)).rgb;
    return pow(sampled, vec3<f32>(1.8)) * sky.moisture_level;
}

// 22-degree ice halo overlay (`skyF.glsl` `halo22`): an ice-scaled ring sampled
// from the halo texture.
fn halo22(d: f32) -> vec3<f32> {
    let dd = clamp(d, 0.1, 1.0);
    let v = sqrt(clamp(1.0 - (dd * dd), 0.0, 1.0));
    return textureSample(halo_texture, halo_sampler, vec2<f32>(0.0, v)).rgb * sky.ice_level;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Camera-relative position, lifted 50 m to avoid a horizon singularity, then
    // clamped in altitude (`skyV.glsl`): above the horizon to `max_y`, below it to
    // a deep floor so the ray length grows toward the horizon.
    var rel_pos = in.local_position + vec3<f32>(0.0, 50.0, 0.0);
    if (rel_pos.y > 0.0) {
        rel_pos = rel_pos * (sky.max_y / rel_pos.y);
    }
    if (rel_pos.y < 0.0) {
        rel_pos = rel_pos * (-32000.0 / rel_pos.y);
    }
    let rel_pos_norm = normalize(rel_pos);
    let rel_pos_len = length(rel_pos);
    let rel_pos_lightnorm_dot = dot(rel_pos_norm, sky.lightnorm);

    // Day uses the sun colour; night uses the moon colour dimmed by the reference's
    // magic 0.7 to match legacy tone.
    var sunlight = select(sky.moonlight_color * 0.7, sky.sunlight_color, sky.sun_up_factor >= 0.5);

    // Sunlight atmospheric attenuation (hue + brightness) and the blue/haze weights.
    let light_atten = (sky.blue_density + vec3<f32>(sky.haze_density * 0.25))
        * (sky.density_multiplier * sky.max_y);
    var combined_haze = max(abs(sky.blue_density) + vec3<f32>(abs(sky.haze_density)), vec3<f32>(1e-6));
    let blue_weight = sky.blue_density / combined_haze;
    let haze_weight = vec3<f32>(sky.haze_density) / combined_haze;

    // Attenuate sunlight along the (long) sky ray.
    let off_axis = 1.0 / max(1e-6, max(0.0, rel_pos_norm.y) + sky.lightnorm.y);
    sunlight = sunlight * exp(-light_atten * off_axis);

    // Atmospheric transmittance over the ray.
    let density_dist = rel_pos_len * sky.density_multiplier;
    combined_haze = exp(-combined_haze * density_dist);

    // Anti-solar haze glow: brightest toward the sun, shaped by `glow`.
    var haze_glow = 1.0 - rel_pos_lightnorm_dot;
    haze_glow = max(haze_glow, 0.001);
    haze_glow = haze_glow * sky.glow.x;
    haze_glow = pow(haze_glow, sky.glow.z);
    // Add the minimum anti-solar illumination for the sun; remove glow for the moon.
    haze_glow = select(0.0, sky.sun_moon_glow_factor * (haze_glow + 0.25), sky.sun_moon_glow_factor >= 1.0);

    // Haze colour above the clouds.
    var color = sky.blue_horizon * blue_weight * (sunlight + sky.ambient_color)
        + (sky.haze_horizon * haze_weight) * (sunlight * haze_glow + sky.ambient_color);
    color = color * (vec3<f32>(1.0) - combined_haze);

    // More cloud cover lifts ambient and dims direct sunlight; recompute the
    // below-cloud haze colour and blend toward it at the horizon.
    let ambient = sky.ambient_color
        + max(vec3<f32>(0.0), vec3<f32>(1.0) - sky.ambient_color) * sky.cloud_shadow * 0.5;
    sunlight = sunlight * max(0.0, 1.0 - sky.cloud_shadow);
    let add_below_cloud = sky.blue_horizon * blue_weight * (sunlight + ambient)
        + (sky.haze_horizon * haze_weight) * (sunlight * haze_glow + ambient);
    combined_haze = sqrt(combined_haze);
    color = color + (add_below_cloud - color) * (vec3<f32>(1.0) - sqrt(combined_haze));

    // Rainbow / halo overlays, then the reference's final ×2 and clamp (`skyF`).
    let optic_d = rel_pos_lightnorm_dot;
    color = color + rainbow(optic_d);
    color = color + halo22(optic_d);
    color = color * 2.0;
    color = clamp(color, vec3<f32>(0.0), vec3<f32>(5.0));

    return vec4<f32>(color, 1.0);
}
