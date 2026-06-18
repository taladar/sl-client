//! Minimal JPEG 2000 (`.j2c`) codestream *header* parsing — just enough to
//! drive level-of-detail (discard-level) texture fetching. This deliberately
//! does **not** decode the image into pixels: it reads the `SIZ` (image and
//! tile size) and `COD` (coding style) marker segments to learn a texture's
//! pixel dimensions, component count and wavelet decomposition-level count,
//! and ports the viewer's byte-size estimate (`LLImageJ2C::calcDataSizeJ2C`)
//! so a caller can keep only the prefix of the codestream needed for a given
//! discard (LOD) level.
//!
//! A Second Life texture codestream is encoded so that a *prefix* of its bytes
//! is itself a valid, lower-resolution image (progressive by resolution). The
//! viewer uses an estimate — not the true layer boundary, which would be too
//! large for fast fetching — of how many bytes correspond to each discard
//! level; [`truncate_to_discard`] reproduces that so the HTTP texture fetch can
//! return a smaller image for a coarser LOD.
//!
//! All multi-byte codestream values are big-endian. The compression-rate term
//! of the viewer's estimate is the default `1/8`, so the size arithmetic here
//! is exact integer division by 8 (no floating point, hence no lossy casts).

/// The viewer's `FIRST_PACKET_SIZE`: a lower bound on the estimated byte size
/// of any discard level, large enough to always cover the codestream header.
/// Also the size of the initial HTTP `Range` probe a runtime issues to read a
/// texture's [`Header`] before requesting the full LOD prefix.
pub const FIRST_PACKET_SIZE: usize = 600;

/// The J2C marker prefix byte: every two-byte marker starts with `0xFF`.
const MARKER_PREFIX: u8 = 0xFF;

/// The second byte of the `SIZ` marker (`0xFF51`) — the image-and-tile-size
/// segment carrying the canvas dimensions and component count.
const MARKER_SIZ: u8 = 0x51;

/// The second byte of the `COD` marker (`0xFF52`) — the default coding-style
/// segment whose `SPcod` field carries the decomposition-level count.
const MARKER_COD: u8 = 0x52;

/// How far into the codestream we scan for the `SIZ`/`COD` markers. They appear
/// in the main header right after `SOC`, so a small window suffices; the cap
/// guards against scanning an arbitrarily long non-J2C blob.
const SCAN_WINDOW: usize = 64;

/// The parsed header of a J2C codestream: enough to estimate per-LOD byte
/// sizes without decoding the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Image width in pixels (`Xsiz - XOsiz`).
    pub width: u32,
    /// Image height in pixels (`Ysiz - YOsiz`).
    pub height: u32,
    /// Number of components (`Csiz`; e.g. 3 for RGB, 4 with alpha).
    pub components: u16,
    /// Number of wavelet decomposition levels (`COD` `SPcod`), or `None` if no
    /// `COD` segment was found in the scanned window. The highest meaningful
    /// discard level equals this count.
    pub decomposition_levels: Option<u8>,
}

impl Header {
    /// The estimated number of leading codestream bytes that make up the image
    /// at `discard_level` (0 = full resolution; each level halves both
    /// dimensions). Mirrors the viewer's `calcDataSizeJ2C` with its default
    /// `1/8` compression rate.
    #[must_use]
    pub fn discard_data_size(&self, discard_level: u8) -> usize {
        discard_data_size(self.width, self.height, self.components, discard_level)
    }

    /// The highest discard level that still has a distinct resolution, i.e. the
    /// decomposition-level count (defaulting to 0 if no `COD` was parsed).
    #[must_use]
    pub fn max_discard_level(&self) -> u8 {
        self.decomposition_levels.unwrap_or(0)
    }
}

/// Reads a big-endian `u16` at `offset` in `data`, or `None` if out of bounds.
/// (Assembled by explicit shifts rather than `from_be_bytes` to satisfy the
/// crate's endian-byte-method lint.)
fn read_u16_be(data: &[u8], offset: usize) -> Option<u16> {
    let high = u16::from(*data.get(offset)?);
    let low = u16::from(*data.get(offset.checked_add(1)?)?);
    Some((high << 8) | low)
}

/// Reads a big-endian `u32` at `offset` in `data`, or `None` if out of bounds.
fn read_u32_be(data: &[u8], offset: usize) -> Option<u32> {
    let b0 = u32::from(*data.get(offset)?);
    let b1 = u32::from(*data.get(offset.checked_add(1)?)?);
    let b2 = u32::from(*data.get(offset.checked_add(2)?)?);
    let b3 = u32::from(*data.get(offset.checked_add(3)?)?);
    Some((b0 << 24) | (b1 << 16) | (b2 << 8) | b3)
}

/// Finds the offset of the byte *following* the two-byte marker `0xFF<second>`
/// within the first [`SCAN_WINDOW`] bytes of `data`, or `None`.
fn find_marker(data: &[u8], second: u8) -> Option<usize> {
    let limit = data.len().min(SCAN_WINDOW);
    let window = data.get(..limit)?;
    for (index, pair) in window.windows(2).enumerate() {
        if pair.first() == Some(&MARKER_PREFIX) && pair.get(1) == Some(&second) {
            return index.checked_add(2);
        }
    }
    None
}

/// Parses the `SIZ` (and, if present, `COD`) marker segments of a J2C
/// codestream into a [`Header`], or `None` if no `SIZ` segment is found in
/// the scanned window (i.e. the data is not a recognisable J2C codestream).
#[must_use]
pub fn parse_header(data: &[u8]) -> Option<Header> {
    // After the `SIZ` marker: Lsiz(2) Rsiz(2) Xsiz(4) Ysiz(4) XOsiz(4) YOsiz(4)
    // then tile fields, then Csiz(2). Offsets are measured from the first byte
    // after the marker.
    let siz = find_marker(data, MARKER_SIZ)?;
    let x_size = read_u32_be(data, siz.checked_add(4)?)?;
    let y_size = read_u32_be(data, siz.checked_add(8)?)?;
    let x_offset = read_u32_be(data, siz.checked_add(12)?)?;
    let y_offset = read_u32_be(data, siz.checked_add(16)?)?;
    let width = x_size.checked_sub(x_offset)?;
    let height = y_size.checked_sub(y_offset)?;
    // Csiz sits after Lsiz+Rsiz (4 bytes), the four canvas u32s (16 bytes) and
    // the four tile u32s (16 bytes): 4 + 16 + 16 = 36 bytes past the marker.
    let components = read_u16_be(data, siz.checked_add(36)?)?;

    // After the `COD` marker: Lcod(2) Scod(1) ProgOrder(1) NumLayers(2) MCT(1)
    // then SPcod's first byte is the decomposition-level count (offset 9).
    let decomposition_levels = find_marker(data, MARKER_COD)
        .and_then(|cod| cod.checked_add(7))
        .and_then(|offset| data.get(offset).copied());

    Some(Header {
        width,
        height,
        components,
        decomposition_levels,
    })
}

/// The viewer's `calcDataSizeJ2C` byte-size estimate for `discard_level` of an
/// image of the given pixel dimensions and component count. Each discard level
/// halves both dimensions (floored at 1); the default `1/8` compression rate is
/// applied as exact integer division. The result is never below the viewer's
/// `FIRST_PACKET_SIZE` (600 bytes).
#[must_use]
pub fn discard_data_size(width: u32, height: u32, components: u16, discard_level: u8) -> usize {
    let mut w = u64::from(width);
    let mut h = u64::from(height);
    let mut level = discard_level;
    while level > 0 && w > 1 && h > 1 {
        w >>= 1_u64;
        h >>= 1_u64;
        level = level.saturating_sub(1);
    }
    let raw = w.saturating_mul(h).saturating_mul(u64::from(components)) / 8;
    usize::try_from(raw)
        .unwrap_or(usize::MAX)
        .max(FIRST_PACKET_SIZE)
}

/// Returns the prefix of a J2C codestream that represents the image at
/// `discard_level`, using [`discard_data_size`] on the parsed header. Returns
/// the whole input unchanged at discard level 0, or when the data is not a
/// recognisable J2C codestream (so a caller always gets *some* bytes back).
#[must_use]
pub fn truncate_to_discard(data: &[u8], discard_level: u8) -> &[u8] {
    if discard_level == 0 {
        return data;
    }
    let Some(header) = parse_header(data) else {
        return data;
    };
    let size = header.discard_data_size(discard_level).min(data.len());
    data.get(..size).unwrap_or(data)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{FIRST_PACKET_SIZE, discard_data_size, parse_header, truncate_to_discard};

    /// Appends `value` to `data` as big-endian bytes of `width` bytes (avoiding
    /// the endian-byte-method lint), e.g. `width = 4` for a `u32` field.
    fn push_be(data: &mut Vec<u8>, value: u32, width: u32) {
        let mut shift = width.saturating_mul(8);
        while shift >= 8 {
            shift = shift.saturating_sub(8);
            let byte = u8::try_from((value >> shift) & 0xFF).unwrap_or(0);
            data.push(byte);
        }
    }

    /// Builds a minimal J2C main header (`SOC` + `SIZ` + `COD`) for the given
    /// dimensions/components/levels, padded so the markers are well within the
    /// scan window. Only the fields the parser reads are filled meaningfully.
    fn synth_header(width: u32, height: u32, components: u16, levels: u8) -> Vec<u8> {
        let mut data = Vec::new();
        // SOC marker.
        data.extend_from_slice(&[0xFF, 0x4F]);
        // SIZ marker + Lsiz + Rsiz.
        data.extend_from_slice(&[0xFF, 0x51]);
        push_be(&mut data, 38, 2); // Lsiz (nominal)
        push_be(&mut data, 0, 2); // Rsiz
        push_be(&mut data, width, 4); // Xsiz
        push_be(&mut data, height, 4); // Ysiz
        push_be(&mut data, 0, 4); // XOsiz
        push_be(&mut data, 0, 4); // YOsiz
        push_be(&mut data, width, 4); // XTsiz
        push_be(&mut data, height, 4); // YTsiz
        push_be(&mut data, 0, 4); // XTOsiz
        push_be(&mut data, 0, 4); // YTOsiz
        push_be(&mut data, u32::from(components), 2); // Csiz
        // One component descriptor (Ssiz, XRsiz, YRsiz).
        data.extend_from_slice(&[7, 1, 1]);
        // COD marker + Lcod + Scod + ProgOrder + NumLayers + MCT + SPcod[0].
        data.extend_from_slice(&[0xFF, 0x52]);
        push_be(&mut data, 12, 2); // Lcod
        data.push(0); // Scod
        data.push(0); // ProgOrder
        push_be(&mut data, 1, 2); // NumLayers
        data.push(0); // MCT
        data.push(levels); // SPcod: decomposition levels
        data
    }

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    #[test]
    fn parses_dimensions_components_and_levels() -> Result<(), TestError> {
        let data = synth_header(512, 256, 3, 5);
        let header = parse_header(&data).ok_or("recognisable J2C header")?;
        assert_eq!(header.width, 512);
        assert_eq!(header.height, 256);
        assert_eq!(header.components, 3);
        assert_eq!(header.decomposition_levels, Some(5));
        assert_eq!(header.max_discard_level(), 5);
        Ok(())
    }

    #[test]
    fn discard_size_halves_per_level_and_floors_at_first_packet() {
        // 512*512*3/8 = 98304 at discard 0.
        assert_eq!(discard_data_size(512, 512, 3, 0), 98_304);
        // Each discard level halves both dimensions -> quarter the area.
        assert_eq!(discard_data_size(512, 512, 3, 1), 24_576);
        assert_eq!(discard_data_size(512, 512, 3, 2), 6_144);
        // A tiny image floors at FIRST_PACKET_SIZE.
        assert_eq!(discard_data_size(8, 8, 3, 0), FIRST_PACKET_SIZE);
    }

    #[test]
    fn truncate_keeps_full_data_at_discard_zero_and_prefix_otherwise() {
        let mut data = synth_header(512, 512, 3, 5);
        data.resize(40_000, 0xAB); // pad to a plausible codestream length
        assert_eq!(truncate_to_discard(&data, 0).len(), data.len());
        // discard 1 estimate is 24576 bytes, which fits inside 40000.
        assert_eq!(truncate_to_discard(&data, 1).len(), 24_576);
        // A non-J2C blob is returned unchanged.
        let junk = [0_u8; 16];
        assert_eq!(truncate_to_discard(&junk, 2).len(), junk.len());
    }
}
