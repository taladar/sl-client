//! The **flexible-prim ("flexi") chain simulation** — a client-side softbody
//! deformation of the extrusion [`path`](crate::path).
//!
//! A flexi prim is an ordinary volume prim carrying an `LLFlexibleObjectData`
//! extra param (softness, tension, air friction, gravity, wind sensitivity, a
//! user force). Its geometry is *not* server-authoritative: the simulator sends
//! the prim's base position / rotation, and the viewer runs a local spring / chain
//! solver that bends the prim's path over time — a flexi antenna swings as you
//! move, a flexi skirt droops and settles. This module is the pure port of that
//! solver.
//!
//! It is a faithful re-implementation of Firestorm `LLVolumeImplFlexible`
//! (`indra/newview/llflexibleobject.cpp`) — `setAttributesOfAllSections` (chain
//! initialisation) and `doFlexibleUpdate` (the per-frame integration) — reworked
//! to the workspace's restriction lints and staying, like the rest of `sl-prim`,
//! Bevy-free and I/O-free. A [`FlexiChain`] holds the persistent per-section
//! state; [`FlexiChain::step`] advances it one frame; [`FlexiChain::path`] reads
//! the current sections out as an extrusion [`Path`](crate::path) the volume
//! sweep ([`tessellate_with_path`](crate::tessellate_with_path)) re-tessellates
//! the prim's profile along.
//!
//! # Coordinate frames
//!
//! The reference solver runs in **world** space (Z-up metres): the chain has
//! inertia in world space, which is what makes a flexi prim lag and swing when
//! its anchor moves. So [`FlexiChain::step`] takes the prim's world base position
//! and rotation, and the gravity / user-force / tension forces integrate world
//! section positions. [`FlexiChain::path`] then transforms those world sections
//! back into the prim's **local** frame as full-size **metre** geometry (the prim's
//! X/Y scale baked into the profile before the section rotation), matching the
//! reference viewer's flexi volume — which renders with an identity-scale relative
//! transform. The viewer therefore gives a flexi prim an identity geometry holder
//! (the way grass is generated in absolute metres), not the per-object scale a
//! rigid prim's holder applies. Baking the scale before the bend — rather than
//! letting a non-uniform holder scale a unit-local mesh after it — is what keeps
//! the cross-section from shearing as the chain droops (see [`FlexiChain::path`]).
//!
//! # Simplifications relative to the reference
//!
//! - **No screen-area LOD throttling.** The reference varies each flexi's
//!   simulate / render section count and update period by its on-screen pixel
//!   area (`updateRenderRes` / `doIdleUpdate`); here the section count is fixed by
//!   the softness (`1 << softness`) and every chain steps every frame. Flexi prims
//!   are few and low-resolution (at most eight sections), so this is cheap.
//! - **No wind.** The viewer has no region wind field, so the wind force is zero
//!   (the `wind_sensitivity` param is ingested but contributes nothing).
//! - **No collision sphere.** The reference's collision-sphere push-out is
//!   `#if 0`-ed out there too, so this omits it as well.

use crate::path::{Path, PathPoint, lerp, quat_axis_z, quat_mul, rotate_vector};
use crate::shape::PrimShape;

/// The maximum flexi softness / simulate-LOD level (`FLEXIBLE_OBJECT_MAX_SECTIONS`
/// in `llprimitive.h`): the section count is `1 << softness`, so a softness of 3
/// gives eight sections.
const MAX_SOFTNESS: u8 = 3;

/// The cap on the internal tension force per frame
/// (`FLEXIBLE_OBJECT_MAX_INTERNAL_TENSION_FORCE`): the tension coefficient is
/// clamped to this so a stiff chain cannot overshoot into instability.
const MAX_TENSION_FORCE: f32 = 0.99;

/// The largest per-frame time step the integrator honours (Firestorm's
/// `secondsThisFrame > 0.2f` clamp): a long stall (a slow frame, a paused window)
/// is treated as a 0.2 s step so the chain eases rather than exploding.
const MAX_TIMESTEP: f32 = 0.2;

/// The **fixed** time step the chain integrates at, decoupled from the render frame
/// (a 60 FPS step). Unlike the reference, which integrates each frame's variable
/// `secondsThisFrame` in one pass, [`FlexiChain::step`] accumulates real elapsed
/// time and advances the chain in whole steps of exactly this size (a fixed-timestep
/// accumulator; the leftover carries to the next frame).
///
/// This is what a stable spring sim needs and the reference lacks. A flexi's rest
/// pose is a **per-step-size equilibrium** (gravity / user force scale linearly with
/// the step, tension saturates), so if the step size tracked the frame time the
/// equilibrium would drift with the frame rate — and a hitch (a snapshot read-back
/// stall, an asset-decode burst, an alt-tab) would hand the integrator one huge step,
/// shoving a settled chain far off its rest pose so it visibly swings back and
/// re-settles. Integrating at a *constant* step instead pins the equilibrium: at any
/// frame rate, and across any stall, a settled chain sees only more equilibrium-sized
/// steps and so stays put, while a moving chain advances by the correct total sim
/// time (frame-rate independent). A frame longer than [`MAX_TIMESTEP`] still caps the
/// work (and drops the excess) so a long pause cannot spiral into unbounded catch-up.
const FIXED_TIMESTEP: f32 = 1.0 / 60.0;

/// The tension decay base (Firestorm's `pow(0.85f, dt*30)`): the tension
/// coefficient approaches its target with this per-`1/30 s` retention.
const TENSION_DECAY_BASE: f32 = 0.85;

/// The per-second tension decay exponent scale (Firestorm's `dt*30`).
const TENSION_DECAY_RATE: f32 = 30.0;

/// The dequantized flexible-object parameters driving a [`FlexiChain`] — the
/// solver's view of an `LLFlexibleObjectData` block, in Second Life semantics.
///
/// The viewer maps its decoded wire block onto this so `sl-prim` stays free of the
/// wire types; the field meanings match the reference viewer's
/// `LLFlexibleObjectData` accessors.
#[derive(Clone, Copy, PartialEq, Debug)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Flexi*` names read clearly"
)]
pub struct FlexiAttributes {
    /// The softness / simulate-LOD level (0–3); the chain has `1 << softness`
    /// sections. Clamped to `MAX_SOFTNESS` (3) on use.
    pub softness: u8,
    /// Path stiffness (resistance to bending), `0..10`.
    pub tension: f32,
    /// Air friction (how quickly motion damps), `0..10`.
    pub air_friction: f32,
    /// Gravity pulling the chain down along world `-Z`, `-10..10`.
    pub gravity: f32,
    /// Sensitivity to region wind, `0..10`. Not simulated here (no wind field).
    pub wind_sensitivity: f32,
    /// A constant force pushing every section, in world (Z-up) metres.
    pub user_force: [f32; 3],
}

impl FlexiAttributes {
    /// The chain section count for this softness: `1 << softness`, at least one.
    #[must_use]
    fn num_sections(&self) -> usize {
        1usize.wrapping_shl(u32::from(self.softness.min(MAX_SOFTNESS)))
    }
}

/// One simulated node of the flexi chain — the counterpart of Firestorm's
/// `LLFlexibleObjectSection`. Positions / velocities / directions / rotations are
/// **world** space (Z-up metres); the scale and axis rotation are constant per
/// section, set from the prim's taper and twist at construction.
#[derive(Clone, Copy, Debug)]
struct Section {
    /// The node's world position (Z-up metres).
    position: [f32; 3],
    /// The node's world velocity (a per-frame position delta, damped by inertia).
    velocity: [f32; 3],
    /// The unit direction from the parent node to this one (world space).
    direction: [f32; 3],
    /// The node's world orientation quaternion `(x, y, z, w)` in the crate's
    /// row-vector (`v * q`) convention.
    rotation: [f32; 4],
    /// The constant per-section twist about the local Z axis (from the prim's
    /// begin/end twist), applied before the simulated rotation when building the
    /// path frame.
    axis_rotation: [f32; 4],
    /// The constant per-section profile X/Y scale **fraction** (from the prim's
    /// taper); the viewer's scale holder multiplies in the prim's metre scale.
    scale: [f32; 2],
}

/// The persistent state of one flexi prim's chain simulation.
///
/// Constructed once (per prim, per softness) with [`FlexiChain::new`], advanced
/// each frame with [`FlexiChain::step`], and read out as a deformed extrusion
/// [`Path`] with [`FlexiChain::path`]. Holds `num_sections + 1` nodes: node 0 is
/// the anchor (pinned to the prim's base each step), nodes `1..=num_sections`
/// follow the local physics.
#[derive(Clone, Debug)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where the `Flexi*` names read clearly"
)]
pub struct FlexiChain {
    /// The chain nodes, anchor first (`num_sections + 1` entries).
    sections: Vec<Section>,
    /// The simulated section count (`1 << softness`); the node count is one more.
    num_sections: usize,
    /// Unspent real time carried between frames for the fixed-timestep accumulator
    /// ([`FIXED_TIMESTEP`]): [`step`](Self::step) adds the frame's `dt`, integrates
    /// as many whole [`FIXED_TIMESTEP`] steps as have accumulated, and keeps the
    /// remainder (`< FIXED_TIMESTEP`) here for the next frame.
    accumulator: f32,
}

impl FlexiChain {
    /// Build a chain for `shape` with `attributes`, at the prim's `object_scale`
    /// (metres per axis) and initial world `base_position` / `base_rotation`.
    ///
    /// Mirrors `setAttributesOfAllSections`: the per-section profile scale comes
    /// from the prim's path begin/end scale (taper) and the axis rotation from its
    /// begin/end twist, and the nodes are strung out straight along the anchor
    /// direction (the `remapSections` "generate from section 0" branch) as the
    /// rest pose the first [`step`](Self::step) relaxes from.
    #[must_use]
    pub fn new(
        shape: &PrimShape,
        attributes: &FlexiAttributes,
        object_scale: [f32; 3],
        base_position: [f32; 3],
        base_rotation: [f32; 4],
    ) -> Self {
        let num_sections = attributes.num_sections();

        // The path taper: a top size above 1 tapers the begin, below 1 the end
        // (Firestorm `LLPathParams::getBeginScale` / `getEndScale`).
        let (begin_scale, end_scale) = taper_scales(shape);
        let begin_rot = core::f32::consts::PI * shape.twist_begin;
        let end_rot = core::f32::consts::PI * shape.twist_end;

        let scale_z = object_scale.get(2).copied().unwrap_or(1.0);
        let section_length = scale_z / usize_to_f32(num_sections);

        // The anchor (node 0): the prim's base, offset down half its length along
        // its local Z, pointing along that same axis.
        let anchor_direction = rotate_vector(base_rotation, [0.0, 0.0, 1.0]);
        let anchor_position = vec_sub(base_position, vec_scale(anchor_direction, scale_z * 0.5));

        let anchor = Section {
            position: anchor_position,
            velocity: [0.0; 3],
            direction: anchor_direction,
            rotation: base_rotation,
            axis_rotation: quat_axis_z(begin_rot),
            scale: begin_scale,
        };
        let mut sections = Vec::with_capacity(num_sections.saturating_add(1));
        sections.push(anchor);

        let t_inc = 1.0 / usize_to_f32(num_sections);
        for i in 1..=num_sections {
            let t = usize_to_f32(i) * t_inc;
            // The parent is always present (node `i-1` was just pushed); the anchor
            // is a safe, unreachable fallback that keeps this index-free.
            let parent = sections.get(i.saturating_sub(1)).copied().unwrap_or(anchor);
            sections.push(Section {
                position: vec_add(parent.position, vec_scale(parent.direction, section_length)),
                velocity: [0.0; 3],
                direction: anchor_direction,
                rotation: base_rotation,
                axis_rotation: quat_axis_z(lerp(begin_rot, end_rot, t)),
                scale: [
                    lerp(begin_scale[0], end_scale[0], t),
                    lerp(begin_scale[1], end_scale[1], t),
                ],
            });
        }

        Self {
            sections,
            num_sections,
            accumulator: 0.0,
        }
    }

    /// The simulated section count (`1 << softness`); the chain holds one more
    /// node than this. A change in softness needs a fresh chain (the node count
    /// changes), so the viewer compares this to decide when to rebuild.
    #[must_use]
    pub const fn num_sections(&self) -> usize {
        self.num_sections
    }

    /// Advance the chain by the frame's `dt` seconds, with the prim's current world
    /// `base_position` / `base_rotation` and `object_scale` (metres per axis).
    ///
    /// A **fixed-timestep accumulator**: `dt` is added to the carried remainder and
    /// the chain is integrated in whole `FIXED_TIMESTEP` passes, keeping any leftover
    /// (`< FIXED_TIMESTEP`) for the next frame. Integrating at a constant step —
    /// rather than the reference's variable per-frame step — pins the flexi's rest
    /// equilibrium so it neither drifts with the frame rate nor lurches after a hitch
    /// (see `FIXED_TIMESTEP`); each pass runs the faithful `doFlexibleUpdate`
    /// (`integrate`) body. The accumulated backlog is capped at `MAX_TIMESTEP`
    /// (0.2 s) so a long stall cannot spiral into unbounded catch-up, and a
    /// non-positive or `NaN` `dt` is a no-op.
    pub fn step(
        &mut self,
        attributes: &FlexiAttributes,
        object_scale: [f32; 3],
        base_position: [f32; 3],
        base_rotation: [f32; 4],
        dt: f32,
    ) {
        if dt.is_nan() || dt <= 0.0 {
            return;
        }
        // Accumulate the frame's elapsed time, then drain it in fixed steps. The
        // backlog is bounded so a multi-second pause runs at most a dozen catch-up
        // passes (and drops the rest) rather than freezing on a burst of integration.
        self.accumulator = (self.accumulator + dt).min(MAX_TIMESTEP);
        while self.accumulator >= FIXED_TIMESTEP {
            self.integrate(
                attributes,
                object_scale,
                base_position,
                base_rotation,
                FIXED_TIMESTEP,
            );
            self.accumulator -= FIXED_TIMESTEP;
        }
    }

    /// One explicit-Euler integration pass over `dt` seconds — the faithful port of
    /// `doFlexibleUpdate`'s body. Pins the anchor to the prim base, then for each
    /// node integrates gravity, the user force, chain tension toward the parent
    /// direction, and inertia, clamps the bend angle to the per-section maximum, and
    /// re-derives each node's velocity and orientation. [`step`](Self::step) calls
    /// this once per sub-step; `dt` here is already the (sub-)step size.
    fn integrate(
        &mut self,
        attributes: &FlexiAttributes,
        object_scale: [f32; 3],
        base_position: [f32; 3],
        base_rotation: [f32; 4],
        dt: f32,
    ) {
        let num_sections = self.num_sections;
        if self.sections.len() <= num_sections {
            return;
        }

        let scale_z = object_scale.get(2).copied().unwrap_or(1.0);
        let section_length = scale_z / usize_to_f32(num_sections);

        // Anchor node (0): pinned to the prim base each frame.
        let anchor_direction = rotate_vector(base_rotation, [0.0, 0.0, 1.0]);
        let anchor_position = vec_sub(base_position, vec_scale(anchor_direction, scale_z * 0.5));
        if let Some(anchor) = self.sections.first_mut() {
            anchor.position = anchor_position;
            anchor.direction = anchor_direction;
            anchor.rotation = base_rotation;
        }

        // Coefficients constant across sections this frame.
        let mut t_factor = attributes.tension * 0.1;
        t_factor *= 1.0 - TENSION_DECAY_BASE.powf(dt * TENSION_DECAY_RATE);
        let t_factor = t_factor.min(MAX_TENSION_FORCE);

        let friction_coeff = attributes.air_friction * 2.0 + 1.0;
        let friction_coeff = 10.0_f32.powf(friction_coeff * dt).max(1.0);
        let momentum = 1.0 / friction_coeff;

        let max_angle = (section_length * 2.0).atan();
        let force_factor = section_length * dt;

        let mut parent_segment_rotation = base_rotation;

        for i in 1..=num_sections {
            let parent_idx = i.saturating_sub(1);
            let grand_idx = i.saturating_sub(2);
            // Node `i` and its parent are always present (`i <= num_sections` and
            // the chain holds `num_sections + 1` nodes); skip defensively otherwise.
            let (Some(mut cur), Some(parent)) = (
                self.sections.get(i).copied(),
                self.sections.get(parent_idx).copied(),
            ) else {
                continue;
            };
            let parent_position = parent.position;
            let parent_direction = parent.direction;
            // The direction the tension pulls toward: at node 1 it is the anchor's,
            // otherwise the grand-parent's (Firestorm's `parentSectionVector`).
            let parent_section_vector = if i == 1 {
                self.sections
                    .first()
                    .map_or(parent_direction, |s| s.direction)
            } else {
                self.sections
                    .get(grand_idx)
                    .map_or(parent_direction, |s| s.direction)
            };

            let last_position = cur.position;

            // Gravity (world -Z) and the user force.
            cur.position[2] -= attributes.gravity * force_factor;
            cur.position = vec_add(cur.position, vec_scale(attributes.user_force, force_factor));

            // Tension toward the parent's direction: the current node should sit
            // `section_length` along the grand-parent's direction from its parent.
            let current_vector = vec_sub(cur.position, parent_position);
            let difference = vec_sub(
                vec_scale(parent_section_vector, section_length),
                current_vector,
            );
            cur.position = vec_add(cur.position, vec_scale(difference, t_factor));

            // Inertia.
            cur.position = vec_add(cur.position, vec_scale(cur.velocity, momentum));

            // Clamp the bend length & angle: derive the raw direction, find the
            // shortest-arc rotation from the parent direction, clamp it to
            // `max_angle`, and re-place the node exactly `section_length` along the
            // clamped direction so the chain stays rigid-length.
            let raw_direction = normalize(vec_sub(cur.position, parent_position));
            let mut delta_rotation = shortest_arc(parent_direction, raw_direction);
            let (mut angle, axis) = angle_axis(delta_rotation);
            if angle > core::f32::consts::PI {
                angle -= core::f32::consts::TAU;
            }
            if angle < -core::f32::consts::PI {
                angle += core::f32::consts::TAU;
            }
            if angle > max_angle {
                delta_rotation = quat_axis_angle(axis, max_angle);
            } else if angle < -max_angle {
                delta_rotation = quat_axis_angle(axis, -max_angle);
            }

            let segment_rotation = quat_mul(parent_segment_rotation, delta_rotation);
            parent_segment_rotation = segment_rotation;

            let clamped_direction = rotate_vector(delta_rotation, parent_direction);
            cur.direction = clamped_direction;
            cur.position = vec_add(
                parent_position,
                vec_scale(clamped_direction, section_length),
            );
            cur.rotation = segment_rotation;

            // Velocity is the frame's position delta, clamped to unit length.
            let mut velocity = vec_sub(cur.position, last_position);
            if dot(velocity, velocity) > 1.0 {
                velocity = normalize(velocity);
            }
            cur.velocity = velocity;

            // Propagate half the bend up to the parent (smooths the chain). This
            // only affects the parent's render orientation, not the physics, so it
            // is applied after the node is otherwise settled.
            if i > 1 {
                let half = quat_axis_angle(axis, angle * 0.5);
                if let Some(parent_slot) = self.sections.get_mut(parent_idx) {
                    parent_slot.rotation = quat_mul(parent_slot.rotation, half);
                }
            }
            if let Some(slot) = self.sections.get_mut(i) {
                *slot = cur;
            }
        }
    }

    /// Read the current chain out as a deformed extrusion [`Path`], in **metre**
    /// geometry ready for [`tessellate_with_path`](crate::tessellate_with_path).
    ///
    /// Each node's world position is expressed relative to the prim base and
    /// rotated into the prim's local frame (Firestorm's `delta_rot = ~frameRot`
    /// relative transform), giving positions in real metres; the profile scale
    /// bakes the prim's `object_scale` X/Y in metres. So the swept geometry is
    /// already full-size — matching the reference viewer's flexi volume, which
    /// renders with an **identity-scale** relative transform. The viewer therefore
    /// gives a flexi prim an identity geometry holder (like grass), rather than the
    /// per-object scale a rigid prim's holder applies.
    ///
    /// Baking the metre scale into the profile *before* the section rotation (the
    /// way `frame.place` scales then rotates) is what keeps the cross-section
    /// undistorted as the chain bends: a unit-local geometry scaled by a very
    /// non-uniform holder *after* the bend would shear the profile badly (a thin,
    /// long flexi prim — say `0.3 × 0.3 × 4 m` — would balloon into a slab once it
    /// droops), so the metre bake is not merely a fidelity nicety but a correctness
    /// requirement. The path frame's rotation composes the constant twist, the
    /// node's simulated orientation, and the inverse base rotation.
    #[must_use]
    pub fn path(
        &self,
        base_position: [f32; 3],
        base_rotation: [f32; 4],
        object_scale: [f32; 3],
    ) -> Path {
        let delta_rotation = quat_conjugate(base_rotation);
        let scale_x = object_scale.first().copied().unwrap_or(1.0);
        let scale_y = object_scale.get(1).copied().unwrap_or(1.0);
        let count = self.num_sections;
        let inv_count = 1.0 / usize_to_f32(count);

        let mut points = Vec::with_capacity(count.saturating_add(1));
        for (i, section) in self
            .sections
            .iter()
            .enumerate()
            .take(count.saturating_add(1))
        {
            let relative = vec_sub(section.position, base_position);
            // Metre-space local position (no per-axis division): the holder is
            // identity, so this renders at full size.
            let position = rotate_vector(delta_rotation, relative);
            let rotation = quat_mul(
                quat_mul(section.axis_rotation, section.rotation),
                delta_rotation,
            );
            points.push(PathPoint {
                position,
                // Bake the prim's metre X/Y scale into the profile (the taper
                // fraction times the object scale), before the section rotation.
                scale: [section.scale[0] * scale_x, section.scale[1] * scale_y],
                rotation,
                tex_t: usize_to_f32(i) * inv_count,
            });
        }

        let total = points.len();
        Path {
            points,
            // A flexi chain is always an open sweep (a line bent through space).
            open: true,
            total,
        }
    }
}

/// The prim's path begin/end profile scale (Firestorm `LLPathParams::getBeginScale`
/// / `getEndScale`): a top size above 1 tapers the begin, below 1 tapers the end.
fn taper_scales(shape: &PrimShape) -> ([f32; 2], [f32; 2]) {
    let (sx, sy) = (shape.path_scale_x, shape.path_scale_y);
    let begin = [
        if sx > 1.0 { 2.0 - sx } else { 1.0 },
        if sy > 1.0 { 2.0 - sy } else { 1.0 },
    ];
    let end = [
        if sx < 1.0 { sx } else { 1.0 },
        if sy < 1.0 { sy } else { 1.0 },
    ];
    (begin, end)
}

/// Add two 3-vectors.
fn vec_add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Subtract `b` from `a` (3-vectors).
fn vec_sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Scale a 3-vector by a scalar.
fn vec_scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// The dot product of two 3-vectors.
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The cross product of two 3-vectors.
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Normalize a 3-vector; a (near-)zero vector maps to `+Z` (an arbitrary but
/// stable fallback, matching how the chain's rest direction points up).
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len_sq = dot(v, v);
    if len_sq > f32::EPSILON {
        let inv = 1.0 / len_sq.sqrt();
        [v[0] * inv, v[1] * inv, v[2] * inv]
    } else {
        [0.0, 0.0, 1.0]
    }
}

/// The conjugate (inverse, for a unit quaternion) of `(x, y, z, w)`.
fn quat_conjugate(q: [f32; 4]) -> [f32; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

/// The quaternion `(x, y, z, w)` for a rotation of `angle` radians about the unit
/// `axis` (Firestorm `LLQuaternion::setQuat(angle, axis)`). A degenerate axis
/// yields the identity.
fn quat_axis_angle(axis: [f32; 3], angle: f32) -> [f32; 4] {
    let axis = normalize_or_zero(axis);
    let half = angle * 0.5;
    let s = half.sin();
    [axis[0] * s, axis[1] * s, axis[2] * s, half.cos()]
}

/// Normalize a vector, returning the zero vector for a degenerate input (so
/// [`quat_axis_angle`] falls back to the identity rotation).
fn normalize_or_zero(v: [f32; 3]) -> [f32; 3] {
    let len_sq = dot(v, v);
    if len_sq > f32::EPSILON {
        let inv = 1.0 / len_sq.sqrt();
        [v[0] * inv, v[1] * inv, v[2] * inv]
    } else {
        [0.0, 0.0, 0.0]
    }
}

/// The shortest-arc rotation carrying unit vector `from` onto unit vector `to`
/// (Firestorm `LLQuaternion::shortestArc`). Antiparallel inputs pick an arbitrary
/// perpendicular axis for the half turn.
fn shortest_arc(from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    let d = dot(from, to);
    if d >= 1.0 - f32::EPSILON {
        return [0.0, 0.0, 0.0, 1.0];
    }
    if d <= -1.0 + f32::EPSILON {
        // Antiparallel: rotate 180° about any axis perpendicular to `from`.
        let axis = perpendicular(from);
        return [axis[0], axis[1], axis[2], 0.0];
    }
    let c = cross(from, to);
    let q = [c[0], c[1], c[2], 1.0 + d];
    normalize_quat(q)
}

/// Normalize a quaternion; a degenerate one maps to the identity.
fn normalize_quat(q: [f32; 4]) -> [f32; 4] {
    let len_sq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if len_sq > f32::EPSILON {
        let inv = 1.0 / len_sq.sqrt();
        [q[0] * inv, q[1] * inv, q[2] * inv, q[3] * inv]
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// A unit vector perpendicular to `v` (for the antiparallel shortest-arc case):
/// cross `v` with whichever principal axis it is least aligned to.
fn perpendicular(v: [f32; 3]) -> [f32; 3] {
    let axis = if v[0].abs() <= v[1].abs() && v[0].abs() <= v[2].abs() {
        [1.0, 0.0, 0.0]
    } else if v[1].abs() <= v[2].abs() {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    normalize(cross(v, axis))
}

/// The rotation angle (radians) and unit axis of a quaternion (Firestorm
/// `LLQuaternion::getAngleAxis`). A near-identity quaternion returns a zero angle
/// about `+X`.
fn angle_axis(q: [f32; 4]) -> (f32, [f32; 3]) {
    let w = q[3].clamp(-1.0, 1.0);
    let sin_half_sq = 1.0 - w * w;
    if sin_half_sq <= f32::EPSILON {
        return (0.0, [1.0, 0.0, 0.0]);
    }
    let inv = 1.0 / sin_half_sq.sqrt();
    let angle = 2.0 * w.acos();
    (angle, [q[0] * inv, q[1] * inv, q[2] * inv])
}

/// Convert a small section count / index to `f32` (the counts are tiny, so exact).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "value is a tiny section count/index that converts to f32 exactly"
)]
const fn usize_to_f32(value: usize) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    use super::{FlexiAttributes, FlexiChain};
    use crate::shape::PrimShape;
    use pretty_assertions::assert_eq;
    use sl_proto::PrimShapeParams;

    /// A default unit-cylinder flexi prim shape (line path, circle profile).
    fn cylinder_shape() -> PrimShape {
        PrimShape::from_params(&PrimShapeParams {
            path_curve: 0x10,
            profile_curve: 0x00,
            path_begin: 0,
            path_end: 0,
            path_scale_x: 100,
            path_scale_y: 100,
            path_shear_x: 0,
            path_shear_y: 0,
            path_twist: 0,
            path_twist_begin: 0,
            path_radius_offset: 0,
            path_taper_x: 0,
            path_taper_y: 0,
            path_revolutions: 0,
            path_skew: 0,
            profile_begin: 0,
            profile_end: 0,
            profile_hollow: 0,
        })
    }

    /// A mid-range flexi block (softness 2, some tension / gravity, no user force).
    fn attributes() -> FlexiAttributes {
        FlexiAttributes {
            softness: 2,
            tension: 1.0,
            air_friction: 2.0,
            gravity: 0.3,
            wind_sensitivity: 0.0,
            user_force: [0.0, 0.0, 0.0],
        }
    }

    /// Softness `n` yields `1 << n` sections and `1 << n + 1` path points.
    #[test]
    fn section_count_follows_softness() {
        for softness in 0..=3u8 {
            let mut attrs = attributes();
            attrs.softness = softness;
            let chain = FlexiChain::new(
                &cylinder_shape(),
                &attrs,
                [1.0, 1.0, 4.0],
                [0.0, 0.0, 20.0],
                [0.0, 0.0, 0.0, 1.0],
            );
            let expected = 1usize.wrapping_shl(u32::from(softness));
            assert_eq!(chain.num_sections(), expected);
            let path = chain.path([0.0, 0.0, 20.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 4.0]);
            assert_eq!(path.point_count(), expected.saturating_add(1));
        }
    }

    /// The rest chain (before any step) is a straight, full-size **metre** vertical
    /// line: Z runs from `-scaleZ/2` (anchor) to `+scaleZ/2` (tip), X/Y ≈ 0. The
    /// geometry is full-size because the flexi holder is identity (no re-scaling).
    #[test]
    fn rest_path_is_a_metre_vertical_line() {
        let scale = [2.0, 2.0, 8.0];
        let base_pos = [128.0, 128.0, 25.0];
        let base_rot = [0.0, 0.0, 0.0, 1.0];
        let chain = FlexiChain::new(&cylinder_shape(), &attributes(), scale, base_pos, base_rot);
        let path = chain.path(base_pos, base_rot, scale);
        let first = path.points.first().copied().unwrap_or_default();
        let last = path.points.last().copied().unwrap_or_default();
        // Half the 8 m length, either side of the base.
        assert!(
            (first.position[2] - -4.0).abs() < 1.0e-4,
            "anchor Z {first:?}"
        );
        assert!((last.position[2] - 4.0).abs() < 1.0e-4, "tip Z {last:?}");
        for point in &path.points {
            assert!(point.position[0].abs() < 1.0e-4, "X drift {point:?}");
            assert!(point.position[1].abs() < 1.0e-4, "Y drift {point:?}");
        }
        // The profile scale bakes the prim's 2 m X/Y (unit taper × object scale).
        assert!((first.scale[0] - 2.0).abs() < 1.0e-4, "profile X {first:?}");
    }

    /// Gravity bends a **horizontal** chain: a vertical chain under axial gravity
    /// stays straight (the length constraint absorbs it), so the prim is laid on
    /// its side (its local `+Z` points along world `+X`). Gravity then pulls the
    /// far nodes down laterally, deflecting the tip away from its straight rest
    /// pose. The deflection shows in the tip's local position moving off the rest
    /// `[0, 0, 0.5]`.
    #[test]
    fn gravity_droops_a_horizontal_chain() {
        let scale = [1.0, 1.0, 4.0];
        let base_pos = [128.0, 128.0, 30.0];
        // A -90° turn about world Y: the prim's local +Z axis lies along world +X.
        let base_rot = [
            0.0,
            -core::f32::consts::FRAC_1_SQRT_2,
            0.0,
            core::f32::consts::FRAC_1_SQRT_2,
        ];
        let attrs = attributes();
        let mut chain = FlexiChain::new(&cylinder_shape(), &attrs, scale, base_pos, base_rot);
        // Integrate a second of simulation in 1/45 s steps.
        for _ in 0..45 {
            chain.step(&attrs, scale, base_pos, base_rot, 1.0 / 45.0);
        }
        let path = chain.path(base_pos, base_rot, scale);
        let tip = path.points.last().copied().unwrap_or_default();
        // Distance of the tip's local position from the straight rest `[0,0,2]`
        // (metre geometry: half the 4 m length along local Z).
        let deflection = (tip.position[0] * tip.position[0]
            + tip.position[1] * tip.position[1]
            + (tip.position[2] - 2.0) * (tip.position[2] - 2.0))
            .sqrt();
        assert!(
            deflection > 0.05,
            "gravity should bend the horizontal chain, tip {tip:?}"
        );
        // The tip stays finite (the chain does not explode).
        assert!(tip.position.iter().all(|c| c.is_finite()));
    }

    /// A non-positive `dt` is a no-op: the chain does not move.
    #[test]
    fn zero_dt_is_a_noop() {
        let scale = [1.0, 1.0, 4.0];
        let base_pos = [0.0, 0.0, 10.0];
        let base_rot = [0.0, 0.0, 0.0, 1.0];
        let attrs = attributes();
        let mut chain = FlexiChain::new(&cylinder_shape(), &attrs, scale, base_pos, base_rot);
        let before = chain.path(base_pos, base_rot, scale);
        chain.step(&attrs, scale, base_pos, base_rot, 0.0);
        let after = chain.path(base_pos, base_rot, scale);
        for (a, b) in before.points.iter().zip(after.points.iter()) {
            let delta = [
                a.position[0] - b.position[0],
                a.position[1] - b.position[1],
                a.position[2] - b.position[2],
            ];
            assert!(
                delta.iter().all(|c| c.abs() < f32::EPSILON),
                "zero-dt step moved a node: {a:?} vs {b:?}"
            );
        }
    }

    /// The fixed-timestep accumulator makes **frame splitting irrelevant**: one big
    /// frame integrates the same chain as several smaller frames summing to it. This
    /// is the property that keeps a hitch (one huge frame) equivalent to the many
    /// normal frames it replaced, so a settled chain cannot lurch. Uses a total of
    /// `0.18 s` — safely between the 10th and 11th `FIXED_TIMESTEP` boundary (10.8
    /// steps) — so float rounding cannot make the two paths drain a different number
    /// of fixed steps.
    #[test]
    fn a_split_frame_equals_one_whole_frame() {
        let scale = [1.0, 1.0, 4.0];
        let base_pos = [128.0, 128.0, 30.0];
        // Laid on its side (local +Z along world +X) so gravity actually bends it —
        // a straight-up chain would just settle axially and mask any divergence.
        let base_rot = [
            0.0,
            -core::f32::consts::FRAC_1_SQRT_2,
            0.0,
            core::f32::consts::FRAC_1_SQRT_2,
        ];
        let mut attrs = attributes();
        attrs.user_force = [0.0, 0.5, 0.0];

        // One 0.18 s frame (a hitch), against eighteen 0.01 s frames (normal play).
        let mut whole = FlexiChain::new(&cylinder_shape(), &attrs, scale, base_pos, base_rot);
        whole.step(&attrs, scale, base_pos, base_rot, 0.18);

        let mut split = FlexiChain::new(&cylinder_shape(), &attrs, scale, base_pos, base_rot);
        for _ in 0..18 {
            split.step(&attrs, scale, base_pos, base_rot, 0.01);
        }

        let whole_path = whole.path(base_pos, base_rot, scale);
        let split_path = split.path(base_pos, base_rot, scale);
        for (w, s) in whole_path.points.iter().zip(split_path.points.iter()) {
            for (wp, sp) in w.position.iter().zip(s.position.iter()) {
                assert!(
                    (wp - sp).abs() < 1.0e-6,
                    "the hitch frame diverged from the frames it replaced: {w:?} vs {s:?}"
                );
            }
        }
    }

    /// The bug (`viewer-flexi-resettle-after-snapshot`): a **settled** chain must not
    /// lurch when a single hitch frame arrives. A settled chain is handed a `0.2 s`
    /// frame two ways — the accumulating [`FlexiChain::step`] and a single raw
    /// [`FlexiChain::integrate`] pass (the reference's one-shot behaviour) — and the
    /// accumulated chain moves far less: it stays on its fixed-timestep equilibrium
    /// while the one-shot pass visibly swings out.
    #[test]
    fn a_settled_chain_survives_a_frame_spike() {
        let scale = [1.0, 1.0, 4.0];
        let base_pos = [128.0, 128.0, 30.0];
        let base_rot = [
            0.0,
            -core::f32::consts::FRAC_1_SQRT_2,
            0.0,
            core::f32::consts::FRAC_1_SQRT_2,
        ];
        // A gentle, near-straight chain: its rest bend sits below the angle clamp, so
        // its equilibrium is genuinely step-size dependent (a chain forced hard into
        // the clamp is pinned rigid and a big step cannot move it, hiding the effect).
        let mut attrs = attributes();
        attrs.user_force = [0.0, 0.5, 0.0];

        // Settle through `step` so the rest pose is the fixed-timestep equilibrium —
        // the state the live viewer sits in for minutes, regardless of frame rate.
        let mut chain = FlexiChain::new(&cylinder_shape(), &attrs, scale, base_pos, base_rot);
        let frame = 1.0 / 60.0;
        for _ in 0..600 {
            chain.step(&attrs, scale, base_pos, base_rot, frame);
        }
        let settled_tip = chain
            .path(base_pos, base_rot, scale)
            .points
            .last()
            .copied()
            .unwrap_or_default()
            .position;

        // Distance the tip moves from that rest pose under a 0.2 s hitch frame.
        let displacement = |mut c: FlexiChain, one_shot: bool| -> f32 {
            if one_shot {
                c.integrate(&attrs, scale, base_pos, base_rot, 0.2);
            } else {
                c.step(&attrs, scale, base_pos, base_rot, 0.2);
            }
            let tip = c
                .path(base_pos, base_rot, scale)
                .points
                .last()
                .copied()
                .unwrap_or_default()
                .position;
            ((tip[0] - settled_tip[0]).powi(2)
                + (tip[1] - settled_tip[1]).powi(2)
                + (tip[2] - settled_tip[2]).powi(2))
            .sqrt()
        };

        let one_shot = displacement(chain.clone(), true);
        let accumulated = displacement(chain, false);
        // The one-shot pass shoves the tip a visible distance off its rest pose (the
        // swing the user sees); the accumulator keeps it an order of magnitude closer.
        assert!(
            one_shot > 0.005,
            "the one-shot 0.2 s pass should lurch a settled chain (moved {one_shot})"
        );
        assert!(
            accumulated < one_shot * 0.2,
            "the fixed-timestep accumulator should keep the settled chain near rest \
             (accumulated {accumulated} vs one-shot {one_shot})"
        );
    }
}
