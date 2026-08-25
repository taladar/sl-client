// The Second Life / Firestorm glow **extract** pass (`glowExtractF.glsl`), the
// first stage of `LLPipeline::generateGlow`.
//
// The reference draws this with `BT_ADD_WITH_ALPHA` blending
// (`dst = src.rgb * src.a`) into a cleared low-res glow buffer, and — because the
// main path runs the luminance branch at `minLuminance = 9999` (off) — the extract
// alpha reduces to the scene's own alpha, the per-face **glow mask**. So the glow
// buffer receives `scene_rgb * glow_mask`. This shader folds that blend into the
// output directly (the target is cleared, single draw): `rgb = scene.rgb * mask`,
// carrying the mask in alpha for parity with the reference buffer.
//
// The scene sampled here is the **tone-mapped** composited frame (the reference
// runs glow in `renderFinalize` after `tonemap`), so the glow is built and added in
// display space.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var scene_texture: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let c = textureSample(scene_texture, scene_sampler, in.uv);
    // `max(..., 0)` mirrors the reference's non-negativity guard on the glow buffer.
    let mask = max(c.a, 0.0);
    return vec4<f32>(max(c.rgb, vec3<f32>(0.0)) * mask, mask);
}
