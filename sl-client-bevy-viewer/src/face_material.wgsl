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
}

// Must match the `MAP_FLAG_*` constants in face_material.rs.
const MAP_FLAG_NORMAL: u32 = 1u;
const MAP_FLAG_MR: u32 = 2u;
const MAP_FLAG_EMISSIVE: u32 = 4u;

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
#ifdef VERTEX_TANGENTS
    if (sl.map_flags & MAP_FLAG_NORMAL) != 0u {
        let uv = sl_uv(sl.uv_normal_mat, sl.uv_translations_a.xy, base_uv);
#ifdef BINDLESS
        let nt = textureSampleBias(
            bindless_textures_2d[sl_indices[slot].normal_map],
            bindless_samplers_filtering[sl_indices[slot].normal_map_sampler],
            uv,
            view.mip_bias,
        ).rgb;
#else   // BINDLESS
        let nt = textureSampleBias(sl_normal_tex, sl_normal_samp, uv, view.mip_bias).rgb;
#endif  // BINDLESS
        let tbn = pbr_functions::calculate_tbn_mikktspace(pbr_input.world_normal, in.world_tangent);
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
#endif // VERTEX_TANGENTS
#endif // VERTEX_UVS

    // Alpha mask / cutoff, exactly as `StandardMaterial` does.
    pbr_input.material.base_color =
        pbr_functions::alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    // Reuse `StandardMaterial`'s metallic-roughness PBR lighting + post processing.
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
