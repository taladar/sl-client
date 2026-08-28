//! Pure terrain texture-splat blend-weight math for Second Life / OpenSim
//! clients — the height-blended shading counterpart of `sl-prim` and `sl-mesh`.
//!
//! See the crate `README.md` for an overview. A region ships four ground
//! ("detail") textures plus, for each of its four corners, a *start height* and
//! a *height range*; the ground texture at any point is chosen by elevation,
//! blending between the four textures as the ground rises through the
//! per-corner altitude bands, with a Perlin-noise transition band so the
//! boundaries wobble naturally.
//!
//! This crate is deliberately **Bevy-free and I/O-free**. It bilinearly
//! interpolates the per-corner parameters across the region, scales an
//! elevation-plus-noise value into the `[0, 3]` detail-texture index range, and
//! resolves that scalar into a normalised four-component blend weight — one
//! weight per detail texture. The GPU side (a material that samples the four
//! textures and blends them by these per-vertex weights) lives in
//! `sl-client-bevy`.
//!
//! The algorithm mirrors Firestorm's
//! `indra/newview/llvlcomposition.cpp::LLVLComposition::generateHeights` and
//! its terrain shaders, reimplemented idiomatically rather than copied — with
//! one deliberate exception. The Perlin noise itself is a *verbatim*
//! port, tables and all: its exact values decide where one ground texture gives
//! way to the next, so an idiomatic substitute would put every transition band
//! somewhere the region's map tile and every other viewer do not.

mod noise;

use noise::{noise2, turbulence2};

/// The number of detail (ground) textures a region blends between.
pub const DETAIL_COUNT: usize = 4;

/// The highest valid fractional detail-texture index; a region blends across
/// `[0, HIGHEST_INDEX]`, i.e. between detail texture `0` (lowest ground) and
/// detail texture `3` (highest ground).
const HIGHEST_INDEX: f32 = 3.0;

/// The detail-texture count as a float, the span the elevation band is scaled
/// into (Firestorm's `ASSET_COUNT`).
const DETAIL_SPAN: f32 = 4.0;

/// The horizontal noise-lattice scale inverse (Firestorm `1 / xyScale`,
/// `xyScale == 4.9215`): global metres are divided by `xyScale` before being
/// sampled as noise, so the transition band repeats on a ~5 m lattice.
const XY_SCALE_INV: f32 = 1.0 / 4.9215;

/// The scale applied to the low-frequency noise component before sampling
/// (Firestorm `0.2222222222`), giving it a ~22 m period versus the high-frequency
/// turbulence.
///
/// This is every digit of the reference's literal that an `f32` can tell apart,
/// and it matters: a region's global coordinates are tens of thousands of metres,
/// so rounding the constant one digit *short* of that shifts the noise lattice by
/// about a thousandth of a cell — and the transition bands visibly with it. It
/// was the last divergence to fall, after the raw noise already matched.
const LOW_FREQUENCY_SCALE: f32 = 0.222_222_22;

/// The amplitude of the low-frequency noise component (Firestorm `6.5`).
const LOW_FREQUENCY_AMPLITUDE: f32 = 6.5;

/// The amplitude of the high-frequency turbulence component (Firestorm
/// `slope_squared == 1.5 * 1.5`).
const TURBULENCE_AMPLITUDE: f32 = 2.25;

/// The overall degree to which noise modulates the elevation band (Firestorm
/// `noise_magnitude`), applied to the summed low-frequency and turbulence
/// components.
const NOISE_MAGNITUDE: f32 = 2.0;

/// The starting octave frequency of the turbulence sum (Firestorm
/// `turbulence2(vec, 2)`): octaves are summed at frequency `2` then `1`.
const TURBULENCE_FREQUENCY: f32 = 2.0;

/// The smallest height range used, guarding the elevation-band division against
/// a zero or negative range (which would otherwise yield a non-finite value).
const MIN_HEIGHT_RANGE: f32 = 1.0e-3;

/// A region's terrain-compositing parameters: the four per-corner start heights
/// and height ranges, the region edge length, and the region's global
/// south-west origin (used so the noise is continuous across region borders).
///
/// The corner arrays are ordered to match the `RegionHandshake` wire fields
/// `terrain_start_height00 / 01 / 10 / 11` (and the matching `height_range`
/// fields): index `0` is the `00` corner, `1` the `01`, `2` the `10`, and `3`
/// the `11` — which Firestorm reads as south-west / south-east / north-west /
/// north-east respectively.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerrainComposition {
    /// The four per-corner start heights (`terrain_start_height00 / 01 / 10 /
    /// 11`), the elevation at which each corner's blend begins.
    pub start_heights: [f32; DETAIL_COUNT],
    /// The four per-corner height ranges (`terrain_height_range00 / 01 / 10 /
    /// 11`), the elevation span over which each corner blends through all four
    /// detail textures.
    pub height_ranges: [f32; DETAIL_COUNT],
    /// The region edge length in metres (256 for a standard region).
    pub region_size: f32,
    /// The region's global south-west corner in metres (`[x, y]`), derived from
    /// the region handle; makes the noise continuous across region borders.
    pub global_origin: [f32; 2],
}

impl TerrainComposition {
    /// Build a [`TerrainComposition`] from the per-corner parameters, the region
    /// edge length, and the region's global south-west origin in metres.
    #[must_use]
    pub const fn new(
        start_heights: [f32; DETAIL_COUNT],
        height_ranges: [f32; DETAIL_COUNT],
        region_size: f32,
        global_origin: [f32; 2],
    ) -> Self {
        Self {
            start_heights,
            height_ranges,
            region_size,
            global_origin,
        }
    }

    /// The fractional detail-texture index in `[0, 3]` at a region-local ground
    /// point (`local_x`, `local_y` in metres, `elevation` in metres): the
    /// elevation, offset by the bilinearly-interpolated per-corner start height
    /// and a Perlin-noise transition term, scaled by the interpolated height
    /// range. `0` selects the lowest-ground detail texture, `3` the highest.
    #[must_use]
    pub fn composition_value(&self, local_x: f32, local_y: f32, elevation: f32) -> f32 {
        let region_size = if self.region_size > 0.0 {
            self.region_size
        } else {
            256.0
        };
        let x_frac = (local_x / region_size).clamp(0.0, 1.0);
        let y_frac = (local_y / region_size).clamp(0.0, 1.0);
        let start_height = bilinear(self.start_heights, x_frac, y_frac);
        let height_range = bilinear(self.height_ranges, x_frac, y_frac).max(MIN_HEIGHT_RANGE);

        let [origin_x, origin_y] = self.global_origin;
        let sample_x = (origin_x + local_x) * XY_SCALE_INV;
        let sample_y = (origin_y + local_y) * XY_SCALE_INV;

        // Low-frequency component for large divisions, plus high-frequency
        // turbulence, scaled by the overall noise magnitude.
        let low = noise2(
            sample_x * LOW_FREQUENCY_SCALE,
            sample_y * LOW_FREQUENCY_SCALE,
        ) * LOW_FREQUENCY_AMPLITUDE;
        let high = turbulence2(sample_x, sample_y, TURBULENCE_FREQUENCY) * TURBULENCE_AMPLITUDE;
        let twiddle = (low + high) * NOISE_MAGNITUDE;

        let scaled = (elevation + twiddle - start_height) * DETAIL_SPAN / height_range;
        scaled.clamp(0.0, HIGHEST_INDEX)
    }

    /// The normalised four-component detail-texture blend weight at a
    /// region-local ground point (see [`Self::composition_value`]). The four
    /// weights sum to one; at most two adjacent weights are non-zero, giving a
    /// linear blend between the two detail textures bracketing the point's
    /// elevation band.
    #[must_use]
    pub fn blend_weights(&self, local_x: f32, local_y: f32, elevation: f32) -> [f32; DETAIL_COUNT] {
        detail_blend_weights(self.composition_value(local_x, local_y, elevation))
    }
}

/// Resolve a fractional detail-texture index in `[0, 3]` into a normalised
/// four-component blend weight. Each weight is a "tent" over its own index that
/// falls linearly to zero one index away, so the four weights partition unity
/// across `[0, 3]` and only the two textures bracketing the value contribute.
#[must_use]
pub fn detail_blend_weights(value: f32) -> [f32; DETAIL_COUNT] {
    let clamped = value.clamp(0.0, HIGHEST_INDEX);
    [
        (1.0 - clamped).clamp(0.0, 1.0),
        (1.0 - (clamped - 1.0).abs()).clamp(0.0, 1.0),
        (1.0 - (clamped - 2.0).abs()).clamp(0.0, 1.0),
        (clamped - 2.0).clamp(0.0, 1.0),
    ]
}

/// Bilinearly interpolate the four per-corner values at region fractions
/// (`x_frac`, `y_frac`) in `[0, 1]`.
///
/// The corners are ordered `[00, 01, 10, 11]`, which Firestorm's
/// `LLVLComposition::generateHeights` treats as south-west, south-east,
/// north-west, north-east and interpolates as
/// `bilinear(SW, SE, NW, NE, x_frac, y_frac)`.
fn bilinear(corners: [f32; DETAIL_COUNT], x_frac: f32, y_frac: f32) -> f32 {
    let [south_west, south_east, north_west, north_east] = corners;
    let inv_x = 1.0 - x_frac;
    let inv_y = 1.0 - y_frac;
    inv_x * inv_y * south_west
        + x_frac * inv_y * north_west
        + inv_x * y_frac * south_east
        + x_frac * y_frac * north_east
}

#[cfg(test)]
mod tests {
    use super::noise::{noise2, turbulence2};
    use super::{DETAIL_COUNT, TURBULENCE_FREQUENCY, TerrainComposition, detail_blend_weights};
    use pretty_assertions::assert_eq;

    /// The absolute tolerance for float comparisons in these tests.
    const EPSILON: f32 = 1.0e-5;

    /// Assert that two four-component weights match within [`EPSILON`], avoiding
    /// the exact-float comparison `assert_eq!` would perform.
    fn assert_weights(actual: [f32; DETAIL_COUNT], expected: [f32; DETAIL_COUNT]) {
        for (a, e) in actual.iter().zip(expected.iter()) {
            assert!(
                (a - e).abs() < EPSILON,
                "weights {actual:?} differ from expected {expected:?}"
            );
        }
    }

    /// The four blend weights always sum to one across the whole `[0, 3]` range.
    #[test]
    fn blend_weights_partition_unity() {
        let mut value = 0.0_f32;
        while value <= 3.0 {
            let weights = detail_blend_weights(value);
            let sum: f32 = weights.iter().sum();
            assert!(
                (sum - 1.0).abs() < EPSILON,
                "weights {weights:?} at {value} sum to {sum}"
            );
            value += 0.05;
        }
    }

    /// At an integer index exactly one texture is selected, at full weight.
    #[test]
    fn blend_weights_are_pure_at_integer_indices() {
        assert_weights(detail_blend_weights(0.0), [1.0, 0.0, 0.0, 0.0]);
        assert_weights(detail_blend_weights(1.0), [0.0, 1.0, 0.0, 0.0]);
        assert_weights(detail_blend_weights(2.0), [0.0, 0.0, 1.0, 0.0]);
        assert_weights(detail_blend_weights(3.0), [0.0, 0.0, 0.0, 1.0]);
    }

    /// Halfway between two indices the two adjacent textures share the weight
    /// evenly and the others are zero.
    #[test]
    fn blend_weights_split_evenly_between_adjacent() {
        assert_weights(detail_blend_weights(0.5), [0.5, 0.5, 0.0, 0.0]);
        assert_weights(detail_blend_weights(1.5), [0.0, 0.5, 0.5, 0.0]);
        assert_weights(detail_blend_weights(2.5), [0.0, 0.0, 0.5, 0.5]);
    }

    /// Out-of-range values clamp to the end textures rather than producing
    /// negative or non-normalised weights.
    #[test]
    fn blend_weights_clamp_out_of_range() {
        assert_weights(detail_blend_weights(-1.0), [1.0, 0.0, 0.0, 0.0]);
        assert_weights(detail_blend_weights(9.0), [0.0, 0.0, 0.0, 1.0]);
    }

    /// A uniform composition (equal corners) with no noise contribution rises
    /// monotonically through the four textures as the ground rises: low ground
    /// selects texture 0, high ground selects texture 3.
    #[test]
    fn higher_ground_selects_higher_detail_texture() {
        let comp = TerrainComposition::new([10.0; 4], [40.0; 4], 256.0, [256_000.0, 256_000.0]);
        let low = comp.composition_value(128.0, 128.0, 8.0);
        let high = comp.composition_value(128.0, 128.0, 60.0);
        assert!(
            high > low,
            "higher ground {high} should exceed lower ground {low}"
        );
        // Ground well below the start height pins to the lowest texture.
        assert_weights(
            comp.blend_weights(128.0, 128.0, -50.0),
            [1.0, 0.0, 0.0, 0.0],
        );
        // Ground well above start + range pins to the highest texture.
        assert_weights(
            comp.blend_weights(128.0, 128.0, 500.0),
            [0.0, 0.0, 0.0, 1.0],
        );
    }

    /// A composition with differing corners bilinearly interpolates the start
    /// height: the same elevation lands in a different band at opposite corners.
    #[test]
    fn corners_interpolate_the_band() {
        // South-west corner (index 0) starts blending at 0 m; north-east
        // (index 3) not until 100 m. At 50 m the SW corner is already high in
        // its band while the NE corner is still at the lowest texture.
        let comp = TerrainComposition::new(
            [0.0, 0.0, 0.0, 100.0],
            [40.0, 40.0, 40.0, 40.0],
            256.0,
            [0.0, 0.0],
        );
        let sw = comp.composition_value(0.0, 0.0, 50.0);
        let ne = comp.composition_value(256.0, 256.0, 50.0);
        assert!(sw > ne, "SW band value {sw} should exceed NE {ne}");
    }

    /// The Perlin noise is deterministic and bounded, and varies with position.
    #[test]
    fn perlin_is_deterministic_and_bounded() {
        let a = noise2(12.3, 45.6);
        let b = noise2(12.3, 45.6);
        assert_eq!(a.to_bits(), b.to_bits());
        assert!(a.abs() <= 1.5, "noise {a} out of expected bound");
        // At least one nearby sample differs, so the field is not constant.
        let differs = (0_u8..8).any(|i| {
            let offset = f32::from(i) * 0.37;
            (noise2(12.3 + offset, 45.6) - a).abs() > EPSILON
        });
        assert!(differs, "noise field appears constant");
    }

    /// Assert a value matches a reference-viewer sample within [`EPSILON`].
    fn assert_reference(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "{what}: {actual} differs from the reference's {expected}"
        );
    }

    /// [`noise2`] reproduces the reference viewer's `noise2` value for value.
    ///
    /// These are not self-generated regression values: each was produced by
    /// compiling a verbatim extract of Firestorm's `indra/newview/noise.{h,cpp}`
    /// (its tables plus `noise2` / `turbulence2`) and printing the result. They
    /// are what pins our port to the reference rather than to itself — the whole
    /// reason for reproducing its gradient tables instead of substituting a
    /// modern Perlin.
    #[test]
    fn noise_matches_reference_viewer_samples() {
        // Classic Perlin is zero at every integer lattice point, in both
        // implementations — the cheapest check that the lattice lines up.
        assert_reference(noise2(0.0, 0.0), 0.0, "noise2 at the origin");
        assert_reference(noise2(1000.0, 2000.0), 0.0, "noise2 at a lattice point");

        assert_reference(noise2(0.5, 2.5), 0.246_166_65, "noise2 mid-cell");
        assert_reference(noise2(1.25, -7.5), 0.323_311_4, "noise2 negative y");
        assert_reference(noise2(-3.75, 0.25), -0.131_473_93, "noise2 negative x");
        assert_reference(noise2(17.125, 33.75), 0.003_635_108_5, "noise2 mid-range");
        // Region-scale global coordinates, where the lattice index wraps.
        assert_reference(
            noise2(123.456, -654.321),
            -0.383_213_76,
            "noise2 at grid scale",
        );
        // Just inside the reference's +4096 lattice offset, either side of zero.
        assert_reference(
            noise2(4095.5, -4095.5),
            0.387_040_17,
            "noise2 at the offset edge",
        );
        // A sub-cell sample, where the ease curve dominates: the cubic
        // `3t² − 2t³` and the quintic `6t⁵ − 15t⁴ + 10t³` differ most here.
        assert_reference(
            noise2(-0.001, 0.002),
            -0.000_815_997_66,
            "noise2 close to the origin",
        );

        // The summed octaves the terrain's high-frequency term uses.
        assert_reference(
            turbulence2(1.25, -7.5, TURBULENCE_FREQUENCY),
            0.407_186_3,
            "turbulence2 negative y",
        );
        assert_reference(
            turbulence2(123.456, -654.321, TURBULENCE_FREQUENCY),
            -0.356_719_55,
            "turbulence2 at grid scale",
        );
    }

    /// The whole composition value — bilinear band interpolation plus the noise
    /// twiddle — matches the reference viewer's `LLVLComposition::generateHeights`
    /// for a region with four differing corners.
    ///
    /// Same provenance as [`noise_matches_reference_viewer_samples`]: the
    /// expected values come from that same verbatim extract, extended with
    /// `generateHeights`' per-texel math. This is the end-to-end pin — it would
    /// catch a corner-ordering slip or a wrong scale constant that the raw noise
    /// samples cannot.
    #[test]
    fn composition_value_matches_the_reference_viewer() {
        // A region at grid 1000,1000 (global 256 km), corners deliberately all
        // different so a transposed corner would show.
        let comp = TerrainComposition::new(
            [10.0, 20.0, 30.0, 40.0],
            [40.0, 50.0, 60.0, 70.0],
            256.0,
            [256_000.0, 256_000.0],
        );
        for (x, y, elevation, expected) in [
            (0.0_f32, 0.0_f32, 20.0_f32, 0.959_388_55_f32),
            (64.0, 128.0, 25.0, 0.223_824_77),
            (128.0, 128.0, 30.0, 0.282_789_2),
            // Ground below the band: clamped to the lowest detail texture.
            (200.0, 50.0, 15.0, 0.0),
            (255.0, 255.0, 60.0, 1.260_850_2),
            (32.0, 224.0, 42.5, 1.354_598_5),
        ] {
            assert_reference(
                comp.composition_value(x, y, elevation),
                expected,
                &format!("composition at ({x}, {y}, {elevation})"),
            );
        }

        // The weights the renderer actually consumes follow from that value.
        assert_weights(comp.blend_weights(200.0, 50.0, 15.0), [1.0, 0.0, 0.0, 0.0]);
    }
}
