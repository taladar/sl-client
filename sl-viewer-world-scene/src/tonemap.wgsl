// The Second Life / Firestorm tone mapper, as a fullscreen post-process: a port of
// the reference's `class1/deferred/tonemapUtilF.glsl` `toneMap` (driven by
// `postDeferredTonemap.glsl`), which is the *only* place the reference converts its
// linear HDR scene into displayable colour.
//
// The reference's transfer, in order: multiply by the exposure, run the chosen tone
// curve (Khronos PBR Neutral or the ACES Hill fit), blend the curve back toward the
// merely-exposed linear colour by `tonemap_mix` (the reference default is 0.7 — the
// curve is deliberately not applied at full strength), and clamp. The sRGB encode
// the reference then does explicitly is left to the hardware here, which writes the
// view target through an sRGB surface.
//
// A *legacy* (classic-mode) sky is exempt from all of that: see `no_post` below.
//
// Everything the viewer draws — the custom sky / terrain / water materials and
// Bevy's `StandardMaterial` prims, meshes and avatars alike — reaches this pass in
// one linear space and leaves it through one transfer, which is what makes a
// reflection probe's captured cubemap (linear, un-tonemapped) reproduce exactly what
// the eye sees of the same surroundings.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct SlTonemap {
    // The reference `RenderExposure`: a scale on the linear scene colour before the
    // tone curve.
    exposure: f32,
    // The reference `RenderTonemapMix`: how far to blend from the exposed linear
    // colour toward the tone-mapped one.
    tonemap_mix: f32,
    // The reference `RenderTonemapType`: 0 = Khronos PBR Neutral, 1 = ACES (Hill).
    // 2 is this port's own addition: no curve at all (exposure and clamp only), an
    // A/B knob for judging what the curve is doing.
    tonemap_type: u32,
    // The reference's `no_post`: non-zero for a legacy / classic sky, which the
    // reference exempts from the tone mapper entirely (`gNoPostTonemapProgram`).
    no_post: u32,
}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> tonemap: SlTonemap;
// The 1×1 dynamic-exposure map (`exposure.wgsl`): the reference's `exposureMap`,
// the scene-luminance-driven scale multiplied onto the static exposure. `1.0`
// (a no-op) for a legacy sky or when the dynamic exposure is disabled.
@group(0) @binding(3) var exposure_texture: texture_2d<f32>;
@group(0) @binding(4) var exposure_sampler: sampler;

// sRGB => XYZ => D65_2_D60 => AP1 => RRT_SAT (the reference's `ACESInputMat`).
// GLSL and WGSL both build a 3×3 from its *columns*, so the reference's rows of
// three carry over one-for-one.
const ACES_INPUT: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(0.59719, 0.07600, 0.02840),
    vec3<f32>(0.35458, 0.90834, 0.13383),
    vec3<f32>(0.04823, 0.01566, 0.83777),
);

// ODT_SAT => XYZ => D60_2_D65 => sRGB (the reference's `ACESOutputMat`).
const ACES_OUTPUT: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.60475, -0.10208, -0.00327),
    vec3<f32>(-0.53108, 1.10813, -0.07276),
    vec3<f32>(-0.07367, -0.00605, 1.07602),
);

// The reference `RRTAndODTFit`.
fn rrt_and_odt_fit(color: vec3<f32>) -> vec3<f32> {
    let a = color * (color + 0.0245786) - 0.000090537;
    let b = color * (0.983729 * color + 0.4329510) + 0.238081;
    return a / b;
}

// The reference `toneMapACES_Hill`.
fn tone_map_aces_hill(color: vec3<f32>) -> vec3<f32> {
    var mapped = ACES_INPUT * color;
    mapped = rrt_and_odt_fit(mapped);
    mapped = ACES_OUTPUT * mapped;
    return clamp(mapped, vec3<f32>(0.0), vec3<f32>(1.0));
}

// The reference `PBRNeutralToneMapping` (the Khronos PBR Neutral curve): compress
// the peak channel toward 1 and desaturate as it goes, so an over-bright colour
// fades to white instead of clipping to a hue.
fn tone_map_khronos_neutral(color: vec3<f32>) -> vec3<f32> {
    let start_compression = 0.8 - 0.04;
    let desaturation = 0.15;

    var mapped = color;
    let x = min(mapped.r, min(mapped.g, mapped.b));
    var offset = 0.04;
    if (x < 0.08) {
        offset = x - 6.25 * x * x;
    }
    mapped = mapped - offset;

    let peak = max(mapped.r, max(mapped.g, mapped.b));
    if (peak < start_compression) {
        return mapped;
    }

    let d = 1.0 - start_compression;
    let new_peak = 1.0 - d * d / (peak + d - start_compression);
    mapped = mapped * (new_peak / peak);

    let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
    return mix(mapped, new_peak * vec3<f32>(1.0), g);
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let source = textureSample(screen_texture, screen_sampler, in.uv);
    // The reference's `NO_POST` path. `LLPipeline::tonemap` binds
    // `gNoPostTonemapProgram` when the active sky's probe ambiance is zero — a
    // legacy / classic sky — and that program's `postDeferredTonemap.glsl` never
    // calls `toneMap`: it writes `clamp(diff.rgb, 0, 1)` and nothing else. So the
    // exemption drops the `RenderExposure` multiply along with the curve and the
    // mix, which is why it is a branch here rather than just a zero `tonemap_mix`
    // (the reference's `getTonemapMix` returns that zero too, but under `NO_POST`
    // nothing reads it).
    if tonemap.no_post != 0u {
        return vec4<f32>(clamp(source.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), source.a);
    }
    // The reference `toneMap`: `final_exposure = RenderExposure * exposureMap`. The
    // 1×1 exposure map holds the scene-luminance-driven dynamic scale (`1.0` for a
    // legacy sky / when disabled), so this multiply is the counterweight that keeps
    // an EEP `sky_hdr_scale` sky from washing out.
    let exp_scale = textureSample(exposure_texture, exposure_sampler, vec2<f32>(0.5, 0.5)).r;
    let final_exposure = tonemap.exposure * exp_scale;
    // The tone curves assume a non-negative linear input; a negative channel (which
    // no pass should produce, but a filtered mip or a fit can undershoot into) would
    // otherwise come back out of `rrt_and_odt_fit` as a NaN.
    let exposed = max(source.rgb, vec3<f32>(0.0)) * final_exposure;

    var mapped = exposed;
    switch tonemap.tonemap_type {
        case 0u: {
            mapped = tone_map_khronos_neutral(exposed);
        }
        case 1u: {
            mapped = tone_map_aces_hill(exposed);
        }
        default: {
            // No curve: the reference's `NO_POST` path, exposure and clamp only.
        }
    }

    let color = clamp(mix(exposed, mapped, tonemap.tonemap_mix), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(color, source.a);
}
