//! Protocol-level diagnostics: anomalies the session noticed in inbound data.

use sl_wire::{MessageId, SequenceNumber, WireError};

/// A protocol-level anomaly the session noticed while processing inbound data.
///
/// Diagnostics are kept strictly **separate** from [`Event`](crate::Event): a
/// match on `Event` never sees a diagnostic, and vice versa. Where an `Event`
/// is a successfully understood happening a client acts on, a `Diagnostic`
/// surfaces something the session would otherwise *silently drop* — a datagram
/// whose body failed to decode, a decoded message with no handler, an unknown
/// or malformed CAPS event-queue payload, or a reliable request whose expected
/// reply never arrived. They exist so a test client (or a developer chasing a
/// protocol gap) can see exactly what the session is ignoring.
///
/// Collection is **off by default** — diagnostics are produced only after
/// [`Session::set_diagnostics(true)`](crate::Session::set_diagnostics), so the
/// raw-byte capture and bookkeeping cost nothing on the normal path. Drain them
/// with [`Session::poll_diagnostic`](crate::Session::poll_diagnostic).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Diagnostic {
    /// An inbound datagram carried a message whose id was recognised (or at
    /// least frequency-decodable) but whose body failed to decode. The session
    /// drops such datagrams; this captures what was lost.
    DecodeFailed {
        /// The frequency-coded id read from the datagram.
        id: MessageId,
        /// The message name, when `id` maps to a known template message
        /// (`None` for an unrecognised id).
        name: Option<&'static str>,
        /// The wire error that decoding produced.
        error: WireError,
        /// The decoded message body (post zero-decode), captured for a hexdump.
        /// Only populated while diagnostics are enabled.
        raw: Vec<u8>,
        /// The reader offset into `raw` at which decoding stopped — the byte to
        /// mark in a hexdump.
        failed_offset: usize,
    },
    /// A message decoded successfully but reached the dispatch table's
    /// catch-all arm: nothing in the session acts on it. (Expected for traffic
    /// the client does not model; useful to know which messages those are.)
    UnhandledMessage {
        /// The message's frequency-coded id.
        id: MessageId,
        /// The message name.
        name: &'static str,
        /// Whether it arrived on a child-agent circuit (a neighbouring region)
        /// rather than the root circuit.
        child: bool,
    },
    /// A CAPS event-queue event (or capability reply) arrived under a name the
    /// session does not handle.
    UnknownCapsEvent {
        /// The event / capability name as delivered.
        message: String,
    },
    /// A CAPS event the session *does* handle arrived, but its LLSD body failed
    /// to parse into the expected shape (a required field was absent, a field
    /// held the wrong LLSD kind, or a legacy `from_llsd` returned `None`).
    CapsDecodeFailed {
        /// The event / capability name whose body could not be parsed.
        message: String,
        /// The decode error that caused the drop, rendered for debugging (which
        /// field was missing or malformed). [`None`] for the legacy
        /// `Option`-returning decoders that do not report a specific cause.
        reason: Option<String>,
    },
    /// A reliable request never received its expected reply: either a reliable
    /// packet exhausted its retransmission budget, or an operation awaiting a
    /// reply (logout, sit) timed out. (Teleport timeouts stay
    /// [`Event::TeleportFailed`](crate::Event::TeleportFailed) instead.)
    ExpectedReplyMissing {
        /// A short label for the request whose reply is missing (e.g. the
        /// reliable message name, or `"Logout"` / `"Sit"`).
        request: String,
        /// The sequence number of the unacked reliable packet, when one is
        /// known (`None` for operation-level timeouts).
        sequence: Option<SequenceNumber>,
    },
}

impl std::fmt::Display for Diagnostic {
    /// The one-line summary a log line or a transcript row carries — literal,
    /// with no symbolization, and **without** the captured bytes (those are
    /// [`Diagnostic::hexdump`], which is many lines and belongs behind a
    /// verbose log level).
    ///
    /// The match is exhaustive from inside the crate even though the type is
    /// `#[non_exhaustive]`, so a new variant fails to compile here until it is
    /// given a rendering rather than silently falling into a `Debug` catch-all.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DecodeFailed {
                id,
                name,
                error,
                failed_offset,
                raw: _captured,
            } => {
                let displayed_name = name.unwrap_or("?");
                write!(
                    f,
                    "DecodeFailed id={id:?} name={displayed_name} error={error} \
                     failed_offset={failed_offset}"
                )
            }
            Self::UnhandledMessage { id, name, child } => {
                write!(f, "UnhandledMessage id={id:?} name={name} child={child}")
            }
            Self::UnknownCapsEvent { message } => {
                write!(f, "UnknownCapsEvent message={message}")
            }
            Self::CapsDecodeFailed { message, reason } => {
                write!(f, "CapsDecodeFailed message={message}")?;
                match reason {
                    Some(reason) => write!(f, " reason={reason}"),
                    None => Ok(()),
                }
            }
            Self::ExpectedReplyMissing { request, sequence } => match sequence {
                Some(sequence) => {
                    write!(
                        f,
                        "ExpectedReplyMissing request={request} sequence={sequence}"
                    )
                }
                None => write!(f, "ExpectedReplyMissing request={request} sequence=-"),
            },
        }
    }
}

impl Diagnostic {
    /// The [`Diagnostic::ExpectedReplyMissing`] `request` label for a logout
    /// whose `LogoutReply` never arrived.
    ///
    /// One of the two **operation** labels: `request` is otherwise an open
    /// vocabulary — a wire message name, or (for a driver that reports a failed
    /// capability request this way) a capability name — so a consumer that
    /// treats operations differently from background traffic recognises them by
    /// these names. A missing logout reply is already surfaced as the logout
    /// itself ([`Event::LoggedOut`](crate::Event::LoggedOut) follows
    /// immediately).
    pub const LOGOUT_REQUEST: &'static str = "Logout";

    /// The [`Diagnostic::ExpectedReplyMissing`] `request` label for a sit whose
    /// `AvatarSitResponse` never arrived.
    ///
    /// The other operation label (see [`Diagnostic::LOGOUT_REQUEST`]), and the
    /// one an agent can *feel*: the session keeps running and simply never sits,
    /// with nothing else surfaced, so a client that tells the user when an
    /// action did nothing reports this one.
    pub const SIT_REQUEST: &'static str = "Sit";

    /// The marked [`hexdump`] of the bytes this diagnostic captured, or [`None`]
    /// for a variant that captures none.
    ///
    /// Only [`Diagnostic::DecodeFailed`] carries bytes, and only while
    /// diagnostics are enabled — so an empty capture still renders, saying so.
    /// Kept apart from [`Display`](std::fmt::Display) because it is many lines:
    /// a caller logs the summary at `warn` and this at `debug`.
    #[must_use]
    pub fn hexdump(&self) -> Option<String> {
        match self {
            Self::DecodeFailed {
                raw, failed_offset, ..
            } => Some(hexdump(raw, Some(*failed_offset))),
            _no_bytes => None,
        }
    }
}

/// Render `bytes` as a classic offset / hex / ASCII dump, 16 bytes per row.
///
/// When `mark` is `Some(offset)` the byte at that offset is wrapped in square
/// brackets (`[ab]` rather than ` ab `, keeping every cell four columns wide so
/// the rows stay aligned). A `mark` at or past the end of `bytes` — the reader
/// position a decode stopped at — is noted on a trailing line instead.
#[must_use]
pub fn hexdump(bytes: &[u8], mark: Option<usize>) -> String {
    let mut out = String::new();
    let _rendered = write_hexdump(&mut out, bytes, mark);
    out
}

/// Write a marked offset / hex / ASCII dump of `bytes` into `out`.
fn write_hexdump(out: &mut String, bytes: &[u8], mark: Option<usize>) -> std::fmt::Result {
    use std::fmt::Write as _;

    if bytes.is_empty() {
        out.push_str("(no bytes)");
        if let Some(at) = mark {
            write!(out, " — mark at offset {at}")?;
        }
        return Ok(());
    }
    for (row, chunk) in bytes.chunks(16).enumerate() {
        let base = row.saturating_mul(16);
        write!(out, "{base:08x} ")?;
        for (col, byte) in chunk.iter().enumerate() {
            let offset = base.saturating_add(col);
            if Some(offset) == mark {
                write!(out, "[{byte:02x}]")?;
            } else {
                write!(out, " {byte:02x} ")?;
            }
        }
        out.push_str(" |");
        for byte in chunk {
            out.push(printable(*byte));
        }
        out.push('|');
        out.push('\n');
    }
    if let Some(at) = mark
        && at >= bytes.len()
    {
        write!(out, "(mark at offset {at} = end of {} bytes)", bytes.len())?;
    }
    Ok(())
}

/// The printable ASCII glyph for `byte`, or `.` for a non-printable byte.
fn printable(byte: u8) -> char {
    if (0x20..=0x7e).contains(&byte) {
        char::from(byte)
    } else {
        '.'
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sl_wire::{MessageId, WireError};

    use super::Diagnostic;

    /// The summary is one line and never carries the captured bytes: a consumer
    /// logs it at `warn`, where a hexdump would be unreadable.
    #[test]
    fn display_is_one_line_without_the_bytes() {
        let diagnostic = Diagnostic::DecodeFailed {
            id: MessageId::High(1),
            name: Some("ObjectUpdate"),
            error: WireError::UnexpectedEof {
                needed: 4,
                available: 1,
            },
            raw: vec![0xde, 0xad, 0xbe, 0xef],
            failed_offset: 2,
        };
        let summary = diagnostic.to_string();
        assert!(
            !summary.contains('\n'),
            "the summary is a single line: {summary}"
        );
        assert!(
            !summary.contains("dead"),
            "the captured bytes stay out of the summary: {summary}"
        );
        assert!(summary.starts_with("DecodeFailed "), "{summary}");
        assert!(summary.contains("name=ObjectUpdate"), "{summary}");
        assert!(summary.contains("failed_offset=2"), "{summary}");
    }

    /// The bytes come back separately, with the failing offset marked — and
    /// only for the one variant that captures any.
    #[test]
    fn only_a_failed_decode_carries_a_hexdump() {
        let dump = Diagnostic::DecodeFailed {
            id: MessageId::High(1),
            name: None,
            error: WireError::UnexpectedEof {
                needed: 4,
                available: 1,
            },
            raw: vec![0xde, 0xad, 0xbe, 0xef],
            failed_offset: 2,
        }
        .hexdump();
        let dump = dump.unwrap_or_default();
        assert!(
            dump.contains("[be]"),
            "the byte at the failing offset is bracketed: {dump}"
        );
        assert_eq!(
            Diagnostic::ExpectedReplyMissing {
                request: "Sit".to_owned(),
                sequence: None,
            }
            .hexdump(),
            None,
            "a variant with no captured bytes has no dump"
        );
    }

    /// A capture that was never taken (diagnostics were off) still renders,
    /// saying so, rather than producing an empty string a caller might log.
    #[test]
    fn an_empty_capture_says_so() {
        let dump = Diagnostic::DecodeFailed {
            id: MessageId::High(1),
            name: None,
            error: WireError::UnexpectedEof {
                needed: 4,
                available: 0,
            },
            raw: Vec::new(),
            failed_offset: 0,
        }
        .hexdump();
        assert_eq!(dump.as_deref(), Some("(no bytes) — mark at offset 0"));
    }
}
