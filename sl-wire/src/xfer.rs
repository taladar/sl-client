//! The packet framing of the legacy UDP **Xfer** file transfer
//! (`RequestXfer` → `SendXferPacket` stream ↔ `ConfirmXferPacket`).
//!
//! An Xfer moves one named file as a sequence of `SendXferPacket`s, one in
//! flight at a time. Two framing rules sit on top of the generated message
//! codec and are shared by every sender and receiver, client or simulator:
//!
//! - **The first packet is length-prefixed.** Packet 0's data begins with a
//!   little-endian `u32` holding the total file length; every later packet
//!   carries raw file bytes.
//! - **EOF is a flag on the packet number.** The top bit
//!   ([`XFER_EOF_FLAG`]) of the `Packet` field marks the last packet; the low
//!   31 bits are the sequence number. A `ConfirmXferPacket` echoes the raw
//!   field, flag included.
//!
//! Cross-checked against the reference viewer's `LLXfer::processEOF` /
//! `LLXfer_File::sendNextPacket` (`indra/llmessage/llxfer.cpp`, the
//! `LL_XFER_CHUNK_SIZE` small-packet variant) and OpenSim's
//! `XferModule.XferDownLoad` / `EstateTerrainXferHandler.XferReceive`.

use crate::field::{Reader, Writer};

/// The payload size of every Xfer packet but the last (`LL_XFER_CHUNK_SIZE`,
/// the small-packet variant both grids use when `UseBigPackets` is unset).
/// The first packet carries this many file bytes *plus* the four-byte length
/// prefix.
pub const XFER_CHUNK_SIZE: usize = 1000;

/// The bit of the `SendXferPacket` / `ConfirmXferPacket` `Packet` field that
/// marks the final packet of a transfer.
pub const XFER_EOF_FLAG: u32 = 0x8000_0000;

/// The decoded `Packet` field of a `SendXferPacket`: the 31-bit sequence
/// number and the EOF flag, kept apart so a handler never masks by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct XferPacketId {
    /// The 31-bit sequence number (0 for the length-prefixed first packet).
    sequence: u32,
    /// Whether this is the transfer's final packet.
    is_last: bool,
}

impl XferPacketId {
    /// Builds a packet id from a sequence number and the EOF marker. A
    /// sequence with the flag bit already set is masked down.
    #[must_use]
    pub const fn new(sequence: u32, is_last: bool) -> Self {
        Self {
            sequence: sequence & !XFER_EOF_FLAG,
            is_last,
        }
    }

    /// Decodes the raw wire `Packet` field.
    #[must_use]
    pub const fn from_raw(raw: u32) -> Self {
        Self {
            sequence: raw & !XFER_EOF_FLAG,
            is_last: raw & XFER_EOF_FLAG != 0,
        }
    }

    /// Encodes back to the raw wire `Packet` field — what a
    /// `ConfirmXferPacket` must echo.
    #[must_use]
    pub const fn raw(self) -> u32 {
        if self.is_last {
            self.sequence | XFER_EOF_FLAG
        } else {
            self.sequence
        }
    }

    /// The 31-bit sequence number.
    #[must_use]
    pub const fn sequence(self) -> u32 {
        self.sequence
    }

    /// Whether this is the transfer's final packet.
    #[must_use]
    pub const fn is_last(self) -> bool {
        self.is_last
    }

    /// Whether this is the length-prefixed first packet.
    #[must_use]
    pub const fn is_first(self) -> bool {
        self.sequence == 0
    }
}

/// A decoded `SendXferPacket` payload: the file bytes it carries and, on the
/// first packet, the total length the sender declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XferChunk<'a> {
    /// The sender's declared total file length — `Some` on the first packet
    /// only. Informational: the reference receivers accept whatever arrives.
    pub declared_len: Option<u32>,
    /// The file bytes of this packet (prefix stripped).
    pub payload: &'a [u8],
}

/// Frames one chunk of a file for the given packet: prepends the total-length
/// prefix on the first packet, passes later chunks through verbatim. The
/// prefix saturates at `u32::MAX` for an (impossible on the wire) larger file.
#[must_use]
pub fn encode_xfer_chunk(packet: XferPacketId, total_len: usize, chunk: &[u8]) -> Vec<u8> {
    let mut writer = Writer::new();
    if packet.is_first() {
        writer.put_u32(u32::try_from(total_len).unwrap_or(u32::MAX));
    }
    writer.bytes(chunk);
    writer.into_bytes()
}

/// Decodes one `SendXferPacket` payload: strips (and reports) the length
/// prefix on the first packet. A first packet shorter than the prefix yields
/// no declared length and an empty payload — the lenient reference behaviour.
#[must_use]
pub fn decode_xfer_chunk(packet: XferPacketId, data: &[u8]) -> XferChunk<'_> {
    if !packet.is_first() {
        return XferChunk {
            declared_len: None,
            payload: data,
        };
    }
    let mut reader = Reader::new(data);
    match reader.u32() {
        Ok(declared_len) => XferChunk {
            declared_len: Some(declared_len),
            payload: reader.take_rest(),
        },
        Err(_short) => XferChunk {
            declared_len: None,
            payload: &[],
        },
    }
}

/// One outgoing `SendXferPacket` produced by [`next_xfer_chunk`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XferOutgoingPacket {
    /// The packet id to put on the wire (sequence + EOF flag).
    pub id: XferPacketId,
    /// The framed payload (prefix included on the first packet).
    pub payload: Vec<u8>,
    /// The file cursor after this packet — how many file bytes have been sent
    /// once it goes out; feed it back as `sent` for the next call.
    pub sent: usize,
}

/// Produces the next framed packet of `data` given how many file bytes were
/// already sent and the sequence number to stamp on it: at most
/// [`XFER_CHUNK_SIZE`] file bytes (plus the prefix on sequence 0), flagged
/// last when it drains the file. An empty file is a single, last, prefix-only
/// packet. Returns `None` once `sent` has reached the end of a non-empty
/// file (nothing left to send).
#[must_use]
pub fn next_xfer_chunk(data: &[u8], sent: usize, sequence: u32) -> Option<XferOutgoingPacket> {
    if sent >= data.len() && !(data.is_empty() && sequence == 0) {
        return None;
    }
    let remaining = data.len().saturating_sub(sent);
    let take = remaining.min(XFER_CHUNK_SIZE);
    let end = sent.saturating_add(take);
    let chunk = data.get(sent..end).unwrap_or(&[]);
    let id = XferPacketId::new(sequence, end >= data.len());
    Some(XferOutgoingPacket {
        id,
        payload: encode_xfer_chunk(id, data.len(), chunk),
        sent: end,
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        XFER_CHUNK_SIZE, XFER_EOF_FLAG, XferChunk, XferPacketId, decode_xfer_chunk,
        encode_xfer_chunk, next_xfer_chunk,
    };

    /// A deterministic 2500-byte file (2 full chunks + a 500-byte tail).
    fn file() -> Vec<u8> {
        (0..2500u32)
            .map(|i| u8::try_from(i % 251).unwrap_or(0))
            .collect()
    }

    #[test]
    fn packet_id_round_trips_the_eof_flag() {
        let plain = XferPacketId::from_raw(7);
        assert_eq!(plain, XferPacketId::new(7, false), "plain id");
        assert_eq!(plain.raw(), 7, "plain raw");
        assert!(!plain.is_last(), "plain is not last");
        assert!(!plain.is_first(), "7 is not first");

        let last = XferPacketId::from_raw(2 | XFER_EOF_FLAG);
        assert_eq!(last, XferPacketId::new(2, true), "last id");
        assert_eq!(last.sequence(), 2, "last sequence");
        assert_eq!(last.raw(), 0x8000_0002, "last raw");
        assert!(last.is_last(), "last is last");

        assert!(XferPacketId::from_raw(0).is_first(), "zero is first");
        assert!(
            XferPacketId::from_raw(XFER_EOF_FLAG).is_first(),
            "flagged zero is first"
        );
        assert_eq!(
            XferPacketId::new(5 | XFER_EOF_FLAG, false).raw(),
            5,
            "new masks the flag out of the sequence"
        );
    }

    #[test]
    fn first_chunk_is_length_prefixed_and_later_chunks_are_raw()
    -> Result<(), Box<dyn std::error::Error>> {
        let data = file();
        let first_packet = next_xfer_chunk(&data, 0, 0).ok_or("first chunk")?;
        let (first_id, first) = (first_packet.id, first_packet.payload);
        assert_eq!(first_id, XferPacketId::new(0, false), "first id");
        assert_eq!(first_packet.sent, 1000, "first cursor");
        assert_eq!(first.len(), XFER_CHUNK_SIZE + 4, "first length");
        assert_eq!(
            first.get(0..4),
            Some([0xc4, 0x09, 0, 0].as_slice()),
            "2500 little-endian prefix"
        );
        assert_eq!(first.get(4..), data.get(0..1000), "first payload");

        let second_packet = next_xfer_chunk(&data, 1000, 1).ok_or("second chunk")?;
        let (second_id, second) = (second_packet.id, second_packet.payload);
        assert_eq!(second_packet.sent, 2000, "second cursor");
        assert_eq!(second_id, XferPacketId::new(1, false), "second id");
        assert_eq!(
            second.as_slice(),
            data.get(1000..2000).unwrap_or(&[]),
            "second payload"
        );

        let third_packet = next_xfer_chunk(&data, 2000, 2).ok_or("third chunk")?;
        let (third_id, third) = (third_packet.id, third_packet.payload);
        assert_eq!(third_packet.sent, 2500, "third cursor");
        assert_eq!(third_id, XferPacketId::new(2, true), "third id is last");
        assert_eq!(third_id.raw(), 2 | XFER_EOF_FLAG, "third raw");
        assert_eq!(
            third.as_slice(),
            data.get(2000..).unwrap_or(&[]),
            "third payload"
        );

        assert_eq!(
            next_xfer_chunk(&data, 2500, 3),
            None,
            "nothing after the end"
        );
        Ok(())
    }

    #[test]
    fn exact_multiple_file_flags_its_final_full_chunk() -> Result<(), Box<dyn std::error::Error>> {
        let data = vec![0xabu8; 2000];
        let packet = next_xfer_chunk(&data, 1000, 1).ok_or("second chunk")?;
        assert_eq!(
            packet.id,
            XferPacketId::new(1, true),
            "second of two is last"
        );
        Ok(())
    }

    #[test]
    fn empty_file_is_one_prefix_only_last_packet() -> Result<(), Box<dyn std::error::Error>> {
        let packet = next_xfer_chunk(&[], 0, 0).ok_or("empty file chunk")?;
        assert_eq!(packet.id, XferPacketId::new(0, true), "single last packet");
        assert_eq!(packet.payload, vec![0, 0, 0, 0], "prefix only");
        assert_eq!(packet.sent, 0, "empty cursor");
        assert_eq!(next_xfer_chunk(&[], 0, 1), None, "no second packet");
        Ok(())
    }

    #[test]
    fn decode_strips_the_prefix_on_the_first_packet_only() {
        let first = encode_xfer_chunk(XferPacketId::new(0, false), 2500, &[1, 2, 3]);
        assert_eq!(
            decode_xfer_chunk(XferPacketId::new(0, false), &first),
            XferChunk {
                declared_len: Some(2500),
                payload: &[1, 2, 3],
            },
            "first packet"
        );
        let later = encode_xfer_chunk(XferPacketId::new(1, true), 2500, &[4, 5]);
        assert_eq!(later, vec![4, 5], "later packets are raw");
        assert_eq!(
            decode_xfer_chunk(XferPacketId::new(1, true), &later),
            XferChunk {
                declared_len: None,
                payload: &[4, 5],
            },
            "later packet"
        );
        assert_eq!(
            decode_xfer_chunk(XferPacketId::new(0, true), &[1, 2]),
            XferChunk {
                declared_len: None,
                payload: &[],
            },
            "short first packet is empty"
        );
    }
}
