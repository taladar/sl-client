// The in-world tracking-beacon beam (the reference viewer's `LLTracker::drawBeacon`
// / `renderBeacon`): a tall, camera-facing translucent blade the viewer draws at a
// tracked position so you can walk / fly toward it. Two stacked blades make one
// beacon — a blue lower shaft from the ground up to the target, and a red upper
// shaft from the target up to the sky ceiling — with the target sitting at the
// colour seam.
//
// The blade is billboarded on the CPU (its entity is yawed to face the camera each
// frame), so this shader only carries the mesh through the standard transform. The
// per-vertex colour's `a` is the soft-edge profile (opaque at the blade's centre
// spine, transparent at its two side edges — the reference's bright core / clear
// edge), and the material's `beam_color` uniform carries the shaft's RGB tint and
// base alpha. The fragment ramps the alpha with camera distance exactly as the
// reference does (`llmax(0.2, llmin(0.5, (dist - FADE_DIST) / FADE_DIST))`), so a
// distant beacon reads a little more solid, and clamps so it never fully vanishes.
//
// Unlit and alpha-blended (depth-tested against the world, no depth write, like the
// reference's `LLGLDepthTest(GL_TRUE, GL_FALSE)`): the colour is authored data, not
// a lit surface, and the tall upper shaft pokes above nearer geometry so the beacon
// still reads as a waypoint from behind a building.

#import bevy_pbr::{
    mesh_functions,
    mesh_view_bindings as view_bindings,
    view_transformations::position_world_to_clip,
}

// One blade vertex: its position (blade-local metres, carried into Bevy world space
// by the billboarded entity transform) and the soft-edge profile in the colour's
// `a` (`1` on the centre spine, `0` on the two side edges; `rgb` unused — the tint
// comes from the `beam_color` uniform).
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
}

// The interpolated per-fragment inputs: the world position (for the camera-distance
// alpha ramp) and the soft-edge alpha profile.
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) edge_alpha: f32,
}

// The shaft's RGB tint (`rgb`) and base alpha (`a`) — set per beacon-half from the
// tracked-thing's colour code, never mutated per frame.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> beam_color: vec4<f32>;

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
    out.edge_alpha = in.color.a;
    return out;
}

// The reference's distance-driven beacon alpha: transparent-ish near, ramping to a
// half-opaque cap far away, and never below a floor so the beacon is always at
// least faintly visible (`lltracker.cpp` renderBeacon).
const FADE_DIST: f32 = 3.0;
const MIN_ALPHA: f32 = 0.35;
const MAX_ALPHA: f32 = 0.7;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dist = distance(in.world_position, view_bindings::view.world_position);
    let distance_alpha = clamp((dist - FADE_DIST) / FADE_DIST, MIN_ALPHA, MAX_ALPHA);
    let alpha = in.edge_alpha * beam_color.a * distance_alpha;
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(beam_color.rgb, alpha);
}
