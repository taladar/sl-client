//! Offline protocol-trace tooling for the `sl-conformance-trace` binary.
//!
//! Turns a captured `.pcap` (full LLUDP UDP datagrams) plus, optionally, a
//! Firestorm `SecondLife.log` (with `LogMessages = 1`) into a single
//! chronological, human-readable timeline of every UDP message exchanged
//! between the viewer and the simulator, parsed with the workspace's own
//! `sl-wire` decoders. It exists so a divergence between `sl-client` and a real
//! viewer can be compared side by side.
//!
//! - [`pcap`] reads the capture and peels link/IP/UDP off each frame.
//! - [`logfile`] parses the `#Messaging#` lines of `SecondLife.log`.
//! - [`timeline`] correlates the two, decodes the LLUDP bodies, and renders the
//!   text and JSON-Lines output.
//!
//! This iteration covers **UDP only**; the truncated `QAModeHttpTrace` CAPS
//! bodies are intentionally out of scope (see the crate's trace design notes).

pub mod logfile;
pub mod pcap;
pub mod timeline;

/// Errors that can occur while building a trace timeline.
#[expect(
    clippy::module_name_repetitions,
    reason = "`TraceError` reads best as this module's public error name"
)]
#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    /// An I/O error reading one of the input files.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// The `.pcap` container could not be parsed.
    #[error("pcap error: {0}")]
    Pcap(String),
    /// A link-layer type the tool does not know how to peel.
    #[error("unsupported link-layer type: {0:?}")]
    UnsupportedLinkType(pcap_file::DataLink),
    /// No direction could be established because neither a `--log` nor an
    /// explicit `--sim-addr`/`--viewer-addr` identified the simulator.
    #[error(
        "cannot determine packet direction: pass --log with a Firestorm \
         SecondLife.log, or --sim-addr / --viewer-addr"
    )]
    NoEndpoints,
    /// A timestamp could not be represented.
    #[error("timestamp error: {0}")]
    Time(String),
    /// Serializing a JSON-Lines record failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// The direction of a datagram relative to the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Sent by the viewer to the simulator.
    ViewerToSim,
    /// Sent by the simulator to the viewer.
    SimToViewer,
}

impl Direction {
    /// The compact label used in the human-readable timeline (`V->S` / `S->V`).
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ViewerToSim => "V->S",
            Self::SimToViewer => "S->V",
        }
    }

    /// The stable machine-readable label used in the JSON-Lines output.
    #[must_use]
    pub const fn json(self) -> &'static str {
        match self {
            Self::ViewerToSim => "viewer_to_sim",
            Self::SimToViewer => "sim_to_viewer",
        }
    }
}

/// The transport a timeline entry came from.
///
/// Only [`Transport::Udp`] is produced in this iteration, but the field is a
/// discriminator on every record so a later CAPS/HTTP phase can add entries to
/// the same stream without a schema change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// An LLUDP datagram carried over UDP.
    Udp,
}
