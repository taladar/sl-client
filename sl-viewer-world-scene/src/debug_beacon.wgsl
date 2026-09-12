// The in-world **debug beacon** (the reference viewer's
// `LLViewerObjectList::renderObjectBeacons`): a coloured cross marking a place a
// floater is talking about — the telehub and its selected spawn point, an object
// picked out of a search list.
//
// The reference draws it twice: once with the depth test off and the colour's
// alpha quartered, so the marker is faintly visible *through* whatever stands in
// front of it, and once depth-tested at full alpha so the part in clear view reads
// solid. Both passes come through this shader; which one a draw is belongs to the
// material (`through`), which specialises the depth compare on the CPU side.
//
// The geometry carries no colour of its own — one mesh per pass shape is shared by
// every beacon and the tint rides the `beacon_color` uniform, so a floater marking
// two places in two colours costs two materials and no extra geometry.

#import bevy_pbr::{
    mesh_functions,
    view_transformations::position_world_to_clip,
}

// One marker vertex: its position in marker-local metres, carried into Bevy world
// space by the entity transform (which places and orients the marker).
struct Vertex {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
}

// The interpolated per-fragment inputs: nothing but the clip position — the marker
// is a flat, unlit, uniformly tinted solid.
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
}

// The marker's RGB tint (`rgb`) and alpha (`a`) — set per beacon, never mutated per
// frame.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> beacon_color: vec4<f32>;

@vertex
fn vertex(in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
    let world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(in.position, 1.0),
    );
    out.clip_position = position_world_to_clip(world_position.xyz);
    return out;
}

@fragment
fn fragment(_in: VertexOutput) -> @location(0) vec4<f32> {
    return beacon_color;
}
