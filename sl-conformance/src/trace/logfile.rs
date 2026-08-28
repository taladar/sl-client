//! Parsing the `#Messaging#` lines of a Firestorm `SecondLife.log`.
//!
//! With `LogMessages = 1` the viewer logs one line per LLUDP message with its
//! direction, the simulator `host:port`, sizes, packet id, message name and
//! flags. The tool uses these to identify the simulator endpoint(s), to label
//! each captured datagram's direction, and to annotate it with the viewer's own
//! (coarse, one-second) timestamp. It does **not** carry the message body — the
//! full body comes from the pcap.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::Path;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::trace::{Direction, TraceError};

/// One `#Messaging#` log line.
#[derive(Debug, Clone)]
pub struct LogMessage {
    /// The viewer's timestamp for the line (one-second resolution), if it
    /// parsed.
    pub timestamp: Option<OffsetDateTime>,
    /// The direction relative to the viewer.
    pub direction: Direction,
    /// The simulator `ip:port`.
    pub host: SocketAddr,
    /// The (uncompressed) message size the viewer logged.
    pub size: u32,
    /// The reliable-packet id (the datagram sequence number).
    pub packet_id: u32,
    /// The LLUDP message template name.
    pub name: String,
    /// Whether the `reliable` flag was logged.
    pub reliable: bool,
    /// Whether the `resent` flag was logged.
    pub resent: bool,
    /// Whether appended acks were logged.
    pub acks: bool,
}

/// The parsed `#Messaging#` lines plus the simulator endpoints they mention.
#[derive(Debug, Clone, Default)]
pub struct LogFile {
    /// Every parsed message line, in file (chronological) order.
    pub messages: Vec<LogMessage>,
    /// The distinct simulator `ip:port` endpoints seen, used to label pcap
    /// direction. The **port** matters: on a loopback grid the simulator and
    /// the viewer share an IP, and only the port tells the two sides apart.
    pub sim_hosts: HashSet<SocketAddr>,
    /// How many lines looked like `#Messaging#` lines but did not parse — a
    /// silent zero here is what tells the caller the log format has not
    /// drifted out from under the tool.
    pub skipped_lines: usize,
}

/// What one log line turned out to be.
enum LineKind {
    /// A well-formed `#Messaging#` line.
    Message(Box<LogMessage>),
    /// A `MSG: ->` / `MSG: <-` line whose fields did not parse.
    Malformed,
    /// Some other log line, of no interest here.
    Other,
}

/// Reads and parses the `#Messaging#` lines of the log at `path`.
///
/// # Errors
///
/// Returns [`TraceError`] if the file cannot be read.
pub fn read_log(path: &Path) -> Result<LogFile, TraceError> {
    let text = fs_err::read_to_string(path)?;
    let mut log = LogFile::default();
    for line in text.lines() {
        match classify_line(line) {
            LineKind::Message(message) => {
                log.sim_hosts.insert(message.host);
                log.messages.push(*message);
            }
            LineKind::Malformed => {
                log.skipped_lines = log.skipped_lines.saturating_add(1);
            }
            LineKind::Other => {}
        }
    }
    Ok(log)
}

/// Classifies a single line as a parsed message, a malformed message line, or
/// an unrelated log line.
///
/// A line counts as a message line — and so as *malformed* when its fields do
/// not parse — as soon as its `MSG:` marker is followed by a direction arrow.
/// Anything else is simply another log line.
fn classify_line(line: &str) -> LineKind {
    let Some(marker) = line.find("MSG:") else {
        return LineKind::Other;
    };
    let Some(rest) = line.get(marker..) else {
        return LineKind::Other;
    };
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    // tokens: 0=MSG:, 1=arrow, 2=host, 3=size, 4=compressed, 5=packet_id,
    //         6=name, 7..=flags
    let direction = match tokens.get(1) {
        Some(&"->") => Direction::ViewerToSim,
        Some(&"<-") => Direction::SimToViewer,
        _ => return LineKind::Other,
    };
    match parse_fields(line, &tokens, direction) {
        Some(message) => LineKind::Message(Box::new(message)),
        None => LineKind::Malformed,
    }
}

/// Parses the fields after the direction arrow of a `#Messaging#` line.
fn parse_fields(line: &str, tokens: &[&str], direction: Direction) -> Option<LogMessage> {
    let host: SocketAddr = tokens.get(2)?.parse().ok()?;
    let size: u32 = tokens.get(3)?.parse().ok()?;
    let packet_id: u32 = tokens.get(5)?.parse().ok()?;
    let name = (*tokens.get(6)?).to_owned();
    let flags = tokens.get(7..).unwrap_or(&[]);
    let has = |flag: &str| flags.contains(&flag);

    let timestamp = line
        .split_whitespace()
        .next()
        .and_then(|first| OffsetDateTime::parse(first, &Rfc3339).ok());

    Some(LogMessage {
        timestamp,
        direction,
        host,
        size,
        packet_id,
        name,
        reliable: has("reliable"),
        resent: has("resent"),
        acks: has("acks"),
    })
}

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;

    use crate::trace::Direction;
    use crate::trace::logfile::{LineKind, LogMessage, classify_line};

    /// The parsed message of a line, or `None` if it was not one.
    fn parse(line: &str) -> Option<LogMessage> {
        match classify_line(line) {
            LineKind::Message(message) => Some(*message),
            LineKind::Malformed | LineKind::Other => None,
        }
    }

    #[test]
    fn parses_an_outgoing_line() {
        let line = "2024-01-15T10:30:45Z INFO #Messaging# message.cpp(1319) \
                    LLMessageSystem::sendMessage : MSG: -> 192.168.1.100:13000\t1024\t\
                    1024\t12345 StartAvatarMovement reliable";
        let parsed = parse(line);
        assert!(parsed.is_some());
        if let Some(message) = parsed {
            assert_eq!(message.direction, Direction::ViewerToSim);
            assert_eq!(message.host.port(), 13000);
            assert_eq!(message.packet_id, 12345);
            assert_eq!(message.name, "StartAvatarMovement");
            assert!(message.reliable);
            assert!(!message.resent);
            assert!(message.timestamp.is_some());
        }
    }

    #[test]
    fn parses_an_incoming_line_with_flags() {
        let line = "2024-01-15T10:30:45Z INFO #Messaging# message.cpp(1443) \
                    LLMessageSystem::logValidMsg : MSG: <- 192.168.1.100:13000\t512\t512\t\
                    54321 AvatarAnimation reliable resent acks";
        let parsed = parse(line);
        assert!(parsed.is_some());
        if let Some(message) = parsed {
            assert_eq!(message.direction, Direction::SimToViewer);
            assert_eq!(message.name, "AvatarAnimation");
            assert!(message.reliable);
            assert!(message.resent);
            assert!(message.acks);
        }
    }

    #[test]
    fn ignores_non_message_lines() {
        assert!(matches!(
            classify_line("2024-01-15T10:30:45Z INFO #Foo# a.cpp(1) f : hi"),
            LineKind::Other
        ));
        assert!(matches!(classify_line(""), LineKind::Other));
    }

    #[test]
    fn a_message_line_the_tool_cannot_parse_is_counted_not_ignored() {
        // The arrow says this is a message line, so a host field the tool does
        // not understand is format drift worth reporting — not a line to skip
        // in silence.
        assert!(matches!(
            classify_line(
                "2024-01-15T10:30:45Z INFO #Messaging# message.cpp(1319) f : \
                 MSG: -> sim.example.com:13000\t1024\t1024\t12345 AgentUpdate"
            ),
            LineKind::Malformed
        ));
    }
}
