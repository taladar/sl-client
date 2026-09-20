// Stretch the reduced-resolution world frame back over the window
// (`RenderResolutionDivisor`).
//
// The world camera has rendered the 3D scene into an image `1/n` the size of the
// window — through its whole chain, fog, exposure, tone map and glow included —
// so by the time this runs the colour is finished and this pass is a scaled
// copy and nothing else. No transfer, no exposure, no clamp: anything applied
// here would be applied a second time to a frame that already had it.
//
// The alpha is copied with the colour because in this viewer a frame's alpha is
// the **glow mask** (`glow.rs`), not coverage; dropping it would hand the
// overlays that draw next a mask of whatever the window texture last held.
//
// The magnification filter is the sampler's (linear, `ClampToEdge`), which is
// what the reference's own blit of `mRT->screen` to the window uses.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var world_texture: texture_2d<f32>;
@group(0) @binding(1) var world_sampler: sampler;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return textureSample(world_texture, world_sampler, in.uv);
}
