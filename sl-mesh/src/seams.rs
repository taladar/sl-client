//! UV-seam smoothness analysis (R22 diagnostic).
//!
//! A rigged mesh body's arm is one submesh whose geometry and normals are smooth,
//! yet a textured render can show hard tone "seams" at the elbow / wrist. Those are
//! **UV-island seams**: duplicated-position vertices carrying two different UVs, so
//! the rendered surface samples the texture at two places at once. If those samples
//! differ, a visible tone step appears; a correct sampler (and the reference viewer)
//! keeps them equal.
//!
//! [`analyze_uv_seams`] measures this directly on a decoded [`Submesh`] against any
//! texture look-up, so a unit test can confirm a bake maps smoothly across a mesh's
//! seams without a live viewer — and pin the cause (e.g. a wrong UV orientation) to
//! the sampler.

use std::collections::HashMap;

use crate::decode::Submesh;

/// The scale (texels-ish) at which two UVs count as the same when de-duplicating
/// the UVs at a shared position — `1/1024` of the UV domain.
const UV_QUANTIZE: f32 = 1024.0;

/// One UV seam: a 3D position shared by vertices with two or more different UVs,
/// and the colour spread a texture look-up produced across those UVs. Many hits lie
/// along one visible seam *line* (each vertex on the line is a hit), so a caller
/// that wants distinct seams clusters hits by [`position`](Self::position).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeamHit {
    /// A representative 3D position of the seam (mesh-local space).
    pub position: [f32; 3],
    /// The largest per-channel colour delta (`0..1`) across the seam's UVs.
    pub delta: f32,
}

/// A report on how smoothly a texture maps across a submesh's UV seams.
#[derive(Debug, Clone, PartialEq)]
pub struct UvSeamReport {
    /// Distinct 3D positions shared by vertices carrying two or more different UVs
    /// (the UV-island seams).
    pub seam_positions: usize,
    /// The largest colour delta (max abs per-channel difference, `0..1`) sampled
    /// across any single seam. `0.0` when there are no seams.
    pub max_delta: f32,
    /// The mean per-seam colour delta (`0..1`). `0.0` when there are no seams.
    pub mean_delta: f32,
    /// How many seams have a delta at or above the `visible` threshold passed to
    /// [`analyze_uv_seams`].
    pub visible_seams: usize,
}

/// Analyse texture continuity across `submesh`'s UV seams.
///
/// Groups the submesh's vertices by 3D position (quantised to `position_epsilon`);
/// for each position shared by vertices with differing UVs, samples `sample` at
/// each distinct UV and measures the per-channel colour spread. A large spread is a
/// visible tone seam.
///
/// `sample` maps a UV to a linear-or-sRGB RGB triple in `0..1`; the caller applies
/// whatever orientation the renderer uses (e.g. the `v -> 1 - v` flip), so passing
/// two orientations tells which one maps smoothly. `visible` is the per-channel
/// delta at or above which a seam counts as visible.
#[must_use]
#[expect(
    clippy::type_complexity,
    reason = "a private local grouping map; naming it would not aid readability"
)]
pub fn uv_seam_hits(
    submesh: &Submesh,
    position_epsilon: f32,
    sample: impl Fn([f32; 2]) -> [f32; 3],
) -> Vec<SeamHit> {
    let scale = if position_epsilon > 0.0 {
        1.0 / position_epsilon
    } else {
        1.0
    };
    // Every UV, plus a representative position, seen at each quantised 3D position.
    let mut by_position: HashMap<[u32; 3], (Vec<[f32; 2]>, [f32; 3])> = HashMap::new();
    for (index, &position) in submesh.positions.iter().enumerate() {
        let Some(&uv) = submesh.uvs.get(index) else {
            continue;
        };
        let key = [
            quantize_bits(position[0], scale),
            quantize_bits(position[1], scale),
            quantize_bits(position[2], scale),
        ];
        by_position
            .entry(key)
            .or_insert_with(|| (Vec::new(), position))
            .0
            .push(uv);
    }

    let mut hits = Vec::new();
    for (uvs, position) in by_position.values() {
        let distinct = distinct_uvs(uvs);
        if distinct.len() < 2 {
            continue;
        }
        let colours: Vec<[f32; 3]> = distinct.iter().map(|&uv| sample(uv)).collect();
        hits.push(SeamHit {
            position: *position,
            delta: colour_spread(&colours),
        });
    }
    hits
}

/// Summarise a submesh's UV-seam texture continuity: a wrapper over
/// [`uv_seam_hits`] that counts the seam positions and their colour deltas.
///
/// Note the counts are per **vertex-position**, not per visible seam *line* — one
/// seam line is a chain of many hits. Cluster [`uv_seam_hits`] by position for
/// distinct seams.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `analyze_uv_seams` reads clearly"
)]
pub fn analyze_uv_seams(
    submesh: &Submesh,
    position_epsilon: f32,
    visible: f32,
    sample: impl Fn([f32; 2]) -> [f32; 3],
) -> UvSeamReport {
    let hits = uv_seam_hits(submesh, position_epsilon, sample);
    let mut visible_seams = 0_usize;
    let mut sum_delta = 0.0_f32;
    let mut count = 0.0_f32;
    let mut max_delta = 0.0_f32;
    for hit in &hits {
        sum_delta += hit.delta;
        count += 1.0;
        if hit.delta > max_delta {
            max_delta = hit.delta;
        }
        if hit.delta >= visible {
            visible_seams = visible_seams.saturating_add(1);
        }
    }
    let seam_positions = hits.len();
    let mean_delta = if count > 0.0 { sum_delta / count } else { 0.0 };
    UvSeamReport {
        seam_positions,
        max_delta,
        mean_delta,
        visible_seams,
    }
}

/// Quantise a coordinate to a stable [`u32`] grid key (via the rounded value's bit
/// pattern), so two vertices at the same position map to the same key. Adding `0.0`
/// normalises a `-0.0` round result to `0.0`.
fn quantize_bits(value: f32, scale: f32) -> u32 {
    ((value * scale).round() + 0.0).to_bits()
}

/// The distinct UVs in `uvs`, treating UVs within `1/UV_QUANTIZE` as equal.
fn distinct_uvs(uvs: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut seen: Vec<[u32; 2]> = Vec::new();
    let mut out: Vec<[f32; 2]> = Vec::new();
    for &uv in uvs {
        let key = [
            (uv[0] * UV_QUANTIZE).round().to_bits(),
            (uv[1] * UV_QUANTIZE).round().to_bits(),
        ];
        if !seen.contains(&key) {
            seen.push(key);
            out.push(uv);
        }
    }
    out
}

/// The largest per-channel spread (max − min) across a set of sampled colours.
fn colour_spread(colours: &[[f32; 3]]) -> f32 {
    let mut spread = 0.0_f32;
    for channel in 0..3 {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for colour in colours {
            let value = colour.get(channel).copied().unwrap_or(0.0);
            if value < lo {
                lo = value;
            }
            if value > hi {
                hi = value;
            }
        }
        let delta = hi - lo;
        if delta > spread {
            spread = delta;
        }
    }
    spread
}

#[cfg(test)]
mod tests {
    use super::analyze_uv_seams;
    use crate::decode::Submesh;
    use pretty_assertions::assert_eq;

    /// A submesh with the given per-vertex positions and UVs (no rig / indices —
    /// the analysis reads positions + UVs only).
    fn submesh(positions: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>) -> Submesh {
        Submesh {
            positions,
            normals: Vec::new(),
            uvs,
            indices: Vec::new(),
            weights: None,
            normalized_scale: [1.0, 1.0, 1.0],
            no_geometry: false,
        }
    }

    /// A seam (two UVs at one position) that samples two different colours is
    /// reported as a large-delta, visible seam.
    #[test]
    fn a_mismatched_seam_is_flagged() {
        // Two vertices at the same point, UVs 0.25 and 0.75; the sampler returns a
        // colour that grows with u, so the two sides differ by 0.5.
        let mesh = submesh(vec![[0.0; 3], [0.0; 3]], vec![[0.25, 0.5], [0.75, 0.5]]);
        let report = analyze_uv_seams(&mesh, 1e-4, 0.1, |uv| [uv[0], 0.0, 0.0]);
        assert_eq!(report.seam_positions, 1);
        assert_eq!(report.visible_seams, 1);
        assert!(
            (report.max_delta - 0.5).abs() < 1e-6,
            "delta {}",
            report.max_delta
        );
    }

    /// A seam whose two UVs sample the *same* colour is smooth: it is still a seam
    /// position, but its delta is zero and it is not counted visible.
    #[test]
    fn a_matched_seam_is_smooth() {
        let mesh = submesh(vec![[0.0; 3], [0.0; 3]], vec![[0.25, 0.5], [0.75, 0.5]]);
        // A constant sampler: both UVs return the same colour.
        let report = analyze_uv_seams(&mesh, 1e-4, 0.1, |_uv| [0.3, 0.3, 0.3]);
        assert_eq!(report.seam_positions, 1);
        assert_eq!(report.visible_seams, 0);
        assert!(report.max_delta.abs() < 1e-9, "delta {}", report.max_delta);
        assert!(report.mean_delta.abs() < 1e-9, "mean {}", report.mean_delta);
    }

    /// Vertices at distinct positions, or one position with a single UV, are not
    /// seams.
    #[test]
    fn non_seams_are_ignored() {
        // Same UV at one position, plus a lone vertex elsewhere: no seam.
        let mesh = submesh(
            vec![[0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]],
            vec![[0.5, 0.5], [0.5, 0.5], [0.2, 0.2]],
        );
        let report = analyze_uv_seams(&mesh, 1e-4, 0.1, |uv| [uv[0], uv[1], 0.0]);
        assert_eq!(report.seam_positions, 0);
        assert_eq!(report.visible_seams, 0);
        assert!(report.max_delta.abs() < 1e-9);
    }
}
