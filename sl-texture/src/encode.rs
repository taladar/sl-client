//! JPEG-2000 *encoding* of canonical RGBA8 images to raw `.j2c` codestreams.
//!
//! [`encode_j2c`] is the inverse of [`decode_j2c`](crate::decode::decode_j2c):
//! it takes a [`DecodedImage`] (tightly packed 8-bit RGBA) and produces the raw
//! JPEG-2000 codestream Second Life / OpenSim stores textures as — the byte
//! form the `UploadBakedTexture` capability accepts. It exists so a client that
//! composites its own avatar bake (the client-side / legacy bake path) can
//! publish the result to the grid so the simulator and other viewers see it.
//!
//! The actual OpenJPEG encode (and the `unsafe` FFI it needs) lives in the
//! dedicated [`sl_j2c_encode`] crate; this is a thin, `DecodedImage`-shaped
//! wrapper co-located with [`decode_j2c`](crate::decode::decode_j2c), behind the
//! default-off `encode` feature so the encoder is only linked where publishing a
//! bake is needed.

use crate::decode::DecodedImage;

/// A texture encode failure.
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `EncodeError` reads clearly"
)]
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// The `encode` feature is disabled, so no JPEG-2000 encoder is linked.
    #[error("texture encoding is disabled (the `encode` feature is off)")]
    Disabled,
    /// The image has a zero dimension, so there is nothing to encode.
    #[error("cannot encode an image with zero width or height")]
    Empty,
    /// The pixel buffer does not hold `width * height * 4` bytes.
    #[error("pixel buffer is {got} bytes but {expected} were expected ({width}x{height} RGBA)")]
    PixelLen {
        /// The actual byte count of the pixel buffer.
        got: usize,
        /// The byte count required for `width * height * 4`.
        expected: usize,
        /// The image width the buffer was measured against.
        width: u32,
        /// The image height the buffer was measured against.
        height: u32,
    },
    /// The underlying JPEG-2000 encoder failed at some stage.
    #[error("JPEG-2000 encode failed: {0}")]
    Codec(String),
}

/// Encodes a canonical RGBA8 [`DecodedImage`] to a raw JPEG-2000 (`.j2c`)
/// codestream — the byte form Second Life stores textures as and the
/// `UploadBakedTexture` capability accepts.
///
/// A fully-opaque image is written with three (RGB) components; an image with
/// any non-opaque pixel keeps its alpha as a fourth component so an alpha-masked
/// bake round-trips its cut-outs. Encoding is lossy (the reference viewer's bake
/// path is too).
///
/// # Errors
///
/// Returns [`EncodeError::Disabled`] when built without the `encode` feature,
/// [`EncodeError::Empty`] for a zero-sized image, [`EncodeError::PixelLen`] when
/// the pixel buffer length does not match the geometry, and
/// [`EncodeError::Codec`] when the OpenJPEG encoder rejects the image or fails to
/// produce a codestream.
#[cfg(feature = "encode")]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `encode_j2c` reads clearly"
)]
pub fn encode_j2c(image: &DecodedImage) -> Result<Vec<u8>, EncodeError> {
    sl_j2c_encode::encode_rgba8(image.width, image.height, &image.pixels).map_err(|error| {
        match error {
            sl_j2c_encode::EncodeError::Empty => EncodeError::Empty,
            sl_j2c_encode::EncodeError::PixelLen {
                got,
                expected,
                width,
                height,
            } => EncodeError::PixelLen {
                got,
                expected,
                width,
                height,
            },
            sl_j2c_encode::EncodeError::Codec(message) => EncodeError::Codec(message),
        }
    })
}

/// Stub used when the `encode` feature is disabled: always fails so the rest of
/// the crate can still compile and run without the OpenJPEG dependency.
///
/// # Errors
///
/// Always returns [`EncodeError::Disabled`].
#[cfg(not(feature = "encode"))]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `encode_j2c` reads clearly"
)]
pub const fn encode_j2c(_image: &DecodedImage) -> Result<Vec<u8>, EncodeError> {
    Err(EncodeError::Disabled)
}

#[cfg(test)]
mod tests {
    use super::encode_j2c;
    use crate::decode::{DecodedImage, decode_j2c};
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sl_proto::DiscardLevel;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Build an RGBA8 test image from a per-pixel alpha function over an
    /// `x`/`y`-derived colour.
    fn gradient(width: u32, height: u32, alpha: impl Fn(u32, u32) -> u8) -> DecodedImage {
        let w = usize::try_from(width).unwrap_or(0);
        let h = usize::try_from(height).unwrap_or(0);
        let mut pixels = Vec::with_capacity(w.saturating_mul(h).saturating_mul(4));
        for y in 0..height {
            for x in 0..width {
                let r = u8::try_from(x % 256).unwrap_or(0);
                let g = u8::try_from(y % 256).unwrap_or(0);
                let b = u8::try_from((x ^ y) % 256).unwrap_or(0);
                pixels.extend_from_slice(&[r, g, b, alpha(x, y)]);
            }
        }
        DecodedImage {
            width,
            height,
            components: 4,
            discard_level: DiscardLevel::FULL,
            pixels: Bytes::from(pixels),
            aux: None,
        }
    }

    /// The mean absolute per-channel difference between two equal-length buffers.
    fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
        if a.is_empty() || a.len() != b.len() {
            return f64::INFINITY;
        }
        let total: u64 = a
            .iter()
            .zip(b)
            .map(|(x, y)| u64::from(x.abs_diff(*y)))
            .sum();
        let len = u32::try_from(a.len()).unwrap_or(1);
        f64::from(u32::try_from(total).unwrap_or(u32::MAX)) / f64::from(len)
    }

    #[test]
    #[cfg_attr(not(feature = "encode"), ignore = "requires the `encode` feature")]
    fn encodes_a_valid_j2c_codestream() -> Result<(), TestError> {
        let image = gradient(64, 64, |_x, _y| u8::MAX);
        let bytes = encode_j2c(&image)?;
        // A raw JPEG-2000 codestream starts with the SOC marker 0xFF4F.
        assert_eq!(bytes.first(), Some(&0xFF));
        assert_eq!(bytes.get(1), Some(&0x4F));
        Ok(())
    }

    #[test]
    #[cfg_attr(not(feature = "encode"), ignore = "requires the `encode` feature")]
    fn opaque_image_round_trips_through_decode() -> Result<(), TestError> {
        let image = gradient(64, 64, |_x, _y| u8::MAX);
        let bytes = encode_j2c(&image)?;
        let decoded = decode_j2c(&bytes, DiscardLevel::FULL)?;
        assert_eq!((decoded.width, decoded.height), (64, 64));
        // Lossy, but a smooth gradient should reconstruct closely.
        assert!(
            mean_abs_diff(&image.pixels, &decoded.pixels) < 8.0,
            "opaque round-trip drifted too far"
        );
        // Alpha stays fully opaque.
        assert!(
            decoded
                .pixels
                .chunks_exact(4)
                .all(|px| px.get(3) == Some(&u8::MAX))
        );
        Ok(())
    }

    #[test]
    #[cfg_attr(not(feature = "encode"), ignore = "requires the `encode` feature")]
    fn alpha_is_preserved_when_present() -> Result<(), TestError> {
        // Left half transparent, right half opaque.
        let image = gradient(64, 64, |x, _y| if x < 32 { 0 } else { u8::MAX });
        let bytes = encode_j2c(&image)?;
        let decoded = decode_j2c(&bytes, DiscardLevel::FULL)?;
        assert_eq!(decoded.components, 4, "alpha channel should survive");
        // The transparent and opaque regions keep their character.
        let first = decoded.pixels.get(3).copied().unwrap_or(255);
        let last_texel = decoded.pixels.len().saturating_sub(1);
        let last = decoded.pixels.get(last_texel).copied().unwrap_or(0);
        assert!(first < 64, "left half should decode near-transparent");
        assert!(last > 192, "right half should decode near-opaque");
        Ok(())
    }

    #[test]
    fn rejects_zero_sized_image() {
        let image = DecodedImage {
            width: 0,
            height: 4,
            components: 4,
            discard_level: DiscardLevel::FULL,
            pixels: Bytes::new(),
            aux: None,
        };
        // Errors either way: `Empty` with the encoder linked, `Disabled` without.
        let rejected = encode_j2c(&image).is_err();
        assert!(rejected, "zero-sized image should be rejected");
    }
}
