//! JPEG-2000 decoding to canonical RGBA8 and pixel downsampling for
//! level-of-detail downgrades.
//!
//! [`decode_j2c`] decodes a (possibly truncated) `.j2c` codestream directly to a
//! target [`DiscardLevel`] using OpenJPEG's resolution reduction (via the
//! `jpeg2k` crate, behind the default-on `decode` feature), then canonicalises
//! whatever component layout OpenJPEG produced to 8-bit RGBA. [`downsample`]
//! produces a coarser image from an already-decoded one with a box filter — no
//! re-decode — which is how a texture's in-memory LOD is *lowered* to reclaim
//! memory.

use bytes::Bytes;
use sl_proto::DiscardLevel;

/// A decoded texture: canonical 8-bit RGBA pixels plus geometry and the LOD the
/// pixels were decoded (or downsampled) to.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    /// Decoded image width in pixels (at [`Self::discard_level`]).
    pub width: u32,
    /// Decoded image height in pixels (at [`Self::discard_level`]).
    pub height: u32,
    /// The source codestream's component count (1 = grey, 3 = RGB, 4 = RGBA,
    /// 5 = a Second Life server "Sunshine" avatar bake, `R G B alpha mask`),
    /// retained as metadata; [`Self::pixels`] is always expanded to RGBA8 from
    /// the first four source channels, and a 5th channel is kept in
    /// [`Self::aux`].
    pub components: u16,
    /// The level of detail these pixels represent.
    pub discard_level: DiscardLevel,
    /// Tightly packed 8-bit RGBA pixels, `width * height * 4` bytes, row-major.
    /// For a Second Life 5-component bake this is `R G B alpha` — the first four
    /// channels, alpha included (matching the reference viewer's
    /// `decodeChannels(.., 0, 4)`).
    pub pixels: Bytes,
    /// The auxiliary 5th channel of a Second Life server avatar bake — the
    /// clothing/bump "mask" (`M` of the `RGBHM` layout) the reference viewer
    /// decodes separately (`decodeChannels(.., 4, 4)`) as the morph/material
    /// mask — one byte per pixel, `width * height` bytes, row-major. `None` for
    /// any source with four or fewer components.
    pub aux: Option<Bytes>,
    /// The smallest alpha byte over all [`Self::pixels`], computed once at
    /// construction (off the frame thread — see [`alpha_range`]) so consumers
    /// classify transparency without rescanning the image:
    /// `min_alpha < cutoff` ⇔ "at least one texel below cutoff". `255` for a
    /// source without an alpha channel (the decoder fills alpha opaque) and
    /// for an empty image.
    pub min_alpha: u8,
    /// The largest alpha byte over all [`Self::pixels`] (see
    /// [`Self::min_alpha`]): `max_alpha < cutoff` ⇔ "every texel below
    /// cutoff". `0` for an empty image — test [`Self::min_alpha`] first.
    pub max_alpha: u8,
}

/// The `(min, max)` alpha byte over tightly packed RGBA8 `pixels` — the
/// one-pass scan backing [`DecodedImage::min_alpha`] / \
/// [`DecodedImage::max_alpha`], run where the pixels are produced (the decode
/// / composite tasks, off the frame thread). `(255, 0)` for an empty slice.
#[must_use]
pub fn alpha_range(pixels: &[u8]) -> (u8, u8) {
    let mut min = u8::MAX;
    let mut max = u8::MIN;
    for &alpha in pixels.iter().skip(3).step_by(RGBA_CHANNELS) {
        min = min.min(alpha);
        max = max.max(alpha);
    }
    (min, max)
}

impl DecodedImage {
    /// Build a decoded image, computing [`Self::min_alpha`] /
    /// [`Self::max_alpha`] from `pixels` (see [`alpha_range`]). Prefer this
    /// over a struct literal so no construction site can forget the
    /// precomputed alpha stats.
    #[must_use]
    pub fn new(
        width: u32,
        height: u32,
        components: u16,
        discard_level: DiscardLevel,
        pixels: Bytes,
        aux: Option<Bytes>,
    ) -> Self {
        let (min_alpha, max_alpha) = alpha_range(&pixels);
        Self {
            width,
            height,
            components,
            discard_level,
            pixels,
            aux,
            min_alpha,
            max_alpha,
        }
    }

    /// The number of bytes [`Self::pixels`] should contain for this geometry
    /// (`width * height * 4`), saturating rather than overflowing.
    #[must_use]
    pub fn expected_len(&self) -> usize {
        let width = usize::try_from(self.width).unwrap_or(0);
        let height = usize::try_from(self.height).unwrap_or(0);
        width.saturating_mul(height).saturating_mul(RGBA_CHANNELS)
    }
}

/// The number of channels in a canonical RGBA8 pixel.
const RGBA_CHANNELS: usize = 4;

/// The default alpha applied when the source has no alpha channel.
const OPAQUE_ALPHA: u8 = 255;

/// A texture decode failure.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `DecodeError` reads clearly"
)]
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// The `decode` feature is disabled, so no JPEG-2000 decoder is linked.
    #[error("texture decoding is disabled (the `decode` feature is off)")]
    Disabled,
    /// The underlying JPEG-2000 decoder rejected the codestream.
    #[error("JPEG-2000 decode failed: {0}")]
    Codec(String),
    /// The decoder returned an empty or malformed image.
    #[error("decoded image was empty or had zero dimensions")]
    Empty,
    /// The codestream's `SIZ` segment declares an image outside the protocol's
    /// caps ([`sl_proto::j2c::Header::within_limits`]), so it is refused
    /// before OpenJPEG is asked to allocate for it.
    #[error(
        "JPEG-2000 header declares an out-of-range image: {width}x{height}, {components} components"
    )]
    OutOfRange {
        /// The width the header claimed.
        width: u32,
        /// The height the header claimed.
        height: u32,
        /// The component count the header claimed.
        components: u16,
    },
}

/// Decodes a `.j2c` codestream to RGBA8 at `discard_level`, using OpenJPEG's
/// resolution reduction so only the requested level of detail is reconstructed.
///
/// A truncated codestream (a fetched LOD prefix) decodes to the resolution its
/// bytes cover; the decoder is asked to reduce by the discard level regardless.
///
/// # Errors
///
/// Returns [`DecodeError::Disabled`] when built without the `decode` feature,
/// [`DecodeError::OutOfRange`] when the codestream header declares an image
/// beyond the protocol's size caps, [`DecodeError::Codec`] when OpenJPEG
/// rejects the data, and [`DecodeError::Empty`] when the decoded image has no
/// pixels.
#[cfg(feature = "decode")]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `decode_j2c` reads clearly"
)]
pub fn decode_j2c(
    codestream: &[u8],
    discard_level: DiscardLevel,
) -> Result<DecodedImage, DecodeError> {
    use jpeg2k::{DecodeParameters, Image};

    // OpenJPEG sizes its own buffers from the `SIZ` segment, and `to_rgba8`
    // then allocates `width * height * 4` on top — both driven by numbers that
    // came off the wire. Refuse an image the protocol cannot produce before
    // either allocation happens. A codestream whose header is unparsable is
    // left to OpenJPEG, which rejects it with a codec error.
    if let Some(header) = sl_proto::j2c::parse_header_unvalidated(codestream)
        && !header.within_limits()
    {
        return Err(DecodeError::OutOfRange {
            width: header.width,
            height: header.height,
            components: header.components,
        });
    }

    let params = DecodeParameters::default().reduce(discard_level.reduce_factor());
    let image = Image::from_bytes_with(codestream, params)
        .map_err(|error| DecodeError::Codec(error.to_string()))?;
    let components = u16::try_from(image.num_components()).unwrap_or(0);
    // A Second Life server ("Sunshine") avatar bake is a 5-component J2C
    // (`R G B alpha mask`), which `jpeg2k`'s `get_pixels` rejects (it only maps
    // 1–4 components), so it is read channel by channel instead — keeping the
    // composited alpha and the aux mask (see `decode_multicomponent`).
    if image.num_components() > 4 {
        return decode_multicomponent(&image, discard_level);
    }
    let data = image
        .get_pixels(Some(u32::from(OPAQUE_ALPHA)))
        .map_err(|error| DecodeError::Codec(error.to_string()))?;
    if data.width == 0 || data.height == 0 {
        return Err(DecodeError::Empty);
    }
    let pixels = to_rgba8(&data.data);
    if pixels.is_empty() {
        return Err(DecodeError::Empty);
    }
    let (min_alpha, max_alpha) = alpha_range(&pixels);
    Ok(DecodedImage {
        width: data.width,
        height: data.height,
        components,
        discard_level,
        pixels: Bytes::from(pixels),
        aux: None,
        min_alpha,
        max_alpha,
    })
}

/// Decodes a J2C with more than four components — a Second Life server "Sunshine"
/// avatar bake, whose 5 channels are `R G B alpha mask` (the reference viewer's
/// `RGBHM`: colour, heightfield/alpha, clothing mask) — into RGBA8 plus the
/// separate aux mask channel.
///
/// `jpeg2k`'s [`get_pixels`](jpeg2k::Image::get_pixels) only maps 1–4 components,
/// so a 5-component bake is read here from its individual components instead. The
/// first four full-resolution channels become the RGBA8 [`pixels`](DecodedImage::pixels)
/// — keeping the composited alpha (channel 3), which is what makes a hair bake
/// soft or a bald/mesh-hair bake transparent (R16) — matching the reference
/// viewer's `decodeChannels(.., 0, 4)`. The 5th channel (the clothing/bump mask)
/// is kept in [`aux`](DecodedImage::aux), mirroring the reference viewer's second
/// `decodeChannels(.., 4, 4)` pass; a channel whose sample resolution differs from
/// the image (a subsampled aux) is dropped rather than misaligned.
#[cfg(feature = "decode")]
fn decode_multicomponent(
    image: &jpeg2k::Image,
    discard_level: DiscardLevel,
) -> Result<DecodedImage, DecodeError> {
    let width = image.width();
    let height = image.height();
    let pixel_count = usize::try_from(width)
        .unwrap_or(0)
        .saturating_mul(usize::try_from(height).unwrap_or(0));
    if pixel_count == 0 {
        return Err(DecodeError::Empty);
    }
    let comps = image.components();
    // Each source component as full 8-bit samples; a component absent or at a
    // different resolution than the image yields an empty vec (handled per use).
    let channel = |index: usize| -> Vec<u8> {
        comps
            .get(index)
            .map(|comp| comp.data_u8().collect::<Vec<u8>>())
            .filter(|samples| samples.len() >= pixel_count)
            .unwrap_or_default()
    };
    let red = channel(0);
    if red.len() < pixel_count {
        return Err(DecodeError::Empty);
    }
    let green = channel(1);
    let blue = channel(2);
    // The composited alpha (channel 3); a source lacking it stays fully opaque.
    let alpha = channel(3);
    let mut pixels = Vec::with_capacity(pixel_count.saturating_mul(4));
    for index in 0..pixel_count {
        let r = red.get(index).copied().unwrap_or(0);
        pixels.push(r);
        pixels.push(green.get(index).copied().unwrap_or(r));
        pixels.push(blue.get(index).copied().unwrap_or(r));
        pixels.push(alpha.get(index).copied().unwrap_or(OPAQUE_ALPHA));
    }
    // The clothing/bump mask (channel 4), kept for later material use; dropped if
    // the source has no full-resolution 5th channel.
    let aux = {
        let mask = channel(4);
        (mask.len() >= pixel_count).then(|| Bytes::from(mask))
    };
    let (min_alpha, max_alpha) = alpha_range(&pixels);
    Ok(DecodedImage {
        width,
        height,
        components: u16::try_from(comps.len()).unwrap_or(0),
        discard_level,
        pixels: Bytes::from(pixels),
        aux,
        min_alpha,
        max_alpha,
    })
}

/// Stub used when the `decode` feature is disabled: always fails so the rest of
/// the crate can still compile and run without the OpenJPEG C dependency.
///
/// # Errors
///
/// Always returns [`DecodeError::Disabled`].
#[cfg(not(feature = "decode"))]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `decode_j2c` reads clearly"
)]
pub fn decode_j2c(
    _codestream: &[u8],
    _discard_level: DiscardLevel,
) -> Result<DecodedImage, DecodeError> {
    Err(DecodeError::Disabled)
}

/// Reduces a 16-bit sample to 8 bits by keeping its high byte.
#[cfg(feature = "decode")]
fn narrow(sample: u16) -> u8 {
    u8::try_from(sample >> 8_u16).unwrap_or(0)
}

/// Expands any of `jpeg2k`'s pixel layouts to tightly packed RGBA8. Grey
/// channels are broadcast to R/G/B; 16-bit samples are reduced to their high
/// byte; a missing alpha channel defaults to fully opaque.
#[cfg(feature = "decode")]
fn to_rgba8(data: &jpeg2k::ImagePixelData) -> Vec<u8> {
    use jpeg2k::ImagePixelData;

    match data {
        ImagePixelData::L8(values) => values
            .iter()
            .flat_map(|&l| [l, l, l, OPAQUE_ALPHA])
            .collect(),
        ImagePixelData::La8(values) => values
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[l, a]| [l, l, l, a])
            .collect(),
        ImagePixelData::Rgb8(values) => values
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, OPAQUE_ALPHA])
            .collect(),
        ImagePixelData::Rgba8(values) => values.clone(),
        ImagePixelData::L16(values) => values
            .iter()
            .flat_map(|&l| {
                let l = narrow(l);
                [l, l, l, OPAQUE_ALPHA]
            })
            .collect(),
        ImagePixelData::La16(values) => values
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[l, a]| {
                let l = narrow(l);
                [l, l, l, narrow(a)]
            })
            .collect(),
        ImagePixelData::Rgb16(values) => values
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [narrow(r), narrow(g), narrow(b), OPAQUE_ALPHA])
            .collect(),
        ImagePixelData::Rgba16(values) => values
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| [narrow(r), narrow(g), narrow(b), narrow(a)])
            .collect(),
    }
}

/// Produces a coarser copy of `image` at `target` by box-filter downsampling its
/// RGBA8 pixels (halving both dimensions once per discard step), without any
/// re-decode. Returns `image` unchanged if `target` is not strictly coarser, or
/// if the geometry is degenerate.
///
/// This is how an in-memory texture's level of detail is *lowered* to reclaim
/// memory: a `1024²` RGBA image (4 MiB) downsampled to discard level 2 is a
/// `256²` image (256 KiB), computed from pixels already in hand.
///
/// A small image runs out of halvings before it reaches `target` — a `2²` image
/// cannot be quartered twice. The result is then labelled with the level it
/// actually reached, not the one that was asked for: the
/// [`discard_level`](DecodedImage::discard_level) is what every LOD decision
/// downstream reads, so it has to describe the pixels in hand.
#[must_use]
pub fn downsample(image: &DecodedImage, target: DiscardLevel) -> DecodedImage {
    if target.get() <= image.discard_level.get() {
        return image.clone();
    }
    let steps = target.get().saturating_sub(image.discard_level.get());
    let mut width = image.width;
    let mut height = image.height;
    let mut level = image.discard_level;
    let mut pixels = image.pixels.to_vec();
    // Downsample the aux mask channel (R16) in lockstep so a lowered LOD keeps it.
    let mut aux = image.aux.as_ref().map(|mask| mask.to_vec());
    for _step in 0..steps {
        if width <= 1 || height <= 1 {
            break;
        }
        let (halved, next_width, next_height) =
            halve_channels(&pixels, width, height, RGBA_CHANNELS);
        if let Some(mask) = &aux {
            aux = Some(halve_channels(mask, width, height, 1).0);
        }
        pixels = halved;
        width = next_width;
        height = next_height;
        level = level.coarser();
    }
    let (min_alpha, max_alpha) = alpha_range(&pixels);
    DecodedImage {
        width,
        height,
        components: image.components,
        discard_level: level,
        pixels: Bytes::from(pixels),
        aux: aux.map(Bytes::from),
        min_alpha,
        max_alpha,
    }
}

/// Reads one channel byte at `base + channel` of an RGBA8 buffer, treating an
/// out-of-range index as 0 (used by the box filter at image edges).
fn sample(pixels: &[u8], base: usize, channel: usize) -> u16 {
    base.checked_add(channel)
        .and_then(|index| pixels.get(index))
        .copied()
        .map_or(0, u16::from)
}

/// Halves an RGBA8 image once with a 2×2 box filter — a [`halve_channels`] at the
/// canonical [`RGBA_CHANNELS`] stride.
#[cfg(test)]
fn halve_rgba8(pixels: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    halve_channels(pixels, width, height, RGBA_CHANNELS)
}

/// Halves an interleaved `channels`-per-pixel image once with a 2×2 box filter,
/// returning the new pixels and dimensions. Each output channel is the average of
/// the four covered input samples. Assumes `pixels` holds `width * height *
/// channels` bytes (RGBA8 body pixels, or a single-channel aux mask).
fn halve_channels(pixels: &[u8], width: u32, height: u32, channels: usize) -> (Vec<u8>, u32, u32) {
    let out_width = (width >> 1_u32).max(1);
    let out_height = (height >> 1_u32).max(1);
    let width_usize = usize::try_from(width).unwrap_or(0);
    let stride = width_usize.saturating_mul(channels);
    let out_w = usize::try_from(out_width).unwrap_or(0);
    let out_h = usize::try_from(out_height).unwrap_or(0);
    let mut out = Vec::with_capacity(out_w.saturating_mul(out_h).saturating_mul(channels));

    for out_y in 0..out_h {
        let top = out_y.saturating_mul(2).saturating_mul(stride);
        let bottom = top.saturating_add(stride);
        for out_x in 0..out_w {
            let left = out_x.saturating_mul(2).saturating_mul(channels);
            let base00 = top.saturating_add(left);
            let base01 = base00.saturating_add(channels);
            let base10 = bottom.saturating_add(left);
            let base11 = base10.saturating_add(channels);
            for channel in 0..channels {
                let sum = sample(pixels, base00, channel)
                    .saturating_add(sample(pixels, base01, channel))
                    .saturating_add(sample(pixels, base10, channel))
                    .saturating_add(sample(pixels, base11, channel));
                out.push(u8::try_from(sum >> 2_u16).unwrap_or(0));
            }
        }
    }
    (out, out_width, out_height)
}

#[cfg(test)]
mod tests {
    use super::{DecodedImage, downsample, halve_rgba8};
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_proto::DiscardLevel;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// A solid-colour RGBA image of the given size.
    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> DecodedImage {
        let count = usize::try_from(width)
            .unwrap_or(0)
            .saturating_mul(usize::try_from(height).unwrap_or(0));
        let pixels: Vec<u8> = std::iter::repeat_n(rgba, count).flatten().collect();
        DecodedImage::new(
            width,
            height,
            4,
            DiscardLevel::FULL,
            Bytes::from(pixels),
            None,
        )
    }

    #[test]
    fn halve_averages_a_2x2_block() {
        // One 2x2 block with channel-0 values 0, 100, 200, 40 -> average 85.
        let pixels = vec![
            0, 0, 0, 255, 100, 0, 0, 255, // row 0
            200, 0, 0, 255, 40, 0, 0, 255, // row 1
        ];
        let (out, w, h) = halve_rgba8(&pixels, 2, 2);
        assert_eq!((w, h), (1, 1));
        // (0 + 100 + 200 + 40) / 4 = 85; alpha averages back to 255.
        assert_eq!(out, vec![85, 0, 0, 255]);
    }

    #[test]
    fn downsample_reduces_dimensions_and_sets_level() -> Result<(), TestError> {
        let image = solid(8, 8, [10, 20, 30, 255]);
        let two = DiscardLevel::new(2).ok_or("level 2")?;
        let out = downsample(&image, two);
        // Two halving steps: 8 -> 4 -> 2.
        assert_eq!((out.width, out.height), (2, 2));
        assert_eq!(out.discard_level, two);
        // Averaging a solid colour leaves it unchanged.
        assert_eq!(out.pixels.first(), Some(&10));
        assert_eq!(out.pixels.len(), out.expected_len());
        Ok(())
    }

    #[test]
    fn downsample_noop_when_not_coarser() {
        let image = solid(4, 4, [1, 2, 3, 4]);
        let same = downsample(&image, DiscardLevel::FULL);
        assert_eq!(same.width, 4);
        assert_eq!(same.height, 4);
        assert_eq!(same.discard_level, DiscardLevel::FULL);
    }

    #[test]
    fn downsample_reports_the_level_it_could_reach() -> Result<(), TestError> {
        // A 4x4 image has two halvings in it (4 -> 2 -> 1), so a request for
        // discard 4 stops at 2 — and must say so, rather than claiming a level
        // its pixels never reached.
        let image = solid(4, 4, [10, 20, 30, 255]);
        let four = DiscardLevel::new(4).ok_or("level 4")?;
        let out = downsample(&image, four);
        assert_eq!((out.width, out.height), (1, 1));
        assert_eq!(out.discard_level, DiscardLevel::new(2).ok_or("level 2")?);

        // A 1x1 image cannot halve at all: the level is unchanged.
        let pixel = solid(1, 1, [1, 2, 3, 4]);
        let stuck = downsample(&pixel, four);
        assert_eq!((stuck.width, stuck.height), (1, 1));
        assert_eq!(stuck.discard_level, DiscardLevel::FULL);

        // A non-square image is limited by its shorter axis: 8x2 halves once
        // (to 4x1) and then stops.
        let wide = solid(8, 2, [5, 6, 7, 8]);
        let out = downsample(&wide, four);
        assert_eq!((out.width, out.height), (4, 1));
        assert_eq!(out.discard_level, DiscardLevel::new(1).ok_or("level 1")?);
        Ok(())
    }

    /// Builds a minimal J2C main header (`SOC` + `SIZ`) declaring the given
    /// geometry — enough for the decoder's pre-check to read it.
    #[cfg(feature = "decode")]
    fn synth_j2c_header(width: u32, height: u32, components: u16) -> Vec<u8> {
        /// Appends `value` as `width` big-endian bytes (avoiding the
        /// endian-byte-method lint).
        fn push_be(data: &mut Vec<u8>, value: u32, width: u32) {
            let mut shift = width.saturating_mul(8);
            while shift >= 8 {
                shift = shift.saturating_sub(8);
                data.push(u8::try_from((value >> shift) & 0xFF).unwrap_or(0));
            }
        }
        let mut data = vec![0xFF, 0x4F, 0xFF, 0x51]; // SOC, SIZ
        push_be(&mut data, 38, 2); // Lsiz
        push_be(&mut data, 0, 2); // Rsiz
        for value in [width, height, 0, 0, width, height, 0, 0] {
            push_be(&mut data, value, 4); // Xsiz..YTOsiz
        }
        push_be(&mut data, u32::from(components), 2); // Csiz
        data.extend_from_slice(&[7, 1, 1]); // one component descriptor
        data
    }

    #[test]
    #[cfg(feature = "decode")]
    fn decode_refuses_an_out_of_range_header_before_allocating() -> Result<(), TestError> {
        use super::{DecodeError, decode_j2c};

        // Sixteen times the 4096 cap in each axis: a header claiming this would
        // have OpenJPEG (and then `to_rgba8`) allocate from a number that came
        // off the wire.
        const HUGE: u32 = 0x0001_0000;

        let data = synth_j2c_header(HUGE, HUGE, 8);
        let Err(DecodeError::OutOfRange {
            width,
            height,
            components,
        }) = decode_j2c(&data, DiscardLevel::FULL)
        else {
            return Err("an out-of-range header should be refused".into());
        };
        assert_eq!((width, height, components), (HUGE, HUGE, 8));
        Ok(())
    }

    #[test]
    #[cfg(feature = "decode")]
    fn decode_leaves_an_in_range_header_to_the_codec() {
        use super::{DecodeError, decode_j2c};

        // A header within the caps but with no codestream body behind it is a
        // codec error, not a range refusal — the pre-check must not swallow the
        // ordinary "malformed data" path.
        let data = synth_j2c_header(512, 512, 3);
        assert!(matches!(
            decode_j2c(&data, DiscardLevel::FULL),
            Err(DecodeError::Codec(_))
        ));
        // Neither does it refuse a blob with no recognisable header at all.
        assert!(matches!(
            decode_j2c(&[0_u8; 32], DiscardLevel::FULL),
            Err(DecodeError::Codec(_))
        ));
    }
}
