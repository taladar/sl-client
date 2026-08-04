// The Second Life face material shader (the `SlFaceExt` extension of Bevy's
// `StandardMaterial`).
//
// PBR **per-map UV transforms**: Bevy's `pbr_input_from_standard_material` samples
// every map at one shared `uv_transform`, so the PBR normal / metallic-roughness /
// emissive maps are carried in the EXTENSION (not the base material) and sampled
// here, each at its own transformed UV; the base keeps only the base-colour
// texture and the scalar factors, which we multiply the samples by. A plain
// diffuse / avatar / legacy face has `map_flags == 0`, so none of this runs and it
// renders as a bare `StandardMaterial`.
//
// The extension is **bindless** so the whole material stays bindless (cross-material
// draw-call batching) — this shader therefore has both a `BINDLESS` path (data +
// map indices come from the extension's index table, textures from the global
// bindless arrays) and a non-bindless path (plain uniform + per-binding textures),
// mirroring how `bevy_pbr::pbr_bindings` / `pbr_fragment` are written.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types,
    pbr_bindings,
    mesh_view_bindings::view,
    mesh_view_bindings::globals,
    mesh_view_bindings::lights,
    mesh_bindings::mesh,
    forward_io::{VertexOutput, FragmentOutput},
}
#import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_2d}

// Mirrors `SlFaceParams` (face_material.rs): only vec4 / vec2 / u32 / f32, so the
// `encase` std140 layout matches this field-for-field.
struct SlFaceParams {
    uv_normal_mat: vec4<f32>,
    uv_mr_mat: vec4<f32>,
    uv_emissive_mat: vec4<f32>,
    uv_spec_mat: vec4<f32>,
    specular_color: vec4<f32>,
    uv_translations_a: vec4<f32>, // (normal.xy, mr.xy)
    uv_translations_b: vec4<f32>, // (emissive.xy, spec.xy)
    anim_params: vec4<f32>,       // (rate, start, length, start_time)
    anim_static: vec4<f32>,       // fall-back (rotation, offset_s, offset_t, scale_s)
    anim_grid: vec4<f32>,         // (size_x, size_y, scale_t, unused)
    mode: u32,
    map_flags: u32,
    anim_mode: u32,
    glossiness: f32,
    env_intensity: f32,
    // The faithful SL glow mask (glow.rs): the face's glow scalar, written into the
    // fragment's alpha channel for an opaque / mask face so the glow pass blooms it.
    // A negative value is the sentinel "leave alpha untouched" — used for a blend
    // face, whose alpha is its coverage. Set on the CPU where the alpha mode is
    // known (`textures::face_material`).
    glow: f32,
}

// Must match the `MAP_FLAG_*` constants in face_material.rs.
const MAP_FLAG_NORMAL: u32 = 1u;
const MAP_FLAG_MR: u32 = 2u;
const MAP_FLAG_EMISSIVE: u32 = 4u;
const MAP_FLAG_SPEC: u32 = 8u;

// Must match the `SL_FACE_MODE_*` constants in face_material.rs.
const SL_FACE_MODE_LEGACY: u32 = 1u;

// The reference viewer's `RenderSpecularExponent` (settings.xml default): the scale
// the legacy glossiness²·specExp Blinn-Phong exponent is built from — see
// `sl_blinn_phong_specular`.
const SL_SPECULAR_EXPONENT: f32 = 368.0;
const SL_PI: f32 = 3.14159265358979;

// Texture-animation mode bits — must match `texture_anim_mode` (sl-proto).
const ANIM_ON: u32 = 0x01u;
const ANIM_LOOP: u32 = 0x02u;
const ANIM_REVERSE: u32 = 0x04u;
const ANIM_PING_PONG: u32 = 0x08u;
const ANIM_SMOOTH: u32 = 0x10u;
const ANIM_ROTATE: u32 = 0x20u;
const ANIM_SCALE: u32 = 0x40u;

// The hour-wrap period of `globals.time` (`Time::DEFAULT_WRAP_PERIOD`, 3600s), used
// to unwind the shader animation clock across the wrap.
const ANIM_TIME_WRAP: f32 = 3600.0;

#ifdef BINDLESS
// The extension's bindless index table (slots 50..58, in field order): slot 50 is
// the `#[data]` params array index, then the four map texture/sampler indices into
// the global bindless arrays.
struct SlFaceIndices {
    params: u32,
    specular_map: u32,
    specular_map_sampler: u32,
    normal_map: u32,
    normal_map_sampler: u32,
    metallic_roughness_map: u32,
    metallic_roughness_map_sampler: u32,
    emissive_map: u32,
    emissive_map_sampler: u32,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<storage> sl_indices: array<SlFaceIndices>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<storage> sl_data: array<SlFaceParams>;
#else   // BINDLESS
@group(#{MATERIAL_BIND_GROUP}) @binding(50) var<uniform> sl_material: SlFaceParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(51) var sl_spec_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(52) var sl_spec_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(53) var sl_normal_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(54) var sl_normal_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(55) var sl_mr_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(56) var sl_mr_samp: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(57) var sl_emissive_tex: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(58) var sl_emissive_samp: sampler;
#endif  // BINDLESS

// Apply a packed 2x2 matrix (m = col0.xy, col1.xy) + translation to a UV.
fn sl_uv(m: vec4<f32>, t: vec2<f32>, uv: vec2<f32>) -> vec2<f32> {
    return mat2x2<f32>(m.xy, m.zw) * uv + t;
}

// The face texture-placement transform (a port of `texture_uv_transform` /
// `LLFace::xform`): map `uv` about the face centre (0.5, 0.5) by rotation, repeats
// (scale_s/scale_t), then offset. See sl_client_bevy::texture_uv_transform.
fn sl_texture_uv_transform(
    rotation: f32, offset_s: f32, offset_t: f32, scale_s: f32, scale_t: f32, uv: vec2<f32>,
) -> vec2<f32> {
    let s = sin(rotation);
    let c = cos(rotation);
    let ms = scale_s;
    let mt = scale_t;
    // Columns of the linear part: s-column (ms*c, -mt*s), t-column (ms*s, mt*c).
    let m = mat2x2<f32>(ms * c, -mt * s, ms * s, mt * c);
    let t = vec2<f32>(
        offset_s + 0.5 - 0.5 * ms * (c + s),
        offset_t + 0.5 + 0.5 * mt * (s - c),
    );
    return m * uv + t;
}

// GPU texture animation (P28.2): evaluate the running animation at `globals.time`
// and return the animated UV for the mesh `uv`. A faithful port of the Rust
// `animate` + `AnimatedPlacement::uv_transform` (texture_anim.rs), which is itself a
// port of `LLViewerTextureAnim::animateTextures`. Only reached when `ANIM_ON` is set.
fn sl_animated_uv(sl: SlFaceParams, uv: vec2<f32>) -> vec2<f32> {
    let mode = sl.anim_mode;
    let rate = sl.anim_params.x;
    let start = sl.anim_params.y;
    let length = sl.anim_params.z;
    let start_time = sl.anim_params.w;
    let size_x = sl.anim_grid.x;
    let size_y = sl.anim_grid.y;

    // Elapsed since the animation started, unwinding the hourly wrap of globals.time.
    var elapsed = globals.time - start_time;
    if elapsed < 0.0 {
        elapsed = elapsed + ANIM_TIME_WRAP;
    }

    var num_frames = max(size_x * size_y, 1.0);
    if length != 0.0 {
        num_frames = length;
    }

    var full_length = num_frames;
    if (mode & ANIM_PING_PONG) != 0u {
        if (mode & ANIM_SMOOTH) != 0u {
            full_length = 2.0 * num_frames;
        } else if (mode & ANIM_LOOP) != 0u {
            full_length = max(2.0 * num_frames - 2.0, 1.0);
        } else {
            full_length = max(2.0 * num_frames - 1.0, 1.0);
        }
    }

    var fc = elapsed * rate;
    if (mode & ANIM_LOOP) != 0u {
        fc = fc % full_length;
    } else {
        fc = min(fc, full_length - 1.0);
    }
    if (mode & ANIM_SMOOTH) == 0u {
        fc = floor(fc + 0.01);
        fc = min(fc, full_length - 1.0);
    }
    if ((mode & ANIM_PING_PONG) != 0u) && (fc >= num_frames) {
        if (mode & ANIM_SMOOTH) != 0u {
            fc = num_frames - (fc - num_frames);
        } else {
            fc = (num_frames - 1.99) - (fc - num_frames);
        }
    }
    if (mode & ANIM_REVERSE) != 0u {
        if (mode & ANIM_SMOOTH) != 0u {
            fc = num_frames - fc;
        } else {
            fc = (num_frames - 0.99) - fc;
        }
    }
    fc = fc + start;
    if (mode & ANIM_SMOOTH) == 0u {
        fc = round(fc);
    }

    // Resolve the placement, falling back to the face's static values for whatever
    // the animation does not drive.
    var rotation = sl.anim_static.x;
    var offset_s = sl.anim_static.y;
    var offset_t = sl.anim_static.z;
    var scale_s = sl.anim_static.w;
    var scale_t = sl.anim_grid.z;
    if (mode & ANIM_ROTATE) != 0u {
        rotation = fc;
    } else if (mode & ANIM_SCALE) != 0u {
        scale_s = fc;
        scale_t = fc;
    } else if (size_x != 0.0) && (size_y != 0.0) {
        // Flip-book: page a size_x × size_y grid, one cell per frame.
        let ss = 1.0 / size_x;
        let st = 1.0 / size_y;
        let x_frame = fc % size_x;
        let y_frame = trunc(fc / size_x);
        scale_s = ss;
        scale_t = st;
        offset_s = (-0.5 + 0.5 * ss) + x_frame * ss;
        offset_t = (0.5 - 0.5 * st) - y_frame * st;
    } else {
        // No grid: scroll the texture across the face (offset only).
        offset_s = fc;
        offset_t = 0.0;
    }
    return sl_texture_uv_transform(rotation, offset_s, offset_t, scale_s, scale_t, uv);
}

// Re-sample the base `StandardMaterial`'s base-colour texture at `uv` and multiply by
// the base-colour factor — the same result `pbr_input_from_standard_material`
// produces, but at an arbitrary UV (used to override the base colour of a texture-
// animated face at its GPU-animated UV, independent of `base.uv_transform`).
fn sl_sample_base_color(slot: u32, uv: vec2<f32>) -> vec4<f32> {
#ifdef BINDLESS
    let indices = pbr_bindings::material_indices[slot];
    let m = pbr_bindings::material_array[indices.material];
    var color = m.base_color;
    if (m.flags & pbr_types::STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        color = color * textureSampleBias(
            bindless_textures_2d[indices.base_color_texture],
            bindless_samplers_filtering[indices.base_color_sampler],
            uv,
            view.mip_bias,
        );
    }
    return color;
#else   // BINDLESS
    var color = pbr_bindings::material.base_color;
    if (pbr_bindings::material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_BASE_COLOR_TEXTURE_BIT) != 0u {
        color = color * textureSampleBias(
            pbr_bindings::base_color_texture,
            pbr_bindings::base_color_sampler,
            uv,
            view.mip_bias,
        );
    }
    return color;
#endif  // BINDLESS
}

// Sample the legacy specular map (extension slot 51) at `uv`, returning white when
// the face carries none (a spec-colour-only material). The RGB is the per-texel
// specular tint and the alpha the per-texel environment weight, exactly as the
// reference `getSpecular` / `env_intensity * spec.a` read it.
fn sl_sample_specular(slot: u32, uv: vec2<f32>) -> vec4<f32> {
#ifdef BINDLESS
    return textureSampleBias(
        bindless_textures_2d[sl_indices[slot].specular_map],
        bindless_samplers_filtering[sl_indices[slot].specular_map_sampler],
        uv,
        view.mip_bias,
    );
#else   // BINDLESS
    return textureSampleBias(sl_spec_tex, sl_spec_samp, uv, view.mip_bias);
#endif  // BINDLESS
}

// A per-fragment tangent basis (`mat3x3(T, B, N)`) derived from screen-space
// derivatives — Christian Schüler's "normal mapping without precomputed tangents"
// (the cotangent frame). Second Life prim / mesh / avatar face meshes never carry
// vertex tangents (the geometry pipeline stores only position / normal / UV), so
// the reference viewer's per-vertex MikkTSpace tangents are unavailable here; this
// reconstructs an equivalent basis per fragment from the world-position and UV
// gradients, letting a normal map perturb the shading normal faithfully with no
// per-mesh tangent-generation cost. `n` is the interpolated geometric world normal,
// `p` the world position, `uv` the (transformed) UV the normal map is sampled at.
fn sl_cotangent_frame(n: vec3<f32>, p: vec3<f32>, uv: vec2<f32>) -> mat3x3<f32> {
    let dp1 = dpdx(p);
    let dp2 = dpdy(p);
    let duv1 = dpdx(uv);
    let duv2 = dpdy(uv);
    // Solve the linear system for the tangent / bitangent in the plane of `n`.
    let dp2perp = cross(dp2, n);
    let dp1perp = cross(n, dp1);
    let t = dp2perp * duv1.x + dp1perp * duv2.x;
    let b = dp2perp * duv1.y + dp1perp * duv2.y;
    // Scale so the larger of T / B has unit length (a degenerate patch keeps a
    // finite basis rather than exploding).
    let invmax = inverseSqrt(max(dot(t, t), dot(b, b)));
    return mat3x3<f32>(t * invmax, b * invmax, n);
}

// The legacy Blinn-Phong specular highlight (Phase 2): an **analytic normalized
// Blinn-Phong** lobe added over the matte base of a legacy (pre-PBR) face, the
// closed form the reference viewer bakes into its `lightFunc` LUT
// (`pipeline.cpp` `createLUTBuffers`) assembled with the material specular terms
// (`class3/deferred/materialF.glsl`). It is an approximation, not the exact port:
// it uses the dominant directional light (the sun) as the reference's sun term
// does but omits the reflection-probe environment (there is none on the headless
// path) — the pixel-closer exact port is tracked in `viewer-legacy-material-exact-port`.
//
// `n` = the perturbed surface normal (`pbr_input.N`, the normal map already folded
// in where the face has tangents), `world_pos` the fragment world position,
// `spec_rgb` the specular tint (map × `specular_color`), `glossiness` the exponent
// scalar (`specular_exponent / 255`, already modulated by the normal-map alpha).
fn sl_blinn_phong_specular(
    n: vec3<f32>, world_pos: vec3<f32>, spec_rgb: vec3<f32>, glossiness: f32,
) -> vec3<f32> {
    if glossiness <= 0.0 || lights.n_directional_lights == 0u {
        return vec3<f32>(0.0);
    }
    let light = lights.directional_lights[0];
    let l = normalize(light.direction_to_light);
    let v = normalize(view.world_position - world_pos);
    let h = normalize(l + v);
    let nl = dot(n, l);
    let nh = dot(n, h);
    if nl <= 0.0 || nh <= 0.0 {
        return vec3<f32>(0.0);
    }
    let nv = max(dot(n, v), 1e-4);
    let vh = max(dot(v, h), 1e-4);
    // Normalized Blinn-Phong distribution — the exact closed form the reference's
    // lightFunc LUT samples: exponent = glossiness²·specExp, then the full
    // normalization curve `((n+2)(n+4)) / (8π(2^(-n/2) + n))`.
    let expn = glossiness * glossiness * SL_SPECULAR_EXPONENT;
    var d = pow(nh, expn);
    d = d * (((expn + 2.0) * (expn + 4.0)) / (8.0 * SL_PI * (pow(2.0, -expn / 2.0) + expn)));
    // The reference's Fresnel + geometry assembly (`materialF.glsl`).
    let fres = pow(1.0 - vh, 5.0) * 0.4 + 0.5;
    let gtdenom = 2.0 * nh;
    let gt = max(0.0, min(gtdenom * nv / vh, gtdenom * nl / vh));
    let lit = min(nl * 6.0, 1.0);
    let scol = fres * d * gt / (nh * nl);
    return lit * scol * light.color.rgb * spec_rgb;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    // Base-colour (at its folded UV transform) + the scalar factors. The base
    // material carries no normal/MR/emissive texture for a PBR face, so metallic /
    // roughness / emissive here are just the factors, which we multiply below.
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
#ifdef BINDLESS
    let sl = sl_data[sl_indices[slot].params];
#else   // BINDLESS
    let sl = sl_material;
#endif  // BINDLESS

    // Legacy glossiness is modulated per-texel by the normal-map alpha (the
    // reference `getNormal`'s `glossiness *= vNt.a`); captured in the normal branch
    // below and applied to the specular lobe. Unity where the face has no normal map
    // (or no tangents to sample it with).
    var gloss_modulator = 1.0;
    // The legacy specular-map sample (RGB tint, alpha environment weight), sampled at
    // its own UV in the VERTEX_UVS block below and read by the legacy-specular block
    // after lighting. White for a spec-colour-only material (or a face with no UVs).
    var spec_sample = vec4<f32>(1.0);

// The extension maps are sampled from `base_uv`; only reachable when the mesh has
// UVs. `map_flags` / `anim_mode` are uniforms, so each branch is uniform control
// flow (texture sampling with implicit derivatives is valid).
#ifdef VERTEX_UVS
    // GPU texture animation (P28.2): a running animation replaces the face's texture
    // placement with a time-derived one (evaluated here, not written per frame on the
    // CPU). `base_uv` then drives the base-colour re-sample and every extension map,
    // so the whole face animates. Inert (`ANIM_ON` clear) leaves `base_uv == in.uv`.
    var base_uv = in.uv;
    if (sl.anim_mode & ANIM_ON) != 0u {
        base_uv = sl_animated_uv(sl, in.uv);
        pbr_input.material.base_color = sl_sample_base_color(slot, base_uv);
    }

    if (sl.map_flags & MAP_FLAG_MR) != 0u {
        let uv = sl_uv(sl.uv_mr_mat, sl.uv_translations_a.zw, base_uv);
#ifdef BINDLESS
        let mr = textureSampleBias(
            bindless_textures_2d[sl_indices[slot].metallic_roughness_map],
            bindless_samplers_filtering[sl_indices[slot].metallic_roughness_map_sampler],
            uv,
            view.mip_bias,
        );
#else   // BINDLESS
        let mr = textureSampleBias(sl_mr_tex, sl_mr_samp, uv, view.mip_bias);
#endif  // BINDLESS
        // Second Life packs ORM: green = roughness, blue = metallic, red = occlusion.
        pbr_input.material.perceptual_roughness *= mr.g;
        pbr_input.material.metallic *= mr.b;
        pbr_input.diffuse_occlusion = vec3<f32>(mr.r);
    }
    if (sl.map_flags & MAP_FLAG_EMISSIVE) != 0u {
        let uv = sl_uv(sl.uv_emissive_mat, sl.uv_translations_b.xy, base_uv);
        // The emissive map is uploaded sRGB, so the sample is already linear.
#ifdef BINDLESS
        let e = textureSampleBias(
            bindless_textures_2d[sl_indices[slot].emissive_map],
            bindless_samplers_filtering[sl_indices[slot].emissive_map_sampler],
            uv,
            view.mip_bias,
        );
#else   // BINDLESS
        let e = textureSampleBias(sl_emissive_tex, sl_emissive_samp, uv, view.mip_bias);
#endif  // BINDLESS
        pbr_input.material.emissive = vec4<f32>(
            pbr_input.material.emissive.rgb * e.rgb,
            pbr_input.material.emissive.a,
        );
    }
    if (sl.map_flags & MAP_FLAG_NORMAL) != 0u {
        let uv = sl_uv(sl.uv_normal_mat, sl.uv_translations_a.xy, base_uv);
#ifdef BINDLESS
        let ns = textureSampleBias(
            bindless_textures_2d[sl_indices[slot].normal_map],
            bindless_samplers_filtering[sl_indices[slot].normal_map_sampler],
            uv,
            view.mip_bias,
        );
#else   // BINDLESS
        let ns = textureSampleBias(sl_normal_tex, sl_normal_samp, uv, view.mip_bias);
#endif  // BINDLESS
        let nt = ns.rgb;
        // A legacy normal map packs per-texel glossiness in its alpha channel.
        gloss_modulator = ns.a;
        // Prefer the mesh's own vertex tangents when it has them; SL face meshes
        // never do, so fall back to a screen-space cotangent frame (both give
        // `apply_normal_mapping` the `mat3x3(T, B, N)` it expects).
#ifdef VERTEX_TANGENTS
        let tbn = pbr_functions::calculate_tbn_mikktspace(pbr_input.world_normal, in.world_tangent);
#else   // VERTEX_TANGENTS
        let tbn = sl_cotangent_frame(pbr_input.world_normal, pbr_input.world_position.xyz, uv);
#endif  // VERTEX_TANGENTS
        let double_sided =
            (pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u;
        pbr_input.N = pbr_functions::apply_normal_mapping(
            pbr_input.material.flags,
            tbn,
            double_sided,
            is_front,
            nt,
        );
    }
    // The legacy specular map (its own per-map transform), read by the
    // legacy-specular block after lighting. Only a legacy face sets MAP_FLAG_SPEC.
    if (sl.map_flags & MAP_FLAG_SPEC) != 0u {
        let uv = sl_uv(sl.uv_spec_mat, sl.uv_translations_b.zw, base_uv);
        spec_sample = sl_sample_specular(slot, uv);
    }
#endif // VERTEX_UVS

    // Alpha mask / cutoff, exactly as `StandardMaterial` does.
    pbr_input.material.base_color =
        pbr_functions::alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    // Honour the `StandardMaterial` **unlit** flag, exactly as Bevy's own `pbr`
    // fragment does. Legacy SL **fullbright** faces (P27.4, `bump::apply_surface_flags`)
    // and every **HUD** face (`hud::apply_hud_fullbright`) set it: such a face is
    // shown at full texture brightness, ignoring scene lighting, so its base colour
    // *is* the whole surface — no PBR lighting term and no legacy specular. This
    // custom shader must branch on the flag too; when it always applied lighting, a
    // fullbright face rendered black wherever no light reached it (a HUD is on its
    // own layer, which the world's sun does not light at all — the reported all-black
    // HUD, and dark fullbright prims at night).
    if (pbr_input.material.flags & pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT) != 0u {
        out.color = pbr_input.material.base_color;
    } else {
        // Reuse `StandardMaterial`'s metallic-roughness PBR lighting (a legacy face's
        // base is matte — metallic 0, roughness 1, reflectance 0 — so this is just its
        // diffuse term and the legacy specular is added below, no doubled GGX lobe).
        out.color = apply_pbr_lighting(pbr_input);

        // Legacy Blinn-Phong specular (Phase 2): a legacy face adds the SL specular
        // highlight — the analytic normalized Blinn-Phong lobe, plus a crude
        // environment reflection term (no reflection probe on the headless path).
        // Added before fog / post so the highlight is fogged like the reference's.
        // Inert for a PBR / diffuse face (`mode != LEGACY`), where the base material
        // is the whole surface.
        if sl.mode == SL_FACE_MODE_LEGACY {
            let spec_rgb = sl.specular_color.rgb * spec_sample.rgb;
            let glossiness = sl.glossiness * gloss_modulator;
            let highlight = sl_blinn_phong_specular(
                pbr_input.N, pbr_input.world_position.xyz, spec_rgb, glossiness,
            );
            // Environment reflection: with no reflection probe headless, approximate
            // the reference's `applyLegacyEnv` as a spec-tinted ambient term scaled by
            // the environment weight (`env_intensity * spec.a`).
            let env = sl.env_intensity * spec_sample.a;
            let ambient = env * spec_rgb * lights.ambient_color.rgb;
            out.color = vec4<f32>(out.color.rgb + highlight + ambient, out.color.a);
        }
    }

    out.color = main_pass_post_lighting_processing(pbr_input, out.color);

    // The faithful SL glow mask (`glow.rs`): an opaque / mask face writes its glow
    // scalar into alpha (`0` for a non-glowing face), which the glow pass reads as
    // the per-face glow mask. A blend face's alpha is its coverage, so the sentinel
    // `glow < 0` leaves it untouched (set on the CPU where the alpha mode is known).
    // Inert until the glow pass is enabled — nothing else reads the scene alpha.
    if sl.glow >= 0.0 {
        out.color.a = sl.glow;
    }
    return out;
}
