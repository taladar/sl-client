// The Second Life / Firestorm glow **combine** pass (`glowcombineF.glsl`, the
// reference `LLPipeline::combineGlow`): `frag_color = scene + glow`, the additive
// composite of the blurred glow buffer back over the tone-mapped scene.
//
// The scene's own alpha (the glow mask) is carried through unchanged so a following
// pass could still read it; nothing downstream does, but keeping it costs nothing
// and matches the reference passing the frame straight through.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var scene_texture: texture_2d<f32>;
@group(0) @binding(1) var scene_sampler: sampler;
@group(0) @binding(2) var glow_texture: texture_2d<f32>;
@group(0) @binding(3) var glow_sampler: sampler;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(scene_texture, scene_sampler, in.uv);
    let glow = textureSample(glow_texture, glow_sampler, in.uv);
    return vec4<f32>(scene.rgb + max(glow.rgb, vec3<f32>(0.0)), scene.a);
}
