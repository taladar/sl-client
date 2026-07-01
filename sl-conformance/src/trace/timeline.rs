//! Correlating the pcap datagrams with the log, decoding the LLUDP bodies, and
//! rendering the merged timeline as text and JSON-Lines.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::net::IpAddr;

use sl_wire::{
    AnyMessage, MessageId, PacketFlags, ParsedDatagram, Reader, WireError, message_name,
    parse_datagram, zero_decode,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::trace::logfile::LogFile;
use crate::trace::pcap::UdpDatagram;
use crate::trace::{Direction, TraceError, Transport};

/// The simulator and (optionally) viewer IP addresses used to label direction.
#[derive(Debug, Clone, Default)]
pub struct Endpoints {
    /// IP addresses known to be the simulator side.
    pub sim_ips: HashSet<IpAddr>,
    /// IP addresses known to be the viewer side (a fallback when no sim IP is
    /// known for a datagram).
    pub viewer_ips: HashSet<IpAddr>,
}

impl Endpoints {
    /// Labels a datagram's direction, or `None` if neither endpoint is known.
    #[must_use]
    pub fn direction_of(&self, datagram: &UdpDatagram) -> Option<Direction> {
        let source = datagram.source.ip();
        let destination = datagram.destination.ip();
        if self.sim_ips.contains(&destination) || self.viewer_ips.contains(&source) {
            Some(Direction::ViewerToSim)
        } else if self.sim_ips.contains(&source) || self.viewer_ips.contains(&destination) {
            Some(Direction::SimToViewer)
        } else {
            None
        }
    }
}

/// The LLUDP framing metadata common to any datagram whose header parsed.
#[derive(Debug, Clone)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "these are the four independent LLUDP packet-header flags"
)]
struct Framing {
    /// Whether the `RELIABLE` flag was set.
    reliable: bool,
    /// Whether the `RESENT` flag was set.
    resent: bool,
    /// Whether the `ZEROCODED` flag was set.
    zerocoded: bool,
    /// Whether the `ACK` (appended acks) flag was set.
    ack: bool,
    /// The datagram sequence number.
    sequence: u32,
    /// The raw extra-header bytes (usually empty).
    extra: Vec<u8>,
    /// The appended acknowledgement sequence numbers.
    acks: Vec<u32>,
    /// The message body after zero-decoding.
    decoded_body: Vec<u8>,
    /// The on-the-wire body length (before zero-decoding).
    wire_body_len: usize,
}

/// The outcome of decoding one datagram's LLUDP content.
#[derive(Debug)]
enum Decoded {
    /// The message decoded fully.
    Message {
        /// The parsed framing metadata.
        framing: Framing,
        /// The decoded message.
        message: Box<AnyMessage>,
    },
    /// The header parsed but the message body did not.
    BodyError {
        /// The parsed framing metadata.
        framing: Framing,
        /// The message template name, if the id resolved.
        name: Option<&'static str>,
        /// The decode error.
        error: WireError,
    },
    /// The datagram header itself did not parse as LLUDP.
    FrameError {
        /// The header parse error.
        error: WireError,
    },
}

/// One entry in the merged timeline.
#[expect(
    clippy::module_name_repetitions,
    reason = "`TimelineEntry` reads best as this module's public entry type"
)]
#[derive(Debug)]
pub struct TimelineEntry {
    /// The datagram and its IP/UDP metadata.
    datagram: UdpDatagram,
    /// The direction relative to the viewer.
    direction: Direction,
    /// The viewer's own timestamp for this message, if it correlated to a log
    /// line.
    viewer_timestamp: Option<OffsetDateTime>,
    /// The decoded LLUDP content.
    decoded: Decoded,
}

/// Builds the merged, pcap-time-ordered timeline.
///
/// Datagrams whose direction cannot be established (neither endpoint is a known
/// simulator or viewer address) are dropped, which also filters out non-circuit
/// UDP such as DNS.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "`build_timeline` reads best as this module's public entry point"
)]
pub fn build_timeline(
    datagrams: Vec<UdpDatagram>,
    log: &LogFile,
    endpoints: &Endpoints,
) -> Vec<TimelineEntry> {
    let mut index: HashMap<(Direction, u32, String), VecDeque<Option<OffsetDateTime>>> =
        HashMap::new();
    for message in &log.messages {
        index
            .entry((message.direction, message.packet_id, message.name.clone()))
            .or_default()
            .push_back(message.timestamp);
    }

    let mut entries = Vec::new();
    for datagram in datagrams {
        let Some(direction) = endpoints.direction_of(&datagram) else {
            continue;
        };
        let decoded = decode(&datagram.payload);
        let viewer_timestamp = correlate(&mut index, direction, &decoded);
        entries.push(TimelineEntry {
            datagram,
            direction,
            viewer_timestamp,
            decoded,
        });
    }

    entries.sort_by_key(|entry| entry.datagram.timestamp);
    entries
}

/// The number of entries whose message did not fully decode.
#[must_use]
pub fn error_count(entries: &[TimelineEntry]) -> usize {
    entries
        .iter()
        .filter(|entry| !matches!(entry.decoded, Decoded::Message { .. }))
        .count()
}

/// Looks up (and consumes) the viewer timestamp for a decoded datagram.
fn correlate(
    index: &mut HashMap<(Direction, u32, String), VecDeque<Option<OffsetDateTime>>>,
    direction: Direction,
    decoded: &Decoded,
) -> Option<OffsetDateTime> {
    let key = match decoded {
        Decoded::Message { framing, message } => {
            (direction, framing.sequence, message.name().to_owned())
        }
        Decoded::BodyError {
            framing,
            name: Some(name),
            ..
        } => (direction, framing.sequence, (*name).to_owned()),
        Decoded::BodyError { name: None, .. } | Decoded::FrameError { .. } => return None,
    };
    index.get_mut(&key).and_then(VecDeque::pop_front).flatten()
}

/// Decodes one UDP payload into its LLUDP framing and message.
fn decode(payload: &[u8]) -> Decoded {
    let parsed = match parse_datagram(payload) {
        Ok(parsed) => parsed,
        Err(error) => return Decoded::FrameError { error },
    };
    let zerocoded = parsed.flags.contains(PacketFlags::ZEROCODED);
    let decoded_body = if zerocoded {
        match zero_decode(parsed.body) {
            Ok(body) => body,
            Err(error) => {
                let framing = make_framing(&parsed, zerocoded, parsed.body.to_vec());
                return Decoded::BodyError {
                    framing,
                    name: None,
                    error,
                };
            }
        }
    } else {
        parsed.body.to_vec()
    };

    let (name, message_result) = {
        let mut reader = Reader::new(&decoded_body);
        match MessageId::decode(&mut reader) {
            Ok(id) => (message_name(id), AnyMessage::decode(id, &mut reader)),
            Err(error) => (None, Err(error)),
        }
    };
    let framing = make_framing(&parsed, zerocoded, decoded_body);
    match message_result {
        Ok(message) => Decoded::Message {
            framing,
            message: Box::new(message),
        },
        Err(error) => Decoded::BodyError {
            framing,
            name,
            error,
        },
    }
}

/// Assembles the [`Framing`] metadata from a parsed datagram.
fn make_framing(parsed: &ParsedDatagram<'_>, zerocoded: bool, decoded_body: Vec<u8>) -> Framing {
    Framing {
        reliable: parsed.flags.contains(PacketFlags::RELIABLE),
        resent: parsed.flags.contains(PacketFlags::RESENT),
        zerocoded,
        ack: parsed.flags.contains(PacketFlags::ACK),
        sequence: parsed.sequence.get(),
        extra: parsed.extra.to_vec(),
        acks: parsed.acks.iter().map(|sequence| sequence.get()).collect(),
        wire_body_len: parsed.body.len(),
        decoded_body,
    }
}

/// Formats an [`OffsetDateTime`] as RFC-3339, or a placeholder on failure.
fn format_time(time: OffsetDateTime) -> String {
    time.format(&Rfc3339)
        .unwrap_or_else(|_| "<bad-timestamp>".to_owned())
}

/// Renders bytes as space-separated lowercase hex.
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The set flag names of a framing, space-joined (or `-` if none).
fn flag_labels(framing: &Framing) -> String {
    let mut labels = Vec::new();
    if framing.reliable {
        labels.push("reliable");
    }
    if framing.resent {
        labels.push("resent");
    }
    if framing.zerocoded {
        labels.push("zerocoded");
    }
    if framing.ack {
        labels.push("ack");
    }
    if labels.is_empty() {
        "-".to_owned()
    } else {
        labels.join(" ")
    }
}

/// Renders the timeline as the human-readable text form.
///
/// When `include_raw` is set, successfully-decoded messages also get a raw hex
/// dump of their decoded body.
#[must_use]
pub fn render_text(entries: &[TimelineEntry], include_raw: bool) -> String {
    let mut out = String::new();
    for entry in entries {
        write_entry(&mut out, entry, include_raw).ok();
    }
    out
}

/// Writes one text entry. Writing to a `String` cannot actually fail.
fn write_entry(out: &mut String, entry: &TimelineEntry, include_raw: bool) -> std::fmt::Result {
    let datagram = &entry.datagram;
    let heading = match &entry.decoded {
        Decoded::Message { message, .. } => message.name().to_owned(),
        Decoded::BodyError {
            name: Some(name), ..
        } => format!("{name} <PARSE ERROR>"),
        Decoded::BodyError { name: None, .. } | Decoded::FrameError { .. } => {
            "<PARSE ERROR>".to_owned()
        }
    };
    writeln!(
        out,
        "{}  {}  {heading}",
        format_time(datagram.timestamp),
        entry.direction.label()
    )?;
    if let Some(viewer) = entry.viewer_timestamp {
        writeln!(out, "    viewer_ts {}", format_time(viewer))?;
    }
    writeln!(
        out,
        "    ip   {} -> {}  ttl={} len={}",
        datagram.source.ip(),
        datagram.destination.ip(),
        datagram.ip_hop_limit,
        datagram.ip_total_len
    )?;
    writeln!(
        out,
        "    udp  {} -> {}  len={}",
        datagram.source.port(),
        datagram.destination.port(),
        datagram.udp_length
    )?;

    match &entry.decoded {
        Decoded::Message { framing, message } => {
            write_framing(out, framing)?;
            writeln!(out, "    {message:#?}")?;
            if include_raw {
                writeln!(out, "    raw (decoded): {}", hex(&framing.decoded_body))?;
            }
        }
        Decoded::BodyError { framing, error, .. } => {
            write_framing(out, framing)?;
            writeln!(out, "    error: {error}")?;
            writeln!(out, "    raw (decoded): {}", hex(&framing.decoded_body))?;
        }
        Decoded::FrameError { error } => {
            writeln!(out, "    error: {error}")?;
            writeln!(out, "    raw (payload): {}", hex(&datagram.payload))?;
        }
    }
    writeln!(out)
}

/// Writes the `udp2` LLUDP-framing line of a text entry.
fn write_framing(out: &mut String, framing: &Framing) -> std::fmt::Result {
    let extra = if framing.extra.is_empty() {
        String::new()
    } else {
        hex(&framing.extra)
    };
    writeln!(
        out,
        "    udp2 seq={}  {}  acks={:?}  extra=\"{extra}\"  body={}B(decoded {}B)",
        framing.sequence,
        flag_labels(framing),
        framing.acks,
        framing.wire_body_len,
        framing.decoded_body.len()
    )
}

/// The IP-header metadata of a JSON-Lines record.
#[derive(serde::Serialize)]
struct IpJson {
    /// The source IP address.
    src: String,
    /// The destination IP address.
    dst: String,
    /// The IP version (4 or 6).
    version: u8,
    /// The IPv4 TTL / IPv6 hop limit.
    hop_limit: u8,
    /// The IPv4 total length / IPv6 payload length.
    total_len: u16,
}

/// The UDP-header metadata of a JSON-Lines record.
#[derive(serde::Serialize)]
struct UdpJson {
    /// The source port.
    src_port: u16,
    /// The destination port.
    dst_port: u16,
    /// The UDP length field.
    len: u16,
    /// The UDP checksum field.
    checksum: u16,
}

/// The LLUDP-framing metadata of a JSON-Lines record.
#[derive(serde::Serialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "these are the four independent LLUDP packet-header flags"
)]
struct LludpJson {
    /// The sequence number.
    seq: u32,
    /// Whether the reliable flag was set.
    reliable: bool,
    /// Whether the resent flag was set.
    resent: bool,
    /// Whether the zero-coded flag was set.
    zerocoded: bool,
    /// Whether the appended-acks flag was set.
    ack: bool,
    /// The appended acknowledgement sequence numbers.
    acks: Vec<u32>,
    /// The raw extra-header bytes as hex.
    extra_hex: String,
    /// The on-the-wire body length.
    body_len: usize,
    /// The decoded (zero-expanded) body length.
    decoded_len: usize,
}

/// One JSON-Lines timeline record.
#[derive(serde::Serialize)]
struct Record<'a> {
    /// The capture timestamp (RFC-3339).
    ts: String,
    /// The viewer's timestamp, if correlated.
    #[serde(skip_serializing_if = "Option::is_none")]
    viewer_ts: Option<String>,
    /// The direction relative to the viewer.
    direction: &'static str,
    /// The transport discriminator (always `udp` this iteration).
    transport: Transport,
    /// The message template name, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    /// Whether the message decoded fully.
    ok: bool,
    /// The IP-header metadata.
    ip: IpJson,
    /// The UDP-header metadata.
    udp: UdpJson,
    /// The LLUDP-framing metadata (absent when the header did not parse).
    #[serde(skip_serializing_if = "Option::is_none")]
    lludp: Option<LludpJson>,
    /// The decoded message, structured, on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    body: Option<&'a AnyMessage>,
    /// The decode error, when the message did not decode.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    /// The raw decoded bytes (or payload for a frame error) as hex.
    raw_hex: String,
}

/// Renders the timeline as JSON-Lines (one JSON object per line).
///
/// # Errors
///
/// Returns [`TraceError`] if a record cannot be serialized.
pub fn render_jsonl(entries: &[TimelineEntry]) -> Result<String, TraceError> {
    let mut out = String::new();
    for entry in entries {
        let record = to_record(entry);
        let line = serde_json::to_string(&record)?;
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

/// Builds the JSON-Lines record for one entry.
fn to_record(entry: &TimelineEntry) -> Record<'_> {
    let datagram = &entry.datagram;
    let (name, ok, lludp, body, error, raw_hex) = match &entry.decoded {
        Decoded::Message { framing, message } => (
            Some(message.name()),
            true,
            Some(lludp_json(framing)),
            Some(message.as_ref()),
            None,
            hex(&framing.decoded_body),
        ),
        Decoded::BodyError {
            framing,
            name,
            error,
        } => (
            *name,
            false,
            Some(lludp_json(framing)),
            None,
            Some(error.to_string()),
            hex(&framing.decoded_body),
        ),
        Decoded::FrameError { error } => (
            None,
            false,
            None,
            None,
            Some(error.to_string()),
            hex(&datagram.payload),
        ),
    };

    Record {
        ts: format_time(datagram.timestamp),
        viewer_ts: entry.viewer_timestamp.map(format_time),
        direction: entry.direction.json(),
        transport: Transport::Udp,
        name,
        ok,
        ip: IpJson {
            src: datagram.source.ip().to_string(),
            dst: datagram.destination.ip().to_string(),
            version: datagram.ip_version,
            hop_limit: datagram.ip_hop_limit,
            total_len: datagram.ip_total_len,
        },
        udp: UdpJson {
            src_port: datagram.source.port(),
            dst_port: datagram.destination.port(),
            len: datagram.udp_length,
            checksum: datagram.udp_checksum,
        },
        lludp,
        body,
        error,
        raw_hex,
    }
}

/// Builds the LLUDP-framing JSON metadata.
fn lludp_json(framing: &Framing) -> LludpJson {
    LludpJson {
        seq: framing.sequence,
        reliable: framing.reliable,
        resent: framing.resent,
        zerocoded: framing.zerocoded,
        ack: framing.ack,
        acks: framing.acks.clone(),
        extra_hex: hex(&framing.extra),
        body_len: framing.wire_body_len,
        decoded_len: framing.decoded_body.len(),
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::net::{IpAddr, SocketAddr};

    use pretty_assertions::assert_eq;
    use time::OffsetDateTime;

    use crate::trace::logfile::LogFile;
    use crate::trace::pcap::UdpDatagram;
    use crate::trace::timeline::{Endpoints, build_timeline, render_jsonl, render_text};

    /// A `CompletePingCheck` (High id 2, `ping_id = 7`, seq 42, unreliable)
    /// wrapped as a UDP datagram from `source` to `destination`.
    fn ping_datagram(source: SocketAddr, destination: SocketAddr) -> UdpDatagram {
        UdpDatagram {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            source,
            destination,
            ip_version: 4,
            ip_hop_limit: 64,
            ip_total_len: 36,
            udp_length: 16,
            udp_checksum: 0,
            payload: vec![0x00, 0x00, 0x00, 0x00, 0x2a, 0x00, 0x02, 0x07],
        }
    }

    #[test]
    fn decodes_and_labels_a_ping() -> Result<(), Box<dyn std::error::Error>> {
        let sim = SocketAddr::new(IpAddr::from([1, 2, 3, 4]), 13000);
        let viewer = SocketAddr::new(IpAddr::from([10, 0, 0, 1]), 52344);
        let mut endpoints = Endpoints::default();
        endpoints.sim_ips.insert(sim.ip());

        let entries = build_timeline(
            vec![ping_datagram(viewer, sim)],
            &LogFile::default(),
            &endpoints,
        );
        assert_eq!(entries.len(), 1);

        let text = render_text(&entries, false);
        assert!(text.contains("V->S"));
        assert!(text.contains("CompletePingCheck"));

        let jsonl = render_jsonl(&entries)?;
        assert!(jsonl.contains("\"direction\":\"viewer_to_sim\""));
        assert!(jsonl.contains("CompletePingCheck"));
        assert!(jsonl.contains("\"ok\":true"));
        Ok(())
    }

    #[test]
    fn drops_non_circuit_datagrams() {
        let endpoints = Endpoints {
            sim_ips: HashSet::from([IpAddr::from([1, 2, 3, 4])]),
            viewer_ips: HashSet::new(),
        };
        let unrelated = build_timeline(
            vec![ping_datagram(
                SocketAddr::new(IpAddr::from([9, 9, 9, 9]), 53),
                SocketAddr::new(IpAddr::from([8, 8, 8, 8]), 53),
            )],
            &LogFile::default(),
            &endpoints,
        );
        assert!(unrelated.is_empty());
    }
}
