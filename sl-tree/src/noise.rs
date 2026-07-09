//! A port of Firestorm's `LLPerlinNoise` 3D turbulence, used to perturb the trunk
//! radius so bark reads as irregular rather than a smooth cone.
//!
//! The reference viewer seeds its permutation / gradient tables from C `rand()`
//! with no dedicated `srand()`, which — per the C standard — is the default seed
//! `srand(1)`. On the Linux Firestorm build that is glibc's TYPE_3 `random()`
//! generator, so this module ports **that exact generator** (seeded `1`) and
//! consumes the stream in the same order `LLPerlinNoise::init` does (per lattice
//! point: one `g1` draw, two `g2` draws, three `g3` draws; then the permutation
//! shuffle). The `noise3` / `turbulence3` lattice math (`B = 256`, gradient dot at
//! the eight surrounding corners, `s_curve` interpolation) is unchanged. The
//! result matches a fresh-process reference sequence; a live viewer's global
//! `rand()` state can differ, but there is no more canonical bark to match.

use std::sync::OnceLock;

/// Perlin lattice size (`LLPerlinNoise`'s `B`).
const B: usize = 256;
/// Doubled lattice size as a `u32`, the modulus for a gradient component draw.
const TWO_B_U32: u32 = 512;
/// Lattice size as a `u32`, the modulus for the permutation shuffle.
const B_U32: u32 = 256;
/// Table length (`B + B + 2`): the lattice, doubled, plus the two-vertex overlap.
const TABLE: usize = B + B + 2;
/// The large offset `fast_setup` adds so a small coordinate truncates cleanly
/// (`LLPerlinNoise`'s `NF32`).
const NF32: f32 = 4096.0;
/// The C default `rand()` seed the reference implicitly uses (never calls
/// `srand()`, so `srand(1)`).
const SEED: i32 = 1;
/// Turbulence start frequency (`LLVOTree`'s `fractal_depth`).
const FRACTAL_DEPTH: f32 = 5.0;

/// The permutation and 3D gradient tables, built once from the reference seed.
struct Tables {
    /// Permutation table (values `0..B`), doubled with a two-entry overlap.
    p: [usize; TABLE],
    /// Unit 3D gradients, indexed by the permutation, doubled to match `p`.
    g3: [[f32; 3]; TABLE],
}

/// glibc's TYPE_3 `random()` generator (degree 31, separation 3), seeded exactly
/// as `srandom(seed)` — the generator behind Linux `rand()`. Ported so the noise
/// tables match the reference viewer's default (`srand(1)`) stream.
struct GlibcRand {
    /// The additive-feedback state ring (`r_deg = 31` words).
    r: [i32; 31],
    /// The "front" pointer index into [`r`](Self::r) (starts at the separation, 3).
    fptr: usize,
    /// The "rear" pointer index into [`r`](Self::r) (starts at 0).
    rptr: usize,
}

impl GlibcRand {
    /// Seed as `srandom(seed)`: Schrage-fill the state ring from the seed, then
    /// warm the generator up `10 * 31` steps (glibc's `kc *= 10`).
    #[expect(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "fixed 31-word ring indexed 1..31; Schrage arithmetic stays within i32 by construction"
    )]
    fn new(seed: i32) -> Self {
        let seed = if seed == 0 { 1 } else { seed };
        let mut r = [0_i32; 31];
        r[0] = seed;
        for i in 1..31 {
            // r[i] = (16807 * r[i-1]) % 2147483647, via Schrage to avoid overflow.
            let prev = r[i - 1];
            let hi = prev / 127_773;
            let lo = prev % 127_773;
            let mut word = 16807_i32.wrapping_mul(lo) - 2836_i32.wrapping_mul(hi);
            if word < 0 {
                word += 0x7fff_ffff;
            }
            r[i] = word;
        }
        let mut rng = Self {
            r,
            fptr: 3,
            rptr: 0,
        };
        for _ in 0..310 {
            let _: u32 = rng.next();
        }
        rng
    }

    /// One `random()` step, returning a value in `0..=0x7fff_ffff` (glibc `rand()`).
    #[expect(
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        reason = "fixed 31-word ring with both pointers kept in 0..31; u32 sum wraps by design"
    )]
    const fn next(&mut self) -> u32 {
        let sum = self.r[self.fptr]
            .cast_unsigned()
            .wrapping_add(self.r[self.rptr].cast_unsigned());
        self.r[self.fptr] = sum.cast_signed();
        let result = (sum >> 1) & 0x7fff_ffff;
        self.fptr += 1;
        if self.fptr >= 31 {
            self.fptr = 0;
        }
        self.rptr += 1;
        if self.rptr >= 31 {
            self.rptr = 0;
        }
        result
    }

    /// The next value reduced to `0..modulus` (the reference's `rand() % m`).
    fn next_mod(&mut self, modulus: u32) -> u32 {
        self.next().checked_rem(modulus).unwrap_or(0)
    }
}

/// The lazily-built noise tables (from the reference seed).
fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(build_tables)
}

/// Build the permutation and gradient tables, mirroring `LLPerlinNoise::init`
/// exactly — including the `g1` / `g2` draws whose values this port discards but
/// must still consume to keep the shared `rand()` stream aligned with `g3`.
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "fixed-size lattice tables indexed by values provably in 0..B; index math cannot overflow usize"
)]
fn build_tables() -> Tables {
    let mut rng = GlibcRand::new(SEED);
    let mut p = [0usize; TABLE];
    let mut g3 = [[0.0_f32; 3]; TABLE];
    for i in 0..B {
        p[i] = i;
        // The reference draws g1 (1) then g2 (2) before g3 (3); discard the first
        // three so the g3 gradients come from the same stream positions.
        let _g1 = rng.next_mod(TWO_B_U32);
        let _g2x = rng.next_mod(TWO_B_U32);
        let _g2y = rng.next_mod(TWO_B_U32);
        let g = [
            gradient_component(rng.next_mod(TWO_B_U32)),
            gradient_component(rng.next_mod(TWO_B_U32)),
            gradient_component(rng.next_mod(TWO_B_U32)),
        ];
        g3[i] = normalize3(g);
    }
    // Shuffle the permutation (the reference's `while (--i)`, from B-1 down to 1).
    let mut i = B - 1;
    while i >= 1 {
        let k = p[i];
        let j = usize_from_u32(rng.next_mod(B_U32));
        p[i] = p[j];
        p[j] = k;
        i -= 1;
    }
    // Duplicate the first B+2 entries so lattice-corner lookups never wrap.
    for i in 0..(B + 2) {
        p[B + i] = p[i];
        g3[B + i] = g3[i];
    }
    Tables { p, g3 }
}

/// Map a `0..2B` draw to a gradient component in `[-1, 1)` (`(F32)(x - B) / B`).
fn gradient_component(value: u32) -> f32 {
    (f32_from_u32(value) - f32_from_usize(B)) / f32_from_usize(B)
}

/// Normalize a 3D vector (its components are never all zero here).
fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = v;
    let len = (x * x + y * y + z * z).sqrt();
    if len <= f32::EPSILON {
        v
    } else {
        [x / len, y / len, z / len]
    }
}

/// Hermite ease curve `t²(3 − 2t)` (`LLPerlinNoise`'s `s_curve`).
fn s_curve(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Linear interpolation (`LLPerlinNoise`'s `lerp`).
fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

/// Split a coordinate into its two lattice indices and fractional offsets
/// (`LLPerlinNoise`'s `fast_setup`).
fn fast_setup(v: f32) -> (usize, usize, f32, f32) {
    let r1 = v + NF32;
    let t = i32_from_f32_trunc(r1);
    let b0 = usize_from_i32(t & 0xff);
    let b1 = b0.wrapping_add(1) & 0xff;
    let r0 = r1 - f32_from_i32(t);
    (b0, b1, r0, r0 - 1.0)
}

/// The dot of a fractional offset with a lattice gradient (`fast_at3`).
fn at3(rx: f32, ry: f32, rz: f32, q: [f32; 3]) -> f32 {
    let [qx, qy, qz] = q;
    rx * qx + ry * qy + rz * qz
}

/// Fetch a permutation entry (all indices are provably in `0..TABLE`).
fn perm(t: &Tables, index: usize) -> usize {
    t.p.get(index).copied().unwrap_or(0)
}

/// Fetch a gradient entry (all indices are provably in `0..TABLE`).
fn grad(t: &Tables, index: usize) -> [f32; 3] {
    t.g3.get(index).copied().unwrap_or([0.0; 3])
}

/// 3D Perlin noise (`LLPerlinNoise::noise3`), in roughly `[-1, 1]`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "permutation indices are values in 0..B; their sums stay within the doubled table"
)]
fn noise3(x: f32, y: f32, z: f32) -> f32 {
    let t = tables();
    let (bx0, bx1, rx0, rx1) = fast_setup(x);
    let (by0, by1, ry0, ry1) = fast_setup(y);
    let (bz0, bz1, rz0, rz1) = fast_setup(z);

    let i = perm(t, bx0);
    let j = perm(t, bx1);
    let b00 = perm(t, i + by0);
    let b10 = perm(t, j + by0);
    let b01 = perm(t, i + by1);
    let b11 = perm(t, j + by1);

    let sx = s_curve(rx0);
    let sy = s_curve(ry0);
    let sz = s_curve(rz0);

    let u = at3(rx0, ry0, rz0, grad(t, b00 + bz0));
    let v = at3(rx1, ry0, rz0, grad(t, b10 + bz0));
    let a = lerp(sx, u, v);
    let u = at3(rx0, ry1, rz0, grad(t, b01 + bz0));
    let v = at3(rx1, ry1, rz0, grad(t, b11 + bz0));
    let b = lerp(sx, u, v);
    let c = lerp(sy, a, b);

    let u = at3(rx0, ry0, rz1, grad(t, b00 + bz1));
    let v = at3(rx1, ry0, rz1, grad(t, b10 + bz1));
    let a = lerp(sx, u, v);
    let u = at3(rx0, ry1, rz1, grad(t, b01 + bz1));
    let v = at3(rx1, ry1, rz1, grad(t, b11 + bz1));
    let b = lerp(sx, u, v);
    let d = lerp(sy, a, b);

    lerp(sz, c, d)
}

/// 3D fractal turbulence (`LLPerlinNoise::turbulence3`): sum `noise3` octaves from
/// [`FRACTAL_DEPTH`] down, each halved in frequency and amplitude.
#[must_use]
pub(crate) fn turbulence3(x: f32, y: f32, z: f32) -> f32 {
    let mut t = 0.0;
    let mut freq = FRACTAL_DEPTH;
    while freq >= 1.0 {
        t += noise3(freq * x, freq * y, freq * z) / freq;
        freq *= 0.5;
    }
    t
}

/// Widen a small count to `f32` (exact — all values here are `< 2^13`).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "values are small lattice indices, exact in f32"
)]
const fn f32_from_usize(value: usize) -> f32 {
    value as f32
}

/// Widen a `0..2B` draw to `f32` (exact).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "value is a 0..512 draw, exact in f32"
)]
const fn f32_from_u32(value: u32) -> f32 {
    value as f32
}

/// Widen a small `i32` lattice index to `f32` (exact for the values used here).
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "value is a small truncated lattice coordinate, exact in f32"
)]
const fn f32_from_i32(value: i32) -> f32 {
    value as f32
}

/// Truncate a positive `f32` (a coordinate offset by `NF32`) toward zero to `i32`.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "value is a coordinate near NF32 (~4096), far within i32 range"
)]
const fn i32_from_f32_trunc(value: f32) -> i32 {
    if value.is_finite() {
        value.trunc() as i32
    } else {
        0
    }
}

/// Widen a `0..B` draw to `usize`.
fn usize_from_u32(value: u32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

/// Narrow a masked (`0..256`) lattice index to `usize`.
fn usize_from_i32(value: i32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{GlibcRand, noise3, turbulence3};
    use pretty_assertions::assert_eq;

    #[test]
    fn glibc_rand_matches_known_seed_1_sequence() {
        // The canonical glibc `rand()` output for the default seed (`srand(1)`),
        // which is the stream Firestorm's `LLPerlinNoise::init` draws from.
        let mut rng = GlibcRand::new(1);
        let expected = [
            1_804_289_383_u32,
            846_930_886,
            1_681_692_777,
            1_714_636_915,
            1_957_747_793,
            424_238_335,
        ];
        for (index, &want) in expected.iter().enumerate() {
            assert_eq!(rng.next(), want, "glibc rand() mismatch at draw {index}");
        }
    }

    #[test]
    fn noise_is_deterministic_and_bounded() {
        // Same input → same output (the whole point of the fixed seed).
        let a = noise3(1.5, -2.3, 0.7);
        let b = noise3(1.5, -2.3, 0.7);
        assert!((a - b).abs() < f32::EPSILON);
        // Perlin noise stays within roughly [-1, 1].
        for &(x, y, z) in &[(0.1, 0.2, 0.3), (12.0, -4.0, 8.0), (-30.0, 5.0, 0.0)] {
            let n = noise3(x, y, z);
            assert!(n.is_finite() && n.abs() < 2.0, "noise3 out of range: {n}");
        }
    }

    #[test]
    fn turbulence_is_finite_and_varies() {
        let a = turbulence3(0.0, 0.0, 0.0);
        let b = turbulence3(3.0, 1.0, -2.0);
        assert!(a.is_finite() && b.is_finite());
        // Different positions give different turbulence (not a constant field).
        assert!((a - b).abs() > 1e-4);
    }
}
