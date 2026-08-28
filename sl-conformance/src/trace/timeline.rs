//! Correlating the pcap datagrams with the log, decoding the LLUDP bodies, and
//! rendering the merged timeline as text and JSON-Lines.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::net::{AddrParseError, IpAddr, SocketAddr};
use std::str::FromStr;

use sl_wire::{
    AnyMessage, MessageId, PacketFlags, ParsedDatagram, Reader, WireError, message_name,
    parse_datagram, zero_decode,
};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::trace::logfile::LogFile;
use crate::trace::pcap::UdpDatagram;
use crate::trace::{Direction, TraceError, Transport};

/// One endpoint the caller named: a full `ip:port`, or a bare IP standing for
/// every port on that address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointSpec {
    /// One exact `ip:port`.
    Socket(SocketAddr),
    /// Every port on one IP address.
    Ip(IpAddr),
}

impl FromStr for EndpointSpec {
    type Err = AddrParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text.parse::<SocketAddr>() {
            Ok(socket) => Ok(Self::Socket(socket)),
            Err(_) => text.parse::<IpAddr>().map(Self::Ip),
        }
    }
}

/// The addresses known to belong to one side of the conversation.
#[derive(Debug, Clone, Default)]
pub struct Side {
    /// Exact `ip:port` endpoints on this side.
    pub sockets: HashSet<SocketAddr>,
    /// IP addresses on this side, whatever the port.
    pub ips: HashSet<IpAddr>,
}

impl Side {
    /// Adds one caller-named endpoint to this side.
    pub fn insert(&mut self, spec: EndpointSpec) {
        match spec {
            EndpointSpec::Socket(socket) => {
                self.sockets.insert(socket);
            }
            EndpointSpec::Ip(ip) => {
                self.ips.insert(ip);
            }
        }
    }

    /// Whether nothing at all is known about this side.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sockets.is_empty() && self.ips.is_empty()
    }

    /// How specifically `address` is known to be on this side: an exact
    /// `ip:port` match outranks a whole-IP match, which outranks no match.
    fn specificity(&self, address: SocketAddr) -> u8 {
        if self.sockets.contains(&address) {
            2
        } else if self.ips.contains(&address.ip()) {
            1
        } else {
            0
        }
    }
}

/// The simulator and (optionally) viewer addresses used to label direction.
#[derive(Debug, Clone, Default)]
pub struct Endpoints {
    /// Addresses known to be the simulator side.
    pub sim: Side,
    /// Addresses known to be the viewer side (a fallback when no simulator
    /// address matches a datagram).
    pub viewer: Side,
}

/// How confidently a datagram's direction could be labelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Labelling {
    /// The direction is known.
    Known(Direction),
    /// Both ends match a known side equally well, so the direction cannot be
    /// told apart — the loopback case, where the simulator and the viewer share
    /// an IP and only a `--sim-addr` carrying the **port** separates them.
    Ambiguous,
    /// Neither end is a known simulator or viewer address.
    Unknown,
}

impl Endpoints {
    /// Labels a datagram's direction.
    ///
    /// The two candidate directions are scored by how specifically each end
    /// matches a known side, and the better-evidenced one wins. Scoring rather
    /// than testing the simulator side first is what keeps a loopback capture
    /// honest: with `127.0.0.1:9000` known as the simulator, the datagram *to*
    /// port 9000 and the reply *from* it score differently, where a plain
    /// "is either end the simulator's IP?" test would call both of them
    /// viewer-to-simulator.
    #[must_use]
    pub fn label(&self, datagram: &UdpDatagram) -> Labelling {
        let to_sim = self
            .sim
            .specificity(datagram.destination)
            .max(self.viewer.specificity(datagram.source));
        let to_viewer = self
            .sim
            .specificity(datagram.source)
            .max(self.viewer.specificity(datagram.destination));
        match to_sim.cmp(&to_viewer) {
            Ordering::Greater => Labelling::Known(Direction::ViewerToSim),
            Ordering::Less => Labelling::Known(Direction::SimToViewer),
            Ordering::Equal if to_sim == 0 => Labelling::Unknown,
            Ordering::Equal => Labelling::Ambiguous,
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

/// What one log line is matched against.
///
/// LLUDP sequence numbers are per **circuit**, and a viewer holds a root circuit
/// plus a child circuit per neighbouring region at once, so the simulator
/// endpoint has to be part of the key: without it an `AgentUpdate` seq 5 to
/// region A collides with seq 5 to region B and the two regions' timestamps get
/// swapped.
type CorrelationKey = (Direction, SocketAddr, u32, String);

/// The merged timeline plus what building it had to drop.
#[derive(Debug, Default)]
pub struct Timeline {
    /// The entries, in capture-timestamp order.
    pub entries: Vec<TimelineEntry>,
    /// Datagrams dropped because both ends matched a known side equally well.
    pub ambiguous: usize,
    /// Datagrams dropped because neither end was a known address — the
    /// non-circuit UDP (DNS and friends) the capture happened to include.
    pub unlabelled: usize,
}

/// Builds the merged, pcap-time-ordered timeline.
///
/// Datagrams whose direction cannot be established are dropped, which also
/// filters out non-circuit UDP such as DNS; [`Timeline::ambiguous`] and
/// [`Timeline::unlabelled`] say how many, and why.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "`build_timeline` reads best as this module's public entry point"
)]
pub fn build_timeline(
    mut datagrams: Vec<UdpDatagram>,
    log: &LogFile,
    endpoints: &Endpoints,
) -> Timeline {
    let mut index: HashMap<CorrelationKey, VecDeque<Option<OffsetDateTime>>> = HashMap::new();
    for message in &log.messages {
        index
            .entry((
                message.direction,
                message.host,
                message.packet_id,
                message.name.clone(),
            ))
            .or_default()
            .push_back(message.timestamp);
    }

    // Correlation consumes each log line in order, so the datagrams have to be
    // in time order *before* it runs — a merged or multi-interface capture is
    // not in time order on disk, and correlating in file order would hand each
    // repeat of a message the wrong occurrence's viewer timestamp.
    datagrams.sort_by_key(|datagram| datagram.timestamp);

    let mut timeline = Timeline::default();
    for datagram in datagrams {
        let direction = match endpoints.label(&datagram) {
            Labelling::Known(direction) => direction,
            Labelling::Ambiguous => {
                timeline.ambiguous = timeline.ambiguous.saturating_add(1);
                continue;
            }
            Labelling::Unknown => {
                timeline.unlabelled = timeline.unlabelled.saturating_add(1);
                continue;
            }
        };
        let sim_host = match direction {
            Direction::ViewerToSim => datagram.destination,
            Direction::SimToViewer => datagram.source,
        };
        let decoded = decode(&datagram.payload);
        let viewer_timestamp = correlate(&mut index, direction, sim_host, &decoded);
        timeline.entries.push(TimelineEntry {
            datagram,
            direction,
            viewer_timestamp,
            decoded,
        });
    }

    timeline
}

/// The number of entries whose message did not fully decode.
#[must_use]
pub fn error_count(entries: &[TimelineEntry]) -> usize {
    entries
        .iter()
        .filter(|entry| !matches!(entry.decoded, Decoded::Message { .. }))
        .count()
}

/// The number of entries whose datagram was snaplen-truncated by the capture.
#[must_use]
pub fn truncated_count(entries: &[TimelineEntry]) -> usize {
    entries
        .iter()
        .filter(|entry| entry.datagram.truncated)
        .count()
}

/// Looks up (and consumes) the viewer timestamp for a decoded datagram.
fn correlate(
    index: &mut HashMap<CorrelationKey, VecDeque<Option<OffsetDateTime>>>,
    direction: Direction,
    sim_host: SocketAddr,
    decoded: &Decoded,
) -> Option<OffsetDateTime> {
    let key = match decoded {
        Decoded::Message { framing, message } => (
            direction,
            sim_host,
            framing.sequence,
            message.name().to_owned(),
        ),
        Decoded::BodyError {
            framing,
            name: Some(name),
            ..
        } => (direction, sim_host, framing.sequence, (*name).to_owned()),
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

/// The heading tag for an entry that did not decode.
///
/// A snaplen-truncated datagram says so, because "the capture stopped short" and
/// "the two implementations disagree" are the two conclusions this tool exists
/// to tell apart, and an undifferentiated `<PARSE ERROR>` on a short capture
/// reads exactly like the divergence being hunted.
const fn error_tag(truncated: bool) -> &'static str {
    if truncated {
        "<TRUNCATED CAPTURE>"
    } else {
        "<PARSE ERROR>"
    }
}

/// The first line's message heading for an entry.
fn heading(entry: &TimelineEntry) -> String {
    let truncated = entry.datagram.truncated;
    match &entry.decoded {
        Decoded::Message { message, .. } if truncated => {
            format!("{} <TRUNCATED CAPTURE>", message.name())
        }
        Decoded::Message { message, .. } => message.name().to_owned(),
        Decoded::BodyError {
            name: Some(name), ..
        } => format!("{name} {}", error_tag(truncated)),
        Decoded::BodyError { name: None, .. } | Decoded::FrameError { .. } => {
            error_tag(truncated).to_owned()
        }
    }
}

/// Writes one text entry. Writing to a `String` cannot actually fail.
fn write_entry(out: &mut String, entry: &TimelineEntry, include_raw: bool) -> std::fmt::Result {
    let datagram = &entry.datagram;
    writeln!(
        out,
        "{}  {}  {}",
        format_time(datagram.timestamp),
        entry.direction.label(),
        heading(entry)
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
    let captured = if datagram.truncated {
        format!("  captured={}B", datagram.payload.len())
    } else {
        String::new()
    };
    writeln!(
        out,
        "    udp  {} -> {}  len={}{captured}",
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
    /// Whether the capture holds fewer payload bytes than `len` declares.
    truncated: bool,
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
            truncated: datagram.truncated,
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
    use std::net::{IpAddr, SocketAddr};

    use pretty_assertions::assert_eq;
    use time::{Duration, OffsetDateTime};

    use crate::trace::Direction;
    use crate::trace::logfile::{LogFile, LogMessage};
    use crate::trace::pcap::UdpDatagram;
    use crate::trace::timeline::{
        EndpointSpec, Endpoints, Labelling, build_timeline, render_jsonl, render_text,
    };

    /// A `CompletePingCheck` (High id 2, `ping_id = 7`, unreliable) at sequence
    /// `sequence`, wrapped as a UDP datagram from `source` to `destination`.
    fn ping_at(
        source: SocketAddr,
        destination: SocketAddr,
        sequence: u8,
        seconds: i64,
    ) -> UdpDatagram {
        UdpDatagram {
            timestamp: OffsetDateTime::UNIX_EPOCH.saturating_add(Duration::seconds(seconds)),
            source,
            destination,
            ip_version: 4,
            ip_hop_limit: 64,
            ip_total_len: 36,
            udp_length: 16,
            udp_checksum: 0,
            truncated: false,
            payload: vec![0x00, 0x00, 0x00, 0x00, sequence, 0x00, 0x02, 0x07],
        }
    }

    /// A `CompletePingCheck` datagram at sequence 42, at the epoch.
    fn ping_datagram(source: SocketAddr, destination: SocketAddr) -> UdpDatagram {
        ping_at(source, destination, 42, 0)
    }

    /// A log line for a `CompletePingCheck` on `host` at `sequence`, stamped
    /// `seconds` after the epoch.
    fn ping_log_line(
        direction: Direction,
        host: SocketAddr,
        sequence: u32,
        seconds: i64,
    ) -> LogMessage {
        LogMessage {
            timestamp: Some(OffsetDateTime::UNIX_EPOCH.saturating_add(Duration::seconds(seconds))),
            direction,
            host,
            size: 8,
            packet_id: sequence,
            name: "CompletePingCheck".to_owned(),
            reliable: false,
            resent: false,
            acks: false,
        }
    }

    /// Endpoints with one simulator address and nothing else.
    fn sim_only(spec: EndpointSpec) -> Endpoints {
        let mut endpoints = Endpoints::default();
        endpoints.sim.insert(spec);
        endpoints
    }

    #[test]
    fn decodes_and_labels_a_ping() -> Result<(), Box<dyn std::error::Error>> {
        let sim = SocketAddr::new(IpAddr::from([1, 2, 3, 4]), 13000);
        let viewer = SocketAddr::new(IpAddr::from([10, 0, 0, 1]), 52344);
        let endpoints = sim_only(EndpointSpec::Ip(sim.ip()));

        let timeline = build_timeline(
            vec![ping_datagram(viewer, sim)],
            &LogFile::default(),
            &endpoints,
        );
        assert_eq!(timeline.entries.len(), 1);

        let text = render_text(&timeline.entries, false);
        assert!(text.contains("V->S"));
        assert!(text.contains("CompletePingCheck"));

        let jsonl = render_jsonl(&timeline.entries)?;
        assert!(jsonl.contains("\"direction\":\"viewer_to_sim\""));
        assert!(jsonl.contains("CompletePingCheck"));
        assert!(jsonl.contains("\"ok\":true"));
        Ok(())
    }

    #[test]
    fn drops_non_circuit_datagrams() {
        let endpoints = sim_only(EndpointSpec::Ip(IpAddr::from([1, 2, 3, 4])));
        let timeline = build_timeline(
            vec![ping_datagram(
                SocketAddr::new(IpAddr::from([9, 9, 9, 9]), 53),
                SocketAddr::new(IpAddr::from([8, 8, 8, 8]), 53),
            )],
            &LogFile::default(),
            &endpoints,
        );
        assert!(timeline.entries.is_empty());
        assert_eq!(timeline.unlabelled, 1);
    }

    #[test]
    fn a_loopback_capture_is_labelled_by_port() {
        // The local OpenSim: viewer and simulator share 127.0.0.1, so only the
        // port tells the two directions apart.
        let sim = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 9000);
        let viewer = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 52344);
        let endpoints = sim_only(EndpointSpec::Socket(sim));

        let timeline = build_timeline(
            vec![ping_at(viewer, sim, 1, 0), ping_at(sim, viewer, 2, 1)],
            &LogFile::default(),
            &endpoints,
        );

        let directions: Vec<Direction> = timeline
            .entries
            .iter()
            .map(|entry| entry.direction)
            .collect();
        assert_eq!(
            directions,
            vec![Direction::ViewerToSim, Direction::SimToViewer]
        );
    }

    #[test]
    fn a_loopback_capture_known_only_by_ip_is_ambiguous_not_guessed() {
        let sim = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 9000);
        let viewer = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 52344);
        let endpoints = sim_only(EndpointSpec::Ip(sim.ip()));

        assert_eq!(
            endpoints.label(&ping_at(viewer, sim, 1, 0)),
            Labelling::Ambiguous
        );
        let timeline = build_timeline(
            vec![ping_at(viewer, sim, 1, 0), ping_at(sim, viewer, 2, 1)],
            &LogFile::default(),
            &endpoints,
        );
        assert!(timeline.entries.is_empty());
        assert_eq!(timeline.ambiguous, 2);
    }

    #[test]
    fn an_exact_endpoint_outranks_a_whole_ip_on_the_other_side() {
        let sim = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 9000);
        let viewer = SocketAddr::new(IpAddr::from([127, 0, 0, 1]), 52344);
        let mut endpoints = sim_only(EndpointSpec::Socket(sim));
        endpoints.viewer.insert(EndpointSpec::Ip(viewer.ip()));

        // The viewer IP matches both ends, so only the exact simulator socket
        // carries information — and it must win.
        assert_eq!(
            endpoints.label(&ping_at(sim, viewer, 1, 0)),
            Labelling::Known(Direction::SimToViewer)
        );
    }

    #[test]
    fn the_same_sequence_on_two_circuits_keeps_its_own_timestamp() {
        // A root circuit and a child circuit, each sent seq 5 — the classic
        // collision, since LLUDP sequence spaces are per circuit.
        let root = SocketAddr::new(IpAddr::from([1, 2, 3, 4]), 13000);
        let child = SocketAddr::new(IpAddr::from([1, 2, 3, 5]), 13000);
        let viewer = SocketAddr::new(IpAddr::from([10, 0, 0, 1]), 52344);
        let mut endpoints = Endpoints::default();
        endpoints.sim.insert(EndpointSpec::Socket(root));
        endpoints.sim.insert(EndpointSpec::Socket(child));

        let log = LogFile {
            messages: vec![
                ping_log_line(Direction::ViewerToSim, child, 5, 100),
                ping_log_line(Direction::ViewerToSim, root, 5, 200),
            ],
            ..LogFile::default()
        };

        let timeline = build_timeline(
            vec![ping_at(viewer, root, 5, 0), ping_at(viewer, child, 5, 1)],
            &log,
            &endpoints,
        );

        let stamps: Vec<Option<i64>> = timeline
            .entries
            .iter()
            .map(|entry| entry.viewer_timestamp.map(OffsetDateTime::unix_timestamp))
            .collect();
        // The root datagram gets the root line's 200 s, the child datagram the
        // child line's 100 s — not whichever line came first in the log.
        assert_eq!(stamps, vec![Some(200), Some(100)]);
    }

    #[test]
    fn correlation_follows_capture_time_not_file_order() {
        let sim = SocketAddr::new(IpAddr::from([1, 2, 3, 4]), 13000);
        let viewer = SocketAddr::new(IpAddr::from([10, 0, 0, 1]), 52344);
        let endpoints = sim_only(EndpointSpec::Socket(sim));

        let log = LogFile {
            messages: vec![
                ping_log_line(Direction::ViewerToSim, sim, 7, 100),
                ping_log_line(Direction::ViewerToSim, sim, 7, 200),
            ],
            ..LogFile::default()
        };

        // A merged capture hands the two occurrences over out of order.
        let timeline = build_timeline(
            vec![ping_at(viewer, sim, 7, 20), ping_at(viewer, sim, 7, 10)],
            &log,
            &endpoints,
        );

        let stamps: Vec<Option<i64>> = timeline
            .entries
            .iter()
            .map(|entry| entry.viewer_timestamp.map(OffsetDateTime::unix_timestamp))
            .collect();
        // Earliest datagram first, and it takes the earliest log line.
        assert_eq!(stamps, vec![Some(100), Some(200)]);
    }

    #[test]
    fn a_truncated_datagram_says_so_instead_of_reading_as_a_divergence() {
        let sim = SocketAddr::new(IpAddr::from([1, 2, 3, 4]), 13000);
        let viewer = SocketAddr::new(IpAddr::from([10, 0, 0, 1]), 52344);
        let endpoints = sim_only(EndpointSpec::Socket(sim));

        let mut datagram = ping_datagram(viewer, sim);
        datagram.udp_length = 400;
        datagram.truncated = true;
        datagram.payload.truncate(5);

        let timeline = build_timeline(vec![datagram], &LogFile::default(), &endpoints);
        let text = render_text(&timeline.entries, false);
        assert!(text.contains("<TRUNCATED CAPTURE>"));
        assert!(!text.contains("<PARSE ERROR>"));
        assert!(text.contains("captured=5B"));
    }
}
