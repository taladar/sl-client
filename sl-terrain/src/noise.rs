//! The reference viewer's Perlin noise, ported verbatim — the transition-band
//! noise Second Life's terrain compositing is authored against.
//!
//! [`noise2`] and [`turbulence2`] are a faithful port of Firestorm's
//! `indra/newview/noise.{h,cpp}` (`noise2` / `turbulence2` and the `init()` that
//! builds their tables), which `LLVLComposition::generateHeights` samples to
//! wobble the ground-texture elevation bands. It is a *classic* Perlin
//! implementation — a shuffled 256-entry permutation table indexing 256
//! precomputed unit gradients, interpolated with the cubic ease `3t² − 2t³` —
//! and not the modern quintic-ease, hash-on-the-fly formulation.
//!
//! **The exact values matter.** The blend weights this feeds decide where one
//! ground texture gives way to the next; any other noise puts the transition
//! bands somewhere else, so the terrain would not match the region's server-
//! rendered map tile, other viewers, or a side-by-side Firestorm on the same
//! region. That is why the tables are reproduced rather than a "good enough"
//! noise substituted.
//!
//! ## Where the tables come from
//!
//! The reference builds them at first use, in `noise.h`'s `init()`, from the C
//! library's `rand()` seeded with a fixed `srand(42)` — its comment says "we want
//! repeatable noise (e.g. for stable terrain texturing), so seed with known
//! value". That makes the tables constant for a given C library, but computed:
//! `p` is `0..255` shuffled by `rand() % 256`, and each `g2[i]` is a pair of
//! `(rand() % 512 - 256) / 256` values normalised to unit length.
//!
//! Reproducing `rand()` would mean reproducing one C library's generator, so the
//! resulting tables are baked in below instead. They were dumped from a verbatim
//! extract of `noise.{h,cpp}` — `init()`, then `p[0..256]` and `g2[0..256]` —
//! compiled and run against glibc, the same libc the Linux Firestorm builds this
//! workspace is compared against use. Baking them also makes our noise identical
//! on every platform, which the reference's own is not.

/// The number of entries in the permutation and gradient tables (the reference's
/// `B`).
const PERMUTATION_LEN: usize = 256;

/// The mask folding any table index into `0..PERMUTATION_LEN` (the reference's
/// `BM`), standing in for its duplicated second half.
const PERMUTATION_MASK: usize = PERMUTATION_LEN - 1;

/// The large positive offset added to a coordinate before truncation so the
/// lattice index of a negative coordinate still truncates toward the correct
/// cell (the reference's `NF32`, `4096.0`).
const LATTICE_OFFSET: f32 = 4096.0;

mod tables;

use tables::{GRADIENTS, PERMUTATION};

/// The lattice setup for one axis (the reference's `fast_setup`): the two
/// bracketing lattice indices and the offsets from each to the sample point.
///
/// Returns `(low, high, from_low, from_high)`, where `from_high == from_low - 1`.
fn lattice(coordinate: f32) -> (usize, usize, f32, f32) {
    let shifted = coordinate + LATTICE_OFFSET;
    let truncated = shifted.trunc();
    let low = index_of(truncated);
    let from_low = shifted - truncated;
    (
        low,
        low.wrapping_add(1) & PERMUTATION_MASK,
        from_low,
        from_low - 1.0,
    )
}

/// The table index a truncated lattice coordinate falls on, wrapping into
/// `0..PERMUTATION_LEN` exactly as the reference's cast to `U8` does.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the reference truncates the same float to an integer before masking it to a byte; an out-of-range coordinate saturates, which the mask then folds like any other value"
)]
fn index_of(truncated: f32) -> usize {
    let whole = truncated as i32;
    usize::try_from(whole & 0xFF).unwrap_or(0)
}

/// The permutation table entry at `index`, folded into range.
fn permuted(index: usize) -> usize {
    usize::from(
        PERMUTATION
            .get(index & PERMUTATION_MASK)
            .copied()
            .unwrap_or(0),
    )
}

/// The unit gradient vector at `index`, folded into range.
fn gradient(index: usize) -> [f32; 2] {
    GRADIENTS
        .get(index & PERMUTATION_MASK)
        .copied()
        .unwrap_or([0.0, 0.0])
}

/// The reference's ease curve `3t² − 2t³` (`s_curve`) — cubic Hermite
/// smoothstep, *not* the later quintic `6t⁵ − 15t⁴ + 10t³`.
fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Linear interpolation from `a` to `b` by `t` (the reference's `lerp_m`).
fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t * (b - a)
}

/// The dot product of the gradient at `index` with the offset (`x`, `y`) from
/// its lattice point (the reference's `fast_at2`).
fn gradient_dot(index: usize, x: f32, y: f32) -> f32 {
    let [gx, gy] = gradient(index);
    x * gx + y * gy
}

/// Two-dimensional Perlin gradient noise at (`x`, `y`), in roughly `[-1, 1]` —
/// the reference viewer's `noise2`.
pub(crate) fn noise2(x: f32, y: f32) -> f32 {
    let (bx0, bx1, rx0, rx1) = lattice(x);
    let (by0, by1, ry0, ry1) = lattice(y);

    let i = permuted(bx0);
    let j = permuted(bx1);

    let b00 = permuted(i.wrapping_add(by0));
    let b10 = permuted(j.wrapping_add(by0));
    let b01 = permuted(i.wrapping_add(by1));
    let b11 = permuted(j.wrapping_add(by1));

    let sx = ease(rx0);
    let sy = ease(ry0);

    let low = lerp(sx, gradient_dot(b00, rx0, ry0), gradient_dot(b10, rx1, ry0));
    let high = lerp(sx, gradient_dot(b01, rx0, ry1), gradient_dot(b11, rx1, ry1));
    lerp(sy, low, high)
}

/// Summed-octave turbulence at (`x`, `y`) starting from `frequency` — the
/// reference viewer's `turbulence2`: octaves are halved down to `1`, each
/// weighted by the inverse of its frequency.
pub(crate) fn turbulence2(x: f32, y: f32, frequency: f32) -> f32 {
    let mut frequency = frequency;
    let mut total = 0.0;
    while frequency >= 1.0 {
        total += noise2(frequency * x, frequency * y) / frequency;
        frequency *= 0.5;
    }
    total
}
