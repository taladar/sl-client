//! Ground fixtures: the heightfield a region's sessions stream as `LayerData`,
//! the wind and cloud fields that go with it, and the detail-texture
//! composition its `RegionHandshake` names.
//!
//! A real simulator sends the whole region's ground at an arriving viewer as a
//! spiral of compressed 16×16 patches, and serves the same heights again as the
//! estate "download RAW terrain" file. A [`TerrainFixture`] is the one source
//! both come from ([`to_patches`] and [`to_raw`]), so a test that asserts a
//! height sees the same number whichever path it read it through.
//!
//! [`to_patches`]: TerrainFixture::to_patches
//! [`to_raw`]: TerrainFixture::to_raw
#![expect(
    clippy::module_name_repetitions,
    reason = "TerrainFixture is re-exported at the crate root, next to the other fixtures"
)]

use sl_proto::{
    DEFAULT_TERRAIN_DETAIL_TEXTURES, RegionTerrainComposition, STANDARD_REGION_SIZE_METRES,
    TerrainLayerType, TerrainPatch,
};
use sl_wire::RegionHandle;

use crate::udp_assets::{TERRAIN_RAW_CHANNELS, TERRAIN_RAW_SIDE};

/// The edge of one terrain patch, in cells (a standard region's patch; the
/// variable-region 32-cell patch is not modelled here).
pub const PATCH_CELLS: u32 = 16;

/// Patches along each edge of a standard 256 m region: `256 / 16`.
pub const PATCHES_PER_EDGE: u32 = STANDARD_REGION_SIZE_METRES / PATCH_CELLS;

/// The wind and cloud layers are one 16×16 field over the whole region, not
/// one field per patch, so their patches are this many cells on a side too —
/// each cell covering 16 m of ground.
const FIELD_CELLS: usize = 16;

/// The height-multiplier divisors a RAW heightmap may use, finest first: a
/// sample's height is `height_byte * multiplier / 128`, so a smaller
/// multiplier buys resolution and costs range.
const RAW_MULTIPLIERS: [u8; 4] = [16, 32, 64, 128];

/// The shape of a region's ground. Every variant is a closed form over the
/// region's metre coordinates, so the same fixture answers a patch cell and a
/// RAW sample with the same height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Heightfield {
    /// One height everywhere — the ground a viewer test stands on when the
    /// terrain itself is not what is under test.
    Flat {
        /// The height, in metres.
        height: f32,
    },
    /// A plane rising west to east, from `low` at `x = 0` to `high` at the
    /// region's east edge. A negative rise (`high < low`) falls instead.
    Slope {
        /// The height at the west edge, in metres.
        low: f32,
        /// The height at the east edge, in metres.
        high: f32,
    },
    /// A ridge running west to east along the region's centre line: `base` at
    /// the north and south edges, rising linearly to `peak` at `y = 128`.
    Ridge {
        /// The height at the north and south edges, in metres.
        base: f32,
        /// The height along the centre line, in metres.
        peak: f32,
    },
    /// `count` flat terraces stepping up west to east, the westmost at `base`
    /// and each next one `rise` metres above the last — the shape a
    /// ground-snapping or foot-IK test wants, because every height is exact on
    /// a terrace and discontinuous at its edge.
    Steps {
        /// The westmost terrace's height, in metres.
        base: f32,
        /// The height each terrace adds over the one west of it, in metres.
        rise: f32,
        /// The number of terraces across the region (`0` and `1` are flat).
        count: u32,
    },
}

impl Heightfield {
    /// The ground height, in metres, at the region-local metre coordinate
    /// (`x`, `y`). Coordinates outside the region are evaluated by the same
    /// formula rather than clamped, so a caller sampling a patch edge does not
    /// see a seam.
    #[must_use]
    pub fn height_at(&self, x: f32, y: f32) -> f32 {
        let edge = metres(STANDARD_REGION_SIZE_METRES);
        match *self {
            Self::Flat { height } => height,
            Self::Slope { low, high } => (high - low).mul_add(x / edge, low),
            Self::Ridge { base, peak } => {
                // 1.0 at the centre line, 0.0 at the north and south edges.
                let ridge = 1.0 - ((y - edge / 2.0) / (edge / 2.0)).abs();
                (peak - base).mul_add(ridge, base)
            }
            Self::Steps { base, rise, count } => {
                let terraces = metres(count.max(1));
                let terrace = (x / edge * terraces).floor().clamp(0.0, terraces - 1.0);
                rise.mul_add(terrace, base)
            }
        }
    }

    /// The highest point of the field over the region, in metres — what
    /// [`TerrainFixture::to_raw`] sizes its height multiplier from.
    #[must_use]
    pub fn max_height(&self) -> f32 {
        match *self {
            Self::Flat { height } => height,
            Self::Slope { low, high } => low.max(high),
            Self::Ridge { base, peak } => base.max(peak),
            Self::Steps { base, rise, count } => {
                base.max(rise.mul_add(metres(count.max(1)) - 1.0, base))
            }
        }
    }
}

/// A small count as `f32` (region metres and patch counts are all far inside
/// the mantissa, so the conversion is exact; there is no `From<u32>`).
fn metres(value: u32) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

/// The ground of one region: its heights, the wind and cloud fields blowing
/// over it, and the detail-texture composition its `RegionHandshake` names.
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainFixture {
    /// The shape of the ground.
    pub heights: Heightfield,
    /// A uniform wind velocity `(east, north)` in metres per second, or `None`
    /// for a region that sends no wind layer.
    pub wind: Option<[f32; 2]>,
    /// A uniform cloud density in `0.0..=1.0`, or `None` for a region that
    /// sends no cloud layer.
    pub clouds: Option<f32>,
    /// The detail textures and their per-corner blend heights — what the
    /// region's `RegionHandshake` carries and the viewer shades the ground
    /// with.
    pub composition: RegionTerrainComposition,
}

impl Default for TerrainFixture {
    /// Flat ground at [`STOCK_TERRAIN_HEIGHT_M`] with a light easterly breeze,
    /// no clouds, and the four default Linden detail textures.
    ///
    /// [`STOCK_TERRAIN_HEIGHT_M`]: crate::scenario::STOCK_TERRAIN_HEIGHT_M
    fn default() -> Self {
        Self {
            heights: Heightfield::Flat {
                height: f32::from(crate::scenario::STOCK_TERRAIN_HEIGHT_M),
            },
            wind: Some([1.0, 0.0]),
            clouds: None,
            composition: default_composition(),
        }
    }
}

/// The stock terrain composition: the four default Linden detail textures,
/// each corner blending from 10 m over a 60 m range (OpenSim's own defaults).
#[must_use]
pub const fn default_composition() -> RegionTerrainComposition {
    RegionTerrainComposition {
        detail_textures: DEFAULT_TERRAIN_DETAIL_TEXTURES,
        start_heights: [10.0; 4],
        height_ranges: [60.0; 4],
    }
}

impl TerrainFixture {
    /// Flat ground at `height` metres, otherwise the stock fixture.
    #[must_use]
    pub fn flat(height: f32) -> Self {
        Self {
            heights: Heightfield::Flat { height },
            ..Self::default()
        }
    }

    /// The same fixture with `heights` instead.
    #[must_use]
    pub const fn with_heights(mut self, heights: Heightfield) -> Self {
        self.heights = heights;
        self
    }

    /// The ground height, in metres, at the region-local metre coordinate
    /// (`x`, `y`).
    #[must_use]
    pub fn height_at(&self, x: f32, y: f32) -> f32 {
        self.heights.height_at(x, y)
    }

    /// The region's ground as the 256 land patches a standard region streams,
    /// each covering a 16 × 16 metre square, stamped with `handle`. Cells are
    /// row-major within a patch (`row * 16 + column`) from its south-west
    /// corner, which is what the client's decoder produces and the viewer's
    /// mesh builder reads.
    #[must_use]
    pub fn to_patches(&self, handle: RegionHandle) -> Vec<TerrainPatch> {
        let mut patches = Vec::new();
        for patch_y in 0..PATCHES_PER_EDGE {
            for patch_x in 0..PATCHES_PER_EDGE {
                let mut values = Vec::with_capacity(256);
                for cell_y in 0..PATCH_CELLS {
                    for cell_x in 0..PATCH_CELLS {
                        let x = metres(patch_x.saturating_mul(PATCH_CELLS).saturating_add(cell_x));
                        let y = metres(patch_y.saturating_mul(PATCH_CELLS).saturating_add(cell_y));
                        values.push(self.height_at(x, y));
                    }
                }
                patches.push(TerrainPatch {
                    region_handle: handle,
                    layer: TerrainLayerType::Land,
                    patch_x,
                    patch_y,
                    size: PATCH_CELLS,
                    values,
                });
            }
        }
        patches
    }

    /// The wind layer's two patches — the east then the north velocity
    /// component of one 16×16 field over the whole region, both at patch
    /// position `(0, 0)`, exactly as OpenSim's `SendWindData` packs them — or
    /// an empty vector when the fixture carries no wind.
    #[must_use]
    pub fn wind_patches(&self, handle: RegionHandle) -> Vec<TerrainPatch> {
        let Some([east, north]) = self.wind else {
            return Vec::new();
        };
        [east, north]
            .into_iter()
            .map(|component| field_patch(handle, TerrainLayerType::Wind, component))
            .collect()
    }

    /// The cloud layer's single 16×16 density patch, or an empty vector when
    /// the fixture carries no clouds.
    #[must_use]
    pub fn cloud_patches(&self, handle: RegionHandle) -> Vec<TerrainPatch> {
        self.clouds
            .map(|density| vec![field_patch(handle, TerrainLayerType::Cloud, density)])
            .unwrap_or_default()
    }

    /// The region's ground as the estate "download RAW terrain" file: `256 ×
    /// 256` samples of [`TERRAIN_RAW_CHANNELS`] bytes, row-major from the
    /// south-west corner, height in channel 0 scaled by the multiplier in
    /// channel 1 over 128. The multiplier is the finest of the four the
    /// format allows whose range still covers the field's highest point, so
    /// the file quantizes the same heights the patches carry to within half a
    /// step. The eleven land-data channels are zero.
    #[must_use]
    pub fn to_raw(&self) -> Vec<u8> {
        let multiplier = raw_multiplier(self.heights.max_height());
        let scale = f32::from(RAW_MULTIPLIER_DIVISOR) / f32::from(multiplier);
        let mut raw = Vec::with_capacity(
            TERRAIN_RAW_SIDE
                .saturating_mul(TERRAIN_RAW_SIDE)
                .saturating_mul(TERRAIN_RAW_CHANNELS),
        );
        for row in 0..TERRAIN_RAW_SIDE {
            for column in 0..TERRAIN_RAW_SIDE {
                let x = sample_metres(column);
                let y = sample_metres(row);
                let mut sample = [0_u8; TERRAIN_RAW_CHANNELS];
                if let Some(height) = sample.first_mut() {
                    *height = round_to_u8(self.height_at(x, y) * scale);
                }
                if let Some(slot) = sample.get_mut(1) {
                    *slot = multiplier;
                }
                raw.extend_from_slice(&sample);
            }
        }
        raw
    }
}

/// The divisor the RAW format's height multiplier is taken over.
const RAW_MULTIPLIER_DIVISOR: u8 = 128;

/// A RAW sample index as metres (indices are bounded by the region edge, so
/// the conversion is exact).
fn sample_metres(index: usize) -> f32 {
    f32::from(u16::try_from(index).unwrap_or(u16::MAX))
}

/// The finest RAW height multiplier whose range (`255 * multiplier / 128`
/// metres) still covers `max_height`, or the coarsest when nothing does.
fn raw_multiplier(max_height: f32) -> u8 {
    RAW_MULTIPLIERS
        .into_iter()
        .find(|&multiplier| {
            f32::from(u8::MAX) * f32::from(multiplier) / f32::from(RAW_MULTIPLIER_DIVISOR)
                >= max_height
        })
        .unwrap_or(RAW_MULTIPLIER_DIVISOR)
}

/// Rounds a scaled height to the nearest RAW height byte, saturating at both
/// ends (a negative height is sea floor the format cannot express).
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped into 0..=255 before the cast; no From impl exists"
)]
fn round_to_u8(value: f32) -> u8 {
    value.round().clamp(0.0, f32::from(u8::MAX)) as u8
}

/// One whole-region 16×16 field patch of a constant `value` — the shape the
/// wind and cloud layers travel in.
fn field_patch(handle: RegionHandle, layer: TerrainLayerType, value: f32) -> TerrainPatch {
    TerrainPatch {
        region_handle: handle,
        layer,
        patch_x: 0,
        patch_y: 0,
        size: PATCH_CELLS,
        values: vec![value; FIELD_CELLS.saturating_mul(FIELD_CELLS)],
    }
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::*;

    /// A boxed error for terse test signatures.
    type TestError = Box<dyn core::error::Error>;

    /// A handle for the fixtures under test.
    fn handle() -> RegionHandle {
        RegionHandle::from_grid(1000, 1000)
    }

    /// Asserts two heights agree to within `tolerance` metres.
    #[track_caller]
    fn assert_close(got: f32, want: f32, tolerance: f32, what: &str) {
        assert!(
            (got - want).abs() <= tolerance,
            "{what}: got {got}, wanted {want} (± {tolerance})"
        );
    }

    #[test]
    fn a_flat_field_is_flat_everywhere() {
        let field = Heightfield::Flat { height: 25.0 };
        assert_close(field.height_at(0.0, 0.0), 25.0, 0.0, "south-west");
        assert_close(field.height_at(255.0, 255.0), 25.0, 0.0, "north-east");
        assert_close(field.max_height(), 25.0, 0.0, "max");
    }

    #[test]
    fn a_slope_rises_west_to_east() {
        let field = Heightfield::Slope {
            low: 20.0,
            high: 40.0,
        };
        assert_close(field.height_at(0.0, 128.0), 20.0, 0.0, "west edge");
        assert_close(field.height_at(128.0, 0.0), 30.0, 0.01, "midpoint");
        // The last sample is one metre short of the 256 m edge.
        assert_close(field.height_at(255.0, 0.0), 39.92, 0.01, "east edge");
        assert_close(field.max_height(), 40.0, 0.0, "max");
    }

    #[test]
    fn a_ridge_peaks_on_the_centre_line() {
        let field = Heightfield::Ridge {
            base: 20.0,
            peak: 50.0,
        };
        assert_close(field.height_at(0.0, 128.0), 50.0, 0.0, "centre line");
        assert_close(field.height_at(200.0, 128.0), 50.0, 0.0, "centre line east");
        assert_close(field.height_at(0.0, 0.0), 20.0, 0.0, "south edge");
        assert_close(field.height_at(0.0, 64.0), 35.0, 0.01, "half way up");
        assert_close(field.max_height(), 50.0, 0.0, "max");
    }

    #[test]
    fn steps_are_flat_terraces() {
        let field = Heightfield::Steps {
            base: 20.0,
            rise: 2.0,
            count: 4,
        };
        // Four 64 m terraces: 20, 22, 24, 26.
        assert_close(field.height_at(0.0, 0.0), 20.0, 0.0, "first terrace");
        assert_close(field.height_at(63.0, 0.0), 20.0, 0.0, "first terrace east");
        assert_close(field.height_at(64.0, 0.0), 22.0, 0.0, "second terrace");
        assert_close(field.height_at(255.0, 0.0), 26.0, 0.0, "last terrace");
        assert_close(field.max_height(), 26.0, 0.0, "max");
    }

    #[test]
    fn a_region_is_two_hundred_and_fifty_six_patches() {
        let fixture = TerrainFixture::flat(25.0);
        let patches = fixture.to_patches(handle());
        assert_eq!(patches.len(), 256);
        assert!(
            patches
                .iter()
                .all(|patch| patch.region_handle == handle() && patch.values.len() == 256)
        );
        // Every patch position appears exactly once.
        let mut positions: Vec<(u32, u32)> = patches
            .iter()
            .map(|patch| (patch.patch_x, patch.patch_y))
            .collect();
        positions.sort_unstable();
        positions.dedup();
        assert_eq!(positions.len(), 256);
    }

    #[test]
    fn a_patch_cell_carries_the_height_of_its_metre() -> Result<(), TestError> {
        let fixture = TerrainFixture::default().with_heights(Heightfield::Slope {
            low: 20.0,
            high: 40.0,
        });
        let patches = fixture.to_patches(handle());
        // Patch (3, 2) covers x 48..64, y 32..48; its cell (5, 1) is (53, 33).
        let patch = patches
            .iter()
            .find(|patch| (patch.patch_x, patch.patch_y) == (3, 2))
            .ok_or("no patch (3, 2)")?;
        assert_close(
            patch.value(5, 1).ok_or("no cell (5, 1)")?,
            fixture.height_at(53.0, 33.0),
            0.0,
            "cell height",
        );
        Ok(())
    }

    #[test]
    fn wind_is_two_patches_and_clouds_one() -> Result<(), TestError> {
        let fixture = TerrainFixture {
            wind: Some([1.5, -2.5]),
            clouds: Some(0.25),
            ..TerrainFixture::default()
        };
        let wind = fixture.wind_patches(handle());
        let components: Vec<f32> = wind.iter().filter_map(|patch| patch.value(0, 0)).collect();
        assert_eq!(components.len(), 2);
        assert_close(
            components.first().copied().ok_or("no east wind")?,
            1.5,
            0.0,
            "east wind",
        );
        assert_close(
            components.get(1).copied().ok_or("no north wind")?,
            -2.5,
            0.0,
            "north wind",
        );
        // Both components share the whole-region patch position (0, 0).
        assert!(
            wind.iter()
                .all(|patch| patch.layer == TerrainLayerType::Wind
                    && (patch.patch_x, patch.patch_y) == (0, 0))
        );

        let clouds = fixture.cloud_patches(handle());
        let density: Vec<f32> = clouds
            .iter()
            .filter_map(|patch| patch.value(9, 9))
            .collect();
        assert_eq!(density.len(), 1);
        assert_close(
            density.first().copied().ok_or("no density")?,
            0.25,
            0.0,
            "density",
        );
        assert!(
            clouds
                .iter()
                .all(|patch| patch.layer == TerrainLayerType::Cloud)
        );

        let still = TerrainFixture {
            wind: None,
            clouds: None,
            ..TerrainFixture::default()
        };
        assert!(still.wind_patches(handle()).is_empty());
        assert!(still.cloud_patches(handle()).is_empty());
        Ok(())
    }

    #[test]
    fn the_raw_download_carries_the_same_ground() {
        let fixture = TerrainFixture::default().with_heights(Heightfield::Slope {
            low: 20.0,
            high: 40.0,
        });
        let raw = fixture.to_raw();
        assert_eq!(
            raw.len(),
            TERRAIN_RAW_SIDE * TERRAIN_RAW_SIDE * TERRAIN_RAW_CHANNELS
        );
        // A 40 m maximum fits the 63.75 m range of the 32 multiplier, whose
        // quarter-metre step is what the tolerance below allows for.
        for (index, sample) in raw.as_chunks::<TERRAIN_RAW_CHANNELS>().0.iter().enumerate() {
            let x = sample_metres(index % TERRAIN_RAW_SIDE);
            let y = sample_metres(index / TERRAIN_RAW_SIDE);
            let [height, multiplier, ..] = *sample;
            assert_eq!(multiplier, 32, "the multiplier of every sample");
            let metres = f32::from(height) * f32::from(multiplier) / 128.0;
            assert_close(metres, fixture.height_at(x, y), 0.125, "raw sample");
        }
    }

    #[test]
    fn a_tall_field_falls_back_to_a_coarser_multiplier() -> Result<(), TestError> {
        assert_eq!(raw_multiplier(20.0), 16);
        assert_eq!(raw_multiplier(50.0), 32);
        assert_eq!(raw_multiplier(100.0), 64);
        assert_eq!(raw_multiplier(200.0), 128);
        // Above the format's range the coarsest multiplier saturates.
        assert_eq!(raw_multiplier(400.0), 128);
        let raw = TerrainFixture::flat(4000.0).to_raw();
        let first = raw
            .as_chunks::<TERRAIN_RAW_CHANNELS>()
            .0
            .first()
            .ok_or("an empty heightmap")?;
        assert_eq!(first.first().copied(), Some(u8::MAX));
        Ok(())
    }
}
