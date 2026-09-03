//! The whole region's ground arrives as `LayerData` on the way in.
//!
//! The arrival burst a simulator sends when an agent is rooted includes the
//! region's terrain, compressed into `LayerData` packets of 16 × 16-metre
//! patches. A standard 256 m region is 16 × 16 = 256 of them, and a viewer that
//! is short of one draws a hole in the ground where it should be.
//!
//! This is the protocol half of what the full-stack render harness asserts in
//! pixels ("the ground is under the camera"): the *set* of patches, that each is
//! stamped with the region the agent is in, that each carries a full 16 × 16
//! grid of decoded heights, and that those heights are the ones the fixture
//! declares. A picture cannot distinguish a missing patch from one drawn dark;
//! this can.
//!
//! Fake grid only. Neither live grid's ground is a value this workspace
//! declares — OpenSim's is whatever the last OAR load left and Second Life's is
//! whatever the estate looks like — so "the heights are the fixture's" has no
//! meaning there, and "all 256 arrived" is the *client's* claim about a region
//! whose patch count nobody promised.

use std::collections::HashSet;

use sl_client_tokio::{Event, TerrainLayerType};

use crate::context::{TestContext, TestFailure};
use crate::grid::Grid;
use crate::registry::{GridTest, TestFuture};
use crate::support::{REGION_TIMEOUT, check, check_eq, count_metric};

/// Patches along each edge of a standard 256 m region: `256 / 16`.
const PATCHES_PER_EDGE: u32 = 16;

/// Every land patch a standard region streams: [`PATCHES_PER_EDGE`] squared,
/// as a count.
const REGION_PATCHES: usize = 16 * 16;

/// The edge length, in cells, of one standard patch.
const PATCH_CELLS: u32 = 16;

/// The cells one standard patch decodes to: [`PATCH_CELLS`] squared, as a
/// count.
const PATCH_CELL_COUNT: usize = 16 * 16;

/// The ground height the fake grid's stock terrain is flat at, in metres —
/// [`sl_fake_grid::scenario::STOCK_TERRAIN_HEIGHT_M`] as a float, because the
/// wire carries decoded heights as floats.
///
/// Written out rather than converted because the grid's constant is a `u8` and
/// `f32::from` is not a `const fn`; the test below ties the two together, which
/// is the same bargain the catalogue's row height strikes.
const STOCK_HEIGHT_M: f32 = 25.0;

/// How far a decoded height may sit from the height the fixture declares, in
/// metres.
///
/// The `LayerData` codec is lossy by construction: heights go over the wire as
/// a quantised, DCT-compressed patch, so a flat 25 m plane comes back as 25 m
/// give or take the quantiser — not as the same bits. A tenth of a metre is far
/// inside "this is the ground the fixture declared" and far outside "this is a
/// different height".
const HEIGHT_TOLERANCE_M: f32 = 0.1;

/// Collects the region's land patches from the arrival burst and asserts the
/// whole ground arrived.
#[derive(Debug)]
pub struct TerrainLayerData;

impl GridTest for TerrainLayerData {
    fn name(&self) -> &'static str {
        "terrain-layerdata"
    }

    fn description(&self) -> &'static str {
        "Collect the region's LayerData land patches and check the whole ground arrived"
    }

    fn grids(&self) -> &'static [Grid] {
        &[Grid::Fake]
    }

    fn run<'a>(&'a self, ctx: &'a mut TestContext) -> TestFuture<'a> {
        Box::pin(async move {
            let session = ctx.primary();
            session.wait_for_region(REGION_TIMEOUT).await?;
            let region_handle = session.region_handle().ok_or_else(|| {
                TestFailure::Assertion("login reported no region handle".to_owned())
            })?;

            // Collect until the whole set has arrived; the wait fails on its own
            // if it never does, which is exactly the failure this case exists to
            // catch.
            let mut seen: HashSet<(u32, u32)> = HashSet::new();
            let mut worst_error = 0.0_f32;
            let mut cells = 0_usize;
            session
                .wait_for(REGION_TIMEOUT, |event| {
                    let Event::TerrainPatch(patch) = event else {
                        return None;
                    };
                    if patch.layer != TerrainLayerType::Land {
                        return None;
                    }
                    if patch.region_handle == region_handle && patch.size == PATCH_CELLS {
                        for height in &patch.values {
                            worst_error = worst_error.max((height - STOCK_HEIGHT_M).abs());
                            cells = cells.saturating_add(1);
                        }
                        let _new = seen.insert((patch.patch_x, patch.patch_y));
                    }
                    (seen.len() >= REGION_PATCHES).then_some(())
                })
                .await?;

            check_eq("land patches", &seen.len(), &REGION_PATCHES)?;
            check_eq(
                "decoded height cells",
                &cells,
                &REGION_PATCHES.saturating_mul(PATCH_CELL_COUNT),
            )?;
            // Every patch index in the 16 × 16 grid, exactly once: a set of the
            // right *size* could still be the same corner sent 256 times.
            for patch_y in 0..PATCHES_PER_EDGE {
                for patch_x in 0..PATCHES_PER_EDGE {
                    check(
                        seen.contains(&(patch_x, patch_y)),
                        &format!("the ground is missing its patch at ({patch_x}, {patch_y})"),
                    )?;
                }
            }
            check(
                worst_error <= HEIGHT_TOLERANCE_M,
                &format!(
                    "a decoded ground height is {worst_error} m from the fixture's \
                     {STOCK_HEIGHT_M} m, beyond the codec's {HEIGHT_TOLERANCE_M} m tolerance"
                ),
            )?;

            let metrics = ctx.metrics();
            metrics.set(
                &count_metric("land_patches"),
                i64::try_from(seen.len()).unwrap_or(-1),
            );
            metrics.set("worst_height_error_m", f64::from(worst_error));
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{PATCH_CELL_COUNT, PATCH_CELLS, PATCHES_PER_EDGE, REGION_PATCHES, STOCK_HEIGHT_M};
    use pretty_assertions::assert_eq;

    /// The written-out counts are the squares they claim to be, and the height
    /// this case expects is the height the grid's fixture is flat at. All three
    /// are literals because the arithmetic and the conversion are not `const`;
    /// this is what keeps them honest.
    #[expect(
        clippy::float_cmp,
        reason = "both sides are the same small whole number of metres, exactly \
                  representable; the point of the test is that they are the same one"
    )]
    #[test]
    fn the_written_out_constants_match_what_they_stand_for() {
        assert_eq!(
            REGION_PATCHES,
            usize::try_from(PATCHES_PER_EDGE.saturating_mul(PATCHES_PER_EDGE)).unwrap_or(0)
        );
        assert_eq!(
            PATCH_CELL_COUNT,
            usize::try_from(PATCH_CELLS.saturating_mul(PATCH_CELLS)).unwrap_or(0)
        );
        assert_eq!(
            STOCK_HEIGHT_M,
            f32::from(sl_fake_grid::scenario::STOCK_TERRAIN_HEIGHT_M)
        );
    }
}
