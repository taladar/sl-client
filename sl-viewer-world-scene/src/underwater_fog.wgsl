// Underwater fog post-process: a port of the Second Life / Firestorm water fog
// (`class1/environment/waterFogF.glsl` `getWaterFogViewNoClip` /
// `applyWaterFogViewLinear`), applied as a fullscreen pass so it fogs *all*
// underwater geometry (terrain, objects, avatars, the water underside) uniformly
// — not just one material.
//
// The scene colour and the depth buffer are the inputs. Each pixel's depth is
// reconstructed into a world position, the reference's per-fragment water-plane
// clip is applied (a fragment above the surface passes through, so the waterline
// splits cleanly), and everything below runs the reference's
// `getWaterFogViewNoClip` transmittance / in-scatter, re-derived for a horizontal
// plane (Bevy +Y up).
//
// It is compiled twice, once per eye state (`WATER_HAZE_ABOVE`), because the two
// run at different points in the frame — see the branch in `fragment` below. Above
// water this is not a tint over an already-finished sea: it runs *before* the water
// surface, and what it leaves behind is what the surface refracts, so it is where
// the colour of deep water comes from.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

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

// The reference `srgb_to_linear` (`class1/environment/srgbF.glsl`), as ported in
// `sky.wgsl` / `clouds.wgsl` / `water.wgsl`. `getWaterFogViewNoClip` decodes the
// authored (sRGB) water fog colour with it before mixing it into a linear frame.
fn srgb_to_linear(cs: vec3<f32>) -> vec3<f32> {
    let low = cs / 12.92;
    let high = pow((cs + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, cs <= vec3<f32>(0.04045));
}

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
    // "Leave this pixel alone" is therefore `vec4(0, 0, 0, 1)`: add nothing, keep all
    // of the destination.
    let unchanged = vec4<f32>(0.0, 0.0, 0.0, 1.0);

    // Which eye state this pipeline is for. The two run at different points in the
    // frame and each does nothing in the other's state, so exactly one fogs any given
    // frame. The reference likewise carries an `above_water` uniform through one
    // shader (`class3/deferred/waterHazeF.glsl`).
    //
    // Above water this pass runs **before** the water surface, because the surface
    // refracts a copy of what this leaves behind — that is where the sea's colour
    // comes from. Submerged it runs **after everything**, because then the fog is not
    // a backdrop but the medium the whole picture is seen through, including the
    // translucent content drawn after the water (the cloud dome above all, which
    // would otherwise hang unfogged in the distance) and the surface itself, whose
    // underside the reference fogs by exactly this distance.
#ifdef WATER_HAZE_ABOVE
    if (fog.camera_pos.y <= fog.water_height) {
        return unchanged;
    }
#else
    if (fog.camera_pos.y > fog.water_height) {
        return unchanged;
    }
#endif

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
    // this does: the position is reconstructed from a depth buffer, and the further
    // the fragment the coarser that reconstruction, while the thing most often sitting
    // *exactly* on the plane is the water surface itself. Without the tolerance its
    // far pixels fall on either side of the test from one to the next and the fog
    // breaks up along the horizon. A fragment this admits is at most a thousandth of
    // its own distance above the surface, where the fog it gets is imperceptible
    // anyway.
    if (world_pos.y > fog.water_height + length(world_pos - fog.camera_pos.xyz) * 1.0e-3) {
        return unchanged;
    }

    // --- getWaterFogViewNoClip, re-derived for a horizontal plane (+Y up). ---
    let eye = fog.camera_pos.xyz;
    let view = normalize(world_pos - eye);
    // es = -dot(view, plane_normal) with the plane normal pointing up.
    let es = -view.y;
    // e0 = eye depth below the surface (0 when the eye is above water).
    let e0 = max(fog.water_height - eye.y, 0.0);

    // The water ray "entry": the eye itself when submerged, else where the view ray
    // crosses the surface plane (so `l` is the thickness of water actually traversed).
    var entry = eye;
    if (eye.y > fog.water_height && abs(view.y) > 1.0e-5) {
        let t = (fog.water_height - eye.y) / view.y;
        entry = eye + view * t;
    }
    let l = max(length(world_pos - entry), 0.1);

    let kd = fog.fog_density;
    let ks = fog.fog_ks;
    let f = 0.98;
    let t1 = -kd * pow(f, ks * e0);
    // Guard the denominator away from zero (the reference divides by `t2` unguarded,
    // but a grazing view can make it vanish and produce a NaN).
    var t2 = kd + ks * es;
    if (abs(t2) < 1.0e-3) {
        t2 = 1.0e-3;
    }
    let t3 = pow(f, t2 * l) - 1.0;
    // The reference clamps this only from above (`min(_, 1.0)`); clamped from below
    // as well because `pow` of a negative base is not a real number, and a negative
    // density can drive the product there even after `getModifiedWaterFogDensity`
    // has rescued the density itself — a NaN pixel rather than a dark one.
    let scatter = pow(clamp(t1 / t2 * t3, 0.0, 1.0), 1.0 / 1.7);
    let transmittance = pow(0.98, l * kd);

    // The in-scatter as colour, the transmittance as alpha: the blender then computes
    // `dst * D + srgb_to_linear(fogColor) * L`, which is
    // `applyWaterFogViewLinearNoClip`.
    return vec4<f32>(srgb_to_linear(fog.fog_color.rgb) * scatter, transmittance);
}
