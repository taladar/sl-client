// Underwater fog post-process: a port of the Second Life / Firestorm water fog
// (`class1/environment/waterFogF.glsl` `getWaterFogViewNoClip` /
// `applyWaterFogViewLinear`), applied as a fullscreen pass so it fogs *all*
// underwater geometry (terrain, objects, avatars, the water underside) uniformly
// — not just one material.
//
// The scene colour and the depth buffer are the inputs; per pixel the depth is
// reconstructed into a world position, and the reference's per-fragment water-plane
// clip is applied: a fragment above the water surface passes through untouched, so
// a camera straddling the surface splits cleanly along the waterline (and an
// underwater fragment seen from above water — the sea floor through the surface —
// still fogs). Everything else runs the reference's `getWaterFogViewNoClip`
// transmittance / in-scatter, re-derived for a horizontal plane (Bevy +Y up).

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
    _pad: f32,
};

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> fog: UnderwaterFog;
@group(0) @binding(3) var depth_texture: texture_depth_multisampled_2d;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let scene = textureSample(screen_texture, screen_sampler, in.uv);

    // Read the (multisampled) depth for this pixel. Reverse-Z: the far plane / empty
    // sky is depth 0.0, which has no geometry to fog.
    let coord = vec2<i32>(in.position.xy);
    let depth = textureLoad(depth_texture, coord, 0);
    if (depth <= 0.0) {
        return scene;
    }

    // Reconstruct the fragment's world position from its NDC (uv -> clip xy, with the
    // y flip between the top-left uv origin and clip space).
    let ndc = vec3<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0, depth);
    let world_h = fog.world_from_clip * vec4<f32>(ndc, 1.0);
    let world_pos = world_h.xyz / world_h.w;

    // getWaterFogView per-fragment clip: a fragment above the water surface is not
    // fogged (so the waterline split and half-submerged objects work).
    if (world_pos.y > fog.water_height) {
        return scene;
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
    let scatter = pow(min(t1 / t2 * t3, 1.0), 1.0 / 1.7);
    let transmittance = pow(0.98, l * kd);

    // applyWaterFogViewLinearNoClip: color = color * D + fogColor * L.
    let fogged = scene.rgb * transmittance + fog.fog_color.rgb * scatter;
    return vec4<f32>(fogged, scene.a);
}
