//! Decoder for the patched-DCT-compressed terrain layers carried in the
//! `LayerData` message (LAND/WATER/WIND/CLOUD, plus the variable-region
//! "extended" variants).
//!
//! The wire format is a bit-packed stream of 16×16 (or, for variable regions,
//! 32×32) patches, each entropy-coded then inverse-DCT'd back into a grid of
//! values (ground heights for the LAND layer). This is a faithful port of the
//! Second Life viewer's decoder (`indra/llmessage/patch_code.cpp` and
//! `patch_idct.cpp`) and OpenSim's `TerrainCompressor.cs`, which agree on the
//! wire format.
//!
//! Integer index arithmetic uses `wrapping_*` purely to satisfy the crate's
//! `arithmetic_side_effects` lint: every operand here is bounded by the patch
//! size (≤ 32), so no wrap can actually occur. Floating-point arithmetic (the
//! DCT itself) is unrestricted.

use crate::types::{TerrainLayerType, TerrainPatch};

/// The per-patch quant/wbits byte value that marks the end of the patch stream.
const END_OF_PATCHES: u32 = 97;

/// `1/sqrt(2)`, the DC scaling factor in the inverse DCT (LL's `OO_SQRT2`).
const OO_SQRT2: f32 = core::f32::consts::FRAC_1_SQRT_2;

/// The largest patch edge length we will decode, as a sanity cap on the
/// group-header `patch_size` (standard regions use 16, variable regions 32).
const MAX_PATCH_SIZE: u32 = 32;

/// The largest number of patches we will decode from a single message, as a
/// guard against a malformed length driving an unbounded loop. A 32×32-patch
/// variable region has 1024 patches per layer; allow some headroom.
const MAX_PATCHES: usize = 4096;

/// Converts a decoded patch coefficient to `f32`. Coefficients are small
/// (`|c| < 2^17`, well within the 24-bit `f32` mantissa) so the conversion is
/// exact; there is no `From<i32>` for `f32`, so the cast lints are expected.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "i32 patch coefficient (|c| < 2^17) to f32 is exact; no From impl exists"
)]
const fn coeff_to_f32(value: i32) -> f32 {
    value as f32
}

/// Converts a small unsigned magnitude (`< 2^24`, exact in an `f32`) to `f32`.
/// Used for the quantizer, the half-quantum bias, and the patch-grid indices;
/// there is no `From<u32>` for `f32`, so the cast lints are expected.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "small u32 (< 2^24) to f32 is exact; no From impl exists"
)]
const fn small_u32_to_f32(value: u32) -> f32 {
    value as f32
}

/// An MSB-first bit reader matching the viewer's `LLBitPack::bitUnpack`: bits
/// are consumed most-significant-first from each byte, and a multi-bit unsigned
/// value is reassembled little-endian (the first 8 bits read form its low
/// byte). Reading past the end yields zero bits and sets [`overrun`].
///
/// [`overrun`]: BitReader::overrun
struct BitReader<'a> {
    /// The remaining input bytes, consumed front to back.
    bytes: core::slice::Iter<'a, u8>,
    /// The byte currently being shifted out, MSB first.
    current: u8,
    /// The number of unread bits left in `current`.
    bits_left: u8,
    /// Set once a read ran off the end of the input.
    overrun: bool,
}

impl<'a> BitReader<'a> {
    /// Creates a reader over `bytes`.
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes: bytes.iter(),
            current: 0,
            bits_left: 0,
            overrun: false,
        }
    }

    /// Reads the next single bit (0 or 1), MSB first. Past the end, returns 0
    /// and latches [`overrun`](BitReader::overrun).
    fn bit(&mut self) -> u32 {
        if self.bits_left == 0 {
            match self.bytes.next() {
                Some(&byte) => {
                    self.current = byte;
                    self.bits_left = 8;
                }
                None => {
                    self.overrun = true;
                    return 0;
                }
            }
        }
        let value = u32::from(self.current >> 7);
        self.current <<= 1;
        self.bits_left = self.bits_left.wrapping_sub(1);
        value & 1
    }

    /// Reads `count` bits (0..=32) as an unsigned value, reassembled the way the
    /// viewer's `bitUnpack` writes into a little-endian integer: the first byte
    /// read is the value's low byte.
    fn unpack(&mut self, count: u32) -> u32 {
        let mut value: u32 = 0;
        let mut shift: u32 = 0;
        let mut remaining = count;
        while remaining > 0 {
            let take = remaining.min(8);
            remaining = remaining.wrapping_sub(take);
            let mut chunk: u32 = 0;
            for _ in 0..take {
                chunk = (chunk << 1) | self.bit();
            }
            value |= chunk << shift;
            shift = shift.wrapping_add(8);
        }
        value
    }
}

/// One patch decoded out of a `LayerData` message, before its region handle and
/// layer are attached by the session.
pub(crate) struct DecodedPatch {
    /// The patch column (grid X) within the region.
    pub(crate) patch_x: u32,
    /// The patch row (grid Y) within the region.
    pub(crate) patch_y: u32,
    /// The patch edge length in cells (16 or 32).
    pub(crate) size: u32,
    /// The decoded values, row-major (`row * size + col`), length `size*size`.
    pub(crate) values: Vec<f32>,
}

/// Decodes a `LayerData` payload into its layer type and the patches it carries.
/// Returns `None` if the group header is malformed (e.g. a zero or oversized
/// patch size). Individual short/garbled patches are tolerated: decoding stops
/// at the first overrun and returns whatever was decoded so far.
pub(crate) fn decode_layer(data: &[u8]) -> Option<(TerrainLayerType, Vec<DecodedPatch>)> {
    let mut reader = BitReader::new(data);
    // Group header: stride (u16), patch size (u8), layer type (u8).
    let _stride = reader.unpack(16);
    let patch_size = reader.unpack(8);
    let layer = TerrainLayerType::from_code(u8::try_from(reader.unpack(8) & 0xff).ok()?);
    if reader.overrun || patch_size == 0 || patch_size > MAX_PATCH_SIZE {
        return None;
    }
    let size_usize = usize::try_from(patch_size).ok()?;
    let total = size_usize.checked_mul(size_usize)?;
    let large = layer.is_extended();

    // Per-patch tables depend only on the patch size; build them once.
    let dequantize = build_dequantize_table(size_usize);
    let icosines = build_icosine_table(patch_size, size_usize);
    let decopy = build_decopy_matrix(patch_size, total)?;

    let mut patches = Vec::new();
    while patches.len() < MAX_PATCHES {
        if reader.overrun {
            break;
        }
        let quant_wbits = reader.unpack(8);
        if quant_wbits == END_OF_PATCHES {
            break;
        }
        let prequant = (quant_wbits >> 4).wrapping_add(2);
        let word_bits = (quant_wbits & 0x0f).wrapping_add(2);
        let dc_offset = f32::from_bits(reader.unpack(32));
        let range = reader.unpack(16);
        let patch_ids = reader.unpack(if large { 32 } else { 10 });
        let (patch_x, patch_y) = if large {
            (patch_ids >> 16, patch_ids & 0xffff)
        } else {
            (patch_ids >> 5, patch_ids & 0x1f)
        };
        if reader.overrun {
            break;
        }
        let coefficients = decode_patch_data(&mut reader, total, word_bits);
        let values = decompress_patch(
            &coefficients,
            size_usize,
            prequant,
            range,
            dc_offset,
            &dequantize,
            &icosines,
            &decopy,
        );
        patches.push(DecodedPatch {
            patch_x,
            patch_y,
            size: patch_size,
            values,
        });
    }
    Some((layer, patches))
}

/// Entropy-decodes one patch's `total` quantized DCT coefficients (in
/// transmission/zigzag order). Each coefficient is coded as: a `0` bit for zero;
/// `10` for end-of-block (all remaining coefficients zero); or `11` followed by
/// a sign bit and `word_bits` magnitude bits.
fn decode_patch_data(reader: &mut BitReader<'_>, total: usize, word_bits: u32) -> Vec<i32> {
    let mut coefficients = Vec::with_capacity(total);
    while coefficients.len() < total {
        if reader.overrun {
            break;
        }
        if reader.bit() == 0 {
            coefficients.push(0);
            continue;
        }
        if reader.bit() == 0 {
            // End-of-block: every remaining coefficient is zero.
            break;
        }
        let negative = reader.bit() == 1;
        let magnitude = i32::try_from(reader.unpack(word_bits)).unwrap_or(0);
        coefficients.push(if negative {
            magnitude.wrapping_neg()
        } else {
            magnitude
        });
    }
    coefficients.resize(total, 0);
    coefficients
}

/// Reconstructs a patch's grid from its quantized coefficients: dequantize and
/// un-zigzag into a block, run the 2-D inverse DCT, then scale by the patch's
/// range/offset. Returns the values row-major (`row * size + col`).
#[expect(
    clippy::too_many_arguments,
    reason = "the per-patch tables and header fields are all decode inputs"
)]
fn decompress_patch(
    coefficients: &[i32],
    size: usize,
    prequant: u32,
    range: u32,
    dc_offset: f32,
    dequantize: &[f32],
    icosines: &[f32],
    decopy: &[u32],
) -> Vec<f32> {
    let quantize = small_u32_to_f32(1u32 << prequant.min(31));
    let half_quantum = small_u32_to_f32(1u32 << prequant.wrapping_sub(1).min(31));
    let multiplier = small_u32_to_f32(range) / quantize;
    let add_value = multiplier.mul_add(half_quantum, dc_offset);

    // Dequantize and un-zigzag: block[k] = coefficient[decopy[k]] * dequant[k].
    let block: Vec<f32> = decopy
        .iter()
        .zip(dequantize.iter())
        .map(|(&zigzag_index, &factor)| {
            let index = usize::try_from(zigzag_index).unwrap_or(0);
            coeff_to_f32(coefficients.get(index).copied().unwrap_or(0)) * factor
        })
        .collect();

    let block = inverse_dct(&block, icosines, size);

    block
        .iter()
        .map(|value| value.mul_add(multiplier, add_value))
        .collect()
}

/// The 2-D inverse DCT over a `size`×`size` block: an inverse-DCT pass down each
/// column, then one across each row (LL's `idct_patch`). The row pass carries
/// the `2/size` normalisation.
fn inverse_dct(block: &[f32], icosines: &[f32], size: usize) -> Vec<f32> {
    let block_rows: Vec<&[f32]> = block.chunks(size).collect();
    let icosine_rows: Vec<&[f32]> = icosines.chunks(size).collect();

    // Column pass: temp[n][col] = OO_SQRT2*block[0][col]
    //                            + sum_{u=1}^{size-1} block[u][col] * cos[u][n].
    let mut temp = Vec::with_capacity(block.len());
    for n in 0..size {
        for col in 0..size {
            let mut total = OO_SQRT2
                * block_rows
                    .first()
                    .and_then(|row| row.get(col))
                    .copied()
                    .unwrap_or(0.0);
            for u in 1..size {
                let coefficient = block_rows
                    .get(u)
                    .and_then(|row| row.get(col))
                    .copied()
                    .unwrap_or(0.0);
                let cosine = icosine_rows
                    .get(u)
                    .and_then(|row| row.get(n))
                    .copied()
                    .unwrap_or(0.0);
                total = coefficient.mul_add(cosine, total);
            }
            temp.push(total);
        }
    }

    let temp_rows: Vec<&[f32]> = temp.chunks(size).collect();
    let normalise = 2.0 / small_u32_to_f32(u32::try_from(size).unwrap_or(0));

    // Row pass: out[line][n] = (OO_SQRT2*temp[line][0]
    //                         + sum_{u=1}^{size-1} temp[line][u] * cos[u][n]) * 2/size.
    let mut out = Vec::with_capacity(temp.len());
    for line in 0..size {
        for n in 0..size {
            let row = temp_rows.get(line);
            let mut total = OO_SQRT2 * row.and_then(|row| row.first()).copied().unwrap_or(0.0);
            for u in 1..size {
                let coefficient = row.and_then(|row| row.get(u)).copied().unwrap_or(0.0);
                let cosine = icosine_rows
                    .get(u)
                    .and_then(|cos| cos.get(n))
                    .copied()
                    .unwrap_or(0.0);
                total = coefficient.mul_add(cosine, total);
            }
            out.push(total * normalise);
        }
    }
    out
}

/// Builds the dequantize table: `table[j*size + i] = 1 + 2*(i + j)`
/// (LL's `build_patch_dequantize_table`).
fn build_dequantize_table(size: usize) -> Vec<f32> {
    let mut table = Vec::with_capacity(size.saturating_mul(size));
    for j in 0..size {
        for i in 0..size {
            let sum = small_u32_to_f32(u32::try_from(i.wrapping_add(j)).unwrap_or(0));
            table.push(2.0f32.mul_add(sum, 1.0));
        }
    }
    table
}

/// Builds the inverse-DCT cosine table:
/// `table[u*size + n] = cos((2n+1) * u * (pi/2)/size)`
/// (LL's `setup_patch_icosines`).
fn build_icosine_table(patch_size: u32, size: usize) -> Vec<f32> {
    let oosob = core::f32::consts::FRAC_PI_2 / small_u32_to_f32(patch_size);
    let mut table = Vec::with_capacity(size.saturating_mul(size));
    for u in 0..size {
        for n in 0..size {
            let u_f = small_u32_to_f32(u32::try_from(u).unwrap_or(0));
            let n_f = small_u32_to_f32(u32::try_from(n).unwrap_or(0));
            let angle = 2.0f32.mul_add(n_f, 1.0) * u_f * oosob;
            table.push(angle.cos());
        }
    }
    table
}

/// Builds the un-zigzag (de-copy) matrix: for each row-major position
/// `j*size + i`, the index of the coefficient that was transmitted there, by
/// walking the patch in the same diagonal zigzag order the encoder used (LL's
/// `build_decopy_matrix`). Returns the steps reordered into row-major order.
fn build_decopy_matrix(patch_size: u32, total: usize) -> Option<Vec<u32>> {
    let mut pairs: Vec<(usize, u32)> = Vec::with_capacity(total);
    let mut i: u32 = 0;
    let mut j: u32 = 0;
    let mut count: u32 = 0;
    let mut diagonal = false;
    let mut rightward = true;
    let last = patch_size.wrapping_sub(1);
    // Bound the walk by `total` steps; a well-formed walk visits each cell once.
    for _ in 0..total {
        if i >= patch_size || j >= patch_size {
            break;
        }
        let position = usize::try_from(j.wrapping_mul(patch_size).wrapping_add(i)).ok()?;
        pairs.push((position, count));
        count = count.wrapping_add(1);
        if diagonal {
            if rightward {
                i = i.wrapping_add(1);
                j = j.wrapping_sub(1);
                if i == last || j == 0 {
                    diagonal = false;
                }
            } else {
                i = i.wrapping_sub(1);
                j = j.wrapping_add(1);
                if i == 0 || j == last {
                    diagonal = false;
                }
            }
        } else if rightward {
            if i < last {
                i = i.wrapping_add(1);
            } else {
                j = j.wrapping_add(1);
            }
            rightward = false;
            diagonal = true;
        } else {
            if j < last {
                j = j.wrapping_add(1);
            } else {
                i = i.wrapping_add(1);
            }
            rightward = true;
            diagonal = true;
        }
    }
    pairs.sort_by_key(|&(position, _)| position);
    Some(pairs.into_iter().map(|(_, step)| step).collect())
}

/// Builds the [`TerrainPatch`] values from [`decode_layer`] output, attaching
/// the layer type and region handle. Kept here so the session layer stays free
/// of the decoder's value plumbing.
pub(crate) fn into_terrain_patch(
    decoded: DecodedPatch,
    layer: TerrainLayerType,
    region_handle: u64,
) -> TerrainPatch {
    TerrainPatch {
        region_handle,
        layer,
        patch_x: decoded.patch_x,
        patch_y: decoded.patch_y,
        size: decoded.size,
        values: decoded.values,
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{BitReader, decode_layer};
    use crate::types::TerrainLayerType;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A writer mirroring the viewer's `LLBitPack::bitPack` (the encoder the
    /// decoder must invert): bits are emitted MSB-first, and a multi-bit value
    /// is taken little-endian (its low byte first). Used to synthesise test
    /// payloads independently of the reader.
    #[derive(Default)]
    struct BitWriter {
        bits: Vec<u8>,
    }

    impl BitWriter {
        fn push(&mut self, value: u32, count: u32) {
            let mut remaining = count;
            let mut byte_shift = 0u32;
            while remaining > 0 {
                let take = remaining.min(8);
                remaining = remaining.wrapping_sub(take);
                let chunk = (value >> byte_shift) & 0xff;
                byte_shift = byte_shift.wrapping_add(8);
                // Emit this chunk's `take` bits, MSB first.
                let mut bit_index = take;
                while bit_index > 0 {
                    bit_index = bit_index.wrapping_sub(1);
                    self.bits
                        .push(u8::try_from((chunk >> bit_index) & 1).unwrap_or(0));
                }
            }
        }

        fn into_bytes(self) -> Vec<u8> {
            let mut out = Vec::new();
            let mut current = 0u8;
            let mut filled = 0u8;
            for bit in self.bits {
                current = (current << 1) | (bit & 1);
                filled = filled.wrapping_add(1);
                if filled == 8 {
                    out.push(current);
                    current = 0;
                    filled = 0;
                }
            }
            if filled > 0 {
                current <<= 8u8.wrapping_sub(filled);
                out.push(current);
            }
            out
        }
    }

    #[test]
    fn bit_reader_round_trips_values() {
        let mut writer = BitWriter::default();
        writer.push(264, 16);
        writer.push(16, 8);
        writer.push(0x3ff, 10);
        writer.push(5, 3);
        let bytes = writer.into_bytes();
        let mut reader = BitReader::new(&bytes);
        assert_eq!(reader.unpack(16), 264);
        assert_eq!(reader.unpack(8), 16);
        assert_eq!(reader.unpack(10), 0x3ff);
        assert_eq!(reader.unpack(3), 5);
        assert!(!reader.overrun);
    }

    /// Builds a single-patch LAND payload whose DCT coefficients are all zero
    /// (an end-of-block immediately), so every cell decodes to the closed-form
    /// flat height `range/2 + dc_offset`.
    fn flat_land_payload(
        patch_x: u32,
        patch_y: u32,
        dc_offset: f32,
        range: u32,
        prequant: u32,
    ) -> Vec<u8> {
        let mut writer = BitWriter::default();
        // Group header.
        writer.push(264, 16);
        writer.push(16, 8);
        writer.push(u32::from(b'L'), 8);
        // Patch header: quant_wbits packs (prequant-2) high, (wbits-2) low.
        // Low nibble 0 => wbits = 2.
        let quant_wbits = prequant.wrapping_sub(2) << 4;
        writer.push(quant_wbits, 8);
        writer.push(dc_offset.to_bits(), 32);
        writer.push(range, 16);
        writer.push((patch_x << 5) | patch_y, 10);
        // Patch data: `11`? No — `10` is end-of-block (all coefficients zero).
        writer.push(1, 1);
        writer.push(0, 1);
        // End of patches.
        writer.push(97, 8);
        writer.into_bytes()
    }

    #[test]
    fn decodes_flat_land_patch() -> Result<(), TestError> {
        let payload = flat_land_payload(2, 3, 20.0, 8, 10);
        let (layer, patches) = decode_layer(&payload).ok_or("payload should decode")?;
        assert_eq!(layer, TerrainLayerType::Land);
        assert_eq!(patches.len(), 1);
        let patch = patches.first().ok_or("expected one patch")?;
        assert_eq!(patch.patch_x, 2);
        assert_eq!(patch.patch_y, 3);
        assert_eq!(patch.size, 16);
        assert_eq!(patch.values.len(), 256);
        // All cells share the flat height range/2 + dc_offset = 8/2 + 20 = 24.
        for value in &patch.values {
            assert!((value - 24.0).abs() < 1e-3, "height {value} != 24.0");
        }
        Ok(())
    }

    #[test]
    fn rejects_zero_patch_size() {
        let mut writer = BitWriter::default();
        writer.push(264, 16);
        writer.push(0, 8);
        writer.push(u32::from(b'L'), 8);
        assert!(decode_layer(&writer.into_bytes()).is_none());
    }

    #[test]
    fn stops_at_end_of_patches_with_no_patches() -> Result<(), TestError> {
        let mut writer = BitWriter::default();
        writer.push(264, 16);
        writer.push(16, 8);
        writer.push(u32::from(b'W'), 8);
        writer.push(97, 8);
        let (layer, patches) = decode_layer(&writer.into_bytes()).ok_or("should decode")?;
        assert_eq!(layer, TerrainLayerType::Water);
        assert!(patches.is_empty());
        Ok(())
    }
}
