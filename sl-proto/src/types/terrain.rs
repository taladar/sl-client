//! Terrain layer kinds and patch headers.

/// The kind of layer carried in a `LayerData` message, identified by the
/// single-byte type code in the layer's group header. LAND is the terrain
/// heightmap (the one a renderer needs for the ground); WIND/CLOUD/WATER carry
/// the per-region wind field, cloud density, and water height respectively, in
/// the same patched-DCT encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TerrainLayerType {
    /// Terrain heightmap (`'L'`). Each cell is a ground height in metres.
    Land,
    /// Wind field (`'7'`). Carries the per-patch wind velocity components.
    Wind,
    /// Cloud density (`'8'`).
    Cloud,
    /// Water height (`'W'`).
    Water,
    /// Terrain heightmap for a variable-sized ("large"/var) region (`'M'`),
    /// which packs the patch coordinates in 32 bits instead of 10.
    LandExtended,
    /// Wind field for a variable-sized region (`'9'`).
    WindExtended,
    /// Cloud density for a variable-sized region (`':'`).
    CloudExtended,
    /// Water height for a variable-sized region (`'X'`).
    WaterExtended,
    /// An unrecognised layer type code.
    Unknown(u8),
}

impl TerrainLayerType {
    /// Classifies a `LayerData` group-header layer-type code.
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        match code {
            b'L' => Self::Land,
            b'7' => Self::Wind,
            b'8' => Self::Cloud,
            b'W' => Self::Water,
            b'M' => Self::LandExtended,
            b'9' => Self::WindExtended,
            b':' => Self::CloudExtended,
            b'X' => Self::WaterExtended,
            other => Self::Unknown(other),
        }
    }

    /// The wire layer-type code for this layer.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Land => b'L',
            Self::Wind => b'7',
            Self::Cloud => b'8',
            Self::Water => b'W',
            Self::LandExtended => b'M',
            Self::WindExtended => b'9',
            Self::CloudExtended => b':',
            Self::WaterExtended => b'X',
            Self::Unknown(other) => other,
        }
    }

    /// Whether this is a variable-region ("large") layer, whose patches pack
    /// their coordinates in 32 bits rather than 10.
    #[must_use]
    pub const fn is_extended(self) -> bool {
        matches!(
            self,
            Self::LandExtended | Self::WindExtended | Self::CloudExtended | Self::WaterExtended
        )
    }

    /// Whether this is a terrain (ground-height) layer (`Land`/`LandExtended`).
    #[must_use]
    pub const fn is_land(self) -> bool {
        matches!(self, Self::Land | Self::LandExtended)
    }
}

/// One decoded terrain patch: a `size`×`size` block of values (row-major, the
/// row index running along the region's Y axis) at patch grid position
/// (`patch_x`, `patch_y`) within its region. A standard region is 16×16 patches
/// of 16×16 cells (256×256 metres); cell (`x`, `y`) within the patch maps to
/// region cell (`patch_x*size + x`, `patch_y*size + y`). For a [`Land`] patch
/// the values are ground heights in metres.
///
/// [`Land`]: TerrainLayerType::Land
#[derive(Debug, Clone, PartialEq)]
pub struct TerrainPatch {
    /// The region this patch belongs to (its `RegionHandle`), or 0 if not yet
    /// known for the originating simulator.
    pub region_handle: u64,
    /// The layer this patch belongs to.
    pub layer: TerrainLayerType,
    /// The patch column (grid X) within the region.
    pub patch_x: u32,
    /// The patch row (grid Y) within the region.
    pub patch_y: u32,
    /// The patch edge length in cells (16 for a standard region, 32 for a
    /// variable-region "large" patch).
    pub size: u32,
    /// The decoded values, row-major (`row * size + col`), length `size*size`.
    /// For a terrain layer these are ground heights in metres.
    pub values: Vec<f32>,
}

impl TerrainPatch {
    /// The value at cell (`x`, `y`) within the patch (`x`/`y` in `0..size`), or
    /// `None` if out of range. For a terrain layer this is a height in metres.
    #[must_use]
    pub fn value(&self, x: u32, y: u32) -> Option<f32> {
        if x >= self.size || y >= self.size {
            return None;
        }
        let index = usize::try_from(y.wrapping_mul(self.size).wrapping_add(x)).ok()?;
        self.values.get(index).copied()
    }
}
