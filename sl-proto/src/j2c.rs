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

    /// A safe upper bound on the *full-resolution* codestream length: the
    /// uncompressed pixel-byte count (`width * height * components`). A JPEG-2000
    /// codestream is always smaller than its raw pixels for real Second Life
    /// textures, so fetching this many leading bytes is guaranteed to cover the
    /// entire codestream.
    ///
    /// Unlike [`Self::discard_data_size`]'s `1/8`-rate *estimate* — a valid
    /// prefix boundary only for coarser LODs — this is what a full-resolution
    /// (discard 0) fetch must use: the estimate can fall short of a
    /// poorly-compressing texture's true length and truncate it mid-tile-part,
    /// which OpenJPEG then rejects. Never below [`FIRST_PACKET_SIZE`].
    #[must_use]
    pub fn full_data_size_bound(&self) -> usize {
        let raw = u64::from(self.width)
            .saturating_mul(u64::from(self.height))
            .saturating_mul(u64::from(self.components));
        usize::try_from(raw)
            .unwrap_or(usize::MAX)
            .max(FIRST_PACKET_SIZE)
    }
}

/// The highest meaningful discard (LOD) level in the Second Life protocol,
/// mirroring the viewer's `MAX_DISCARD_LEVEL`. Each level halves both image
/// dimensions, so level 5 is a 1/32-scale thumbnail of the full image.
pub const MAX_DISCARD_LEVEL: u8 = 5;

/// A texture level-of-detail: a discard level in `0..=`[`MAX_DISCARD_LEVEL`],
/// where `0` is full resolution and larger values are coarser (each step halves
/// both dimensions). The newtype makes out-of-range levels unrepresentable and
/// gives the LOD its own idiomatic operations.
///
/// Ordering follows resolution: a *smaller* discard level is *finer* (higher
/// resolution), so `DiscardLevel::FULL` is the minimum and
/// [`DiscardLevel::MAX`] the maximum. "At least as fine as" is therefore `<=`.
///
/// This types the HTTP `GetTexture` fetch/decode LOD path. It intentionally does
/// **not** replace the signed discard field of the UDP `RequestImage` message
/// (`Command::RequestTexture`), whose `-1` sentinel cancels an in-flight request
/// and so needs its own signed representation.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DiscardLevel(u8);

impl DiscardLevel {
    /// Full resolution (discard level `0`) — the finest, largest image.
    pub const FULL: Self = Self(0);

    /// The coarsest supported level ([`MAX_DISCARD_LEVEL`]).
    pub const MAX: Self = Self(MAX_DISCARD_LEVEL);

    /// Builds a level, returning `None` if it exceeds [`MAX_DISCARD_LEVEL`].
    #[must_use]
    pub const fn new(level: u8) -> Option<Self> {
        if level > MAX_DISCARD_LEVEL {
            None
        } else {
            Some(Self(level))
        }
    }

    /// Builds a level, clamping anything above [`MAX_DISCARD_LEVEL`] to
    /// [`DiscardLevel::MAX`].
    #[must_use]
    pub const fn from_clamped(level: u8) -> Self {
        if level > MAX_DISCARD_LEVEL {
            Self::MAX
        } else {
            Self(level)
        }
    }

    /// The raw discard level (`0..=`[`MAX_DISCARD_LEVEL`]).
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// The OpenJPEG resolution-reduction factor for decoding directly to this
    /// LOD: the number of resolution levels to discard, i.e. the level itself.
    #[must_use]
    pub fn reduce_factor(self) -> u32 {
        u32::from(self.0)
    }

    /// The next finer (higher-resolution) level, saturating at
    /// [`DiscardLevel::FULL`].
    #[must_use]
    pub const fn finer(self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    /// The next coarser (lower-resolution) level, saturating at
    /// [`DiscardLevel::MAX`].
    #[must_use]
    pub const fn coarser(self) -> Self {
        let next = self.0.saturating_add(1);
        if next > MAX_DISCARD_LEVEL {
            Self::MAX
        } else {
            Self(next)
        }
    }

    /// Whether this is full resolution (discard level `0`).
    #[must_use]
    pub const fn is_full(self) -> bool {
        self.0 == 0
    }

    /// Whether this level is at least as fine (high-resolution) as `other`,
    /// i.e. its discard level is less than or equal to `other`'s.
    #[must_use]
    pub const fn is_at_least_as_fine_as(self, other: Self) -> bool {
        self.0 <= other.0
    }

    /// The estimated leading-byte count of `header`'s codestream needed to
    /// decode the image at this level (delegates to [`Header::discard_data_size`]).
    #[must_use]
    pub fn data_size(self, header: &Header) -> usize {
        header.discard_data_size(self.0)
    }

    /// Clamps to a level no coarser than `header` supports — i.e. no greater
    /// than the image's [`Header::max_discard_level`] — since coarser levels add
    /// no distinct resolution for that image.
    #[must_use]
    pub fn clamp_to_image(self, header: &Header) -> Self {
        let max = header.max_discard_level();
        if self.0 > max { Self(max) } else { self }
    }

    /// Selects the discard level whose decoded resolution matches a desired
    /// on-screen `pixel_area`, given the texture's full (discard-0) pixel
    /// dimensions.
    ///
    /// Ports the reference viewer's
    /// `discard = floor(log4(full_texels / virtual_size))`
    /// (`LLViewerLODTexture::processTextureStats`): each discard step quarters
    /// the texel count, so the base-4 logarithm of the ratio of full texels to
    /// the on-screen pixel area is the number of resolution levels to discard.
    /// Fewer on-screen pixels ⇒ a coarser (larger) level; an object covering at
    /// least the full texel count selects [`DiscardLevel::FULL`]. Clamps into
    /// `[FULL, MAX]`; a non-positive or non-finite area (an off-screen /
    /// behind-camera object) selects [`DiscardLevel::MAX`].
    #[must_use]
    pub fn for_pixel_area(pixel_area: f32, full_width: u32, full_height: u32) -> Self {
        let texels = f64::from(full_width) * f64::from(full_height);
        let area = f64::from(pixel_area);
        if !area.is_finite() || area <= 0.0 || texels <= 0.0 {
            return Self::MAX;
        }
        // floor(log4(full_texels / on-screen area)) by repeated division by
        // four — each discard level quarters the texel count. Avoids a float
        // `log` (and its lossy back-cast to the integer level).
        let mut ratio = texels / area;
        let mut level = 0_u8;
        while ratio >= 4.0 && level < MAX_DISCARD_LEVEL {
            ratio /= 4.0;
            level = level.saturating_add(1);
        }
        Self(level)
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

    use super::{
        DiscardLevel, FIRST_PACKET_SIZE, MAX_DISCARD_LEVEL, discard_data_size, parse_header,
        truncate_to_discard,
    };

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
    fn full_data_size_bound_is_the_uncompressed_pixel_count() -> Result<(), TestError> {
        let header = parse_header(&synth_header(512, 512, 4, 5)).ok_or("header")?;
        // The uncompressed size (512*512*4) bounds the codestream, and is far
        // larger than the 1/8-rate discard-0 estimate that could truncate it.
        assert_eq!(header.full_data_size_bound(), 512 * 512 * 4);
        assert!(header.full_data_size_bound() > header.discard_data_size(0));
        // A tiny image still floors at FIRST_PACKET_SIZE.
        let tiny = parse_header(&synth_header(4, 4, 3, 1)).ok_or("tiny header")?;
        assert_eq!(tiny.full_data_size_bound(), FIRST_PACKET_SIZE);
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
    fn discard_level_construction_validates_and_clamps() {
        assert_eq!(DiscardLevel::new(0), Some(DiscardLevel::FULL));
        assert_eq!(
            DiscardLevel::new(MAX_DISCARD_LEVEL),
            Some(DiscardLevel::MAX)
        );
        assert_eq!(DiscardLevel::new(MAX_DISCARD_LEVEL + 1), None);
        assert_eq!(DiscardLevel::from_clamped(200), DiscardLevel::MAX);
        assert_eq!(DiscardLevel::from_clamped(2).get(), 2);
    }

    #[test]
    fn discard_level_finer_coarser_saturate() {
        assert_eq!(DiscardLevel::FULL.finer(), DiscardLevel::FULL);
        assert_eq!(DiscardLevel::MAX.coarser(), DiscardLevel::MAX);
        assert_eq!(DiscardLevel::FULL.coarser().get(), 1);
        assert_eq!(DiscardLevel::MAX.finer().get(), MAX_DISCARD_LEVEL - 1);
        assert!(DiscardLevel::FULL.is_full());
        assert!(!DiscardLevel::MAX.is_full());
    }

    #[test]
    fn discard_level_ordering_is_finer_first() {
        // Smaller discard level = finer = "less than".
        assert!(DiscardLevel::FULL < DiscardLevel::MAX);
        assert!(DiscardLevel::FULL.is_at_least_as_fine_as(DiscardLevel::MAX));
        assert!(!DiscardLevel::MAX.is_at_least_as_fine_as(DiscardLevel::FULL));
        let two = DiscardLevel::from_clamped(2);
        assert!(two.is_at_least_as_fine_as(two));
    }

    #[test]
    fn discard_level_for_pixel_area_matches_log4_of_the_texel_ratio() {
        // A 1024x1024 texture (1_048_576 texels). Each discard level quarters
        // the texel count, so the level is floor(log4(texels / on-screen area)).
        let full = |area: f32| DiscardLevel::for_pixel_area(area, 1024, 1024).get();
        // Covering the full texel count (or more) wants full resolution.
        assert_eq!(full(1024.0 * 1024.0), 0);
        assert_eq!(full(4_000_000.0), 0);
        // A quarter of the texels (512x512 on screen) is one level coarser.
        assert_eq!(full(512.0 * 512.0), 1);
        // 256x256 on screen ⇒ discard 2 (1024 → 512 → 256).
        assert_eq!(full(256.0 * 256.0), 2);
        // A vanishingly small area saturates at the coarsest level.
        assert_eq!(full(1.0), DiscardLevel::MAX.get());
        // A non-positive / non-finite area (off-screen, behind the camera) is
        // the coarsest level.
        assert_eq!(
            DiscardLevel::for_pixel_area(0.0, 1024, 1024),
            DiscardLevel::MAX
        );
        assert_eq!(
            DiscardLevel::for_pixel_area(-5.0, 1024, 1024),
            DiscardLevel::MAX
        );
        assert_eq!(
            DiscardLevel::for_pixel_area(f32::NAN, 1024, 1024),
            DiscardLevel::MAX
        );
        // A degenerate (zero-dimension) texture cannot be ranked ⇒ coarsest.
        assert_eq!(DiscardLevel::for_pixel_area(100.0, 0, 0), DiscardLevel::MAX);
    }

    #[test]
    fn discard_level_for_pixel_area_uses_the_native_dimensions() {
        // The same on-screen area picks a coarser level for a larger native
        // texture: 128x128 on screen (16_384 px) wants native/16_384 texels.
        let area = 128.0 * 128.0;
        // 2048x2048 native ⇒ 4_194_304 / 16_384 = 256 ⇒ log4 = 4.
        assert_eq!(DiscardLevel::for_pixel_area(area, 2048, 2048).get(), 4);
        // 512x512 native ⇒ 262_144 / 16_384 = 16 ⇒ log4 = 2. Both resolve to a
        // 128x128 decoded image, the on-screen size.
        assert_eq!(DiscardLevel::for_pixel_area(area, 512, 512).get(), 2);
    }

    #[test]
    fn discard_level_clamp_to_image_respects_header() -> Result<(), TestError> {
        let header = parse_header(&synth_header(512, 512, 3, 3)).ok_or("header")?;
        // The image supports at most discard 3; a coarser request is clamped.
        assert_eq!(DiscardLevel::MAX.clamp_to_image(&header).get(), 3);
        // A finer request is left untouched.
        let one = DiscardLevel::from_clamped(1);
        assert_eq!(one.clamp_to_image(&header), one);
        // data_size matches the free function at the same level.
        assert_eq!(one.data_size(&header), discard_data_size(512, 512, 3, 1));
        Ok(())
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
