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
    /// The source codestream's component count (1 = grey, 3 = RGB, 4 = RGBA),
    /// retained as metadata; [`Self::pixels`] is always expanded to RGBA8.
    pub components: u16,
    /// The level of detail these pixels represent.
    pub discard_level: DiscardLevel,
    /// Tightly packed 8-bit RGBA pixels, `width * height * 4` bytes, row-major.
    pub pixels: Bytes,
}

impl DecodedImage {
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
/// [`DecodeError::Codec`] when OpenJPEG rejects the data, and
/// [`DecodeError::Empty`] when the decoded image has no pixels.
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

    let params = DecodeParameters::default().reduce(discard_level.reduce_factor());
    let image = Image::from_bytes_with(codestream, params)
        .map_err(|error| DecodeError::Codec(error.to_string()))?;
    let components = u16::try_from(image.num_components()).unwrap_or(0);
    // A Second Life server ("Sunshine") avatar bake is a 5-component J2C
    // (`R, G, B, bump, clothing`), which `jpeg2k`'s `get_pixels` rejects (it only
    // maps 1–4 components). Take the diffuse RGB from the first three channels and
    // drop the bump/clothing aux channels (opaque alpha), matching the reference
    // viewer, which decodes a baked texture with `decodeChannels(.., 0, 4)` and
    // uses just the colour for the opaque body.
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
    Ok(DecodedImage {
        width: data.width,
        height: data.height,
        components,
        discard_level,
        pixels: Bytes::from(pixels),
    })
}

/// Decodes a J2C with more than four components — a Second Life server avatar bake
/// (`R, G, B, bump, clothing`) — into opaque RGBA, taking the diffuse RGB from the
/// first three components and dropping the bump/clothing aux channels.
///
/// `jpeg2k`'s [`get_pixels`](jpeg2k::Image::get_pixels) only maps 1–4 components,
/// so a 5-component bake is read here from its individual components instead. A
/// component whose sample resolution differs from the image (a subsampled aux
/// channel) is ignored — only the first three (full-resolution) colour channels
/// are used. Reports [`components`](DecodedImage::components) as 3 so the alpha is
/// treated as absent (a wholly opaque bake).
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
    // The first three components as full 8-bit channels; a monochrome source (only
    // one component) replicates its single channel across RGB.
    let channel = |index: usize| -> Vec<u8> {
        comps
            .get(index)
            .map_or_else(Vec::new, |comp| comp.data_u8().collect())
    };
    let red = channel(0);
    if red.len() < pixel_count {
        return Err(DecodeError::Empty);
    }
    let green = channel(1);
    let blue = channel(2);
    let mut pixels = Vec::with_capacity(pixel_count.saturating_mul(4));
    for index in 0..pixel_count {
        let r = red.get(index).copied().unwrap_or(0);
        pixels.push(r);
        pixels.push(green.get(index).copied().unwrap_or(r));
        pixels.push(blue.get(index).copied().unwrap_or(r));
        pixels.push(OPAQUE_ALPHA);
    }
    Ok(DecodedImage {
        width,
        height,
        components: 3,
        discard_level,
        pixels: Bytes::from(pixels),
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
            .chunks_exact(2)
            .filter_map(|la| match (la.first(), la.get(1)) {
                (Some(&l), Some(&a)) => Some([l, l, l, a]),
                _other => None,
            })
            .flatten()
            .collect(),
        ImagePixelData::Rgb8(values) => values
            .chunks_exact(3)
            .filter_map(|rgb| match (rgb.first(), rgb.get(1), rgb.get(2)) {
                (Some(&r), Some(&g), Some(&b)) => Some([r, g, b, OPAQUE_ALPHA]),
                _other => None,
            })
            .flatten()
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
            .chunks_exact(2)
            .filter_map(|la| match (la.first(), la.get(1)) {
                (Some(&l), Some(&a)) => {
                    let l = narrow(l);
                    Some([l, l, l, narrow(a)])
                }
                _other => None,
            })
            .flatten()
            .collect(),
        ImagePixelData::Rgb16(values) => values
            .chunks_exact(3)
            .filter_map(|rgb| match (rgb.first(), rgb.get(1), rgb.get(2)) {
                (Some(&r), Some(&g), Some(&b)) => {
                    Some([narrow(r), narrow(g), narrow(b), OPAQUE_ALPHA])
                }
                _other => None,
            })
            .flatten()
            .collect(),
        ImagePixelData::Rgba16(values) => values
            .chunks_exact(4)
            .filter_map(
                |rgba| match (rgba.first(), rgba.get(1), rgba.get(2), rgba.get(3)) {
                    (Some(&r), Some(&g), Some(&b), Some(&a)) => {
                        Some([narrow(r), narrow(g), narrow(b), narrow(a)])
                    }
                    _other => None,
                },
            )
            .flatten()
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
#[must_use]
pub fn downsample(image: &DecodedImage, target: DiscardLevel) -> DecodedImage {
    if target.get() <= image.discard_level.get() {
        return image.clone();
    }
    let steps = target.get().saturating_sub(image.discard_level.get());
    let mut width = image.width;
    let mut height = image.height;
    let mut pixels = image.pixels.to_vec();
    for _step in 0..steps {
        if width <= 1 || height <= 1 {
            break;
        }
        let (halved, next_width, next_height) = halve_rgba8(&pixels, width, height);
        pixels = halved;
        width = next_width;
        height = next_height;
    }
    DecodedImage {
        width,
        height,
        components: image.components,
        discard_level: target,
        pixels: Bytes::from(pixels),
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

/// Halves an RGBA8 image once with a 2×2 box filter, returning the new pixels and
/// dimensions. Each output channel is the average of the four covered input
/// samples. Assumes `pixels` holds `width * height * 4` bytes.
fn halve_rgba8(pixels: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let out_width = (width >> 1_u32).max(1);
    let out_height = (height >> 1_u32).max(1);
    let width_usize = usize::try_from(width).unwrap_or(0);
    let stride = width_usize.saturating_mul(RGBA_CHANNELS);
    let out_w = usize::try_from(out_width).unwrap_or(0);
    let out_h = usize::try_from(out_height).unwrap_or(0);
    let mut out = Vec::with_capacity(out_w.saturating_mul(out_h).saturating_mul(RGBA_CHANNELS));

    for out_y in 0..out_h {
        let top = out_y.saturating_mul(2).saturating_mul(stride);
        let bottom = top.saturating_add(stride);
        for out_x in 0..out_w {
            let left = out_x.saturating_mul(2).saturating_mul(RGBA_CHANNELS);
            let base00 = top.saturating_add(left);
            let base01 = base00.saturating_add(RGBA_CHANNELS);
            let base10 = bottom.saturating_add(left);
            let base11 = base10.saturating_add(RGBA_CHANNELS);
            for channel in 0..RGBA_CHANNELS {
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
        DecodedImage {
            width,
            height,
            components: 4,
            discard_level: DiscardLevel::FULL,
            pixels: Bytes::from(pixels),
        }
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
}
