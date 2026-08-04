// World-space name-tag billboard shader (the reference viewer's `LLHUDNameTag`
// rendering, `llhudnametag.cpp::renderText`): one mesh per tag, authored in
// tag-local **physical pixels ÷ 1024** around the bubble centre, expanded here
// into a camera-facing quad fan whose on-screen size stays constant at every
// distance — the reference's "pixel vector" behaviour
// (`LLViewerCamera::getPixelVectors`).
//
// Per-frame data reaches the shader without touching the material asset (which
// would recreate its bind group every frame):
// - the anchor's world position rides the entity transform (mesh uniform);
// - the anti-overlap screen offset rides the per-instance `MeshTag` u32 (two
//   offset-biased u16 pixel components, billboard-local, +y up);
// - the distance fade is computed here from the view position, so tags fade
//   with range (`alpha = 1 − (dist − fade_start) / fade_range`, the reference's
//   `mFadeDistance`/`mFadeRange`) with zero CPU writes.
//
// Vertex channels:
// - `position.xy`: tag-local pixels ÷ 1024 (bubble-centred, +y up);
// - `position.z`: the camera-pull distance in metres (the reference pushes a
//   tag toward the camera by the source object's radius so the avatar's own
//   head does not swallow it);
// - `uv0`: atlas UV for glyph quads; **negative sentinel** on bubble vertices,
//   carrying `(−half_w, −half_h)` of the bubble in pixels (constant across the
//   quad, so the fragment recovers the SDF half-extents as `−uv0`);
// - `uv1`: bubble vertices carry the corner's signed offset from the bubble
//   centre in pixels (interpolating to the fragment's local position — the
//   rounded-rect SDF sample point); glyph vertices carry `(0, 0)`;
// - `color`: linear premultiplied-nothing straight RGBA — the bubble tint, the
//   drop-shadow black, or the glyph line colour (white for colour emoji).

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings as view_bindings,
}

// Material params: x = fade-start distance (m), y = fade range (m),
// z, w = unused (the reference's +25 px screen lift is baked into the mesh,
// where the layout scale factor is known).
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: vec4<f32>;
// The glyph-atlas page this tag mesh (or page mesh) samples.
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var atlas_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var atlas_sampler: sampler;

// One tag vertex (see the channel table in the header comment).
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) uv0: vec2<f32>,
    @location(2) uv1: vec2<f32>,
    @location(3) color: vec4<f32>,
}

// Interpolated fragment inputs: the atlas UV (or bubble sentinel), the bubble
// SDF sample point, the authored colour, and the camera→anchor distance the
// distance fade needs.
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv0: vec2<f32>,
    @location(1) uv1: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) anchor_dist: f32,
}

// Tag-local pixel coordinates are stored ÷ 1024 so the mesh AABB stays tiny and
// the Transparent3d sort key (the transformed AABB centre) stays on the anchor.
const UNITS_TO_PIXELS: f32 = 1024.0;

// The bias added to each signed 16-bit `MeshTag` offset component.
const TAG_OFFSET_BIAS: f32 = 32768.0;

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
    // The entity translation IS the anchor (the mesh is bubble-centred).
    let anchor = world_from_local[3].xyz;
    let camera = view_bindings::view.world_position;
    let anchor_dist = distance(anchor, camera);

    // Pull the billboard origin toward the camera by the baked radius so the
    // avatar's own body cannot occlude its tag, clamping so a very close
    // camera never pulls it behind the near plane (nor past itself).
    let pull = min(in.position.z, max(anchor_dist - 0.05, 0.0));
    let to_camera = select(
        vec3<f32>(0.0, 0.0, 0.0),
        (camera - anchor) / max(anchor_dist, 0.001),
        anchor_dist > 0.001,
    );
    let pulled = anchor + to_camera * pull;

    // Metres per physical pixel at the pulled point's view depth: the vertical
    // NDC range (2) spans `viewport.h` pixels scaled by the projection's
    // `f = clip_from_view[1][1]` (the main camera is always perspective).
    let view_pos = view_bindings::view.view_from_world * vec4<f32>(pulled, 1.0);
    let depth = max(-view_pos.z, 0.05);
    let metres_per_pixel =
        2.0 * depth / (view_bindings::view.viewport.w * view_bindings::view.clip_from_view[1][1]);

    // Billboard basis: the view's world-space right/up columns.
    let right = view_bindings::view.world_from_view[0].xyz;
    let up = view_bindings::view.world_from_view[1].xyz;

    // Per-instance anti-overlap offset (billboard-local px, +y up), packed as
    // two offset-biased u16s in the mesh tag.
    let tag_bits = mesh_functions::get_tag(in.instance_index);
    let overlap_offset = vec2<f32>(
        f32(tag_bits >> 16u),
        f32(tag_bits & 0xffffu),
    ) - TAG_OFFSET_BIAS;

    let local_px = in.position.xy * UNITS_TO_PIXELS + overlap_offset;
    let world = pulled + (right * local_px.x + up * local_px.y) * metres_per_pixel;
    out.clip_position = view_bindings::view.clip_from_world * vec4<f32>(world, 1.0);
    out.uv0 = in.uv0;
    out.uv1 = in.uv1;
    out.color = in.color;
    out.anchor_dist = anchor_dist;
    return out;
}

// The bubble's rounded-corner radius, in physical pixels (the reference uses a
// `Rounded_Rect` 9-slice; an SDF gives the same read with resolution-independent
// anti-aliased corners).
const CORNER_RADIUS_PX: f32 = 8.0;

// Signed distance to a rounded rectangle centred on the origin with half
// extents `b` and corner radius `r` (negative inside).
fn rounded_rect_sdf(point: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(point) - b + vec2<f32>(r, r);
    return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The reference's distance fade: full inside `fade_start`, gone past
    // `fade_start + fade_range` (`llhudnametag.cpp` `mFadeDistance`/`mFadeRange`).
    let fade = clamp(
        1.0 - (in.anchor_dist - params.x) / max(params.y, 0.001),
        0.0,
        1.0,
    );

    var color: vec4<f32>;
    if in.uv0.x < 0.0 {
        // Bubble quad: rounded-rect SDF, half extents recovered from the
        // constant negative sentinel, sample point from the interpolated
        // corner offsets, ~1 px anti-aliased edge.
        let half_extents = -in.uv0;
        let d = rounded_rect_sdf(in.uv1, half_extents, CORNER_RADIUS_PX);
        let aa = max(fwidth(d), 0.001);
        let coverage = 1.0 - smoothstep(-aa, aa, d);
        color = vec4<f32>(in.color.rgb, in.color.a * coverage);
    } else {
        // Glyph (or drop-shadow) quad: atlas pages store white + alpha for
        // mask glyphs and full colour for emoji, so `sample × vertex colour`
        // is correct for both.
        let sampled = textureSample(atlas_texture, atlas_sampler, in.uv0);
        color = sampled * in.color;
    }
    color.a = color.a * fade;
    if color.a <= 0.001 {
        discard;
    }
    return color;
}
