// Water-surface material: a port of the Second Life / Firestorm water shaders
// (`class1/environment/waterV.glsl` + `class3/environment/waterF.glsl`, drawn by
// `LLDrawPoolWater`) onto a flat horizontal plane at the region water height.
//
// The reference is a deferred-pipeline shader that reads the screen colour /
// depth buffers for real refraction and reflection probes for reflection; the
// headless viewer has neither, so this evaluates the parts that do not need the
// G-buffer — the scrolling wave normals, the fresnel term, a sky-tinted
// reflection, the water fog (deep-water) tint, and a sun specular highlight —
// exactly the P23.1 scope ("fresnel, reflection tint, scrolling wave normals").
//
// The wave texcoords and normal-map blend follow `waterV.glsl` /
// `generateWaveNormals`; the fresnel follows `calculateFresnelFactors`. The
// surface is horizontal (Bevy +Y up), so the reference's Second Life "xy"
// horizontal plane maps to Bevy "xz", and the tangent-space wave normal's up
// component (its z) maps to Bevy +Y.
//
// This is gated behind the `bevy_pbr` feature: only the windowed viewer needs a
// renderer.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings as view_bindings,
    view_transformations::position_world_to_clip,
}

// Rotate a direction by a quaternion — the reflection-probe view rotation applied
// to an environment-map sample direction (a local copy of the reference
// `bevy_pbr::environment_map::quat_rotate`, inlined to avoid importing that
// `#ifdef`-heavy module).
fn quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    return v + 2.0 * cross(q.xyz, cross(q.xyz, v) + q.w * v);
}

// The water inputs for one frame: the region's EEP `LLSettingsWater` values the
// reference binds as water-shader uniforms, plus the per-frame sun direction, the
// camera position (for the view vector — the terrain material's convention of
// avoiding the view bind group), a sky-reflection tint, and the wave-scroll time.
//
// Laid out as `vec3` + trailing scalar pairs (and a `vec2` + `vec2` pair) so the
// std140 uniform layout matches the Rust `WaterParams` (`ShaderType`) exactly.
struct WaterParams {
    // The direction toward the sun (or, at night, the moon), Bevy Y-up.
    light_dir: vec3<f32>,
    // Accumulated seconds, scrolling the wave texcoords (`waterV.glsl` `time`).
    time: f32,
    // The camera world position, for the per-fragment view vector.
    camera_position: vec3<f32>,
    // The fresnel scale (`fresnelScale`): how strongly grazing angles reflect.
    fresnel_scale: f32,
    // The normal-map (wavelet) scale (`normScale`), X/Y horizontal, Z up.
    normal_scale: vec3<f32>,
    // The fresnel offset (`fresnelOffset`): the base reflectivity looking straight
    // down.
    fresnel_offset: f32,
    // The water fog colour (`waterFogColor`) — the deep-water tint seen looking
    // into the water.
    water_fog_color: vec3<f32>,
    // The water fog density (`waterFogDensity`) — deepens the fog tint.
    water_fog_density: f32,
    // The sky's sunlight colour, tinting the sun specular highlight.
    sunlight_color: vec3<f32>,
    // The reflection blur multiplier (`blurMultiplier`) — the surface roughness,
    // which broadens (blurs) the specular highlight.
    blur_multiplier: f32,
    // The sky-reflection tint (the atmosphere colour the surface mirrors at
    // grazing angles), supplied per frame from the sky settings.
    reflection_color: vec3<f32>,
    // The A/B normal-map blend factor during a day-cycle transition. 0.0 until the
    // day cycle drives it (like the cloud / disc materials).
    blend_factor: f32,
    // Wave-layer 1 scroll direction (`waveDir1`).
    wave1_dir: vec2<f32>,
    // Wave-layer 2 scroll direction (`waveDir2`).
    wave2_dir: vec2<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> water: WaterParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var normal_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var normal_next_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var normal_next_sampler: sampler;

struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // The fragment's world position, from which the wave texcoords, view vector,
    // and fresnel are all derived per fragment.
    @location(0) world_position: vec3<f32>,
};

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );
    out.world_position = world_position.xyz;
    // Unlike the sky / cloud / star domes (whose depth is forced to the far clip
    // plane), the water plane keeps its real world depth so it depth-tests against
    // the terrain and objects — shallow water shows the ground beneath it and the
    // shoreline occludes the sea beyond.
    out.clip_position = position_world_to_clip(world_position.xyz);
    return out;
}

// Sample the wave normal map (tangent-space, encoded 0..1), mixing the current
// and next normal maps by the day-cycle blend factor (`waterF.glsl` `BlendNormal`),
// and decode to a signed tangent-space normal.
fn wave_normal(uv: vec2<f32>) -> vec3<f32> {
    let a = textureSample(normal_texture, normal_sampler, uv).xyz * 2.0 - 1.0;
    let b = textureSample(normal_next_texture, normal_next_sampler, uv).xyz * 2.0 - 1.0;
    return mix(a, b, water.blend_factor);
}

// Map a tangent-space wave normal (x/y horizontal, z the surface up) onto the
// horizontal water plane in Bevy world space: tangent x -> Bevy X, tangent y ->
// Bevy Z, tangent z(up) -> Bevy Y.
fn tangent_to_world(t: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(t.x, t.z, t.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The Second Life horizontal plane (its "xy") is Bevy's "xz"; work in that
    // horizontal 2-space for the wave texcoords.
    let horiz = vec2<f32>(in.world_position.x, in.world_position.z);

    // --- waterV.glsl: sweeping horizontal wave displacement of the texcoord base. ---
    var v = horiz;
    v.x += (cos(v.x * 0.08) + sin(v.y * 0.02)) * 6.0;

    // Three layered wave texcoords, each scrolling with the wave directions and
    // time (`waterV.glsl` bigWave / littleWave.xy / littleWave.zw).
    let big_wave = v * vec2<f32>(0.04, 0.04) + water.wave1_dir * water.time * 0.055;
    let little_wave_a = v * vec2<f32>(0.45, 0.9) + water.wave2_dir * water.time * 0.13;
    let little_wave_b = v * vec2<f32>(0.1, 0.2) + water.wave1_dir * water.time * 0.1;

    // --- waterF.glsl generateWaveNormals + wavef. ---
    // The three tangent-space wave normals (z is the surface up), mapped to world
    // space (tangent x -> Bevy X, tangent y -> Bevy Z, tangent z(up) -> Bevy Y).
    let wave1 = tangent_to_world(wave_normal(big_wave));
    let wave2 = tangent_to_world(wave_normal(little_wave_a));
    let wave3 = tangent_to_world(wave_normal(little_wave_b));
    let wavef = tangent_to_world((wave_normal(big_wave) + wave_normal(little_wave_a) * 0.4
        + wave_normal(little_wave_b) * 0.6) * 0.5);

    // The perturbed surface normal: apply the wavelet (`normScale`) scale to the
    // horizontal components and boost the up component (`waterF.glsl` wave_ibl:
    // `wavef * normScale`, `.z *= 2`), so the surface stays mostly flat with gentle
    // ripples rather than a vertical wall.
    let normal = normalize(vec3<f32>(
        wavef.x * water.normal_scale.x,
        wavef.y * water.normal_scale.z * 2.0,
        wavef.z * water.normal_scale.y,
    ));

    // The eye->surface view vector (the reference `view.xyz` = surface - eye).
    let vv = normalize(in.world_position - water.camera_position);

    // --- waterF.glsl calculateFresnelFactors. ---
    // `df3` is three squared fresnel terms (from three wave normals) summed into the
    // reflection amount `df2.x`; `df2.y` scales the reflected radiance. The
    // reference dots the eye->surface vector with each wave normal; a plain dot makes
    // the underside (an underwater camera looking up) collapse to a pure grazing sky
    // reflection, so the dot is taken as `-abs(...)` — the same into-the-surface
    // incidence for *both* faces, keeping the reference scale/offset/square shape
    // while shading the surface as water from above and below alike.
    var df3 = max(
        vec3<f32>(0.0),
        vec3<f32>(
            -abs(dot(vv, wave1)),
            -abs(dot(vv, (wave2 + wave3) * 0.5)),
            -abs(dot(vv, wave3)),
        ) * water.fresnel_scale + water.fresnel_offset,
    );
    df3 = df3 * df3;
    let reflect_amount = min(1.0, df3.x + df3.y + df3.z);
    let radiance_scale = max(0.0, -abs(dot(vv, wavef)) * water.fresnel_scale + water.fresnel_offset);

    // `color = mix(fb, radiance, df2.x)` — the refracted frame buffer (`fb`, here the
    // water-fog colour, which is exactly the reference's non-transparent-water
    // fallback `applyWaterFogViewLinear(_, white)`) blended toward the reflected
    // radiance (here the sky reflection tint) by the reflection amount.
    let fb = water.water_fog_color;
    // The reflected environment: sample the reflection-probe specular map in the
    // mirror direction (P33) so the water reflects the real surroundings rather than
    // a flat sky tint, falling back to that tint when no probe is bound.
    var reflection = water.reflection_color;
#ifdef ENVIRONMENT_MAP
    if (view_bindings::light_probes.view_cubemap_index >= 0) {
        var refl_dir = reflect(vv, normal);
        refl_dir = quat_rotate(view_bindings::light_probes.view_rotation, refl_dir);
        // Cube maps are left-handed, so negate z (matching the reference sampler).
        refl_dir.z = -refl_dir.z;
        // A blurrier mip for rougher (windier) water.
        let level = clamp(water.blur_multiplier, 0.0, 1.0)
            * f32(view_bindings::light_probes.smallest_specular_mip_level_for_view);
#ifdef MULTIPLE_LIGHT_PROBES_IN_ARRAY
        let cube = u32(view_bindings::light_probes.view_cubemap_index);
        reflection = textureSampleLevel(
            view_bindings::specular_environment_maps[cube],
            view_bindings::environment_map_sampler,
            refl_dir,
            level,
        ).rgb;
#else
        reflection = textureSampleLevel(
            view_bindings::specular_environment_map,
            view_bindings::environment_map_sampler,
            refl_dir,
            level,
        ).rgb;
#endif
        // Scale by the probe intensity *and* the view exposure (as the terrain
        // ambient and the PBR objects do): the viewer calibrates the intensity to
        // `gain / exposure` (P33.3), so the product is the gain, and at the calibrated
        // gain of 1 the water gives back exactly the radiance its surroundings have —
        // a reflection, not a re-lit approximation of one.
        reflection = reflection
            * view_bindings::light_probes.intensity_for_view
            * view_bindings::view.exposure;
    }
#endif
    let radiance = reflection * radiance_scale;
    var color = mix(fb, radiance, reflect_amount);

    // --- The `punctual` sun specular (a Blinn-Phong stand-in for the reference's
    // `pbrPunctual`, whose roughness is `blurMultiplier`). Rougher water gives a
    // broader, dimmer highlight. ---
    let half_vec = normalize(-vv + water.light_dir);
    let spec_angle = max(dot(normal, half_vec), 0.0);
    let shininess = mix(400.0, 20.0, clamp(water.blur_multiplier * 2.0, 0.0, 1.0));
    let specular = pow(spec_angle, shininess) * max(water.light_dir.y, 0.0);
    color += water.sunlight_color * specular;

    // More opaque (reflective) toward grazing and more transparent looking straight
    // down, so shallow water reveals the ground beneath it (approximating the
    // reference's transparent-water refraction, which needs a screen buffer the
    // headless viewer lacks). Alpha-blended, composited over the terrain / sea floor.
    let alpha = clamp(0.6 + reflect_amount * 0.4, 0.0, 1.0);
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), alpha);
}
