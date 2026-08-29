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
// reference binds as water-shader uniforms, plus the per-frame sun direction and
// a sky-reflection tint. The wave-scroll clock is `globals.time` and the view
// vector comes from the view bind group's `world_position`, so neither dirties
// the uniform block per frame.
//
// Laid out as `vec3` + trailing scalar pairs (and a `vec2` + `vec2` pair) so the
// std140 uniform layout matches the Rust `WaterParams` (`ShaderType`) exactly.
struct WaterParams {
    // The direction toward the sun (or, at night, the moon), Bevy Y-up.
    light_dir: vec3<f32>,
    // The fresnel scale (`fresnelScale`): how strongly grazing angles reflect.
    fresnel_scale: f32,
    // The normal-map (wavelet) scale (`normScale`), X/Y horizontal, Z up.
    normal_scale: vec3<f32>,
    // The fresnel offset (`fresnelOffset`): the base reflectivity looking straight
    // down.
    fresnel_offset: f32,
    // The water fog colour (`waterFogColor`) — the deep-water tint seen looking
    // into the water. Authored in sRGB, as the reference's is: it is sampled
    // through `srgb_to_linear`, never used raw.
    water_fog_color: vec3<f32>,
    // The water fog density (`waterFogDensity`), already through the reference's
    // `getModifiedWaterFogDensity` for the eye's current side of the surface — so
    // it is the *submerged* density while the camera is under water.
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
// The screen-space water-exclusion mask: 1 where water renders, 0 where a
// water-exclusion surface ("invisiprim" successor) punches a hole in the sea. The
// viewer renders the exclusion faces into this target with a camera slaved to the
// main view, so it is sampled by the fragment's screen position (the reference's
// `exclusionTex`, `class3/environment/waterF.glsl`).
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var exclusion_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var exclusion_sampler: sampler;

// The reference `srgb_to_linear` (`class1/environment/srgbF.glsl`), as ported in
// `sky.wgsl` / `clouds.wgsl`. The water fog colour is authored in sRGB and the
// reference decodes it inside `getWaterFogViewNoClip` before mixing it into a
// linear frame.
fn srgb_to_linear(cs: vec3<f32>) -> vec3<f32> {
    let low = cs / 12.92;
    let high = pow((cs + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, cs <= vec3<f32>(0.04045));
}

// The reference's non-transparent-water refraction fallback:
// `fb = applyWaterFogViewLinear(viewVec * 2048.0, vec4(1.0))`
// (`class3/environment/waterF.glsl:285` through `getWaterFogViewNoClip`,
// `class1/environment/waterFogF.glsl:39`) — white pushed 2048 m along the view ray
// and fogged by the water it passes through, which is the colour the surface shows
// where it is not reflecting.
//
// The reference has two branches here: with `TRANSPARENT_WATER` it samples the
// refraction texture instead, and reaches this line only when that is off. We have
// no refraction pass, so this branch is the only one we can reproduce, and the
// surface's alpha stands in for the rest.
//
// Re-derived for a horizontal plane (Bevy +Y up) rather than the reference's
// view-space plane equation. The modelview transform is rigid, so every quantity it
// needs survives the change of frame: `waterPlane.w` is the eye's signed height
// above the surface, `es = -dot(view, planeNormal)` is `-view.y`, and the
// above-water test `dot(pos, n) + w > 0` is "is this point higher than the water".
//
// `water_height` is the fragment's own world height, since the surface being shaded
// *is* the water plane.
fn water_fog_fallback(eye: vec3<f32>, view: vec3<f32>, water_height: f32) -> vec3<f32> {
    // The reference's `pos`: 2048 m along the view ray.
    let far_point = eye + view * 2048.0;
    // `applyWaterFogViewLinear`'s clip: a point above the surface is not fogged, so
    // the colour passes through unchanged — and the colour here is white. This is
    // the ray that leaves the water rather than descending into it: the underside
    // of the surface seen by a submerged eye looking up.
    if (far_point.y > water_height) {
        return vec3<f32>(1.0);
    }

    let es = -view.y;
    // The eye's depth below the surface — zero whenever it is above.
    let e0 = max(water_height - eye.y, 0.0);
    // The reference's `int_v`: where the ray enters the water. That is the eye
    // itself when the eye is already under, and the surface crossing when it is not.
    var entry = eye;
    if (eye.y > water_height && abs(view.y) > 1.0e-5) {
        entry = eye + view * ((water_height - eye.y) / view.y);
    }
    // The thickness of water the ray traverses, the reference's `l = max(depth, 0.1)`.
    let l = max(length(far_point - entry), 0.1);

    let kd = water.water_fog_density;
    // `waterFogKS = 1 / max(lightDir.z, 0.3)` (`llsettingsvo.cpp:1123`, the clamp
    // `LLSettingsVOWater::WATER_FOG_LIGHT_CLAMP`), on the active light — which is
    // exactly what `light_dir` already carries, so it costs no uniform.
    let ks = 1.0 / max(water.light_dir.y, 0.3);
    let f = 0.98;
    let t1 = -kd * pow(f, ks * e0);
    // Two guards the reference does without, for the same reason `underwater_fog.wgsl`
    // has them: it divides by `t2` unguarded, and a grazing ray can drive it to zero;
    // and `pow` of a negative base is not a real number, which a negative density can
    // produce here even after `getModifiedWaterFogDensity` has rescued the density
    // itself. Both would reach the frame as a NaN pixel.
    var t2 = kd + ks * es;
    if (abs(t2) < 1.0e-3) {
        t2 = 1.0e-3;
    }
    let t3 = pow(f, t2 * l) - 1.0;
    let scatter = pow(clamp(t1 / t2 * t3, 0.0, 1.0), 1.0 / 1.7);
    let transmittance = pow(0.98, l * kd);

    // `applyWaterFogViewLinearNoClip(pos, vec4(1.0))`: white * D + fogColor * L.
    return vec3<f32>(transmittance) + srgb_to_linear(water.water_fog_color) * scatter;
}

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
    // Water-exclusion (`LLDrawPoolWaterExclusion`): the exclusion faces are rendered
    // into a screen-space mask by a camera slaved to this view, so it is sampled by
    // the fragment's screen position (framebuffer pixel / viewport size). Where the
    // mask reads 0 an exclusion surface covers the water, so discard the sea there —
    // the reference `if (water_mask < 1) discard`, at a 0.5 threshold so an
    // antialiased mask edge falls on the silhouette rather than eroding the water.
    let viewport = view_bindings::view.viewport;
    let screen_uv = (in.clip_position.xy - viewport.xy) / viewport.zw;
    let water_mask = textureSample(exclusion_texture, exclusion_sampler, screen_uv).r;
    if (water_mask < 0.5) {
        discard;
    }

    // The Second Life horizontal plane (its "xy") is Bevy's "xz"; work in that
    // horizontal 2-space for the wave texcoords.
    let horiz = vec2<f32>(in.world_position.x, in.world_position.z);

    // --- waterV.glsl: sweeping horizontal wave displacement of the texcoord base. ---
    var v = horiz;
    v.x += (cos(v.x * 0.08) + sin(v.y * 0.02)) * 6.0;

    // Three layered wave texcoords, each scrolling with the wave directions and
    // time (`waterV.glsl` bigWave / littleWave.xy / littleWave.zw). The clock is
    // `globals.time` (read GPU-side so a running wave never rewrites the
    // uniform block); it wraps hourly, which re-seeds the scroll phase with a
    // one-frame jump — imperceptible on a repeating wave normal map.
    let wave_time = view_bindings::globals.time;
    let big_wave = v * vec2<f32>(0.04, 0.04) + water.wave1_dir * wave_time * 0.055;
    let little_wave_a = v * vec2<f32>(0.45, 0.9) + water.wave2_dir * wave_time * 0.13;
    let little_wave_b = v * vec2<f32>(0.1, 0.2) + water.wave1_dir * wave_time * 0.1;

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

    // The eye->surface view vector (the reference `view.xyz` = surface - eye),
    // from the view bind group's camera position so a moving camera never
    // rewrites the uniform block.
    let vv = normalize(in.world_position - view_bindings::view.world_position);

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

    // `color = mix(fb, radiance, df2.x)` — the refracted frame buffer blended toward
    // the reflected radiance (here the sky reflection tint) by the reflection amount.
    //
    // `fb` is the reference's non-transparent-water fallback
    // `applyWaterFogViewLinear(viewVec * 2048.0, vec4(1.0))`, so the deep-water
    // colour answers to the region's fog *density* and to the viewing angle, not
    // only to its authored fog colour. The surface being shaded is the water plane,
    // so its own world height is the water height the fog measures against.
    let fb = water_fog_fallback(
        view_bindings::view.world_position,
        vv,
        in.world_position.y,
    );
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
