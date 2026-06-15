#![doc = include_str!("../README.md")]

mod endian;
mod error;
mod field;
mod header;
mod login;
mod message;
/// Generated LLUDP message types and their (de)serialization, produced at build
/// time from the vendored `message_template.msg`.
pub mod messages;
mod parcel_flags;
mod zerocode;

pub use error::WireError;
pub use field::{Reader, Writer};
pub use header::{PacketFlags, ParsedDatagram, encode_datagram, parse_datagram};
pub use login::{
    LoginFailure, LoginParseError, LoginRequest, LoginResponse, LoginSuccess, MfaChallenge,
    build_login_request, parse_login_response, password_hash,
};
pub use message::{Message, MessageId};
pub use messages::AnyMessage;
pub use parcel_flags::{ParcelFlags, RegionFlags, sim_access};
pub use zerocode::{decode as zero_decode, encode as zero_encode};

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use super::{
        PacketFlags, Reader, WireError, Writer, encode_datagram, parse_datagram, zero_decode,
        zero_encode,
    };

    #[test]
    fn field_round_trip() -> Result<(), WireError> {
        let mut w = Writer::new();
        w.put_u8(0x12);
        w.put_bool(true);
        w.put_u16(0xABCD);
        w.put_u32(0x0123_4567);
        w.put_u64(0x0011_2233_4455_6677);
        w.put_i16(-5);
        w.put_i32(-100_000);
        w.put_f32(1.5);
        w.put_f64(-2.25);
        w.put_variable1(b"hello")?;
        w.put_variable2(b"world")?;
        let bytes = w.into_bytes();

        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8()?, 0x12);
        assert!(r.bool()?);
        assert_eq!(r.u16()?, 0xABCD);
        assert_eq!(r.u32()?, 0x0123_4567);
        assert_eq!(r.u64()?, 0x0011_2233_4455_6677);
        assert_eq!(r.i16()?, -5);
        assert_eq!(r.i32()?, -100_000);
        assert_eq!(r.f32()?.to_bits(), 1.5_f32.to_bits());
        assert_eq!(r.f64()?.to_bits(), (-2.25_f64).to_bits());
        assert_eq!(r.variable1()?, b"hello");
        assert_eq!(r.variable2()?, b"world");
        assert!(r.is_empty());
        Ok(())
    }

    #[test]
    fn reader_underflow_is_an_error() {
        let mut r = Reader::new(&[0x01, 0x02]);
        assert!(matches!(r.u32(), Err(WireError::UnexpectedEof { .. })));
    }

    #[test]
    fn little_endian_byte_order_on_the_wire() {
        let mut w = Writer::new();
        w.put_u32(0x0102_0304);
        // Little-endian: least significant byte first.
        assert_eq!(w.as_bytes(), &[0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn zerocode_round_trips() -> Result<(), WireError> {
        let cases: &[&[u8]] = &[
            &[],
            &[1, 2, 3],
            &[0, 0, 0, 0],
            &[1, 0, 0, 2, 0, 3],
            &[0xFF, 0x00, 0xFF, 0x00],
        ];
        for original in cases {
            let encoded = zero_encode(original);
            let decoded = zero_decode(&encoded)?;
            assert_eq!(&decoded, original);
        }
        Ok(())
    }

    #[test]
    fn zerocode_long_run_round_trips() -> Result<(), WireError> {
        let original = vec![0u8; 600];
        let encoded = zero_encode(&original);
        assert!(encoded.len() < original.len());
        assert_eq!(zero_decode(&encoded)?, original);
        Ok(())
    }

    #[test]
    fn zerocode_decodes_a_known_run() -> Result<(), WireError> {
        // `0x00 0x03` decodes to three zero bytes around literal data.
        assert_eq!(
            zero_decode(&[0x01, 0x00, 0x03, 0x02])?,
            vec![0x01, 0, 0, 0, 0x02]
        );
        Ok(())
    }

    #[test]
    fn zerocode_truncated_marker_errors() {
        assert!(matches!(
            zero_decode(&[0x01, 0x00]),
            Err(WireError::TruncatedZerocode)
        ));
    }

    #[test]
    fn datagram_header_round_trip() -> Result<(), WireError> {
        let body = [0xDE, 0xAD, 0xBE, 0xEF];
        let datagram = encode_datagram(PacketFlags::RELIABLE, 0x0001_0203, &body);
        // Sequence number is big-endian in the header.
        assert_eq!(datagram.get(1..5), Some(&[0x00, 0x01, 0x02, 0x03][..]));

        let parsed = parse_datagram(&datagram)?;
        assert_eq!(parsed.flags, PacketFlags::RELIABLE);
        assert_eq!(parsed.sequence, 0x0001_0203);
        assert!(parsed.extra.is_empty());
        assert!(parsed.acks.is_empty());
        assert_eq!(parsed.body, &body);
        Ok(())
    }

    #[test]
    fn parse_datagram_strips_appended_acks() -> Result<(), WireError> {
        // Hand-build a datagram with the ACK flag and two big-endian acks.
        let mut datagram = vec![PacketFlags::ACK.bits()];
        datagram.extend_from_slice(&[0x00, 0x00, 0x00, 0x09]); // sequence
        datagram.push(0x00); // extra length
        datagram.extend_from_slice(&[0xAA, 0xBB]); // body
        datagram.extend_from_slice(&[0x00, 0x00, 0x00, 0x07]); // ack 7 (big-endian)
        datagram.extend_from_slice(&[0x00, 0x00, 0x00, 0x08]); // ack 8 (big-endian)
        datagram.push(0x02); // ack count

        let parsed = parse_datagram(&datagram)?;
        assert_eq!(parsed.sequence, 9);
        assert_eq!(parsed.acks, vec![7, 8]);
        assert_eq!(parsed.body, &[0xAA, 0xBB]);
        Ok(())
    }

    #[test]
    fn short_datagram_is_rejected() {
        assert!(matches!(
            parse_datagram(&[0x00, 0x01]),
            Err(WireError::ShortHeader)
        ));
    }

    #[test]
    fn parse_never_panics_on_arbitrary_bytes() {
        // Poke the parser with many short/odd inputs; it must always return
        // (Ok or Err), never panic, under the no-panic lints.
        for seed in 0usize..=2000 {
            let len = seed % 23;
            let bytes: Vec<u8> = (0..len)
                .map(|i| u8::try_from(seed.wrapping_add(i) % 256).unwrap_or(0))
                .collect();
            let _result = parse_datagram(&bytes);
            let _decoded = zero_decode(&bytes);
        }
    }
}
