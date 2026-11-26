// The WindLight sky colour, as one importable module (`sl_client_bevy::sky_colour`):
// the atmosphere `skyF.glsl` writes into the reference's G-buffer, and how
// `softenLight` turns that value into the colour on screen.
//
// The sky dome (`sky.wesl`) draws it. The star field (`stars.wesl`) evaluates it
// too, at its own pixel, because the reference adds each star to the sky's
// *encoded* G-buffer value and decodes the sum, and no blend state can read the
// destination to do that. With the sky in hand the star shader can add the exact
// difference the reference's sum makes (`viewer-stars-add-in-encoded-space`).

import sl_client_bevy::srgb::srgb_to_linear;

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
    // 1.0 to apply srgb_to_linear to the sky colour (the default), 0.0 to skip it
    // (the `SL_VIEWER_SKY_LINEARIZE` A/B knob).
    linearize: f32,
    // The "fake HDR" scale (`SKY_HDR_SCALE`): 1.0 for a legacy / classic-mode sky,
    // sqrt(gamma)*2 for an EEP reflection-probe-ambiance sky. Applied after
    // linearisation, matching the reference `softenLight` WL-sky branch.
    sky_hdr_scale: f32,
    // The radius of the sky dome, in metres: what a consumer that evaluates the sky
    // behind it scales its view direction to (`sky_encoded_colour_along`).
    dome_radius: f32,
};

// Rainbow overlay (`skyF.glsl` `rainbow`): a moisture-scaled band sampled from the
// rainbow texture, keyed by the view/sun dot on its anti-solar side.
fn rainbow(
    sky: SkyParams,
    d: f32,
    rainbow_texture: texture_2d<f32>,
    rainbow_sampler: sampler,
) -> vec3<f32> {
    var dd = clamp(-0.575 - d, 0.0, 1.0);
    let interior_coord = max(0.0, dd - 0.25) * 4.2857;
    dd = clamp(dd, 0.0, 0.25) + interior_coord;
    let rad = (sky.droplet_radius - 5.0) / 1024.0;
    // The rainbow and halo textures hold encoded sRGB bytes
    // (`TextureUpload::WORLD_COLOR`); each sample is decoded after filtering.
    let sampled = srgb_to_linear(
        textureSample(rainbow_texture, rainbow_sampler, vec2<f32>(rad + 0.5, dd)).rgb,
    );
    return pow(sampled, vec3<f32>(1.8)) * sky.moisture_level;
}

// 22-degree ice halo overlay (`skyF.glsl` `halo22`): an ice-scaled ring sampled
// from the halo texture.
fn halo22(sky: SkyParams, d: f32, halo_texture: texture_2d<f32>, halo_sampler: sampler) -> vec3<f32> {
    let dd = clamp(d, 0.1, 1.0);
    let v = sqrt(clamp(1.0 - (dd * dd), 0.0, 1.0));
    let sampled = textureSample(halo_texture, halo_sampler, vec2<f32>(0.0, v)).rgb;
    return srgb_to_linear(sampled) * sky.ice_level;
}

// The sky colour `skyF.glsl` writes into the G-buffer for the dome point
// `local_position` (camera-relative, Bevy Y-up, on a dome of the sky's radius):
// the encoded value, clamped to [0, 1] as the reference's 8-bit `deferredScreen`
// attachment clamps it.
//
// Firestorm computes the haze colour per *vertex* on a tessellated dome; here it
// is computed per *fragment* (the same algorithm, evaluated per pixel), so the
// result is smooth without a dense mesh.
//
// Samples both overlay textures, so call it in uniform control flow.
fn sky_encoded_colour(
    sky: SkyParams,
    local_position: vec3<f32>,
    rainbow_texture: texture_2d<f32>,
    rainbow_sampler: sampler,
    halo_texture: texture_2d<f32>,
    halo_sampler: sampler,
) -> vec3<f32> {
    // Camera-relative position, lifted 50 m to avoid a horizon singularity, then
    // clamped in altitude (`skyV.glsl`): above the horizon to `max_y`, below it to
    // a deep floor so the ray length grows toward the horizon.
    var rel_pos = local_position + vec3<f32>(0.0, 50.0, 0.0);
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
    color = color + rainbow(sky, optic_d, rainbow_texture, rainbow_sampler);
    color = color + halo22(sky, optic_d, halo_texture, halo_sampler);
    color = color * 2.0;
    color = clamp(color, vec3<f32>(0.0), vec3<f32>(5.0));

    // Emulate the reference's deferred G-buffer. `skyF` writes the sky colour to
    // `deferredScreen` — an 8-bit `GL_RGBA` (UNORM, [0,1]) attachment
    // (`pipeline.cpp`: `deferredScreen.allocate(.., GL_RGBA, ..)`) — which clamps it
    // to [0,1] BEFORE `softenLight` reads it back and applies `srgb_to_linear`
    // (× `sky_hdr_scale`). Our forward path linearises in-shader with no such
    // buffer, so without this clamp the bright near-sun haze reaches
    // `srgb_to_linear(5) ≈ 42` and dwarfs the (correctly ~1.0) sun disc, which then
    // reads as a flat grey hole. Clamping the sky's sRGB colour here caps it at
    // `1.0 · sky_hdr_scale` — the same ceiling the pure-white disc hits — so the two
    // blend into one bright orb, faithfully, for every legacy / EEP sky.
    return clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
}

// `sky_encoded_colour` along the view direction `direction` (unit length,
// camera-relative), for a consumer drawn over the dome rather than the dome
// itself: the star field and the cloud layer.
fn sky_encoded_colour_along(
    sky: SkyParams,
    direction: vec3<f32>,
    rainbow_texture: texture_2d<f32>,
    rainbow_sampler: sampler,
    halo_texture: texture_2d<f32>,
    halo_sampler: sampler,
) -> vec3<f32> {
    return sky_encoded_colour(
        sky,
        direction * sky.dome_radius,
        rainbow_texture,
        rainbow_sampler,
        halo_texture,
        halo_sampler,
    );
}

// What the reference deferred `softenLight` SKIP_ATMOS branch makes of a WL-sky
// G-buffer value: `srgb_to_linear`, then `× sky_hdr_scale`, before the tone
// mapper.
//
// The sky's colour settings are authored in sRGB/gamma space, so feeding the
// computed haze straight into our linear tone mapper (`tonemap.wgsl`) left the
// whole sky washed out and desaturated; linearising restores the reference's
// contrast and saturation. `sky_hdr_scale` is 1.0 for a legacy sky (a no-op), and
// above 1.0 for an EEP probe-ambiance sky, so bright haze near the sun expands
// into HDR and blows out under the tone mapper.
fn sky_decoded_colour(sky: SkyParams, encoded: vec3<f32>) -> vec3<f32> {
    let linear = select(encoded, srgb_to_linear(encoded), sky.linearize > 0.5);
    return linear * sky.sky_hdr_scale;
}
