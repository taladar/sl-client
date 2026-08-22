// The radial (pie) menu's ring, drawn as one `UiMaterial` fragment shader
// (`viewer-ui-radial-menu`).
//
// One node, one draw: `bevy_ui` has no wedge primitive, and approximating a
// washer segment out of rectangles would be both ugly and a lie about the
// geometry. The fragment shader instead answers, per pixel, the *same* question
// the selection code answers per frame — "which slot is this direction in?" —
// from the same rotate-by-half-a-slice angle maths. That is deliberate: the
// picture a user learns and the slot the widget picks are computed the same way,
// so they cannot drift apart.
//
// Coordinates here are `bevy_ui`'s: the origin is the node's top-left and **+y
// points down**. The compass maths is done in a y-up frame, so every angle is
// taken from `-p.y` — the single conversion, mirroring `crate::pie_menu`'s own.
//
// Note there is no cursor drawn here. The real pointer stays visible and is the
// only cursor — see `crate::pie_menu`'s placement section for why the reference's
// pointer warp does not port, and what was given up instead.

#import bevy_ui::ui_vertex_output::UiVertexOutput

struct PieParams {
    // The ring's resting fill.
    background: vec4<f32>,
    // The dividers, and the inner / outer edge rings.
    line: vec4<f32>,
    // The highlighted slot's fill.
    selected: vec4<f32>,
    // The dead zone's radius: inside this nothing is selected, so it is drawn as
    // a hole rather than as a slice.
    inner_radius: f32,
    // The ring's outer radius.
    outer_radius: f32,
    // Eight slot states, four bits each, slot 0 in the low nibble. See
    // `SlotState` in `pie_menu.rs` — 0 empty, 1 action, 2 disabled, 3 sub-pie.
    slot_states: u32,
    // The slot under the pointer, or -1 for none (the dead zone).
    highlighted: i32,
}

@group(1) @binding(0)
var<uniform> params: PieParams;

const PI: f32 = 3.14159265359;
const SLICES: f32 = 8.0;
const SLICE_COUNT: u32 = 8u;

// A slot that holds nothing at all. Its wedge still renders — an absent entry
// leaves its slice *empty*, it never lets a neighbour rotate into the gap — but
// it renders dimmer, so "nothing lives north" reads differently from "north is
// unavailable right now".
const STATE_EMPTY: u32 = 0u;
// A slot whose entry cannot be picked in the current state. It keeps its wedge
// and its (faded) label: the position is a property of the entry, not of whether
// it happens to be available.
const STATE_DISABLED: u32 = 2u;
// A slot that opens another pie. It gets an outward chevron on the rim — the
// "this descends" affordance, drawn rather than written as a `>` in the label, so
// there is no bidi mirroring to worry about and no width added to the text. This
// is the honest, *named* sub-pie the widget is built around, not the reference's
// `More >` overflow.
const STATE_SUBPIE: u32 = 3u;

// The chevron's radial depth inward from the rim, in pixels.
const SUBPIE_MARK_DEPTH: f32 = 9.0;
// The chevron's angular half-width at its base, in radians.
const SUBPIE_MARK_HALF_ANGLE: f32 = 0.075;

// Half the angular width of a divider, in radians. The reference's
// `PIE_SLICE_DIVIDER_WIDTH` is 0.04 wide in total.
const DIVIDER_HALF_WIDTH: f32 = 0.02;

// The width, in pixels, of the ring's inner and outer edge lines.
const EDGE_WIDTH: f32 = 1.5;

// The alpha an empty slot's fill is scaled by.
const EMPTY_FILL_FADE: f32 = 0.45;

// Which slot a direction falls in — the shader's copy of `Compass::from_angle`.
//
// Rotating by half a slice before the divide is what puts the slice *centres* on
// the compass points rather than their edges: without it, "due north" would land
// exactly on the boundary between two slices, which is the one direction a user
// must never have to be precise about.
fn slot_of(offset: vec2<f32>) -> u32 {
    // +y is down in UI space, so negate it to get the y-up frame the compass is
    // reasoned about in.
    let angle = atan2(-offset.y, offset.x) + PI / SLICES;
    let wrapped = angle - floor(angle / (2.0 * PI)) * (2.0 * PI);
    return u32(floor(SLICES * wrapped / (2.0 * PI))) % SLICE_COUNT;
}

// One slot's four-bit state.
fn state_of(slot: u32) -> u32 {
    return (params.slot_states >> (slot * 4u)) & 0xFu;
}

// The angular distance from `angle` to the nearest slice boundary.
fn distance_to_divider(offset: vec2<f32>) -> f32 {
    let angle = atan2(-offset.y, offset.x) + PI / SLICES;
    let slice = 2.0 * PI / SLICES;
    let within = angle - floor(angle / slice) * slice;
    return min(within, slice - within);
}

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    // The node is square and the ring fills it, so the centre is the node's
    // centre and the pixel offset follows from the uv.
    let offset = (in.uv - vec2<f32>(0.5, 0.5)) * in.size;
    let distance = length(offset);

    // Outside the ring, or inside the dead zone: nothing to draw. The dead zone
    // is a hole rather than a ninth slice — releasing without moving must cancel,
    // and a hole is what says so.
    if distance > params.outer_radius || distance < params.inner_radius {
        return vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }

    let slot = slot_of(offset);
    let state = state_of(slot);

    var color = params.background;
    if state == STATE_EMPTY {
        color = vec4<f32>(color.rgb, color.a * EMPTY_FILL_FADE);
    }
    // The highlight, but never on a slot that cannot be picked: lighting up a
    // disabled or empty slot would promise a click that does nothing. Drawn as a
    // **radial gradient** — the selected colour at full strength against the dead
    // zone, fading to nothing at the rim — matching the reference's
    // `gl_washer_segment_2d(…, selectedColor, borderColor)`, whose border colour
    // is transparent.
    if i32(slot) == params.highlighted && state != STATE_EMPTY && state != STATE_DISABLED {
        let span = max(params.outer_radius - params.inner_radius, 1.0);
        let t = clamp((distance - params.inner_radius) / span, 0.0, 1.0);
        let selected_alpha = params.selected.a * (1.0 - t);
        color = vec4<f32>(
            mix(color.rgb, params.selected.rgb, selected_alpha),
            color.a + selected_alpha * (1.0 - color.a),
        );
    }

    // The dividers, and the two edge rings, drawn over the fill.
    let divider = 1.0 - smoothstep(DIVIDER_HALF_WIDTH * 0.5, DIVIDER_HALF_WIDTH, distance_to_divider(offset));
    let outer_edge = smoothstep(params.outer_radius - EDGE_WIDTH, params.outer_radius, distance);
    let inner_edge = 1.0 - smoothstep(params.inner_radius, params.inner_radius + EDGE_WIDTH, distance);
    let line_amount = clamp(max(divider, max(outer_edge, inner_edge)), 0.0, 1.0);
    color = mix(color, params.line, line_amount * params.line.a);

    // The sub-pie affordance: an outward chevron on the rim of a descendable
    // slice. A triangle wide at its base (inward) and pointing to the rim, centred
    // on the slice, so it reads as "there is more this way" without a `>` in the
    // text. Drawn on the slice's own centre line so it never strays into a
    // neighbour.
    if state == STATE_SUBPIE {
        let slot_centre = f32(slot) * (2.0 * PI / SLICES);
        let pixel_angle = atan2(-offset.y, offset.x);
        let delta = abs(atan2(sin(pixel_angle - slot_centre), cos(pixel_angle - slot_centre)));
        let base = params.outer_radius - SUBPIE_MARK_DEPTH;
        let tip = params.outer_radius - 1.0;
        if distance > base && distance < tip {
            // Narrows from the base to the tip, so the chevron points outward.
            let along = (distance - base) / (tip - base);
            let allowed = SUBPIE_MARK_HALF_ANGLE * (1.0 - along);
            if delta < allowed {
                color = params.line;
            }
        }
    }

    // Feather the outer rim so the circle does not read as a staircase.
    let rim = 1.0 - smoothstep(params.outer_radius - 1.0, params.outer_radius, distance);
    return vec4<f32>(color.rgb, color.a * rim);
}
