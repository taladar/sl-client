//! Procedural `LLVOGrass` crossed-quad geometry, ported from Firestorm's
//! `LLVOGrass::getGeometry` / `LLVOGrass::initClass`.
//!
//! A grass object (`PCODE_GRASS`) is *not a blade but a clump of grass*: a fan of
//! up to [`GRASS_MAX_BLADES`] textured quad *cards*, each a leaning two-sided
//! blade, distributed with a Gaussian scatter around the object centre. Like the
//! [`crate::geometry`] tree path there is no fetched geometry asset — the clump is
//! generated on the CPU from the object's [`GrassSpecies`] (its blade card size)
//! and the object's scale (which spreads the blade centres).
//!
//! The output is deliberately **Bevy-free**: a [`GrassMesh`] of plain
//! position / normal / uv / index buffers in Second Life's right-handed **Z-up**
//! space, in **absolute metres** — unlike the tree path the object scale is baked
//! into the blade-centre spread here (the reference's `x = exp_x * mScale`), so
//! the caller applies only the object's position / rotation at the transform
//! boundary, **not** a further scale.
//!
//! # Faithful simplifications
//!
//! - **Terrain conformance.** The reference plants each blade base at the *terrain
//!   height* under its scattered `(x, y)` (`resolveHeightRegion`) and the object's
//!   own Z is ignored. This crate is I/O-free and has no heightfield, so every
//!   blade base sits on the object's local `z = 0` plane. A grass patch spans only
//!   a fraction of a metre (a Gaussian SD of `0.15 · scale`), where terrain is
//!   near flat, so a single ground plane is a close approximation.
//! - **Blade layout randomness.** The reference fills its scatter tables once, at
//!   `initClass`, from `ll_frand()` — a PRNG seeded from a *random* UUID, so the
//!   layout differs every viewer run and is shared by every grass object in the
//!   scene. We reproduce the same statistical distribution from a *fixed*-seed
//!   PRNG, so the clump is stable across runs (and, as in the reference, shared by
//!   every grass patch), only the exact blade placement differs from any one
//!   reference run.
//! - **Wind.** The reference's per-blade wind sway (`w_mod` also modulates a
//!   time-varying bend) is not simulated; `w_mod` is applied only as its static
//!   size/spread modulation.

use crate::species::GrassSpecies;

/// Maximum number of blades in a grass clump (`GRASS_MAX_BLADES`). Full detail;
/// the reference viewer sheds blades with distance for performance.
pub const GRASS_MAX_BLADES: usize = 32;

/// Width of a blade at its base, before the species / `w_mod` scaling
/// (`GRASS_BLADE_BASE`), in metres.
const GRASS_BLADE_BASE: f32 = 0.25;

/// Height of a blade before the species / `w_mod` scaling (`GRASS_BLADE_HEIGHT`),
/// in metres.
const GRASS_BLADE_HEIGHT: f32 = 0.5;

/// Standard deviation of the Gaussian blade-centre scatter (`GRASS_DISTRIBUTION_SD`),
/// as a fraction of the object scale.
const GRASS_DISTRIBUTION_SD: f32 = 0.15;

/// Fixed forced Z component of a blade's front normal (`normal1.mV[VZ] = 0.75`),
/// which tilts the blade cards' shading upward before normalisation.
const BLADE_NORMAL_Z: f32 = 0.75;

/// A generated grass clump mesh: a single `TriangleList` in Second Life's
/// right-handed Z-up space, in absolute metres (object scale already folded into
/// the blade-centre spread), ready to be bridged to a renderer's mesh type.
///
/// `positions`, `normals` and `uvs` are parallel (one entry per vertex); `indices`
/// reference them three per triangle. As in the reference each blade contributes 8
/// vertices (its 4 corners duplicated, front copies carrying the up-tilted normal
/// and back copies its mirror) and 12 indices (a two-sided quad).
#[derive(Debug, Clone, Default, PartialEq)]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `GrassMesh` reads clearly"
)]
pub struct GrassMesh {
    /// Vertex positions (metres, object-local Z-up).
    pub positions: Vec<[f32; 3]>,
    /// Per-vertex normals (unit length).
    pub normals: Vec<[f32; 3]>,
    /// Per-vertex texture coordinates into the species texture.
    pub uvs: Vec<[f32; 2]>,
    /// Triangle indices into the vertex buffers.
    pub indices: Vec<u32>,
}

/// One blade's entry in the shared scatter tables (`LLVOGrass::exp_x` … `w_mod`):
/// its Gaussian centre offset, its in-plane orientation, its per-blade top skew,
/// and its size/spread modulation.
#[derive(Debug, Clone, Copy)]
struct Blade {
    /// Gaussian centre offset in X, as a fraction of object scale (`exp_x`).
    exp_x: f32,
    /// Gaussian centre offset in Y, as a fraction of object scale (`exp_y`).
    exp_y: f32,
    /// `sin(rot)` of the blade's in-plane orientation (`rot_x`).
    rot_x: f32,
    /// `cos(rot)` of the blade's in-plane orientation (`rot_y`).
    rot_y: f32,
    /// Per-blade top skew in X (`dz_x`), in metres.
    dz_x: f32,
    /// Per-blade top skew in Y (`dz_y`), in metres.
    dz_y: f32,
    /// Per-blade size / spread modulation (`w_mod`), in `0.5..1.5`.
    w_mod: f32,
}

/// A tiny fixed-seed SplitMix64 PRNG, used to fill the blade scatter tables
/// deterministically (see the module's *blade layout randomness* note).
struct SplitMix64(u64);

impl SplitMix64 {
    /// Draw the next `u64` from the stream.
    const fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Draw the next float in `[0, 1)` (24-bit mantissa), the counterpart of the
    /// reference's `ll_frand()`.
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "the top 24 bits are exactly representable as f32, giving [0, 1)"
    )]
    fn frand(&mut self) -> f32 {
        // Take the top 24 bits so the result lands exactly in [0, 1); 2^24 =
        // 16_777_216 is exactly representable, so the quotient is in [0, 1).
        let numerator = (self.next_u64() >> 40) as f32;
        numerator / 16_777_216.0
    }
}

/// The seed for the blade scatter PRNG. Arbitrary but fixed, so the clump layout
/// is stable across runs (the reference uses a random per-run seed).
const BLADE_SEED: u64 = 0x5109_6A55_C0DE_0001;

/// Generate the shared blade scatter tables, reproducing `LLVOGrass::initClass`'s
/// distribution: a Gaussian `(exp_x, exp_y)` centre offset (Box–Muller, SD
/// [`GRASS_DISTRIBUTION_SD`]), a uniform in-plane rotation, a small uniform top
/// skew, and a `0.5..1.5` size modulation, per blade.
fn blade_layout() -> [Blade; GRASS_MAX_BLADES] {
    let mut rng = SplitMix64(BLADE_SEED);
    // A default-filled array we overwrite; the initialiser value is never read.
    let mut blades = [Blade {
        exp_x: 0.0,
        exp_y: 0.0,
        rot_x: 0.0,
        rot_y: 1.0,
        dz_x: 0.0,
        dz_y: 0.0,
        w_mod: 1.0,
    }; GRASS_MAX_BLADES];
    for blade in &mut blades {
        // Box–Muller: `u = sqrt(-2 ln r1)`, `v = 2π r2`. Guard `r1` off zero so the
        // log stays finite (the reference's ll_frand() can, rarely, return 0).
        let r1 = rng.frand().max(f32::MIN_POSITIVE);
        let u = (-2.0 * r1.ln()).sqrt();
        let v = 2.0 * std::f32::consts::PI * rng.frand();
        let rot = rng.frand() * std::f32::consts::PI;
        *blade = Blade {
            exp_x: u * v.sin() * GRASS_DISTRIBUTION_SD,
            exp_y: u * v.cos() * GRASS_DISTRIBUTION_SD,
            rot_x: rot.sin(),
            rot_y: rot.cos(),
            dz_x: rng.frand() * (GRASS_BLADE_BASE * 0.25),
            dz_y: rng.frand() * (GRASS_BLADE_BASE * 0.25),
            w_mod: 0.5 + rng.frand(),
        };
    }
    blades
}

/// Generate the crossed-quad geometry for a grass clump of `species`, spreading up
/// to `num_blades` (clamped to [`GRASS_MAX_BLADES`]) blades over an area set by the
/// object's `scale_x` / `scale_y`.
///
/// Ports `LLVOGrass::getGeometry`: each blade is a leaning quad card with the
/// species blade size, textured across its full `u ∈ [0, 1]` width and `v ∈ [0,
/// 0.98]` height, and emitted two-sided (front and back copies with opposite
/// normals). The clump is in absolute metres in object-local Z-up space with blade
/// bases on the `z = 0` plane; see the module docs for the terrain / wind
/// simplifications.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `grass_geometry` reads clearly"
)]
pub fn grass_geometry(
    species: &GrassSpecies,
    scale_x: f32,
    scale_y: f32,
    num_blades: usize,
) -> GrassMesh {
    let blades = blade_layout();
    let count = num_blades.min(GRASS_MAX_BLADES);
    let width = species.blade_size_x;
    let height = species.blade_size_y;
    let mut mesh = GrassMesh::default();
    for blade in blades.iter().take(count) {
        // Blade centre (object scale folded in) and half-width offset (species
        // blade size, not object scale — the card keeps its real-world size).
        let x = blade.exp_x * scale_x;
        let y = blade.exp_y * scale_y;
        let xf = blade.rot_x * GRASS_BLADE_BASE * width * blade.w_mod;
        let yf = blade.rot_y * GRASS_BLADE_BASE * width * blade.w_mod;
        let dzx = blade.dz_x;
        let dzy = blade.dz_y;
        let blade_height = GRASS_BLADE_HEIGHT * height * blade.w_mod;

        // The four corners, base plane at z = 0 (see terrain note in module docs).
        let base1 = [x + xf, y + yf, 0.0];
        let top1 = [base1[0] + dzx, base1[1] + dzy, blade_height];
        // NB: base-2's Y uses `- xf`, not `- yf` — a long-standing quirk of
        // `LLVOGrass::getGeometry`, reproduced verbatim so each card leans exactly
        // as the reference viewer's does.
        let base2 = [x - xf, y - xf, 0.0];
        let top2 = [base2[0] + dzx, base2[1] + dzy, blade_height];

        // Front normal: (base1-top1) × (top1-base2), Z forced up, normalised. Back
        // normal mirrors X/Y but keeps the same Z sign (`normal2 = -normal1` then
        // `normal2.z = -normal2.z`).
        let mut front = cross(sub(base1, top1), sub(top1, base2));
        front[2] = BLADE_NORMAL_Z;
        let front = normalize(front);
        let back = [-front[0], -front[1], front[2]];

        let index_base = u32_from_usize(mesh.positions.len());
        // Each corner is emitted twice: an even (front, `front` normal) then an odd
        // (back, `back` normal) vertex, matching the reference's interleaving.
        for (corner, uv) in [
            (base1, [0.0, 0.0]),
            (top1, [0.0, 0.98]),
            (base2, [1.0, 0.0]),
            (top2, [1.0, 0.98]),
        ] {
            mesh.positions.push(corner);
            mesh.normals.push(front);
            mesh.uvs.push(uv);
            mesh.positions.push(corner);
            mesh.normals.push(back);
            mesh.uvs.push(uv);
        }
        // Two front triangles over the even vertices, two back over the odd ones.
        for offset in [0, 2, 4, 2, 6, 4, 1, 5, 3, 3, 5, 7] {
            mesh.indices.push(index_base.saturating_add(offset));
        }
    }
    mesh
}

/// Vector difference `a - b`.
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// Cross product `a × b`.
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Normalize a vector, returning the input unchanged if it is (near) zero length.
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= f32::EPSILON {
        v
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

/// A vertex count as a `u32` index base (grass vertex counts never approach
/// `u32::MAX`: at most `GRASS_MAX_BLADES * 8 = 256`).
fn u32_from_usize(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{GRASS_MAX_BLADES, blade_layout, grass_geometry};
    use crate::species::grass_species;
    use pretty_assertions::assert_eq;

    #[test]
    fn blade_layout_is_deterministic() {
        let first = blade_layout();
        let second = blade_layout();
        for (a, b) in first.iter().zip(second.iter()) {
            // Compare the exact bit patterns: a fixed-seed PRNG must reproduce the
            // same layout every call.
            assert_eq!(a.exp_x.to_bits(), b.exp_x.to_bits());
            assert_eq!(a.w_mod.to_bits(), b.w_mod.to_bits());
        }
    }

    #[test]
    fn blade_layout_stats_match_reference_distribution() {
        let blades = blade_layout();
        for blade in &blades {
            // rot_x / rot_y are a sine / cosine pair (unit length).
            let r = blade.rot_x.hypot(blade.rot_y);
            assert!((r - 1.0).abs() < 1e-4, "rot not unit: {r}");
            // w_mod is 0.5 + frand() ∈ [0.5, 1.5).
            assert!(
                (0.5..1.5).contains(&blade.w_mod),
                "w_mod out of range: {}",
                blade.w_mod
            );
            // dz skews are a small uniform 0..GRASS_BLADE_BASE*0.25 = 0.0625.
            assert!((0.0..=0.0625).contains(&blade.dz_x));
            assert!((0.0..=0.0625).contains(&blade.dz_y));
        }
    }

    #[test]
    fn geometry_vertex_and_index_counts() {
        let Some(species) = grass_species(0) else {
            return; // species 0 is always defined
        };
        let mesh = grass_geometry(species, 1.0, 1.0, GRASS_MAX_BLADES);
        // 8 vertices and 12 indices per blade.
        assert_eq!(mesh.positions.len(), GRASS_MAX_BLADES * 8);
        assert_eq!(mesh.normals.len(), GRASS_MAX_BLADES * 8);
        assert_eq!(mesh.uvs.len(), GRASS_MAX_BLADES * 8);
        assert_eq!(mesh.indices.len(), GRASS_MAX_BLADES * 12);
        // Every index is in range.
        let vertices = u32::try_from(mesh.positions.len()).unwrap_or(u32::MAX);
        assert!(mesh.indices.iter().all(|&i| i < vertices));
    }

    #[test]
    fn num_blades_clamps_and_limits_geometry() {
        let Some(species) = grass_species(0) else {
            return; // species 0 is always defined
        };
        let few = grass_geometry(species, 1.0, 1.0, 4);
        assert_eq!(few.positions.len(), 4 * 8);
        // Over-max clamps to GRASS_MAX_BLADES.
        let many = grass_geometry(species, 1.0, 1.0, 1000);
        assert_eq!(many.positions.len(), GRASS_MAX_BLADES * 8);
    }

    #[test]
    fn scale_spreads_blade_centres_but_not_card_size() {
        let Some(species) = grass_species(0) else {
            return; // species 0 is always defined
        };
        let small = grass_geometry(species, 1.0, 1.0, GRASS_MAX_BLADES);
        let wide = grass_geometry(species, 10.0, 10.0, GRASS_MAX_BLADES);
        // A wider object scale spreads the clump further in X/Y.
        let span = |mesh: &super::GrassMesh| {
            let (mut min, mut max) = (f32::MAX, f32::MIN);
            for p in &mesh.positions {
                min = min.min(p[0]);
                max = max.max(p[0]);
            }
            max - min
        };
        assert!(
            span(&wide) > span(&small),
            "10x scale should spread wider: {} vs {}",
            span(&wide),
            span(&small)
        );
        // Blade height (Z extent) is set by the species card size, not object
        // scale, so it is identical at both scales (compare exact bit patterns).
        let height =
            |mesh: &super::GrassMesh| mesh.positions.iter().map(|p| p[2]).fold(f32::MIN, f32::max);
        assert_eq!(height(&small).to_bits(), height(&wide).to_bits());
    }
}
