// The Second Life / Firestorm **water fog**, as one importable module.
//
// This is the port of `class1/environment/waterFogF.glsl`
// (`getWaterFogViewNoClip` / `applyWaterFogViewLinear`), re-derived for a
// horizontal plane in Bevy's +Y-up world space. It lives here, rather than inside
// the fullscreen haze pass that first needed it, because the reference applies the
// same fog in two different places and so must this viewer:
//
// - once over the **opaque** scene, per pixel of the depth buffer, which is the
//   fullscreen haze pass (`sl_viewer_world_scene::underwater_fog`, the reference's
//   `class3/deferred/waterHazeF.glsl`);
// - and once **per fragment** inside every alpha-blended surface's own shader,
//   because a translucent draw writes no depth and so is not in the buffer the
//   haze pass reads (the reference's `alphaF.glsl` `WATER_FOG` branch, and
//   `underWaterF.glsl` for the surface's own underside).
//
// Both callers must compute the *same* fog for the same point, or the seam where a
// translucent surface meets the opaque scene behind it shows the difference — hence
// one module and not a copy per shader.
//
// The two halves the reference splits, kept split here:
//
// - `water_fog_no_clip` is `getWaterFogViewNoClip`: the transmittance and in-scatter
//   for a fragment already known to be under the surface;
// - the water-plane **clip** is the caller's, because the two callers do not agree
//   on it: a material shader has its fragment's exact world position and tests it
//   exactly, while the haze pass reconstructs the position from a depth buffer and
//   needs a distance-scaled tolerance to keep its far pixels from breaking up along
//   the horizon.

#define_import_path sl_client_bevy::water_fog

// The scene-wide water fog parameters. Every consumer fills these from the same
// region EEP water settings; they are passed as a struct rather than read from a
// shared binding because each consumer carries them in a different place (the haze
// pass in its own uniform, a material in its material uniform).
struct WaterFogParams {
    // The **authored** (sRGB) water fog colour (`waterFogColor`); decoded to linear
    // here, exactly as `getWaterFogViewNoClip` decodes it.
    color: vec3<f32>,
    // The eye-state-modified water fog density (`getModifiedWaterFogDensity`).
    density: f32,
    // The water fog `KS` term, `1 / max(light_dir.z, 0.3)` in the reference's
    // Z-up space — the up component of the direction toward the active light.
    ks: f32,
    // The water surface height, in world metres.
    level: f32,
}

// "Fog this fragment by nothing": add no in-scatter, keep all of the surface.
// The value both `water_fog_no_clip` and a caller's clip return for a fragment the
// water does not reach, and the identity of `apply_water_fog`.
const WATER_FOG_NONE = vec4<f32>(0.0, 0.0, 0.0, 1.0);

// The reference `srgb_to_linear` (`class1/environment/srgbF.glsl`), as ported in
// `sky.wgsl` / `clouds.wgsl` / `water.wgsl`: the authored water fog colour is an
// sRGB value that has to be decoded before it is mixed into a linear frame.
fn water_fog_srgb_to_linear(cs: vec3<f32>) -> vec3<f32> {
    let low = cs / 12.92;
    let high = pow((cs + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, cs <= vec3<f32>(0.04045));
}

// `getModifiedWaterFogDensity`'s eye test, as the reference asks it: of the **eye**,
// not of the fragment (`llsettingsvo.cpp:1128`, `eyedepth = camera.z - water_height;
// underwater = eyedepth <= 0`) — the same test
// `sl_viewer_world_scene::transparency::eye_submerged` makes on the CPU, so a
// material and the sort never disagree about where the camera is.
fn water_fog_eye_submerged(eye: vec3<f32>, level: f32) -> bool {
    return eye.y <= level;
}

// Pick the density for the eye's state. A material shader carries **both**
// densities — the water frame's own, and the one
// `getModifiedWaterFogDensity` raises it to while the eye is submerged — rather
// than the resolved one, so that a camera crossing the waterline changes nothing
// about the material and no material has to be rewritten (and re-prepared) as the
// eye bobs through the surface.
fn water_fog_density(eye: vec3<f32>, level: f32, above: f32, submerged: f32) -> f32 {
    return select(above, submerged, water_fog_eye_submerged(eye, level));
}

// The reference's `waterFogKS = 1 / max(lightDir.z, 0.3)` for a shader that has the
// scene's directional light rather than a CPU-filled uniform: `up` is the up
// component of the direction **toward** the light, which is the active heavenly
// body (the viewer aims one directional light at whichever of the sun and moon is
// up), so this is the same quantity the CPU computes from the sky settings' sun /
// moon rotation. The clamp is the reference's own, and it makes a light below the
// horizon give the same `KS` as no light at all.
fn water_fog_ks(up: f32) -> f32 {
    return 1.0 / max(up, 0.3);
}

// `getWaterFogViewNoClip`: the water fog between `eye` and `world_pos`, as the
// in-scatter (`rgb`, linear) and the transmittance (`a`) — the two halves of the
// reference's `(ONE, SOURCE_ALPHA)` blend, `dst * transmittance + in-scatter`.
//
// Re-derived for a horizontal plane (+Y up) from the reference's arbitrary water
// plane: `es` is the view ray's downwardness, and `e0` the eye's depth below the
// surface (zero when the eye is above it). The ray's **entry** into the water is
// the eye itself when submerged, and where the view ray crosses the surface plane
// when not — so `l`, the length the fog integrates over, is the thickness of water
// actually traversed and not the whole distance to the fragment.
fn water_fog_no_clip(eye: vec3<f32>, world_pos: vec3<f32>, p: WaterFogParams) -> vec4<f32> {
    let view = normalize(world_pos - eye);
    // es = -dot(view, plane_normal), with the plane normal pointing up.
    let es = -view.y;
    // e0 = the eye's depth below the surface (0 when the eye is above water).
    let e0 = max(p.level - eye.y, 0.0);

    var entry = eye;
    if (eye.y > p.level && abs(view.y) > 1.0e-5) {
        let t = (p.level - eye.y) / view.y;
        entry = eye + view * t;
    }
    let l = max(length(world_pos - entry), 0.1);

    let kd = p.density;
    let ks = p.ks;
    let f = 0.98;
    let t1 = -kd * pow(f, ks * e0);
    // Guard the denominator away from zero (the reference divides by `t2`
    // unguarded, but a grazing view can make it vanish and produce a NaN).
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
    let transmittance = pow(f, l * kd);

    return vec4<f32>(water_fog_srgb_to_linear(p.color) * scatter, transmittance);
}

// `applyWaterFogViewLinearNoClip`: put a surface's own linear radiance through the
// fog `water_fog_no_clip` returned. The blender computes exactly this for the
// fullscreen haze pass, which is why that pass returns the value instead of
// applying it; a material shader, which is already holding its colour, applies it
// here.
fn apply_water_fog(color: vec3<f32>, fog: vec4<f32>) -> vec3<f32> {
    return color * fog.a + fog.rgb;
}
