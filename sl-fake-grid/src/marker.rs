//! Markers: the one message a fake grid sends purely so a test can **wait for
//! it**.
//!
//! A full-stack test drives the grid and then asks the viewer a question about
//! the picture. Between the two there is a whole pipeline — a UDP send, the
//! client's network thread, a Bevy frame, a mesh build — and the honest way to
//! know it has run is not to sleep for a plausible number of milliseconds but
//! to send something *after* the work and wait for the client to report it.
//! UDP delivery is ordered per circuit, so a marker sent after a `KillObject`
//! arrives after it.
//!
//! The envelope is a `GenericMessage`, which every client already parses and
//! surfaces (`Event::GenericMessage`) without understanding, so no viewer-side
//! feature is needed to receive one — a marker is inert everywhere except in
//! the harness watching for it.
//!
//! Sent with
//! [`SimSession::send_generic_message`](sl_proto::SimSession::send_generic_message)
//! and read back with [`marker_name`]. The scripted-timeline work
//! (`test-fake-grid-timeline`) emits these from a scenario step; a test that
//! drives the grid by hand sends its own.

use sl_proto::GenericMessage;

/// The `GenericMessage` method name every marker carries.
///
/// Namespaced with this crate's own name so it can never collide with a real
/// simulator feature's method: no grid but this one sends it, and a viewer that
/// does not know it ignores it.
pub const MARKER_METHOD: &str = "sl-fake-grid-marker";

/// The marker message called `name`.
///
/// The name travels as the single parameter blob, UTF-8 encoded — parameters
/// are opaque bytes on the wire, and every real `GenericMessage` feature puts
/// text in them the same way.
#[must_use]
pub fn marker(name: &str) -> GenericMessage {
    GenericMessage {
        method: MARKER_METHOD.to_owned(),
        invoice: sl_proto::InvoiceId::default(),
        params: vec![name.as_bytes().to_vec()],
    }
}

/// The name `generic` marks, or `None` if it is not a marker at all.
///
/// A marker whose name is not UTF-8 is not one this grid sent, so it reads as
/// `None` rather than as a lossily decoded name.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "re-exported at the crate root, where `sl_fake_grid::marker_name` reads clearly"
)]
pub fn marker_name(generic: &GenericMessage) -> Option<String> {
    if generic.method != MARKER_METHOD {
        return None;
    }
    let name = generic.params.first()?;
    String::from_utf8(name.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::{MARKER_METHOD, marker, marker_name};
    use pretty_assertions::assert_eq;

    /// A marker round-trips through its own reader.
    #[test]
    fn a_marker_reads_back_as_its_name() {
        assert_eq!(marker_name(&marker("killed")).as_deref(), Some("killed"));
    }

    /// Another feature's `GenericMessage` is not a marker, however it is shaped.
    #[test]
    fn another_generic_message_is_not_a_marker() {
        let other = sl_proto::GenericMessage {
            method: "autopilot".to_owned(),
            invoice: sl_proto::InvoiceId::default(),
            params: vec![b"killed".to_vec()],
        };
        assert_eq!(marker_name(&other), None);
    }

    /// A marker with no parameter names nothing, rather than naming the empty
    /// string — a test waiting for `""` would otherwise be woken by it.
    #[test]
    fn a_parameterless_marker_names_nothing() {
        let empty = sl_proto::GenericMessage {
            method: MARKER_METHOD.to_owned(),
            invoice: sl_proto::InvoiceId::default(),
            params: Vec::new(),
        };
        assert_eq!(marker_name(&empty), None);
    }
}
