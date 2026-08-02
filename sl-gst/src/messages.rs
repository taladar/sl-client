//! Shared GStreamer bus-message helpers for the video surface and the audio
//! stream player: `missing-plugin` collection, friendly error text, and tag
//! ("now playing") extraction.

/// The human-readable description out of a `missing-plugin` element message's
/// structure (`gst_pbutils` posts these when decodebin finds no decoder /
/// demuxer / URI source), or [`None`] for any other element message.
pub(crate) fn missing_plugin_description(message: &gstreamer::Message) -> Option<String> {
    let gstreamer::MessageView::Element(element) = message.view() else {
        return None;
    };
    let structure = element.structure()?;
    if structure.name() != "missing-plugin" {
        return None;
    }
    // The structure carries a localised human-readable "name" (e.g.
    // "H.264 (High Profile) decoder"); fall back to the technical "detail".
    structure
        .get::<String>("name")
        .ok()
        .or_else(|| structure.get::<String>("detail").ok())
}

/// Compose the user-facing error text for a bus error, folding in any
/// `missing-plugin` descriptions collected earlier — so a codec gap reads
/// *"needs an H.264 decoder — install the matching GStreamer plugin"*, not a
/// bare internal stream error.
///
/// When no plugin is missing, the bare [`glib::Error`] message is often
/// uselessly generic — a demuxer that cannot read the stream and a decoder
/// that chokes on the bitstream both surface as *"Internal data stream
/// error."*. The actual reason lives in the message's **debug string** (e.g.
/// *"Could not update any variant playlist"*), so it is folded in rather than
/// discarded.
pub(crate) fn friendly_error(
    error: &gstreamer::message::Error,
    missing_plugins: &[String],
) -> String {
    let base = error.error().to_string();
    if !missing_plugins.is_empty() {
        return format!(
            "needs {} — install the matching GStreamer plugin(s)",
            missing_plugins.join(", ")
        );
    }
    // The commonest bare failure worth translating: no HTTP source element
    // installed at all.
    if base.contains("No URI handler") {
        return format!(
            "{base} — GStreamer cannot fetch this URL scheme; install the GStreamer HTTP \
             plugin (soup)"
        );
    }
    let debug = error.debug();
    let detail = debug_detail(debug.as_deref());
    // `Internal data stream error.` is GStreamer's generic "the streaming
    // thread stopped" — meaningless alone. The debug string's tail is either a
    // human reason worth keeping (e.g. a demuxer's *"Could not update any
    // variant playlist"*) or GStreamer's own *"streaming stopped, reason error
    // (-5)"* flow-return jargon, which is translated into plain language using
    // the failing element the rest of the debug string names.
    if base.contains("Internal data stream error") {
        return match detail {
            Some(detail) if !detail.starts_with("streaming stopped") => {
                format!("stream error — {detail}")
            }
            _jargon_or_none => stream_stopped_message(debug.as_deref()),
        };
    }
    // Otherwise fold the debug reason into the base message, unless the base
    // already says it.
    match detail {
        Some(detail) if !base.contains(detail.as_str()) => format!("{base} — {detail}"),
        _other => base,
    }
}

/// Plain-language text for a generic *"Internal data stream error."* whose
/// only detail is GStreamer's flow-return jargon — distinguishing a source
/// (HTTP server) failure from a downstream decode failure by the element the
/// debug string names, so a user reads *why nothing plays* rather than
/// *"reason error (-5)"*.
fn stream_stopped_message(debug: Option<&str>) -> String {
    if from_http_source(debug) {
        String::from(
            "stream unavailable — the media server sent no playable data (it may be offline, \
             blocked, or need a login)",
        )
    } else {
        String::from("stream stopped — its media could not be read or decoded")
    }
}

/// Whether a bus error is the generic *"Internal data stream error."* raised by
/// an HTTP **source** element — the case where `souphttpsrc` swallows the real
/// DNS / TCP / TLS / HTTP-status reason (logging it only at its own debug
/// level) and returns a bare flow error, so the true cause never reaches the
/// bus. The owner can react by probing the stream URL itself to recover a
/// precise reason.
pub(crate) fn is_http_source_failure(error: &gstreamer::message::Error) -> bool {
    error
        .error()
        .to_string()
        .contains("Internal data stream error")
        && from_http_source(error.debug().as_deref())
}

/// Whether a GStreamer debug string names an HTTP source element (`souphttpsrc`
/// / `GstSoupHTTPSrc`) as the failing element.
fn from_http_source(debug: Option<&str>) -> bool {
    debug.is_some_and(|debug| debug.contains("HTTPSrc") || debug.contains("httpsrc"))
}

/// The human-readable tail of a GStreamer debug string — the part after the
/// `file(line): function (): element:` location prefix, which carries the
/// actual reason (e.g. *"Could not update any variant playlist"* behind a
/// generic *"Internal data stream error."*).
///
/// GStreamer separates the location prefix from the message with a newline;
/// returns [`None`] when the debug string is absent, is only the prefix (no
/// trailing message), or has an empty tail.
fn debug_detail(debug: Option<&str>) -> Option<String> {
    let (_prefix, message) = debug?.split_once('\n')?;
    let message = message.trim();
    (!message.is_empty()).then(|| String::from(message))
}

/// The stream / track title out of a tag-list message, if it carries one —
/// for radio streams this is the ICY "now playing" metadata `icydemux`
/// re-emits as a title tag.
pub(crate) fn title_from_tags(tags: &gstreamer::TagList) -> Option<String> {
    tags.get::<gstreamer::tags::Title>()
        .map(|title| String::from(title.get()))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{debug_detail, friendly_error};

    /// A GStreamer debug string (`prefix:\nmessage`) yields its trailing
    /// human-readable message, whitespace-trimmed.
    #[test]
    fn debug_detail_takes_the_message_after_the_prefix() {
        let debug = "gsthlsdemux.c(1203): gst_hls_demux_… (): /playbin3/hlsdemux2-0:\nCould not \
                     update any variant playlist\n";
        assert_eq!(
            debug_detail(Some(debug)),
            Some(String::from("Could not update any variant playlist"))
        );
    }

    /// A location-only debug string (no message line) and an absent one both
    /// add nothing.
    #[test]
    fn debug_detail_ignores_prefix_only_and_absent_debug() {
        assert_eq!(
            debug_detail(Some("basesrc.c(3127): gst_base_src_loop ():")),
            None
        );
        assert_eq!(debug_detail(Some("prefix:\n   ")), None);
        assert_eq!(debug_detail(None), None);
    }

    /// End to end: a bare *"Internal data stream error."* whose debug tail is a
    /// human reason keeps that reason; with a missing plugin recorded the
    /// plugin guidance wins instead.
    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a visible skip notice when GStreamer is absent on the host"
    )]
    fn friendly_error_keeps_a_human_debug_reason() -> Result<(), String> {
        if crate::ensure_initialized().is_err() {
            eprintln!("skipping: no usable GStreamer");
            return Ok(());
        }
        let message = gstreamer::message::Error::builder(
            gstreamer::StreamError::Failed,
            "Internal data stream error.",
        )
        .debug(
            "hlsdemux.c(1203): fn (): /playbin3/hlsdemux2-0:\nCould not update any variant \
                playlist",
        )
        .build();
        let gstreamer::MessageView::Error(error) = message.view() else {
            return Err(String::from("built message is not an error"));
        };
        assert_eq!(
            friendly_error(error, &[]),
            "stream error — Could not update any variant playlist"
        );
        assert_eq!(
            friendly_error(error, &[String::from("MP3 decoder")]),
            "needs MP3 decoder — install the matching GStreamer plugin(s)"
        );
        Ok(())
    }

    /// End to end: the exact live failure — a generic internal error whose only
    /// debug detail is GStreamer's *"streaming stopped, reason error (-5)"*
    /// flow-return jargon from an HTTP source — resolves to a readable, plain
    /// sentence, never the `(-5)` code.
    #[test]
    #[expect(
        clippy::print_stderr,
        reason = "a visible skip notice when GStreamer is absent on the host"
    )]
    fn friendly_error_translates_http_source_flow_jargon() -> Result<(), String> {
        if crate::ensure_initialized().is_err() {
            eprintln!("skipping: no usable GStreamer");
            return Ok(());
        }
        let message = gstreamer::message::Error::builder(
            gstreamer::StreamError::Failed,
            "Internal data stream error.",
        )
        .debug(
            "gstbasesrc.c(3187): gst_base_src_loop (): \
             /GstPlayBin3:playbin3-0/GstURIDecodeBin3:uridecodebin3/GstURISourceBin:urisourcebin0/\
             GstSoupHTTPSrc:souphttpsrc1:\nstreaming stopped, reason error (-5)",
        )
        .build();
        let gstreamer::MessageView::Error(error) = message.view() else {
            return Err(String::from("built message is not an error"));
        };
        let text = friendly_error(error, &[]);
        assert_eq!(
            text,
            "stream unavailable — the media server sent no playable data (it may be offline, \
             blocked, or need a login)"
        );
        assert!(!text.contains("-5"), "flow-return jargon leaked: {text}");
        Ok(())
    }
}
