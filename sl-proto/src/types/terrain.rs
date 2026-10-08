//! Terrain layer kinds and patch headers.

use sl_wire::RegionHandle;

/// The kind of layer carried in a `LayerData` message, identified by the
/// single-byte type code in the layer's group header. LAND is the terrain
/// heightmap (the one a renderer needs for the ground); WIND/CLOUD/WATER carry
/// the per-region wind field, cloud density, and water height respectively, in
/// the same patched-DCT encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TerrainPatch {
    /// The region this patch belongs to (its [`RegionHandle`]), or `0` if not yet
    /// known for the originating simulator.
    pub region_handle: RegionHandle,
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

/// One `LayerData` message as it came off the wire: what its two headers
/// state and the patches it carried, in the order it carried them.
///
/// [`Event::TerrainPatch`](crate::Event::TerrainPatch) hands a consumer each
/// patch's values; this is everything else the message said — which is what
/// tells two simulators' terrain streams apart: how many patches one message
/// holds, the order a region is walked in, and how each patch was quantized.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TerrainLayerBatch {
    /// The region the message is about (its [`RegionHandle`]), or `0` if not
    /// yet known for the originating simulator.
    pub region_handle: RegionHandle,
    /// Whether the message came down a child-agent circuit (a neighbouring
    /// region) rather than the root circuit.
    pub child: bool,
    /// The layer the message's own `LayerID.Type` field names.
    pub message_layer: TerrainLayerType,
    /// The layer the payload's group header names. A simulator writes the
    /// same code in both places; they are kept apart so a census can say so.
    pub layer: TerrainLayerType,
    /// The group header's `stride`.
    pub stride: u32,
    /// The group header's patch edge length, in cells.
    pub patch_size: u32,
    /// The payload's length, in bytes.
    pub payload_len: usize,
    /// The header of each patch, in the order the message carried them.
    pub patches: Vec<TerrainPatchHeader>,
}

/// What one patch's header in a `LayerData` message states.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TerrainPatchHeader {
    /// The patch column (grid X) within the region.
    pub patch_x: u32,
    /// The patch row (grid Y) within the region.
    pub patch_y: u32,
    /// The prequantization exponent: the patch's values were scaled to two to
    /// this power of levels before the transform.
    pub prequant: u32,
    /// The magnitude bits each non-zero coefficient is written in.
    pub word_bits: u32,
    /// The lowest value in the patch.
    pub dc_offset: f32,
    /// The whole number the patch's values span.
    pub range: u32,
}

/// How a layer's patches are written into a `LayerData` payload: the parts of
/// the encoding two simulators that agree on the codec still choose
/// differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LayerEncoding {
    /// The group header's `stride`. A decoder never reads it; a simulator
    /// writes its own.
    pub stride: u32,
    /// The prequantization exponent: a patch's values are scaled to two to
    /// this power of levels before the transform. Between 2 and 17.
    pub prequant: u32,
    /// How a patch with no relief is written.
    pub flat_patches: FlatPatches,
}

impl LayerEncoding {
    /// The reference encoding: a stride of 264, ten bits of prequantization,
    /// every patch transformed.
    pub const REFERENCE: Self = Self {
        stride: 264,
        prequant: 10,
        flat_patches: FlatPatches::Transformed,
    };
}

/// How a patch whose values are all the same is written
/// ([`LayerEncoding::flat_patches`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FlatPatches {
    /// Like any other patch: transformed, its one coefficient written out.
    Transformed,
    /// As a header alone — a `QuantWBits` of zero, the value less a half as
    /// `dc_offset`, a `range` of one and no coefficients — which decodes to
    /// the value exactly. OpenSim's "flat terrain speed up".
    HeaderOnly,
}

/// How many patches go into one `LayerData` message, by the size of what has
/// been written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LayerPacking {
    /// Close a message once what is written has passed this many bytes: the
    /// patch that crosses the line is the message's last.
    CloseOnceOver(usize),
    /// Close a message before the patch that would take it past this many
    /// bytes: no message is longer, but for its end marker.
    KeepWithin(usize),
}
