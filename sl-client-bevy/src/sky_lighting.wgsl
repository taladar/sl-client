// The Second Life / Firestorm **legacy surface lighting**, as one importable module:
// how the reference's deferred `softenLight` (`class3/deferred/softenLightF.glsl`,
// its non-PBR branch) lights a diffuse surface from the sky's `sunlit` and
// `amblit`.
//
// The reference lights every non-PBR surface — prims, meshes, sculpts, avatars,
// trees, terrain — in that one pass. This viewer lights each in its own material
// shader, so the arithmetic lives here and every one of them imports it, rather
// than each carrying a copy that could drift from the others.
//
// The inputs arrive as two texels of the shared sky-lighting texture
// (`sl_client_bevy::sky_lighting`); the caller loads them (the binding is its own,
// and differs between a bindless and an ordinary material) and hands them to
// `sky_lighting_from_texels`.
//
// None of this is a physically based light. In particular a legacy sky's
// ("classic mode") combine is done in *gamma* space and converted to linear only
// at the end, and a sun at full elevation lights a white surface to well under its
// own brightness — which is the whole of why this is a port and not a Bevy
// directional light with a well-chosen illuminance.

#define_import_path sl_client_bevy::sky_lighting

#import bevy_pbr::mesh_view_bindings as sky_view_bindings

// `SkyLightingMode` (sky_lighting.rs), as the texture stores it.
const SKY_LIGHTING_MODE_EEP: f32 = 1.0;
const SKY_LIGHTING_MODE_CLASSIC: f32 = 2.0;

// The sky lighting a frame resolved to. See `SkyLighting` (sky_lighting.rs).
struct SkyLighting {
    // `calcAtmosphericVarsLinear`'s `sunlit`, already linear for an EEP sky and
    // already scaled by `sky_sunlight_scale`.
    sunlit: vec3<f32>,
    // `calcAtmosphericVars`'s `amblit`, before `ambientLighting` and before the EEP
    // conversion to linear.
    amblit: vec3<f32>,
    // The sky's `reflection_probe_ambiance`.
    probe_ambiance: f32,
    // 0 = no sky resolved (the caller lights itself its old way), 1 = EEP,
    // 2 = classic.
    mode: f32,
}

// Unpack the two texels of the sky-lighting texture: `(sunlit, mode)` and
// `(amblit, probe_ambiance)`.
fn sky_lighting_from_texels(first: vec4<f32>, second: vec4<f32>) -> SkyLighting {
    var lighting: SkyLighting;
    lighting.sunlit = first.rgb;
    lighting.mode = first.a;
    lighting.amblit = second.rgb;
    lighting.probe_ambiance = second.a;
    return lighting;
}

// Whether any sky has been resolved at all. When not, the caller keeps the lighting
// it had before this module (the gallery, a test scene with no environment).
fn sky_lighting_is_resolved(lighting: SkyLighting) -> bool {
    return lighting.mode > SKY_LIGHTING_MODE_EEP - 0.5;
}

// Whether the sky is a legacy one (`classic_mode`).
fn sky_lighting_is_classic(lighting: SkyLighting) -> bool {
    return lighting.mode > SKY_LIGHTING_MODE_CLASSIC - 0.5;
}

// The reference `srgb_to_linear` (`class1/environment/srgbF.glsl`). Unclamped: a
// value above 1 is carried on up the curve.
fn sky_srgb_to_linear(cs: vec3<f32>) -> vec3<f32> {
    let low_range = cs / vec3<f32>(12.92);
    let high_range = pow((cs + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(high_range, low_range, cs <= vec3<f32>(0.04045));
}

// The reference `linear_to_srgb` (`class1/environment/srgbF.glsl`), which clamps to
// `[0, 1]` first — the clamp is part of how a classic surface looks.
fn sky_linear_to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let cl = clamp(linear, vec3<f32>(0.0), vec3<f32>(1.0));
    let low_range = cl * 12.92;
    let high_range = 1.055 * pow(cl, vec3<f32>(0.41666)) - 0.055;
    return select(high_range, low_range, cl < vec3<f32>(0.0031308));
}

// The reference `ambientLighting` (`atmosphericsFuncs.glsl`): "a touch of lighting
// in the opposite direction of the sun light so areas in shadow don't lose all
// detail" — full ambient on a surface edge-on to the light, three quarters on one
// facing it or facing directly away.
fn sky_ambient_lighting(n: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    var ambient = min(abs(dot(n, light_dir)), 1.0);
    ambient = ambient * 0.5;
    ambient = ambient * ambient;
    return 1.0 - ambient;
}

// The sky's two lighting terms at one fragment: `calcAtmosphericVarsLinear`'s
// per-fragment half (`ambientLighting`, and for an EEP sky the linear, grey
// ambient) followed by `softenLight`'s `sunlit *= 1.35` for a classic sky.
struct SurfaceSkyLight {
    sunlit: vec3<f32>,
    amblit: vec3<f32>,
}

fn sky_surface_light(
    lighting: SkyLighting,
    n: vec3<f32>,
    light_dir: vec3<f32>,
) -> SurfaceSkyLight {
    var light: SurfaceSkyLight;
    var amblit = lighting.amblit * sky_ambient_lighting(n, light_dir);
    if sky_lighting_is_classic(lighting) {
        light.sunlit = lighting.sunlit * 1.35;
    } else {
        amblit = sky_srgb_to_linear(amblit);
        amblit = vec3<f32>(dot(amblit, vec3<f32>(0.2126, 0.7152, 0.0722)));
        light.sunlit = lighting.sunlit;
    }
    // `sky_ambient_scale` (`RenderSkyAmbientScale`) is 1.
    light.amblit = amblit;
    return light;
}

// The reflection probe's diffuse irradiance toward `n`, at scene scale, and whether
// the view had a probe to sample at all.
struct SkyProbeIrradiance {
    irradiance: vec3<f32>,
    present: bool,
}

// Rotate a direction by a quaternion — the probe's view rotation applied to a sample
// direction (`bevy_pbr::environment_map::quat_rotate`, copied rather than imported
// from that `#ifdef`-heavy module, as `water.wgsl` does).
fn sky_quat_rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    return v + 2.0 * cross(q.xyz, cross(q.xyz, v) + q.w * v);
}

// Sample the view's reflection probe for the irradiance arriving along `n`, the way
// Bevy's own environment-map light does, scaled by the probe intensity and the view
// exposure — which the viewer calibrates to multiply to the probe gain, so the
// sample comes back at the radiance the probe captured (P33.3).
fn sky_probe_irradiance(n: vec3<f32>) -> SkyProbeIrradiance {
    var out: SkyProbeIrradiance;
    out.irradiance = vec3<f32>(0.0);
    out.present = false;
#ifdef ENVIRONMENT_MAP
    let probes = sky_view_bindings::light_probes;
    if (probes.view_cubemap_index >= 0) {
        var dir = sky_quat_rotate(probes.view_rotation, n);
        // Cube maps are left-handed, so negate z.
        dir.z = -dir.z;
#ifdef MULTIPLE_LIGHT_PROBES_IN_ARRAY
        let probe_sample = textureSampleLevel(
            sky_view_bindings::diffuse_environment_maps[u32(probes.view_cubemap_index)],
            sky_view_bindings::environment_map_sampler,
            dir,
            0.0,
        ).rgb;
#else
        let probe_sample = textureSampleLevel(
            sky_view_bindings::diffuse_environment_map,
            sky_view_bindings::environment_map_sampler,
            dir,
            0.0,
        ).rgb;
#endif
        out.irradiance = probe_sample * probes.intensity_for_view * sky_view_bindings::view.exposure;
        out.present = true;
    }
#endif
    return out;
}

// The diffuse irradiance a legacy surface is lit by
// (`sampleReflectionProbesLegacy`'s `ambenv`): a classic sky's own ambient, and for
// an EEP sky the reflection probes' irradiance faded in over that ambient by the
// sky's probe ambiance (`tapIrradianceMap`'s `mix(amblit, col, min(ambiance, 1))`).
//
// With no probe to sample, the ambient stands alone. `n` is the shading normal.
fn sky_legacy_irradiance(
    lighting: SkyLighting,
    light: SurfaceSkyLight,
    n: vec3<f32>,
) -> vec3<f32> {
    if sky_lighting_is_classic(lighting) {
        return light.amblit;
    }
    let probe = sky_probe_irradiance(n);
    if !probe.present {
        return light.amblit;
    }
    let ambiance = lighting.probe_ambiance;
    return mix(light.amblit, probe.irradiance * ambiance, min(ambiance, 1.0));
}

// The lit diffuse half of `softenLight`'s legacy branch: what the surface's linear
// albedo is multiplied by, and the sun colour its specular highlight is to use.
struct LegacyDiffuse {
    // Multiply the linear albedo by this.
    light: vec3<f32>,
    // `sunlit_linear` as the branch leaves it for the specular term.
    sunlit_linear: vec3<f32>,
}

// `irradiance` from `sky_legacy_irradiance`; `shadow` the sun's shadow term (`scol`,
// 1 = fully lit); `n` the shading normal and `light_dir` the direction toward the
// active body.
fn sky_legacy_diffuse(
    lighting: SkyLighting,
    light: SurfaceSkyLight,
    irradiance: vec3<f32>,
    n: vec3<f32>,
    light_dir: vec3<f32>,
    shadow: f32,
) -> LegacyDiffuse {
    var out: LegacyDiffuse;
    var da = clamp(dot(n, light_dir), 0.0, 1.0);
    if sky_lighting_is_classic(lighting) {
        da = pow(da, 1.2);
        let sun_contrib = vec3<f32>(min(da, shadow));
        out.light = sky_srgb_to_linear(
            irradiance * 0.9 + sky_linear_to_srgb(sun_contrib) * light.sunlit * 0.7,
        );
        out.sunlit_linear = sky_srgb_to_linear(light.sunlit);
    } else {
        out.light = irradiance + min(da, shadow) * light.sunlit;
        out.sunlit_linear = light.sunlit;
    }
    return out;
}

// `softenLight`'s final touch: a classic sky's surfaces are scaled by 1.1, then every
// surface is clamped to the reference's HDR range (`clampHDRRange`, 0 to 11.2, with
// an infinity pinned to 1 and a NaN to 0).
fn sky_legacy_finish(lighting: SkyLighting, color: vec3<f32>) -> vec3<f32> {
    var scaled = color;
    if sky_lighting_is_classic(lighting) {
        scaled = scaled * 1.1;
    }
    // WGSL has no `isinf` / `isnan`: a NaN is the one value unequal to itself, and
    // an infinity the one past the largest finite `f32`.
    let not_nan = select(vec3<f32>(0.0), scaled, scaled == scaled);
    let finite = select(not_nan, vec3<f32>(1.0), not_nan > vec3<f32>(3.4e38));
    return clamp(finite, vec3<f32>(0.0), vec3<f32>(11.2));
}
