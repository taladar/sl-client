//! Reading `.pcap` / `.pcapng` captures and peeling link/IP/UDP off each frame.
//!
//! Yields the LLUDP UDP datagrams with the IP and UDP header metadata retained,
//! so nothing from the wire is lost before the LLUDP body is decoded.
//!
//! The read is deliberately forgiving, because the captures this tool is handed
//! are the ones a debugging session produced: a `tcpdump` killed with Ctrl-C
//! leaves a half-written final record, a rotated or concatenated `.pcapng` has
//! more than one section, and a capture taken with a short snaplen truncates
//! every large datagram. None of those should cost the datagrams that *did*
//! read, so the reader stops at the damage, keeps what it has, and reports what
//! it skipped.

use std::io::{BufReader, Read, Seek as _};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use etherparse::{
    LaxNetSlice, LaxSlicedPacket, LinuxSllProtocolType, LinuxSllSlice, TransportSlice,
};
use pcap_file::pcap::PcapReader;
use pcap_file::pcapng::{Block, PcapNgReader};
use pcap_file::{DataLink, TsResolution};
use time::OffsetDateTime;

use crate::trace::TraceError;

/// The fixed size of a UDP header, in bytes.
const UDP_HEADER_LEN: usize = 8;

/// The first four bytes of a `.pcapng` Section Header Block, which is how a
/// `.pcapng` is told apart from a classic `.pcap`.
const PCAPNG_MAGIC: [u8; 4] = [0x0a, 0x0d, 0x0d, 0x0a];

/// One UDP datagram recovered from the capture, with its header metadata.
#[derive(Debug, Clone)]
pub struct UdpDatagram {
    /// The capture timestamp.
    pub timestamp: OffsetDateTime,
    /// The source `ip:port`.
    pub source: SocketAddr,
    /// The destination `ip:port`.
    pub destination: SocketAddr,
    /// The IP version (4 or 6).
    pub ip_version: u8,
    /// The IPv4 TTL / IPv6 hop limit.
    pub ip_hop_limit: u8,
    /// The IPv4 total length, or the IPv6 payload length.
    pub ip_total_len: u16,
    /// The UDP header length field.
    pub udp_length: u16,
    /// The UDP header checksum field.
    pub udp_checksum: u16,
    /// Whether fewer payload bytes were captured than the UDP length field
    /// declares — a snaplen-truncated capture, not a protocol fault.
    pub truncated: bool,
    /// The UDP payload (the LLUDP datagram), as far as it was captured.
    pub payload: Vec<u8>,
}

/// Everything one capture file yielded, plus what the read had to skip.
#[derive(Debug, Clone, Default)]
pub struct Capture {
    /// The UDP datagrams recovered, in file order.
    pub datagrams: Vec<UdpDatagram>,
    /// The error that ended the read before the end of the file, if any — a
    /// truncated capture keeps every datagram read before the damage.
    pub stopped_early: Option<String>,
    /// How many of the recovered datagrams were snaplen-truncated.
    pub snaplen_truncated: usize,
    /// How many frames were skipped whole, because their interface's link type
    /// cannot be peeled or their timestamp is not representable.
    pub skipped_frames: usize,
}

impl Capture {
    /// Records one recovered datagram, tallying it if it was truncated.
    fn push(&mut self, datagram: UdpDatagram) {
        if datagram.truncated {
            self.snaplen_truncated = self.snaplen_truncated.saturating_add(1);
        }
        self.datagrams.push(datagram);
    }

    /// Records that one frame was skipped whole.
    const fn skip(&mut self) {
        self.skipped_frames = self.skipped_frames.saturating_add(1);
    }
}

/// The subset of link-layer encapsulations the tool can peel.
#[derive(Debug, Clone, Copy)]
enum LinkKind {
    /// Ethernet II framing.
    Ethernet,
    /// Linux "cooked" SLL framing (a capture on the `any` interface).
    LinuxSll,
    /// A bare IP packet (raw / DLT_IPV4 / DLT_IPV6).
    RawIp,
    /// A BSD loopback frame: a 4-byte address family precedes the IP packet.
    BsdLoopback,
}

/// Maps a libpcap link type to the encapsulation we know how to peel, or
/// `None` if it is unsupported.
const fn link_kind(datalink: DataLink) -> Option<LinkKind> {
    match datalink {
        DataLink::ETHERNET => Some(LinkKind::Ethernet),
        DataLink::LINUX_SLL => Some(LinkKind::LinuxSll),
        DataLink::RAW | DataLink::IPV4 | DataLink::IPV6 => Some(LinkKind::RawIp),
        DataLink::NULL | DataLink::LOOP => Some(LinkKind::BsdLoopback),
        _ => None,
    }
}

/// Reads every UDP datagram from a `.pcap` or `.pcapng` capture at `path`.
///
/// The file is streamed, not slurped, so a multi-gigabyte capture costs a
/// buffer rather than its own size in memory.
///
/// # Errors
///
/// Returns [`TraceError`] if the file cannot be opened, its container header
/// cannot be parsed, or (for classic pcap) the link-layer type is unsupported.
/// A failure *part way through* the records is not an error: the read stops
/// there and the partial [`Capture`] reports it in
/// [`Capture::stopped_early`].
pub fn read_capture(path: &Path) -> Result<Capture, TraceError> {
    let mut file = fs_err::File::open(path)?;
    let mut magic = [0_u8; 4];
    let magic_len = read_prefix(&mut file, &mut magic)?;
    file.rewind()?;
    let reader = BufReader::new(file);
    if magic_len == PCAPNG_MAGIC.len() && magic == PCAPNG_MAGIC {
        read_pcapng(reader)
    } else {
        read_classic_pcap(reader)
    }
}

/// Fills `buffer` from `reader`, returning how many bytes a short file left it
/// with.
fn read_prefix<R: Read>(reader: &mut R, buffer: &mut [u8]) -> Result<usize, TraceError> {
    let mut filled = 0_usize;
    while let Some(rest) = buffer.get_mut(filled..) {
        if rest.is_empty() {
            break;
        }
        let read = reader.read(rest)?;
        if read == 0 {
            break;
        }
        filled = filled.saturating_add(read);
    }
    Ok(filled)
}

/// Reads a classic `.pcap` stream, whose single link type applies to every
/// frame.
///
/// Records are read **raw**: `pcap_file`'s cooked `PcapPacket` rejects any
/// record whose original wire length exceeds the file's snaplen, which is
/// precisely what every record of a `tcpdump -s <n>` capture looks like, and
/// rejecting the first one would abort the read of an otherwise perfectly
/// usable short capture.
fn read_classic_pcap<R: Read>(reader: R) -> Result<Capture, TraceError> {
    let mut reader =
        PcapReader::new(reader).map_err(|error| TraceError::Pcap(error.to_string()))?;
    let header = reader.header();
    let kind =
        link_kind(header.datalink).ok_or(TraceError::UnsupportedLinkType(header.datalink))?;

    let mut capture = Capture::default();
    while let Some(packet) = reader.next_raw_packet() {
        let packet = match packet {
            Ok(packet) => packet,
            Err(error) => {
                capture.stopped_early = Some(error.to_string());
                break;
            }
        };
        let Ok(timestamp) = record_timestamp(packet.ts_sec, packet.ts_frac, header.ts_resolution)
        else {
            capture.skip();
            continue;
        };
        if let Some(datagram) = peel(kind, &packet.data, timestamp) {
            capture.push(datagram);
        }
    }
    Ok(capture)
}

/// Reads a `.pcapng` stream, tracking each interface's link type so packets are
/// peeled with the right encapsulation.
///
/// The interface-ID space restarts at every Section Header Block, so a rotated
/// or concatenated capture resets the table there rather than peeling the new
/// section's packets with the previous section's link types.
fn read_pcapng<R: Read>(reader: R) -> Result<Capture, TraceError> {
    let mut reader =
        PcapNgReader::new(reader).map_err(|error| TraceError::Pcap(error.to_string()))?;
    let mut interface_kinds: Vec<Option<LinkKind>> = Vec::new();
    let mut capture = Capture::default();

    while let Some(block) = reader.next_block() {
        let block = match block {
            Ok(block) => block,
            Err(error) => {
                capture.stopped_early = Some(error.to_string());
                break;
            }
        };
        match block {
            Block::SectionHeader(_) => interface_kinds.clear(),
            Block::InterfaceDescription(description) => {
                interface_kinds.push(link_kind(description.linktype));
            }
            Block::EnhancedPacket(packet) => {
                let index = usize::try_from(packet.interface_id).unwrap_or(usize::MAX);
                let Some(Some(kind)) = interface_kinds.get(index).copied() else {
                    capture.skip();
                    continue;
                };
                let Ok(timestamp) = duration_to_datetime(packet.timestamp) else {
                    capture.skip();
                    continue;
                };
                if let Some(datagram) = peel(kind, &packet.data, timestamp) {
                    capture.push(datagram);
                }
            }
            _ => {}
        }
    }
    Ok(capture)
}

/// Converts a classic-pcap record's two timestamp fields to an
/// [`OffsetDateTime`], honouring whether the file's magic declared the
/// fractional part to be microseconds or nanoseconds.
fn record_timestamp(
    seconds: u32,
    fraction: u32,
    resolution: TsResolution,
) -> Result<OffsetDateTime, TraceError> {
    let per_unit = match resolution {
        TsResolution::MicroSecond => 1_000,
        TsResolution::NanoSecond => 1,
    };
    let total = i128::from(seconds)
        .checked_mul(1_000_000_000)
        .and_then(|whole| whole.checked_add(i128::from(fraction).saturating_mul(per_unit)))
        .ok_or_else(|| TraceError::Time("timestamp out of range".to_owned()))?;
    OffsetDateTime::from_unix_timestamp_nanos(total)
        .map_err(|error| TraceError::Time(error.to_string()))
}

/// Converts a since-epoch [`Duration`] to an [`OffsetDateTime`].
fn duration_to_datetime(since_epoch: Duration) -> Result<OffsetDateTime, TraceError> {
    let seconds = i128::from(since_epoch.as_secs());
    let nanos = i128::from(since_epoch.subsec_nanos());
    let total = seconds
        .checked_mul(1_000_000_000)
        .and_then(|whole| whole.checked_add(nanos))
        .ok_or_else(|| TraceError::Time("timestamp out of range".to_owned()))?;
    OffsetDateTime::from_unix_timestamp_nanos(total)
        .map_err(|error| TraceError::Time(error.to_string()))
}

/// Peels a captured frame down to a UDP datagram, or `None` if it is not a
/// parseable IPv4/IPv6 UDP packet.
///
/// The slicing is *lax*: a frame whose captured bytes stop short of the length
/// its headers declare still yields its datagram, flagged
/// [`UdpDatagram::truncated`], instead of being dropped as unparsable.
fn peel(kind: LinkKind, frame: &[u8], timestamp: OffsetDateTime) -> Option<UdpDatagram> {
    let sliced = match kind {
        LinkKind::Ethernet => LaxSlicedPacket::from_ethernet(frame).ok()?,
        LinkKind::LinuxSll => peel_linux_sll(frame)?,
        LinkKind::RawIp => LaxSlicedPacket::from_ip(frame).ok()?,
        LinkKind::BsdLoopback => LaxSlicedPacket::from_ip(frame.get(4..)?).ok()?,
    };

    let (source_ip, destination_ip, ip_version, ip_hop_limit, ip_total_len) = match sliced.net? {
        LaxNetSlice::Ipv4(ipv4) => {
            let header = ipv4.header();
            (
                IpAddr::V4(header.source_addr()),
                IpAddr::V4(header.destination_addr()),
                4,
                header.ttl(),
                header.total_len(),
            )
        }
        LaxNetSlice::Ipv6(ipv6) => {
            let header = ipv6.header();
            (
                IpAddr::V6(header.source_addr()),
                IpAddr::V6(header.destination_addr()),
                6,
                header.hop_limit(),
                header.payload_length(),
            )
        }
        LaxNetSlice::Arp(_) => return None,
    };

    let TransportSlice::Udp(udp) = sliced.transport? else {
        return None;
    };

    let payload = udp.payload();
    let udp_length = udp.length();
    let captured_len = payload.len().saturating_add(UDP_HEADER_LEN);
    Some(UdpDatagram {
        timestamp,
        source: SocketAddr::new(source_ip, udp.source_port()),
        destination: SocketAddr::new(destination_ip, udp.destination_port()),
        ip_version,
        ip_hop_limit,
        ip_total_len,
        udp_length,
        udp_checksum: udp.checksum(),
        truncated: usize::from(udp_length) > captured_len,
        payload: payload.to_vec(),
    })
}

/// Peels a Linux "cooked" SLL v1 frame, whose 16-byte header ends in the
/// ether type of what it carries.
fn peel_linux_sll(frame: &[u8]) -> Option<LaxSlicedPacket<'_>> {
    let sll = LinuxSllSlice::from_slice(frame).ok()?;
    let LinuxSllProtocolType::EtherType(ether_type) = sll.protocol_type() else {
        return None;
    };
    Some(LaxSlicedPacket::from_ether_type(
        ether_type,
        sll.payload_slice(),
    ))
}

#[cfg(test)]
mod test {
    use std::borrow::Cow;
    use std::io::Cursor;
    use std::time::Duration;

    use etherparse::PacketBuilder;
    use pcap_file::DataLink;
    use pcap_file::pcap::{PcapHeader, PcapPacket, PcapWriter};
    use pcap_file::pcapng::blocks::enhanced_packet::EnhancedPacketBlock;
    use pcap_file::pcapng::blocks::interface_description::InterfaceDescriptionBlock;
    use pcap_file::pcapng::blocks::section_header::SectionHeaderBlock;
    use pcap_file::pcapng::{PcapNgBlock as _, PcapNgWriter};
    use pretty_assertions::assert_eq;

    use crate::trace::pcap::{Capture, TraceError, read_classic_pcap, read_pcapng};

    /// An Ethernet/IPv4/UDP frame from `127.0.0.1:src` to `127.0.0.1:dst` with
    /// `payload` as its body.
    fn ethernet_udp(source_port: u16, destination_port: u16, payload: &[u8]) -> Vec<u8> {
        let builder = PacketBuilder::ethernet2([1, 2, 3, 4, 5, 6], [7, 8, 9, 10, 11, 12])
            .ipv4([127, 0, 0, 1], [127, 0, 0, 1], 64)
            .udp(source_port, destination_port);
        let mut frame = Vec::with_capacity(builder.size(payload.len()));
        builder.write(&mut frame, payload).ok();
        frame
    }

    /// A classic `.pcap` file holding `frames`, with the given link type, a
    /// snaplen of `snaplen`, and each frame's original wire length recorded as
    /// the second element of its pair.
    fn classic_pcap_with(datalink: DataLink, snaplen: u32, frames: &[(Vec<u8>, u32)]) -> Vec<u8> {
        let header = PcapHeader {
            datalink,
            snaplen,
            ..PcapHeader::default()
        };
        let mut writer = PcapWriter::with_header(Vec::new(), header).ok();
        if let Some(writer) = writer.as_mut() {
            for (frame, original_len) in frames {
                writer
                    .write_packet(&PcapPacket::new(
                        Duration::from_secs(1),
                        *original_len,
                        frame,
                    ))
                    .ok();
            }
        }
        writer.map(PcapWriter::into_writer).unwrap_or_default()
    }

    /// A classic `.pcap` file of whole, untruncated frames.
    fn classic_pcap(datalink: DataLink, frames: &[Vec<u8>]) -> Vec<u8> {
        let sized: Vec<(Vec<u8>, u32)> = frames
            .iter()
            .map(|frame| {
                (
                    frame.clone(),
                    u32::try_from(frame.len()).unwrap_or(u32::MAX),
                )
            })
            .collect();
        classic_pcap_with(datalink, 0xFFFF, &sized)
    }

    #[test]
    fn reads_a_classic_pcap() -> Result<(), TraceError> {
        let bytes = classic_pcap(
            DataLink::ETHERNET,
            &[ethernet_udp(52344, 9000, &[1, 2, 3, 4])],
        );
        let capture = read_classic_pcap(Cursor::new(bytes))?;
        assert_eq!(capture.datagrams.len(), 1);
        assert_eq!(capture.stopped_early, None);
        let datagram = capture.datagrams.first().map(|datagram| {
            (
                datagram.source.port(),
                datagram.destination.port(),
                datagram.payload.clone(),
                datagram.truncated,
            )
        });
        assert_eq!(datagram, Some((52344, 9000, vec![1, 2, 3, 4], false)));
        Ok(())
    }

    #[test]
    fn an_unsupported_classic_link_type_is_an_error() {
        let bytes = classic_pcap(DataLink::BLUETOOTH_HCI_H4, &[]);
        let read = read_classic_pcap(Cursor::new(bytes));
        assert!(matches!(read, Err(TraceError::UnsupportedLinkType(_))));
    }

    #[test]
    fn a_truncated_classic_pcap_keeps_what_it_read() -> Result<(), TraceError> {
        let mut bytes = classic_pcap(
            DataLink::ETHERNET,
            &[
                ethernet_udp(52344, 9000, &[1, 2, 3, 4]),
                ethernet_udp(9000, 52344, &[5, 6, 7, 8]),
            ],
        );
        // Cut the file in the middle of the second record, the way a Ctrl-C'd
        // `tcpdump` leaves it.
        bytes.truncate(bytes.len().saturating_sub(20));

        let capture = read_classic_pcap(Cursor::new(bytes))?;
        assert_eq!(capture.datagrams.len(), 1);
        assert!(capture.stopped_early.is_some());
        Ok(())
    }

    #[test]
    fn a_snaplen_truncated_datagram_is_flagged_not_dropped() -> Result<(), TraceError> {
        let mut frame = ethernet_udp(52344, 9000, &[1, 2, 3, 4, 5, 6, 7, 8]);
        let wire_len = u32::try_from(frame.len()).unwrap_or(u32::MAX);
        // Keep the headers, drop half the UDP payload, as a short snaplen does.
        frame.truncate(frame.len().saturating_sub(4));
        // A `tcpdump -s <n>` capture records the whole wire length even though
        // it only stored `n` bytes, so `orig_len` legitimately exceeds the
        // snaplen — a shape the reader has to accept, not reject.
        let snaplen = u32::try_from(frame.len()).unwrap_or(u32::MAX);
        let capture = read_classic_pcap(Cursor::new(classic_pcap_with(
            DataLink::ETHERNET,
            snaplen,
            &[(frame, wire_len)],
        )))?;

        assert_eq!(capture.snaplen_truncated, 1);
        let datagram = capture.datagrams.first().map(|datagram| {
            (
                datagram.truncated,
                datagram.udp_length,
                datagram.payload.len(),
            )
        });
        assert_eq!(datagram, Some((true, 16, 4)));
        Ok(())
    }

    /// A `.pcapng` with two sections: an Ethernet interface in the first and an
    /// unsupported one in the second, each carrying one frame.
    fn two_section_pcapng(second: DataLink, frame: &[u8]) -> Vec<u8> {
        let mut writer = PcapNgWriter::new(Vec::new()).ok();
        if let Some(writer) = writer.as_mut() {
            let packet = EnhancedPacketBlock {
                interface_id: 0,
                timestamp: Duration::from_secs(1),
                original_len: u32::try_from(frame.len()).unwrap_or(u32::MAX),
                data: Cow::Borrowed(frame),
                options: Vec::new(),
            };
            for datalink in [DataLink::ETHERNET, second] {
                if datalink != DataLink::ETHERNET {
                    writer
                        .write_block(&SectionHeaderBlock::default().into_block())
                        .ok();
                }
                writer
                    .write_pcapng_block(InterfaceDescriptionBlock {
                        linktype: datalink,
                        snaplen: 0xFFFF,
                        options: Vec::new(),
                    })
                    .ok();
                writer.write_block(&packet.clone().into_block()).ok();
            }
        }
        writer.map(PcapNgWriter::into_inner).unwrap_or_default()
    }

    #[test]
    fn a_new_section_restarts_the_interface_table() -> Result<(), TraceError> {
        let frame = ethernet_udp(52344, 9000, &[1, 2, 3, 4]);

        // Both sections describe Ethernet: both frames peel.
        let both = read_pcapng(Cursor::new(two_section_pcapng(DataLink::ETHERNET, &frame)))?;
        assert_eq!(both.datagrams.len(), 2);

        // The second section's interface 0 is a link type we cannot peel. It
        // must not inherit the first section's Ethernet, which would have
        // peeled its frame as if the table had never restarted.
        let mixed = read_pcapng(Cursor::new(two_section_pcapng(
            DataLink::BLUETOOTH_HCI_H4,
            &frame,
        )))?;
        assert_eq!(mixed.datagrams.len(), 1);
        assert_eq!(mixed.skipped_frames, 1);
        Ok(())
    }

    #[test]
    fn a_capture_default_is_empty() {
        let capture = Capture::default();
        assert!(capture.datagrams.is_empty());
        assert_eq!(capture.snaplen_truncated, 0);
        assert_eq!(capture.skipped_frames, 0);
        assert_eq!(capture.stopped_early, None);
    }
}
