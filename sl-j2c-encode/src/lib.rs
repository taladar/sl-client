//! In-memory JPEG-2000 encoding of canonical RGBA8 images to raw `.j2c`
//! codestreams.
//!
//! [`encode_rgba8`] is the inverse of a JPEG-2000 *decode*: it takes tightly
//! packed 8-bit RGBA pixels and produces the raw JPEG-2000 codestream Second
//! Life / OpenSim stores textures as — the byte form the `UploadBakedTexture`
//! capability accepts. It exists so a client that composites its own avatar bake
//! (the client-side / legacy bake path) can publish the result to the grid so
//! the simulator and other viewers see it.
//!
//! Encoding runs entirely in memory (no temp file) through the OpenJPEG C
//! library (`openjpeg-sys`) — deliberately the *same* backend `jpeg2k` decodes
//! with, so only one OpenJPEG implementation is ever linked into a binary (the
//! pure-Rust `openjp2` port would export duplicate `#[no_mangle]` `opj_*` C
//! symbols that collide with it at link time). This crate is the *only* place in
//! the workspace that owns `unsafe` FFI, so the OpenJPEG bindings — and the raw
//! pointer bookkeeping they require — are isolated here behind one safe function.
//!
//! A fully-opaque image is encoded as three (RGB) components; one with any
//! transparency keeps its alpha as a fourth component, so an alpha-masked bake
//! round-trips its cut-outs. Encoding is lossy (the reference viewer's bake path
//! is too).

use core::ffi::c_void;

use openjpeg_sys::{
    OPJ_CODEC_FORMAT, OPJ_COLOR_SPACE, OPJ_OFF_T, OPJ_SIZE_T, opj_codec_t, opj_cparameters_t,
    opj_create_compress, opj_destroy_codec, opj_encode, opj_end_compress, opj_image_comptparm,
    opj_image_create, opj_image_destroy, opj_image_t, opj_set_default_encoder_parameters,
    opj_setup_encoder, opj_start_compress, opj_stream_default_create, opj_stream_destroy,
    opj_stream_set_seek_function, opj_stream_set_skip_function, opj_stream_set_user_data,
    opj_stream_set_write_function, opj_stream_t,
};

/// The number of channels in a canonical RGBA8 pixel (the input layout).
const RGBA_CHANNELS: usize = 4;

/// The lossy compression ratio requested of the encoder (`N`:1). Avatar bakes
/// are lossy in the reference viewer; a modest ratio keeps them visually intact
/// while bounding the uploaded size.
const COMPRESSION_RATIO: f32 = 8.0;

/// The maximum number of wavelet resolution levels to request (OpenJPEG's own
/// default); clamped down for images too small to be halved that many times.
const MAX_RESOLUTIONS: u32 = 6;

/// The C `true` value OpenJPEG's `OPJ_BOOL` uses.
const OPJ_TRUE: i32 = 1;

/// A JPEG-2000 encode failure.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
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
    /// The underlying OpenJPEG encoder failed at some stage.
    #[error("JPEG-2000 encode failed: {0}")]
    Codec(String),
}

/// Encodes `width`×`height` tightly-packed RGBA8 `pixels` to a raw JPEG-2000
/// (`.j2c`) codestream — the byte form Second Life stores textures as and the
/// `UploadBakedTexture` capability accepts.
///
/// A fully-opaque image is written with three (RGB) components; an image with
/// any non-opaque pixel keeps its alpha as a fourth component so an alpha-masked
/// bake round-trips its cut-outs. Encoding is lossy.
///
/// # Errors
///
/// Returns [`EncodeError::Empty`] for a zero-sized image, [`EncodeError::PixelLen`]
/// when the pixel buffer length does not match the geometry, and
/// [`EncodeError::Codec`] when the OpenJPEG encoder rejects the image or fails to
/// produce a codestream.
pub fn encode_rgba8(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, EncodeError> {
    if width == 0 || height == 0 {
        return Err(EncodeError::Empty);
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|texels| texels.checked_mul(RGBA_CHANNELS))
        .ok_or_else(|| EncodeError::Codec("image geometry overflows usize".to_owned()))?;
    if pixels.len() != expected {
        return Err(EncodeError::PixelLen {
            got: pixels.len(),
            expected,
            width,
            height,
        });
    }
    encode(width, height, pixels)
}

/// A growable, seekable byte sink for OpenJPEG's output stream. The J2K encoder
/// seeks backwards to patch tile-part lengths, so a plain append buffer will not
/// do — this tracks a write cursor and grows on demand.
#[derive(Default)]
struct MemStream {
    /// The accumulated codestream bytes (the maximum extent ever written).
    buf: Vec<u8>,
    /// The current write cursor.
    pos: usize,
}

/// OpenJPEG output-stream write callback: copy `count` bytes from `buffer` into
/// the sink at the cursor, growing it as needed, and advance.
unsafe extern "C" fn write_fn(
    buffer: *mut c_void,
    count: OPJ_SIZE_T,
    user: *mut c_void,
) -> OPJ_SIZE_T {
    if user.is_null() || buffer.is_null() {
        return OPJ_SIZE_T::MAX;
    }
    // SAFETY: `user` is the `MemStream` we registered.
    let mem = unsafe { &mut *user.cast::<MemStream>() };
    // SAFETY: `buffer`/`count` come from OpenJPEG and describe a valid readable
    // region of `count` bytes.
    let src = unsafe { core::slice::from_raw_parts(buffer.cast::<u8>(), count) };
    let end = mem.pos.saturating_add(count);
    if mem.buf.len() < end {
        mem.buf.resize(end, 0);
    }
    if let Some(dest) = mem.buf.get_mut(mem.pos..end) {
        dest.copy_from_slice(src);
    }
    mem.pos = end;
    count
}

/// OpenJPEG output-stream skip callback: advance the cursor by `count`,
/// zero-filling any gap.
unsafe extern "C" fn skip_fn(count: OPJ_OFF_T, user: *mut c_void) -> OPJ_OFF_T {
    let Ok(count_usize) = usize::try_from(count) else {
        return -1;
    };
    if user.is_null() {
        return -1;
    }
    // SAFETY: `user` is the `MemStream` we registered.
    let mem = unsafe { &mut *user.cast::<MemStream>() };
    let end = mem.pos.saturating_add(count_usize);
    if mem.buf.len() < end {
        mem.buf.resize(end, 0);
    }
    mem.pos = end;
    count
}

/// OpenJPEG output-stream seek callback: move the cursor to an absolute
/// position, extending the buffer if it lands past the end.
unsafe extern "C" fn seek_fn(pos: OPJ_OFF_T, user: *mut c_void) -> i32 {
    let Ok(pos_usize) = usize::try_from(pos) else {
        return 0;
    };
    if user.is_null() {
        return 0;
    }
    // SAFETY: `user` is the `MemStream` we registered.
    let mem = unsafe { &mut *user.cast::<MemStream>() };
    if mem.buf.len() < pos_usize {
        mem.buf.resize(pos_usize, 0);
    }
    mem.pos = pos_usize;
    OPJ_TRUE
}

/// Owns an `opj_codec_t` handle and destroys it on drop.
struct Codec(*mut opj_codec_t);
impl Drop for Codec {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` is a codec we created and have not destroyed.
            unsafe { opj_destroy_codec(self.0) };
        }
    }
}

/// Owns an `opj_image_t` handle and destroys it on drop.
struct Image(*mut opj_image_t);
impl Drop for Image {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` is an image we created and have not destroyed.
            unsafe { opj_image_destroy(self.0) };
        }
    }
}

/// Owns an `opj_stream_t` handle and destroys it on drop.
struct Stream(*mut opj_stream_t);
impl Drop for Stream {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` is a stream we created and have not destroyed.
            unsafe { opj_stream_destroy(self.0) };
        }
    }
}

/// The number of wavelet resolution levels for a `min_dim`-pixel-narrow image:
/// [`MAX_RESOLUTIONS`], clamped so the coarsest level is at least one pixel
/// (`2^(levels-1) <= min_dim`).
fn resolutions_for(min_dim: u32) -> i32 {
    let mut levels = 1_u32;
    while levels < MAX_RESOLUTIONS && (1_u32 << levels) <= min_dim {
        levels = levels.saturating_add(1);
    }
    i32::try_from(levels).unwrap_or(1)
}

/// Encode `width`×`height` RGBA8 `pixels` (validated by the caller) to a raw
/// `.j2c` codestream via the in-memory OpenJPEG pipeline.
fn encode(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, EncodeError> {
    // Opaque images drop their (redundant) alpha plane; transparency keeps it.
    let opaque = pixels
        .chunks_exact(RGBA_CHANNELS)
        .all(|texel| texel.get(3) == Some(&u8::MAX));
    let numcomps: usize = if opaque { 3 } else { 4 };

    // One component descriptor per output channel: 8-bit unsigned, no
    // sub-sampling, origin at (0, 0).
    let params = [opj_image_comptparm {
        dx: 1,
        dy: 1,
        w: width,
        h: height,
        x0: 0,
        y0: 0,
        prec: 8,
        bpp: 0,
        sgnd: 0,
    }; 4];
    let comptparms = params.as_ptr().cast_mut();
    let numcomps_u32 = u32::try_from(numcomps).unwrap_or(3);

    // SAFETY: `comptparms` points at `numcomps` (<= 4) initialised descriptors.
    let image = Image(unsafe {
        opj_image_create(numcomps_u32, comptparms, OPJ_COLOR_SPACE::OPJ_CLRSPC_SRGB)
    });
    if image.0.is_null() {
        return Err(EncodeError::Codec(
            "opj_image_create returned null".to_owned(),
        ));
    }

    // OpenJPEG's `opj_image_create` leaves the image extent unset; the caller
    // must fill it (origin 0, extent = width/height as there is no sub-sampling),
    // then copy each interleaved RGBA byte into its planar `i32` component.
    // SAFETY: `image.0` is a valid, non-null image whose components each hold
    // `width*height` i32s (allocated by `opj_image_create`); the pixel index `i`
    // stays below that count.
    unsafe {
        (*image.0).x0 = 0;
        (*image.0).y0 = 0;
        (*image.0).x1 = width;
        (*image.0).y1 = height;
        let comps = (*image.0).comps;
        for channel in 0..numcomps {
            let data = (*comps.add(channel)).data;
            for (i, texel) in pixels.chunks_exact(RGBA_CHANNELS).enumerate() {
                let sample = texel.get(channel).copied().unwrap_or(0);
                *data.add(i) = i32::from(sample);
            }
        }
    }

    // Lossy encoder parameters (rate-controlled 9-7 wavelet), resolution levels
    // clamped to the image size.
    let mut parameters = core::mem::MaybeUninit::<opj_cparameters_t>::uninit();
    // SAFETY: the call zeroes then default-initialises the whole struct through
    // the pointer, so it is fully initialised afterwards.
    unsafe { opj_set_default_encoder_parameters(parameters.as_mut_ptr()) };
    // SAFETY: fully initialised by the call above.
    let mut parameters = unsafe { parameters.assume_init() };
    parameters.tcp_numlayers = 1;
    if let Some(rate) = parameters.tcp_rates.first_mut() {
        *rate = COMPRESSION_RATIO;
    }
    parameters.cp_disto_alloc = 1;
    parameters.irreversible = 1;
    parameters.numresolution = resolutions_for(width.min(height));

    // SAFETY: creating a J2K (raw codestream) compressor.
    let codec = Codec(unsafe { opj_create_compress(OPJ_CODEC_FORMAT::OPJ_CODEC_J2K) });
    if codec.0.is_null() {
        return Err(EncodeError::Codec(
            "opj_create_compress returned null".to_owned(),
        ));
    }

    // SAFETY: codec, parameters and image are all valid and non-null.
    let setup = unsafe { opj_setup_encoder(codec.0, &raw mut parameters, image.0) };
    if setup != OPJ_TRUE {
        return Err(EncodeError::Codec("opj_setup_encoder failed".to_owned()));
    }

    // A boxed sink whose stable heap address backs the output stream's user data;
    // kept alive here for the whole encode.
    let mut sink = Box::new(MemStream::default());
    let sink_ptr: *mut c_void = (&raw mut *sink).cast::<c_void>();

    // SAFETY: creating a default output (write) stream.
    let stream = Stream(unsafe { opj_stream_default_create(0) });
    if stream.0.is_null() {
        return Err(EncodeError::Codec(
            "opj_stream_default_create returned null".to_owned(),
        ));
    }
    // SAFETY: `stream.0` is a valid output stream; the callbacks and user data (a
    // live boxed `MemStream`) outlive the encode. No free callback is registered
    // because Rust owns `sink`.
    unsafe {
        opj_stream_set_write_function(stream.0, Some(write_fn));
        opj_stream_set_skip_function(stream.0, Some(skip_fn));
        opj_stream_set_seek_function(stream.0, Some(seek_fn));
        opj_stream_set_user_data(stream.0, sink_ptr, None);
    }

    // SAFETY: codec, image and stream are all valid and non-null.
    unsafe {
        if opj_start_compress(codec.0, image.0, stream.0) != OPJ_TRUE {
            return Err(EncodeError::Codec("opj_start_compress failed".to_owned()));
        }
        if opj_encode(codec.0, stream.0) != OPJ_TRUE {
            return Err(EncodeError::Codec("opj_encode failed".to_owned()));
        }
        if opj_end_compress(codec.0, stream.0) != OPJ_TRUE {
            return Err(EncodeError::Codec("opj_end_compress failed".to_owned()));
        }
    }

    // `Codec`/`Image`/`Stream` drop here, destroying the OpenJPEG objects before
    // `sink` (still owned by Rust) is consumed.
    drop(stream);
    drop(codec);
    drop(image);
    if sink.buf.is_empty() {
        return Err(EncodeError::Codec("encoder produced no output".to_owned()));
    }
    Ok(sink.buf)
}

#[cfg(test)]
mod tests {
    use super::{EncodeError, encode_rgba8};
    use pretty_assertions::assert_eq;

    /// A boxed error so tests can use `?` instead of disallowed `unwrap`/`expect`.
    type TestError = Box<dyn core::error::Error>;

    /// Build an RGBA8 test image from a per-pixel alpha function over an
    /// `x`/`y`-derived colour.
    fn gradient(width: u32, height: u32, alpha: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                let r = u8::try_from(x % 256).unwrap_or(0);
                let g = u8::try_from(y % 256).unwrap_or(0);
                let b = u8::try_from((x ^ y) % 256).unwrap_or(0);
                pixels.extend_from_slice(&[r, g, b, alpha(x, y)]);
            }
        }
        pixels
    }

    #[test]
    fn encodes_a_valid_j2c_codestream() -> Result<(), TestError> {
        let pixels = gradient(64, 64, |_x, _y| u8::MAX);
        let bytes = encode_rgba8(64, 64, &pixels)?;
        // A raw JPEG-2000 codestream starts with the SOC marker 0xFF4F.
        assert_eq!(bytes.first(), Some(&0xFF));
        assert_eq!(bytes.get(1), Some(&0x4F));
        assert!(bytes.len() > 2);
        Ok(())
    }

    #[test]
    fn rejects_zero_sized_image() {
        assert!(matches!(encode_rgba8(0, 4, &[]), Err(EncodeError::Empty)));
    }

    #[test]
    fn rejects_mismatched_pixel_len() {
        assert!(matches!(
            encode_rgba8(4, 4, &[0, 0, 0]),
            Err(EncodeError::PixelLen { .. })
        ));
    }
}
