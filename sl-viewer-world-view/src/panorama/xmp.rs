//! The metadata half of the 360° panorama ([`super`]): the **XMP / GPano**
//! packet that tells a viewer or a platform this 2:1 image is a sphere and not
//! a very wide photograph, and the two ways of getting that packet into a file.
//!
//! Without it a panorama is an ordinary picture: Google Photos, Facebook,
//! Flickr and every desktop photo-sphere viewer decide what they are looking at
//! by reading `GPano:ProjectionType` out of the file's XMP, not by measuring its
//! aspect ratio. So the packet is not decoration — it is the difference between
//! shipping a panorama and shipping a stretched screenshot.
//!
//! # What is written, and where our fields differ from the reference's
//!
//! The standard Photo Sphere block ([`packet`]) carries the projection, the
//! full-pano and cropped-area sizes (all four, unlike the reference viewer,
//! which omits them — several readers treat a missing `CroppedArea*` as "not a
//! panorama"), the capture and stitching software, the capture dates and the
//! two headings:
//!
//! - **`PoseHeadingDegrees`** is the compass heading of the **centre** of the
//!   image, which for our layout is the direction the camera was facing (see
//!   [`super::equirect`]).
//! - **`InitialViewHeadingDegrees`** is therefore `0`: the view a reader should
//!   open on is the centre of the image, which is the shot the photographer
//!   composed. The reference viewer writes the compass heading into *this*
//!   field instead, because its panorama is built around world axes and the
//!   camera's heading is an offset within it.
//!
//! The Second Life specifics the reference also writes — the region name, the
//! location URL, the source cube-map size — are written here too, but under a
//! **declared namespace**. The reference emits them as bare, namespace-less
//! elements inside `rdf:Description`, which is not well-formed RDF; a strict
//! XMP parser rejects the whole packet, taking the GPano block with it. The
//! local names are the same, so a tool looking for `SLRegionName` still finds
//! it.
//!
//! # Getting the packet into a file
//!
//! Neither of the two encoders the viewer uses for a panorama will write XMP,
//! so the packet is spliced into the encoded bytes:
//!
//! - **JPEG** ([`embed_in_jpeg`]): an `APP1` segment carrying the Adobe XAP
//!   identifier, inserted after the `APP0`/JFIF segment the encoder writes (or
//!   directly after the `SOI` when there is none).
//! - **PNG** ([`embed_in_png`]): an `iTXt` chunk under the
//!   `XML:com.adobe.xmp` keyword, inserted after `IHDR`.
//!
//! Both work on an already-encoded buffer and leave every other byte of it
//! alone, so the image data is exactly what the encoder produced.
//!
//! Reference (Firestorm, read-only):
//! `skins/default/html/common/equirectangular/js/jpeg_encoder_basic.js` (the
//! `writeAPP0` / XMP marker block).

/// Big-endian byte conversions, which both container formats are defined in.
///
/// The workspace denies `big_endian_bytes` (LLUDP field payloads are
/// little-endian, and a stray `to_be_bytes` there is a wire bug), so the two
/// conversions a JPEG marker length and a PNG chunk header need are confined
/// here behind one localized expectation — the same shape `sl_wire::endian`
/// uses.
mod bytes {
    #![expect(
        clippy::big_endian_bytes,
        reason = "JPEG marker-segment lengths and PNG chunk lengths and CRCs are \
                  format-defined big-endian"
    )]

    /// A JPEG marker segment's two length bytes.
    pub(super) const fn u16_to_be(value: u16) -> [u8; 2] {
        value.to_be_bytes()
    }

    /// Read a JPEG marker segment's two length bytes.
    pub(super) const fn u16_from_be(bytes: [u8; 2]) -> u16 {
        u16::from_be_bytes(bytes)
    }

    /// A PNG chunk's four length or CRC bytes.
    pub(super) const fn u32_to_be(value: u32) -> [u8; 4] {
        value.to_be_bytes()
    }
}

/// The XMP packet identifier an `APP1` segment must start with, including its
/// terminating NUL.
const XAP_IDENTIFIER: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// The PNG chunk keyword XMP lives under.
const PNG_XMP_KEYWORD: &[u8] = b"XML:com.adobe.xmp";

/// The eight bytes every PNG starts with.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The largest payload a JPEG marker segment can carry, including its own
/// two length bytes.
const MAX_JPEG_SEGMENT: usize = 0xFFFF;

/// The panorama version string the reference viewer stamps its captures with,
/// carried so a file from either viewer reads the same way.
const PANO_VERSION: &str = "2.2.1";

/// What a panorama knows about itself, as the XMP packet needs it.
#[derive(Debug, Clone)]
pub struct PanoramaMetadata {
    /// The finished panorama's width in pixels.
    pub width: u32,
    /// Its height in pixels (always half the width).
    pub height: u32,
    /// The compass heading, in degrees clockwise from north, of the centre
    /// column of the image — where the camera was looking.
    pub heading_degrees: f32,
    /// The edge length of each cube face the panorama was stitched from.
    pub face_size: u32,
    /// The viewer that shot and stitched it, name and version.
    pub software: String,
    /// The region the capture was taken in, when there was one.
    pub region_name: Option<String>,
    /// A link back to the spot it was taken from, when one could be built.
    pub region_url: Option<String>,
    /// When the shutter fired, as an ISO-8601 timestamp.
    pub captured_at: String,
}

/// The XMP packet for `metadata`, as the XML that goes into a file.
#[must_use]
pub fn packet(metadata: &PanoramaMetadata) -> String {
    let software = escape(&metadata.software);
    let region_name = escape(metadata.region_name.as_deref().unwrap_or_default());
    let region_url = escape(metadata.region_url.as_deref().unwrap_or_default());
    let captured_at = escape(&metadata.captured_at);
    let heading = normalise_heading(metadata.heading_degrees);
    let (width, height) = (metadata.width, metadata.height);
    let face_size = metadata.face_size;
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"{software}\">\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         <rdf:Description rdf:about=\"\"\n\
         xmlns:GPano=\"http://ns.google.com/photos/1.0/panorama/\"\n\
         xmlns:sl=\"http://secondlife.com/xmp/panorama/1.0/\">\n\
         <GPano:ProjectionType>equirectangular</GPano:ProjectionType>\n\
         <GPano:UsePanoramaViewer>True</GPano:UsePanoramaViewer>\n\
         <GPano:FullPanoWidthPixels>{width}</GPano:FullPanoWidthPixels>\n\
         <GPano:FullPanoHeightPixels>{height}</GPano:FullPanoHeightPixels>\n\
         <GPano:CroppedAreaImageWidthPixels>{width}</GPano:CroppedAreaImageWidthPixels>\n\
         <GPano:CroppedAreaImageHeightPixels>{height}</GPano:CroppedAreaImageHeightPixels>\n\
         <GPano:CroppedAreaLeftPixels>0</GPano:CroppedAreaLeftPixels>\n\
         <GPano:CroppedAreaTopPixels>0</GPano:CroppedAreaTopPixels>\n\
         <GPano:PoseHeadingDegrees>{heading:.1}</GPano:PoseHeadingDegrees>\n\
         <GPano:InitialViewHeadingDegrees>0</GPano:InitialViewHeadingDegrees>\n\
         <GPano:CaptureSoftware>{software}</GPano:CaptureSoftware>\n\
         <GPano:StitchingSoftware>{software}</GPano:StitchingSoftware>\n\
         <GPano:FirstPhotoDate>{captured_at}</GPano:FirstPhotoDate>\n\
         <GPano:LastPhotoDate>{captured_at}</GPano:LastPhotoDate>\n\
         <sl:SLPanoVersion>{PANO_VERSION}</sl:SLPanoVersion>\n\
         <sl:SourceCubeMapSizePixels>{face_size}</sl:SourceCubeMapSizePixels>\n\
         <sl:SLRegionName>{region_name}</sl:SLRegionName>\n\
         <sl:SLRegionURL>{region_url}</sl:SLRegionURL>\n\
         </rdf:Description>\n\
         </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"r\"?>"
    )
}

/// A heading folded into `0..360`, with anything non-finite reading as north.
fn normalise_heading(degrees: f32) -> f32 {
    if !degrees.is_finite() {
        return 0.0;
    }
    let wrapped = degrees % 360.0;
    if wrapped < 0.0 {
        wrapped + 360.0
    } else {
        wrapped
    }
}

/// The five XML entities, so a region name with an apostrophe or an ampersand
/// in it cannot break the packet (Firestorm deletes such characters instead).
fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            other => escaped.push(other),
        }
    }
    escaped
}

/// What can go wrong splicing a packet into an encoded image.
#[derive(Debug, thiserror::Error)]
pub enum XmpError {
    /// The buffer is not the format it was handed to.
    #[error("the encoded image is not a {expected} file")]
    NotThatFormat {
        /// The format the splice was expecting.
        expected: &'static str,
    },
    /// The packet is too large for the container's metadata slot.
    #[error("an XMP packet of {size} bytes does not fit a {container} ({limit} byte limit)")]
    TooLarge {
        /// The packet's size in bytes.
        size: usize,
        /// What it was being put into.
        container: &'static str,
        /// The largest packet that container takes.
        limit: usize,
    },
}

/// Splice `packet` into an encoded JPEG as an `APP1` segment.
///
/// # Errors
///
/// [`XmpError`] when the buffer does not start with a JPEG `SOI`, or when the
/// packet is larger than a single marker segment can carry.
pub fn embed_in_jpeg(jpeg: &[u8], packet: &str) -> Result<Vec<u8>, XmpError> {
    if jpeg.first() != Some(&0xFF) || jpeg.get(1) != Some(&0xD8) {
        return Err(XmpError::NotThatFormat { expected: "JPEG" });
    }
    // A marker segment's length field counts itself, the identifier and the
    // payload — and it is a `u16`.
    let payload = XAP_IDENTIFIER
        .len()
        .saturating_add(packet.len())
        .saturating_add(2);
    let limit = MAX_JPEG_SEGMENT
        .saturating_sub(XAP_IDENTIFIER.len())
        .saturating_sub(2);
    if payload > MAX_JPEG_SEGMENT {
        return Err(XmpError::TooLarge {
            size: packet.len(),
            container: "JPEG APP1 segment",
            limit,
        });
    }
    let insert_at = jpeg_insert_offset(jpeg);
    let (head, tail) = jpeg.split_at(insert_at.min(jpeg.len()));
    let mut out = Vec::with_capacity(jpeg.len().saturating_add(payload).saturating_add(2));
    out.extend_from_slice(head);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&bytes::u16_to_be(
        u16::try_from(payload).unwrap_or(u16::MAX),
    ));
    out.extend_from_slice(XAP_IDENTIFIER);
    out.extend_from_slice(packet.as_bytes());
    out.extend_from_slice(tail);
    Ok(out)
}

/// Where the `APP1` goes: after the `APP0`/JFIF segment when the encoder wrote
/// one (the JFIF specification wants that segment first), otherwise directly
/// after the `SOI`.
fn jpeg_insert_offset(jpeg: &[u8]) -> usize {
    let after_soi = 2_usize;
    let is_app0 =
        jpeg.get(after_soi) == Some(&0xFF) && jpeg.get(after_soi.saturating_add(1)) == Some(&0xE0);
    if !is_app0 {
        return after_soi;
    }
    let high = jpeg.get(after_soi.saturating_add(2)).copied().unwrap_or(0);
    let low = jpeg.get(after_soi.saturating_add(3)).copied().unwrap_or(0);
    let length = usize::from(bytes::u16_from_be([high, low]));
    after_soi.saturating_add(2).saturating_add(length)
}

/// Splice `packet` into an encoded PNG as an `iTXt` chunk after `IHDR`.
///
/// # Errors
///
/// [`XmpError::NotThatFormat`] when the buffer is not a PNG, or its first chunk
/// is not the `IHDR` every PNG must open with.
pub fn embed_in_png(png: &[u8], packet: &str) -> Result<Vec<u8>, XmpError> {
    if png.get(..PNG_SIGNATURE.len()) != Some(&PNG_SIGNATURE[..]) {
        return Err(XmpError::NotThatFormat { expected: "PNG" });
    }
    // The header chunk: 4 length bytes, 4 type bytes, 13 data bytes, 4 CRC.
    let header_end = PNG_SIGNATURE.len().saturating_add(25);
    let header_type =
        png.get(PNG_SIGNATURE.len().saturating_add(4)..PNG_SIGNATURE.len().saturating_add(8));
    if header_type != Some(b"IHDR") || png.len() < header_end {
        return Err(XmpError::NotThatFormat { expected: "PNG" });
    }
    // iTXt data: keyword, NUL, compression flag, compression method, NUL
    // (empty language tag), NUL (empty translated keyword), then the text.
    let mut data = Vec::with_capacity(
        PNG_XMP_KEYWORD
            .len()
            .saturating_add(packet.len())
            .saturating_add(5),
    );
    data.extend_from_slice(PNG_XMP_KEYWORD);
    data.extend_from_slice(&[0, 0, 0, 0, 0]);
    data.extend_from_slice(packet.as_bytes());

    let length = u32::try_from(data.len()).map_err(|_ignored| XmpError::TooLarge {
        size: data.len(),
        container: "PNG iTXt chunk",
        limit: usize::try_from(u32::MAX).unwrap_or(usize::MAX),
    })?;
    let mut chunk = Vec::with_capacity(data.len().saturating_add(12));
    chunk.extend_from_slice(&bytes::u32_to_be(length));
    chunk.extend_from_slice(b"iTXt");
    chunk.extend_from_slice(&data);
    // The CRC covers the chunk type and its data, not the length.
    let mut crc_input = Vec::with_capacity(data.len().saturating_add(4));
    crc_input.extend_from_slice(b"iTXt");
    crc_input.extend_from_slice(&data);
    chunk.extend_from_slice(&bytes::u32_to_be(crc32(&crc_input)));

    let (head, tail) = png.split_at(header_end.min(png.len()));
    let mut out = Vec::with_capacity(png.len().saturating_add(chunk.len()));
    out.extend_from_slice(head);
    out.extend_from_slice(&chunk);
    out.extend_from_slice(tail);
    Ok(out)
}

/// The CRC-32 (IEEE 802.3, reflected, `0xEDB88320`) a PNG chunk carries.
///
/// Written out rather than pulled in as a dependency: it is the one thing a
/// hand-built chunk needs, and a wrong one makes every decoder reject the file
/// — which the tests here check by decoding the spliced result.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _step in 0..8 {
            let carry = crc & 1;
            crc >>= 1;
            if carry != 0 {
                crc ^= 0xEDB8_8320;
            }
        }
    }
    crc ^ u32::MAX
}

#[cfg(test)]
mod tests {
    use super::{
        PNG_SIGNATURE, PanoramaMetadata, XAP_IDENTIFIER, crc32, embed_in_jpeg, embed_in_png,
        normalise_heading, packet,
    };
    use pretty_assertions::assert_eq;

    /// A boxed error so tests use `?` rather than the disallowed `expect`.
    type TestError = Box<dyn core::error::Error>;

    /// How far a folded heading may sit from the expected one, in degrees.
    const HEADING_EPSILON: f32 = 1.0e-3;

    /// The signature, the whole `IHDR` chunk and the new chunk's own length
    /// field: where an `iTXt` type tag lands when it is the second chunk.
    const AFTER_IHDR: usize = 8 + 25 + 4;

    /// A metadata block to write packets from.
    fn metadata() -> PanoramaMetadata {
        PanoramaMetadata {
            width: 4096,
            height: 2048,
            heading_degrees: 137.5,
            face_size: 2048,
            software: "sl-client-bevy-viewer 0.1.0".to_owned(),
            region_name: Some("Test Region".to_owned()),
            region_url: Some(
                "https://maps.secondlife.com/secondlife/Test%20Region/128/128/25".to_owned(),
            ),
            captured_at: "2026-09-20T12:34:56+02:00".to_owned(),
        }
    }

    /// The fields a photo-sphere reader looks for are all present — without
    /// them the file is a wide picture, not a panorama.
    #[test]
    fn the_packet_carries_the_photo_sphere_fields() {
        let written = packet(&metadata());
        for needle in [
            "<GPano:ProjectionType>equirectangular</GPano:ProjectionType>",
            "<GPano:UsePanoramaViewer>True</GPano:UsePanoramaViewer>",
            "<GPano:FullPanoWidthPixels>4096</GPano:FullPanoWidthPixels>",
            "<GPano:FullPanoHeightPixels>2048</GPano:FullPanoHeightPixels>",
            "<GPano:CroppedAreaImageWidthPixels>4096</GPano:CroppedAreaImageWidthPixels>",
            "<GPano:CroppedAreaLeftPixels>0</GPano:CroppedAreaLeftPixels>",
            "<GPano:PoseHeadingDegrees>137.5</GPano:PoseHeadingDegrees>",
            "<GPano:InitialViewHeadingDegrees>0</GPano:InitialViewHeadingDegrees>",
            "<sl:SourceCubeMapSizePixels>2048</sl:SourceCubeMapSizePixels>",
        ] {
            assert!(written.contains(needle), "the packet is missing {needle}");
        }
    }

    /// A region name is XML text, and region names contain apostrophes and
    /// ampersands. Escaping them keeps the packet well-formed; the reference
    /// viewer deletes the characters instead.
    #[test]
    fn a_region_name_is_escaped_not_deleted() {
        let mut meta = metadata();
        meta.region_name = Some("Bob & Alice's <Place>".to_owned());
        let written = packet(&meta);
        assert!(written.contains("Bob &amp; Alice&apos;s &lt;Place&gt;"));
        assert!(!written.contains("Alice's"));
    }

    /// Headings are folded into a compass circle whatever arrives.
    #[test]
    fn headings_are_folded_into_the_compass() {
        for (given, wanted) in [(0.0, 0.0), (-90.0, 270.0), (450.0, 90.0), (f32::NAN, 0.0)] {
            let folded = normalise_heading(given);
            assert!(
                (folded - wanted).abs() < HEADING_EPSILON,
                "{given} folded to {folded}, expected {wanted}"
            );
        }
    }

    /// A one-pixel JPEG to splice into.
    fn tiny_jpeg() -> Result<Vec<u8>, TestError> {
        let mut encoded = Vec::new();
        let image = image::RgbImage::from_pixel(4, 2, image::Rgb([10, 20, 30]));
        image::DynamicImage::ImageRgb8(image).write_to(
            &mut std::io::Cursor::new(&mut encoded),
            image::ImageFormat::Jpeg,
        )?;
        Ok(encoded)
    }

    /// A one-pixel PNG to splice into.
    fn tiny_png() -> Result<Vec<u8>, TestError> {
        let mut encoded = Vec::new();
        let image = image::RgbImage::from_pixel(4, 2, image::Rgb([10, 20, 30]));
        image::DynamicImage::ImageRgb8(image).write_to(
            &mut std::io::Cursor::new(&mut encoded),
            image::ImageFormat::Png,
        )?;
        Ok(encoded)
    }

    /// The JPEG splice adds an `APP1` segment and leaves the rest of the file
    /// byte for byte where it was.
    #[test]
    fn the_jpeg_splice_inserts_a_marker_and_changes_nothing_else() -> Result<(), TestError> {
        let original = tiny_jpeg()?;
        let written = packet(&metadata());
        let spliced = embed_in_jpeg(&original, &written)?;
        assert_eq!(spliced.get(..2), original.get(..2), "the SOI moved");
        let marker = spliced
            .windows(2)
            .position(|pair| pair == [0xFF, 0xE1])
            .ok_or("the APP1 marker should be in the file")?;
        let identifier_at = marker.saturating_add(4);
        assert_eq!(
            spliced.get(identifier_at..identifier_at.saturating_add(XAP_IDENTIFIER.len())),
            Some(XAP_IDENTIFIER),
        );
        let length = super::bytes::u16_from_be([
            spliced
                .get(marker.saturating_add(2))
                .copied()
                .unwrap_or_default(),
            spliced
                .get(marker.saturating_add(3))
                .copied()
                .unwrap_or_default(),
        ]);
        assert_eq!(
            usize::from(length),
            XAP_IDENTIFIER
                .len()
                .saturating_add(written.len())
                .saturating_add(2),
            "the segment length does not cover its payload"
        );
        // Everything after the inserted segment is the original tail.
        let inserted = spliced.len().saturating_sub(original.len());
        let tail_at = marker.saturating_add(inserted);
        assert_eq!(spliced.get(tail_at..), original.get(marker..));
        Ok(())
    }

    /// And the spliced JPEG still decodes — the check that the segment is
    /// where a decoder will tolerate it.
    #[test]
    fn a_spliced_jpeg_still_decodes() -> Result<(), TestError> {
        let spliced = embed_in_jpeg(&tiny_jpeg()?, &packet(&metadata()))?;
        let decoded = image::load_from_memory_with_format(&spliced, image::ImageFormat::Jpeg)?;
        assert_eq!(decoded.width(), 4);
        assert_eq!(decoded.height(), 2);
        Ok(())
    }

    /// The PNG splice keeps the signature, lands its chunk after `IHDR`, and
    /// leaves a file every decoder still reads — which only holds if the CRC
    /// is right.
    #[test]
    fn a_spliced_png_still_decodes() -> Result<(), TestError> {
        let original = tiny_png()?;
        let written = packet(&metadata());
        let spliced = embed_in_png(&original, &written)?;
        assert_eq!(spliced.get(..8), Some(&PNG_SIGNATURE[..]));
        let chunk_at = spliced
            .windows(4)
            .position(|quad| quad == b"iTXt")
            .ok_or("the iTXt chunk should be in the file")?;
        assert_eq!(chunk_at, AFTER_IHDR, "the chunk is not right after IHDR");
        let decoded = image::load_from_memory_with_format(&spliced, image::ImageFormat::Png)?;
        assert_eq!(decoded.width(), 4);
        assert_eq!(decoded.height(), 2);
        Ok(())
    }

    /// The hand-written CRC is the one PNG means: the standard check value of
    /// the IEEE polynomial over `"123456789"`.
    #[test]
    fn the_crc_is_the_standard_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    /// A buffer that is not the format asked for is refused rather than
    /// corrupted.
    #[test]
    fn a_foreign_buffer_is_refused() -> Result<(), TestError> {
        assert!(matches!(embed_in_jpeg(&tiny_png()?, "x"), Err(_png)));
        assert!(matches!(embed_in_png(&tiny_jpeg()?, "x"), Err(_jpeg)));
        Ok(())
    }

    /// A packet larger than a marker segment is refused — silently truncating
    /// it would produce a file whose metadata is half an XML document.
    #[test]
    fn an_oversized_jpeg_packet_is_refused() -> Result<(), TestError> {
        let huge = "x".repeat(70_000);
        assert!(matches!(embed_in_jpeg(&tiny_jpeg()?, &huge), Err(_big)));
        Ok(())
    }
}
