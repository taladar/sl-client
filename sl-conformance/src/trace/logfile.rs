//! Parsing the `#Messaging#` lines of a Firestorm `SecondLife.log`.
//!
//! With `LogMessages = 1` the viewer logs one line per LLUDP message with its
//! direction, the simulator `host:port`, sizes, packet id, message name and
//! flags. The tool uses these to identify the simulator endpoint(s), to label
//! each captured datagram's direction, and to annotate it with the viewer's own
//! (coarse, one-second) timestamp. It does **not** carry the message body — the
//! full body comes from the pcap.

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
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

/// The parsed `#Messaging#` lines plus the simulator IPs they mention.
#[derive(Debug, Clone, Default)]
pub struct LogFile {
    /// Every parsed message line, in file (chronological) order.
    pub messages: Vec<LogMessage>,
    /// The distinct simulator IP addresses seen, used to label pcap direction.
    pub sim_hosts: HashSet<IpAddr>,
}

/// Reads and parses the `#Messaging#` lines of the log at `path`.
///
/// # Errors
///
/// Returns [`TraceError`] if the file cannot be read.
pub fn read_log(path: &Path) -> Result<LogFile, TraceError> {
    let text = fs_err::read_to_string(path)?;
    let mut messages = Vec::new();
    let mut sim_hosts = HashSet::new();
    for line in text.lines() {
        if let Some(message) = parse_message_line(line) {
            sim_hosts.insert(message.host.ip());
            messages.push(message);
        }
    }
    Ok(LogFile {
        messages,
        sim_hosts,
    })
}

/// Parses a single line, returning the message if it is a well-formed
/// `MSG: -> / <-` line and `None` otherwise.
fn parse_message_line(line: &str) -> Option<LogMessage> {
    let marker = line.find("MSG:")?;
    let tokens: Vec<&str> = line.get(marker..)?.split_whitespace().collect();
    // tokens: 0=MSG:, 1=arrow, 2=host, 3=size, 4=compressed, 5=packet_id,
    //         6=name, 7..=flags
    let direction = match *tokens.get(1)? {
        "->" => Direction::ViewerToSim,
        "<-" => Direction::SimToViewer,
        _ => return None,
    };
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

    #[test]
    fn parses_an_outgoing_line() {
        let line = "2024-01-15T10:30:45Z INFO #Messaging# message.cpp(1319) \
                    LLMessageSystem::sendMessage : MSG: -> 192.168.1.100:13000\t1024\t\
                    1024\t12345 StartAvatarMovement reliable";
        let parsed = super::parse_message_line(line);
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
        let parsed = super::parse_message_line(line);
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
        assert!(
            super::parse_message_line("2024-01-15T10:30:45Z INFO #Foo# a.cpp(1) f : hi").is_none()
        );
        assert!(super::parse_message_line("").is_none());
    }
}
