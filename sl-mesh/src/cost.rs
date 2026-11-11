//! The triangle-count estimate the reference viewer derives from a mesh
//! header's level block sizes (`LLMeshCostData::init`), before any geometry is
//! decoded.
//!
//! Two consumers read it: the render-cost model, which weights each level by
//! the screen area it is drawn at, and the Build window's Animated Mesh gate,
//! which compares a linkset's finest-level estimate with the region's
//! `AnimatedObjectMaxTris` (`LLVOVolume::canBeAnimatedObject`).

use sl_proto::MESH_LOD_COUNT;

/// Bytes discounted from each level's block size before triangles are
/// estimated (`MeshMetaDataDiscount`).
pub const MESH_METADATA_DISCOUNT: f32 = 384.0;

/// The floor a level's discounted size is raised to (`MeshMinimumByteSize`), so
/// nothing is free.
pub const MESH_MINIMUM_BYTE_SIZE: f32 = 16.0;

/// Bytes per triangle in the size → triangle estimate (`MeshBytesPerTriangle`).
pub const MESH_BYTES_PER_TRIANGLE: f32 = 16.0;

/// The estimated triangle count of each level of detail, coarsest first, from
/// the per-level block byte sizes — the reference's `LLMeshCostData::init`.
///
/// A level the asset omits inherits the next finer one's size, so a
/// single-level mesh is charged for that level at every distance.
#[must_use]
pub fn est_triangles_by_lod(lod_bytes: [u32; MESH_LOD_COUNT]) -> [f32; MESH_LOD_COUNT] {
    let mut bytes = lod_bytes;
    for index in (0..MESH_LOD_COUNT.saturating_sub(1)).rev() {
        let finer = bytes.get(index.saturating_add(1)).copied().unwrap_or(0);
        if let Some(slot) = bytes.get_mut(index)
            && *slot == 0
        {
            *slot = finer;
        }
    }
    bytes.map(|size| {
        (f32_from_u32(size) - MESH_METADATA_DISCOUNT).max(MESH_MINIMUM_BYTE_SIZE)
            / MESH_BYTES_PER_TRIANGLE
    })
}

/// A `u32` as `f32`: block sizes are far below where the precision matters to
/// an estimate.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "a triangle estimate is approximate by construction; f32 covers the range"
)]
const fn f32_from_u32(value: u32) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests {
    use super::{MESH_BYTES_PER_TRIANGLE, MESH_METADATA_DISCOUNT, est_triangles_by_lod};
    use crate::decode::{BlockRef, MeshHeader};
    use pretty_assertions::assert_eq;

    /// An absent level is filled from the next finer one, and every level's
    /// header bytes are discounted, as the reference does.
    #[test]
    fn level_estimates_backfill_and_discount() {
        let tris = est_triangles_by_lod([0, 0, 0, 0x4000]);
        let finest = (16_384.0 - MESH_METADATA_DISCOUNT) / MESH_BYTES_PER_TRIANGLE;
        for (level, estimate) in tris.iter().enumerate() {
            assert!(
                (estimate - finest).abs() < 0.01,
                "level {level} of a high-only mesh is charged for the high block: {tris:?}"
            );
        }
    }

    /// A tiny block is floored rather than estimated at zero or less.
    #[test]
    fn a_tiny_level_is_floored() {
        let tris = est_triangles_by_lod([10, 10, 10, 10]);
        assert!(
            tris.iter().all(|estimate| (estimate - 1.0).abs() < 0.01),
            "{tris:?}"
        );
    }

    /// The header's maximum is its largest level's estimate, wherever in the
    /// list that level sits.
    #[test]
    fn the_maximum_is_the_largest_level() {
        let block = |size: usize| Some(BlockRef { offset: 0, size });
        let header = MeshHeader {
            lods: [block(0x1000), block(0x4000), block(0x0800), None],
            ..MeshHeader::default()
        };
        let largest = (16_384.0 - MESH_METADATA_DISCOUNT) / MESH_BYTES_PER_TRIANGLE;
        assert!((header.est_triangles_max() - largest).abs() < 0.01);
        assert_eq!(MeshHeader::default().lod_bytes(), [0; 4]);
    }
}
