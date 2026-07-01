//! Reading `.pcap` / `.pcapng` captures and peeling link/IP/UDP off each frame.
//!
//! Yields the LLUDP UDP datagrams with the IP and UDP header metadata retained,
//! so nothing from the wire is lost before the LLUDP body is decoded.

use std::io::Cursor;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use etherparse::{NetSlice, SlicedPacket, TransportSlice};
use pcap_file::DataLink;
use pcap_file::pcap::PcapReader;
use pcap_file::pcapng::{Block, PcapNgReader};
use time::OffsetDateTime;

use crate::trace::TraceError;

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
    /// The UDP payload (the LLUDP datagram).
    pub payload: Vec<u8>,
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
/// # Errors
///
/// Returns [`TraceError`] if the file cannot be read, the container cannot be
/// parsed, or (for classic pcap) the link-layer type is unsupported.
pub fn read_udp_datagrams(path: &Path) -> Result<Vec<UdpDatagram>, TraceError> {
    let bytes = fs_err::read(path)?;
    if bytes.get(0..4) == Some(&[0x0a, 0x0d, 0x0d, 0x0a]) {
        read_pcapng(&bytes)
    } else {
        read_classic_pcap(&bytes)
    }
}

/// Reads a classic `.pcap` file, whose single link type applies to every frame.
fn read_classic_pcap(bytes: &[u8]) -> Result<Vec<UdpDatagram>, TraceError> {
    let mut reader =
        PcapReader::new(Cursor::new(bytes)).map_err(|error| TraceError::Pcap(error.to_string()))?;
    let datalink = reader.header().datalink;
    let kind = link_kind(datalink).ok_or(TraceError::UnsupportedLinkType(datalink))?;

    let mut datagrams = Vec::new();
    while let Some(packet) = reader.next_packet() {
        let packet = packet.map_err(|error| TraceError::Pcap(error.to_string()))?;
        let timestamp = duration_to_datetime(packet.timestamp)?;
        if let Some(datagram) = peel(kind, &packet.data, timestamp) {
            datagrams.push(datagram);
        }
    }
    Ok(datagrams)
}

/// Reads a `.pcapng` file, tracking each interface's link type so packets are
/// peeled with the right encapsulation.
fn read_pcapng(bytes: &[u8]) -> Result<Vec<UdpDatagram>, TraceError> {
    let mut reader = PcapNgReader::new(Cursor::new(bytes))
        .map_err(|error| TraceError::Pcap(error.to_string()))?;
    let mut interface_kinds: Vec<Option<LinkKind>> = Vec::new();
    let mut datagrams = Vec::new();

    while let Some(block) = reader.next_block() {
        let block = block.map_err(|error| TraceError::Pcap(error.to_string()))?;
        match block {
            Block::InterfaceDescription(description) => {
                interface_kinds.push(link_kind(description.linktype));
            }
            Block::EnhancedPacket(packet) => {
                let index = usize::try_from(packet.interface_id).unwrap_or(usize::MAX);
                let Some(Some(kind)) = interface_kinds.get(index).copied() else {
                    continue;
                };
                let timestamp = duration_to_datetime(packet.timestamp)?;
                if let Some(datagram) = peel(kind, &packet.data, timestamp) {
                    datagrams.push(datagram);
                }
            }
            _ => {}
        }
    }
    Ok(datagrams)
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
fn peel(kind: LinkKind, frame: &[u8], timestamp: OffsetDateTime) -> Option<UdpDatagram> {
    let sliced = match kind {
        LinkKind::Ethernet => SlicedPacket::from_ethernet(frame).ok()?,
        LinkKind::LinuxSll => SlicedPacket::from_linux_sll(frame).ok()?,
        LinkKind::RawIp => SlicedPacket::from_ip(frame).ok()?,
        LinkKind::BsdLoopback => SlicedPacket::from_ip(frame.get(4..)?).ok()?,
    };

    let (source_ip, destination_ip, ip_version, ip_hop_limit, ip_total_len) = match sliced.net? {
        NetSlice::Ipv4(ipv4) => {
            let header = ipv4.header();
            (
                IpAddr::V4(header.source_addr()),
                IpAddr::V4(header.destination_addr()),
                4,
                header.ttl(),
                header.total_len(),
            )
        }
        NetSlice::Ipv6(ipv6) => {
            let header = ipv6.header();
            (
                IpAddr::V6(header.source_addr()),
                IpAddr::V6(header.destination_addr()),
                6,
                header.hop_limit(),
                header.payload_length(),
            )
        }
        _ => return None,
    };

    let TransportSlice::Udp(udp) = sliced.transport? else {
        return None;
    };

    Some(UdpDatagram {
        timestamp,
        source: SocketAddr::new(source_ip, udp.source_port()),
        destination: SocketAddr::new(destination_ip, udp.destination_port()),
        ip_version,
        ip_hop_limit,
        ip_total_len,
        udp_length: udp.length(),
        udp_checksum: udp.checksum(),
        payload: udp.payload().to_vec(),
    })
}
