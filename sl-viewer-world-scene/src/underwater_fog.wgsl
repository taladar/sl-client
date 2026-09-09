// The water-haze pass: the Second Life / Firestorm water fog applied across the
// **opaque** scene, from the depth buffer — a port of
// `class3/deferred/waterHazeF.glsl`, which the reference likewise runs over its
// deferred (opaque) render, before its water pool and before its alpha pools.
//
// The fog arithmetic itself is not here: it is the shared
// `sl_client_bevy::water_fog` module, because the alpha-blended materials apply the
// *same* fog per fragment (a translucent draw writes no depth, so it is nowhere in
// the buffer this pass reads). This shader is only the part that is peculiar to a
// fullscreen pass — turning a depth sample back into a world position, and deciding
// what an *empty* depth stands for.
//
// One pipeline covers both eye states, as the reference's one shader does: which
// side of the surface the eye is on changes only where the view ray enters the
// water, which `water_fog_no_clip` works out per fragment.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import sl_client_bevy::water_fog::{
    WaterFogParams,
    WATER_FOG_NONE,
    water_fog_no_clip,
}

struct UnderwaterFog {
    // World-from-clip matrix, to reconstruct a fragment's world position from its
    // depth (built on the CPU as `inverse(clip_from_view * view_from_world)`).
    world_from_clip: mat4x4<f32>,
    // The camera world position (the reference eye), xyz + padding.
    camera_pos: vec4<f32>,
    // The water fog colour (`waterFogColor`), rgb + padding.
    fog_color: vec4<f32>,
    // The water surface height, in world metres (the region water level).
    water_height: f32,
    // The (eye-state-modified) water fog density (`getModifiedWaterFogDensity`).
    fog_density: f32,
    // The water fog `KS` term (`1 / max(lightDir.z, 0.3)`).
    fog_ks: f32,
    // The camera's far clip distance, in world metres: how far this frame draws
    // anything at all, and so how far a pixel the depth buffer left empty is known
    // to be clear. Carried rather than read out of `world_from_clip`, whose
    // reverse-Z *infinite* perspective has no far plane in it.
    far_plane: f32,
};

@group(0) @binding(0) var<uniform> fog: UnderwaterFog;
@group(0) @binding(1) var depth_texture: texture_depth_multisampled_2d;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // This pass **blends** over the scene rather than reading and rewriting it, which
    // is the reference's own arrangement: its haze is drawn with a `(ONE,
    // SOURCE_ALPHA)` blend, so the shader emits the in-scatter as colour and the
    // transmittance as alpha and the blender does `dst * D + L`. That is not a
    // stylistic choice here — the pass runs *inside* the main pass, where the scene
    // lives in the multisampled attachment and the resolved texture a post-process
    // would read is a frame's worth of stale, so blending into the attachment is the
    // only way to fog what is actually being drawn.
    //
    // "Leave this pixel alone" is therefore `WATER_FOG_NONE`: add nothing, keep all
    // of the destination.

    // Read the (multisampled) depth for this pixel. Reverse-Z: the far plane — empty
    // sky, or the void past a region edge — is depth 0.0.
    let coord = vec2<i32>(in.position.xy);
    let depth = textureLoad(depth_texture, coord, 0);
    // uv -> clip xy, with the y flip between the top-left uv origin and clip space.
    let ndc_xy = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    var world_pos: vec3<f32>;

    // Where this pixel's geometry is, in the world. With no geometry at all — open
    // sky, or the void past a region edge — the reference's haze reads the far depth
    // and fogs it just the same, which is what gives open water its colour where
    // there is no sea floor to fog. Reverse-Z's *infinite* far plane is a point at
    // infinity rather than a distance, so take the one the empty depth actually
    // stands for: the camera's own far clip, the range this frame drew nothing
    // within. A ray that is still *above* the surface at that range is then rejected
    // by the water-plane clip below, so this fogs the sea and not the sky.
    //
    // The distance has to be that far clip and not some other stand-in. It used to be
    // a flat 2048 m (after `waterF.glsl:285`'s `viewVec*2048.0`), which is far shorter
    // than the sea the viewer draws — 17 region cells of it — so every ray shallow
    // enough to meet the surface beyond 2048 m sampled a point still up in the air,
    // failed the clip, and came out unfogged. That drew a hard ring on the open sea at
    // the one distance where the sampled point crossed the surface: fogged sea inside
    // it, raw sky showing through the sea outside it, and a step of a pixel or two
    // between them (`viewer-sea-distance-band-hard-seam`). Measuring to the far clip
    // instead puts that crossing at the edge of what is drawn at all, where the sea
    // ends and the sky begins anyway.
    if (depth <= 0.0) {
        let mid = fog.world_from_clip * vec4<f32>(ndc_xy, 0.5, 1.0);
        let dir = normalize(mid.xyz / mid.w - fog.camera_pos.xyz);
        world_pos = fog.camera_pos.xyz + dir * fog.far_plane;
    } else {
        let world_h = fog.world_from_clip * vec4<f32>(ndc_xy, depth, 1.0);
        world_pos = world_h.xyz / world_h.w;
    }

    // getWaterFogView per-fragment clip: a fragment above the water surface is not
    // fogged, so the waterline splits cleanly — a submerged camera looking up past it
    // (at the shore, or a half-submerged object) sees the part above unfogged.
    //
    // With a tolerance that grows with distance, which the reference does not need and
    // this does — and which is why the clip is the caller's here and not part of the
    // shared module: the position is reconstructed from a depth buffer, and the
    // further the fragment the coarser that reconstruction, while the thing most
    // often sitting *exactly* on the plane is the water surface itself. Without the
    // tolerance its far pixels fall on either side of the test from one to the next
    // and the fog breaks up along the horizon. A fragment this admits is at most a
    // thousandth of its own distance above the surface, where the fog it gets is
    // imperceptible anyway.
    if (world_pos.y > fog.water_height + length(world_pos - fog.camera_pos.xyz) * 1.0e-3) {
        return WATER_FOG_NONE;
    }

    var params: WaterFogParams;
    params.color = fog.fog_color.rgb;
    params.density = fog.fog_density;
    params.ks = fog.fog_ks;
    params.level = fog.water_height;

    // The in-scatter as colour, the transmittance as alpha: the blender then computes
    // `dst * D + srgb_to_linear(fogColor) * L`, which is
    // `applyWaterFogViewLinearNoClip`.
    return water_fog_no_clip(fog.camera_pos.xyz, world_pos, params);
}
