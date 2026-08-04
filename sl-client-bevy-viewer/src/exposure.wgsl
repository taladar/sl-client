// The Second Life / Firestorm dynamic-exposure sample pass: a port of
// `generateLuminance` + `exposureF.glsl`, collapsed into one fullscreen pass that
// writes a 1x1 exposure map the tone mapper (`tonemap.wgsl`) then samples.
//
// The reference renders a per-pixel luminance map of the composited linear scene
// (`luminanceF.glsl`, cropped to the central 60% and nudged down 0.1 to favour the
// ground over the bright sky), builds its mip chain, and reads the coarsest mip —
// the frame's average luminance `L` — in `exposureF.glsl`, mapping it through
// `s = mix(exp_max, exp_min, pow(clamp(L / coeff, 0, 1), 2))`. Here the average is
// taken directly by grid-sampling the scene texture over the same central crop
// (a documented approximation of the mip average: the exposure is a slowly-varying
// per-frame scalar, so a fixed grid of samples over the smooth composited frame
// stands in for the true 2x2-averaged mip without a downsample pyramid), then the
// same curve is applied and the scalar written out.
//
// For a legacy sky the exposure range is (1, 1), so `s == 1` for any luminance and
// this pass is inert (matching the reference, where the dynamic exposure never
// touches a classic-mode frame).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct SlExposure {
    // The reference `generateExposure` exposure floor (`exp_min`): the scale a
    // bright frame is pulled down to (`1 / hdr_scale` for an EEP sky, `1` legacy).
    exp_min: f32,
    // The exposure ceiling (`exp_max`): the scale a dark frame is lifted to
    // (`hdr_scale` for an EEP sky, `1` legacy).
    exp_max: f32,
    // The reference `RenderDynamicExposureCoefficient` (`exposureF.glsl`'s `max_L`):
    // the average luminance at which the scale reaches `exp_min`.
    coefficient: f32,
    // 1.0 to run the dynamic exposure, 0.0 to force the scale to `1.0` (the
    // reference `RenderDynamicExposureEnabled = false`, and the A/B disable knob).
    enabled: f32,
}

@group(0) @binding(0) var scene_texture: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var<uniform> exposure: SlExposure;

// The NTSC luma weights the reference `lum()` uses (`exposureF.glsl` /
// `luminanceF.glsl`).
const LUM_WEIGHTS: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);

// The central-crop grid the average is taken over: the reference samples
// `tc = vary_fragcoord * 0.6 + 0.2` (the central 60%) nudged down 0.1 in y to
// favour the ground. `GRID` samples per axis over that window.
const GRID: i32 = 16;
const CROP_SCALE: f32 = 0.6;
const CROP_OFFSET: f32 = 0.2;
const GROUND_NUDGE: f32 = 0.1;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    if (exposure.enabled < 0.5) {
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }

    // Average the scene luminance over the central-crop grid.
    var sum = 0.0;
    let step = CROP_SCALE / f32(GRID);
    for (var y = 0; y < GRID; y = y + 1) {
        for (var x = 0; x < GRID; x = x + 1) {
            var tc = vec2<f32>(
                CROP_OFFSET + (f32(x) + 0.5) * step,
                CROP_OFFSET + (f32(y) + 0.5) * step - GROUND_NUDGE,
            );
            let c = textureSampleLevel(scene_texture, scene_sampler, tc, 0.0).rgb;
            // The scene reaches this pass in linear HDR; a filtered undershoot could
            // dip a channel negative, which `dot` would turn into a spurious
            // negative luminance, so clamp at zero like the reference `max(L, 0)`.
            sum = sum + max(dot(max(c, vec3<f32>(0.0)), LUM_WEIGHTS), 0.0);
        }
    }
    let average = sum / f32(GRID * GRID);

    // `exposureF.glsl`: L = clamp(L, 0, coeff); L /= coeff; L = pow(L, 2).
    let clamped = clamp(average, 0.0, exposure.coefficient);
    var normalised = 0.0;
    if (exposure.coefficient > 0.0) {
        normalised = clamped / exposure.coefficient;
    }
    let shaped = normalised * normalised;
    // `s = mix(exp_max, exp_min, L)`.
    let s = mix(exposure.exp_max, exposure.exp_min, shaped);

    return vec4<f32>(s, s, s, 1.0);
}
