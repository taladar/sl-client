//! Voice-version negotiation push (`RequiredVoiceVersion`).
//!
//! When the agent enters a region the simulator pushes a `RequiredVoiceVersion`
//! event over the CAPS event queue, naming the voice protocol the region
//! expects. The reference viewer compares it against its own voice module's
//! version and warns the user on a mismatch (Firestorm
//! `indra/newview/llvoiceclient.cpp`, `LLViewerRequiredVoiceVersion`). It
//! surfaces here as a typed [`Event`](super::Event) instead of being dropped to
//! a `Diagnostic::UnknownCapsEvent`; whether to act on a version mismatch is
//! left to the consumer.

/// The voice protocol version a region requires (`RequiredVoiceVersion`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequiredVoiceVersion {
    /// The major version of the voice protocol the region expects
    /// (`major_version`). A viewer warns when this exceeds the version its
    /// voice module implements.
    pub major_version: i32,
    /// The name of the region the requirement applies to (`region_name`); empty
    /// when the sim omits it.
    pub region_name: String,
    /// The voice backend the region uses (`voice_server_type`, e.g. `"vivox"`
    /// or `"webrtc"`); `None` when absent, which the reference viewer treats as
    /// the default (`"vivox"`).
    pub voice_server_type: Option<String>,
}
