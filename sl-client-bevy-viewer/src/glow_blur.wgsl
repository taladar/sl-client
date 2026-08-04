// The Second Life / Firestorm glow **blur** pass (`glowF.glsl` + `glowV.glsl`), the
// separable Gaussian of `LLPipeline::generateGlow`.
//
// The reference runs this `RenderGlowIterations * 2` times, alternating a
// horizontal and a vertical `glowDelta`, each pass an 8-tap kernel
// `[.25, .5, .8, 1, 1, .8, .5, .25]` at offsets `glowDelta * [-3.5 .. 3.5]`, and
// multiplies the result by `RenderGlowStrength`. `glowDelta = RenderGlowWidth /
// glow_res` along the pass axis. The tap offsets, computed per-vertex in
// `glowV.glsl`, are folded into the fragment here (the fullscreen vertex shader
// gives only the base `uv`).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct GlowBlur {
    // The per-pass step: `(delta, 0)` for a horizontal pass, `(0, delta)` for a
    // vertical one, with `delta = RenderGlowWidth / glow_res`.
    delta: vec2<f32>,
    // The reference `RenderGlowStrength`, applied every pass.
    strength: f32,
    // std140 padding to a 16-byte boundary.
    padding: f32,
}

@group(0) @binding(0) var glow_texture: texture_2d<f32>;
@group(0) @binding(1) var glow_sampler: sampler;
@group(0) @binding(2) var<uniform> blur: GlowBlur;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let d = blur.delta;
    var col = vec4<f32>(0.0);
    col = col + 0.25 * textureSample(glow_texture, glow_sampler, in.uv + d * -3.5);
    col = col + 0.5 * textureSample(glow_texture, glow_sampler, in.uv + d * -2.5);
    col = col + 0.8 * textureSample(glow_texture, glow_sampler, in.uv + d * -1.5);
    col = col + 1.0 * textureSample(glow_texture, glow_sampler, in.uv + d * -0.5);
    col = col + 1.0 * textureSample(glow_texture, glow_sampler, in.uv + d * 0.5);
    col = col + 0.8 * textureSample(glow_texture, glow_sampler, in.uv + d * 1.5);
    col = col + 0.5 * textureSample(glow_texture, glow_sampler, in.uv + d * 2.5);
    col = col + 0.25 * textureSample(glow_texture, glow_sampler, in.uv + d * 3.5);
    // `glowF.glsl`: `frag_color = max(vec4(col.rgb * glowStrength, col.a), 0)`.
    return max(
        vec4<f32>(col.rgb * blur.strength, col.a),
        vec4<f32>(0.0),
    );
}
