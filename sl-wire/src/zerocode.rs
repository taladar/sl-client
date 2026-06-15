//! Zero-run-length coding of LLUDP message bodies.
//!
//! When the `ZEROCODED` packet flag is set, runs of zero bytes in the message
//! body are compressed: a zero byte is replaced by the marker `0x00` followed
//! by a single count byte giving the run length. This is applied to the message
//! body only — never to the packet header or to appended acknowledgements.
//!
//! The decoder follows the semantics implemented by the official viewer: a
//! `0x00` marker is always followed by a one-byte count, and that many zero
//! bytes are emitted. The encoder is its exact inverse: runs longer than 255
//! are split into successive `0x00 0xFF` chunks so a round trip is lossless.

use crate::error::WireError;

/// The marker byte introducing a run of zeros in zero-coded data.
const ZERO_MARKER: u8 = 0x00;

/// The maximum run length representable by a single count byte.
const MAX_RUN: usize = 255;

/// Expands zero-coded `body` bytes into their original form.
///
/// # Errors
///
/// Returns [`WireError::TruncatedZerocode`] if a `0x00` marker appears with no
/// following count byte.
pub fn decode(body: &[u8]) -> Result<Vec<u8>, WireError> {
    let mut out = Vec::with_capacity(body.len());
    let mut iter = body.iter().copied();
    while let Some(byte) = iter.next() {
        if byte == ZERO_MARKER {
            let count = iter.next().ok_or(WireError::TruncatedZerocode)?;
            out.extend(core::iter::repeat_n(0u8, usize::from(count)));
        } else {
            out.push(byte);
        }
    }
    Ok(out)
}

/// Compresses `body` bytes by zero-run-length coding.
///
/// This is the exact inverse of [`decode`]: runs of zero bytes longer than 255
/// are split into successive maximal chunks.
#[must_use]
pub fn encode(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    let mut index = 0;
    while let Some(&byte) = body.get(index) {
        if byte == ZERO_MARKER {
            // Measure the run of consecutive zeros starting at `index`.
            let mut run = 0usize;
            while body.get(index.saturating_add(run)) == Some(&ZERO_MARKER) {
                run = run.saturating_add(1);
            }
            index = index.saturating_add(run);
            // Emit the run as one or more `0x00 count` chunks.
            while run > 0 {
                let chunk = run.min(MAX_RUN);
                out.push(ZERO_MARKER);
                out.push(chunk_to_byte(chunk));
                run = run.saturating_sub(chunk);
            }
        } else {
            out.push(byte);
            index = index.saturating_add(1);
        }
    }
    out
}

/// Narrows a run-length chunk (guaranteed `1..=255`) to its count byte.
fn chunk_to_byte(chunk: usize) -> u8 {
    u8::try_from(chunk).unwrap_or(u8::MAX)
}
