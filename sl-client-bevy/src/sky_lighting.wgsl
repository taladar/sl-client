// The Second Life / Firestorm **surface lighting**, as one importable module: how
// the reference's deferred `softenLight` (`class3/deferred/softenLightF.glsl`)
// lights a surface from the sky's `sunlit` and `amblit` — its legacy branch for a
// diffuse surface, and its PBR branch (below) for a glTF one.
//
// The reference lights every surface — prims, meshes, sculpts, avatars, trees,
// terrain — in that one pass. This viewer lights each in its own material
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

// The diffuse irradiance a surface is lit by (the `ambenv` of both
// `sampleReflectionProbesLegacy` and `sampleReflectionProbes`, which agree on it): a
// classic sky's own ambient, and for an EEP sky the reflection probes' irradiance
// faded in over that ambient by the sky's probe ambiance (`tapIrradianceMap`'s
// `mix(amblit, col, min(ambiance, 1))`).
//
// With no probe to sample, the ambient stands alone. `n` is the shading normal.
fn sky_irradiance(
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

// `irradiance` from `sky_irradiance`; `shadow` the sun's shadow term (`scol`,
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

// ---------------------------------------------------------------------------
// PBR (glTF) surfaces
//
// The reference lights a glTF face in the same deferred pass, through a branch of
// its own: `pbrBaseLight` (`class1/deferred/deferredUtil.glsl`), fed the same
// `sunlit`, `amblit` and probe irradiance as the legacy branch plus the probes'
// radiance. It is a glTF metallic-roughness model for an EEP sky, and for a legacy
// sky an explicit reconstruction of the Blinn-Phong look ("classic mode"), again
// combined in gamma space.
// ---------------------------------------------------------------------------

const SKY_PI: f32 = 3.14159265;

// A glTF surface's diffuse and specular colours (`calcDiffuseSpecular`): a
// dielectric reflects 4 %, a metal reflects its base colour and diffuses nothing.
struct SkyPbrColors {
    diffuse: vec3<f32>,
    specular: vec3<f32>,
}

fn sky_pbr_colors(base_color: vec3<f32>, metallic: f32) -> SkyPbrColors {
    let f0 = vec3<f32>(0.04);
    var colors: SkyPbrColors;
    colors.diffuse = base_color * (vec3<f32>(1.0) - f0) * (1.0 - metallic);
    colors.specular = mix(f0, base_color, metallic);
    return colors;
}

// `pbrPunctual`'s outputs: the clamped `n·l`, and the diffuse and specular BRDF
// terms *without* the `n·l` or the light's colour, which each caller applies its
// own way.
struct SkyPbrPunctual {
    nl: f32,
    diffuse: vec3<f32>,
    specular: vec3<f32>,
}

// The reference `pbrPunctual`: Lambert diffuse over pi, a Schlick Fresnel whose
// grazing reflectance fades out below 4 %, Smith-Schlick geometric occlusion and a
// GGX distribution. `v` is toward the eye, `l` toward the light.
fn sky_pbr_punctual(
    colors: SkyPbrColors,
    perceptual_roughness: f32,
    n: vec3<f32>,
    v: vec3<f32>,
    l: vec3<f32>,
) -> SkyPbrPunctual {
    // "make sure specular highlights from punctual lights don't fall off of
    // polished surfaces"
    let rough = max(perceptual_roughness, 8.0 / 255.0);
    let alpha_roughness = rough * rough;

    let reflectance = max(max(colors.specular.r, colors.specular.g), colors.specular.b);
    let reflectance90 = clamp(reflectance * 25.0, 0.0, 1.0);
    let r0 = colors.specular;
    let r90 = vec3<f32>(reflectance90);

    let h = normalize(l + v);
    let nl = clamp(dot(n, l), 0.001, 1.0);
    let nv = clamp(abs(dot(n, v)), 0.001, 1.0);
    let nh = clamp(dot(n, h), 0.0, 1.0);
    let vh = clamp(dot(v, h), 0.0, 1.0);

    let f = r0 + (r90 - r0) * pow(clamp(1.0 - vh, 0.0, 1.0), 5.0);

    let r2 = alpha_roughness * alpha_roughness;
    let attenuation_l = 2.0 * nl / (nl + sqrt(r2 + (1.0 - r2) * (nl * nl)));
    let attenuation_v = 2.0 * nv / (nv + sqrt(r2 + (1.0 - r2) * (nv * nv)));
    let g = attenuation_l * attenuation_v;

    let d_denominator = (nh * r2 - nh) * nh + 1.0;
    let d = r2 / (SKY_PI * d_denominator * d_denominator);

    var out: SkyPbrPunctual;
    out.nl = nl;
    out.diffuse = (vec3<f32>(1.0) - f) * colors.diffuse / SKY_PI;
    out.specular = f * g * d / (4.0 * nl * nv);
    return out;
}

// The split-sum environment BRDF's scale and bias to F0 at `(n·v, roughness)`.
//
// The reference reads it from `brdfLut`, a table `genbrdflutF.glsl` integrates at
// start-up (GGX importance sampling with Smith-Schlick visibility), indexed so that
// `BRDF(nv, 1 - perceptualRoughness)` looks up the *perceptual* roughness. This is
// Karis's analytic fit of that same integral ("Physically Based Shading on Mobile"),
// the one Bevy's `F_AB` uses when it has no table either — copied rather than
// imported, as `sky_quat_rotate` is.
fn sky_env_brdf(nv: f32, perceptual_roughness: f32) -> vec2<f32> {
    let c0 = vec4<f32>(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let r = perceptual_roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * nv)) * r.x + r.y;
    return vec2<f32>(-1.04, 1.04) * a004 + r.zw;
}

// The view's reflection probe radiance along the reflection of the eye ray about
// `n` (`sampleProbes`), at the mip the roughness selects — the reference's
// `(1 - glossiness) * max_probe_lod` is the same linear map from perceptual
// roughness onto the probe's mip chain. Zero with no probe; at scene scale like
// `sky_probe_irradiance`.
fn sky_probe_radiance(n: vec3<f32>, v: vec3<f32>, perceptual_roughness: f32) -> vec3<f32> {
#ifdef ENVIRONMENT_MAP
    let probes = sky_view_bindings::light_probes;
    if (probes.view_cubemap_index >= 0) {
        var dir = sky_quat_rotate(probes.view_rotation, reflect(-v, n));
        // Cube maps are left-handed, so negate z.
        dir.z = -dir.z;
#ifdef MULTIPLE_LIGHT_PROBES_IN_ARRAY
        let map_index = u32(probes.view_cubemap_index);
        let lod = perceptual_roughness * f32(
            textureNumLevels(sky_view_bindings::specular_environment_maps[map_index]) - 1u
        );
        let probe_sample = textureSampleLevel(
            sky_view_bindings::specular_environment_maps[map_index],
            sky_view_bindings::environment_map_sampler,
            dir,
            lod,
        ).rgb;
#else
        let lod = perceptual_roughness * f32(probes.smallest_specular_mip_level_for_view);
        let probe_sample = textureSampleLevel(
            sky_view_bindings::specular_environment_map,
            sky_view_bindings::environment_map_sampler,
            dir,
            lod,
        ).rgb;
#endif
        return probe_sample * probes.intensity_for_view * sky_view_bindings::view.exposure;
    }
#endif
    return vec3<f32>(0.0);
}

// The reference `pbrBaseLight`, image-based and sun terms together, with the
// emissive term added: what a glTF face is lit by before the local lights and
// `sky_legacy_finish` (which applies the classic sky's 1.1 and the HDR clamp, as
// `softenLight` does to both branches).
//
// `colors` from `sky_pbr_colors`; `v` toward the eye; `shadow` the sun's shadow
// term; `irradiance` from `sky_irradiance`; `ao` the material's occlusion;
// `emissive` linear.
fn sky_pbr_base_light(
    lighting: SkyLighting,
    light: SurfaceSkyLight,
    colors: SkyPbrColors,
    perceptual_roughness: f32,
    n: vec3<f32>,
    v: vec3<f32>,
    light_dir: vec3<f32>,
    shadow: f32,
    irradiance: vec3<f32>,
    ao: vec3<f32>,
    emissive: vec3<f32>,
) -> vec3<f32> {
    // `pbrIbl`: the probe's radiance through the split-sum BRDF, and (for an EEP
    // sky) the irradiance through the diffuse colour, both occluded.
    let nv = clamp(abs(dot(n, v)), 0.001, 1.0);
    let brdf = sky_env_brdf(nv, perceptual_roughness);
    let radiance = sky_probe_radiance(n, v, perceptual_roughness);
    let ibl_specular = radiance * (colors.specular * brdf.x + brdf.y) * ao;

    let punctual = sky_pbr_punctual(colors, perceptual_roughness, n, v, normalize(light_dir));

    var color: vec3<f32>;
    if sky_lighting_is_classic(lighting) {
        // "Reconstruct the diffuse lighting that we do for blinn-phong materials
        // here": the legacy branch's gamma-space sun, times pi to undo the Lambert
        // divide inside `pbrPunctual`, recombined with an ambient that — unlike the
        // EEP one — takes no occlusion.
        let ambient = sky_srgb_to_linear(irradiance * 0.9) * colors.diffuse;
        let da = pow(punctual.nl, 1.2);
        let sun_contrib = sky_srgb_to_linear(
            sky_linear_to_srgb(vec3<f32>(min(da, shadow))) * light.sunlit * 0.7,
        ) * SKY_PI;
        let sun = clamp(
            sun_contrib * ((punctual.diffuse + punctual.specular) * shadow),
            vec3<f32>(0.0),
            vec3<f32>(10.0),
        );
        color = sky_srgb_to_linear(
            sky_linear_to_srgb(ambient) + sky_linear_to_srgb(sun) * 1.1,
        );
    } else {
        color = irradiance * colors.diffuse * ao
            + clamp(
                punctual.nl * (punctual.diffuse + punctual.specular),
                vec3<f32>(0.0),
                vec3<f32>(10.0),
            ) * light.sunlit * 3.0 * shadow;
    }
    return color + ibl_specular + emissive;
}
