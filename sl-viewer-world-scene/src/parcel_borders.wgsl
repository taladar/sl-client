// In-world parcel property-line bands: short, vertical, ownership-coloured strips
// draped along parcel boundaries (the reference viewer's "Show Property Lines",
// `LLViewerParcelOverlay::renderPropertyLines`). Each band's per-vertex colour
// carries the ownership tint in `rgb` and a bottom→top fade in `a` (opaque at the
// ground, transparent at the top, giving the characteristic soft band); this
// shader multiplies that by a camera-distance fade so the bands thin out and
// vanish with range, mirroring the reference's per-edge distance clip
// (`PROPERTY_LINE_CLIP_DIST`).
//
// Unlit and alpha-blended: the colour is authored data, not a lit surface. World
// position comes from the mesh transform (one entity per region, placed at the
// region's south-west corner like the terrain patches), and the camera position
// from the view bind group — so nothing on the CPU side changes per frame.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings as view_bindings,
    view_transformations::position_world_to_clip,
}

// One band vertex: its position (Second Life space, relative to the region
// south-west corner, carried into Bevy space by the entity transform) and its
// authored ownership colour (`rgb` tint, `a` the bottom→top band fade).
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

// The interpolated per-fragment band inputs: the world position (for the
// camera-distance fade) and the authored colour.
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
    let world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(in.position, 1.0),
    );
    out.world_position = world_position.xyz;
    out.clip_position = position_world_to_clip(world_position.xyz);
    out.color = in.color;
    return out;
}

// Distance (metres) at which the bands start thinning out, and the range at which
// they are fully gone — the reference clips property lines at 256 m; the band
// ramps down over the last half of that so it fades with distance rather than
// popping out.
const FADE_START_METRES: f32 = 128.0;
const CLIP_METRES: f32 = 256.0;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dist = distance(in.world_position, view_bindings::view.world_position);
    let distance_fade = 1.0 - smoothstep(FADE_START_METRES, CLIP_METRES, dist);
    let alpha = in.color.a * distance_fade;
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(in.color.rgb, alpha);
}
